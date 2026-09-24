// Copyright 2017-2022 F4PGA Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! The byte layout of a cache file (design document §8.8):
//!
//! ```text
//! prefix (88 bytes)
//!   magic          [u8; 8]   b"FASMXDB1"
//!   format_version u32
//!   header_len     u32       length of the header block
//!   payload_len    u64       length of the payload
//!   header_hash    [u8; 32]  BLAKE3 of the header block
//!   payload_hash   [u8; 32]  BLAKE3 of the payload
//! header block     (CacheHeader: provenance and source fingerprints)
//! payload          (the Database tables, in independent sections)
//! ```
//!
//! The payload is a section table (count, then kind and length of each)
//! followed by the sections: `TYPE_GROUPS` groups of consecutive tile
//! types, the grid and the part data. Each section starts with its own
//! string tables and refers to strings by index, so the sections are
//! decoded, and their names interned, on separate threads.
//!
//! Everything is little endian; strings are a `u32` byte length and UTF-8
//! bytes (the canonical database root: raw OS bytes on Unix). Every byte
//! of the file is covered by the magic, the version, the lengths or one of
//! the two hashes, so any truncation or flipped byte is detected before
//! the payload is decoded; the decoder still bounds checks every index so
//! that a crafted file with valid hashes can only produce an error.

use std::path::{Path, PathBuf};

use fasm::idstring::IdString;
use foldhash::HashMap;

use crate::arch::{Architecture, BlockType};
use crate::db::{Database, Layout, PartInfo, TileType, TileTypeFiles};
use crate::part::{BanksTilesRegistry, ConfigBus, ConfigRow, PackagePin, Part};
use crate::segbits::{PpipType, SegBit, SegbitsEntry, TileSegbits};
use crate::tilegrid::{BitAlias, BitsBlock, ClockRegion, Grid, Span, Tile};

use super::task;

/// The first 8 bytes of every cache file.
pub const MAGIC: [u8; 8] = *b"FASMXDB1";

/// The layout version; bumped on any change of the byte layout (the
/// loader fingerprint covers changes of what the text loader produces).
pub const FORMAT_VERSION: u32 = 1;

/// Length of the fixed prefix.
pub(crate) const PREFIX_LEN: usize = 8 + 4 + 4 + 8 + 32 + 32;

/// A 32 byte BLAKE3 hash.
pub type Hash = [u8; 32];

pub(crate) fn hash(bytes: &[u8]) -> Hash {
    *blake3::hash(bytes).as_bytes()
}

/// A decoding error: what is malformed.
pub(crate) type Corrupt = String;

// ---------------------------------------------------------------------
// Primitive writer / reader.

#[derive(Default)]
pub(crate) struct Writer {
    pub(crate) buf: Vec<u8>,
}

impl Writer {
    pub(crate) fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub(crate) fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn bytes(&mut self, v: &[u8]) {
        self.len(v.len());
        self.buf.extend_from_slice(v);
    }
    pub(crate) fn str(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }
    pub(crate) fn hash(&mut self, v: &Hash) {
        self.buf.extend_from_slice(v);
    }
    /// A count or length (the tables are far below 4 G entries: the
    /// loader stores its own indexes as `u32`).
    pub(crate) fn len(&mut self, n: usize) {
        self.u32(u32::try_from(n).expect("table too large for the cache format"));
    }
}

pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], Corrupt> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&end| end <= self.buf.len())
            .ok_or_else(|| format!("truncated at byte {}", self.pos))?;
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }
    pub(crate) fn u8(&mut self) -> Result<u8, Corrupt> {
        Ok(self.take(1)?[0])
    }
    pub(crate) fn bool(&mut self) -> Result<bool, Corrupt> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            v => Err(format!("invalid flag {v}")),
        }
    }
    pub(crate) fn u32(&mut self) -> Result<u32, Corrupt> {
        Ok(le32(self.take(4)?))
    }
    pub(crate) fn u64(&mut self) -> Result<u64, Corrupt> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes(b.try_into().expect("8 bytes")))
    }
    pub(crate) fn i64(&mut self) -> Result<i64, Corrupt> {
        Ok(self.u64()? as i64)
    }
    pub(crate) fn len(&mut self) -> Result<usize, Corrupt> {
        Ok(self.u32()? as usize)
    }
    pub(crate) fn bytes(&mut self) -> Result<&'a [u8], Corrupt> {
        let n = self.len()?;
        self.take(n)
    }
    pub(crate) fn str(&mut self) -> Result<&'a str, Corrupt> {
        std::str::from_utf8(self.bytes()?).map_err(|e| format!("invalid UTF-8: {e}"))
    }
    pub(crate) fn string(&mut self) -> Result<String, Corrupt> {
        self.str().map(str::to_owned)
    }
    pub(crate) fn hash(&mut self) -> Result<Hash, Corrupt> {
        Ok(self.take(32)?.try_into().expect("32 bytes"))
    }
    /// A table of records of `size` bytes: its count, then the records.
    fn table(&mut self, size: usize) -> Result<&'a [u8], Corrupt> {
        let n = self.len()?;
        let total = n
            .checked_mul(size)
            .ok_or_else(|| "table length overflow".to_owned())?;
        self.take(total)
    }
    /// The records of a [`Reader::table`].
    fn records(&mut self, size: usize) -> Result<std::slice::ChunksExact<'a, u8>, Corrupt> {
        Ok(self.table(size)?.chunks_exact(size))
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.pos == self.buf.len()
    }
}

// ---------------------------------------------------------------------
// Payload: the Database.

/// Collects the distinct strings of the [`IdString`]s written.
#[derive(Default)]
struct StringTable {
    index: HashMap<IdString, u32>,
    list: Vec<IdString>,
}

impl StringTable {
    fn id(&mut self, s: IdString) -> u32 {
        let next = self.list.len() as u32;
        *self.index.entry(s).or_insert_with(|| {
            self.list.push(s);
            next
        })
    }
}

pub(crate) fn arch_code(arch: Architecture) -> u8 {
    match arch {
        Architecture::Series7 => 0,
        Architecture::UltraScale => 1,
        Architecture::UltraScalePlus => 2,
    }
}

pub(crate) fn arch_from_code(code: u8) -> Result<Architecture, Corrupt> {
    Architecture::ALL
        .get(code as usize)
        .copied()
        .filter(|&a| arch_code(a) == code)
        .ok_or_else(|| format!("invalid architecture {code}"))
}

pub(crate) fn layout_code(layout: Layout) -> u8 {
    match layout {
        Layout::Prjxray => 0,
        Layout::Prjuray => 1,
    }
}

pub(crate) fn layout_from_code(code: u8) -> Result<Layout, Corrupt> {
    match code {
        0 => Ok(Layout::Prjxray),
        1 => Ok(Layout::Prjuray),
        _ => Err(format!("invalid layout {code}")),
    }
}

fn block_type(code: u8) -> Result<BlockType, Corrupt> {
    BlockType::from_raw(code).ok_or_else(|| format!("invalid block type {code}"))
}

fn ppip_code(t: PpipType) -> u8 {
    match t {
        PpipType::Always => 0,
        PpipType::Default => 1,
        PpipType::Hint => 2,
    }
}

fn ppip_type(code: u8) -> Result<PpipType, Corrupt> {
    match code {
        0 => Ok(PpipType::Always),
        1 => Ok(PpipType::Default),
        2 => Ok(PpipType::Hint),
        _ => Err(format!("invalid pseudo PIP type {code}")),
    }
}

fn files_code(f: TileTypeFiles) -> u8 {
    u8::from(f.segbits)
        | u8::from(f.block_ram_segbits) << 1
        | u8::from(f.ppips) << 2
        | u8::from(f.mask) << 3
}

fn files_from_code(code: u8) -> Result<TileTypeFiles, Corrupt> {
    if code > 0xf {
        return Err(format!("invalid tile type files {code}"));
    }
    Ok(TileTypeFiles {
        segbits: code & 1 != 0,
        block_ram_segbits: code & 2 != 0,
        ppips: code & 4 != 0,
        mask: code & 8 != 0,
    })
}

fn opt_str(w: &mut Writer, s: Option<&str>) {
    w.u8(u8::from(s.is_some()));
    if let Some(s) = s {
        w.str(s);
    }
}

fn read_opt_string(r: &mut Reader<'_>) -> Result<Option<String>, Corrupt> {
    Ok(if r.bool()? { Some(r.string()?) } else { None })
}

fn span(w: &mut Writer, s: Span) {
    w.u32(s.start);
    w.u32(s.len);
}

// The payload is a table of independent sections, each with its own
// string table, so that they can be decoded (and their strings interned)
// on several threads:
//
//   n_sections u32, then per section: kind u8, length u64,
//   then the sections, in order.
//
// Sections: `TYPE_GROUPS` groups of consecutive tile types, the grid, the
// part data.

const SECTION_TYPES: u8 = 0;
const SECTION_GRID: u8 = 1;
const SECTION_PART: u8 = 2;

/// Number of tile type sections written (balanced by size).
const TYPE_GROUPS: usize = 4;

/// Serialises the tables of `db` (everything but the root, layout and
/// architecture, which are in the header, and the derived indexes, which
/// are rebuilt).
///
/// The sections are encoded on scoped threads; `None` if one of them
/// panicked.
pub(crate) fn encode_payload(db: &Database) -> Option<Vec<u8>> {
    let sections: Vec<(u8, Vec<u8>)> = std::thread::scope(|scope| {
        let mut tasks = Vec::new();
        for group in type_groups(&db.tile_types) {
            tasks.push((
                SECTION_TYPES,
                task::spawn(scope, move || {
                    encode_section(|w, [strings, _]| encode_types(w, strings, group))
                }),
            ));
        }
        tasks.push((
            SECTION_GRID,
            task::spawn(scope, || {
                encode_section(|w, tables| encode_grid(w, tables, db.grid.as_ref()))
            }),
        ));
        tasks.push((
            SECTION_PART,
            task::spawn(scope, || {
                encode_section(|w, [strings, _]| encode_part(w, strings, db.part.as_ref()))
            }),
        ));
        tasks
            .into_iter()
            .map(|(kind, task)| Some((kind, task.join()?)))
            .collect::<Option<_>>()
    })?;
    let mut out = Writer::default();
    out.len(sections.len());
    for (kind, bytes) in &sections {
        out.u8(*kind);
        out.u64(bytes.len() as u64);
    }
    for (_, bytes) in &sections {
        out.buf.extend_from_slice(bytes);
    }
    Some(out.buf)
}

/// Splits the tile types into up to [`TYPE_GROUPS`] runs of about the same
/// size.
fn type_groups(types: &[TileType]) -> Vec<&[TileType]> {
    // Decoding time is mostly interning names and filling the hash
    // maps, one per entry, addressed entry and pseudo PIP.
    let weight = |t: &TileType| {
        let s = &t.segbits;
        1 + s.entries.len() + s.addressed.len() + s.ppips.len() + s.bits.len() / 8
    };
    let total: usize = types.iter().map(weight).sum();
    let target = total.div_ceil(TYPE_GROUPS).max(1);
    let mut groups = Vec::new();
    let (mut start, mut acc) = (0, 0);
    for (i, t) in types.iter().enumerate() {
        acc += weight(t);
        if acc >= target {
            groups.push(&types[start..=i]);
            start = i + 1;
            acc = 0;
        }
    }
    if start < types.len() || groups.is_empty() {
        groups.push(&types[start..]);
    }
    groups
}

/// A section: two string tables, then what `body` writes. (Only the
/// grid uses the second table: its tile names are interned first, then
/// the tiles are indexed on one thread while the other names are
/// interned on another.)
fn encode_section(body: impl FnOnce(&mut Writer, &mut [StringTable; 2])) -> Vec<u8> {
    let mut tables = [StringTable::default(), StringTable::default()];
    let mut w = Writer::default();
    body(&mut w, &mut tables);
    let mut out = Writer::default();
    for strings in &tables {
        out.len(strings.list.len());
        let mut blob = Vec::new();
        for s in &strings.list {
            s.with_str(|s| {
                out.u32(s.len() as u32);
                blob.extend_from_slice(s.as_bytes());
            });
        }
        out.bytes(&blob);
    }
    out.buf.extend_from_slice(&w.buf);
    out.buf
}

fn encode_types(w: &mut Writer, strings: &mut StringTable, types: &[TileType]) {
    w.len(types.len());
    for tile_type in types {
        w.u32(strings.id(tile_type.name));
        w.u8(files_code(tile_type.files));
        let s = &tile_type.segbits;
        w.u64(s.foreign_lines as u64);
        w.len(s.entries.len());
        for e in &s.entries {
            w.u32(strings.id(e.feature));
            w.u8(e.block_type.raw());
            w.u32(e.start);
            w.u32(e.len);
        }
        w.len(s.bits.len());
        for b in &s.bits {
            w.u32(b.word_column);
            w.u32(b.word_bit);
            w.u8(u8::from(b.is_set));
        }
        w.len(s.ppips.len());
        for &(feature, kind) in &s.ppips {
            w.u32(strings.id(feature));
            w.u8(ppip_code(kind));
        }
        w.len(s.addressed.len());
        for (&(base, address), &entry) in &s.addressed {
            w.u32(strings.id(base));
            w.u32(address);
            w.u32(entry);
        }
    }
}

/// The grid: tile names, types and clock regions in the first string
/// table, the other names in the second.
fn encode_grid(
    w: &mut Writer,
    [tile_strings, strings]: &mut [StringTable; 2],
    grid: Option<&Grid>,
) {
    w.u8(u8::from(grid.is_some()));
    let Some(g) = grid else {
        return;
    };
    w.len(g.tiles.len());
    for t in &g.tiles {
        w.u32(tile_strings.id(t.name));
        w.u32(tile_strings.id(t.tile_type));
        w.i32(t.grid_x);
        w.i32(t.grid_y);
        match t.clock_region {
            None => {
                w.u8(0);
                w.u32(0);
                w.u32(0);
                w.u32(0);
            }
            Some(cr) => {
                w.u8(1);
                w.u32(tile_strings.id(cr.name));
                w.u32(cr.x);
                w.u32(cr.y);
            }
        }
        w.u32(t.type_index);
        span(w, t.bits);
        span(w, t.sites);
        span(w, t.pin_functions);
        span(w, t.prohibited_sites);
    }
    w.len(g.bits.len());
    for b in &g.bits {
        w.u8(b.block_type.raw());
        w.u32(b.base_address);
        w.u32(b.frames);
        w.u32(b.offset);
        w.u32(b.words);
        w.u32(b.alias.unwrap_or(u32::MAX));
    }
    w.len(g.aliases.len());
    for a in &g.aliases {
        w.u32(strings.id(a.tile_type));
        w.u32(a.start_offset);
        span(w, a.sites);
    }
    w.len(g.pairs.len());
    for &(a, b) in &g.pairs {
        w.u32(strings.id(a));
        w.u32(strings.id(b));
    }
    w.len(g.names.len());
    for &n in &g.names {
        w.u32(strings.id(n));
    }
}

fn encode_part(w: &mut Writer, strings: &mut StringTable, info: Option<&PartInfo>) {
    w.u8(u8::from(info.is_some()));
    let Some(info) = info else {
        return;
    };
    w.str(&info.name);
    opt_str(w, info.device.as_deref());
    w.str(&info.fabric);
    w.u8(u8::from(info.part.is_some()));
    if let Some(p) = &info.part {
        w.u8(arch_code(p.architecture));
        w.u32(p.idcode);
        w.len(p.rows().len());
        for row in p.rows() {
            w.u8(u8::from(row.bottom));
            w.u32(row.row);
            w.len(row.buses.len());
            for bus in &row.buses {
                w.u8(bus.block_type.raw());
                w.len(bus.columns.len());
                for &(column, frames) in &bus.columns {
                    w.u32(column);
                    w.u32(frames);
                }
            }
        }
    }
    w.u8(u8::from(info.idcode.is_some()));
    w.u32(info.idcode.unwrap_or(0));
    w.u8(u8::from(info.iobanks.is_some()));
    if let Some(banks) = &info.iobanks {
        w.len(banks.len());
        for &(bank, loc) in banks {
            w.u32(strings.id(bank));
            w.u32(strings.id(loc));
        }
    }
    w.u8(u8::from(info.package_pins.is_some()));
    if let Some(pins) = &info.package_pins {
        w.len(pins.len());
        for p in pins {
            for s in [p.pin, p.bank, p.site, p.tile, p.pin_function] {
                w.u32(strings.id(s));
            }
        }
    }
    w.len(info.required_features.len());
    for f in &info.required_features {
        w.str(f);
    }
}

/// The interned strings of a section.
struct Ids(Vec<IdString>);

impl Ids {
    #[inline]
    fn get(&self, raw: u32) -> Result<IdString, Corrupt> {
        self.0
            .get(raw as usize)
            .copied()
            .ok_or_else(|| format!("string index {raw} out of range"))
    }
}

/// A string table, not interned yet.
struct RawStrings<'a> {
    lengths: &'a [u8],
    blob: &'a str,
}

impl<'a> RawStrings<'a> {
    fn read(r: &mut Reader<'a>) -> Result<Self, Corrupt> {
        let n = r.len()?;
        let lengths = r.take(n.checked_mul(4).ok_or("string table overflow")?)?;
        let blob = r.str()?;
        Ok(RawStrings { lengths, blob })
    }

    /// Interns the strings.
    fn intern(&self) -> Result<Ids, Corrupt> {
        let mut ids = Vec::with_capacity(self.lengths.len() / 4);
        let mut pos = 0usize;
        for len in self.lengths.chunks_exact(4) {
            let end = pos.saturating_add(le32(len) as usize);
            let s = self
                .blob
                .get(pos..end)
                .ok_or_else(|| "string table out of range".to_owned())?;
            ids.push(IdString::new(s));
            pos = end;
        }
        if pos != self.blob.len() {
            return Err("string table has trailing bytes".to_owned());
        }
        Ok(Ids(ids))
    }
}

fn read_span(c: &[u8]) -> Span {
    Span {
        start: le32(&c[0..4]),
        len: le32(&c[4..8]),
    }
}

fn check_span(s: Span, len: usize, what: &str) -> Result<(), Corrupt> {
    if (s.start as usize).saturating_add(s.len as usize) <= len {
        Ok(())
    } else {
        Err(format!("{what} span out of range"))
    }
}

fn decode_types(r: &mut Reader<'_>, ids: &Ids) -> Result<Vec<TileType>, Corrupt> {
    let n = r.len()?;
    let mut types = Vec::with_capacity(n.min(1 << 12));
    for _ in 0..n {
        let name = ids.get(r.u32()?)?;
        let files = files_from_code(r.u8()?)?;
        let segbits = decode_segbits(r, ids)?;
        types.push(TileType {
            name,
            segbits,
            files,
        });
    }
    Ok(types)
}

fn decode_segbits(r: &mut Reader<'_>, ids: &Ids) -> Result<TileSegbits, Corrupt> {
    let foreign_lines = usize::try_from(r.u64()?).map_err(|_| "foreign_lines overflow")?;
    let entries = r
        .records(13)?
        .map(|c| {
            Ok(SegbitsEntry {
                feature: ids.get(le32(&c[0..4]))?,
                block_type: block_type(c[4])?,
                start: le32(&c[5..9]),
                len: le32(&c[9..13]),
            })
        })
        .collect::<Result<Vec<_>, Corrupt>>()?;
    let bits = r
        .records(9)?
        .map(|c| {
            Ok(SegBit {
                word_column: le32(&c[0..4]),
                word_bit: le32(&c[4..8]),
                is_set: match c[8] {
                    0 => false,
                    1 => true,
                    v => return Err(format!("invalid bit flag {v}")),
                },
            })
        })
        .collect::<Result<Vec<_>, Corrupt>>()?;
    for e in &entries {
        check_span(
            Span {
                start: e.start,
                len: e.len,
            },
            bits.len(),
            "segbits entry",
        )?;
    }
    let ppips = r
        .records(5)?
        .map(|c| Ok((ids.get(le32(&c[0..4]))?, ppip_type(c[4])?)))
        .collect::<Result<Vec<_>, Corrupt>>()?;
    let addressed_records = r.records(12)?;
    let mut addressed = HashMap::default();
    addressed.reserve(addressed_records.len());
    for c in addressed_records {
        let entry = le32(&c[8..12]);
        if entry as usize >= entries.len() {
            return Err("addressed entry out of range".to_owned());
        }
        addressed.insert((ids.get(le32(&c[0..4]))?, le32(&c[4..8])), entry);
    }
    // The derived indexes, as `TileSegbitsBuilder::finish` builds them.
    let mut by_name = HashMap::default();
    by_name.reserve(entries.len());
    for (i, e) in entries.iter().enumerate() {
        by_name.entry(e.feature).or_insert(i as u32);
    }
    let mut ppip_index = HashMap::default();
    ppip_index.reserve(ppips.len());
    ppip_index.extend(ppips.iter().copied());
    Ok(TileSegbits {
        entries,
        bits,
        by_name,
        addressed,
        ppips,
        ppip_index,
        foreign_lines,
    })
}

fn decode_tiles(records: &[u8], ids: &Ids) -> Result<Vec<Tile>, Corrupt> {
    records
        .chunks_exact(TILE_RECORD)
        .map(|c| {
            let clock_region = match c[16] {
                0 => None,
                1 => Some(ClockRegion {
                    name: ids.get(le32(&c[17..21]))?,
                    x: le32(&c[21..25]),
                    y: le32(&c[25..29]),
                }),
                v => return Err(format!("invalid clock region flag {v}")),
            };
            Ok(Tile {
                name: ids.get(le32(&c[0..4]))?,
                tile_type: ids.get(le32(&c[4..8]))?,
                grid_x: le32(&c[8..12]) as i32,
                grid_y: le32(&c[12..16]) as i32,
                clock_region,
                // Checked against the number of tile types by
                // `decode_payload`.
                type_index: le32(&c[29..33]),
                bits: read_span(&c[33..41]),
                sites: read_span(&c[41..49]),
                pin_functions: read_span(&c[49..57]),
                prohibited_sites: read_span(&c[57..65]),
            })
        })
        .collect()
}

/// `Grid::by_loc` from the raw tile records (no strings needed).
fn tiles_by_loc(records: &[u8]) -> HashMap<(i32, i32), u32> {
    let mut by_loc = HashMap::default();
    by_loc.reserve(records.len() / TILE_RECORD);
    for (i, c) in records.chunks_exact(TILE_RECORD).enumerate() {
        by_loc.insert((le32(&c[8..12]) as i32, le32(&c[12..16]) as i32), i as u32);
    }
    by_loc
}

/// The tiles, with `Grid::by_name`.
fn decode_indexed_tiles(
    records: &[u8],
    ids: &Ids,
) -> Result<(Vec<Tile>, HashMap<IdString, u32>), Corrupt> {
    let tiles = decode_tiles(records, ids)?;
    let mut by_name = HashMap::default();
    by_name.reserve(tiles.len());
    for (i, t) in tiles.iter().enumerate() {
        by_name.insert(t.name, i as u32);
    }
    Ok((tiles, by_name))
}

/// The per tile arrays of the grid other than the tiles.
struct GridArrays {
    bits: Vec<BitsBlock>,
    aliases: Vec<BitAlias>,
    pairs: Vec<(IdString, IdString)>,
    names: Vec<IdString>,
}

fn decode_grid_arrays(
    bits: &[u8],
    aliases: &[u8],
    pairs: &[u8],
    names: &[u8],
    ids: &Ids,
) -> Result<GridArrays, Corrupt> {
    let bits = bits
        .chunks_exact(21)
        .map(|c| {
            let alias = le32(&c[17..21]);
            Ok(BitsBlock {
                block_type: block_type(c[0])?,
                base_address: le32(&c[1..5]),
                frames: le32(&c[5..9]),
                offset: le32(&c[9..13]),
                words: le32(&c[13..17]),
                alias: (alias != u32::MAX).then_some(alias),
            })
        })
        .collect::<Result<Vec<_>, Corrupt>>()?;
    let aliases = aliases
        .chunks_exact(16)
        .map(|c| {
            Ok(BitAlias {
                tile_type: ids.get(le32(&c[0..4]))?,
                start_offset: le32(&c[4..8]),
                sites: read_span(&c[8..16]),
            })
        })
        .collect::<Result<Vec<_>, Corrupt>>()?;
    let pairs = pairs
        .chunks_exact(8)
        .map(|c| Ok((ids.get(le32(&c[0..4]))?, ids.get(le32(&c[4..8]))?)))
        .collect::<Result<Vec<_>, Corrupt>>()?;
    let names = names
        .chunks_exact(4)
        .map(|c| ids.get(le32(c)))
        .collect::<Result<Vec<_>, Corrupt>>()?;
    Ok(GridArrays {
        bits,
        aliases,
        pairs,
        names,
    })
}

/// Size of a tile record.
const TILE_RECORD: usize = 65;

/// Decodes the grid section after its two string tables (tile names,
/// types and clock regions; the other names). With `parallel`, `by_loc`
/// is built on a scoped thread from the start (it needs no strings), and
/// the tiles and `by_name` on another one as soon as the tile names are
/// interned, while this thread interns the other names.
fn decode_grid(
    r: &mut Reader<'_>,
    tile_strings: &RawStrings<'_>,
    strings: &RawStrings<'_>,
    parallel: bool,
) -> Result<Option<Grid>, Corrupt> {
    if !r.bool()? {
        return Ok(None);
    }
    let tile_records = r.table(TILE_RECORD)?;
    let bits = r.table(21)?;
    let aliases = r.table(16)?;
    let pairs = r.table(8)?;
    let names = r.table(4)?;

    let ((tiles, by_name), by_loc, arrays) = if parallel {
        std::thread::scope(|scope| {
            let by_loc = task::spawn(scope, || tiles_by_loc(tile_records));
            let tile_ids = tile_strings.intern();
            let indexed = task::spawn(scope, move || {
                decode_indexed_tiles(tile_records, &tile_ids?)
            });
            let arrays = strings
                .intern()
                .and_then(|ids| decode_grid_arrays(bits, aliases, pairs, names, &ids));
            let panicked = || "grid decoder panicked".to_owned();
            let indexed = indexed.join().unwrap_or_else(|| Err(panicked()))?;
            let by_loc = by_loc.join().ok_or_else(panicked)?;
            Ok::<_, Corrupt>((indexed, by_loc, arrays?))
        })?
    } else {
        let tile_ids = tile_strings.intern()?;
        let ids = strings.intern()?;
        (
            decode_indexed_tiles(tile_records, &tile_ids)?,
            tiles_by_loc(tile_records),
            decode_grid_arrays(bits, aliases, pairs, names, &ids)?,
        )
    };
    let GridArrays {
        bits,
        aliases,
        pairs,
        names,
    } = arrays;

    for b in &bits {
        if b.alias.is_some_and(|a| a as usize >= aliases.len()) {
            return Err("alias index out of range".to_owned());
        }
    }
    for a in &aliases {
        check_span(a.sites, pairs.len(), "alias sites")?;
    }
    for t in &tiles {
        check_span(t.bits, bits.len(), "tile bits")?;
        check_span(t.sites, pairs.len(), "tile sites")?;
        check_span(t.pin_functions, pairs.len(), "tile pin functions")?;
        check_span(t.prohibited_sites, names.len(), "tile prohibited sites")?;
    }
    Ok(Some(Grid {
        tiles,
        by_name,
        by_loc,
        bits,
        aliases,
        pairs,
        names,
    }))
}

fn decode_frame_tree(r: &mut Reader<'_>) -> Result<Part, Corrupt> {
    let architecture = arch_from_code(r.u8()?)?;
    let idcode = r.u32()?;
    let n_rows = r.len()?;
    let mut rows = Vec::with_capacity(n_rows.min(1024));
    for _ in 0..n_rows {
        let bottom = r.bool()?;
        let row = r.u32()?;
        let n_buses = r.len()?;
        let mut buses = Vec::with_capacity(n_buses.min(8));
        for _ in 0..n_buses {
            let block_type = block_type(r.u8()?)?;
            let columns = r
                .records(8)?
                .map(|c| (le32(&c[0..4]), le32(&c[4..8])))
                .collect();
            buses.push(ConfigBus {
                block_type,
                columns,
            });
        }
        rows.push(ConfigRow { bottom, row, buses });
    }
    Part::new(architecture, idcode, rows).map_err(|e| format!("invalid part: {e}"))
}

fn decode_part(r: &mut Reader<'_>, ids: &Ids, root: &Path) -> Result<Option<PartInfo>, Corrupt> {
    if !r.bool()? {
        return Ok(None);
    }
    let name = r.string()?;
    let device = read_opt_string(r)?;
    let fabric = r.string()?;
    let part = if r.bool()? {
        Some(decode_frame_tree(r)?)
    } else {
        None
    };
    let has_idcode = r.bool()?;
    let idcode = r.u32()?;
    let iobanks = if r.bool()? {
        Some(
            r.records(8)?
                .map(|c| Ok((ids.get(le32(&c[0..4]))?, ids.get(le32(&c[4..8]))?)))
                .collect::<Result<Vec<_>, Corrupt>>()?,
        )
    } else {
        None
    };
    let package_pins = if r.bool()? {
        Some(
            r.records(20)?
                .map(|c| {
                    Ok(PackagePin {
                        pin: ids.get(le32(&c[0..4]))?,
                        bank: ids.get(le32(&c[4..8]))?,
                        site: ids.get(le32(&c[8..12]))?,
                        tile: ids.get(le32(&c[12..16]))?,
                        pin_function: ids.get(le32(&c[16..20]))?,
                    })
                })
                .collect::<Result<Vec<_>, Corrupt>>()?,
        )
    } else {
        None
    };
    let n = r.len()?;
    let mut required_features = Vec::with_capacity(n.min(1 << 16));
    for _ in 0..n {
        required_features.push(r.string()?);
    }
    Ok(Some(PartInfo {
        directory: root.join(&name),
        name,
        device,
        fabric,
        part,
        idcode: has_idcode.then_some(idcode),
        iobanks,
        package_pins,
        required_features,
    }))
}

/// A decoded section.
enum Section {
    Types(Vec<TileType>),
    Grid(Option<Grid>),
    Part(Option<PartInfo>),
}

fn decode_section(kind: u8, bytes: &[u8], root: &Path, parallel: bool) -> Result<Section, Corrupt> {
    let mut r = Reader::new(bytes);
    let first = RawStrings::read(&mut r)?;
    let second = RawStrings::read(&mut r)?;
    let section = match kind {
        SECTION_GRID => Section::Grid(decode_grid(&mut r, &first, &second, parallel)?),
        SECTION_TYPES | SECTION_PART => {
            if !second.lengths.is_empty() {
                return Err("unexpected second string table".to_owned());
            }
            let ids = first.intern()?;
            if kind == SECTION_TYPES {
                Section::Types(decode_types(&mut r, &ids)?)
            } else {
                Section::Part(decode_part(&mut r, &ids, root)?)
            }
        }
        _ => return Err(format!("unknown section kind {kind}")),
    };
    if !r.is_empty() {
        return Err("trailing bytes in a section".to_owned());
    }
    Ok(section)
}

/// Rebuilds a [`Database`] from a payload written by [`encode_payload`].
///
/// With `parallel`, the sections are decoded on separate threads (the
/// calling thread and scoped threads): most of the time goes into
/// interning names and building hash maps, and the sections are
/// independent.
pub(crate) fn decode_payload(
    payload: &[u8],
    root: &Path,
    layout: Layout,
    architecture: Architecture,
    parallel: bool,
) -> Result<Database, Corrupt> {
    let mut r = Reader::new(payload);
    let n = r.len()?;
    let mut table = Vec::with_capacity(n.min(64));
    for _ in 0..n {
        let kind = r.u8()?;
        let len = usize::try_from(r.u64()?).map_err(|_| "section too large")?;
        table.push((kind, len));
    }
    let mut sections = Vec::with_capacity(table.len());
    for (kind, len) in table {
        sections.push((kind, r.take(len)?));
    }
    if !r.is_empty() {
        return Err("trailing bytes after the sections".to_owned());
    }

    let decoded: Vec<Result<Section, Corrupt>> = if !parallel {
        sections
            .iter()
            .map(|&(kind, bytes)| decode_section(kind, bytes, root, false))
            .collect()
    } else {
        std::thread::scope(|scope| {
            // The largest section (the grid of a big part, or a tile type
            // group) runs on this thread, the others on scoped threads.
            let largest = (0..sections.len())
                .max_by_key(|&i| sections[i].1.len())
                .unwrap_or(0);
            let tasks: Vec<_> = sections
                .iter()
                .enumerate()
                .map(|(i, &(kind, bytes))| {
                    (i != largest).then(|| {
                        task::spawn(scope, move || decode_section(kind, bytes, root, true))
                    })
                })
                .collect();
            let mut own = sections
                .get(largest)
                .map(|&(kind, bytes)| decode_section(kind, bytes, root, true));
            let panicked = || Err("section decoder panicked".to_owned());
            tasks
                .into_iter()
                .map(|task| match task {
                    Some(task) => task.join().unwrap_or_else(panicked),
                    None => own.take().unwrap_or_else(panicked),
                })
                .collect()
        })
    };

    let mut tile_types = Vec::new();
    let mut grid = None;
    let mut part = None;
    let (mut have_grid, mut have_part) = (false, false);
    for section in decoded {
        match section? {
            Section::Types(types) => tile_types.extend(types),
            Section::Grid(g) if !have_grid => {
                have_grid = true;
                grid = g;
            }
            Section::Part(p) if !have_part => {
                have_part = true;
                part = p;
            }
            _ => return Err("repeated section".to_owned()),
        }
    }
    if !have_grid || !have_part {
        return Err("missing section".to_owned());
    }
    if let Some(g) = &grid {
        if g.tiles
            .iter()
            .any(|t| t.type_index != u32::MAX && t.type_index as usize >= tile_types.len())
        {
            return Err("tile type index out of range".to_owned());
        }
    }
    let mut tile_type_index = HashMap::default();
    tile_type_index.reserve(tile_types.len());
    for (i, t) in tile_types.iter().enumerate() {
        tile_type_index.insert(t.name, i as u32);
    }
    let banks =
        part.as_ref().and_then(
            |info: &PartInfo| match (&info.iobanks, &info.package_pins) {
                (Some(iobanks), Some(pins)) => Some(BanksTilesRegistry::new(iobanks, pins)),
                _ => None,
            },
        );
    Ok(Database {
        root: PathBuf::from(root),
        layout,
        architecture,
        tile_types,
        tile_type_index,
        grid,
        part,
        banks,
    })
}

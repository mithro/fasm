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

//! `tilegrid.json`: the tiles of a fabric (prjxray-db) or part
//! (prjuray-db) and their configuration `bits` blocks (`prjxray/grid.py`,
//! design document §3.1).

use std::fmt;
use std::path::Path;

use fasm::idstring::IdString;
use foldhash::HashMap;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, Visitor};
use serde::Deserialize;

use crate::arch::BlockType;
use crate::error::{read_file, DbError};
use crate::json::{JStr, OrderedMap, PyInt};

/// A range of one of the flat arrays of [`Grid`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Span {
    start: u32,
    len: u32,
}

impl Span {
    fn range(self) -> std::ops::Range<usize> {
        self.start as usize..(self.start + self.len) as usize
    }
}

/// A clock region name `X<x>Y<y>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClockRegion {
    /// The name, e.g. `X0Y1`.
    pub name: IdString,
    /// The X coordinate.
    pub x: u32,
    /// The Y coordinate.
    pub y: u32,
}

impl ClockRegion {
    /// Parses `X<digits>Y<digits>` (prjxray's `X([0-9])Y([0-9])` allows
    /// one digit per coordinate; more are accepted here).
    pub fn parse(name: &str) -> Option<Self> {
        let rest = name.strip_prefix('X')?;
        let (x, y) = rest.split_once('Y')?;
        Some(ClockRegion {
            name: IdString::new(name),
            x: crate::segbits::parse_u32(x)?,
            y: crate::segbits::parse_u32(y)?,
        })
    }
}

/// One `bits` block of a tile: where the tile's bits for one bus live in
/// the frames (`grid_types.Bits`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BitsBlock {
    /// The bus (the key of the block in `bits`).
    pub block_type: BlockType,
    /// Frame address of the tile's first frame (`baseaddr`).
    pub base_address: u32,
    /// Number of frames (`frames`).
    pub frames: u32,
    /// First word of the tile in each frame (`offset`), in units of
    /// [`crate::Architecture::segbit_word_bits`].
    pub offset: u32,
    /// Number of words (`words`), same unit.
    pub words: u32,
    /// Index of the alias in the grid (see [`Grid::alias`]).
    alias: Option<u32>,
}

impl BitsBlock {
    /// `true` if the block carries an `alias`.
    pub fn has_alias(&self) -> bool {
        self.alias.is_some()
    }
}

/// The `alias` of a `bits` block (`grid_types.BitAlias`): the tile uses
/// the segbits of another tile type, starting `start_offset` words into
/// them, with its site names mapped by `sites`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BitAlias {
    /// The tile type whose segbits are used.
    pub tile_type: IdString,
    /// Word offset into the aliased tile type's bits.
    pub start_offset: u32,
    sites: Span,
}

/// One tile of the grid (`grid_types.GridInfo` + name and location).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tile {
    /// Tile name (the key in `tilegrid.json`, the first component of a
    /// FASM feature), e.g. `CLBLL_L_X2Y0`.
    pub name: IdString,
    /// Tile type, e.g. `CLBLL_L`.
    pub tile_type: IdString,
    /// Grid X coordinate.
    pub grid_x: i32,
    /// Grid Y coordinate.
    pub grid_y: i32,
    /// Clock region, if any.
    pub clock_region: Option<ClockRegion>,
    /// Index of the tile type in the database, `u32::MAX` if the family
    /// has no `tile_type_<TYPE>.json` for it.
    pub(crate) type_index: u32,
    bits: Span,
    sites: Span,
    pin_functions: Span,
    prohibited_sites: Span,
}

impl Tile {
    /// Index of the tile type in [`crate::Database::tile_types`], `None`
    /// if the database has no such tile type.
    pub fn tile_type_index(&self) -> Option<usize> {
        (self.type_index != u32::MAX).then_some(self.type_index as usize)
    }
}

/// The tile grid of a fabric (`prjxray.grid.Grid`).
///
/// Tiles are kept in file order. Variable sized per tile data (bits
/// blocks, sites, pin functions, prohibited sites, alias site maps) is
/// stored in flat arrays that tiles refer to by range, so the grid is a
/// handful of plain vectors (cheap to build, easy to serialise later).
#[derive(Clone, Debug, Default)]
pub struct Grid {
    tiles: Vec<Tile>,
    by_name: HashMap<IdString, u32>,
    by_loc: HashMap<(i32, i32), u32>,
    bits: Vec<BitsBlock>,
    aliases: Vec<BitAlias>,
    pairs: Vec<(IdString, IdString)>,
    names: Vec<IdString>,
}

impl Grid {
    /// Parses a `tilegrid.json` file.
    ///
    /// # Errors
    ///
    /// [`DbError::MissingFile`] / [`DbError::Io`] if the file cannot be
    /// read, [`DbError::Json`] (with line and column) if it is not valid
    /// JSON, has a tile without `type`/`grid_x`/`grid_y`, a malformed
    /// `bits` block, an unknown bus name, a malformed clock region, a
    /// repeated tile name or two tiles at the same location.
    pub fn from_file(path: &Path) -> Result<Self, DbError> {
        let data = read_file(path)?;
        Self::from_json_slice(&data).map_err(|e| DbError::json(path, &e))
    }

    /// Parses the contents of a `tilegrid.json` file.
    pub fn from_json_slice(data: &[u8]) -> Result<Self, serde_json::Error> {
        let mut grid = Grid::default();
        let mut de = serde_json::Deserializer::from_slice(data);
        GridSeed(&mut grid).deserialize(&mut de)?;
        de.end()?;
        grid.tiles.shrink_to_fit();
        grid.bits.shrink_to_fit();
        grid.pairs.shrink_to_fit();
        grid.names.shrink_to_fit();
        Ok(grid)
    }

    /// All tiles, in file order.
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    /// Mutable access for the database (tile type indexes).
    pub(crate) fn tiles_mut(&mut self) -> &mut [Tile] {
        &mut self.tiles
    }

    /// Number of tiles.
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// `true` if the grid has no tiles.
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Index of the tile named `name`.
    pub fn tile_index(&self, name: IdString) -> Option<usize> {
        self.by_name.get(&name).map(|&i| i as usize)
    }

    /// The tile named `name` (`gridinfo_at_tilename`).
    pub fn tile(&self, name: IdString) -> Option<&Tile> {
        self.tile_index(name).map(|i| &self.tiles[i])
    }

    /// The tile at a grid location (`gridinfo_at_loc`).
    pub fn tile_at(&self, grid_x: i32, grid_y: i32) -> Option<&Tile> {
        self.by_loc
            .get(&(grid_x, grid_y))
            .map(|&i| &self.tiles[i as usize])
    }

    /// `(x_min, x_max, y_min, y_max)` of the tile locations
    /// (`Grid.dims`), `None` for an empty grid.
    pub fn dims(&self) -> Option<(i32, i32, i32, i32)> {
        let first = self.tiles.first()?;
        let init = (first.grid_x, first.grid_x, first.grid_y, first.grid_y);
        Some(self.tiles.iter().fold(init, |(x0, x1, y0, y1), t| {
            (
                x0.min(t.grid_x),
                x1.max(t.grid_x),
                y0.min(t.grid_y),
                y1.max(t.grid_y),
            )
        }))
    }

    /// The `bits` blocks of a tile, in file order.
    pub fn bits(&self, tile: &Tile) -> &[BitsBlock] {
        &self.bits[tile.bits.range()]
    }

    /// The `bits` block of a tile for one bus.
    pub fn bits_block(&self, tile: &Tile, block_type: BlockType) -> Option<&BitsBlock> {
        self.bits(tile).iter().find(|b| b.block_type == block_type)
    }

    /// `(site name, site type)` pairs of a tile, in file order.
    pub fn sites(&self, tile: &Tile) -> &[(IdString, IdString)] {
        &self.pairs[tile.sites.range()]
    }

    /// `(site name, pin function)` pairs of a tile, in file order.
    ///
    /// prjuray-db also has sites whose value is a map of package pin to
    /// function (`"SYSMONE4_X0Y0": {"R13": "VP", "T12": "VN"}`); those give
    /// one pair per map entry, `(site, function)`, and the package pin
    /// names are dropped (nothing in the FASM -> frames pipeline uses
    /// UltraScale+ pin functions).
    pub fn pin_functions(&self, tile: &Tile) -> &[(IdString, IdString)] {
        &self.pairs[tile.pin_functions.range()]
    }

    /// Prohibited site names of a tile.
    pub fn prohibited_sites(&self, tile: &Tile) -> &[IdString] {
        &self.names[tile.prohibited_sites.range()]
    }

    /// The alias of a `bits` block.
    pub fn alias(&self, block: &BitsBlock) -> Option<&BitAlias> {
        block.alias.map(|i| &self.aliases[i as usize])
    }

    /// `(site, aliased site)` pairs of an alias.
    pub fn alias_sites(&self, alias: &BitAlias) -> &[(IdString, IdString)] {
        &self.pairs[alias.sites.range()]
    }

    /// The offset to use with the segbits of a block: `offset` for a plain
    /// block, `offset - alias.start_offset` for an aliased one
    /// (`TileSegbitsAlias.__init__`, may be negative).
    pub fn effective_offset(&self, block: &BitsBlock) -> i64 {
        let start = self.alias(block).map_or(0, |a| i64::from(a.start_offset));
        i64::from(block.offset) - start
    }

    /// Iterates over all `(tile, bits block)` pairs (`iter_all_frames`).
    pub fn iter_bits(&self) -> impl Iterator<Item = (&Tile, &BitsBlock)> {
        self.tiles
            .iter()
            .flat_map(move |tile| self.bits(tile).iter().map(move |b| (tile, b)))
    }

    fn push_pairs(&mut self, pairs: OrderedMap<JStr<'_>, JStr<'_>>) -> Result<Span, String> {
        let start = self.pairs.len();
        self.pairs.extend(
            pairs
                .0
                .iter()
                .map(|(k, v)| (IdString::new(k.as_str()), IdString::new(v.as_str()))),
        );
        span(start, self.pairs.len())
    }

    fn add_tile(&mut self, name: &str, raw: RawTile<'_>) -> Result<(), String> {
        let index = u32::try_from(self.tiles.len()).map_err(|_| "too many tiles".to_owned())?;
        let name = IdString::new(name);
        if self.by_name.insert(name, index).is_some() {
            return Err(format!("tile {name} is listed twice"));
        }
        if let Some(&other) = self.by_loc.get(&(raw.grid_x, raw.grid_y)) {
            return Err(format!(
                "tiles {} and {name} are both at grid location ({}, {})",
                self.tiles[other as usize].name, raw.grid_x, raw.grid_y
            ));
        }
        self.by_loc.insert((raw.grid_x, raw.grid_y), index);

        let clock_region =
            match &raw.clock_region {
                None => None,
                Some(s) => Some(ClockRegion::parse(s.as_str()).ok_or_else(|| {
                    format!("tile {name}: malformed clock_region {:?}", s.as_str())
                })?),
            };

        let bits_start = self.bits.len();
        for (bus, block) in raw.bits.map(|b| b.0).unwrap_or_default() {
            let block_type = BlockType::from_name(bus.as_str())
                .ok_or_else(|| format!("tile {name}: unknown bus {:?} in bits", bus.as_str()))?;
            if self.bits[bits_start..]
                .iter()
                .any(|b| b.block_type == block_type)
            {
                return Err(format!("tile {name}: bus {block_type} listed twice"));
            }
            let alias = match block.alias {
                None => None,
                Some(alias) => {
                    let sites = self.push_pairs(alias.sites)?;
                    let i = u32::try_from(self.aliases.len())
                        .map_err(|_| "too many aliases".to_owned())?;
                    self.aliases.push(BitAlias {
                        tile_type: IdString::new(alias.tile_type.as_str()),
                        start_offset: alias.start_offset,
                        sites,
                    });
                    Some(i)
                }
            };
            self.bits.push(BitsBlock {
                block_type,
                base_address: block.baseaddr.0,
                frames: block.frames,
                offset: block.offset,
                words: block.words,
                alias,
            });
        }
        let bits = span(bits_start, self.bits.len())?;
        let sites = self.push_pairs(raw.sites)?;
        let pin_start = self.pairs.len();
        for (site, function) in &raw.pin_functions.0 {
            let site = IdString::new(site.as_str());
            match function {
                PinFunction::One(f) => self.pairs.push((site, IdString::new(f.as_str()))),
                PinFunction::PerPin(map) => self
                    .pairs
                    .extend(map.0.iter().map(|(_, f)| (site, IdString::new(f.as_str())))),
            }
        }
        let pin_functions = span(pin_start, self.pairs.len())?;
        let names_start = self.names.len();
        self.names.extend(
            raw.prohibited_sites
                .iter()
                .map(|s| IdString::new(s.as_str())),
        );
        let prohibited_sites = span(names_start, self.names.len())?;

        self.tiles.push(Tile {
            name,
            tile_type: IdString::new(raw.tile_type.as_str()),
            grid_x: raw.grid_x,
            grid_y: raw.grid_y,
            clock_region,
            type_index: u32::MAX,
            bits,
            sites,
            pin_functions,
            prohibited_sites,
        });
        Ok(())
    }
}

fn span(start: usize, end: usize) -> Result<Span, String> {
    let too_big = || "tilegrid too large".to_owned();
    Ok(Span {
        start: u32::try_from(start).map_err(|_| too_big())?,
        len: u32::try_from(end - start).map_err(|_| too_big())?,
    })
}

#[derive(Deserialize)]
struct RawAlias<'a> {
    #[serde(rename = "type", borrow)]
    tile_type: JStr<'a>,
    start_offset: u32,
    #[serde(default, borrow)]
    sites: OrderedMap<JStr<'a>, JStr<'a>>,
}

#[derive(Deserialize)]
struct RawBits<'a> {
    baseaddr: PyInt,
    frames: u32,
    offset: u32,
    words: u32,
    #[serde(default, borrow)]
    alias: Option<RawAlias<'a>>,
}

/// One tile entry. `sites`, `prohibited_sites` and `pin_functions` default
/// to empty (prjxray requires the first two; prjuray-db has no
/// `prohibited_sites`); unknown keys (`segment`, `height`, ...) are
/// ignored.
#[derive(Deserialize)]
struct RawTile<'a> {
    #[serde(rename = "type", borrow)]
    tile_type: JStr<'a>,
    grid_x: i32,
    grid_y: i32,
    #[serde(default, borrow)]
    bits: Option<OrderedMap<JStr<'a>, RawBits<'a>>>,
    #[serde(default, borrow)]
    clock_region: Option<JStr<'a>>,
    #[serde(default, borrow)]
    sites: OrderedMap<JStr<'a>, JStr<'a>>,
    #[serde(default, borrow)]
    pin_functions: OrderedMap<JStr<'a>, PinFunction<'a>>,
    #[serde(default, borrow)]
    prohibited_sites: Vec<JStr<'a>>,
}

/// A `pin_functions` value: a string (prjxray-db) or a map of package pin
/// to function (some prjuray-db sites).
#[derive(Deserialize)]
#[serde(untagged)]
enum PinFunction<'a> {
    One(#[serde(borrow)] JStr<'a>),
    PerPin(#[serde(borrow)] OrderedMap<JStr<'a>, JStr<'a>>),
}

/// Streams the top level object into the grid, one tile at a time.
struct GridSeed<'g>(&'g mut Grid);

impl<'de> DeserializeSeed<'de> for GridSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for GridSeed<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an object of tiles")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(name) = map.next_key::<JStr<'de>>()? {
            let raw: RawTile<'de> = map.next_value()?;
            self.0
                .add_tile(name.as_str(), raw)
                .map_err(de::Error::custom)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: &str = r#"{
        "A_X0Y0": {"type": "A", "grid_x": 0, "grid_y": 1, "sites": {"S_X0Y0": "SLICEL"},
                   "prohibited_sites": [], "bits": {"CLB_IO_CLK": {"baseaddr": "0x00400100",
                   "frames": 36, "offset": 2, "words": 2, "height": 2}}, "segment": "x",
                   "clock_region": "X12Y3"},
        "B_X1Y0": {"type": "B", "grid_x": 1, "grid_y": 1, "sites": {}, "prohibited_sites": ["P"],
                   "pin_functions": {"IOB_X0Y1": "IO_L1P_T0_PUDC_B_14", "SYSMON": {"R13": "VP", "T12": "VN"}},
                   "bits": {"CLB_IO_CLK": {"alias": {"sites": {"IOB33_Y0": "IOB33_Y1"},
                   "start_offset": 2, "type": "A"}, "baseaddr": "0x00400000", "frames": 42,
                   "offset": 0, "words": 2},
                   "BLOCK_RAM": {"baseaddr": 8388608, "frames": 128, "offset": 0, "words": 10}},
                   "clock_region": null},
        "C_X2Y0": {"type": "C", "grid_x": 2, "grid_y": 0}
    }"#;

    #[test]
    fn parse_grid() {
        let grid = Grid::from_json_slice(GRID.as_bytes()).unwrap();
        assert_eq!(grid.len(), 3);
        let names: Vec<_> = grid.tiles().iter().map(|t| t.name.to_string()).collect();
        assert_eq!(names, ["A_X0Y0", "B_X1Y0", "C_X2Y0"]);
        let a = grid.tile(IdString::new("A_X0Y0")).unwrap();
        assert_eq!(a.tile_type, "A");
        assert_eq!(a.clock_region.unwrap().x, 12);
        assert_eq!(a.clock_region.unwrap().y, 3);
        assert_eq!(
            grid.sites(a),
            &[(IdString::new("S_X0Y0"), IdString::new("SLICEL"))]
        );
        let block = grid.bits_block(a, BlockType::ClbIoClk).unwrap();
        assert_eq!(
            (block.base_address, block.frames, block.offset, block.words),
            (0x0040_0100, 36, 2, 2)
        );
        assert_eq!(grid.effective_offset(block), 2);
        assert!(grid.bits_block(a, BlockType::BlockRam).is_none());

        let b = grid.tile_at(1, 1).unwrap();
        assert_eq!(b.name, "B_X1Y0");
        assert_eq!(b.clock_region, None);
        assert_eq!(grid.prohibited_sites(b), &[IdString::new("P")]);
        let pin_functions: Vec<(String, String)> = grid
            .pin_functions(b)
            .iter()
            .map(|(s, f)| (s.to_string(), f.to_string()))
            .collect();
        let pair = |s: &str, f: &str| (s.to_owned(), f.to_owned());
        assert_eq!(
            pin_functions,
            [
                pair("IOB_X0Y1", "IO_L1P_T0_PUDC_B_14"),
                pair("SYSMON", "VP"),
                pair("SYSMON", "VN")
            ]
        );
        let clb = grid.bits_block(b, BlockType::ClbIoClk).unwrap();
        let alias = grid.alias(clb).unwrap();
        assert_eq!(alias.tile_type, "A");
        assert_eq!(alias.start_offset, 2);
        assert_eq!(grid.alias_sites(alias)[0].1, "IOB33_Y1");
        assert_eq!(grid.effective_offset(clb), -2);
        assert_eq!(
            grid.bits_block(b, BlockType::BlockRam)
                .unwrap()
                .base_address,
            0x0080_0000
        );
        assert_eq!(grid.bits(b).len(), 2);

        let c = grid.tile(IdString::new("C_X2Y0")).unwrap();
        assert!(grid.bits(c).is_empty() && grid.sites(c).is_empty());
        assert_eq!(grid.dims(), Some((0, 2, 0, 1)));
        assert_eq!(grid.iter_bits().count(), 3);
        assert_eq!(c.tile_type_index(), None);
    }

    #[test]
    fn errors_have_positions() {
        let cases = [
            (
                r#"{"A": {"type": "A", "grid_x": 0, "grid_y": 0}, "B": {"type": "B", "grid_x": 0, "grid_y": 0}}"#,
                "both at grid location",
            ),
            (
                r#"{"A": {"type": "A", "grid_x": 0, "grid_y": 0}, "A": {"type": "B", "grid_x": 1, "grid_y": 0}}"#,
                "listed twice",
            ),
            (r#"{"A": {"type": "A", "grid_x": 0}}"#, "grid_y"),
            (
                r#"{"A": {"type": "A", "grid_x": 0, "grid_y": 0, "bits": {"FOO": {"baseaddr": "0", "frames": 1, "offset": 0, "words": 1}}}}"#,
                "unknown bus",
            ),
            (
                r#"{"A": {"type": "A", "grid_x": 0, "grid_y": 0, "bits": {"CLB_IO_CLK": {"baseaddr": "zz", "frames": 1, "offset": 0, "words": 1}}}}"#,
                "not an unsigned",
            ),
            (
                r#"{"A": {"type": "A", "grid_x": 0, "grid_y": 0, "clock_region": "Y1X1"}}"#,
                "clock_region",
            ),
            (r#"{"A": {"type": "A", "grid_x": 0, "grid_y": 0}"#, "EOF"),
            (r#"[]"#, "object"),
        ];
        for (text, message) in cases {
            let err = Grid::from_json_slice(text.as_bytes()).unwrap_err();
            assert!(err.to_string().contains(message), "{text}: {err}");
            assert!(err.line() >= 1, "{err}");
        }
        let err = Grid::from_file(Path::new("/nonexistent/tilegrid.json")).unwrap_err();
        assert!(matches!(err, DbError::MissingFile { .. }), "{err}");
    }

    #[test]
    fn fuzz_does_not_panic() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let bytes = GRID.as_bytes();
        for _ in 0..3000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let mut data = bytes.to_vec();
            let i = (state % data.len() as u64) as usize;
            data[i] = (state >> 32) as u8;
            let _ = Grid::from_json_slice(&data);
            data.truncate(i);
            let _ = Grid::from_json_slice(&data);
        }
    }
}

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

//! `segbits_<type>.db`, `segbits_<type>.block_ram.db` and `ppips_<type>.db`
//! files: the per tile type feature -> bits tables (`prjxray/tile_segbits.py`,
//! design document §3.2, §3.3).

use std::fmt;
use std::path::Path;

use fasm::idstring::IdString;
use foldhash::HashMap;

use crate::arch::BlockType;
use crate::error::{read_text, DbError};

/// One configuration bit of a segbits entry (`[!]<word_column>_<word_bit>`).
///
/// `word_column` is added to the tile's `baseaddr` to get the frame;
/// `word_bit` is a raw bit offset from the start of the tile's words in the
/// frame (**not** limited to 0..31: `.block_ram.db` files use values up
/// to a few thousand). See [`crate::Architecture::segbit_position`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SegBit {
    /// Frame offset from the tile's base address.
    pub word_column: u32,
    /// Bit offset from the tile's first word in the frame.
    pub word_bit: u32,
    /// `false` for a `!` bit: enabling the feature *clears* it.
    pub is_set: bool,
}

impl SegBit {
    /// Parses one bit token (`29_14`, `!30_00`), like `parsebit` of
    /// `prjxray/tile_segbits.py` but with ASCII digits only.
    pub fn parse(token: &str) -> Option<Self> {
        let (is_set, rest) = match token.strip_prefix('!') {
            Some(rest) => (false, rest),
            None => (true, token),
        };
        let (column, bit) = rest.split_once('_')?;
        Some(SegBit {
            word_column: parse_u32(column)?,
            word_bit: parse_u32(bit)?,
            is_set,
        })
    }
}

impl fmt::Display for SegBit {
    /// The database form, `[!]CC_BB` (two digits minimum).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_set {
            f.write_str("!")?;
        }
        write!(f, "{:02}_{:02}", self.word_column, self.word_bit)
    }
}

/// Parses a non-empty run of ASCII digits.
pub(crate) fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// The type of a pseudo PIP (`ppips_<type>.db`).
///
/// The assembler treats all three the same: the feature is valid and sets
/// no bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PpipType {
    /// `always`: hard wired, no configuration bit.
    Always,
    /// `default`: what a routing mux selects when nothing else is set.
    Default,
    /// `hint`: informational.
    Hint,
}

impl PpipType {
    /// The database name.
    pub const fn name(self) -> &'static str {
        match self {
            PpipType::Always => "always",
            PpipType::Default => "default",
            PpipType::Hint => "hint",
        }
    }

    /// Parses a database name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "always" => Some(PpipType::Always),
            "default" => Some(PpipType::Default),
            "hint" => Some(PpipType::Hint),
            _ => None,
        }
    }
}

/// One feature of a tile type: its bus and its bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegbitsEntry {
    /// The feature name within the tile, i.e. the segbits tag without the
    /// leading `<TILE_TYPE>.` (e.g. `SLICEL_X0.ALUT.INIT[00]`).
    pub feature: IdString,
    /// The bus: [`BlockType::ClbIoClk`] for `segbits_<type>.db`,
    /// [`BlockType::BlockRam`] for `segbits_<type>.block_ram.db`.
    pub block_type: BlockType,
    pub(crate) start: u32,
    pub(crate) len: u32,
}

/// The result of [`TileSegbits::feature_to_bits`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegbitsMatch<'a> {
    /// The feature is a pseudo PIP: valid, no bits.
    PseudoPip(PpipType),
    /// The feature's segbits entry.
    Entry(&'a SegbitsEntry),
}

/// The segbits and pseudo PIPs of one tile type
/// (`prjxray.tile_segbits.TileSegbits`).
///
/// Features are keyed by the [`IdString`] of the name *within the tile*
/// (the segbits tag without its `<TILE_TYPE>.` prefix), so a FASM feature
/// `<tile>.<feature>` is looked up with the handle of `<feature>`.
///
/// Storage is flat (all bits of all entries in one `Vec`, entries refer to
/// ranges of it) so the whole table can later be written to the binary
/// cache as a few arrays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TileSegbits {
    pub(crate) entries: Vec<SegbitsEntry>,
    pub(crate) bits: Vec<SegBit>,
    /// Exact name -> entry; the `CLB_IO_CLK` entry wins over a
    /// `BLOCK_RAM` entry of the same name (dict order in prjxray).
    pub(crate) by_name: HashMap<IdString, u32>,
    /// (`NAME` of `NAME[N]`, N) -> entry (prjxray `feature_addresses`;
    /// `BLOCK_RAM` wins over `CLB_IO_CLK`, it is inserted later).
    pub(crate) addressed: HashMap<(IdString, u32), u32>,
    pub(crate) ppips: Vec<(IdString, PpipType)>,
    pub(crate) ppip_index: HashMap<IdString, PpipType>,
    pub(crate) foreign_lines: usize,
}

impl TileSegbits {
    /// All entries, `CLB_IO_CLK` ones first, each group in file order (a
    /// tag repeated in one file keeps its first position and its last
    /// bits, like a Python dict).
    pub fn entries(&self) -> &[SegbitsEntry] {
        &self.entries
    }

    /// The bits of an entry of this table.
    ///
    /// # Panics
    ///
    /// Panics if `entry` belongs to another table.
    pub fn bits(&self, entry: &SegbitsEntry) -> &[SegBit] {
        &self.bits[entry.start as usize..(entry.start + entry.len) as usize]
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if the tile type has no segbits.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total number of bits of all entries.
    pub fn bit_count(&self) -> usize {
        self.entries.iter().map(|e| e.len as usize).sum()
    }

    /// The entry named exactly `feature` (first `CLB_IO_CLK`, then
    /// `BLOCK_RAM`).
    pub fn get(&self, feature: IdString) -> Option<&SegbitsEntry> {
        self.by_name
            .get(&feature)
            .map(|&i| &self.entries[i as usize])
    }

    /// The entry of bit `address` of the multi bit feature `base` (the
    /// entry named `base[address]`, whatever the number of digits).
    pub fn get_addressed(&self, base: IdString, address: u32) -> Option<&SegbitsEntry> {
        self.addressed
            .get(&(base, address))
            .map(|&i| &self.entries[i as usize])
    }

    /// The pseudo PIP type of `feature`, if it is one.
    pub fn ppip(&self, feature: IdString) -> Option<PpipType> {
        self.ppip_index.get(&feature).copied()
    }

    /// All pseudo PIPs, in file order.
    pub fn ppips(&self) -> &[(IdString, PpipType)] {
        &self.ppips
    }

    /// Number of lines whose tag did not start with this tile type's
    /// name; they can never be looked up (prjxray builds the lookup key as
    /// `<tile_type>.<feature>`) and are dropped.
    pub fn foreign_lines(&self) -> usize {
        self.foreign_lines
    }

    /// `TileSegbits.feature_to_bits` of prjxray (`tile_segbits.py:169-184`):
    ///
    /// 1. a pseudo PIP matches first (any `address`);
    /// 2. with `address == 0`, the entry named exactly `feature`;
    /// 3. otherwise (or if 2 found nothing) the entry `feature[address]`.
    ///
    /// Returns `None` where prjxray raises `KeyError`.
    pub fn feature_to_bits(&self, feature: IdString, address: u32) -> Option<SegbitsMatch<'_>> {
        if let Some(ppip) = self.ppip(feature) {
            return Some(SegbitsMatch::PseudoPip(ppip));
        }
        if address == 0 {
            if let Some(entry) = self.get(feature) {
                return Some(SegbitsMatch::Entry(entry));
            }
        }
        self.get_addressed(feature, address)
            .map(SegbitsMatch::Entry)
    }
}

/// Builds a [`TileSegbits`] from the database files of one tile type.
pub(crate) struct TileSegbitsBuilder {
    segbits: TileSegbits,
    /// `<TILE_TYPE>.`
    prefix: String,
}

impl TileSegbitsBuilder {
    pub(crate) fn new(tile_type: &str) -> Self {
        TileSegbitsBuilder {
            segbits: TileSegbits::default(),
            prefix: format!("{tile_type}."),
        }
    }

    /// Reads a `segbits_*.db` file for bus `block_type`.
    ///
    /// The format is `read_segbits` of prjxray: one `TAG BIT...` line per
    /// feature, blank lines ignored. Differences: fields may be separated
    /// by any run of ASCII whitespace (prjxray crashes on two spaces) and
    /// numbers must be plain ASCII digits.
    pub(crate) fn read_segbits(
        &mut self,
        path: &Path,
        block_type: BlockType,
    ) -> Result<(), DbError> {
        let text = read_text(path)?;
        self.add_segbits_text(path, &text, block_type)
    }

    pub(crate) fn add_segbits_text(
        &mut self,
        path: &Path,
        text: &str,
        block_type: BlockType,
    ) -> Result<(), DbError> {
        let err = |line: usize, message: String| DbError::Segbits {
            path: path.to_path_buf(),
            line,
            message,
        };
        // Tag -> entry of this file, for repeated tags.
        let mut in_file: HashMap<IdString, u32> = HashMap::default();
        for (index, line) in text.lines().enumerate() {
            let lineno = index + 1;
            let mut tokens = line.split_ascii_whitespace();
            let Some(tag) = tokens.next() else {
                continue;
            };
            let start = self.segbits.bits.len();
            for token in tokens {
                let bit = SegBit::parse(token).ok_or_else(|| {
                    err(
                        lineno,
                        format!("malformed bit {token:?} (expected [!]<column>_<bit>)"),
                    )
                })?;
                self.segbits.bits.push(bit);
            }
            let len = self.segbits.bits.len() - start;
            if len == 0 {
                return Err(err(lineno, format!("feature {tag:?} has no bits")));
            }
            // Validate a `[N]` suffix now so that the addressed index can
            // be built without errors.
            split_address(tag).map_err(|m| err(lineno, m))?;
            let Some(feature) = tag.strip_prefix(self.prefix.as_str()) else {
                self.segbits.bits.truncate(start);
                self.segbits.foreign_lines += 1;
                continue;
            };
            let (start, len) =
                to_u32_range(start, len).ok_or_else(|| err(lineno, "too many bits".to_owned()))?;
            let feature = IdString::new(feature);
            let entry = SegbitsEntry {
                feature,
                block_type,
                start,
                len,
            };
            match in_file.get(&feature) {
                // Python dict assignment: the key keeps its position, the
                // value is replaced (the old bits stay unused in `bits`).
                Some(&i) => self.segbits.entries[i as usize] = entry,
                None => {
                    let i = u32::try_from(self.segbits.entries.len())
                        .map_err(|_| err(lineno, "too many features".to_owned()))?;
                    in_file.insert(feature, i);
                    self.segbits.entries.push(entry);
                }
            }
        }
        Ok(())
    }

    /// Reads a `ppips_*.db` file (`read_ppips`: `FEATURE TYPE` lines).
    pub(crate) fn read_ppips(&mut self, path: &Path) -> Result<(), DbError> {
        let text = read_text(path)?;
        self.add_ppips_text(path, &text)
    }

    pub(crate) fn add_ppips_text(&mut self, path: &Path, text: &str) -> Result<(), DbError> {
        for (index, line) in text.lines().enumerate() {
            let err = |message: String| DbError::Segbits {
                path: path.to_path_buf(),
                line: index + 1,
                message,
            };
            let tokens: Vec<&str> = line.split_ascii_whitespace().collect();
            let (tag, kind) = match tokens.as_slice() {
                [] => continue,
                [tag, kind] => (*tag, *kind),
                _ => {
                    return Err(err(format!(
                        "expected `FEATURE always|default|hint`, got {line:?}"
                    )))
                }
            };
            let kind = PpipType::from_name(kind)
                .ok_or_else(|| err(format!("unknown pseudo PIP type {kind:?}")))?;
            let Some(feature) = tag.strip_prefix(self.prefix.as_str()) else {
                self.segbits.foreign_lines += 1;
                continue;
            };
            let feature = IdString::new(feature);
            if self.segbits.ppip_index.insert(feature, kind).is_some() {
                // Later line wins, at the original position (dict).
                if let Some(slot) = self.segbits.ppips.iter_mut().find(|(f, _)| *f == feature) {
                    slot.1 = kind;
                }
            } else {
                self.segbits.ppips.push((feature, kind));
            }
        }
        Ok(())
    }

    /// Builds the lookup indexes.
    pub(crate) fn finish(mut self) -> TileSegbits {
        let segbits = &mut self.segbits;
        // Entries are grouped by block type in load order (CLB_IO_CLK
        // first), which is prjxray's `self.segbits` dict order.
        for (i, entry) in segbits.entries.iter().enumerate() {
            let i = i as u32;
            segbits.by_name.entry(entry.feature).or_insert(i);
            let found = entry.feature.with_str(|name| {
                split_address(name)
                    .ok()
                    .flatten()
                    .map(|(base, address)| (IdString::new(base), address))
            });
            if let Some(key) = found {
                segbits.addressed.insert(key, i);
            }
        }
        segbits.bits.shrink_to_fit();
        segbits.entries.shrink_to_fit();
        self.segbits
    }
}

fn to_u32_range(start: usize, len: usize) -> Option<(u32, u32)> {
    Some((u32::try_from(start).ok()?, u32::try_from(len).ok()?))
}

/// Splits `NAME[N]` like `TileSegbits.__init__`: the last `[` starts the
/// address, which ends at the last `]`. Returns `Ok(None)` for a name
/// without `[`.
pub(crate) fn split_address(tag: &str) -> Result<Option<(&str, u32)>, String> {
    let Some(open) = tag.rfind('[') else {
        return Ok(None);
    };
    let close = tag
        .rfind(']')
        .filter(|&close| close > open)
        .ok_or_else(|| format!("feature {tag:?} has `[` without a following `]`"))?;
    let address = parse_u32(&tag[open + 1..close])
        .ok_or_else(|| format!("feature {tag:?} has a malformed address"))?;
    Ok(Some((&tag[..open], address)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(tile_type: &str, clb: &str, bram: &str, ppips: &str) -> Result<TileSegbits, DbError> {
        let mut builder = TileSegbitsBuilder::new(tile_type);
        let path = Path::new("test.db");
        builder.add_segbits_text(path, clb, BlockType::ClbIoClk)?;
        builder.add_segbits_text(path, bram, BlockType::BlockRam)?;
        builder.add_ppips_text(path, ppips)?;
        Ok(builder.finish())
    }

    #[test]
    fn parse_bits() {
        assert_eq!(
            SegBit::parse("!012_23"),
            Some(SegBit {
                word_column: 12,
                word_bit: 23,
                is_set: false
            })
        );
        assert_eq!(SegBit::parse("00_2204").unwrap().word_bit, 2204);
        for bad in [
            "",
            "!",
            "12",
            "12_",
            "_3",
            "1_2_3",
            "a_1",
            "1_-2",
            "+1_2",
            "!!1_2",
            "99999999999_1",
        ] {
            assert_eq!(SegBit::parse(bad), None, "{bad:?}");
        }
        assert_eq!(SegBit::parse("!3_7").unwrap().to_string(), "!03_07");
    }

    #[test]
    fn lookup_order() {
        let t = build(
            "BRAM_L",
            "BRAM_L.A 01_02\n\nBRAM_L.BOTH 00_01\nBRAM_L.M[0] 02_03\nBRAM_L.M[01] !02_04 02_05\nBRAM_L.PP 00_00\n",
            "BRAM_L.BOTH 00_80\nBRAM_L.M[01] 00_1000\nBRAM_L.INIT[000] 00_00\n",
            "BRAM_L.PP hint\nBRAM_L.P2 always\n",
        )
        .unwrap();
        let id = IdString::new;
        let bits = |m: Option<SegbitsMatch<'_>>| match m {
            Some(SegbitsMatch::Entry(e)) => Some((e.block_type, t.bits(e).to_vec())),
            _ => None,
        };
        let b = |word_column, word_bit, is_set| SegBit {
            word_column,
            word_bit,
            is_set,
        };
        // Exact name: CLB_IO_CLK first.
        assert_eq!(
            bits(t.feature_to_bits(id("BOTH"), 0)),
            Some((BlockType::ClbIoClk, vec![b(0, 1, true)]))
        );
        // Addressed: BLOCK_RAM inserted later, wins.
        assert_eq!(
            bits(t.feature_to_bits(id("M"), 1)),
            Some((BlockType::BlockRam, vec![b(0, 1000, true)]))
        );
        // Address 0 falls through to M[0].
        assert_eq!(
            bits(t.feature_to_bits(id("M"), 0)),
            Some((BlockType::ClbIoClk, vec![b(2, 3, true)]))
        );
        assert_eq!(
            bits(t.feature_to_bits(id("INIT"), 0)),
            Some((BlockType::BlockRam, vec![b(0, 0, true)]))
        );
        // A non zero address never matches an exact name.
        assert_eq!(t.feature_to_bits(id("A"), 1), None);
        assert_eq!(t.feature_to_bits(id("M"), 2), None);
        assert_eq!(t.feature_to_bits(id("NOPE"), 0), None);
        // Pseudo PIPs first, even when a segbits entry exists.
        assert_eq!(
            t.feature_to_bits(id("PP"), 0),
            Some(SegbitsMatch::PseudoPip(PpipType::Hint))
        );
        assert_eq!(
            t.feature_to_bits(id("P2"), 7),
            Some(SegbitsMatch::PseudoPip(PpipType::Always))
        );
        assert_eq!(t.len(), 8);
        assert_eq!(t.bit_count(), 9);
        assert_eq!(t.ppips().len(), 2);
    }

    #[test]
    fn duplicates_and_foreign_lines() {
        let t = build(
            "INT_L",
            "INT_L.X 00_01\nINT_R.Y 00_02\nINT_L.Z 00_03\nINT_L.X 00_04 00_05\nINT_L 00_06\n",
            "",
            "INT_L.P default\nINT_L.P always\nOTHER.P hint\n",
        )
        .unwrap();
        let x = t.get(IdString::new("X")).unwrap();
        assert_eq!(t.bits(x).len(), 2);
        assert_eq!(t.entries()[0].feature, "X");
        assert_eq!(t.entries()[1].feature, "Z");
        assert_eq!(t.len(), 2);
        assert_eq!(t.foreign_lines(), 3);
        assert_eq!(t.ppip(IdString::new("P")), Some(PpipType::Always));
        assert_eq!(t.ppips(), &[(IdString::new("P"), PpipType::Always)]);
    }

    #[test]
    fn malformed_lines_are_errors() {
        let cases = [
            ("T.A\n", "has no bits"),
            ("T.A 1_2\nT.B 1-2\n", "malformed bit"),
            ("T.A[ 1_2\n", "without a following"),
            ("T.A]x[3 1_2\n", "without a following"),
            ("T.A[x] 1_2\n", "malformed address"),
            ("T.A[] 1_2\n", "malformed address"),
            ("T.A !_1\n", "malformed bit"),
        ];
        for (text, message) in cases {
            let err = build("T", text, "", "").unwrap_err();
            let DbError::Segbits {
                line, message: m, ..
            } = &err
            else {
                panic!("{err:?}");
            };
            assert!(m.contains(message), "{text:?}: {m}");
            assert_eq!(*line, text.lines().count(), "{text:?}");
            assert!(err.to_string().starts_with("test.db:"), "{err}");
        }
        for text in ["T.A always extra\n", "T.A sometimes\n", "T.A\n"] {
            assert!(build("T", "", "", text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn whitespace_is_lenient() {
        let t = build(
            "T",
            "  T.A   01_02\t!03_04 \r\n\n   \n",
            "",
            " T.P  hint \r\n",
        )
        .unwrap();
        assert_eq!(t.bits(t.get(IdString::new("A")).unwrap()).len(), 2);
        assert_eq!(t.ppip(IdString::new("P")), Some(PpipType::Hint));
    }

    #[test]
    fn split_addresses() {
        assert_eq!(split_address("A.B"), Ok(None));
        assert_eq!(split_address("A.INIT[07]"), Ok(Some(("A.INIT", 7))));
        assert_eq!(split_address("A[1].B[2]"), Ok(Some(("A[1].B", 2))));
        assert_eq!(split_address("A[1]x"), Ok(Some(("A", 1))));
    }

    /// Arbitrary bytes never panic the parsers.
    #[test]
    fn fuzz_lines() {
        let mut state = 0x1234_5678_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let alphabet = b"T.A[]!_0123456789 \t\nxyz:-";
        for _ in 0..5000 {
            let len = (next() % 40) as usize;
            let text: String = (0..len)
                .map(|_| alphabet[(next() % alphabet.len() as u64) as usize] as char)
                .collect();
            let _ = build("T", &text, &text, &text);
        }
    }
}

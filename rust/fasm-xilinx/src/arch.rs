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

//! Architectures, configuration bus block types, frame addresses and the
//! segbit -> (frame, word, bit) arithmetic (`DESIGN-xilinx-db.md` §4).

use std::fmt;

use crate::segbits::SegBit;

/// A Xilinx configuration architecture, as named by the `--architecture`
/// flag of prjxray's `xc7frames2bit` / `bitread`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Architecture {
    /// 7 series (prjxray-db: artix7, kintex7, spartan7, zynq7).
    Series7,
    /// UltraScale (prjuray-tools `xcuseries`; no database exists yet).
    UltraScale,
    /// UltraScale+ (prjuray-db `zynqusp`, prjuray-tools `xcupseries`).
    UltraScalePlus,
}

/// Bit positions (top, bottom; both inclusive) of the frame address
/// fields of one architecture.
#[derive(Clone, Copy, Debug)]
struct Layout {
    block_type: (u32, u32),
    /// The top/bottom half bit.
    half: u32,
    /// The row number without the half bit.
    row: (u32, u32),
    column: (u32, u32),
    minor: (u32, u32),
}

const SERIES7_LAYOUT: Layout = Layout {
    block_type: (25, 23),
    half: 22,
    row: (21, 17),
    column: (16, 7),
    minor: (6, 0),
};

/// `prjuray-tools/lib/include/prjxray/xilinx/xcupseries/frame_address.h`:
/// everything one bit higher than Series7 and an 8 bit minor.
const ULTRASCALE_PLUS_LAYOUT: Layout = Layout {
    block_type: (26, 24),
    half: 23,
    row: (22, 18),
    column: (17, 8),
    minor: (7, 0),
};

const fn mask(top: u32, bottom: u32) -> u32 {
    let width = top - bottom + 1;
    let ones = if width >= 32 {
        u32::MAX
    } else {
        (1u32 << width) - 1
    };
    ones << bottom
}

const fn field_get(value: u32, (top, bottom): (u32, u32)) -> u32 {
    (value & mask(top, bottom)) >> bottom
}

/// `bit_field_set` of prjxray `bit_ops.h`: the value is masked to the
/// field width.
const fn field_set(reg: u32, (top, bottom): (u32, u32), value: u32) -> u32 {
    (reg & !mask(top, bottom)) | ((value << bottom) & mask(top, bottom))
}

const fn field_max(field: (u32, u32)) -> u32 {
    mask(field.0, field.1) >> field.1
}

impl Architecture {
    /// All architectures.
    pub const ALL: [Architecture; 3] = [
        Architecture::Series7,
        Architecture::UltraScale,
        Architecture::UltraScalePlus,
    ];

    /// The name used by the `--architecture` flag of the prjxray tools
    /// (`Series7`, `UltraScale`, `UltraScalePlus`).
    pub const fn name(self) -> &'static str {
        match self {
            Architecture::Series7 => "Series7",
            Architecture::UltraScale => "UltraScale",
            Architecture::UltraScalePlus => "UltraScalePlus",
        }
    }

    /// Parses an `--architecture` flag value.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|arch| arch.name() == name)
    }

    /// The namespace of the C++ types in the YAML tags of `part.yaml`
    /// (`!<xilinx/xc7series/part>`, `xcuseries`, `xcupseries`).
    pub const fn yaml_namespace(self) -> &'static str {
        match self {
            Architecture::Series7 => "xc7series",
            Architecture::UltraScale => "xcuseries",
            Architecture::UltraScalePlus => "xcupseries",
        }
    }

    /// The architecture whose C++ namespace is `namespace` (see
    /// [`Architecture::yaml_namespace`]).
    pub fn from_yaml_namespace(namespace: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|arch| arch.yaml_namespace() == namespace)
    }

    /// Number of 32-bit words in one configuration frame (101, 123, 93).
    pub const fn words_per_frame(self) -> usize {
        match self {
            Architecture::Series7 => 101,
            Architecture::UltraScale => 123,
            Architecture::UltraScalePlus => 93,
        }
    }

    /// The unit, in bits, of the tilegrid `offset` / `words` fields and
    /// of the `word_bit` of segbits: 32 for Series7 (prjxray
    /// `bitstream.WORD_SIZE_BITS`), 16 for UltraScale and UltraScale+
    /// (prjuray `bitstream.WORD_SIZE_BITS`, frames modelled as 16-bit
    /// half words; see §4.2 of the design document).
    pub const fn segbit_word_bits(self) -> u32 {
        match self {
            Architecture::Series7 => 32,
            Architecture::UltraScale | Architecture::UltraScalePlus => 16,
        }
    }

    /// `true` if the part frame tree is split into top and bottom global
    /// clock regions (Series7 `part.yaml`); UltraScale and UltraScale+
    /// have a flat list of rows whose row number includes the half bit.
    pub const fn has_global_clock_regions(self) -> bool {
        matches!(self, Architecture::Series7)
    }

    /// The configuration bits the bitstream writer overwrites with the
    /// frame ECC, as `(32-bit word index, bit mask)` pairs:
    ///
    /// * Series7: the low 13 bits of word 50 (`xc7series/ecc.cc`,
    ///   `kECCFrameNumber = 0x32`, `data[50] & 0xFFFFE000`);
    /// * UltraScale: word 60 and the low 16 bits of word 61
    ///   (`prjuray-tools/lib/xilinx/xcuseries/ecc.cc`,
    ///   `kECCFrameNumber = 60`);
    /// * UltraScale+: word 45 and the low 16 bits of word 46
    ///   (`xcupseries/ecc.cc`, `kECCFrameNumber = 45`).
    pub const fn ecc_reserved_bits(self) -> &'static [(usize, u32)] {
        match self {
            Architecture::Series7 => &[(50, 0x1FFF)],
            Architecture::UltraScale => &[(60, u32::MAX), (61, 0xFFFF)],
            Architecture::UltraScalePlus => &[(45, u32::MAX), (46, 0xFFFF)],
        }
    }

    /// Returns `true` if `position` is one of the ECC bits of
    /// [`Architecture::ecc_reserved_bits`].
    pub fn is_ecc_bit(self, position: BitPosition) -> bool {
        self.ecc_reserved_bits()
            .iter()
            .any(|&(word, mask)| position.word as usize == word && mask & (1 << position.bit) != 0)
    }

    const fn layout(self) -> Layout {
        match self {
            Architecture::Series7 | Architecture::UltraScale => SERIES7_LAYOUT,
            Architecture::UltraScalePlus => ULTRASCALE_PLUS_LAYOUT,
        }
    }

    /// Largest row number (without the half bit) a frame address holds.
    pub const fn max_row(self) -> u8 {
        field_max(self.layout().row) as u8
    }

    /// Largest column number a frame address holds.
    pub const fn max_column(self) -> u16 {
        field_max(self.layout().column) as u16
    }

    /// Largest minor (frame within a column) a frame address holds: 127
    /// (Series7, UltraScale) or 255 (UltraScale+).
    pub const fn max_minor(self) -> u16 {
        field_max(self.layout().minor) as u16
    }

    /// Maps one segbit of a tile to its position in the frames, exactly
    /// like `TileSegbits.map_bit_to_frame` + `FasmAssembler.enable_feature`
    /// (`prjxray/tile_segbits.py:161-167`, `prjxray/fasm_assembler.py:128-133`):
    ///
    /// ```text
    /// frame        = base_address + segbit.word_column
    /// absolute_bit = offset * segbit_word_bits + segbit.word_bit
    /// word         = absolute_bit / 32
    /// bit          = absolute_bit % 32
    /// ```
    ///
    /// `base_address` and `offset` come from the tile's `bits` block for
    /// the segbit's bus; `offset` is signed because an alias tile uses
    /// `offset - alias.start_offset`, which can be negative (see
    /// [`crate::Grid::effective_offset`]). The word and bit are always
    /// returned in 32-bit words; for UltraScale/UltraScale+ the 16-bit
    /// unit of the database is converted here (prjuray's `fasm2frames.py`
    /// `output_bits` does the same conversion).
    ///
    /// **Negative bits wrap like Python list indexes.** The reference
    /// computes `word = absolute_bit // 32` (floor) and stores into
    /// `frame[word]`, so a negative word `-n` (`n <= words_per_frame`)
    /// writes word `words_per_frame - n`. This happens for real: the
    /// `LIOB33_SING` alias tiles have `offset 0, start_offset 2`, and the
    /// f4pga-xc-fasm `liob_stepdown` golden output has
    /// `bit_00400000_099_03` from `LIOB33.IOB_Y0.SOMETHING.STEPDOWN 00_03`
    /// on `LIOB33_SING_X0Y0` (`-2 * 32 + 3 = -61` -> word -2 -> word 99,
    /// bit 3). This function reproduces that
    /// ([`Architecture::segbit_absolute_bit`] tells whether a bit wraps);
    /// only bits before `-words_per_frame` words (a Python `IndexError`)
    /// are errors.
    ///
    /// # Errors
    ///
    /// * [`BitPositionError::FrameOverflow`] if the frame address does not
    ///   fit in 32 bits;
    /// * [`BitPositionError::NegativeBit`] if the absolute bit is before
    ///   `-words_per_frame` words;
    /// * [`BitPositionError::WordOutOfFrame`] if the word is not below
    ///   [`Architecture::words_per_frame`] (prjxray prints a warning and
    ///   drops such writes; it does not happen with the real databases).
    pub fn segbit_position(
        self,
        base_address: u32,
        offset: i64,
        segbit: SegBit,
    ) -> Result<BitPosition, BitPositionError> {
        let frame = base_address
            .checked_add(segbit.word_column)
            .ok_or(BitPositionError::FrameOverflow)?;
        let mut absolute_bit = self.segbit_absolute_bit(offset, segbit);
        let frame_bits = self.words_per_frame() as i64 * 32;
        if absolute_bit < 0 {
            if absolute_bit < -frame_bits {
                return Err(BitPositionError::NegativeBit { absolute_bit });
            }
            absolute_bit += frame_bits;
        }
        let word = absolute_bit / 32;
        if word >= self.words_per_frame() as i64 {
            return Err(BitPositionError::WordOutOfFrame {
                frame: FrameAddress(frame),
                word: word as u64,
            });
        }
        Ok(BitPosition {
            frame: FrameAddress(frame),
            word: word as u32,
            bit: (absolute_bit % 32) as u32,
        })
    }

    /// The segbit's bit offset from the start of the frame
    /// (`offset * segbit_word_bits + word_bit`), before the negative
    /// wrap-around of [`Architecture::segbit_position`]; negative for the
    /// bits that wrap.
    pub fn segbit_absolute_bit(self, offset: i64, segbit: SegBit) -> i64 {
        offset
            .saturating_mul(i64::from(self.segbit_word_bits()))
            .saturating_add(i64::from(segbit.word_bit))
    }
}

impl fmt::Display for Architecture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A configuration bus (the `block_type` field of a frame address and the
/// key of a tilegrid `bits` block).
///
/// The tilegrid loader accepts all three names of the C++ `BlockType`
/// enum (`lib/include/prjxray/xilinx/xc7series/block_type.h`); the
/// Python `grid_types.BlockType` only knows the first two (design
/// document §9 item 6). `CFG_CLB` never occurs in a real tilegrid and no
/// segbits file exists for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum BlockType {
    /// `CLB_IO_CLK`: CLBs, interconnect, clocks and IOs
    /// (`segbits_<type>.db`).
    ClbIoClk = 0,
    /// `BLOCK_RAM`: block RAM contents (`segbits_<type>.block_ram.db`).
    BlockRam = 1,
    /// `CFG_CLB`.
    CfgClb = 2,
}

impl BlockType {
    /// All block types, in frame address order.
    pub const ALL: [BlockType; 3] = [BlockType::ClbIoClk, BlockType::BlockRam, BlockType::CfgClb];

    /// The database name (`CLB_IO_CLK`, `BLOCK_RAM`, `CFG_CLB`).
    pub const fn name(self) -> &'static str {
        match self {
            BlockType::ClbIoClk => "CLB_IO_CLK",
            BlockType::BlockRam => "BLOCK_RAM",
            BlockType::CfgClb => "CFG_CLB",
        }
    }

    /// Parses a database name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|bt| bt.name() == name)
    }

    /// The value of the frame address `block_type` field.
    pub const fn raw(self) -> u8 {
        self as u8
    }

    /// The block type of a frame address `block_type` field value (3 to 7
    /// are reserved).
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(BlockType::ClbIoClk),
            1 => Some(BlockType::BlockRam),
            2 => Some(BlockType::CfgClb),
            _ => None,
        }
    }
}

impl fmt::Display for BlockType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A 32-bit configuration frame address (the value written to the FAR
/// register and printed in `.frm` files).
///
/// Its bit fields depend on the [`Architecture`]:
///
/// | field | Series7 / UltraScale | UltraScale+ |
/// |---|---|---|
/// | block type | 25:23 | 26:24 |
/// | bottom half | 22 | 23 |
/// | row | 21:17 | 22:18 |
/// | column | 16:7 | 17:8 |
/// | minor | 6:0 | 7:0 |
///
/// `Ord` is the numeric order of the raw value (the order of frames in a
/// bitstream).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FrameAddress(pub u32);

/// The decoded fields of a [`FrameAddress`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameAddressFields {
    /// Raw block type field (see [`BlockType::from_raw`]).
    pub block_type: u8,
    /// Bottom half of the device (`is_bottom_half_rows`).
    pub bottom: bool,
    /// Row within the half.
    pub row: u8,
    /// Configuration column.
    pub column: u16,
    /// Frame within the column.
    pub minor: u16,
}

impl FrameAddress {
    /// Builds a frame address from its fields, or `None` if a field does
    /// not fit its bit range.
    pub fn compose(arch: Architecture, fields: FrameAddressFields) -> Option<Self> {
        let layout = arch.layout();
        if u32::from(fields.block_type) > field_max(layout.block_type)
            || u32::from(fields.row) > field_max(layout.row)
            || u32::from(fields.column) > field_max(layout.column)
            || u32::from(fields.minor) > field_max(layout.minor)
        {
            return None;
        }
        Some(Self::compose_masked(
            arch,
            u32::from(fields.block_type),
            fields.bottom,
            u32::from(fields.row),
            u32::from(fields.column),
            u32::from(fields.minor),
        ))
    }

    /// The C++ `FrameAddress(block_type, bottom, row, column, minor)`
    /// constructor: every value is masked to its field.
    pub(crate) const fn compose_masked(
        arch: Architecture,
        block_type: u32,
        bottom: bool,
        row: u32,
        column: u32,
        minor: u32,
    ) -> Self {
        let layout = arch.layout();
        let mut raw = field_set(0, layout.block_type, block_type);
        raw = field_set(raw, (layout.half, layout.half), bottom as u32);
        raw = field_set(raw, layout.row, row);
        raw = field_set(raw, layout.column, column);
        raw = field_set(raw, layout.minor, minor);
        FrameAddress(raw)
    }

    /// Builds an address from a C++ style row index (see
    /// [`FrameAddress::row_index`]).
    pub(crate) const fn compose_row_index(
        arch: Architecture,
        block_type: u32,
        bottom: bool,
        row_index: u32,
        column: u32,
        minor: u32,
    ) -> Self {
        if arch.has_global_clock_regions() {
            Self::compose_masked(arch, block_type, bottom, row_index, column, minor)
        } else {
            let layout = arch.layout();
            let row_bits = layout.row.0 - layout.row.1 + 1;
            Self::compose_masked(
                arch,
                block_type,
                (row_index >> row_bits) & 1 != 0,
                row_index,
                column,
                minor,
            )
        }
    }

    /// Decodes all fields.
    pub fn fields(self, arch: Architecture) -> FrameAddressFields {
        FrameAddressFields {
            block_type: self.block_type_raw(arch),
            bottom: self.is_bottom_half(arch),
            row: self.row(arch),
            column: self.column(arch),
            minor: self.minor(arch),
        }
    }

    /// Raw block type field.
    pub fn block_type_raw(self, arch: Architecture) -> u8 {
        field_get(self.0, arch.layout().block_type) as u8
    }

    /// The block type, or `None` for a reserved value.
    pub fn block_type(self, arch: Architecture) -> Option<BlockType> {
        BlockType::from_raw(self.block_type_raw(arch))
    }

    /// `true` for the bottom half of the device.
    pub fn is_bottom_half(self, arch: Architecture) -> bool {
        let half = arch.layout().half;
        field_get(self.0, (half, half)) != 0
    }

    /// Row within the half (without the half bit).
    pub fn row(self, arch: Architecture) -> u8 {
        field_get(self.0, arch.layout().row) as u8
    }

    /// The row as the C++ `FrameAddress::row()` returns it, i.e. the key
    /// of the rows of `part.yaml`: the row within the half for Series7,
    /// and the row *including* the half bit as its top bit for
    /// UltraScale / UltraScale+ (`ROW_HIGH` is the half bit there).
    pub fn row_index(self, arch: Architecture) -> u8 {
        let layout = arch.layout();
        if arch.has_global_clock_regions() {
            field_get(self.0, layout.row) as u8
        } else {
            field_get(self.0, (layout.half, layout.row.1)) as u8
        }
    }

    /// Configuration column.
    pub fn column(self, arch: Architecture) -> u16 {
        field_get(self.0, arch.layout().column) as u16
    }

    /// Frame within the column.
    pub fn minor(self, arch: Architecture) -> u16 {
        field_get(self.0, arch.layout().minor) as u16
    }
}

impl fmt::Display for FrameAddress {
    /// `0x%08X`, the `.frm` format.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:08X}", self.0)
    }
}

impl From<u32> for FrameAddress {
    fn from(raw: u32) -> Self {
        FrameAddress(raw)
    }
}

impl From<FrameAddress> for u32 {
    fn from(addr: FrameAddress) -> Self {
        addr.0
    }
}

/// The position of one configuration bit: frame, 32-bit word within the
/// frame and bit within the word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BitPosition {
    /// Frame address.
    pub frame: FrameAddress,
    /// 32-bit word within the frame (`< words_per_frame`).
    pub word: u32,
    /// Bit within the word (`< 32`).
    pub bit: u32,
}

/// Why [`Architecture::segbit_position`] could not place a segbit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitPositionError {
    /// `base_address + word_column` does not fit in 32 bits.
    FrameOverflow,
    /// The absolute bit (`offset * unit + word_bit`) is negative.
    NegativeBit {
        /// The computed absolute bit.
        absolute_bit: i64,
    },
    /// The word is beyond the end of the frame.
    WordOutOfFrame {
        /// The frame.
        frame: FrameAddress,
        /// The computed word.
        word: u64,
    },
}

impl fmt::Display for BitPositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BitPositionError::FrameOverflow => f.write_str("frame address overflows 32 bits"),
            BitPositionError::NegativeBit { absolute_bit } => {
                write!(f, "negative bit offset {absolute_bit} in the frame")
            }
            BitPositionError::WordOutOfFrame { frame, word } => {
                write!(f, "invalid word address {word} in frame {frame}")
            }
        }
    }
}

impl std::error::Error for BitPositionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series7_fields_match_addr_bits2word() {
        // prjxray/bitstream.py addr_bits2word("BLOCK_RAM", "bottom", 3, 17, 5)
        let expected = (1 << 23) | (1 << 22) | (3 << 17) | (17 << 7) | 5;
        let fields = FrameAddressFields {
            block_type: 1,
            bottom: true,
            row: 3,
            column: 17,
            minor: 5,
        };
        let addr = FrameAddress::compose(Architecture::Series7, fields).unwrap();
        assert_eq!(addr.0, expected);
        assert_eq!(addr.fields(Architecture::Series7), fields);
        assert_eq!(
            addr.block_type(Architecture::Series7),
            Some(BlockType::BlockRam)
        );
        assert_eq!(addr.row_index(Architecture::Series7), 3);
        // The mini db CLBLM_L_X10Y102 base address.
        let clb = FrameAddress(0x0002_0500);
        assert_eq!(
            clb.fields(Architecture::Series7),
            FrameAddressFields {
                block_type: 0,
                bottom: false,
                row: 1,
                column: 10,
                minor: 0
            }
        );
        assert_eq!(clb.to_string(), "0x00020500");
    }

    #[test]
    fn round_trip_boundaries() {
        for arch in Architecture::ALL {
            for block_type in [0u8, 1, 2, 7] {
                for bottom in [false, true] {
                    for row in [0, 1, arch.max_row()] {
                        for column in [0, 1, arch.max_column()] {
                            for minor in [0, 1, arch.max_minor()] {
                                let fields = FrameAddressFields {
                                    block_type,
                                    bottom,
                                    row,
                                    column,
                                    minor,
                                };
                                let addr = FrameAddress::compose(arch, fields).unwrap();
                                assert_eq!(addr.fields(arch), fields, "{arch} {addr}");
                            }
                        }
                    }
                }
            }
            let too_big = FrameAddressFields {
                block_type: 0,
                bottom: false,
                row: 0,
                column: 0,
                minor: arch.max_minor() + 1,
            };
            assert_eq!(FrameAddress::compose(arch, too_big), None);
        }
        assert_eq!(Architecture::Series7.max_minor(), 127);
        assert_eq!(Architecture::UltraScale.max_minor(), 127);
        assert_eq!(Architecture::UltraScalePlus.max_minor(), 255);
    }

    #[test]
    fn ultrascale_plus_layout() {
        let arch = Architecture::UltraScalePlus;
        let addr = FrameAddress::compose(
            arch,
            FrameAddressFields {
                block_type: 1,
                bottom: true,
                row: 2,
                column: 5,
                minor: 200,
            },
        )
        .unwrap();
        assert_eq!(addr.0, (1 << 24) | (1 << 23) | (2 << 18) | (5 << 8) | 200);
        // The C++ row() includes the half bit for UltraScale(+).
        assert_eq!(addr.row_index(arch), 32 + 2);
        assert_eq!(
            FrameAddress::compose_row_index(arch, 1, false, 34, 5, 200),
            addr
        );
        let us = FrameAddress::compose_row_index(Architecture::UltraScale, 0, false, 33, 0, 0);
        assert!(us.is_bottom_half(Architecture::UltraScale));
        assert_eq!(us.row(Architecture::UltraScale), 1);
    }

    #[test]
    fn segbit_positions() {
        let arch = Architecture::Series7;
        let bit = |word_column, word_bit| SegBit {
            word_column,
            word_bit,
            is_set: true,
        };
        // CLBLM_L_X10Y102 (baseaddr 0x00020500, offset 4) A5FF.ZINI 31_06.
        assert_eq!(
            arch.segbit_position(0x0002_0500, 4, bit(31, 6)),
            Ok(BitPosition {
                frame: FrameAddress(0x0002_051F),
                word: 4,
                bit: 6
            })
        );
        // BLOCK_RAM word_bit > 31: 00_80 -> word 2, bit 16.
        assert_eq!(
            arch.segbit_position(0x0080_0000, 0, bit(0, 80)),
            Ok(BitPosition {
                frame: FrameAddress(0x0080_0000),
                word: 2,
                bit: 16
            })
        );
        // HCLK middle word.
        assert_eq!(arch.segbit_position(0, 50, bit(0, 14)).unwrap().word, 50);
        assert!(arch.is_ecc_bit(arch.segbit_position(0, 50, bit(0, 12)).unwrap()));
        assert!(!arch.is_ecc_bit(arch.segbit_position(0, 50, bit(0, 13)).unwrap()));
        // Negative bits wrap like Python list indexes: the f4pga-xc-fasm
        // liob_stepdown golden output has bit_00400000_099_03 from
        // `LIOB33.IOB_Y0.SOMETHING.STEPDOWN 00_03` on LIOB33_SING_X0Y0
        // (offset 0, alias start_offset 2).
        assert_eq!(
            arch.segbit_position(0x0040_0000, -2, bit(0, 3)),
            Ok(BitPosition {
                frame: FrameAddress(0x0040_0000),
                word: 99,
                bit: 3
            })
        );
        assert_eq!(arch.segbit_absolute_bit(-2, bit(0, 3)), -61);
        assert_eq!(arch.segbit_position(0, -101, bit(0, 0)).unwrap().word, 0);
        assert_eq!(
            arch.segbit_position(0, -102, bit(0, 31)),
            Err(BitPositionError::NegativeBit {
                absolute_bit: -3233
            })
        );
        // UltraScale+ wraps at 93 words too (186 16-bit words in prjuray).
        let usp_wrap = Architecture::UltraScalePlus.segbit_position(0, -1, bit(0, 0));
        assert_eq!(usp_wrap.map(|p| (p.word, p.bit)), Ok((92, 16)));
        assert!(matches!(
            arch.segbit_position(0, 100, bit(0, 32)),
            Err(BitPositionError::WordOutOfFrame { word: 101, .. })
        ));
        assert_eq!(
            arch.segbit_position(u32::MAX, 0, bit(1, 0)),
            Err(BitPositionError::FrameOverflow)
        );
        // UltraScale+: 16-bit units; RCLK offset 93 -> 32-bit word 46, upper half.
        let usp = Architecture::UltraScalePlus;
        assert_eq!(
            usp.segbit_position(0, 93, bit(0, 3)).unwrap(),
            BitPosition {
                frame: FrameAddress(0),
                word: 46,
                bit: 19
            }
        );
        assert!(!usp.is_ecc_bit(usp.segbit_position(0, 93, bit(0, 0)).unwrap()));
        assert!(usp.is_ecc_bit(usp.segbit_position(0, 92, bit(0, 15)).unwrap()));
        assert!(usp.is_ecc_bit(usp.segbit_position(0, 90, bit(0, 0)).unwrap()));
    }

    #[test]
    fn names() {
        for arch in Architecture::ALL {
            assert_eq!(Architecture::from_name(arch.name()), Some(arch));
            assert_eq!(
                Architecture::from_yaml_namespace(arch.yaml_namespace()),
                Some(arch)
            );
        }
        for bt in BlockType::ALL {
            assert_eq!(BlockType::from_name(bt.name()), Some(bt));
            assert_eq!(BlockType::from_raw(bt.raw()), Some(bt));
        }
        assert_eq!(BlockType::from_name("clb_io_clk"), None);
        assert_eq!(BlockType::from_raw(3), None);
    }
}

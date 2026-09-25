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

//! `.bit` -> frames: prjxray's (and prjuray-tools') `BitstreamReader` and
//! `Configuration<ArchType>::InitWithPackets` (design document §6.6,
//! §8.10), the library behind `bitread`.

use std::collections::BTreeMap;
use std::fmt;

use super::ecc::Ecc;
use super::packet::{command, register, Packet, PacketIter, OPCODE_WRITE};
use super::writer::{same_row, BitstreamFormat};
use crate::arch::{Architecture, FrameAddress};
use crate::frames::Frames;
use crate::part::Part;

/// The sync word, searched as bytes (`BitstreamReader::kSyncWord`).
pub const SYNC_WORD: [u8; 4] = [0xAA, 0x99, 0x55, 0x66];

/// The configuration words of a bitstream: everything after the first
/// sync word, as big-endian 32-bit words (`BitstreamReader::InitWithBytes`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitstreamReader {
    words: Vec<u32>,
    trailing_bytes: usize,
}

impl BitstreamReader {
    /// Finds the first sync word (`AA 99 55 66`, at any byte offset, so
    /// that any header is skipped) and reads the rest as big-endian words.
    /// Returns `None` if there is no sync word ("Input doesn't look like a
    /// bitstream").
    ///
    /// Trailing bytes that do not make a whole word are ignored (the
    /// reference throws `std::out_of_range` and aborts); see
    /// [`BitstreamReader::trailing_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let sync = bytes.windows(4).position(|w| w == SYNC_WORD)?;
        let data = &bytes[sync + 4..];
        let chunks = data.chunks_exact(4);
        let trailing_bytes = chunks.remainder().len();
        let words = chunks
            .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        Some(BitstreamReader {
            words,
            trailing_bytes,
        })
    }

    /// The configuration words after the sync word.
    pub fn words(&self) -> &[u32] {
        &self.words
    }

    /// The number of bytes after the last whole word (0 to 3).
    pub fn trailing_bytes(&self) -> usize {
        self.trailing_bytes
    }

    /// The configuration packets (see [`PacketIter`]).
    pub fn packets(&self) -> PacketIter<'_> {
        PacketIter::new(&self.words)
    }

    /// Replays the packets on `part` (see [`Configuration::from_packets`]).
    ///
    /// # Errors
    ///
    /// See [`Configuration::from_packets`].
    pub fn configuration(&self, part: &Part) -> Result<Configuration<'_>, ReadError> {
        Configuration::from_packets(part, self.packets())
    }

    /// Replays the packets on `part` in `format` (see
    /// [`Configuration::from_packets_with`]).
    ///
    /// # Errors
    ///
    /// See [`Configuration::from_packets_with`].
    pub fn configuration_with(
        &self,
        part: &Part,
        format: &BitstreamFormat,
    ) -> Result<Configuration<'_>, ReadError> {
        Configuration::from_packets_with(part, format, self.packets())
    }
}

/// Why a bitstream cannot be read for a part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The part is not of the format's part type
    /// ([`BitstreamFormat::addressing`]).
    ArchitectureMismatch {
        /// The part's architecture.
        part: Architecture,
        /// The part type of the format.
        format: Architecture,
    },
    /// An `IDCODE` write does not match the part ("Bitstream does not
    /// appear to be for this part").
    IdcodeMismatch {
        /// The IDCODE of the bitstream.
        found: u32,
        /// The IDCODE of the part.
        expected: u32,
    },
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::ArchitectureMismatch { part, format } => write!(
                f,
                "the part is a {part} part, the bitstream format needs a {format} part"
            ),
            ReadError::IdcodeMismatch { found, expected } => write!(
                f,
                "bitstream IDCODE 0x{found:08X} is not the part's 0x{expected:08X}"
            ),
        }
    }
}

impl std::error::Error for ReadError {}

/// The frames written by a bitstream: frame address -> words (usually
/// the format's words per frame; the last frame of a packet can be
/// shorter).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Configuration<'a> {
    frames: BTreeMap<u32, &'a [u32]>,
    words_per_frame: usize,
    ecc: Ecc,
}

impl<'a> Configuration<'a> {
    /// [`Configuration::from_packets_with`] in the part's
    /// [`BitstreamFormat::native`] format.
    ///
    /// # Errors
    ///
    /// See [`Configuration::from_packets_with`].
    pub fn from_packets(
        part: &Part,
        packets: impl IntoIterator<Item = Packet<'a>>,
    ) -> Result<Self, ReadError> {
        Self::from_packets_with(part, &BitstreamFormat::native(part.architecture), packets)
    }

    /// `Configuration<ArchType>::InitWithPackets` (Series7, UltraScale and
    /// UltraScale+ share it), a literal port of the register machine: only
    /// `Write` packets count; `MASK`, `CTL1` (masked by `MASK`), `CMD`
    /// (`WCFG` starts a new write), `IDCODE` (must match the part), `FAR`
    /// (restarts the write if `CMD` holds `WCFG` and bit 21 of `CTL1`, the
    /// per frame CRC quirk, is clear) and `FDRI`: its data is cut into
    /// frames of the format's word count starting at the latched frame
    /// address and following [`Part::next_frame_address`], skipping two
    /// frames of padding whenever the next address is in another row,
    /// half or block type; a later write of the same address replaces the
    /// frame.
    ///
    /// # Errors
    ///
    /// [`ReadError::IdcodeMismatch`]; [`ReadError::ArchitectureMismatch`]
    /// if the part is not of the format's part type.
    pub fn from_packets_with(
        part: &Part,
        format: &BitstreamFormat,
        packets: impl IntoIterator<Item = Packet<'a>>,
    ) -> Result<Self, ReadError> {
        let arch = part.architecture;
        if arch != format.addressing {
            return Err(ReadError::ArchitectureMismatch {
                part: arch,
                format: format.addressing,
            });
        }
        let wpf = format.words_per_frame;
        let mut command_register = 0u32;
        let mut frame_address_register = 0u32;
        let mut mask_register = 0u32;
        let mut ctl1_register = 0u32;
        let mut start_new_write = false;
        let mut current = FrameAddress(0);
        let mut frames = BTreeMap::new();
        for packet in packets {
            if packet.opcode != OPCODE_WRITE {
                continue;
            }
            let first = packet.data.first().copied();
            match packet.register {
                register::MASK => {
                    let Some(v) = first else { continue };
                    mask_register = v;
                }
                register::CTL1 => {
                    let Some(v) = first else { continue };
                    ctl1_register = v & mask_register;
                }
                register::CMD => {
                    let Some(v) = first else { continue };
                    command_register = v;
                    if command_register == command::WCFG {
                        start_new_write = true;
                    }
                }
                register::IDCODE => {
                    let Some(v) = first else { continue };
                    if v != part.idcode {
                        return Err(ReadError::IdcodeMismatch {
                            found: v,
                            expected: part.idcode,
                        });
                    }
                }
                register::FAR => {
                    let Some(v) = first else { continue };
                    frame_address_register = v;
                    if (ctl1_register >> 21) & 1 == 0 && command_register == command::WCFG {
                        start_new_write = true;
                    }
                }
                register::FDRI => {
                    if start_new_write {
                        current = FrameAddress(frame_address_register);
                        start_new_write = false;
                    }
                    let data = packet.data;
                    let mut ii = 0;
                    while ii < data.len() {
                        let end = data.len().min(ii + wpf);
                        frames.insert(current.0, &data[ii..end]);
                        let Some(next) = part.next_frame_address(current) else {
                            break;
                        };
                        if !same_row(arch, current, next) {
                            ii += 2 * wpf;
                        }
                        current = next;
                        ii += wpf;
                    }
                }
                _ => {}
            }
        }
        Ok(Configuration {
            frames,
            words_per_frame: wpf,
            ecc: format.ecc,
        })
    }

    /// The words per frame of the format the bitstream was read in.
    pub fn words_per_frame(&self) -> usize {
        self.words_per_frame
    }

    /// The frame ECC of the format the bitstream was read in.
    pub fn ecc(&self) -> Ecc {
        self.ecc
    }

    /// Number of frames.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// `true` if no frame was written.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The frames in ascending address order.
    pub fn frames(&self) -> impl ExactSizeIterator<Item = (u32, &'a [u32])> + '_ {
        self.frames.iter().map(|(&a, &w)| (a, w))
    }

    /// The words of the frame at `address`.
    pub fn get(&self, address: u32) -> Option<&'a [u32]> {
        self.frames.get(&address).copied()
    }

    /// The frames as [`Frames`] (a short frame is zero filled to the full
    /// word count), for writing a `.frm` file or a new bitstream. With
    /// `clear_ecc`, the ECC bits of the format ([`Ecc::ecc_mask`]; Series7:
    /// the low 13 bits of word 50) are cleared, which gives back the
    /// frames `fasm2frames` wrote (its `.frm` files never have ECC bits
    /// set); with `skip_zero`, all zero frames are left out (a sparse
    /// `.frm`).
    pub fn to_frames(&self, clear_ecc: bool, skip_zero: bool) -> Frames {
        let mut out = Frames::new(self.words_per_frame);
        let ecc = self.ecc;
        for (&address, &words) in &self.frames {
            let ecc_clear = |i: usize, w: u32| {
                if clear_ecc {
                    w & !ecc.ecc_mask(i)
                } else {
                    w
                }
            };
            if skip_zero && words.iter().enumerate().all(|(i, &w)| ecc_clear(i, w) == 0) {
                continue;
            }
            let frame = out.get_or_insert_zeroed(address);
            for (i, &w) in words.iter().enumerate() {
                frame[i] = ecc_clear(i, w);
            }
        }
        out
    }
}

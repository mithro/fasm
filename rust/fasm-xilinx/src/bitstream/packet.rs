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

//! Series7 configuration packets (UG470 pg. 108; prjxray
//! `configuration_packet.{h,cc}`, `bitstream_writer.cc` `packet2header`,
//! `bitstream_reader.h`; design document §6.2).

/// Configuration register addresses (`Series7ConfigurationRegister`).
pub mod register {
    /// `CRC`.
    pub const CRC: u32 = 0x00;
    /// `FAR`: frame address.
    pub const FAR: u32 = 0x01;
    /// `FDRI`: frame data input.
    pub const FDRI: u32 = 0x02;
    /// `FDRO`: frame data output.
    pub const FDRO: u32 = 0x03;
    /// `CMD`: command.
    pub const CMD: u32 = 0x04;
    /// `CTL0`.
    pub const CTL0: u32 = 0x05;
    /// `MASK`.
    pub const MASK: u32 = 0x06;
    /// `STAT`.
    pub const STAT: u32 = 0x07;
    /// `LOUT`.
    pub const LOUT: u32 = 0x08;
    /// `COR0`: configuration options 0.
    pub const COR0: u32 = 0x09;
    /// `MFWR`.
    pub const MFWR: u32 = 0x0a;
    /// `CBC`.
    pub const CBC: u32 = 0x0b;
    /// `IDCODE`.
    pub const IDCODE: u32 = 0x0c;
    /// `AXSS`.
    pub const AXSS: u32 = 0x0d;
    /// `COR1`: configuration options 1.
    pub const COR1: u32 = 0x0e;
    /// `WBSTAR`.
    pub const WBSTAR: u32 = 0x10;
    /// `TIMER`.
    pub const TIMER: u32 = 0x11;
    /// `UNKNOWN` (0x13, written by the reference sequence).
    pub const UNKNOWN: u32 = 0x13;
    /// `BOOTSTS`.
    pub const BOOTSTS: u32 = 0x16;
    /// `CTL1`.
    pub const CTL1: u32 = 0x18;
    /// `BSPI`.
    pub const BSPI: u32 = 0x1F;
}

/// `CMD` register commands (`xc7series::Command`).
pub mod command {
    /// `NOP`.
    pub const NOP: u32 = 0x0;
    /// `WCFG`: write configuration data.
    pub const WCFG: u32 = 0x1;
    /// `MFW`.
    pub const MFW: u32 = 0x2;
    /// `LFRM`: last frame.
    pub const LFRM: u32 = 0x3;
    /// `RCFG`.
    pub const RCFG: u32 = 0x4;
    /// `START`.
    pub const START: u32 = 0x5;
    /// `RCAP`.
    pub const RCAP: u32 = 0x6;
    /// `RCRC`: reset CRC.
    pub const RCRC: u32 = 0x7;
    /// `AGHIGH`.
    pub const AGHIGH: u32 = 0x8;
    /// `SWITCH`.
    pub const SWITCH: u32 = 0x9;
    /// `GRESTORE`.
    pub const GRESTORE: u32 = 0xA;
    /// `SHUTDOWN`.
    pub const SHUTDOWN: u32 = 0xB;
    /// `GCAPTURE`.
    pub const GCAPTURE: u32 = 0xC;
    /// `DESYNC`.
    pub const DESYNC: u32 = 0xD;
    /// `IPROG`.
    pub const IPROG: u32 = 0xF;
    /// `CRCC`.
    pub const CRCC: u32 = 0x10;
    /// `LTIMER`.
    pub const LTIMER: u32 = 0x11;
    /// `BSPI_READ`.
    pub const BSPI_READ: u32 = 0x12;
    /// `FALL_EDGE`.
    pub const FALL_EDGE: u32 = 0x13;
}

/// Packet opcode `NOP`.
pub const OPCODE_NOP: u32 = 0;
/// Packet opcode `Read`.
pub const OPCODE_READ: u32 = 1;
/// Packet opcode `Write`.
pub const OPCODE_WRITE: u32 = 2;

/// The header word of a Type1 packet (`packet2header`): header type 1 in
/// bits 31:29, `opcode` in 28:27, `register` in 26:13 and the word count
/// in 10:0 (each value masked to its field, like `bit_field_set`).
pub const fn type1_header(opcode: u32, register: u32, word_count: u32) -> u32 {
    (1 << 29) | ((opcode & 0x3) << 27) | ((register & 0x3FFF) << 13) | (word_count & 0x7FF)
}

/// The header word of a Type2 packet: header type 2 in bits 31:29,
/// `opcode` in 28:27 and the word count in 26:0 (masked).
pub const fn type2_header(opcode: u32, word_count: u32) -> u32 {
    (2 << 29) | ((opcode & 0x3) << 27) | (word_count & 0x7FF_FFFF)
}

/// The header word of a NOP packet (`NopPacket`: Type1, opcode NOP,
/// register `CRC`, no data).
pub const NOP_HEADER: u32 = type1_header(OPCODE_NOP, register::CRC, 0);

/// One configuration packet read from a bitstream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    /// Header type: 0 (padding), 1 or 2.
    pub header_type: u32,
    /// Opcode ([`OPCODE_NOP`], [`OPCODE_READ`], [`OPCODE_WRITE`] or the
    /// reserved 3).
    pub opcode: u32,
    /// Register address (a Type2 packet inherits the previous packet's).
    pub register: u32,
    /// Data words.
    pub data: &'a [u32],
}

/// `ConfigurationPacket::InitWithWords` for Series7: parses one packet at
/// the start of `words`. Returns the rest and the packet:
///
/// * `(words, None)` if `words` is empty or holds an incomplete packet;
/// * a Type0 word (`BITSTREAM.GENERAL.DEBUGBITSTREAM` padding) is one
///   word, a NOP packet of header type 0;
/// * a Type2 packet takes the register of `previous` and is `None`
///   without a previous packet (its words are consumed);
/// * header types 3 to 7 end the stream: `(&[], None)`.
fn init_with_words<'a>(
    words: &'a [u32],
    previous: Option<&Packet<'a>>,
) -> (&'a [u32], Option<Packet<'a>>) {
    let Some(&header) = words.first() else {
        return (words, None);
    };
    let header_type = header >> 29;
    let opcode = (header >> 27) & 0x3;
    match header_type {
        0 => (
            &words[1..],
            Some(Packet {
                header_type,
                opcode: OPCODE_NOP,
                register: register::CRC,
                data: &[],
            }),
        ),
        1 | 2 => {
            let count = if header_type == 1 {
                header & 0x7FF
            } else {
                header & 0x7FF_FFFF
            } as usize;
            if count > words.len() - 1 {
                return (words, None);
            }
            let data = &words[1..=count];
            let rest = &words[count + 1..];
            let register = if header_type == 1 {
                Some((header >> 13) & 0x3FFF)
            } else {
                previous.map(|p| p.register)
            };
            let packet = register.map(|register| Packet {
                header_type,
                opcode,
                register,
                data,
            });
            (rest, packet)
        }
        _ => (&[], None),
    }
}

/// Iterator over the packets of configuration words, following
/// `BitstreamReader::iterator` exactly: parsing stops at the first
/// incomplete packet or at a header type above 2, and words that yield no
/// packet (a Type2 packet with no packet before it) are skipped.
#[derive(Clone, Debug)]
pub struct PacketIter<'a> {
    rest: &'a [u32],
    previous: Option<Packet<'a>>,
    done: bool,
}

impl<'a> PacketIter<'a> {
    /// The packets of `words` (the words after the sync word).
    pub fn new(words: &'a [u32]) -> Self {
        PacketIter {
            rest: words,
            previous: None,
            done: false,
        }
    }
}

impl<'a> Iterator for PacketIter<'a> {
    type Item = Packet<'a>;

    fn next(&mut self) -> Option<Packet<'a>> {
        if self.done {
            return None;
        }
        loop {
            let (rest, packet) = init_with_words(self.rest, self.previous.as_ref());
            if rest.len() == self.rest.len() {
                // A valid header without enough words (or no words left).
                self.done = true;
                return None;
            }
            self.rest = rest;
            self.previous = packet;
            if self.rest.is_empty() || packet.is_some() {
                break;
            }
        }
        if self.previous.is_none() {
            self.done = true;
        }
        self.previous
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers() {
        assert_eq!(NOP_HEADER, 0x2000_0000);
        assert_eq!(type1_header(OPCODE_WRITE, register::CMD, 1), 0x3000_8001);
        assert_eq!(type1_header(OPCODE_WRITE, register::FDRI, 0), 0x3000_4000);
        assert_eq!(type2_header(OPCODE_WRITE, 547_420), 0x5000_0000 | 547_420);
        assert_eq!(type2_header(OPCODE_WRITE, 1 << 27), 0x5000_0000);
    }

    fn collect(words: &[u32]) -> Vec<(u32, u32, u32, Vec<u32>)> {
        PacketIter::new(words)
            .map(|p| (p.header_type, p.opcode, p.register, p.data.to_vec()))
            .collect()
    }

    /// Cases of prjxray `configuration_packet_test.cc` and
    /// `bitstream_reader_test.cc`.
    #[test]
    fn parsing() {
        // Empty.
        assert!(collect(&[]).is_empty());
        // Type0 padding is a NOP.
        assert_eq!(collect(&[0]), [(0, OPCODE_NOP, register::CRC, vec![])]);
        // Type1 with data.
        let w = [type1_header(OPCODE_WRITE, 3, 2), 0xAA, 0xBB];
        assert_eq!(collect(&w), [(1, OPCODE_WRITE, 3, vec![0xAA, 0xBB])]);
        // Incomplete packet: nothing.
        assert!(collect(&w[..2]).is_empty());
        // Type2 needs a previous packet: skipped without one.
        let w = [type2_header(OPCODE_WRITE, 1), 0xCC, NOP_HEADER];
        assert_eq!(collect(&w), [(1, OPCODE_NOP, 0, vec![])]);
        // Type2 after Type1 inherits the register.
        let w = [
            type1_header(OPCODE_WRITE, 3, 0),
            type2_header(OPCODE_WRITE, 12),
            1,
            2,
            3,
            4,
            5,
            6,
            7,
            8,
            9,
            10,
            11,
            12,
        ];
        let packets = collect(&w);
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[1], (2, OPCODE_WRITE, 3, (1..=12).collect()));
        // Header types 3 to 7 end the stream.
        let w = [NOP_HEADER, 0x6000_0000, NOP_HEADER];
        assert_eq!(collect(&w).len(), 1);
        // An incomplete packet ends the stream.
        let w = [NOP_HEADER, type1_header(OPCODE_WRITE, 1, 5), 1, NOP_HEADER];
        assert_eq!(collect(&w).len(), 1);
    }

    #[test]
    fn fuzz_never_panics() {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..5000 {
            let len = (next() % 40) as usize;
            let words: Vec<u32> = (0..len)
                .map(|_| {
                    let r = next();
                    // Bias towards small counts so that packets complete.
                    (r as u32)
                        & if r & (1 << 40) != 0 {
                            0xE000_000F
                        } else {
                            0xFFFF_FFFF
                        }
                })
                .collect();
            let total: usize = PacketIter::new(&words).map(|p| p.data.len() + 1).sum();
            assert!(total <= words.len());
        }
    }
}

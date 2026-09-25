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

//! Frame ECC (design document §6.5 and §8.10):
//!
//! * Series7 (prjxray `lib/xilinx/xc7series/ecc.cc`): a 13-bit value in
//!   the low bits of word 50 of every 101-word frame;
//! * UltraScale and UltraScale+ (prjuray-tools
//!   `lib/xilinx/xcuseries/ecc.cc`, `xcupseries/ecc.cc`): a 48-bit value
//!   in word 60 and the low half of word 61 of a 123-word frame
//!   (UltraScale), word 45 and the low half of word 46 of a 93-word frame
//!   (UltraScale+).
//!
//! [`Ecc`] selects the algorithm; [`update_ecc`], [`frame_ecc`] and
//! [`verify_ecc`] are the Series7 one.

use std::sync::OnceLock;

use crate::arch::Architecture;

/// The word holding the Series7 frame ECC (`kECCFrameNumber = 0x32`).
pub const SERIES7_ECC_WORD: usize = 0x32;

/// The bits of [`SERIES7_ECC_WORD`] holding the ECC.
pub const SERIES7_ECC_MASK: u32 = 0x1FFF;

/// `xc7series::icap_ecc(idx, data, ecc)`, a literal port: folds word `idx`
/// of a frame (value `data`) into the running ECC value `ecc`.
///
/// Every set bit `i` of `data` XORs `idx * 32 + i` plus a band offset
/// (`0x1320` for words 0-6, `0x1340` for 7-37, `0x1360` above) into the
/// ECC; the ECC bits of word 50 are masked out first, and word 100 (the
/// last one) folds the parity of the low 12 bits into bit 12.
pub fn icap_ecc(idx: u32, data: u32, ecc: u32) -> u32 {
    let mut val = idx.wrapping_mul(32);
    if idx > 0x25 {
        val = val.wrapping_add(0x1360);
    } else if idx > 0x6 {
        val = val.wrapping_add(0x1340);
    } else {
        val = val.wrapping_add(0x1320);
    }
    let mut data = data;
    if idx == 0x32 {
        data &= 0xFFFF_E000;
    }
    let mut ecc = ecc;
    for i in 0..32 {
        if data & 1 != 0 {
            ecc ^= val.wrapping_add(i);
        }
        data >>= 1;
    }
    if idx == 0x64 {
        ecc ^= parity12(ecc) << 12;
    }
    ecc
}

/// The parity of the low 12 bits (the fold of `icap_ecc` at word 100).
fn parity12(ecc: u32) -> u32 {
    let mut v = ecc & 0xFFF;
    v ^= v >> 8;
    v ^= v >> 4;
    v ^= v >> 2;
    v ^= v >> 1;
    v & 1
}

/// The ECC contribution of one word, computed without a loop over the
/// bits: `val + i` equals `val | i` because `val` is a multiple of 32, so
/// the XOR over the set bits is `val` (if their count is odd) XOR the XOR
/// of their indexes, whose bit `k` is the parity of the set bits whose
/// index has bit `k` set.
fn word_ecc(idx: u32, data: u32) -> u32 {
    const INDEX_BITS: [u32; 5] = [
        0xAAAA_AAAA,
        0xCCCC_CCCC,
        0xF0F0_F0F0,
        0xFF00_FF00,
        0xFFFF_0000,
    ];
    let band = if idx > 0x25 {
        0x1360
    } else if idx > 0x6 {
        0x1340
    } else {
        0x1320
    };
    let val = idx.wrapping_mul(32).wrapping_add(band);
    let mut result = if data.count_ones() & 1 != 0 { val } else { 0 };
    for (k, mask) in INDEX_BITS.iter().enumerate() {
        result ^= ((data & mask).count_ones() & 1) << k;
    }
    result
}

/// `calculateECC` of `xc7series/ecc.cc`: the ECC of a whole frame (any
/// length; the reference is only meaningful for 101 words), including
/// the parity fold at word 100.
pub fn frame_ecc(frame: &[u32]) -> u32 {
    let mut ecc = 0;
    for (idx, &word) in frame.iter().enumerate() {
        let idx = idx as u32;
        let data = if idx == 0x32 {
            word & 0xFFFF_E000
        } else {
            word
        };
        if data != 0 {
            ecc ^= word_ecc(idx, data);
        }
        if idx == 0x64 {
            ecc ^= parity12(ecc) << 12;
        }
    }
    ecc
}

/// `xc7series::updateECC`: replaces the low 13 bits of word 50 of a
/// Series7 frame with the frame's ECC. Frames with 50 words or fewer are
/// left unchanged (the reference asserts at least 50 and then writes word
/// 50, undefined behaviour for exactly 50).
pub fn update_ecc(frame: &mut [u32]) {
    if frame.len() <= SERIES7_ECC_WORD {
        return;
    }
    let ecc = frame_ecc(frame);
    frame[SERIES7_ECC_WORD] =
        (frame[SERIES7_ECC_WORD] & !SERIES7_ECC_MASK) | (ecc & SERIES7_ECC_MASK);
}

/// `xc7series::verifyECC`: the ECC of the whole frame (the stored ECC
/// bits masked out) equals the low 13 bits of word 50. `None` for a frame
/// of 50 words or fewer (`data.at(50)` throws `std::out_of_range`).
pub fn verify_ecc(frame: &[u32]) -> Option<bool> {
    let stored = frame.get(SERIES7_ECC_WORD)? & SERIES7_ECC_MASK;
    Some(frame_ecc(frame) == stored)
}

/// The frame ECC algorithm of an architecture (`Frames<ArchType>::updateECC`
/// and `verifyECC<ArchType>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ecc {
    /// `xc7series::updateECC`: 13 bits in word 50. prjxray also uses it
    /// for its UltraScale and UltraScale+ architectures (on their 123 and
    /// 93 word frames).
    Series7,
    /// `xcuseries::updateECC`: 48 bits in word 60 and the low half of
    /// word 61 (123-word frames).
    UltraScale,
    /// `xcupseries::updateECC`: 48 bits in word 45 and the low half of
    /// word 46 (93-word frames).
    UltraScalePlus,
}

/// The parameters of the UltraScale(+) ECC: the first ECC word
/// (`kECCFrameNumber`) and the last word of the frame (the `255 - N` of
/// `calculate_us_ecc`).
#[derive(Clone, Copy, Debug)]
struct UsEcc {
    ecc_word: usize,
    last_word: u32,
}

const XCUSERIES: UsEcc = UsEcc {
    ecc_word: 60,
    last_word: 122,
};

const XCUPSERIES: UsEcc = UsEcc {
    ecc_word: 45,
    last_word: 92,
};

/// `calculate_us_ecc(word, bit)` of `xcu(p)series/ecc.cc`, a literal port:
/// the 11-bit offset `(word + 255 - last_word) << 3 | bit / 4` with an odd
/// parity bit 11, each of its 12 bits expanded to one bit per nibble,
/// shifted by `bit % 4`.
fn calculate_us_ecc(last_word: u32, word: u32, bit: u32) -> u64 {
    let nib = bit / 4;
    let nibbit = bit % 4;
    let mut offset = (word.wrapping_add(255 - last_word) << 3) | nib;
    offset ^= 1 << 11;
    for i in 0..11 {
        if offset & (1 << i) != 0 {
            offset ^= 1 << 11;
        }
    }
    let mut exp_offset = 0u64;
    for i in 0..12 {
        if offset & (1 << i) != 0 {
            exp_offset |= 1u64 << (4 * i);
        }
    }
    exp_offset << nibbit
}

impl UsEcc {
    /// `calculate_us_ecc` for every bit of the frame's words, cached.
    fn table(&self) -> &'static [u64] {
        static XCU: OnceLock<Vec<u64>> = OnceLock::new();
        static XCUP: OnceLock<Vec<u64>> = OnceLock::new();
        let cell = if self.last_word == XCUSERIES.last_word {
            &XCU
        } else {
            &XCUP
        };
        let last_word = self.last_word;
        cell.get_or_init(|| {
            (0..=last_word)
                .flat_map(|word| (0..32).map(move |bit| calculate_us_ecc(last_word, word, bit)))
                .collect()
        })
    }

    /// `get_us_ecc(idx, data, 0)`: the ECC words are masked out (the whole
    /// first one, the low half of the second one).
    fn word_ecc(&self, table: &[u64], idx: usize, data: u32) -> u64 {
        let mut data = data;
        if idx == self.ecc_word {
            data = 0;
        }
        if idx == self.ecc_word + 1 {
            data &= 0xFFFF_0000;
        }
        let mut ecc = 0;
        while data != 0 {
            let bit = data.trailing_zeros();
            data &= data - 1;
            ecc ^= match table.get(idx * 32 + bit as usize) {
                Some(&v) => v,
                // A frame longer than the architecture's (never written
                // by the tools): the literal formula.
                None => calculate_us_ecc(self.last_word, idx as u32, bit),
            };
        }
        ecc
    }

    /// `calculateECC`: the 48-bit ECC of a frame of any length.
    fn frame_ecc(&self, frame: &[u32]) -> u64 {
        let table = self.table();
        frame
            .iter()
            .enumerate()
            .fold(0, |ecc, (idx, &w)| ecc ^ self.word_ecc(table, idx, w))
    }

    fn update(&self, frame: &mut [u32]) {
        if frame.len() <= self.ecc_word + 1 {
            return;
        }
        let ecc = self.frame_ecc(frame);
        frame[self.ecc_word] = ecc as u32;
        let high = &mut frame[self.ecc_word + 1];
        *high = (*high & 0xFFFF_0000) | ((ecc >> 32) as u32 & 0xFFFF);
    }

    fn verify(&self, frame: &[u32]) -> Option<bool> {
        let high = u64::from(*frame.get(self.ecc_word + 1)? & 0xFFFF);
        let stored = (high << 32) | u64::from(frame[self.ecc_word]);
        Some(self.frame_ecc(frame) == stored)
    }
}

impl Ecc {
    /// The algorithm of prjuray-tools for `arch` (prjxray uses
    /// [`Ecc::Series7`] for every architecture).
    pub const fn of(arch: Architecture) -> Self {
        match arch {
            Architecture::Series7 => Ecc::Series7,
            Architecture::UltraScale => Ecc::UltraScale,
            Architecture::UltraScalePlus => Ecc::UltraScalePlus,
        }
    }

    fn us(self) -> Option<UsEcc> {
        match self {
            Ecc::Series7 => None,
            Ecc::UltraScale => Some(XCUSERIES),
            Ecc::UltraScalePlus => Some(XCUPSERIES),
        }
    }

    /// `updateECC`: replaces the ECC bits of the frame with the ECC of
    /// the frame. A frame too short to hold them is left unchanged (the
    /// reference asserts).
    pub fn update(self, frame: &mut [u32]) {
        match self.us() {
            None => update_ecc(frame),
            Some(us) => us.update(frame),
        }
    }

    /// The ECC value [`Ecc::update`] stores (13 or 48 bits).
    pub fn compute(self, frame: &[u32]) -> u64 {
        match self.us() {
            None => u64::from(frame_ecc(frame)),
            Some(us) => us.frame_ecc(frame),
        }
    }

    /// `verifyECC<ArchType>` (prjuray-tools `bitread`): the stored ECC
    /// equals the computed one. `None` where the reference's `at()`
    /// throws `std::out_of_range` (a frame too short to hold the ECC
    /// bits).
    pub fn verify(self, frame: &[u32]) -> Option<bool> {
        match self.us() {
            None => verify_ecc(frame),
            Some(us) => us.verify(frame),
        }
    }

    /// `is_ecc_bit<ArchType>(word, bit)` of prjuray-tools' `ecc.h`: the
    /// ECC bits `bitread -x`/`-y` leave out unless `-C` is given.
    pub const fn is_ecc_bit(self, word: usize, bit: u32) -> bool {
        self.ecc_mask(word) & (1 << (bit & 31)) != 0
    }

    /// The mask of the ECC bits of word `word`.
    pub const fn ecc_mask(self, word: usize) -> u32 {
        match (self, word) {
            (Ecc::Series7, SERIES7_ECC_WORD) => SERIES7_ECC_MASK,
            (Ecc::UltraScale, 60) | (Ecc::UltraScalePlus, 45) => u32::MAX,
            (Ecc::UltraScale, 61) | (Ecc::UltraScalePlus, 46) => 0xFFFF,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `calculateECC` exactly as written in the reference.
    fn reference_frame_ecc(frame: &[u32]) -> u32 {
        frame
            .iter()
            .enumerate()
            .fold(0, |ecc, (i, &w)| icap_ecc(i as u32, w, ecc))
    }

    /// prjxray `lib/xilinx/tests/xc7series/ecc_test.cc`.
    #[test]
    fn icap_ecc_reference_vectors() {
        // ECC for zero data.
        assert_eq!(icap_ecc(0, 0, 0), 0x0);
        // 0x1320 - 0x13FF (avoid lower).
        assert_eq!(icap_ecc(0, 1, 0), 0x1320);
        // 0x1420 - 0x17FF (avoid 0x400).
        assert_eq!(icap_ecc(0x7, 1, 0), 0x1420);
        // 0x1820 - 0x1FFF (avoid 0x800).
        assert_eq!(icap_ecc(0x26, 1, 0), 0x1820);
        // Masked ECC value.
        assert_eq!(icap_ecc(0x32, !0, 0), 0x0000_19AC);
        // Final ECC parity.
        assert_eq!(icap_ecc(0x64, 0, 1), 0x0000_1001);
    }

    #[test]
    fn fast_ecc_matches_reference() {
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for round in 0..2000 {
            let mut frame = vec![0u32; 101];
            // Mix of dense, sparse and empty frames.
            let density = round % 4;
            for word in &mut frame {
                let r = next();
                *word = match density {
                    0 => r as u32,
                    1 => (r as u32) & (r >> 32) as u32 & (r >> 16) as u32,
                    2 if r % 7 == 0 => 1 << (r % 32),
                    _ => 0,
                };
            }
            assert_eq!(frame_ecc(&frame), reference_frame_ecc(&frame));
            for idx in [0u32, 6, 7, 0x25, 0x26, 0x32, 0x63, 200] {
                let data = next() as u32;
                let masked = if idx == 0x32 {
                    data & 0xFFFF_E000
                } else {
                    data
                };
                assert_eq!(icap_ecc(idx, data, 0), word_ecc(idx, masked));
            }
        }
    }

    #[test]
    fn update_ecc_writes_word_50() {
        let mut frame = vec![0u32; 101];
        update_ecc(&mut frame);
        assert!(frame.iter().all(|&w| w == 0));
        frame[0] = 1;
        frame[50] = 0xFFFF_FFFF;
        update_ecc(&mut frame);
        let expected = reference_frame_ecc(&frame) & 0x1FFF;
        assert_eq!(frame[50], 0xFFFF_E000 | expected);
        // Idempotent: the old ECC bits are ignored.
        let before = frame.clone();
        update_ecc(&mut frame);
        assert_eq!(frame, before);
        // Short frames are left alone.
        let mut short = vec![1u32; 50];
        update_ecc(&mut short);
        assert_eq!(short, vec![1u32; 50]);
    }

    /// `calculateECC` of `xcu(p)series/ecc.cc` exactly as written (a loop
    /// over every bit of every word).
    fn reference_us_ecc(ecc: Ecc, frame: &[u32]) -> u64 {
        let us = ecc.us().unwrap();
        let mut result = 0u64;
        for (idx, &word) in frame.iter().enumerate() {
            let mut data = word;
            if idx == us.ecc_word {
                data = 0;
            }
            if idx == us.ecc_word + 1 {
                data &= 0xFFFF_0000;
            }
            for i in 0..32 {
                if data & 1 != 0 {
                    result ^= calculate_us_ecc(us.last_word, idx as u32, i);
                }
                data >>= 1;
            }
        }
        result
    }

    #[test]
    fn us_ecc_offset_expansion() {
        // The example of the comment in `calculate_us_ecc`: word 3, bit 9.
        assert_eq!(calculate_us_ecc(92, 3, 9), 0x202_0022_0020);
        // Odd parity: an offset with an even number of ones gets bit 11.
        let v = calculate_us_ecc(92, 0, 0);
        // (0 + 163) << 3 = 0b10100011000: 4 ones -> parity bit set.
        assert_eq!(v, 0x1101_0001_1000);
    }

    #[test]
    fn us_ecc_matches_literal_port() {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for ecc in [Ecc::UltraScale, Ecc::UltraScalePlus] {
            let wpf = if ecc == Ecc::UltraScale { 123 } else { 93 };
            for round in 0..300 {
                let mut frame: Vec<u32> = (0..wpf)
                    .map(|_| {
                        let r = next();
                        if round % 3 == 0 {
                            r as u32
                        } else if r % 5 == 0 {
                            1 << (r % 32)
                        } else {
                            0
                        }
                    })
                    .collect();
                let expected = reference_us_ecc(ecc, &frame);
                assert_eq!(ecc.compute(&frame), expected);
                ecc.update(&mut frame);
                let us = ecc.us().unwrap();
                assert_eq!(frame[us.ecc_word], expected as u32);
                assert_eq!(frame[us.ecc_word + 1] & 0xFFFF, (expected >> 32) as u32);
                assert_eq!(ecc.verify(&frame), Some(true));
                // Idempotent, and the upper half of the second word is data.
                let before = frame.clone();
                ecc.update(&mut frame);
                assert_eq!(frame, before);
                frame[3] ^= 1;
                assert_eq!(ecc.verify(&frame), Some(false));
                frame[us.ecc_word + 1] ^= 1 << 20;
                assert_ne!(ecc.compute(&frame), expected);
            }
            // Longer frames use the literal formula past the table.
            let long = vec![0xFFFF_FFFFu32; wpf + 3];
            assert_eq!(ecc.compute(&long), reference_us_ecc(ecc, &long));
            // Frames too short for the ECC words.
            let us = ecc.us().unwrap();
            let mut short = vec![7u32; us.ecc_word + 1];
            ecc.update(&mut short);
            assert_eq!(short, vec![7u32; us.ecc_word + 1]);
            assert_eq!(ecc.verify(&short), None);
            assert_eq!(ecc.verify(&vec![0; us.ecc_word + 2]), Some(true));
        }
    }

    /// Frames of the Vivado bitstreams of prjuray-tools'
    /// `ToolsTestData.tar.gz` (as printed by the reference `bitread -o`).
    #[test]
    fn us_ecc_of_vivado_frames() {
        // UltraScalePlus/design.bit, frame 0x0000013A: words 0, 1, 2 repeat
        // 0x00000001, 0x00010000, 0; the ECC is word 45 (0) and the low
        // half of word 46 (0x1001).
        let mut frame: Vec<u32> = (0..93).map(|i| [1, 0x1_0000, 0][i % 3]).collect();
        frame[45] = 0;
        frame[46] = 0x0000_1001;
        frame[47] = 0;
        assert_eq!(Ecc::UltraScalePlus.verify(&frame), Some(true));
        assert_eq!(Ecc::UltraScalePlus.compute(&frame), 0x1001_0000_0000);
        // UltraScale/design.bit, frame 0x00000003: 8 in words 8, 18, ...,
        // 58 and 71, 81, ..., 121 (the ECC words 60 and 61 in between).
        let mut frame = vec![0u32; 123];
        for i in (8..=58).step_by(10).chain((71..=121).step_by(10)) {
            frame[i] = 8;
        }
        assert_eq!(Ecc::UltraScale.verify(&frame), Some(true));
        // Series7 via Ecc.
        let mut s7 = vec![0u32; 101];
        s7[3] = 0x1234_5678;
        Ecc::Series7.update(&mut s7);
        assert_eq!(Ecc::Series7.verify(&s7), Some(true));
        assert_eq!(u64::from(s7[50] & 0x1FFF), Ecc::Series7.compute(&s7));
        assert_eq!(Ecc::Series7.verify(&s7[..50]), None);
    }

    #[test]
    fn ecc_bits() {
        let count = |ecc: Ecc, wpf: usize| {
            (0..wpf)
                .flat_map(|w| (0..32).map(move |b| (w, b)))
                .filter(|&(w, b)| ecc.is_ecc_bit(w, b))
                .count()
        };
        assert_eq!(count(Ecc::Series7, 101), 13);
        assert_eq!(count(Ecc::UltraScale, 123), 48);
        assert_eq!(count(Ecc::UltraScalePlus, 93), 48);
        assert!(Ecc::Series7.is_ecc_bit(50, 12) && !Ecc::Series7.is_ecc_bit(50, 13));
        assert!(Ecc::UltraScale.is_ecc_bit(61, 15) && !Ecc::UltraScale.is_ecc_bit(61, 16));
        assert!(Ecc::UltraScalePlus.is_ecc_bit(46, 15) && !Ecc::UltraScalePlus.is_ecc_bit(46, 16));
        for arch in Architecture::ALL {
            let ecc = Ecc::of(arch);
            for &(word, mask) in arch.ecc_reserved_bits() {
                assert_eq!(ecc.ecc_mask(word), mask);
            }
        }
    }
}

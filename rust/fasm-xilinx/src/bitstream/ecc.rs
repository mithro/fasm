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

//! Series7 frame ECC (prjxray `lib/xilinx/xc7series/ecc.cc`, design
//! document §6.5): a 13-bit value in the low bits of word 50 of every
//! 101-word frame.

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
}

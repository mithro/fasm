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

//! Conversion of already validated digit strings to [`FeatureValue`]s.
//!
//! The scanner has checked that every byte is either `_` or a digit of the
//! radix, so these functions never fail; they are still written so that an
//! unexpected byte can only produce a wrong value, never a panic.

use crate::model::{FeatureValue, INLINE_BITS};

/// Number of `u64` limbs a [`FeatureValue`] holds inline.
const INLINE_LIMBS: usize = (INLINE_BITS / 64) as usize;

/// Value of an ASCII digit (`0-9a-fA-F`); `0` for anything else.
fn digit_value(b: u8) -> u64 {
    match b {
        b'0'..=b'9' => u64::from(b - b'0'),
        b'a'..=b'f' => u64::from(b - b'a' + 10),
        b'A'..=b'F' => u64::from(b - b'A' + 10),
        _ => 0,
    }
}

/// Maximum number of significant digits (`_` and leading zeros not
/// counted) of a decimal value: Python's default
/// `sys.get_int_max_str_digits()`, which the textX parser runs into
/// (`ValueError: Exceeds the limit (4300 digits)`; Python also counts
/// leading zeros, the ANTLR parser does not). The ANTLR parser's limits
/// are much lower (2^31 - 1 for plain values, 2^64 - 1 for `'d` values).
pub(super) const MAX_DECIMAL_DIGITS: usize = 4300;

/// `10^i` for `i` in `0..=19`.
const POW10: [u64; 20] = {
    let mut t = [1u64; 20];
    let mut i = 1;
    while i < 20 {
        t[i] = t[i - 1] * 10;
        i += 1;
    }
    t
};

/// Parses decimal `digits` (`_` separators ignored). No `_`-free digit at
/// all gives `0` (like the ANTLR parser does for `'d_`).
///
/// Converts 19 digits per step. Allocation free up to 256 bits; the
/// caller bounds the digit count ([`MAX_DECIMAL_DIGITS`]), which bounds
/// the (quadratic) cost of wider values.
pub(super) fn decimal(digits: &[u8]) -> FeatureValue {
    let mut stack = [0u64; INLINE_LIMBS];
    if decimal_into(digits, &mut stack) {
        return FeatureValue::from_le_limbs(&stack);
    }
    // log2(10) < 10 / 3: every 3 digits need at most 10 bits.
    let bits = digits.len().div_ceil(3).saturating_mul(10);
    let mut heap = vec![0u64; bits / 64 + 1];
    decimal_into(digits, &mut heap);
    FeatureValue::from_le_limbs(&heap)
}

/// Accumulates decimal `digits` into `limbs` (little endian, zeroed by the
/// caller), 19 digits at a time; `false` if the value does not fit.
fn decimal_into(digits: &[u8], limbs: &mut [u64]) -> bool {
    let mut chunk = 0u64;
    let mut len = 0usize;
    for &b in digits {
        if b == b'_' {
            continue;
        }
        // At most 19 digits: below 10^19 < 2^64.
        chunk = chunk * 10 + digit_value(b);
        len += 1;
        if len == 19 {
            if !mul_add(limbs, POW10[19], chunk) {
                return false;
            }
            chunk = 0;
            len = 0;
        }
    }
    len == 0 || mul_add(limbs, POW10[len], chunk)
}

/// `limbs = limbs * mul + add`; `false` if the result does not fit.
fn mul_add(limbs: &mut [u64], mul: u64, add: u64) -> bool {
    let mut carry = u128::from(add);
    for limb in limbs.iter_mut() {
        let x = u128::from(*limb) * u128::from(mul) + carry;
        *limb = x as u64;
        carry = x >> 64;
    }
    carry == 0
}

/// Parses `digits` of a power of two radix, `bits` bits per digit (1 for
/// binary, 3 for octal, 4 for hexadecimal; `_` separators ignored). No
/// `_`-free digit at all gives `0` (like the ANTLR parser does for `'h_`).
///
/// Works from the least significant end, 8 digits at a time when a chunk
/// holds no `_` (see [`chunk_value`]), one digit at a time otherwise.
pub(super) fn power_of_two(digits: &[u8], bits: u32) -> FeatureValue {
    // Leading zeros (and `_`) do not count towards the size.
    let first = digits
        .iter()
        .position(|&b| b != b'0' && b != b'_')
        .unwrap_or(digits.len());
    let digits = digits.get(first..).unwrap_or_default();
    // Upper bound of the bit length (`_` take no bits).
    let max_bits = digits.len().saturating_mul(bits as usize);
    let nlimbs = max_bits.div_ceil(64);
    let mut stack = [0u64; INLINE_LIMBS];
    let mut heap: Vec<u64>;
    let limbs: &mut [u64] = if nlimbs <= INLINE_LIMBS {
        &mut stack[..]
    } else {
        heap = vec![0; nlimbs];
        &mut heap
    };

    let mut bit = 0usize;
    let mut chunks = digits.rchunks_exact(8);
    for chunk in chunks.by_ref() {
        match chunk_value(chunk, bits) {
            Some(v) => {
                put(limbs, bit, v, 8 * bits);
                bit += 8 * bits as usize;
            }
            None => bit = put_digits(limbs, bit, chunk, bits),
        }
    }
    put_digits(limbs, bit, chunks.remainder(), bits);
    FeatureValue::from_le_limbs(limbs)
}

/// Stores the digits of `chunk` (most significant first) one by one at
/// bit `bit` of `limbs`; returns the bit after the last stored digit.
fn put_digits(limbs: &mut [u64], mut bit: usize, chunk: &[u8], bits: u32) -> usize {
    for &b in chunk.iter().rev() {
        if b != b'_' {
            put(limbs, bit, digit_value(b), bits);
            bit += bits as usize;
        }
    }
    bit
}

/// ORs the `width` (at most 32) bit value `v` into `limbs` at bit `bit`.
fn put(limbs: &mut [u64], bit: usize, v: u64, width: u32) {
    let idx = bit / 64;
    let off = (bit % 64) as u32;
    if let Some(limb) = limbs.get_mut(idx) {
        *limb |= v << off;
    }
    if off + width > 64 {
        if let Some(limb) = limbs.get_mut(idx + 1) {
            *limb |= v >> (64 - off);
        }
    }
}

/// Value of 8 digits (most significant first) of `bits` bits each (1, 3
/// or 4), converted in parallel within a `u64` (SWAR); `None` if the chunk
/// holds a `_`. The bytes are otherwise known to be valid digits.
fn chunk_value(chunk: &[u8], bits: u32) -> Option<u64> {
    const ONES: u64 = 0x0101_0101_0101_0101;
    let bytes: [u8; 8] = chunk.try_into().ok()?;
    let x = u64::from_be_bytes(bytes);
    // Classic "has a zero byte" test on x ^ "________".
    let y = x ^ (ONES * u64::from(b'_'));
    if y.wrapping_sub(ONES) & !y & (ONES * 0x80) != 0 {
        return None;
    }
    // Digit values, one per byte: `0-9` are 0x30-0x39, letters (bit 6
    // set) are 0x41-0x46 / 0x61-0x66 and need + 9.
    let letters = (x & (ONES * 0x40)) >> 6;
    let mut v = (x & (ONES * 0x0F)) + letters * 9;
    // Pack pairs, then quads, then octets of digits.
    let b = u64::from(bits);
    v = (v | (v >> (8 - b))) & lane_mask(16, 2 * b);
    v = (v | (v >> (16 - 2 * b))) & lane_mask(32, 4 * b);
    v = (v | (v >> (32 - 4 * b))) & lane_mask(64, 8 * b);
    Some(v)
}

/// A mask with the low `width` bits of every `lane` bit lane set.
const fn lane_mask(lane: u64, width: u64) -> u64 {
    let one_lane = (1u64 << width) - 1;
    let mut mask = 0;
    let mut shift = 0;
    while shift < 64 {
        mask |= one_lane << shift;
        shift += lane;
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(digits: &str, radix: u32) {
        let expected = FeatureValue::from_digits(digits.as_bytes(), radix).unwrap();
        let got = match radix {
            10 => decimal(digits.as_bytes()),
            2 => power_of_two(digits.as_bytes(), 1),
            8 => power_of_two(digits.as_bytes(), 3),
            16 => power_of_two(digits.as_bytes(), 4),
            _ => unreachable!(),
        };
        assert_eq!(got, expected, "{digits} radix {radix}");
    }

    #[test]
    fn matches_from_digits() {
        for radix in [2u32, 8, 10, 16] {
            let alphabet: &[u8] = match radix {
                2 => b"01",
                8 => b"01234567",
                10 => b"0123456789",
                _ => b"0123456789abcdefABCDEF",
            };
            // Deterministic pseudo random digit strings of many lengths.
            let mut state = 0x2545_f491_4f6c_dd1du64 ^ u64::from(radix);
            for len in 1..400 {
                let mut s = String::new();
                for i in 0..len {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    if i > 0 && state.is_multiple_of(7) {
                        s.push('_');
                    }
                    s.push(alphabet[(state % alphabet.len() as u64) as usize] as char);
                }
                check(&s, radix);
            }
            // All ones / max digits across the inline/heap boundaries.
            let max = *alphabet.last().unwrap() as char;
            for len in [1usize, 20, 21, 22, 63, 64, 65, 85, 86, 87, 256, 257] {
                check(&max.to_string().repeat(len), radix);
            }
        }
    }

    #[test]
    fn only_underscores_is_zero() {
        assert_eq!(decimal(b"_"), FeatureValue::zero());
        assert_eq!(power_of_two(b"__", 4), FeatureValue::zero());
    }
}

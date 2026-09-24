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

//! [`FeatureValue`]: an arbitrary width unsigned integer.
//!
//! Python's `SetFasmFeature.value` is an unbounded `int`. Real FASM files
//! only ever use 1 to 256 bit values (BRAM `INIT` strings are the widest,
//! at 256 bits), but nothing in the grammar caps the width, so `FeatureValue`
//! supports arbitrary widths: values of up to [`INLINE_BITS`] bits are
//! stored inline (no heap allocation), wider values spill to a heap
//! allocated `Box<[u64]>`.
//!
//! # Representation and invariants
//!
//! `FeatureValue` wraps a private `Repr`:
//!
//! * `Repr::Inline([u64; INLINE_LIMBS])`: exactly [`INLINE_LIMBS`] limbs,
//!   little endian (`limb[0]` holds bits `0..64`), zero padded above the
//!   value's highest set bit. Used whenever the value fits in
//!   [`INLINE_BITS`] bits, including zero.
//! * `Repr::Heap(Box<[u64]>)`: used only when the value needs more than
//!   [`INLINE_BITS`] bits. Its length is always the minimal number of limbs
//!   (the top limb is always non-zero).
//!
//! Every constructor and mutating operation goes through
//! [`FeatureValue::from_limb_vec`], which trims trailing (most significant)
//! zero limbs and picks `Inline` or `Heap` accordingly. This keeps the
//! representation of a given numeric value canonical, which is what lets
//! `#[derive(PartialEq, Eq, Hash)]` below be correct: two `FeatureValue`s
//! compare equal (and hash equal) if and only if they hold the same number,
//! regardless of the history of operations that produced them.
//!
//! [`Ord`] is implemented by hand (see its impl) rather than derived,
//! because the derived, variant-then-field order does not match numeric
//! order (`Inline` limbs are stored least significant first, the opposite
//! of what a derived, most-significant-field-first comparison needs).

use std::cmp::Ordering;
use std::fmt;

use super::error::ValueParseError;

/// Number of `u64` limbs stored inline before falling back to a heap
/// allocation.
const INLINE_LIMBS: usize = 4;

/// Number of bits a [`FeatureValue`] can hold without a heap allocation
/// (`INLINE_LIMBS * 64`). Large enough for a 256-bit BRAM `INIT` value.
pub const INLINE_BITS: u32 = (INLINE_LIMBS * 64) as u32;

/// The digit characters used by [`FeatureValue::to_radix_string`], indexed
/// by digit value.
const LOWER_DIGITS: &[u8; 16] = b"0123456789abcdef";
const UPPER_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// The largest power of ten that fits in a `u64` (`10^19`); used to convert
/// to decimal a "big digit" (19 decimal digits) at a time instead of one
/// decimal digit at a time.
const DECIMAL_CHUNK: u64 = 10_000_000_000_000_000_000;
/// Number of decimal digits in [`DECIMAL_CHUNK`], i.e. `log10(DECIMAL_CHUNK)`.
const DECIMAL_CHUNK_DIGITS: usize = 19;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Repr {
    /// Zero padded to exactly `INLINE_LIMBS` limbs (see module docs).
    Inline([u64; INLINE_LIMBS]),
    /// Never empty; the top (last) limb is always non-zero (see module
    /// docs).
    Heap(Box<[u64]>),
}

impl Default for Repr {
    fn default() -> Self {
        Repr::Inline([0; INLINE_LIMBS])
    }
}

/// An arbitrary width, non-negative integer: the value of a
/// [`super::SetFasmFeature`].
///
/// Mirrors Python's unbounded `int` (as used for `SetFasmFeature.value`).
/// Values up to [`INLINE_BITS`] (256) bits are stored inline, with no heap
/// allocation; wider values (nothing in the FASM grammar forbids them) fall
/// back to a heap allocation. `size_of::<FeatureValue>()` is documented in
/// `docs/rewrite/DESIGN-model.md` (target: at most 40 bytes).
///
/// `FeatureValue::default()` is `0`, matching Rust's usual numeric
/// `Default`. Note this is *not* the same as an omitted `FeatureValue` in
/// FASM source, which means `1` (see `docs/specification/line.rst`); the
/// caller (the parser, T1.3) is responsible for substituting
/// `FeatureValue::from_u64(1)` in that case.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct FeatureValue(Repr);

impl FeatureValue {
    /// The value `0`.
    #[must_use]
    pub fn zero() -> Self {
        Self::default()
    }

    /// Builds a `FeatureValue` from a `u64`.
    #[must_use]
    pub fn from_u64(v: u64) -> Self {
        FeatureValue(Repr::Inline([v, 0, 0, 0]))
    }

    /// Builds a `FeatureValue` from a `u128`.
    #[must_use]
    pub fn from_u128(v: u128) -> Self {
        let lo = v as u64;
        let hi = (v >> 64) as u64;
        FeatureValue(Repr::Inline([lo, hi, 0, 0]))
    }

    /// Builds a `FeatureValue` of `0` or `1` from a `bool`.
    #[must_use]
    pub fn from_bool(b: bool) -> Self {
        Self::from_u64(u64::from(b))
    }

    /// Parses `digits` (ASCII digits of the given `radix`, `_` separators
    /// ignored) into a `FeatureValue`, mirroring
    /// `int(text.replace('_', ''), radix)` in the Python parsers.
    ///
    /// `radix` must be 2, 8, 10 or 16.
    ///
    /// # Errors
    ///
    /// * [`ValueParseError::UnsupportedRadix`] if `radix` is not one of
    ///   2, 8, 10, 16.
    /// * [`ValueParseError::InvalidDigit`] if a byte (other than `_`) is
    ///   not a valid digit of `radix`.
    /// * [`ValueParseError::EmptyDigits`] if, after removing `_`, no digits
    ///   remain.
    pub fn from_digits(digits: &[u8], radix: u32) -> Result<Self, ValueParseError> {
        if !matches!(radix, 2 | 8 | 10 | 16) {
            return Err(ValueParseError::UnsupportedRadix(radix));
        }

        let mut value = FeatureValue::default();
        let mut saw_digit = false;

        for &byte in digits {
            if byte == b'_' {
                continue;
            }

            let digit = match byte {
                b'0'..=b'9' => u32::from(byte - b'0'),
                b'a'..=b'f' => u32::from(byte - b'a') + 10,
                b'A'..=b'F' => u32::from(byte - b'A') + 10,
                _ => {
                    return Err(ValueParseError::InvalidDigit {
                        digit: byte as char,
                        radix,
                    });
                }
            };

            if digit >= radix {
                return Err(ValueParseError::InvalidDigit {
                    digit: byte as char,
                    radix,
                });
            }

            value = value.mul_add_small(radix, digit);
            saw_digit = true;
        }

        if !saw_digit {
            return Err(ValueParseError::EmptyDigits);
        }

        Ok(value)
    }

    /// Convenience wrapper around [`Self::from_digits`] with `radix = 16`.
    ///
    /// # Errors
    ///
    /// See [`Self::from_digits`].
    pub fn from_hex_str(s: &str) -> Result<Self, ValueParseError> {
        Self::from_digits(s.as_bytes(), 16)
    }

    /// Convenience wrapper around [`Self::from_digits`] with `radix = 2`.
    ///
    /// # Errors
    ///
    /// See [`Self::from_digits`].
    pub fn from_bin_str(s: &str) -> Result<Self, ValueParseError> {
        Self::from_digits(s.as_bytes(), 2)
    }

    /// The stored limbs, little endian, including any high padding zero
    /// limbs (`Inline` is always [`INLINE_LIMBS`] limbs long).
    fn limbs(&self) -> &[u64] {
        match &self.0 {
            Repr::Inline(limbs) => limbs.as_slice(),
            Repr::Heap(limbs) => limbs.as_ref(),
        }
    }

    /// The stored limbs with high (most significant) zero limbs removed.
    /// Empty for `0`.
    fn trimmed(&self) -> &[u64] {
        let limbs = self.limbs();
        let mut n = limbs.len();
        while n > 0 && limbs[n - 1] == 0 {
            n -= 1;
        }
        &limbs[..n]
    }

    /// Builds a canonical `FeatureValue` from a little endian limb vector,
    /// trimming high zero limbs and choosing `Inline`/`Heap` per the module
    /// invariants. All constructors and operations that can change the
    /// limb count go through this.
    fn from_limb_vec(mut limbs: Vec<u64>) -> Self {
        while limbs.last() == Some(&0) {
            limbs.pop();
        }

        if limbs.len() <= INLINE_LIMBS {
            let mut arr = [0u64; INLINE_LIMBS];
            arr[..limbs.len()].copy_from_slice(&limbs);
            FeatureValue(Repr::Inline(arr))
        } else {
            FeatureValue(Repr::Heap(limbs.into_boxed_slice()))
        }
    }

    /// `self * mul + add` for a small (`u32`) multiplier and addend, used
    /// by [`Self::from_digits`] to accumulate digits.
    fn mul_add_small(&self, mul: u32, add: u32) -> Self {
        let limbs = self.limbs();
        let mut out = Vec::with_capacity(limbs.len() + 1);
        let mut carry = u128::from(add);

        for &limb in limbs {
            let v = u128::from(limb) * u128::from(mul) + carry;
            out.push(v as u64);
            carry = v >> 64;
        }
        if carry > 0 {
            out.push(carry as u64);
        }

        Self::from_limb_vec(out)
    }

    /// Returns `true` if the value is `0`.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.trimmed().is_empty()
    }

    /// Returns `true` if the value is `1`.
    #[must_use]
    pub fn is_one(&self) -> bool {
        self.trimmed() == [1]
    }

    /// The number of bits needed to hold the value: `0` for `0`, otherwise
    /// one more than the index of the highest set bit.
    #[must_use]
    pub fn bit_len(&self) -> u32 {
        let trimmed = self.trimmed();
        match trimmed.last() {
            None => 0,
            Some(&top) => (trimmed.len() as u32 - 1) * 64 + (64 - top.leading_zeros()),
        }
    }

    /// Returns bit `i` (bit `0` is the least significant bit). `false` for
    /// any `i` at or beyond [`Self::bit_len`].
    #[must_use]
    pub fn bit(&self, i: u32) -> bool {
        let idx = (i / 64) as usize;
        let off = i % 64;
        let limbs = self.limbs();
        idx < limbs.len() && (limbs[idx] >> off) & 1 == 1
    }

    /// Returns `true` if the value is strictly less than `2^width`, i.e. it
    /// fits in a `FeatureAddress` of the given bit width.
    #[must_use]
    pub fn fits_in_bits(&self, width: u32) -> bool {
        self.bit_len() <= width
    }

    /// Returns the value as a `u64` if it fits (i.e. `bit_len() <= 64`),
    /// `None` otherwise.
    #[must_use]
    pub fn to_u64(&self) -> Option<u64> {
        if self.bit_len() <= 64 {
            Some(self.limbs()[0])
        } else {
            None
        }
    }

    /// Iterates over the indexes of the set bits, ascending.
    pub fn iter_set_bits(&self) -> impl Iterator<Item = u32> + '_ {
        self.limbs().iter().enumerate().flat_map(|(i, &limb)| {
            #[allow(clippy::cast_possible_truncation)]
            let base = (i as u32) * 64;
            LimbBits { limb, base }
        })
    }

    /// Returns `self >> n` (an arithmetic-free, logical right shift; the
    /// low `n` bits are discarded).
    #[must_use]
    pub fn shr(&self, n: u32) -> Self {
        if n == 0 || self.is_zero() {
            return self.clone();
        }

        let limbs = self.limbs();
        let limb_shift = (n / 64) as usize;
        let bit_shift = n % 64;

        if limb_shift >= limbs.len() {
            return FeatureValue::default();
        }

        let mut out = Vec::with_capacity(limbs.len() - limb_shift);
        for i in limb_shift..limbs.len() {
            let mut v = limbs[i] >> bit_shift;
            if bit_shift > 0 {
                if let Some(&next) = limbs.get(i + 1) {
                    v |= next << (64 - bit_shift);
                }
            }
            out.push(v);
        }

        Self::from_limb_vec(out)
    }

    /// Returns `self << n`.
    ///
    /// Allocates `O(n)` bits (roughly `n / 8` bytes, on the heap once that
    /// exceeds [`INLINE_BITS`]): there is no upper bound on `n` for the
    /// caller to accidentally exceed, so a caller building a value from
    /// untrusted or unchecked input (e.g. a `FeatureAddress` bit index
    /// straight from parsed text) should bound `n` itself first rather
    /// than relying on this to fail fast.
    #[must_use]
    pub fn shl(&self, n: u32) -> Self {
        if n == 0 || self.is_zero() {
            return self.clone();
        }

        let limbs = self.limbs();
        let limb_shift = (n / 64) as usize;
        let bit_shift = n % 64;

        let mut out = vec![0u64; limbs.len() + limb_shift + 1];
        for (i, &limb) in limbs.iter().enumerate() {
            if bit_shift == 0 {
                out[i + limb_shift] |= limb;
            } else {
                out[i + limb_shift] |= limb << bit_shift;
                out[i + limb_shift + 1] |= limb >> (64 - bit_shift);
            }
        }

        Self::from_limb_vec(out)
    }

    /// Returns the value with only its lowest `width` bits kept (bits at or
    /// above `width` are cleared).
    #[must_use]
    pub fn mask(&self, width: u32) -> Self {
        if width == 0 {
            return FeatureValue::default();
        }

        let full_limbs = (width / 64) as usize;
        let rem = width % 64;
        let limbs = self.limbs();

        let mut out: Vec<u64> = limbs.iter().take(full_limbs).copied().collect();
        if rem > 0 {
            if let Some(&extra) = limbs.get(full_limbs) {
                out.push(extra & ((1u64 << rem) - 1));
            }
        }

        Self::from_limb_vec(out)
    }

    /// Sets bit `i` (bit `0` is the least significant bit), growing the
    /// value if needed.
    pub fn set_bit(&mut self, i: u32) {
        let idx = (i / 64) as usize;
        let off = i % 64;

        match &mut self.0 {
            Repr::Inline(limbs) if idx < limbs.len() => {
                limbs[idx] |= 1u64 << off;
                return;
            }
            Repr::Heap(limbs) if idx < limbs.len() => {
                limbs[idx] |= 1u64 << off;
                return;
            }
            _ => {}
        }

        let mut limbs = self.limbs().to_vec();
        if limbs.len() <= idx {
            limbs.resize(idx + 1, 0);
        }
        limbs[idx] |= 1u64 << off;
        *self = Self::from_limb_vec(limbs);
    }

    /// Returns the bitwise OR of `self` and `other`.
    #[must_use]
    pub fn bitor(&self, other: &Self) -> Self {
        let a = self.limbs();
        let b = other.limbs();
        let n = a.len().max(b.len());

        let mut out = vec![0u64; n];
        for (i, limb) in out.iter_mut().enumerate() {
            *limb = a.get(i).copied().unwrap_or(0) | b.get(i).copied().unwrap_or(0);
        }

        Self::from_limb_vec(out)
    }

    /// Formats the value in the given `radix` (2, 8, 10 or 16), with no
    /// leading zeros and `"0"` for zero: exactly what Python's
    /// `'{:b}'`/`'{:o}'`/`'{}'`/`'{:X}'`/`'{:x}'` produce for a
    /// non-negative `int` (no `0x`/`0o`/`0b` prefix).
    ///
    /// `uppercase` selects `A-F` vs `a-f` for hexadecimal; it has no effect
    /// for the other radixes.
    ///
    /// # Panics
    ///
    /// Panics if `radix` is not one of 2, 8, 10, 16.
    #[must_use]
    pub fn to_radix_string(&self, radix: u32, uppercase: bool) -> String {
        assert!(
            matches!(radix, 2 | 8 | 10 | 16),
            "unsupported radix {radix} (must be 2, 8, 10 or 16)"
        );

        if self.is_zero() {
            return "0".to_string();
        }

        if radix == 10 {
            self.to_decimal_string()
        } else {
            self.to_pow2_radix_string(radix, uppercase)
        }
    }

    /// `to_radix_string` for `radix` a power of two (2, 8, 16). Requires
    /// `self` to be non-zero.
    fn to_pow2_radix_string(&self, radix: u32, uppercase: bool) -> String {
        let bits_per_digit = radix.trailing_zeros();
        let bit_len = self.bit_len();
        let num_digits = bit_len.div_ceil(bits_per_digit);
        let digits = if uppercase {
            UPPER_DIGITS
        } else {
            LOWER_DIGITS
        };

        let mut s = String::with_capacity(num_digits as usize);
        for d in (0..num_digits).rev() {
            let start = d * bits_per_digit;
            let mut v: u32 = 0;
            for b in 0..bits_per_digit {
                if self.bit(start + b) {
                    v |= 1 << b;
                }
            }
            s.push(digits[v as usize] as char);
        }
        s
    }

    /// `to_radix_string` for `radix = 10`. Requires `self` to be non-zero.
    fn to_decimal_string(&self) -> String {
        let mut limbs = self.trimmed().to_vec();

        let mut chunks = Vec::new();
        while !limbs.is_empty() {
            chunks.push(divmod_small(&mut limbs, DECIMAL_CHUNK));
        }

        // `chunks` is least significant chunk first; the last chunk (most
        // significant) is printed without padding, the rest zero padded to
        // `DECIMAL_CHUNK_DIGITS` digits.
        let most_significant = chunks.len() - 1;
        let mut s = chunks[most_significant].to_string();
        for chunk in chunks[..most_significant].iter().rev() {
            s.push_str(&format!("{chunk:0width$}", width = DECIMAL_CHUNK_DIGITS));
        }
        s
    }
}

/// Divides the big integer held (little endian) in `limbs` by the small
/// `divisor` in place, returning the remainder. Trims high zero limbs
/// afterwards.
fn divmod_small(limbs: &mut Vec<u64>, divisor: u64) -> u64 {
    let mut rem: u128 = 0;
    for limb in limbs.iter_mut().rev() {
        let cur = (rem << 64) | u128::from(*limb);
        *limb = (cur / u128::from(divisor)) as u64;
        rem = cur % u128::from(divisor);
    }
    while limbs.last() == Some(&0) {
        limbs.pop();
    }
    rem as u64
}

/// Iterator over the set bit indexes of a single limb, used by
/// [`FeatureValue::iter_set_bits`].
struct LimbBits {
    limb: u64,
    base: u32,
}

impl Iterator for LimbBits {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        if self.limb == 0 {
            return None;
        }
        let tz = self.limb.trailing_zeros();
        self.limb &= self.limb - 1; // clear the lowest set bit
        Some(self.base + tz)
    }
}

impl fmt::Debug for FeatureValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FeatureValue(0x{})", self.to_radix_string(16, true))
    }
}

/// Decimal formatting, matching Python's `'{}'.format(value)`.
impl fmt::Display for FeatureValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_radix_string(10, false))
    }
}

impl PartialOrd for FeatureValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Numeric ordering (hand written: see the module docs for why this is not
/// derived).
impl Ord for FeatureValue {
    fn cmp(&self, other: &Self) -> Ordering {
        let a = self.trimmed();
        let b = other.trimmed();

        match a.len().cmp(&b.len()) {
            Ordering::Equal => {
                for i in (0..a.len()).rev() {
                    match a[i].cmp(&b[i]) {
                        Ordering::Equal => continue,
                        ord => return ord,
                    }
                }
                Ordering::Equal
            }
            ord => ord,
        }
    }
}

impl From<u64> for FeatureValue {
    fn from(v: u64) -> Self {
        Self::from_u64(v)
    }
}

impl From<u128> for FeatureValue {
    fn from(v: u128) -> Self {
        Self::from_u128(v)
    }
}

impl PartialEq<u64> for FeatureValue {
    fn eq(&self, other: &u64) -> bool {
        self.to_u64() == Some(*other)
    }
}

impl PartialEq<FeatureValue> for u64 {
    fn eq(&self, other: &FeatureValue) -> bool {
        other == self
    }
}

const _: () = assert!(std::mem::size_of::<FeatureValue>() <= 40);

#[cfg(test)]
mod tests;

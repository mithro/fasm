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

//! Bit layout of the 8 byte handle.
//!
//! ```text
//!  63            40 39           20 19            0
//! +----------------+---------------+---------------+
//! |  level 0 (24)  |  level 1 (20) |  level 2 (20) |   hierarchical form
//! +----------------+---------------+---------------+
//! |   0xFF_FFFF    |    overflow table index (40)  |   overflow form
//! +----------------+-------------------------------+
//! ```
//!
//! Level fields hold `entry number + 1`; `0` means the level is absent.
//! Level 0 is always present, so the value is never zero.

use std::num::{NonZeroU32, NonZeroU64};

/// Number of levels a string is split into (the last one holds the
/// remainder of the string, dots included).
pub(crate) const LEVELS: usize = 3;

/// Bit position of each level field.
const LEVEL_SHIFT: [u32; LEVELS] = [40, 20, 0];

/// Mask of each level field (after shifting it down).
const LEVEL_MASK: [u64; LEVELS] = [(1 << 24) - 1, (1 << 20) - 1, (1 << 20) - 1];

/// Value of the level 0 field that marks the overflow form.
const OVERFLOW_MARK: u64 = (1 << 24) - 1;

/// Mask of the overflow table index.
const OVERFLOW_INDEX_MASK: u64 = (1 << 40) - 1;

/// Maximum number of entries of each level table: every field value except
/// `0` (absent) and, for level 0, [`OVERFLOW_MARK`].
pub(crate) const LEVEL_LIMITS: [u32; LEVELS] = [(1 << 24) - 2, (1 << 20) - 1, (1 << 20) - 1];

/// Maximum number of entries of the overflow table (bounded by the `u32`
/// entry numbers of the table implementation, not by the 40 bit field).
pub(crate) const OVERFLOW_LIMIT: u32 = u32::MAX;

/// The overflow form with index 0.
const OVERFLOW_BASE: NonZeroU64 = match NonZeroU64::new(OVERFLOW_MARK << LEVEL_SHIFT[0]) {
    Some(v) => v,
    None => panic!("overflow mark must be non zero"),
};

/// `2^40`, the multiplier that moves a value into the level 0 field.
const LEVEL0_UNIT: NonZeroU64 = match NonZeroU64::new(1 << LEVEL_SHIFT[0]) {
    Some(v) => v,
    None => panic!("level 0 unit must be non zero"),
};

/// A decoded handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Repr {
    /// Level table entry numbers plus one, `0` for absent levels.
    Levels([u32; LEVELS]),
    /// Entry number in the overflow table.
    Overflow(u32),
}

/// Encodes a hierarchical handle from the level 0 entry number and the
/// fields (`entry number + 1`, or `0` when absent) of levels 1 and 2.
///
/// The caller guarantees `pos0 < LEVEL_LIMITS[0]` and `fields[i] <=
/// LEVEL_LIMITS[i]`.
pub(crate) fn encode_levels(pos0: u32, fields: [u32; LEVELS - 1]) -> NonZeroU64 {
    debug_assert!(pos0 < LEVEL_LIMITS[0]);
    let field0 = NonZeroU64::from(NonZeroU32::MIN.saturating_add(pos0));
    // field0 < 2^24, so this product never saturates; it is non zero by
    // construction, which avoids any fallible conversion.
    let mut raw = field0.saturating_mul(LEVEL0_UNIT);
    for (level, &field) in fields.iter().enumerate() {
        debug_assert!(field <= LEVEL_LIMITS[level + 1]);
        raw |= u64::from(field) << LEVEL_SHIFT[level + 1];
    }
    raw
}

/// Encodes an overflow handle for entry `pos` of the overflow table.
pub(crate) fn encode_overflow(pos: u32) -> NonZeroU64 {
    OVERFLOW_BASE | u64::from(pos)
}

/// Decodes a handle.
pub(crate) fn decode(raw: NonZeroU64) -> Repr {
    let raw = raw.get();
    let field0 = (raw >> LEVEL_SHIFT[0]) & LEVEL_MASK[0];
    if field0 == OVERFLOW_MARK {
        // Only values produced by `encode_overflow` (< 2^32) are stored.
        return Repr::Overflow((raw & OVERFLOW_INDEX_MASK) as u32);
    }
    let mut fields = [0u32; LEVELS];
    for (level, field) in fields.iter_mut().enumerate() {
        *field = ((raw >> LEVEL_SHIFT[level]) & LEVEL_MASK[level]) as u32;
    }
    Repr::Levels(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_round_trip() {
        let cases = [
            (0, [0, 0]),
            (0, [1, 0]),
            (5, [7, 9]),
            (LEVEL_LIMITS[0] - 1, [LEVEL_LIMITS[1], LEVEL_LIMITS[2]]),
        ];
        for (pos0, fields) in cases {
            let raw = encode_levels(pos0, fields);
            assert_eq!(decode(raw), Repr::Levels([pos0 + 1, fields[0], fields[1]]));
        }
        assert_eq!(encode_levels(0, [0, 0]).get(), 1 << 40);
    }

    #[test]
    fn overflow_round_trip() {
        for pos in [0, 1, 12345, u32::MAX - 1, u32::MAX] {
            let raw = encode_overflow(pos);
            assert_eq!(decode(raw), Repr::Overflow(pos));
        }
    }

    #[test]
    fn forms_do_not_collide() {
        let top_level = encode_levels(LEVEL_LIMITS[0] - 1, [LEVEL_LIMITS[1], LEVEL_LIMITS[2]]);
        assert!(top_level < encode_overflow(0));
    }
}

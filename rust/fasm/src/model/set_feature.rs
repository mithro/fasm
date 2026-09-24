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

//! [`SetFasmFeature`]: the Rust equivalent of Python's `SetFasmFeature`
//! namedtuple.

use crate::idstring::IdString;

use super::error::ModelError;
use super::feature_value::FeatureValue;
use super::value_format::ValueFormat;

/// A single `feature[end:start] = value` assignment, e.g.
/// `CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[63:32] = 32'b1111...`.
///
/// Mirrors Python's `fasm.model.SetFasmFeature` namedtuple field for field:
///
/// * `feature`: the feature name.
/// * `start`/`end`: the `FeatureAddress`, or `None`/`None` when the address
///   was omitted (address `0`, width `1`).
/// * `value`: the value; `1` when `FeatureValue` was omitted.
/// * `value_format`: how to print `value`, or `None` when the value (and
///   any `=`) should be omitted from output (only valid when `value == 1`).
///
/// Addresses are `u32` here: Python's `int` (and the ANTLR grammar's
/// `DecimalValue`) are unbounded, but the reference ANTLR C++ parser
/// truncates addresses to 32 bits in practice; anything above `u32::MAX` is
/// therefore a parse error in the Rust parser (T1.3), to be recorded in
/// `docs/rewrite/COMPAT.md`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct SetFasmFeature {
    /// Feature name, e.g. `feature`.
    pub feature: IdString,
    /// Starting bit of the feature range (`FeatureAddress` low bound), or
    /// `None` when no `FeatureAddress` was given.
    pub start: Option<u32>,
    /// Ending bit of the feature range (`FeatureAddress` high bound), or
    /// `None` when the address was a single bit (or omitted).
    pub end: Option<u32>,
    /// The value being assigned.
    pub value: FeatureValue,
    /// How `value` should be printed, or `None` if it (and the value) are
    /// implicit and should be omitted from output.
    pub value_format: Option<ValueFormat>,
}

impl SetFasmFeature {
    /// Builds a `SetFasmFeature`, validating the address and that `value`
    /// fits in the address width (mirroring the asserts in Python's
    /// `fasm.output.set_feature_width`/`set_feature_to_str`).
    ///
    /// # Errors
    ///
    /// * [`ModelError::EndWithoutStart`] if `end.is_some()` but
    ///   `start.is_none()`.
    /// * [`ModelError::EndBeforeStart`] if `end < start`.
    /// * [`ModelError::ValueTooWide`] if `value` needs more bits than the
    ///   address width (see [`Self::width`]) allows.
    pub fn new(
        feature: IdString,
        start: Option<u32>,
        end: Option<u32>,
        value: FeatureValue,
        value_format: Option<ValueFormat>,
    ) -> Result<Self, ModelError> {
        if end.is_some() && start.is_none() {
            return Err(ModelError::EndWithoutStart);
        }

        if let (Some(s), Some(e)) = (start, end) {
            if e < s {
                return Err(ModelError::EndBeforeStart { start: s, end: e });
            }
        }

        let width = match end {
            None => 1,
            Some(e) => e - start.expect("checked above") + 1,
        };

        if !value.fits_in_bits(width) {
            return Err(ModelError::ValueTooWide {
                width,
                bit_len: value.bit_len(),
            });
        }

        Ok(SetFasmFeature {
            feature,
            start,
            end,
            value,
            value_format,
        })
    }

    /// Builds a `SetFasmFeature` without validating `start`/`end`/`value`.
    ///
    /// For the parser's (T1.3) hot path, where the grammar and value width
    /// checks have already been applied while parsing. Prefer
    /// [`Self::new`] anywhere the inputs have not already been validated;
    /// [`Self::width`] and other code may assume the invariants
    /// [`Self::new`] checks hold.
    #[must_use]
    pub fn new_unchecked(
        feature: IdString,
        start: Option<u32>,
        end: Option<u32>,
        value: FeatureValue,
        value_format: Option<ValueFormat>,
    ) -> Self {
        SetFasmFeature {
            feature,
            start,
            end,
            value,
            value_format,
        }
    }

    /// The bit width of the `FeatureAddress`: `1` if `end` is `None`,
    /// otherwise `end - start + 1`.
    ///
    /// Mirrors Python's `fasm.output.set_feature_width`. Assumes the
    /// `SetFasmFeature` invariants (`end.is_some()` implies
    /// `start.is_some()` and `end >= start`) hold, which [`Self::new`]
    /// checks and [`Self::new_unchecked`] does not.
    ///
    /// # Panics
    ///
    /// Panics if `end.is_some()` but `start.is_none()` (an invariant
    /// violation only reachable via [`Self::new_unchecked`]).
    #[must_use]
    pub fn width(&self) -> u32 {
        match self.end {
            None => 1,
            Some(end) => {
                let start = self
                    .start
                    .expect("SetFasmFeature invariant violated: end without start");
                end - start + 1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature() -> IdString {
        IdString::new("set_feature_tests.SOME_FEATURE")
    }

    #[test]
    fn width_no_address() {
        let f =
            SetFasmFeature::new(feature(), None, None, FeatureValue::from_u64(1), None).unwrap();
        assert_eq!(f.width(), 1);
    }

    #[test]
    fn width_single_bit_address() {
        let f =
            SetFasmFeature::new(feature(), Some(5), None, FeatureValue::from_u64(1), None).unwrap();
        assert_eq!(f.width(), 1);
    }

    #[test]
    fn width_range_address() {
        let f = SetFasmFeature::new(
            feature(),
            Some(0),
            Some(31),
            FeatureValue::from_u64(0),
            None,
        )
        .unwrap();
        assert_eq!(f.width(), 32);

        let f = SetFasmFeature::new(
            feature(),
            Some(32),
            Some(63),
            FeatureValue::from_u64(0),
            None,
        )
        .unwrap();
        assert_eq!(f.width(), 32);
    }

    #[test]
    fn new_rejects_end_without_start() {
        assert_eq!(
            SetFasmFeature::new(feature(), None, Some(3), FeatureValue::from_u64(0), None),
            Err(ModelError::EndWithoutStart)
        );
    }

    #[test]
    fn new_rejects_end_before_start() {
        assert_eq!(
            SetFasmFeature::new(feature(), Some(5), Some(3), FeatureValue::from_u64(0), None),
            Err(ModelError::EndBeforeStart { start: 5, end: 3 })
        );
    }

    #[test]
    fn new_accepts_end_equal_to_start() {
        let f = SetFasmFeature::new(feature(), Some(5), Some(5), FeatureValue::from_u64(1), None)
            .unwrap();
        assert_eq!(f.width(), 1);
    }

    #[test]
    fn new_rejects_value_too_wide_for_single_bit() {
        assert_eq!(
            SetFasmFeature::new(feature(), Some(0), None, FeatureValue::from_u64(2), None),
            Err(ModelError::ValueTooWide {
                width: 1,
                bit_len: 2
            })
        );
    }

    #[test]
    fn new_rejects_value_too_wide_for_range() {
        // [3:0] is 4 bits wide; 16 needs 5 bits.
        assert_eq!(
            SetFasmFeature::new(
                feature(),
                Some(0),
                Some(3),
                FeatureValue::from_u64(16),
                None
            ),
            Err(ModelError::ValueTooWide {
                width: 4,
                bit_len: 5
            })
        );
    }

    #[test]
    fn new_accepts_max_value_for_width() {
        // [3:0] is 4 bits wide; 15 (0b1111) just fits.
        let f = SetFasmFeature::new(
            feature(),
            Some(0),
            Some(3),
            FeatureValue::from_u64(15),
            None,
        );
        assert!(f.is_ok());
    }

    #[test]
    fn new_unchecked_skips_validation() {
        // Deliberately invalid (value too wide); new_unchecked must not
        // panic or validate.
        let f = SetFasmFeature::new_unchecked(
            feature(),
            Some(0),
            None,
            FeatureValue::from_u64(2),
            None,
        );
        assert_eq!(f.value, FeatureValue::from_u64(2));
    }

    #[test]
    fn equal_features_are_equal() {
        let a =
            SetFasmFeature::new(feature(), Some(0), None, FeatureValue::from_u64(1), None).unwrap();
        let b =
            SetFasmFeature::new(feature(), Some(0), None, FeatureValue::from_u64(1), None).unwrap();
        assert_eq!(a, b);
    }
}

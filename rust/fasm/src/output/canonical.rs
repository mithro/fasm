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

//! [`canonical_features`] / [`try_canonical_features`]: the Rust
//! equivalent of Python's `canonical_features` generator in
//! `fasm/__init__.py`.

use super::super::model::{FeatureValue, SetFasmFeature};
use super::error::OutputError;

/// Expands `set_feature` into its canonical, single-bit `SetFasmFeature`s
/// (width 1, value 1, no `value_format`), skipping any bit that is not set.
/// A `set_feature` whose value is `0` yields nothing.
///
/// Mirrors Python's `canonical_features` generator in `fasm/__init__.py`:
///
/// * no address (`start` and `end` both `None`): yields the feature with no
///   address (the value must be 1; see below).
/// * a single bit address (`start` given, `end` `None`): yields the feature
///   with no address if `start == 0`, otherwise with `[start]`.
/// * a range address: yields one feature per set bit in `[start, end]`, as
///   `[address]` (or no address for `address == 0`).
///
/// # Errors
///
/// The Python generator's `assert`s (the value must be 1 whenever the
/// width is 1, i.e. whenever there is no `end`; `end` is never given
/// without a `start`; `end >= start`) all follow from the
/// `SetFasmFeature` invariants [`super::super::model::SetFasmFeature::new`]
/// enforces, so they cannot fail for a `set_feature` built that way (or by
/// the parser, T1.3). They are only reachable for one built with
/// [`super::super::model::SetFasmFeature::new_unchecked`] from
/// inconsistent inputs, in which case this returns
/// [`OutputError::CanonicalEndWithoutStart`],
/// [`OutputError::CanonicalValueNotOne`] or
/// [`OutputError::CanonicalEndBeforeStart`] instead of panicking (unlike
/// the Python `assert`, which raises `AssertionError`).
pub fn try_canonical_features(
    set_feature: &SetFasmFeature,
) -> Result<Vec<SetFasmFeature>, OutputError> {
    let mut out = Vec::new();

    if set_feature.value.is_zero() {
        return Ok(out);
    }

    match (set_feature.start, set_feature.end) {
        (None, end) => {
            if end.is_some() {
                return Err(OutputError::CanonicalEndWithoutStart);
            }
            if !set_feature.value.is_one() {
                return Err(OutputError::CanonicalValueNotOne);
            }
            out.push(bare_feature(set_feature));
        }
        (Some(start), None) => {
            if !set_feature.value.is_one() {
                return Err(OutputError::CanonicalValueNotOne);
            }
            if start == 0 {
                out.push(bare_feature(set_feature));
            } else {
                out.push(single_bit_feature(set_feature, start));
            }
        }
        (Some(start), Some(end)) => {
            if end < start {
                return Err(OutputError::CanonicalEndBeforeStart { start, end });
            }
            for address in start..=end {
                if set_feature.value.bit(address - start) {
                    if address == 0 {
                        out.push(bare_feature(set_feature));
                    } else {
                        out.push(single_bit_feature(set_feature, address));
                    }
                }
            }
        }
    }

    Ok(out)
}

/// A canonical `SetFasmFeature` with no address (value 1, no
/// `value_format`).
fn bare_feature(set_feature: &SetFasmFeature) -> SetFasmFeature {
    SetFasmFeature::new_unchecked(
        set_feature.feature,
        None,
        None,
        FeatureValue::from_u64(1),
        None,
    )
}

/// A canonical `SetFasmFeature` with a single-bit address (value 1, no
/// `value_format`).
fn single_bit_feature(set_feature: &SetFasmFeature, address: u32) -> SetFasmFeature {
    SetFasmFeature::new_unchecked(
        set_feature.feature,
        Some(address),
        None,
        FeatureValue::from_u64(1),
        None,
    )
}

/// Infallible counterpart of [`try_canonical_features`] for a `set_feature`
/// already known to uphold the `SetFasmFeature` invariants (i.e. built with
/// [`super::super::model::SetFasmFeature::new`], or produced by the parser,
/// T1.3): the common case in [`super::fasm_line_to_string`].
///
/// Note this collects eagerly into a `Vec` rather than truly streaming like
/// the Python generator (see `docs/rewrite/DESIGN-output.md`): every real
/// FASM feature is at most a few hundred bits wide (256 for the widest
/// known case, a BRAM `INIT`), so this is a small, bounded allocation in
/// practice; a `set_feature` with a deliberately huge range (up to
/// `u32::MAX` bits, which nothing in the grammar forbids) would allocate
/// and iterate proportionally to its width.
///
/// # Panics
///
/// Panics (via [`try_canonical_features`]'s documented error conditions)
/// if `set_feature` violates the `SetFasmFeature` invariants; only
/// reachable via `new_unchecked`.
pub fn canonical_features(set_feature: &SetFasmFeature) -> impl Iterator<Item = SetFasmFeature> {
    try_canonical_features(set_feature)
        .expect("canonical_features: invalid SetFasmFeature (see try_canonical_features)")
        .into_iter()
}

#[cfg(test)]
mod tests;

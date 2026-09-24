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

//! [`OutputError`]: everything the `output` module returns instead of the
//! `assert`s scattered across Python's `fasm/__init__.py` and
//! `fasm/output.py`.
//!
//! Every Python `assert` this module mirrors (see `docs/rewrite/DESIGN-output.md`)
//! is only ever false for a [`super::super::SetFasmFeature`] that violates
//! the invariants [`super::super::SetFasmFeature::new`] enforces, i.e. one
//! built with [`super::super::SetFasmFeature::new_unchecked`] from bad
//! inputs. A `SetFasmFeature` produced by `new` (or by the parser, T1.3)
//! never triggers any of these.

use std::fmt;

use super::super::model::ModelError;

/// Errors returned by the `output` module.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputError {
    /// [`super::set_feature_to_str`] / [`super::write_set_feature`]: the
    /// feature's value needs more bits than its `FeatureAddress` width
    /// allows. Mirrors the Python `assert set_feature.value < 2**width` in
    /// `fasm/__init__.py`'s `set_feature_to_str`.
    ValueTooWideForFeature {
        /// The `FeatureAddress` width in bits.
        width: u32,
        /// The number of bits the value actually needs.
        bit_len: u32,
    },
    /// `check_if_canonical = true` and the feature's width is not 1.
    NotCanonicalWidth {
        /// The feature's actual width.
        width: u32,
    },
    /// `check_if_canonical = true` and the feature has an `end` address.
    NotCanonicalHasEnd,
    /// `check_if_canonical = true` and the feature's `start` is `Some(0)`
    /// (canonical single-bit features either have no address or a nonzero
    /// one).
    NotCanonicalStartZero,
    /// `check_if_canonical = true` and the feature has a `value_format`
    /// (canonical features never print `= value`).
    NotCanonicalHasValueFormat,
    /// A [`super::super::SetFasmFeature`] has `end.is_some()` but
    /// `start.is_none()`, which [`super::super::SetFasmFeature::new`] never
    /// produces (only reachable via `new_unchecked`). Used by
    /// [`super::canonical_features`] / [`super::try_canonical_features`]
    /// and by [`super::write_set_feature`] / [`super::set_feature_to_str`]
    /// (both would otherwise reach this through
    /// [`super::super::SetFasmFeature::width`], which panics on it).
    EndWithoutStart,
    /// [`super::canonical_features`] / [`super::try_canonical_features`]:
    /// the feature has no `end` (a single implicit or explicit bit) but a
    /// value other than 1 (only reachable for a feature whose width is not
    /// really 1, i.e. built with `new_unchecked`).
    CanonicalValueNotOne,
    /// A [`super::super::SetFasmFeature`]'s `end` is before its `start`
    /// (`new` never produces this; only reachable via `new_unchecked`).
    /// Used by [`super::canonical_features`] /
    /// [`super::try_canonical_features`] and by
    /// [`super::write_set_feature`] / [`super::set_feature_to_str`] (both
    /// would otherwise reach this through
    /// [`super::super::SetFasmFeature::width`], which panics on it).
    EndBeforeStart {
        /// The given start.
        start: u32,
        /// The given end, smaller than `start`.
        end: u32,
    },
    /// A [`super::super::SetFasmFeature`]'s `FeatureAddress` range (`end -
    /// start + 1`) does not fit in a `u32` (only possible for `start == 0`
    /// and `end == u32::MAX`; only reachable via `new_unchecked`, since
    /// [`super::super::SetFasmFeature::new`] rejects it). Used by
    /// [`super::write_set_feature`] / [`super::set_feature_to_str`], which
    /// would otherwise reach this through
    /// [`super::super::SetFasmFeature::width`], which panics on it.
    AddressRangeTooWide {
        /// The given start.
        start: u32,
        /// The given end.
        end: u32,
    },
    /// [`super::merge_features`]: `features` was empty, or its entries do
    /// not all share the same [`super::super::model::SetFasmFeature::feature`].
    /// Mirrors the Python `assert len(set(feature.feature for feature in
    /// features)) == 1` in `fasm/output.py`'s `merge_features` (which also
    /// fails for an empty list, since `len(set()) == 0 != 1`).
    MergeFeaturesNotSingleFeature,
    /// [`super::merge_features`]: a feature has `end.is_some()` but
    /// `start.is_none()`, which [`super::super::SetFasmFeature::new`]
    /// never produces.
    MergeFeaturesEndWithoutStart,
    /// [`super::merge_features`]: the same bit was both set by one feature
    /// and cleared by another. Mirrors the Python `assert bit not in
    /// cleared_bits` / `assert bit not in set_bits` in `merge_features`.
    MergeFeaturesConflictingBit {
        /// The conflicting bit index.
        bit: u32,
    },
    /// Building the merged/canonical [`super::super::model::SetFasmFeature`]
    /// with [`super::super::model::SetFasmFeature::new`] failed (only
    /// reachable for a bit index of `u32::MAX`, which no real FASM file
    /// comes close to).
    Model(ModelError),
    /// Writing to the caller supplied [`std::fmt::Write`] sink failed.
    Fmt,
}

impl From<ModelError> for OutputError {
    fn from(e: ModelError) -> Self {
        OutputError::Model(e)
    }
}

impl From<fmt::Error> for OutputError {
    fn from(_: fmt::Error) -> Self {
        OutputError::Fmt
    }
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputError::ValueTooWideForFeature { width, bit_len } => write!(
                f,
                "value needs {bit_len} bit(s), which does not fit in the {width}-bit \
                 FeatureAddress"
            ),
            OutputError::NotCanonicalWidth { width } => {
                write!(f, "not canonical: width is {width}, expected 1")
            }
            OutputError::NotCanonicalHasEnd => {
                write!(f, "not canonical: feature has an end address")
            }
            OutputError::NotCanonicalStartZero => {
                write!(f, "not canonical: start address is explicitly 0")
            }
            OutputError::NotCanonicalHasValueFormat => {
                write!(f, "not canonical: feature has a value_format")
            }
            OutputError::EndWithoutStart => {
                write!(f, "invalid SetFasmFeature: end given without a start")
            }
            OutputError::CanonicalValueNotOne => {
                write!(f, "invalid SetFasmFeature: single bit value is not 0 or 1")
            }
            OutputError::EndBeforeStart { start, end } => write!(
                f,
                "invalid SetFasmFeature: end ({end}) is before start ({start})"
            ),
            OutputError::AddressRangeTooWide { start, end } => write!(
                f,
                "invalid SetFasmFeature: [{end}:{start}] is 2^32 bits wide, which does not fit \
                 in a u32 width"
            ),
            OutputError::MergeFeaturesNotSingleFeature => write!(
                f,
                "merge_features requires a non-empty slice of features that all share the \
                 same feature name"
            ),
            OutputError::MergeFeaturesEndWithoutStart => {
                write!(f, "invalid SetFasmFeature: end given without a start")
            }
            OutputError::MergeFeaturesConflictingBit { bit } => write!(
                f,
                "bit {bit} is both set and cleared by different features being merged"
            ),
            OutputError::Model(e) => write!(f, "{e}"),
            OutputError::Fmt => write!(f, "failed to write to output sink"),
        }
    }
}

impl std::error::Error for OutputError {}

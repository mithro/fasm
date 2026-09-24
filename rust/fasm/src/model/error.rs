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

//! Error types for the `model` module.

use std::fmt;

/// An error returned while parsing digits into a [`super::FeatureValue`]
/// (see [`super::FeatureValue::from_digits`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueParseError {
    /// The digit string was empty once `_` separators were removed.
    EmptyDigits,
    /// A byte was not a valid digit for the given radix.
    InvalidDigit {
        /// The offending character.
        digit: char,
        /// The radix it was rejected for.
        radix: u32,
    },
    /// The requested radix is not one of 2, 8, 10 or 16.
    UnsupportedRadix(u32),
}

impl fmt::Display for ValueParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueParseError::EmptyDigits => {
                write!(f, "no digits to parse (empty after removing '_')")
            }
            ValueParseError::InvalidDigit { digit, radix } => {
                write!(f, "{digit:?} is not a valid digit for radix {radix}")
            }
            ValueParseError::UnsupportedRadix(radix) => {
                write!(f, "unsupported radix {radix} (must be 2, 8, 10 or 16)")
            }
        }
    }
}

impl std::error::Error for ValueParseError {}

/// An error returned while constructing or interpreting model types.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModelError {
    /// A [`super::SetFasmFeature`] was given `end` but no `start`.
    EndWithoutStart,
    /// A [`super::SetFasmFeature`]'s `end` is before its `start`.
    EndBeforeStart {
        /// The given start.
        start: u32,
        /// The given end, which was smaller than `start`.
        end: u32,
    },
    /// A [`super::SetFasmFeature`]'s value does not fit in its width.
    ValueTooWide {
        /// The width in bits implied by `start`/`end`.
        width: u32,
        /// The number of bits actually needed to hold the value.
        bit_len: u32,
    },
    /// A byte is not a valid [`super::ValueFormat`] discriminant.
    InvalidValueFormat(u8),
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::EndWithoutStart => {
                write!(f, "FeatureAddress end given without a start")
            }
            ModelError::EndBeforeStart { start, end } => {
                write!(f, "FeatureAddress end ({end}) is before start ({start})")
            }
            ModelError::ValueTooWide { width, bit_len } => write!(
                f,
                "value needs {bit_len} bit(s), which does not fit in the {width}-bit \
                 FeatureAddress"
            ),
            ModelError::InvalidValueFormat(v) => {
                write!(f, "{v} is not a valid ValueFormat discriminant (0-4)")
            }
        }
    }
}

impl std::error::Error for ModelError {}

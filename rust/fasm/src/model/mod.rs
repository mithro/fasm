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

//! The FASM data model: [`ValueFormat`], [`FeatureValue`], [`SetFasmFeature`],
//! [`Annotation`] and [`FasmLine`].
//!
//! These types are produced by the `parser` module (T1.3) and consumed by
//! the `output` module (T1.4); they mirror the namedtuples in Python's
//! `fasm.model` field for field, since the Python bindings (T3.1) convert
//! between the two. Formatting/printing to FASM source text is *not* part
//! of this module (that is `output`, T1.4); the only text producing method
//! here is `#[derive(Debug)]`/a hand written `Debug` impl, for
//! troubleshooting.
//!
//! See `docs/rewrite/DESIGN-model.md` for the design decisions behind this
//! module (in particular [`FeatureValue`]'s representation).

#![forbid(unsafe_code)]

mod annotation;
mod error;
mod feature_value;
mod line;
mod set_feature;
mod value_format;

pub use annotation::Annotation;
pub use error::{ModelError, ValueParseError};
pub use feature_value::{FeatureValue, INLINE_BITS};
pub use line::FasmLine;
pub use set_feature::SetFasmFeature;
pub use value_format::ValueFormat;

#[cfg(test)]
mod tests;

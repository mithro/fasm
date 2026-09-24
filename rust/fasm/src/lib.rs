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

//! Core library for the FASM (FPGA Assembly) file format.
//!
//! This crate is the Rust rewrite of the `fasm` Python package: it will
//! provide (see `docs/rewrite/PLAN.md` for the full architecture)
//!
//! * the [`idstring`] module: a `Copy`, 8-byte interned handle for
//!   hierarchical dotted feature names (following
//!   <https://github.com/mithro/idstring>);
//! * a `model` module: `ValueFormat`, `FeatureValue`, `SetFasmFeature`,
//!   `Annotation` and `FasmLine`, mirroring the Python namedtuples;
//! * a `parser` module: a hand written, byte oriented, zero-copy line
//!   parser matching the ANTLR and textX reference grammars;
//! * an `output` module: string formatting, canonicalisation and
//!   `merge_features` / `merge_and_sort` (`MergeModel`) equivalents.
//!
//! [`idstring`], `model` and `output` exist so far; the `parser` module
//! follows in a later task of `docs/rewrite/TASKS.md`.

pub mod idstring;
pub mod model;
pub mod output;

pub use model::{
    Annotation, FasmLine, FeatureValue, ModelError, SetFasmFeature, ValueFormat, ValueParseError,
};
pub use output::{
    canonical_features, fasm_line_to_string, fasm_tuple_to_string, fasm_value_to_str,
    merge_and_sort, merge_and_sort_by_key, merge_features, set_feature_to_str,
    try_canonical_features, write_fasm_value, write_set_feature, MergeModel, OutputError,
};

/// The version of this crate, taken from `Cargo.toml` at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_not_empty() {
        assert!(!VERSION.is_empty());
    }
}

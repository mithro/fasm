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
//! Only [`idstring`] exists so far; the other modules follow in later tasks
//! of `docs/rewrite/TASKS.md`.

pub mod idstring;

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

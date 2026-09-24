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

//! FASM source text output: string formatting, canonicalisation and
//! feature/line merging.
//!
//! The Rust equivalent of Python's `fasm/__init__.py` (the
//! `fasm_value_to_str`/`set_feature_to_str`/`canonical_features`/
//! `fasm_line_to_string`/`fasm_tuple_to_string` functions) and
//! `fasm/output.py` (`merge_features`, `MergeModel`, `merge_and_sort`).
//!
//! See `docs/rewrite/DESIGN-output.md` for the design decisions behind this
//! module, in particular the deliberately reproduced `MergeModel` grouping
//! quirk (a duplicated group of lines in one specific situation) and why
//! several functions here return `Result` where the Python originals
//! `assert`.

#![forbid(unsafe_code)]

mod canonical;
mod error;
mod format;
mod line;
mod merge;

pub use canonical::{canonical_features, try_canonical_features};
pub use error::OutputError;
pub use format::{fasm_value_to_str, set_feature_to_str, write_fasm_value, write_set_feature};
pub use line::{fasm_line_to_string, fasm_tuple_to_string};
pub use merge::{merge_and_sort, merge_and_sort_by_key, merge_features, MergeModel};

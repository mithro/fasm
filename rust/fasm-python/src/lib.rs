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

//! Python bindings for the `fasm` crate.
//!
//! This crate will become the `fasm._fasm_rs` pyo3 extension module built
//! with maturin, exposing `parse_fasm_filename` / `parse_fasm_string` (and
//! later `fasm_tuple_to_string`, `merge_and_sort`) to `fasm/parser/rust.py`
//! (see task T3.1 and `docs/rewrite/PLAN.md`). pyo3 is intentionally not a
//! dependency yet.
//!
//! No Python bindings exist yet; this crate currently only depends on the
//! `fasm` core crate for the workspace skeleton (task T0.3).

#[cfg(test)]
mod tests {
    #[test]
    fn depends_on_fasm_crate() {
        assert!(!fasm::VERSION.is_empty());
    }
}

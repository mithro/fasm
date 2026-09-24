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

//! T1.6 fuzz target 1: `parse_fasm_bytes` on arbitrary bytes.
//!
//! `parse_fasm_bytes` must never panic on any input (valid UTF-8 or not,
//! valid FASM or not) and must return in bounded time/memory (T1.3's
//! `huge_values_are_fast_and_errors_short` already covers the "huge
//! decimal literal" class of this on purpose-built inputs; this target
//! covers everything else libFuzzer's mutations can reach). No
//! correctness oracle here beyond "does not panic, does not hang,
//! finishes" — `tools/difftest.py` (T1.5) is what checks the *result* of
//! a successful parse against the Python oracle.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = fasm::parse_fasm_bytes(data);
});

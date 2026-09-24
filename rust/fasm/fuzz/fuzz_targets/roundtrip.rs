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

//! T1.6 fuzz target 2: parse -> `fasm_tuple_to_string` -> parse round trip,
//! both plain and canonical.
//!
//! For every input that parses (`parse_fasm_bytes` succeeds):
//!
//! * `fasm_tuple_to_string` on the parsed model must not error (a parser
//!   produced model always satisfies the `SetFasmFeature` invariants, so
//!   this should be infallible in practice; a panic or an unexpected
//!   `Err` here is a real bug).
//! * The rendered text must itself parse back successfully (our own
//!   output must always be valid FASM source).
//! * Rendering is idempotent: rendering the re-parsed model produces
//!   byte-identical text to the first rendering. This is the practical
//!   form of "parse -> print -> parse gives the same model": two models
//!   that always print identically are indistinguishable through the
//!   public string API, which is what every real caller (the CLI,
//!   `tools/difftest.py`) actually observes, and it is robust to any
//!   internal representation detail that does not affect output (unlike
//!   a raw `FasmLine` equality check, which would make this target
//!   trivially find "divergences" for representation-only differences
//!   that were never a difftest failure).
//! * In canonical mode specifically, every re-parsed line is checked to
//!   actually be in canonical form (single bit, value 1, no
//!   `value_format`) — i.e. the parse of canonical output is the
//!   canonical expansion, not just "some text that happens to re-render
//!   the same".
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(model) = fasm::parse_fasm_bytes(data) else {
        return;
    };

    for canonical in [false, true] {
        let text1 = fasm::fasm_tuple_to_string(&model, canonical).unwrap_or_else(|e| {
            panic!(
                "fasm_tuple_to_string errored on a parser-produced model (canonical={canonical}): {e}"
            )
        });

        let reparsed = fasm::parse_fasm_bytes(text1.as_bytes()).unwrap_or_else(|e| {
            panic!(
                "re-parsing our own rendered output failed (canonical={canonical}): {e}\n\
                 --- rendered text ---\n{text1}"
            )
        });

        if canonical {
            for line in &reparsed {
                let Some(set_feature) = &line.set_feature else {
                    continue;
                };
                assert!(
                    set_feature.end.is_none(),
                    "canonical output re-parsed to a line with an address range (not canonical): \
                     {text1}"
                );
                assert!(
                    set_feature.value.is_one(),
                    "canonical output re-parsed to a line whose value is not 1: {text1}"
                );
                assert!(
                    set_feature.value_format.is_none(),
                    "canonical output re-parsed to a line with a value_format: {text1}"
                );
            }
        }

        let text2 = fasm::fasm_tuple_to_string(&reparsed, canonical).unwrap_or_else(|e| {
            panic!(
                "fasm_tuple_to_string errored on the re-parsed model (canonical={canonical}): {e}"
            )
        });

        assert_eq!(
            text1, text2,
            "round trip is not idempotent (canonical={canonical}): first render {text1:?}, \
             render of the re-parse {text2:?}"
        );
    }
});

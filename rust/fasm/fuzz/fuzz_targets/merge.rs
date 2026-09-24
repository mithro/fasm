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

//! T1.6 fuzz target 3: `merge_and_sort` on arbitrary parsed models.
//!
//! `merge_and_sort` must never panic; an invalid combination (e.g. the
//! same bit set by one feature and cleared by another) must come back as
//! `Err`, never a panic.
//!
//! Models are built by parsing arbitrary bytes as FASM source
//! (`parse_fasm_bytes`) rather than deriving `arbitrary::Arbitrary` for
//! the model types directly: every parser-produced `SetFasmFeature`
//! already satisfies the `SetFasmFeature::new` invariants (see
//! `rust/fasm/src/output/canonical.rs`'s doc comments), so this reaches
//! `merge_and_sort` with exactly the same kind of input it sees in
//! practice (parsed FASM), without a second, separately-maintained
//! "arbitrary but valid model" generator that could itself drift from
//! those invariants.
//!
//! One input class is deliberately skipped: `merge_features`
//! (`rust/fasm/src/output/merge.rs`) iterates the *entire* address range
//! of each input feature (`for bit in start..=end`), not just its set
//! bits — this is an intentional, documented, Python-equivalent cost
//! (see `docs/rewrite/DESIGN-output.md`'s "`merge_features` still
//! iterates the full address range" section and this crate's
//! `fuzz/README.md`), not a bug T1.6 is meant to catch. A tiny input like
//! `a[4294967294:0]=1` would otherwise turn into a ~4 billion iteration
//! loop on every run that mutates into it, burning the entire fuzzing
//! time budget on a known cost and reporting it as a libFuzzer timeout
//! "crash" that is not a real bug. `MAX_FEATURE_WIDTH` below is a
//! generous bound (real FASM features are at most a few hundred bits
//! wide) that lets huge-but-valid ranges still reach `merge_and_sort`
//! while keeping each run fast enough for effective fuzzing.
#![no_main]

use libfuzzer_sys::fuzz_target;

/// Skip feeding `merge_and_sort` a model containing a feature wider than
/// this; see the module doc comment. Far larger than any real FASM
/// feature (256 bits, the widest known case, a BRAM `INIT`) but far
/// smaller than `u32::MAX`, so `merge_features`'s `O(width)` loop stays
/// fast.
const MAX_FEATURE_WIDTH: u32 = 1 << 16;

fuzz_target!(|data: &[u8]| {
    let Ok(model) = fasm::parse_fasm_bytes(data) else {
        return;
    };

    let too_wide = model.iter().any(|line| {
        line.set_feature
            .as_ref()
            .is_some_and(|f| f.width() > MAX_FEATURE_WIDTH)
    });
    if too_wide {
        return;
    }

    // Never panics; a bit-conflict (or any other invalid combination) is
    // `Err`, which is a perfectly fine result here.
    let _ = fasm::merge_and_sort(model, None);
});

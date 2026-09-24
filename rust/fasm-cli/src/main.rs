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

//! `fasm` command line tool.
//!
//! This will become a byte for byte compatible replacement for
//! `python -m fasm.tool` / the `fasm` console script (same arguments, same
//! stdout/stderr behaviour, same exit codes), see `docs/rewrite/PLAN.md`.
//! Argument parsing and behaviour are added in task T2.1; this is only the
//! workspace skeleton (task T0.3).

fn main() {
    // Nothing to do yet: this is the workspace skeleton, argument parsing
    // and `fasm/tool.py` compatible behaviour land in task T2.1.
}

#[cfg(test)]
mod tests {
    #[test]
    fn depends_on_fasm_crate() {
        assert!(!fasm::VERSION.is_empty());
    }
}

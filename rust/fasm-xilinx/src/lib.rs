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

//! Xilinx bitstream support for FASM.
//!
//! This crate will provide, per `docs/rewrite/PLAN.md`:
//!
//! * a database loader for prjxray-db / prjuray-db layouts (`settings.sh`,
//!   `tilegrid.json`, `segbits_*.db`, `ppips_*.db`, `mask_*.db`,
//!   `part.yaml` / `part.json`, `package_pins.csv`), with an optional
//!   content hashed binary cache;
//! * a `FasmAssembler` equivalent of `prjxray.fasm_assembler` +
//!   `xc_fasm.fasm2frames`, producing frames from FASM features;
//! * a bitstream writer/reader equivalent of prjxray `xc7frames2bit` for
//!   Series7, UltraScale and UltraScale+ (prjuray) architectures.
//!
//! None of the above exists yet; this crate currently only depends on the
//! `fasm` core crate for the workspace skeleton (task T0.3).

#[cfg(test)]
mod tests {
    #[test]
    fn depends_on_fasm_crate() {
        assert!(!fasm::VERSION.is_empty());
    }
}

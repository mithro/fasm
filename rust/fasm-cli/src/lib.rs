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

//! Command line tools for FASM, as a library shared by the binaries:
//!
//! * `fasm` ([`tool`]): a byte for byte compatible replacement for the
//!   original Python `fasm` console script (`fasm/tool.py`);
//! * `fasm2frames` ([`fasm2frames`]): a replacement for f4pga-xc-fasm's
//!   `xc_fasm.fasm2frames` (FASM -> `.frm` frames for Xilinx 7 series
//!   parts);
//! * `xcfasm` ([`xcfasm`]): f4pga-xc-fasm's `xc_fasm.xc_fasm` (FASM ->
//!   `.frm` -> `.bit` in one step);
//! * `xc7frames2bit` ([`xc7frames2bit`]) and `bitread` ([`bitread`]):
//!   prjxray's C++ tools (`.frm` -> `.bit`, `.bit` -> frames);
//! * `fasm-db-cache` ([`db_cache`]): maintenance of the binary database
//!   cache that `fasm2frames` and `xcfasm` use (Rust only, no reference
//!   tool).
//!
//! The Python tools parse their arguments with an emulation of Python's
//! argparse ([`argparse`]), the prjxray tools with an emulation of gflags
//! ([`gflags`]). The known differences to the original tools are listed
//! in `docs/rewrite/COMPAT.md`.

pub mod argparse;
pub mod bitread;
pub mod db_cache;
pub mod fasm2frames;
pub mod gflags;
pub mod pystr;
pub mod terminal;
pub mod tool;
mod unicode_tables;
pub mod xc7frames2bit;
pub mod xcfasm;

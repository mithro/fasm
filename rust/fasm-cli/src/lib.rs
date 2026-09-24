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
//!   original Python `fasm` console script (`fasm/tool.py`).
//!
//! The tools parse their arguments with an emulation of Python's argparse
//! ([`argparse`]). The known differences to the original tools are listed
//! in `docs/rewrite/COMPAT.md`.

pub mod argparse;
pub mod pystr;
pub mod terminal;
pub mod tool;
mod unicode_tables;

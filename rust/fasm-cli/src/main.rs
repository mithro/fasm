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

//! `fasm` command line tool: a byte for byte compatible replacement for the
//! original Python `fasm` console script (`fasm/tool.py`).
//!
//! ```text
//! usage: FASM tool [-h] [--canonical] [--parser PARSER] file
//! ```
//!
//! Same arguments (parsed by an emulation of Python's argparse, including
//! its abbreviations, error messages and help text), same stdout/stderr
//! output and exit codes: the FASM file is printed back (`--canonical`:
//! one line per set bit, sorted and deduplicated) followed by an empty
//! line, and errors reading or parsing it are printed as `Error: ...` on
//! stdout with exit code 0. The known differences are listed in the CLI
//! section of `docs/rewrite/COMPAT.md`.

use std::io;
use std::process::ExitCode;

use fasm_cli::pystr::PyStr;
use fasm_cli::{terminal, tool};

fn main() -> ExitCode {
    let args: Vec<PyStr> = std::env::args_os()
        .skip(1)
        .map(|arg| PyStr::from_os_str(&arg))
        .collect();
    let stdout = io::stdout();
    let stderr = io::stderr();
    let code = tool::run(
        &args,
        terminal::columns,
        &mut stdout.lock(),
        &mut stderr.lock(),
    );
    ExitCode::from(code)
}

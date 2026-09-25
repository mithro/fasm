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

//! `uray-fasm2frames` command line tool: a drop-in replacement for
//! prjuray's `utils/fasm2frames.py` (FASM -> `.frm` frames of 16-bit words
//! for prjuray-db parts); see [`fasm_cli::uray_fasm2frames`].

#![forbid(unsafe_code)]

use std::io;
use std::process::ExitCode;

use fasm_cli::fasm2frames::prog_name;
use fasm_cli::pystr::PyStr;
use fasm_cli::terminal;
use fasm_cli::uray_fasm2frames::{environment_from_process, run};

fn main() -> ExitCode {
    let mut argv = std::env::args_os();
    let prog = prog_name(argv.next().as_deref());
    let args: Vec<PyStr> = argv.map(|arg| PyStr::from_os_str(&arg)).collect();
    let stdout = io::stdout();
    let stderr = io::stderr();
    let code = run(
        &prog,
        &args,
        &environment_from_process(),
        terminal::columns,
        &mut stdout.lock(),
        &mut stderr.lock(),
    );
    ExitCode::from(code)
}

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

//! `xcfasm` command line tool: a drop-in replacement for f4pga-xc-fasm's
//! `xcfasm` (`xc_fasm.xc_fasm`: FASM -> `.frm` -> `.bit` for Xilinx 7
//! series parts, with the bitstream written in process); see
//! [`fasm_cli::xcfasm`].

#![forbid(unsafe_code)]

use std::io;
use std::process::ExitCode;

use fasm_cli::fasm2frames::{prog_name, Environment};
use fasm_cli::pystr::PyStr;
use fasm_cli::terminal;
use fasm_cli::xc7frames2bit::Env;
use fasm_cli::xcfasm::run;

fn main() -> ExitCode {
    let mut argv = std::env::args_os();
    let prog = prog_name(argv.next().as_deref());
    let args: Vec<PyStr> = argv.map(|arg| PyStr::from_os_str(&arg)).collect();
    let stdout = io::stdout();
    let stderr = io::stderr();
    let code = run(
        &prog,
        &args,
        &Environment::from_process(),
        &Env::from_process(),
        terminal::columns,
        &mut stdout.lock(),
        &mut stderr.lock(),
    );
    ExitCode::from(code)
}

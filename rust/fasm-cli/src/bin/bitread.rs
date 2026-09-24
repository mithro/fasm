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

//! `bitread` command line tool: a drop-in replacement for prjxray's C++
//! `bitread` (`.bit` bitstream -> frames as text, Series7); see
//! [`fasm_cli::bitread`].

#![forbid(unsafe_code)]

use std::io;
use std::process::ExitCode;

use fasm_cli::bitread::run;
use fasm_cli::gflags::os_bytes;
use fasm_cli::xc7frames2bit::Env;

fn main() -> ExitCode {
    let mut argv = std::env::args_os();
    let argv0 = argv.next().map(|a| os_bytes(&a)).unwrap_or_default();
    let args: Vec<Vec<u8>> = argv.map(|a| os_bytes(&a)).collect();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let stderr = io::stderr();
    let code = run(
        &argv0,
        &args,
        &Env::from_process(),
        &mut stdin.lock(),
        &mut stdout.lock(),
        &mut stderr.lock(),
    );
    ExitCode::from(code)
}

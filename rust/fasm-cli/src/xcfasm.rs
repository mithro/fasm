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

//! The `xcfasm` tool: `xc_fasm.xc_fasm.main` of f4pga-xc-fasm (FASM ->
//! `.frm` -> `.bit` in one step).
//!
//! ```text
//! usage: xcfasm [-h] --db-root DB_ROOT --part PART --part_file PART_FILE
//!               [--sparse] [--roi ROI] [--emit_pudc_b_pullup] [--debug]
//!               [--frm2bit FRM2BIT] [--fn_in FN_IN] [--bit_out BIT_OUT]
//!               [--frm_out FRM_OUT]
//! ```
//!
//! The same arguments (argparse emulation, [`crate::argparse`]); the
//! frames are assembled like `fasm2frames` and written to `--frm_out`,
//! then the bitstream is written **in process** (the reference runs
//! `<frm2bit> --frm_file <frm_out> --output_file <bit_out> --part_name
//! <part> --part_file <part_file>` through the shell): `--frm2bit` is
//! accepted and ignored. Without `--frm_out`, no `.frm` file is written
//! (the reference writes a temporary file it never deletes) and the
//! bitstream header names the `--fn_in` file instead. See the `xcfasm`
//! section of `docs/rewrite/COMPAT.md`.
//!
//! Like `fasm2frames`, the database is opened through the binary cache
//! configured by the environment (`FASM_XDB_CACHE`, see
//! [`fasm_xilinx::cache`]).

#![forbid(unsafe_code)]

use std::io::Write;

use fasm_xilinx::bitstream::BitstreamFormat;
use fasm_xilinx::Architecture;

use crate::argparse::{Argument, ArgumentParser, Outcome, Values};
use crate::fasm2frames::{
    build_frames, create_output, parser as fasm2frames_parser, path, write_frm_file, Environment,
};
use crate::pystr::PyStr;
use crate::xc7frames2bit::{read_part, write_frames, Env, PartError};

/// The argument parser of `xc_fasm.xc_fasm.main`.
#[must_use]
pub fn parser(prog: &str, env: &Environment) -> ArgumentParser {
    // --db-root and --part (prjxray.util's db_root_arg / part_arg) as in
    // fasm2frames.
    let base = fasm2frames_parser(prog, env);
    let db_root = base.arguments[1].clone();
    let part = base.arguments[2].clone();
    ArgumentParser {
        prog: prog.to_string(),
        description: base.description,
        arguments: vec![
            Argument::help(),
            db_root,
            part,
            Argument::option("--part_file", "part_file", "Part YAML file.").required(true),
            Argument::flag("--sparse", "sparse", "Don't zero fill all frames"),
            Argument::option(
                "--roi",
                "roi",
                "ROI design.json file defining which tiles are within the ROI.",
            ),
            Argument::flag(
                "--emit_pudc_b_pullup",
                "emit_pudc_b_pullup",
                "Emit an IBUF and PULLUP on the PUDC_B pin if unused",
            ),
            Argument::flag("--debug", "debug", "Print debug dump"),
            Argument::option("--frm2bit", "frm2bit", "xc7frames2bit tool.")
                .default(PyStr::from_str("xc7frames2bit")),
            Argument::option("--fn_in", "fn_in", "Input FPGA assembly (.fasm) file"),
            Argument::option("--bit_out", "bit_out", "Output FPGA bitstream (.bit) file"),
            Argument::option("--frm_out", "frm_out", "Output FPGA frame (.frm) file"),
        ],
    }
}

/// The text Python's `str.format` gives an optional argument (`None`
/// for a missing one), as bytes.
fn format_arg(values: &Values, dest: &str) -> Vec<u8> {
    values.str(dest).map_or_else(
        || b"None".to_vec(),
        |s| crate::gflags::os_bytes(&s.to_os_string()),
    )
}

/// Runs the tool with the command line arguments `args` (without the
/// program name); returns the exit code.
pub fn run(
    prog: &str,
    args: &[PyStr],
    env: &Environment,
    process_env: &Env,
    columns: impl FnOnce() -> i64,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let parser = parser(prog, env);
    let values = match parser.parse_args(args) {
        Outcome::Run(values) => values,
        Outcome::Help => {
            let _ = stdout.write_all(parser.format_help(columns()).as_bytes());
            let _ = stdout.flush();
            return 0;
        }
        Outcome::Error(message) => {
            let _ = stderr.write_all(parser.format_error(&message, columns()).as_bytes());
            let _ = stderr.flush();
            return 2;
        }
    };
    let code = match assemble(&values, env, process_env, stdout, stderr) {
        Ok(code) => code,
        Err(message) => {
            let _ = stderr.write_all(message.as_bytes());
            1
        }
    };
    let _ = stdout.flush();
    let _ = stderr.flush();
    code
}

fn assemble(
    values: &Values,
    env: &Environment,
    process_env: &Env,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<u8, String> {
    let frm_out = values.str("frm_out").map(|_| path(values, "frm_out"));
    let file = frm_out.as_deref().map(create_output).transpose()?;
    let fn_in = values.str("fn_in").map(|_| path(values, "fn_in"));
    let frames = build_frames(values, fn_in.as_deref(), &env.db_cache, stderr)?;
    if values.flag("debug") {
        fasm_xilinx::dump_frames_sparse(&frames, stdout)
            .map_err(|e| crate::fasm2frames::write_error(&e))?;
    }
    if let Some(file) = file {
        write_frm_file(&frames, file)?;
    }

    // `subprocess.check_output("{frm2bit} --frm_file {frm_out}
    // --output_file {bit_out} --part_name {part} --part_file {part_file}",
    // shell=True)`, run in process.
    let frm_name = if values.str("frm_out").is_some() {
        format_arg(values, "frm_out")
    } else {
        format_arg(values, "fn_in")
    };
    let bit_out = format_arg(values, "bit_out");
    let part = format_arg(values, "part");
    let part_file = format_arg(values, "part_file");
    let command = [
        format_arg(values, "frm2bit"),
        b" --frm_file ".to_vec(),
        frm_name.clone(),
        b" --output_file ".to_vec(),
        bit_out.clone(),
        b" --part_name ".to_vec(),
        part.clone(),
        b" --part_file ".to_vec(),
        part_file.clone(),
    ]
    .concat();
    let failed = |code: u8| {
        format!(
            "subprocess.CalledProcessError: Command '{}' returned non-zero exit status {code}.\n",
            String::from_utf8_lossy(&command)
        )
    };
    let part_data = match read_part(&part_file, Architecture::Series7) {
        Ok(part) => part,
        Err(PartError::Abort(message)) => {
            // The tool dies with SIGABRT; the shell (`/bin/sh`, dash)
            // reports it and exits with 128 + 6.
            let _ = stderr.write_all(message.as_bytes());
            let _ = stderr.write_all(b"Aborted\n");
            return Err(failed(134));
        }
        Err(PartError::Invalid) => {
            let mut message = b"Part file ".to_vec();
            message.extend_from_slice(&part_file);
            message.extend_from_slice(b" not found or invalid\n");
            let _ = stderr.write_all(&message);
            return Err(failed(1));
        }
    };
    let result = write_frames(
        &frames,
        &part_data,
        &BitstreamFormat::native(Architecture::Series7),
        &frm_name,
        &part,
        &bit_out,
        process_env,
        stderr,
    );
    if result.code != 0 {
        return Err(failed(result.code));
    }
    Ok(0)
}

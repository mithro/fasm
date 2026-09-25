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

//! The `uray-fasm2frames` tool: prjuray's `utils/fasm2frames.py` (FASM ->
//! `.frm` frames for the parts of prjuray-db).
//!
//! ```text
//! usage: fasm2frames.py [-h] --db-root DB_ROOT --part PART [--sparse]
//!                       [--roi ROI] [--debug] [--dump_bits]
//!                       fn_in [fn_out]
//! ```
//!
//! The same arguments (the argparse emulation, with prjuray's `util.py`
//! defaults: `--db-root` from `$URAY_DATABASE_DIR/$URAY_DATABASE`, `--part`
//! from `$URAY_PART`), the same output: a `.frm` file whose frames are
//! written as 16-bit words (186 per UltraScale+ frame, prjuray's
//! `bitstream.WORD_SIZE_BITS`), `--dump_bits` for the `.bits` format
//! instead, `--debug` for the sparse dump in 16-bit words, the same exit
//! codes, and the last line of the reference's traceback on errors (see
//! [`crate::fasm2frames`]; the assembler exceptions are
//! `utils.fasm_assembler.*`).
//!
//! prjuray's assembler differs from xc_fasm's (the Rust `fasm2frames`):
//! no IO bank / STEPDOWN / PUDC_B handling, no dropping of bits beyond
//! the frame (an `IndexError` instead), conflict messages in 16-bit words
//! ([`fasm_xilinx::FasmAssembler::set_prjuray`]). Its `--help` fails like
//! the reference's: argparse expands the `%` of the `--dump_bits` help
//! (`TypeError: %x format: an integer is required, not dict`, exit code
//! 1). The `.frm` of this tool is not for `xcframes2bit` (whose frames are
//! 32-bit words): the Rust `fasm2frames` writes those for a prjuray-db
//! part. See the `uray-fasm2frames` section of `docs/rewrite/COMPAT.md`.

use std::io::{BufWriter, Write};
use std::path::PathBuf;

use fasm_xilinx::cache::CacheOptions;
use fasm_xilinx::{
    dump_frames_sparse_halfwords, uray_fasm2frames, write_bits, write_frm_halfwords,
    AssemblerError, Database, Fasm2FramesOptions,
};

use crate::argparse::{Argument, ArgumentParser, Outcome, Values};
use crate::fasm2frames::{
    create_output, path, path_join, path_str, report_parse_error, write_error, Environment,
};
use crate::pystr::PyStr;

/// The environment of prjuray's `util.db_root_arg` / `part_arg`
/// (`URAY_DATABASE_DIR`, `URAY_DATABASE`, `URAY_PART`, in the fields of
/// [`Environment`]) and the database cache settings.
pub fn environment_from_process() -> Environment {
    let get = |name| std::env::var_os(name).map(|v| PyStr::from_os_str(&v));
    Environment {
        xray_database_dir: get("URAY_DATABASE_DIR"),
        xray_database: get("URAY_DATABASE"),
        xray_part: get("URAY_PART"),
        db_cache: CacheOptions::from_env(),
    }
}

/// The argument parser of prjuray's `utils/fasm2frames.py` (`env` holds
/// the `URAY_*` variables, see [`environment_from_process`]).
#[must_use]
pub fn parser(prog: &str, env: &Environment) -> ArgumentParser {
    let db_root = Argument::option("--db-root", "db_root", "Database root.");
    let db_root = match (&env.xray_database_dir, &env.xray_database) {
        (Some(dir), Some(database)) => db_root.required(false).default(path_join(dir, database)),
        _ => db_root.required(true),
    };
    let part = Argument::option(
        "--part",
        "part",
        "Part name. When not given defaults to URAY_PART env.",
    );
    let part = match &env.xray_part {
        Some(value) => part.required(false).default(value.clone()),
        None => part.required(true),
    };
    ArgumentParser {
        prog: prog.to_string(),
        description: Some(
            "Convert FPGA configuration description (\"FPGA assembly\") into binary frame \
             equivalent",
        ),
        arguments: vec![
            Argument::help(),
            db_root,
            part,
            Argument::flag("--sparse", "sparse", "Don't zero fill all frames"),
            Argument::option(
                "--roi",
                "roi",
                "ROI design.json file defining which tiles are within the ROI.",
            ),
            Argument::flag("--debug", "debug", "Print debug dump"),
            Argument::flag(
                "--dump_bits",
                "dump_bits",
                "Output in bits format (bit_%08x_%03d_%02d)",
            ),
            Argument::positional("fn_in", "Input FPGA assembly (.fasm) file"),
            Argument::positional("fn_out", "Output FPGA frame (.frm) file")
                .optional()
                .default(PyStr::from_str("/dev/stdout")),
        ],
    }
}

/// What the reference prints for `--help`: argparse's `_expand_help`
/// formats the `--dump_bits` help, `"... (bit_%08x_%03d_%02d)" % params`,
/// which fails.
pub const HELP_ERROR: &str = "TypeError: %x format: an integer is required, not dict\n";

/// Runs the tool with the command line arguments `args` (without the
/// program name), writing to `stdout` (`--debug`) and `stderr`, and
/// returns the exit code. `columns` gives the terminal width for the
/// usage messages.
pub fn run(
    prog: &str,
    args: &[PyStr],
    env: &Environment,
    columns: impl FnOnce() -> i64,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let parser = parser(prog, env);
    let values = match parser.parse_args(args) {
        Outcome::Run(values) => values,
        Outcome::Help => {
            let _ = stderr.write_all(HELP_ERROR.as_bytes());
            let _ = stderr.flush();
            return 1;
        }
        Outcome::Error(message) => {
            let _ = stderr.write_all(parser.format_error(&message, columns()).as_bytes());
            let _ = stderr.flush();
            return 2;
        }
    };
    let code = match assemble(&values, &env.db_cache, stdout) {
        Ok(()) => 0,
        Err(message) => {
            let _ = stderr.write_all(message.as_bytes());
            1
        }
    };
    let _ = stdout.flush();
    let _ = stderr.flush();
    code
}

/// The traceback line of `e` with prjuray's module names.
fn traceback(e: AssemblerError) -> String {
    let line = e.traceback_line();
    let line = match line.strip_prefix("prjxray.fasm_assembler.") {
        Some(rest) => format!("utils.fasm_assembler.{rest}"),
        None => line,
    };
    format!("{line}\n")
}

/// `main()` after `parse_args`: the error text (the last line of the
/// Python traceback, with its newline) on failure.
fn assemble(values: &Values, cache: &CacheOptions, stdout: &mut dyn Write) -> Result<(), String> {
    // `f_out=open(args.fn_out, 'w')` is evaluated before `run()`.
    let fn_out = path(values, "fn_out");
    let file = create_output(&fn_out)?;
    let fn_in = path(values, "fn_in");
    let db = Database::open_cached(
        &path(values, "db_root"),
        Some(&path_str(values, "part")),
        cache,
    )
    .map_err(|e| traceback(e.into()))?;
    let options = Fasm2FramesOptions {
        sparse: values.flag("sparse"),
        roi: values.str("roi").map(|r| PathBuf::from(r.to_os_string())),
        emit_pudc_b_pullup: false,
    };
    let frames = uray_fasm2frames(&db, &fn_in, &options).map_err(|e| match e {
        AssemblerError::Parse(first) => traceback(AssemblerError::Parse(report_parse_error(
            first, &db, &options, &fn_in,
        ))),
        e => traceback(e),
    })?;
    if values.flag("debug") {
        dump_frames_sparse_halfwords(&frames, stdout).map_err(|e| write_error(&e))?;
        stdout.flush().map_err(|e| write_error(&e))?;
    }
    let mut out = BufWriter::with_capacity(1 << 16, file);
    if values.flag("dump_bits") {
        write_bits(&frames, &mut out)
    } else {
        write_frm_halfwords(&frames, &mut out)
    }
    .and_then(|()| out.flush())
    .map_err(|e| write_error(&e))
}

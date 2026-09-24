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

//! The `fasm2frames` tool: `xc_fasm.fasm2frames.main` of f4pga-xc-fasm.
//!
//! ```text
//! usage: fasm2frames [-h] --db-root DB_ROOT --part PART [--sparse]
//!                    [--roi ROI] [--emit_pudc_b_pullup] [--debug]
//!                    fn_in [fn_out]
//! ```
//!
//! Same arguments (Python's argparse, emulated by [`crate::argparse`],
//! including `prjxray.util.db_root_arg` / `part_arg`: `--db-root` defaults
//! to `$XRAY_DATABASE_DIR/$XRAY_DATABASE` and `--part` to `$XRAY_PART`
//! when set), same `.frm` output, same exit codes. The program name is the
//! base name of `argv[0]`, like argparse's (`fasm2frames.py` for the
//! original run with `python -m`).
//!
//! Like the original, the output file is opened (and truncated) first,
//! then the database is opened and the FASM file assembled. Where the
//! original dies with an exception (exit code 1, a traceback on stderr),
//! this tool prints only the last line of the traceback,
//! `<exception type>: <message>` (for example
//! `prjxray.fasm_assembler.FasmLookupError: Segment DB ...`), and exits
//! with 1. See the `fasm2frames` section of `docs/rewrite/COMPAT.md`.
//!
//! The database is opened through the binary cache of
//! [`fasm_xilinx::cache`] as configured by the environment
//! (`FASM_XDB_CACHE`, `FASM_XDB_CACHE_VERBOSE`; no command line flag, the
//! command line is the reference one): the output is identical with and
//! without it.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use fasm::ParseError;
use fasm_xilinx::cache::CacheOptions;
use fasm_xilinx::{
    dump_frames_sparse, fasm2frames, read_roi_design, AssemblerError, Database, Fasm2FramesOptions,
};

use crate::argparse::{Argument, ArgumentParser, Outcome, Values};
use crate::pystr::PyStr;

/// The environment variables `prjxray.util` reads for the defaults, and
/// the database cache settings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Environment {
    /// `XRAY_DATABASE_DIR`.
    pub xray_database_dir: Option<PyStr>,
    /// `XRAY_DATABASE`.
    pub xray_database: Option<PyStr>,
    /// `XRAY_PART`.
    pub xray_part: Option<PyStr>,
    /// The binary database cache ([`CacheOptions::from_env`] for the
    /// process; disabled by default).
    pub db_cache: CacheOptions,
}

impl Environment {
    /// The variables of this process.
    pub fn from_process() -> Self {
        let get = |name| std::env::var_os(name).map(|v| PyStr::from_os_str(&v));
        Environment {
            xray_database_dir: get("XRAY_DATABASE_DIR"),
            xray_database: get("XRAY_DATABASE"),
            xray_part: get("XRAY_PART"),
            db_cache: CacheOptions::from_env(),
        }
    }
}

/// `os.path.join(a, b)` (POSIX).
fn path_join(a: &PyStr, b: &PyStr) -> PyStr {
    let slash = u32::from('/');
    if b.at(0) == Some(slash) || a.is_empty() {
        return b.clone();
    }
    let mut out = a.clone();
    if out.0.last() != Some(&slash) {
        out.0.push(slash);
    }
    out.0.extend_from_slice(&b.0);
    out
}

/// The argument parser of `xc_fasm.fasm2frames.main`.
#[must_use]
pub fn parser(prog: &str, env: &Environment) -> ArgumentParser {
    // prjxray.util.db_root_arg
    let db_root = Argument::option("--db-root", "db_root", "Database root.");
    let db_root = match (&env.xray_database_dir, &env.xray_database) {
        (Some(dir), Some(database)) => db_root.required(false).default(path_join(dir, database)),
        _ => db_root.required(true),
    };
    // prjxray.util.part_arg
    let part = Argument::option(
        "--part",
        "part",
        "Part name. When not given defaults to XRAY_PART env. var.",
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
            Argument::flag(
                "--emit_pudc_b_pullup",
                "emit_pudc_b_pullup",
                "Emit an IBUF and PULLUP on the PUDC_B pin if unused",
            ),
            Argument::flag("--debug", "debug", "Print debug dump"),
            Argument::positional("fn_in", "Input FPGA assembly (.fasm) file"),
            Argument::positional("fn_out", "Output FPGA frame (.frm) file")
                .optional()
                .default(PyStr::from_str("/dev/stdout")),
        ],
    }
}

/// The program name argparse derives from `argv[0]`
/// (`os.path.basename(sys.argv[0])`).
#[must_use]
pub fn prog_name(argv0: Option<&std::ffi::OsStr>) -> String {
    let argv0 = argv0
        .map(|a| a.to_string_lossy().into_owned())
        .unwrap_or_default();
    match argv0.rfind('/') {
        Some(i) => argv0[i + 1..].to_string(),
        None => argv0,
    }
}

/// An error writing the output (`OSError` of `f.write`).
pub(crate) fn write_error(error: &io::Error) -> String {
    let text = error.to_string();
    let strerror = text
        .rfind(" (os error ")
        .map_or(text.as_str(), |i| &text[..i]);
    let name = if error.raw_os_error() == Some(32) {
        "BrokenPipeError"
    } else {
        "OSError"
    };
    match error.raw_os_error() {
        Some(errno) => format!("{name}: [Errno {errno}] {strerror}\n"),
        None => format!("{name}: {strerror}\n"),
    }
}

/// Runs the tool with the command line arguments `args` (without the
/// program name), writing to `stdout` (`--debug`) and `stderr`, and
/// returns the exit code. `columns` gives the terminal width for the help
/// and usage messages (see [`crate::terminal::columns`]).
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
            // argparse ignores errors writing the help (`_print_message`).
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
    let code = match assemble(&values, &env.db_cache, stdout, stderr) {
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

pub(crate) fn path(values: &Values, dest: &str) -> PathBuf {
    PathBuf::from(values.str(dest).cloned().unwrap_or_default().to_os_string())
}

/// `main()` after `parse_args`: the error text (the last line of the
/// Python traceback, with its newline) on failure.
fn assemble(
    values: &Values,
    cache: &CacheOptions,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), String> {
    // `f_out=open(args.fn_out, 'w')` is evaluated before `fasm2frames()`.
    let fn_out = path(values, "fn_out");
    let file = create_output(&fn_out)?;
    let fn_in = path(values, "fn_in");
    let frames = build_frames(values, Some(&fn_in), cache, stderr)?;
    write_frm_file(&frames, file)?;
    if values.flag("debug") {
        dump_frames_sparse(&frames, stdout).map_err(|e| write_error(&e))?;
    }
    Ok(())
}

/// `open(path, 'w')`, or the traceback line of its error.
pub(crate) fn create_output(path: &Path) -> Result<File, String> {
    File::create(path).map_err(|source| {
        format!(
            "{}\n",
            AssemblerError::Io {
                path: path.to_path_buf(),
                source,
            }
            .traceback_line()
        )
    })
}

/// `dump_frm(f_out, frames)` and closing the file, or the traceback line
/// of the error.
pub(crate) fn write_frm_file(frames: &fasm_xilinx::Frames, file: File) -> Result<(), String> {
    let mut out = BufWriter::with_capacity(1 << 16, file);
    frames
        .write_frm(&mut out)
        .and_then(|()| out.flush())
        .map_err(|e| write_error(&e))
}

/// `fasm2frames()` of `xc_fasm.fasm2frames` with the arguments of
/// `values` (`db_root`, `part`, `sparse`, `roi`, `emit_pudc_b_pullup`):
/// opens the database (through `cache`, which never changes the result)
/// and assembles `fn_in`, or returns the traceback line of the error.
/// `fn_in` of `None` (`xcfasm` without `--fn_in`) fails like the reference
/// when the FASM file would be parsed.
pub(crate) fn build_frames(
    values: &Values,
    fn_in: Option<&Path>,
    cache: &CacheOptions,
    stderr: &mut dyn Write,
) -> Result<fasm_xilinx::Frames, String> {
    let traceback = |e: AssemblerError| format!("{}\n", e.traceback_line());
    let db = Database::open_cached(
        &path(values, "db_root"),
        Some(&path_str(values, "part")),
        cache,
    )
    .map_err(|e| traceback(e.into()))?;
    let options = Fasm2FramesOptions {
        sparse: values.flag("sparse"),
        roi: values.str("roi").map(|r| PathBuf::from(r.to_os_string())),
        emit_pudc_b_pullup: values.flag("emit_pudc_b_pullup"),
    };
    let Some(fn_in) = fn_in else {
        // `bytes(None, 'ascii')` in the ANTLR wrapper, after the ROI has
        // been read.
        if let Some(roi) = options.roi.as_deref().filter(|r| !r.as_os_str().is_empty()) {
            read_roi_design(roi).map_err(traceback)?;
        }
        return Err("TypeError: encoding without a string argument\n".to_string());
    };
    fasm2frames(&db, fn_in, &options, &mut |warning| {
        let _ = writeln!(stderr, "{warning}");
    })
    .map_err(|e| match e {
        AssemblerError::Parse(first) => traceback(AssemblerError::Parse(report_parse_error(
            first, &db, &options, fn_in,
        ))),
        e => traceback(e),
    })
}

/// The syntax error the reference reports for a FASM text whose first
/// error (in file order) is `first`: the ANTLR parser checks the syntax of
/// the whole text before it decodes any value, so a syntax error later in
/// the text wins over an earlier value range error
/// ([`crate::tool::error_to_report`], as for the `fasm` tool).
///
/// `fasm2frames()` parses, in this order, the ROI's `required_features`,
/// the part's `required_features.fasm` and the FASM file, and stops at the
/// first text with an error: the first of them that does not parse is the
/// one `first` comes from. (The PUDC_B and STEPDOWN features are single
/// generated lines, to which the precedence does not apply.)
fn report_parse_error(
    first: ParseError,
    db: &Database,
    options: &Fasm2FramesOptions,
    fasm: &Path,
) -> ParseError {
    let mut texts: Vec<Vec<u8>> = Vec::new();
    if let Some(roi) = options.roi.as_deref().filter(|r| !r.as_os_str().is_empty()) {
        match read_roi_design(roi) {
            Ok(design) => texts.extend(design.required_features.map(String::into_bytes)),
            Err(_) => return first,
        }
    }
    if let Some(info) = db.part_info() {
        texts.push(
            db.get_required_fasm_features(Some(&info.name))
                .join("\n")
                .into_bytes(),
        );
    }
    if let Ok(data) = std::fs::read(fasm) {
        texts.push(data);
    }
    for text in texts {
        if let Err(error) = fasm::parse_fasm_bytes(&text) {
            return if error == first {
                crate::tool::error_to_report(&text, first)
            } else {
                first
            };
        }
    }
    first
}

/// The part name as a string (a part name is always ASCII; other code
/// points are replaced).
pub(crate) fn path_str(values: &Values, dest: &str) -> String {
    values
        .str(dest)
        .cloned()
        .unwrap_or_default()
        .to_os_string()
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests;

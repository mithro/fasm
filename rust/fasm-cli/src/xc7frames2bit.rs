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

//! The `xc7frames2bit` tool: prjxray's `tools/xc7frames2bit.cc` (`.frm`
//! frames -> `.bit` bitstream).
//!
//! ```text
//! xc7frames2bit --part_file=<part.yaml> --part_name=<part> \
//!     --frm_file=<in.frm> --output_file=<out.bit> [--architecture=Series7]
//! ```
//!
//! gflags style flags ([`crate::gflags`]), the same messages and exit
//! codes, and byte for byte the same `.bit` for the same inputs, date and
//! time (the header has the current UTC time like the reference, or
//! `$SOURCE_DATE_EPOCH` when it is set, an extension for reproducible
//! builds). Only the Series7 architecture is implemented; see the
//! `xc7frames2bit` section of `docs/rewrite/COMPAT.md`.

#![forbid(unsafe_code)]

use std::io::Write;

use fasm_xilinx::bitstream::{bitstream_bytes, utc_date_time, BitstreamOptions};
use fasm_xilinx::{Architecture, Frames, FrmErrorKind, Part};

use crate::gflags::{self, Flag, FlagType, Outcome, Program};

const FILE: &str = "tools/xc7frames2bit.cc";

/// The flags of `xc7frames2bit`.
pub fn flags() -> Vec<Flag> {
    let f = |name, default, help| Flag {
        name,
        file: FILE,
        ty: FlagType::String,
        default,
        help,
    };
    vec![
        f("part_name", "", "Name of the 7-series part"),
        f("part_file", "", "Definition file for target 7-series part"),
        f(
            "frm_file",
            "",
            "File containing a list of frame deltas to be applied to the base bitstream.  Each line in the file is of the form: <frame_address> <word1>,...,<word101>.",
        ),
        f("output_file", "", "Write bitstream to file"),
        f(
            "architecture",
            "Series7",
            "Architecture of the provided bitstream",
        ),
    ]
}

/// The exit code for an abort (`std::terminate` after an uncaught C++
/// exception): the binary calls `std::process::abort()` for it.
pub const ABORT: u8 = 134;

/// The process environment the tools read.
#[derive(Clone, Debug, Default)]
pub struct Env {
    /// All variables (for `--fromenv`, `SOURCE_DATE_EPOCH`).
    pub vars: Vec<(String, Vec<u8>)>,
}

impl Env {
    /// The variables of this process.
    pub fn from_process() -> Self {
        Env {
            vars: std::env::vars_os()
                .filter_map(|(k, v)| Some((k.into_string().ok()?, gflags::os_bytes(&v))))
                .collect(),
        }
    }

    /// The value of `name`.
    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.vars
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }

    /// The header `(date, time)` from `SOURCE_DATE_EPOCH` (decimal seconds
    /// since the epoch), if set and valid.
    pub fn source_date(&self) -> Option<(String, String)> {
        let value = self.get("SOURCE_DATE_EPOCH")?;
        let seconds: i64 = std::str::from_utf8(&value).ok()?.trim().parse().ok()?;
        Some(utc_date_time(seconds))
    }
}

/// A path from flag bytes.
pub(crate) fn os_path(bytes: &[u8]) -> std::path::PathBuf {
    gflags::bytes_path(bytes)
}

/// `--architecture`: `ArchitectureFactory::create_architecture` returns
/// the first alternative of the variant (Series7) for an unknown name.
/// `Err` with the message for the architectures that are not implemented.
pub(crate) fn architecture(tool: &str, name: &[u8]) -> Result<Architecture, String> {
    match name {
        b"UltraScale" | b"UltraScalePlus" | b"Spartan6" => Err(format!(
            "{tool}: --architecture={} is not supported yet (only Series7)\n",
            String::from_utf8_lossy(name)
        )),
        _ => Ok(Architecture::Series7),
    }
}

/// `ArchType::Part::FromFile` for Series7.
pub(crate) fn read_part(path: &[u8]) -> Option<Part> {
    Part::from_yaml_file(&os_path(path), Architecture::Series7)
        .ok()
        .filter(|p| p.architecture == Architecture::Series7)
}

/// The message of `std::terminate` for an uncaught exception of
/// `std::stoul`.
pub(crate) fn stoul_terminate(kind: FrmErrorKind) -> String {
    let exception = match kind {
        FrmErrorKind::InvalidArgument => "std::invalid_argument",
        FrmErrorKind::OutOfRange => "std::out_of_range",
    };
    format!("terminate called after throwing an instance of '{exception}'\n  what():  stoul\n")
}

/// What the (in process) `xc7frames2bit` run did.
pub(crate) struct Frames2Bit {
    /// Exit code.
    pub code: u8,
}

/// Writes `frames` as a bitstream like `Frames2BitWriter` after the frames
/// have been read: `part_file` is read (error: exit 1), then the
/// bitstream is built and written to `output_file` (a file that cannot be
/// created is reported with exit code 0, like the reference).
pub(crate) fn write_frames(
    frames: &Frames,
    part: &Part,
    frm_file: &[u8],
    part_name: &[u8],
    output_file: &[u8],
    env: &Env,
    stderr: &mut dyn Write,
) -> Frames2Bit {
    let (date, time) = env.source_date().unzip();
    let options = BitstreamOptions {
        design_name: frm_file.to_vec(),
        generator: b"xc7frames2bit".to_vec(),
        part_name: part_name.to_vec(),
        date,
        time,
    };
    let bytes = match bitstream_bytes(part, frames, &options) {
        Ok(bytes) => bytes,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            return Frames2Bit { code: 1 };
        }
    };
    let mut file = match std::fs::File::create(os_path(output_file)) {
        Ok(file) => file,
        Err(_) => {
            let _ = stderr.write_all(b"Unable to open file for writting: ");
            let _ = stderr.write_all(output_file);
            let _ = stderr.write_all(b"\nFailed to write bitstream\nExitting\n");
            return Frames2Bit { code: 0 };
        }
    };
    if let Err(e) = file.write_all(&bytes).and_then(|()| file.flush()) {
        let _ = stderr.write_all(b"Error writing ");
        let _ = stderr.write_all(output_file);
        let _ = writeln!(stderr, ": {e}\nFailed to write bitstream\nExitting");
        return Frames2Bit { code: 1 };
    }
    Frames2Bit { code: 0 }
}

/// Reads the `.frm` file like `Frames::readFrames` (a directory reads as
/// empty, like an `std::ifstream` on it); `Err` is the exit code after the
/// messages have been written.
pub(crate) fn read_frames(frm_file: &[u8], stderr: &mut dyn Write) -> Result<Frames, u8> {
    let path = os_path(frm_file);
    let data = if path.is_dir() {
        Vec::new()
    } else {
        match std::fs::read(&path) {
            Ok(data) => data,
            Err(_) => {
                let _ = stderr.write_all(b"Unable to open frm file: ");
                let _ = stderr.write_all(frm_file);
                let _ = stderr.write_all(b"\nFrames file ");
                let _ = stderr.write_all(frm_file);
                let _ = stderr.write_all(b" not found or invalid\n");
                return Err(1);
            }
        }
    };
    let wpf = Architecture::Series7.words_per_frame();
    let mut warnings = Vec::new();
    let result = Frames::read_frm(&data, wpf, &mut |w| {
        warnings.extend_from_slice(w.as_bytes());
        warnings.push(b'\n');
    });
    let _ = stderr.write_all(&warnings);
    result.map_err(|e| {
        let _ = stderr.write_all(stoul_terminate(e.kind).as_bytes());
        ABORT
    })
}

/// Runs the tool with `args` (without `argv[0]`); returns the exit code
/// ([`ABORT`] for an abort).
pub fn run(
    argv0: &[u8],
    args: &[Vec<u8>],
    env: &Env,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let program = Program {
        argv0: argv0.to_vec(),
        usage: argv0.to_vec(),
        flags: flags(),
    };
    let parsed = match gflags::parse(&program, args, &|name| env.get(name)) {
        Outcome::Run(parsed) => parsed,
        Outcome::Exit {
            code,
            stdout: out,
            stderr: err,
        } => {
            let _ = stdout.write_all(&out);
            let _ = stderr.write_all(&err);
            return code;
        }
    };
    if let Err(message) = architecture("xc7frames2bit", parsed.string("architecture")) {
        let _ = stderr.write_all(message.as_bytes());
        return 1;
    }
    let part_file = parsed.string("part_file");
    let Some(part) = read_part(part_file) else {
        let _ = stderr.write_all(b"Part file ");
        let _ = stderr.write_all(part_file);
        let _ = stderr.write_all(b" not found or invalid\n");
        return 1;
    };
    let frm_file = parsed.string("frm_file");
    let frames = match read_frames(frm_file, stderr) {
        Ok(frames) => frames,
        Err(code) => return code,
    };
    write_frames(
        &frames,
        &part,
        frm_file,
        parsed.string("part_name"),
        parsed.string("output_file"),
        env,
        stderr,
    )
    .code
}

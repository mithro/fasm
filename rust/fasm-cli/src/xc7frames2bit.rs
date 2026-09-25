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

//! The `xc7frames2bit` tool (prjxray's `tools/xc7frames2bit.cc`) and the
//! `xcframes2bit` tool (prjuray-tools' `tools/xcframes2bit.cc`): `.frm`
//! frames -> `.bit` bitstream.
//!
//! ```text
//! xc7frames2bit --part_file=<part.yaml> --part_name=<part> \
//!     --frm_file=<in.frm> --output_file=<out.bit> [--architecture=Series7]
//! xcframes2bit ... [--architecture=UltraScalePlus]
//! ```
//!
//! gflags style flags ([`crate::gflags`]), the same messages and exit
//! codes, and byte for byte the same `.bit` for the same inputs, date and
//! time (the header has the current UTC time like the reference, or
//! `$SOURCE_DATE_EPOCH` when it is set, an extension for reproducible
//! builds). The two tools differ ([`Tool`]):
//!
//! * `--architecture`: `xc7frames2bit` makes UltraScale and UltraScale+
//!   bitstreams from Series7 `part.yaml` files with the Series7 ECC and
//!   takes an unknown name for Series7; `xcframes2bit` uses the
//!   `xcuseries` / `xcupseries` parts, frame addresses and ECC and aborts
//!   on an unknown name ([`BitstreamFormat`]);
//! * `xcframes2bit` rejects a `.frm` frame that is not in the part
//!   (`Frames file contains an invalid frame: ...`);
//! * the gflags copies differ (`--helpful` / `--helpfull`).
//!
//! Spartan6 is not implemented; see the `xc7frames2bit` and
//! `xcframes2bit` sections of `docs/rewrite/COMPAT.md`.

#![forbid(unsafe_code)]

use std::io::Write;

use fasm_xilinx::bitstream::{
    bitstream_bytes_with, utc_date_time, BitstreamFormat, BitstreamOptions,
};
use fasm_xilinx::{Architecture, FrameAddress, Frames, FrmErrorKind, Part};

use crate::gflags::{self, Flag, FlagType, Gflags, Outcome, Program};

/// Which reference tool is reproduced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// prjxray's `xc7frames2bit`.
    Xc7frames2bit,
    /// prjuray-tools' `xcframes2bit`.
    Xcframes2bit,
}

impl Tool {
    /// The source file the help lists the tool's flags under.
    pub const fn file(self) -> &'static str {
        match self {
            Tool::Xc7frames2bit => "tools/xc7frames2bit.cc",
            Tool::Xcframes2bit => "tools/xcframes2bit.cc",
        }
    }

    /// The tool's name.
    pub const fn name(self) -> &'static str {
        match self {
            Tool::Xc7frames2bit => "xc7frames2bit",
            Tool::Xcframes2bit => "xcframes2bit",
        }
    }

    /// The gflags copy the tool is built with.
    pub const fn gflags(self) -> Gflags {
        match self {
            Tool::Xc7frames2bit => Gflags::Prjxray,
            Tool::Xcframes2bit => Gflags::Prjuray,
        }
    }

    /// `true` for the prjuray-tools implementation.
    pub const fn is_prjuray(self) -> bool {
        matches!(self, Tool::Xcframes2bit)
    }
}

/// The flags of `xc7frames2bit`.
pub fn flags() -> Vec<Flag> {
    flags_of(Tool::Xc7frames2bit)
}

/// The flags of `tool` (the same flags and texts, another file).
pub fn flags_of(tool: Tool) -> Vec<Flag> {
    let file = tool.file();
    let f = |name, default, help| Flag {
        name,
        file,
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
    /// since the epoch): `Ok(None)` if it is not set, `Err` with the value
    /// if it is not an integer (the current time is used then, with a
    /// warning).
    pub fn source_date(&self) -> Result<Option<(String, String)>, String> {
        let Some(value) = self.get("SOURCE_DATE_EPOCH") else {
            return Ok(None);
        };
        std::str::from_utf8(&value)
            .ok()
            .and_then(|v| v.trim().parse::<i64>().ok())
            .map(|seconds| Some(utc_date_time(seconds)))
            .ok_or_else(|| String::from_utf8_lossy(&value).into_owned())
    }
}

/// A path from flag bytes.
pub(crate) fn os_path(bytes: &[u8]) -> std::path::PathBuf {
    gflags::bytes_path(bytes)
}

/// The `std::terminate` message of `ArchitectureFactory::create_architecture`
/// in prjuray-tools for an unknown name.
pub(crate) const BAD_VARIANT_TERMINATE: &str =
    "terminate called after throwing an instance of 'absl::bad_variant_access'\n  what():  Bad variant access\n";

/// Why `--architecture` cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ArchitectureError {
    /// Spartan6 (supported by the references, not implemented): the
    /// message, exit code 1.
    Unsupported(String),
    /// prjuray-tools aborts on an unknown name: the `std::terminate`
    /// message ([`ABORT`]).
    Abort(String),
}

/// `ArchitectureFactory::create_architecture(name)` of the tool's
/// reference: prjxray's returns the default constructed variant (Series7)
/// for an unknown name and has the Series7 part types for UltraScale and
/// UltraScale+ ([`BitstreamFormat::prjxray`]); prjuray-tools' throws
/// `absl::bad_variant_access` for an unknown name and has their own
/// ([`BitstreamFormat::native`]).
pub(crate) fn architecture(
    tool: &str,
    prjuray: bool,
    name: &[u8],
) -> Result<BitstreamFormat, ArchitectureError> {
    let arch = match name {
        b"Series7" => Architecture::Series7,
        b"UltraScale" => Architecture::UltraScale,
        b"UltraScalePlus" => Architecture::UltraScalePlus,
        b"Spartan6" => {
            return Err(ArchitectureError::Unsupported(format!(
                "{tool}: --architecture=Spartan6 is not supported yet (only Series7, UltraScale and UltraScalePlus)\n"
            )))
        }
        _ if prjuray => {
            return Err(ArchitectureError::Abort(BAD_VARIANT_TERMINATE.to_owned()));
        }
        _ => Architecture::Series7,
    };
    Ok(if prjuray {
        BitstreamFormat::native(arch)
    } else {
        BitstreamFormat::prjxray(arch)
    })
}

/// Why [`read_part`] failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PartError {
    /// `Part file ... not found or invalid` (`FromFile` returns nothing).
    Invalid,
    /// The reference aborts: yaml-cpp reading a directory through an
    /// `std::ifstream` throws an uncaught `std::__ios_failure`. The
    /// message of `std::terminate`.
    Abort(String),
}

/// The `std::terminate` message of the reference for a part file that is
/// a directory.
pub(crate) const DIRECTORY_TERMINATE: &str = "terminate called after throwing an instance of 'std::__ios_failure'\n  what():  basic_filebuf::underflow error reading the file: Is a directory\n";

/// `ArchType::Part::FromFile` for the part type `arch` (`xc7series`,
/// `xcuseries`, `xcupseries`): a `part.yaml` with another architecture's
/// tag is invalid, an untagged one is read as `arch`.
pub(crate) fn read_part(path: &[u8], arch: Architecture) -> Result<Part, PartError> {
    let path = os_path(path);
    if path.is_dir() {
        return Err(PartError::Abort(DIRECTORY_TERMINATE.to_owned()));
    }
    Part::from_yaml_file(&path, arch)
        .ok()
        .filter(|p| p.architecture == arch)
        .ok_or(PartError::Invalid)
}

/// The message of `std::terminate` for an uncaught exception of
/// `std::stoul`.
pub(crate) fn stoul_terminate(kind: FrmErrorKind) -> String {
    let exception = match kind {
        FrmErrorKind::OutOfRange => "std::out_of_range",
        _ => "std::invalid_argument",
    };
    format!("terminate called after throwing an instance of '{exception}'\n  what():  stoul\n")
}

/// `operator<<(std::ostream&, const FrameAddress&)` of the C++ frame
/// address of `arch` (on a fresh stream):
/// `[<%#10x>] <TOP|BOTTOM> Row=%2d Column=%2d Minor=%2d Type=<type>`,
/// the `TOP`/`BOTTOM` part only for Series7 (`xc7series`), whose row has
/// no half bit; the UltraScale(+) row includes it and two spaces are left
/// instead (`] ` and ` Row=`). `%#x` of 0 is `0` (no `0x`), like `std::showbase`.
/// The block type is `CLB/IO/CLK`, `Block RAM`, `Config CLB` or nothing.
pub fn cpp_frame_address(arch: Architecture, address: FrameAddress) -> String {
    let hex = if address.0 == 0 {
        "0".to_owned()
    } else {
        format!("0x{:x}", address.0)
    };
    let half = if arch == Architecture::Series7 {
        if address.is_bottom_half(arch) {
            "BOTTOM"
        } else {
            "TOP"
        }
    } else {
        ""
    };
    let block_type = match address.block_type_raw(arch) {
        0 => "CLB/IO/CLK",
        1 => "Block RAM",
        2 => "Config CLB",
        _ => "",
    };
    format!(
        "[{hex:>10}] {half} Row={:2} Column={:2} Minor={:2} Type={block_type}",
        address.row_index(arch),
        address.column(arch),
        address.minor(arch)
    )
}

/// What the (in process) `xc7frames2bit` run did.
pub(crate) struct Frames2Bit {
    /// Exit code.
    pub code: u8,
}

/// Writes `frames` as a bitstream like `Frames2BitWriter` after the frames
/// have been read: the bitstream in `format` is built and written to
/// `output_file` (a file that cannot be created is reported with exit code
/// 0, like the reference).
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_frames(
    frames: &Frames,
    part: &Part,
    format: &BitstreamFormat,
    frm_file: &[u8],
    part_name: &[u8],
    output_file: &[u8],
    env: &Env,
    stderr: &mut dyn Write,
) -> Frames2Bit {
    let (date, time) = match env.source_date() {
        Ok(date_time) => date_time.unzip(),
        Err(value) => {
            let _ = writeln!(
                stderr,
                "warning: SOURCE_DATE_EPOCH={value:?} is not an integer, using the current time"
            );
            (None, None)
        }
    };
    let options = BitstreamOptions {
        design_name: frm_file.to_vec(),
        generator: b"xc7frames2bit".to_vec(),
        part_name: part_name.to_vec(),
        date,
        time,
    };
    let bytes = match bitstream_bytes_with(part, frames, &options, format) {
        Ok(bytes) => bytes,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            return Frames2Bit { code: 1 };
        }
    };
    let mut bytes = bytes;
    let mut file = match std::fs::File::create(os_path(output_file)) {
        Ok(file) => file,
        Err(_) => {
            let _ = stderr.write_all(b"Unable to open file for writting: ");
            let _ = stderr.write_all(output_file);
            let _ = stderr.write_all(b"\nFailed to write bitstream\nExitting\n");
            return Frames2Bit { code: 0 };
        }
    };
    // `writeBitstream` seeks back to fill in the data length of the
    // header (field `e`); on an unseekable output (a pipe) the seek fails
    // and the field stays zero.
    if std::io::Seek::stream_position(&mut file).is_err() {
        if let Some(header) = fasm_xilinx::bitstream::BitHeader::parse(&bytes) {
            let end = header.header_length;
            bytes[end - 4..end].fill(0);
        }
    }
    if let Err(e) = file.write_all(&bytes).and_then(|()| file.flush()) {
        let _ = stderr.write_all(b"Error writing ");
        let _ = stderr.write_all(output_file);
        let _ = writeln!(stderr, ": {e}\nFailed to write bitstream\nExitting");
        return Frames2Bit { code: 1 };
    }
    Frames2Bit { code: 0 }
}

/// Reads the `.frm` file like `Frames::readFrames` (a directory reads as
/// empty, like an `std::ifstream` on it) with `words_per_frame` words per
/// frame; with `valid_in`, like prjuray-tools' `readFrames(file, part)`,
/// which stops at the first frame that is not in the part. `Err` is the
/// exit code after the messages have been written.
pub(crate) fn read_frames(
    frm_file: &[u8],
    words_per_frame: usize,
    valid_in: Option<&Part>,
    stderr: &mut dyn Write,
) -> Result<Frames, u8> {
    let not_found = |stderr: &mut dyn Write| {
        let _ = stderr.write_all(b"Frames file ");
        let _ = stderr.write_all(frm_file);
        let _ = stderr.write_all(b" not found or invalid\n");
    };
    let path = os_path(frm_file);
    let data = if path.is_dir() {
        Vec::new()
    } else {
        match std::fs::read(&path) {
            Ok(data) => data,
            Err(_) => {
                let _ = stderr.write_all(b"Unable to open frm file: ");
                let _ = stderr.write_all(frm_file);
                let _ = stderr.write_all(b"\n");
                not_found(stderr);
                return Err(1);
            }
        }
    };
    let mut warnings = Vec::new();
    let is_valid = |address: u32| {
        valid_in.is_none_or(|part| part.is_valid_frame_address(FrameAddress(address)))
    };
    let result = Frames::read_frm_checked(
        &data,
        words_per_frame,
        &mut |w| {
            warnings.extend_from_slice(w.as_bytes());
            warnings.push(b'\n');
        },
        valid_in.map(|_| &is_valid as &dyn Fn(u32) -> bool),
    );
    let _ = stderr.write_all(&warnings);
    result.map_err(|e| match (e.kind, valid_in) {
        (FrmErrorKind::InvalidFrame(address), Some(part)) => {
            let _ = writeln!(
                stderr,
                "Frames file contains an invalid frame: {}",
                cpp_frame_address(part.architecture, FrameAddress(address))
            );
            not_found(stderr);
            1
        }
        (kind, _) => {
            let _ = stderr.write_all(stoul_terminate(kind).as_bytes());
            ABORT
        }
    })
}

/// Runs `xc7frames2bit` with `args` (without `argv[0]`); returns the exit
/// code ([`ABORT`] for an abort).
pub fn run(
    argv0: &[u8],
    args: &[Vec<u8>],
    env: &Env,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    run_tool(Tool::Xc7frames2bit, argv0, args, env, stdout, stderr)
}

/// Runs `tool` with `args` (without `argv[0]`); returns the exit code
/// ([`ABORT`] for an abort).
pub fn run_tool(
    tool: Tool,
    argv0: &[u8],
    args: &[Vec<u8>],
    env: &Env,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let program = Program {
        argv0: argv0.to_vec(),
        usage: argv0.to_vec(),
        flags: flags_of(tool),
        gflags: tool.gflags(),
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
    let format = match architecture(
        tool.name(),
        tool.is_prjuray(),
        parsed.string("architecture"),
    ) {
        Ok(format) => format,
        Err(ArchitectureError::Unsupported(message)) => {
            let _ = stderr.write_all(message.as_bytes());
            return 1;
        }
        Err(ArchitectureError::Abort(message)) => {
            let _ = stderr.write_all(message.as_bytes());
            return ABORT;
        }
    };
    let part_file = parsed.string("part_file");
    let part = match read_part(part_file, format.addressing) {
        Ok(part) => part,
        Err(PartError::Abort(message)) => {
            let _ = stderr.write_all(message.as_bytes());
            return ABORT;
        }
        Err(PartError::Invalid) => {
            let _ = stderr.write_all(b"Part file ");
            let _ = stderr.write_all(part_file);
            let _ = stderr.write_all(b" not found or invalid\n");
            return 1;
        }
    };
    let frm_file = parsed.string("frm_file");
    let valid_in = tool.is_prjuray().then_some(&part);
    let frames = match read_frames(frm_file, format.words_per_frame, valid_in, stderr) {
        Ok(frames) => frames,
        Err(code) => return code,
    };
    write_frames(
        &frames,
        &part,
        &format,
        frm_file,
        parsed.string("part_name"),
        parsed.string("output_file"),
        env,
        stderr,
    )
    .code
}

#[cfg(test)]
mod tests {
    use super::*;
    use fasm_xilinx::FrameAddressFields;

    #[test]
    fn frame_address_printing() {
        let usp = Architecture::UltraScalePlus;
        let a = FrameAddress::compose(
            usp,
            FrameAddressFields {
                block_type: 1,
                bottom: true,
                row: 2,
                column: 5,
                minor: 200,
            },
        )
        .unwrap();
        assert_eq!(
            cpp_frame_address(usp, a),
            "[ 0x18805c8]  Row=34 Column= 5 Minor=200 Type=Block RAM"
        );
        assert_eq!(
            cpp_frame_address(usp, FrameAddress(0)),
            "[         0]  Row= 0 Column= 0 Minor= 0 Type=CLB/IO/CLK"
        );
        assert_eq!(
            cpp_frame_address(Architecture::Series7, FrameAddress(0x0042_0100)),
            "[  0x420100] BOTTOM Row= 1 Column= 2 Minor= 0 Type=CLB/IO/CLK"
        );
        assert_eq!(
            cpp_frame_address(Architecture::UltraScale, FrameAddress(0x0380_0000)),
            "[ 0x3800000]  Row= 0 Column= 0 Minor= 0 Type="
        );
    }

    #[test]
    fn architectures() {
        let prjxray = |name: &[u8]| architecture("t", false, name);
        let prjuray = |name: &[u8]| architecture("t", true, name);
        for arch in Architecture::ALL {
            let name = arch.name().as_bytes();
            assert_eq!(prjxray(name), Ok(BitstreamFormat::prjxray(arch)));
            assert_eq!(prjuray(name), Ok(BitstreamFormat::native(arch)));
        }
        assert_eq!(
            prjxray(b"Foo"),
            Ok(BitstreamFormat::native(Architecture::Series7))
        );
        assert_eq!(
            prjuray(b"Foo"),
            Err(ArchitectureError::Abort(BAD_VARIANT_TERMINATE.to_owned()))
        );
        assert!(matches!(
            prjuray(b"Spartan6"),
            Err(ArchitectureError::Unsupported(_))
        ));
    }
}

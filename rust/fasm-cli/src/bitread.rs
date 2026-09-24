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

//! The `bitread` tool: prjxray's `tools/bitread.cc` (`.bit` -> frames,
//! printed as text).
//!
//! ```text
//! bitread --part_file=<part.yaml> [-o out] [-x|-y|-p] [-z] [-C]
//!     [-f frame] [-F first:last] [--aux file] [bitfile]
//! ```
//!
//! gflags style flags ([`crate::gflags`]) and the same output, messages
//! and exit codes as the reference for every flag (`-c` is accepted and
//! ignored, like in the reference). Extension: `--frm_out=<file>` writes
//! the selected frames as a `.frm` file (ECC bits cleared unless `-C`),
//! the inverse of `xc7frames2bit`. Only the Series7 architecture is
//! implemented; see the `bitread` section of `docs/rewrite/COMPAT.md`.

#![forbid(unsafe_code)]

use std::io::{BufWriter, Read, Write};

use fasm_xilinx::bitstream::{BitstreamReader, Configuration};
use fasm_xilinx::{Architecture, FrameAddress, Frames};

use crate::gflags::{self, Flag, FlagType, Outcome, Program};
use crate::xc7frames2bit::{architecture, os_path, read_part, Env};

const FILE: &str = "tools/bitread.cc";
/// The file the help lists the Rust only flags under.
const EXTENSION_FILE: &str = "rust/fasm-cli/src/bitread_extensions.rs";

const X_HELP: &str = "use format 'bit_%%08x_%%03d_%%02d_t%%d_h%%d_r%%d_c%%d_m%%d'\n\
The fields have the following meaning:\n  \
- complete 32 bit hex frame id\n  \
- word index with that frame (decimal)\n  \
- bit index with that word (decimal)\n  \
- decoded frame type from frame id\n  \
- decoded top/botttom from frame id (top=0)\n  \
- decoded row address from frame id\n  \
- decoded column address from frame id\n  \
- decoded minor address from frame id\n";

/// The flags of `bitread` (and the `--frm_out` extension).
pub fn flags() -> Vec<Flag> {
    use FlagType::{Bool, Int32, String};
    let f = |name, ty, default, help| Flag {
        name,
        file: FILE,
        ty,
        default,
        help,
    };
    vec![
        f("c", Bool, "false", "output '*' for repeating patterns"),
        f("C", Bool, "false", "do not ignore the checksum in each frame"),
        f(
            "f",
            Int32,
            "-1",
            "only dump the specified frame (might be used more than once)",
        ),
        f(
            "F",
            String,
            "",
            "<first_frame_address>:<last_frame_address> only dump frame in the specified range",
        ),
        f(
            "o",
            String,
            "",
            "write machine-readable output file with config frames",
        ),
        f("p", Bool, "false", "output a binary netpgm image"),
        f("x", Bool, "false", X_HELP),
        f("y", Bool, "false", "use format 'bit_%%08x_%%03d_%%02d'"),
        f(
            "z",
            Bool,
            "false",
            "skip zero frames (frames with all bits cleared) in o",
        ),
        f("part_file", String, "", "YAML file describing a Xilinx part"),
        f(
            "architecture",
            String,
            "Series7",
            "Architecture of the provided bitstream",
        ),
        f(
            "aux",
            String,
            "",
            "write machine-readable output file with auxiliary bitstream data",
        ),
        Flag {
            name: "frm_out",
            file: EXTENSION_FILE,
            ty: String,
            default: "",
            help: "write the frames selected by -z, -f and -F to this file in the .frm format of fasm2frames and xc7frames2bit (the ECC bits are cleared unless -C) [Rust extension]",
        },
    ]
}

/// C `strtol(s, nullptr, 0)`: leading white space, a sign, a `0x` (hex)
/// or `0` (octal) prefix, the longest run of digits; saturates on
/// overflow; 0 without digits.
fn strtol0(s: &[u8]) -> i64 {
    let mut rest = s;
    while let Some((&c, tail)) = rest.split_first() {
        if matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
            rest = tail;
        } else {
            break;
        }
    }
    let mut negative = false;
    if let Some((&c, tail)) = rest.split_first() {
        if c == b'+' || c == b'-' {
            negative = c == b'-';
            rest = tail;
        }
    }
    let base = if rest.len() >= 3
        && rest[0] == b'0'
        && (rest[1] | 0x20) == b'x'
        && rest[2].is_ascii_hexdigit()
    {
        rest = &rest[2..];
        16
    } else if rest.first() == Some(&b'0') {
        8
    } else {
        10
    };
    let mut value: i128 = 0;
    for &c in rest {
        let Some(d) = (c as char).to_digit(base) else {
            break;
        };
        value = (value * i128::from(base) + i128::from(d)).min(i128::from(i64::MAX) + 1);
    }
    let value = if negative { -value } else { value };
    value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

/// The frame selection of `-z`, `-f` and `-F`.
struct Selection {
    skip_zero: bool,
    frame: Option<u32>,
    range: (u32, u32),
}

impl Selection {
    fn selects(&self, address: u32, words: &[u32], words_per_frame: usize) -> bool {
        if self.skip_zero && words.len() == words_per_frame && words.iter().all(|&w| w == 0) {
            return false;
        }
        if self.frame.is_some_and(|f| f != address) {
            return false;
        }
        let (begin, end) = self.range;
        !(begin != end && (address < begin || end <= address))
    }
}

/// The `--aux` file (`PrintHeader`, `PrintFpgaConfigurationLogicData`,
/// `PrintFrameAddresses`).
fn aux_text(bytes: &[u8], reader: &BitstreamReader, config: &Configuration<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(sync) = bytes.windows(4).position(|w| w == [0xAA, 0x99, 0x55, 0x66]) {
        out.extend_from_slice(b"Header bytes:");
        for b in &bytes[..sync + 4] {
            out.extend_from_slice(format!(" {b:02X}").as_bytes());
        }
        out.push(b'\n');
    }
    let words = reader.words();
    let wcfg = words
        .windows(2)
        .position(|w| w == [0x3000_8001, 0x1])
        .unwrap_or(words.len());
    out.extend_from_slice(b"FPGA configuration logic prefix:");
    for w in &words[..wcfg] {
        out.extend_from_slice(format!(" {w:08X}").as_bytes());
    }
    out.push(b'\n');
    let null = words
        .windows(2)
        .rposition(|w| w == [0x3000_8001, 0x0])
        .unwrap_or(words.len());
    out.extend_from_slice(b"FPGA configuration logic suffix:");
    for w in &words[null..] {
        out.extend_from_slice(format!(" {w:08X}").as_bytes());
    }
    out.push(b'\n');
    out.extend_from_slice(b"Frame addresses in bitstream: ");
    let n = config.len();
    for (i, (address, _)) in config.frames().enumerate() {
        out.extend_from_slice(format!("{address:08X}").as_bytes());
        out.push(if i + 1 == n { b'\n' } else { b' ' });
    }
    out
}

/// Runs the tool with `args` (without `argv[0]`), reading the bitstream
/// from `stdin` when there is not exactly one positional argument; returns
/// the exit code.
///
/// The output is streamed (through a buffer) to `stdout` or the `-o`
/// file frame by frame, never held in memory as a whole. Like the
/// reference, whose `std::endl` flushes them, stdout is flushed after the
/// `Bitstream size`, `Config size` and `Number of configuration frames`
/// lines and before anything is written to `stderr`, so that the two
/// streams merged (`2>&1`) come out in the reference's order.
pub fn run(
    argv0: &[u8],
    args: &[Vec<u8>],
    env: &Env,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let mut out = BufWriter::with_capacity(1 << 16, stdout);
    let code = run_streamed(argv0, args, env, stdin, &mut out, stderr);
    let _ = out.flush();
    code
}

/// Writes `message` to `stderr` after flushing `stdout`.
fn error(stdout: &mut dyn Write, stderr: &mut dyn Write, message: &[&[u8]]) {
    let _ = stdout.flush();
    for part in message {
        let _ = stderr.write_all(part);
    }
    let _ = stderr.flush();
}

/// A line written with `std::endl`: written and flushed.
fn endl_line(stdout: &mut dyn Write, line: &str) {
    let _ = stdout.write_all(line.as_bytes());
    let _ = stdout.flush();
}

fn run_streamed(
    argv0: &[u8],
    args: &[Vec<u8>],
    env: &Env,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let mut usage = b"Usage: ".to_vec();
    usage.extend_from_slice(argv0);
    usage.extend_from_slice(b" [options] [bitfile]");
    let program = Program {
        argv0: argv0.to_vec(),
        usage,
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
            error(stdout, stderr, &[&err]);
            return code;
        }
    };
    let frame = u32::try_from(parsed.int32("f")).ok();
    let range = if parsed.string("F").is_empty() {
        (0, 0)
    } else {
        let spec = parsed.string("F");
        let mut pieces = spec.split(|&c| c == b':');
        let first = pieces.next().unwrap_or_default();
        let second = pieces.next().unwrap_or_default();
        (
            strtol0(first) as u32,
            strtol0(second).wrapping_add(1) as u32,
        )
    };
    let selection = Selection {
        skip_zero: parsed.bool("z"),
        frame,
        range,
    };

    let bytes = if parsed.args.len() == 1 {
        let name = &parsed.args[0];
        let path = os_path(name);
        match std::fs::read(&path) {
            Ok(bytes) if !path.is_dir() => bytes,
            _ => {
                error(
                    stdout,
                    stderr,
                    &[b"Can't open input file '", name, b"' for reading!\n"],
                );
                return 1;
            }
        }
    } else {
        let mut bytes = Vec::new();
        let _ = stdin.read_to_end(&mut bytes);
        bytes
    };
    endl_line(stdout, &format!("Bitstream size: {} bytes\n", bytes.len()));

    if let Err(message) = architecture("bitread", parsed.string("architecture")) {
        error(stdout, stderr, &[message.as_bytes()]);
        return 1;
    }
    let arch = Architecture::Series7;
    let wpf = arch.words_per_frame();
    let Some(reader) = BitstreamReader::from_bytes(&bytes) else {
        error(stdout, stderr, &[b"Input doesn't look like a bitstream\n"]);
        return 1;
    };
    endl_line(
        stdout,
        &format!("Config size: {} words\n", reader.words().len()),
    );
    let Some(part) = read_part(parsed.string("part_file")) else {
        error(stdout, stderr, &[b"Part file not found or invalid\n"]);
        return 1;
    };
    let Ok(config) = reader.configuration(&part) else {
        error(
            stdout,
            stderr,
            &[b"Bitstream does not appear to be for this part\n"],
        );
        return 1;
    };
    endl_line(
        stdout,
        &format!("Number of configuration frames: {}\n", config.len()),
    );

    let o = parsed.string("o").to_vec();
    let mut file = None;
    if o.is_empty() {
        let _ = stdout.write_all(b"\n");
    } else {
        match std::fs::File::create(os_path(&o)) {
            Ok(f) => file = Some(BufWriter::with_capacity(1 << 16, f)),
            Err(_) => {
                let _ = stdout.write_all(b"Can't open output file '");
                let _ = stdout.write_all(&o);
                let _ = stdout.write_all(b"' for writing!\n");
                return 1;
            }
        }
    }
    let aux = parsed.string("aux");
    if !aux.is_empty() {
        let text = aux_text(&bytes, &reader, &config);
        let written = std::fs::File::create(os_path(aux)).map(|mut f| f.write_all(&text));
        if written.is_err() {
            let _ = stdout.write_all(b"Can't open aux output file '");
            let _ = stdout.write_all(aux);
            let _ = stdout.write_all(b"' for writing!\n");
            return 1;
        }
    }

    // `f` is stdout or the -o file; each frame is formatted into `chunk`
    // and written out.
    let to_stdout = file.is_none();
    let big_c = parsed.bool("C");
    let (x, y, p) = (parsed.bool("x"), parsed.bool("y"), parsed.bool("p"));
    let mut pgmdata: Vec<&[u32]> = Vec::new();
    let mut pgmsep: Vec<usize> = Vec::new();
    let mut chunk: Vec<u8> = Vec::with_capacity(1 << 16);
    let mut write_failed = false;
    {
        let f: &mut dyn Write = match file.as_mut() {
            Some(file) => file,
            None => &mut *stdout,
        };
        for (address, words) in config.frames() {
            if !selection.selects(address, words, wpf) {
                continue;
            }
            chunk.clear();
            format_frame(
                &mut chunk,
                address,
                words,
                FrameFormat {
                    to_stdout,
                    big_c,
                    x,
                    y,
                    p,
                },
            );
            if p {
                let minor = FrameAddress(address).minor(arch);
                if minor == 0 && !pgmdata.is_empty() {
                    pgmsep.push(pgmdata.len());
                }
                pgmdata.push(words);
            }
            write_failed |= f.write_all(&chunk).is_err();
        }
        if p {
            write_failed |= write_pgm(f, &pgmdata, &pgmsep, wpf).is_err();
        }
        write_failed |= f.flush().is_err();
    }
    if file.is_some() && write_failed {
        error(stdout, stderr, &[b"Error writing '", &o, b"'\n"]);
        return 1;
    }
    drop(file);

    let frm_out = parsed.string("frm_out");
    if !frm_out.is_empty() {
        let mut frames = Frames::new(wpf);
        for (address, words) in config.to_frames(!big_c, false).iter() {
            let original = config.get(address).unwrap_or_default();
            if selection.selects(address, original, wpf) {
                frames.insert_if_absent(address, words);
            }
        }
        let written = std::fs::File::create(os_path(frm_out)).and_then(|mut f| {
            let mut buffered = BufWriter::new(&mut f);
            frames.write_frm(&mut buffered)?;
            buffered.flush()
        });
        if written.is_err() {
            let _ = stdout.write_all(b"Can't open .frm output file '");
            let _ = stdout.write_all(frm_out);
            let _ = stdout.write_all(b"' for writing!\n");
            return 1;
        }
    }
    let _ = stdout.write_all(b"DONE\n");
    0
}

/// How [`format_frame`] prints a frame.
#[derive(Clone, Copy)]
struct FrameFormat {
    to_stdout: bool,
    big_c: bool,
    x: bool,
    y: bool,
    p: bool,
}

/// The text of one frame (everything but the `-p` image).
fn format_frame(f: &mut Vec<u8>, address: u32, words: &[u32], format: FrameFormat) {
    let fields = FrameAddress(address).fields(Architecture::Series7);
    if format.to_stdout {
        f.extend_from_slice(
            format!(
                "Frame 0x{address:08x} (Type={} Top={} Row={} Column={} Minor={}):\n",
                fields.block_type,
                u8::from(fields.bottom),
                fields.row,
                fields.column,
                fields.minor
            )
            .as_bytes(),
        );
    }
    if format.p {
        return;
    }
    if format.x || format.y {
        // `bit_%08x_%03d_%02d` (+ `_t%d_h%d_r%d_c%d_m%d` for -x),
        // formatted by hand: this is most of bitread's run time.
        let prefix = format!("bit_{address:08x}_").into_bytes();
        let suffix = if format.x {
            format!(
                "_t{}_h{}_r{}_c{}_m{}\n",
                fields.block_type,
                u8::from(fields.bottom),
                fields.row,
                fields.column,
                fields.minor
            )
            .into_bytes()
        } else {
            b"\n".to_vec()
        };
        for (i, &word) in words.iter().enumerate() {
            let mut bits = word;
            if i == 50 && !format.big_c {
                bits &= !0x1FFF;
            }
            while bits != 0 {
                let k = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                f.extend_from_slice(&prefix);
                push_decimal(f, i, 3);
                f.push(b'_');
                push_decimal(f, k, 2);
                f.extend_from_slice(&suffix);
            }
        }
        if format.to_stdout {
            f.push(b'\n');
        }
    } else {
        if !format.to_stdout {
            f.extend_from_slice(format!(".frame 0x{address:08x}\n").as_bytes());
        }
        for (i, &word) in words.iter().enumerate() {
            let value = if i != 50 || format.big_c {
                word
            } else {
                word & 0xFFFF_E000
            };
            f.extend_from_slice(format!("{value:08x}").as_bytes());
            f.extend_from_slice(if i % 6 == 5 { b"\n" } else { b" " });
        }
        f.extend_from_slice(b"\n\n");
    }
}

/// Appends `value` as `%0<width>d`.
fn push_decimal(f: &mut Vec<u8>, value: usize, width: usize) {
    let mut digits = [0u8; 20];
    let mut n = value;
    let mut len = 0;
    loop {
        digits[len] = b'0' + (n % 10) as u8;
        len += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for _ in len..width {
        f.push(b'0');
    }
    f.extend(digits[..len].iter().rev());
}

/// The `-p` netpgm image, a literal port, written row by row. A frame's
/// pixels are the bits of its words, bit `k` of word `i` being pixel
/// `32 * i + k`.
fn write_pgm(
    f: &mut dyn Write,
    pgmdata: &[&[u32]],
    pgmsep: &[usize],
    wpf: usize,
) -> std::io::Result<()> {
    let word_length = 32usize;
    let width = pgmdata.len() + pgmsep.len();
    let height = wpf * word_length;
    f.write_all(format!("P5 {width} {height} 15\n").as_bytes())?;
    let mut row: Vec<u8> = Vec::with_capacity(width + 1);
    let mut y = 0usize;
    let mut bit = height as isize - 1;
    while y < height {
        row.clear();
        let (mut x, mut frame, mut sep) = (0usize, 0usize, 0usize);
        while x < width {
            if sep < pgmsep.len() && frame == pgmsep[sep] {
                row.push(8);
                x += 1;
                sep += 1;
            }
            let data = pgmdata.get(frame).copied().unwrap_or_default();
            let set = usize::try_from(bit).ok().and_then(|b| {
                data.get(b / word_length)
                    .map(|w| w & (1 << (b % word_length)) != 0)
            });
            row.push(match set {
                Some(true) => 15,
                _ => 0,
            });
            x += 1;
            frame += 1;
        }
        f.write_all(&row)?;
        if bit.rem_euclid(word_length as isize) == 0 && y != 0 {
            f.write_all(&vec![8u8; width])?;
            y += 1;
        }
        y += 1;
        bit -= 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtol_base_zero() {
        assert_eq!(strtol0(b"0x00400100"), 0x0040_0100);
        assert_eq!(strtol0(b" 12abc"), 12);
        assert_eq!(strtol0(b"010"), 8);
        assert_eq!(strtol0(b"-5"), -5);
        assert_eq!(strtol0(b""), 0);
        assert_eq!(strtol0(b"0x"), 0);
        assert_eq!(strtol0(b"99999999999999999999"), i64::MAX);
        assert_eq!(strtol0(b"-99999999999999999999"), i64::MIN);
    }
}

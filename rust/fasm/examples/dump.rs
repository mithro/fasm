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

//! `dump`: prints a machine readable JSON dump of a FASM file's parse
//! tree, in the *same* shape as `tests/oracle/dump.py`, using the Rust
//! `fasm` crate instead of the Python oracle. Used by `tools/difftest.py`
//! (T1.5) to differentially test the Rust parser/printer against the
//! original Python implementation.
//!
//! ```text
//! dump FILE                        # {"lines": [...]} or {"error": "..."}
//! dump --to-string [--canonical] FILE
//!                                   # fasm_tuple_to_string(parse(FILE), canonical)
//! ```
//!
//! The JSON dump mode writes `{"lines": [{"set_feature": ..., "annotations":
//! ..., "comment": ...}, ...]}` (or `{"error": "<message>"}` on any parse
//! error) with object keys sorted and no extra whitespace, matching
//! `json.dump(doc, sort_keys=True, separators=(',', ':'))` byte for byte,
//! including `ensure_ascii=True` escaping of non-ASCII characters
//! (`tests/oracle/dump.py` relies on the `json` module's default). Message
//! *text* inside `"error"` is the Rust parser's own (see
//! `docs/rewrite/COMPAT.md`, "Errors"): callers compare structure/behaviour,
//! not that text, against the oracle.
//!
//! The `--to-string` mode prints `fasm::fasm_tuple_to_string(&lines,
//! canonical)` verbatim (that function's own trailing `\n` included, no
//! extra one added) to stdout, mirroring what
//! `fasm.fasm_tuple_to_string(model, canonical)` prints when the oracle
//! side is run through a `python -c` snippet that does the same. A parse
//! error is reported on stderr and exits with status 1.

use std::env;
use std::fmt::Write as _;
use std::process::ExitCode;

use fasm::model::{Annotation, FasmLine, SetFasmFeature};
use fasm::output::fasm_tuple_to_string;
use fasm::parser::{parse_fasm_filename, ParseError};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut to_string = false;
    let mut canonical = false;
    let mut file: Option<String> = None;
    for arg in args {
        match arg.as_str() {
            "--to-string" => to_string = true,
            "--canonical" => canonical = true,
            other if !other.starts_with('-') => file = Some(other.to_string()),
            other => {
                eprintln!("dump: unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(file) = file else {
        eprintln!("usage: dump [--to-string] [--canonical] FILE");
        return ExitCode::from(2);
    };

    if to_string {
        run_to_string(&file, canonical)
    } else {
        run_dump(&file)
    }
}

fn run_to_string(file: &str, canonical: bool) -> ExitCode {
    let lines = match parse_fasm_filename(file) {
        Ok(lines) => lines,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    match fasm_tuple_to_string(&lines, canonical) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_dump(file: &str) -> ExitCode {
    let mut out = String::new();
    match parse_fasm_filename(file) {
        Ok(lines) => {
            out.push_str("{\"lines\":[");
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                dump_line(&mut out, line);
            }
            out.push_str("]}");
        }
        Err(e) => {
            out.push_str("{\"error\":");
            json_string(&mut out, &error_message(&e));
            out.push('}');
        }
    }
    out.push('\n');
    print!("{out}");
    ExitCode::SUCCESS
}

/// What `dump --to-string`/`--parser` comparisons treat as "the error
/// message": the `Display` text of the [`ParseError`] (`"Parse error at
/// {line}:{column} - {message}"`), same shape as what the ANTLR wrapper
/// raises, but with the Rust parser's own message text.
fn error_message(e: &ParseError) -> String {
    e.to_string()
}

fn dump_line(out: &mut String, line: &FasmLine) {
    out.push('{');
    // Sorted keys: annotations, comment, set_feature.
    out.push_str("\"annotations\":");
    dump_annotations(out, line.annotations.as_deref());
    out.push_str(",\"comment\":");
    match &line.comment {
        Some(c) => json_string(out, c),
        None => out.push_str("null"),
    }
    out.push_str(",\"set_feature\":");
    dump_set_feature(out, line.set_feature.as_ref());
    out.push('}');
}

fn dump_annotations(out: &mut String, annotations: Option<&[Annotation]>) {
    match annotations {
        None => out.push_str("null"),
        Some(list) => {
            out.push('[');
            for (i, a) in list.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                // Sorted keys: name, value.
                out.push('{');
                out.push_str("\"name\":");
                json_string(out, &a.name);
                out.push_str(",\"value\":");
                json_string(out, &a.value);
                out.push('}');
            }
            out.push(']');
        }
    }
}

fn dump_set_feature(out: &mut String, set_feature: Option<&SetFasmFeature>) {
    match set_feature {
        None => out.push_str("null"),
        Some(f) => {
            // Sorted keys: end, feature, start, value, value_format.
            out.push('{');
            out.push_str("\"end\":");
            dump_opt_u32(out, f.end);
            out.push_str(",\"feature\":");
            json_string(out, &f.feature.resolve());
            out.push_str(",\"start\":");
            dump_opt_u32(out, f.start);
            out.push_str(",\"value\":");
            json_string(out, &f.value.to_string());
            out.push_str(",\"value_format\":");
            match f.value_format {
                Some(vf) => json_string(out, vf.python_name()),
                None => out.push_str("null"),
            }
            out.push('}');
        }
    }
}

fn dump_opt_u32(out: &mut String, v: Option<u32>) {
    match v {
        Some(v) => {
            let _ = write!(out, "{v}");
        }
        None => out.push_str("null"),
    }
}

/// Writes `s` as a JSON string literal, matching Python's
/// `json.dumps(s, ensure_ascii=True)`: ASCII printable characters verbatim
/// (with `"`, `\` and control characters escaped per the JSON spec), and
/// every non-ASCII Unicode scalar value as `\uXXXX` (surrogate pairs for
/// codepoints above `U+FFFF`).
fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c if (c as u32) < 0x7F => out.push(c),
            c => {
                let cp = c as u32;
                if cp <= 0xFFFF {
                    let _ = write!(out, "\\u{cp:04x}");
                } else {
                    // Encode as a UTF-16 surrogate pair, like Python's
                    // json module does for astral codepoints.
                    let v = cp - 0x1_0000;
                    let hi = 0xD800 + (v >> 10);
                    let lo = 0xDC00 + (v & 0x3FF);
                    let _ = write!(out, "\\u{hi:04x}\\u{lo:04x}");
                }
            }
        }
    }
    out.push('"');
}

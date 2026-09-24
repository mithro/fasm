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

//! The behaviour of the original `fasm/tool.py` `main()`:
//!
//! ```python
//! args = parser.parse_args()
//! try:
//!     fasm_parser = get_fasm_parser(args.parser)
//!     fasm_tuples = fasm_parser.parse_fasm_filename(args.file)
//!     print(fasm_tuple_to_string(fasm_tuples, args.canonical))
//! except Exception as e:
//!     print('Error: ' + str(e))
//! ```
//!
//! The whole file is parsed before anything is printed, so an invalid file
//! prints only its `Error: ...` line (on stdout, exit code 0), like the
//! original. The FASM text is rendered while it is parsed (in one pass,
//! without keeping the parsed lines) into an output buffer that is written
//! to stdout once the parse has succeeded.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::ops::Range;

use fasm::output::{try_canonical_features, write_set_feature};
use fasm::parser::{parse_lines, ParseError, ParseErrorKind};
use fasm::{FasmLine, OutputError};

use crate::argparse::{self, Parsed};
use crate::pystr::PyStr;

/// The `--parser` values that select the (only) Rust parser: the original
/// tool's `antlr` and `textx`, and `rust` (the name the Rust based Python
/// package gives it).
const PARSER_NAMES: [&str; 3] = ["antlr", "textx", "rust"];

/// The UTF-8 encoded byte order mark (skipped at the start of a file).
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Runs the tool with the command line arguments `args` (without the
/// program name), writing to `stdout` and `stderr`, and returns the exit
/// code. `columns` gives the terminal width for the help and usage
/// messages (see [`crate::terminal::columns`]); it is only called when
/// one of them is printed.
pub fn run(
    args: &[PyStr],
    columns: impl FnOnce() -> i64,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let namespace = match argparse::parse_args(args) {
        Parsed::Run(namespace) => namespace,
        Parsed::Help => {
            // argparse ignores errors writing the help (`_print_message`).
            let _ = stdout.write_all(argparse::format_help(columns()).as_bytes());
            let _ = stdout.flush();
            return 0;
        }
        Parsed::Error(message) => {
            let _ = stderr.write_all(argparse::format_error(&message, columns()).as_bytes());
            let _ = stderr.flush();
            return 2;
        }
    };

    let output = match process(&namespace) {
        Ok(output) => output,
        Err(message) => {
            let mut line = b"Error: ".to_vec();
            line.extend_from_slice(&message);
            line.push(b'\n');
            line
        }
    };
    match stdout.write_all(&output).and_then(|()| stdout.flush()) {
        Ok(()) => 0,
        // Python dies of a `BrokenPipeError` (exit code 1, with a
        // traceback on stderr) when the reader of its stdout has gone
        // away; exit with 1 as quietly as other Rust tools.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => 1,
        Err(e) => {
            let _ = writeln!(stderr, "fasm: error writing to stdout: {e}");
            1
        }
    }
}

/// The body of the `try` block: the complete stdout output on success, or
/// the `str(e)` of the error (as bytes, encoded like Python's stdout).
fn process(namespace: &argparse::Namespace) -> Result<Vec<u8>, Vec<u8>> {
    if let Some(parser) = &namespace.parser {
        if !PARSER_NAMES.iter().any(|name| parser.eq_str(name)) {
            let mut message = b"Parser '".to_vec();
            message.extend_from_slice(&parser.encode_surrogateescape());
            message.extend_from_slice(b"' is not available.");
            return Err(message);
        }
    }
    let path = namespace.file.to_os_string();
    let data = std::fs::read(&path).map_err(|e| {
        let path = std::path::Path::new(&path);
        let error = ParseError::new(
            0,
            0,
            ParseErrorKind::Io,
            format!("Couldn't open file {}: {e}", path.display()),
        );
        error.to_string().into_bytes()
    })?;
    render(&data, namespace.canonical)
        .map(String::into_bytes)
        .map_err(String::into_bytes)
}

/// Renders the FASM file `data` like
/// `print(fasm_tuple_to_string(parse_fasm_filename(file), canonical))`:
/// the returned text ends with the extra newline of `print`.
///
/// # Errors
///
/// The `str(e)` of the error the original tool would print: the
/// [`ParseError`] to report (see [`error_to_report`]), or an
/// [`OutputError`] (which the parser's output never triggers).
pub fn render(data: &[u8], canonical: bool) -> Result<String, String> {
    let result = if canonical {
        render_canonical(data)
    } else {
        render_lines(data)
    };
    result.map_err(|failure| match failure {
        Failure::Parse(error) => error_to_report(data, error).to_string(),
        Failure::Output(error) => error.to_string(),
    })
}

enum Failure {
    Parse(ParseError),
    Output(OutputError),
}

impl From<ParseError> for Failure {
    fn from(e: ParseError) -> Self {
        Failure::Parse(e)
    }
}

impl From<OutputError> for Failure {
    fn from(e: OutputError) -> Self {
        Failure::Output(e)
    }
}

/// `fasm_tuple_to_string(lines, canonical=False)` + `print`'s newline:
/// every line followed by `\n` (a lone `\n` for no lines), then `\n`.
fn render_lines(data: &[u8]) -> Result<String, Failure> {
    let mut out = String::with_capacity(data.len() + 2);
    let mut any = false;
    for line in parse_lines(data) {
        write_line(&mut out, &line?)?;
        out.push('\n');
        any = true;
    }
    if !any {
        out.push('\n');
    }
    out.push('\n');
    Ok(out)
}

/// `fasm_line_to_string(line, canonical=False)`: the feature, the
/// annotation block and the comment, joined by single spaces.
fn write_line(out: &mut String, line: &FasmLine) -> Result<(), OutputError> {
    let mut empty = true;
    if let Some(set_feature) = &line.set_feature {
        write_set_feature(out, set_feature, false)?;
        empty = false;
    }
    if let Some(annotations) = line.annotations.as_ref().filter(|a| !a.is_empty()) {
        if !empty {
            out.push(' ');
        }
        out.push_str("{ ");
        for (i, annotation) in annotations.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            write!(out, "{} = \"{}\"", annotation.name, annotation.value)?;
        }
        out.push_str(" }");
        empty = false;
    }
    if let Some(comment) = &line.comment {
        if !empty {
            out.push(' ');
        }
        out.push('#');
        out.push_str(comment);
    }
    Ok(())
}

/// `fasm_tuple_to_string(lines, canonical=True)` + `print`'s newline: the
/// sorted, deduplicated canonical feature lines.
fn render_canonical(data: &[u8]) -> Result<String, Failure> {
    // All canonical lines, back to back, and the range of each.
    let mut arena = String::new();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for line in parse_lines(data) {
        if let Some(set_feature) = &line?.set_feature {
            for feature in try_canonical_features(set_feature)? {
                let start = arena.len();
                write_set_feature(&mut arena, &feature, true)?;
                ranges.push(start..arena.len());
            }
        }
    }
    // Python's `sorted(set(lines))` compares code points, which is the
    // byte order of UTF-8.
    ranges.sort_unstable_by(|a, b| arena[a.clone()].cmp(&arena[b.clone()]));
    ranges.dedup_by(|a, b| arena[a.clone()] == arena[b.clone()]);
    let mut out = String::with_capacity(arena.len() + ranges.len() + 2);
    for range in &ranges {
        out.push_str(&arena[range.clone()]);
        out.push('\n');
    }
    if ranges.is_empty() {
        out.push('\n');
    }
    out.push('\n');
    Ok(out)
}

/// `true` for the errors the original ANTLR based parser only detects
/// after it has parsed the whole file (its `assert`s on the decoded
/// values), so that a syntax error anywhere in the file is reported
/// instead.
fn is_detected_after_parse(kind: ParseErrorKind) -> bool {
    matches!(
        kind,
        ParseErrorKind::ValueExceedsAddressWidth
            | ParseErrorKind::ValueExceedsDeclaredWidth
            | ParseErrorKind::AddressEndBeforeStart
    )
}

/// The error the tool reports for a file whose first error (in file
/// order) is `first`.
///
/// The original (ANTLR) parser checks the syntax of the whole file before
/// it decodes any value, so a syntax error later in the file wins over an
/// earlier value range error. This does the same: after a value range
/// error, parsing resumes after that line; the first later error that is
/// not a value range error is reported, if there is one, and `first`
/// otherwise.
#[must_use]
pub fn error_to_report(data: &[u8], first: ParseError) -> ParseError {
    let mut error = first.clone();
    while is_detected_after_parse(error.kind) {
        // A value range error is only reported for a syntactically valid
        // line, located at its value or its `[`.
        let Some(position) = byte_offset(data, error.line, error.column) else {
            return first;
        };
        let end = logical_line_end(data, position);
        if end >= data.len() {
            return first;
        }
        // Resume at the line terminator, so that the rest is not taken for
        // the start of a file (no byte order mark skipping, ANTLR's first
        // line error recovery).
        let skipped = &data[position..end];
        let (end_line, end_column) = match skipped.iter().rposition(|&b| b == b'\n') {
            None => (error.line, error.column + code_points(skipped)),
            Some(last) => (
                error.line + skipped.iter().filter(|&&b| b == b'\n').count(),
                code_points(&skipped[last + 1..]),
            ),
        };
        let Some(next) = parse_lines(&data[end..]).find_map(Result::err) else {
            return first;
        };
        error = if next.line == 1 {
            ParseError::new(end_line, end_column + next.column, next.kind, next.message)
        } else {
            ParseError::new(
                end_line + next.line - 1,
                next.column,
                next.kind,
                next.message,
            )
        };
    }
    error
}

/// Number of code points in `bytes` as the parser counts columns (UTF-8
/// continuation bytes do not count).
fn code_points(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b & 0xC0 != 0x80).count()
}

/// The byte offset of the parser position `line`:`column`.
fn byte_offset(data: &[u8], line: usize, column: usize) -> Option<usize> {
    let mut start = if data.starts_with(UTF8_BOM) {
        UTF8_BOM.len()
    } else {
        0
    };
    for _ in 1..line {
        start += data.get(start..)?.iter().position(|&b| b == b'\n')? + 1;
    }
    let mut position = start;
    let mut seen = 0;
    while seen < column {
        position += 1;
        while data.get(position).is_some_and(|&b| b & 0xC0 == 0x80) {
            position += 1;
        }
        seen += 1;
    }
    (position <= data.len()).then_some(position)
}

/// The offset of the line terminator (`\n` or `\r`) ending the
/// syntactically valid FASM line that `position` is in, before any
/// annotation or comment (or `data.len()`). Line terminators inside
/// annotation values do not end the line.
fn logical_line_end(data: &[u8], position: usize) -> usize {
    let mut i = position;
    while let Some(&b) = data.get(i) {
        match b {
            b'\n' | b'\r' => return i,
            b'#' => {
                return data[i..]
                    .iter()
                    .position(|&b| b == b'\n' || b == b'\r')
                    .map_or(data.len(), |n| i + n);
            }
            b'"' => {
                i += 1;
                while let Some(&c) = data.get(i) {
                    match c {
                        b'\\' => i += 2,
                        b'"' => break,
                        _ => i += 1,
                    }
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    data.len()
}

#[cfg(test)]
mod tests;

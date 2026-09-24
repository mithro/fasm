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

//! The FASM parser: a hand written byte scanner producing
//! [`FasmLine`]s.
//!
//! It accepts everything the original ANTLR based parser
//! (`fasm.parser.antlr`) accepts and produces the same [`FasmLine`]s for
//! it, plus a few cases where only the textX parser and the specification
//! accept the input (`_` separators in plain decimal values and
//! addresses); where the ANTLR implementation is plainly buggy (32 bit
//! truncation, whitespace after `'h`, ...) it does the sane thing. Every
//! such difference is listed in `docs/rewrite/COMPAT.md`.
//!
//! Like both original parsers, lines with neither a feature, annotations
//! nor a comment (blank lines) produce no [`FasmLine`]. `\n` and `\r` both
//! end a line (so `\r\n` line endings work), but only `\n` increments the
//! line number reported in a [`ParseError`], like ANTLR. A UTF-8 byte
//! order mark at the start of the input is skipped, like ANTLR does.
//!
//! Entry points:
//!
//! * [`parse_fasm_string`], [`parse_fasm_bytes`], [`parse_fasm_filename`]:
//!   parse a whole input into a `Vec<FasmLine>`, stopping at the first
//!   error;
//! * [`parse_lines`]: a streaming iterator ([`Lines`]) over a byte
//!   buffer, which does not copy the input;
//! * [`parse_line`]: parses a single line.
//!
//! Feature names are interned with [`IdString::new`](crate::idstring::IdString::new);
//! parsing a feature line allocates nothing else unless its value is wider
//! than 256 bits. Comments and annotations allocate their text.
//!
//! ```
//! use fasm::parser::parse_fasm_string;
//!
//! let lines = parse_fasm_string("A.B[3:0] = 4'b1010 # comment\n").unwrap();
//! let feature = lines[0].set_feature.as_ref().unwrap();
//! assert_eq!(feature.feature, "A.B");
//! assert_eq!((feature.start, feature.end), (Some(0), Some(3)));
//! assert_eq!(feature.value, 10u64);
//! assert_eq!(lines[0].comment.as_deref(), Some(" comment"));
//!
//! let err = parse_fasm_string("A.B = 1\nC = 2\n").unwrap_err();
//! assert_eq!((err.line, err.column), (2, 4));
//! ```

#![forbid(unsafe_code)]

mod error;
mod line;
mod number;

use std::path::Path;

pub use error::{ParseError, ParseErrorKind};

use crate::model::FasmLine;
use line::{parse_logical_line, RawError};

/// The UTF-8 encoded byte order mark.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Parses FASM text. Stops at the first error.
///
/// # Errors
///
/// Returns the first [`ParseError`] of the input.
pub fn parse_fasm_string(s: &str) -> Result<Vec<FasmLine>, ParseError> {
    parse_fasm_bytes(s.as_bytes())
}

/// Parses FASM text given as bytes. Stops at the first error.
///
/// The input does not have to be UTF-8: only comments and annotation
/// values (the only places where non-ASCII characters are allowed) must
/// be valid UTF-8.
///
/// # Errors
///
/// Returns the first [`ParseError`] of the input.
pub fn parse_fasm_bytes(bytes: &[u8]) -> Result<Vec<FasmLine>, ParseError> {
    parse_lines(bytes).collect()
}

/// Reads and parses a FASM file. Stops at the first error.
///
/// # Errors
///
/// Returns the first [`ParseError`] of the file, or a
/// [`ParseErrorKind::Io`] error (at line 0, column 0, like the ANTLR
/// parser's "Couldn't open file") if it cannot be read.
pub fn parse_fasm_filename(path: impl AsRef<Path>) -> Result<Vec<FasmLine>, ParseError> {
    let data = std::fs::read(path.as_ref()).map_err(|e| {
        ParseError::new(
            0,
            0,
            ParseErrorKind::Io,
            format!("Couldn't open file {}: {e}", path.as_ref().display()),
        )
    })?;
    parse_fasm_bytes(&data)
}

/// Returns a streaming iterator over the [`FasmLine`]s of `bytes`.
///
/// See [`Lines`].
#[must_use]
pub fn parse_lines(bytes: &[u8]) -> Lines<'_> {
    Lines::new(bytes, 1)
}

/// Parses a single FASM line; `line_no` (1 based) is used for the
/// position of errors.
///
/// Returns `Ok(None)` for a blank line. A trailing line terminator (`\n`,
/// `\r\n`, `\r`) is allowed, and so is a leading UTF-8 byte order mark
/// when `line_no` is 1. Input holding more than one non blank line
/// (a `\n` or `\r` in the middle, outside of an annotation value) is an
/// error: use [`parse_lines`] for that.
///
/// # Errors
///
/// Returns the [`ParseError`] of the line.
pub fn parse_line(bytes: &[u8], line_no: usize) -> Result<Option<FasmLine>, ParseError> {
    let mut lines = Lines::new(bytes, line_no);
    let first = match lines.next() {
        None => return Ok(None),
        Some(first) => first?,
    };
    let before_second = lines.clone();
    match lines.next() {
        None => Ok(Some(first)),
        Some(Err(e)) => Err(e),
        Some(Ok(_)) => Err(before_second.error(RawError {
            pos: lines.item_start,
            kind: ParseErrorKind::Syntax,
            message: "more than one FASM line given to parse_line()".to_string(),
        })),
    }
}

/// Streaming iterator over the [`FasmLine`]s of a byte buffer, created by
/// [`parse_lines`].
///
/// Blank lines are skipped. After an error has been returned the iterator
/// is exhausted (it returns `None`).
#[derive(Clone, Debug)]
pub struct Lines<'a> {
    buf: &'a [u8],
    /// Offset of the next logical line.
    pos: usize,
    /// Line number (ANTLR style: counting `\n` only) of `line_start`.
    line_no: usize,
    /// Offset of the first byte after the last `\n` before `pos` (or 0).
    line_start: usize,
    /// Offset of the logical line returned last.
    item_start: usize,
    /// Line number of the logical line returned last.
    item_line: usize,
    /// Offset of the first line of the file, if `buf` starts with it
    /// (`line_no` 1); see `parse_logical_line`.
    first_line: Option<usize>,
    done: bool,
}

impl<'a> Lines<'a> {
    /// Iterator over `buf`, whose first line has number `line_no`. A UTF-8
    /// byte order mark at the start of line 1 is skipped (the ANTLR input
    /// stream does that too); columns then count from after it.
    fn new(buf: &'a [u8], line_no: usize) -> Self {
        let start = if line_no == 1 && buf.starts_with(UTF8_BOM) {
            UTF8_BOM.len()
        } else {
            0
        };
        Lines {
            buf,
            pos: start,
            line_no,
            line_start: start,
            item_start: start,
            item_line: line_no,
            first_line: (line_no == 1).then_some(start),
            done: false,
        }
    }

    /// The line number (1 based, counting `\n` like [`ParseError::line`])
    /// on which the [`FasmLine`] returned last by [`Iterator::next`]
    /// starts. Before the first call, the number of the first line.
    #[must_use]
    pub fn line_number(&self) -> usize {
        self.item_line
    }

    /// Converts an error located by byte offset into a [`ParseError`]
    /// with ANTLR style line and column.
    fn error(&self, raw: RawError) -> ParseError {
        let pos = raw.pos.min(self.buf.len());
        let from = self.line_start.min(pos);
        let mut line = self.line_no;
        let mut column_start = from;
        for (i, &b) in self
            .buf
            .get(from..pos)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            if b == b'\n' {
                line += 1;
                column_start = from + i + 1;
            }
        }
        // Code points: count the bytes that are not UTF-8 continuation
        // bytes.
        let column = self
            .buf
            .get(column_start..pos)
            .unwrap_or_default()
            .iter()
            .filter(|&&b| b & 0xC0 != 0x80)
            .count();
        ParseError::new(line, column, raw.kind, raw.message)
    }
}

impl Iterator for Lines<'_> {
    type Item = Result<FasmLine, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.done {
            // Fast path over empty lines.
            while let Some(&b @ (b'\n' | b'\r')) = self.buf.get(self.pos) {
                self.pos += 1;
                if b == b'\n' {
                    self.line_no += 1;
                    self.line_start = self.pos;
                }
            }
            let start = self.pos;
            let line_no = self.line_no;
            let first_line = self.first_line == Some(start);
            let outcome = match parse_logical_line(self.buf, start, first_line) {
                Ok(outcome) => outcome,
                Err(raw) => {
                    self.done = true;
                    return Some(Err(self.error(raw)));
                }
            };
            if let Some(last) = outcome.last_newline {
                self.line_no += outcome.newlines;
                self.line_start = last + 1;
            }
            match self.buf.get(outcome.end) {
                None => self.done = true,
                Some(&terminator) => {
                    self.pos = outcome.end + 1;
                    if terminator == b'\n' {
                        self.line_no += 1;
                        self.line_start = self.pos;
                    }
                }
            }
            if let Some(line) = outcome.line {
                self.item_start = start;
                self.item_line = line_no;
                return Some(Ok(line));
            }
        }
        None
    }
}

impl std::iter::FusedIterator for Lines<'_> {}

#[cfg(test)]
mod tests;

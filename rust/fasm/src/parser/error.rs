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

//! [`ParseError`] and [`ParseErrorKind`].

use std::fmt;

/// The class of a [`ParseError`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum ParseErrorKind {
    /// The input does not match the FASM grammar (unexpected character,
    /// unexpected end of line, unterminated annotation value, ...).
    Syntax,
    /// A `FeatureAddress` number is larger than `u32::MAX`, or a
    /// `[end:start]` range is `2^32` bits wide.
    AddressOutOfRange,
    /// A `[end:start]` `FeatureAddress` whose end is smaller than its
    /// start.
    AddressEndBeforeStart,
    /// A Verilog value does not fit in its declared width
    /// (`4'h1F`).
    ValueExceedsDeclaredWidth,
    /// A value does not fit in the width of the `FeatureAddress` (1 bit
    /// without an address or with a single bit address, `end - start + 1`
    /// bits for a range).
    ValueExceedsAddressWidth,
    /// A comment or an annotation value is not valid UTF-8.
    InvalidUtf8,
    /// The input file could not be read (only from
    /// [`super::parse_fasm_filename`]; `line` and `column` are then `0`).
    Io,
}

/// An error found while parsing FASM text.
///
/// `Display` produces `Parse error at {line}:{column} - {message}`, the
/// format of the exception raised by the original ANTLR based Python
/// parser (`fasm.parser.antlr`). The message texts themselves are this
/// crate's own (see `docs/rewrite/COMPAT.md`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ParseError {
    /// Line of the error, 1 based. Lines are counted like ANTLR does: only
    /// `\n` starts a new line (a lone `\r` ends a FASM line but does not
    /// increment the line number). `0` for [`ParseErrorKind::Io`].
    pub line: usize,
    /// Column of the error, 0 based, in Unicode code points (invalid UTF-8
    /// bytes count one each) since the last `\n`: ANTLR's
    /// `charPositionInLine`. `0` for [`ParseErrorKind::Io`].
    pub column: usize,
    /// The class of the error.
    pub kind: ParseErrorKind,
    /// Human readable description of the error.
    pub message: String,
}

impl ParseError {
    /// Creates a new `ParseError`.
    #[must_use]
    pub fn new(
        line: usize,
        column: usize,
        kind: ParseErrorKind,
        message: impl Into<String>,
    ) -> Self {
        ParseError {
            line,
            column,
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Parse error at {}:{} - {}",
            self.line, self.column, self.message
        )
    }
}

impl std::error::Error for ParseError {}

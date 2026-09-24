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

//! The byte scanner: parses one logical FASM line starting at a byte
//! offset of the input buffer.
//!
//! The grammar is the ANTLR one (`src/antlr/FasmLexer.g4` and
//! `FasmParser.g4` of the original implementation), with the deliberate
//! differences listed in `docs/rewrite/COMPAT.md`:
//!
//! ```text
//! line        := S* set_feature? S* annotations? S* comment?  (then \n, \r or EOF)
//! set_feature := FEATURE S* address? S* ('=' S* value)?
//! FEATURE     := IDENT ('.' IDENT)*        IDENT := [a-zA-Z][0-9a-zA-Z_]*
//! address     := '[' S* ADDR S* (':' S* ADDR S*)? ']'
//! ADDR        := [0-9]+ ('_' [0-9]+)*       (at most u32::MAX)
//! value       := ([0-9]+ S*)? "'" base S* DIGITS | PLAIN
//! base DIGITS := 'h' [0-9a-fA-F_]+ | 'b' [01_]+ | 'd' [0-9_]+ | 'o' [0-7_]+
//! PLAIN       := [0-9]+ ('_' [0-9]+)*
//! annotations := '{' S* ann (S* ',' S* ann)* S* '}'
//! ann         := [.a-zA-Z][0-9a-zA-Z_]* S* '=' S* '"' ([^\\"] | '\\' [\\"])* '"'
//! comment     := '#' [^\n\r]*
//! S           := ' ' | '\t'
//! ```
//!
//! Annotation values may contain line terminators (both original parsers
//! accept that), so a logical line can span several physical lines.

use super::error::ParseErrorKind;
use super::number;
use crate::idstring::IdString;
use crate::model::{Annotation, FasmLine, FeatureValue, SetFasmFeature, ValueFormat};

/// `[a-zA-Z]`: first character of an identifier.
const IDENT_START: u8 = 1 << 0;
/// `[0-9a-zA-Z_]`: other characters of an identifier.
const IDENT_CONT: u8 = 1 << 1;
/// `[0-9]`.
const DIGIT: u8 = 1 << 2;
/// `[0-9a-fA-F_]`: hexadecimal Verilog digits.
const HEX: u8 = 1 << 3;
/// `[01_]`: binary Verilog digits.
const BIN: u8 = 1 << 4;
/// `[0-9_]`: decimal Verilog digits.
const DEC: u8 = 1 << 5;
/// `[0-7_]`: octal Verilog digits.
const OCT: u8 = 1 << 6;
/// `[.a-zA-Z]`: first character of an annotation name.
const ANN_START: u8 = 1 << 7;

/// Character class of every byte (bit set of the constants above).
static CLASS: [u8; 256] = build_class_table();

const fn build_class_table() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let b = i as u8;
        let mut c = 0;
        let alpha = b.is_ascii_alphabetic();
        let digit = b.is_ascii_digit();
        if alpha {
            c |= IDENT_START | IDENT_CONT | ANN_START;
        }
        if digit {
            c |= IDENT_CONT | DIGIT | DEC;
        }
        if b == b'_' {
            c |= IDENT_CONT | HEX | BIN | DEC | OCT;
        }
        if b.is_ascii_hexdigit() {
            c |= HEX;
        }
        if b == b'0' || b == b'1' {
            c |= BIN;
        }
        if b >= b'0' && b <= b'7' {
            c |= OCT;
        }
        if b == b'.' {
            c |= ANN_START;
        }
        t[i] = c;
        i += 1;
    }
    t
}

/// A parse error located by byte offset in the scanned buffer; converted
/// to a line/column [`super::ParseError`] by the caller.
#[derive(Debug)]
pub(super) struct RawError {
    pub(super) pos: usize,
    pub(super) kind: ParseErrorKind,
    pub(super) message: String,
}

/// Result of parsing one logical line.
pub(super) struct LineOutcome {
    /// `None` for a line with no feature, annotation or comment.
    pub(super) line: Option<FasmLine>,
    /// Offset of the line terminator (`\n` or `\r`) ending the line, or
    /// the buffer length.
    pub(super) end: usize,
    /// Number of `\n` inside annotation values of the line.
    pub(super) newlines: usize,
    /// Offset of the last of those `\n`, if any.
    pub(super) last_newline: Option<usize>,
}

/// Parses the logical line of `buf` starting at `start`. `text` is `buf`
/// as a `str` if the whole buffer is valid UTF-8 (then text is sliced
/// out of it without validating it again). `first_line` is `true` for the
/// first line of a file (it only changes the position of some errors, see
/// [`Scanner::unexpected_la`]).
pub(super) fn parse_logical_line<'a>(
    buf: &'a [u8],
    text: Option<&'a str>,
    start: usize,
    first_line: bool,
) -> Result<LineOutcome, RawError> {
    let mut s = Scanner {
        buf,
        text,
        pos: start,
        newlines: 0,
        last_newline: None,
        pending: None,
        first_line,
    };
    let line = s.line()?;
    Ok(LineOutcome {
        line,
        end: s.pos,
        newlines: s.newlines,
        last_newline: s.last_newline,
    })
}

/// A value as found in the input, before conversion.
struct RawValue<'a> {
    /// The digits (`_` separators included).
    digits: &'a [u8],
    /// Bits per digit (1, 3 or 4), or 0 for decimal.
    bits: u32,
    format: ValueFormat,
    /// The declared width of a Verilog value (saturated to `u64::MAX`).
    declared_width: Option<u64>,
}

/// Text for a value in an error message: the decimal value, or its bit
/// length and leading hexadecimal digits when it is wider than 256 bits
/// (a message must stay short, and decimal rendering is quadratic).
fn value_text(value: &FeatureValue) -> String {
    let bits = value.bit_len();
    if bits <= 256 {
        format!("value {value}")
    } else {
        let top = value.shr(bits - 64).to_u64().unwrap_or(0);
        format!("{bits} bit value 0x{top:x}...")
    }
}

/// The lexer mode ANTLR is in when it lexes a token (see
/// [`Scanner::unexpected_la`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Outside of `{ ... }`.
    Default,
    /// Inside of `{ ... }`.
    Annotation,
}

/// What was parsed so far on a line; selects the "expected ..." text of
/// an error at the end of the line.
#[derive(Clone, Copy)]
enum After {
    Nothing,
    Feature,
    Address,
    Value,
    Annotations,
}

impl After {
    fn expected(self) -> &'static str {
        match self {
            After::Nothing => "a feature name, '{', '#' or end of line",
            After::Feature => "'[', '=', '{', '#' or end of line",
            After::Address => "'=', '{', '#' or end of line",
            After::Value => "'{', '#' or end of line",
            After::Annotations => "'#' or end of line",
        }
    }
}

struct Scanner<'a> {
    buf: &'a [u8],
    /// `buf` as a `str`, if it is valid UTF-8.
    text: Option<&'a str>,
    pos: usize,
    newlines: usize,
    last_newline: Option<usize>,
    /// The first range error (address or value out of range) of the line.
    /// Reported only once the whole line is syntactically valid, because
    /// the ANTLR parser checks these after parsing (so a syntax error
    /// later on the same line takes precedence).
    pending: Option<RawError>,
    /// Whether this is the first line of the file.
    first_line: bool,
}

impl<'a> Scanner<'a> {
    /// Records a range error, reported at the end of the line unless a
    /// syntax error comes first (see [`Scanner::pending`]).
    fn defer(&mut self, error: RawError) {
        if self.pending.is_none() {
            self.pending = Some(error);
        }
    }

    #[inline]
    fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    #[inline]
    fn peek_at(&self, pos: usize) -> Option<u8> {
        self.buf.get(pos).copied()
    }

    #[inline]
    fn class(&self) -> u8 {
        self.peek().map_or(0, |b| CLASS[usize::from(b)])
    }

    #[inline]
    fn class_at(&self, pos: usize) -> u8 {
        self.peek_at(pos).map_or(0, |b| CLASS[usize::from(b)])
    }

    #[inline]
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    #[inline]
    fn skip_class(&mut self, mask: u8) {
        let rest = self.buf.get(self.pos..).unwrap_or_default();
        self.pos += rest
            .iter()
            .position(|&b| CLASS[usize::from(b)] & mask == 0)
            .unwrap_or(rest.len());
    }

    #[inline]
    fn slice(&self, from: usize, to: usize) -> &'a [u8] {
        self.buf.get(from..to).unwrap_or_default()
    }

    #[inline]
    fn at_line_end(&self) -> bool {
        matches!(self.peek(), None | Some(b'\n' | b'\r'))
    }

    fn error(&self, pos: usize, kind: ParseErrorKind, message: impl Into<String>) -> RawError {
        RawError {
            pos,
            kind,
            message: message.into(),
        }
    }

    /// A syntax error at the current position: "unexpected X, expected Y".
    fn unexpected(&self, expected: &str) -> RawError {
        self.error(
            self.pos,
            ParseErrorKind::Syntax,
            format!(
                "unexpected {}, expected {expected}",
                describe(self.buf, self.pos)
            ),
        )
    }

    /// Like [`Scanner::unexpected`], but emulating where ANTLR reports the
    /// error: on an unexpected token T, ANTLR's error recovery (single
    /// token deletion) lexes the token after T before reporting. If that
    /// token cannot be lexed, the lexer error is reported first, at its
    /// position. This matters when the token after T is lexed in
    /// annotation mode, where not every character starts a token: when T
    /// is inside `{ ... }` (`mode` is [`Mode::Annotation`]), or when T is
    /// the `{` opening an annotation block.
    ///
    /// Only for the error points where ANTLR does look ahead: every token
    /// match and the entry of `( ... )*` loops, but not their loop back
    /// (the `,` loop of an annotation block after its second annotation,
    /// the line loop after the first line).
    fn unexpected_la(&self, expected: &str, mode: Mode) -> RawError {
        let t = self.pos;
        // End of T, if the token after it is lexed in annotation mode.
        let end = match (mode, self.peek_at(t)) {
            (Mode::Default, Some(b'{')) | (Mode::Annotation, Some(b'=' | b',')) => Some(t + 1),
            (Mode::Annotation, Some(b)) if CLASS[usize::from(b)] & ANN_START != 0 => {
                let mut p = t + 1;
                while self.class_at(p) & IDENT_CONT != 0 {
                    p += 1;
                }
                Some(p)
            }
            (Mode::Annotation, Some(b'"')) => self.lex_annotation_value(t),
            // Default mode tokens other than `{`, `}` (the token after it
            // is lexed in default mode), end of input, or T itself cannot
            // be lexed (the error is at T).
            _ => None,
        };
        let Some(mut p) = end else {
            return self.unexpected(expected);
        };
        while matches!(self.peek_at(p), Some(b' ' | b'\t')) {
            p += 1;
        }
        let lexable = match self.peek_at(p) {
            None | Some(b'=' | b',' | b'}') => true,
            Some(b'"') => self.lex_annotation_value(p).is_some(),
            Some(b) => CLASS[usize::from(b)] & ANN_START != 0,
        };
        if lexable {
            return self.unexpected(expected);
        }
        let message = if self.peek_at(p) == Some(b'"') {
            format!(
                "unterminated annotation value or invalid escape sequence (after unexpected \
                 {}, expected {expected})",
                describe(self.buf, t)
            )
        } else {
            format!(
                "unexpected {} inside an annotation (after unexpected {}, expected {expected})",
                describe(self.buf, p),
                describe(self.buf, t)
            )
        };
        self.error(p, ParseErrorKind::Syntax, message)
    }

    /// Lexes an annotation value starting with the `"` at `quote` like the
    /// ANTLR lexer; returns the offset after the closing `"`, or `None` if
    /// the value is unterminated or holds an invalid escape sequence.
    fn lex_annotation_value(&self, quote: usize) -> Option<usize> {
        let mut p = quote + 1;
        loop {
            match self.peek_at(p)? {
                b'"' => return Some(p + 1),
                b'\\' => match self.peek_at(p + 1)? {
                    b'\\' | b'"' => p += 2,
                    _ => return None,
                },
                _ => p += 1,
            }
        }
    }

    /// Validates `bytes` (found at offset `base`) as UTF-8.
    fn utf8(&self, base: usize, end: usize, what: &str) -> Result<Box<str>, RawError> {
        self.str(base, end, what).map(Box::from)
    }

    /// The text from `base` to `end` as a `str`: sliced from
    /// [`Scanner::text`] when the input is known to be UTF-8 (`get` only
    /// checks that both ends are character boundaries), validated
    /// otherwise.
    #[inline]
    fn str(&self, base: usize, end: usize, what: &str) -> Result<&'a str, RawError> {
        if let Some(s) = self.text.and_then(|t| t.get(base..end)) {
            return Ok(s);
        }
        match std::str::from_utf8(self.slice(base, end)) {
            Ok(s) => Ok(s),
            Err(e) => Err(self.error(
                base + e.valid_up_to(),
                ParseErrorKind::InvalidUtf8,
                format!("{what} is not valid UTF-8"),
            )),
        }
    }

    fn line(&mut self) -> Result<Option<FasmLine>, RawError> {
        self.skip_ws();
        let mut after = After::Nothing;

        let set_feature = if self.class() & IDENT_START != 0 {
            let (feature, a) = self.set_feature()?;
            after = a;
            Some(feature)
        } else {
            None
        };

        let annotations = if self.peek() == Some(b'{') {
            let annotations = self.annotations()?;
            after = After::Annotations;
            self.skip_ws();
            Some(annotations)
        } else {
            None
        };

        let comment = if self.peek() == Some(b'#') {
            Some(self.comment()?)
        } else {
            None
        };

        if !self.at_line_end() {
            return Err(if self.first_line {
                self.unexpected_la(after.expected(), Mode::Default)
            } else {
                self.unexpected(after.expected())
            });
        }
        if let Some(error) = self.pending.take() {
            return Err(error);
        }

        if set_feature.is_none() && annotations.is_none() && comment.is_none() {
            return Ok(None);
        }
        Ok(Some(FasmLine {
            set_feature,
            annotations,
            comment,
        }))
    }

    /// `FEATURE S* address? S* ('=' S* value)? S*`; the current byte is an
    /// identifier start.
    fn set_feature(&mut self) -> Result<(SetFasmFeature, After), RawError> {
        let feature_start = self.pos;
        loop {
            // At an identifier start.
            self.pos += 1;
            self.skip_class(IDENT_CONT);
            if self.peek() == Some(b'.') && self.class_at(self.pos + 1) & IDENT_START != 0 {
                self.pos += 1;
            } else {
                break;
            }
        }
        // ASCII by construction, so never an error.
        let name = self.str(feature_start, self.pos, "feature name")?;
        let feature = IdString::new(name);
        let mut after = After::Feature;
        self.skip_ws();

        let (start, end) = if self.peek() == Some(b'[') {
            after = After::Address;
            let address = self.address()?;
            self.skip_ws();
            address
        } else {
            (None, None)
        };

        let mut value = FeatureValue::from_u64(1);
        let mut value_format = None;
        if self.peek() == Some(b'=') {
            after = After::Value;
            self.pos += 1;
            self.skip_ws();
            let value_pos = self.pos;
            let raw = self.value()?;
            self.skip_ws();

            // With an error already pending the line is rejected anyway:
            // the value is not needed.
            if self.pending.is_none() {
                if let Some(v) = self.convert_value(&raw, value_pos, start, end) {
                    value = v;
                }
            }
            value_format = Some(raw.format);
        }

        Ok((
            SetFasmFeature::new_unchecked(feature, start, end, value, value_format),
            after,
        ))
    }

    /// Converts the value found at `value_pos` and checks, like the ANTLR
    /// decoder, that it fits in its declared width (when it has one other
    /// than 0: `if width:` in `antlr_to_tuple.pyx`) and in the address
    /// width (1 bit without an address or with a single bit address).
    ///
    /// Errors are deferred to the end of the line (and `None` returned).
    /// The cost is linear in the number of digits: decimal values are
    /// limited to [`number::MAX_DECIMAL_DIGITS`] digits, and a value whose
    /// digit count alone shows it is too wide is not converted at all.
    fn convert_value(
        &mut self,
        raw: &RawValue<'a>,
        value_pos: usize,
        start: Option<u32>,
        end: Option<u32>,
    ) -> Option<FeatureValue> {
        let declared = raw.declared_width.filter(|&w| w > 0);
        let address_width = match (start, end) {
            (Some(s), Some(e)) => u64::from(e).saturating_sub(u64::from(s)) + 1,
            _ => 1,
        };

        let significant = raw
            .digits
            .iter()
            .filter(|&&b| b != b'_')
            .skip_while(|&&b| b == b'0')
            .count() as u64;
        if raw.bits == 0 && significant > number::MAX_DECIMAL_DIGITS as u64 {
            self.defer(self.error(
                value_pos,
                ParseErrorKind::DecimalValueTooLong,
                format!(
                    "decimal value has {significant} significant digits, more than the limit \
                     of {}",
                    number::MAX_DECIMAL_DIGITS
                ),
            ));
            return None;
        }

        // Lower bound of the bit length from the digit count: a value with
        // n significant digits is at least radix^(n-1).
        let min_bits = match (significant, raw.bits) {
            (0, _) => 0,
            // 3.321928 < log2(10).
            (n, 0) => (n - 1).saturating_mul(3_321_928) / 1_000_000 + 1,
            (n, bits) => (n - 1).saturating_mul(u64::from(bits)) + 1,
        };
        // Upper bound of the bit length.
        let max_bits = match raw.bits {
            // log2(10) < 3.33.
            0 => significant.saturating_mul(333) / 100 + 1,
            bits => significant.saturating_mul(u64::from(bits)),
        };
        let too_wide = |bits: u64| {
            if declared.is_some_and(|w| bits > w) {
                Some(ParseErrorKind::ValueExceedsDeclaredWidth)
            } else if bits > address_width {
                Some(ParseErrorKind::ValueExceedsAddressWidth)
            } else {
                None
            }
        };
        let limit_text = |kind| match (kind, declared) {
            (ParseErrorKind::ValueExceedsDeclaredWidth, Some(w)) => {
                format!("the declared width of {w} bit(s)")
            }
            _ => format!("the {address_width} bit(s) addressed by the feature"),
        };

        // For values wider than 256 bits (short ones are cheap to convert,
        // and then get an exact message), report from the bounds when the
        // exact bit length would give the same error (the declared width
        // is checked first).
        let early = match too_wide(min_bits).filter(|_| min_bits > 256) {
            Some(ParseErrorKind::ValueExceedsAddressWidth)
                if declared.is_some_and(|w| max_bits > w) =>
            {
                None
            }
            kind => kind,
        };
        if let Some(kind) = early {
            let text = format!(
                "value with {significant} significant digit(s) (at least {min_bits} bits) does \
                 not fit in {}",
                limit_text(kind)
            );
            self.defer(self.error(value_pos, kind, text));
            return None;
        }

        let value = if raw.bits == 0 {
            number::decimal(raw.digits)
        } else {
            number::power_of_two(raw.digits, raw.bits)
        };
        if let Some(kind) = too_wide(u64::from(value.bit_len())) {
            let text = format!(
                "{} does not fit in {}",
                value_text(&value),
                limit_text(kind)
            );
            self.defer(self.error(value_pos, kind, text));
            return None;
        }
        Some(value)
    }

    /// `'[' S* ADDR S* (':' S* ADDR S*)? ']'`; returns `(start, end)`.
    /// Range errors are deferred to the end of the line.
    fn address(&mut self) -> Result<(Option<u32>, Option<u32>), RawError> {
        let open = self.pos;
        self.pos += 1;
        self.skip_ws();
        let first = self.address_number()?;
        self.skip_ws();
        let second = if self.peek() == Some(b':') {
            self.pos += 1;
            self.skip_ws();
            let second = self.address_number()?;
            self.skip_ws();
            Some(second)
        } else {
            None
        };
        if self.peek() != Some(b']') {
            let expected = if second.is_some() {
                "']'"
            } else {
                "':' or ']'"
            };
            return Err(self.unexpected_la(expected, Mode::Default));
        }
        self.pos += 1;

        match second {
            None => Ok((Some(first), None)),
            Some(start) => {
                let end = first;
                if end < start {
                    self.defer(self.error(
                        open,
                        ParseErrorKind::AddressEndBeforeStart,
                        format!(
                            "feature address [{end}:{start}] has its end ({end}) before its \
                             start ({start})"
                        ),
                    ));
                } else if start == 0 && end == u32::MAX {
                    self.defer(self.error(
                        open,
                        ParseErrorKind::AddressOutOfRange,
                        format!("feature address [{end}:{start}] is wider than 2^32 - 1 bits"),
                    ));
                }
                Ok((Some(start), Some(end)))
            }
        }
    }

    /// `[0-9]+ ('_' [0-9]+)*`, at most `u32::MAX`.
    fn address_number(&mut self) -> Result<u32, RawError> {
        let start = self.pos;
        if self.class() & DIGIT == 0 {
            return Err(self.unexpected_la("a decimal number", Mode::Default));
        }
        let mut v: u64 = 0;
        loop {
            while let Some(b) = self.peek().filter(u8::is_ascii_digit) {
                v = v.saturating_mul(10).saturating_add(u64::from(b - b'0'));
                self.pos += 1;
            }
            if self.peek() == Some(b'_') && self.class_at(self.pos + 1) & DIGIT != 0 {
                self.pos += 1;
            } else {
                break;
            }
        }
        match u32::try_from(v) {
            Ok(v) => Ok(v),
            Err(_) => {
                // Keep the message short for absurdly long numbers.
                let text = self.slice(start, self.pos);
                let shown = String::from_utf8_lossy(text.get(..24).unwrap_or(text));
                let more = if text.len() > 24 { "..." } else { "" };
                self.defer(self.error(
                    start,
                    ParseErrorKind::AddressOutOfRange,
                    format!("feature address {shown}{more} is larger than {}", u32::MAX),
                ));
                Ok(u32::MAX)
            }
        }
    }

    /// A value after `=`, not converted yet (see [`Scanner::convert_value`]).
    fn value(&mut self) -> Result<RawValue<'a>, RawError> {
        match self.peek() {
            Some(b'0'..=b'9') => {
                let digits_start = self.pos;
                let mut first_underscore = None;
                loop {
                    self.skip_class(DIGIT);
                    if self.peek() == Some(b'_') && self.class_at(self.pos + 1) & DIGIT != 0 {
                        first_underscore.get_or_insert(self.pos);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                let digits = self.slice(digits_start, self.pos);
                let digits_end = self.pos;
                self.skip_ws();
                if self.peek() == Some(b'\'') {
                    if let Some(underscore) = first_underscore {
                        return Err(self.error(
                            underscore,
                            ParseErrorKind::Syntax,
                            "'_' is not allowed in the width of a Verilog value",
                        ));
                    }
                    let width = digits.iter().fold(0u64, |w, &b| {
                        w.saturating_mul(10)
                            .saturating_add(u64::from(b.wrapping_sub(b'0')))
                    });
                    self.verilog_value(Some(width))
                } else {
                    self.pos = digits_end;
                    Ok(RawValue {
                        digits,
                        bits: 0,
                        format: ValueFormat::Plain,
                        declared_width: None,
                    })
                }
            }
            Some(b'\'') => self.verilog_value(None),
            _ => Err(self.unexpected_la("a value", Mode::Default)),
        }
    }

    /// `"'" base S* DIGITS`; the current byte is `'`.
    fn verilog_value(&mut self, declared_width: Option<u64>) -> Result<RawValue<'a>, RawError> {
        let quote = self.pos;
        let (mask, format, bits, name) = match self.peek_at(quote + 1) {
            Some(b'h') => (HEX, ValueFormat::VerilogHex, 4, "hexadecimal"),
            Some(b'b') => (BIN, ValueFormat::VerilogBinary, 1, "binary"),
            Some(b'd') => (DEC, ValueFormat::VerilogDecimal, 0, "decimal"),
            Some(b'o') => (OCT, ValueFormat::VerilogOctal, 3, "octal"),
            _ => {
                return Err(self.error(
                    quote,
                    ParseErrorKind::Syntax,
                    format!(
                        "invalid Verilog value: expected 'h', 'b', 'd' or 'o' after \"'\", found \
                         {}",
                        describe(self.buf, quote + 1)
                    ),
                ))
            }
        };
        self.pos = quote + 2;
        self.skip_ws();
        let digits_start = self.pos;
        self.skip_class(mask);
        if self.pos == digits_start {
            return Err(self.error(
                quote,
                ParseErrorKind::Syntax,
                format!(
                    "invalid Verilog value: expected {name} digits, found {}",
                    describe(self.buf, self.pos)
                ),
            ));
        }
        Ok(RawValue {
            digits: self.slice(digits_start, self.pos),
            bits,
            format,
            declared_width,
        })
    }

    /// `'{' S* ann (S* ',' S* ann)* S* '}'`; the current byte is `{`.
    fn annotations(&mut self) -> Result<Vec<Annotation>, RawError> {
        self.pos += 1;
        let mut annotations = Vec::new();
        loop {
            self.skip_ws();
            if self.class() & ANN_START == 0 {
                return Err(self.unexpected_la("an annotation name", Mode::Annotation));
            }
            let name_start = self.pos;
            self.pos += 1;
            self.skip_class(IDENT_CONT);
            let name = self.utf8(name_start, self.pos, "annotation name")?;
            self.skip_ws();
            if self.peek() != Some(b'=') {
                return Err(self.unexpected_la("'=' after the annotation name", Mode::Annotation));
            }
            self.pos += 1;
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.unexpected_la("'\"' (an annotation value)", Mode::Annotation));
            }
            let value = self.annotation_value()?;
            annotations.push(Annotation { name, value });
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(annotations);
                }
                // ANTLR looks ahead at the entry of the `(',' annotation)*`
                // loop (after the first annotation) but not at its loop
                // back.
                _ if annotations.len() == 1 => {
                    return Err(self.unexpected_la("',' or '}'", Mode::Annotation))
                }
                _ => return Err(self.unexpected("',' or '}'")),
            }
        }
    }

    /// `'"' ([^\\"] | '\\' [\\"])* '"'`; the current byte is `"`. The
    /// value is kept verbatim (escape sequences are not decoded).
    fn annotation_value(&mut self) -> Result<Box<str>, RawError> {
        let quote = self.pos;
        self.pos += 1;
        let value_start = self.pos;
        loop {
            match self.peek() {
                None => {
                    return Err(self.error(
                        quote,
                        ParseErrorKind::Syntax,
                        "unterminated annotation value",
                    ))
                }
                Some(b'"') => break,
                Some(b'\\') => match self.peek_at(self.pos + 1) {
                    Some(b'\\' | b'"') => self.pos += 2,
                    _ => {
                        return Err(self.error(
                            quote,
                            ParseErrorKind::Syntax,
                            format!(
                                "invalid escape sequence in annotation value: '\\' followed by \
                                 {} (only \\\\ and \\\" are allowed)",
                                describe(self.buf, self.pos + 1)
                            ),
                        ))
                    }
                },
                Some(b'\n') => {
                    self.newlines += 1;
                    self.last_newline = Some(self.pos);
                    self.pos += 1;
                }
                Some(_) => self.pos += 1,
            }
        }
        let value_end = self.pos;
        self.pos += 1;
        self.utf8(value_start, value_end, "annotation value")
    }

    /// `'#' [^\n\r]*`; the current byte is `#`. Returns the text after the
    /// `#`.
    fn comment(&mut self) -> Result<Box<str>, RawError> {
        let start = self.pos + 1;
        let rest = self.slice(start, self.buf.len());
        let len = rest
            .iter()
            .position(|&b| b == b'\n' || b == b'\r')
            .unwrap_or(rest.len());
        self.pos = start + len;
        self.utf8(start, self.pos, "comment")
    }
}

/// Describes the input at `pos` for an error message.
pub(super) fn describe(buf: &[u8], pos: usize) -> String {
    match buf.get(pos) {
        None => "end of file".to_string(),
        Some(b'\n' | b'\r') => "end of line".to_string(),
        Some(b'\'') => "\"'\"".to_string(),
        Some(&b) if b.is_ascii_graphic() || b == b' ' => format!("'{}'", char::from(b)),
        Some(&b) if b.is_ascii() => format!("byte 0x{b:02x}"),
        Some(&b) => {
            let rest = buf.get(pos..buf.len().min(pos + 4)).unwrap_or_default();
            let valid = match std::str::from_utf8(rest) {
                Ok(s) => s,
                Err(e) => std::str::from_utf8(rest.get(..e.valid_up_to()).unwrap_or_default())
                    .unwrap_or_default(),
            };
            match valid.chars().next() {
                Some(c) => format!("'{c}'"),
                None => format!("byte 0x{b:02x}"),
            }
        }
    }
}

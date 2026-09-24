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

//! [`PyStr`]: a Python `str` as the original tool sees its command line
//! arguments and environment, and the few `str` operations whose exact
//! behaviour shows in its output (`repr()`, encoding to stdout/stderr,
//! `int()`).
//!
//! Python decodes `argv` and the environment with the file system encoding
//! (UTF-8) and the `surrogateescape` error handler: every byte that is not
//! part of a valid UTF-8 sequence becomes the lone surrogate
//! `U+DC80 + (byte - 0x80)`. A `PyStr` is therefore a sequence of code
//! points that may include surrogates, which a Rust `char`/`String` cannot
//! hold.

use std::ffi::{OsStr, OsString};

use crate::unicode_tables::{DECIMAL_ZEROS, NON_PRINTABLE, WHITESPACE};

/// A Python `str`: a sequence of Unicode code points, lone surrogates
/// included.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PyStr(pub Vec<u32>);

impl PyStr {
    /// Decodes `bytes` like Python decodes `argv` on POSIX: UTF-8 with the
    /// `surrogateescape` error handler.
    #[must_use]
    pub fn from_bytes_surrogateescape(bytes: &[u8]) -> Self {
        let mut out = Vec::with_capacity(bytes.len());
        for chunk in bytes.utf8_chunks() {
            out.extend(chunk.valid().chars().map(u32::from));
            out.extend(chunk.invalid().iter().map(|&b| 0xDC00 + u32::from(b)));
        }
        PyStr(out)
    }

    /// Decodes an OS string (a command line argument or an environment
    /// variable) like Python does.
    #[must_use]
    pub fn from_os_str(s: &OsStr) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self::from_bytes_surrogateescape(s.as_bytes())
        }
        #[cfg(windows)]
        {
            // Python keeps lone surrogates of the UTF-16 command line.
            use std::os::windows::ffi::OsStrExt;
            let units: Vec<u16> = s.encode_wide().collect();
            PyStr(
                char::decode_utf16(units.iter().copied())
                    .map(|r| r.map_or_else(|e| u32::from(e.unpaired_surrogate()), u32::from))
                    .collect(),
            )
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self::from_bytes_surrogateescape(s.to_string_lossy().as_bytes())
        }
    }

    /// The inverse of [`PyStr::from_os_str`].
    #[must_use]
    pub fn to_os_string(&self) -> OsString {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(self.encode_surrogateescape())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            let mut units = Vec::with_capacity(self.0.len());
            for &c in &self.0 {
                match char::from_u32(c) {
                    Some(ch) => {
                        let mut buf = [0u16; 2];
                        units.extend_from_slice(ch.encode_utf16(&mut buf));
                    }
                    // A lone surrogate: fits in one unit.
                    None => units.push(u16::try_from(c).unwrap_or(0xFFFD)),
                }
            }
            OsString::from_wide(&units)
        }
        #[cfg(not(any(unix, windows)))]
        {
            OsString::from(String::from_utf8_lossy(&self.encode_surrogateescape()).into_owned())
        }
    }

    /// A `PyStr` from ASCII/Unicode text.
    // Infallible, unlike `FromStr::from_str`.
    #[allow(clippy::should_implement_trait)]
    #[must_use]
    pub fn from_str(s: &str) -> Self {
        PyStr(s.chars().map(u32::from).collect())
    }

    /// Number of code points (`len()`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `not s`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `self == other` for an ASCII/Unicode `other`.
    #[must_use]
    pub fn eq_str(&self, other: &str) -> bool {
        self.0.iter().copied().eq(other.chars().map(u32::from))
    }

    /// `other.startswith(self)` for an ASCII/Unicode `other`.
    #[must_use]
    pub fn is_prefix_of(&self, other: &str) -> bool {
        let mut chars = other.chars().map(u32::from);
        self.0.iter().all(|&c| chars.next() == Some(c))
    }

    /// `self[i]`, if it exists.
    #[must_use]
    pub fn at(&self, i: usize) -> Option<u32> {
        self.0.get(i).copied()
    }

    /// `self[from..]`.
    #[must_use]
    pub fn slice_from(&self, from: usize) -> PyStr {
        PyStr(self.0.get(from..).unwrap_or_default().to_vec())
    }

    /// `self.partition('=')`: `(before, Some(after))`, or `(self, None)`
    /// if there is no `=`.
    #[must_use]
    pub fn partition_eq(&self) -> (PyStr, Option<PyStr>) {
        match self.0.iter().position(|&c| c == u32::from('=')) {
            Some(i) => (PyStr(self.0[..i].to_vec()), Some(self.slice_from(i + 1))),
            None => (self.clone(), None),
        }
    }

    /// `c in self`.
    #[must_use]
    pub fn contains(&self, c: char) -> bool {
        self.0.contains(&u32::from(c))
    }

    /// Encodes like Python's `sys.stdout` does in the UTF-8 mode / C
    /// locale the original tool runs with: UTF-8 with the `surrogateescape`
    /// error handler (`U+DC80..=U+DCFF` become the bytes `0x80..=0xFF`).
    /// Other lone surrogates (only possible on Windows) are written as
    /// `?`; Python would raise `UnicodeEncodeError` for them.
    #[must_use]
    pub fn encode_surrogateescape(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.0.len());
        for &c in &self.0 {
            if let Some(ch) = char::from_u32(c) {
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            } else if (0xDC80..=0xDCFF).contains(&c) {
                out.push(u8::try_from(c - 0xDC00).unwrap_or(b'?'));
            } else {
                out.push(b'?');
            }
        }
        out
    }

    /// Encodes like Python's `sys.stderr` does: UTF-8 with the
    /// `backslashreplace` error handler (a lone surrogate becomes
    /// `\udcXX`).
    #[must_use]
    pub fn encode_backslashreplace(&self) -> String {
        let mut out = String::with_capacity(self.0.len());
        for &c in &self.0 {
            match char::from_u32(c) {
                Some(ch) => out.push(ch),
                None => out.push_str(&format!("\\u{c:04x}")),
            }
        }
        out
    }

    /// Python's `repr()` of the `str`.
    #[must_use]
    pub fn repr(&self) -> PyStr {
        let squote = u32::from('\'');
        let dquote = u32::from('"');
        let quote = if self.0.contains(&squote) && !self.0.contains(&dquote) {
            dquote
        } else {
            squote
        };
        let mut out = vec![quote];
        let push_str = |out: &mut Vec<u32>, s: &str| out.extend(s.chars().map(u32::from));
        for &c in &self.0 {
            if c == quote || c == u32::from('\\') {
                out.push(u32::from('\\'));
                out.push(c);
            } else if c == u32::from('\t') {
                push_str(&mut out, "\\t");
            } else if c == u32::from('\n') {
                push_str(&mut out, "\\n");
            } else if c == u32::from('\r') {
                push_str(&mut out, "\\r");
            } else if c < 0x20 || c == 0x7F {
                push_str(&mut out, &format!("\\x{c:02x}"));
            } else if c < 0x7F || is_printable(c) {
                out.push(c);
            } else if c <= 0xFF {
                push_str(&mut out, &format!("\\x{c:02x}"));
            } else if c <= 0xFFFF {
                push_str(&mut out, &format!("\\u{c:04x}"));
            } else {
                push_str(&mut out, &format!("\\U{c:08x}"));
            }
        }
        out.push(quote);
        PyStr(out)
    }
}

/// `chr(c).isprintable()` for a non-ASCII `c`.
fn is_printable(c: u32) -> bool {
    // Index of the first range whose end is >= c.
    let i = NON_PRINTABLE.partition_point(|&(_, end)| end < c);
    NON_PRINTABLE.get(i).is_none_or(|&(start, _)| start > c)
}

/// The decimal value of `c` (`unicodedata.decimal(c)`; `\d` in a `str`
/// regex matches exactly these), or `None`.
#[must_use]
pub fn decimal_value(c: u32) -> Option<u32> {
    let i = DECIMAL_ZEROS.partition_point(|&zero| zero <= c);
    let zero = *DECIMAL_ZEROS.get(i.checked_sub(1)?)?;
    (c - zero < 10).then_some(c - zero)
}

/// `chr(c).isspace()`.
#[must_use]
pub fn is_space(c: u32) -> bool {
    WHITESPACE.binary_search(&c).is_ok()
}

/// Python's default limit on the number of digits `int()` converts
/// (`sys.int_max_str_digits`).
pub const INT_MAX_STR_DIGITS: usize = 4300;

/// Python's `int(s)` (base 10) for a `str`, saturated to the `i64` range;
/// `None` where Python raises `ValueError`.
///
/// Leading and trailing whitespace is skipped, an optional sign is
/// accepted, digits may be any Unicode decimal digits and may be separated
/// by single underscores. More than [`INT_MAX_STR_DIGITS`] digits (leading
/// zeros included; underscores, whitespace and the sign excluded) is an
/// error, like with Python's default limit (`PYTHONINTMAXSTRDIGITS` and
/// `-X int_max_str_digits` are not honoured).
#[must_use]
pub fn py_int(s: &PyStr) -> Option<i64> {
    let chars = &s.0;
    let start = chars.iter().position(|&c| !is_space(c))?;
    let end = chars.iter().rposition(|&c| !is_space(c))? + 1;
    let mut body = &chars[start..end];
    let negative = match body.first().copied() {
        Some(c) if c == u32::from('-') => {
            body = &body[1..];
            true
        }
        Some(c) if c == u32::from('+') => {
            body = &body[1..];
            false
        }
        _ => false,
    };
    if body.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    let mut previous_was_digit = false;
    let mut digits = 0;
    for &c in body {
        if c == u32::from('_') {
            if !previous_was_digit {
                return None;
            }
            previous_was_digit = false;
        } else {
            let digit = decimal_value(c)?;
            digits += 1;
            value = value.saturating_mul(10).saturating_add(i64::from(digit));
            previous_was_digit = true;
        }
    }
    if !previous_was_digit || digits > INT_MAX_STR_DIGITS {
        return None;
    }
    Some(if negative { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> PyStr {
        PyStr::from_str(text)
    }

    #[test]
    fn surrogateescape_round_trip() {
        let bytes = b"a\xff\xc3\xa9\xe2\x82";
        let p = PyStr::from_bytes_surrogateescape(bytes);
        assert_eq!(p.0, vec![0x61, 0xDCFF, 0xE9, 0xDCE2, 0xDC82]);
        assert_eq!(p.encode_surrogateescape(), bytes);
        assert_eq!(p.encode_backslashreplace(), "a\\udcffé\\udce2\\udc82");
    }

    #[test]
    fn repr_matches_python() {
        // Expected values from Python 3.11's repr().
        assert_eq!(s("x").repr(), s("'x'"));
        assert_eq!(s("").repr(), s("''"));
        assert_eq!(s("it's").repr(), s("\"it's\""));
        assert_eq!(s("it's \"q\"").repr(), s("'it\\'s \"q\"'"));
        assert_eq!(s("a\\b").repr(), s("'a\\\\b'"));
        assert_eq!(s("\t\n\r\x01\x7f").repr(), s("'\\t\\n\\r\\x01\\x7f'"));
        assert_eq!(s("é\u{a0}\u{ad}").repr(), s("'é\\xa0\\xad'"));
        assert_eq!(s("\u{2028}\u{e000}").repr(), s("'\\u2028\\ue000'"));
        assert_eq!(s("\u{10ffff}\u{1f600}").repr(), s("'\\U0010ffff\u{1f600}'"));
        let p = PyStr::from_bytes_surrogateescape(b"x\xff");
        assert_eq!(p.repr(), s("'x\\udcff'"));
    }

    #[test]
    fn printable() {
        assert!(is_printable(0xE9));
        assert!(!is_printable(0x85));
        assert!(!is_printable(0xA0));
        assert!(!is_printable(0xDC80));
        // Unassigned in Unicode 14 (Python 3.11): not printable.
        assert!(!is_printable(0x0378));
        assert!(is_printable(0x1F600));
    }

    #[test]
    fn decimal_values() {
        assert_eq!(decimal_value(u32::from('0')), Some(0));
        assert_eq!(decimal_value(u32::from('9')), Some(9));
        assert_eq!(decimal_value(u32::from('a')), None);
        assert_eq!(decimal_value(u32::from('/')), None);
        assert_eq!(decimal_value(0x0663), Some(3)); // ARABIC-INDIC DIGIT THREE
        assert_eq!(decimal_value(0x00B2), None); // SUPERSCRIPT TWO: not Nd
        assert_eq!(decimal_value(0), None);
    }

    #[test]
    fn int_matches_python() {
        assert_eq!(py_int(&s("80")), Some(80));
        assert_eq!(py_int(&s(" \t+80\n")), Some(80));
        assert_eq!(py_int(&s("-5")), Some(-5));
        assert_eq!(py_int(&s("1_0")), Some(10));
        assert_eq!(py_int(&s("007")), Some(7));
        assert_eq!(py_int(&s("\u{3000}\u{0661}\u{0662}")), Some(12));
        assert_eq!(py_int(&s("99999999999999999999999")), Some(i64::MAX));
        for bad in [
            "", " ", "+", "-", "_1", "1_", "1__0", "1 0", "0x10", "1.0", "a",
        ] {
            assert_eq!(py_int(&s(bad)), None, "{bad:?}");
        }
        assert_eq!(py_int(&PyStr::from_bytes_surrogateescape(b"8\xff")), None);
        // `sys.int_max_str_digits`: at most 4300 digits, leading zeros
        // included, underscores, whitespace and the sign excluded.
        let digits = |prefix: &str, n: usize, suffix: &str| {
            py_int(&s(&format!("{prefix}{}{suffix}", "1".repeat(n))))
        };
        assert_eq!(digits("", 4300, ""), Some(i64::MAX));
        assert_eq!(digits(" -", 4300, " "), Some(-i64::MAX));
        assert_eq!(digits("", 4301, ""), None);
        assert_eq!(py_int(&s(&format!("{}50", "0".repeat(4299)))), None);
        assert_eq!(py_int(&s(&format!("{}50", "0".repeat(4298)))), Some(50));
        assert_eq!(
            py_int(&s(&format!("{}1", "1_".repeat(4299)))),
            Some(i64::MAX)
        );
    }
}

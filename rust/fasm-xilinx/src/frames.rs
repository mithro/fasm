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

//! [`Frames`]: configuration frames (frame address -> words), the `.frm`
//! text format writer (`dump_frm` of `xc_fasm/fasm2frames.py`) and reader
//! (`Frames<ArchType>::readFrames` of prjxray `lib/include/prjxray/xilinx/frames.h`).

use std::fmt;
use std::io::{self, Write};

/// A set of configuration frames: frame address -> `words_per_frame`
/// 32-bit words, kept in ascending address order.
///
/// The words of all frames are stored in one contiguous vector (a dense
/// xc7a200t has 24060 frames of 101 words, 9.7 MB), next to the sorted
/// address list. The word count per frame is a run time value (101 for
/// Series7, 123 for UltraScale, 93 for UltraScale+; see
/// [`crate::Architecture::words_per_frame`]).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Frames {
    words_per_frame: usize,
    addresses: Vec<u32>,
    words: Vec<u32>,
}

impl fmt::Debug for Frames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frames")
            .field("words_per_frame", &self.words_per_frame)
            .field("frames", &self.addresses.len())
            .finish_non_exhaustive()
    }
}

impl Frames {
    /// An empty set of frames of `words_per_frame` words each.
    pub fn new(words_per_frame: usize) -> Self {
        Frames {
            words_per_frame,
            addresses: Vec::new(),
            words: Vec::new(),
        }
    }

    /// Zero filled frames at `addresses` (any order, duplicates allowed).
    pub fn zeroed(words_per_frame: usize, addresses: impl IntoIterator<Item = u32>) -> Self {
        let mut addresses: Vec<u32> = addresses.into_iter().collect();
        addresses.sort_unstable();
        addresses.dedup();
        let words = vec![0; addresses.len() * words_per_frame];
        Frames {
            words_per_frame,
            addresses,
            words,
        }
    }

    /// Number of 32-bit words per frame.
    pub fn words_per_frame(&self) -> usize {
        self.words_per_frame
    }

    /// Number of frames.
    pub fn len(&self) -> usize {
        self.addresses.len()
    }

    /// `true` if there are no frames.
    pub fn is_empty(&self) -> bool {
        self.addresses.is_empty()
    }

    /// The frame addresses, ascending.
    pub fn addresses(&self) -> &[u32] {
        &self.addresses
    }

    /// Index of the frame at `address`.
    fn index(&self, address: u32) -> Option<usize> {
        self.addresses.binary_search(&address).ok()
    }

    fn slice(&self, index: usize) -> &[u32] {
        let start = index * self.words_per_frame;
        &self.words[start..start + self.words_per_frame]
    }

    fn slice_mut(&mut self, index: usize) -> &mut [u32] {
        let start = index * self.words_per_frame;
        &mut self.words[start..start + self.words_per_frame]
    }

    /// `true` if there is a frame at `address`.
    pub fn contains(&self, address: u32) -> bool {
        self.index(address).is_some()
    }

    /// The words of the frame at `address`.
    pub fn get(&self, address: u32) -> Option<&[u32]> {
        self.index(address).map(|i| self.slice(i))
    }

    /// The words of the frame at `address`, mutable.
    pub fn get_mut(&mut self, address: u32) -> Option<&mut [u32]> {
        self.index(address).map(|i| self.slice_mut(i))
    }

    /// The frame at `address`, inserted zero filled if it does not exist
    /// (`init_frame_at_address` of prjxray's `fasm_assembler.py`).
    /// Appending in ascending address order is O(1); inserting before the
    /// last frame moves the following frames.
    pub fn get_or_insert_zeroed(&mut self, address: u32) -> &mut [u32] {
        let index = match self.addresses.last() {
            Some(&last) if last < address => {
                self.push_zeroed(address);
                self.addresses.len() - 1
            }
            None => {
                self.push_zeroed(address);
                0
            }
            Some(_) => match self.addresses.binary_search(&address) {
                Ok(i) => i,
                Err(i) => {
                    self.addresses.insert(i, address);
                    let at = i * self.words_per_frame;
                    self.words
                        .splice(at..at, std::iter::repeat_n(0, self.words_per_frame));
                    i
                }
            },
        };
        self.slice_mut(index)
    }

    fn push_zeroed(&mut self, address: u32) {
        self.addresses.push(address);
        self.words
            .resize(self.words.len() + self.words_per_frame, 0);
    }

    /// Inserts a frame unless one already exists at `address` (the first
    /// one wins, like `std::map::insert` in prjxray's `readFrames`).
    /// Returns `false` if the frame existed. `words` must have
    /// [`Frames::words_per_frame`] words; missing words are zero and extra
    /// words are ignored.
    pub fn insert_if_absent(&mut self, address: u32, words: &[u32]) -> bool {
        if self.contains(address) {
            return false;
        }
        let n = self.words_per_frame.min(words.len());
        self.get_or_insert_zeroed(address)[..n].copy_from_slice(&words[..n]);
        true
    }

    /// Iterates over `(address, words)` in ascending address order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (u32, &[u32])> + '_ {
        self.addresses
            .iter()
            .enumerate()
            .map(move |(i, &address)| (address, self.slice(i)))
    }

    /// All set bits as `(frame address, word, bit)`, ascending: the
    /// representation of bitread's `-y` output and of f4pga-xc-fasm's
    /// `frm2bits` test helper.
    pub fn set_bits(&self) -> impl Iterator<Item = (u32, u32, u32)> + '_ {
        self.iter().flat_map(|(address, words)| {
            words.iter().enumerate().flat_map(move |(w, &word)| {
                (0..32)
                    .filter(move |b| word & (1 << b) != 0)
                    .map(move |b| (address, w as u32, b))
            })
        })
    }

    /// The differences between `self` (expected) and `other` (actual):
    /// frames present on one side only and differing words, in address
    /// order. Empty if the two are equal (a helper for tests).
    pub fn diff(&self, other: &Frames) -> Vec<FrameDifference> {
        let mut out = Vec::new();
        let (mut i, mut j) = (0, 0);
        while i < self.len() || j < other.len() {
            let a = self.addresses.get(i).copied();
            let b = other.addresses.get(j).copied();
            match (a, b) {
                (Some(a), Some(b)) if a == b => {
                    let (x, y) = (self.slice(i), other.slice(j));
                    for word in 0..x.len().max(y.len()) {
                        let (ex, ac) = (x.get(word).copied(), y.get(word).copied());
                        if ex != ac {
                            out.push(FrameDifference::Word {
                                address: a,
                                word,
                                expected: ex,
                                actual: ac,
                            });
                        }
                    }
                    i += 1;
                    j += 1;
                }
                (Some(a), b) if b.is_none_or(|b| a < b) => {
                    out.push(FrameDifference::Missing { address: a });
                    i += 1;
                }
                (_, Some(b)) => {
                    out.push(FrameDifference::Extra { address: b });
                    j += 1;
                }
                (a, None) => {
                    // Unreachable: covered by the arm above.
                    out.extend(a.map(|address| FrameDifference::Missing { address }));
                    i += 1;
                }
            }
        }
        out
    }

    /// Writes the frames in the `.frm` text format, exactly like
    /// `dump_frm` of `xc_fasm/fasm2frames.py`: one line per frame in
    /// ascending address order, `0x%08X ` followed by the words as
    /// `0x%08X` separated by `,`, and `\n`.
    ///
    /// # Errors
    ///
    /// The errors of `out`.
    pub fn write_frm(&self, out: &mut dyn Write) -> io::Result<()> {
        let mut line = Vec::with_capacity(11 + 11 * self.words_per_frame);
        for (address, words) in self.iter() {
            line.clear();
            push_hex(&mut line, address);
            line.push(b' ');
            for (i, &word) in words.iter().enumerate() {
                if i > 0 {
                    line.push(b',');
                }
                push_hex(&mut line, word);
            }
            line.push(b'\n');
            out.write_all(&line)?;
        }
        Ok(())
    }

    /// The `.frm` text of [`Frames::write_frm`] as a string.
    pub fn to_frm_string(&self) -> String {
        let mut out = Vec::new();
        self.write_frm(&mut out)
            .expect("writing to a Vec cannot fail");
        String::from_utf8(out).expect("the .frm text is ASCII")
    }

    /// Reads a `.frm` file's contents following prjxray's
    /// `Frames<ArchType>::readFrames` (`lib/include/prjxray/xilinx/frames.h`,
    /// used by `xc7frames2bit`):
    ///
    /// * lines are separated by `\n` (a `\r` before it is ignored by the
    ///   number parsing, as it is by `std::stoul`); a line starting with
    ///   `#` is skipped;
    /// * the line is split at spaces: the first field is the frame
    ///   address and the second one the comma separated words (further
    ///   fields are ignored, like the `std::pair` form of `absl::StrSplit`);
    /// * numbers are parsed like `std::stoul(s, nullptr, 16)`: leading
    ///   white space, an optional sign and `0x` prefix, then as many hex
    ///   digits as there are (trailing garbage is ignored), truncated to 32
    ///   bits;
    /// * a line whose word count is not `words_per_frame` is skipped with a
    ///   warning (`Frame <hex address>: found <n> words instead of <n>`,
    ///   passed to `warn`);
    /// * a second frame with the same address is ignored (the first wins).
    ///
    /// Unlike `readFrames`, the words are returned as read: the ECC word(s)
    /// are not recomputed (the bitstream writer does that).
    ///
    /// # Errors
    ///
    /// [`FrmError`] where `std::stoul` would throw (and `xc7frames2bit`
    /// abort): an address or word without any hex digit (an empty line
    /// included) or one that does not fit in 64 bits.
    pub fn read_frm(
        data: &[u8],
        words_per_frame: usize,
        warn: &mut dyn FnMut(&str),
    ) -> Result<Frames, FrmError> {
        let mut frames = Frames::new(words_per_frame);
        let mut words = Vec::with_capacity(words_per_frame);
        let mut lines = data.split(|&b| b == b'\n').enumerate().peekable();
        while let Some((index, line)) = lines.next() {
            // `std::getline` does not produce an empty last line after the
            // final `\n`.
            if line.is_empty() && lines.peek().is_none() {
                break;
            }
            let line_no = index + 1;
            if line.first() == Some(&b'#') {
                continue;
            }
            let mut fields = line.split(|&b| b == b' ');
            let address_text = fields.next().unwrap_or_default();
            let words_text = fields.next().unwrap_or_default();
            let address = stoul16(address_text).map_err(|kind| FrmError {
                line: line_no,
                text: String::from_utf8_lossy(address_text).into_owned(),
                kind,
            })? as u32;
            let count = words_text.split(|&b| b == b',').count();
            if count != words_per_frame {
                warn(&format!(
                    "Frame {address:x}: found {count} words instead of {words_per_frame}"
                ));
                continue;
            }
            words.clear();
            for text in words_text.split(|&b| b == b',') {
                let value = stoul16(text).map_err(|kind| FrmError {
                    line: line_no,
                    text: String::from_utf8_lossy(text).into_owned(),
                    kind,
                })?;
                words.push(value as u32);
            }
            frames.insert_if_absent(address, &words);
        }
        Ok(frames)
    }
}

/// One difference found by [`Frames::diff`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameDifference {
    /// The frame is only in the expected frames.
    Missing {
        /// Frame address.
        address: u32,
    },
    /// The frame is only in the actual frames.
    Extra {
        /// Frame address.
        address: u32,
    },
    /// A word differs (or exists on one side only).
    Word {
        /// Frame address.
        address: u32,
        /// Word index.
        word: usize,
        /// Expected value.
        expected: Option<u32>,
        /// Actual value.
        actual: Option<u32>,
    },
}

impl fmt::Display for FrameDifference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex = |v: Option<u32>| v.map_or("-".to_string(), |v| format!("0x{v:08X}"));
        match *self {
            FrameDifference::Missing { address } => write!(f, "0x{address:08X}: missing frame"),
            FrameDifference::Extra { address } => write!(f, "0x{address:08X}: extra frame"),
            FrameDifference::Word {
                address,
                word,
                expected,
                actual,
            } => write!(
                f,
                "0x{address:08X} word {word}: expected {}, got {}",
                hex(expected),
                hex(actual)
            ),
        }
    }
}

/// Why `std::stoul` would throw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrmErrorKind {
    /// No digits (`std::invalid_argument`).
    InvalidArgument,
    /// Does not fit in 64 bits (`std::out_of_range`).
    OutOfRange,
}

/// A number of a `.frm` file that prjxray's reader cannot parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrmError {
    /// 1-based line.
    pub line: usize,
    /// The text of the number.
    pub text: String,
    /// What is wrong.
    pub kind: FrmErrorKind,
}

impl fmt::Display for FrmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.kind {
            FrmErrorKind::InvalidArgument => "not a hexadecimal number",
            FrmErrorKind::OutOfRange => "hexadecimal number out of range",
        };
        write!(f, "line {}: {what}: {:?}", self.line, self.text)
    }
}

impl std::error::Error for FrmError {}

/// `std::stoul(text, nullptr, 16)` on a 64-bit `unsigned long` (C
/// `strtoul`): optional leading white space, sign and `0x`/`0X` prefix,
/// then the longest run of hex digits; the rest is ignored. A `-` negates
/// the value modulo 2^64.
fn stoul16(text: &[u8]) -> Result<u64, FrmErrorKind> {
    let mut rest = text;
    while let Some((&c, tail)) = rest.split_first() {
        // C `isspace` in the "C" locale.
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
    // `0x` is only a prefix if a hex digit follows; otherwise the `0` is
    // the number.
    if rest.len() >= 3 && rest[0] == b'0' && (rest[1] | 0x20) == b'x' && rest[2].is_ascii_hexdigit()
    {
        rest = &rest[2..];
    }
    let digits = rest.iter().take_while(|c| c.is_ascii_hexdigit()).count();
    if digits == 0 {
        return Err(FrmErrorKind::InvalidArgument);
    }
    let mut value: u64 = 0;
    for &c in &rest[..digits] {
        let digit = (c as char).to_digit(16).unwrap_or(0);
        value = value
            .checked_mul(16)
            .and_then(|v| v.checked_add(u64::from(digit)))
            .ok_or(FrmErrorKind::OutOfRange)?;
    }
    Ok(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

/// Appends `0x%08X`.
fn push_hex(out: &mut Vec<u8>, value: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.extend_from_slice(b"0x");
    for shift in (0..8).rev() {
        out.push(HEX[((value >> (shift * 4)) & 0xF) as usize]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str, words: usize) -> (Result<Frames, FrmError>, Vec<String>) {
        let mut warnings = Vec::new();
        let result = Frames::read_frm(text.as_bytes(), words, &mut |w| warnings.push(w.to_owned()));
        (result, warnings)
    }

    #[test]
    fn insert_keeps_order() {
        let mut frames = Frames::new(3);
        frames.get_or_insert_zeroed(5)[1] = 7;
        frames.get_or_insert_zeroed(1)[0] = 1;
        frames.get_or_insert_zeroed(9)[2] = 9;
        frames.get_or_insert_zeroed(5)[2] = 8;
        assert_eq!(frames.addresses(), [1, 5, 9]);
        assert_eq!(frames.get(5), Some(&[0, 7, 8][..]));
        assert_eq!(frames.get(1), Some(&[1, 0, 0][..]));
        assert_eq!(frames.get(9), Some(&[0, 0, 9][..]));
        assert_eq!(frames.get(2), None);
        assert!(!frames.insert_if_absent(5, &[1, 1, 1]));
        assert!(frames.insert_if_absent(3, &[1, 2, 3]));
        assert_eq!(frames.addresses(), [1, 3, 5, 9]);
        assert_eq!(frames.get(3), Some(&[1, 2, 3][..]));
        assert_eq!(
            frames.set_bits().collect::<Vec<_>>(),
            [
                (1, 0, 0),
                (3, 0, 0),
                (3, 1, 1),
                (3, 2, 0),
                (3, 2, 1),
                (5, 1, 0),
                (5, 1, 1),
                (5, 1, 2),
                (5, 2, 3),
                (9, 2, 0),
                (9, 2, 3)
            ]
        );
    }

    #[test]
    fn frm_format() {
        let mut frames = Frames::zeroed(2, [0x00400100, 0x10]);
        frames.get_mut(0x10).unwrap()[1] = 0xDEADBEEF;
        assert_eq!(
            frames.to_frm_string(),
            "0x00000010 0x00000000,0xDEADBEEF\n0x00400100 0x00000000,0x00000000\n"
        );
        assert_eq!(Frames::new(101).to_frm_string(), "");
        let (back, warnings) = read(&frames.to_frm_string(), 2);
        assert_eq!(back.unwrap(), frames);
        assert!(warnings.is_empty());
    }

    #[test]
    fn frm_reader_rules() {
        let text = "# comment\n\
                    0x10 1,2\r\n\
                    10 5,6\n\
                    0x20 0x3,\t-1 ignored\n\
                    0x30 1,2,3\n\
                    0x0g 0xAz,0x\n";
        let (frames, warnings) = read(text, 2);
        let frames = frames.unwrap();
        assert_eq!(frames.addresses(), [0, 0x10, 0x20]);
        assert_eq!(frames.get(0x10), Some(&[1, 2][..]));
        assert_eq!(frames.get(0x20), Some(&[3, 0xFFFF_FFFF][..]));
        assert_eq!(frames.get(0), Some(&[0xA, 0][..]));
        assert_eq!(warnings, ["Frame 30: found 3 words instead of 2"]);

        // No trailing newline.
        let (frames, _) = read("0x1 1,2", 2);
        assert_eq!(frames.unwrap().get(1), Some(&[1, 2][..]));
        // Line without words: one empty word.
        let (frames, warnings) = read("0x1\n", 2);
        assert!(frames.unwrap().is_empty());
        assert_eq!(warnings, ["Frame 1: found 1 words instead of 2"]);
    }

    #[test]
    fn frm_reader_errors() {
        let (e, _) = read("0x1 1,2\n\n0x2 1,2\n", 2);
        assert_eq!(
            e.unwrap_err(),
            FrmError {
                line: 2,
                text: String::new(),
                kind: FrmErrorKind::InvalidArgument
            }
        );
        let (e, _) = read("0x1 1,z\n", 2);
        assert_eq!(e.unwrap_err().kind, FrmErrorKind::InvalidArgument);
        let (e, _) = read("0x10000000000000000 1,2\n", 2);
        assert_eq!(e.unwrap_err().kind, FrmErrorKind::OutOfRange);
        // 64 bit values are truncated to 32 bits.
        let (frames, _) = read("0x1FFFFFFFF 1,2\n", 2);
        assert_eq!(frames.unwrap().addresses(), [0xFFFF_FFFF]);
    }

    #[test]
    fn diff_reports_differences() {
        let mut a = Frames::zeroed(2, [1, 2, 4]);
        let mut b = Frames::zeroed(2, [2, 3, 4]);
        a.get_mut(4).unwrap()[1] = 5;
        b.get_mut(4).unwrap()[1] = 6;
        assert_eq!(
            a.diff(&b),
            [
                FrameDifference::Missing { address: 1 },
                FrameDifference::Extra { address: 3 },
                FrameDifference::Word {
                    address: 4,
                    word: 1,
                    expected: Some(5),
                    actual: Some(6)
                }
            ]
        );
        assert!(a.diff(&a).is_empty());
    }
}

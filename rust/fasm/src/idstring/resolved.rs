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

//! [`Resolved`]: the interned pieces of a handle and every string
//! operation on them, implemented without joining the pieces.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::fmt;

use super::repr::LEVELS;

/// Names up to this many bytes are joined in a per thread buffer by
/// [`Resolved::with_str`]; longer ones in a new `String`.
const MAX_BUFFERED: usize = 1024;

thread_local! {
    /// Reused buffer of [`Resolved::with_str`] (at most [`MAX_BUFFERED`]
    /// bytes, so it never pins much memory).
    static BUFFER: RefCell<String> = const { RefCell::new(String::new()) };
}

/// The resolved text of an [`IdString`](super::IdString): one to three
/// interned pieces that, joined with `.`, form the string.
///
/// A `Resolved` is obtained with [`IdString::resolved`](super::IdString::resolved)
/// or [`Interner::resolved`](super::Interner::resolved). Resolving costs a
/// few table reads (no lock); afterwards every operation works directly on
/// the `&'static str` pieces, so resolve once when several operations are
/// needed on the same handle.
///
/// Comparisons (`==`, `<`, ...) are by string value.
#[derive(Clone, Copy)]
pub struct Resolved {
    pieces: [&'static str; LEVELS],
    count: usize,
}

impl Resolved {
    /// Builds a view from `count` (between 1 and [`LEVELS`]) pieces.
    pub(crate) fn from_pieces(pieces: [&'static str; LEVELS], count: usize) -> Self {
        debug_assert!((1..=LEVELS).contains(&count));
        Resolved {
            pieces,
            count: count.clamp(1, LEVELS),
        }
    }

    /// The interned pieces (only the last one can contain `.`).
    fn pieces(&self) -> &[&'static str] {
        &self.pieces[..self.count]
    }

    /// The bytes of the string as a sequence of chunks (the pieces and
    /// the `.` separators between them).
    fn chunks(&self) -> Chunks {
        let mut chunks = Chunks {
            chunks: [&[]; MAX_CHUNKS],
            count: 1,
        };
        chunks.chunks[0] = self.pieces[0].as_bytes();
        for piece in &self.pieces[1..self.count] {
            chunks.chunks[chunks.count] = b".";
            chunks.chunks[chunks.count + 1] = piece.as_bytes();
            chunks.count += 2;
        }
        chunks
    }

    /// Length of the string in bytes.
    pub fn len(&self) -> usize {
        let pieces = self.pieces();
        pieces.iter().map(|piece| piece.len()).sum::<usize>() + pieces.len() - 1
    }

    /// Returns `true` for the empty string.
    pub fn is_empty(&self) -> bool {
        self.count == 1 && self.pieces[0].is_empty()
    }

    /// Iterates over the `.` separated components of the string (like
    /// `str::split('.')`, so the empty string has one empty component).
    pub fn components(&self) -> impl Iterator<Item = &'static str> {
        let pieces = self.pieces;
        pieces
            .into_iter()
            .take(self.count)
            .flat_map(|piece| piece.split('.'))
    }

    /// The first `.` separated component (the whole string if it has no
    /// `.`).
    pub fn first_component(&self) -> &'static str {
        let first = self.pieces[0];
        first.split_once('.').map_or(first, |(head, _)| head)
    }

    /// Returns `true` if the string equals `prefix` or starts with `prefix`
    /// followed by `.`, i.e. `prefix` is made of whole leading components.
    ///
    /// For `"A.BC.D"` this is true for `"A"`, `"A.BC"` and `"A.BC.D"`, and
    /// false for `"A.B"` (partial component), `"A.BC."` and `"A.BC.D.E"`.
    pub fn starts_with_component(&self, prefix: &str) -> bool {
        let mut rest = prefix.as_bytes();
        let chunks = self.chunks();
        for chunk in chunks.as_slice().iter().filter(|chunk| !chunk.is_empty()) {
            if rest.is_empty() {
                return chunk.first() == Some(&b'.');
            }
            let n = rest.len().min(chunk.len());
            if chunk[..n] != rest[..n] {
                return false;
            }
            rest = &rest[n..];
            if n < chunk.len() {
                // The prefix ended inside this chunk.
                return chunk[n] == b'.';
            }
        }
        rest.is_empty()
    }

    /// Calls `f` with the string.
    ///
    /// A string made of a single interned piece is passed directly. Other
    /// strings of at most 1024 bytes are joined in a reused per thread
    /// buffer, so no heap allocation happens in steady state; longer
    /// strings, and calls nested inside `f`, join into a new `String`.
    pub fn with_str<R>(&self, f: impl FnOnce(&str) -> R) -> R {
        if let [single] = self.pieces() {
            return f(single);
        }
        let mut f = Some(f);
        if self.len() <= MAX_BUFFERED {
            let result = BUFFER.try_with(|cell| {
                // Busy when `f` itself calls `with_str`.
                let mut buffer = cell.try_borrow_mut().ok()?;
                let f = f.take()?;
                buffer.clear();
                self.push_to(&mut buffer);
                Some(f(&buffer))
            });
            if let Ok(Some(result)) = result {
                return result;
            }
        }
        match f {
            Some(f) => f(&self.into_string()),
            // `f` is only taken right before it is called and its result
            // returned above.
            None => unreachable!("with_str callback consumed without a result"),
        }
    }

    /// Appends the string to `out`.
    fn push_to(&self, out: &mut String) {
        for (i, piece) in self.pieces().iter().enumerate() {
            if i > 0 {
                out.push('.');
            }
            out.push_str(piece);
        }
    }

    /// Returns the string as a newly allocated `String`.
    pub fn into_string(self) -> String {
        let mut out = String::with_capacity(self.len());
        self.push_to(&mut out);
        out
    }
}

/// Maximum number of chunks of a string: the pieces and the separators.
const MAX_CHUNKS: usize = 2 * LEVELS - 1;

/// A string as a short sequence of byte chunks.
struct Chunks {
    chunks: [&'static [u8]; MAX_CHUNKS],
    count: usize,
}

impl Chunks {
    fn as_slice(&self) -> &[&'static [u8]] {
        &self.chunks[..self.count]
    }
}

/// Lexicographic byte comparison of two strings given as chunk sequences.
fn cmp_chunks(a: &[&[u8]], b: &[&[u8]]) -> Ordering {
    let (mut next_a, mut next_b) = (0, 0);
    let mut chunk_a: &[u8] = &[];
    let mut chunk_b: &[u8] = &[];
    loop {
        while chunk_a.is_empty() && next_a < a.len() {
            chunk_a = a[next_a];
            next_a += 1;
        }
        while chunk_b.is_empty() && next_b < b.len() {
            chunk_b = b[next_b];
            next_b += 1;
        }
        match (chunk_a.is_empty(), chunk_b.is_empty()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => {}
        }
        let n = chunk_a.len().min(chunk_b.len());
        match chunk_a[..n].cmp(&chunk_b[..n]) {
            Ordering::Equal => {
                chunk_a = &chunk_a[n..];
                chunk_b = &chunk_b[n..];
            }
            unequal => return unequal,
        }
    }
}

impl fmt::Display for Resolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.width().is_some() || f.precision().is_some() {
            return self.with_str(|s| f.pad(s));
        }
        for (i, piece) in self.pieces().iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            f.write_str(piece)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Resolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.with_str(|s| fmt::Debug::fmt(s, f))
    }
}

impl PartialEq for Resolved {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Resolved {}

impl PartialOrd for Resolved {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Resolved {
    fn cmp(&self, other: &Self) -> Ordering {
        cmp_chunks(self.chunks().as_slice(), other.chunks().as_slice())
    }
}

impl PartialEq<str> for Resolved {
    fn eq(&self, other: &str) -> bool {
        self.len() == other.len()
            && cmp_chunks(self.chunks().as_slice(), &[other.as_bytes()]) == Ordering::Equal
    }
}

impl PartialEq<&str> for Resolved {
    fn eq(&self, other: &&str) -> bool {
        *self == **other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leak(s: &str) -> &'static str {
        Box::leak(Box::from(s))
    }

    /// Splits like the interner does: two leading components and the rest.
    fn view(s: &str) -> Resolved {
        let mut pieces = [""; LEVELS];
        let mut parts = s.splitn(LEVELS, '.');
        let mut count = 0;
        for (slot, part) in pieces.iter_mut().zip(&mut parts) {
            *slot = leak(part);
            count += 1;
        }
        Resolved::from_pieces(pieces, count)
    }

    /// A single piece view, like an overflowed handle.
    fn flat(s: &str) -> Resolved {
        Resolved::from_pieces([leak(s), "", ""], 1)
    }

    const SAMPLES: &[&str] = &[
        "",
        ".",
        "..",
        "...",
        "A",
        "A.",
        ".A",
        "A.B",
        "A..B",
        "A.B.C",
        "A.B.C.D",
        "A.B.C.D.",
        "A.BC",
        "A.B-",
        "A-B",
        "AB",
        "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT",
        "CLBLL_L_X12Y124.SLICEL_X0",
        "CLBLL_L_X12Y124.SLICEL_X0.BLUT",
        "CLBLL_L_X12Y124.SLICEL_X1",
        "ü.ß.漢字.🙂",
    ];

    #[test]
    fn string_operations_match_str() {
        for &s in SAMPLES {
            for v in [view(s), flat(s)] {
                assert_eq!(v.into_string(), s);
                assert_eq!(v.to_string(), s);
                assert_eq!(format!("{v:?}"), format!("{s:?}"));
                assert_eq!(v.len(), s.len());
                assert_eq!(v.is_empty(), s.is_empty());
                assert_eq!(
                    v.components().collect::<Vec<_>>(),
                    s.split('.').collect::<Vec<_>>()
                );
                assert_eq!(v.first_component(), s.split('.').next().unwrap_or(""));
                assert_eq!(v.with_str(str::to_owned), s);
                assert!(v == *s);
                assert!(v == s);
            }
        }
    }

    #[test]
    fn comparisons_match_str() {
        for &a in SAMPLES {
            for &b in SAMPLES {
                let expected = a.cmp(b);
                assert_eq!(view(a).cmp(&view(b)), expected, "{a:?} vs {b:?}");
                assert_eq!(flat(a).cmp(&view(b)), expected, "{a:?} vs {b:?}");
                assert_eq!(view(a).cmp(&flat(b)), expected, "{a:?} vs {b:?}");
                assert_eq!(view(a) == view(b), a == b);
                assert_eq!(view(a) == *b, a == b);
            }
        }
    }

    #[test]
    fn starts_with_component() {
        let reference = |s: &str, p: &str| s == p || s.starts_with(&format!("{p}."));
        let prefixes = [
            "",
            ".",
            "A",
            "A.",
            "A.B",
            "A.B.",
            "A.B.C",
            "A.BC",
            "CLBLL_L_X12Y124",
            "CLBLL",
        ];
        for &s in SAMPLES {
            for v in [view(s), flat(s)] {
                for p in prefixes.iter().copied().chain(SAMPLES.iter().copied()) {
                    assert_eq!(v.starts_with_component(p), reference(s, p), "{s:?} / {p:?}");
                }
            }
        }
    }

    #[test]
    fn with_str_nested_and_long() {
        let a = view("A.B.C");
        let b = view("D.E");
        let joined = a.with_str(|x| b.with_str(|y| format!("{x}|{y}")));
        assert_eq!(joined, "A.B.C|D.E");
        let long = format!("{}.{}", "L".repeat(MAX_BUFFERED), "M");
        assert_eq!(view(&long).with_str(str::to_owned), long);
        assert_eq!(a.with_str(str::to_owned), "A.B.C");
        assert_eq!(format!("{:.3}|{:6}|", a, b), "A.B|D.E   |");
    }

    #[test]
    fn long_strings_and_padding() {
        let long = vec!["COMPONENT"; 1000].join(".");
        let v = view(&long);
        assert_eq!(v.with_str(str::to_owned), long);
        assert_eq!(v.components().count(), 1000);
        assert_eq!(
            format!("{:>6}|{:<6}|", view("A.B"), view("A.B")),
            "   A.B|A.B   |"
        );
    }
}

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

//! [`Interner`]: the per-level tables behind [`IdString`].

use std::cmp::Ordering;
use std::fmt;
use std::hash::BuildHasher;
use std::sync::OnceLock;

use foldhash::fast::RandomState;

use super::repr::{
    decode, encode_levels, encode_overflow, Repr, LEVELS, LEVEL_LIMITS, OVERFLOW_LIMIT,
};
use super::resolved::Resolved;
use super::storage::Table;
use super::IdString;

/// A string interner producing [`IdString`] handles.
///
/// Strings are split on `.` into at most three levels (first component,
/// second component, remainder) and each level is interned in its own
/// table. The process wide interner used by all [`IdString`] methods is
/// [`GLOBAL`](super::GLOBAL); a separate `Interner` is useful for tests and
/// for isolating unrelated data.
///
/// All methods take `&self` and are thread safe. Resolving a handle and
/// looking up known text take no lock; only inserting new text takes the
/// writer lock of one of 16 shards of the table concerned.
///
/// When a level table is full, strings that need a new entry in it are
/// interned whole in a separate overflow table instead (the canonical
/// handle of such a string is then the overflow form, forever). Nothing is
/// lost and nothing panics; see `docs/rewrite/DESIGN-idstring.md`.
///
/// Interned text is never freed, not even when a private `Interner` is
/// dropped (only its index structures are); this is what allows the
/// `&'static str` results of [`Resolved`].
///
/// A handle must only be used with the interner that created it: resolving
/// a foreign handle panics (unknown entry) or yields an unrelated string.
pub struct Interner {
    hasher: OnceLock<RandomState>,
    levels: [Table; LEVELS],
    overflow: Table,
}

/// Size statistics of an [`Interner`], see [`Interner::stats`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct InternerStats {
    /// Number of distinct texts in each level table (first component,
    /// second component, remainder).
    pub level_entries: [usize; LEVELS],
    /// Number of strings stored whole in the overflow table.
    pub overflow_entries: usize,
    /// Heap bytes allocated by the tables (text, entry slots and hash
    /// indexes). Excludes the fixed size `Interner` value itself.
    pub heap_bytes: usize,
}

/// `min` for `u32` in const context.
const fn min(a: u32, b: u32) -> u32 {
    if a < b {
        a
    } else {
        b
    }
}

/// Position of the first `.` in `bytes`.
///
/// Scans eight bytes at a time (a "has zero byte" test on the word XORed
/// with dots); FASM components are typically 5 to 25 bytes long, where this
/// beats both a byte loop and `memchr` call overhead.
fn find_dot(bytes: &[u8]) -> Option<usize> {
    const DOTS: u64 = u64::from_ne_bytes([b'.'; 8]);
    const LOW: u64 = u64::from_ne_bytes([0x01; 8]);
    const HIGH: u64 = u64::from_ne_bytes([0x80; 8]);
    let (words, tail) = bytes.as_chunks::<8>();
    for (i, word) in words.iter().enumerate() {
        let x = u64::from_le_bytes(*word) ^ DOTS;
        // The lowest set bit marks the first zero byte of `x` (borrows
        // only produce false positives above it).
        let found = x.wrapping_sub(LOW) & !x & HIGH;
        if found != 0 {
            return Some(i * 8 + (found.trailing_zeros() / 8) as usize);
        }
    }
    let at = tail.iter().position(|&b| b == b'.')?;
    Some(words.len() * 8 + at)
}

/// Splits `s` into its levels: the first two `.` separated components and
/// the remainder. Returns the pieces and their number (1 to [`LEVELS`]).
fn split_levels(s: &str) -> ([&str; LEVELS], usize) {
    let mut pieces = [""; LEVELS];
    let mut rest = s;
    let mut count = 0;
    while count < LEVELS - 1 {
        let Some(dot) = find_dot(rest.as_bytes()) else {
            break;
        };
        // `dot` is the index of an ASCII byte, hence a char boundary.
        let (head, tail) = rest.split_at(dot);
        pieces[count] = head;
        rest = &tail[1..];
        count += 1;
    }
    pieces[count] = rest;
    (pieces, count + 1)
}

#[cold]
#[inline(never)]
fn foreign(id: IdString) -> ! {
    // Do not format `id` with `Debug`: that would resolve it again.
    panic!(
        "IdString {:#018x} was not created by this interner",
        id.raw().get()
    )
}

impl Interner {
    /// Creates an empty interner with the full table capacities (16,777,214
    /// distinct first components and 1,048,575 distinct second components
    /// and remainders).
    pub const fn new() -> Self {
        Self::with_level_limit(u32::MAX)
    }

    /// Creates an empty interner whose level tables hold at most `limit`
    /// entries each (capped at the capacities of [`Interner::new`]).
    ///
    /// Handles have the same layout as with [`Interner::new`]; this is
    /// meant for testing what happens when a level table is full.
    pub const fn with_level_limit(limit: u32) -> Self {
        Interner {
            hasher: OnceLock::new(),
            levels: [
                Table::new(min(limit, LEVEL_LIMITS[0])),
                Table::new(min(limit, LEVEL_LIMITS[1])),
                Table::new(min(limit, LEVEL_LIMITS[2])),
            ],
            overflow: Table::new(OVERFLOW_LIMIT),
        }
    }

    fn hasher(&self) -> &RandomState {
        self.hasher.get_or_init(RandomState::default)
    }

    /// Interns `s` and returns its handle.
    ///
    /// Every string has exactly one handle per interner, so equal strings
    /// give equal handles. When all components are already known, this does
    /// one hash lookup per level and no allocation.
    ///
    /// # Panics
    ///
    /// Panics only if the overflow table is exhausted, which takes more
    /// than 4 billion distinct overflowed strings (hundreds of GiB of text)
    /// and is treated like running out of memory.
    pub fn intern(&self, s: &str) -> IdString {
        let hasher = self.hasher();
        let mut pos0 = 0;
        let mut fields = [0u32; LEVELS - 1];
        let (pieces, count) = split_levels(s);
        for (level, (table, &piece)) in self.levels.iter().zip(&pieces[..count]).enumerate() {
            let Some(pos) = table.intern(hasher.hash_one(piece), piece) else {
                // `piece` is not in the table and the table is full, which
                // can never change: `s` is represented in the overflow table.
                return self.intern_overflow(s);
            };
            if level == 0 {
                pos0 = pos;
            } else {
                fields[level - 1] = pos + 1;
            }
        }
        IdString::from_raw(encode_levels(pos0, fields))
    }

    #[cold]
    fn intern_overflow(&self, s: &str) -> IdString {
        let hasher = self.hasher();
        match self.overflow.intern(hasher.hash_one(s), s) {
            Some(pos) => IdString::from_raw(encode_overflow(pos)),
            None => panic!("idstring overflow table exhausted ({OVERFLOW_LIMIT} entries)"),
        }
    }

    /// Returns the handle of `s` if it can be produced without adding
    /// anything to the tables, i.e. without interning `s`.
    ///
    /// This is the case for every string interned before, but also for a
    /// string that was never interned whole when each of its levels is
    /// already known (after interning `A.B.C` and `X.Y`, `get("A.Y")` is
    /// `Some`). So `get` is a cheap lookup, not a set membership test.
    pub fn get(&self, s: &str) -> Option<IdString> {
        let hasher = self.hasher();
        let mut pos0 = 0;
        let mut fields = [0u32; LEVELS - 1];
        let (pieces, count) = split_levels(s);
        for (level, (table, &piece)) in self.levels.iter().zip(&pieces[..count]).enumerate() {
            let Some(pos) = table.find(hasher.hash_one(piece), piece) else {
                // Some component is unknown: `s` can only be known as an
                // overflowed string.
                let pos = self.overflow.find(hasher.hash_one(s), s)?;
                return Some(IdString::from_raw(encode_overflow(pos)));
            };
            if level == 0 {
                pos0 = pos;
            } else {
                fields[level - 1] = pos + 1;
            }
        }
        Some(IdString::from_raw(encode_levels(pos0, fields)))
    }

    /// The text of level `level` of a hierarchical handle (the level must
    /// be present).
    fn level_text(&self, id: IdString, fields: &[u32; LEVELS], level: usize) -> &'static str {
        self.levels[level]
            .text(fields[level].wrapping_sub(1))
            .unwrap_or_else(|| foreign(id))
    }

    /// Resolves the levels `from..` of a hierarchical handle (the first of
    /// them must be present).
    fn resolve_levels(&self, id: IdString, fields: &[u32; LEVELS], from: usize) -> Resolved {
        let mut pieces = [""; LEVELS];
        let mut count = 0;
        for level in from..LEVELS {
            if fields[level] == 0 {
                break;
            }
            pieces[count] = self.level_text(id, fields, level);
            count += 1;
        }
        Resolved::from_pieces(pieces, count)
    }

    /// Returns the interned pieces of `id`.
    ///
    /// # Panics
    ///
    /// Panics if `id` was not created by this interner and refers to an
    /// entry that does not exist.
    pub fn resolved(&self, id: IdString) -> Resolved {
        match decode(id.raw()) {
            Repr::Levels(fields) => self.resolve_levels(id, &fields, 0),
            Repr::Overflow(pos) => {
                let text = self.overflow.text(pos).unwrap_or_else(|| foreign(id));
                Resolved::from_pieces([text, "", ""], 1)
            }
        }
    }

    /// Returns the string of `id` as a new `String`.
    ///
    /// # Panics
    ///
    /// See [`Interner::resolved`].
    pub fn resolve(&self, id: IdString) -> String {
        self.resolved(id).into_string()
    }

    /// Calls `f` with the string of `id`; see [`Resolved::with_str`].
    ///
    /// # Panics
    ///
    /// See [`Interner::resolved`].
    pub fn with_str<R>(&self, id: IdString, f: impl FnOnce(&str) -> R) -> R {
        self.resolved(id).with_str(f)
    }

    /// Compares the strings of two handles (lexicographically by bytes,
    /// like `str`).
    ///
    /// Equal handles and equal leading levels are decided without reading
    /// the tables; the remaining pieces are compared without allocating.
    ///
    /// # Panics
    ///
    /// See [`Interner::resolved`].
    pub fn cmp(&self, a: IdString, b: IdString) -> Ordering {
        if a == b {
            return Ordering::Equal;
        }
        if let (Repr::Levels(x), Repr::Levels(y)) = (decode(a.raw()), decode(b.raw())) {
            if let Some(level) = (0..LEVELS).find(|&level| x[level] != y[level]) {
                // Levels before `level` hold the same text. If one side has
                // no level `level`, it is a proper prefix of the other one.
                // Otherwise both continue with '.', so compare what follows.
                if x[level] == 0 {
                    return Ordering::Less;
                }
                if y[level] == 0 {
                    return Ordering::Greater;
                }
                // The two texts of `level` differ. Unless one is a prefix of
                // the other, they decide the order on their own.
                let (text_a, text_b) =
                    (self.level_text(a, &x, level), self.level_text(b, &y, level));
                let common = text_a.len().min(text_b.len());
                let order = text_a.as_bytes()[..common].cmp(&text_b.as_bytes()[..common]);
                if order != Ordering::Equal {
                    return order;
                }
                return self
                    .resolve_levels(a, &x, level)
                    .cmp(&self.resolve_levels(b, &y, level));
            }
        }
        self.resolved(a).cmp(&self.resolved(b))
    }
}

impl Interner {
    /// Returns the number of entries and the heap usage of the tables.
    ///
    /// Takes every shard lock briefly (one at a time); meant for reporting,
    /// not for hot paths.
    pub fn stats(&self) -> InternerStats {
        InternerStats {
            level_entries: self.levels.each_ref().map(|table| table.len() as usize),
            overflow_entries: self.overflow.len() as usize,
            heap_bytes: self
                .levels
                .iter()
                .chain([&self.overflow])
                .map(Table::heap_bytes)
                .sum(),
        }
    }
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Interner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stats = self.stats();
        f.debug_struct("Interner")
            .field("level_entries", &stats.level_entries)
            .field("overflow_entries", &stats.overflow_entries)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_dot_matches_position() {
        let long = format!("{}.{}", "x".repeat(37), "y".repeat(9));
        for s in [
            "",
            ".",
            "a",
            "a.",
            ".a",
            "abcdefg.",
            "abcdefgh.",
            "abcdefghi.",
            "ü.ß",
            &long,
        ] {
            assert_eq!(find_dot(s.as_bytes()), s.find('.'), "{s:?}");
        }
        // Bytes whose XOR with '.' borrows (0x2f, 0xae) must not confuse
        // the word scan.
        let tricky = [0x2f, 0x2f, 0xae, 0x2d, 0x2e, 0x2f, 0x2e, 0x00, 0x2f, 0x2e];
        for start in 0..tricky.len() {
            let expected = tricky[start..].iter().position(|&b| b == b'.');
            assert_eq!(find_dot(&tricky[start..]), expected);
        }
    }

    #[test]
    fn split_levels_matches_splitn() {
        for s in [
            "",
            ".",
            "..",
            "...",
            "A",
            "A.B",
            "A.B.C",
            "A.B.C.D",
            "A..B",
            ".A.",
            "ü.ß.漢.字",
        ] {
            let (pieces, count) = split_levels(s);
            assert_eq!(
                pieces[..count],
                s.splitn(LEVELS, '.').collect::<Vec<_>>()[..],
                "{s:?}"
            );
        }
    }
}

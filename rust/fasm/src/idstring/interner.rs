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
use std::hash::{BuildHasher, Hasher};
use std::str::Utf8Error;
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
///
/// Handle values are made of entry numbers handed out in first-come order,
/// so they (and their `Hash`) differ between runs; order handles with
/// [`Interner::cmp`] (or `Ord` for [`GLOBAL`](super::GLOBAL) handles) for
/// reproducible output.
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
#[inline]
fn find_dot(bytes: &[u8]) -> Option<usize> {
    /// Bit 7 of each byte of the result is set where `word` has a `.`,
    /// exactly up to and including the first one (borrows only produce
    /// false positives above it).
    #[inline]
    fn dots(word: [u8; 8]) -> u64 {
        const DOTS: u64 = u64::from_ne_bytes([b'.'; 8]);
        const LOW: u64 = u64::from_ne_bytes([0x01; 8]);
        const HIGH: u64 = u64::from_ne_bytes([0x80; 8]);
        let x = u64::from_le_bytes(word) ^ DOTS;
        x.wrapping_sub(LOW) & !x & HIGH
    }
    let (words, tail) = bytes.as_chunks::<8>();
    for (i, word) in words.iter().enumerate() {
        let found = dots(*word);
        if found != 0 {
            return Some(i * 8 + (found.trailing_zeros() / 8) as usize);
        }
    }
    if tail.is_empty() {
        return None;
    }
    if let Some(last) = bytes.last_chunk::<8>() {
        // Rescan the tail as part of the (overlapping) last word. Its
        // first bytes were scanned above and hold no `.`, so they cannot
        // produce a false positive and the lowest set bit is exact.
        let found = dots(*last);
        return (found != 0).then(|| bytes.len() - 8 + (found.trailing_zeros() / 8) as usize);
    }
    tail.iter().position(|&b| b == b'.')
}

/// Splits `s` into its levels: the first two `.` separated components and
/// the remainder. Returns the pieces and their number (1 to [`LEVELS`]).
///
/// Equivalent to `s.splitn(LEVELS, '.')` on the corresponding `str`.
#[inline]
fn split_levels(s: &[u8]) -> ([&[u8]; LEVELS], usize) {
    const _: () = assert!(LEVELS == 3);
    let Some(first) = find_dot(s) else {
        return ([s, &[], &[]], 1);
    };
    let (head, rest) = (&s[..first], &s[first + 1..]);
    let Some(second) = find_dot(rest) else {
        return ([head, rest, &[]], 2);
    };
    ([head, &rest[..second], &rest[second + 1..]], 3)
}

/// The level pieces of `s` given the positions of its first two `.`
/// (`usize::MAX` where there is none), like [`split_levels`].
#[inline]
fn split_at_dots(s: &[u8], dots: [usize; 2]) -> ([&[u8]; LEVELS], usize) {
    let Some((head, rest)) = s.split_at_checked(dots[0]) else {
        return ([s, &[], &[]], 1);
    };
    let rest = rest.get(1..).unwrap_or_default();
    match dots[1]
        .checked_sub(dots[0] + 1)
        .and_then(|at| rest.split_at_checked(at))
    {
        Some((middle, last)) => ([head, middle, last.get(1..).unwrap_or_default()], 3),
        None => ([head, rest, &[]], 2),
    }
}

/// Hash of a level piece (or of a whole overflowed string) under the
/// interner's seed.
///
/// A single `write` of the bytes (unlike `hash_one(&str)`, which also
/// writes a terminator byte and so costs a second multiplication).
#[inline]
fn hash(state: &RandomState, bytes: &[u8]) -> u64 {
    let mut hasher = state.build_hasher();
    hasher.write(bytes);
    hasher.finish()
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

    #[inline]
    fn hasher(&self) -> &RandomState {
        self.hasher.get_or_init(RandomState::default)
    }

    /// The hierarchical handle of `s` if every level of it is already in
    /// the level tables. Takes no lock and allocates nothing.
    ///
    /// `s` need not be valid UTF-8: a result means that every level equals
    /// interned text, so `s` is valid UTF-8 (the levels joined by `.`).
    ///
    /// A string whose levels are all present is never in the overflow
    /// table (a component missing from a full table can never be added),
    /// so the result is the canonical handle.
    #[inline]
    fn find_levels(&self, state: &RandomState, s: &[u8]) -> Option<IdString> {
        let (pieces, count) = split_levels(s);
        self.find_pieces(state, pieces, count)
    }

    /// [`Interner::find_levels`] for a string already split into its
    /// `count` level pieces.
    #[inline]
    fn find_pieces(
        &self,
        state: &RandomState,
        pieces: [&[u8]; LEVELS],
        count: usize,
    ) -> Option<IdString> {
        let first = pieces[0];
        let pos0 = self.levels[0].find(hash(state, first), first)?;
        let mut fields = [0u32; LEVELS - 1];
        for (level, field) in fields.iter_mut().enumerate().take(count - 1) {
            let piece = pieces[level + 1];
            *field = self.levels[level + 1].find(hash(state, piece), piece)? + 1;
        }
        Some(IdString::from_raw(encode_levels(pos0, fields)))
    }

    /// The overflow handle of `s` if `s` is in the overflow table. Takes no
    /// lock and allocates nothing; like [`Interner::find_levels`], a result
    /// implies that `s` is valid UTF-8.
    ///
    /// A string in the overflow table can never become hierarchical (one of
    /// its components is missing from a full table forever), so the result
    /// is the canonical handle.
    #[inline]
    fn find_overflow(&self, state: &RandomState, s: &[u8]) -> Option<IdString> {
        // Nothing overflowed (the normal case): skip hashing `s`. A stale
        // zero only makes this miss an entry being inserted concurrently.
        if self.overflow.len() == 0 {
            return None;
        }
        let pos = self.overflow.find(hash(state, s), s)?;
        Some(IdString::from_raw(encode_overflow(pos)))
    }

    /// Interns `s` and returns its handle.
    ///
    /// Every string has exactly one handle per interner, so equal strings
    /// give equal handles. When all components are already known, this does
    /// one lock free hash lookup per level and no allocation.
    ///
    /// # Panics
    ///
    /// Panics only if the overflow table is exhausted, which takes more
    /// than 4 billion distinct overflowed strings (hundreds of GiB of text)
    /// and is treated like running out of memory.
    pub fn intern(&self, s: &str) -> IdString {
        let state = self.hasher();
        match self.find_levels(state, s.as_bytes()) {
            Some(id) => id,
            None => self.intern_missing(state, s),
        }
    }

    /// Interns the UTF-8 string `bytes` and returns its handle.
    ///
    /// Equivalent to `intern(std::str::from_utf8(bytes)?)`, but the UTF-8
    /// validation is only done when `bytes` is not known yet: a known
    /// string equals interned text, which is valid UTF-8. So interning known
    /// names from a byte buffer (a parser's input) costs the same as
    /// [`Interner::intern`].
    ///
    /// # Errors
    ///
    /// Returns the UTF-8 error if `bytes` is not valid UTF-8.
    ///
    /// # Panics
    ///
    /// See [`Interner::intern`].
    pub fn intern_bytes(&self, bytes: &[u8]) -> Result<IdString, Utf8Error> {
        let state = self.hasher();
        if let Some(id) = self.find_levels(state, bytes) {
            return Ok(id);
        }
        let s = std::str::from_utf8(bytes)?;
        Ok(self.intern_missing(state, s))
    }

    /// [`Interner::intern_bytes`] for a name whose first two `.` are known
    /// to be at `dots[0]` and `dots[1]` (`usize::MAX` where there is no
    /// such `.`), as found by a scanner that has just read the name: skips
    /// the search for them.
    ///
    /// The result is the same as `intern_bytes(bytes)` (a debug assertion
    /// checks the positions).
    ///
    /// # Errors
    ///
    /// Returns the UTF-8 error if `bytes` is not valid UTF-8.
    #[inline]
    pub(crate) fn intern_split(
        &self,
        bytes: &[u8],
        dots: [usize; 2],
    ) -> Result<IdString, Utf8Error> {
        let (pieces, count) = split_at_dots(bytes, dots);
        debug_assert_eq!((pieces, count), split_levels(bytes));
        let state = self.hasher();
        if let Some(id) = self.find_pieces(state, pieces, count) {
            return Ok(id);
        }
        let s = std::str::from_utf8(bytes)?;
        Ok(self.intern_missing(state, s))
    }

    /// [`Interner::intern`] for a string with at least one level missing
    /// from the level tables (as seen by a lock free probe).
    #[cold]
    #[inline(never)]
    fn intern_missing(&self, state: &RandomState, s: &str) -> IdString {
        // An overflowed string is interned again without taking any lock:
        // a string in the overflow table can never become hierarchical
        // (one of its components is missing from a full table forever), so
        // this handle is final. A miss here (not overflowed, or inserted
        // concurrently and not visible yet) takes the locked path below,
        // which decides authoritatively.
        if let Some(id) = self.find_overflow(state, s.as_bytes()) {
            return id;
        }
        let mut pos0 = 0;
        let mut fields = [0u32; LEVELS - 1];
        // The same pieces as `split_levels` (checked by a test), so the
        // same hashes.
        for (level, (table, piece)) in self.levels.iter().zip(s.splitn(LEVELS, '.')).enumerate() {
            let Some(pos) = table.intern(hash(state, piece.as_bytes()), piece) else {
                // `piece` is not in the table and the table is full, which
                // can never change: `s` is represented in the overflow table.
                return self.intern_overflow(state, s);
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
    fn intern_overflow(&self, state: &RandomState, s: &str) -> IdString {
        match self.overflow.intern(hash(state, s.as_bytes()), s) {
            Some(pos) => IdString::from_raw(encode_overflow(pos)),
            None => panic!("idstring overflow table exhausted ({OVERFLOW_LIMIT} entries)"),
        }
    }

    /// Returns the handle `s` would have, if that handle can be produced
    /// without adding anything to the tables. Takes no lock, allocates
    /// nothing and never interns.
    ///
    /// **This is not a membership test.** It returns `Some` for every
    /// string interned before, but also for strings that were never
    /// interned whole when each of their levels is already known: after
    /// interning `A.B.C` and `X.Y`, `lookup("A.Y")` and `lookup("X.B.C")`
    /// are `Some`. Use it to avoid growing the tables (e.g. to look a name
    /// up in a map keyed by `IdString`: a `None` means that no key can
    /// equal `s`), not to ask whether `s` was seen.
    ///
    /// A `None` may also be returned for a string that another thread is
    /// interning at the same moment (there is no happens-before relation
    /// with that insertion); once the interning call has returned and that
    /// is visible to this thread (thread join, a lock, a `Release` store
    /// read with `Acquire`, ...), `lookup` finds it.
    pub fn lookup(&self, s: &str) -> Option<IdString> {
        let state = self.hasher();
        let bytes = s.as_bytes();
        self.find_levels(state, bytes)
            .or_else(|| self.find_overflow(state, bytes))
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
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn known_and_overflowed_names_are_interned_without_locking() {
        let interner = Interner::with_level_limit(2);
        let names = [
            "A.B.C", "A.B.D", "A.X.C", "P.B.C", "A", "A.B", "P.X", // hierarchical
            "A.B.E", "A.Y.C", "Q.B.C", "Q", "A.Y", "Q.B.C.D", "", // overflowed
        ];
        let ids: Vec<IdString> = names.iter().map(|s| interner.intern(s)).collect();
        let overflowed = ids
            .iter()
            .filter(|id| matches!(decode(id.raw()), Repr::Overflow(_)))
            .count();
        assert_eq!(overflowed, 7);
        // With every writer lock held by this thread, interning known
        // names (hierarchical or overflowed) must still complete.
        let guards: Vec<_> = self::tables(&interner).map(Table::lock_writers).collect();
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let (interner, names) = (&interner, &names);
            scope.spawn(move || {
                let again: Vec<IdString> = names.iter().map(|s| interner.intern(s)).collect();
                let bytes: Vec<IdString> = names
                    .iter()
                    .filter_map(|s| interner.intern_bytes(s.as_bytes()).ok())
                    .collect();
                let found: Vec<Option<IdString>> =
                    names.iter().map(|s| interner.lookup(s)).collect();
                // The receiver may have given up already.
                let _ = tx.send((again, bytes, found));
            });
            let result = rx.recv_timeout(Duration::from_secs(60));
            drop(guards);
            let (again, bytes, found) = result.expect("interning a known name took a lock");
            assert_eq!(again, ids);
            assert_eq!(bytes, ids);
            assert_eq!(found, ids.iter().copied().map(Some).collect::<Vec<_>>());
        });
    }

    fn tables(interner: &Interner) -> impl Iterator<Item = &Table> {
        interner.levels.iter().chain([&interner.overflow])
    }

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
        // Every length up to three words, with no dot, one dot or two dots
        // at every position (covers the overlapping last word), over
        // backgrounds of bytes next to '.' (0x2e).
        for background in [b'x', 0x2f, 0x2d, 0x00, 0xae, 0xff] {
            for len in 0..=24 {
                for first in 0..=len {
                    for second in first..=len {
                        let mut bytes = vec![background; len];
                        if first < len {
                            bytes[first] = b'.';
                        }
                        if second < len {
                            bytes[second] = b'.';
                        }
                        let expected = bytes.iter().position(|&b| b == b'.');
                        assert_eq!(find_dot(&bytes), expected, "{bytes:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn split_levels_matches_splitn() {
        let long = format!("{}.{}.{}", "a".repeat(17), "b".repeat(9), "c.d".repeat(5));
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
            "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT",
            &long,
        ] {
            let (pieces, count) = split_levels(s.as_bytes());
            let expected: Vec<&[u8]> = s.splitn(LEVELS, '.').map(str::as_bytes).collect();
            assert_eq!(pieces[..count], expected[..], "{s:?}");
        }
    }
}

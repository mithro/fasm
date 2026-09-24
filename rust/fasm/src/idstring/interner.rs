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

use super::repr::{decode, encode_levels, Repr, LEVELS, LEVEL_LIMITS};
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
/// All methods take `&self` and are thread safe. Resolving a handle takes no
/// lock; interning takes a read lock on one of 16 shards per level (a write
/// lock when the text is new).
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
}

/// `min` for `u32` in const context.
const fn min(a: u32, b: u32) -> u32 {
    if a < b {
        a
    } else {
        b
    }
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
    pub fn intern(&self, s: &str) -> IdString {
        let hasher = self.hasher();
        let mut pos0 = 0;
        let mut fields = [0u32; LEVELS - 1];
        for (level, piece) in s.splitn(LEVELS, '.').enumerate() {
            let table = &self.levels[level];
            let Some(pos) = table.intern(hasher.hash_one(piece), piece, hasher) else {
                panic!("idstring level {level} table is full");
            };
            if level == 0 {
                pos0 = pos;
            } else {
                fields[level - 1] = pos + 1;
            }
        }
        IdString::from_raw(encode_levels(pos0, fields))
    }

    /// Returns the handle of `s` if it has already been interned, without
    /// interning it.
    pub fn get(&self, s: &str) -> Option<IdString> {
        let hasher = self.hasher();
        let mut pos0 = 0;
        let mut fields = [0u32; LEVELS - 1];
        for (level, piece) in s.splitn(LEVELS, '.').enumerate() {
            let pos = self.levels[level].find(hasher.hash_one(piece), piece)?;
            if level == 0 {
                pos0 = pos;
            } else {
                fields[level - 1] = pos + 1;
            }
        }
        Some(IdString::from_raw(encode_levels(pos0, fields)))
    }

    /// Resolves the levels `from..` of a hierarchical handle (the first of
    /// them must be present).
    fn resolve_levels(&self, id: IdString, fields: [u32; LEVELS], from: usize) -> Resolved {
        let mut pieces = [""; LEVELS];
        let mut count = 0;
        for (table, &field) in self.levels.iter().zip(&fields).skip(from) {
            if field == 0 {
                break;
            }
            pieces[count] = table.text(field - 1).unwrap_or_else(|| foreign(id));
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
            Repr::Levels(fields) => self.resolve_levels(id, fields, 0),
            Repr::Overflow(_) => foreign(id),
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
                return self
                    .resolve_levels(a, x, level)
                    .cmp(&self.resolve_levels(b, y, level));
            }
        }
        self.resolved(a).cmp(&self.resolved(b))
    }
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Interner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entries = self.levels.each_ref().map(Table::len);
        f.debug_struct("Interner")
            .field("level_entries", &entries)
            .finish_non_exhaustive()
    }
}

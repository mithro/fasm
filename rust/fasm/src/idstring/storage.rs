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

//! Storage building blocks of the interner (see `docs/rewrite/DESIGN-idstring.md`).
//!
//! * [`Arena`]: a bump allocator handing out `&'static str` copies of
//!   interned text from leaked chunks (text is never freed).
//! * [`Slots`]: an append only, segmented array of `&'static str` whose
//!   entries are written once and read without taking any lock.
//! * [`Table`]: one interning table (one level, or the overflow table):
//!   a sharded `text -> entry number` hash index on top of [`Slots`].
//!
//! Everything here is safe code: publication uses `OnceLock` and the
//! arena carves leaked chunks with `split_at_mut`.

use std::hash::BuildHasher;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{OnceLock, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use hashbrown::hash_table::Entry;
use hashbrown::HashTable;

/// Size of the first arena chunk; later chunks double up to [`MAX_CHUNK`].
const MIN_CHUNK: usize = 1024;
/// Largest arena chunk.
const MAX_CHUNK: usize = 64 * 1024;
/// Strings longer than this get their own leaked allocation instead of a
/// piece of a chunk, which bounds the waste at the end of each chunk.
const MAX_ARENA_STRING: usize = 4096;

/// Bump allocator for interned text.
///
/// Chunks are leaked (`Box::leak`) so every piece handed out is a genuine
/// `&'static str`. The arena itself only remembers the unused tail of the
/// current chunk.
pub(crate) struct Arena {
    rest: &'static mut [u8],
    next_chunk: usize,
    /// Total bytes allocated (chunks and dedicated strings).
    allocated: usize,
}

impl Arena {
    /// Creates an empty arena (no allocation until the first string).
    pub(crate) const fn new() -> Self {
        Arena {
            rest: &mut [],
            next_chunk: MIN_CHUNK,
            allocated: 0,
        }
    }

    /// Total bytes this arena allocated.
    pub(crate) fn allocated(&self) -> usize {
        self.allocated
    }

    /// Copies `s` into leaked memory and returns the copy.
    pub(crate) fn alloc(&mut self, s: &str) -> &'static str {
        let n = s.len();
        if n == 0 {
            return "";
        }
        if n > MAX_ARENA_STRING {
            self.allocated += n;
            return Box::leak(Box::<str>::from(s));
        }
        if self.rest.len() < n {
            // Any tail left in the old chunk is abandoned (at most
            // MAX_ARENA_STRING bytes out of a chunk of up to MAX_CHUNK).
            let size = self.next_chunk.max(n);
            self.next_chunk = (self.next_chunk * 2).min(MAX_CHUNK);
            self.allocated += size;
            self.rest = Box::leak(vec![0u8; size].into_boxed_slice());
        }
        let (piece, rest) = std::mem::take(&mut self.rest).split_at_mut(n);
        self.rest = rest;
        piece.copy_from_slice(s.as_bytes());
        match std::str::from_utf8(piece) {
            Ok(text) => text,
            // Cannot happen (the bytes were copied from a `&str`); stay
            // correct without panicking or using `unsafe` anyway.
            Err(_) => {
                self.allocated += n;
                Box::leak(Box::<str>::from(s))
            }
        }
    }
}

/// log2 of the size of the first [`Slots`] segment.
const FIRST_SEGMENT_BITS: u32 = 6;
/// Number of segments needed to address every `u32` position: segment `k`
/// holds `64 << k` slots and covers positions `[64 * (2^k - 1), 64 * (2^(k+1) - 1))`.
const SEGMENTS: usize = 27;

/// Maps a slot position to `(segment, offset in segment)`.
fn locate(pos: u32) -> (usize, usize) {
    let q = u64::from(pos) + (1u64 << FIRST_SEGMENT_BITS);
    let bit = 63 - q.leading_zeros();
    let segment = (bit - FIRST_SEGMENT_BITS) as usize;
    let offset = (q - (1u64 << bit)) as usize;
    (segment, offset)
}

/// Number of slots in segment `segment`.
const fn segment_len(segment: usize) -> usize {
    1usize << (segment + FIRST_SEGMENT_BITS as usize)
}

/// Append only array of interned strings.
///
/// Segments are allocated on first use and never move or shrink, and each
/// slot is written exactly once, so a reader only needs two `Acquire`
/// loads (inside `OnceLock::get`) and never takes a lock.
pub(crate) struct Slots {
    segments: [OnceLock<Box<[OnceLock<&'static str>]>>; SEGMENTS],
}

impl Slots {
    /// Creates an empty array (no allocation).
    pub(crate) const fn new() -> Self {
        Slots {
            segments: [const { OnceLock::new() }; SEGMENTS],
        }
    }

    /// Returns the string at `pos`, or `None` if it was never set.
    pub(crate) fn get(&self, pos: u32) -> Option<&'static str> {
        let (segment, offset) = locate(pos);
        self.segments
            .get(segment)?
            .get()?
            .get(offset)?
            .get()
            .copied()
    }

    /// Publishes `text` at `pos`. Each position must be set at most once
    /// (positions are handed out exclusively by [`Table::reserve`]).
    pub(crate) fn set(&self, pos: u32, text: &'static str) {
        let (segment, offset) = locate(pos);
        let Some(cell) = self.segments.get(segment) else {
            debug_assert!(false, "slot position {pos} out of range");
            return;
        };
        let slots =
            cell.get_or_init(|| (0..segment_len(segment)).map(|_| OnceLock::new()).collect());
        let stored = slots.get(offset).map(|slot| slot.set(text));
        debug_assert_eq!(stored, Some(Ok(())), "slot {pos} set twice");
    }

    /// Bytes allocated for the segments.
    pub(crate) fn allocated(&self) -> usize {
        self.segments
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.get().is_some())
            .map(|(segment, _)| segment_len(segment) * size_of::<OnceLock<&'static str>>())
            .sum()
    }
}

/// Number of lock shards per [`Table`].
const SHARDS: usize = 16;

/// The part of a [`Table`] protected by one shard lock.
struct Shard {
    /// Entry numbers, hashed and compared by their text in [`Slots`].
    map: HashTable<u32>,
    /// Storage for the text of the entries inserted through this shard.
    arena: Arena,
}

/// One interning table: dense entry numbers `0..limit` for distinct
/// strings.
///
/// Lookups take the read lock of one shard (chosen from the hash);
/// insertions take its write lock. Entry numbers come from a shared atomic
/// counter so they stay dense across shards. Reading the text of an entry
/// ([`Table::text`]) takes no lock.
pub(crate) struct Table {
    limit: u32,
    next: AtomicU32,
    slots: Slots,
    shards: [RwLock<Shard>; SHARDS],
}

impl Table {
    /// Creates an empty table that will hold at most `limit` entries.
    pub(crate) const fn new(limit: u32) -> Self {
        Table {
            limit,
            next: AtomicU32::new(0),
            slots: Slots::new(),
            shards: [const {
                RwLock::new(Shard {
                    map: HashTable::new(),
                    arena: Arena::new(),
                })
            }; SHARDS],
        }
    }

    /// The text of entry `pos`, or `None` if there is no such entry.
    pub(crate) fn text(&self, pos: u32) -> Option<&'static str> {
        self.slots.get(pos)
    }

    /// Number of entries in the table (including ones being inserted by
    /// other threads right now).
    pub(crate) fn len(&self) -> u32 {
        self.next.load(Ordering::Relaxed)
    }

    /// Heap bytes used by the table: slot segments, hash indexes and text.
    pub(crate) fn heap_bytes(&self) -> usize {
        let shards: usize = self
            .shards
            .iter()
            .map(|shard| {
                let shard = shard.read().unwrap_or_else(PoisonError::into_inner);
                shard.map.allocation_size() + shard.arena.allocated()
            })
            .sum();
        self.slots.allocated() + shards
    }

    fn shard_index(hash: u64) -> usize {
        // Bits 40..44: hashbrown uses the low bits for the bucket and the
        // top 7 bits for its control bytes, so these are independent.
        ((hash >> 40) as usize) & (SHARDS - 1)
    }

    fn read(&self, hash: u64) -> RwLockReadGuard<'_, Shard> {
        self.shards[Self::shard_index(hash)]
            .read()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self, hash: u64) -> RwLockWriteGuard<'_, Shard> {
        self.shards[Self::shard_index(hash)]
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Looks `s` (whose hash under the interner's hasher is `hash`) up
    /// without inserting it.
    pub(crate) fn find(&self, hash: u64, s: &str) -> Option<u32> {
        let shard = self.read(hash);
        shard
            .map
            .find(hash, |&pos| self.slots.get(pos) == Some(s))
            .copied()
    }

    /// Returns the entry number of `s`, inserting it if needed. Returns
    /// `None` only when `s` is not present and the table is full; since a
    /// full table stays full, the answer for `s` never changes afterwards.
    pub(crate) fn intern(&self, hash: u64, s: &str, hasher: &impl BuildHasher) -> Option<u32> {
        if let Some(pos) = self.find(hash, s) {
            return Some(pos);
        }
        let mut guard = self.write(hash);
        let Shard { map, arena } = &mut *guard;
        let entry = map.entry(
            hash,
            |&pos| self.slots.get(pos) == Some(s),
            |&pos| self.slots.get(pos).map_or(0, |text| hasher.hash_one(text)),
        );
        match entry {
            Entry::Occupied(occupied) => Some(*occupied.get()),
            Entry::Vacant(vacant) => {
                let pos = self.reserve()?;
                self.slots.set(pos, arena.alloc(s));
                vacant.insert(pos);
                Some(pos)
            }
        }
    }

    /// Reserves the next entry number, or `None` if the table is full.
    fn reserve(&self) -> Option<u32> {
        let limit = self.limit;
        self.next
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < limit).then_some(n + 1)
            })
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use foldhash::fast::RandomState;

    #[test]
    fn locate_covers_positions_densely() {
        assert_eq!(locate(0), (0, 0));
        assert_eq!(locate(63), (0, 63));
        assert_eq!(locate(64), (1, 0));
        assert_eq!(locate(191), (1, 127));
        assert_eq!(locate(192), (2, 0));
        let (segment, offset) = locate(u32::MAX);
        assert_eq!(segment, SEGMENTS - 1);
        assert!(offset < segment_len(segment));
        // Consecutive positions map to consecutive (segment, offset) pairs.
        let mut previous = locate(0);
        for pos in 1..100_000u32 {
            let current = locate(pos);
            if current.0 == previous.0 {
                assert_eq!(current.1, previous.1 + 1);
            } else {
                assert_eq!(current, (previous.0 + 1, 0));
                assert_eq!(previous.1 + 1, segment_len(previous.0));
            }
            previous = current;
        }
    }

    #[test]
    fn arena_copies_strings() {
        let mut arena = Arena::new();
        let long = "x".repeat(MAX_ARENA_STRING + 1);
        let medium = "y".repeat(MIN_CHUNK + 10);
        let inputs = ["", "a", "héllo", long.as_str(), medium.as_str(), "b"];
        let copies: Vec<&'static str> = inputs.iter().map(|s| arena.alloc(s)).collect();
        assert_eq!(copies, inputs);
        for _ in 0..10_000 {
            assert_eq!(arena.alloc("CLBLL_L_X12Y124"), "CLBLL_L_X12Y124");
        }
        assert!(arena.allocated() >= 150_000 + long.len() + medium.len());
        assert!(arena.allocated() < 300_000);
    }

    #[test]
    fn slots_set_and_get() {
        let slots = Slots::new();
        assert_eq!(slots.get(0), None);
        assert_eq!(slots.get(u32::MAX), None);
        slots.set(0, "zero");
        slots.set(1000, "thousand");
        assert_eq!(slots.get(0), Some("zero"));
        assert_eq!(slots.get(1000), Some("thousand"));
        assert_eq!(slots.get(999), None);
    }

    #[test]
    fn table_interns_and_respects_limit() {
        let hasher = RandomState::default();
        let table = Table::new(3);
        let intern = |s: &str| table.intern(hasher.hash_one(s), s, &hasher);
        assert_eq!(intern("a"), Some(0));
        assert_eq!(intern("b"), Some(1));
        assert_eq!(intern("a"), Some(0));
        assert_eq!(intern(""), Some(2));
        assert_eq!(intern("c"), None);
        assert_eq!(intern("c"), None);
        assert_eq!(intern("b"), Some(1));
        assert_eq!(table.len(), 3);
        assert_eq!(table.find(hasher.hash_one("c"), "c"), None);
        assert_eq!(table.find(hasher.hash_one(""), ""), Some(2));
        assert_eq!(table.text(1), Some("b"));
        assert_eq!(table.text(3), None);
        assert!(table.heap_bytes() > 0);
    }

    #[test]
    fn table_grows() {
        let hasher = RandomState::default();
        let table = Table::new(u32::MAX);
        let names: Vec<String> = (0..50_000).map(|i| format!("INT_L_X{i}Y{i}")).collect();
        for (i, name) in names.iter().enumerate() {
            let pos = table.intern(hasher.hash_one(name.as_str()), name, &hasher);
            assert_eq!(pos, Some(i as u32));
        }
        for (i, name) in names.iter().enumerate() {
            assert_eq!(
                table.find(hasher.hash_one(name.as_str()), name),
                Some(i as u32)
            );
            assert_eq!(table.text(i as u32), Some(name.as_str()));
        }
    }
}

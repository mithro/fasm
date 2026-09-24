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
//!   a sharded, lock free readable `text -> entry number` hash index on
//!   top of [`Slots`].
//!
//! Everything here is safe code: publication uses `OnceLock` and atomics,
//! and the arena carves leaked chunks with `split_at_mut`.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

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
    #[inline]
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

/// Number of shards per [`Table`] (each with its own index and writer
/// lock).
const SHARDS: usize = 16;
/// log2 of the number of buckets of the first index generation.
const MIN_BUCKETS_BITS: u32 = 4;
/// Number of index generations; the last one has `2^33` buckets, more than
/// `u32::MAX` entries need at the maximum load factor of 3/4.
const GENERATIONS: usize = 30;

/// Number of buckets of index generation `generation`, if addressable.
fn bucket_count(generation: usize) -> Option<usize> {
    let bits = MIN_BUCKETS_BITS.checked_add(u32::try_from(generation).ok()?)?;
    1usize.checked_shl(bits)
}

/// Result of probing an index for a string.
enum Probe {
    /// The string is entry `pos`.
    Found(u32),
    /// The string is not in the index.
    Vacant,
}

/// Mutable state of a shard, protected by its writer lock.
struct Writer {
    /// Number of entries in this shard's index.
    len: usize,
    /// Storage for the text of the entries inserted through this shard.
    arena: Arena,
}

/// One shard of a [`Table`]: a lock free readable hash index plus the
/// writer state.
///
/// The index is an open addressing (linear probing) array of `AtomicU64`
/// buckets, each `0` (empty) or `tag << 32 | (entry number + 1)` where
/// `tag` is the upper half of the hash. When it gets 3/4 full, the writer
/// builds the next, twice as large, *generation* and publishes it by
/// storing its number in `current`; older generations are kept (readers may
/// still be probing them) until the table is dropped, which at most doubles
/// the memory of the index.
///
/// Readers load `current` and probe without any lock or atomic read modify
/// write. A reader probing an old generation may miss a very recent entry;
/// callers that must not miss one ([`Table::intern`]) repeat the probe
/// under the writer lock.
struct Shard {
    generations: [OnceLock<Box<[AtomicU64]>>; GENERATIONS],
    current: AtomicUsize,
    writer: Mutex<Writer>,
}

impl Shard {
    const fn new() -> Self {
        Shard {
            generations: [const { OnceLock::new() }; GENERATIONS],
            current: AtomicUsize::new(0),
            writer: Mutex::new(Writer {
                len: 0,
                arena: Arena::new(),
            }),
        }
    }

    /// The current index generation (`None` before the first insertion).
    #[inline]
    fn buckets(&self) -> Option<&[AtomicU64]> {
        let current = self.current.load(Ordering::Acquire);
        self.generations.get(current)?.get().map(|b| &**b)
    }

    /// Allocates generation `generation` if it is addressable.
    fn allocate(&self, generation: usize) -> Option<&[AtomicU64]> {
        let size = bucket_count(generation)?;
        let cell = self.generations.get(generation)?;
        Some(cell.get_or_init(|| (0..size).map(|_| AtomicU64::new(0)).collect()))
    }

    /// Returns an index with room for one more entry, growing it if needed
    /// (called with the writer lock held). `None` if it cannot grow.
    fn room_for_one_more(&self, len: usize) -> Option<&[AtomicU64]> {
        let current = self.current.load(Ordering::Relaxed);
        let Some(buckets) = self.buckets() else {
            return self.allocate(current);
        };
        if (len + 1) * 4 <= buckets.len() * 3 {
            return Some(buckets);
        }
        let Some(next) = self.allocate(current + 1) else {
            // Cannot grow any more: keep at least one empty bucket so that
            // probes terminate.
            return (len + 1 < buckets.len()).then_some(buckets);
        };
        for bucket in buckets {
            let entry = bucket.load(Ordering::Relaxed);
            if entry != 0 {
                next[vacant_bucket(next, (entry >> 32) as u32)].store(entry, Ordering::Relaxed);
            }
        }
        // Publishes the fully built generation to readers.
        self.current.store(current + 1, Ordering::Release);
        Some(next)
    }
}

/// First empty bucket of the probe sequence of `tag` (the index must have
/// an empty bucket).
fn vacant_bucket(buckets: &[AtomicU64], tag: u32) -> usize {
    let mask = buckets.len() - 1;
    let mut i = tag as usize & mask;
    while buckets[i].load(Ordering::Relaxed) != 0 {
        i = (i + 1) & mask;
    }
    i
}

/// `a == b`, inlined for short strings.
///
/// Level texts are mostly 5 to 30 bytes long; comparing them a word at a
/// time (the last word overlapping) is several times cheaper than the
/// `memcmp` call behind `==` on slices.
#[inline]
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    /// Longest strings compared here; longer ones use `==`.
    const MAX_INLINE: usize = 32;
    let word = |w: &[u8; 8]| u64::from_ne_bytes(*w);
    if a.len() != b.len() {
        return false;
    }
    match (a.last_chunk::<8>(), b.last_chunk::<8>()) {
        (Some(last_a), Some(last_b)) if a.len() <= MAX_INLINE => {
            word(last_a) == word(last_b)
                && a.as_chunks::<8>()
                    .0
                    .iter()
                    .zip(b.as_chunks::<8>().0)
                    .all(|(x, y)| word(x) == word(y))
        }
        (Some(_), Some(_)) => a == b,
        _ => match (a.first_chunk::<4>(), b.first_chunk::<4>()) {
            // 4 to 7 bytes: first and last four bytes (overlapping).
            (Some(first_a), Some(first_b)) => {
                first_a == first_b && a.last_chunk::<4>() == b.last_chunk::<4>()
            }
            _ => a.iter().zip(b).all(|(x, y)| x == y),
        },
    }
}

/// One interning table: dense entry numbers `0..limit` for distinct
/// strings.
///
/// Lookups ([`Table::find`], and [`Table::intern`] for known strings) take
/// no lock. Insertions take the writer lock of one shard (chosen from the
/// hash). Entry numbers come from a shared atomic counter so they stay
/// dense across shards. Reading the text of an entry ([`Table::text`])
/// takes no lock either.
pub(crate) struct Table {
    limit: u32,
    next: AtomicU32,
    slots: Slots,
    shards: [Shard; SHARDS],
}

impl Table {
    /// Creates an empty table that will hold at most `limit` entries.
    pub(crate) const fn new(limit: u32) -> Self {
        Table {
            limit,
            next: AtomicU32::new(0),
            slots: Slots::new(),
            shards: [const { Shard::new() }; SHARDS],
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
                let index: usize = shard
                    .generations
                    .iter()
                    .filter_map(OnceLock::get)
                    .map(|buckets| buckets.len() * size_of::<AtomicU64>())
                    .sum();
                let writer = shard.writer.lock().unwrap_or_else(PoisonError::into_inner);
                index + writer.arena.allocated()
            })
            .sum();
        self.slots.allocated() + shards
    }

    #[inline]
    fn shard(&self, hash: u64) -> &Shard {
        // The low bits pick the shard, the high half is the bucket tag.
        &self.shards[(hash as usize) & (SHARDS - 1)]
    }

    /// Probes `buckets` for `s` with tag `tag`.
    #[inline]
    fn probe(&self, buckets: &[AtomicU64], tag: u32, s: &[u8]) -> Probe {
        let mask = buckets.len() - 1;
        let mut i = tag as usize & mask;
        loop {
            let entry = buckets[i].load(Ordering::Acquire);
            if entry == 0 {
                return Probe::Vacant;
            }
            if (entry >> 32) as u32 == tag {
                let pos = (entry as u32).wrapping_sub(1);
                if self
                    .slots
                    .get(pos)
                    .is_some_and(|text| bytes_eq(text.as_bytes(), s))
                {
                    return Probe::Found(pos);
                }
            }
            i = (i + 1) & mask;
        }
    }

    /// Looks `s` (whose hash under the interner's hasher is `hash`) up
    /// without inserting it. Takes no lock.
    ///
    /// `s` is a byte string: finding it means that it equals interned text,
    /// which is valid UTF-8, so callers need not validate it first.
    #[inline]
    pub(crate) fn find(&self, hash: u64, s: &[u8]) -> Option<u32> {
        let buckets = self.shard(hash).buckets()?;
        match self.probe(buckets, (hash >> 32) as u32, s) {
            Probe::Found(pos) => Some(pos),
            Probe::Vacant => None,
        }
    }

    /// Returns the entry number of `s`, inserting it if needed. Returns
    /// `None` only when `s` is not present and the table is full; since a
    /// full table stays full, the answer for `s` never changes afterwards.
    pub(crate) fn intern(&self, hash: u64, s: &str) -> Option<u32> {
        if let Some(pos) = self.find(hash, s.as_bytes()) {
            return Some(pos);
        }
        let tag = (hash >> 32) as u32;
        let shard = self.shard(hash);
        let mut writer = shard.writer.lock().unwrap_or_else(PoisonError::into_inner);
        // Only writers holding this lock change the index, so this probe of
        // the current generation is authoritative.
        if let Some(buckets) = shard.buckets() {
            if let Probe::Found(pos) = self.probe(buckets, tag, s.as_bytes()) {
                return Some(pos);
            }
        }
        let buckets = shard.room_for_one_more(writer.len)?;
        let pos = self.reserve()?;
        self.slots.set(pos, writer.arena.alloc(s));
        let entry = (u64::from(tag) << 32) | u64::from(pos + 1);
        // Publishes the entry (its slot was set above) to readers.
        buckets[vacant_bucket(buckets, tag)].store(entry, Ordering::Release);
        writer.len += 1;
        Some(pos)
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
    use std::hash::BuildHasher;

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
    fn bytes_eq_matches_slice_eq() {
        for len in 0..=40usize {
            let a: Vec<u8> = (0..len).map(|i| b'A' + (i % 26) as u8).collect();
            assert!(bytes_eq(&a, &a.clone()), "{len}");
            for at in 0..len {
                let mut b = a.clone();
                b[at] ^= 0x80;
                assert!(!bytes_eq(&a, &b), "{len} {at}");
                assert!(!bytes_eq(&b, &a), "{len} {at}");
            }
            for other in 0..=40usize {
                let b: Vec<u8> = (0..other).map(|i| b'A' + (i % 26) as u8).collect();
                assert_eq!(bytes_eq(&a, &b), len == other, "{len} {other}");
            }
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
        let intern = |s: &str| table.intern(hasher.hash_one(s), s);
        assert_eq!(intern("a"), Some(0));
        assert_eq!(intern("b"), Some(1));
        assert_eq!(intern("a"), Some(0));
        assert_eq!(intern(""), Some(2));
        assert_eq!(intern("c"), None);
        assert_eq!(intern("c"), None);
        assert_eq!(intern("b"), Some(1));
        assert_eq!(table.len(), 3);
        assert_eq!(table.find(hasher.hash_one("c"), b"c"), None);
        assert_eq!(table.find(hasher.hash_one(""), b""), Some(2));
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
            let pos = table.intern(hasher.hash_one(name.as_str()), name);
            assert_eq!(pos, Some(i as u32));
        }
        for (i, name) in names.iter().enumerate() {
            assert_eq!(
                table.find(hasher.hash_one(name.as_str()), name.as_bytes()),
                Some(i as u32)
            );
            assert_eq!(table.text(i as u32), Some(name.as_str()));
        }
    }
}

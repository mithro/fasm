# Design: `fasm::idstring`

Status: implemented in task T1.1 (`rust/fasm/src/idstring/`); intern hit
path sped up, `get` renamed to `lookup` and `intern_bytes` added in T1.1b.

## Purpose

A FASM file is mostly a long list of dotted feature names such as

```
CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT
INT_L_X10Y146.SW6BEG0.WW2END0
```

A routed design for a large part produces millions of these lines, and the
downstream tools (merge/sort, the frame assembler) key everything on the
feature name. Storing every name as a `String` costs 24 bytes of header plus a
heap block (about 32 to 48 bytes with allocator overhead for a 31 byte name)
per feature, makes every hash and comparison a string operation and does one
heap allocation per feature.

`IdString` follows the idea of <https://github.com/mithro/idstring>: the name
is split on `.` into a few *levels*, each level's text is interned in a
per-level table, and the handle itself is a single 8 byte integer made of
the per-level table indexes. The component texts (tile names, site names, pip
names, ...) are shared by a huge number of features, so the tables stay small
while the per-feature cost is exactly 8 bytes, equality and hashing are
integer operations and interning an already known name allocates nothing.

## Measurements that drove the design

The complete feature space of the largest 7 series Artix part in
prjxray-db (`xc7a200t`: every tile of `tilegrid.json` crossed with every tag
of the matching `segbits_*.db` / `ppips_*.db`) was generated with a small
script and analysed. It is an upper bound for any single design on that part.

| quantity                                   | value        |
|--------------------------------------------|--------------|
| distinct feature names                     | 90,246,253   |
| average name length                        | 30.9 bytes   |
| names with 2 / 3 / 4 / 5 / 6 components    | 3,770 / 87,162,717 / 2,978,666 / 88,620 / 12,480 |

Distinct texts per level for different splitting strategies (last level holds
the remainder, which may contain dots):

| strategy                          | level 0 | level 1 | level 2 | level 3 |
|-----------------------------------|--------:|--------:|--------:|--------:|
| 3 levels (`a`, `b`, `rest`)        | 46,611  | 6,888   | 7,790   | -       |
| 4 levels (`a`, `b`, `c`, `rest`)   | 46,611  | 6,888   | 7,344   | 193     |
| 4 levels idstring style (`a`, `b`, `middle`, `last`) | 46,611 | 6,888 | 7,393 | 169 |

Average component length per level (3 level split): 14.8, 20.8 and 17.8
bytes. Share of distinct components that are at most 7 bytes long (the
size that fits inline in a tagged pointer): 0 %, 3 % and 3 %.

Conclusions:

* **Level 0 (the tile name) is by far the largest table.** A 7 series 200T
  already uses 46,611 of the 65,535 entries a `u16` index can address.
  UltraScale+ parts are much larger (a VU9P has roughly 150,000 CLE tiles
  alone), so the idstring default of four `u16` levels would overflow at
  level 0 on real designs. Level 0 needs more than 16 bits.
* **A fourth level buys nothing on real data**: 96.6 % of names have exactly
  three components and splitting the remainder further saves 253 entries out
  of ~61,000. Each extra level costs one more hash lookup per intern and one
  more string piece per resolve.
* **Inline small string storage does not pay off for FASM components**: only
  about 3 % of the distinct components would fit in 7 bytes. What the tagged
  pointer mainly avoids in idstring (one `strndup` per string) is achieved
  more simply with a bump arena.

## Layout of the 8 byte handle

`IdString` wraps a `NonZeroU64` so that `Option<IdString>` is also 8 bytes.
Three levels are used with uneven index widths:

```
 63            40 39           20 19            0
+----------------+---------------+---------------+
|  level 0 (24)  |  level 1 (20) |  level 2 (20) |   hierarchical form
+----------------+---------------+---------------+

+----------------+-------------------------------+
|   0xFF_FFFF    |    overflow table index (40)  |   overflow form
+----------------+-------------------------------+
```

* Level 0 holds the first component, level 1 the second one and level 2
  **the whole remainder** (`BLUT.INIT` in the example above, dots included).
* Index `0` means "level absent" (the name has fewer components). Present
  components, *including empty ones*, always get an index `>= 1`, so `a`,
  `a.` and `a..` are three different handles. Level 0 is always present (the
  empty string is one empty component), so the hierarchical form is never
  zero.
* Level 0 index `0xFF_FFFF` is reserved as the tag of the overflow form, so
  level 0 has 16,777,214 usable entries and levels 1 and 2 have 1,048,575
  each: 360 times the measured 7 series need at level 0, 150 times at levels
  1 and 2.
* The overflow form stores an index into a single flat table of whole
  strings (see below). Its top 24 bits are all ones, so it is never zero
  either.

Because every string maps to exactly one handle (see *Canonical form*),
`Eq` and `Hash` are derived on the integer.

The entry numbers, and therefore the handle values, are handed out in
first-come order. The same string gets a different value in another run or
with another thread schedule, so raw values, `Hash` output and the
iteration order of a `HashMap<IdString, _>` are not deterministic across
runs. Reproducible output must be ordered with `Ord` (string order, see
*Ordering*); the `IdString` and `Interner` docs say so.

## Splitting rule and round trip

`s` is split at its first and second `.` (if any):

| input           | level 0      | level 1 | level 2      |
|-----------------|--------------|---------|--------------|
| `""`            | `""`         | absent  | absent       |
| `"A"`           | `"A"`        | absent  | absent       |
| `"A.B"`         | `"A"`        | `"B"`   | absent       |
| `"A.B.C.D"`     | `"A"`        | `"B"`   | `"C.D"`      |
| `".A."`         | `""`         | `"A"`   | `""`         |
| `"A..B"`        | `"A"`        | `""`    | `"B"`        |

Resolving joins the present levels with `.`, so every `&str` (empty,
leading/trailing/double dots, thousands of components, any Unicode) round
trips exactly. Splitting is on the byte `.` which never occurs inside a
multi-byte UTF-8 sequence.

## Interner

```
Interner
 +-- hasher: OnceLock<foldhash::fast::RandomState>   (per interner seed)
 +-- levels[0..3]: Table   (limits 2^24-2, 2^20-1, 2^20-1)
 +-- overflow:     Table   (limit u32::MAX, whole strings)

Table
 +-- next:   AtomicU32                              reserved entry count
 +-- slots:  [OnceLock<Box<[OnceLock<&'static str>]>>; 27]
 |           append only, segment k holds 64 << k entries, never moved
 +-- shards: [Shard; 16]              shard = low 4 bits of the hash
      Shard
       +-- generations: [OnceLock<Box<[AtomicU64]>>; 30]
       |                open addressing index, generation g has 16 << g
       |                buckets; bucket = 0 or (hash >> 32) << 32 | entry+1
       +-- current:     AtomicUsize         generation readers probe
       +-- writer:      Mutex<{ len, arena }>
                        arena: leaked byte chunks (1 KiB doubling up to
                        64 KiB) holding the text; strings over 4 KiB get
                        their own leaked allocation
```

**Intern** (`Interner::intern`) has a lock free hit path and an out of line
insertion path.

* *Hit path* (`find_levels`, also used by `lookup` and `intern_bytes`):
  split the string as bytes into its levels (a word at a time search for
  `.`; the last partial word is read as an overlapping word instead of a
  byte loop), then for each level hash the piece once and probe the shard
  index *without any lock*: load `current`, walk the linear probe sequence
  from `tag & mask`, and for a bucket whose 32 bit tag matches read the
  candidate's text through `slots` and compare (an inlined word compare,
  cheaper than a `memcmp` call for 5 to 30 byte texts). If every level is
  found, the handle is returned without allocating or writing anything.
* *Insertion path* (`intern_missing`, `#[cold]`, only when some level was
  not found): first a lock free probe of the overflow table (see *Overflow*),
  then for each level the probe is repeated and, if the piece is still
  missing, the shard's writer mutex is taken, the probe is repeated on the
  current generation (authoritative, only lock holders change it), the index
  is grown if it would exceed 3/4 load, an entry number is reserved from
  `next` (a compare and swap loop that refuses to go past the limit), the
  text is copied into the shard arena, the slot is published with
  `OnceLock::set` and finally the bucket is written with `Release`
  ordering.

The hit path works on bytes, so **`intern_bytes(&[u8])` (and
`IdString::from_bytes`) validate UTF-8 only on the insertion path**: a hit
means every level equals interned text, which is valid UTF-8, and the
levels joined by `.` are valid UTF-8 too (a `.` never splits a multi-byte
sequence, and a piece holding half of one can never equal interned text).
A parser can therefore intern names straight from its input buffer at the
cost of `intern(&str)`.

**Growing the index** builds generation `g + 1` (twice as large) from
generation `g` under the writer lock and publishes it with a `Release` store
of `current`. Old generations are never modified again and are kept until the
interner is dropped because readers may still be probing them; this at most
doubles the index memory. A reader that probes an old generation can miss an
entry added a moment ago; `intern` then finds it under the lock, and
`lookup` simply reports it as not yet present (there is no happens-before
relation with that insertion anyway). Once the interning call happens-before
the reader's lookup (thread join, a lock, a `Release` store read with
`Acquire`), the reader's `Acquire` load of `current` sees the generation the
entry was written to or a later one (a later one is built under the lock
from a copy that includes it), so it finds the entry; the test
`published_names_are_found_while_tables_grow` checks exactly this while all
tables and their generations grow.

**Resolve**: `slots` is an append only segmented array whose segments never
move and whose entries are written exactly once, so reading an entry is two
`Acquire` loads (segment pointer, slot) and takes no lock. All texts live in
leaked memory, so the resolved pieces are `&'static str`.

**Sharding**: 16 shards per table let several parser threads insert
concurrently; entry numbers are still dense per table because they come from
the shared atomic counter.

**Hashing**: `foldhash` (a small crate with no dependencies, the default
hasher of `hashbrown`) is several times faster than `SipHash` on 10 to 30
byte keys. A per interner random seed keeps adversarial FASM input from
forcing collisions. The seed only affects the index layout, never the handles
(entry numbers are allocation order), so ids are deterministic for a given
insertion order. Each piece is hashed with a single `Hasher::write` of its
bytes (`hash_one(&str)` also writes a terminator byte, which costs a second
multiplication); the insertion path hashes the same bytes, so both paths
agree.

**Thread local cache: considered and rejected.** A per thread, per level,
direct mapped cache of recently found texts (64 entries of `(interner id,
&'static str, entry number)` per level, 6 KiB per thread) was prototyped to
skip the shared index when consecutive lines share their tile or pip names.
It executed about the same number of instructions as the plain probe
(callgrind: 687 M vs 703 M for the same run) but was 50 % *slower* (29 to
45 ns per hit on names with small tables, 48 to 75 ns on pip heavy names):
whether a level hits the cache is data dependent, so the branch mispredicts
cost more than the probe it saves, and the probe of a small table is
already in L1. It would only pay off on input with very long runs of equal
components, and it adds a thread local access (a function call in a
`cdylib` such as the Python extension).

**Why not `RwLock` + `hashbrown::HashTable<u32>`**: that was the first
implementation (see the git history). Profiling showed that the two atomic
read-modify-write operations of every read lock/unlock (one pair per level)
were a large part of an intern hit and made 8 concurrent threads 3 times
slower than the lock free index. The custom index costs more memory per
entry (8 byte buckets at at most 3/4 load, plus old generations) but the
tables are tiny compared to the 8 bytes per feature.

**No `unsafe`**: the arena hands out `&'static str` by splitting a leaked
`&'static mut [u8]` chunk (`split_at_mut`) and validating the copied bytes
with `str::from_utf8`; publication uses `OnceLock` and atomics. The cost
compared to a hand written tagged pointer table is about 16 bytes per
distinct component (a `OnceLock<&str>` slot is 24 bytes instead of 8), i.e.
about 1 MB for the full 200T feature space.

**Lifetime**: interned text is never freed. This is what makes `&'static str`
access sound and resolving lock free. A private `Interner` frees its index
structures on drop but leaks its text; private interners are meant for tests
and short lived tools. The process wide interner is `static GLOBAL:
Interner`.

## Overflow / fallback

When a component is missing from a level table and that table is full, the
whole string is interned in the flat *overflow* table and the overflow form is
returned. Nothing panics and nothing is lost; overflowed names simply do not
share storage with other names.

**Canonical form.** Every string has exactly one handle, which is what makes
integer `Eq`/`Hash` correct:

* A table only ever grows and a full table stays full forever.
* Inserting a component (or deciding that it is missing from a full table) is
  serialised by the writer lock of that component's shard.
* So a string goes to the overflow table only if one of its components is
  missing from a full table, and that component can never be added later:
  the string can never become hierarchical. Conversely, once all components of
  a string are present, every later intern finds them all.
* `lookup` (without interning) checks the level tables first and the
  overflow table only if some component is missing. A hit in either is the
  canonical handle: all components present means the string was never
  overflowed, and an overflowed string can never become hierarchical.
* For the same reason, interning an already overflowed string again needs
  no lock: when the lock free level lookup misses, `intern` probes the
  overflow table (lock free) before taking any shard lock and returns the
  overflow handle on a hit. A miss there (not overflowed, or inserted
  concurrently and not visible yet) takes the locked path, which decides
  authoritatively. The probe is skipped while the overflow table is empty.
  A test holds every writer lock of an interner and checks that known
  hierarchical and overflowed names are still interned from another
  thread.

Components inserted into earlier levels before a later level turned out to be
full stay in their tables (harmless, they may be shared by later names).

The overflow table itself is limited to `u32::MAX` entries (the layout
reserves 40 bits for a future increase). Reaching it would require more than
4 billion distinct overflowed names, i.e. well over 100 GiB of text, and is
treated like an allocation failure (panic with an explicit message). A shard
index that cannot grow any more (2^33 buckets) is treated like a full table.

`Interner::with_level_limit(n)` builds an interner whose level tables hold at
most `n` entries each (the handle layout is unchanged); the tests use it to
exercise the overflow path with a handful of strings.

## Ordering

`Ord` compares the full strings byte by byte (the same order as `str`), which
requires table lookups:

1. equal handles are `Equal` (no lookup);
2. for two hierarchical handles, leading levels with equal indexes are equal
   texts and are skipped without a lookup; if one side has no more levels it
   is a prefix of the other and sorts first (no lookup);
3. otherwise the two texts of the first differing level are read and
   compared; unless one is a prefix of the other they decide the order;
4. in the remaining cases (and for overflowed handles) the rest of both
   strings is compared chunk by chunk with `memcmp`, without allocating.

`Ord` for private interners is `Interner::cmp`.

## Public API

```rust
pub struct IdString(NonZeroU64);        // Copy, Eq, Hash, Ord, Send, Sync

impl IdString {
    pub fn new(s: &str) -> IdString;                           // interns
    pub fn from_bytes(b: &[u8]) -> Result<IdString, Utf8Error>; // intern_bytes
    pub fn lookup(s: &str) -> Option<IdString>;                // no interning
    pub fn resolve(self) -> String;
    pub fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R;
    pub fn resolved(self) -> Resolved;
    pub fn components(self) -> impl Iterator<Item = &'static str>;
    pub fn first_component(self) -> &'static str;
    pub fn starts_with_component(self, prefix: &str) -> bool;
    pub fn len(self) -> usize;
    pub fn is_empty(self) -> bool;
}
// Display, Debug (IdString("A.B")), From<&str>, FromStr (Infallible),
// PartialEq<str>, PartialEq<&str> (both directions), Ord/PartialOrd by
// string value.

pub struct Interner { .. }            // Default, Debug (entry counts)
impl Interner {
    pub const fn new() -> Interner;
    pub const fn with_level_limit(limit: u32) -> Interner;
    pub fn intern(&self, s: &str) -> IdString;
    pub fn intern_bytes(&self, b: &[u8]) -> Result<IdString, Utf8Error>;
    pub fn lookup(&self, s: &str) -> Option<IdString>;
    pub fn resolved(&self, id: IdString) -> Resolved;
    pub fn resolve(&self, id: IdString) -> String;
    pub fn with_str<R>(&self, id: IdString, f: impl FnOnce(&str) -> R) -> R;
    pub fn cmp(&self, a: IdString, b: IdString) -> Ordering;
    pub fn stats(&self) -> InternerStats;
}
pub static GLOBAL: Interner;

#[non_exhaustive]
pub struct InternerStats {
    pub level_entries: [usize; 3],
    pub overflow_entries: usize,
    pub heap_bytes: usize,
}

/// The resolved pieces of a handle (up to three `&'static str` joined by
/// '.'); all string operations are implemented here once.
pub struct Resolved { .. }   // Copy; Display, Debug, Eq, Ord, PartialEq<str>
impl Resolved {
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn components(&self) -> impl Iterator<Item = &'static str>;
    pub fn first_component(&self) -> &'static str;
    pub fn starts_with_component(&self, prefix: &str) -> bool;
    pub fn with_str<R>(&self, f: impl FnOnce(&str) -> R) -> R;
    pub fn into_string(self) -> String;
}
```

* `lookup(s)` returns the handle `s` *would* have if it can be produced
  without adding anything to the tables. That is the case for every string
  interned before, but also for a string never interned whole whose levels
  are all known (after `A.B.C` and `X.Y`, `lookup("A.Y")` is `Some`). It is
  a cheap lookup, **not a set membership test** (it was called `get` before
  T1.1b; renamed because `get` suggested one).
* `intern_bytes(b)` / `IdString::from_bytes(b)` equal
  `intern(str::from_utf8(b)?)` but validate UTF-8 only for names that are
  not known yet (see *Interner*).
* `with_str` passes a single piece handle (one component, or overflowed)
  directly; other names up to 1024 bytes are joined in a reused per thread
  `String`, so there is no heap allocation in steady state. Longer names and
  calls nested inside the callback join into a new `String`. `Display`
  writes the pieces directly (and pads through `with_str` when a width or
  precision is given).
* `components()` yields every `.` separated component (the remainder level is
  split too), independent of the internal representation, so an overflowed
  handle behaves exactly like a hierarchical one.
* `starts_with_component(p)` is true when the name equals `p` or starts with
  `p` followed by `.` (a prefix aligned on component boundaries).
* `stats()` reports entries per table and the exact heap bytes the tables
  allocated (text, slots, index generations); used by the benchmark.
* All methods on `IdString` use `GLOBAL`, and so do its `Display`, `Debug`,
  `Ord`/`PartialOrd` and `PartialEq<str>` impls (documented under
  `# Panics` on each). A handle created by a private `Interner` must only be
  used through that interner's methods; using it with another interner
  either panics (unknown entry) or yields another string. Handles do not
  record their interner to keep them 8 bytes.
* No allocation once names are known: interning them again (by `&str` or
  bytes), `lookup`, `with_str`, `Display`/`Debug` (with or without padding),
  `PartialEq<str>`, `Ord` and `components` allocate nothing. Checked with a
  counting global allocator in `rust/fasm/tests/idstring_alloc.rs`.

## Trade offs and alternatives considered

* **Four `u16` levels (idstring default)**: overflows at level 0 on large
  parts and the fourth level does not reduce memory on real data (see the
  measurements).
* **Tagged pointer entries with inline small strings**: would save about 16
  bytes per distinct component but needs `unsafe` pointer tagging and helps
  almost no FASM component (they are longer than 7 bytes).
* **One global table of whole names**: no sharing, 30+ bytes of text per
  distinct feature plus table overhead; kept only as the overflow path.
* **`RwLock<HashMap<String, u32>>` + `Vec<String>`**: resolve would need a
  lock, and each entry costs two heap allocations.
* **Freeing strings**: would need reference counting or an epoch scheme and
  would make resolve slower; FASM tools are batch processes, so leaking the
  (small) component tables is the right trade.
* **Interning cost**: an intern hit does three hash lookups, so it costs
  about 1.4 times a single whole-string `HashMap` lookup when everything is
  in cache, and less than half of one when the whole-string map would not
  fit in cache (see below). The design optimises memory and handle
  operations (`Eq`/`Hash`/copy) rather than the one-time intern.

## Benchmarks

Machine: 4 vCPU Intel Xeon @ 2.10 GHz (cloud VM, shared with other jobs),
Rust 1.94.1, release profile. `cargo bench -p fasm --bench idstring`
(harness-less, best of 5 rounds; numbers vary by about 10 % between runs,
the 8 thread figure by more). "T1.1" is the original implementation, "T1.1b"
the current one; both binaries were run alternately in the same session and
the table shows typical values of three runs.

**Default input**: the 3 distinct feature names of `examples/many.fasm`, each
repeated over a 150 x 150 tile grid (`INT_L_X{x}Y{y}.SW6BEG0.WW2END0`, ...):
67,500 distinct names, 32.5 bytes on average, 67,500 distinct tile names.

| operation                                   | T1.1 ns/op | T1.1b ns/op |
|---------------------------------------------|-----------:|------------:|
| intern, miss (new name, new tile)           | 178        | 181         |
| intern, hit                                 | 82         | 53          |
| intern, hit, 8 threads (wall clock / ops)   | 35-52      | 25-34       |
| `intern_bytes`, hit                         | -          | 53          |
| `from_utf8` + `intern`, hit                 | -          | 59          |
| `lookup` (`get` in T1.1), hit               | 80         | 52          |
| `with_str`                                  | 20         | 20          |
| `resolve` (new `String`)                    | 29         | 28          |
| sort `Vec<IdString>` (per element)          | 170        | 170         |
| sort `Vec<String>` (per element)            | 66         | 66          |
| `HashMap<String, u32>::get` (baseline)      | 38         | 38          |

The hit path is about 1.55 times faster. The miss path is a few percent
slower: it now probes the missing level once more (lock free, in
`find_levels`) before the insertion path repeats the probe and takes the
lock. Instruction counts (callgrind) of a hit went from about 710 to 520.

**Pip heavy names** (scratch program, 200,000 names shaped like the parser
benchmark's `pips` input, `INT_L_X{x}Y{y}.{dst}.{src}`, 15,251 tiles):
intern hit 75 -> 48 ns, `from_bytes` hit 83 -> 52 ns (it no longer runs a
separate UTF-8 pass).

Interner heap: 6.28 MB = 93 bytes per distinct table entry (here every name
has its own tile, so also per name). For the 67,500 entry level 0 table this
splits into slots 46.6 bytes (24 byte `OnceLock<&str>`, segment 10 just
started so half of it is unused), index 31 bytes (8 byte buckets, current plus
old generations) and text plus arena slack about 15 bytes. T1.1b changed no
data structure: the heap use is byte for byte the same.

**prjxray sample**: `FASM_IDSTRING_BENCH_FILE` with 2,000,000 features drawn
at random from the full `xc7a200t` feature space (38,924 / 1,842 / 2,104
distinct level entries), T1.1 -> T1.1b: intern miss 106 -> 78 ns (most
levels of a new name are known), hit 95 -> 68 ns, 8 threads 34-44 -> 27-30
ns, `lookup` 93 -> 68 ns, `intern_bytes` hit 68 ns (`from_utf8` + `intern`
83 ns); unchanged: `with_str` 34 ns, `resolve` 41 ns, sort 260 ns/element
vs 197 for `String`, and `HashMap<String, u32>::get` 175 ns (its 2M entries
do not fit in cache).
Interner heap 4.0 MB = **2.0 bytes per distinct name** (plus the 8 byte
handle), versus 56 bytes for a `String` (24 byte header + text, before
allocator overhead).

**Full feature space** (scratch program, not in the repository): interning
all 90,246,253 names of the `xc7a200t` feature space (2.79 GB of text) into
`GLOBAL` while reading them from a file took 12.6 s (139 ns per name
including I/O). Level tables: 46,611 / 6,888 / 7,790 entries (exactly the
measured distinct counts, no overflow), interner heap 5.05 MB (82 bytes per
entry, 0.06 bytes per name), `Vec<IdString>` 721 MB, peak RSS 713 MB. The
same names as `Vec<String>` would need more than 5 GB. Sorting 10 million of
the handles took 2.5 s (252 ns per element). (Measured with T1.1.)

**Effect on the parser** (measured on a scratch copy of the T1.3 parser
branch with the T1.1b interner, 100 MB inputs, warm passes): `pips`
165-171 -> 178-190 MB/s, `mixed` 266-284 -> 284-299 MB/s. Interning went
from 38.5 % to 26 % of the parser's instructions (callgrind, all passes).
`str::from_utf8` is only about 1.5 % of the instructions of the current
parser branch, so switching it to `IdString::from_bytes` changes little
(within noise). The rest of the `pips` gap to 200 MB/s is in the parser
itself (about 1,300 instructions per line besides interning).

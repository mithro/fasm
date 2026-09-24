# Design: `fasm::idstring`

Status: implemented in task T1.1 (`rust/fasm/src/idstring/`).

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
 +-- overflow:     Table   (limit u32::MAX - 1, whole strings)

Table
 +-- next:   AtomicU32                              reserved entry count
 +-- slots:  [OnceLock<Box<[OnceLock<&'static str>]>>; 27]
 |           append only, segment k holds 64 << k entries, never moved
 +-- shards: [RwLock<Shard>; 16]      shard = bits 40..44 of the hash
      Shard
       +-- map:   hashbrown::HashTable<u32>   entry number, hashed by text
       +-- arena: leaked byte chunks (1 KiB doubling up to 64 KiB) holding
                  the component text; strings over 4 KiB get their own
                  leaked allocation
```

**Intern** (`Interner::intern`): for each level, hash the component once,
take the shard read lock and probe the `HashTable` (comparisons read the
candidate's text through `slots`, which needs no lock). On a hit the index is
returned without allocating. On a miss the shard write lock is taken, the
probe is repeated, an entry number is reserved from `next` (a compare and
swap loop that refuses to go past the limit), the text is copied into the
shard arena, the slot is published with `OnceLock::set` and the entry number
is inserted into the map.

**Resolve**: `slots` is an append only segmented array whose segments never
move and whose entries are written exactly once, so reading an entry is two
`Acquire` loads (segment pointer, slot) and takes no lock. All texts live in
leaked memory, so the resolved pieces are `&'static str`.

**Sharding**: 16 shards per table make concurrent interning from several
parser threads scale; entry numbers are still dense per table because they
come from the shared atomic counter.

**Hashing**: `foldhash` (the default hasher of `hashbrown`, a small crate
with no dependencies) is several times faster than `SipHash` on 10 to 30 byte
keys. A per interner random seed keeps adversarial FASM input from forcing
collisions. The seed only affects the hash table layout, never the handles
(entry numbers are allocation order), so ids stay deterministic for a given
insertion order.

**Why `hashbrown::HashTable`**: it stores just the `u32` entry number (4
bytes plus 1 control byte per bucket) and lets the equality and rehash
closures look the text up in `slots`; `std::collections::HashMap` would need
the key stored in the map (16 bytes for a `&str`) because the raw entry API is
not stable.

**No `unsafe`**: the arena hands out `&'static str` by splitting a leaked
`&'static mut [u8]` chunk (`split_at_mut`) and validating the copied bytes
with `str::from_utf8`; publication uses `OnceLock`. The only cost compared to
a hand written tagged pointer table is 16 extra bytes per distinct component
(a `OnceLock<&str>` slot is 24 bytes instead of 8), i.e. about 1 MB for the
full 200T feature space, which is negligible next to the 8 bytes per feature.

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
  serialised by the shard lock of that component.
* So a string goes to the overflow table only if one of its components is
  missing from a full table, and that component can never be added later:
  the string can never become hierarchical. Conversely, once all components of
  a string are present, every later intern finds them all.
* `get` (lookup without interning) checks the level tables first and the
  overflow table only if some component is missing.

Components inserted into earlier levels before a later level turned out to be
full stay in their tables (harmless, they may be shared by later names).

The overflow table itself is limited to `u32::MAX - 1` entries (the layout
reserves 40 bits for a future increase). Reaching it would require more than
4 billion distinct overflowed names, i.e. well over 100 GiB of text, and is
treated like an allocation failure (panic with an explicit message).

`Interner::with_level_limit(n)` builds an interner whose level tables hold at
most `n` entries each (the handle layout is unchanged); the tests use it to
exercise the overflow path with a handful of strings.

## Ordering

`Ord` compares the full strings byte by byte (the same order as `str`), which
requires table lookups:

1. equal handles are `Equal` (no lookup);
2. for two hierarchical handles, leading levels with equal indexes are equal
   texts and are skipped without a lookup;
3. the remaining pieces of both sides (joined by `.`) are compared chunk by
   chunk with `memcmp`, without allocating.

`Ord` for private interners is `Interner::cmp`.

## Public API

```rust
pub struct IdString(NonZeroU64);        // Copy, Eq, Hash, Ord, Send, Sync

impl IdString {
    pub fn new(s: &str) -> IdString;                           // interns
    pub fn from_bytes(b: &[u8]) -> Result<IdString, Utf8Error>;
    pub fn get(s: &str) -> Option<IdString>;                   // no interning
    pub fn resolve(self) -> String;
    pub fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R;  // no heap
                                   // allocation for names up to 256 bytes
    pub fn resolved(self) -> Resolved;
    pub fn components(self) -> impl Iterator<Item = &'static str>;
    pub fn first_component(self) -> &'static str;
    pub fn starts_with_component(self, prefix: &str) -> bool;
    pub fn len(self) -> usize;
    pub fn is_empty(self) -> bool;
}
// Display, Debug (IdString("A.B")), From<&str>, FromStr (Infallible),
// PartialEq<str>, PartialEq<&str>, Ord/PartialOrd by string value.

pub struct Interner { .. }
impl Interner {
    pub const fn new() -> Interner;
    pub const fn with_level_limit(limit: u32) -> Interner;
    pub fn intern(&self, s: &str) -> IdString;
    pub fn get(&self, s: &str) -> Option<IdString>;
    pub fn resolved(&self, id: IdString) -> Resolved;
    pub fn resolve(&self, id: IdString) -> String;
    pub fn with_str<R>(&self, id: IdString, f: impl FnOnce(&str) -> R) -> R;
    pub fn cmp(&self, a: IdString, b: IdString) -> Ordering;
}
pub static GLOBAL: Interner;

/// The resolved pieces of a handle (up to three `&'static str` joined by
/// '.'); all string operations are implemented here once.
pub struct Resolved { .. }   // Copy; Display, Debug, Ord, PartialEq<str>
impl Resolved {
    pub fn len(&self) -> usize;  pub fn is_empty(&self) -> bool;
    pub fn components(&self) -> impl Iterator<Item = &'static str>;
    pub fn first_component(&self) -> &'static str;
    pub fn starts_with_component(&self, prefix: &str) -> bool;
    pub fn with_str<R>(&self, f: impl FnOnce(&str) -> R) -> R;
    pub fn into_string(self) -> String;
}
```

* `components()` yields every `.` separated component (the remainder level is
  split too), independent of the internal representation, so an overflowed
  handle behaves exactly like a hierarchical one.
* `starts_with_component(p)` is true when the name equals `p` or starts with
  `p` followed by `.` (a prefix aligned on component boundaries).
* All methods on `IdString` use `GLOBAL`. A handle created by a private
  `Interner` must only be used through that interner's methods; using it with
  another interner either panics (index out of range) or yields another
  string. Handles do not record their interner to keep them 8 bytes.

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

## Benchmarks

See the numbers section added after implementation below.

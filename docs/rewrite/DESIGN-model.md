# Design notes: `fasm::model` (T1.2)

Short design record for `rust/fasm/src/model/`, the Rust equivalent of
Python's `fasm.model` namedtuples. See the module's own doc comments
(`rust/fasm/src/model/*.rs`) for the field-by-field API; this file only
records the decisions that are not obvious from the code.

## `FeatureValue` representation

Python's `SetFasmFeature.value` is an unbounded `int`. Real FASM files only
ever use 1 to 256 bit values (a BRAM `INIT` string is the widest, at 256
bits), but nothing in the grammar caps the width. `FeatureValue` stores:

* `Repr::Inline([u64; 4])`: little endian, always zero padded to exactly 4
  limbs (256 bits). Used for every value that fits, including `0`.
* `Repr::Heap(Box<[u64]>)`: used only above 256 bits; its length is always
  minimal (the top limb is never zero).

Every constructor and mutating operation funnels through
`FeatureValue::from_limb_vec`, which trims high zero limbs and picks the
representation. This keeps the representation of a given number canonical,
which is what makes `#[derive(PartialEq, Eq, Hash)]` correct: two values
compare/hash equal iff they hold the same number, independent of how they
were built. `Ord` is hand written (not derived) because the derived,
variant-then-field order does not match numeric order — `Inline`'s limbs are
stored least-significant-first, the opposite of what a naive
most-significant-first `Ord` derive would need, and comparing `Inline`
against `Heap` by field order is wrong (a small number can be forced into
`Heap` by history if `from_limb_vec` were bypassed, which the invariant
above rules out, but hand writing `Ord` avoids relying on that reasoning).

`size_of::<FeatureValue>()` is 40 bytes (measured; see
`model::feature_value::tests::size_of_feature_value`): a `Box<[u64]>` is a
16-byte fat pointer, `[u64; 4]` is 32 bytes, and the enum discriminant does
not fit in a niche of either variant, so the enum is `32 + 8` = 40 bytes
(padded for `u64` alignment). This meets the task's "at most 40 bytes"
target.

`num-bigint` is a **dev-dependency only** (proptest cross-checking in
`model::feature_value::tests::proptest_cross_check`); it is never linked
into non-test builds.

### Why no explicit "used limb count" field

`Inline` always physically stores 4 limbs; the *logical* length (used by
`bit_len`, `Ord`, formatting, …) is recomputed on demand by scanning from
the top with `FeatureValue::trimmed`. This avoids a separate length field
(and keeping it in sync) at the cost of an `O(limbs)` scan per query —
limbs is at most 4 for the overwhelmingly common case, so this is cheap in
practice and simpler to keep correct than caching a length.

## Address width: `u32`

`SetFasmFeature::start`/`end` are `u32`, not Python's unbounded `int`. The
reference ANTLR C++ parser already truncates `FeatureAddress` decimal
literals to 32 bits in practice, so this is not a new restriction beyond
what real toolchains produce; an address literal above `u32::MAX` will be a
parse error in the Rust parser (T1.3), to be recorded in
`docs/rewrite/COMPAT.md` once that module exists.

## `SetFasmFeature::new` vs `new_unchecked`

`new` reproduces the asserts scattered across
`fasm/__init__.py::set_feature_width`/`set_feature_to_str` and
`fasm/parser/textx.py::set_feature_model_to_tuple` as a `Result`:
`end` without `start`, `end < start`, a `FeatureAddress` range that does not
fit in a `u32` (`start == 0, end == u32::MAX`), and a value that needs more
bits than the address width allows are all rejected with a `ModelError`
instead of panicking or overflowing. `new_unchecked` skips all of that for
the parser's (T1.3) hot path, where the grammar and width checks will
already have been applied
while parsing; `SetFasmFeature::width()` (and anything else that assumes
the invariant) documents that it can panic if `new_unchecked` was used to
build an invalid value.

## `FasmLine` truthiness helpers

`is_blank`/`is_only_comment`/`is_only_annotation` mirror Python's
`fasm.output.is_blank_line`/`is_only_comment`/`is_only_annotation` exactly,
including Python's truthiness of the `annotations` and `comment` fields:

* `not line.annotations` is `True` for both `annotations = None` **and**
  `annotations = []` (an empty list is falsy in Python) — mirrored by
  `annotations.as_ref().is_none_or(Vec::is_empty)`.
* `not line.comment` is `True` for both `comment = None` **and**
  `comment = ""` (an empty string is falsy) — mirrored by
  `comment.as_deref().is_none_or(str::is_empty)`.

A bare `#` with nothing after it therefore parses to `comment: Some("")`,
which is **not** "only a comment" by these helpers (it is a blank line),
exactly matching the Python behaviour; see
`model::tests::bare_hash_comment_is_some_empty_string_and_counts_as_blank`.

A `{}` empty annotation block is not valid FASM syntax (the grammar in
`docs/specification/syntax.rst` requires `Annotations` to hold at least one
`Annotation`), so a parser never produces `annotations: Some(vec![])`; the
type still allows it so it is representable when building a `FasmLine`
programmatically (e.g. by the `output`/merge code in T1.4).

## Errors

`ValueParseError` (from `FeatureValue::from_digits`) and `ModelError` (from
`SetFasmFeature::new` and `ValueFormat::try_from`) are hand written enums
implementing `Display` + `std::error::Error`; `thiserror` was not added
since two small hand written `Display` impls do not justify a new
`[workspace.dependencies]` entry.

## Measured sizes

| Type              | `size_of` | Notes |
|-------------------|----------:|-------|
| `FeatureValue`    | 40 bytes  | target (task brief): ≤ 40 |
| `SetFasmFeature`  | 72 bytes  | dominated by `FeatureValue`'s 40 bytes |
| `FasmLine`        | 112 bytes | `Option<SetFasmFeature>` has no spare niche (unlike the `Vec`/`Box<str>` fields, whose pointers supply one), so it costs a full extra discriminant |
| `Annotation`      | 32 bytes  | two `Box<str>` fat pointers |

(All measured with `rustc 1.94.1` per `rust-toolchain.toml`; asserted by
`model::feature_value::tests::size_of_feature_value` and
`model::tests::size_of_set_fasm_feature`/`size_of_fasm_line`/
`size_of_annotation`, so a toolchain change that shifts these will fail
loudly rather than silently.)

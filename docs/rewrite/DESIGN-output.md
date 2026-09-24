# Design notes: `fasm::output` (T1.4)

Short design record for `rust/fasm/src/output/`, the Rust equivalent of
Python's `fasm/__init__.py` (`fasm_value_to_str`, `set_feature_width`,
`set_feature_to_str`, `canonical_features`, `fasm_line_to_string`,
`fasm_tuple_to_string`) and `fasm/output.py` (`merge_features`,
`MergeModel`, `merge_and_sort`). See the module's own doc comments for the
field-by-field API; this file only records decisions that are not obvious
from the code, plus one faithfully reproduced Python bug.

No changes were made to `idstring` or `model`; `output` only depends on
their existing public API (`SetFasmFeature::width`, `FeatureValue::bit`/
`is_zero`/`is_one`/`set_bit`/`fits_in_bits`, `IdString::first_component`/
`with_str`, `FasmLine::is_blank`/`is_only_comment`/`is_only_annotation`).

## `Result` instead of Python's `assert`

Every Python `assert` in the ported functions is only reachable when a
`SetFasmFeature` violates the invariants `SetFasmFeature::new` enforces
(end without start, value too wide for its address, etc.) — i.e. one built
with `SetFasmFeature::new_unchecked` from bad inputs. Rather than `panic!`
(which an `assert` in Rust would do, same as Python), every function that
carries one of these asserts returns `Result<_, OutputError>` instead:
`set_feature_to_str`/`write_set_feature`, `try_canonical_features`,
`fasm_line_to_string`, `fasm_tuple_to_string`, `merge_features`,
`MergeModel::merge_addresses`, `merge_and_sort`. A `SetFasmFeature` built
with `new` (or by the parser, T1.3) never triggers any of these — see
`OutputError`'s doc comments for the exact mapping from Python `assert` to
`OutputError` variant.

`canonical_features` (as opposed to `try_canonical_features`) is the one
exception: the task brief asked for it to return a plain
`impl Iterator<Item = SetFasmFeature>`, so it is a thin, `expect`-based
wrapper for the common case (a `SetFasmFeature` already known valid); see
its doc comment for why this is safe for anything the parser or
`SetFasmFeature::new` produces.

## `canonical_features` collects eagerly

The Python generator is lazy; `try_canonical_features` collects into a
`Vec` up front instead. Every real FASM feature is at most a few hundred
bits wide (256 for the widest known case, a BRAM `INIT`), so this is a
small, bounded allocation in the overwhelmingly common case. A
`SetFasmFeature` with a deliberately huge range (nothing in the grammar
caps `u32` addresses) would allocate and iterate proportionally to its
width either way — laziness would only change *when* that cost is paid,
not the total work, since the caller (`fasm_line_to_string`) always
exhausts the iterator. Given `output` is not the hot path called out in
`PLAN.md` (that is the parser and the `set_feature`/value formatting used
per-feature during frame assembly), the simpler `Vec`-backed
implementation was chosen over a hand written lazy iterator/state machine.

## `merge_and_sort` split into two functions

Python's `merge_and_sort(model, zero_function=None, sort_key=None)` takes
a dynamically typed `sort_key` callback returning anything comparable.
Rust needs a concrete `K: Ord` for the sort to type-check, and that `K`
cannot be inferred when `sort_key` is `None`. Rather than force every
caller to pick a `K` even when they don't need one, `output` exposes two
functions built on the same private generic engine:

* `merge_and_sort(model, zero_function)`: no custom sort key; group ids
  sort by plain `IdString`/string order, matching Python's
  `sorted(feature_groups.keys())` (no `sort_key`).
* `merge_and_sort_by_key::<K: Ord>(model, zero_function, sort_key)`:
  matches Python's `sort_key` argument.

`MergeModel::output_sorted_lines<K: Ord>` (the lower level, `pub` method
mirroring Python's `MergeModel.output_sorted_lines`) keeps the single
`Option<&dyn Fn(&str) -> K>` signature the task brief asked for; the split
only exists at the two convenience free functions.

## A faithfully reproduced `MergeModel` bug

Python's `MergeModel.add_to_comment_group` has this `else` branch (the
group ends because of a non-comment, non-annotation line):

```python
else:
    if not is_blank_line(line):
        self.current_group.append(line)

    self.groups.append(self.current_group)
    self.state = MergeModel.State.NoGroup
```

Unlike the equivalent branch in `add_to_annotation_group` (which sets
`self.current_group = None` right after appending it to `self.groups`),
this branch does **not** reset `self.current_group`. Because Python lists
are mutable, shared references, `self.current_group` still points at the
list that was just appended to `self.groups`. `start_comment_group` and
`start_annotation_group` only check `if self.current_group is not None`
before appending it (again) to `self.groups` — they have no way to tell
"already flushed" from "still open" — so if another comment or annotation
group starts before anything else reassigns `current_group`, that stale
group gets pushed into `self.groups` a **second** time, duplicating its
lines in the final output.

Verified directly against the oracle (`tests/oracle/venv/bin/python`):

```python
lines = [
    FasmLine(set_feature=None, annotations=None, comment=' a'),
    FasmLine(set_feature=SetFasmFeature(feature='X', start=None, end=None, value=1, value_format=None), annotations=None, comment=None),
    FasmLine(set_feature=None, annotations=None, comment=' b'),
]
list(fasm.output.merge_and_sort(lines))
```

renders as `# a`, `X`, `# a`, `X`, *(blank)*, `# b` — the `# a`/`X` group
is duplicated. Further oracle probing (see the task's scratchpad scripts)
confirmed:

* The same duplication happens whether the *next* group is a new comment
  group or a new annotation group (both `start_*` methods share the same
  `is not None` check).
* An annotation group ended the same way does **not** duplicate (its
  `else` branch does reset `current_group` to `None`).
* If nothing else starts a new comment/annotation group before the input
  ends, the stale group is simply dropped, not duplicated: the final flush
  in `merge_and_sort` only re-appends `current_group` when
  `merged_model.state != NoGroup`, and this branch already put the state
  back to `NoGroup`.
* A blank line between the stale group and the next trigger does not
  prevent the duplication (blank lines are simply discarded and never
  touch `current_group`).

Per the task instructions ("mirror EXACTLY the ORIGINAL Python functions"),
`MergeModel` reproduces this exactly, without needing Python's aliasing
semantics: `add_to_comment_group`'s equivalent branch pushes a *clone* of
`current_group` into `self.groups` and leaves `self.current_group` as
`Some(..)` (not reset), while `add_to_annotation_group`'s branch uses
`.take()` (clearing it), matching Python's asymmetry. `MergeModel`'s own
doc comment carries this same explanation, and
`rust/fasm/src/output/merge/tests.rs` has four tests exercising it
(duplication via a new comment group, via a new annotation group, its
absence when the *previous* group was an annotation group, and its absence
when nothing follows).

## Insertion order in `merge_addresses`/`output_sorted_lines`

Python dicts preserve insertion order, which two places in `MergeModel`
rely on for tie-breaking in a stable sort (`sorted(..., key=...)` when two
groups share a sort key): `MergeModel.merge_addresses`'s
`eligable_address_features` dict, and `output_sorted_lines`'s
`feature_groups[group_id]` lists. `merge_addresses` uses a
`Vec<(IdString, Vec<SetFasmFeature>)>` (linear lookup) instead of a
`HashMap` for `eligible_address_features` specifically to preserve this
insertion order faithfully; `output_sorted_lines` can use a plain
`HashMap<IdString, Vec<&[FasmLine]>>` for `feature_groups` since only each
key's own `Vec` order matters (preserved by `Vec::push`), not the
iteration order of the map's keys (those are always explicitly sorted
before use). Neither of these is a hot path (see the `PLAN.md` note above),
so the linear lookup's `O(n)` cost per distinct feature name was accepted
for exact behavioural parity over a `HashMap`'s `O(1)`.

## Oracle-verified test fixtures

`tests/corpus/oracle/many.fasm.{out,canonical}.txt` are the raw return
values of `fasm.fasm_tuple_to_string(fasm.parse_fasm_filename('examples/many.fasm'), canonical=...)`
from the oracle (not the `tests/oracle/fasm-oracle` CLI, which would add an
extra trailing newline from `print()`); see
`tests/corpus/oracle/README.md`. `rust/fasm/src/output/line/tests.rs`
builds the same `examples/many.fasm` model by hand (the `parser` module,
T1.3, is a separate branch under review and was not available to this
task) and compares `fasm_tuple_to_string`'s output against these files
byte for byte, in both canonical and non-canonical mode.

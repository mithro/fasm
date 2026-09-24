# Design notes: Python bindings (`rust/fasm-python`, T3.1)

Design record for the `fasm._fasm_rs` extension module and its use by the
`fasm` Python package. The module's own doc comments
(`rust/fasm-python/src/*.rs`) describe the API; this file records the
decisions and the measurements behind them. Behavioural differences from
the original package are listed in `COMPAT.md` ("Python bindings").

## Layout

* `rust/fasm-python` (crate `fasm-python`, lib name `_fasm_rs`): pyo3
  module `fasm._fasm_rs` with `parse_fasm_string`, `parse_fasm_bytes`,
  `parse_fasm_filename`, `fasm_tuple_to_string` and `FasmParseError`.
  * `src/lib.rs`: the functions, error mapping, module definition.
  * `src/convert.rs`: Rust model -> `fasm.model` namedtuples.
  * `src/output.rs`: `fasm.model` namedtuples -> Rust model (for the
    `fasm_tuple_to_string` fast path).
* `fasm/parser/rust.py`: `implementation = 'rust'`, `parse_fasm_string`,
  `parse_fasm_filename` (the API of the other parser modules), plus
  `parse_fasm_bytes` and `FasmParseError`.
* `fasm/parser/__init__.py`: imports `rust`, then `antlr` (only built by the
  legacy `setup.py`), then falls back to `textx`; `available` lists the
  parsers that imported, in that order, plus `textx`: `['rust', 'textx']`
  for a maturin build (`['rust', 'antlr', 'textx']` if an old ANTLR build
  is also present). The first one is the default.
* `pyproject.toml`: maturin build (`python-source = "."`, `module-name =
  "fasm._fasm_rs"`, `manifest-path = "rust/fasm-python/Cargo.toml"`,
  `strip = true`), with the metadata that used to be in `setup.py`.
  maturin places the extension next to the pure Python package
  (`fasm/_fasm_rs.abi3.so`).

## Returning the existing namedtuples

The module never defines its own `FasmLine` & co: `fasm.model` is imported
on first use and `FasmLine`, `SetFasmFeature`, `Annotation` and the five
`ValueFormat` members are cached in a `PyOnceLock` (`PyModel` in
`convert.rs`). Results are therefore indistinguishable from the pure Python
parsers' (`type(line) is fasm.model.FasmLine`, `==` with hand built
tuples, pickling, `_replace`, ...). `fasm.model` is imported lazily (not at
module init) so that importing `fasm._fasm_rs` from `fasm/__init__.py`'s
import chain cannot hit a partially initialised `fasm.model`. Reloading
`fasm.model` (`importlib.reload`) would leave the module building the old
classes; nothing does that.

Field types follow the ANTLR parser (the original default):
`annotations` is a `list` of `Annotation` or `None` (textX returns a
`tuple`), `comment` a `str` or `None`, `start`/`end` `int` or `None`,
`value` an `int` (1 when absent), `value_format` a `ValueFormat` member or
`None`. The functions return a `list` (textX returns a generator).

### Construction strategy

A namedtuple instance is built with `tuple.__new__(cls, items)` (a cached
`tuple.__new__` called with the class and a plain tuple of the fields).
That is what the namedtuple's generated `__new__` does itself
(`_tuple_new(_cls, (feature, start, ...))`), minus the Python level
function call; the object is identical (namedtuples have
`__slots__ = ()`). Measured in Python 3.11: `SetFasmFeature(...)` 193 ns,
`tuple.__new__(SetFasmFeature, (...))` 87 ns, `SetFasmFeature._make(...)`
158 ns.

Rejected: allocating the tuple directly (`tp_alloc` + `PyTuple_SetItem`
through the FFI). It would save the temporary tuple, but it needs `unsafe`
code that bypasses `tuple`'s own subclass construction, whose internals are
not part of the stable ABI and can change between CPython versions, while
one abi3 wheel serves all of them (including future ones).

### Integers

`FeatureValue::to_u64` covers almost every value: those become a Python
`int` directly (`PyLong_FromUnsignedLongLong`; small values come from
CPython's small int cache). Wider values (BRAM `INIT` strings are 256 bits)
go through `int.from_bytes(<little endian bytes>, 'little')` built from
`FeatureValue::as_le_limbs` (a small accessor added to the `fasm` crate for
this), which is linear in the value's size and exact for any width.
The reverse direction (`fasm_tuple_to_string`) uses `int.to_bytes`.

### Strings

Feature names are resolved from the `IdString` with `with_str` (no
allocation) into a new `str`; comments and annotation values are copied
from the Rust model. The `bytes` given to `parse_fasm_bytes` are borrowed
(`PyBackedBytes`; a `bytearray` is copied); the `str` given to
`parse_fasm_string` is encoded to a UTF-8 `bytes` copy (`PyBackedStr`: the
zero copy `PyUnicode_AsUTF8AndSize` is only in the limited API from 3.10
on, and the module targets 3.9). A `str` with lone surrogates cannot be
encoded to UTF-8 and raises `UnicodeEncodeError`, like the ANTLR parser
did for any non-ASCII input. `parse_fasm_filename` accepts `str`, `bytes`
and `os.PathLike`: the argument goes through `os.fsdecode` and back to a
`PathBuf` (lossless on Unix: surrogateescape).

### Interned feature names

The parser interns every feature name into the `fasm` crate's global
`IdString` interner (`fasm::idstring::GLOBAL`), and so does the
`fasm_tuple_to_string` fast path for every name it converts back into the
Rust model. That interner never frees anything: a long running process
that parses or formats many *distinct* feature names keeps growing, by
the size of every new per-level text (the `.` separated components are
interned per level and shared between names, see `DESIGN-idstring.md`),
without bound. Real FASM files of one device reuse a bounded set of names
(the device's tiles, sites and features), so this only matters for a
process that goes through arbitrary or generated names; there is no way to
reset the interner from Python yet.

## GIL and garbage collector

Reading the file, parsing (`fasm::parse_fasm_*`) and formatting
(`fasm::fasm_tuple_to_string`) run with the GIL released
(`Python::detach`); other Python threads run meanwhile, and several
threads can parse at once. Building the Python objects needs the GIL.

Every line becomes two or three tuples tracked by the cyclic garbage
collector, which keeps running young and (increasingly expensive) full
collections while the result list grows, although nothing in it can be
garbage. From 256 lines on, the collector is paused while the list is
built and its previous state restored afterwards (also on error); only
builtin constructors run meanwhile. This took `parse_fasm_bytes` of the
100k line file below from 0.101 s to 0.057 s (CPython 3.11).

## Errors

Parse errors and I/O errors raise `FasmParseError`, a subclass of
`Exception` created by the module (`fasm.parser.rust.FasmParseError`, also
`fasm._fasm_rs.FasmParseError`), whose `str()` is the Rust
`ParseError`'s `Display`: `Parse error at L:C - message`, the format of the
plain `Exception` the ANTLR wrapper raised. Code catching `Exception` and
the `fasm` tool (`Error: ...`) keep working. The exception also has `line`
and `column` attributes. A file that cannot be read gives
`Parse error at 0:0 - Couldn't open file <path>: <OS error>`, the same text
as the Rust `fasm` CLI (`COMPAT.md`). Value range errors (value wider than
its address) are `FasmParseError`s too; the ANTLR parser printed an
`AssertionError` and returned `None`.

## `fasm_tuple_to_string` fast path

`_fasm_rs.fasm_tuple_to_string(model, canonical=False)` converts the
namedtuples back into the Rust model and formats them with
`fasm::fasm_tuple_to_string`. The conversion is strict: it only accepts a
`list`/`tuple` of exact `fasm.model` types with exact `str`, `int`
(non negative, addresses below 2^32), `None` and `ValueFormat` member
fields, whose Python `str.format` output is known to be what Rust
produces. Anything else (an `int` or `str` subclass, a generator, a
negative value, ...) and every input for which the Rust formatter reports
an error (where Python raises an `AssertionError`) returns `None`, and the
caller is expected to run the pure Python function, which gives the
reference result or exception. `tests/test_rust_parser.py` checks that it
never returns something else than Python over the corpus and edge cases.
`fasm.fasm_tuple_to_string(model, canonical=False)` (`fasm/__init__.py`)
calls it first (when `fasm._fasm_rs` imported) and only falls back to the
pure Python implementation below it in the same function when the result
is `None` (T3.3): measured on the 100k line file, 0.085 s vs 0.109 s in
Python, and 0.98 s vs 3.95 s with `canonical=True` (where most of the time
is spent producing and sorting the 2 million canonical lines).

## Fast path wiring (T3.3)

`fasm/__init__.py` and `fasm/output.py` each do
`try: from fasm import _fasm_rs / except ImportError: _fasm_rs = None` at
import time (mirroring `fasm/parser/__init__.py`'s own try/except, but
without its `RuntimeWarning`: that warning already fires once, from
`fasm.parser`, which `fasm/__init__.py` always imports first). Both public
functions then call the Rust fast path first and only run their own pure
Python body when it is unavailable (`_fasm_rs is None`) or declines
(returns `None`):

* `fasm.fasm_tuple_to_string(model, canonical=False)` calls
  `_fasm_rs.fasm_tuple_to_string(model, canonical)` (T3.1, unchanged by
  T3.3 beyond being wired in).
* `fasm.output.merge_and_sort(model, zero_function=None, sort_key=None)`
  calls the new `_fasm_rs.merge_and_sort(model, zero_function, sort_key)`
  (below). The pure Python implementation is kept, renamed to
  `fasm.output._merge_and_sort_py`, so both implementations can be
  exercised directly (`tests/test_fast_paths.py`) and so the fallback path
  is a plain call to it, not a copy of its body.

### Only whole-model functions get a fast path

`fasm_value_to_str`, `set_feature_to_str`, `canonical_features` and
`fasm_line_to_string` (all in `fasm/__init__.py`) stay pure Python; they
are not given `_fasm_rs`-backed fast paths. Each is called once per
feature or line and does a handful of string operations — the
Python/Rust FFI call overhead itself would be a significant fraction of
that cost, unlike `fasm_tuple_to_string`/`merge_and_sort`, which convert a
whole model in one call and do all the per-line work on the Rust side.
Nothing in the Python package calls these per-line functions in a loop
that would benefit from a batched fast path (`fasm_line_to_string` is
itself only called from `fasm_tuple_to_string`'s pure Python fallback,
which the fast path replaces wholesale); a caller with its own hot per-line
loop is better served by building a whole model list and calling
`fasm_tuple_to_string` once.

### `merge_and_sort` fast path (`_fasm_rs.merge_and_sort`)

Unlike `fasm_tuple_to_string`, this cannot be a thin "convert, call into
`fasm::output`, convert back" wrapper. `fasm::output::merge_and_sort`/
`merge_and_sort_by_key` (`rust/fasm/src/output/merge.rs`, T1.4) take
`zero_function`/`sort_key` as `dyn Fn(&str) -> bool` / `-> K` closures with
`K: Ord`, which cannot represent a Python callable: it may raise (a
`dyn Fn` cannot return `Result`), and `sort_key`'s result is an arbitrary
Python object compared with rich comparison, not a concrete Rust `Ord`.
`rust/fasm-python/src/merge.rs` therefore reimplements
`MergeModel::output_sorted_lines`'s grouping/sorting logic directly against
[`MergeModel::groups`] (a small `&[Vec<FasmLine>]` accessor added to that
type in `rust/fasm/src/output/merge.rs` for this — the only change to
`rust/fasm/src/output` this task made), calling `zero_function`/`sort_key`
as Python callables with `?`-propagated `PyResult`s instead.

Grouping and address merging (`add_to_model`/`finish`/`merge_addresses`)
are pure Rust with no Python calls, so they run with the GIL released
(`Python::detach`), like parsing and `fasm_tuple_to_string`. If
`merge_addresses` returns an `OutputError` (the model hits one of the
`AssertionError`s `fasm.output.merge_features` raises, e.g. two merged
features with a conflicting bit — reachable even for a model the parser
produced, unlike `fasm_tuple_to_string`'s errors, since `merge_features`'s
invariants are about *combinations* of otherwise well formed features, not
about any single one), the function returns `None`: neither callable has
been called yet at that point (matching Python's `merge_addresses()` being
called, and able to raise, before `output_sorted_lines` — and
`zero_function`/`sort_key` — even starts), so the caller's Python fallback
runs `_merge_and_sort_py` from scratch and raises the same
`AssertionError`, with no callable called twice.

Once `zero_function`/`sort_key` has been called at least once, though, this
function commits: any exception either raises propagates directly out of
`_fasm_rs.merge_and_sort` (a Python exception, not `None`). It does not
fall back to Python at that point, which would call the same callable
again — for a callable with a side effect (as the call-count/order tests in
`tests/test_fast_paths.py` use), that would duplicate it. This is a
deliberate asymmetry from `fasm_tuple_to_string`, which never partially
commits (it does not call back into Python at all): "decline" only ever
means "before any observable side effect".

`sort_key` is called exactly once per distinct group id (a feature's first
`.` separated component), in the order group ids are first seen while
walking `MergeModel::groups()` — the same order Python's
`sorted(feature_groups.keys(), key=sort_key)` sees them in, since Python
dicts (and `MergeModel::merge_addresses`, deliberately, see the "Insertion
order" section of `docs/rewrite/DESIGN-output.md`) preserve insertion
order. The resulting Python key objects are then stably sorted
(`Vec::sort_by`, documented stable) with a comparator built from a single
`PyAny::lt` (`<`) per pair — `a < b` is `Less`, `b < a` is `Greater`,
otherwise `Equal` — deliberately **not**
[`PyAny::compare`](https://docs.rs/pyo3/0.29.2/pyo3/types/trait.PyAnyMethods.html#tymethod.compare),
which also calls `==`/`>` and requires every pair to be resolved by one of
the three: that rejects a pair a plain `<`-based sort accepts (e.g. two
instances of a class that only defines `__lt__`, like
`tests/test_fast_paths.py`'s `LtOnly` — `compare` raises `TypeError` for
two unequal, incomparable instances, but Python's own `sorted`/`list.sort`
never call `==`/`>` at all and treat "neither `a < b` nor `b < a`" as
equal for ordering purposes). `zero_function` is called once per
`set_feature` line of a group (in the same flattened, sorted-by-full-name
order Python's generator expression sees), stopping at the first `False`
— mirroring Python's `all(zero_function(...) for line in flattened_group
if line.set_feature)`, which short circuits the same way.

### Eager vs. lazy evaluation

`_merge_and_sort_py` (like the original `merge_and_sort`) returns a lazy
generator: `zero_function`/`sort_key` are called as the caller consumes
it. `_fasm_rs.merge_and_sort` cannot do that (it has to run the whole
algorithm, including every `zero_function`/`sort_key` call, to know
whether it can produce a result at all, i.e. whether to return a list or
`None`), so it returns a materialised Python `list`, which
`fasm.output.merge_and_sort` wraps in `iter()` so callers still see a
plain iterator either way. `zero_function`/`sort_key` therefore end up
called the same number of times, with the same arguments, in the same
order, but **sooner** — at the `merge_and_sort(...)` call itself rather
than while iterating its result — whenever the fast path runs. This is
the one observable difference from the original API for a caller whose
`zero_function`/`sort_key` has side effects timed against partial
consumption of the returned iterator (e.g. one that stops after the first
few lines); `tests/test_fast_paths.py`'s call-order tests check counts and
ordering, not timing against partial consumption, since nothing in the
existing test suite or `fasm.tool`/`fasm.output` callers relies on it.
Recorded in `docs/rewrite/COMPAT.md`.

### Benchmarks

Best of 7, wall time, CPython 3.11 (same machine as the `fasm_tuple_to_string`/
`parse_fasm_filename` benchmarks above):

| File | Function | Fast path | Pure Python |
|---|---|---:|---:|
| `counter_test/arty_35/top.fasm` (706 lines) | `fasm_tuple_to_string` | 0.3 ms | 0.4 ms |
| `counter_test/arty_35/top.fasm` (706 lines) | `merge_and_sort` | 0.8 ms | 1.0 ms |
| generated, 100k lines (70% plain features, 20% 64 bit binary values, 10% 256 bit hex, annotations and comments) | `fasm_tuple_to_string` | 89 ms | 127 ms |
| generated, 100k lines | `merge_and_sort` | 243 ms | 520 ms |

`merge_and_sort`'s fast path is relatively less of a speedup than
`fasm_tuple_to_string`'s: it still builds the same number of `fasm.model`
namedtuples for its result as the pure Python implementation (only the
grouping/sorting itself moves to Rust), while `fasm_tuple_to_string`
replaces both the per-line Python string formatting *and* (indirectly, by
not needing the intermediate namedtuples at all for its own output)
avoids that cost entirely.

## abi3

The crate uses pyo3's `abi3-py39` feature: one `cp39-abi3` wheel per
platform works on every CPython >= 3.9 (tested: the same wheel on 3.10,
3.11 and 3.13, and the sdist on 3.12). The cost is small: the version
specific (non abi3) build parses the 100k line file 5 to 8% faster
(0.0576 s vs 0.0620 s best of 9, CPython 3.11), because some CPython
macros become function calls. PyPy and the free-threaded builds are not
covered by abi3 wheels (they would need version specific builds).

## Build

* pyo3 0.29's `extension-module` feature is deprecated: maturin (>= 1.9.4,
  required in `[build-system]`) sets `PYO3_BUILD_EXTENSION_MODULE` so the
  wheel does not link libpython. The crate therefore does not enable the
  feature, and `cargo build` / `cargo test` / `cargo clippy` of
  `fasm-python` work like for any pyo3 program: they link libpython, so
  they need a Python >= 3.9 interpreter (and its shared library) on `PATH`
  or named by `PYO3_PYTHON`; without any interpreter the `fasm-python`
  build script fails (`no Python 3.x interpreter found`).
* The workspace's `default-members` (root `Cargo.toml`) leave
  `rust/fasm-python` out, so a bare `cargo build` / `cargo test` (without
  `--workspace` or `-p fasm-python`) needs no Python at all. CI and the
  verification commands keep `--workspace`, which builds and tests
  `fasm-python` too (GitHub's runners have a Python).
* `pip install` from source (an sdist, a checkout, a `git archive`
  tarball) needs a Rust toolchain. Without one, maturin's build backend
  downloads a Rust toolchain through `puccinialin` (about 676 MB, measured
  in the T3.1 review; needs network access) into a cache and builds with
  it. Wheels need no toolchain.
* The crate's unit tests cover only pure Rust helpers (they never start an
  interpreter); the module is tested from Python: `maturin develop` (or
  `pip install .`) into a venv, then
  `pytest tests/test_simple.py tests/test_rust_parser.py`.
* Stripped extension: 613 KB (`x86_64` Linux), wheel 295 KB.
* Version: static `0.1.0.dev0` in `pyproject.toml` for now (TODO T3.2: the
  `git describe` scheme of `update_version.py`). `fasm.__version__` comes
  from the package metadata. `fasm/version.py` (generated by the legacy
  `update_version.py`, stale since 2021) is no longer tracked in git and
  is excluded from wheels and sdists (`exclude` in `[tool.maturin]`):
  maturin only honours `.gitignore` inside a git work tree, so without the
  explicit exclude a build from a `git archive` tarball, or from a tree
  where `update_version.py` was run, would ship it and report its version
  instead. A `fasm/version.py` found in a source tree still takes
  precedence when the package is imported from that tree.

## Benchmarks

`parse_fasm_filename` (best of N, wall time, 4 core Xeon @ 2.1 GHz,
CPython 3.11, result freed outside the timed region), Rust = this module
(abi3 release build), ANTLR = the oracle (`tests/oracle/venv`), textX =
the pure Python parser:

| File | Rust | ANTLR (oracle) | textX |
|---|---:|---:|---:|
| `tests/corpus/.../counter_test/arty_35/top.fasm` (781 lines, 706 `FasmLine`s) | 0.3 ms | 3.5 ms | 74 ms |
| generated, 100k lines, 5.9 MB (70% plain features, 20% 64 bit binary `INIT`s, 10% 256 bit hex, annotations and comments) | 66 ms | 1.08 s | 22.4 s |

On CPython 3.13 (same abi3 wheel) the 100k line file takes 66 ms too. For
comparison the Rust `fasm` CLI parses, formats and writes the same file in
66 ms, so building the Python objects costs about as much as parsing.

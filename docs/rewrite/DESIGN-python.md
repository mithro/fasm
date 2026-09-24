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
It is not used by `fasm.fasm_tuple_to_string` yet (T3.3); measured on the
100k line file: 0.085 s vs 0.109 s in Python, and 0.98 s vs 3.95 s with
`canonical=True` (where most of the time is spent producing and sorting
the 2 million canonical lines).

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

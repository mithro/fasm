# Python API guide

This is the user guide for the `fasm` Python package: installing it,
touring the `fasm` and `fasm.xilinx` APIs, the fast paths, the exceptions
you can catch, and performance notes. It mirrors the reference APIs
(`fasm` from the original chipsalliance/fasm package, `xc_fasm` from
f4pga-xc-fasm) so that existing code keeps working unchanged. The design
decisions and measurements behind this API live in
`docs/rewrite/DESIGN-python.md`; behavioural differences from the
original package are tracked in
[`docs/rewrite/COMPAT.md`](rewrite/COMPAT.md).

## Install

```
pip install fasm
```

This installs the pure Python `fasm` package together with its default
parser implemented in Rust: the `fasm._fasm_rs` extension module (built
from `rust/fasm-python` with [maturin](https://www.maturin.rs/)). A pure
Python parser based on [textX](https://textx.github.io/textX/) is always
installed too, and used automatically if the compiled extension cannot be
imported (no wheel for your platform, or the extension failed to load).

```python
import fasm.parser as p
print(p.available)        # e.g. ['rust', 'textx']
print(p.implementation)   # the one actually in use
```

### Developing from source

```
python3 -m venv .venv && . .venv/bin/activate
pip install .[dev]              # maturin, textX, pytest, flake8, yapf
maturin develop --release       # builds fasm/_fasm_rs.abi3.so in place
pytest tests/test_simple.py tests/test_rust_parser.py tests/test_xilinx_python.py
```

`make build`, `make install`, `make test`, `make lint` and `make
format-py` wrap the same commands (see the repository `README.md` and
`tests/README.md`).

## API tour: `fasm`

The public API is unchanged from the reference implementation:

```python
import fasm

# Parse a whole file or string into a list of FasmLine namedtuples.
lines = fasm.parse_fasm_filename('design.fasm')
lines = fasm.parse_fasm_string(text)

for line in lines:
    if line.set_feature is not None:
        print(line.set_feature.feature, line.set_feature.value)

# Print a whole file back to FASM text (or fasm.fasm_tuple_to_string(lines,
# canonical=True) for canonical form, like `fasm --canonical`).
text = fasm.fasm_tuple_to_string(lines)

# fasm_line_to_string prints one FasmLine (a generator; non-canonical mode
# always yields exactly one string).
line_text = next(fasm.fasm_line_to_string(lines[0]))

# Merge and sort a model: fasm.output, not fasm (the `fasm` command line
# tool itself has no merge option; this is a library-only operation).
import fasm.output
merged = fasm.output.merge_and_sort(lines)
text = fasm.fasm_tuple_to_string(merged)
```

`fasm.model` defines the namedtuples every parser returns, so results
from the Rust parser, the textX parser, and hand built tuples are
interchangeable (`isinstance`, `==`, pickling, `_replace` all work
identically):

* `FasmLine(set_feature, annotations, comment)`
* `SetFasmFeature(feature, start, end, value, value_format)`
* `Annotation(name, value)`
* `ValueFormat`: `PLAIN`, `VERILOG`, `BINARY`, `HEX`, `DECIMAL`

`fasm` itself has the string-formatting building blocks
(`fasm_value_to_str`, `set_feature_width`, `set_feature_to_str`,
`canonical_features`, `fasm_line_to_string`, `fasm_tuple_to_string`);
`fasm.output` (a separate module, not re-exported by `fasm`) has
`merge_features`, `merge_and_sort` and `MergeModel`; `fasm.tool` is the
implementation behind the `fasm` console script (`fasm --help` for its
options; see COMPAT.md for its exact compatibility scope).

### Choosing a parser explicitly

`fasm.parser.rust` and `fasm.parser.textx` (there is no `fasm.parser.antlr`
module: the pre-rewrite ANTLR/setup.py build is gone, and `--parser antlr`
is just an alias for `'rust'`, see below) each expose the same
`parse_fasm_filename`/`parse_fasm_string` pair, so you can bypass
`fasm.parser.implementation`'s default:

```python
from fasm.parser import rust as fasm_rust
lines = fasm_rust.parse_fasm_filename('design.fasm')
```

`fasm.parser.rust` additionally exposes `parse_fasm_bytes` (parses a
`bytes`/`bytearray` directly, without a `str` round trip) and
`FasmParseError`.

### Fast paths

Two functions are backed by Rust when the extension is loaded, and
otherwise silently fall back to the pure Python implementation, with
identical results either way (`docs/rewrite/DESIGN-python.md` documents
the measurements):

* `fasm.fasm_tuple_to_string`: formats a whole model (an iterable of
  `FasmLine`s) back to FASM text.
* `fasm.output.merge_and_sort`: merges and sorts a model, matching
  `MergeModel`'s semantics (not necessarily canonical form: pass the
  result through `fasm_tuple_to_string(..., canonical=True)` for that).

Neither function needs the Rust extension to be present; both are pure
Python fallbacks otherwise, so code that imports them keeps working on
any platform.

### Exceptions

Parse errors and I/O errors raise `fasm.parser.rust.FasmParseError` (also
importable as `fasm._fasm_rs.FasmParseError`), a plain `Exception`
subclass whose `str()` is `Parse error at L:C - message` — the same shape
the original ANTLR-backed parser raised, so `except Exception` keeps
working. It additionally carries `.line` and `.column` attributes. A file
that cannot be opened raises
`Parse error at 0:0 - Couldn't open file <path>: <OS error>`, matching the
Rust `fasm` CLI's own message (see COMPAT.md). Value-range errors (a
value wider than the address range it is assigned to) are also
`FasmParseError`s; the original ANTLR parser instead raised
`AssertionError` and returned `None` for these (documented in COMPAT.md).

### Performance notes

* Parsing and formatting release the GIL while doing Rust work, so
  multiple threads can parse concurrently.
* For files over ~256 lines, the cyclic garbage collector is paused while
  building the result list (nothing in it can be cyclic garbage) and
  restored afterwards, even on error.
* `fasm_tuple_to_string`/`merge_and_sort`'s Rust fast paths intern feature
  names into a process-global interner that never frees entries; this is
  unbounded only if your program formats or merges very many *distinct*
  feature names (arbitrary/generated names), not for the normal case of a
  bounded set of names from one or a few devices.
* See `docs/rewrite/BENCHMARKS.md` for parser throughput numbers against
  the textX/ANTLR oracle.

## API tour: `fasm.xilinx`

`fasm.xilinx` is Python bindings for the `fasm-xilinx` Rust crate: loading
a prjxray-db/prjuray-db part database, assembling FASM into frames (the
`fasm2frames` flow, including UltraScale(+) via prjuray), and writing/
reading Series7/UltraScale/UltraScale+ bitstreams. It is part of the
`fasm` package and needs no database to install or import — a database is
only read when you call `Database.open`.

```python
import fasm.xilinx as fx

# Step by step, mirroring prjxray.fasm_assembler / xc_fasm.fasm2frames:
db = fx.Database.open('prjxray-db/artix7', 'xc7a35tcsg324-1')  # binary-cached
asm = fx.FasmAssembler(db)
asm.parse_fasm_filename('top.fasm')
asm.add_required_features()
asm.propagate_stepdown()
frames = asm.get_frames(sparse=True)   # a Frames: address -> list[int] words
frames.write_frm('top.frm')            # byte for byte fasm2frames.py's output
fx.write_bitstream(frames, db, 'top.bit')  # byte for byte xc7frames2bit's

# One shot, mirroring xc_fasm.fasm2frames.fasm2frames() / the xcfasm tool:
frames = fx.fasm2frames('prjxray-db/artix7', 'xc7a35tcsg324-1', 'top.fasm')
fx.fasm2bit('prjxray-db/artix7', 'xc7a35tcsg324-1', 'top.fasm', 'top.bit')
back = fx.read_bitstream('top.bit', db)  # like bitread --frm_out
```

prjuray-db (UltraScale/UltraScale+) parts work the same way; pass
`format='UltraScale'`/`'UltraScalePlus'` (or leave `format=None` to use
the database's own architecture) to `write_bitstream`/`read_bitstream`.

Main types (full signatures and the reference each mirrors are in
`docs/rewrite/DESIGN-python.md` "`fasm.xilinx`" and the `.pyi` stubs):

* `Database.open(db_root, part=None, cache=None)`: `.root`, `.part`,
  `.layout`, `.architecture`, `.words_per_frame`, `.idcode`,
  `.tile_types()`, `.tiles()`, `.required_features()`,
  `.frame_addresses()`, `.lookup_feature(feature, address=0)`.
* `FasmAssembler(db, prjuray=None)`: `parse_fasm_filename`/`_string`/
  `_bytes`, `add_fasm_line`, `add_required_features()`,
  `mark_roi_frames(roi)`, `propagate_stepdown()`,
  `set_feature_callback(fn)`, `get_frames(sparse=False)`, `.warnings`.
* `Frames`: a read-only `collections.abc.Mapping` of address to a list of
  32-bit words; `to_frm()`, `write_frm(path)`, `Frames.read_frm(path)`,
  `to_bytes()`, `set_bits()`.
* `write_bitstream(frames, part, output=None, *, format=None, ...)`,
  `read_bitstream(source, part, *, format=None, ...)`.
* `fasm2frames(db_root, part=None, filename_in=None, ...)`,
  `fasm2bit(db_root, part, fn_in, bit_out, ...)`: the one-shot
  convenience functions, same positional argument order as
  `xc_fasm.fasm2frames.fasm2frames`.

`cache=None` (default) or `True` opens the database through the same
binary cache as the command line tools (`FASM_XDB_CACHE`, see the
repository README's "Environment variables"); `False` disables it for
that call; a path uses that directory.

### Exceptions

`fasm.xilinx.Error` is the base class for `DbError`, `FasmLookupError`
(one message per missing feature bit), `FasmInconsistentBits`,
`FasmKeyError` (also a `KeyError`), `FasmParseError` (also
`fasm.parser.rust.FasmParseError`), `FrmError` (also a `ValueError`) and
`BitstreamError`. A file that cannot be opened raises the builtin
`OSError` with Python's own message. Every raised exception's `str()` is
the message the reference command line tool prints, and it carries a
`reference_exception` attribute naming that tool's exception class, so
`f'{e.reference_exception}: {e}'` reproduces its stderr line.

`fasm/xilinx/__init__.pyi` provides type stubs, and `fasm/py.typed` marks
the package as typed (PEP 561), so type checkers pick them up
automatically.

### Threads

`Database` is immutable and shareable across threads. `FasmAssembler` is
internally locked; parsing, assembling, database opens and bitstream I/O
release the GIL, so other Python threads run while Rust code runs. A
feature callback (`set_feature_callback`) that calls back into its own
assembler raises `RuntimeError` instead of deadlocking.

### Performance notes

Opening a database through the binary cache is roughly 4-6x faster than a
cold parse of the text database; assembling and writing a bitstream with
an already-open database is low single-digit milliseconds for a small
design. See `docs/rewrite/DESIGN-python.md` ("Tests and performance") and
`docs/rewrite/BENCHMARKS.md` for measured numbers and methodology.

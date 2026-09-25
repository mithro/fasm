## FPGA Assembly (FASM) Parser and Generation library

This repository documents the FASM file format and provides parsing
libraries and simple tooling for working with FASM files. The parser and
tools are implemented in Rust (see `rust/`), with a Python package and a C
API built on top of the Rust library.

## Python package

```
pip install fasm
```

This installs the `fasm` Python package with its default parser
implemented in Rust (the `fasm._fasm_rs` extension module, built from
`rust/fasm-python`); a pure Python parser based on `textx` is always
installed too and used as a fallback if the compiled extension cannot be
imported (a wheel is not available for your platform, or the extension
failed to load).

Which parsers are available in your installation can be found with:

```
python3 -c "import fasm.parser as p; print(p.available)"
```

The parser currently in use is `fasm.parser.implementation`.

### Developing the Python package

The package is built with [maturin](https://www.maturin.rs/):

```
python3 -m venv .venv && . .venv/bin/activate
pip install .[dev]              # or: pip install -r requirements.txt
maturin develop --release       # editable install: builds fasm/_fasm_rs.abi3.so
pytest tests/test_simple.py tests/test_rust_parser.py
```

`.[dev]` (equivalently `requirements.txt`) installs maturin, textX,
pytest, flake8 and yapf. See `tests/README.md` for the two ways to run the
Python tests (editable install vs. against a built wheel/sdist) and
`tox.ini` for running them across Python versions. `make build`, `make
install`, `make test`, `make lint` and `make format-py` wrap the same
commands; `make check-license` and `make check-python-scripts` check
license headers and script headers.

## Rust workspace

The FASM parser, model, output formatting and command line tool live in
the Cargo workspace under `rust/`:

* `rust/fasm`: the core library crate (parsing, the in-memory model,
  output formatting/canonicalisation).
* `rust/fasm-cli`: the `fasm` command line tool (a drop-in replacement for
  the original `fasm/tool.py` console script), the Xilinx tools
  `fasm2frames`, `xcfasm` (f4pga-xc-fasm), `xc7frames2bit` and `bitread`
  (prjxray), the UltraScale / UltraScale+ tools `xcframes2bit` and
  `uray-bitread` (prjuray-tools' `xcframes2bit` and `bitread`) and
  `uray-fasm2frames` (prjuray's `utils/fasm2frames.py`), and
  `fasm-db-cache`. `fasm2frames` also assembles prjuray-db (UltraScale+)
  parts, into the 32-bit word `.frm` files `xcframes2bit` reads:

  ```
  fasm2frames --db-root prjuray-db/zynqusp --part xczu3eg-sfvc784-1-e \
      design.fasm design.frm
  xcframes2bit --architecture=UltraScalePlus --part_name=xczu3eg \
      --part_file=prjuray-db/zynqusp/xczu3eg-sfvc784-1-e/part.yaml \
      --frm_file=design.frm --output_file=design.bit
  uray-bitread --architecture=UltraScalePlus -z -y \
      --part_file=prjuray-db/zynqusp/xczu3eg-sfvc784-1-e/part.yaml design.bit
  ```
* `rust/fasm-xilinx`: Xilinx database loading (prjxray-db, prjuray-db),
  FASM -> frames, and the Series7, UltraScale and UltraScale+ bitstream
  writer and reader (see `docs/rewrite/DESIGN-xilinx-db.md`).
* `rust/fasm-capi`: the C ABI (`libfasm_capi`) of `fasm` and `fasm-xilinx`,
  with a generated header at `include/fasm/fasm.h` (see
  `docs/rewrite/DESIGN-capi.md`).
* `rust/fasm-python`: the pyo3 extension module behind the Python package
  above, including the `fasm.xilinx` bindings of `fasm-xilinx` (see
  `docs/rewrite/DESIGN-python.md`).

```
cargo build --workspace
cargo test --workspace
cargo run -p fasm-cli -- --help
```

`make rust-build`, `make rust-test`, `make rust-lint` and `make rust-doc`
wrap the same commands; `make capi-header`, `make capi-header-check` and
`make capi-test` build and test the C API and its generated header, and
its header-only C++17 RAII wrapper `include/fasm/fasm.hpp` (`fasm::File`,
`fasm::Line`, ...; see `docs/rewrite/DESIGN-capi.md`). `make capi-install
PREFIX=/some/prefix` installs both headers, `libfasm_capi.{so,a}` and a
`fasm.pc` pkg-config file for either language (`rust/fasm-capi/fasm.pc.in`,
`rust/fasm-capi/examples/cpp` for a minimal C++ example that builds
against it with `pkg-config --cflags --libs fasm`).

### Xilinx database cache

`fasm2frames` and `xcfasm` keep a binary cache of each prjxray-db /
prjuray-db part they open, so that only the first run of a part pays for
parsing the text database (about 4-6x faster opens: e.g. 23 ms instead of
about 100 ms for xc7a35t). A cache file is only used when none of the files
it was built from changed (size, stat fingerprint, BLAKE3 content hash),
otherwise it is silently rebuilt; the output is identical either way.
It is configured by the environment only (the command lines stay those
of the reference tools):

* `FASM_XDB_CACHE`: the cache directory (default
  `$XDG_CACHE_HOME/fasm/db`, else `~/.cache/fasm/db`); `0` or empty
  disables the cache. (`FASM_DB_CACHE` is where `tools/fetch-db.sh` puts
  the text databases.)
* `FASM_XDB_CACHE_VERBOSE=1`: report cache hits, rebuilds and their
  reason on stderr.

`fasm-db-cache build DB_ROOT PART...` (or `build --all DB_ROOT`) fills
the cache ahead of time; `verify` re-hashes every source file, `info`,
`list` and `clear` do what they say (`fasm-db-cache --help`). See
`docs/rewrite/DESIGN-xilinx-db.md` §8.8.

### Xilinx bitstreams from Python, C and C++

The same FASM -> frames -> `.bit` flow is available as a library. From
Python (`fasm.xilinx`, part of the `fasm` package; no database is needed
to install or import it):

```python
import fasm.xilinx as fx

db = fx.Database.open('prjxray-db/artix7', 'xc7a35tcsg324-1')  # cached like the CLI
asm = fx.FasmAssembler(db)
asm.parse_fasm_filename('top.fasm')
asm.add_required_features()
asm.propagate_stepdown()
frames = asm.get_frames(sparse=True)       # a mapping: address -> words
frames.write_frm('top.frm')                # byte for byte fasm2frames' output
fx.write_bitstream(frames, db, 'top.bit')  # byte for byte xc7frames2bit's

# Or in one step, like xc_fasm's fasm2frames() / the xcfasm tool:
frames = fx.fasm2frames('prjxray-db/artix7', 'xc7a35tcsg324-1', 'top.fasm')
fx.fasm2bit('prjxray-db/artix7', 'xc7a35tcsg324-1', 'top.fasm', 'top.bit')
back = fx.read_bitstream('top.bit', db)    # like bitread --frm_out
```

prjuray-db (UltraScale+) parts work the same way (`format=` selects the
UltraScale / UltraScale+ bitstream variants). Errors are
`fasm.xilinx.Error` subclasses (`DbError`, `FasmLookupError`,
`FasmInconsistentBits`, ...) with the messages of the command line tools.
See `docs/rewrite/DESIGN-python.md`.

From C (`include/fasm/fasm.h`, `libfasm_capi`; error handling shortened):

```c
fasm_xilinx_database *db = NULL;
fasm_xilinx_frames *frames = NULL;
fasm_xilinx_part *part = NULL;
fasm_error *err = NULL;
fasm_xilinx_fasm2frames_options options = {0};
options.sparse = true;
if (fasm_xilinx_database_open_cached("prjxray-db/artix7", "xc7a35tcsg324-1", NULL, &db, &err) ||
    fasm_xilinx_fasm2frames_file(db, "top.fasm", &options, &frames, &err) ||
    fasm_xilinx_part_from_database(db, &part, &err) ||
    fasm_xilinx_bitstream_write_file(part, frames, NULL, "top.bit", &err)) {
    fprintf(stderr, "%s: %s\n", fasm_error_kind(err), fasm_error_message(err));
    fasm_error_free(err);
}
fasm_xilinx_part_free(part);
fasm_xilinx_frames_free(frames);
fasm_xilinx_database_free(db);
```

and from C++ (`include/fasm/fasm.hpp`, `namespace fasm::xilinx`):

```cpp
namespace fx = fasm::xilinx;
auto db = fx::Database::open_cached("prjxray-db/artix7", "xc7a35tcsg324-1");
fx::Frames frames = fx::fasm2frames(db, "top.fasm");
fx::write_bitstream_file(fx::Part::from_database(db), frames, "top.bit");
```

See `docs/rewrite/DESIGN-capi.md` ("Xilinx").

## What changed in this rewrite

This repository is being rewritten from the original Python/ANTLR/C++
implementation to Rust; see `docs/rewrite/PLAN.md` for the architecture,
`docs/rewrite/TASKS.md` for progress and
**[`docs/rewrite/COMPAT.md`](docs/rewrite/COMPAT.md)** for every documented
behavioural difference from the original package and `fasm` command line
tool (parser acceptance, error messages and positions, the Python bindings,
the C API). Until a 1.0 release, `fasm.__version__` and the package version
are a static `0.1.0.dev0` (see `docs/rewrite/DESIGN-python.md`).

## FPGA Assembly (FASM)

FPGA Assembly is a file format designed by the
[F4PGA Project](https://f4pga.org/) developers to provide a plain
text file format for configuring the internals of an FPGA.

It is designed to allow FPGA place and route to not care about the *actual*
bitstream format used on an FPGA.

![FASM Ecosystem Diagram](docs/_static/image/fasm-diagram.png)

### Properties

 * Removing a line from a FASM file leaves you with a valid FASM file.
 * Allow annotation with human readable comments.
 * Allow annotation with "computer readable" comments.
 * Has syntactic sugar for expressing memory / lut init bits / other large
   arrays of data.
 * Has a canonical form.
 * Does not require any specific bitstream format.

### Supported By

FASM is currently supported by the
[F4PGA Verilog to Routing fork](https://github.com/f4pga/vtr-verilog-to-routing),
but we hope to get it merged upstream sometime soon.

It is also used by [Project X-Ray](https://github.com/f4pga/prjxray).

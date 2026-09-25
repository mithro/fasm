## FPGA Assembly (FASM) Parser and Generation library

This repository documents the FASM file format and provides a drop-in
compatible **Rust** implementation of the original Python/ANTLR/C++ `fasm`
tools, plus Xilinx 7-series, UltraScale and UltraScale+ frame and
bitstream tooling (a Rust equivalent of Project X-Ray's `fasm2frames.py` /
`xc7frames2bit` and prjuray-tools' `xcframes2bit`/`bitread`), with **Python
bindings** and a **C/C++ API** built on the same Rust library. Every
binary and API is verified byte-for-byte against the originals over real
designs; see [Verification](#verification) below.

This repository is in the middle of that Rust rewrite; see [What changed
in this rewrite](#what-changed-in-this-rewrite) and
`docs/rewrite/PLAN.md`/`docs/rewrite/TASKS.md` for status.

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

(`pip install fasm` today installs the pre-rewrite package published on
PyPI, not this repository's build; publishing the Rust rewrite's wheels
is T8.4, not yet done. Until then, build and install from source as
below, or with `pip install .` / `pip wheel .` from a checkout.)

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

See **[`docs/PYTHON.md`](docs/PYTHON.md)** for a full API tour of `fasm`
and `fasm.xilinx` (parsing, output/merge fast paths, exceptions,
performance notes).

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

### Binaries

Every binary is a byte/exit-code compatible drop-in replacement for the
named original tool (see `docs/rewrite/COMPAT.md` for the exact, tested
scope of "compatible" and any documented divergence):

| Binary | Crate | Compatible with |
|---|---|---|
| `fasm` | `fasm-cli` | `fasm/tool.py` (`fasm` console script) |
| `fasm2frames` | `fasm-cli` | f4pga-xc-fasm's `xc_fasm/fasm2frames.py` (7 series **and** UltraScale/UltraScale+ parts) |
| `xc7frames2bit` | `fasm-cli` | prjxray's C++ `xc7frames2bit` |
| `bitread` | `fasm-cli` | prjxray's C++ `bitread` |
| `xcfasm` | `fasm-cli` | f4pga-xc-fasm's `xc_fasm/xcfasm.py` one-shot fasm -> bit tool |
| `xcframes2bit` | `fasm-cli` | prjuray-tools' `xcframes2bit` (UltraScale/UltraScale+) |
| `uray-bitread` | `fasm-cli` | prjuray-tools' `bitread` |
| `uray-fasm2frames` | `fasm-cli` | prjuray's `utils/fasm2frames.py` |
| `fasm-db-cache` | `fasm-cli` | new: maintenance for the binary database cache below (no Python equivalent) |

All of them are built by `cargo build --workspace` and installed by
`cargo install --path rust/fasm-cli` (or run in place with `cargo run -p
fasm-cli --bin <name> --`).

### Environment variables

| Variable | Used by | Meaning |
|---|---|---|
| `FASM_XDB_CACHE` | `fasm2frames`, `xcfasm`, `uray-fasm2frames`, `fasm-db-cache`, `fasm.xilinx` | Binary database cache directory (default `$XDG_CACHE_HOME/fasm/db`, else `~/.cache/fasm/db`); `0` or empty disables it. See "Xilinx database cache" below. |
| `FASM_XDB_CACHE_VERBOSE` | same as above | `1` reports cache hits/rebuilds and the reason on stderr. |
| `FASM_DB_CACHE` | `tools/fetch-db.sh`, the test suite | Where the *text* prjxray-db/prjuray-db checkouts used by tests are cached (not the binary cache above). |
| `XRAY_DATABASE_DIR`, `XRAY_DATABASE`, `XRAY_PART` | `fasm2frames`, `xcfasm` | Defaults for `--db-root`/`--part` (`rust/fasm-cli/src/fasm2frames.rs`), mirroring prjxray-db's own `fasm2frames.py`/`xcfasm.py` environment fallback. |
| `URAY_DATABASE_DIR`, `URAY_DATABASE`, `URAY_PART` | `uray-fasm2frames` | Defaults for `--db-root`/`--part`, mirroring prjuray's own `fasm2frames.py` environment fallback. |
| `SOURCE_DATE_EPOCH` | `xc7frames2bit`, `xcframes2bit`, `xcfasm`, and `fasm.xilinx.write_bitstream`/`fasm2bit` and the C `fasm_xilinx_bitstream_write*` functions | Reproducible-build extension (as used by e.g. Nix/Bazel): when set to an integer, used as the bitstream header timestamp instead of the current time. |

### Xilinx database cache

`fasm2frames`, `xcfasm` and `uray-fasm2frames` keep a binary cache of
each prjxray-db / prjuray-db part they open, so that only the first run
of a part pays for parsing the text database (about 4-6x faster opens:
e.g. 23 ms instead of about 100 ms for xc7a35t). A cache file is only
used when none of the files it was built from changed (size, stat
fingerprint, BLAKE3 content hash), otherwise it is silently rebuilt; the
output is identical either way. It is configured by the environment only
(`FASM_XDB_CACHE`, `FASM_XDB_CACHE_VERBOSE`; see "Environment variables"
above) so the command lines stay those of the reference tools.

See [`docs/rewrite/BENCHMARKS.md`](docs/rewrite/BENCHMARKS.md) for the
full benchmark suite and numbers: parser throughput vs. the Python/ANTLR
oracle (double digit to 300x+ speed-ups depending on file size and
parser), database open with/without the cache, assembly and bitstream
timings, and Python binding overheads.

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

See **[`docs/CAPI.md`](docs/CAPI.md)** for the full C/C++ user guide:
building/linking with pkg-config or CMake, the error-handling pattern,
ownership rules, threading and ABI stability notes, and complete
parse/print/merge examples in addition to the Xilinx ones above.

## Repository layout

```
fasm/                       the Python package (thin layer over the Rust
                            extension, textX fallback kept; see docs/PYTHON.md)
rust/
  fasm/                     core library: idstring, model, parser, output
  fasm-cli/                 the binaries in the table above
  fasm-xilinx/              prjxray-db/prjuray-db loader, FASM -> frames,
                            frames -> bitstream (Series7/UltraScale(+))
  fasm-capi/                the C ABI (`libfasm_capi`) behind include/fasm/
  fasm-python/              the pyo3 extension module behind fasm/
include/fasm/                fasm.h (generated by cbindgen) and fasm.hpp
tests/                      Python tests, Rust integration tests, e2e tests
tests/corpus/               FASM/frames/bitstream corpus for differential tests
tests/oracle/               scripts that build/run the original Python and
                            C++ tools as the reference ("oracle") for tests
tools/                      corpus generation, differential test drivers,
                            toolchain setup scripts (tools/e2e/)
docs/rewrite/                PLAN.md, TASKS.md, LOG.md, WORKFLOW.md, COMPAT.md,
                            BENCHMARKS.md, DESIGN-*.md
docs/PYTHON.md, docs/CAPI.md  user guides for the Python and C/C++ APIs
```

## Verification

Every tool and binding is checked against the original implementation with
byte-for-byte differential tests ("difftest"), not just unit tests. See
**[`docs/rewrite/COMPAT.md`](docs/rewrite/COMPAT.md)** for the full,
per-tool list of tested scope and every documented divergence, and
**[`docs/rewrite/BENCHMARKS.md`](docs/rewrite/BENCHMARKS.md)** for
performance numbers. Headline results from `docs/rewrite/LOG.md` (see that
file for the run that produced each number):

* **Parser/model/output/CLI**: 0 unexplained differences from the textX
  and ANTLR oracle parsers over the whole FASM corpus
  (`tools/difftest.py`); every intentional divergence is documented in
  COMPAT.md ("Parser"). The CLI is differential-tested byte-for-byte for
  stdout/stderr/exit code (`tests/cli`).
* **Every 7-series part**: all 125 prjxray-db parts, every-feature
  synthetic corpus (`tools/gen-xilinx-corpus.py`; `tools/gen-corpus.py`
  is the plain-FASM parser corpus, a different generator),
  `fasm2frames`/`xc7frames2bit` output identical to the Python/C++
  reference tools (the remaining differences are explained, one
  documented error case per part, not unexplained divergence) — see
  COMPAT.md and LOG.md's T5.9/T6.3 entries.
* **prjuray-db (UltraScale+)**: both shipped zynqusp parts, every-feature
  corpus, identical `uray-fasm2frames`/`xcframes2bit`/`uray-bitread`
  output (T6.3).
* **f4pga-examples**: 30 design/board pairs built through the real f4pga
  (VPR) flow, FASM collected and compared frame-for-frame and bit-for-bit
  against the flow's own tools and the pinned oracle (T7.3).
* **fpgas.online-test-designs**: 18 Xilinx designs built through LiteX +
  openXC7, frames identical to the flow's own `fasm2frames` (T7.2).
* **nextpnr-xilinx / openXC7**: 21 example designs from
  `nextpnr-xilinx/xilinx/examples` and related repos, identical to the
  installed openXC7 toolchain (T7.6).
* **VTR `genfasm`**: 447 designs produce FASM (428 generic VTR benchmarks +
  19 Xilinx designs through the f4pga flow), identical to the Python
  oracle parser and, for the Xilinx subset, to the flow's own tools.
  T7.4 is implemented but **still in review** and not yet merged into
  this tree at the time of writing (see TASKS.md) — `tools/e2e/` does
  not yet have the VTR setup/run scripts described below.

## Running the test suites

* `cargo test --workspace` — Rust unit/integration/doc tests.
* `make test` — the Python test suite (`tests/test_simple.py`,
  `tests/test_rust_parser.py`, `tests/test_xilinx_python.py`); needs `make
  install` (or `maturin develop --release`) first.
* `tests/oracle/setup.sh` (and `tests/oracle/setup-xilinx.sh` for the
  Xilinx tools) build the reference "oracle" venv/tools from the
  pre-rewrite history; then `tools/difftest.py` and
  `tools/difftest-xilinx.py` compare Rust output against it over
  `tests/corpus/` (see `tests/oracle/README.md`, `tests/corpus/README.md`).
* `tools/e2e/` sets up real toolchains (openXC7, f4pga/VPR; VTR once T7.4
  merges, see "Verification" above) and runs `pytest tests/e2e` to
  reproduce the Verification numbers above; see `tools/e2e/README.md`
  for prerequisites (these download multi-GB toolchains and are not run
  by default CI).
* `make lint` / `make format-py` (flake8/yapf) and `make rust-lint`
  (`cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`) for style.

## What changed in this rewrite

This repository is being rewritten from the original Python/ANTLR/C++
implementation to Rust; see `docs/rewrite/PLAN.md` for the architecture,
`docs/rewrite/TASKS.md` for progress and
**[`docs/rewrite/COMPAT.md`](docs/rewrite/COMPAT.md)** for every documented
behavioural difference from the original package and `fasm` command line
tool (parser acceptance, error messages and positions, the Python bindings,
the C API). Until a 1.0 release, `fasm.__version__` and the package version
are a static `0.1.0.dev0` (see `docs/rewrite/DESIGN-python.md`).

## License

Apache License 2.0 (`LICENSE`, `SPDX-License-Identifier: Apache-2.0`); see
`AUTHORS` for the list of significant contributors.

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

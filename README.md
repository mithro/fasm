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
  the original `fasm/tool.py` console script).
* `rust/fasm-xilinx`: Xilinx 7 series database loading and bitstream
  generation (in progress, see `docs/rewrite/TASKS.md`).
* `rust/fasm-capi`: the C ABI (`libfasm_capi`), with a generated header at
  `include/fasm/fasm.h` (see `docs/rewrite/DESIGN-capi.md`).
* `rust/fasm-python`: the pyo3 extension module behind the Python package
  above (see `docs/rewrite/DESIGN-python.md`).

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

# Changelog

All notable changes to this project. Versions follow
[Semantic Versioning](https://semver.org/) once `0.1.0` ships; see
`docs/RELEASING.md` for how a release is cut and where each version
string lives.

## [0.1.0-dev] - unreleased

Not yet published anywhere (see `docs/RELEASING.md`). This entry
summarises the Rust rewrite of the FASM tooling up to and including T8.4
(packaging); see `docs/rewrite/PLAN.md` and `docs/rewrite/LOG.md` for the
full, dated history.

### Added

* A Rust implementation of the FASM (FPGA Assembly) file format: idstring
  interning, data model, parser and output/merge support (crate `fasm`),
  verified byte-for-byte against the original Python/ANTLR parser over
  the test corpus and a fuzzer.
* Xilinx Series-7 / UltraScale / UltraScale+ tooling (crate
  `fasm-xilinx`): a prjxray/prjuray tile-grid database loader (with a
  binary cache), a FASM to configuration-frames assembler and a frames to
  bitstream writer/reader, verified against Project X-Ray's
  `fasm2frames.py`/`xc7frames2bit` and prjuray-tools'
  `xcframes2bit`/`bitread` over real and synthetic designs (including
  designs run through the f4pga and openXC7/nextpnr-xilinx toolchains and
  VTR's `genfasm`).
* Command line tools (crate `fasm-cli`): a `fasm` binary command-line
  compatible with the original Python `fasm/tool.py`, plus
  `fasm2frames`, `xcfasm`, `xc7frames2bit`, `bitread`,
  `uray-xcframes2bit`, `uray-bitread`, `uray-fasm2frames` and a
  `fasm-db-cache` database cache tool.
* A C ABI and header-only C++17 wrapper (crate `fasm-capi`,
  `include/fasm/fasm.h` / `fasm.hpp`) covering parsing, the data model,
  output/merge and the Xilinx frame/bitstream functions, installable with
  `make capi-install` (pkg-config `fasm.pc` and, as of T8.4, a CMake
  package config for `find_package(fasm CONFIG)`).
* Python bindings (`fasm._fasm_rs`, a pyo3 extension built with maturin
  from crate `fasm-python`) exposing the same `fasm`/`fasm.xilinx` API as
  the original package, used automatically in place of the pure-Python
  textX parser when available, with a documented, always-available
  textX fallback.
* Benchmarks (`docs/rewrite/BENCHMARKS.md`) showing the Rust parser well
  above the Python/ANTLR and textX implementations, and `fasm2frames`
  30-40x faster than the Python oracle on real designs.
* Documentation: a rewritten front-page README, `docs/PYTHON.md` and
  `docs/CAPI.md` user guides (every example in them is executed by the
  test suite/CI), `docs/rewrite/COMPAT.md` (every known behavioural
  difference from the original tools), and this release process
  (`docs/RELEASING.md`).
* crates.io publish metadata for `fasm`, `fasm-xilinx`, `fasm-cli` and
  `fasm-capi` (description, license, readme, keywords/categories,
  include lists) and a `wheels.yml` CI workflow building manylinux/macOS/
  Windows wheels and an sdist (T8.4). Not yet published: see
  `docs/RELEASING.md` for what is still needed, notably an unrelated
  crate already using the name `fasm` on crates.io.

### Changed from the original `chipsalliance/fasm` package

* The default Python parser is now the Rust extension, not textX or the
  ANTLR C++ parser (both still work: textX as the always-installed
  fallback; see `docs/rewrite/COMPAT.md` for the small number of parser
  behaviour differences this uncovered, e.g. around malformed input
  diagnostics).
* Packaging is maturin-based (`pyproject.toml`); the legacy setuptools +
  CMake + ANTLR build (`setup.py`, `CMakeLists.txt` at the repository
  root) is removed.
* Version is currently a static pre-release string rather than derived
  from `git describe` (the legacy `update_version.py`); see
  `docs/RELEASING.md`.

### Known gaps (tracked in `docs/rewrite/TASKS.md`)

* T7.5 (RapidWright end-to-end compatibility) not started.
* T8.2b (parser performance follow-ups on the `stress` benchmark class)
  not started.
* T8.3b (`fasm/__init__.py`'s `dir(fasm)` `TypeError`, inherited from the
  original package) not resolved either way yet.

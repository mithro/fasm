# FASM Rust Rewrite: Plan

This document is the master plan for rewriting the FASM assembler in Rust.
It is kept in the repository so that any agent (human or AI) can resume the
work from any point. See `TASKS.md` for the live task list and `LOG.md` for
the progress log. See `WORKFLOW.md` for the agent workflow rules.

## Status

See `TASKS.md` for the authoritative, up to date task list (this section
is a pointer, not a duplicate — do not edit `TASKS.md`/`LOG.md` from here).
As of the T8.3 documentation pass, Phases 0-6 are complete (`[x]`) except
four small, deferred follow-up tasks logged from their task's review
(not blockers): T0.4b (ANTLR oracle build flakiness), T5.3b (database
cache follow-ups), T5.8b (`fetch-db.sh openxc7`'s bundled prjxray-db),
T6.3b (small T6.3 review follow-ups). Phase 7 (toolchain end-to-end
compatibility) is complete except T7.4 (VTR: implemented, `[r]` in
review, not yet merged into this tree) and T7.5 (RapidWright, `[ ]` not
started). Phase 8 (performance/docs/packaging): T8.1 and T8.2 are done,
T8.2b (T8.2 performance follow-ups) is `[ ]` not started, T8.3 (this
documentation pass) is `[~]` in progress, and T8.4 (packaging: crates.io
metadata, maturin wheels workflow, CMake install, release notes) is
`[ ]` not started. See `LOG.md` for the detailed, dated history behind
every one of these (branch names, commit hashes, review verdicts,
measured numbers).

### Design docs index

The design documents referenced throughout this plan and by `TASKS.md`:

| Document | Covers |
|---|---|
| `docs/rewrite/DESIGN-idstring.md` | the `idstring` interned string module (T1.1) |
| `docs/rewrite/DESIGN-model.md` | the `model` module: `FasmLine`, `SetFasmFeature`, `Annotation`, `ValueFormat` (T1.2) |
| `docs/rewrite/DESIGN-output.md` | output formatting, canonicalisation, merge/sort (T1.4) |
| `docs/rewrite/DESIGN-python.md` | the `fasm-python` pyo3 bindings, including `fasm.xilinx` (T3.1-T3.3, T5.10) |
| `docs/rewrite/DESIGN-capi.md` | the C ABI (`fasm-capi`) and the C++ header-only wrapper, including the Xilinx C/C++ API (T4.1, T4.2, T5.10) |
| `docs/rewrite/DESIGN-xilinx-db.md` | the prjxray-db/prjuray-db database format, loader, cache, assembler and bitstream writer/reader (T5.1-T6.3) |
| `docs/rewrite/BENCHMARKS.md` | the benchmark suite and measured numbers (T8.1, updated for T8.2) |
| `docs/rewrite/COMPAT.md` | every documented behavioural divergence from the original Python/C++ tools, by tool |

## Goals (from the original request)

1. Rewrite the `fasm` assembler in Rust, split into a **library** and a
   **command line tool**.
2. The command line tool is Rust only and **100% command line compatible**
   with the original Python `fasm` tool (`fasm/tool.py`): same arguments,
   same stdout/stderr behaviour, same exit codes.
3. Provide **Python bindings** that are API compatible with the existing
   Python package (`fasm`, `fasm.model`, `fasm.output`, `fasm.parser`,
   `fasm.tool`).
4. Provide **C** and **C++** wrappers for the library.
5. Support **fast and efficient bitstream generation** for all parts found in
   prjxray (Xilinx 7 series) and prjuray (UltraScale, UltraScale+).
6. Heavy compatibility testing against FASM produced by **VPR** (`genfasm`),
   **nextpnr-xilinx** (openXC7 fork) and **RapidWright**.
7. Internal string representation based on the ideas of
   <https://github.com/mithro/idstring> (8-byte hierarchical interned id).
8. <https://github.com/lromor/fpga-assembler> used as a reference for the
   frames/bitstream pipeline.
9. All examples in <https://github.com/chipsalliance/f4pga-examples> work.
10. All Xilinx designs in
    <https://github.com/fpgas-online/fpgas.online-test-designs> work.
11. The tooling works with as many verilog-to-routing test designs as
    possible.
12. Small incremental commits, merges not rebases, progress log + task list
    kept in the repository, code reviewed by independent sub-agents, at most
    2 sub-agents running at any time.
13. Never push anywhere except `mithro/fasm`.

## Repository layout (target)

```
fasm/                       existing Python package (kept; becomes a thin
                            layer over the Rust extension, textX fallback kept)
rust/
  Cargo.toml                (workspace root is the repo root Cargo.toml)
  fasm/                     crate `fasm`     : core library (idstring, model,
                                                parser, output, merge/sort)
  fasm-cli/                 crate `fasm-cli` : `fasm` binary (100% compatible
                                                with fasm/tool.py) and the
                                                Xilinx bitstream binaries
                                                (`fasm2frames`, `xc7frames2bit`,
                                                `xcfasm` compatible CLIs)
  fasm-xilinx/              crate `fasm-xilinx`: prjxray/prjuray database
                                                loader (+ binary cache),
                                                FASM -> frames assembler,
                                                frames -> bitstream writer
                                                (Series7 / UltraScale /
                                                UltraScale+), bit -> frames
  fasm-capi/                crate `fasm-capi`: C ABI (`libfasm`), header
                                                `include/fasm/fasm.h` and C++
                                                header-only wrapper
                                                `include/fasm/fasm.hpp`
  fasm-python/              crate `fasm-python`: pyo3 extension module
                                                `fasm._fasm_rs` built with
                                                maturin
include/fasm/               C and C++ headers
tests/                      Python tests (existing) + compatibility suites
tests/corpus/               FASM corpus used by differential tests
tests/oracle/               scripts to build/run the original Python
                            implementation as an oracle
tools/                      helper scripts (toolchain setup, corpus
                            generation, differential test drivers)
docs/rewrite/               PLAN.md, TASKS.md, LOG.md, WORKFLOW.md, DESIGN-*.md
```

## Architecture

### `fasm` crate (core)

* `idstring` module: `IdString` is a `Copy`, 8-byte handle for a
  hierarchical dotted string (e.g. `CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT`).
  Following mithro/idstring: the string is split on `.` into up to N levels,
  each level is interned in a per-level table and the handle stores the
  per-level indexes. The last level stores the remainder when there are more
  than N components. Requirements:
  * `Eq`/`Hash` are integer operations, `Ord` compares by string value.
  * `resolve()` / `Display` reconstruct the string without allocation where
    possible.
  * Thread-safe global interner (`RwLock` + fast path) with an explicit
    `Interner` type also available for tests.
  * Graceful fallback when a level table overflows (never panics).
  * Per-level tables use small-string inline storage where cheap.
* `model` module: `ValueFormat`, `FeatureValue` (arbitrary width bit vector,
  inline up to 256 bits, heap beyond), `SetFasmFeature`, `Annotation`,
  `FasmLine`, exactly mirroring the Python namedtuples semantics.
* `parser` module: hand written, byte oriented, zero-copy line parser that
  implements the grammar from `docs/specification/syntax.rst` and matches the
  ANTLR + textX implementations bit for bit (including the accepted/rejected
  edge cases, `_` separators, whitespace, escapes, value/width checks).
  Streaming iterator API, callback API and `parse_fasm_string` /
  `parse_fasm_filename` convenience functions. Errors carry line/column and
  reproduce the Python error text.
* `output` module: `fasm_value_to_str`, `set_feature_width`,
  `set_feature_to_str`, `canonical_features`, `fasm_line_to_string`,
  `fasm_tuple_to_string`, `merge_features`, `merge_and_sort` (MergeModel).

### `fasm-xilinx` crate

* Database loader for prjxray-db / prjuray-db layouts (`settings.sh`,
  `<fabric>/tilegrid.json`, `segbits_*.db`, `ppips_*.db`, `mask_*.db`,
  `<part>/part.yaml`, `<part>/part.json`, `<part>/package_pins.csv`).
* Optional binary cache of a loaded part database (content hashed) so repeat
  runs are fast.
* `FasmAssembler` equivalent of `prjxray.fasm_assembler` +
  `xc_fasm.fasm2frames` (ROI, required features, PUDC_B pullup, STEPDOWN
  propagation, sparse/zero fill), producing frames.
* Bitstream writer equivalent of prjxray `xc7frames2bit` for Series7,
  UltraScale and UltraScale+ (prjuray) architectures, and a bitstream reader
  (bit -> frames) for verification.
* Everything keyed by `IdString`, no per-feature heap allocation in the hot
  path.

### `fasm-cli` crate

* `fasm` : byte for byte compatible with `python -m fasm.tool` /
  `fasm` entry point (argparse help text, `--canonical`, `--parser`,
  `Error: ...` on stdout with exit code 0, trailing blank line from
  `print()`).
* `fasm2frames`, `xc7frames2bit`, `xcfasm`: compatible with the f4pga-xc-fasm
  and prjxray tools so they can be dropped into existing flows
  (openXC7 / LiteX / f4pga).

### `fasm-python` crate + `fasm` Python package

* `fasm._fasm_rs` pyo3 module. `fasm/parser/rust.py` exposes
  `parse_fasm_filename`, `parse_fasm_string`, `implementation = 'rust'`.
* `fasm.parser.available` becomes `['rust', 'textx']` (antlr removed; the
  `--parser antlr` CLI option keeps working as an alias for the Rust parser).
* Public functions in `fasm/__init__.py` and `fasm/output.py` remain pure
  Python namedtuple based for compatibility; the Rust module also offers fast
  paths (`fasm_tuple_to_string`) used when available.
* `fasm.xilinx` module: Python access to database loading, `fasm2frames` and
  bitstream writing.
* Built with maturin (`pyproject.toml`), wheels for Linux at least.

### `fasm-capi` crate

* `libfasm` (cdylib + staticlib), header generated with cbindgen and checked
  in at `include/fasm/fasm.h`.
* C++ RAII wrapper `include/fasm/fasm.hpp` (header only).
* Examples and tests compiled with CMake in `rust/fasm-capi/tests`.

## Testing strategy

1. **Rust unit tests** in every crate; property tests for parser/printer
   round trips; fuzz-derived regression corpus.
2. **Oracle differential tests**: the original Python implementation
   (textX parser, and ANTLR parser when it can be built) is installed into a
   venv by `tests/oracle/setup.sh`. `tools/difftest.py` runs both the Rust
   CLI/library and the oracle over the whole corpus and diffs the results
   (parse trees, `fasm_tuple_to_string`, canonical output, errors).
3. **Frames/bitstream differential tests**: reference `fasm2frames.py`
   (f4pga-xc-fasm + prjxray python) and `xc7frames2bit` (prjxray C++) are
   built by `tests/oracle/setup-xilinx.sh`; `tools/difftest-xilinx.py`
   compares `.frm` and `.bit` outputs over the corpus for every part with a
   database.
4. **Corpus sources** (`tests/corpus/`, large files stored compressed):
   * repo `examples/*.fasm`;
   * f4pga-xc-fasm `tests/test_data`;
   * VTR `utils/fasm/test` golden files and any VTR regression designs that
     go through `genfasm` with the f4pga arch definitions;
   * nextpnr-xilinx (openXC7) outputs for f4pga-examples and
     fpgas.online-test-designs Xilinx designs;
   * RapidWright generated FASM (if RapidWright can emit it) or RapidWright
     bitstream comparisons;
   * prjxray/prjuray fuzzer style synthetic FASM covering every segbits tag
     of every tile type in the databases (generated by `tools/gen-corpus.py`).
5. **Toolchain based end-to-end tests** (`tools/e2e/`): scripts that install
   openXC7 (snap image), OSS CAD Suite (yosys), LiteX, the f4pga flow, VTR;
   build the example designs, produce FASM, then run the Rust tools and
   compare frames/bitstreams against the reference tools.
6. **Performance benchmarks** (`cargo bench`) for parsing and assembling the
   largest corpus files; documented in `docs/rewrite/BENCHMARKS.md`.

## Phases

See `TASKS.md` for the fine grained list. Summary:

* Phase 0: bookkeeping (plan, workflow, task list, log, workspace skeleton,
  CI).
* Phase 1: `fasm` core crate (idstring, model, parser, output) with unit
  tests and oracle differential tests.
* Phase 2: `fasm` CLI, 100% compatible, with CLI differential tests.
* Phase 3: Python bindings + package integration + tests.
* Phase 4: C and C++ wrappers + tests.
* Phase 5: `fasm-xilinx` database loader + cache + assembler + bitstream
  writer/reader for 7 series; differential tests versus reference tools.
* Phase 6: UltraScale / UltraScale+ (prjuray) support.
* Phase 7: Toolchain end-to-end tests (openXC7, f4pga-examples, fpgas.online
  designs, VTR, RapidWright).
* Phase 8: Performance work, benchmarks, documentation, packaging, cleanup.

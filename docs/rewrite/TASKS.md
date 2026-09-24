# Task list

Status legend: `[ ]` todo, `[~]` in progress (branch named), `[r]` in review,
`[x]` done (merged), `[-]` dropped (reason in LOG.md).

Each task must be small enough for one sub-agent session and end with a
reviewed merge. Dependencies are listed as `(after Tn)`.

## Phase 0: Bookkeeping and skeleton

- [x] T0.1 Sync fork `master` with upstream chipsalliance/fasm and merge into
      the working branch.
- [x] T0.2 Write PLAN.md, WORKFLOW.md, TASKS.md, LOG.md, root AGENTS.md.
- [x] T0.3 Cargo workspace skeleton: root `Cargo.toml`, empty crates `fasm`,
      `fasm-cli`, `fasm-xilinx`, `fasm-capi`, `fasm-python` with
      `cargo build --workspace` and `cargo test --workspace` passing;
      `rust-toolchain.toml`; `.gitignore` for `target/`; `deny.toml`/
      `clippy` config; `Makefile` targets `rust-build`, `rust-test`,
      `rust-lint`.
- [x] T0.4 Oracle setup: `tests/oracle/setup.sh` creates a venv with the
      original Python package (textX parser always, ANTLR parser when the
      C++ build succeeds) from the git history (`git worktree` of the
      pre-rewrite commit) so it can be used as a golden reference.
      `tests/oracle/run_fasm.py` wrapper. Document in `tests/oracle/README.md`.
- [ ] T0.4b Investigate the reported flakiness of the ANTLR extension build in
      `tests/oracle/setup.sh` (T5.8 implementer saw it succeed once and fall
      back to textX on another identical run); make the build deterministic
      or fail loudly (after T0.4).
- [x] T0.5 CI: GitHub Actions workflow for `cargo fmt --check`, `clippy`,
      `cargo test`, Python tests. (Keep the existing Python workflows
      working until Phase 3 replaces them.)

## Phase 1: Core library crate `fasm`

- [x] T1.1 `idstring` module (design doc `docs/rewrite/DESIGN-idstring.md`
      first, then implementation + unit tests + micro benchmarks).
- [x] T1.1b idstring follow ups from review (optional items): strengthen
      `concurrent_lookups_while_tables_grow` into a positive publication test;
      fast path for repeat interning of overflowed names (check overflow table
      before locking); `# Panics` docs on `Ord`/`Display`/`PartialEq<str>`
      about the GLOBAL interner and private interner handles; rename or
      loudly document `IdString::get` as not a membership test; document that
      raw handle values are insertion order dependent; zero allocation test;
      `Display` padding test (after T1.1).
- [x] T1.2 `model` module: `ValueFormat`, `FeatureValue` (arbitrary width),
      `SetFasmFeature`, `Annotation`, `FasmLine` + tests.
- [x] T1.3 `parser` module: tokenizer + line parser implementing the full
      grammar with exact ANTLR/textX compatible acceptance and error
      behaviour; streaming and whole-file APIs; tests from
      `examples/*.fasm` and a hand written edge case table (after T1.2).
- [x] T1.4 `output` module: string formatting, canonicalisation,
      `merge_features`, `merge_and_sort` (MergeModel) with tests ported from
      Python behaviour (after T1.2).
- [r] T1.5 Corpus v1 in `tests/corpus/` (repo examples, f4pga-xc-fasm test
      data, VTR `utils/fasm/test` files, synthetic edge cases) plus
      `tools/difftest.py` comparing Rust parse/print/canonical output with
      the oracle for every corpus file (after T0.4, T1.3, T1.4).
- [ ] T1.6 Fuzzing (`cargo fuzz` or proptest) for parser/printer round trip;
      crashes fixed; regression files added to the corpus (after T1.3).

## Phase 2: Command line tool

- [x] T2.1 `fasm` binary: argparse compatible parsing, identical `--help`
      text, identical output/exit behaviour (including `Error: ...` on
      stdout) (after T1.4).
- [x] T2.2 CLI differential test: run oracle `fasm` and Rust `fasm` over the
      corpus with every option combination; compare stdout, stderr, exit
      code byte for byte (after T2.1, T1.5).

## Phase 3: Python bindings

- [~] T3.1 `fasm-python` pyo3 crate producing `fasm._fasm_rs` with
      `parse_fasm_string`, `parse_fasm_filename` returning the existing
      namedtuples; maturin `pyproject.toml`; `fasm/parser/rust.py`;
      `fasm.parser.available` update; keep textX fallback (after T1.4).
- [ ] T3.2 Replace setup.py/ANTLR build with maturin based build; update
      `tests/test_simple.py` and add parity tests between `rust` and `textx`
      parsers over the corpus; `tox.ini`; wheel build workflow (after T3.1).
- [ ] T3.3 Fast paths: Rust backed `fasm_tuple_to_string` and
      `merge_and_sort` exposed and tested for identical results (after T3.1).

## Phase 4: C and C++ wrappers

- [ ] T4.1 `fasm-capi` crate: C ABI for parsing (callback + array APIs),
      formatting, canonicalisation; `include/fasm/fasm.h` via cbindgen;
      C test program built with CMake (after T1.4).
- [ ] T4.2 C++ header only wrapper `include/fasm/fasm.hpp` with RAII types
      and iterators; C++ test program; install rules; pkg-config file
      (after T4.1).

## Phase 5: Xilinx 7 series bitstream generation

- [x] T5.1 Database format study: `docs/rewrite/DESIGN-xilinx-db.md`
      describing prjxray-db and prjuray-db layouts, segbits/ppips/mask
      formats, part.yaml, tilegrid.json bits blocks, ROI, required
      features, and how `fasm_assembler.py` + `fasm2frames.py` +
      `xc7frames2bit` behave (after T0.3).
- [ ] T5.2 `fasm-xilinx`: database loader (tilegrid, segbits, ppips, part
      yaml/json, package pins) with tests on a checked in miniature database
      (from f4pga-xc-fasm test data) (after T5.1).
- [ ] T5.3 Binary cache for a loaded part database (versioned, content
      hashed, memory mappable) + `fasm-db-cache` maintenance subcommand
      (after T5.2).
- [ ] T5.4 Frame assembler: `FasmAssembler` (feature lookup, multi bit
      features, `!` cleared bits, pseudo pips, unknown feature errors, sparse
      vs full frames, ROI, required features, PUDC_B, STEPDOWN) (after T5.2).
- [ ] T5.5 `.frm` writer/reader and `fasm2frames` compatible CLI; tests vs
      reference `.frm` files from f4pga-xc-fasm test data (after T5.4).
- [ ] T5.6 Series7 bitstream writer (`xc7frames2bit` compatible: header,
      packets, CRC, part idcode) + reader; `xc7frames2bit` compatible CLI;
      tests vs prjxray test bitstreams (after T5.5).
- [ ] T5.7 `xcfasm` compatible one shot CLI (fasm -> bit) (after T5.6).
- [x] T5.8 Reference tool setup `tests/oracle/setup-xilinx.sh` (prjxray
      python package, f4pga-xc-fasm, prjxray C++ tools build) and database
      fetch script `tools/fetch-db.sh` (sparse clone per family) (after T0.4).
- [ ] T5.9 `tools/gen-corpus.py`: synthetic FASM exercising every segbits
      feature of every tile type in a database; `tools/difftest-xilinx.py`
      comparing frames and bitstreams against the reference tools for every
      artix7/kintex7/spartan7/zynq7 part in prjxray-db (after T5.6, T5.8).
- [ ] T5.10 Python bindings for `fasm-xilinx` (`fasm.xilinx`) and C API
      entry points (after T5.6, T3.1, T4.1).

## Phase 6: UltraScale and UltraScale+

- [ ] T6.1 prjuray-db support in the loader (differences documented in
      DESIGN-xilinx-db.md) (after T5.2).
- [ ] T6.2 UltraScale / UltraScale+ bitstream writer and reader (prjuray
      `frames2bit`), CLI flags (`--architecture`), tests vs prjuray tools
      (after T6.1, T5.6).
- [ ] T6.3 Differential tests for every part in prjuray-db (after T6.2).

## Phase 7: Toolchain end-to-end compatibility

- [x] T7.1 `tools/e2e/setup-openxc7.sh`: fetch openXC7 snap (unsquashfs) or
      build nextpnr-xilinx + chipdb for the needed parts; yosys from OSS CAD
      Suite; document versions.
- [ ] T7.2 fpgas.online-test-designs: build every Xilinx design (LiteX +
      openXC7), collect FASM into the corpus, run Rust `fasm2frames` /
      bitstream and compare against the reference flow (after T7.1, T5.9).
- [ ] T7.3 f4pga-examples: set up the f4pga (VPR based) flow, build all xc7
      examples, collect FASM, compare frames/bitstreams (after T7.1, T5.9).
- [ ] T7.4 VTR: build VTR `genfasm`, run the VTR regression designs that can
      produce FASM (f4pga arch defs + VTR `utils/fasm/test`), add to corpus
      (after T1.5).
- [ ] T7.5 RapidWright: determine FASM/bitstream export capability, generate
      reference data for the corpus designs, compare (after T5.6).
- [ ] T7.6 nextpnr-xilinx (openXC7) test designs beyond the two repos:
      `nextpnr-xilinx/xilinx/examples` designs (after T7.1).

## Phase 8: Performance, docs, packaging

- [ ] T8.1 Benchmarks (`cargo bench`) and `docs/rewrite/BENCHMARKS.md` with
      comparison against Python textX/ANTLR and fasm2frames.py.
- [ ] T8.2 Optimise hot paths found in T8.1 (parser SIMD scanning, database
      cache, frame assembly). Known: on pip heavy FASM the parser runs at
      150-164 MB/s (target 200) with ~48% of instructions in idstring
      interning and ~9% in UTF-8 validation of names; speed up the interner
      hit path and validate names byte-wise (they are ASCII by grammar).
- [ ] T8.3 Documentation: README update, crate docs, Python docs, C/C++ docs,
      `docs/rewrite/COMPAT.md`.
- [ ] T8.4 Packaging: crates.io metadata, maturin wheels workflow, CMake
      install for the C API, release notes.

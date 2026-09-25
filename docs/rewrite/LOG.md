# Progress log

Append only. Newest entries at the bottom. Each entry: date, task ids,
what happened, branch/commit references, open issues.

## 2026-09-24

* T0.1: Added `upstream` remote (chipsalliance/fasm), fast-forwarded
  `master` by 50 commits to `ffafe82` (Apache-2.0 relicense, ANTLR 4.9.3,
  docs restructure), merged into `claude/epic-goldberg-uc7xqf`.
* Environment survey: cargo 1.94.1, python 3.11, cmake, gcc/clang, java are
  available. pip and crates.io reachable through the proxy. GitHub reachable
  for clones and release downloads; api.github.com is blocked. `apt-get`
  works (used for squashfs-tools). No yosys/nextpnr/vpr installed.
* Reference repositories cloned into the session scratchpad (not committed):
  mithro/idstring, lromor/fpga-assembler, chipsalliance/f4pga-examples,
  fpgas-online/fpgas.online-test-designs, chipsalliance/f4pga-xc-fasm,
  f4pga/prjxray, f4pga/prjxray-db (sparse, artix7 only, 181 MB).
* Findings that shape the plan:
  * Neither f4pga-examples nor fpgas.online-test-designs contain FASM files;
    they are Verilog/LiteX designs that need a synthesis + place and route
    flow (VPR based f4pga flow, or yosys + nextpnr-xilinx from openXC7) to
    produce FASM. openXC7 publishes x86_64 snap images
    (openXC7/openXC7-snap releases) that can be extracted with unsquashfs.
  * prjxray-db layout: `<family>/<fabric>/tilegrid.json`,
    `<family>/segbits_<tile_type>.db`, `ppips_*.db`, `mask_*.db`,
    `<family>/<part>/{part.yaml,part.json,package_pins.csv}`; part.json names
    the fabric.
  * f4pga-xc-fasm `tests/test_data` contains a miniature database plus
    `.fasm` inputs and expected frames: ideal for checked in unit tests.
  * The Python `fasm` CLI prints `Error: ...` to stdout and exits 0 on
    parse errors, and prints an extra blank line after the output (from
    `print`). The Rust CLI must reproduce this exactly.
* T0.2: wrote PLAN.md, WORKFLOW.md, TASKS.md, LOG.md, AGENTS.md.
* Push to origin currently fails with HTTP 403 (Claude GitHub App not
  authorised for mithro/fasm in this session). Work continues locally;
  pushes are retried after each merge.
* T0.3 and T0.4 started (sub-agents in isolated worktrees).
* Corpus research: VTR keeps its FASM tests in `utils/fasm/test`
  (test_fasm.cpp generates FASM from `test_fasm_arch.xml` + `wire.eblif`
  at test time; no golden `.fasm` files are stored). RapidWright's public
  repository has no FASM writer and no public bitstream package, so the
  RapidWright path for T7.5 will be RapidWright -> FPGA interchange physical
  netlist -> python-fpga-interchange `fasm_generators` (xc7) -> FASM.
* T0.3 done: Cargo workspace skeleton (branch
  `worktree-agent-ad78092928b7f4c17`, 4 commits) reviewed (APPROVE, no
  required changes) and merged with --no-ff. `cargo build/test/fmt/clippy`
  all pass on the merged tree. Optional reviewer nits (double blank line in
  Makefile, redundant `[lib] name` in rust/fasm/Cargo.toml) left for a later
  cleanup commit.
* T1.1 (idstring design + implementation) started.
* T0.4 implemented (branch `worktree-agent-a0d3c2061a4ef0f32`): oracle venv
  with BOTH original parsers (ANTLR C++ extension built fine here, textX).
  Review requested one change: the oracle must be installed from a pinned
  pre-rewrite commit (ffafe82) via a detached worktree, not an editable
  install of the live tree. Fix in progress.
* Reviewer also found a pre-existing divergence between the original
  parsers: annotation values containing an escaped quote (`\"`) parse with
  ANTLR but fail with textX (`fasm.tx` ordered choice matches a lone `\`
  first). The corpus (T1.5) must include this case and COMPAT.md must
  record which behaviour the Rust parser follows (ANTLR).
* T0.4 done: oracle fix commit 03b4f7e re-reviewed (APPROVE), branch merged
  with --no-ff (one trivial .gitignore conflict resolved by keeping both
  hunks). `tests/oracle/setup.sh` run in the main tree: both ANTLR and textX
  parsers available, pinned to ffafe82, 9 oracle tests pass. Optional
  suggestion left open: surface build/worktree.log when `git worktree add`
  fails.
* T1.1 implemented on branch `worktree-agent-a1361d4f5c2229f2a` (11 commits,
  design doc docs/rewrite/DESIGN-idstring.md). Implementer chose 3 levels
  with 24/20/20 bit indexes (measured on the full xc7a200t feature space:
  46,611 tiles, 6,888 / 7,790 entries on the other levels) and a lock free
  sharded index (foldhash only runtime dependency). Sent for independent
  review.
* T5.1 (Xilinx database format study) started.
* T5.1 written on branch `worktree-agent-a17f8eb0ad8fd6a96` (3 commits,
  docs/rewrite/DESIGN-xilinx-db.md, 1749 lines). Key findings: the
  UltraScale Python/C++ core lives in SymbiFlow/prjuray-tools, not prjuray;
  UltraScale+ frame address layout and 48 bit ECC (words 45/46) differ from
  Series7 (13 bit ECC in word 50); prjxray's bitstream writer never computes
  a CRC; row padding is 2 zero frames; sparse and dense `.frm` inputs give
  identical `.bit` output; prjuray-db only ships `zynqusp` (2 parts), keyed
  per part, without ppips/mask files or iobanks. Sent for fact checking
  review.
* T5.1 done: fact check review verified 40+ claims, found 2 errors (UltraScale
  NOP count; prjuray fasm_assembler.py path) and 1 gap (ECC word 50 vs HCLK
  tiles) which were corrected (commits f8aac80, e1998d4; invariant test
  added as item 19 of §8.3). Merged with --no-ff.
* T5.8 (reference Xilinx tools + database fetch scripts) started.
* T1.1 done: Opus review (APPROVE, no required changes) ran the tests per
  commit, a scratch stress crate (publication test under weak memory,
  mixed overflow ordering, counting allocator: zero allocations on hit /
  get / with_str / Display / cmp) and Miri on the lock free index; no race,
  data loss or user reachable panic found. Merged with --no-ff; 44 tests
  pass, fmt/clippy clean. Optional suggestions recorded as T1.1b.
* T1.2 (model module) started.
* T5.8 implemented on branch `worktree-agent-a494eff241535ec3f` (4 commits):
  setup-xilinx.sh builds prjxray AND prjuray-tools C++ tools (all targets),
  venv-xilinx with pinned original fasm + prjxray + xc-fasm + prjuray,
  wrappers, tools/fetch-db.sh (artix7 181 MiB in ~6 s, zynqusp 217 MiB),
  smoke corpus tests/corpus/xilinx/artix7/smoke_x1y0.* with golden .frm/.bit.
  Pins: prjxray c9f02d8, f4pga-xc-fasm 25dc605, prjuray c550b03,
  prjuray-tools f53f07b, prjxray-db 0a0adde, prjuray-db affbc5e. Note:
  `.bit` headers embed build time and the frm path, so bit files are not
  byte reproducible; tests compare `.frm` and bitread output. Sent for
  review. Implementer reported the base oracle ANTLR build is flaky across
  runs: recorded as T0.4b.
* T1.2 implemented on branch `worktree-agent-a7483256a8644b3ab` (5 commits,
  docs/rewrite/DESIGN-model.md): FeatureValue inline [u64;4] / heap enum
  with canonical representation (40 bytes), SetFasmFeature 72 bytes,
  FasmLine 112 bytes, 126 tests incl. proptest vs num-bigint. Sent for
  review.
* T5.8 done: review APPROVE (reviewer regenerated the smoke `.frm` and
  bitread output byte identically, confirmed the venv's fasm comes from the
  pinned pristine tree, confirmed clean skips without the db). Merged with
  --no-ff; setup-xilinx.sh run in the main tree (183 s, prjxray and
  prjuray-tools C++ tools built), artix7 db fetched (181 MiB), 4 smoke tests
  pass. Reviewer's evidence on the ANTLR flakiness (T0.4b): setup.py swallows
  the CMake build failure (`except BaseException`) so nothing is logged;
  likely resource contention during the unbounded `-j` build. Optional:
  make the `test_badkey` deselect conditional on `antlr_built`.
* T1.2 review: REQUEST CHANGES. Real bug: unchecked u32 arithmetic in
  `SetFasmFeature::new`/`width()` overflows for `[4294967295:0]`. Also wrong
  citations (`fasm/__init__.py`, not `fasm/output.py`). Reviewer confirmed
  a textX vs ANTLR divergence the parser (T1.3) must decide on: textX
  rejects a declared width wider than the address width (`a.b[0] = 3'b001`),
  ANTLR accepts it. Fix in progress.
* T7.1 (openXC7 toolchain setup script) started.
* T1.2 done: fixes 4771f31 (checked width arithmetic, new
  `ModelError::AddressRangeTooWide`, 4 tests) and 5ed2f07 (citations, shl
  doc note) verified by the orchestrator; merged with --no-ff. 137 tests
  pass, fmt/clippy clean.
* T1.3 (parser) started. Decision for the parser, to be recorded in
  COMPAT.md: follow the ANTLR parser where textX and ANTLR disagree
  (escaped quotes in annotation values, declared width wider than the
  address width, `_` in plain decimals) except where ANTLR is plainly buggy
  (32 bit truncation of addresses/plain values), and document every case.
* T7.1 implemented on branch `worktree-agent-ab7be3a18223e07a8` (4 commits):
  openXC7 snap 0.8.2 (nextpnr-xilinx, bbasm, prjxray tools + bundled
  prjxray-db for artix7/kintex7/spartan7/zynq7) made runnable without snapd
  by patchelf-ing the interpreter and sed-fixing the python wrappers; yosys
  from OSS CAD Suite 2026-09-21; ~4.7 GiB total. Chipdb for xc7a35tcsg324-1
  built in ~12 s. First end-to-end design (f4pga-examples counter_test on
  Arty 35T) runs in 5.4 s and produced a deterministic 781 line FASM, now in
  tests/corpus/xilinx/artix7/designs/f4pga-examples/counter_test/arty_35/.
  Blocker for large parts: `bbaexport.py` for xc7a200t exceeded 8 GiB RAM
  and 4 minutes on this 4 core / 15 GiB machine (killed); xc7a100t not
  measured. `nextpnr-xilinx --test` fails an internal assert on the built
  chipdb but the real flow works. Sent for review.
* T1.3 implemented on branch `worktree-agent-ab081dee5b7bd884f` (5 commits):
  hand written parser (`parser/{mod,error,line,number}.rs`), streaming
  `parse_lines` iterator, `parse_line`, whole file APIs; 169 case edge table
  checked against the oracle with 0 mismatches for the "Same" cases; golden
  ANTLR parse trees of examples/*.fasm in tests/corpus/oracle/*.json;
  204-285 MB/s on a 100 MB synthetic file (interning is ~40% of the time);
  docs/rewrite/COMPAT.md created with all divergences (notably: `_` in plain
  decimals accepted; ANTLR bugs fixed: 31 bit plain values, `'d` > 2^32
  truncation, octal > 32 bit, whitespace after `'h` decoded as a digit,
  address truncation; Rust rejects `a[0:1] = 0` which both originals
  accept; BOM skipped like ANTLR). Sent for Opus review with an adversarial
  differential corpus.
* T7.1 done: review found `--parts DEVICE` (space form) was documented but
  not accepted, and `--force --force` was a no-op; fixed in 586d1ca (while/
  shift parser accepting both forms, FORCE counter). Merged with --no-ff.
  Toolchain installed in the main tree (110 s with cached downloads, 4.7
  GiB), counter flow reproduces the corpus FASM sha256 exactly, 4 e2e
  smoke tests pass.
* Housekeeping: removed the worktrees of all merged agent branches
  (branches kept); disk went from 6.7 GB to 14 GB free. Worktree disk use
  is a real constraint on this machine: toolchains and venvs should be
  built in the main tree, not per worktree, wherever possible.
* T1.4 (output module) started.
* T1.4 implemented on branch `worktree-agent-a9a6c3f4a5ecb9b14` (6 commits,
  docs/rewrite/DESIGN-output.md, golden outputs for examples/many.fasm in
  tests/corpus/oracle/). All Python asserts became `Result` errors.
  Implementer found and deliberately reproduced a Python bug:
  `MergeModel.add_to_comment_group` does not reset `current_group` after a
  feature line ends a comment group, so the group can be emitted twice
  (verified against the oracle). Sent for differential review.
* T1.3 review (Opus, 5092 file differential corpus): REQUEST CHANGES.
  Findings: quadratic decimal conversion running before the width check
  (10 MB value never finished; both originals cap decimals at 4300 digits),
  unbounded error messages embedding huge values, ANTLR's annotation mode
  lexer lookahead error positions not reproduced (6 cases), and COMPAT.md
  inaccuracies (rule 3 modulo relaxations, octal > 10 digits mis-decoding
  in ANTLR, NUL handling, `\r` in annotation values). No panics in 3M
  random inputs; 253-289 MB/s. Fixes in progress.
* T1.4 review (~13,900 differential cases vs the oracle): REQUEST CHANGES
  with one finding: `write_set_feature` panics (via `width()`) on malformed
  features instead of returning OutputError. Also flagged: the linear scan
  in `merge_addresses` is O(G^2) in distinct feature names (fast follow
  needed before full chip use), and the CLI will need a small
  fmt::Write-over-io::Write adapter. Fix in progress.
* T1.4 done: fixes 0f50186 (checked width validation in write_set_feature,
  shared EndWithoutStart/EndBeforeStart/AddressRangeTooWide errors, O(1)
  HashMap index for merge_addresses keeping insertion order, 500 name
  test) and 15c8dd8 (canonical line/tuple paths use try_canonical_features).
  Orchestrator re-ran the reviewer's harness: all 11 suites 0 mismatches.
  Merged with --no-ff; 205 tests pass, fmt/clippy clean; worktree removed.
* T0.5 (CI workflow) started.
* T1.3 fixes landed (6 commits): linear time huge values with a 4300
  significant digit decimal cap, short error messages, ANTLR annotation
  mode lookahead error positions reproduced (35 more edge cases, 210 total,
  0 mismatches vs oracle), Lines::line_number, blank line fast path,
  allocation test, pip heavy bench, COMPAT.md corrections. Reviewer harness
  re-run by implementer: 0 unexplained of 5092. Pip heavy input parses at
  150-164 MB/s (below the 200 target; interning dominates) recorded under
  T8.2. Sent back to the reviewer for re-review.
* T0.5 implemented on branch `worktree-agent-a8af0fc27a5fcfc7c` (4 commits):
  .github/workflows/rust.yml (lint, test on 3 OSes + doc, msrv 1.88.0,
  oracle job building the Python oracle), dependabot, workspace
  rust-version = 1.88 (as_chunks needs 1.88), Makefile rust-doc/rust-check.
  Sent for review.
* T0.5 done: review APPROVE (actionlint clean, MSRV verified, cache path
  reasoning checked). Merged with --no-ff; worktree removed. Noted: `cargo
  doc` warns about the output filename collision between the `fasm` binary
  and the `fasm` lib (cargo bug 6313); harmless, consider renaming the doc
  target later.
* T1.1b (idstring follow ups + hit path speed) started.
* T1.3 re-review: annotation lookahead emulation verified on 1444 files (0
  different from ANTLR), corpus 0 unexplained, stress inputs all fast except
  ONE remaining quadratic case: decimals with millions of leading zeros on a
  wide address (heap limb sizing counts leading zeros). Fix requested.
* T1.3 done: final fix 2cc3224 (linear time leading zeros; 10 MB of zeros now
  48 ms instead of 226 s), 4dcf3cc (validate input as UTF-8 once; realistic
  file 235 MB/s, pip heavy 156-164 MB/s), 8393d01 (COMPAT.md). Orchestrator
  rebuilt the reviewer's harness on the final branch: 5092 files, 0
  unexplained, parse trees identical to the previously reviewed run. Merged
  with --no-ff (lib.rs re-export conflict with the output branch resolved
  by keeping both); 228 tests pass, fmt/clippy/doc/MSRV clean; worktree
  removed.
* T2.1 (compatible `fasm` CLI) started.
* T1.1b implemented on branch `worktree-agent-a525263a33b581c6e` (10 commits):
  intern hit 82 -> 53 ns (1.55x), 8 threads 36-52 -> 25-34 ns, lookup 80 ->
  50 ns, memory unchanged; `get` renamed to `lookup`; publication test;
  lock free re-intern of overflowed names; Panics docs; alloc test; Display
  test. Thread local cache tried and rejected (measured slower). Parser
  bench with new idstring: pips 168 -> 184 MB/s (interning now 26% of
  instructions); remaining gap is the parser itself (T8.2). In review.
* T2.1 implemented on branch `worktree-agent-a019d02c283a0a548` (4 commits):
  argparse emulation following CPython 3.11 step by step (prefixes, `=`,
  `--`, repr() based errors, help wrapping at terminal width), Python string
  behaviour for non UTF-8 argv, unicode tables generated from the oracle's
  Python, ANTLR error precedence emulation, `Error:` on stdout with exit 0.
  Differential test tests/cli/test_cli_compat.py: 692 cases, 0 differences
  (3 documented normalisation rules for error message texts). 200k lines
  in 0.09 s vs 3.6 s for the original. In review.
* T1.1b done: Opus review APPROVE (intern_bytes soundness argument checked,
  stress crate + Miri re-run, per-commit bisectability). Merged with
  --no-ff; 235 tests pass. Parser bench on the merged tree: mixed 224 cold /
  276 warm MB/s, pip heavy 164 cold / 174 warm MB/s.
* T1.5 (corpus v1 + Rust vs oracle differential test tool) started.
* T2.1 review (Opus, 1,368 hand written command lines incl. 408 pty width
  cases + 6,000 fuzz runs, 40k panic fuzz runs): REQUEST CHANGES for two
  undocumented differences: `COLUMNS` with > 4300 digits (Python int()
  limit) and `-h` with stdout closed (Python prints help to stderr). All
  other differences are documented. Measured: 11 MB file, plain output
  0.15 s / 35 MB RSS vs 5.7 s / 1.08 GB for the original; canonical 1.6 s /
  304 MB vs 14.3 s / 1.2 GB. Fixes in progress.
* T2.1 done: fixes 30e8052/6010c23 (int() 4300 digit limit for COLUMNS),
  cd2dcd1 (closed stdout test), c8bd28f/e07484c (COMPAT.md, generator
  doc). Merged with --no-ff; 264 Rust tests pass; CLI difftest in the main
  tree: 715 passed, 0 differences. T2.2 (CLI differential test) is covered
  by tests/cli/test_cli_compat.py and `make cli-difftest`, so marked done.
* T3.1 (Python bindings) started.
* T1.5 implemented on branch `worktree-agent-ae95e0887b836dfce` (8 commits):
  corpus v1 (examples, f4pga-xc-fasm, VTR literals, synthetic edge/invalid
  cases from tools/gen-corpus.py; 600 KB), rust/fasm/examples/dump.rs
  (same JSON as dump.py), tools/difftest.py with COMPAT.md based classifier
  (70 files, 0 unexplained), `make difftest`, pytest wrapper. In review.
* T1.5 done: review APPROVE (classifier verified honest with three injected
  deviation experiments, dump.rs byte identical to dump.py, provenance
  checked against the reference checkouts, generator deterministic). Merged
  with --no-ff; `tools/difftest.py --jobs 4` in the main tree: 70 files, 0
  unexplained. Optional follow ups: cover the remaining COMPAT.md rows in
  gen-corpus.py and commit one small `.fasm.xz` to exercise decompression.
* T4.1 (C API) started.
* Orchestrator error: merge commit fc5cb8d (T1.5) was recorded with an
  unresolved Makefile conflict (the `cli-difftest` vs `difftest` targets);
  fixed in the next commit keeping both targets. Lesson recorded in
  WORKFLOW.md: never chain `git commit -a` after a merge without checking
  its exit status.
* T3.1 implemented on branch `worktree-agent-a7576ae0294e12fea` (11 commits):
  pyo3 0.29 `fasm._fasm_rs` (abi3-py39, GIL released, cyclic GC paused
  while building big results), returns the existing namedtuples, `rust`
  parser default with textX fallback, maturin packaging (static version
  0.1.0.dev0, TODO T3.2), `FasmParseError`, fast path
  `_fasm_rs.fasm_tuple_to_string`. Benchmarks: 100k line file 66 ms (Rust)
  vs 1.08 s (ANTLR) vs 22.4 s (textX); 781 line design 0.3 ms vs 3.5 ms.
  Noted: the plain cargo workspace build now needs a Python interpreter
  (consider excluding fasm-python from default-members); the error
  precedence emulation lives in fasm-cli only (Python reports the first
  error): documented in COMPAT.md, candidate follow up to move it into
  fasm::parser. In review.
* T4.1 implemented on branch `worktree-agent-ae7a5f569464f533b` (8 commits):
  44 exported `fasm_*` functions (status + `fasm_error**` pattern,
  catch_unwind everywhere, NULL safe), include/fasm/fasm.h (921 lines)
  generated by cbindgen with a freshness test, C99 test program (359
  checks) built with CMake against both the shared and static library and
  run under valgrind (0 errors), make targets capi-header /
  capi-header-check / capi-test, DESIGN-capi.md. In review.
* T4.1 review (Opus): REQUEST CHANGES for three small items (Miri UB in a
  Rust unit test, sort key callback called O(n log n) times and a
  non-deterministic key panics, wrong "aborts" wording for foreign
  unwinds). NULL fuzzing of all 44 functions, self aliasing push under Miri,
  8 thread reads, byte identical output vs the CLI on all 70 corpus files.
  Reviewer also found a core hazard: canonical output of `W[4294967295:1]`
  loops ~4G times: recorded as T1.4b. Fixes in progress.
* T4.1 done: 8 fix commits (Miri clean tests, sort key cached once per
  group with (i64, IdString) keys, UB wording, COMPAT.md C API section,
  layout tests, CARGO_TARGET_DIR support). Merged with --no-ff; 279 Rust
  tests pass, `make capi-test` 4/4 incl. valgrind, header freshness test
  passes; worktree removed.
* T5.2 (fasm-xilinx database loader) started.
* T3.1 review (Opus): 9,644 file differential vs oracle and vs the Rust CLI,
  100k model fast path fuzz, leak/thread/GC pause checks: all clean (every
  divergence documented). REQUEST CHANGES for two packaging items: stale
  tracked `fasm/version.py` gets packaged from tarball builds; the new
  tests need `maturin develop` (cwd=ROOT shadowing). Also asked to add
  `default-members` so bare `cargo build` stays Python free. Fixes in
  progress. Note for T3.2: legacy wheel.yml/tox.ini/ANTLR files are now
  stale and must be removed/rewritten.
* T3.1 done: fixes 6f9d717 (version.py removed + maturin exclude), c6362ca
  (tests runnable against an installed package), 47b0223 (workspace
  default-members without fasm-python). Merged with --no-ff (COMPAT.md
  section conflict resolved explicitly). Integration issue found on the
  merged tree: the parity tests iterate the T1.5 synthetic corpus, which
  contains documented textX-only inputs; fixed by the orchestrator in
  tests/test_rust_parser.py (skip rust_stricter / invalid files, fast path
  must decline models Python asserts on). `maturin develop` in a scratch
  venv: available ['rust', 'textx'], implementation rust; Python tests
  pass (full run after the fix: 443 passed, 19 skipped). flake8 clean apart from the F401 in fasm/parser/__init__.py that
  tox.ini's per-file-ignores already allow.
* T5.2 implemented on branch `worktree-agent-a23111aea3549812f` (7 commits):
  Database::open for prjxray-db and prjuray-db, segbits/ppips/tilegrid/
  part.yaml (hand written subset parser)/part.json/package_pins readers,
  lookup in prjxray order (~23 ns split, ~100 ns from a whole name),
  FrameAddress + segbit_position, iter_frame_addresses (matches
  xc7frames2bit+bitread: 5408 frames xc7a35t, 24060 xc7a200t), ECC
  invariant (no violations on artix7/zynqusp), mini-db (15 KB) + synthetic
  db test data; xc7a50t opens in 91 ms / 24 MiB, xc7a200t 171 ms / 41 MiB
  (prjxray python: 0.53 s / 0.68 s). Findings for T5.4: negative bit
  offsets of _SING alias tiles wrap to the frame end in prjxray and the
  golden files depend on it; unknown tiles raise KeyError not
  FasmLookupError in prjxray. In review.
* T3.2 (maturin based build, legacy ANTLR removal, CI/tox/wheels) started.
* T5.2 done: Opus review APPROVE. Differential vs prjxray/prjuray Python:
  all tiles of xc7a50t/xc7a100t/xc7a200t/xczu3eg identical; 19.3M segbit
  position queries with 0 mismatches (incl. _SING start_offset, negative
  wrap, bits past the frame end); segbits/ppips tables identical; frame
  enumeration identical to xc7frames2bit+bitread for all 88 artix7 parts
  and both zynqusp parts; banks registry identical for all parts; 9000
  mutation fuzz iterations without panic; 0 allocations per lookup. Merged
  with --no-ff; worktree removed. Optional follow ups: serde_json in the
  public API of Grid::from_json_slice, Part::new visibility, workspace dep
  comment.
* T5.4 (frame assembler) started.
* T3.2 implemented on branch `worktree-agent-a88301b57c5b468a9` (6 commits):
  legacy ANTLR/Cython/CMake build, setup.py, MANIFEST.in, update_version.py,
  third_party submodules, conda Makefile targets and the four legacy Python
  workflows removed (grammars moved to docs/specification/antlr/); new
  .github/workflows/python.yml (lint, test 3.9/3.11/3.13, abi3 manylinux
  wheels x86_64+aarch64, sdist, trusted publishing on v* tags); tox.ini for
  tox 4; .flake8; README rewritten. Found and fixed a NameError in the
  missing-extension fallback path. In review.
* T3.2 done: review APPROVE after one doc fix (17152c8: grammar files now
  referenced and literalincluded from docs/specification/syntax.rst).
  Merged with --no-ff; on the merged tree: cargo tests pass, oracle tests
  9 passed (the oracle uses the pristine ffafe82 checkout), `maturin
  develop` gives ['rust', 'textx'], flake8 and check-license clean;
  worktree removed. Note: PyPI trusted publishing must be configured on
  pypi.org before a `v*` tag can publish.
* T4.2 (C++ header only wrapper) started.
* T4.2 implemented on branch `worktree-agent-a3e944fe1ed19b3ec` (4 commits):
  include/fasm/fasm.hpp (C++17 header only: Error, String, Value,
  SetFeature, Line, File with iterators, exception trampoline for
  callbacks, streaming parse_each), C++ test (203 checks) + header only
  compile matrix (g++/clang++ x C++17/20 -Werror), `make capi-install
  PREFIX=` with fasm.pc, pkg-config example. 12/12 ctest incl. valgrind.
  In review.
* T4.2 review: REQUEST CHANGES. Real bug: merge_and_sort's exception
  trampolines keep the LAST exception (the C zero/sort key callbacks have
  no early stop, so callbacks keep running); plus misleading trampoline
  docs, a missing test, and a broken static link recipe in fasm.pc.in
  (`--libs.private` is not a pkg-config option). Everything else verified
  (12/12 tests, -Wshadow -Wconversion clean, 5000 char names, self move,
  non-UTF-8 paths, pkg-config shared build). Fixes in progress.
* T4.2 done: fixes d4ea350 (first exception kept), 2fdcf2b (tests),
  f4bce2a (verified static/shared pkg-config recipes, docs), 9ce0806
  (check_license.sh scans .hpp/.pc.in). Merged with --no-ff; `make
  capi-test` 12/12 incl. valgrind and the compiler matrix; worktree
  removed. Phase 4 (C and C++ wrappers) complete.
* T3.3 (Python fast paths: fasm_tuple_to_string, merge_and_sort) started.
* Integration fix (orchestrator): the CLI difftest tests/cli had been
  failing on the T1.5 synthetic divergence corpus since the T2.1 merge (the
  earlier "715 passed" run predated the corpus); documented divergent
  classes and the invalid set are now skipped there (tools/difftest.py
  covers them). Result: 1132 passed, 12 skipped in 59 s (was 21 minutes
  with 149 failures). Noted: yapf 0.24.0 cannot parse
  tests/cli/test_cli_compat.py (control characters in string literals);
  CI's yapf job only covers fasm/ and tests/*.py.
* T5.4/T5.5 interim (branch `worktree-agent-a201f3154dd57871c`, 11
  commits): assembler + .frm + fasm2frames CLI done; mini-db fixtures and
  the counter design match the oracle .frm byte for byte (dense, sparse,
  PUDC_B); xilinx-difftest 69/69 identical; 1M line xc7a200t assembly 0.9 s
  vs 28 s (31x); counter design only 3-4x because of the eager database
  load (T5.3 cache will fix). Waiting for the final report.
* T5.4/T5.5 final (13 commits incl. merge of the main branch): assembler,
  Frames + .frm I/O, `fasm2frames` CLI with a generalised declarative
  argparse emulation shared by both binaries (fasm_cli library),
  tools/difftest-xilinx.py (69/69 identical), 100 fasm2frames CLI cases,
  tests/cli 1320 passed 12 skipped, 375 Rust tests. Sent for Opus review
  with an adversarial corpus.
* T3.3 implemented on branch `worktree-agent-ac5b9a0423fabdc2d` (5 commits):
  `fasm.fasm_tuple_to_string` uses the Rust fast path with Python fallback;
  new `_fasm_rs.merge_and_sort` (Python callables called once per group in
  first seen order, `<` based stable sort, declines before any callback on
  OutputError so Python raises the same AssertionError), wired into
  `fasm.output.merge_and_sort` (callbacks now run eagerly at call time:
  documented). 1447 Python tests pass; oracle parity 38/38 corpus files;
  100k lines: to_string 89 vs 127 ms, merge_and_sort 243 vs 520 ms. In
  review. Implementer saw the timing based parser test fail once under
  load: recorded as T1.3b.
* Housekeeping: the shared scratchpad reached ENOSPC (11 GB of finished
  review/implementation scratch); deleted finished scratch directories,
  17 GB free again. Reviewer briefs now cap and clean their scratch use.
* T3.3 done: review found only a documentation gap (tied sort keys may
  have `__lt__` called twice per comparison, unlike CPython's sort), fixed
  in b957be5 with a regression test. Merged with --no-ff; Python suite
  1011 passed on the merged tree; worktree removed. Phase 3 complete.
* T7.2 (fpgas.online-test-designs Xilinx designs via LiteX + openXC7)
  started.
* T5.4/T5.5 review (Opus): 974k generated feature lines in 132 runs, 108
  hand written edge case runs, 130 ROI runs, 410 fuzz runs, 1M line
  xc7a200t file (Rust 3.1 s / 412 MiB vs oracle 285 s / 1815 MiB): all
  .frm identical except two items: fasm2frames does not emulate ANTLR's
  parse error precedence (the fasm binary does), and ROI required_features
  given as a JSON object are joined in sorted instead of insertion order.
  Fixes requested; also COMPAT/difftest notes (STEPDOWN KeyError tile is
  PYTHONHASHSEED dependent in Python; parser level differences; explicit
  value range rule in difftest-xilinx.py; hot path allocation numbers).
* T5.4/T5.5 done: fixes 1842161 (parse error precedence in fasm2frames),
  15ed304 (ROI required_features object keeps file order), a12b8d3
  (warning line rendered once per feature), 1a1ec26 (tests), b1b16ae
  (docs). Orchestrator re-ran both reviewer repros: identical to the
  oracle. Merged with --no-ff; on the merged tree: 378 Rust tests, `make
  xilinx-difftest` 87/87 identical, tests/cli 1364 passed 12 skipped;
  worktree removed.
* T5.6 (Series7 bitstream writer/reader, xc7frames2bit compatible CLI)
  started.
* T7.2 implemented on branch `worktree-agent-a413aea60a910a8c3` (10 commits):
  LiteX + openXC7 flow scripts (setup-litex.sh, run-fpgas-online.sh), 18 of
  20 Xilinx design/board combinations built (uart, pmod-loopback,
  pmod-pin-id, spi-flash-id, ethernet-test, ddr-memory on Arty A7-35T,
  NeTV2 xc7a35tfgg484-2, LiteFury xc7a100tfgg484-2, Acorn
  xc7a200tfbg484-3; the xc7a200t chipdb export succeeded this time, 349 s,
  8.5 GiB peak); the two PCIe designs fail for structural reasons (LitePCIe
  needs Vivado only IP / toolchain attributes). Corpus: 11 MiB of FASM with
  dense/sparse reference .frm. Quirks fixed: LiteX chipdb name parsing,
  yosys `$buf` cells need a techmap for nextpnr-xilinx.
  Orchestrator check with the merged Rust fasm2frames: 14/18 identical;
  the 4 spi-flash-id designs differed because their goldens were produced
  with the openXC7 snap's bundled prjxray-db (a fork that knows STARTUP
  features such as CFG_CENTER_MID.STARTUP.USRCCLKO_CONNECTED, absent from
  the pinned f4pga/prjxray-db, where both the oracle and Rust reject the
  file identically). With the snap database Rust matches all 18. The
  corpus must name that database; in review.
* T7.2 review: REQUEST CHANGES. Confirmed 18/18 sparse and 7/7 dense goldens
  match Rust with the snap database; the pinned f4pga db lacks 6 segbits
  entries and 3 ppips files (CFG_CENTER_STARTUP_*) that spi-flash-id needs.
  Required: name the database in every README (snap 0.8.2, Info.md
  "Project X-Ray 4c157493, 2021-12-14"), point the e2e test at the snap db,
  fix a `find | head` under pipefail that aborts run-fpgas-online.sh on a
  fresh setup, and commit the yosys `$buf` techmap workaround as a patch.
  Recorded T5.8b (fetch-db.sh openxc7 source). Fixes in progress.
* T5.6/T5.7 implemented on branch `worktree-agent-ae46691f80970f2d8` (16
  commits): Series7 bitstream writer (header, 13 sync words, §6.3 packet
  sequence, addMissingFrames, ECC word 50, 2 zero frame padding, RCRC
  without CRC) and reader (packet iterator, FAR tracking, to_frames),
  gflags emulation, `xc7frames2bit`/`bitread`/`xcfasm` binaries
  (SOURCE_DATE_EPOCH added to pin the header time), configuration_ranges
  part.yaml support. Golden smoke .bit byte identical; counter dense =
  sparse = oracle; difftest-xilinx: 87 fasm2frames + 154 xc7frames2bit/
  bitread + 45 xcfasm + 6 reference bitstreams all identical; cli-difftest
  1535 passed. xc7a200t dense .bit in 8-17 ms (0.12 s whole binary vs
  0.25 s reference). In review.
* T7.2 done: fixes 8ad7dc8 (committed yosys `$buf` patch applied
  idempotently, pipefail bug, --help), 259f0e7/7a5f132 (READMEs name the
  snap prjxray-db: snap 0.8.2, Info.md "Project X-Ray 4c157493,
  2021-12-14"), 55cc5de (e2e test uses the snap db, never the pinned one;
  fixed a wrong `fasm parse` invocation), b31b8c7 (docs). Merged with
  --no-ff; `pytest tests/e2e`: 59 passed in the main tree (18/18 designs'
  frames identical with Rust fasm2frames + difftest over the arty
  subset); corpus 11 MiB; worktree removed. Note from the implementer:
  LiteX SoC builds are not bit for bit deterministic across runs on this
  toolchain; the committed FASM is the frozen reference.
* T1.4b + T1.3b + T1.6 (core crate hardening) started as one task.
* T5.6/T5.7 review (Opus, ~8,000 runs: 280 random .frm on 20 parts, 4880
  bitread flag runs, 750 synthetic configuration_ranges part runs, 1600
  reader mutations, 795 gflags cases, third reference = openXC7 flow's own
  top.bit): all byte identical. REQUEST CHANGES: bitread buffers its whole
  output (1.3 GB peak on a dense xc7a200t `-x -o`) and prints stderr before
  stdout in merged logs; three undocumented differences (mmap of non
  regular files, unseekable output header length, directory as part
  file). Fixes in progress.
* T5.6/T5.7 done: fixes 540a40c (bitread streams and flushes like the
  reference: 22 MiB peak vs 68 MiB reference), 3e6f640 (size-0/pipe inputs,
  unseekable output header, directory part file emulated), 8c441d2 (YAML
  block sequences), d107b33 (11 bitread flag sets), docs. Merged with
  --no-ff; on the merged tree: 418 Rust tests, difftest-xilinx 107/107
  fasm2frames + 60/60 xcfasm + 6/6 reference bitstreams identical;
  binaries fasm, fasm2frames, xc7frames2bit, bitread, xcfasm; worktree
  removed. The complete 7 series FASM -> bitstream path is in place.
* T6.1 marked done: the loader already reads prjuray-db (zynqusp, per-part
  layout) with 0 mismatches in the T5.2 review; remaining UltraScale work
  is T6.2/T6.3.
* T6.2 (UltraScale / UltraScale+ assembler validation and bitstream
  writer/reader vs prjuray-tools) started.
* Core hardening implemented on branch `worktree-agent-a7c6a3455fe10e01d`
  (4 commits): canonical output iterates set bits (`W[4294967295:1] = 1`
  now 2 ms via the CLI), timing test budget 5 s unless FASM_TIMING_TESTS=1
  (strict variant ignored by default), cargo-fuzz crate rust/fasm/fuzz
  (parse / roundtrip / merge targets, 3 x 10 minute runs, ~3.25M execs, 0
  crashes), `make fuzz`. Found pre-existing: tools/difftest.py has no class
  for the T5.4 fasm2frames error corpus (3 unexplained); classifier fix
  requested on the same branch before review.
* Core hardening (T1.4b, T1.3b, T1.6) reviewed (Opus): 2,500 random huge
  range cases 0 mismatches vs the oracle, difftest 0 unexplained with the
  new `xilinx_error_corpus` class. REQUEST CHANGES: the roundtrip fuzz
  target did not assert model equality in the non-canonical branch; fixed
  in 2756667 (plus `*.fasm.xz` seeds, 100 seed files per target, 20,000
  fuzz runs clean). Merged 79e66ea with --no-ff; on the merged tree: fmt,
  clippy -D warnings clean, all workspace tests pass (229 in fasm),
  difftest.py 100/100 explained. Worktree removed.
* T5.3 (binary database cache + fasm-db-cache tool) started (Opus) in the
  slot freed by the hardening merge; T6.2 still running.
* T5.3 implemented on branch `worktree-agent-a4735f947a5fdafa9` (10
  commits): rust/fasm-xilinx/src/cache/ (magic FASMXDB1, BLAKE3 header
  and payload hashes, per source stat fingerprint + content hash, loader
  fingerprint from build.rs, atomic writes, parallel section decode),
  `Database::open_cached`, env `FASM_XDB_CACHE` (not FASM_DB_CACHE, which
  names the fetched text databases) / `FASM_XDB_CACHE_VERBOSE`,
  `fasm-db-cache` build/verify/info/list/clear binary, fasm2frames/xcfasm
  use it with no new flags. Measured cached open 23 ms xc7a35t / 41 ms
  xc7a200t vs 100-200 ms text load (interning 98k-195k names dominates;
  short of the few ms target), counter_test fasm2frames 31-34 ms vs
  94-101 ms (14x vs reference). xilinx-difftest identical with the cache
  on. Independent review (Opus) started.
* T5.3 review (Opus): byte identical with and without the cache on 7 real
  designs incl. error paths, ~9k mutation iterations 0 panics, 8 process
  race clean, 15 staleness scenarios correct. REQUEST CHANGES: on 1 s
  timestamp filesystems (ext3 -I 128 reproduced) a source rewritten in
  place during the cache build could leave a permanently stale cache
  (12/40 builds); fix requested (do not write when a source timestamp is
  inside the racy window / hash the parsed bytes) plus tests. Follow-ups
  recorded under T5.3b (loader fingerprint in the file name, lazy tile
  type decode, skip payload hash on stat hit, NFS caveat).
* T5.3 done: review fixes 468b8b5 (no cache write when a source changed
  inside the 5 s racy window, unit test with explicit window, §8.8),
  097969c (fasm-db-cache messages, verify/info without a cache dir),
  e015b4e (tests/cli and difftest-xilinx use a temporary FASM_XDB_CACHE).
  Merged with --no-ff; merged tree: fmt/clippy clean, 450 workspace tests
  pass, real db cache tests 6/6, xilinx-difftest 107/107 fasm2frames,
  60/60 xcfasm, 6/6 bitread identical with the cache on. Worktree
  removed. Remaining cache work is T5.3b.
* Container restart interrupted the T6.2 agent before its first commit
  (worktree clean at 57e7d1f, reconnaissance scratch in its target/t62
  kept); resumed from its transcript. T5.9 (per database synthetic
  corpus generator + all-parts difftest over artix7/kintex7/spartan7/
  zynq7) started (Opus) in the slot freed by the T5.3 merge.
* T5.9 implemented on branch `worktree-agent-a0b441475f0044a50` (8 commits:
  tools/gen-xilinx-corpus.py, all-parts mode of tools/difftest-xilinx.py,
  `make xilinx-difftest-all`, tests/cli/test_xilinx_corpus.py and
  test_gen_xilinx_corpus.py, §8.9). The agent's permission classifier
  refused to run the reference tools, so the orchestrator ran
  `make xilinx-difftest-all` (artix7, kintex7, spartan7, zynq7 fetched):
  125 parts, 2151 FASM files, 6.6 M lines; fasm2frames 2651 runs = 2526
  identical + 125 explained (value-range error message, one per part) + 0
  different; xc7frames2bit+bitread 8814 runs and xcfasm 375 runs all
  identical; wall time 4114 s with 4 jobs. Golden for xc7a35t committed
  (be7df9b); the agent is finishing the golden header, docs with the real
  numbers and a quick mode before review.
* A second container restart killed the T6.2 agent again; it had hung on
  the same tar extraction both times and never committed. Its worktree
  (agent-a9a575d5bcc2c6743, clean at 57e7d1f) could not be removed (the
  auto mode classifier refused) and is left in place. T6.2 relaunched
  fresh (Opus) with a brief pointing at the reference tools and warning
  about the hang.
* T5.9 follow-ups done (1e83766 golden header records the reference
  commits from tests/oracle/build/xilinx/status.json, 8713229 ETA +
  --parts-sample + `make xilinx-difftest-quick`, 0a9852c docs with the
  real numbers; the 125 explained runs are exactly errors/value_range.fasm
  = COMPAT rule 4, the 588 traceback normalisations are exactly the error
  files). Independent review (Opus) started; T6.2 running.
* T5.9 review (Opus): independent coverage checker, cache keying, rule
  classification, determinism across Python 3.10-3.13, quick target 127 s
  all fine. REQUEST CHANGES: STEPDOWN units of the second _SING alias group
  were silently skipped on all 125 parts (host chosen per tile type, not
  per alias group) and the coverage tests could not detect drops; run
  count typo in §8.9. Fixes requested; golden must be regenerated after.
* T5.9 done: review fixes 5ab7dbd (STEPDOWN host per alias group, per
  group coverage check and manifest, aliased pseudo PIPs, db commit
  check), b82763f (symbolic HEAD, --parts exit 3, stricter rule 4),
  27b9d6a (docs), golden regenerated with the reference (d01146b).
  Orchestrator ran `make xilinx-difftest-quick` on the version 2 corpus:
  4 parts, 59 files, fasm2frames 75 runs = 71 identical + 4 explained
  (value_range) + 0 different, 249 bitstream tool runs, 12 xcfasm runs
  identical. Merged with --no-ff; merged tree: fmt/clippy clean, 450
  Rust tests, tests/cli 1608 passed / 12 skipped, flake8 clean. Worktree
  removed. The full version 2 all-parts run (about 70 min) is started in
  the background and its result logged when done.
* T7.3 (f4pga-examples through the f4pga/VPR flow) started (Opus) in the
  slot freed by the T5.9 merge; T6.2 running; version 2 all-parts
  reference run in the background (3 jobs, ~90 min estimate).
* T6.2 implemented on branch `worktree-agent-aaffa2f97024e493a` (14
  commits): UltraScale/UltraScale+ ECC and bitstream writer/reader with
  `BitstreamFormat` (prjuray-tools native vs prjxray's UltraScale
  variant), new binaries `xcframes2bit`, `uray-bitread`,
  `uray-fasm2frames`, UltraScale in `xc7frames2bit`/`bitread`,
  `fasm2frames` on prjuray-db parts, configuration_ranges part.yaml,
  synthetic-usp-db fixture, uray-* oracle wrappers,
  tests/cli/test_uray_tools_compat.py (92), `make uray-difftest`, gflags
  --helppackage fix, §8.10 + COMPAT sections. Reported byte identical on
  220 uray-fasm2frames runs (both zynqusp parts), 1078 derived bitstream
  runs, ToolsTestData round trips, prjxray regression unchanged; Rust
  2-8x faster. xcfasm has no UltraScale (xc_fasm lacks it); Spartan6 not
  implemented. Independent review (Opus) started.
* T6.2 review (Opus): C++ sources read and matched (ECC, address fields,
  part walk, headers, packets), 131 adversarial comparisons + 13,000 fuzz
  mutations, all branch verification reproduced (473 Rust tests,
  uray-difftest 220/220 + 1078 + 5/5, xilinx-difftest unchanged, 92 + 128
  compat tests), --helppackage fix confirmed against the C++ tools.
  REQUEST CHANGES: doc-only (COMPAT row for xcu(p)series part.yaml values
  that overflow the address fields: reference hangs/accepts, Rust
  rejects). T6.3 scope refined from the reviewer's coverage findings.
* T6.2 done: doc fix d467739 merged with --no-ff (c7c5951). The merge
  conflicted with T5.9 in Makefile, tools/difftest-xilinx.py and §8.9/8.10
  (both sides appended); kept both sides and fixed three semantic
  conflicts in the prjuray mode against T5.9's Runner API: `run()` now
  takes the runner, `--seed` collided (prjuray option renamed
  `--uray-seed`), `bit_time()` takes bytes. Merged tree: fmt/clippy
  clean, 473 Rust tests, tests/cli 1708 passed / 12 skipped,
  xilinx-difftest 107/60/6 identical, uray-difftest 220/220 + 5/5
  identical. Worktree removed. Next: T6.3.
* T6.3 (prjuray-db all-parts differential tests with every-feature
  coverage) started (Opus); T7.3 running; version 2 all-parts prjxray run
  at 74/125 parts in the background.
* T6.3 implemented on branch `worktree-agent-a2a7bb03c7cd519cf` (7
  commits): gen-xilinx-corpus.py for the prjuray-db layout (16-bit words,
  past-frame-end handling, new error files), `--prjuray` over every
  family/part with the every-feature corpus, result cache, JSON report,
  `make uray-difftest-all`, test_uray_corpus.py + golden under
  tests/corpus/prjuray/, 6 new generator tests, §8.12. Upstream prjuray-db
  (affbc5e5) has only zynqusp with the two xczu3eg parts and no native
  UltraScale part. Coverage 27/27 segbits tile types, 54542/54542 reachable
  features, 34 keys unreachable (digit-leading name parts neither parser
  accepts). 268 uray-fasm2frames runs = 258 identical + 10 explained
  (value range) + 0 different, 1248 bitstream tool runs identical, 406 s.
  No Rust bug found. Independent review (Opus) started.
* Version 2 all-parts prjxray reference run finished: 125 parts, 1752
  files, 6.71 M lines; fasm2frames 2252 runs = 2127 identical + 125
  explained (value_range) + 0 different; 7617 bitstream tool runs and 375
  xcfasm runs identical; 7039 s with 3 jobs. Recorded in §8.9,
  tests/oracle/README.md and COMPAT.md.
* T6.3 review (Opus): independent coverage checker confirmed 54542/54542
  reachable keys and the 34 digit-leading unreachable names (rejected by
  both parsers), model matches prjuray fasm2frames.py and the oracle,
  full --prjuray run reproduced (268 runs, 0 different, 247 s; cache
  rerun 71 s), prjxray mode byte identical to before, determinism across
  Python 3.10-3.13. APPROVE, optional notes only (help text, venv hash
  filter, --list fetching, tests/corpus/README table): recorded as T6.3b.
  Merged with --no-ff; merged tree: 473 Rust tests, xilinx-difftest and
  uray-difftest identical, tests/cli suite green. Worktree removed.
  Phase 6 (UltraScale/UltraScale+) is complete.
* T5.10 (fasm.xilinx Python bindings + fasm_xilinx_* C API) started
  (Opus); T7.3 running.
* T7.3 implemented on branch `worktree-agent-a10d4cdf1c15a7c33` (13
  commits): tools/e2e/setup-f4pga.sh (micromamba, explicit conda lock with
  md5s, pip freeze, sha256 checked arch-defs, --big-files-dir for the
  4.8 GiB xc7a100t device), run-f4pga-examples.sh,
  compare-f4pga-examples.py, tests/e2e/test_f4pga_examples.py (70 tests),
  difftest-xilinx.py corpus extensions (difftest.json dirs, .fasm.xz,
  --corpus-root), 8.3 MB corpus of 30 design/board pairs (counter_test x5,
  picosoc x4, litex_demo x4, linux_litex_demo x2, timer, pulse_width_led,
  button_controller, projf hello A-L), §8.11. All 30 byte identical to the
  flow's own tools and to the pinned oracle (120/120 fasm2frames, 90/90
  xcfasm each side, 30/30 difftest.py); the flow's prjxray-db equals the
  pinned 0a0adde. Not built: the 2 nexys_video (xc7a200t) pairs, whose
  10.5 GiB arch defs do not fit this container. Reference quirks: broken
  bin/fasm2frames entry, genfasm OOM kill ignored by the flow (truncated
  FASM counted as success), non reproducible .bit header. Rust tools
  30-35x faster summed over the designs. The agent's commit trailers name
  the sub-agent model (Claude Opus 5.5) rather than the session line;
  left as is (history is not rewritten). Independent review (Opus)
  started.
* T7.3 review (Opus): corpus hashes 30/30, Rust = flow = oracle on 9
  designs incl. both linux_litex_demo, rebuilds byte identical, 94 tests,
  xilinx-difftest 227/150/6 with the new corpus, 107 old cases unchanged.
  REQUEST CHANGES: the genfasm failure check misses SIGBUS/SIGTERM/plain
  non-zero exits; setup-f4pga.sh --help truncated. Fixes requested.
* T5.10 implemented on branch `worktree-agent-a9a77b4dc9608e6b7` (6
  commits): `fasm.xilinx` as a default-on `xilinx` feature of
  rust/fasm-python (Database.open with cache control, FasmAssembler,
  Frames mapping, write/read_bitstream with format strings,
  fasm2frames/fasm2bit mirroring xc_fasm, exception hierarchy with
  reference_exception, GIL released), `fasm_xilinx_*` C API (status codes
  7-12, fasm_bytes, options struct) + fasm::xilinx C++ wrappers, C/C++
  tests under valgrind, 48 Python tests, docs/CI. counter_test
  fasm2frames 60 ms (13 ms cached) vs 289 ms xc_fasm; 2.3 ms with the
  database open. Noted: parse-error precedence differs from the CLI
  (documented), synthetic-db Series7 read-back oddity to investigate, the
  agent's classifier once denied a heredoc creating a file (it used the
  Write tool). Independent review (Opus) started.
* T7.3 done: review fixes a50606d (tools/e2e/f4pga/check-genfasm.sh: log
  must end with genfasm's VPR footer and no bash signal report; fake
  genfasm self tests for KILL/BUS/TERM/SEGV/ABRT/exit), 2eb2f9f (--help,
  free space check, pip --no-deps), 0f86712 (LiteX marker in the env,
  compare exit status), 0466044 (docs). Merged 26b0958 with --no-ff;
  conflicts with T6.2/T6.3 in difftest-xilinx.py argparse and §8.10-8.12
  resolved keeping both sides and ordering the sections. Merged tree:
  fmt/clippy clean, 473 Rust tests, xilinx-difftest 56 files 227/227 +
  150/150 xcfasm + 6/6, uray-difftest 268 runs 0 different, tests/cli +
  tests/e2e 2094 passed / 14 skipped. The 4.8 GB f4pga toolchain lives in
  the T7.3 worktree (.claude/worktrees/agent-a10d4cdf1c15a7c33/tools/e2e/
  build, baked conda paths; use F4PGA_E2E_ROOT=<that>/tools/e2e/build);
  the worktree directory is kept for that reason (7.8 GB free, a fresh
  `tools/e2e/setup-f4pga.sh` needs ~5 GB) and can be removed when disk is
  needed. The two nexys_video (xc7a200t) pairs remain unbuilt here.
* T7.6 (nextpnr-xilinx example designs via the installed openXC7
  toolchain) started (Opus); T5.10 under review.
* T5.10 review (Opus): wheel built and 539 Python tests pass, byte
  identical vs xc_fasm, 20/20 ctest under valgrind, adversarial C/Python
  cases clean, synthetic-db read-back oddity explained (fixture part.yaml
  rows have gaps vs the tilegrid; reference tools identical).
  REQUEST CHANGES: a Python feature callback referencing its own
  assembler is a GC-invisible cycle (no __traverse__/__clear__), leaking
  the assembler and database. Fix requested plus small optionals. The
  reviewer was refused deleting its own scratch/target by its classifier;
  those are removed with the worktree after the merge.
* T5.10 done: review fixes 88fe289 (FasmAssembler takes part in GC:
  __traverse__/__clear__, weakref support, per call callback errors),
  b9f67d9 (bytes paths, ValueError for empty frames, KeyError for any
  missing key, py.typed), f10a637 (OSError kind on C file errors, ABI
  note). Merged with --no-ff (no conflicts). Merged tree: fmt/clippy/doc
  clean, 478 Rust tests, capi 20/20 under valgrind, Python (maturin
  build from the repo root; a stale fasm/_fasm_rs.abi3.so from an older
  develop had to be rebuilt) 1638 passed / 37 skipped incl. 51/51
  test_xilinx_python.py, xilinx-difftest 227/227, uray-difftest 268 runs 0
  different. Worktree removed. Phase 5 is complete except T5.3b/T5.8b.
* T8.1 (benchmark suite + docs/rewrite/BENCHMARKS.md) started (Sonnet);
  T7.6 running.
* T8.1 implemented on branch `worktree-agent-a0371a01da3c39a86` (3
  commits, Sonnet): parser bench corpus classes (lut, annotated, stress),
  bitstream bench over Series7 + UltraScale+ (fixing a hard coded
  words_per_frame), tools/bench/run-benchmarks.py (Rust vs ANTLR/textX,
  xc_fasm, prjuray; median/min wall time, peak RSS), BENCHMARKS.md, README
  pointer. Headline: parser 125-427 MB/s by corpus class (pips at the 200
  MB/s target, stress below), idstring hit 52 ns vs plain HashMap 38 ns,
  IdString sort 2.7x slower than String sort, fasm on linux_litex_demo
  93 ms vs ANTLR 3.5 s / textX 66 s, fasm2frames counter_test 34-37 ms
  cached vs 400-500 ms reference. Not measured: 1M line textX (RSS blow
  up), perf profile (no perf), python suite re-run. Independent review
  (Opus) started.
* T8.1 review (Opus): end-to-end numbers reproduced within noise, benches
  and clippy clean. REQUEST CHANGES: the every-feature benchmark timed a
  run that fails with a conflict on both sides (conflict free runs: Rust
  0.32 s vs oracle 18 s, 56x, not 26x); failed runs shown as timings;
  peak RSS inherits the driver floor via vfork/exec; database cache state
  not controlled; missing rows (picosoc, linux_litex_demo, 1M line
  parser, canonical 1M lines 9.7 s / 2.4 GiB); several factual errors;
  README pointer placement. Fixes requested.
* T7.6 implemented on branch `worktree-agent-a64c002dea262a71e` (6
  commits): OPENXC7_E2E_BUILD, tools/e2e/run-nextpnr-examples.sh (+
  compare/install scripts), 2.5 MB corpus of 20 designs from
  nextpnr-xilinx 0.8.2 examples, openXC7/demo-projects and
  primitive-tests (blinky, attosoc, litex-ddr 196k lines, regression and
  primitive tests on xc7a35t/xc7a100t/xc7a200t), test_nextpnr_examples.py
  (104 tests), §8.13, COMPAT section on the snap's tools. All 20 byte
  identical vs the snap tools and the oracle; 4 designs fail identically
  on the pinned db (STARTUPE2/BSCANE2 cfg centre ppips and a GTP refclk
  bit exist only in the snap db). difftest-xilinx over the corpus 80/80 +
  60/60 + 576/720 bitstream runs identical; Rust fasm2frames 66x faster
  summed. Skipped: other families (zynq7, kintex7, spartan7, UltraScale+
  produce no FASM), 12 designs nextpnr 0.8.2 cannot place/route. Snap
  quirks: --emit_pudc_b_pullup always fails (old IN_ONLY name), fasm falls
  back to textX (libffi.so.7). Independent review (Opus) started.
* T8.1 done: review fixes b1c75f8 (every-feature timed per conflict free
  file with rc==0 required: xc7a35t features.fasm Rust 363 ms vs oracle
  18.6 s (51x), all 11 files 0.69 s vs 22.2 s; xczu3eg 0.41 s vs 8.8 s;
  failures/timeouts reported as such; /bin/true RSS floor row; explicit
  primed FASM_XDB_CACHE), 2bacd39 (corrected numbers: picosoc and
  linux_litex_demo rows, 1M line parser 492 ms / 127 MB/s vs ANTLR 16 s,
  canonical on 1M lines 9.7 s and 2.4 GiB because tool.rs buffers the
  expanded output; factual fixes). Merged with --no-ff; fmt/clippy
  --all-targets/test clean (478), flake8 clean. Worktree removed. T8.2
  targets, ranked: interner hit path (52 vs 38 ns), byte-wise ASCII name
  validation, IdString sort (2.7x), streaming canonical output, stress /
  pips parser classes below 200 MB/s.
* T8.2 (hot path optimisation: interner hit path, byte-wise name
  validation, IdString sort, streaming canonical output) started (Opus);
  T7.6 under review.
* T7.6 review (Opus): every equivalence claim confirmed (20/20 hashes,
  oracle wrappers on 10 designs, 5 rebuilds identical, 104 tests,
  xilinx-difftest grows to 76 files / 307 runs / 210 xcfasm identical).
  REQUEST CHANGES: run_regression does not export CHIPDB nor write
  nextpnr.log for the cases check.sh scripts (verdict reasons wrong in
  two READMEs); primitive-tests bscane2 and the non Artix directories are
  not listed as attempted/skipped. Fixes requested.
* T7.6 done: review fixes d8d8c00 (regression check.sh gets CHIPDB and
  nextpnr.log, no_route cases never counted as built, bscane2 entry, 15
  non Artix skip entries, depth 1 fetch, usage errors), acd04ac (bscane2
  corpus entry: 2371 lines, identical vs the snap tools, same
  FasmLookupError as the oracle on the pinned db; README regeneration
  with check.log verdicts), b1f75b6 (docs incl. tests/corpus/README.md
  size table). Merged with --no-ff (no conflicts). Merged tree:
  fmt/clippy clean, xilinx-difftest 77 files 311/311 + 213/213 xcfasm +
  6/6 identical, uray-difftest 268 runs 0 different, tests/cli + tests/e2e
  2400 passed / 15 skipped. Worktree removed. 21 nextpnr-xilinx /
  openXC7 designs in the corpus. Disk pressure: stale review scratch and
  the debug target removed.
* T7.4 (VTR genfasm designs via the vtr-optimized binaries in the f4pga
  conda env and the upstream VTR checkout) started (Opus); T8.2 running.
* GitHub access granted: claude/epic-goldberg-uc7xqf pushed (483 commits);
  pushes now succeed after every merge. Local master holds the T0.1
  fast-forward to upstream ffafe82 (50 commits); on the user's
  instruction it is pushed as `origin/claude/master` (not `master`), which
  local `master` now tracks.
* T8.2 implemented on branch `worktree-agent-ad96d03561f44665a` (5
  commits): 8-byte name scanner with dot positions + `intern_split`
  (UTF-8 checked only on a miss, whole buffer check lazy), streaming
  `fasm --canonical` with a counting sort (1M lines: 12 s / 2.4 GiB ->
  2.1 s / 490 MiB, byte identical; errors still raised before any
  output), `sort_by_string` (1.8x faster than Ord) used by merge_and_sort
  and the Python fast path, inlining regression fix, BENCHMARKS "After
  T8.2". Parser now above 200 MB/s for pips/mixed/lut/annotated; stress
  cold 120-129 MB/s remains below (new tile name insertion ~175 ns).
  Interner hit path not improved: six approaches measured and rejected
  (documented in DESIGN-idstring.md); the level-0 probe is memory bound.
  Identity: difftest.py 0 unexplained, tests/cli 1928 passed,
  xilinx-difftest and uray-difftest identical, fuzz 300k runs clean. The
  agent's classifier refused a callgrind profile and reading the foldhash
  source. Independent review (Opus) started.
* T7.4 implemented on branch `worktree-agent-af7815efaa978bd81` (12
  commits): genfasm (f4pga vtr-optimized 8.0.0_5699, VTR 25e723a24) on
  1502 generic BLIFs with VTR's test_fasm_arch (428 produce FASM, 44.5k
  lines; the rest do not fit / need >6 input LUTs / too large) and 42
  Xilinx designs (19 produce FASM: symbiflow task benchmarks incl.
  picosoc, murax, ibex, minilitex, linux_arty 352k lines, and 9 VTR
  Verilog benchmarks through the f4pga flow). All identical vs the Python
  oracle parser and, for Xilinx, vs the flow's tools and the pinned db
  (difftest-xilinx 64/64 + 48/48 + 576 identical); Rust xcfasm 30-40x
  faster. VTR's own `wire` test emits digit-leading routing features that
  all three parsers reject (new difftest class all_three_reject). Corpus
  +4.3 MB (tests/corpus/vtr/test_fasm_arch, xilinx/artix7/designs/vtr),
  tools/e2e/run-vtr-genfasm.sh + helpers, test_vtr_genfasm.py (469),
  §8.14, COMPAT "VTR genfasm output". difftest.py 201 files 0 unexplained.
  No Rust change. Independent review (Opus) started.
* T8.2 review (Opus): scanner proven exact (exhaustive classifier, word
  scan vs byte loop, 6,000 random differential inputs and 32 adversarial
  UTF-8 files identical to the pre-change binary), canonical ordering
  argument verified incl. the `[` fallback, sort_by_string stable and
  identical to string order, all identity suites and 200k fuzz runs
  clean, benchmarks reproduced (canonical 1M lines 10.2 s -> 2.1 s, 2466
  -> 490 MiB). APPROVE. Merged with --no-ff; merged tree: 487 Rust tests,
  difftest.py 151 files 0 unexplained, tests/cli 2126 passed,
  xilinx-difftest 311/311 + 213/213, uray-difftest 0 different, Python
  suites 1710 passed against a fresh maturin build. Worktree removed;
  follow-ups recorded as T8.2b.
* T8.3 (documentation pass) started (Sonnet); T7.4 under review.
* T8.3 first pass on branch `worktree-agent-aed2d965dc1a11931` (5
  commits): README front page (binaries and env var tables, layout,
  verification numbers, test suites), docs/PYTHON.md and docs/CAPI.md
  user guides, COMPAT.md table of contents, PLAN.md status section and
  design doc index, Python module/function docstrings. Rust crates already
  had 0 missing_docs; 9 doctests. Asked to close the remaining output.py /
  _types.py docstring gaps and add a stub audit test before review.
* T8.3 complete on its branch (8 commits: + remaining docstrings, yapf
  fixes, tests/test_stubs.py auditing the fasm.xilinx stub, 25/25
  checks). Found pre-existing: fasm/__init__.py binds a string to
  `__dir__` so dir(fasm) raises TypeError (also in the original
  package?); to be judged by the reviewer. Independent review (Opus)
  started.
* T8.3 review (Opus): install, Xilinx examples, Rust docs, style, links
  and numbers verified; REQUEST CHANGES: the fasm API examples in
  PYTHON.md and the C/C++ parse/print/merge examples in CAPI.md do not
  run (wrong module paths, missing canonical argument, wrong C++ method
  name), several docstring/README claims wrong (antlr parser mentions,
  env var table incomplete, comment group rule), test_stubs.py ignores
  parameter kind/defaults and is not wired into CI. 14 fixes requested.
  The __dir__ TypeError is inherited from the original package: T8.3b.
* T8.3 done: 8 fix commits (every code example in README/PYTHON.md/CAPI.md
  now executed by the implementer: Python against a maturin build, C/C++
  compiled with -Wall -Wformat and run, capi-install + pkg-config link;
  docstring corrections; env var table incl. XRAY_*; PLAN status matches
  TASKS; test_stubs.py compares parameter kind and defaults and runs in
  make test/CI; README cargo run needs --bin fasm). Merged with --no-ff;
  merged tree: cargo doc -D warnings, capi-header-check, flake8, yapf
  0.24 clean, Rust tests pass, Python stub/simple/xilinx suites pass.
  Worktree removed.
* T8.4 (packaging: crates.io metadata, wheels workflow, CMake install,
  RELEASING.md/CHANGELOG; no publishing) started (Sonnet); T7.4 under
  review.
* T7.4 review (Opus): corpus hashes, reruns and difftest counts confirmed
  (xilinx-difftest grows to 93 files / 375 runs / 261 xcfasm identical;
  difftest.py 201 files, the 151 pre-existing classifications unchanged).
  REQUEST CHANGES: benchmark licences misrecorded (VTR benchmarks and the
  symbiflow SoCs have their own terms), genfasm value format / duplicate
  count misstated, all_three_reject checks only the line, and
  run-vtr-genfasm.sh could rm -rf a user supplied VTR_ROOT. Fixes
  requested.
* T8.4 implemented on branch `worktree-agent-a4b50d199c81e7c11` (3
  commits, Sonnet): crates.io metadata and include lists (cargo package
  -p fasm verified: 398 KiB / 96 KiB compressed), per crate READMEs,
  version 0.1.0-dev, wheels.yml (manylinux x86_64/aarch64, macOS,
  Windows, parser only wheel, sdist, wheel tests; publish disabled
  pending PyPI trusted publishing), CMake package config for the C API
  (find_package(fasm) shared and static verified), package job in
  rust.yml, docs/RELEASING.md and CHANGELOG.md; sdist 289 KiB and wheel
  854 KiB pass twine check and a fresh venv install. Finding: the crate
  name `fasm` is already taken on crates.io by an unrelated crate
  (zk2u/fasm); publishing needs a rename or another decision by the
  user (documented in RELEASING.md). cargo audit: 0 vulnerabilities.
  Independent review (Opus) started.

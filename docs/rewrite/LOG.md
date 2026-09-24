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

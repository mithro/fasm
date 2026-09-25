# Benchmarks (T8.1)

This documents the benchmark suite added for T8.1, how to reproduce it,
and the headline numbers measured on the machine this was run on. It
reconciles with the measurements already recorded in
`docs/rewrite/DESIGN-xilinx-db.md` §8.6, §8.8, §8.10 and §8.11, and closes
with a hotspot list for T8.2.

## Methodology

**Machine**: 4 vCPUs (`nproc`), `Intel(R) Xeon(R) Processor @ 2.10GHz`
(Firecracker VM, per DESIGN-xilinx-db.md's own notes; `lscpu` does not
show a real turbo/base clock in this virtualised environment), 15.7 GiB
RAM, kernel `6.18.44-fc-v37`. Load average at the time of each run is
recorded with the run (see below); this is a shared container with other
agents' builds running concurrently (per `AGENTS.md`), so runs were taken
when 1-minute load was under nproc where possible and are reported as
medians/best-of-N rather than single samples, to average out that noise.

**Toolchain**: `rustc 1.94.1 (e408947bf 2026-03-25)`, release builds
(`cargo build --release`, `cargo bench` uses Cargo's `bench` profile,
which is `opt-level=3` like release). Python 3.11.15.

**Reference versions** (`tests/oracle/build/status.json`,
`tests/oracle/build/xilinx/status.json`):

| Reference | Commit |
|---|---|
| `fasm` oracle (textX + ANTLR, pre-rewrite `chipsalliance/fasm`) | `ffafe82` |
| `f4pga-xc-fasm` (`xc_fasm`) | `25dc605c` |
| prjxray (Python + C++ tools) | `c9f02d85` |
| prjuray | `c550b03a` |
| prjuray-tools (C++) | `f53f07b8` |

**Two measurement tools**:

1. `cargo bench` (custom harnesses, not criterion — see below) for
   in-process, allocation/CPU-level measurements of the Rust hot paths:
   `rust/fasm/benches/{idstring,parser}.rs`,
   `rust/fasm-xilinx/benches/{db,assemble,bitstream}.rs`. Each prints
   throughput/ns-per-operation to stdout; there is no criterion
   dependency (kept out to avoid pulling in gnuplot/HTML report
   generation for a workspace that otherwise has minimal dependencies),
   so numbers are "best of N" or medians computed by the bench itself, as
   documented in each file's module doc comment.
2. `tools/bench/run-benchmarks.py` (stdlib only) for end-to-end,
   subprocess-level comparisons of the Rust CLI tools and Python bindings
   against the Python/C++ references, with wall time (median, min, over
   the runs that exited 0 -- a failed run's time is never averaged in;
   a run whose measurement had *any* failed repeat is reported as
   `failed (rc=N)` instead of a time, and a run that hit its timeout as
   `timed out`, both explicitly, not silently) and peak RSS (`os.wait4`'s
   per-child `ru_maxrss` — see the script's docstring, and
   "Peak RSS is not a clean per-program number" below, for why this is
   still not simply "the benchmarked program's memory" and how the
   script corrects for that). Every "default-cache" measurement uses an
   explicit, freshly primed `--xdb-cache-dir` (never the environment/XDG
   default cache location), so a run's cache state is reproducible and
   does not depend on, or pollute, anything left on the machine by a
   previous run (see `prime_xdb_cache()`).

**Peak RSS is not a clean per-program number.** `os.wait4`'s `ru_maxrss`
for a short-lived child process is contaminated by its *parent's* memory
history: right after `fork()`/`posix_spawn()`, a child's pages are the
parent's via copy-on-write, and on this container that measurably leaks
into the reported peak even after `exec()` replaces the child's own
address space -- `/bin/true` run alone reports about 15 MiB, but the same
`/bin/true` run immediately after the driver script allocates and frees
an unrelated 300 MB buffer reports about 322 MiB. Shelling out to `xz`
for decompression (`materialize()`) removes one source of that growth,
but not the general problem. `tools/bench/run-benchmarks.py` now samples
`/bin/true` through the same measurement path before and after each run
(`measure_rss_floor()`) and reports the larger sample as that run's RSS
floor (16.9 MiB in this session's own runs -- reported with each run in
its Markdown report). A measurement whose RSS is at or below the floor is
marked `(floor)` explicitly and should be read as "at or below the noise
floor", not as a real number for that program (this session's `/bin/true`
baseline is the only row that hit it -- most measurements below, even the
small `counter_test` ones at 32-59 MiB, sit clearly above the 16.9 MiB
floor, so their RSS numbers are trusted as real). Tables that omit a Peak
RSS column (the `cargo bench` sections above) are unaffected: those are
in-process measurements, not `os.wait4` on a spawned child, so this
caveat does not apply to them.

**Reproducing**:

```sh
# Rust hot-path benches (needs a database; FASM_DB_CACHE from tools/fetch-db.sh):
cargo bench -p fasm --bench idstring
cargo bench -p fasm --bench parser              # FASM_PARSER_BENCH_MB to size the input
FASM_DB_CACHE=<db> cargo bench -p fasm-xilinx --bench db
FASM_DB_CACHE=<db> cargo bench -p fasm-xilinx --bench assemble
FASM_DB_CACHE=<db> cargo bench -p fasm-xilinx --bench bitstream

# End-to-end vs. the Python/C++ references (needs the built oracle --
# tests/oracle/setup.sh / setup-xilinx.sh -- and target/release built):
cargo build --release -p fasm-cli
python3 tools/bench/run-benchmarks.py \
    --oracle-dir /path/to/checkout/with/tests/oracle/venv-built \
    --out-json bench-report.json --out-md bench-report.md
```

From a git worktree (as this session ran in), `tests/oracle/{venv,
venv-xilinx,build}` are gitignored build output and not shared between
worktrees, so `--oracle-dir` (or `$FASM_ORACLE_DIR`) must point at a
checkout where `tests/oracle/setup.sh`/`setup-xilinx.sh` actually ran —
here, the main checkout `/home/user/fasm`. See `tools/bench/run-benchmarks.py --help`
for `--quick`, `--skip`, `--full-corpus` and `--repeats`.

## Parser throughput vs. the 200 MB/s target

`cargo bench -p fasm --bench parser` (`FASM_PARSER_BENCH_MB=30`, streaming
`fasm::parse_lines`, warm = every feature name already interned, cold =
fresh interner), release build, this machine:

| Corpus class | Cold | Warm (best of 2) |
|---|---:|---:|
| `mixed` (pips + single-bit + LUT/BRAM `INIT`, realistic Xilinx shape) | 248 MB/s | 369-385 MB/s |
| `pips` (short routing-pip lines, no values — worst case per byte) | 192 MB/s | 203-208 MB/s |
| `lut` (64-bit LUT / 256-bit BRAM `INIT` heavy — value-parser heavy) | 385 MB/s | 393-427 MB/s |
| `annotated` (every line followed by `{ .. }`, 1/3 comments/blank) | 234 MB/s | 320-321 MB/s |
| `stress` ("every feature" shape: many short components, multi-bit values, duplicates) | 125 MB/s | 194-208 MB/s |

Against the **200 MB/s target** (PLAN.md): `mixed` and `lut` classes are
comfortably above target warm and even cold; `pips` (the worst case for
per-byte overhead, since almost the entire line is the interned name and
there is very little else to amortize the interning cost against) sits
just under target warm and clearly under it cold; `stress` (churns many
distinct, short, one-off names — closest to a cold interner in the middle
of a large design) is the slowest class, at 125-208 MB/s. This matches
T8.2's own diagnosis in TASKS.md: **on pip-heavy FASM the parser measures
150-164 MB/s with ~48% of instructions in idstring interning and ~9% in
UTF-8 validation of names** — the `pips` and `stress` classes here are
exactly the shapes that stress the interner hit path the hardest (many
short components, looked up over and over), and their throughput
(125-208 MB/s) is consistent with that breakdown. See "What T8.2 should
optimise" below.

## idstring interning (`cargo bench -p fasm --bench idstring`)

67,500 distinct names (3 real feature names x 150x150 tile grid,
`examples/many.fasm`), release build:

| Operation | ns/op |
|---|---:|
| intern (miss, cold interner) | 181.6 |
| intern (hit) | 52.4 |
| intern (hit, 8 threads, wall/total-ops) | 32.6 |
| `intern_bytes` (hit, no UTF-8 validation) | 52.7 |
| `from_utf8` + intern (hit, separate validation pass) | 58.6 |
| lookup (hit) | 50.7 |
| `with_str` | 19.5 |
| resolve (to `String`) | 29.1 |
| sort `IdString` | 171.2 /element |
| sort `String` | 63.0 /element |
| `HashMap<String,u32>` get (baseline) | 38.2 |

Two things stand out for T8.2. First, **the interner's hit path (52.4 ns)
is slower than a plain `HashMap<String,u32>` get (38.2 ns)** — the
interner is currently slower than the naive baseline it exists to beat on
a per-lookup basis, which only pays for itself through `IdString`'s
8-byte size (vs. 59.8 bytes/name for `String`, a 7.5x memory reduction:
`interner heap 6,280,320 bytes = 93.0 bytes/distinct name`) and cheap
equality (two `u64` handles) once interned. This comparison is
conservative in the interner's favor, not against it: the baseline
`HashMap` here is `std`'s default (SipHash, a cryptographic hash chosen
to resist hash-flooding, not for speed), while the interner's own tables
use `foldhash` (a fast, non-cryptographic hash) — so a from-scratch
per-level `HashMap<String,u32>` sharing the interner's own hasher, not
measured here, would likely narrow this gap further, making the
regression this points at real but possibly smaller than 52.4 vs 38.2 ns
suggests. The `intern_bytes` fast path (52.7 ns, skipping UTF-8
validation on bytes already known to be ASCII by the grammar) is barely
faster than the validated path (58.6 ns) at this sample size, which is
smaller than TASKS.md's claimed ~9% UTF-8 validation share of the
*parser's* profile — the two numbers measure different things (isolated
interning-only loop vs. whole-parser instruction profile including the
surrounding tokenizer), and are noted here rather than reconciled, since
only a `perf` profile of the actual parser (not available in this
environment — see below) would settle it precisely. Second, **sorting
`IdString` handles (171.2 ns/element) is 2.7x slower than sorting
`String`s directly (63.0 ns/element)**: `Interner::cmp` is not O(1) per
comparison (an earlier draft of this document wrongly called `IdString`
ordering O(1) once interned, which the 2.7x gap itself contradicts) — it
walks the handle's hierarchical levels comparing their component indexes,
stopping at the first level that differs and resolving only *that*
level's text to break the tie by string order (see
`docs/rewrite/DESIGN-idstring.md`, "Ordering"), which is still cheaper
than a full string compare per pair but not free; this is a concrete
T8.2 target for anything that sorts many `IdString`s (canonical output,
`merge_and_sort`), most straightforwardly by resolving each handle to its
`&str` once before the sort (decorate-sort-undecorate) rather than paying
`Interner::cmp`'s per-comparison cost repeatedly.

## `fasm_tuple_to_string` / canonical / `merge_and_sort` (Python fast paths)

Already benchmarked and documented in `docs/rewrite/DESIGN-python.md`
("Benchmarks", "### Benchmarks" under "`fasm.xilinx`"); not re-run in this
session (no code changed there since). Summary (best of 7, CPython 3.11):

| File | Function | Rust fast path | Pure Python |
|---|---|---:|---:|
| `top.fasm` (706 lines) | `fasm_tuple_to_string` | 0.3 ms | 0.4 ms |
| `top.fasm` (706 lines) | `merge_and_sort` | 0.8 ms | 1.0 ms |
| generated, 100k lines | `fasm_tuple_to_string` | 89 ms | 127 ms |
| generated, 100k lines | `merge_and_sort` | 243 ms | 520 ms |
| `top.fasm` (781 lines, 706 `FasmLine`s) | `parse_fasm_filename` (Rust vs. ANTLR vs. textX) | 0.3 ms | 3.5 ms (ANTLR) / 74 ms (textX) |
| generated, 100k lines, 5.9 MB | `parse_fasm_filename` | 66 ms | 1.08 s (ANTLR) / 22.4 s (textX) |

Speed-ups (pure Python / fast path, recomputed from the table above and
corrected against an earlier draft of this document that had them
backwards): `fasm_tuple_to_string` is 1.3x on the small file and 1.4x at
100k lines; `merge_and_sort` is 1.25x on the small file and 2.1x at 100k
lines — `merge_and_sort`'s fast path is the *bigger* win at the larger
size, not the smaller one, even though it still constructs the same
Python `fasm.model` namedtuples for its result (only the grouping/sorting
itself moves to Rust) while `fasm_tuple_to_string` skips building them
entirely; the per-line Python object construction `merge_and_sort` still
pays for evidently costs proportionally less than the sorting work Rust
now does for it, at this size.

## End-to-end: Rust CLI vs. Python/C++ references

`tools/bench/run-benchmarks.py`, this session's run (see "Reproducing"
above for the exact command; `--oracle-dir` pointed at the main
checkout's built oracle since this ran in a worktree).

### `fasm` parse/print/`--canonical` vs. ANTLR and textX

Wall time, median of repeated runs (rust: 3-5 repeats; oracle ANTLR/textX:
1-3, since the textX runs on medium/large files take seconds to tens of
seconds each):

| Input | Rust `fasm` | Rust `fasm --canonical` | oracle ANTLR | oracle textX |
|---|---:|---:|---:|---:|
| `counter_test`/`top.fasm` (781 lines) | 2.6 ms | 2.6 ms | 51.8 ms | 167.6 ms |
| `picosoc_demo`/`vpr.fasm` (97,441 lines, 3.6 MB) | 23.8 ms | 63.4 ms | 889.1 ms (37x) | 16.05 s (674x) |
| `linux_litex_demo`/`vpr.fasm` (344,230 lines, 14.5 MB) | 92.6 ms | 248.3 ms | 3.46 s (37x) | 66.32 s (716x) |

(oracle columns' `Nx` is the speed-up of the Rust `fasm` parse/print
column over that reference, same input.)

A generated 1,000,000-line synthetic file (62 MB, same "mixed" shape as
the `cargo bench` `mixed` corpus class) was also run:

| Tool | Result |
|---|---:|
| Rust `fasm` (parse/print) | 492 ms median (~127 MB/s) |
| Rust `fasm --canonical` | 9.7 s, 2.4 GiB peak RSS, writing 27.2M lines (1.04 GB) -- see the T8.2 list below |
| oracle ANTLR | 16.2 s, 2.87 GiB peak RSS |
| oracle textX | timed out (bounded to 5 minutes; its RSS was still climbing past 2 GiB, well beyond the ~400 MB it uses on the 344k-line file, when the bound was reached) |

The Rust CLI's own throughput on this file (~127 MB/s) is on the low end
of the `cargo bench --bench parser` range above for a `mixed`-shaped
input (248-385 MB/s): this file mixes in annotation/comment lines too (see
`generate_synthetic_fasm()` in `tools/bench/run-benchmarks.py`), and,
unlike the `cargo bench` harness's "warm" pass, this is a single cold
run through a fresh CLI process end to end (parse, format, write to
stdout) rather than an isolated parse-only loop over data already in
memory — both closer to what a real one-shot CLI invocation costs and
consistent with being nearer the `annotated`/`stress` end of that range
than the plain `mixed` one. `textX`'s worse-than-linear memory growth on
this shape (2+ GiB and climbing on 1M lines vs. ~400 MB on 344k lines)
points at the textX/Arpeggio grammar's model construction, not at
anything in this repository's Rust or reference-wrapper code. This does
not affect the Rust throughput numbers already measured and reported
above via `cargo bench`, which do not depend on the oracle at all;
reproduce with `tools/bench/run-benchmarks.py` (the parser suite, not
`--quick`).

The `fasm` CLI numbers here are lower than DESIGN-python.md's
`parse_fasm_filename` Python-binding numbers for a similar file size (e.g.
66 ms for a 100k line file via the Python binding, vs. tens of ms here):
the CLI parses, formats and writes text without ever constructing Python
objects, while `parse_fasm_filename` additionally builds one
`fasm.model.SetFasmFeature`/`Annotation` namedtuple per line — the two
numbers measure different things and are not in tension (DESIGN-python.md
itself notes "building the Python objects costs about as much as
parsing").

### Xilinx `fasm2frames`/`xcfasm` vs. `xc_fasm`/prjxray

Wall time, median of 3 (1 for the no-cache row, deliberately run once
since it forces a full text load each time). The binary database cache
used by the "cache" column is an explicit, freshly primed
`--xdb-cache-dir` (never the environment/XDG default) — see
`prime_xdb_cache()` in `tools/bench/run-benchmarks.py`.

| Design (part `xc7a35tcsg324-1`) | Tool | Rust (cache) | Rust (no cache) | oracle |
|---|---|---:|---:|---:|
| `counter_test`/`top.fasm` (781 lines) | `fasm2frames` | 35.9 ms | 92.9 ms | 391.1 ms (11x/4x) |
| | `xcfasm` (-> `.bit`) | 37.9 ms | 99.4 ms | 488.0 ms (13x/5x) |
| `picosoc_demo`/`vpr.fasm` (97,441 lines) | `fasm2frames` | 128.1 ms | 197.9 ms | 2.90 s (23x/15x) |
| | `xcfasm` | 124.9 ms | 200.0 ms | 2.93 s (23x/15x) |
| `linux_litex_demo`/`vpr.fasm` (344,230 lines) | `fasm2frames` | 420.7 ms | 481.0 ms | 12.08 s (29x/25x) |
| | `xcfasm` | 395.7 ms | 468.3 ms | 12.06 s (30x/26x) |

"Every feature" sample corpus (`tools/gen-xilinx-corpus.py --tiles
sample 20`, xc7a35tcsg324-1, 11 conflict-free pass files from
`manifest.json`'s `"files"`, **each run on its own** — see
`run_every_feature_corpus()` in `tools/bench/run-benchmarks.py` for why
concatenating them, as an earlier version of this document and script
did, is invalid: the pass files exist specifically because their
features conflict with each other on the same tile when combined, so a
concatenated file makes both tools correctly reject it before doing most
of the real assembly work, which is not a meaningful throughput
measurement). Every one of the 22 runs below (11 files x 2 tools) exited
0:

| | Rust `fasm2frames` | oracle `xc_fasm.fasm2frames` | speed-up |
|---|---:|---:|---:|
| `features.fasm` (169,623 lines, the large majority of the corpus) | 363.3 ms | 18.64 s | 51x |
| `features-2.fasm` through `features-11.fasm` (10 smaller files) | 30.9-35.6 ms each | 332-388 ms each | ~11x each |
| **sum of all 11 files** | **689.2 ms** | **22.21 s** | **32x** |

### UltraScale+ `uray-fasm2frames` vs. prjuray

Same every-feature-corpus treatment (`tools/gen-xilinx-corpus.py --tiles
sample 20`, xczu3eg-sfvc784-1-e, 14 pass files, each run on its own, all
28 runs exited 0):

| | Rust `uray-fasm2frames` | oracle `prjuray`'s `utils/fasm2frames.py` | speed-up |
|---|---:|---:|---:|
| `features.fasm` (largest file) | 405.6 ms | 8.77 s | 22x |
| `features-2.fasm` through `features-14.fasm` (13 smaller files) | 271.8-326.2 ms each | 1.01-1.24 s each | ~4x each |
| **sum of all 14 files** | **4.21 s** | **23.50 s** | **5.6x** |

The smaller UltraScale+ pass files show a noticeably lower speed-up
(~4x) than the Series7 "smaller" files (~11x) — but these are not
comparably sized: xc7a35t's `features-2.fasm` through `features-11.fasm`
are genuinely tiny (7-496 lines each; `wc -l` on the generated corpus),
so their ~31-36 ms Rust time is dominated by fixed per-process overhead
(a cache-hit database open costs ~22-32 ms alone, per "Database open"
below) rather than by real per-feature work, while xczu3eg's
`features-2.fasm` through `features-14.fasm` are 23-1,065 lines each —
substantially larger — so a bigger share of their wall time on both
sides is genuine per-feature assembly work rather than fixed overhead,
which is why their ratio sits closer to `features.fasm`'s (22x) than the
Series7 small files' ratio does.

### Python bindings (`fasm.xilinx`) vs. `xc_fasm`, with/without the cache

Not re-run as an automated `run-benchmarks.py python` suite in this
session (building the extension into a scratch venv with `maturin
develop --release` was judged not worth the extra several minutes given
the numbers are already documented, current and unchanged in
DESIGN-python.md's "Tests and performance" section under "`fasm.xilinx`:
bindings of `fasm-xilinx`"; reproduce with `python3 tools/bench/run-benchmarks.py --skip parser --skip xilinx7 --skip ultrascale`
if a fresh number is needed). Summary, `counter_test`/`top.fasm` on
`xc7a35tcsg324-1`, best of several runs in one process, release build:

| | dense | sparse |
|---|---:|---:|
| `xc_fasm.fasm2frames.fasm2frames` (reference, opens the db each call) | 289 ms | 172 ms |
| `fasm.xilinx.fasm2frames` + `to_frm()`, no cache, opens the db each call | 60 ms | 54 ms |
| the same through the binary cache | 13 ms | 8 ms |
| database already open: `fasm2frames` + `write_bitstream` | 2.3 ms | |

## Database open: text vs. cache, per family

`cargo bench -p fasm-xilinx --bench db`, this session, release, best of 3,
fresh process per open (empty interner), two separate runs (the range
shows this machine's run-to-run noise directly, rather than relying on
comparison to an older document):

| Part (family) | Text files (first open) | Cache hit | Cache file | Text load + cache write |
|---|---:|---:|---|---:|
| xc7a35tcsg324-1 (artix7) | 85.1-88.7 ms | 21.9-25.7 ms | 9.0 MiB | 105.2-124.2 ms |
| xc7a200tffg1156-1 (artix7) | 169.0-179.0 ms | 44.3-54.1 ms | 14.9 MiB | 236.9-249.4 ms |
| xczu3eg-sfvc784-1-e (zynqusp/prjuray) | 100.6-101.0 ms | 24.8-32.1 ms | 10.0 MiB | 129.6-130.0 ms |

This matches DESIGN-xilinx-db.md §8.8's own table closely (there: xc7a35t
102-138/23/125-149 ms; xc7a200t 176-196/40-42/251-283 ms; xczu3eg
100-124/24-25/140-187 ms) — both this session's own two-run range and
§8.8's range overlap comfortably (e.g. the first xc7a200t cache-hit
sample here, 54.1 ms, looked like a possible regression against §8.8's
40-42 ms until a second run gave 44.3 ms, squarely inside §8.8's range);
this is the run-to-run noise §8.8 itself documents (a shared, 4-core VM),
not a regression — no cache format or loader change happened between the
two measurements.

Feature lookup (from the same run): `lookup_feature` 22.6-29.7 ns,
`lookup_fasm_feature` (splits the whole FASM feature handle first)
108.7-116.1 ns, `IdString::new` of the remainder alone 25.2-29.2 ns —
consistent with §8.8's breakdown that most of `lookup_fasm_feature`'s
cost is the `IdString::lookup` split, not the final table lookup.

## Assembly and bitstream

`cargo bench -p fasm-xilinx --bench assemble`, `counter_test`/`top.fasm`
on `xc7a35tcsg324-1`, release, this session:

| Phase | Time |
|---|---:|
| open database (text, uncached) | 81.4 ms |
| parse (706 lines) | 0.2 ms |
| assemble (parse+assemble combined) | 0.4 ms |
| `get_frames(false)` (dense, 4974 frames) + `write_frm` | 0.5 ms + 6.2 ms (5.3 MiB) |
| `get_frames(true)` (sparse, 662 frames) + `write_frm` | 0.1 ms + 0.5 ms (0.7 MiB) |
| RSS growth of the assembler | 5.6 MiB |

The 1M-line breakdown from DESIGN-xilinx-db.md §8.8 (open 170 ms, parse
150-170 ms, assemble 326 ms, `get_frames` dense 40-50 ms, `write_frm`
20-25 ms for a 21.6 MiB `.frm`) was not independently re-run this session:
reproducing it needs a 1M-line FASM whose *every* feature resolves
against a real part's segbits (a hand-written synthetic file, tried in
this session, immediately hit `KeyError` on a made-up tile name — the
assembler benchmark needs valid features, unlike the pure parser
benchmark above, which only tokenizes); the every-feature generator
(`tools/gen-xilinx-corpus.py`) produces valid-but-small files by design.
The parser-only throughput of that same file shape is covered by the
`cargo bench -p fasm --bench parser` numbers above instead (a directly
comparable 62 MB, 1,000,000-line synthetic file of the same "mixed
features + comments + annotations" shape parsed at 344.9 ms / ~180 MB/s
in the `run-benchmarks.py` run below — see the parser table).

`cargo bench -p fasm-xilinx --bench bitstream` (every frame of a part,
zero vs. 30%-random words, release, this session, best of 5):

| Part (architecture) | Frames | `.frm` | `read_frm` | ECC | `bitstream_bytes` | read+`to_frames` | `write_frm` |
|---|---:|---:|---:|---:|---:|---:|---:|
| xc7a35tcsg324-1 (Series7), zero | 5,408 | 5.8 MiB | 18.9 ms | 0.75 ms | 1.33 ms (2.1 MiB) | 1.17 ms | 4.61 ms |
| xc7a35tcsg324-1 (Series7), random | 5,408 | 5.8 MiB | 23.8 ms | 2.77 ms | 3.63 ms (2.1 MiB) | 1.31 ms | 4.75 ms |
| xc7a200tffg1156-1 (Series7), zero | 24,060 | 25.7 MiB | 86.8 ms | 3.98 ms | 6.51 ms (9.3 MiB) | 6.68 ms | 31.6 ms |
| xc7a200tffg1156-1 (Series7), random | 24,060 | 25.7 MiB | 107.7 ms | 12.13 ms | 14.86 ms (9.3 MiB) | 6.01 ms | 30.6 ms |
| xczu3eg-sfvc784-1-e (UltraScale+), zero | 14,952 | 14.7 MiB | 47.8 ms | 1.90 ms | 3.35 ms (5.3 MiB) | 3.66 ms | 11.2 ms |
| xczu3eg-sfvc784-1-e (UltraScale+), random | 14,952 | 14.7 MiB | 61.5 ms | 6.83 ms | 15.71 ms (5.3 MiB) | 3.63 ms | 11.4 ms |

The bitstream bench previously covered only one hardcoded Series7 part;
this session extended it to a `Target` list (Series7 xc7a35t/xc7a200t,
UltraScale+ zynqusp xczu3eg) using `Architecture::words_per_frame()`
instead of the hardcoded Series7 value of 101 words/frame (UltraScale+ is
93). **UltraScale** (prjuray-tools `xcuseries`) is skipped: prjuray-db has
no native UltraScale part (§8.10/§8.12 of DESIGN-xilinx-db.md — only
synthetic and `ToolsTestData` UltraScale fixtures exist), and the
bitstream bench only opens real `part.yaml`s from a database directory;
UltraScale bitstream correctness is covered by the differential tests
(`tests/oracle/uray-*-oracle`, `make uray-difftest-all`) instead of a
throughput bench.

Reconciling with §8.7/§8.9's own numbers (25.7 MiB text 94-113 ms
[here: read_frm 86.8-107.7 ms — same order, slightly faster, consistent
with run-to-run VM noise], ECC 4-13 ms [here: 4.0-12.1 ms, matches],
`bitstream_bytes` 8-17 ms [here: 6.5-14.9 ms, matches], reading the
bitstream back 5-6 ms [here: 6.0-6.7 ms, matches], `write_frm` 40 ms
[here: 30.6-31.6 ms, a little faster]): no discrepancy large enough to
suggest a regression; the small differences are consistent with the
documented VM noise, not a code change (no bitstream writer/reader code
changed between the two measurement sessions).

## f4pga-examples end-to-end (already measured; not re-run)

DESIGN-xilinx-db.md §8.11 already has a full per-design table (30
designs, `counter_test` through `linux_litex_demo`, `xcfasm`/
`fasm2frames`/`fasm --canonical`, flow vs. Rust) from building the real
f4pga (VPR) flow end to end — that flow's toolchain lives in a different
worktree (`agent-a10d4cdf1c15a7c33`, ~4.8 GB, marked read-only for this
task) and rebuilding 30 designs (each 60-660 seconds of synthesis/pack/
place/route) was out of scope for this session's time budget. Headline,
summed over the 30 designs (§8.11): `xcfasm` flow 108 s / Rust 3.1 s
(35x); `fasm2frames` flow 107 s / Rust 3.0 s (36x); `fasm --canonical`
flow 27.6 s / Rust 1.6 s (17x); `xc7frames2bit` 1.4 s / 0.5 s and
`bitread -z -y -o` 1.1 s / 0.3 s (both C++ on the flow side, dominated by
process start and I/O at these sizes). This session's own
`run-benchmarks.py` table above re-measures the same three designs
(`counter_test`, `picosoc_demo`, `linux_litex_demo`) directly against the
committed corpus files (no flow rebuild needed) and is consistent with
§8.11's per-design rows for those three.

## What T8.2 should optimise, ranked by measured impact

1. **The interner's hit path is slower than a plain `HashMap<String,u32>`
   get** (52.4 ns vs. 38.2 ns baseline, this session's idstring bench,
   itself a conservative comparison — see the idstring section above for
   why) — this is the single largest, most concretely measured item, and
   matches TASKS.md's own note that ~48% of pip-heavy-FASM parsing
   instructions are in idstring interning. The interner's payoff is in
   memory (7.5x smaller handles) and cheap equality once interned, not
   per-lookup speed or ordering (see item 3); T8.2 should either close
   this gap (a faster per-level probe, fewer indirections on the hit
   path) or make sure callers that intern the same name repeatedly in a
   tight loop (e.g. `stress`-shaped FASM) take the fastest available
   path.
2. **UTF-8 validation on interning from parser bytes.** TASKS.md's ~9%
   instruction share and the `intern_bytes` vs. `from_utf8`-then-intern
   gap in this session's idstring bench (52.7 vs. 58.6 ns, a real but
   modest difference in isolation) point the same direction: validate
   byte-wise once, at the tokenizer, exploiting that feature names are
   ASCII by grammar, rather than paying a general UTF-8 validator per
   intern call. This is the pip/annotation-heavy corpus classes' main
   remaining cost once interning itself is faster.
3. **Sorting `IdString`s is 2.7x slower than sorting `String`s directly**
   (171.2 vs. 63.0 ns/element, this session) because the comparator
   resolves both operands through the interner on every comparison.
   Anything that sorts many `IdString` handles — canonical output,
   `merge_and_sort`'s Rust fast path — pays this; caching the resolved
   `&str` alongside the handle for the duration of one sort (a
   decorate-sort-undecorate pass) would very likely close most of the
   gap, and is a self-contained, low-risk change to try first.
4. **The `stress` and `pips` parser corpus classes are the furthest below
   the 200 MB/s target** (125-208 MB/s and 192-208 MB/s respectively vs.
   369-427 MB/s for `mixed`/`lut`) — both are shapes dominated by many
   short, distinct or one-off interned components relative to the bytes
   parsed, so they are downstream of (1) and (2) rather than a separate
   problem; re-measure these two corpus classes specifically after (1)/(2)
   land, since they are the most sensitive indicator of whether the
   interner work actually moved the needle on real parsing throughput
   rather than only the isolated microbenchmark.
5. **Database cache open** (21.9-54.1 ms cache-hit vs. the "few ms"
   target noted in both §8.6 and §8.8 — and a full text load, on a cache
   miss, is still slower yet: 85.1-179.0 ms first open, or 67.5-150.8 ms
   for a warm-interner re-open in the same process) is already tracked
   as T5.3b in TASKS.md (lazy per-tile-type decoding, interner bulk
   insert) and is not re-ranked here above (1)-(3): a cache hit affects
   only the *first* database open of a process (subsequent opens in the
   same process are already fast, per the db bench's "re-open, warm
   interner" numbers), whereas (1)-(3) affect every parse and every
   canonical/sorted output, in every process.
6. **Canonical output can allocate multiple gigabytes for wide `INIT`
   values.** `fasm --canonical` on the generated 1,000,000-line synthetic
   file (which includes 256-bit BRAM `INIT` values, roughly half set)
   took 9.7 s and 2.4 GiB peak RSS to write 27.2 million lines (1.04 GB)
   of output (see the parser table above) — this is not a bug, it is
   exactly what the canonical FASM format requires
   (`rust/fasm/src/output/canonical.rs`'s `canonical_features` expands
   every `SetFasmFeature` into one single-bit feature line per *set* bit,
   per the FASM specification), but it means a design with enough wide
   `INIT` values can make `--canonical` output scale with total bits set
   rather than with FASM line count, in both time and memory. Confirmed
   by reading the code, not just inferred from the RSS number:
   `rust/fasm-cli/src/tool.rs`'s `render`/`render_canonical` build the
   entire output as one in-memory `String` (an `arena` buffer) and only
   call `stdout.write_all` once that whole string is complete, so the
   2.4 GiB is the whole expanded output held in memory at once, not an
   unrelated leak. A streaming canonical writer (write each expanded line
   to `stdout` as it is produced instead) would bound the memory side of
   this without changing the (format-mandated) output size; ranked last
   because it is
   real FASM content behaving as specified, not a performance regression,
   and because `--canonical` on files this INIT-heavy is a less common
   path than plain parsing.

**Not measured, and why**: a `perf` profile of the actual parser (to
settle the UTF-8-validation share precisely, per item 2, and to find any
hotspot not visible from the two synthetic microbenchmarks above) —
`perf` is not installed in this container and installing it was judged
out of scope for a benchmarking task versus an optimisation one; T8.2
should use `perf record`/`perf report` (or `cargo flamegraph`) directly,
since TASKS.md's existing "150-164 MB/s, ~48%/~9%" figures already came
from a `perf` session this one could not reproduce. The full f4pga
end-to-end rebuild (§8.11) and the `xc7a200t --tiles all` every-feature
corpus (multi-GB, tens of minutes per DESIGN-xilinx-db.md §8.9) were not
re-run for the reasons given above; `run-benchmarks.py --full-corpus`
reproduces the latter when there is time/RAM budget for it. Python
`fasm.xilinx` binding numbers were reused from DESIGN-python.md rather
than rebuilt with maturin into a fresh venv (see above); nothing in
`fasm-python`'s Rust source changed since those numbers were taken.

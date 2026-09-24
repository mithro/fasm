# Fuzzing (T1.6)

`cargo-fuzz` (libFuzzer) targets for the `fasm` core crate. This directory
is a standalone crate (`fasm-fuzz`, its own `[workspace]` — see its
`Cargo.toml`), deliberately **excluded** from the top level workspace
(`../../../Cargo.toml`'s `[workspace] exclude`): `cargo-fuzz` needs a
nightly toolchain and sanitizer support that the rest of this repository
(stable, MSRV 1.88) must never depend on. A plain `cargo build`/`cargo test`
(with or without `--workspace`) from the repository root never touches
this directory.

## Setup

```sh
cargo install cargo-fuzz
rustup toolchain install nightly
```

## Targets

| Target | File | What it checks |
|---|---|---|
| `parse` | `fuzz_targets/parse.rs` | `parse_fasm_bytes` on arbitrary bytes never panics and finishes (T1.3 already makes parsing linear time; this exercises everything else libFuzzer's mutations reach — malformed UTF-8, truncated annotations/values, degenerate whitespace, ...). |
| `roundtrip` | `fuzz_targets/roundtrip.rs` | For every input that parses: `fasm_tuple_to_string` never errors on a parser-produced model; the rendered text always re-parses; re-rendering the re-parsed model is byte-identical to the first rendering (idempotence — see the file's doc comment for why this, rather than raw `FasmLine` equality, is the round trip property checked); and, in canonical mode, every re-parsed line is actually in canonical form (single bit, value 1, no `value_format`). |
| `merge` | `fuzz_targets/merge.rs` | `merge_and_sort` on models built by parsing arbitrary bytes never panics; an invalid combination (e.g. a bit set by one feature and cleared by another) comes back as `Err`, not a panic. Skips models containing a feature wider than `MAX_FEATURE_WIDTH` (64Ki bits) — see the file's doc comment and "Known, accepted cost: `merge_features`" below. |

## Running

```sh
make fuzz                      # every target, 10 minutes each, -jobs=2
make fuzz FUZZ_SECONDS=60      # shorter, for a quick check
make fuzz-corpus                # just (re)seed fuzz/corpus/<target>/, no fuzzing

# One target directly:
rust/fasm/fuzz/seed-corpus.sh parse
cd rust/fasm && cargo +nightly fuzz run parse -- -max_total_time=600 -jobs=2
```

`make fuzz`/`seed-corpus.sh` populate `fuzz/corpus/<target>/` (gitignored)
from every `tests/corpus/**/*.fasm` and `examples/*.fasm` file in the
repository — real, parseable FASM, so fuzzing starts from meaningful
inputs instead of an empty corpus. What's committed to git is
`seed-corpus.sh` (i.e. the *list* of source files it draws from, expressed
as those two globs) plus the source corpus itself under `tests/corpus/`
and `examples/`; the copies under `fuzz/corpus/` and anything libFuzzer
finds under `fuzz/artifacts/` are not (`fuzz/.gitignore`).

## Known, accepted cost: `merge_features`

`merge_features` (`rust/fasm/src/output/merge.rs`) iterates the *entire*
address range of each input feature, not just its set bits, to correctly
track which bits were explicitly cleared (needed for bit-conflict
detection) — this is an intentional, Python-equivalent cost, documented in
`docs/rewrite/DESIGN-output.md`'s "`merge_features` still iterates the
full address range" section, not a bug. Left un-bounded, a trivially short
fuzz input like `a[4294967294:0]=1` would turn `merge_and_sort` into a ~4
billion iteration loop, burning the whole fuzzing time budget on this
known cost and reporting it as a libFuzzer timeout "crash" that is not a
real bug (and not something T1.6 was asked to fix — see T1.4b/T1.6 in
`docs/rewrite/TASKS.md`). The `merge` target therefore skips any input
whose parsed model contains a feature wider than `MAX_FEATURE_WIDTH`
(2^16 bits — far larger than any real FASM feature, 256 bits at most, but
far smaller than `u32::MAX`) before calling `merge_and_sort`. `parse` and
`roundtrip` need no such guard: parsing is linear time (T1.3) and
canonical rendering is `O(set bits)` since T1.4b, neither reachable by an
`O(range width)` blowup from a small input.

## Findings

No crash, panic, timeout or OOM has been found by the runs performed for
T1.6: each target below ran for 601s (`-max_total_time=600`, `-jobs=2`,
seeded via `make fuzz-corpus` from 87 `tests/corpus/**/*.fasm` +
`examples/*.fasm` files), one after another on the same 4-core machine.

| Target | Runs (both workers) | Time | Exec/s | Final coverage (edges/features) | Crashes |
|---|---|---|---|---|---|
| `parse` | 2,733,327 | 601s | ~4,550 | 1,429 / 7,047 | 0 |
| `roundtrip` | 178,380 | 601s | ~296 | 1,826 / 9,582 | 0 |
| `merge` | 341,872 | 601s | ~568 | 2,431 / 12,637 | 0 |

`roundtrip` and `merge` run substantially fewer execs/s than `parse`
because each input does several times the work (parse, render twice,
re-parse, and — for `roundtrip` — compare; for `merge`, additionally
group/merge/sort), not because either is anywhere near the `O(range
width)` cost the `MAX_FEATURE_WIDTH` guard above exists to avoid (no run
logged a slow unit or an OOM). If a future run does find a crash: minimise it
with `cargo +nightly fuzz tmin <target> fuzz/artifacts/<target>/<crash-file>`,
add the minimised input as a regression file under
`tests/corpus/synthetic/regressions/` (generate it with
`tools/gen-corpus.py`, following its existing `edge-cases`/`invalid`
patterns, so it is picked up by `tools/difftest.py` too — see
`tests/corpus/README.md`) or as a unit test near the code it exercises,
fix the underlying bug, and record it prominently in this section and in
`docs/rewrite/LOG.md`.

# FASM differential test corpus (T1.5)

Every FASM file under here (plus `examples/*.fasm`) is discovered by
`tools/difftest.py`, which compares the Rust `fasm` crate against the
original Python `fasm` package (the oracle, `tests/oracle/`) over it: parse
tree, `fasm_tuple_to_string` (canonical and non-canonical) and a parse ->
print -> parse round trip. See `docs/rewrite/PLAN.md` ("Testing strategy")
and `docs/rewrite/TASKS.md` (T1.5) for how this fits the rewrite, and
`docs/rewrite/COMPAT.md` for the documented Rust/oracle divergence classes
that `tools/difftest.py`'s classifier checks
`tests/corpus/synthetic/edge-cases/` files against.

Every `*.fasm` file here (plus `examples/*.fasm`) also seeds the T1.6
`cargo-fuzz` targets under `rust/fasm/fuzz/` (`make fuzz`, or
`rust/fasm/fuzz/seed-corpus.sh`; see `rust/fasm/fuzz/README.md`). A
minimised regression file for a bug a fuzz run finds belongs under a new
`synthetic/regressions/` directory (added via `tools/gen-corpus.py`,
following the existing `edge-cases`/`invalid` patterns), so it is picked
up by both `tools/difftest.py` and the next fuzz run.

Run it with `make difftest` (builds the `fasm-dump` helper binary first) or
directly: `python3 tools/difftest.py --jobs 4`. `tests/difftest/test_difftest.py`
is a pytest wrapper that does the same and asserts zero unexplained
differences (it skips cleanly if the oracle venv or `fasm-dump` are not
built).

## Directories

| Directory | Size | Source | Licence |
|---|---|---|---|
| `fasm-examples/` | 20K | this repository's own `examples/*.fasm` (copied so the corpus is self contained) | Apache-2.0 |
| `f4pga-xc-fasm/` | 40K | `chipsalliance/f4pga-xc-fasm` `tests/test_data/*.fasm` (commit `25dc605c9c0896204f0c3425b52a332034cf5e5c`) | Apache-2.0 |
| `vtr/` | 556K | FASM literals extracted from `verilog-to-routing`'s `utils/fasm/test/` (commit `34f65cb7c14dd4a59d5b95a5d65920dcc0d9a89b`; VTR keeps no committed `.fasm` files itself), and `test_fasm_arch/`: VTR `genfasm`'s FASM of the 428 VTR netlists that fit VTR's genfasm test architecture (T7.4, VTR `25e723a24`; `genfasm.json.xz` lists all 1502 run, one file with the rr edge metadata of VTR's own test is invalid FASM, listed in `expected-errors.json`) | MIT for VTR's own code (`utils/fasm/test/`); the genfasm FASM of `test_fasm_arch/` is of VTR's benchmark circuits (MCNC LGSynth93, VTR test netlists), which carry no licence notice; VTR's `LICENSE.md` leaves benchmark circuits to their own terms (see `vtr/README.md`) |
| `xilinx/artix7/designs/vtr/` | 3.7M | FASM of VTR `genfasm` on the f4pga `xc7a50t_test` architecture (T7.4): VTR's nightly `symbiflow` benchmarks and VTR's Verilog benchmarks (f4pga flow), with difftest.json, README and the reference frames/bitstream sha256 | per design, recorded in each README ("Source and licence"): ISC (symbiflow-arch-defs, PicoRV32/PicoSoC), MIT (VexRiscv/Murax), Apache-2.0 (Ibex), BSD-2-Clause + MIT (LiteX SoCs), the OpenCores notice of `sha`, no notice (`diffeq2`, `spree` and VTR's small test designs); VTR's `LICENSE.md` leaves its benchmark circuits to their own terms |
| `xilinx/artix7/` (without `designs/`) | 200K | prjxray-db derived hand written fixture (`smoke_x1y0.fasm`, predates T1.5, see its own `README.md`), `synthetic/` (hand written `fasm2frames` cases and the error corpus below, T5.4) and `generated/` (golden reference results for `tools/gen-xilinx-corpus.py`'s corpus of one part, T5.9) | Apache-2.0 |
| `xilinx/<family>/designs/f4pga-examples/` | 8.2M | FASM (openXC7 `top.fasm` of T7.1, f4pga/VPR `vpr.fasm` of T7.3) of the chipsalliance/f4pga-examples designs, with frames and a README each (artix7 and zynq7) | Apache-2.0 |
| `xilinx/artix7/designs/fpgas.online-test-designs/` | 11M | FASM and frames of the fpgas-online/fpgas.online-test-designs LiteX designs built with openXC7 (T7.2) | that repository's licence (built from its sources; see `tools/e2e/README.md`) |
| `xilinx/artix7/designs/{nextpnr-xilinx,openxc7-demo-projects,openxc7-primitive-tests}/` | 2.9M | FASM and frames of the nextpnr-xilinx `xilinx/examples` and openXC7 demo-projects / primitive-tests designs built with the openXC7 snap (T7.6) | ISC (nextpnr-xilinx), BSD-3-Clause (demo-projects, primitive-tests); built from their sources |
| `xilinx/*/synthetic/errors/` | (part of the above) | T5.4's `fasm2frames` error-path corpus: files deliberately invalid at either the FASM-parse or the `fasm2frames` lookup level, to exercise `fasm2frames`' own error reporting. A file that still parses identically across all three (a lookup-only error, e.g. an unknown feature) is compared exactly like any other file here; one where the parse trees differ is classified `xilinx_error_corpus` by `tools/difftest.py` when Rust reports a parse error and the ANTLR oracle also errored (in any form -- some of ANTLR's errors for this corpus are internal Python exceptions, not a clean parse error) and textX errored or matched Rust exactly; see the class's doc comment in `tools/difftest.py` for the exact rule. Anything else in that directory is still `unexplained`, i.e. this is not a blanket skip of the directory. | Apache-2.0 |
| `prjuray/` | 20K | golden reference results (`uray-fasm2frames`) for the generated corpus of one UltraScale+ part (T6.3) | Apache-2.0 |
| `oracle/` | 36K | golden oracle output for `examples/many.fasm` (T1.4 fixture, not itself a `.fasm` corpus); predates T1.5 (see its own `README.md`) | Apache-2.0 |
| `synthetic/` | 300K | generated by `tools/gen-corpus.py` (deterministic, seeded, stdlib only): `edge-cases/` (grammar productions and `COMPAT.md` divergences, one small file per case, classified in `edge-cases/manifest.json`), `invalid/` (one invalid construct per file plus a `.expected` sidecar with the Rust error position) and `xilinx-like.fasm` (a synthetic, realistically-shaped 7-series-like file) | Apache-2.0 (generated) |

**Total: ~27M** (`du -sh tests/corpus`; almost all of it the toolchain
built designs of Phase 7, stored `xz -9` where large). Recheck after
adding anything.

Large files (uncompressed >1 MB) are stored `xz -9` compressed as
`*.fasm.xz`; `tools/difftest.py` decompresses them to a temp file
transparently.

## What `tools/gen-corpus.py` also does that is *not* committed

`tools/gen-corpus.py stress --out PATH --size BYTES [--seed N]` writes one
large (up to 100 MB+), deterministic, realistically-shaped FASM file to
`PATH`; `tools/difftest.py --size BYTES` calls it on the fly to add a
one-off stress/performance check (parsed by `fasm-dump`, round-tripped
through Rust only -- the oracle is not run over it, it is far too slow at
that size) to a run. Never add its output to this directory.

## Regenerating the synthetic corpus

```sh
python3 tools/gen-corpus.py write-all --out-dir tests/corpus/synthetic
```

Deterministic (seeded, default seed 0): re-running produces byte identical
output, so this is safe to do at any time (e.g. after adding a new case to
`tools/gen-corpus.py`'s `EDGE_CASES`/`INVALID_CASES` tables).

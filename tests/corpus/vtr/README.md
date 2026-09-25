# vtr corpus

VTR (verilog-to-routing) keeps **no committed `.fasm` files**: its
`genfasm`/`FasmWriterVisitor` writes FASM lines at runtime from
architecture `fasm_*` metadata plus the routed/packed netlist. This
directory holds:

* `vtr_test_fasm_literals.fasm` (T1.5): the literal FASM strings of VTR's
  genfasm unit test, extracted by hand (below);
* `test_fasm_arch/` (T7.4): the FASM that genfasm itself writes for every
  VTR design that it can write FASM for on VTR's own genfasm test
  architecture (generic, not Xilinx, FASM), from
  `tools/e2e/run-vtr-genfasm.sh test_fasm_arch` and
  `tools/e2e/install-vtr-genfasm-corpus.py`.

The Xilinx FASM that genfasm writes for VTR's `symbiflow` regression
benchmarks on the f4pga `xc7a50t_test` architecture is in
`tests/corpus/xilinx/artix7/designs/vtr/` (T7.4, with frames and
bitstreams: `docs/rewrite/DESIGN-xilinx-db.md` §8.14).

## genfasm output on the test architecture (`test_fasm_arch/`, T7.4)

### Which VTR designs can produce FASM

genfasm writes FASM only from `fasm_*` metadata in the architecture and
the rr graph. At VTR `25e723a24` (the VTR of the genfasm used here) the
only such architectures are

* `utils/fasm/test/test_fasm_arch.xml`, the architecture of VTR's genfasm
  unit/integration test `utils/fasm/test/test_fasm.cpp` (no
  architecture under `vtr_flow/arch/` has any `fasm_*` metadata: `grep
  -rl fasm_ vtr_flow/arch vtr_flow/tasks` finds nothing), and
* the symbiflow-arch-defs architectures VTR's nightly `symbiflow` task
  downloads (`vtr_flow/scripts/download_symbiflow.py`), i.e. the
  `xc7a50t_test` architecture of the f4pga toolchain: Xilinx FASM, see
  above.

`test_fasm_arch.xml` is a fixed 6x6 layout: 16 CLBs, each with two
fracturable LUT6 (or 2x LUT5) + FF elements (`fasm_lut`, `fasm_mux`,
`fasm_features`, `fasm_prefix` and per tile `fasm_placeholders`
metadata), and 16 IO tiles of 8 IOs. So every VTR netlist that uses only
`.names` with at most 6 inputs, `.latch`, inputs and outputs, and fits 32
LUT6 / 64 LUT5 and 128 IOs, can produce FASM. `run-vtr-genfasm.sh` runs
all of VTR's BLIF netlists on it (`--route_chan_width 100`, like the
test):

| netlists (`vtr_flow/benchmarks/...`) | circuits | FASM written | not implementable | not run | FASM lines (sum / max) | genfasm s (sum) |
|---|---|---|---|---|---|---|
| `utils/fasm/test/wire.eblif` (`fasm-test/wire`) | 1 | 1 | 0 | 0 | 333 | 0.2 |
| `microbenchmarks/*.blif` | 27 | 20 | 7 | 0 | 921 / 301 | 1.7 |
| `tests/*.{blif,eblif}` | 4 | 2 | 1 | 1 | 11 / 7 | 0.1 |
| `blif/*.blif` (the 20 largest MCNC circuits and two clock tests) | 23 | 2 | 1 | 20 | 117 / 73 | 0.2 |
| `blif/2/` (MCNC, 2-LUT mapped) | 198 | 48 | 35 | 115 | 6684 / 287 | 4.6 |
| `blif/3/` | 198 | 62 | 38 | 98 | 7587 / 291 | 5.8 |
| `blif/4/` | 198 | 68 | 43 | 87 | 7071 / 256 | 6.3 |
| `blif/5/` | 198 | 80 | 41 | 77 | 8108 / 249 | 8.1 |
| `blif/6/` | 198 | 84 | 42 | 72 | 10595 / 336 | 7.7 |
| `blif/7/` | 198 | 30 | 103 | 65 | 1533 / 208 | 2.6 |
| `blif/8/` | 198 | 30 | 110 | 58 | 1533 / 208 | 2.6 |
| `blif/multiclock/` | 1 | 1 | 0 | 0 | 12 | 0.1 |
| `blif/wiremap6/` | 60 | 0 | 0 | 60 | 0 | 0 |
| **total** | **1502** | **428** | **421** | **653** | **44505** | **40.0** |

* *not implementable* (VPR's own error, recorded per circuit in
  `genfasm.json.xz`): 205 do not fit (`Failed to find device which
  satisifies resource requirements`), 209 have a `.names` with more than 6
  inputs (the 7- and 8-LUT mapped MCNC sets), 7 use primitives the
  architecture has no model for (`CARRY0`, `DFF`, `DFFE`, `adder`,
  `PRIMITIVE`, `IO_0`: the microbenchmarks written for other
  architectures);
* *not run*: more than 128 `.names`, twice what the 32 LUT6 slots can
  take (the largest netlist that fitted has 66 `.names`; `run-vtr-genfasm.sh
  --no-size-filter` runs them anyway, and VPR rejects them: with the "does
  not fit" error, or first with `BLIF .names input size (N) greater than
  .names model input size (6)` for a netlist with a wider `.names`, e.g.
  the `s1196` / `s1494` circuits of `blif/7` and `blif/8`).

The FASM files: `fasm-test/wire/genfasm.fasm`, `microbenchmarks/<c>/`,
`tests/<c>/` and `blif/<c>/genfasm.fasm` hold one circuit each;
`blif/<K>/genfasm-all.fasm[.xz]` hold every circuit of
`vtr_flow/benchmarks/blif/<K>/` that produced FASM, each circuit's FASM
unchanged after a `# circuit <name> sha256 <hex> lines <n>` line (a
comment: the file as a whole is still plain FASM). `genfasm.json.xz`
lists all 1502 circuits: netlist, VPR's result, `.names` count, VPR and
genfasm wall time (this machine, 4 cores, 2 runs at a time) and, for the
FASM, its sha256, lines, bytes and where it is stored;
`tests/e2e/test_vtr_genfasm.py` checks every stored FASM against it.

`fasm-test/wire/genfasm-rr-metadata.fasm` is the FASM of
`test_fasm.cpp`'s `fasm_integration_test` itself: the test adds
`fasm_features` metadata to every rr graph edge (`<src>_<sink>_<switch>`,
plus `PIN_<x>_<y>_<sub tile>_<port>_<pin>` for edges into block input
pins) and runs genfasm with that rr graph. `run-vtr-genfasm.sh` does the
same from the command line (`vpr --write_rr_graph`,
`tools/e2e/vtr/add-rr-edge-metadata.py`, `genfasm --read_rr_graph`).
Those edge features start with a digit (`533_557_0`), which is not a FASM
identifier (the FASM specification, the ANTLR grammar and the textX
grammar all require a letter first), so no FASM parser accepts the file:
Rust, ANTLR and textX all stop at its first routing line (334).
`fasm-test/wire/expected-errors.json` records that, and
`tools/difftest.py` classifies the file `all_three_reject` (see
`docs/rewrite/COMPAT.md`, "VTR genfasm output").

### Tools and provenance

* genfasm and VPR: the f4pga toolchain's conda package `vtr-optimized
  8.0.0_5699_g25e723a24` (`tools/e2e/setup-f4pga.sh`), `vpr --version`:
  `8.1.0-dev+25e723a24-dirty`, revision `8.0.0-5699-g25e723a24-dirty`,
  i.e. VTR
  [`25e723a24aa0ae7a0061cd89dd84b1fb62afcc09`](https://github.com/verilog-to-routing/vtr-verilog-to-routing/commit/25e723a24aa0ae7a0061cd89dd84b1fb62afcc09)
  (2022-07-16). The architecture and netlists are from that commit.
* Commands, per circuit (in the output directory):
  `vpr test_fasm_arch.xml <netlist> --route_chan_width 100`, then
  `genfasm test_fasm_arch.xml <netlist> --route_chan_width 100` (genfasm
  loads VPR's `.net`, `.place` and `.route` and writes
  `<netlist model>.fasm`).
* Licences. VTR's `LICENSE.md` (at `25e723a24`) puts VTR's own code
  under MIT, but not the benchmark circuits: "The benchmark circuits are
  all open source but each have their own individual terms and conditions
  which are listed in the source code of each benchmark." So:
  * `test_fasm_arch.xml` and `fasm-test/wire` (`utils/fasm/test/`, VTR's
    genfasm test and its netlist): VTR's own code, MIT;
  * `blif/<K>/` and `blif/multiclock/`: the MCNC (LGSynth93) benchmark
    circuits (`vtr_flow/benchmarks/blif/README`: optimised with SIS and
    technology mapped); the netlists carry no licence or author notice;
  * `blif/clock_aliases`, `blif/clock_set_delay_aliases` (Yosys output),
    `microbenchmarks/*` and `tests/*`: VTR's small test netlists; no
    licence or author notice in them.

  None of these files states terms beyond being distributed with VTR as
  open source benchmarks. The FASM here is genfasm's output for them.

Every file here parses identically with Rust, ANTLR and textX, prints
identically with `fasm_tuple_to_string` (both modes) and round trips
(`tools/difftest.py`), except `genfasm-rr-metadata.fasm` (above).
genfasm's output is simple FASM: one feature per line, no comments,
annotations or blank lines; on this architecture values only as
`[hi:lo]=<width>'b<bits>` (`LUT[63:0]=64'b...` from `fasm_lut`). (On
`xc7a50t_test` genfasm also writes range-less one bit values such as
`F=1'b0`, see `docs/rewrite/COMPAT.md`, "VTR genfasm output".)

## `vtr_test_fasm_literals.fasm` (T1.5)

What VTR commits is a unit test of its FASM writer
(`utils/fasm/test/test_fasm.cpp`, Catch2) and the tiny synthetic
architecture it runs against (`utils/fasm/test/test_fasm_arch.xml`).
`vtr_test_fasm_literals.fasm` extracts every literal FASM feature string
(and, where the metadata itself uses `{tag}` placeholders that VTR fills
in per tile instance, the substituted forms for two representative tile
instances) findable in those two files into one committed FASM fixture;
see the comments inside that file for exactly where each line came from
and how any placeholder was substituted.

### Origin

* Repository: <https://github.com/verilog-to-routing/vtr-verilog-to-routing>
* Commit: `34f65cb7c14dd4a59d5b95a5d65920dcc0d9a89b` (from a local read-only,
  blob-less checkout at session time; `git -C <checkout> rev-parse HEAD`).
* Licence: MIT (see `LICENSE.md` in that repository; VTR itself notes ABC,
  benchmark circuits and some libraries are under other licences, none of
  which are involved in this file -- `utils/fasm/` is VTR's own MIT
  licensed code).
* Paths: `utils/fasm/test/test_fasm.cpp`, `utils/fasm/test/test_fasm_arch.xml`.

No files from that checkout are copied verbatim; `vtr_test_fasm_literals.fasm`
is a hand written extraction (see its header comment) of the literal and
placeholder-substituted FASM strings found in those two files, done by
reading them at the commit above.

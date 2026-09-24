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

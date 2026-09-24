# End-to-end toolchain (T7.1)

Makes an open source Xilinx 7 series synthesis + place-and-route flow
available on this machine, so later tasks (T7.2, T7.3) can turn the
Verilog/LiteX designs in
[f4pga-examples](https://github.com/chipsalliance/f4pga-examples) and
[fpgas.online-test-designs](https://github.com/fpgas-online/fpgas.online-test-designs)
into FASM files for the Rust rewrite's differential tests. Neither of
those repositories contains any FASM itself -- they are plain
Verilog/LiteX sources that need synthesis + place-and-route to produce
FASM in the first place.

This is a companion to, and deliberately separate from,
`tests/oracle/setup-xilinx.sh`: that script builds the *reference*
prjxray/prjuray C++ tools from source, pinned to exact commits, as a
byte-exact oracle for frames/bitstream differential tests (T5.9, T6.3).
This one installs an independent, pre-built toolchain whose job is
synthesis and place-and-route -- something the oracle tools cannot do (they
only convert FASM <-> frames <-> bitstream).

## What gets installed

Run once per machine:

```
tools/e2e/setup-openxc7.sh
```

It downloads and extracts, under the gitignored `tools/e2e/build/`
(nothing here is ever committed):

| Component | Source | Size (extracted) |
|---|---|---|
| nextpnr-xilinx, bbasm, fasm2frames, bit2fasm, xc7frames2bit, bitread, bittool, frame_address_decoder, gen_part_base_yaml, xc7patch, segmatch, prjxray-db (artix7/kintex7/spartan7/zynq7), nextpnr-xilinx-meta, bbaexport.py | [openXC7 snap](https://github.com/openXC7/openXC7-snap) `0.8.2` | ~1.3 GiB |
| yosys (+ the rest of OSS CAD Suite: nextpnr-{ice40,ecp5,generic}, icestorm, trellis, sby, verilator, ...) | [OSS CAD Suite](https://github.com/YosysHQ/oss-cad-suite-build) build `2026-09-21` | ~2.5 GiB |
| downloaded archives (kept, sha256-verified, for idempotent re-setup) | -- | ~0.9 GiB |
| chip database `xc7a35tcsg324-1.bin` (built on demand, see below) | generated from the snap's bundled prjxray-db | ~90 MiB |

Total after a default run: **~4.7 GiB**, well under the ~10 GiB budget for
this task. Pinned URLs and their sha256 are recorded in
`tools/e2e/setup-openxc7.sh`'s header comment and in
`tools/e2e/build/openxc7/status.json` after a run.

The openXC7 snap does **not** bundle yosys (its own `meta/snap.yaml`
description says so explicitly: *"This package does not include Yosys,
which needs to be installed separately"*) or any prebuilt chip database
`.bin` files -- only the source data (`prjxray-db`, `nextpnr-xilinx-meta`)
and the `bbaexport.py`/`bbasm` tools to build one per device on demand.
`setup-openxc7.sh` always builds the one needed by `run-counter.sh`
(`xc7a35tcsg324-1`); pass `--parts DEVICE[,DEVICE...]` to build others (see
"Chip database sizes and timings" below before requesting a large part).

`fpgas.online-test-designs`'s own `scripts/setup_toolchains.py` resolves
"latest OSS CAD Suite" through `api.github.com`, which is **not** reachable
from this machine (see the Rust rewrite's `docs/rewrite/LOG.md`,
2026-09-24 environment survey). `setup-openxc7.sh` instead hardcodes a
release tag verified to exist with `curl -sSIL` at the time this was
written (`2026-09-21`) and, if that ever stops resolving, falls back to
probing nearby dates directly against `github.com` release URLs (no API
needed) -- see `find_oss_cad_suite_url()` in the script.

## How the snap was made runnable

The snap's base is `core20`, confinement `classic`: its ELF binaries carry
`/snap/core20/current/lib64/ld-linux-x86-64.so.2` as their dynamic linker
(`PT_INTERP`). That path does not exist on a machine without snapd/core20
installed, so the kernel refuses to exec them at all
(`cannot execute: required file not found`) -- `ldd` still resolves every
*other* shared library fine, because the binaries carry a `$ORIGIN`
relative RPATH into the snap's own `usr/lib/x86_64-linux-gnu`; only the
interpreter path itself is the problem.

Rather than downloading and unpacking the `core20` snap too (another
~100+ MiB, and it would need to live at the literal absolute path
`/snap/core20/current/`, which this task should not be writing to),
`setup-openxc7.sh` runs `patchelf --set-interpreter /lib64/ld-linux-x86-64.so.2`
(the host's own dynamic linker) on every extracted binary under `usr/bin`
whose interpreter starts with `/snap/`. This machine's host glibc (Ubuntu
24.04, glibc 2.39) is new enough to run the snap's bundled binaries (built
against an older Ubuntu 20.04 base) without issue. Verified against:
`nextpnr-xilinx --version`/`--test`, `bbasm --help`, `xc7frames2bit --help`,
and the complete `yosys -> nextpnr-xilinx -> fasm2frames -> xc7frames2bit`
pipeline in `run-counter.sh`.

`nextpnr-xilinx --test` (architecture database integrity self-check) does
fail with `ERROR: Assert 'bel == bel2' failed in .../archcheck.cc:41` on
the `xc7a35tcsg324-1` chip database built here; this looks like a
pre-existing openXC7/nextpnr-xilinx issue independent of the interpreter
patch (the same assert would fire regardless of how the binary was
launched), and it does **not** block actual place-and-route: the full
`run-counter.sh` pipeline (which does not use `--test`) works and produces
a structurally valid FASM/frames/bitstream. Not investigated further here;
worth a note if T7.2/T7.3 hit design-specific PnR failures.

The snap's own python wrapper scripts (`bin/fasm2frames`, `bin/bit2fasm`,
`bin/fasm`) have a similar problem one level up: their shebang line *and*
their `sys.path.append()` calls are hardcoded to
`/snap/openxc7/current/...` (where snapd would have mounted the snap).
Since that literal string is the *entire* absolute prefix in both places,
a single `sed` substitution of `/snap/openxc7/current` for the real
extraction root fixes both at once -- see step 3 of
`tools/e2e/setup-openxc7.sh`. The rewritten copies are what
`tools/e2e/build/openxc7/bin/fasm2frames` etc. actually are.

No system libraries beyond `squashfs-tools` (`unsquashfs`) and `patchelf`
were needed; both are installed with `apt-get` if missing (see the "system
prerequisites" step in `setup-openxc7.sh`).

## Chip database sizes and timings

Measured on this machine (4 cores / 15 GiB RAM) building via
`bbaexport.py --device <device> --bba x.bba` then `bbasm --l x.bba x.bin`:

| Device | .bba (text) | .bin (chipdb) | Time | Peak RAM |
|---|---|---|---|---|
| `xc7a35tcsg324-1` | 266 MB | 89 MiB | ~6 s (bbaexport) + ~6 s (bbasm) | modest |
| `xc7a200tsbg484-1` | (>980 MB after 4+ min, killed before completion) | -- | **not completed**; >4 min and still growing | **>8 GiB and rising** |

Building the chip database for `xc7a35t` is cheap and is done by default.
Building it for `xc7a100t`/`xc7a200t` (needed by some fpgas.online and
f4pga-examples designs) is **not** attempted by default: on this machine's
4-core/15 GiB budget, `xc7a200t`'s `bbaexport.py` alone was still running
after 4+ minutes, already using over 8 GiB of RAM and still climbing, with
no yosys/nextpnr running concurrently. T7.2/T7.3 (which need these larger
parts) should budget a dedicated run for each (`tools/e2e/setup-openxc7.sh
--parts xc7a100tcsg324-1,xc7a200tsbg484-1`, run alone, likely several
minutes and several GiB of RAM each) and watch for OOM on a 15 GiB
machine -- this may need a bigger box, or an upstream fix/optimisation in
`bbaexport.py`, rather than being solved here.

Device names available (i.e. present in the bundled prjxray-db, so
*buildable* even though most are not built by default) for the target
parts named in the T7.1 brief:

* f4pga-examples targets: `xc7a35t*` (many packages, e.g.
  `xc7a35tcsg324-1` for Arty), `xc7a100t*`, `xc7a200t*` (all under
  `prjxray-db/artix7`), `xc7z010*` (under `prjxray-db/zynq7`).
* fpgas.online-test-designs targets: `xc7a35t*`, `xc7a100t*`, `xc7a200t*`
  (same `artix7` family).

All of these device directories exist in the snap's bundled
`opt/nextpnr-xilinx/external/prjxray-db/{artix7,zynq7}/`; only
`xc7a35tcsg324-1`'s chip database is actually built by this task.

## How to run

```
tools/e2e/setup-openxc7.sh          # once per machine (~100 s once archives are cached, ~5-6 min cold)
tools/e2e/run-counter.sh            # ~5 s: synth + PnR + frames + bitstream for the Arty counter example
```

`tools/e2e/openxc7-env.sh` (source, don't execute) puts the toolchain on
`PATH` and exports `OPENXC7_ROOT`, `PRJXRAY_DB_DIR`, `CHIPDB_DIR`,
`OPENXC7_PYTHON3` for ad-hoc use -- see its header comment for the exact
list and why `yosys` is added to `PATH` from its own directory rather than
symlinked (its launcher script resolves its own library directory via
`dirname "${BASH_SOURCE[0]}"`, which breaks under a symlink elsewhere).

`run-counter.sh`'s output (`top.json`, `top_routed.json`, `top.fasm`,
`top.frm`, `top.bit`, and each tool's log) lands under
`tools/e2e/build/out/counter/` (gitignored). The resulting `top.fasm` is
also checked into the corpus at
`tests/corpus/xilinx/artix7/designs/f4pga-examples/counter_test/arty_35/top.fasm`
(see that directory's `README.md` for exact tool versions/commits/commands
and output checksums); the design sources it was built from are checked
into `tests/e2e/designs/f4pga-examples/counter_test/` so the whole flow is
reproducible without an f4pga-examples checkout.

`tests/e2e/test_openxc7.py` is a pytest smoke test that skips cleanly when
`tools/e2e/setup-openxc7.sh` has not been run, and otherwise checks
`yosys -V` / `nextpnr-xilinx --version`, and that the checked-in
`top.fasm` parses with the original Python oracle
(`tests/oracle/venv/bin/python tests/oracle/dump.py`, from
`tests/oracle/setup.sh`) and has a few hundred lines.

## Known limitations

* Only `xc7a35tcsg324-1`'s chip database is built by default; `xc7a100t*`/
  `xc7a200t*`/`xc7z010*` need `--parts` and a machine with more headroom
  (see "Chip database sizes and timings").
* `nextpnr-xilinx --test`'s architecture integrity self-check fails on the
  `xc7a35t` chip database built here (see "How the snap was made
  runnable"); this did not block the actual PnR flow and was not
  investigated further.
* The prjxray-db copy bundled inside the openXC7 snap is not independently
  version-pinned the way `tools/fetch-db.sh` pins prjxray-db for the
  oracle -- it is whatever commit the snap 0.8.2 build shipped with (its
  own `README.md`/`Info.md` carry no version marker). `PRJXRAY_DB_DIR`
  (from `openxc7-env.sh`) points at this copy; T7.2/T7.3 should decide
  whether that is acceptable or whether they need `tools/fetch-db.sh`'s
  independently pinned copy instead for their comparisons.
* UltraScale/UltraScale+ (prjuray) are out of scope for this task and for
  the openXC7 snap (it only covers Xilinx 7 series: Spartan7, Artix7,
  Kintex7, Zynq7).
* Only Linux x86_64 is covered (matches this machine and both the openXC7
  snap and OSS CAD Suite release matrices).

## fpgas.online-test-designs corpus (T7.2)

Builds as many of the Xilinx designs of
[fpgas.online-test-designs](https://github.com/fpgas-online/fpgas.online-test-designs)
as feasible with LiteX + the openXC7 flow above, and collects the
produced FASM (plus a reference `.frm`, regenerated with the byte-exact
oracle tools, not openXC7's own bundled copies) into
`tests/corpus/xilinx/artix7/designs/fpgas.online-test-designs/<design>/<board>/`.
Unlike T7.1's `counter_test` (plain Verilog), these are real LiteX SoC
targets (CPU + BIOS for several of them) -- see each design's own
`designs/<design>/README.md` in that repository for what it verifies.

### Setup

```
tools/e2e/setup-openxc7.sh                            # T7.1, once (xc7a35tcsg324-1 always)
tools/e2e/setup-openxc7.sh --parts xc7a35tfgg484-2     # NeTV2 (from the MAIN tree, /home/user/fasm)
tools/e2e/setup-openxc7.sh --parts xc7a100tfgg484-2    # LiteFury          (same)
tools/e2e/setup-openxc7.sh --parts xc7a200tfbg484-3    # Acorn CLE-215+    (same; see below)
tools/e2e/setup-litex.sh                               # pinned LiteX venv, tools/e2e/build/litex-venv
```

A pinned checkout of fpgas.online-test-designs at
`tools/e2e/build/fpgas.online-test-designs` (gitignored) is also required;
it was made with (git operations against a path outside this checkout are
restricted in the agent sandbox this session ran in, hence the tarball
detour rather than a plain `git clone` into `tools/e2e/build/`):

```
git clone https://github.com/fpgas-online/fpgas.online-test-designs.git /path/to/scratch/fpgas-src
git -C /path/to/scratch/fpgas-src checkout 37d24079b28179558632abc12fd92af4ff00a036
cp -a /path/to/scratch/fpgas-src/. tools/e2e/build/fpgas.online-test-designs/
echo 37d24079b28179558632abc12fd92af4ff00a036 > tools/e2e/build/fpgas.online-test-designs/.checkout-commit
```

`tools/e2e/setup-litex.sh` installs LiteX/migen/litex-boards/litedram/
liteeth/litepcie/litespi/litescope/pythondata-cpu-vexriscv and the two
pythondata-software-* packages into `tools/e2e/build/litex-venv`, pinned
to the exact git commits fpgas.online-test-designs' own `uv.lock` uses
(see the script's header comment). `--with-riscv-gcc` additionally fetches
the xpack RISC-V GCC cross-compiler (~100 MB) for the few designs that
have no way to skip software compilation (see "Designs" below); most
designs are built with `--no-compile-software` instead, since only the
gateware/FASM matters for this task, not a working BIOS.

### Running

```
tools/e2e/run-fpgas-online.sh --list              # every known design:board pair, part, extra args
tools/e2e/run-fpgas-online.sh DESIGN BOARD         # build one (output under tools/e2e/build/out/fpgas-online/)
tools/e2e/install-fpgas-online-corpus.sh DESIGN BOARD   # copy the result into the corpus + write its README.md
```

### A LiteX chipdb-naming quirk (Arty a7-35)

`litex/build/xilinx/yosys_nextpnr.py`'s `finalize()` derives the chipdb
filename it looks for (`$CHIPDB/<dbpart>.bin`) from the platform's *raw*,
pre-normalisation `device` string via a regex
(`xc7([aksz])([0-9]+)(.*)-([0-9])`), not from the corrected `--part` name.
For most parts this just strips the trailing `-<speedgrade>` (e.g.
`xc7a35tfgg484-2` -> `xc7a35tfgg484`), matching `setup-openxc7.sh --parts`
naming. But Digilent Arty's `a7-35` variant has raw device
`xc7a35ticsg324-1L` (an industrial-temperature/low-power grade LiteX
itself has to special-case elsewhere for openXC7's `--part`/`--device`
flags) -- the regex mis-parses it and asks for `xc7a35icsg324.bin`
(missing the `t`), which will never exist. Rather than replicate that
parsing bug per board or let LiteX's own hardcoded-`/snap/openxc7/current`
auto-generation path run (unreliable in this environment -- see
"Requires, in order" above), `run-fpgas-online.sh` probes LiteX's own
"Chip database file '...' not found" error message on a first attempt and
symlinks the real, pre-built chipdb under whatever name LiteX actually
asked for, then retries once. This is transparent and self-correcting;
nothing under the shared main-tree `tools/e2e/build/openxc7/chipdb/`
install is ever touched or duplicated, only a per-worktree
`tools/e2e/build/chipdb-overlay/` of symlinks.

### A Yosys/abc9 `$buf` cell workaround

Beyond the `$scopeinfo` strip fpgas.online-test-designs' own
`designs/_shared/yosys_workarounds.py` already applies for openXC7
builds, every SoC design (those with a CPU) additionally hit:

```
ERROR: Unable to place cell '$auto$rtlil_bufnorm.cc:462:bufNormalize$...', no Bels remaining of type '$buf'
```

This machine's pinned Yosys (OSS CAD Suite `2026-09-21`) + `-abc9` flow
leaves stray RTLIL "buffer normal form" `$buf` pass-through cells behind
that nextpnr-xilinx has no Bel type for. Unlike `$scopeinfo` (debug
annotations, safe to `delete`), a `$buf` cell's output wire would be left
undriven by a bare `delete` -- instead, `techmap -map +/techmap.v t:$buf`
(inserted into the same local copy of
`designs/_shared/yosys_workarounds.py` used to add the `$scopeinfo`
strip, right before it) resolves each `$buf` into a plain connection.
This is a **local, uncommitted patch** to the pinned checkout under
`tools/e2e/build/` (gitignored) -- it is not part of
fpgas.online-test-designs upstream. To reproduce a fresh checkout that
still needs it, add this one line to that file's
`YOSYS_TEMPLATE_STRIP_SCOPEINFO` construction, right before the
`$scopeinfo` delete:

```python
YOSYS_TEMPLATE_STRIP_SCOPEINFO.insert(_i, "techmap -map +/techmap.v t:$buf")
```

The pure-gateware designs (`pmod-loopback`, `pmod-pin-id`) never hit this
(no CPU, much smaller/simpler netlists); every SoC design did.

### Designs attempted

| Design | Board | Part | Outcome | FASM lines | LiteX build time |
|---|---|---|---|---|---|
| pmod-loopback | arty | xc7a35tcsg324-1 | built | 294 | 6s |
| pmod-loopback | netv2 | xc7a35tfgg484-2 | built | 37 | 5s |
| pmod-pin-id | arty | xc7a35tcsg324-1 | built | 34578 | 22s |
| pmod-pin-id | netv2 | xc7a35tfgg484-2 | built | 2120 | 6s |
| pmod-pin-id | litefury | xc7a100tfgg484-2 | built | 4029 | 14s |
| uart | arty | xc7a35tcsg324-1 | built (`--no-compile-software`) | 85331 | 62s |
| uart | netv2 | xc7a35tfgg484-2 | built (`--no-compile-software`) | 86529 | 83s |
| uart | litefury | xc7a100tfgg484-2 | built (`--no-compile-software`) | 88358 | 74s |
| spi-flash-id | arty | xc7a35tcsg324-1 | built (`--no-compile-software`) | 57204 | 47s |
| spi-flash-id | netv2 | xc7a35tfgg484-2 | built (`--no-compile-software`) | 59106 | 63s |
| spi-flash-id | litefury | xc7a100tfgg484-2 | built (`--no-compile-software`) | 56354 | 57s |
| ethernet-test | arty | xc7a35tcsg324-1 | built (`--no-compile-software`) | 252344 | 205s |
| ethernet-test | netv2 | xc7a35tfgg484-2 | built (`--no-compile-software`) | 310255 | 227s |
| ddr-memory | arty | xc7a35tcsg324-1 | built (`--no-compile-software`) | 189674 | 145s |
| ddr-memory | netv2 | xc7a35tfgg484-2 | built (`--no-compile-software`) | 273281 | 205s |
| pcie-enumeration | netv2 | xc7a35tfgg484-2 | **failed** -- see below | -- | -- |
| pmod-pin-id | acorn | xc7a200tfbg484-3 | built | 4057 | 31s |
| uart | acorn | xc7a200tfbg484-3 | built (`--no-compile-software`) | 89363 | 93s |
| spi-flash-id | acorn | xc7a200tfbg484-3 | built (`--no-compile-software`) | 56825 | 71s |
| acorn-pcie | acorn | xc7a200tfbg484-3 | **failed** -- see below | -- | -- |

Not attempted: `fomu`/`tt` variants of every design (Lattice iCE40, out of
scope for this Xilinx-only task).

### `pcie-enumeration` / `acorn-pcie` (GTP/PCIe): failure, precisely

`pcie-enumeration/netv2` (`--variant a7-35`) needed two additional fixes
just to reach synthesis (neither GTP-related, both applied in
`run-fpgas-online.sh`, safe for every other design too):

1. This script builds a `Builder` directly (not through
   `build_soc`/`LiteXArgumentParser`'s `--no-compile-software`), so it
   always tries to compile the BIOS -- needing a RISC-V cross compiler on
   `PATH`. Fixed by installing one (`tools/e2e/setup-litex.sh
   --with-riscv-gcc`, xpack RISC-V GCC 14.2.0-3) and adding its `bin/` to
   `PATH`.
2. `Builder._check_meson()` then requires `meson`/`ninja` on `PATH` (they
   are pip-installed into `tools/e2e/build/litex-venv` by
   `setup-litex.sh` already, just not on `PATH` outside the venv). Fixed
   by adding the venv's `bin/` to `PATH` too.

With both fixed, synthesis itself fails:

```
ERROR: Module `\pcie_s7' referenced in module `\kosagi_netv2' in cell `\pcie_s7' is not part of the design.
```

`pcie_s7` is LitePCIe's Xilinx Series-7 PCIe PHY wrapper
(`litepcie.phy.s7pciephy`), which instantiates Xilinx's `PCIE_2_1` hard
IP block via a Vivado-generated, IP-catalogue-specific wrapper module
that is never present as plain Verilog for Yosys to read -- Vivado
normally supplies it from its own IP catalogue at synthesis time. There
is no open source implementation of this wrapper for nextpnr-xilinx/
openXC7 to synthesize against (unlike LUTs/FFs/BRAM/DSP, the `PCIE_2_1`
hard block's internal netlist is not part of prjxray's reverse-engineered
database). This is a genuine, structural GTP/PCIe-hard-IP gap in the
openXC7 flow, not a configuration problem -- consistent with the T7.2
brief's expectation that these designs "may fail with openXC7". Not
investigated further (would need an open source `PCIE_2_1` model, out of
scope for this task).

`acorn-pcie/acorn` (`--variant cle-215+`) fails even earlier, before
synthesis, for a related but distinct reason:

```
AttributeError: 'XilinxYosysNextpnrToolchain' object has no attribute 'pre_placement_commands'
```

`litepcie.phy.s7pciephy.S7PCIEPHY.add_gt_loc_constraints()` (called from
`acorn_pcie_soc.py`'s `AcornPCIeSoC.__init__`, to pin the PCIe GTP
channel location) reads
`self.platform.toolchain.pre_placement_commands`, an attribute LiteX's
Vivado toolchain class provides (to inject a Vivado Tcl constraint) but
the `openxc7`/`yosys+nextpnr` toolchain class
(`XilinxYosysNextpnrToolchain`) does not -- confirming litepcie's Xilinx
Series-7 PHY integration code is Vivado-only, independent of and prior to
the `pcie_s7` blackbox-module gap `pcie-enumeration` hits. Neither is a
configuration problem to work around; both are genuine gaps in
openXC7/LiteX's support for Xilinx PCIe hard IP. Not investigated
further, same rationale as `pcie-enumeration` above.

### Larger parts: xc7a100t (LiteFury) and xc7a200t (Acorn CLE-215+)

Per T7.1's measurements (`xc7a200t`'s chip database export alone exceeded
8 GiB RAM and was still climbing after 4+ minutes on this machine),
`xc7a100tfgg484-2`'s chipdb was built first, timeboxed and RAM-watched:

```
tools/e2e/setup-openxc7.sh --parts xc7a100tfgg484-2
```

completed in 156s, peak RSS ~4.0 GiB (well under
this machine's 16 GiB) -- **no memory pressure issue on this machine**,
unlike T7.1's xc7a200t note; three LiteFury designs (uart, spi-flash-id,
pmod-pin-id, `--variant cle-101`) were then built successfully (see
table above). Building it also surfaced a second `_acorn.py`-specific
quirk: `uart_soc_acorn.py`/`spiflash_soc_acorn.py`/`pmod_pin_id_acorn.py`
all hardcode `"acorn"` as their own build subdirectory name regardless of
`--variant` (LiteFury/NiteFury/Acorn CLE-215+ share one gateware script
per design), so `run-fpgas-online.sh` maps the `litefury` board name to
`build/acorn/` when locating the produced FASM.

`xc7a200tfbg484-3` (Acorn CLE-215+; note this is the litex-boards default
device for the `cle-215+` variant, `xc7a200t-fbg484-3` with the dash
stripped -- *not* `xc7a200tsbg484-2` as an earlier draft of this task
assumed):

```
ulimit -v 12000000   # ~11.4 GiB virtual memory cap, so a runaway export
                      # fails cleanly instead of OOM-killing the machine
tools/e2e/setup-openxc7.sh --parts xc7a200tfbg484-3
```

completed successfully in 349s, producing a 317 MiB chipdb, peak RSS
~8.5 GiB (this machine has 16 GiB total). This **succeeded** on this
machine, unlike T7.1's report of the same part's `bbaexport.py` alone
still climbing past 8 GiB and unfinished after 4+ minutes -- plausibly a
difference in available headroom between sessions (concurrent load from
other work) rather than a hard limit; `ulimit -v 12000000` was in effect
the whole time and never triggered. Acorn CLE-215+ designs (uart,
spi-flash-id, pmod-pin-id, `--variant cle-215+`) then built successfully
too -- see the table above.

### Tests

`tests/e2e/test_fpgas_online.py`: for every committed design/board in
the corpus, its FASM parses cleanly (tries `target/release/fasm`, then an
installed `fasm` Python package, then the pristine oracle -- whichever is
available) and is a substantial number of lines (not empty/truncated).
Two further checks are written to activate automatically once their
prerequisites land in this checkout, skipping cleanly until then:

* a byte-for-byte comparison of `target/release/fasm2frames`'s output
  against each committed reference `.frm` -- skipped everywhere right
  now, since the Rust `fasm-xilinx` frame assembler / `fasm2frames` CLI
  (T5.4/T5.5) is `[r]` in `docs/rewrite/TASKS.md` (in review, not yet
  merged into this branch);
* running `tools/difftest-xilinx.py` (T5.9's frames differential test
  driver) over the whole corpus -- also not present in this checkout yet
  (same reason), so also skipped, per this task's brief.

30 passed / 16 skipped, ~76s (`pytest tests/e2e/test_fpgas_online.py -v`)
against the 18 built design/board pairs above.

### Corpus summary

18 design/board pairs built and committed, 2 failed for documented,
structural (not configuration) reasons (`pcie-enumeration`/netv2,
`acorn-pcie`/acorn -- both need Xilinx PCIe hard IP openXC7 does not
support). Corpus on-disk size:
`tests/corpus/xilinx/artix7/designs/fpgas.online-test-designs/` is 11 MiB
total. FASM plain or `xz`'d over 1 MiB, `.frm` (dense + sparse) always
`xz`'d, no `.bit` files ever committed (not byte-reproducible -- embeds a
build timestamp; each design's own README.md records its sha256
instead).



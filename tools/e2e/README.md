# End-to-end toolchains (T7.1, T7.2, T7.3, T7.6)

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
* The prjxray-db copy bundled inside the openXC7 snap is not pinned to
  the same commit as `tools/fetch-db.sh`'s independently fetched
  prjxray-db for the oracle -- it is whatever commit the snap 0.8.2 build
  shipped with. **Correction (T7.2 review):** an earlier version of this
  bullet claimed its own `README.md`/`Info.md` "carry no version marker
  either" -- that is wrong: `Info.md` does record one (`Info.md`: "Created
  using Project X-Ray version 4c157493, last updated Tue Dec 14 07:31:38
  PM UTC 2021"; full commit `4c157493ec9f13caea4ad3f0c02f8f318f198846`).
  It is simply a *different, independent* pin from `tools/fetch-db.sh`'s.
  `PRJXRAY_DB_DIR` (from `openxc7-env.sh`) points at this copy; T7.2 does
  use it (deliberately -- it is the database nextpnr-xilinx's own chipdb
  and the whole LiteX openxc7 flow are built against, so it is the
  correct database for reproducing what openXC7 itself did) -- see
  "A note on prjxray-db provenance" in the T7.2 section below for the
  exact, verified differences against the pinned copy and why they
  matter for some designs' FASM.
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

### A note on prjxray-db provenance

**Fixed after T7.2 review; read this before comparing any `.frm` here
against a different prjxray-db.** Every `.frm`/`.bit` in this corpus was
regenerated with the oracle tools (`tests/oracle/{fasm2frames,
xc7frames2bit,bitread}-oracle`) run against `$PRJXRAY_DB_DIR` from
`tools/e2e/openxc7-env.sh` -- i.e.
`tools/e2e/build/openxc7/root/opt/nextpnr-xilinx/external/prjxray-db`,
**the openXC7 snap's own bundled copy**. This is deliberate, not an
oversight: it is the exact database nextpnr-xilinx's chipdb and the whole
LiteX openxc7 flow are built against for these designs, so it is the
database that reproduces what openXC7 itself actually did, bit for bit.

It is **not** the independently pinned `f4pga/prjxray-db` that
`tests/oracle/setup-xilinx.sh`'s own `tools/fetch-db.sh` fetches for the
rest of this repository's Xilinx differential tests (`tests/oracle/build/db/prjxray-db`,
what `tests/oracle/xilinx-env.sh`'s `PRJXRAY_DB_ROOT` points at). The two
are different, independently maintained pins of the same underlying
Project X-Ray reverse-engineering project and are **not always
identical**.

**Provenance of the snap's copy:** openXC7 snap `0.8.2`
(sha256 `6b2e07ce99ef33d3a4e41e2fd2eb916f26bb0ece34a97216ed840a0032e98587`,
same as pinned in `tools/e2e/setup-openxc7.sh`). Its bundled
`prjxray-db/Info.md` records: *"Created using Project X-Ray version
[4c157493](https://github.com/SymbiFlow/prjxray/commit/4c157493ec9f13caea4ad3f0c02f8f318f198846),
last updated Tue Dec 14 07:31:38 PM UTC 2021"* -- this **does** carry a
version marker (an earlier draft of this README's "Known limitations"
section, written for T7.1, incorrectly claimed it did not; corrected
above).

**Verified differences (artix7 family), diffed directly against
`tests/oracle/build/db/prjxray-db/artix7` on this machine:**

| File | Difference |
|---|---|
| `segbits_cfg_center_mid.db` (+ its `.origin_info.db` companion) | snap has an extra line: `CFG_CENTER_MID.STARTUP.USRCCLKO_CONNECTED 26_2196 27_2197 27_2198` (also a harmless line-order difference on `ICAP_WIDTH_X16`, not a content difference) |
| `segbits_gtp_common.db` | snap has an extra line: `GTP_COMMON.GTPE2_COMMON.GTGREFCLK0_USED 28_1438 28_1439 29_1438` |
| `segbits_lioi3.db`, `segbits_lioi3_tbytesrc.db`, `segbits_rioi3_tbytesrc.db` | snap has extra `IOI_OCLKM_0`/`IOI_OCLKM_1` entries |
| `ppips_cfg_center_bot.db`, `ppips_cfg_center_mid.db`, `ppips_cfg_center_top.db` | present **only** in the snap db (the pinned db has no `ppips_cfg_center_*.db` files at all) -- the `CFG_CENTER_STARTUP_*` pseudo-PIPs |

(Spot-checked directly for `segbits_cfg_center_mid.db`, `segbits_gtp_common.db`
and the three `ppips_cfg_center_*.db` files this session; the
`segbits_lioi3*`/`rioi3_tbytesrc` entries are as reported by the T7.2
review and not independently re-diffed here.)

**Which designs this actually affects:** `spi-flash-id` (all four boards
-- arty/netv2/litefury/acorn) routes its SPI clock through `STARTUPE2`'s
`USRCCLKO` pin (see `designs/spi-flash-id/gateware/*.py`'s own
docstring: *"Clock routed via STARTUPE2"*), which sets
`CFG_CENTER_MID.STARTUP.USRCCLKO_CONNECTED` -- the exact tag only present
in the snap db above. A differential test that assembles `spi-flash-id`'s
FASM against the *pinned* db instead will therefore legitimately fail to
find that tag (`FasmLookupError` or equivalent), not because of a bug in
the Rust rewrite. Every other design in this corpus does not exercise
`STARTUPE2`/`GTP`/the `CFG_CENTER` ppips and was independently verified
identical against the Rust `fasm2frames` with *both* databases where
applicable, and with the snap db everywhere (18/18 designs; see "Tests"
below).

**Recommendation for later tasks** (tracked by the orchestrator, not
implemented here): `tools/fetch-db.sh` gaining an `openxc7` source that
exposes the snap's bundled db at a stable, independent cache path (e.g.
alongside its `prjxray`/`prjuray` sources) would let this corpus's
differential tests select the right database by name instead of relying
on `tools/e2e/build/openxc7`'s specific layout, and would let a
future differential run pin *both* databases explicitly per FASM file.

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
(inserted into `designs/_shared/yosys_workarounds.py`, right before the
existing `$scopeinfo` delete) resolves each `$buf` into a plain
connection.

This fix is a **committed patch file**,
`tools/e2e/patches/fpgas-online-yosys-workarounds-buf.patch` (a standard
unified diff against `designs/_shared/yosys_workarounds.py` at the pinned
commit) -- it is not part of fpgas.online-test-designs upstream, so it is
kept here rather than edited into the pinned (gitignored, never
committed) checkout by hand. `tools/e2e/run-fpgas-online.sh` applies it
**automatically**, every run, to `tools/e2e/build/fpgas.online-test-designs`:
it checks the target file's own content for the patch's marker comment
first (not `patch`'s exit status -- GNU patch's `--forward` exits 1, not
0, for a hunk it skips as already applied, which would otherwise abort
this script under `set -e` after the first run) and only invokes
`patch -p1 --forward` when the marker is absent, so it is a safe,
idempotent no-op on every run after the first. Apply it by hand with:

```
patch -p1 -d tools/e2e/build/fpgas.online-test-designs --forward -r - \
  < tools/e2e/patches/fpgas-online-yosys-workarounds-buf.patch
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
prerequisites land in this checkout, skipping cleanly until then, and
**always** against the openXC7 snap's own bundled prjxray-db specifically
(see "A note on prjxray-db provenance" above) -- never the differently
pinned `tests/oracle` db:

* a byte-for-byte comparison of `target/release/fasm2frames`'s output
  against each committed reference `.frm`;
* running `tools/difftest-xilinx.py` (T5.9's frames differential test
  driver, `--db-cache` pointed at the snap db) over the one subset of
  this corpus its own hardcoded single-part-per-family table can validly
  cover (the arty-board, plain-text FASM files -- see the module
  docstring in `tests/e2e/test_fpgas_online.py` for why).

Neither `target/release/fasm2frames` nor `tools/difftest-xilinx.py`
exists in *this* checkout as committed (T5.4/T5.5, the Rust `fasm-xilinx`
frame assembler / `fasm2frames` CLI, is `[r]` in `docs/rewrite/TASKS.md`
-- in review, not yet merged into this branch), so both skip cleanly by
default: **36 passed / 19 skipped**, ~93s
(`pytest tests/e2e/test_fpgas_online.py -v`).

Verified once with `target/release/{fasm,fasm2frames}` symlinked in from
an already-built main-tree checkout (`ln -s /home/user/fasm/target/release/{fasm,fasm2frames} target/release/`,
not committed -- `target/` is gitignored): **54 passed / 1 skipped**
(only `test_difftest_xilinx_over_corpus` still skips, since
`tools/difftest-xilinx.py` genuinely is not present in this checkout).
All **18/18** `test_corpus_frames_match_rust_fasm2frames` cases passed --
the Rust `fasm2frames`, run against the snap db, reproduces every
committed dense `.frm` byte for byte, for every design/board in this
corpus, confirming the T7.2 review's own finding.

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

## f4pga-examples corpus (T7.3)

Builds every Xilinx 7 series example of
[f4pga-examples](https://github.com/chipsalliance/f4pga-examples)
(commit `13f11197b33dae1cde3bf146f317d63f0134eacf`) with the **f4pga**
flow (Yosys + VPR, the flow those examples are written for, not
openXC7), collects the flow's FASM, frames and bitstream, and compares
the Rust tools against them, against the flow's own tools and against
the oracle. Results matrix and timings:
`docs/rewrite/DESIGN-xilinx-db.md` §8.11.

### Setup

```
tools/e2e/setup-f4pga.sh                                   # conda env + xc7a50t_test + xc7z010_test
tools/e2e/setup-f4pga.sh --devices xc7a100t_test           # Arty A7-100T, Nexys 4 DDR
tools/e2e/setup-f4pga.sh --remove-devices xc7a50t_test     # make room
source tools/e2e/f4pga-env.sh                              # "conda activate xc7"
```

`setup-f4pga.sh` follows f4pga-examples' `docs/getting.rst` under
`tools/e2e/build/f4pga` (`$F4PGA_E2E_ROOT`; `F4PGA_INSTALL_DIR` of the
documentation, `FPGA_FAM=xc7`), everything pinned (see its header):

| Component | Pin | Size |
|---|---|---|
| conda environment `xc7` (yosys 0.27_29_g0f5e7c244 + symbiflow-yosys-plugins, vtr-optimized 8.0.0_5699_g25e723a24 (VPR, genfasm), prjxray-tools 0.1_3015_gae546d6b (xc7frames2bit, bitread), prjxray-db 0.0_257_g0a0adde, gcc-riscv64-elf-newlib 10.1.0, openFPGALoader, python 3.7.16) | explicit lock with md5s, `tools/e2e/f4pga/xc7-conda-explicit.txt` (77 packages, channels litex-hub + defaults) | 1.9 GiB |
| PyPI part (f4pga `e1cd038f`, prjxray `ae546d6b`, f4pga-xc-fasm `25dc605c` (xcfasm), fasm 0.0.2.post88, numpy, scipy, ...) | `tools/e2e/f4pga/xc7-pip-freeze.txt` | 0.3 GiB |
| symbiflow-arch-defs `20220920-124259`/`007d1c1`, `install-xc7` | sha256 in the script | 0.6 MiB |
| `xc7a50t_test` (arty_35, basys3) | sha256 in the script | 2.6 GiB |
| `xc7z010_test` (zybo) | sha256 in the script | 1.5 GiB |
| `xc7a100t_test` (arty_100, nexys4ddr) | sha256 in the script | 4.8 GiB |
| `xc7a200t_test` (nexys_video) | sha256 in the script | 10.5 GiB |

Almost all of a device package is one file, the VPR routing graph
`rr_graph_<device>.rr_graph.real.bin`. With this task's 6 GiB disk budget
the devices were installed one at a time (`--remove-devices`), and
`xc7a100t_test` was installed into a tmpfs (`--big-files-dir
/dev/shm/f4pga-t73`: the device directory lives there and
`arch/xc7a100t_test` is a symlink to it; lost at reboot, the script
reinstalls it when run again). Only the directory can be a symlink:
VPR maps the graph with the size `lstat()` gives for its path, so a
symlinked graph file fails with `mmap_file.cpp:34 size_ 73 is not a
multiple of capnp::word`. **`xc7a200t_test` was not installed**: its 10.5 GiB graph
fits neither the disk budget nor, next to VPR, this machine's 15 GiB of
RAM, so the two Nexys Video designs (`counter_test`, `litex_sata_demo`)
were not built.

Deviations from the documented procedure, none of which changes what is
installed:

* micromamba 2.3.2 (one static binary, pinned) instead of the unpinned
  `Miniconda3-latest` installer, creating the environment from the
  explicit lock of what `conda env create -f xc7/environment.yml`
  resolves to;
* the f4pga python package is installed from a git clone at the pinned
  commit: the documented `github.com/chipsalliance/f4pga/archive/<commit>.zip`
  URL is refused with HTTP 403 by this machine's egress proxy (git clones
  of the same repository are not);
* the downloaded archives are sha256 checked, extracted and deleted.

### Running

```
tools/e2e/run-f4pga-examples.sh --list                  # design, board, device, part, family
tools/e2e/run-f4pga-examples.sh DESIGN BOARD [...]      # build, into tools/e2e/build/out/f4pga-examples/
tools/e2e/run-f4pga-examples.sh --device xc7a50t_test   # every design of an installed device
tools/e2e/compare-f4pga-examples.py [DESIGN/BOARD ...]  # Rust vs the flow's outputs and tools, timings
python3 tools/difftest-xilinx.py --corpus-root tools/e2e/build/out/f4pga-examples ...   # see below
tools/e2e/install-f4pga-examples-corpus.py [DESIGN/BOARD ...]   # into the corpus
```

`run-f4pga-examples.sh` clones f4pga-examples at the pinned commit (with
its submodules) into `tools/e2e/build/f4pga-examples`
(`$F4PGA_EXAMPLES_DIR`) and runs each design's documented command (those
of f4pga-examples' `.github/scripts/build-examples.sh`): `TARGET=<board>
make -C <example>` for the Makefile examples (`counter_test` on
`arty_35` goes through `f4pga build --flow flow.json`, the others
through the `symbiflow_*` wrappers), LiteX's `arty.py --toolchain=symbiflow
--cpu-type {picorv32,vexriscv}` for `litex_demo`, the project F
`projf-makefiles/hello/hello-arty/{A..L}` designs. It keeps the flow's
`top.fasm` and `top.bit`, and reruns the flow's exact `xcfasm` command
line with `--frm_out` to keep its frames (`top.frm`; the flow writes them
to a temporary file). The `litex_demo` designs need the LiteX packages
of `xc7/litex_demo/requirements.txt` in the environment: the script
installs the 18 its Arty targets use, at the pinned commits, as shallow
clones (the full list also clones the pythondata-cpu packages of
blackparrot, rocket, microwatt, ..., several GiB of git history).

The flow's prjxray-db is the conda package `prjxray-db
0.0_257_g0a0adde`: prjxray-db commit `0a0added`, the same commit
`tools/fetch-db.sh` pins, and every database file is identical to
`tests/oracle/build/db/prjxray-db` (`diff -r`: only the repository's
top level README/LICENSE/Makefile files are not in the package). The
flow's reference tools are prjxray `ae546d6b` (C++ tools and python
package) and f4pga-xc-fasm `25dc605c`; the oracle's are prjxray
`c9f02d85` and the same f4pga-xc-fasm.

Both comparisons of the dense/sparse/pudc variants:

```
# against the oracle and the pinned database
python3 tools/difftest-xilinx.py --corpus-root tools/e2e/build/out/f4pga-examples \
  --oracle tests/oracle/fasm2frames-oracle --frames2bit-oracle tests/oracle/xc7frames2bit-oracle \
  --bitread-oracle tests/oracle/bitread-oracle --xcfasm-oracle tests/oracle/xcfasm-oracle
# against the flow's own tools and database
E=tools/e2e/build/f4pga/xc7/conda/envs/xc7
python3 tools/difftest-xilinx.py --corpus-root tools/e2e/build/out/f4pga-examples \
  --oracle tools/e2e/f4pga/fasm2frames-flow --frames2bit-oracle $E/bin/xc7frames2bit \
  --bitread-oracle $E/bin/bitread --xcfasm-oracle $E/bin/xcfasm --db-cache $E/share/symbiflow
```

### Quirks of the flow found here

* The environment's `bin/fasm2frames` (prjxray's console script) does
  not run: `ModuleNotFoundError: No module named 'utils'` (prjxray's pip
  package does not install its `utils/` directory). The flow itself only
  uses `xcfasm`; `tools/e2e/f4pga/fasm2frames-flow` runs the flow's
  `xc_fasm.fasm2frames` instead (with `python -I`: from the root of this
  repository `import fasm` would otherwise find this repository's
  `fasm/` package).
* The flow's `top.bit` header names the temporary `.frm` file of its
  `xcfasm` (`/tmp/tmpXXXXXXXX`) and the build time: not byte
  reproducible, as documented for `xcfasm` in `docs/rewrite/COMPAT.md`.
  Everything after the header is.
* The flow is deterministic: rebuilding `counter_test/arty_35` gives the
  committed FASM byte for byte (`tests/e2e/test_f4pga_examples.py`).

* `symbiflow_write_fasm` (the make and LiteX flows) runs `genfasm` in
  `/bin/bash -c` with more commands after it and without `set -e`: a
  `genfasm` that is killed (OOM here: `counter_test/arty_100` first came
  out as a 320 line FASM without routing) or fails leaves a truncated
  FASM, and the flow writes its bitstream and succeeds.
  `run-f4pga-examples.sh` checks each build with
  `tools/e2e/f4pga/check-genfasm.sh`: genfasm's own log (`fasm.log`, or
  `vpr_stdout.log` for `f4pga build`) must end with `Writing
  Implementation FASM: ...` and `The entire flow of VPR took ...`, and
  the build output must not hold a bash signal report naming genfasm.

### Tests

`tests/e2e/test_f4pga_examples.py` (109 tests): check-genfasm.sh on fake
genfasm runs (killed by SIGKILL/SIGBUS/SIGTERM/SIGSEGV/SIGABRT, failing,
succeeding; run through `/bin/bash -c` like the flow), the corpus metadata, that
`make xilinx-difftest` covers every entry (its `difftest.json`), the Rust
`fasm` parses every FASM and the Rust `fasm2frames --sparse
--emit_pudc_b_pullup` reproduces the flow's frames (committed
`vpr.frm.xz` or the recorded sha256) with the pinned database; with the
toolchain, the flow's tools run and `counter_test/arty_35` is rebuilt end
to end and compared (`F4PGA_EXAMPLES_DIR` or `F4PGA_EXAMPLES_BUILD=1`).
Each part skips cleanly without its prerequisites.

## nextpnr-xilinx examples corpus (T7.6)

Builds the example designs of nextpnr-xilinx itself
(`xilinx/examples`) and of the openXC7 organisation's demo repositories
([demo-projects](https://github.com/openXC7/demo-projects),
[primitive-tests](https://github.com/openXC7/primitive-tests)) with the
openXC7 snap toolchain above, following each example's own script or
Makefile (yosys `synth_xilinx` -> `nextpnr-xilinx --fasm` -> the snap's
`fasm2frames` -> the snap's `xc7frames2bit`, all with the snap's
prjxray-db), keeps the FASM and the snap tools' frames and bitstream, and
compares the Rust tools with them. Results matrix and timings:
`docs/rewrite/DESIGN-xilinx-db.md` §8.13.

### Sources (pinned in `run-nextpnr-examples.sh`)

* **nextpnr-xilinx**: the openXC7 fork's tag `0.8.2`
  (`dea2f28c67fd1193ec72d0ba586800285e4c3648`), the source the installed
  snap `0.8.2` was built from (openXC7-snap's `snapcraft.yaml`:
  `source-branch: 0.8.2`). Its `xilinx/examples` are identical to
  upstream gatecat/nextpnr-xilinx's (`xilinx-upstream`,
  `8f178fc6a6d4dfbc57bef66c3ccff34d558047d5`). The fork's `main`
  (`bc9b2346`) adds only `counter25` (Virtex-7 VC707, through RapidWright;
  not built).
* **openXC7/demo-projects** `c5246c583a7db3a73543af72e00193f2fa990d34`:
  the last commit before the demos followed the toolchain to the
  himbaechel based openXC7/nextpnr (`373b7643`), i.e. the last one
  written for this snap's generation. Its `regression/` cases (moved
  there from nextpnr-xilinx) are run like `regression/run.sh`.
* **openXC7/primitive-tests** `d29ee7c58bdad361c690298d0c1b22004f9f4c02`.
  Its `bscane2` is a LiteX build directory (`build/build_top.sh`, the
  generated `top.v`, `top.ys`, `top.xdc`): built with the commands of
  `build_top.sh`, `top.ys`'s absolute `read_verilog` path of the machine
  LiteX ran on replaced with `top.v`.
* openXC7's other repositories were checked: `iologic-tests` and
  `dsp-tests` target only Kintex-7 parts, `xc7k325t-blinky-nextpnr` and
  `xc7k325t-picosoc-nextpnr` are Kintex-7 too; `toolchain-nix` and
  `getting-started` (only a README) hold no designs, and
  `toolchain-installer`'s only design is a one-LED blinky written inline
  by its `tests/smoke-toolchain.sh` for the himbaechel toolchain (the same
  shape as the blinky examples above; not built).

### Running

```
tools/e2e/run-nextpnr-examples.sh --list        # every entry: part, chipdb, kind, availability
tools/e2e/run-nextpnr-examples.sh --fetch       # clone the pinned sources into tools/e2e/build/nextpnr-examples-src
tools/e2e/run-nextpnr-examples.sh [--build-chipdb] ID [ID ...]   # or --all
python3 tools/e2e/compare-nextpnr-examples.py [--json FILE] [ID ...]
python3 tools/e2e/install-nextpnr-examples-corpus.py [--compare-json FILE] [ID ...]
```

`ID` is `SOURCE/EXAMPLE/BOARD` (`nextpnr-xilinx/blinky/arty-a35`,
`openxc7-demo-projects/regression-clock-srcc-bufg/xc7a200tfbg484`, ...).
Each design is built in a copy (its directory copied, the rest of the
source tree linked, so `../openXC7.mk`, `../vexriscv/VexRiscv.v` and
`../attosoc/attosoc.v` resolve) under `$NEXTPNR_EXAMPLES_OUT/ID/`
(default `tools/e2e/build/out/nextpnr-examples`), which keeps `top.fasm`,
`top.frm` (the snap's `fasm2frames`, dense), `top.bit`, the logs,
`commands.txt` and `info.json` (status, times, sha256). Every tool run is
capped at `NEXTPNR_EXAMPLES_TIMEOUT` seconds (1200) and
`NEXTPNR_EXAMPLES_VMEM_KB` of virtual memory (10 GiB). The Makefile flows
run their own targets one by one (`make <project>.json`, `.fasm`,
`.frames`, `.bit`) with `CHIPDB` pointing at a directory of links named
as the Makefile expects (`<part without speed grade>.bin`) and
`PRJXRAY_DB_DIR`/`DB_DIR` at the snap's database; the nextpnr-xilinx
examples' `.sh` scripts are run with their commands (the chipdb path and
prjxray's `utils/fasm2frames.py`/`xc7frames2bit`/database mapped to the
snap's).

From a second working tree without its own toolchain, point
`OPENXC7_E2E_BUILD` (read by `openxc7-env.sh` too) at the main checkout's
`tools/e2e/build`, and `NEXTPNR_XILINX_DIR`, `OPENXC7_DEMOS_DIR`,
`OPENXC7_PRIMITIVE_TESTS_DIR` at existing checkouts at the pinned commits.

### Chip databases

A chipdb is per package: the speed grade only selects the prjxray-db part
directory, and those of one package are identical (`diff -r
xc7a100tfgg484-1 xc7a100tfgg484-2`, `xc7a200tfbg484-2 -3`), so a design
for `xc7a100tfgg484-1` uses setup-openxc7.sh's `xc7a100tfgg484-2.bin` and
the regression cases for `xc7a200tfbg484-2` use `xc7a200tfbg484-3.bin`.
Two packages had none; `--build-chipdb` builds them (bbaexport.py +
bbasm, like setup-openxc7.sh, into `$NEXTPNR_EXAMPLES_CHIPDB_DIR`,
default `tools/e2e/build/nextpnr-examples-chipdb`), as the demo
Makefiles' own chipdb rule would: `xc7a35tcpg236-1` (Basys 3) in 85 s,
89 MiB, and `xc7a100tfgg676-1` (QMTech Artix-7 board) in 152 s, 152 MiB.

### Workaround: `$buf` cells

This machine's Yosys (OSS CAD Suite `2026-09-21`) leaves `$buf` cells in
the `-abc9` netlists of almost every design, which nextpnr-xilinx `0.8.2`
cannot place (`no Bels remaining of type '$buf'`; the problem T7.2 met,
see "A Yosys/abc9 `$buf` cell workaround" above). So that each example's
own synthesis command stays unchanged, the script fixes the written
netlist instead: when it has `$buf` cells it runs `yosys -p 'read_json
X.json; techmap -map +/techmap.v t:$buf; write_json X.json'` before
place and route, and records it (`commands.txt`, the note in
`info.json` and the corpus README).

### What was not built

* nextpnr-xilinx `artyz7-20/blinky` (Zynq-7000 `xc7z020`), the
  `attosoc`/`blinky` examples for `xczu2cg` and `zcu104/blinky`
  (UltraScale+: nextpnr-xilinx writes no FASM there, they go through
  RapidWright's json2dcp and Vivado), and the regression cases
  `fdse-fdpe-undefined-init` and `lut_shared_pin` (`xc7z010`): other
  families, listed as `skip` by `--list`.
* demo-projects designs for Kintex-7, Spartan-7 and Zynq parts (every
  other directory; the brief covers Artix-7 parts only), and
  primitive-tests' `mmcm-blinky` (Spartan-7 `xc7s50csga324-1`),
  `mmcm-blinky-kintex`, `gtx_channel`, `gtx_common/internal-refclk`
  (Kintex-7 `xc7k70t`), `dsp-tests/*` and `iologic-tests/*` (Kintex-7
  `xc7k160t`/`xc7k325t`): all listed as `skip` by `--list`.
* Designs that nextpnr-xilinx `0.8.2` cannot place and route (the
  regression cases guard fixes made after it; the error is in each
  `info.json`): `litex-sata/alientek-davincipro` (`IBUFDS_GTE2 ... must
  be connected to a GTPE2_COMMON`), `gtp_common-external-refclk`
  (`Invalid global constant node 'INT_L_X0Y173/VCC_WIRE'`), and the
  regression cases `bufio-in-use`, `bufr-pad-site`, `bufr-sink-region`
  (no BUFIO/BUFR bels; the last two are placement-level cases,
  `no_route`, that would not give a FASM anyway: the script never installs
  such a case), `lutram-clkinv`, `lutram-ram64x1s` (no
  `RAM64X1S`), `iddr-four-iff-flops`, `srl-init` (`Invalid global
  constant node 'INT_L_X0Y113/GND_WIRE'`), `dsp-const-only-pins`
  (unroutable `CARRYCASCIN`), `dup-package-pin` (expected to fail) and
  `srl-wemux` (router still running after the 1200 s cap).

### Tests

`tests/e2e/test_nextpnr_examples.py`: without any toolchain, the corpus
metadata (README sha256 of the FASM and frames, difftest.json, the entry
in the script's table) and that `make xilinx-difftest` covers every
entry; with the Rust tools, `fasm` parses every FASM and `fasm2frames`
reproduces the flow's frames with the snap's database and, with the
pinned database, either the same frames or the error the README records;
with the toolchain and a nextpnr-xilinx checkout,
`nextpnr-xilinx/blinky/arty-a35` is rebuilt end to end (the flow is
deterministic: the FASM must be the committed one) and
`compare-nextpnr-examples.py` must pass on it. 109 tests: all pass in 16 s
with the toolchain, the Rust tools and both databases (`ORACLE_DIR`,
`FASM_DB_CACHE` and `OPENXC7_E2E_BUILD` pointing at the main checkout
from a second working tree, `NEXTPNR_XILINX_DIR` at a checkout); with
the Rust tools but no database or toolchain 65 pass and 44 skip; with
nothing built 44 pass and 65 skip.

The regression cases also run their own `check.sh` like
`regression/run.sh` does (nextpnr's output in the case's `nextpnr.log`,
`CHIPDB` set); its verdict against nextpnr-xilinx 0.8.2 is in
`info.json` and the corpus README (`const-holdout` passes;
`bufh-clock-constraint` and `xorigport-unknown-name` fail as expected of
a nextpnr-xilinx without their fixes).

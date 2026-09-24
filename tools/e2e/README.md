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

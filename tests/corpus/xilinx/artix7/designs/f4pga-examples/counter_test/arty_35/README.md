# counter_test / arty_35 -- openXC7 end-to-end FASM (T7.1)

`top.fasm` is the FASM produced by placing and routing f4pga-examples'
`xc7/counter_test/counter.v` (design sources checked into this repo at
`tests/e2e/designs/f4pga-examples/counter_test/`, see the README there) for
the Digilent Arty A7-35T with the openXC7 toolchain, via
`tools/e2e/run-counter.sh` (see `tools/e2e/README.md` for the toolchain
setup, `tools/e2e/setup-openxc7.sh`). It is real, working FASM -- not
hand-written -- covering CLB slices (LUTs, FFs, CARRY4), BUFG, IBUF/OBUF
and interconnect for a small counter design.

Regenerate with:

```
tools/e2e/setup-openxc7.sh   # once per machine
tools/e2e/run-counter.sh
cp tools/e2e/build/out/counter/top.fasm \
   tests/corpus/xilinx/artix7/designs/f4pga-examples/counter_test/arty_35/top.fasm
```

## Target

* Part: `xc7a35tcsg324-1` (Digilent Arty A7-35T)
* Constraints: `arty.xdc` (clock on `E3`, `led[3:0]` on `H5`/`J5`/`T9`/`T10`)
* Top module: `top`

## Tool versions / commits / URLs (as resolved 2026-09-24)

* **yosys** (synthesis): OSS CAD Suite build `2026-09-21`,
  `Yosys 0.69+77 (git sha1 9ff27d29c-dirty, Release, Clang /usr/bin/clang++ 21.1.8)`
  <https://github.com/YosysHQ/oss-cad-suite-build/releases/download/2026-09-21/oss-cad-suite-linux-x64-20260921.tgz>
  sha256 `fc11a9c05c1de96b2821a5468ed02ba35253b278a78106cbe68167a5c419970c`
* **nextpnr-xilinx** (place & route), **bbasm** (chip database assembler),
  **fasm2frames**, **xc7frames2bit** (frames -> bitstream): openXC7 snap
  `0.8.2`, `nextpnr-xilinx -- Next Generation Place and Route (Version 0.8.2)`
  <https://github.com/openXC7/openXC7-snap/releases/download/0.8.2/openxc7_0.8.2_amd64.snap>
  sha256 `6b2e07ce99ef33d3a4e41e2fd2eb916f26bb0ece34a97216ed840a0032e98587`
  (see `tools/e2e/README.md` for how this snap's binaries were made to run
  without snapd/core20).
* **prjxray-db** (device database used by both nextpnr-xilinx's chip
  database and fasm2frames/xc7frames2bit): the copy bundled inside the
  openXC7 snap at `opt/nextpnr-xilinx/external/prjxray-db/artix7`
  (upstream <https://github.com/f4pga/prjxray-db>; the snap does not
  record which commit it was built from -- its own `README.md`/`Info.md`
  carry no version marker either -- so this is not independently pinned
  the way `tools/fetch-db.sh` pins prjxray-db for the oracle; it is simply
  "whatever prjxray-db 0.8.2's snap build shipped with").

## Commands (exactly as run by `tools/e2e/run-counter.sh`)

```
yosys -p "read_verilog counter.v; synth_xilinx -flatten -abc9 -nobram -arch xc7 -top top; write_json top.json"

nextpnr-xilinx --chipdb <chipdb>/xc7a35tcsg324-1.bin --xdc arty.xdc \
  --json top.json --write top_routed.json --fasm top.fasm

fasm2frames --db-root <prjxray-db>/artix7 --part xc7a35tcsg324-1 top.fasm top.frm

xc7frames2bit -frm_file top.frm -output_file top.bit \
  -part_name xc7a35tcsg324-1 -part_file <prjxray-db>/artix7/xc7a35tcsg324-1/part.yaml
```

Where `<chipdb>/xc7a35tcsg324-1.bin` was built by `bbaexport.py --device
xc7a35tcsg324-1` (from the snap's `opt/nextpnr-xilinx/python/`) piped
through `bbasm --l`.

## Output checksums (this run; `.bit` is NOT committed -- see below)

```
sha256  top.fasm  bf7f6a8a94641f26d176aeea82a71d6f33e79ddf210db9fcaa5be0291a77b320
sha256  top.frm   6013f01912aaff1b7461ceb5b350ab82694629eaa9ff8fe62dc086664591a359
sha256  top.bit   d35f2c625d9be3192150d7a589a400db47e25f653a90462e3a0167695f4661f8
```

`top.fasm` is 781 lines / 27651 bytes (well under the 1 MiB threshold this
task uses to decide whether to `xz -9` a corpus FASM file, so it is
committed as plain text). `top.frm` (5.4 MiB) and `top.bit` (2.1 MiB,
`.bit` headers embed a build timestamp and are therefore not byte
reproducible across runs -- same caveat as `tests/corpus/xilinx/artix7/README.md`
for the oracle's own smoke corpus) are NOT committed; regenerate them with
`tools/e2e/run-counter.sh` and compare their sha256 only where that matters
(the Rust rewrite's own differential tests, once `fasm-xilinx` exists,
should reproduce `top.frm`'s *content* -- frame addresses and words -- not
this exact file, since sparse vs dense `.frm` and any future prjxray-db
revision can both change immaterial details).

## Verified

* `top.fasm` parses cleanly with the original Python oracle:
  `tests/oracle/venv/bin/python tests/oracle/dump.py top.fasm` exits 0.
* `top.bit` was produced by the reference `xc7frames2bit` tool (from the
  openXC7 snap, itself built from prjxray's `tools/xc7frames2bit.cc`) --
  the same code path the oracle in `tests/oracle/setup-xilinx.sh` builds
  from source, so this is a genuine, structurally valid Series7 bitstream
  (not independently verified against real Arty A7 hardware in this
  session).

## Reference `.frm` files (T5.4/T5.5)

`top.frm.xz`, `top.sparse.frm.xz` and `top.pudc.frm.xz` (`xz -9`) are the
output of the oracle `fasm2frames` (f4pga-xc-fasm
`25dc605c9c0896204f0c3425b52a332034cf5e5c` on prjxray
`c9f02d8576042325425824647ab5555b1bc77833`, `tests/oracle/setup-xilinx.sh`)
with the pinned prjxray-db (`tools/fetch-db.sh prjxray artix7`), dense,
`--sparse` and `--emit_pudc_b_pullup`; the dense one is identical to the
openXC7 snap's `top.frm` above (same sha256). They are compared byte for
byte with the Rust `fasm2frames` output by
`rust/fasm-xilinx/tests/assembler_real_db.rs`:

```sh
DB=tests/oracle/build/db/prjxray-db/artix7
tests/oracle/fasm2frames-oracle --db-root $DB --part xc7a35tcsg324-1 top.fasm top.frm
tests/oracle/fasm2frames-oracle --db-root $DB --part xc7a35tcsg324-1 --sparse top.fasm top.sparse.frm
tests/oracle/fasm2frames-oracle --db-root $DB --part xc7a35tcsg324-1 --emit_pudc_b_pullup top.fasm top.pudc.frm
xz -9 top.frm top.sparse.frm top.pudc.frm
```

sha256 of the uncompressed files:

```
6013f01912aaff1b7461ceb5b350ab82694629eaa9ff8fe62dc086664591a359  top.frm
a0ff0016cd2b2b3f6fbf2e541a6cdb154ee48558c1500c09ef31314b0707bc51  top.sparse.frm
a1f670ac5e1c8be76e4ba65ad03d60ce4616945fb86a2828baa3f00561fef4de  top.pudc.frm
```

## The f4pga (VPR) flow FASM of the same design (T7.3)

`vpr.fasm` (with `vpr.frm.xz` and `difftest.json`) in this directory is
the same design built with the f4pga Yosys + VPR flow; see
`README.vpr.md`.

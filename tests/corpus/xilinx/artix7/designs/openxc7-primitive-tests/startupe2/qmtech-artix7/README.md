# openxc7-primitive-tests/startupe2/qmtech-artix7 -- nextpnr-xilinx (openXC7) FASM (T7.6)

`top.fasm` is the FASM nextpnr-xilinx wrote for openXC7 primitive-tests `startupe2` ([source](https://github.com/openXC7/primitive-tests/tree/d29ee7c58bdad361c690298d0c1b22004f9f4c02/startupe2), commit `d29ee7c58bdad361c690298d0c1b22004f9f4c02`), built by `tools/e2e/run-nextpnr-examples.sh openxc7-primitive-tests/startupe2/qmtech-artix7` following the example's own Makefile (see `tools/e2e/README.md`, "nextpnr-xilinx examples corpus (T7.6)"). `top.frm.xz` is the flow's frames: the snap's `fasm2frames`, dense, with the snap's prjxray-db.

## Target

* Part: `xc7a100tfgg676-1` (family `artix7`)
* Chip database: `xc7a100tfgg676-1.bin` (built from the snap's prjxray-db; the design's part)
* FASM: 799 lines, 30938 bytes
* Flow times on this machine (4 cores, s): buf_fix 0.77, fasm2frames 1.12, pnr 2.76, synth 2.95, xc7frames2bit 0.12

* Note: $buf cells removed with techmap (yosys workaround)

## Tools

* Toolchain: openXC7 snap `0.8.2`
  (<https://github.com/openXC7/openXC7-snap/releases/download/0.8.2/openxc7_0.8.2_amd64.snap>,
  sha256 `6b2e07ce99ef33d3a4e41e2fd2eb916f26bb0ece34a97216ed840a0032e98587`,
  `tools/e2e/setup-openxc7.sh`): nextpnr-xilinx -- Next Generation Place and Route (Version 0.8.2), built from
  openXC7/nextpnr-xilinx tag `0.8.2`
  (`dea2f28c67fd1193ec72d0ba586800285e4c3648`); the snap's `fasm2frames`
  (prjxray `utils/fasm2frames.py`) and `xc7frames2bit`.
* Synthesis: Yosys 0.69+77 (git sha1 9ff27d29c-dirty, Release, Clang /usr/bin/clang++ 21.1.8) (OSS CAD Suite `2026-09-21`; the snap has no yosys).
* Database: the snap's bundled prjxray-db (`Info.md`: Project X-Ray
  `4c157493`), not the pinned `tools/fetch-db.sh` copy; see
  `tools/e2e/README.md`, "A note on prjxray-db provenance".

## Commands

```
yosys  -p "synth_xilinx -flatten -abc9  -arch xc7 -top startup; write_json startup.json" startup.v
nextpnr-xilinx --chipdb <chipdb dir>/xc7a100tfgg676.bin --xdc startup.xdc --pack-only --json startup.json --write startup.pack.json
nextpnr-xilinx --chipdb <chipdb dir>/xc7a100tfgg676.bin --xdc startup.xdc --no-pack --no-route --json startup.pack.json --write startup.place.json
nextpnr-xilinx --chipdb <chipdb dir>/xc7a100tfgg676.bin --xdc startup.xdc --no-pack --no-place --json startup.place.json --fasm startup.fasm --write startup.route.json
fasm2frames --part xc7a100tfgg676-1 --db-root <snap prjxray-db>/artix7 startup.fasm > startup.frames
xc7frames2bit --part_file <snap prjxray-db>/artix7/xc7a100tfgg676-1/part.yaml --part_name xc7a100tfgg676-1 --frm_file startup.frames --output_file startup.bit
yosys -p 'read_json startup.json; techmap -map +/techmap.v t:$buf; write_json startup.json'   # after synthesis: the $buf workaround
```

## Reference outputs of the flow

```
sha256  top.fasm  91592327c9a9cc27ea2b84f2b1885eb5efda23e9e89ac1e8114c5470e95618c3
sha256  top.frm   e3c528076152e7cd8e1338319dab6af7940132718220d41b101def97c66459c2  (10111464 bytes)
sha256  top.bit   cfb2fb2abd0fb454ce40c1db11a3f5b532dea5914526b415316c2edb303cfdbf  (3825894 bytes)
```

`top.bit` is not committed: its header holds the build date and time and the `.frm` path.

## Comparison (`tools/e2e/compare-nextpnr-examples.py`)

* bitread = snap (11 flag sets): yes
* fasm  = oracle: yes
* fasm  = snap: yes
* fasm --canonical = oracle: yes
* fasm --canonical = snap: yes
* fasm2frames = flow (snap db, dense): yes
* fasm2frames = oracle (pinned db): yes
* fasm2frames pudc = oracle (snap db): yes
* fasm2frames pudc = snap: not applicable
* fasm2frames sparse = oracle (snap db): yes
* fasm2frames sparse = snap (snap db): yes
* pinned db = snap db frames: no
* xc7frames2bit = flow: yes
* xcfasm = flow (frm, bit): yes
* explained: fasm2frames --emit_pudc_b_pullup: the snap's fails (FasmLookupError, LVCMOS12_LVCMOS15_LVCMOS18_LVCMOS25_LVCMOS33_LVTTL_SSTL135_SSTL15.IN_ONLY not found)
* note: pinned db: exit code 1, Segment DB CFG_CENTER_MID, key CFG_CENTER_MID.CFG_CENTER_STARTUP_USRCCLKO.CFG_CENTER_CLK1_7 not found from line 'CFG_CENTER_MID_X46Y84.CFG_CENTER_STARTUP_USRCCLKO.CFG_CENTER_CLK1_7'

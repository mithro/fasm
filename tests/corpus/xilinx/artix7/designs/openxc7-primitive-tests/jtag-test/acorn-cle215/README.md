# openxc7-primitive-tests/jtag-test/acorn-cle215 -- nextpnr-xilinx (openXC7) FASM (T7.6)

`top.fasm` is the FASM nextpnr-xilinx wrote for openXC7 primitive-tests `clb-tests/jtag-test` ([source](https://github.com/openXC7/primitive-tests/tree/d29ee7c58bdad361c690298d0c1b22004f9f4c02/clb-tests/jtag-test), commit `d29ee7c58bdad361c690298d0c1b22004f9f4c02`), built by `tools/e2e/run-nextpnr-examples.sh openxc7-primitive-tests/jtag-test/acorn-cle215` following the example's own Makefile (see `tools/e2e/README.md`, "nextpnr-xilinx examples corpus (T7.6)"). `top.frm.xz` is the flow's frames: the snap's `fasm2frames`, dense, with the snap's prjxray-db.

## Target

* Part: `xc7a200tfbg484-3` (family `artix7`)
* Chip database: `xc7a200tfbg484-3.bin` (built from the snap's prjxray-db; the design's part)
* FASM: 818 lines, 29242 bytes
* Flow times on this machine (4 cores, s): buf_fix 0.71, fasm2frames 1.82, pnr 5.32, synth 3.19, xc7frames2bit 0.26

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
yosys -DTEST_RAM32X1D -p "synth_xilinx -flatten -abc9  -arch xc7 -top top; write_json top.json" top.v
nextpnr-xilinx --chipdb <chipdb dir>/xc7a200tfbg484.bin --xdc cle215.xdc  --pack-only --json top.json --write top.pack.json
nextpnr-xilinx --chipdb <chipdb dir>/xc7a200tfbg484.bin --xdc cle215.xdc  --no-pack --no-route --json top.pack.json --write top.place.json
nextpnr-xilinx --chipdb <chipdb dir>/xc7a200tfbg484.bin --xdc cle215.xdc  --no-pack --no-place --json top.place.json --fasm top.fasm --write top.route.json
fasm2frames --part xc7a200tfbg484-3 --db-root <snap prjxray-db>/artix7 top.fasm > top.frames
xc7frames2bit --part_file <snap prjxray-db>/artix7/xc7a200tfbg484-3/part.yaml --part_name xc7a200tfbg484-3 --frm_file top.frames --output_file top.bit
yosys -p 'read_json top.json; techmap -map +/techmap.v t:$buf; write_json top.json'   # after synthesis: the $buf workaround
```

## Reference outputs of the flow

```
sha256  top.fasm  b3117b0dcfd09bb5a9c2e45d039b748d1b37841794c3fbd79f4f1aaaf4a371f2
sha256  top.frm   4d3840d0bf015498f4f6bdc70dd876962b36347bfa20e5d5664573476382a453  (22698060 bytes)
sha256  top.bit   32235e7de1f09d072c684cb61c22643cd76668c6dc3e76a1badd6dbd5f6eea0b  (9730754 bytes)
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
* note: pinned db: exit code 1, Segment DB CFG_CENTER_MID, key CFG_CENTER_MID.CFG_CENTER_LOGIC_OUTS_B17_11.CFG_CENTER_BSCAN3_TDI not found from line 'CFG_CENTER_MID_X61Y136.CFG_CENTER_LOGIC_OUTS_B17_11.CFG_CENTER_BSCAN3_TDI'

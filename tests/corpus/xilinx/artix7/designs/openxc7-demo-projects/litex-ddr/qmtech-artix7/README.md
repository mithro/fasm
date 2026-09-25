# openxc7-demo-projects/litex-ddr/qmtech-artix7 -- nextpnr-xilinx (openXC7) FASM (T7.6)

`top.fasm.xz` is the FASM nextpnr-xilinx wrote for openXC7 demo-projects `litex-ddr-qmtech-artix7` ([source](https://github.com/openXC7/demo-projects/tree/c5246c583a7db3a73543af72e00193f2fa990d34/litex-ddr-qmtech-artix7), commit `c5246c583a7db3a73543af72e00193f2fa990d34`), built by `tools/e2e/run-nextpnr-examples.sh openxc7-demo-projects/litex-ddr/qmtech-artix7` following the example's own Makefile (see `tools/e2e/README.md`, "nextpnr-xilinx examples corpus (T7.6)"). `top.frm.xz` is the flow's frames: the snap's `fasm2frames`, dense, with the snap's prjxray-db.

## Target

* Part: `xc7a100tfgg676-1` (family `artix7`)
* Chip database: `xc7a100tfgg676-1.bin` (built from the snap's prjxray-db; the design's part)
* FASM: 196344 lines, 7876658 bytes
* Flow times on this machine (4 cores, s): buf_fix 1.19, fasm2frames 46.48, pnr 53.4, synth 25.58, xc7frames2bit 0.13

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
yosys -p "synth_xilinx -flatten -abc9  -arch xc7 -top qmtech_artix7_fgg676; write_json qmtech_artix7_fgg676.json" qmtech_artix7_fgg676.v ../vexriscv/VexRiscv.v
yosys -p 'read_json qmtech_artix7_fgg676.json; techmap -map +/techmap.v t:$buf; write_json qmtech_artix7_fgg676.json'   # after synthesis: the $buf workaround
nextpnr-xilinx --chipdb <chipdb dir>/xc7a100tfgg676.bin --xdc qmtech_artix7_fgg676.xdc --json qmtech_artix7_fgg676.json --fasm qmtech_artix7_fgg676.fasm
fasm2frames --part xc7a100tfgg676-1 --db-root <snap prjxray-db>/artix7 qmtech_artix7_fgg676.fasm > qmtech_artix7_fgg676.frames
xc7frames2bit --part_file <snap prjxray-db>/artix7/xc7a100tfgg676-1/part.yaml --part_name xc7a100tfgg676-1 --frm_file qmtech_artix7_fgg676.frames --output_file qmtech_artix7_fgg676.bit
```

## Reference outputs of the flow

```
sha256  top.fasm  8043f30a977aa818962c89e7f0f88b52633329da92aae6bd7a591ee2c4d8b76b
sha256  top.frm   e707f7d72d8a297b9bfbf9170774a997f05f96d88ee442cc77a845f6e85f938c  (10111464 bytes)
sha256  top.bit   fa435e36ecdd11f0778db973d1a9814f586721e2bf9839eca150c3f9da17c325  (3825907 bytes; header with the build time)
```

`top.bit` is not committed and its sha256 is not reproducible: its header holds the build date and time and the `.frm` path.

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
* pinned db = snap db frames: yes
* xc7frames2bit = flow: yes
* xcfasm = flow (frm, bit): yes
* explained: fasm2frames --emit_pudc_b_pullup: the snap's fails (FasmLookupError, LVCMOS12_LVCMOS15_LVCMOS18_LVCMOS25_LVCMOS33_LVTTL_SSTL135_SSTL15.IN_ONLY not found)

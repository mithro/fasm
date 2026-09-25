# openxc7-primitive-tests/pll-reconfig/qmtech-artix7 -- nextpnr-xilinx (openXC7) FASM (T7.6)

`top.fasm` is the FASM nextpnr-xilinx wrote for openXC7 primitive-tests `pll-reconfig` ([source](https://github.com/openXC7/primitive-tests/tree/d29ee7c58bdad361c690298d0c1b22004f9f4c02/pll-reconfig), commit `d29ee7c58bdad361c690298d0c1b22004f9f4c02`), built by `tools/e2e/run-nextpnr-examples.sh openxc7-primitive-tests/pll-reconfig/qmtech-artix7` following the example's own Makefile (see `tools/e2e/README.md`, "nextpnr-xilinx examples corpus (T7.6)"). `top.frm.xz` is the flow's frames: the snap's `fasm2frames`, dense, with the snap's prjxray-db.

## Target

* Part: `xc7a100tfgg676-1` (family `artix7`)
* Chip database: `xc7a100tfgg676-1.bin` (built from the snap's prjxray-db; the design's part)
* FASM: 4277 lines, 155403 bytes
* Flow times on this machine (4 cores, s): buf_fix 0.72, fasm2frames 1.47, pnr 3.5, synth 3.33, xc7frames2bit 0.11

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
yosys  -p "synth_xilinx -flatten -abc9  -arch xc7 -top pll_reconfig; write_json pll_reconfig.json" pll_reconfig.v xilinx7_pll_reconfig.v
yosys -p 'read_json pll_reconfig.json; techmap -map +/techmap.v t:$buf; write_json pll_reconfig.json'   # after synthesis: the $buf workaround
nextpnr-xilinx --chipdb <chipdb dir>/xc7a100tfgg676.bin --xdc pll_reconfig.xdc --pack-only --json pll_reconfig.json --write pll_reconfig.pack.json
nextpnr-xilinx --chipdb <chipdb dir>/xc7a100tfgg676.bin --xdc pll_reconfig.xdc --no-pack --no-route --json pll_reconfig.pack.json --write pll_reconfig.place.json
nextpnr-xilinx --chipdb <chipdb dir>/xc7a100tfgg676.bin --xdc pll_reconfig.xdc --no-pack --no-place --json pll_reconfig.place.json --fasm pll_reconfig.fasm --write pll_reconfig.route.json
fasm2frames --part xc7a100tfgg676-1 --db-root <snap prjxray-db>/artix7 pll_reconfig.fasm > pll_reconfig.frames
xc7frames2bit --part_file <snap prjxray-db>/artix7/xc7a100tfgg676-1/part.yaml --part_name xc7a100tfgg676-1 --frm_file pll_reconfig.frames --output_file pll_reconfig.bit
```

## Reference outputs of the flow

```
sha256  top.fasm  1e7c3dd7cd54d22c0335691f05a922b6e62d5429445c20d98c56c91c417a9cdf
sha256  top.frm   2904bc5c993d5445ebf4be0ec38ff0672ee09e6fc9144b1bd48967bcc66ca2e1  (10111464 bytes)
sha256  top.bit   788a7b57b5fe0d5b39bbf87e8b0594cbe02307f406eafe66b4044c7d3fb67ec8  (3825899 bytes; header with the build time)
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

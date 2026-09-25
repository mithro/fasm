# nextpnr-xilinx/attosoc/arty-a35 -- nextpnr-xilinx (openXC7) FASM (T7.6)

`top.fasm.xz` is the FASM nextpnr-xilinx wrote for nextpnr-xilinx `xilinx/examples/arty-a35/attosoc.sh` ([source](https://github.com/openXC7/nextpnr-xilinx/tree/dea2f28c67fd1193ec72d0ba586800285e4c3648/xilinx/examples/arty-a35), commit `dea2f28c67fd1193ec72d0ba586800285e4c3648`), built by `tools/e2e/run-nextpnr-examples.sh nextpnr-xilinx/attosoc/arty-a35` following the example's own script (see `tools/e2e/README.md`, "nextpnr-xilinx examples corpus (T7.6)"). `top.frm.xz` is the flow's frames: the snap's `fasm2frames`, dense, with the snap's prjxray-db.

## Target

* Part: `xc7a35tcsg324-1` (family `artix7`)
* Chip database: `xc7a35tcsg324-1.bin` (built from the snap's prjxray-db; the design's part)
* FASM: 25718 lines, 908013 bytes
* Flow times on this machine (4 cores, s): buf_fix 0.81, fasm2frames 5.15, pnr 6.83, synth 4.8, xc7frames2bit 0.06

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
yosys -p "synth_xilinx -flatten -nowidelut -abc9 -arch xc7 -top top; write_json attosoc.json" ../attosoc/attosoc.v attosoc_top.v
yosys -p 'read_json attosoc.json; techmap -map +/techmap.v t:$buf; write_json attosoc.json'   # after synthesis: the $buf workaround
nextpnr-xilinx --chipdb xc7a35tcsg324-1.bin --xdc arty.xdc --json attosoc.json --write attosoc_routed.json --fasm attosoc.fasm
fasm2frames --db-root <snap prjxray-db>/artix7 --part xc7a35tcsg324-1 attosoc.fasm > attosoc.frames
xc7frames2bit --part_file <snap prjxray-db>/artix7/xc7a35tcsg324-1/part.yaml --part_name xc7a35tcsg324-1 --frm_file attosoc.frames --output_file attosoc.bit
```

## Reference outputs of the flow

```
sha256  top.fasm  dd4bf11781003871e49fdf8a4aab2725bbca3458cc4cc9a2ccc11338add98d28
sha256  top.frm   dfca1faca62dfcdc8ea0914240fd259a8916c8d3e1f59160fa086aa326b3e5cf  (5580828 bytes)
sha256  top.bit   538e0d7154f174c76d2eba5faadb654a5ac675f2f27aea5f793bad8200baf552  (2192117 bytes; header with the build time)
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

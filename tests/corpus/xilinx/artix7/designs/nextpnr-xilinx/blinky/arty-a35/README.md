# nextpnr-xilinx/blinky/arty-a35 -- nextpnr-xilinx (openXC7) FASM (T7.6)

`top.fasm` is the FASM nextpnr-xilinx wrote for nextpnr-xilinx `xilinx/examples/arty-a35/blinky.sh` ([source](https://github.com/openXC7/nextpnr-xilinx/tree/dea2f28c67fd1193ec72d0ba586800285e4c3648/xilinx/examples/arty-a35), commit `dea2f28c67fd1193ec72d0ba586800285e4c3648`), built by `tools/e2e/run-nextpnr-examples.sh nextpnr-xilinx/blinky/arty-a35` following the example's own script (see `tools/e2e/README.md`, "nextpnr-xilinx examples corpus (T7.6)"). `top.frm.xz` is the flow's frames: the snap's `fasm2frames`, dense, with the snap's prjxray-db.

## Target

* Part: `xc7a35tcsg324-1` (family `artix7`)
* Chip database: `xc7a35tcsg324-1.bin` (built from the snap's prjxray-db; the design's part)
* FASM: 1046 lines, 38310 bytes
* Flow times on this machine (4 cores, s): buf_fix 0.68, fasm2frames 0.71, pnr 1.59, synth 2.92, xc7frames2bit 0.06

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
yosys -p "synth_xilinx -flatten -abc9 -nobram -arch xc7 -top top; write_json blinky.json" blinky.v
nextpnr-xilinx --chipdb xc7a35tcsg324-1.bin --xdc arty.xdc --json blinky.json --write blinky_routed.json --fasm blinky.fasm
fasm2frames --db-root <snap prjxray-db>/artix7 --part xc7a35tcsg324-1 blinky.fasm > blinky.frames
xc7frames2bit --part_file <snap prjxray-db>/artix7/xc7a35tcsg324-1/part.yaml --part_name xc7a35tcsg324-1 --frm_file blinky.frames --output_file blinky.bit
yosys -p 'read_json blinky.json; techmap -map +/techmap.v t:$buf; write_json blinky.json'   # after synthesis: the $buf workaround
```

## Reference outputs of the flow

```
sha256  top.fasm  be5c25cbfeee88994db4c02bdcf8591ee5c7653afae102c1806f29b520b0e35b
sha256  top.frm   ba464c67761b2098cbbec6cade73a0f62b8647bd9dbfa9ab67e0f9d486f876cb  (5580828 bytes)
sha256  top.bit   11f46dba7c459859d87d14f5b00dd8b98970ca9719db25cf652194a5885cac6a  (2192116 bytes)
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
* pinned db = snap db frames: yes
* xc7frames2bit = flow: yes
* xcfasm = flow (frm, bit): yes
* explained: fasm2frames --emit_pudc_b_pullup: the snap's fails (FasmLookupError, LVCMOS12_LVCMOS15_LVCMOS18_LVCMOS25_LVCMOS33_LVTTL_SSTL135_SSTL15.IN_ONLY not found)

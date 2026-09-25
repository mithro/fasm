# openxc7-primitive-tests/bscane2/qmtech-artix7 -- nextpnr-xilinx (openXC7) FASM (T7.6)

`top.fasm` is the FASM nextpnr-xilinx wrote for openXC7 primitive-tests `bscane2` ([source](https://github.com/openXC7/primitive-tests/tree/d29ee7c58bdad361c690298d0c1b22004f9f4c02/bscane2), commit `d29ee7c58bdad361c690298d0c1b22004f9f4c02`), built by `tools/e2e/run-nextpnr-examples.sh openxc7-primitive-tests/bscane2/qmtech-artix7` following the example's own LiteX build script (`build_top.sh`) (see `tools/e2e/README.md`, "nextpnr-xilinx examples corpus (T7.6)"). `top.frm.xz` is the flow's frames: the snap's `fasm2frames`, dense, with the snap's prjxray-db.

## Target

* Part: `xc7a100tfgg676-1` (family `artix7`)
* Chip database: `xc7a100tfgg676-1.bin` (built from the snap's prjxray-db; the design's part)
* FASM: 2371 lines, 81455 bytes
* Flow times on this machine (4 cores, s): buf_fix 0.72, fasm2frames 1.23, pnr 2.97, synth 3.03, xc7frames2bit 0.12

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
sed -i -e 's|^read_verilog .*/top\.v$|read_verilog top.v|' top.ys   # top.ys names LiteX's absolute path
yosys -l top.rpt top.ys
yosys -p 'read_json top.json; techmap -map +/techmap.v t:$buf; write_json top.json'   # after synthesis: the $buf workaround
nextpnr-xilinx --json top.json --xdc top.xdc --fasm top.fasm --chipdb xc7a100tfgg676-1.bin --write top_routed.json --timing-allow-fail --seed 1
fasm2frames --part xc7a100tfgg676-1 --db-root <snap prjxray-db>/artix7 top.fasm > top.frames
xc7frames2bit --part_file <snap prjxray-db>/artix7/xc7a100tfgg676-1/part.yaml --part_name xc7a100tfgg676-1 --frm_file top.frames --output_file top.bit
```

## Reference outputs of the flow

```
sha256  top.fasm  97373d59f82c3f6e60a702d61c7c332ada9dc0a48f69f8648e0e890c5e4da695
sha256  top.frm   cc3028d9cac67098cd255d009111324c6ba3aa642101e0a6672ff06c3d000106  (10111464 bytes)
sha256  top.bit   415d4000dfb0e380047fe2b7bf92d6fcb42ad23ebd995f41275e2e303f24a91c  (3825890 bytes; header with the build time)
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
* pinned db = snap db frames: no
* xc7frames2bit = flow: yes
* xcfasm = flow (frm, bit): yes
* explained: fasm2frames --emit_pudc_b_pullup: the snap's fails (FasmLookupError, LVCMOS12_LVCMOS15_LVCMOS18_LVCMOS25_LVCMOS33_LVTTL_SSTL135_SSTL15.IN_ONLY not found)
* note: pinned db: exit code 1, Segment DB CFG_CENTER_MID, key CFG_CENTER_MID.CFG_CENTER_LOGIC_OUTS_B14_3.CFG_CENTER_BSCAN1_TCK not found from line 'CFG_CENTER_MID_X46Y84.CFG_CENTER_LOGIC_OUTS_B14_3.CFG_CENTER_BSCAN1_TCK'

# openxc7-primitive-tests/gtp_channel/xc7a35tfgg484 -- nextpnr-xilinx (openXC7) FASM (T7.6)

`top.fasm` is the FASM nextpnr-xilinx wrote for openXC7 primitive-tests `gtp_channel` ([source](https://github.com/openXC7/primitive-tests/tree/d29ee7c58bdad361c690298d0c1b22004f9f4c02/gtp_channel), commit `d29ee7c58bdad361c690298d0c1b22004f9f4c02`), built by `tools/e2e/run-nextpnr-examples.sh openxc7-primitive-tests/gtp_channel/xc7a35tfgg484` following the example's own Makefile (see `tools/e2e/README.md`, "nextpnr-xilinx examples corpus (T7.6)"). `top.frm.xz` is the flow's frames: the snap's `fasm2frames`, dense, with the snap's prjxray-db.

## Target

* Part: `xc7a35tfgg484-2` (family `artix7`)
* Chip database: `xc7a35tfgg484-2.bin` (built from the snap's prjxray-db; the design's part)
* FASM: 1975 lines, 97513 bytes
* Flow times on this machine (4 cores, s): buf_fix 0.7, fasm2frames 3.11, pnr 1.73, synth 2.87, xc7frames2bit 0.07

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
yosys -p "hierarchy; synth_xilinx -flatten -abc9 -arch xc7  -top gtp_channel; write_json gtp_channel.json;" gtp_channel.v
yosys -p 'read_json gtp_channel.json; techmap -map +/techmap.v t:$buf; write_json gtp_channel.json'   # after synthesis: the $buf workaround
nextpnr-xilinx --chipdb <chipdb dir>/xc7a35tfgg484.bin --xdc gtp_channel.xdc  --json gtp_channel.json --write gtp_channel_routed.json --fasm gtp_channel.fasm  --freq 100 #--verbose --debug
fasm2frames --part xc7a35tfgg484-2 --db-root <snap prjxray-db>/artix7 gtp_channel.fasm > gtp_channel.frames
xc7frames2bit --part_file <snap prjxray-db>/artix7/xc7a35tfgg484-2/part.yaml --part_name xc7a35tfgg484-2 --frm_file gtp_channel.frames --output_file gtp_channel.bit
```

## Reference outputs of the flow

```
sha256  top.fasm  ef6bf6313bb5ac9cf0f5befeb5f542e5d2330d4708b955e041b1d96c019f9edc
sha256  top.frm   8ae5b140fb2fd0b56b1e8074d3e648cc2f0b8c65082f3415830bc2ed1b37d64e  (5580828 bytes)
sha256  top.bit   e858ee21e285972ec8cd928b50322ae9eec8807e396896c7ee5256ee8fbc95e6  (2192121 bytes; header with the build time)
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

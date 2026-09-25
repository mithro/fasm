# openxc7-primitive-tests/gtp_common-internal-refclk/xc7a100tfgg484 -- nextpnr-xilinx (openXC7) FASM (T7.6)

`top.fasm` is the FASM nextpnr-xilinx wrote for openXC7 primitive-tests `gtp_common/internal-refclk` ([source](https://github.com/openXC7/primitive-tests/tree/d29ee7c58bdad361c690298d0c1b22004f9f4c02/gtp_common/internal-refclk), commit `d29ee7c58bdad361c690298d0c1b22004f9f4c02`), built by `tools/e2e/run-nextpnr-examples.sh openxc7-primitive-tests/gtp_common-internal-refclk/xc7a100tfgg484` following the example's own Makefile (see `tools/e2e/README.md`, "nextpnr-xilinx examples corpus (T7.6)"). `top.frm.xz` is the flow's frames: the snap's `fasm2frames`, dense, with the snap's prjxray-db.

## Target

* Part: `xc7a100tfgg484-1` (family `artix7`)
* Chip database: `xc7a100tfgg484-2.bin` (built from the snap's prjxray-db; the same package and database files, the speed grade does not change a chipdb)
* FASM: 301 lines, 14609 bytes
* Flow times on this machine (4 cores, s): buf_fix 0.73, fasm2frames 5.12, pnr 2.25, synth 2.74, xc7frames2bit 0.12

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
yosys -p "hierarchy; synth_xilinx -flatten -abc9 -arch xc7  -top gtp_common; write_json gtp_common.json;" gtp_common.v
nextpnr-xilinx --chipdb <chipdb dir>/xc7a100tfgg484.bin --xdc gtp_common.xdc  --json gtp_common.json --write gtp_common_routed.json --fasm gtp_common.fasm  --freq 100 --verbose --debug
fasm2frames --part xc7a100tfgg484-1 --db-root <snap prjxray-db>/artix7 gtp_common.fasm > gtp_common.frames
xc7frames2bit --part_file <snap prjxray-db>/artix7/xc7a100tfgg484-1/part.yaml --part_name xc7a100tfgg484-1 --frm_file gtp_common.frames --output_file gtp_common.bit
yosys -p 'read_json gtp_common.json; techmap -map +/techmap.v t:$buf; write_json gtp_common.json'   # after synthesis: the $buf workaround
```

## Reference outputs of the flow

```
sha256  top.fasm  17a3dc7b4567554ce5054def8b51626851bf18e7f2edaecdc9a87ec1a8059e69
sha256  top.frm   5de3f16fca8fe30807125bbe9501f5d554c56b735814489ea47df21ac75d79ba  (10111464 bytes)
sha256  top.bit   5c1bab2d87dc2ec228a193c72ae7e954d3bf0d44d91b3fd8951b5677855247a6  (3825897 bytes)
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
* note: pinned db: exit code 1, prjxray.fasm_assembler.FasmLookupError: Segment DB GTP_COMMON, key GTP_COMMON.GTPE2_COMMON.GTGREFCLK0_USED not found from line 'GTP_COMMON_X130Y179.GTPE2_COMMON.GTGREFCLK0_USED'

# openxc7-demo-projects/regression-config-primitive-startupe2/xc7a200tfbg484 -- nextpnr-xilinx (openXC7) FASM (T7.6)

`top.fasm` is the FASM nextpnr-xilinx wrote for openXC7 demo-projects `regression/config-primitive-startupe2` ([source](https://github.com/openXC7/demo-projects/tree/c5246c583a7db3a73543af72e00193f2fa990d34/regression/config-primitive-startupe2), commit `c5246c583a7db3a73543af72e00193f2fa990d34`), built by `tools/e2e/run-nextpnr-examples.sh openxc7-demo-projects/regression-config-primitive-startupe2/xc7a200tfbg484` following the example's own regression runner (`regression/run.sh`) (see `tools/e2e/README.md`, "nextpnr-xilinx examples corpus (T7.6)"). `top.frm.xz` is the flow's frames: the snap's `fasm2frames`, dense, with the snap's prjxray-db.

## Target

* Part: `xc7a200tfbg484-2` (family `artix7`)
* Chip database: `xc7a200tfbg484-3.bin` (built from the snap's prjxray-db; the same package and database files, the speed grade does not change a chipdb)
* FASM: 875 lines, 32385 bytes
* Flow times on this machine (4 cores, s): buf_fix 0.73, fasm2frames 1.86, pnr 5.08, synth 2.89, xc7frames2bit 0.26

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
yosys -q -p "read_verilog top.v; synth_xilinx -flatten -abc9 -nocarry -nodsp -family xc7 -top top; write_json top.json"
yosys -p 'read_json top.json; techmap -map +/techmap.v t:$buf; write_json top.json'   # after synthesis: the $buf workaround
nextpnr-xilinx --chipdb xc7a200tfbg484-3.bin --xdc top.xdc --json top.json --write top_routed.json --fasm top.fasm   --timing-allow-fail
fasm2frames --part xc7a200tfbg484-2 --db-root <snap prjxray-db>/artix7 top.fasm > top.frames
xc7frames2bit --part_file <snap prjxray-db>/artix7/xc7a200tfbg484-2/part.yaml --part_name xc7a200tfbg484-2 --frm_file top.frames --output_file top.bit
```

## Reference outputs of the flow

```
sha256  top.fasm  727a5298ab01666a7b6ef5add16457c3641d231e22b75ef15e899490de51df89
sha256  top.frm   4a3c8360e830bf17c8a8ba1d2ef98acb1de8423933b141cacd0d25d36578fa30  (22698060 bytes)
sha256  top.bit   c0784f9991db0865a1c9ccf0facab7028cac5e22aa02bd4eec5afdb802b3e531  (9730754 bytes; header with the build time)
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
* note: pinned db: exit code 1, Segment DB CFG_CENTER_MID, key CFG_CENTER_MID.CFG_CENTER_STARTUP_USRDONEO.CFG_CENTER_IMUX42_8 not found from line 'CFG_CENTER_MID_X61Y136.CFG_CENTER_STARTUP_USRDONEO.CFG_CENTER_IMUX42_8'

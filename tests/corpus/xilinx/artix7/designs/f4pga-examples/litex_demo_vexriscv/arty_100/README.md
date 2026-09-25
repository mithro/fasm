# litex_demo_vexriscv / arty_100 -- f4pga (VPR) flow FASM (T7.3)

`vpr.fasm.xz` is the FASM of f4pga-examples' `litex_demo_vexriscv` for the
Digilent Arty A7-100T (`arty_100`), built with the f4pga Yosys + VPR flow exactly as
f4pga-examples documents it, by `tools/e2e/run-f4pga-examples.sh litex_demo_vexriscv
arty_100` (see `tools/e2e/README.md`, "f4pga-examples corpus (T7.3)"):
`top.fasm` of the flow's build directory, i.e. VPR's `genfasm` output with
the flow's extra FASM appended. The frames are not committed (288780 bytes after xz, over the 65536 byte limit of this corpus); their sha256 is below.

## Target

* Part: `xc7a100tcsg324-1` (family `artix7`), VPR device `xc7a100t_test`
* Build command (in f4pga-examples' `xc7/`): `cd litex_demo && ./src/litex/litex/boards/targets/arty.py --toolchain=symbiflow --cpu-type=vexriscv --sys-clk-freq 80e6 --output-dir build/vexriscv/arty_100 --variant a7-100 --build`
* Build time on this machine (4 cores, one build at a time): 530 s
* FASM: 260033 lines, 10687589 bytes

## Tools

* f4pga-examples
  [`13f11197`](https://github.com/chipsalliance/f4pga-examples/commit/13f11197b33dae1cde3bf146f317d63f0134eacf)
  (submodules at the commits it records).
* Toolchain: `tools/e2e/setup-f4pga.sh` (conda environment `xc7`,
  `tools/e2e/f4pga/xc7-conda-explicit.txt` and `xc7-pip-freeze.txt`):
  f4pga python package `e1cd038f`, yosys `0.27_29_g0f5e7c244`,
  symbiflow-yosys-plugins `1.0.0_7_1260_ge7070ca`, vtr-optimized (VPR,
  genfasm) `8.0.0_5699_g25e723a24`, prjxray-tools (xc7frames2bit,
  bitread) `0.1_3015_gae546d6b`, prjxray `ae546d6b` (python), f4pga-xc-fasm
  (xcfasm) `25dc605c`, fasm `0.0.2.post88`, symbiflow-arch-defs
  `20220920-124259`/`007d1c1` (package `xc7a100t_test`, sha256 `7a64daaa04b8f0761a86d3c74d2bc1bcacc27a6680edc8e7672bc2e804a73bcf`).
* Database: the flow's prjxray-db is the conda package
  `prjxray-db 0.0_257_g0a0adde`, prjxray-db commit `0a0added`: every
  database file is identical to the pinned `tools/fetch-db.sh` copy
  (`0a0addedd73e7e4139d52a6d8db4258763e0f1f3`), so the reference outputs
  below are also those of the pinned database.

## Reference outputs of the flow

The flow writes the bitstream with `xcfasm --sparse --emit_pudc_b_pullup`
(frames to a temporary file it does not keep, then `xc7frames2bit`);
`top.frm` is that same xcfasm command line rerun with `--frm_out`.

```
sha256  top.fasm  71eab20a91c11120e33e493e4b1487b81cf32a9d821b83b928643e7002c95539
sha256  top.frm   e9ae571bacc3b3fcecb17d6d94c998107df7f1fa6dbe8cd964b35aa1e90a1b24  (4160376 bytes)
sha256  top.bit   9803a9ad26558dbe0d6aeb46339f345b65f159b699f846a03c6b2cf82165dfad  (3825896 bytes)
```

`top.bit` is not byte reproducible: its header holds the build date and
time and the path of the temporary `.frm` file. The Rust `xcfasm`,
`fasm2frames` and `xc7frames2bit` reproduce `top.frm` byte for byte and
`top.bit` up to that path (with the header's date and time given through
`SOURCE_DATE_EPOCH`); see `docs/rewrite/DESIGN-xilinx-db.md` §8.11.

# counter_test / arty_100 -- f4pga (VPR) flow FASM (T7.3)

`vpr.fasm` is the FASM of f4pga-examples' `counter_test` for the
Digilent Arty A7-100T (`arty_100`), built with the f4pga Yosys + VPR flow exactly as
f4pga-examples documents it, by `tools/e2e/run-f4pga-examples.sh counter_test
arty_100` (see `tools/e2e/README.md`, "f4pga-examples corpus (T7.3)"):
`top.fasm` of the flow's build directory, i.e. VPR's `genfasm` output with
the flow's extra FASM appended. `vpr.frm.xz` is the flow's frames (`xz -9e`), compared byte for byte with the Rust `fasm2frames --sparse --emit_pudc_b_pullup` by `tests/e2e/test_f4pga_examples.py`.

## Target

* Part: `xc7a100tcsg324-1` (family `artix7`), VPR device `xc7a100t_test`
* Build command (in f4pga-examples' `xc7/`): `TARGET="arty_100" make -C counter_test`
* Build time on this machine (4 cores, one build at a time): 102 s
* FASM: 837 lines, 35268 bytes

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
sha256  top.fasm  081aac5c4cf1a86017d04576774d4f56e64fdef2fd8a5eacb4ad6bc2c01fa479
sha256  top.frm   8680b3e4867bbb80ae707468a2dbe6e67d2b7c4c124d4043762eb0b6b7601c39  (1245420 bytes)
sha256  top.bit   20501cc1925eddaf6441b6d2d3532f3de944730e309d0e6feef68c9a7707d04c  (3825896 bytes)
```

`top.bit` is not byte reproducible: its header holds the build date and
time and the path of the temporary `.frm` file. The Rust `xcfasm`,
`fasm2frames` and `xc7frames2bit` reproduce `top.frm` byte for byte and
`top.bit` up to that path (with the header's date and time given through
`SOURCE_DATE_EPOCH`); see `docs/rewrite/DESIGN-xilinx-db.md` §8.11.

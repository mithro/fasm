# pulse_width_led / arty_35 -- f4pga (VPR) flow FASM (T7.3)

`vpr.fasm` is the FASM of f4pga-examples' `pulse_width_led` for the
Digilent Arty A7-35T (`arty_35`), built with the f4pga Yosys + VPR flow exactly as
f4pga-examples documents it, by `tools/e2e/run-f4pga-examples.sh pulse_width_led
arty_35` (see `tools/e2e/README.md`, "f4pga-examples corpus (T7.3)"):
`top.fasm` of the flow's build directory, i.e. VPR's `genfasm` output with
the flow's extra FASM appended. `vpr.frm.xz` is the flow's frames (`xz -9e`), compared byte for byte with the Rust `fasm2frames --sparse --emit_pudc_b_pullup` by `tests/e2e/test_f4pga_examples.py`.

## Target

* Part: `xc7a35tcsg324-1` (family `artix7`), VPR device `xc7a50t_test`
* Build command (in f4pga-examples' `xc7/`): `TARGET="arty_35" make -C pulse_width_led`
* Build time on this machine (4 cores, one build at a time): 73 s
* FASM: 1834 lines, 73612 bytes

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
  `20220920-124259`/`007d1c1` (package `xc7a50t_test`, sha256 `7dafd8b08503afe8baa782218c5a703a8afc5b8c2601a3062307178a620d834d`).
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
sha256  top.fasm  5f1400224f76236b145547a92224b9b5dc937a36482e3ce38f76701625131f6b
sha256  top.frm   7c550aecf91c5e866a5332233e995f8a31e3daf3ab569578d65c882698a2b3b5  (1034484 bytes)
sha256  top.bit   b6da06fc1b4b872f920ffce5fee84b2acaa35c9c2c8b80eb586b226498a1e989  (2192119 bytes)
```

`top.bit` is not byte reproducible: its header holds the build date and
time and the path of the temporary `.frm` file. The Rust `xcfasm`,
`fasm2frames` and `xc7frames2bit` reproduce `top.frm` byte for byte and
`top.bit` up to that path (with the header's date and time given through
`SOURCE_DATE_EPOCH`); see `docs/rewrite/DESIGN-xilinx-db.md` §8.11.

# picosoc_demo / basys3 -- f4pga (VPR) flow FASM (T7.3)

`vpr.fasm.xz` is the FASM of f4pga-examples' `picosoc_demo` for the
Digilent Basys 3 (`basys3`), built with the f4pga Yosys + VPR flow exactly as
f4pga-examples documents it, by `tools/e2e/run-f4pga-examples.sh picosoc_demo
basys3` (see `tools/e2e/README.md`, "f4pga-examples corpus (T7.3)"):
`top.fasm` of the flow's build directory, i.e. VPR's `genfasm` output with
the flow's extra FASM appended. The frames are not committed (115436 bytes after xz, over the 65536 byte limit of this corpus); their sha256 is below.

## Target

* Part: `xc7a35tcpg236-1` (family `artix7`), VPR device `xc7a50t_test`
* Build command (in f4pga-examples' `xc7/`): `TARGET="basys3" make -C picosoc_demo`
* Build time on this machine (4 cores, one build at a time): 239 s
* FASM: 99269 lines, 3680943 bytes

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
sha256  top.fasm  74a1974302ea64984cbc3dd5069c812bb1e51f781482553492d3b69cef992178
sha256  top.frm   8db4e169da4b1bbbb7aca941857ab4be0651471cb4bb3807f455353d64183a8b  (3446784 bytes)
sha256  top.bit   9334a721fe685348382fa5f9a479a178649f8e4e3a11daf7240e62d170dbce60  (2192119 bytes)
```

`top.bit` is not byte reproducible: its header holds the build date and
time and the path of the temporary `.frm` file. The Rust `xcfasm`,
`fasm2frames` and `xc7frames2bit` reproduce `top.frm` byte for byte and
`top.bit` up to that path (with the header's date and time given through
`SOURCE_DATE_EPOCH`); see `docs/rewrite/DESIGN-xilinx-db.md` §8.11.

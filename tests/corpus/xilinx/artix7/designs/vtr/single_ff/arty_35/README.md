# single_ff / arty_35 -- VTR genfasm FASM (T7.4)

`genfasm.fasm` is the FASM of VTR's Verilog benchmark `single_ff`
(`vtr_flow/benchmarks/verilog/single_ff.v`, top module `top`) built with
the f4pga flow for the Digilent Arty A7-35T on the f4pga toolchain's `xc7a50t_test`
architecture, by `tools/e2e/run-vtr-genfasm.sh verilog single_ff` (see
`tools/e2e/README.md`, "VTR genfasm designs (T7.4)"): `symbiflow_synth`
(Yosys), `symbiflow_pack`, `symbiflow_place` (with the PCF of
`tools/e2e/vtr/make-pcf.py`: every port bit on a package pin in pin map
order, the benchmarks have no pin constraints), `symbiflow_route` and
`symbiflow_write_fasm` (genfasm, plus the synthesis' extra FASM, like the
f4pga-examples designs). `genfasm.frm.xz` is the reference frames (`xz -9e`), compared byte for byte with the Rust `fasm2frames --sparse --emit_pudc_b_pullup` by `tests/e2e/test_vtr_genfasm.py`.

## Target

* Board: Digilent Arty A7-35T (`arty_35`), part `xc7a35tcsg324-1` (family `artix7`),
  VPR device `xc7a50t_test`
* VTR hard blocks given Verilog models (`tools/e2e/vtr/hard-block-models.py`):
  none
* Synthesis 11 s, pack + place + route 29 s,
  `symbiflow_write_fasm` 19.2 s
* FASM: 82 lines, 4030 bytes

## Source and licence

* Design: VTR `vtr_flow/benchmarks/verilog/single_ff.v`, one of VTR's small test designs.
* Licence: no licence or author notice in the source (VTR's `LICENSE.md` leaves benchmark circuits to the terms in their source, and this one states none); distributed with VTR.
* The FASM, frames and bitstream are tool output (VPR/genfasm, xcfasm) of this design.

## Tools

* VPR and genfasm: vtr-optimized 8.0.0_5699_g25e723a24 (conda, f4pga toolchain), `vpr --version` 8.1.0-dev+25e723a24-dirty (revision 8.0.0-5699-g25e723a24-dirty)
  (VTR [`25e723a2`](https://github.com/verilog-to-routing/vtr-verilog-to-routing/commit/25e723a24aa0ae7a0061cd89dd84b1fb62afcc09)).
* Architecture: symbiflow-arch-defs `20220920-124259`/`007d1c1` (conda
  package `xc7a50t_test` of `tools/e2e/setup-f4pga.sh`).
* Reference frames and bitstream: the f4pga flow's `xcfasm --sparse
  --emit_pudc_b_pullup` (f4pga-xc-fasm `25dc605c`, prjxray-tools
  `0.1_3015_gae546d6b`) with the flow's prjxray-db `0a0added`, identical
  to the pinned `tools/fetch-db.sh` copy.

## Reference outputs

```
sha256  top.fasm  488c8332d06dd08a5a9f8d56cf4002e72cf2cfd9dcfa9a449ed79eb64eda5a7a
sha256  top.frm   cd3ec0169a69e089caa34a23c29ed894e1fb32dfff0157e3f71f55c8720bdaf8  (166056 bytes)
sha256  top.bit   5aa9ab9d7888a51d0d35e6c1f177cced919d3620941d51198f3d9f543fc51082  (2192227 bytes)
```

`top.bit` holds the build date and time and the `.frm` path in its
header; see `docs/rewrite/DESIGN-xilinx-db.md` §8.14.

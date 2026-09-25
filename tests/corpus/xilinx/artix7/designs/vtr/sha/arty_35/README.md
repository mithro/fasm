# sha / arty_35 -- VTR genfasm FASM (T7.4)

`genfasm.fasm.xz` is the FASM of VTR's Verilog benchmark `sha`
(`vtr_flow/benchmarks/verilog/sha.v`, top module `sha1`) built with
the f4pga flow for the Digilent Arty A7-35T on the f4pga toolchain's `xc7a50t_test`
architecture, by `tools/e2e/run-vtr-genfasm.sh verilog sha` (see
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
* Synthesis 19 s, pack + place + route 84 s,
  `symbiflow_write_fasm` 21.6 s
* FASM: 49080 lines, 1809033 bytes

## Source and licence

* Design: VTR `vtr_flow/benchmarks/verilog/sha.v`: the OpenCores `sha_core` (SHA-160) by marsgod.
* Licence: "Copyright (C) 2002-2004 marsgod": "This source file may be used and distributed without restriction provided that this copyright statement is not removed from the file and that any derivative work contains the original copyright notice and the associated disclaimer" (OpenCores notice in the source header, with an AS IS disclaimer).
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
sha256  top.fasm  d56e69a0b98101bf7b726347a1098c90add184dae73f9579ed6b5fc5ec42bb3c
sha256  top.frm   4b65ac452b01b62b0d7ffbb382a477074d5d388c2b3602aab7b2dd46311b7b36  (2463912 bytes)
sha256  top.bit   415f54449e0f8281b43d594e263728476a3a9232b04ddc5c4b7e884fad92a18e  (2192221 bytes)
```

`top.bit` holds the build date and time and the `.frm` path in its
header; see `docs/rewrite/DESIGN-xilinx-db.md` §8.14.

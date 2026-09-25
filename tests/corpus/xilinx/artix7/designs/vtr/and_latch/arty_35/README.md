# and_latch / arty_35 -- VTR genfasm FASM (T7.4)

`genfasm.fasm` is the FASM of VTR's Verilog benchmark `and_latch`
(`vtr_flow/benchmarks/verilog/and_latch.v`, top module `and_latch`) built with
the f4pga flow for the Digilent Arty A7-35T on the f4pga toolchain's `xc7a50t_test`
architecture, by `tools/e2e/run-vtr-genfasm.sh verilog and_latch` (see
`tools/e2e/README.md`, "VTR genfasm designs (T7.4)"): `symbiflow_synth`
(Yosys), `symbiflow_pack`, `symbiflow_place` (with the PCF of
`tools/e2e/vtr/make-pcf.py`: every port bit on a package pin in pin map
order, the benchmarks have no pin constraints), `symbiflow_route` and
`symbiflow_write_fasm` (genfasm, plus the synthesis' extra FASM, like the
f4pga-examples designs). `genfasm.frm.xz` is the reference frames (`xz -9e`), compared byte for byte with the Rust `fasm2frames --sparse --emit_pudc_b_pullup` by `tests/e2e/test_vtr_genfasm.py`.

## Target

* Board: Digilent Arty A7-35T (`arty_35`), part `xc7a35tcsg324-1` (family `artix7`),
  VPR device `xc7a50t_test`
* Synthesis 12 s, pack + place + route 38 s,
  `symbiflow_write_fasm` 19.8 s
* FASM: 112 lines, 5504 bytes

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
sha256  top.fasm  2d1b4aec3e324062447a5982f4825263fd1bc8cb6a17c002b5c8453c681f5cb3
sha256  top.frm   4b9f8aa358743f52afaf78b186fc30e4822e67d6e810d4e3c0af070986e1a254  (166056 bytes)
sha256  top.bit   2d623322a11e6343bda4a566e6600f79b24c38562f7d3b1c11ccb66b7b9b3c9b  (2192227 bytes)
```

`top.bit` holds the build date and time and the `.frm` path in its
header; see `docs/rewrite/DESIGN-xilinx-db.md` §8.14.

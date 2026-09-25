# diffeq2 / arty_35 -- VTR genfasm FASM (T7.4)

`genfasm.fasm.xz` is the FASM of VTR's Verilog benchmark `diffeq2`
(`vtr_flow/benchmarks/verilog/diffeq2.v`, top module `diffeq_f_systemC`) built with
the f4pga flow for the Digilent Arty A7-35T on the f4pga toolchain's `xc7a50t_test`
architecture, by `tools/e2e/run-vtr-genfasm.sh verilog diffeq2` (see
`tools/e2e/README.md`, "VTR genfasm designs (T7.4)"): `symbiflow_synth`
(Yosys), `symbiflow_pack`, `symbiflow_place` (with the PCF of
`tools/e2e/vtr/make-pcf.py`: every port bit on a package pin in pin map
order, the benchmarks have no pin constraints), `symbiflow_route` and
`symbiflow_write_fasm` (genfasm, plus the synthesis' extra FASM, like the
f4pga-examples designs). The reference frames are not committed (99900 bytes after xz, over the 65536 byte limit of this corpus); their sha256 is below.

## Target

* Board: Digilent Arty A7-35T (`arty_35`), part `xc7a35tcsg324-1` (family `artix7`),
  VPR device `xc7a50t_test`
* VTR hard blocks given Verilog models (`tools/e2e/vtr/hard-block-models.py`):
  none
* Synthesis 25 s, pack + place + route 125 s,
  `symbiflow_write_fasm` 22.8 s
* FASM: 76667 lines, 2882497 bytes

## Source and licence

* Design: VTR `vtr_flow/benchmarks/verilog/diffeq2.v`: a differential equation solver by P. Sridhar, University of Cincinnati (1991, from the High-Level Synthesis Workshop repository; HardwareC original by Rajesh Gupta, Stanford).
* Licence: no licence in the source header, only a disclaimer ("This comes with absolutely no guarantees of any kind"); distributed with VTR as a benchmark circuit.
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
sha256  top.fasm  85d3953dfe8d2b8c3e5e61f3d177d01752b78d69ff1f7cb4d7f32c72fda190ac
sha256  top.frm   dba8e1e65d33358b84ad7d9a6615a83bddcd44707ee909ce73bdd64b12a48a97  (3718308 bytes)
sha256  top.bit   a4e8574b2b687a3a51e7f4b9ad04c98a7f7848dae528d80fda0e87418deb8a3b  (2192225 bytes)
```

`top.bit` holds the build date and time and the `.frm` path in its
header; see `docs/rewrite/DESIGN-xilinx-db.md` §8.14.

# multiclock_reader_writer / arty_35 -- VTR genfasm FASM (T7.4)

`genfasm.fasm` is the FASM of VTR's Verilog benchmark `multiclock_reader_writer`
(`vtr_flow/benchmarks/verilog/multiclock_reader_writer.v`, top module `multiclock_reader_writer`) built with
the f4pga flow for the Digilent Arty A7-35T on the f4pga toolchain's `xc7a50t_test`
architecture, by `tools/e2e/run-vtr-genfasm.sh verilog multiclock_reader_writer` (see
`tools/e2e/README.md`, "VTR genfasm designs (T7.4)"): `symbiflow_synth`
(Yosys), `symbiflow_pack`, `symbiflow_place` (with the PCF of
`tools/e2e/vtr/make-pcf.py`: every port bit on a package pin in pin map
order, the benchmarks have no pin constraints), `symbiflow_route` and
`symbiflow_write_fasm` (genfasm, plus the synthesis' extra FASM, like the
f4pga-examples designs). `genfasm.frm.xz` is the reference frames (`xz -9e`), compared byte for byte with the Rust `fasm2frames --sparse --emit_pudc_b_pullup` by `tests/e2e/test_vtr_genfasm.py`.

## Target

* Board: Digilent Arty A7-35T (`arty_35`), part `xc7a35tcsg324-1` (family `artix7`),
  VPR device `xc7a50t_test`
* Synthesis 12 s, pack + place + route 30 s,
  `symbiflow_write_fasm` 18.8 s
* FASM: 534 lines, 20726 bytes

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
sha256  top.fasm  59d60a98c704453670c3cbe0d54bfa590462ee1b5d5903ceb8bb0613f54631b4
sha256  top.frm   0fd1e121fd84455e4d946bcd7c6545d579f6501cb09f641b321f108cc23c5d52  (632808 bytes)
sha256  top.bit   6918fe0ba0f315a5ccb3f230c7da69764314e7dd61153c9af0b64ff7881fb10a  (2192242 bytes)
```

`top.bit` holds the build date and time and the `.frm` path in its
header; see `docs/rewrite/DESIGN-xilinx-db.md` §8.14.

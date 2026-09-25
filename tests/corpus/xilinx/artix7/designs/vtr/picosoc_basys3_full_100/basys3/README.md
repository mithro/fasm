# picosoc_basys3_full_100 / basys3 -- VTR genfasm FASM (T7.4)

`genfasm.fasm.xz` is the FASM VTR's `genfasm` writes for the `picosoc_basys3_full_100`
benchmark of VTR's nightly `symbiflow` regression task (listed in its `config.txt`) on the f4pga
toolchain's `xc7a50t_test` architecture, by `tools/e2e/run-vtr-genfasm.sh
xc7a50t_test picosoc_basys3_full_100` (see `tools/e2e/README.md`, "VTR genfasm
designs (T7.4)"): VPR packs, places and routes the benchmark's eblif
with the task's options, then genfasm writes the FASM. The reference frames are not committed (117952 bytes after xz, over the 65536 byte limit of this corpus); their sha256 is below.

## Target

* Board: Digilent Basys 3 (`basys3`), part `xc7a35tcpg236-1` (family `artix7`),
  VPR device `xc7a50t_test`
* Netlist, SDC and placement constraints:
  `benchmarks/circuits/picosoc_basys3_full_100.eblif`, `benchmarks/sdc/picosoc_basys3_full_100.sdc`,
  `benchmarks/place_constr/picosoc_basys3_full_100.place`
  of the symbiflow-arch-defs benchmark tarball `fb1b251a` (sha256
  `2f5fed77c069e7e787f909e75f8aaf2db6ec1ea669a17a4f13d196c55931cc3d`,
  what VTR's `vtr_flow/scripts/download_symbiflow.py` downloads); made
  for that symbiflow-arch-defs, read by VPR with the one below
* VPR: `vpr arch.timing.xml picosoc_basys3_full_100.eblif --read_rr_graph
  rr_graph_xc7a50t_test.rr_graph.real.bin <options> --read_router_lookahead
  rr_graph_xc7a50t_test.lookahead.bin --read_placement_delay_lookup
  rr_graph_xc7a50t_test.place_delay.bin --sdc_file picosoc_basys3_full_100.sdc
  --fix_clusters picosoc_basys3_full_100.place`, 198 s
* genfasm: `genfasm arch.timing.xml picosoc_basys3_full_100.eblif --read_rr_graph
  rr_graph_xc7a50t_test.rr_graph.real.bin <options>`, 25.0 s
* `<options>` (the task's `script_params`): `--max_router_iterations 500 --routing_failure_predictor off --router_high_fanout_threshold 1000 --constant_net_method route --route_chan_width 500 --router_heap bucket --clock_modeling route --place_delta_delay_matrix_calculation_method dijkstra --place_delay_model delta_override --router_lookahead extended_map --check_route quick --strict_checks off --allow_dangling_combinational_nodes on --disable_errors check_unbuffered_edges:check_route --congested_routing_iteration_threshold 0.8 --incremental_reroute_delay_ripup off --base_cost_type delay_normalized_length_bounded --bb_factor 10 --initial_pres_fac 4.0 --check_rr_graph off`
* FASM: 102638 lines, 3854147 bytes

## Source and licence

* Design: symbiflow-arch-defs `tests/9-soc/picosoc/` (`basys3-full_demo_100.v`, `firmware_noflash_100.v`) around PicoSoC / PicoRV32 (`picorv32.v`, `picosoc_noflash.v`, `simpleuart.v`, from YosysHQ/picorv32).
* Licence: ISC: PicoRV32 and PicoSoC "Copyright (C) 2015/2017 Claire Xenia Wolf" with the ISC permission notice in their source headers; symbiflow-arch-defs `COPYING` (ISC).
* The eblif netlist (symbiflow-arch-defs `fb1b251a` benchmark tarball) is Yosys output and carries no licence text; its `src` attributes name the sources above.
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
sha256  top.fasm  d245f6f001ea4be5b5979d21cdc57a980c3167b0115b82a63b62043073bd5145
sha256  top.frm   3300c3020ec6697829c4e1746790caba2c70679ea1f7f212033678548b0a89aa  (3410880 bytes)
sha256  top.bit   64c4ef3bcf749f69c1ef9a05a826194ffd479e0584c39a72385fe3578020b540  (2192240 bytes)
```

`top.bit` holds the build date and time and the `.frm` path in its
header; see `docs/rewrite/DESIGN-xilinx-db.md` §8.14.

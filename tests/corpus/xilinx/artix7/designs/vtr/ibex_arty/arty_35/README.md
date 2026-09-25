# ibex_arty / arty_35 -- VTR genfasm FASM (T7.4)

`genfasm.fasm.xz` is the FASM VTR's `genfasm` writes for the `ibex_arty`
benchmark of VTR's nightly `symbiflow` regression task (in the task's benchmark tarball, not in its `config.txt` list) on the f4pga
toolchain's `xc7a50t_test` architecture, by `tools/e2e/run-vtr-genfasm.sh
xc7a50t_test ibex_arty` (see `tools/e2e/README.md`, "VTR genfasm
designs (T7.4)"): VPR packs, places and routes the benchmark's eblif
with the task's options, then genfasm writes the FASM. The reference frames are not committed (110504 bytes after xz, over the 65536 byte limit of this corpus); their sha256 is below.

## Target

* Board: Digilent Arty A7-35T (`arty_35`), part `xc7a35tcsg324-1` (family `artix7`),
  VPR device `xc7a50t_test`
* Netlist, SDC and placement constraints:
  `benchmarks/circuits/ibex_arty.eblif`, `benchmarks/sdc/ibex_arty.sdc`,
  `benchmarks/place_constr/ibex_arty.place`
  of the symbiflow-arch-defs benchmark tarball `fb1b251a` (sha256
  `2f5fed77c069e7e787f909e75f8aaf2db6ec1ea669a17a4f13d196c55931cc3d`,
  what VTR's `vtr_flow/scripts/download_symbiflow.py` downloads); made
  for that symbiflow-arch-defs, read by VPR with the one below
* VPR: `vpr arch.timing.xml ibex_arty.eblif --read_rr_graph
  rr_graph_xc7a50t_test.rr_graph.real.bin <options> --read_router_lookahead
  rr_graph_xc7a50t_test.lookahead.bin --read_placement_delay_lookup
  rr_graph_xc7a50t_test.place_delay.bin --sdc_file ibex_arty.sdc
  --fix_clusters ibex_arty.place`, 101 s
* genfasm: `genfasm arch.timing.xml ibex_arty.eblif --read_rr_graph
  rr_graph_xc7a50t_test.rr_graph.real.bin <options>`, 24.7 s
* `<options>` (the task's `script_params`): `--max_router_iterations 500 --routing_failure_predictor off --router_high_fanout_threshold 1000 --constant_net_method route --route_chan_width 500 --router_heap bucket --clock_modeling route --place_delta_delay_matrix_calculation_method dijkstra --place_delay_model delta_override --router_lookahead extended_map --check_route quick --strict_checks off --allow_dangling_combinational_nodes on --disable_errors check_unbuffered_edges:check_route --congested_routing_iteration_threshold 0.8 --incremental_reroute_delay_ripup off --base_cost_type delay_normalized_length_bounded --bb_factor 10 --initial_pres_fac 4.0 --check_rr_graph off`
* FASM: 103176 lines, 4505447 bytes

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
sha256  top.fasm  900e6683c846b5f4800e56712529d6a0a4eb9fd0694462a4f06a79059632a55f
sha256  top.frm   901425e3992d3af8eca975fa4e3848c58a8cf92af7d7b6006f8e030937731bea  (3204432 bytes)
sha256  top.bit   b55039ff5fc1cbf069c00fb6cb9037238f3503589ce96d53a8ca73bb05da2034  (2192227 bytes)
```

`top.bit` holds the build date and time and the `.frm` path in its
header; see `docs/rewrite/DESIGN-xilinx-db.md` §8.14.

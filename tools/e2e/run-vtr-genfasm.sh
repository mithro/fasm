#!/usr/bin/env bash
#
# Copyright 2017-2022 F4PGA Authors
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# SPDX-License-Identifier: Apache-2.0
#
# Runs VPR and genfasm (VTR's FASM writer) on every VTR design that can
# produce FASM (T7.4), with the VPR/genfasm of the f4pga toolchain
# (tools/e2e/setup-f4pga.sh: conda package vtr-optimized
# 8.0.0_5699_g25e723a24, i.e. VTR 25e723a24), and collects the FASM:
#
# Group `test_fasm_arch` (generic FASM): VTR's own genfasm test
#   architecture utils/fasm/test/test_fasm_arch.xml (the only VTR
#   architecture with fasm_* metadata; a fixed 6x6 layout: 16 CLBs of two
#   fracturable LUT6/FF elements, 128 IOs) with
#     fasm-test/wire           utils/fasm/test/wire.eblif, the design of
#                              test_fasm.cpp's fasm_integration_test, run
#                              twice: plain, and like the test with
#                              fasm_features metadata on every rr graph
#                              edge (vpr --write_rr_graph, then
#                              add-rr-edge-metadata.py, then genfasm
#                              --read_rr_graph), genfasm-rr-metadata.fasm
#     microbenchmarks/<c>      vtr_flow/benchmarks/microbenchmarks/*.blif
#     tests/<c>                vtr_flow/benchmarks/tests/*.{blif,eblif}
#     blif/<c>, blif/<K>/<c>   vtr_flow/benchmarks/blif/**/*.blif (the MCNC
#                              circuits, K = 2..8 and wiremap6 too)
#   all with --route_chan_width 100, like the test. A circuit with more
#   than 128 `.names` is not run (it cannot fit the layout's 32 FLEs;
#   --no-size-filter runs it anyway); one VPR cannot implement on this
#   architecture (a primitive it has no model for, or too large) is
#   recorded as such in its info.json.
#   Output: $OUT/test_fasm_arch/<circuit>/{genfasm.fasm,info.json,*.log.xz}
#
# Group `xc7a50t_test` (Xilinx FASM): the benchmarks of VTR's nightly
#   `symbiflow` task (vtr_flow/tasks/regression_tests/vtr_reg_nightly_test1/
#   symbiflow: eblif netlists, SDC and placement constraints from
#   symbiflow-arch-defs, downloaded by vtr_flow/scripts/download_symbiflow.py;
#   here the pinned tarball below) on the f4pga toolchain's xc7a50t_test
#   architecture, with the task's VPR options. The task lists
#   picosoc_basys3_full_{50,100}, linux_arty and minilitex{,_ddr,_ddr_eth}_arty;
#   the tarball's other xc7a50t_test circuits (counter_basys3,
#   murax_basys3_full_{50,100}, ibex_arty) are run too; its
#   *_arty_a7/*_arty_100t circuits need xc7a100t_test and are skipped
#   when that device is not installed. The reference frames and
#   bitstream are the f4pga flow's (`xcfasm --sparse --emit_pudc_b_pullup`
#   with the flow's prjxray-db), so tools/e2e/compare-f4pga-examples.py
#   --out $OUT/xc7a50t_test and tools/difftest-xilinx.py --corpus-root
#   $OUT/xc7a50t_test compare the Rust tools with them.
#   Output: $OUT/xc7a50t_test/<circuit>/<board>/{top.fasm,top.frm,top.bit,
#   info.json,difftest.json,*.log.xz}
#
# Group `verilog` (Xilinx FASM): VTR's Verilog benchmarks
#   (vtr_flow/benchmarks/verilog/*.v) through the f4pga flow for the
#   Arty A7-35T (xc7a35tcsg324-1, xc7a50t_test): the f4pga-examples
#   Makefile steps symbiflow_synth (Yosys, top module found with
#   `hierarchy -auto-top`), symbiflow_pack, symbiflow_place,
#   symbiflow_route, symbiflow_write_fasm (genfasm), with a PCF from
#   tools/e2e/vtr/make-pcf.py (the benchmarks have no pin constraints; the
#   placer needs one). VTR's hard blocks that a benchmark instantiates
#   (single_port_ram, dual_port_ram, multiply, adder) get Verilog models
#   (tools/e2e/vtr/hard-block-models.py: vtr_flow/primitives.v's, the
#   RAMs rewritten so that Yosys infers block RAM). A
#   benchmark that has more port bits than the package has pins, or that
#   synthesis, packing, placement or routing reject, is recorded as not
#   implementable.
#   Output: like xc7a50t_test, in $OUT/xc7a50t_test/<benchmark>/arty_35/
#
# Usage:
#   tools/e2e/run-vtr-genfasm.sh --list [GROUP]
#   tools/e2e/run-vtr-genfasm.sh [--keep] [--no-size-filter] [--jobs N] GROUP [CIRCUIT...]
#   tools/e2e/run-vtr-genfasm.sh [--keep] [--jobs N] --all      # all three groups
#
# Environment:
#   VTR_ROOT          a VTR checkout at VTR_COMMIT with utils/fasm/test and
#                     vtr_flow/benchmarks (default tools/e2e/build/vtr; a
#                     blobless sparse clone is made when missing)
#   VTR_SYMBIFLOW_BENCHMARKS  the extracted benchmark tarball (default
#                     tools/e2e/build/vtr-symbiflow-benchmarks; downloaded
#                     and checked against its sha256 when missing)
#   F4PGA_E2E_ROOT    the f4pga toolchain (tools/e2e/f4pga-env.sh)
#   VTR_GENFASM_OUT   output directory (default tools/e2e/build/out/vtr-genfasm)
#   VTR_TIMEOUT       seconds per VPR/genfasm run (default 900)
#   VTR_MEMORY_KB     virtual memory limit per run (default 7340032)
#
# Exit status 0 when every selected circuit either produced its FASM or
# was recorded as not implementable on its architecture (test_fasm_arch)
# or as needing a device that is not installed; 1 otherwise.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VTR_COMMIT=25e723a24aa0ae7a0061cd89dd84b1fb62afcc09
VTR="${VTR_ROOT:-$REPO_ROOT/tools/e2e/build/vtr}"
BENCH="${VTR_SYMBIFLOW_BENCHMARKS:-$REPO_ROOT/tools/e2e/build/vtr-symbiflow-benchmarks}"
OUT="${VTR_GENFASM_OUT:-$REPO_ROOT/tools/e2e/build/out/vtr-genfasm}"
TIMEOUT="${VTR_TIMEOUT:-900}"
MEMORY_KB="${VTR_MEMORY_KB:-7340032}"
# What vtr_flow/scripts/download_symbiflow.py's "symbiflow-benchmarks-latest"
# pointed to on 2026-09-25 (symbiflow-arch-defs fb1b251a, 2022-03-22).
BENCH_URL='https://www.googleapis.com/download/storage/v1/b/symbiflow-arch-defs/o/artifacts%2Fprod%2Ffoss-fpga-tools%2Fsymbiflow-arch-defs%2Fcontinuous%2Finstall%2F597%2F20220322-000106%2Fsymbiflow-arch-defs-benchmarks-fb1b251a.tar.xz?generation=1647951797900389&alt=media'
BENCH_SHA256=2f5fed77c069e7e787f909e75f8aaf2db6ec1ea669a17a4f13d196c55931cc3d
SIZE_LIMIT=128

# circuit board device part family in_vtr_task
XC7_CIRCUITS="
counter_basys3 basys3 xc7a50t_test xc7a35tcpg236-1 artix7 no
picosoc_basys3_full_50 basys3 xc7a50t_test xc7a35tcpg236-1 artix7 yes
picosoc_basys3_full_100 basys3 xc7a50t_test xc7a35tcpg236-1 artix7 yes
murax_basys3_full_50 basys3 xc7a50t_test xc7a35tcpg236-1 artix7 no
murax_basys3_full_100 basys3 xc7a50t_test xc7a35tcpg236-1 artix7 no
ibex_arty arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 no
minilitex_arty arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 yes
minilitex_ddr_arty arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 yes
minilitex_ddr_eth_arty arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 yes
linux_arty arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 yes
minilitex_arty_a7 arty_100 xc7a100t_test xc7a100tcsg324-1 artix7 no
minilitex_ddr_arty_a7 arty_100 xc7a100t_test xc7a100tcsg324-1 artix7 no
minilitex_ddr_eth_arty_100t arty_100 xc7a100t_test xc7a100tcsg324-1 artix7 no
linux_arty_100t arty_100 xc7a100t_test xc7a100tcsg324-1 artix7 no
"

# The VPR options of vtr_reg_nightly_test1/symbiflow/config/config.txt
# (script_params without -starting_stage, and additional_files_list).
XC7_VPR_OPTIONS="--max_router_iterations 500 --routing_failure_predictor off
--router_high_fanout_threshold 1000 --constant_net_method route
--route_chan_width 500 --router_heap bucket --clock_modeling route
--place_delta_delay_matrix_calculation_method dijkstra
--place_delay_model delta_override --router_lookahead extended_map
--check_route quick --strict_checks off
--allow_dangling_combinational_nodes on
--disable_errors check_unbuffered_edges:check_route
--congested_routing_iteration_threshold 0.8
--incremental_reroute_delay_ripup off
--base_cost_type delay_normalized_length_bounded --bb_factor 10
--initial_pres_fac 4.0 --check_rr_graph off"

log() { echo "[run-vtr-genfasm] $*" >&2; }

usage() {
  awk 'NR >= 19 { if (!/^#/) exit; sub(/^# ?/, ""); print }' "$0"
}

# The test_fasm_arch circuits: "<circuit> <netlist path relative to VTR>".
test_arch_circuits() {
  echo "fasm-test/wire utils/fasm/test/wire.eblif"
  local f rel
  (cd "$VTR/vtr_flow/benchmarks" && ls microbenchmarks/*.blif tests/*.blif tests/*.eblif blif/*.blif blif/*/*.blif 2>/dev/null) | sort | while read -r rel; do
    f=${rel%.eblif}
    f=${f%.blif}
    echo "$f vtr_flow/benchmarks/$rel"
  done
}

list() {
  local group="${1:-}"
  if [[ -z $group || $group == test_fasm_arch ]]; then
    ensure_vtr
    test_arch_circuits | awk '{ printf "test_fasm_arch %-40s %s\n", $1, $2 }'
  fi
  if [[ -z $group || $group == verilog ]]; then
    ensure_vtr
    verilog_circuits | awk '{ printf "verilog        %s\n", $1 }'
  fi
  if [[ -z $group || $group == xc7a50t_test ]]; then
    echo "$XC7_CIRCUITS" | awk 'NF { printf "xc7a50t_test   %-28s %-9s %-14s %s%s\n", $1, $2, $3, $4, ($6 == "yes" ? "  (VTR nightly task)" : "") }'
  fi
}

ensure_vtr() {
  if [[ -f "$VTR/utils/fasm/test/test_fasm_arch.xml" && -d "$VTR/vtr_flow/benchmarks/blif" \
        && -f "$VTR/vtr_flow/primitives.v" ]]; then
    return 0
  fi
  log "cloning VTR $VTR_COMMIT (blobless, sparse) into $VTR"
  rm -rf "$VTR"
  mkdir -p "$VTR"
  git -C "$VTR" init -q
  git -C "$VTR" remote add origin https://github.com/verilog-to-routing/vtr-verilog-to-routing
  git -C "$VTR" sparse-checkout set --no-cone /utils/fasm/ /vtr_flow/benchmarks/blif/ \
    /vtr_flow/benchmarks/microbenchmarks/ /vtr_flow/benchmarks/tests/ \
    '/vtr_flow/benchmarks/verilog/*.v' /vtr_flow/primitives.v \
    /vtr_flow/tasks/regression_tests/vtr_reg_nightly_test1/symbiflow/ \
    /vtr_flow/scripts/download_symbiflow.py
  timeout 1800 git -C "$VTR" fetch -q --depth 1 --filter=blob:none origin "$VTR_COMMIT"
  git -C "$VTR" checkout -q FETCH_HEAD
}

check_vtr_commit() {
  local head
  head=$(git -C "$VTR" rev-parse HEAD 2>/dev/null || echo unknown)
  if [[ $head != "$VTR_COMMIT" ]]; then
    log "warning: $VTR is at $head, not $VTR_COMMIT (the VTR of the toolchain's genfasm)"
  fi
}

ensure_bench() {
  if [[ -d "$BENCH/benchmarks/circuits" ]]; then
    return 0
  fi
  log "downloading the symbiflow benchmarks (3.6 MB) into $BENCH"
  mkdir -p "$BENCH"
  timeout 600 curl -sSfL -o "$BENCH/benchmarks.tar.xz" "$BENCH_URL"
  echo "$BENCH_SHA256  $BENCH/benchmarks.tar.xz" | sha256sum -c --quiet -
  tar -xJf "$BENCH/benchmarks.tar.xz" -C "$BENCH"
  rm -f "$BENCH/benchmarks.tar.xz"
}

# run_timed LOG CMD...: runs CMD with the time and memory limits, output to
# LOG; prints "<exit status> <seconds>".
run_timed() {
  local logf="$1"
  shift
  local start end rc=0
  start=$(date +%s.%N)
  (ulimit -v "$MEMORY_KB"; exec timeout "$TIMEOUT" "$@") > "$logf" 2>&1 || rc=$?
  end=$(date +%s.%N)
  echo "$rc $(python3 -c "print(round($end - $start, 2))")"
}

# The "Message:" of VPR's first "Error N:" block (which can start in the
# middle of a line of other output).
vpr_error() {
  grep -A6 'Error [0-9]*: *$' "$1" | grep -m1 '^Message:' | sed 's/^Message: *//' | cut -c1-300 || true
}

write_info() {
  python3 "$REPO_ROOT/tools/e2e/vtr/write-info.py" "$@"
}

compress_logs() {
  local d="$1" f
  rm -f "$d/vpr_stdout.log"
  for f in "$d"/*.log; do
    [[ -f "$f" ]] && xz -9 -f "$f"
  done
  return 0
}

run_test_arch() {
  local circuit="$1" netlist="$2" size_filter="$3"
  local out="$OUT/test_fasm_arch/$circuit"
  local arch="$VTR/utils/fasm/test/test_fasm_arch.xml"
  rm -rf "$out"
  mkdir -p "$out"
  local src="$VTR/$netlist" name
  name=$(basename "$netlist")
  cp "$src" "$out/$name"
  local names
  names=$(grep -c '^\.names' "$src" || true)
  local common=(test_fasm_arch.xml "$name" --route_chan_width 100)
  cp "$arch" "$out/test_fasm_arch.xml"
  if [[ $size_filter == 1 && $names -gt $SIZE_LIMIT ]]; then
    write_info "$out" test_fasm_arch "$circuit" "$netlist" 100 \
      "not run: $names .names > $SIZE_LIMIT (does not fit the 6x6 layout)" - - -
    rm -f "$out/$name" "$out/test_fasm_arch.xml"
    return 0
  fi
  local r status vpr_s genfasm_s=-
  local extra=()
  [[ $circuit == fasm-test/wire ]] && extra=(--write_rr_graph rr_graph.xml)
  r=$(cd "$out" && run_timed vpr.log vpr "${common[@]}" "${extra[@]}")
  vpr_s=${r#* }
  if [[ ${r%% *} != 0 ]]; then
    status="unimplementable: $(vpr_error "$out/vpr.log")"
    [[ ${r%% *} == 124 ]] && status="failed: vpr timed out"
    [[ -z ${status#unimplementable: } ]] && status="failed: vpr exit ${r%% *}"
  else
    r=$(cd "$out" && run_timed genfasm.log genfasm "${common[@]}")
    genfasm_s=${r#* }
    local fasm
    fasm=$(cd "$out" && ls ./*.fasm 2>/dev/null | head -1 || true)
    if [[ ${r%% *} != 0 || -z $fasm ]]; then
      status="failed: genfasm exit ${r%% *} $(vpr_error "$out/genfasm.log")"
    elif ! grep -q '^Writing Implementation FASM' "$out/genfasm.log"; then
      status="failed: genfasm did not finish"
    else
      mv "$out/$fasm" "$out/genfasm.fasm"
      status=built
      if [[ $circuit == fasm-test/wire ]]; then
        # test_fasm.cpp's fasm_integration_test: every rr graph edge gets
        # fasm_features metadata, then genfasm reads that rr graph.
        python3 "$REPO_ROOT/tools/e2e/vtr/add-rr-edge-metadata.py" \
          "$out/rr_graph.xml" "$out/rr_graph_meta.xml"
        r=$(cd "$out" && run_timed genfasm-rr-metadata.log genfasm "${common[@]}" --read_rr_graph rr_graph_meta.xml)
        fasm=$(cd "$out" && ls ./*.fasm 2>/dev/null | grep -v '/genfasm.fasm$' | head -1 || true)
        if [[ ${r%% *} != 0 || -z $fasm ]]; then
          status="failed: genfasm --read_rr_graph exit ${r%% *}"
        else
          mv "$out/$fasm" "$out/genfasm-rr-metadata.fasm"
        fi
      fi
    fi
  fi
  if [[ $KEEP == 0 ]]; then
    (cd "$out" && rm -f ./*.xml ./*.net ./*.net.post_routing ./*.place ./*.route ./*.rpt ./*.blif ./*.eblif)
  fi
  compress_logs "$out"
  write_info "$out" test_fasm_arch "$circuit" "$netlist" 100 "$status" "$vpr_s" "$genfasm_s" "$names"
  [[ $status == built || $status == unimplementable* || $status == not\ run* ]]
}

# The reference frames and bitstream of OUT/top.fasm: the f4pga flow's
# xcfasm command line (as tools/e2e/run-f4pga-examples.sh), with --frm_out.
reference_frames() {
  local out="$1" part="$2" family="$3"
  local db="$F4PGA_PRJXRAY_DB/$family"
  xcfasm --db-root "$db" --part "$part" --part_file "$db/$part/part.yaml" \
    --sparse --emit_pudc_b_pullup --fn_in "$out/top.fasm" \
    --frm_out "$out/top.frm" --bit_out "$out/top.bit" \
    --frm2bit xc7frames2bit > "$out/xcfasm.log" 2>&1
}

# The VTR Verilog benchmarks through the f4pga flow (group `verilog`).
verilog_circuits() {
  (cd "$VTR/vtr_flow/benchmarks/verilog" && ls ./*.v) | sed 's#^\./##; s#\.v$##' | sort
}

run_verilog() {
  local circuit="$1"
  local board=arty_35 device=xc7a50t_test part=xc7a35tcsg324-1 family=artix7
  local out="$OUT/xc7a50t_test/$circuit/$board"
  local arch="$F4PGA_INSTALL_DIR/xc7/share/f4pga/arch/$device"
  local netlist="vtr_flow/benchmarks/verilog/$circuit.v"
  rm -rf "$out"
  mkdir -p "$out"
  if [[ ! -f "$VTR/$netlist" ]]; then
    log "unknown verilog circuit $circuit (--list verilog)"
    return 2
  fi
  cp "$VTR/$netlist" "$out/"
  local top status=built synth_s=- pnr_s=- genfasm_s=- r
  # VTR's hard blocks (single_port_ram, ...): models of them.
  local vfiles=("$circuit.v")
  if python3 "$REPO_ROOT/tools/e2e/vtr/hard-block-models.py" "$VTR/vtr_flow/primitives.v" \
      "$out/$circuit.v" "$out/vtr_hard_blocks.v" > "$out/hard_blocks.txt"; then
    vfiles+=(vtr_hard_blocks.v)
  fi
  top=$(cd "$out" && timeout 600 yosys -p "read_verilog ${vfiles[*]}; hierarchy -auto-top" 2>/dev/null \
    | sed -n 's/^Top module:  *\\//p' | head -1 || true)
  if [[ -z $top ]]; then
    status="unimplementable: yosys cannot elaborate the design (no top module)"
  fi
  if [[ $status == built ]]; then
    r=$(cd "$out" && run_timed synth.log symbiflow_synth -t "$top" -v "${vfiles[@]}" -d "$family" -p "$part")
    synth_s=${r#* }
    if [[ ${r%% *} == 124 ]]; then
      status="unimplementable: synthesis timed out ($TIMEOUT s)"
    elif [[ ${r%% *} != 0 ]]; then
      status="unimplementable: synthesis: $(grep -m1 -E '^ERROR|Error' "$out/synth.log" | cut -c1-200)"
      [[ $status == "unimplementable: synthesis: " ]] && status="failed: synthesis exit ${r%% *}"
    fi
  fi
  if [[ $status == built ]]; then
    local why
    if ! why=$(python3 "$REPO_ROOT/tools/e2e/vtr/make-pcf.py" "$out/$top.eblif" "$arch/$part/pinmap.csv" "$out/top.pcf"); then
      status="unimplementable: $why"
    fi
  fi
  if [[ $status == built ]]; then
    local start step
    start=$(date +%s.%N)
    for step in pack place route; do
      local args=(-e "$top.eblif" -d "$device")
      [[ $step == place ]] && args+=(-n "$top.net" -P "$part" -p top.pcf)
      r=$(cd "$out" && run_timed "$step.log" "symbiflow_$step" "${args[@]}")
      if [[ ${r%% *} != 0 ]]; then
        status="unimplementable: $step: $(vpr_error "$out/$step.log")"
        [[ $status == "unimplementable: $step: " ]] && status="failed: $step exit ${r%% *}"
        break
      fi
    done
    pnr_s=$(python3 -c "print(round($(date +%s.%N) - $start, 2))")
  fi
  if [[ $status == built ]]; then
    r=$(cd "$out" && run_timed write_fasm.log symbiflow_write_fasm -e "$top.eblif" -d "$device")
    genfasm_s=${r#* }
    local why
    if [[ ${r%% *} != 0 || ! -f "$out/$top.fasm" ]]; then
      status="failed: symbiflow_write_fasm exit ${r%% *}"
    elif ! why=$("$REPO_ROOT/tools/e2e/f4pga/check-genfasm.sh" "$out" "$out/write_fasm.log"); then
      status="failed: $why"
    else
      [[ $top == top ]] || mv "$out/$top.fasm" "$out/top.fasm"
      reference_frames "$out" "$part" "$family" || status="failed: xcfasm"
    fi
  fi
  if [[ $KEEP == 0 ]]; then
    (cd "$out" && find . -maxdepth 1 -type f ! -name 'top.*' ! -name '*.log' ! -name 'top.pcf' -delete)
  fi
  compress_logs "$out"
  write_info "$out" "$device" "$circuit" "$netlist" 500 "$status" "$pnr_s" "$genfasm_s" - "$board" "$part" "$family" no "$synth_s" "$top"
  [[ $status == built || $status == unimplementable* ]]
}

run_xc7() {
  local circuit="$1"
  local line
  line=$(echo "$XC7_CIRCUITS" | awk -v c="$circuit" '$1 == c')
  if [[ -z $line ]]; then
    log "unknown xc7a50t_test circuit $circuit (--list)"
    return 2
  fi
  local board device part family task
  read -r _ board device part family task <<<"$line"
  local out="$OUT/xc7a50t_test/$circuit/$board"
  local arch="$F4PGA_INSTALL_DIR/xc7/share/f4pga/arch/$device"
  rm -rf "$out"
  mkdir -p "$out"
  local netlist="benchmarks/circuits/$circuit.eblif"
  if [[ ! -f "$arch/arch.timing.xml" ]]; then
    write_info "$out" "$device" "$circuit" "$netlist" 500 "device not installed" - - - "$board" "$part" "$family" "$task"
    return 0
  fi
  cp "$BENCH/$netlist" "$out/$circuit.eblif"
  local rr="rr_graph_${device}"
  # shellcheck disable=SC2206
  local common=("$arch/arch.timing.xml" "$circuit.eblif" --read_rr_graph "$arch/$rr.rr_graph.real.bin" $XC7_VPR_OPTIONS)
  local r status vpr_s genfasm_s=-
  r=$(cd "$out" && run_timed vpr.log vpr "${common[@]}" \
    --read_router_lookahead "$arch/$rr.lookahead.bin" \
    --read_placement_delay_lookup "$arch/$rr.place_delay.bin" \
    --sdc_file "$BENCH/benchmarks/sdc/$circuit.sdc" \
    --fix_clusters "$BENCH/benchmarks/place_constr/$circuit.place")
  vpr_s=${r#* }
  if [[ ${r%% *} != 0 ]]; then
    status="failed: vpr exit ${r%% *} $(vpr_error "$out/vpr.log")"
  else
    r=$(cd "$out" && run_timed genfasm.log genfasm "${common[@]}")
    genfasm_s=${r#* }
    local fasm
    fasm=$(cd "$out" && ls ./*.fasm 2>/dev/null | head -1 || true)
    if [[ ${r%% *} != 0 || -z $fasm ]] || ! grep -q '^Writing Implementation FASM' "$out/genfasm.log"; then
      status="failed: genfasm exit ${r%% *} $(vpr_error "$out/genfasm.log")"
    else
      [[ $fasm == ./top.fasm ]] || mv "$out/$fasm" "$out/top.fasm"
      status=built
      reference_frames "$out" "$part" "$family" || status="failed: xcfasm"
    fi
  fi
  if [[ $KEEP == 0 ]]; then
    (cd "$out" && rm -f ./*.eblif ./*.net ./*.net.post_routing ./*.place ./*.route ./*.rpt)
  fi
  compress_logs "$out"
  write_info "$out" "$device" "$circuit" "$netlist" 500 "$status" "$vpr_s" "$genfasm_s" - "$board" "$part" "$family" "$task"
  [[ $status == built ]]
}

KEEP=0
SIZE_FILTER=1
JOBS=1
GROUP=
SELECT=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --list) shift; F4PGA_E2E_ROOT="${F4PGA_E2E_ROOT:-}" list "${1:-}"; exit 0 ;;
    --keep) KEEP=1; shift ;;
    --no-size-filter) SIZE_FILTER=0; shift ;;
    --jobs) JOBS="$2"; shift 2 ;;
    --all) GROUP=all; shift ;;
    -h|--help) usage; exit 0 ;;
    -*) echo "unknown option $1" >&2; exit 2 ;;
    *) if [[ -z $GROUP ]]; then GROUP="$1"; else SELECT+=("$1"); fi; shift ;;
  esac
done
case "$GROUP" in
  test_fasm_arch|xc7a50t_test|verilog|all) ;;
  *) usage; exit 2 ;;
esac

# shellcheck source=tools/e2e/f4pga-env.sh
source "$REPO_ROOT/tools/e2e/f4pga-env.sh"
if [[ ! -x "$F4PGA_ENV/bin/genfasm" ]]; then
  log "the f4pga toolchain is not installed (tools/e2e/setup-f4pga.sh)"
  exit 3
fi
ensure_vtr
check_vtr_commit
mkdir -p "$OUT"
export OUT KEEP SIZE_FILTER TIMEOUT MEMORY_KB VTR BENCH REPO_ROOT XC7_CIRCUITS XC7_VPR_OPTIONS SIZE_LIMIT

rc=0
if [[ $GROUP == test_fasm_arch || $GROUP == all ]]; then
  if [[ ${#SELECT[@]} -gt 0 && $GROUP != all ]]; then
    todo=$(test_arch_circuits | awk 'NR == FNR { want[$1] = 1; next } $1 in want' <(printf '%s\n' "${SELECT[@]}") -)
  else
    todo=$(test_arch_circuits)
  fi
  log "test_fasm_arch: $(echo "$todo" | grep -c .) circuits, $JOBS at a time"
  export -f run_test_arch run_timed vpr_error write_info compress_logs log
  echo "$todo" | xargs -P "$JOBS" -L 1 bash -c 'run_test_arch "$0" "$1" "$SIZE_FILTER" || echo "FAILED $0" >&2' 2> "$OUT.test_fasm_arch.errors" || rc=1
  if grep -q '^FAILED' "$OUT.test_fasm_arch.errors" 2>/dev/null; then
    grep '^FAILED' "$OUT.test_fasm_arch.errors" >&2
    rc=1
  fi
  rm -f "$OUT.test_fasm_arch.errors"
  python3 "$REPO_ROOT/tools/e2e/vtr/write-info.py" --summary "$OUT/test_fasm_arch"
fi
if [[ $GROUP == xc7a50t_test || $GROUP == all ]]; then
  ensure_bench
  if [[ ${#SELECT[@]} -gt 0 && $GROUP != all ]]; then
    todo=("${SELECT[@]}")
  else
    mapfile -t todo < <(echo "$XC7_CIRCUITS" | awk 'NF { print $1 }')
  fi
  # One at a time: VPR needs about 4-6 GB with the xc7a50t_test rr graph.
  for c in "${todo[@]}"; do
    log "xc7a50t_test: $c"
    run_xc7 "$c" || rc=1
  done
  python3 "$REPO_ROOT/tools/e2e/vtr/write-info.py" --summary "$OUT/xc7a50t_test"
fi
if [[ $GROUP == verilog || $GROUP == all ]]; then
  if [[ ${#SELECT[@]} -gt 0 && $GROUP != all ]]; then
    todo=("${SELECT[@]}")
  else
    mapfile -t todo < <(verilog_circuits)
  fi
  for c in "${todo[@]}"; do
    log "verilog: $c"
    run_verilog "$c" || rc=1
  done
  python3 "$REPO_ROOT/tools/e2e/vtr/write-info.py" --summary "$OUT/xc7a50t_test"
fi
exit $rc

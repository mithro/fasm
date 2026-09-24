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
# First working end-to-end example for T7.1: synthesises f4pga-examples'
# xc7/counter_test/counter.v for the Digilent Arty A7-35T
# (xc7a35tcsg324-1, arty.xdc) with yosys, places & routes it with
# nextpnr-xilinx (openXC7), producing a real top.fasm -- then converts that
# FASM to frames and a bitstream with the openXC7 snap's own
# fasm2frames/xc7frames2bit (always available once tools/e2e/setup-openxc7.sh
# has run; tests/oracle/setup-xilinx.sh's independently-built prjxray C++
# xc7frames2bit is an equally valid alternative for this step -- see
# tests/oracle/xilinx-env.sh -- but is not required here).
#
# Usage:
#   tools/e2e/run-counter.sh
#
# Requires tools/e2e/setup-openxc7.sh to have been run first. Design
# sources are read from tests/e2e/designs/f4pga-examples/counter_test/
# (copied from chipsalliance/f4pga-examples, Apache-2.0; see the LICENSE
# note there) so this works without the upstream repository checked out.
#
# Output goes under tools/e2e/build/out/counter/ (gitignored):
#   top.json          yosys synth_xilinx JSON netlist
#   top_routed.json   nextpnr-xilinx placed & routed JSON
#   top.fasm          the FASM this task exists to produce
#   top.frm           fasm2frames output (frame deltas)
#   top.bit           xc7frames2bit output (a real, loadable Arty bitstream)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DESIGN_DIR="$REPO_ROOT/tests/e2e/designs/f4pga-examples/counter_test"
OUT_DIR="$SCRIPT_DIR/build/out/counter"

PART="xc7a35tcsg324-1"
FAMILY="artix7"
TOP="top"

log() {
  echo "[run-counter] $*"
}

if [[ ! -f "$SCRIPT_DIR/build/openxc7/status.json" ]]; then
  echo "run-counter.sh: openXC7 toolchain not set up; run tools/e2e/setup-openxc7.sh first" >&2
  exit 1
fi
# shellcheck source=/dev/null
source "$SCRIPT_DIR/openxc7-env.sh"

CHIPDB="$CHIPDB_DIR/$PART.bin"
if [[ ! -f "$CHIPDB" ]]; then
  echo "run-counter.sh: chip database for $PART not found at $CHIPDB;" >&2
  echo "  run tools/e2e/setup-openxc7.sh --parts $PART" >&2
  exit 1
fi

for f in counter.v arty.xdc; do
  if [[ ! -f "$DESIGN_DIR/$f" ]]; then
    echo "run-counter.sh: missing design source $DESIGN_DIR/$f" >&2
    exit 1
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
cp "$DESIGN_DIR/counter.v" "$DESIGN_DIR/arty.xdc" "$OUT_DIR/"

RUN_START=$(date +%s)

log "1/4 synthesis (yosys synth_xilinx, part $PART)"
T0=$(date +%s)
(
  cd "$OUT_DIR"
  yosys -p "read_verilog counter.v; synth_xilinx -flatten -abc9 -nobram -arch xc7 -top $TOP; write_json top.json" \
    > "$OUT_DIR/yosys.log" 2>&1
)
T1=$(date +%s)
log "   done in $((T1 - T0))s: $OUT_DIR/top.json ($(du -h "$OUT_DIR/top.json" | cut -f1))"

log "2/4 place & route (nextpnr-xilinx)"
T0=$(date +%s)
(
  cd "$OUT_DIR"
  nextpnr-xilinx --chipdb "$CHIPDB" --xdc arty.xdc --json top.json \
    --write top_routed.json --fasm top.fasm \
    > "$OUT_DIR/nextpnr.log" 2>&1
)
T1=$(date +%s)
if [[ ! -s "$OUT_DIR/top.fasm" ]]; then
  echo "run-counter.sh: nextpnr-xilinx did not produce a FASM file; see $OUT_DIR/nextpnr.log" >&2
  exit 1
fi
log "   done in $((T1 - T0))s: $OUT_DIR/top.fasm ($(wc -l <"$OUT_DIR/top.fasm") lines)"

log "3/4 fasm -> frames (fasm2frames)"
T0=$(date +%s)
fasm2frames --db-root "$PRJXRAY_DB_DIR/$FAMILY" --part "$PART" \
  "$OUT_DIR/top.fasm" "$OUT_DIR/top.frm" \
  > "$OUT_DIR/fasm2frames.log" 2>&1
T1=$(date +%s)
log "   done in $((T1 - T0))s: $OUT_DIR/top.frm ($(du -h "$OUT_DIR/top.frm" | cut -f1))"

log "4/4 frames -> bitstream (xc7frames2bit)"
T0=$(date +%s)
xc7frames2bit \
  -frm_file "$OUT_DIR/top.frm" -output_file "$OUT_DIR/top.bit" \
  -part_name "$PART" \
  -part_file "$PRJXRAY_DB_DIR/$FAMILY/$PART/part.yaml" \
  > "$OUT_DIR/xc7frames2bit.log" 2>&1
T1=$(date +%s)
log "   done in $((T1 - T0))s: $OUT_DIR/top.bit ($(du -h "$OUT_DIR/top.bit" | cut -f1))"

RUN_END=$(date +%s)

log "sha256:"
sha256sum "$OUT_DIR/top.fasm" "$OUT_DIR/top.frm" "$OUT_DIR/top.bit"
log "total time: $((RUN_END - RUN_START))s"
log "outputs under $OUT_DIR"

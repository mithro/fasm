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
# T7.2: builds one fpgas.online-test-designs Xilinx design/board with the
# LiteX SoC builder (tools/e2e/setup-litex.sh) + the openXC7 flow
# (tools/e2e/setup-openxc7.sh), then reproduces the reference frames/
# bitstream for the resulting FASM with the ORACLE tools
# (tests/oracle/{fasm2frames,xc7frames2bit,bitread}-oracle -- NOT openXC7's
# own bundled copies of the same tools, so this task's corpus is directly
# comparable against the rest of the Rust rewrite's differential tests).
#
# Usage:
#   tools/e2e/run-fpgas-online.sh DESIGN BOARD
#
# e.g.:
#   tools/e2e/run-fpgas-online.sh pmod-loopback arty
#   tools/e2e/run-fpgas-online.sh uart netv2
#
# Run `tools/e2e/run-fpgas-online.sh --list` for every known DESIGN/BOARD
# pair and its status (part, whether a chipdb is available yet, etc).
#
# Requires, in order:
#   tools/e2e/setup-openxc7.sh [--parts DEVICE]   (T7.1; DEVICE as printed
#                                                   by --list for parts
#                                                   beyond xc7a35tcsg324-1)
#   tools/e2e/setup-litex.sh
#
# Prerequisite design sources: a pinned checkout of fpgas.online-test-designs
# at tools/e2e/build/fpgas.online-test-designs (gitignored; this script
# does not create it -- see tools/e2e/README.md for how it was made: a
# tarball/checkout of commit 37d24079b28179558632abc12fd92af4ff00a036,
# matching the reference repository this task was briefed against).
#
# On success, copies results into
#   tests/corpus/xilinx/artix7/designs/fpgas.online-test-designs/<design>/<board>/
# (top.fasm, top.frm.xz [dense], top.sparse.frm.xz, README.md -- see
# "Corpus layout" below). The .bit is NEVER committed (not reproducible
# byte-for-byte -- embeds a build timestamp -- and adds no information
# over the .frm for FASM differential testing); its sha256 is recorded in
# the README instead.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FPGAS_SRC="$SCRIPT_DIR/build/fpgas.online-test-designs"
LITEX_VENV="$SCRIPT_DIR/build/litex-venv"
LITEX_PY="$LITEX_VENV/bin/python"
CHIPDB_OVERLAY="$SCRIPT_DIR/build/chipdb-overlay"
WORK_ROOT="$SCRIPT_DIR/build/out/fpgas-online"
CORPUS_ROOT="$REPO_ROOT/tests/corpus/xilinx/artix7/designs/fpgas.online-test-designs"

FPGAS_COMMIT_FILE="$FPGAS_SRC/.checkout-commit"

log() { echo "[run-fpgas-online] $*"; }
die() { echo "run-fpgas-online.sh: ERROR: $*" >&2; exit 1; }

# --- Design/board table ----------------------------------------------------
#
# Fields, '|' separated:
#   script        path under $FPGAS_SRC to the gateware .py
#   part          nextpnr-xilinx/prjxray-db device name (dash-speedgrade
#                 form, matching tools/e2e/setup-openxc7.sh --parts and the
#                 chipdb filename it produces)
#   family        prjxray-db family directory (artix7 for all 7-series
#                 parts used here)
#   extra_args    extra CLI args for the gateware script (space separated;
#                 "-" for none)
#   kind          "soc" (LiteXArgumentParser + Builder; output under
#                 build/<board>/gateware/) or "plain" (bare argparse +
#                 platform.build() directly; output under build/<board>/)
#
# variants not listed here (fomu, tt) are non-Xilinx and out of scope for
# this task (T7.2 is Xilinx only).
design_config() {
  case "$1:$2" in
    pmod-loopback:arty)   echo "gateware/gpio_loopback_arty.py|xc7a35tcsg324-1|artix7|-|plain" ;;
    pmod-loopback:netv2)  echo "gateware/gpio_loopback_netv2.py|xc7a35tfgg484-2|artix7|--variant a7-35|plain" ;;
    pmod-pin-id:arty)     echo "gateware/pmod_pin_id_arty.py|xc7a35tcsg324-1|artix7|-|plain" ;;
    pmod-pin-id:netv2)    echo "gateware/pmod_pin_id_netv2.py|xc7a35tfgg484-2|artix7|--variant a7-35|plain" ;;
    uart:arty)            echo "gateware/uart_soc_arty.py|xc7a35tcsg324-1|artix7|--no-compile-software|soc" ;;
    uart:netv2)           echo "gateware/uart_soc_netv2.py|xc7a35tfgg484-2|artix7|--variant a7-35 --no-compile-software|soc" ;;
    spi-flash-id:arty)    echo "gateware/spiflash_soc_arty.py|xc7a35tcsg324-1|artix7|--no-compile-software|soc" ;;
    spi-flash-id:netv2)   echo "gateware/spiflash_soc_netv2.py|xc7a35tfgg484-2|artix7|--variant a7-35 --no-compile-software|soc" ;;
    ethernet-test:arty)   echo "gateware/ethernet_soc_arty.py|xc7a35tcsg324-1|artix7|--no-compile-software|soc" ;;
    ethernet-test:netv2)  echo "gateware/ethernet_soc_netv2.py|xc7a35tfgg484-2|artix7|--variant a7-35 --no-compile-software|soc" ;;
    ddr-memory:arty)      echo "gateware/ddr_soc_arty.py|xc7a35tcsg324-1|artix7|--no-compile-software|soc" ;;
    ddr-memory:netv2)     echo "gateware/ddr_soc_netv2.py|xc7a35tfgg484-2|artix7|--variant a7-35 --no-compile-software|soc" ;;
    pcie-enumeration:netv2) echo "gateware/pcie_soc_netv2.py|xc7a35tfgg484-2|artix7|--variant a7-35|soc" ;;
    spi-flash-id:litefury) echo "gateware/spiflash_soc_acorn.py|xc7a100tfgg484-2|artix7|--variant cle-101 --no-compile-software|soc" ;;
    uart:litefury)         echo "gateware/uart_soc_acorn.py|xc7a100tfgg484-2|artix7|--variant cle-101 --no-compile-software|soc" ;;
    pmod-pin-id:litefury)  echo "gateware/pmod_pin_id_acorn.py|xc7a100tfgg484-2|artix7|--variant cle-101|plain" ;;
    spi-flash-id:acorn)    echo "gateware/spiflash_soc_acorn.py|xc7a200tfbg484-3|artix7|--variant cle-215+ --no-compile-software|soc" ;;
    uart:acorn)            echo "gateware/uart_soc_acorn.py|xc7a200tfbg484-3|artix7|--variant cle-215+ --no-compile-software|soc" ;;
    pmod-pin-id:acorn)     echo "gateware/pmod_pin_id_acorn.py|xc7a200tfbg484-3|artix7|--variant cle-215+|plain" ;;
    acorn-pcie:acorn)      echo "gateware/acorn_pcie.py|xc7a200tfbg484-3|artix7|--variant cle-215+|soc" ;;
    *) echo "" ;;
  esac
}

ALL_PAIRS=(
  pmod-loopback:arty pmod-loopback:netv2
  pmod-pin-id:arty pmod-pin-id:netv2
  uart:arty uart:netv2
  spi-flash-id:arty spi-flash-id:netv2
  ethernet-test:arty ethernet-test:netv2
  ddr-memory:arty ddr-memory:netv2
  pcie-enumeration:netv2
  spi-flash-id:litefury uart:litefury pmod-pin-id:litefury
  spi-flash-id:acorn uart:acorn pmod-pin-id:acorn
  acorn-pcie:acorn
)

if [[ "${1:-}" == "--config" ]]; then
  design_config "$2" "$3"
  exit 0
fi

if [[ "${1:-}" == "--list" ]]; then
  echo "design:board  part  kind  extra_args"
  for pair in "${ALL_PAIRS[@]}"; do
    design="${pair%%:*}"; board="${pair##*:}"
    cfg="$(design_config "$design" "$board")"
    IFS='|' read -r script part family extra kind <<<"$cfg"
    printf '%-28s %-20s %-6s %s\n' "$pair" "$part" "$kind" "$extra"
  done
  exit 0
fi

[[ $# -eq 2 ]] || { echo "usage: $0 DESIGN BOARD   (or --list)" >&2; exit 2; }
DESIGN="$1"
BOARD="$2"

CFG="$(design_config "$DESIGN" "$BOARD")"
[[ -n "$CFG" ]] || die "unknown design:board '$DESIGN:$BOARD' -- see --list"
IFS='|' read -r SCRIPT_REL PART FAMILY EXTRA_ARGS KIND <<<"$CFG"
[[ "$EXTRA_ARGS" == "-" ]] && EXTRA_ARGS=""

DESIGN_DIR="$FPGAS_SRC/designs/$DESIGN"
SCRIPT_ABS="$DESIGN_DIR/$SCRIPT_REL"
[[ -f "$SCRIPT_ABS" ]] || die "gateware script not found: $SCRIPT_ABS (is $FPGAS_SRC set up? see tools/e2e/README.md)"
[[ -x "$LITEX_PY" ]] || die "$LITEX_PY not found; run tools/e2e/setup-litex.sh first"
[[ -f "$SCRIPT_DIR/build/openxc7/status.json" ]] || die "openXC7 toolchain not set up; run tools/e2e/setup-openxc7.sh first"

# shellcheck source=/dev/null
source "$SCRIPT_DIR/openxc7-env.sh"

# nextpnr-xilinx's chipdb (built by setup-openxc7.sh into the MAIN tree's
# tools/e2e/build/openxc7/chipdb/, shared across worktrees) is named
# "<part>.bin" (dash-speedgrade form, e.g. xc7a35tfgg484-2.bin, matching
# --parts). LiteX's openxc7 toolchain (litex/build/xilinx/yosys_nextpnr.py
# finalize()) instead looks up "$CHIPDB/<dbpart>.bin", where <dbpart> is
# derived with a regex over the PLATFORM's raw (pre fixup) `device`
# string, not the corrected `--part` name -- so for most parts it is just
# the speedgrade-stripped part name (e.g. xc7a35tfgg484), but for some
# boards (e.g. Digilent Arty's a7-35 variant, raw device
# "xc7a35ticsg324-1L") that regex mis-parses an extra temperature-grade
# letter and asks for a differently-misspelled name (observed:
# "xc7a35icsg324.bin", missing the "t") -- a pre-existing LiteX quirk, not
# something to reproduce by guessing: instead of hardcoding it per board,
# $CHIPDB_OVERLAY is populated lazily by probing the actual error LiteX
# prints ("Chip database file '<path>' not found, generating...") on a
# first attempt, then retrying with the right symlink in place. This
# avoids ever letting LiteX's own auto-generation path run (it shells out
# to a hardcoded /snap/openxc7/current path and a bare "python3"/"pypy3",
# neither of which is guaranteed to work here -- tools/e2e/setup-openxc7.sh
# already built every chipdb this task needs, correctly, via the pinned
# python3.8 in the snap).
mkdir -p "$CHIPDB_OVERLAY"
CHIPDB_SRC="$CHIPDB_DIR/$PART.bin"
[[ -f "$CHIPDB_SRC" ]] || die "chip database for $PART not found at $CHIPDB_SRC; run (from the MAIN tree, /home/user/fasm) tools/e2e/setup-openxc7.sh --parts $PART"
DBPART="$(python3 -c "import re,sys; m=re.search(r'xc7([aksz])([0-9]+)(.*)-([0-9])', sys.argv[1]); print(f'xc7{m.group(1)}{m.group(2)}{m.group(3)}')" "$PART")"
ln -sf "$CHIPDB_SRC" "$CHIPDB_OVERLAY/$DBPART.bin"
export CHIPDB="$CHIPDB_OVERLAY"
# PRJXRAY_DB_DIR is already exported correctly by openxc7-env.sh (root
# containing artix7/kintex7/spartan7/zynq7 -- litex appends /$family itself).

OUT_DIR="$WORK_ROOT/$DESIGN-$BOARD"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

run_build() {
  local timeout_s="$1"
  set +e
  timeout "$timeout_s" "$LITEX_PY" "$SCRIPT_ABS" --toolchain openxc7 --build $EXTRA_ARGS \
    > "$OUT_DIR/build.log" 2>&1
  BUILD_RC=$?
  set -e
}

log "building $DESIGN/$BOARD (part $PART, $KIND, extra args: '${EXTRA_ARGS:-<none>}')"
log "script: $SCRIPT_ABS"
RUN_START=$(date +%s)
run_build 3600

if [[ "$BUILD_RC" -ne 0 ]]; then
  MISSING_CHIPDB="$(grep -oE "Chip database file '[^']+' not found" "$OUT_DIR/build.log" | head -1 | sed -e "s/^Chip database file '//" -e "s/' not found\$//" || true)"
  if [[ -n "$MISSING_CHIPDB" && "$MISSING_CHIPDB" != "$CHIPDB_OVERLAY/$DBPART.bin" ]]; then
    log "LiteX asked for a chipdb under a different name than expected ($MISSING_CHIPDB); linking it and retrying once"
    ln -sf "$CHIPDB_SRC" "$MISSING_CHIPDB"
    run_build 3600
  fi
fi
RUN_END=$(date +%s)
BUILD_SECONDS=$((RUN_END - RUN_START))
log "build exit=$BUILD_RC in ${BUILD_SECONDS}s; log: $OUT_DIR/build.log"

if [[ "$BUILD_RC" -ne 0 ]]; then
  log "BUILD FAILED -- last 40 lines of log:"
  tail -40 "$OUT_DIR/build.log" | sed -e 's/^/    /'
  die "build failed for $DESIGN/$BOARD (exit $BUILD_RC); see $OUT_DIR/build.log"
fi

# Find the produced FASM. "soc" builds put it under
# designs/<design>/build/<board>/gateware/*.fasm; "plain" builds put it
# directly under designs/<design>/build/<board>/*.fasm.
BUILD_TREE="$DESIGN_DIR/build/$BOARD"
mapfile -t FASM_FILES < <(find "$BUILD_TREE" -name '*.fasm' 2>/dev/null)
[[ "${#FASM_FILES[@]}" -ge 1 ]] || die "no .fasm produced under $BUILD_TREE (build reported success?); see $OUT_DIR/build.log"
[[ "${#FASM_FILES[@]}" -eq 1 ]] || log "WARNING: multiple .fasm files found, using the first: ${FASM_FILES[*]}"
FASM_SRC="${FASM_FILES[0]}"
cp "$FASM_SRC" "$OUT_DIR/top.fasm"
FASM_LINES=$(wc -l <"$OUT_DIR/top.fasm")
log "FASM: $FASM_SRC -> $OUT_DIR/top.fasm ($FASM_LINES lines)"

DB_ROOT="$PRJXRAY_DB_DIR/$FAMILY"
PART_YAML="$DB_ROOT/$PART/part.yaml"
[[ -f "$PART_YAML" ]] || die "part.yaml not found for $PART at $PART_YAML (prjxray-db doesn't know this part?)"

log "reference fasm2frames (dense)"
T0=$(date +%s)
"$REPO_ROOT/tests/oracle/fasm2frames-oracle" --db-root "$DB_ROOT" --part "$PART" \
  "$OUT_DIR/top.fasm" "$OUT_DIR/top.frm" > "$OUT_DIR/fasm2frames-dense.log" 2>&1
T1=$(date +%s)
DENSE_SECONDS=$((T1 - T0))
log "  done in ${DENSE_SECONDS}s: $OUT_DIR/top.frm ($(du -h "$OUT_DIR/top.frm" | cut -f1))"

log "reference fasm2frames (sparse)"
T0=$(date +%s)
"$REPO_ROOT/tests/oracle/fasm2frames-oracle" --db-root "$DB_ROOT" --part "$PART" --sparse \
  "$OUT_DIR/top.fasm" "$OUT_DIR/top.sparse.frm" > "$OUT_DIR/fasm2frames-sparse.log" 2>&1
T1=$(date +%s)
SPARSE_SECONDS=$((T1 - T0))
log "  done in ${SPARSE_SECONDS}s: $OUT_DIR/top.sparse.frm ($(du -h "$OUT_DIR/top.sparse.frm" | cut -f1))"

log "reference xc7frames2bit (from dense .frm)"
T0=$(date +%s)
"$REPO_ROOT/tests/oracle/xc7frames2bit-oracle" \
  -frm_file "$OUT_DIR/top.frm" -output_file "$OUT_DIR/top.bit" \
  -part_name "$PART" -part_file "$PART_YAML" \
  > "$OUT_DIR/xc7frames2bit.log" 2>&1
T1=$(date +%s)
BIT_SECONDS=$((T1 - T0))
log "  done in ${BIT_SECONDS}s: $OUT_DIR/top.bit ($(du -h "$OUT_DIR/top.bit" | cut -f1))"

log "reference bitread (sanity check on the .bit)"
"$REPO_ROOT/tests/oracle/bitread-oracle" -z -y --part_file "$PART_YAML" -o "$OUT_DIR/top.bits" "$OUT_DIR/top.bit" \
  > "$OUT_DIR/bitread.log" 2>&1 \
  || log "  WARNING: bitread-oracle exited non-zero; see $OUT_DIR/bitread.log (not fatal -- .bit is not committed anyway)"

FASM_SHA256=$(sha256sum "$OUT_DIR/top.fasm" | awk '{print $1}')
FRM_SHA256=$(sha256sum "$OUT_DIR/top.frm" | awk '{print $1}')
SPARSE_FRM_SHA256=$(sha256sum "$OUT_DIR/top.sparse.frm" | awk '{print $1}')
BIT_SHA256=$(sha256sum "$OUT_DIR/top.bit" | awk '{print $1}')
FASM_BYTES=$(stat -c%s "$OUT_DIR/top.fasm")
FRM_BYTES=$(stat -c%s "$OUT_DIR/top.frm")
BIT_BYTES=$(stat -c%s "$OUT_DIR/top.bit")

echo "$FASM_SHA256 $FASM_BYTES $FRM_SHA256 $FRM_BYTES $SPARSE_FRM_SHA256 $BIT_SHA256 $BIT_BYTES $FASM_LINES $BUILD_SECONDS $DENSE_SECONDS $SPARSE_SECONDS $BIT_SECONDS" \
  > "$OUT_DIR/summary.txt"

log "done: $DESIGN/$BOARD"
log "  fasm sha256=$FASM_SHA256 ($FASM_BYTES bytes, $FASM_LINES lines)"
log "  frm  sha256=$FRM_SHA256 ($FRM_BYTES bytes)"
log "  bit  sha256=$BIT_SHA256 ($BIT_BYTES bytes) -- NOT committed"
log "outputs under $OUT_DIR (copy into $CORPUS_ROOT/$DESIGN/$BOARD/ -- see tools/e2e/README.md)"

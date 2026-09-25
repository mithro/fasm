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
# T7.6: builds the example designs of nextpnr-xilinx (xilinx/examples) and
# of the openXC7 organisation's demo repositories (demo-projects,
# primitive-tests) with the openXC7 snap toolchain (tools/e2e/setup-openxc7.sh),
# following each example's own script or Makefile: yosys synth_xilinx ->
# nextpnr-xilinx --fasm -> the snap's fasm2frames -> the snap's
# xc7frames2bit, all against the snap's bundled prjxray-db. Keeps the FASM
# and the reference frames and bitstream of every design;
# tools/e2e/compare-nextpnr-examples.py then compares the Rust tools with
# them (docs/rewrite/DESIGN-xilinx-db.md 8.13, tools/e2e/README.md).
#
# Usage:
#   tools/e2e/run-nextpnr-examples.sh --list          every entry, part, chipdb, status
#   tools/e2e/run-nextpnr-examples.sh --fetch         clone the pinned source repositories
#   tools/e2e/run-nextpnr-examples.sh [--build-chipdb] ID [ID ...]
#   tools/e2e/run-nextpnr-examples.sh [--build-chipdb] --all
#
# ID is SOURCE/EXAMPLE/BOARD as printed by --list. Outputs go to
# $NEXTPNR_EXAMPLES_OUT/ID/ (default tools/e2e/build/out/nextpnr-examples):
# top.fasm, top.frm (the snap fasm2frames' dense frames), top.bit (the
# snap xc7frames2bit's bitstream), the logs and info.json (status,
# commands, timings, sha256).
#
# Environment:
#   OPENXC7_E2E_BUILD          toolchain build directory (default: this
#                              checkout's tools/e2e/build; see openxc7-env.sh)
#   NEXTPNR_EXAMPLES_SRC       where --fetch clones the sources (default
#                              tools/e2e/build/nextpnr-examples-src)
#   NEXTPNR_XILINX_DIR, OPENXC7_DEMOS_DIR, OPENXC7_PRIMITIVE_TESTS_DIR
#                              checkouts to use instead (at the pinned
#                              commits below)
#   NEXTPNR_EXAMPLES_CHIPDB_DIR  chip databases this script builds with
#                              --build-chipdb for parts setup-openxc7.sh did
#                              not build (default
#                              tools/e2e/build/nextpnr-examples-chipdb)
#   NEXTPNR_EXAMPLES_TIMEOUT   seconds per tool run (default 1200)
#   NEXTPNR_EXAMPLES_VMEM_KB   ulimit -v per tool run (default 10485760, 10 GiB)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
E2E_BUILD="${OPENXC7_E2E_BUILD:-$SCRIPT_DIR/build}"
SRC_ROOT="${NEXTPNR_EXAMPLES_SRC:-$SCRIPT_DIR/build/nextpnr-examples-src}"
OUT_ROOT="${NEXTPNR_EXAMPLES_OUT:-$SCRIPT_DIR/build/out/nextpnr-examples}"
EXTRA_CHIPDB_DIR="${NEXTPNR_EXAMPLES_CHIPDB_DIR:-$SCRIPT_DIR/build/nextpnr-examples-chipdb}"
TOOL_TIMEOUT="${NEXTPNR_EXAMPLES_TIMEOUT:-1200}"
VMEM_KB="${NEXTPNR_EXAMPLES_VMEM_KB:-10485760}"

# Pinned sources. nextpnr-xilinx: the openXC7 fork's tag 0.8.2, the source
# of the installed openXC7 snap 0.8.2 (openXC7-snap's snapcraft.yaml:
# "source: https://github.com/openXC7/nextpnr-xilinx.git, source-branch:
# 0.8.2"). Its xilinx/examples are identical to upstream
# gatecat/nextpnr-xilinx's (branch xilinx-upstream,
# 8f178fc6a6d4dfbc57bef66c3ccff34d558047d5). demo-projects: the last
# commit before the demos moved to the himbaechel based openXC7/nextpnr
# (373b7643, "demo: follow the toolchain to openXC7/nextpnr"), i.e. the
# last one written for this toolchain generation.
NEXTPNR_XILINX_URL=https://github.com/openXC7/nextpnr-xilinx.git
NEXTPNR_XILINX_COMMIT=dea2f28c67fd1193ec72d0ba586800285e4c3648
DEMOS_URL=https://github.com/openXC7/demo-projects.git
DEMOS_COMMIT=c5246c583a7db3a73543af72e00193f2fa990d34
PRIMITIVE_TESTS_URL=https://github.com/openXC7/primitive-tests.git
PRIMITIVE_TESTS_COMMIT=d29ee7c58bdad361c690298d0c1b22004f9f4c02

NEXTPNR_XILINX_DIR="${NEXTPNR_XILINX_DIR:-$SRC_ROOT/nextpnr-xilinx}"
OPENXC7_DEMOS_DIR="${OPENXC7_DEMOS_DIR:-$SRC_ROOT/demo-projects}"
OPENXC7_PRIMITIVE_TESTS_DIR="${OPENXC7_PRIMITIVE_TESTS_DIR:-$SRC_ROOT/primitive-tests}"

log() { echo "[run-nextpnr-examples] $*"; }
die() { echo "run-nextpnr-examples.sh: ERROR: $*" >&2; exit 1; }

# --- The table ---------------------------------------------------------
#
# ID|part|chipdb device|family|kind|path|arg
#
#   part          the design's part (fasm2frames --part)
#   chipdb device the nextpnr-xilinx chip database used (the same package;
#                 the speed grade does not change a chipdb, the prjxray-db
#                 part directories of one package are identical)
#   kind          nx-script: nextpnr-xilinx xilinx/examples/<path>/<arg>.sh,
#                   whose commands are run_nx_script's
#                 make: `make` in <path> of the source repository (its
#                   own Makefile), project <arg>
#                 regression: demo-projects regression/<path>, run like
#                   its run.sh, then fasm2frames/xc7frames2bit like
#                   openXC7.mk
#                 litex-build: a LiteX build directory <path>/<arg>
#                   (build_top.sh and the Verilog LiteX generated are
#                   in the repository), run like its build_top.sh
#                 skip: not built, <arg> says why
ENTRIES=(
  "nextpnr-xilinx/blinky/arty-a35|xc7a35tcsg324-1|xc7a35tcsg324-1|artix7|nx-script|arty-a35|blinky"
  "nextpnr-xilinx/attosoc/arty-a35|xc7a35tcsg324-1|xc7a35tcsg324-1|artix7|nx-script|arty-a35|attosoc"
  "nextpnr-xilinx/blinky/artyz7-20|xc7z020clg400-1|-|zynq7|skip|artyz7-20|Zynq-7000 xc7z020 (zynq7; no chipdb, out of scope)"
  "nextpnr-xilinx/blinky/xczu2cg|xczu2cg-sbva484-1-e|-|-|skip|blinky|UltraScale+ (xczu2cg chipdb, RapidWright json2dcp + Vivado, no FASM)"
  "nextpnr-xilinx/attosoc/xczu2cg|xczu2cg-sbva484-1-e|-|-|skip|attosoc|UltraScale+ (xczu2cg chipdb, RapidWright json2dcp + Vivado, no FASM)"
  "nextpnr-xilinx/blinky/zcu104|xczu7ev-ffvc1156-2-e|-|-|skip|zcu104|UltraScale+ (xczu7ev chipdb, RapidWright json2dcp + Vivado, no FASM)"
  "openxc7-demo-projects/blinky/digilent-arty|xc7a35tcsg324-1|xc7a35tcsg324-1|artix7|make|blinky-digilent-arty|blinky"
  "openxc7-demo-projects/blinky/digilent-basys-3|xc7a35tcpg236-1|xc7a35tcpg236-1|artix7|make|blinky-digilent-basys-3|blinky"
  "openxc7-demo-projects/litex-sata/alientek-davincipro|xc7a35tfgg484-2|xc7a35tfgg484-2|artix7|make|litex-sata-alientek-davincipro|litex_pcie"
  "openxc7-demo-projects/litex-ddr/qmtech-artix7|xc7a100tfgg676-1|xc7a100tfgg676-1|artix7|make|litex-ddr-qmtech-artix7|qmtech_artix7_fgg676"
  "openxc7-demo-projects/regression-bram-sdp-unused-port/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|bram-sdp-unused-port|-"
  "openxc7-demo-projects/regression-bufg-fabric-driven/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|bufg-fabric-driven|-"
  "openxc7-demo-projects/regression-bufh-clock-constraint/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|bufh-clock-constraint|-"
  "openxc7-demo-projects/regression-bufio-in-use/xc7a35tcsg324|xc7a35tcsg324-1|xc7a35tcsg324-1|artix7|regression|bufio-in-use|-"
  "openxc7-demo-projects/regression-bufr-pad-site/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|bufr-pad-site|-"
  "openxc7-demo-projects/regression-bufr-sink-region/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|bufr-sink-region|-"
  "openxc7-demo-projects/regression-clock-srcc-bufg/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|clock-srcc-bufg|-"
  "openxc7-demo-projects/regression-config-primitive-startupe2/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|config-primitive-startupe2|-"
  "openxc7-demo-projects/regression-const-holdout/xc7a35tcsg324|xc7a35tcsg324-1|xc7a35tcsg324-1|artix7|regression|const-holdout|-"
  "openxc7-demo-projects/regression-dsp-const-only-pins/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|dsp-const-only-pins|-"
  "openxc7-demo-projects/regression-dup-package-pin/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|dup-package-pin|-"
  "openxc7-demo-projects/regression-iddr-four-iff-flops/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|iddr-four-iff-flops|-"
  "openxc7-demo-projects/regression-lutram-clkinv/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|lutram-clkinv|-"
  "openxc7-demo-projects/regression-lutram-ram64x1s/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|lutram-ram64x1s|-"
  "openxc7-demo-projects/regression-srl-init/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|srl-init|-"
  "openxc7-demo-projects/regression-srl-wemux/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|srl-wemux|-"
  "openxc7-demo-projects/regression-xorigport-unknown-name/xc7a200tfbg484|xc7a200tfbg484-2|xc7a200tfbg484-3|artix7|regression|xorigport-unknown-name|-"
  "openxc7-demo-projects/regression-fdse-fdpe-undefined-init/xc7z010clg400|xc7z010clg400-1|-|zynq7|skip|fdse-fdpe-undefined-init|Zynq-7000 xc7z010 (zynq7; out of scope)"
  "openxc7-demo-projects/regression-lut_shared_pin/xc7z010clg400|xc7z010clg400-1|-|zynq7|skip|lut_shared_pin|Zynq-7000 xc7z010 (zynq7; out of scope)"
  "openxc7-primitive-tests/jtag-test/acorn-cle215|xc7a200tfbg484-3|xc7a200tfbg484-3|artix7|make|clb-tests/jtag-test|top"
  "openxc7-primitive-tests/gtp_channel/xc7a35tfgg484|xc7a35tfgg484-2|xc7a35tfgg484-2|artix7|make|gtp_channel|gtp_channel"
  "openxc7-primitive-tests/gtp_common-external-refclk/xc7a100tfgg484|xc7a100tfgg484-1|xc7a100tfgg484-2|artix7|make|gtp_common/external-refclk|gtp_common"
  "openxc7-primitive-tests/gtp_common-internal-refclk/xc7a100tfgg484|xc7a100tfgg484-1|xc7a100tfgg484-2|artix7|make|gtp_common/internal-refclk|gtp_common"
  "openxc7-primitive-tests/startupe2/qmtech-artix7|xc7a100tfgg676-1|xc7a100tfgg676-1|artix7|make|startupe2|startup"
  "openxc7-primitive-tests/mmcm-blinky-artix/xc7a100tfgg676|xc7a100tfgg676-1|xc7a100tfgg676-1|artix7|make|mmcm-blinky-artix|blinky"
  "openxc7-primitive-tests/mmcm-blinky-artixx/xc7a100tfgg676|xc7a100tfgg676-1|xc7a100tfgg676-1|artix7|make|mmcm-blinky-artixx|blinky"
  "openxc7-primitive-tests/mmcm-reconfig/qmtech-artix7|xc7a100tfgg676-1|xc7a100tfgg676-1|artix7|make|mmcm-reconfig|mmcm_reconfig"
  "openxc7-primitive-tests/pll-reconfig/qmtech-artix7|xc7a100tfgg676-1|xc7a100tfgg676-1|artix7|make|pll-reconfig|pll_reconfig"
  "openxc7-primitive-tests/bscane2/qmtech-artix7|xc7a100tfgg676-1|xc7a100tfgg676-1|artix7|litex-build|bscane2|build"
  "openxc7-primitive-tests/mmcm-blinky/xc7s50csga324|xc7s50csga324-1|-|spartan7|skip|mmcm-blinky|Spartan-7 xc7s50 (spartan7; out of scope)"
  "openxc7-primitive-tests/mmcm-blinky-kintex/xc7k70tfbg676|xc7k70tfbg676-1|-|kintex7|skip|mmcm-blinky-kintex|Kintex-7 xc7k70t (kintex7; out of scope)"
  "openxc7-primitive-tests/gtx_channel/xc7k70tfbg676|xc7k70tfbg676-1|-|kintex7|skip|gtx_channel|Kintex-7 xc7k70t (kintex7; out of scope)"
  "openxc7-primitive-tests/gtx_common-internal-refclk/xc7k70tfbg676|xc7k70tfbg676-1|-|kintex7|skip|gtx_common/internal-refclk|Kintex-7 xc7k70t (kintex7; out of scope)"
  "openxc7-primitive-tests/dsp-tests-basic-mult/xc7k160tffg676|xc7k160tffg676-2|-|kintex7|skip|dsp-tests/basic-mult|Kintex-7 xc7k160t (kintex7; out of scope)"
  "openxc7-primitive-tests/dsp-tests-mult-harness/xc7k160tffg676|xc7k160tffg676-2|-|kintex7|skip|dsp-tests/mult-harness|Kintex-7 xc7k160t (kintex7; out of scope)"
  "openxc7-primitive-tests/iologic-tests-iddr/xc7k160tffg676|xc7k160tffg676-2|-|kintex7|skip|iologic-tests/iddr|Kintex-7 xc7k160t (kintex7; out of scope)"
  "openxc7-primitive-tests/iologic-tests-idelay/xc7k325tffg676|xc7k325tffg676-1|-|kintex7|skip|iologic-tests/idelay|Kintex-7 xc7k325t (kintex7; out of scope)"
  "openxc7-primitive-tests/iologic-tests-iobuf/xc7k325tffg676|xc7k325tffg676-1|-|kintex7|skip|iologic-tests/iobuf|Kintex-7 xc7k325t (kintex7; out of scope)"
  "openxc7-primitive-tests/iologic-tests-iserdes/xc7k325tffg676|xc7k325tffg676-1|-|kintex7|skip|iologic-tests/iserdes|Kintex-7 xc7k325t (kintex7; out of scope)"
  "openxc7-primitive-tests/iologic-tests-mmcm/xc7k160tffg676|xc7k160tffg676-2|-|kintex7|skip|iologic-tests/mmcm|Kintex-7 xc7k160t (kintex7; out of scope)"
  "openxc7-primitive-tests/iologic-tests-oddr/xc7k160tffg676|xc7k160tffg676-2|-|kintex7|skip|iologic-tests/oddr|Kintex-7 xc7k160t (kintex7; out of scope)"
  "openxc7-primitive-tests/iologic-tests-odelay/xc7k160tffg676|xc7k160tffg676-2|-|kintex7|skip|iologic-tests/odelay|Kintex-7 xc7k160t (kintex7; out of scope)"
  "openxc7-primitive-tests/iologic-tests-oserdes/xc7k325tffg676|xc7k325tffg676-1|-|kintex7|skip|iologic-tests/oserdes|Kintex-7 xc7k325t (kintex7; out of scope)"
  "openxc7-primitive-tests/iologic-tests-tristate/xc7k325tffg676|xc7k325tffg676-1|-|kintex7|skip|iologic-tests/tristate|Kintex-7 xc7k325t (kintex7; out of scope)"
)

entry() {
  local e
  for e in "${ENTRIES[@]}"; do
    [[ "${e%%|*}" == "$1" ]] && { echo "$e"; return 0; }
  done
  return 1
}

source_dir() {
  case "$1" in
    nextpnr-xilinx) echo "$NEXTPNR_XILINX_DIR/xilinx/examples" ;;
    openxc7-demo-projects) echo "$OPENXC7_DEMOS_DIR" ;;
    openxc7-primitive-tests) echo "$OPENXC7_PRIMITIVE_TESTS_DIR" ;;
  esac
}

source_commit() {
  case "$1" in
    nextpnr-xilinx) echo "$NEXTPNR_XILINX_COMMIT" ;;
    openxc7-demo-projects) echo "$DEMOS_COMMIT" ;;
    openxc7-primitive-tests) echo "$PRIMITIVE_TESTS_COMMIT" ;;
  esac
}

source_url() {
  case "$1" in
    nextpnr-xilinx) echo "$NEXTPNR_XILINX_URL" ;;
    openxc7-demo-projects) echo "$DEMOS_URL" ;;
    openxc7-primitive-tests) echo "$PRIMITIVE_TESTS_URL" ;;
  esac
}

chipdb_path() {
  # The chipdb for DEVICE: setup-openxc7.sh's, else one built here.
  local device="$1"
  if [[ -f "$E2E_BUILD/openxc7/chipdb/$device.bin" ]]; then
    echo "$E2E_BUILD/openxc7/chipdb/$device.bin"
  elif [[ -f "$EXTRA_CHIPDB_DIR/$device.bin" ]]; then
    echo "$EXTRA_CHIPDB_DIR/$device.bin"
  fi
}

usage() {
  sed -n '/^# Usage:/,/^set -euo/p' "${BASH_SOURCE[0]}" | sed -e '$d' -e 's/^# \{0,1\}//'
}

fetch() {
  local name url commit dir
  for name in nextpnr-xilinx openxc7-demo-projects openxc7-primitive-tests; do
    url="$(source_url "$name")"
    commit="$(source_commit "$name")"
    case "$name" in
      nextpnr-xilinx) dir="$NEXTPNR_XILINX_DIR" ;;
      openxc7-demo-projects) dir="$OPENXC7_DEMOS_DIR" ;;
      *) dir="$OPENXC7_PRIMITIVE_TESTS_DIR" ;;
    esac
    if [[ ! -d "$dir/.git" ]]; then
      log "fetching $url $commit into $dir"
      mkdir -p "$dir"
      git -C "$dir" init -q
      git -C "$dir" remote add origin "$url"
    fi
    # Only the pinned commit (depth 1); no submodules: nextpnr-xilinx's
    # are the prjxray-db and metadata the snap already bundles.
    if [[ "$(git -C "$dir" rev-parse HEAD 2>/dev/null || true)" != "$commit" ]]; then
      timeout 900 git -C "$dir" fetch -q --depth 1 origin "$commit"
      git -C "$dir" -c advice.detachedHead=false checkout -q "$commit"
    fi
    log "$name at $(git -C "$dir" rev-parse HEAD)"
  done
}

check_commit() {
  local name="$1" dir="$2" want head
  want="$(source_commit "$name")"
  [[ -d "$dir" ]] || die "$dir not found; run $0 --fetch (or set its variable, see --help)"
  head="$(git -C "$dir" rev-parse HEAD 2>/dev/null || true)"
  [[ "$head" == "$want" ]] || die "$dir is at '${head:-?}', not the pinned $want ($0 --fetch)"
}

# Copies SRC/REL into DST/REL and links everything else of SRC's tree on
# the way into DST, so the design's relative paths (../openXC7.mk,
# ../vexriscv/VexRiscv.v, ../attosoc/attosoc.v) resolve while the build
# never writes into the source checkout.
mirror() {
  local src="$1" dst="$2" rel="$3" first rest f
  mkdir -p "$dst"
  first="${rel%%/*}"
  rest=""
  [[ "$rel" == */* ]] && rest="${rel#*/}"
  for f in "$src"/* "$src"/.[!.]*; do
    [[ -e "$f" ]] || continue
    [[ "$(basename "$f")" == .git ]] && continue
    [[ "$(basename "$f")" == "$first" ]] && continue
    ln -s "$f" "$dst/$(basename "$f")"
  done
  if [[ -n "$rest" ]]; then
    mirror "$src/$first" "$dst/$first" "$rest"
  else
    cp -a "$src/$first" "$dst/$first"
  fi
}

# Runs a tool with the time and memory caps, appending to LOG; records the
# seconds in the global LAST_SECONDS.
LAST_SECONDS=0
capped() {
  local log="$1"; shift
  local t0 t1 rc
  t0=$(date +%s.%N)
  set +e
  ( ulimit -v "$VMEM_KB"; timeout "$TOOL_TIMEOUT" "$@" ) >>"$log" 2>&1
  rc=$?
  set -e
  t1=$(date +%s.%N)
  LAST_SECONDS=$(python3 -c "print(round($t1 - $t0, 2))")
  return $rc
}

TIMES=()
step() {
  # step NAME LOG COMMAND...: a timed, capped flow step.
  local name="$1" log="$2"; shift 2
  echo "\$ $*" >>"$log"
  local rc=0
  capped "$log" "$@" || rc=$?
  echo "$name=$LAST_SECONDS" >>"$OUT/.times"
  [[ "$rc" -eq 0 ]] || echo "exit status $rc" >>"$log"
  return $rc
}

write_info() {
  # write_info STATUS [NOTE]
  local status="$1" note="${2:-}"
  STATUS="$status" NOTE="$note" ID="$ID" PART="$PART" DEVICE="$DEVICE" \
    FAMILY="$FAMILY" KIND="$KIND" SRC_PATH="$SRC_PATH" ARG="$ARG" \
    COMMIT="$(source_commit "$SOURCE")" URL="$(source_url "$SOURCE")" \
    CHIPDB="${CHIPDB:-}" TIMES="${TIMES[*]:-}" OUT="$OUT" \
    YOSYS_VERSION="$(yosys -V 2>/dev/null || true)" \
    NEXTPNR_VERSION="$(nextpnr-xilinx --version 2>&1 | head -1 || true)" \
    python3 - <<'PYEOF'
import hashlib, json, os
e = os.environ
out = e['OUT']
info = {
    'id': e['ID'], 'status': e['STATUS'], 'part': e['PART'],
    'chipdb_device': e['DEVICE'], 'family': e['FAMILY'], 'kind': e['KIND'],
    'source': {'url': e['URL'], 'commit': e['COMMIT'],
               'path': e['SRC_PATH'], 'arg': e['ARG']},
    'chipdb': e['CHIPDB'],
    'tools': {'yosys': e['YOSYS_VERSION'], 'nextpnr-xilinx':
              e['NEXTPNR_VERSION']},
    'seconds': {k: float(v) for k, v in
                (t.split('=') for t in e['TIMES'].split())},
}
if e['NOTE']:
    info['note'] = e['NOTE']
for name in ('top.fasm', 'top.frm', 'top.bit'):
    p = os.path.join(out, name)
    if os.path.exists(p):
        with open(p, 'rb') as f:
            data = f.read()
        info[name] = {'sha256': hashlib.sha256(data).hexdigest(),
                      'bytes': len(data)}
        if name == 'top.fasm':
            info[name]['lines'] = data.count(b'\n')
cmds = os.path.join(out, 'commands.txt')
if os.path.exists(cmds):
    with open(cmds) as f:
        info['commands'] = [l.rstrip('\n') for l in f if l.strip()]
with open(os.path.join(out, 'info.json'), 'w') as f:
    json.dump(info, f, indent=2, sort_keys=True)
    f.write('\n')
PYEOF
}

# This machine's Yosys (OSS CAD Suite 2026-09-21, much newer than the snap)
# leaves RTLIL buffer-normal-form `$buf` cells in some -abc9 netlists that
# nextpnr-xilinx 0.8.2 cannot place ("no Bels remaining of type '$buf'"),
# the problem T7.2 met (tools/e2e/README.md, "A Yosys/abc9 $buf cell
# workaround"). The same fix, applied to the written netlist so that each
# example's own synthesis command stays as it is: when the JSON has `$buf`
# cells, `techmap -map +/techmap.v t:$buf` turns them into connections.
fix_buf() {
  local json="$1" log="$2"
  if grep -q '"type": "\$buf"' "$json"; then
    echo "$json: \$buf cells, applying the techmap workaround" >>"$log"
    # Recorded right after the (first) yosys line of the flow's commands.
    BUF_LINE="yosys -p 'read_json $(basename "$json"); techmap -map +/techmap.v t:\$buf; write_json $(basename "$json")'   # workaround, see run-nextpnr-examples.sh" \
      python3 -c '
import os, sys
path = sys.argv[1]
lines = open(path).read().splitlines()
at = next((i + 1 for i, l in enumerate(lines) if l.startswith("yosys ")), len(lines))
lines.insert(at, os.environ["BUF_LINE"])
open(path, "w").write("\n".join(lines) + "\n")
' "$OUT/commands.txt"
    step buf_fix "$log" yosys -q -p "read_json $json; techmap -map +/techmap.v t:\$buf; write_json $json" || return 1
    touch "$OUT/.buf_fix"
  fi
}

# nextpnr-xilinx xilinx/examples/arty-a35/{blinky,attosoc}.sh, with the
# chipdb path and the prjxray utilities (XRAY_UTILS_DIR/fasm2frames.py,
# XRAY_TOOLS_DIR/xc7frames2bit, database XRAY_DATABASE_DIR) mapped to the
# snap's.
run_nx_script() {
  local dir="$1" log="$OUT/build.log" synth sources proj="$ARG"
  case "$ARG" in
    blinky)
      synth="synth_xilinx -flatten -abc9 -nobram -arch xc7 -top top; write_json blinky.json"
      sources=(blinky.v) ;;
    attosoc)
      synth="synth_xilinx -flatten -nowidelut -abc9 -arch xc7 -top top; write_json attosoc.json"
      sources=(../attosoc/attosoc.v attosoc_top.v) ;;
    *) die "no commands for nextpnr-xilinx example $ARG" ;;
  esac
  (
    cd "$dir"
    {
      echo "yosys -p \"$synth\" ${sources[*]}"
      echo "nextpnr-xilinx --chipdb $DEVICE.bin --xdc arty.xdc --json $proj.json --write ${proj}_routed.json --fasm $proj.fasm"
      echo "fasm2frames --db-root <snap prjxray-db>/$FAMILY --part $PART $proj.fasm > $proj.frames"
      echo "xc7frames2bit --part_file <snap prjxray-db>/$FAMILY/$PART/part.yaml --part_name $PART --frm_file $proj.frames --output_file $proj.bit"
    } >"$OUT/commands.txt"
    step synth "$log" yosys -p "$synth" "${sources[@]}" || exit 10
    fix_buf "$proj.json" "$log" || exit 10
    step pnr "$log" nextpnr-xilinx --chipdb "$CHIPDB" --xdc arty.xdc \
      --json "$proj.json" --write "${proj}_routed.json" --fasm "$proj.fasm" || exit 11
    step fasm2frames "$log" bash -c 'fasm2frames --db-root "$1" --part "$2" "$3" > "$4"' _ \
      "$PRJXRAY_DB_DIR/$FAMILY" "$PART" "$proj.fasm" "$proj.frames" || exit 12
    step xc7frames2bit "$log" xc7frames2bit --part_file "$PRJXRAY_DB_DIR/$FAMILY/$PART/part.yaml" \
      --part_name "$PART" --frm_file "$proj.frames" --output_file "$proj.bit" || exit 13
  )
}

# The design's own Makefile, one target at a time (for the timings); the
# chipdb through an overlay directory of links named as the Makefile
# expects (<part without speed grade>.bin), the database the snap's.
run_make() {
  local dir="$1" log="$OUT/build.log" proj="$ARG" overlay="$OUT/chipdb" dbpart
  dbpart="$(echo "$PART" | sed -e 's/-[0-9]//g')"
  mkdir -p "$overlay"
  ln -sf "$CHIPDB" "$overlay/$dbpart.bin"
  local vars=(CHIPDB="$overlay" PRJXRAY_DB_DIR="$PRJXRAY_DB_DIR" DB_DIR="$PRJXRAY_DB_DIR")
  (
    cd "$dir"
    # Generated files some examples carry in their repository (e.g.
    # blinky-digilent-arty's blinky.json, blinky.bit): always rebuild.
    rm -f ./*.json ./*.fasm ./*.frames ./*.bit
    make -n "${vars[@]}" "$proj.bit" >"$OUT/commands.txt" 2>&1 || true
    step synth "$log" make "${vars[@]}" "$proj.json" || exit 10
    fix_buf "$proj.json" "$log" || exit 10
    step pnr "$log" make "${vars[@]}" "$proj.fasm" || exit 11
    step fasm2frames "$log" make "${vars[@]}" "$proj.frames" || exit 12
    step xc7frames2bit "$log" make "${vars[@]}" "$proj.bit" || exit 13
  )
}

# A LiteX build directory (primitive-tests bscane2/build): the commands of
# its build_top.sh (written by LiteX for a /usr/share/nextpnr install),
# with the chipdb and the database mapped to the snap's.
run_litex_build() {
  local dir="$1" log="$OUT/build.log"
  (
    cd "$dir"
    rm -f top.json top.fasm top.frames top.bit
    # top.ys reads top.v by the absolute path of the machine LiteX ran on.
    sed -i -e 's|^read_verilog .*/top\.v$|read_verilog top.v|' top.ys
    {
      echo "sed -i -e 's|^read_verilog .*/top\\.v\$|read_verilog top.v|' top.ys   # top.ys names LiteX's absolute path"
      echo "yosys -l top.rpt top.ys"
      echo "nextpnr-xilinx --json top.json --xdc top.xdc --fasm top.fasm --chipdb $DEVICE.bin --write top_routed.json --timing-allow-fail --seed 1"
      echo "fasm2frames --part $PART --db-root <snap prjxray-db>/$FAMILY top.fasm > top.frames"
      echo "xc7frames2bit --part_file <snap prjxray-db>/$FAMILY/$PART/part.yaml --part_name $PART --frm_file top.frames --output_file top.bit"
    } >"$OUT/commands.txt"
    step synth "$log" yosys -l top.rpt top.ys || exit 10
    fix_buf top.json "$log" || exit 10
    step pnr "$log" nextpnr-xilinx --json top.json --xdc top.xdc --fasm top.fasm \
      --chipdb "$CHIPDB" --write top_routed.json --timing-allow-fail --seed 1 || exit 11
    step fasm2frames "$log" bash -c 'fasm2frames --part "$1" --db-root "$2" "$3" > "$4"' _ \
      "$PART" "$PRJXRAY_DB_DIR/$FAMILY" top.fasm top.frames || exit 12
    step xc7frames2bit "$log" xc7frames2bit --part_file "$PRJXRAY_DB_DIR/$FAMILY/$PART/part.yaml" \
      --part_name "$PART" --frm_file top.frames --output_file top.bit || exit 13
  )
}

# demo-projects regression/run.sh for one case (its yosys and nextpnr
# command lines), then the snap fasm2frames and xc7frames2bit like
# openXC7.mk (run.sh itself stops at the FASM). Also records run.sh's own
# verdict (expect.txt, check.sh) against this older nextpnr-xilinx.
run_regression() {
  local d="$1" log="$OUT/build.log" synth_flags nextpnr_flags no_route=""
  synth_flags="-flatten -abc9 -nocarry -nodsp"
  [[ -f "$d/synth_flags" ]] && synth_flags="$(cat "$d/synth_flags")"
  nextpnr_flags=""
  [[ -f "$d/nextpnr_flags" ]] && nextpnr_flags="$(cat "$d/nextpnr_flags")"
  [[ -f "$d/no_route" ]] && no_route="--no-route"
  (
    cd "$d"
    local synth="read_verilog top.v; synth_xilinx $synth_flags -family xc7 -top top; write_json top.json"
    {
      echo "yosys -q -p \"$synth\""
      echo "nextpnr-xilinx --chipdb $DEVICE.bin --xdc top.xdc --json top.json --write top_routed.json --fasm top.fasm $nextpnr_flags $no_route --timing-allow-fail"
      echo "fasm2frames --part $PART --db-root <snap prjxray-db>/$FAMILY top.fasm > top.frames"
      echo "xc7frames2bit --part_file <snap prjxray-db>/$FAMILY/$PART/part.yaml --part_name $PART --frm_file top.frames --output_file top.bit"
    } >"$OUT/commands.txt"
    step synth "$log" yosys -q -p "$synth" || exit 10
    fix_buf top.json "$log" || exit 10
    # run.sh writes nextpnr's output to the case's nextpnr.log, which some
    # check.sh read (dup-package-pin, bufr-*, bufh-clock-constraint).
    # shellcheck disable=SC2086
    step pnr "$log" bash -c 'nextpnr-xilinx "$@" > nextpnr.log 2>&1; rc=$?; cat nextpnr.log; exit $rc' _ \
      --chipdb "$CHIPDB" --xdc top.xdc --json top.json \
      --write top_routed.json --fasm top.fasm $nextpnr_flags $no_route --timing-allow-fail || true
    local verdict=""
    if [[ -f expect_fail ]]; then
      verdict="expected-fail case"
    fi
    if [[ -f expect.txt && -s top.fasm ]]; then
      local miss=0 pat
      while read -r pat; do
        [[ -z "$pat" ]] && continue
        grep -qE -- "$pat" top.fasm || { miss=1; verdict+="${verdict:+; }fasm missing /$pat/"; }
      done < expect.txt
      [[ "$miss" -eq 0 ]] && verdict+="${verdict:+; }expect.txt ok"
    fi
    if [[ -x check.sh || -f check.sh ]] && [[ -s top.fasm || -f expect_fail || -n "$no_route" ]]; then
      # check.sh reads CHIPDB when it reruns nextpnr (xorigport-unknown-name).
      if FASM="$d/top.fasm" CASE_DIR="$d" CHIPDB="$CHIPDB" timeout 600 bash check.sh >"$OUT/check.log" 2>&1; then
        verdict+="${verdict:+; }check.sh ok"
      else
        verdict+="${verdict:+; }check.sh FAILS"
      fi
    fi
    [[ -n "$verdict" ]] && echo "$verdict" >"$OUT/.verdict"
    # A placement-level case: placed (14) or not (11); no FASM either way.
    if [[ -n "$no_route" ]]; then [[ -s top_routed.json ]] && exit 14; exit 11; fi
    [[ -s top.fasm ]] || exit 11
    step fasm2frames "$log" bash -c 'fasm2frames --part "$1" --db-root "$2" "$3" > "$4"' _ \
      "$PART" "$PRJXRAY_DB_DIR/$FAMILY" top.fasm top.frames || exit 12
    step xc7frames2bit "$log" xc7frames2bit --part_file "$PRJXRAY_DB_DIR/$FAMILY/$PART/part.yaml" \
      --part_name "$PART" --frm_file top.frames --output_file top.bit || exit 13
  )
}

build_chipdb() {
  local device="$1" bba bin
  mkdir -p "$EXTRA_CHIPDB_DIR"
  bba="$EXTRA_CHIPDB_DIR/$device.bba"
  bin="$EXTRA_CHIPDB_DIR/$device.bin"
  log "building the chipdb for $device into $EXTRA_CHIPDB_DIR (bbaexport.py + bbasm, like setup-openxc7.sh)"
  local t0 t1
  t0=$(date +%s)
  local fam
  fam="$(python3 -c "import sys; d=sys.argv[1]; print('zynq7' if 'xc7z' in d else 'kintex7' if 'xc7k' in d else 'spartan7' if 'xc7s' in d else 'artix7')" "$device")"
  ( ulimit -v "$VMEM_KB"; timeout "$TOOL_TIMEOUT" "$OPENXC7_PYTHON3" \
      "$OPENXC7_ROOT/opt/nextpnr-xilinx/python/bbaexport.py" --device "$device" \
      --xray "$PRJXRAY_DB_DIR/$fam" \
      --metadata "$OPENXC7_ROOT/opt/nextpnr-xilinx/external/nextpnr-xilinx-meta/$fam" \
      --bba "$bba" ) >"$EXTRA_CHIPDB_DIR/$device.log" 2>&1 \
    || { rm -f "$bba"; die "bbaexport.py failed for $device, see $EXTRA_CHIPDB_DIR/$device.log"; }
  ( ulimit -v "$VMEM_KB"; timeout "$TOOL_TIMEOUT" bbasm --l "$bba" "$bin" ) >>"$EXTRA_CHIPDB_DIR/$device.log" 2>&1 \
    || { rm -f "$bba" "$bin"; die "bbasm failed for $device"; }
  rm -f "$bba"
  t1=$(date +%s)
  log "chipdb for $device built in $((t1 - t0))s ($(du -h "$bin" | cut -f1))"
}

run_one() {
  ID="$1"
  local e
  e="$(entry "$ID")" || die "unknown id $ID (see --list)"
  IFS='|' read -r _ PART DEVICE FAMILY KIND SRC_PATH ARG <<<"$e"
  SOURCE="${ID%%/*}"
  OUT="$OUT_ROOT/$ID"
  TIMES=()
  CHIPDB=""
  rm -rf "$OUT"
  mkdir -p "$OUT"
  if [[ "$KIND" == skip ]]; then
    log "$ID: skipped: $ARG"
    write_info skipped "$ARG"
    return 0
  fi
  CHIPDB="$(chipdb_path "$DEVICE")"
  if [[ -z "$CHIPDB" ]]; then
    if [[ "$BUILD_CHIPDB" == 1 ]]; then
      build_chipdb "$DEVICE"
      CHIPDB="$(chipdb_path "$DEVICE")"
    else
      log "$ID: no chipdb for $DEVICE (tools/e2e/setup-openxc7.sh --parts $DEVICE, or --build-chipdb)"
      write_info "no chipdb" "no chip database for $DEVICE"
      return 0
    fi
  fi
  local src work
  case "$SOURCE" in
    nextpnr-xilinx) check_commit "$SOURCE" "$NEXTPNR_XILINX_DIR" ;;
    openxc7-demo-projects) check_commit "$SOURCE" "$OPENXC7_DEMOS_DIR" ;;
    *) check_commit "$SOURCE" "$OPENXC7_PRIMITIVE_TESTS_DIR" ;;
  esac
  src="$(source_dir "$SOURCE")"
  work="$OUT/work"
  local rc=0 fasm frames bit dir
  log "$ID: $KIND $SRC_PATH ($PART, chipdb $DEVICE)"
  case "$KIND" in
    nx-script)
      mirror "$src" "$work" "$SRC_PATH"
      dir="$work/$SRC_PATH"
      run_nx_script "$dir" || rc=$?
      fasm="$dir/$ARG.fasm"; frames="$dir/$ARG.frames"; bit="$dir/$ARG.bit" ;;
    make)
      mirror "$src" "$work" "$SRC_PATH"
      dir="$work/$SRC_PATH"
      run_make "$dir" || rc=$?
      fasm="$dir/$ARG.fasm"; frames="$dir/$ARG.frames"; bit="$dir/$ARG.bit" ;;
    litex-build)
      mirror "$src" "$work" "$SRC_PATH/$ARG"
      dir="$work/$SRC_PATH/$ARG"
      run_litex_build "$dir" || rc=$?
      fasm="$dir/top.fasm"; frames="$dir/top.frames"; bit="$dir/top.bit" ;;
    regression)
      mirror "$src" "$work" "regression/$SRC_PATH"
      dir="$work/regression/$SRC_PATH"
      rm -f "$dir/top.json" "$dir/top.fasm" "$dir/top_routed.json"
      run_regression "$dir" || rc=$?
      fasm="$dir/top.fasm"; frames="$dir/top.frames"; bit="$dir/top.bit"
      [[ -f "$OUT/.verdict" ]] && cp "$OUT/.verdict" "$OUT/regression-verdict.txt" ;;
  esac
  if [[ -f "$OUT/.times" ]]; then
    mapfile -t TIMES <"$OUT/.times"
  fi
  [[ -s "$fasm" ]] && cp "$fasm" "$OUT/top.fasm"
  [[ -s "$frames" ]] && cp "$frames" "$OUT/top.frm"
  [[ -s "$bit" ]] && cp "$bit" "$OUT/top.bit"
  local status note=""
  case "$rc" in
    0) status=built ;;
    10) status="synthesis failed" ;;
    11) status="place and route failed" ;;
    12) status="fasm2frames failed" ;;
    13) status="xc7frames2bit failed" ;;
    14) status="placement only (no_route case, no FASM)" ;;
    *) status="failed ($rc)" ;;
  esac
  if [[ "$rc" -ne 0 ]]; then
    note="$(grep -E 'ERROR|Error|error:|Traceback|Exception|Killed|exit status' "$OUT/build.log" | tail -3 | tr '\n' ' ' | cut -c1-600)"
  fi
  if grep -q '^exit status 124$' "$OUT/build.log" 2>/dev/null; then
    note="timed out after ${TOOL_TIMEOUT}s${note:+; $note}"
  fi
  if [[ -f "$OUT/.buf_fix" ]]; then
    note="${note:+$note; }\$buf cells removed with techmap (yosys workaround)"
  fi
  if [[ -f "$OUT/regression-verdict.txt" && -s "$OUT/regression-verdict.txt" ]]; then
    note="${note:+$note; }regression run.sh criteria: $(cat "$OUT/regression-verdict.txt")"
  fi
  write_info "$status" "$note"
  # Keep the build logs, drop the (large) work tree.
  cp "$dir"/*.log "$OUT/" 2>/dev/null || true
  rm -rf "$work"
  rm -f "$OUT/.times" "$OUT/.verdict" "$OUT/.buf_fix"
  log "$ID: $status${TIMES[*]:+ (${TIMES[*]})}"
  [[ "$status" == built ]] && log "  $(wc -l <"$OUT/top.fasm") FASM lines, $OUT"
  return 0
}

# --- main ------------------------------------------------------------------
BUILD_CHIPDB=0
IDS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --list)
      printf '%-72s %-18s %-18s %-10s %s\n' ID PART CHIPDB KIND AVAILABLE
      for e in "${ENTRIES[@]}"; do
        IFS='|' read -r id part device family kind path arg <<<"$e"
        if [[ "$kind" == skip ]]; then
          avail="skip: $arg"
        elif [[ -n "$(chipdb_path "$device")" ]]; then
          avail="chipdb $(chipdb_path "$device")"
        else
          avail="no chipdb ($device; --build-chipdb)"
        fi
        printf '%-72s %-18s %-18s %-10s %s\n' "$id" "$part" "$device" "$kind" "$avail"
      done
      exit 0 ;;
    --config)
      [[ $# -ge 2 ]] || { echo "usage: $0 --config ID" >&2; exit 2; }
      entry "$2"; exit $? ;;
    --fetch) fetch; exit 0 ;;
    --build-chipdb) BUILD_CHIPDB=1; shift ;;
    --all) for e in "${ENTRIES[@]}"; do IDS+=("${e%%|*}"); done; shift ;;
    -*) die "unknown option $1 (see --help)" ;;
    *) IDS+=("$1"); shift ;;
  esac
done
[[ ${#IDS[@]} -gt 0 ]] || { usage; exit 2; }
for id in "${IDS[@]}"; do
  entry "$id" >/dev/null || die "unknown id $id (see --list)"
done

[[ -f "$E2E_BUILD/openxc7/status.json" ]] || die "openXC7 toolchain not set up in $E2E_BUILD (tools/e2e/setup-openxc7.sh, or OPENXC7_E2E_BUILD)"
export OPENXC7_E2E_BUILD="$E2E_BUILD"
# shellcheck source=/dev/null
source "$SCRIPT_DIR/openxc7-env.sh"

for id in "${IDS[@]}"; do
  run_one "$id"
done

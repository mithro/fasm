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
# Builds the Xilinx 7 series examples of f4pga-examples with the f4pga
# (Yosys + VPR) flow (T7.3), exactly as their READMEs document (the
# commands of f4pga-examples' .github/scripts/build-examples.sh), and
# collects for each design/board:
#
#   top.fasm        the flow's FASM (genfasm + the extra FASM)
#   top.bit         the flow's bitstream (its own xcfasm --sparse
#                   --emit_pudc_b_pullup + xc7frames2bit)
#   top.frm         the flow's frames: the same xcfasm command line rerun
#                   with --frm_out (xcfasm writes them to a temporary file
#                   it does not keep otherwise)
#   top.rerun.bit   the bitstream of that rerun (identical to top.bit up
#                   to the header's date and .frm path)
#   build.log       the build output
#   info.json       part, device, family, build time, sizes, sha256s
#   difftest.json   part and family, for tools/difftest-xilinx.py
#                   --corpus-root
#
# into $OUT/<design>/<board>/ (default tools/e2e/build/out/f4pga-examples).
#
# Requires tools/e2e/setup-f4pga.sh with the architecture definition
# package ("device") of the design's board installed, and an
# f4pga-examples checkout at F4PGA_EXAMPLES_COMMIT with its submodules
# ($F4PGA_EXAMPLES_DIR, default tools/e2e/build/f4pga-examples; cloned
# when missing). The litex_demo designs also need the LiteX packages of
# xc7/litex_demo/requirements.txt, which the documented flow installs
# into the conda environment (`pip install -r requirements.txt` in
# xc7/litex_demo, cloning them into xc7/litex_demo/src); this script
# installs the ones these designs use on first use (litex_setup below).
#
# Usage:
#   tools/e2e/run-f4pga-examples.sh --list
#   tools/e2e/run-f4pga-examples.sh [--keep-build] DESIGN BOARD [DESIGN BOARD...]
#   tools/e2e/run-f4pga-examples.sh [--keep-build] --device DEVICE   # every design of a device
#   tools/e2e/run-f4pga-examples.sh [--keep-build] --all             # every design whose device is installed

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
F4PGA_EXAMPLES_COMMIT=13f11197b33dae1cde3bf146f317d63f0134eacf
EXAMPLES="${F4PGA_EXAMPLES_DIR:-$REPO_ROOT/tools/e2e/build/f4pga-examples}"
OUT="${F4PGA_EXAMPLES_OUT:-$REPO_ROOT/tools/e2e/build/out/f4pga-examples}"
BUILD_TIMEOUT="${F4PGA_BUILD_TIMEOUT:-3600}"

# design board device part family kind dir [args]
# kind make: TARGET=<board> make -C xc7/<dir> (build dir <dir>/build/<board>)
# kind litex: xc7/litex_demo/src/litex/litex/boards/targets/arty.py with
#   --cpu-type <arg> (build dir litex_demo/build/<arg>/<board>/gateware)
DESIGNS="
counter_test arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make counter_test
counter_test arty_100 xc7a100t_test xc7a100tcsg324-1 artix7 make counter_test
counter_test nexys4ddr xc7a100t_test xc7a100tcsg324-1 artix7 make counter_test
counter_test basys3 xc7a50t_test xc7a35tcpg236-1 artix7 make counter_test
counter_test nexys_video xc7a200t_test xc7a200tsbg484-1 artix7 make counter_test
counter_test zybo xc7z010_test xc7z010clg400-1 zynq7 make counter_test
picosoc_demo arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make picosoc_demo
picosoc_demo arty_100 xc7a100t_test xc7a100tcsg324-1 artix7 make picosoc_demo
picosoc_demo nexys4ddr xc7a100t_test xc7a100tcsg324-1 artix7 make picosoc_demo
picosoc_demo basys3 xc7a50t_test xc7a35tcpg236-1 artix7 make picosoc_demo
litex_demo_picorv32 arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 litex picorv32
litex_demo_picorv32 arty_100 xc7a100t_test xc7a100tcsg324-1 artix7 litex picorv32
litex_demo_vexriscv arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 litex vexriscv
litex_demo_vexriscv arty_100 xc7a100t_test xc7a100tcsg324-1 artix7 litex vexriscv
linux_litex_demo arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make linux_litex_demo
linux_litex_demo arty_100 xc7a100t_test xc7a100tcsg324-1 artix7 make linux_litex_demo
litex_sata_demo nexys_video xc7a200t_test xc7a200tsbg484-1 artix7 make litex_sata_demo
timer basys3 xc7a50t_test xc7a35tcpg236-1 artix7 make timer
pulse_width_led arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make pulse_width_led
button_controller basys3 xc7a50t_test xc7a35tcpg236-1 artix7 make additional_examples/button_controller
hello_a arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make ../projf-makefiles/hello/hello-arty/A
hello_b arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make ../projf-makefiles/hello/hello-arty/B
hello_c arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make ../projf-makefiles/hello/hello-arty/C
hello_d arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make ../projf-makefiles/hello/hello-arty/D
hello_e arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make ../projf-makefiles/hello/hello-arty/E
hello_f arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make ../projf-makefiles/hello/hello-arty/F
hello_g arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make ../projf-makefiles/hello/hello-arty/G
hello_h arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make ../projf-makefiles/hello/hello-arty/H
hello_i arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make ../projf-makefiles/hello/hello-arty/I
hello_j arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make ../projf-makefiles/hello/hello-arty/J
hello_k arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make ../projf-makefiles/hello/hello-arty/K
hello_l arty_35 xc7a50t_test xc7a35tcsg324-1 artix7 make ../projf-makefiles/hello/hello-arty/L
"

log() { echo "[run-f4pga-examples] $*" >&2; }

list() {
  echo "$DESIGNS" | awk 'NF { printf "%-22s %-12s %-14s %-18s %s\n", $1, $2, $3, $4, $5 }'
}

KEEP=0
SELECT=()
case "${1:-}" in
  --list) list; exit 0 ;;
esac
while [[ $# -gt 0 ]]; do
  case "$1" in
    --keep-build) KEEP=1; shift ;;
    --all) mapfile -t -O "${#SELECT[@]}" SELECT < <(echo "$DESIGNS" | awk 'NF { print $1, $2 }'); shift ;;
    --device) mapfile -t -O "${#SELECT[@]}" SELECT < <(echo "$DESIGNS" | awk -v d="$2" 'NF && $3 == d { print $1, $2 }'); shift 2 ;;
    -*) echo "unknown option $1" >&2; exit 2 ;;
    *) SELECT+=("$1 $2"); shift 2 ;;
  esac
done
if [[ ${#SELECT[@]} -eq 0 ]]; then
  sed -n '19,52p' "$0"
  exit 2
fi

# shellcheck source=tools/e2e/f4pga-env.sh
source "$REPO_ROOT/tools/e2e/f4pga-env.sh"
if [[ ! -x "$F4PGA_ENV/bin/vpr" ]]; then
  log "the f4pga toolchain is not installed (tools/e2e/setup-f4pga.sh)"
  exit 3
fi
ARCH="$F4PGA_INSTALL_DIR/xc7/share/f4pga/arch"

if [[ ! -d "$EXAMPLES/.git" ]]; then
  log "cloning f4pga-examples into $EXAMPLES"
  mkdir -p "$(dirname "$EXAMPLES")"
  timeout 900 git clone -q https://github.com/chipsalliance/f4pga-examples.git "$EXAMPLES"
fi
if [[ "$(git -C "$EXAMPLES" rev-parse HEAD)" != "$F4PGA_EXAMPLES_COMMIT" ]]; then
  git -C "$EXAMPLES" checkout -q "$F4PGA_EXAMPLES_COMMIT"
fi
timeout 900 git -C "$EXAMPLES" submodule update --init -q

# The LiteX packages of xc7/litex_demo/requirements.txt that the Arty
# picorv32/vexriscv designs use, installed like its `pip install -e
# git+<url>@<commit>#egg=<egg>` lines (into xc7/litex_demo/src/<egg>, in
# the conda environment) but as shallow clones and without the
# pythondata-cpu packages of the other CPUs (blackparrot, rocket,
# microwatt, ...: GiBs of git history that do not fit this machine's
# disk budget; nothing these designs run imports them) or nmigen.
LITEX_EGGS="migen litex litedram liteeth liteiclink litejesd204b litepcie
litesata litescope litesdcard litevideo litehyperbus litespi litex_boards
pythondata_cpu_picorv32 pythondata_cpu_vexriscv pythondata_misc_tapcfg
pythondata_software_compiler_rt"

litex_setup() {
  local dir="$EXAMPLES/xc7/litex_demo"
  if [[ -f "$dir/.litex-installed" ]]; then return 0; fi
  log "installing the LiteX packages of xc7/litex_demo/requirements.txt"
  local egg line url sha d
  mkdir -p "$dir/src"
  for egg in $LITEX_EGGS; do
    line=$(grep "#egg=$egg\$" "$dir/requirements.txt")
    url=${line#-e git+}
    url=${url%%@*}
    sha=${line##*@}
    sha=${sha%%#*}
    d="$dir/src/$(echo "$egg" | tr 'A-Z' 'a-z')"
    if [[ ! -d "$d/.git" ]]; then
      rm -rf "$d"
      git init -q "$d"
      git -C "$d" remote add origin "$url"
      timeout 900 git -C "$d" fetch -q --depth 1 origin "$sha"
      git -C "$d" checkout -q FETCH_HEAD
    fi
    python3 -m pip install -q --no-deps --no-build-isolation -e "$d"
  done
  touch "$dir/.litex-installed"
}

# The flow's xcfasm command line, from build.log (f4pga build prints it)
# or from symbiflow_write_bitstream's arguments (the make flows).
reference_frames() {
  local bdir="$1" out="$2" part="$3" family="$4"
  local db="$F4PGA_PRJXRAY_DB/$family"
  xcfasm --db-root "$db" --part "$part" --part_file "$db/$part/part.yaml" \
    --sparse --emit_pudc_b_pullup --fn_in "$out/top.fasm" \
    --frm_out "$out/top.frm" --bit_out "$out/top.rerun.bit" \
    --frm2bit xc7frames2bit > "$out/xcfasm-rerun.log" 2>&1
}

build_one() {
  local design="$1" board="$2"
  local line
  line=$(echo "$DESIGNS" | awk -v d="$design" -v b="$board" '$1 == d && $2 == b')
  if [[ -z "$line" ]]; then
    log "unknown design/board $design $board (--list)"
    return 2
  fi
  read -r _ _ device part family kind dir <<<"$line"
  local out="$OUT/$design/$board"
  rm -rf "$out"
  mkdir -p "$out"
  if [[ ! -d "$ARCH/$device" ]] || find "$ARCH/$device" -xtype l | grep -q .; then
    log "$design/$board: device $device not installed (tools/e2e/setup-f4pga.sh --devices $device)"
    echo "{\"design\": \"$design\", \"board\": \"$board\", \"device\": \"$device\", \"part\": \"$part\", \"family\": \"$family\", \"status\": \"device not installed\"}" > "$out/info.json"
    return 1
  fi
  local bdir cmd
  case "$kind" in
    make)
      bdir="$EXAMPLES/xc7/$dir/build/$board"
      rm -rf "$EXAMPLES/xc7/$dir/build"
      cmd=(env TARGET="$board" make -C "$EXAMPLES/xc7/$dir")
      ;;
    litex)
      litex_setup
      bdir="$EXAMPLES/xc7/litex_demo/build/$dir/$board/gateware"
      rm -rf "$EXAMPLES/xc7/litex_demo/build/$dir/$board"
      local variant=a7-35
      [[ $board == arty_100 ]] && variant=a7-100
      cmd=(bash -c "cd '$EXAMPLES/xc7/litex_demo' && ./src/litex/litex/boards/targets/arty.py --toolchain=symbiflow --cpu-type=$dir --sys-clk-freq 80e6 --output-dir build/$dir/$board --variant $variant --build")
      ;;
  esac
  log "$design/$board: building ($device, $part)"
  local start end status=built
  start=$(date +%s.%N)
  if ! (cd "$EXAMPLES/xc7" && timeout "$BUILD_TIMEOUT" "${cmd[@]}") > "$out/build.log" 2>&1; then
    status=failed
  fi
  end=$(date +%s.%N)
  local fasm bit
  fasm=$(ls "$bdir"/*.fasm 2>/dev/null | grep -v '_fasm_extra\.fasm$' | head -1 || true)
  bit=$(ls "$bdir"/*.bit 2>/dev/null | head -1 || true)
  if [[ $status == built && ( -z "$fasm" || -z "$bit" ) ]]; then
    status="failed (no FASM/bitstream)"
  fi
  # symbiflow_write_fasm ignores genfasm's exit status: a genfasm killed
  # (e.g. by the OOM killer) leaves a truncated FASM, and the flow goes on
  # to write a bitstream of it and "succeeds".
  if [[ $status == built ]] && grep -qE '(Killed|Segmentation fault|Aborted).*(genfasm|vpr)' "$out/build.log"; then
    status="failed ($(grep -oE '(Killed|Segmentation fault|Aborted).*(genfasm|vpr)' "$out/build.log" | head -1 | sed -E 's/ +/ /g; s/^(Killed|Segmentation fault|Aborted).*(genfasm|vpr)$/\2 \1/'))"
  fi
  if [[ -n "$fasm" ]]; then
    cp "$fasm" "$out/top.fasm"
  fi
  if [[ -n "$bit" ]]; then
    cp "$bit" "$out/top.bit"
  fi
  if [[ $status == built ]]; then
    reference_frames "$bdir" "$out" "$part" "$family" || status="failed (xcfasm rerun)"
  fi
  if [[ $KEEP == 0 ]]; then
    case "$kind" in
      make) rm -rf "$EXAMPLES/xc7/$dir/build" ;;
      litex) rm -rf "$EXAMPLES/xc7/litex_demo/build/$dir/$board" ;;
    esac
  fi
  python3 - "$out" "$design" "$board" "$device" "$part" "$family" "$status" "$start" "$end" <<'EOF'
import hashlib, json, os, sys
out, design, board, device, part, family, status, start, end = sys.argv[1:]
info = {'design': design, 'board': board, 'device': device, 'part': part,
        'family': family, 'status': status,
        'build_seconds': round(float(end) - float(start), 1)}
for f in ('top.fasm', 'top.bit', 'top.frm', 'top.rerun.bit'):
    p = os.path.join(out, f)
    if os.path.exists(p):
        data = open(p, 'rb').read()
        info[f] = {'bytes': len(data),
                   'sha256': hashlib.sha256(data).hexdigest()}
        if f == 'top.fasm':
            info[f]['lines'] = data.count(b'\n')
json.dump(info, open(os.path.join(out, 'info.json'), 'w'), indent=2,
          sort_keys=True)
# For tools/difftest-xilinx.py --corpus-root.
json.dump({'part': part, 'family': family},
          open(os.path.join(out, 'difftest.json'), 'w'), sort_keys=True)
print('%s/%s: %s in %.0fs' % (design, board, status, info['build_seconds']))
EOF
  [[ $status == built ]]
}

rc=0
for sel in "${SELECT[@]}"; do
  read -r design board <<<"$sel"
  build_one "$design" "$board" || rc=1
done
exit $rc

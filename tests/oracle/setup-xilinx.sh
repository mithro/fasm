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
# Sets up the "Xilinx oracle": the *reference* (pre-Rust-rewrite) Xilinx
# tools -- prjxray (C++ bitstream tools + Python package), f4pga-xc-fasm
# (FASM<->frames), and prjuray/prjuray-tools (UltraScale/UltraScale+,
# best effort) -- pinned to immutable commits, used by the Rust rewrite's
# Xilinx differential tests (T5.9, T6.3) as golden references. See
# tests/oracle/README.md ("Xilinx reference tools") for details, and
# docs/rewrite/DESIGN-xilinx-db.md sections 1, 2 and 8.4 for how these
# tools' CLIs and the database layout work.
#
# This is deliberately a SEPARATE venv (tests/oracle/venv-xilinx) from the
# plain FASM oracle's tests/oracle/venv: venv stays a pristine, minimal
# `fasm` install (see tests/oracle/setup.sh); venv-xilinx additionally
# carries prjxray, xc-fasm and prjuray-tools and their (non-fasm) Python
# dependencies. Both venvs install the *same* pinned `fasm` package from
# the *same* pristine git worktree (tests/oracle/build/pristine-src,
# created by tests/oracle/setup.sh, reused here if present) so both oracles
# agree on what "the original fasm" means.
#
# Usage:
#   tests/oracle/setup-xilinx.sh [--force]
#
#   --force   Delete tests/oracle/venv-xilinx and tests/oracle/build/xilinx
#             and rebuild from scratch (same pins). Does NOT touch
#             tests/oracle/venv or tests/oracle/build/pristine-src -- those
#             belong to tests/oracle/setup.sh.
#             Without --force the script is a fast no-op once the Xilinx
#             oracle has been set up successfully for the requested pins
#             (a pin change is detected automatically, same as setup.sh).
#
# Everything this script creates lives under the gitignored
# tests/oracle/build/xilinx/ and tests/oracle/venv-xilinx/ -- nothing here
# is committed to git.
set -euo pipefail

FORCE=0
for arg in "$@"; do
  case "$arg" in
    --force)
      FORCE=1
      ;;
    -h | --help)
      sed -n '2,52p' "$0" | sed -e 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "setup-xilinx.sh: unknown argument: $arg" >&2
      echo "usage: $0 [--force]" >&2
      exit 2
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORACLE_DIR="$SCRIPT_DIR"
VENV_DIR="$ORACLE_DIR/venv-xilinx"
BUILD_DIR="$ORACLE_DIR/build/xilinx"
SRC_DIR="$BUILD_DIR/src"
BIN_DIR="$BUILD_DIR/bin"
LOG_DIR="$BUILD_DIR/logs"
STATUS_FILE="$BUILD_DIR/status.json"
MARKER="$VENV_DIR/.oracle-xilinx-setup-ok"
PRISTINE_SRC="$ORACLE_DIR/build/pristine-src"

# --- Pinned commits ----------------------------------------------------
#
# Resolved by cloning each repo fresh and reading its default-branch HEAD
# during the development of this script (2026-09-24); recorded here so the
# oracle never silently moves. Override any of these to intentionally
# re-pin (a change is auto-detected below, same scheme as
# tests/oracle/setup.sh's ORACLE_COMMIT).
PRJXRAY_COMMIT="${PRJXRAY_COMMIT:-c9f02d8576042325425824647ab5555b1bc77833}"
F4PGA_XC_FASM_COMMIT="${F4PGA_XC_FASM_COMMIT:-25dc605c9c0896204f0c3425b52a332034cf5e5c}"
PRJURAY_COMMIT="${PRJURAY_COMMIT:-c550b03a26b4c4a9c4453353bd642a21f710b3ec}"
PRJURAY_TOOLS_COMMIT="${PRJURAY_TOOLS_COMMIT:-f53f07b8fe37721137a57e9bee3b2b13e7676f53}"

PRJXRAY_URL="https://github.com/f4pga/prjxray.git"
F4PGA_XC_FASM_URL="https://github.com/chipsalliance/f4pga-xc-fasm.git"
PRJURAY_URL="https://github.com/f4pga/prjuray.git"
PRJURAY_TOOLS_URL="https://github.com/SymbiFlow/prjuray-tools.git"

# Submodules needed by prjxray's / prjuray-tools' CMake build (see their
# CMakeLists.txt: googletest, gflags, cctz, abseil-cpp, yaml-cpp are
# add_subdirectory()'d; sanitizers-cmake only supplies a find_package()
# CMake module). NOT fetched: third_party/fasm, third_party/python-sdf-timing,
# third_party/yosys, third_party/display_port, third_party/edalize,
# third_party/embeddedsw -- unrelated to the C++ tools and, in yosys's
# case, huge.
CPP_SUBMODULES="third_party/abseil-cpp third_party/cctz third_party/gflags third_party/googletest third_party/yaml-cpp third_party/sanitizers-cmake"

log() {
  echo "[xilinx oracle setup] $*"
}

# ---------------------------------------------------------------------

if [[ "$FORCE" -eq 0 && -f "$MARKER" && -f "$STATUS_FILE" ]]; then
  RECORDED="$(python3 -c '
import json, sys
try:
    s = json.load(open(sys.argv[1]))
    print(s.get("prjxray_commit", ""), s.get("f4pga_xc_fasm_commit", ""),
          s.get("prjuray_commit", ""), s.get("prjuray_tools_commit", ""))
except Exception:
    print("")
' "$STATUS_FILE" 2>/dev/null || true)"
  WANTED="$PRJXRAY_COMMIT $F4PGA_XC_FASM_COMMIT $PRJURAY_COMMIT $PRJURAY_TOOLS_COMMIT"
  if [[ -n "$RECORDED" && "$RECORDED" != "$WANTED" ]]; then
    log "a pinned commit changed; forcing a rebuild"
    FORCE=1
  fi
fi

if [[ "$FORCE" -eq 1 ]]; then
  log "--force (or a pin change): removing $VENV_DIR and $BUILD_DIR"
  rm -rf "$VENV_DIR" "$BUILD_DIR"
fi

mkdir -p "$SRC_DIR" "$BIN_DIR" "$LOG_DIR"

if [[ -f "$MARKER" ]]; then
  log "Xilinx oracle already set up at $VENV_DIR (pass --force to rebuild)"
  [[ -f "$STATUS_FILE" ]] && cat "$STATUS_FILE"
  exit 0
fi

SETUP_START=$(date +%s)

# --- 0. Make sure the plain FASM oracle (and its pristine worktree) exists.
#
# Reuses tests/oracle/setup.sh unmodified: it is idempotent, so calling it
# here is a fast no-op if it already ran, and creates tests/oracle/venv +
# tests/oracle/build/pristine-src (pinned to ORACLE_COMMIT) if it has not.
log "ensuring the base FASM oracle (tests/oracle/setup.sh) is set up"
"$ORACLE_DIR/setup.sh" > "$LOG_DIR/base-oracle-setup.log" 2>&1 \
  || { log "ERROR: tests/oracle/setup.sh failed; see $LOG_DIR/base-oracle-setup.log" >&2; tail -n 40 "$LOG_DIR/base-oracle-setup.log" >&2; exit 1; }
if [[ ! -d "$PRISTINE_SRC" ]]; then
  log "ERROR: $PRISTINE_SRC missing after tests/oracle/setup.sh" >&2
  exit 1
fi
log "pristine oracle source: $PRISTINE_SRC"

# --- 1. Best effort: system packages the C++ builds might need. ---
#
# On the container this was developed in, cmake/ninja-build/g++/uuid-dev/
# pkg-config were already present and nothing extra was required; this
# step is kept for portability to a machine that is missing them. Never
# fatal: if apt-get is unavailable or fails, the cmake configure step
# below will fail with a clear message instead.
if command -v apt-get >/dev/null 2>&1; then
  log "attempting to install C++ build prerequisites (best effort, never fatal)"
  APT_CMD=(apt-get)
  if [[ "$(id -u)" -ne 0 ]] && command -v sudo >/dev/null 2>&1; then
    APT_CMD=(sudo -n apt-get)
  fi
  if ! { "${APT_CMD[@]}" update -qq && \
         "${APT_CMD[@]}" install -y -qq \
           cmake ninja-build build-essential uuid-dev pkg-config; } \
      > "$LOG_DIR/apt.log" 2>&1; then
    log "WARNING: apt-get install failed or unavailable" \
      "(see $LOG_DIR/apt.log); continuing, the C++ builds may fail"
  fi
else
  log "no apt-get available; skipping system package install"
fi

# --- 2. Clone the reference repositories, pinned. ---
#
# Idempotent: re-run is a no-op per repo once its src dir HEAD already
# matches the pinned commit.
clone_pinned() {
  local name="$1" url="$2" commit="$3" dir="$4" recurse="$5"
  if [[ -d "$dir/.git" ]] && \
      [[ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" == "$commit" ]]; then
    log "$name already checked out at $commit"
    return 0
  fi
  rm -rf "$dir"
  log "cloning $name ($url) pinned to $commit"
  git clone --quiet "$url" "$dir" > "$LOG_DIR/clone-$name.log" 2>&1
  git -C "$dir" checkout --quiet "$commit" >> "$LOG_DIR/clone-$name.log" 2>&1
  if [[ "$recurse" == "cpp" ]]; then
    log "fetching $name's C++ build submodules ($CPP_SUBMODULES)"
    # shellcheck disable=SC2086
    git -C "$dir" submodule update --init --depth 1 -- $CPP_SUBMODULES \
      >> "$LOG_DIR/clone-$name.log" 2>&1
  fi
}

clone_pinned prjxray "$PRJXRAY_URL" "$PRJXRAY_COMMIT" "$SRC_DIR/prjxray" cpp
clone_pinned f4pga-xc-fasm "$F4PGA_XC_FASM_URL" "$F4PGA_XC_FASM_COMMIT" \
  "$SRC_DIR/f4pga-xc-fasm" no
clone_pinned prjuray "$PRJURAY_URL" "$PRJURAY_COMMIT" "$SRC_DIR/prjuray" no
clone_pinned prjuray-tools "$PRJURAY_TOOLS_URL" "$PRJURAY_TOOLS_COMMIT" \
  "$SRC_DIR/prjuray-tools" cpp

PRJXRAY_RESOLVED="$(git -C "$SRC_DIR/prjxray" rev-parse HEAD)"
F4PGA_XC_FASM_RESOLVED="$(git -C "$SRC_DIR/f4pga-xc-fasm" rev-parse HEAD)"
PRJURAY_RESOLVED="$(git -C "$SRC_DIR/prjuray" rev-parse HEAD)"
PRJURAY_TOOLS_RESOLVED="$(git -C "$SRC_DIR/prjuray-tools" rev-parse HEAD)"

# --- 3. Build the prjxray C++ tools (Series7). ---
log "configuring prjxray C++ build (cmake, Release, ninja)"
mkdir -p "$SRC_DIR/prjxray/build"
(cd "$SRC_DIR/prjxray/build" && cmake -GNinja -DCMAKE_BUILD_TYPE=Release ..) \
  > "$LOG_DIR/prjxray-cmake.log" 2>&1

PRJXRAY_TARGETS="xc7frames2bit bitread frame_address_decoder gen_part_base_yaml bittool xc7patch"
log "building prjxray C++ tools: $PRJXRAY_TARGETS"
# shellcheck disable=SC2086
(cd "$SRC_DIR/prjxray/build" && ninja $PRJXRAY_TARGETS) \
  > "$LOG_DIR/prjxray-ninja.log" 2>&1
for t in $PRJXRAY_TARGETS; do
  cp "$SRC_DIR/prjxray/build/tools/$t" "$BIN_DIR/$t"
done
PRJXRAY_CPP_BUILT=1
log "prjxray C++ tools built and copied to $BIN_DIR"

# --- 4. Build the prjuray-tools C++ tools (UltraScale/UltraScale+), best
#        effort: at most ~20 minutes; a failure here is recorded, not
#        fatal to the rest of the script. ---
PRJURAY_CPP_BUILT=0
PRJURAY_CPP_ERROR=""
URAY_TARGETS="xcframes2bit bitread xc7_frame_address_decoder xcu_frame_address_decoder gen_part_base_yaml bittool"
log "configuring prjuray-tools C++ build (cmake, Release, ninja; best effort)"
set +e
timeout 1200 bash -c "
  set -e
  mkdir -p '$SRC_DIR/prjuray-tools/build'
  cd '$SRC_DIR/prjuray-tools/build'
  cmake -GNinja -DCMAKE_BUILD_TYPE=Release .. > '$LOG_DIR/prjuray-cmake.log' 2>&1
  ninja $URAY_TARGETS > '$LOG_DIR/prjuray-ninja.log' 2>&1
"
URAY_BUILD_STATUS=$?
set -e
if [[ "$URAY_BUILD_STATUS" -eq 0 ]]; then
  for t in $URAY_TARGETS; do
    cp "$SRC_DIR/prjuray-tools/build/tools/$t" "$BIN_DIR/uray-$t"
  done
  PRJURAY_CPP_BUILT=1
  log "prjuray-tools C++ tools built and copied to $BIN_DIR (uray-* prefix)"
else
  PRJURAY_CPP_ERROR="cmake/ninja build failed or timed out (status $URAY_BUILD_STATUS); see $LOG_DIR/prjuray-cmake.log and $LOG_DIR/prjuray-ninja.log"
  log "WARNING: $PRJURAY_CPP_ERROR"
  log "continuing without the prjuray-tools C++ tools (best effort only, per T5.8)"
fi

# --- 5. Python venv: pinned fasm + prjxray + xc-fasm + prjuray + pytest. ---
log "creating virtualenv at $VENV_DIR"
python3 -m venv "$VENV_DIR"
PY="$VENV_DIR/bin/python"
"$PY" -m pip install --quiet --upgrade pip setuptools wheel

# 5a. The SAME pinned, immutable fasm package as tests/oracle/venv, from
# the same pristine worktree, installed the same way (non-editable, with
# an editable-of-pristine-worktree fallback) -- see tests/oracle/setup.sh
# and README.md for why. This must come BEFORE prjxray/xc-fasm below so
# their (deliberately --no-deps'd) installs never get a chance to pull a
# different `fasm` from PyPI. Cython is required by pristine-src's
# setup.py (the ANTLR extension build attempt) even when that build ends
# up falling back to textX-only -- same as tests/oracle/setup.sh's base
# requirements step.
log "installing base requirements (textX, Cython) needed to install the pinned fasm"
"$PY" -m pip install --quiet 'textx' 'Cython' \
  > "$LOG_DIR/pip-fasm.log" 2>&1
log "installing the pinned fasm package into venv-xilinx (non-editable)"
set +e
"$PY" -m pip install --no-build-isolation "$PRISTINE_SRC" \
  >> "$LOG_DIR/pip-fasm.log" 2>&1
FASM_INSTALL_STATUS=$?
set -e
FASM_INSTALL_MODE="non-editable"
if [[ "$FASM_INSTALL_STATUS" -ne 0 ]]; then
  log "WARNING: non-editable fasm install failed; retrying editable" \
    "(see $LOG_DIR/pip-fasm.log)"
  "$PY" -m pip install --no-build-isolation -e "$PRISTINE_SRC" \
    >> "$LOG_DIR/pip-fasm.log" 2>&1
  FASM_INSTALL_MODE="editable-pristine-worktree"
fi

# 5b. prjxray's Python package. Its setup.py declares `install_requires=
# ['fasm', ...]`, which would silently replace the pinned fasm above with
# a PyPI wheel -- installed --no-deps, then its other (non-fasm)
# dependencies explicitly.
log "installing prjxray Python package (--no-deps, then its other deps)"
"$PY" -m pip install --quiet --no-deps "$SRC_DIR/prjxray" \
  > "$LOG_DIR/pip-prjxray.log" 2>&1
"$PY" -m pip install --quiet \
  'intervaltree' 'numpy' 'pyjson5' 'pyyaml' 'simplejson' \
  >> "$LOG_DIR/pip-prjxray.log" 2>&1

# 5c. f4pga-xc-fasm. Same story: install_requires includes both `prjxray`
# and `fasm` from PyPI.
log "installing f4pga-xc-fasm (--no-deps, then its other deps)"
"$PY" -m pip install --quiet --no-deps "$SRC_DIR/f4pga-xc-fasm" \
  > "$LOG_DIR/pip-xc-fasm.log" 2>&1
"$PY" -m pip install --quiet 'intervaltree' 'simplejson' 'textx' \
  >> "$LOG_DIR/pip-xc-fasm.log" 2>&1

# 5d. prjuray-tools' Python package (`prjuray.db`, `prjuray.grid`,
# `prjuray.tile_segbits`, ...). Its install_requires is just
# ['intervaltree', 'simplejson'] -- no fasm/prjxray pin to defend against.
log "installing prjuray-tools Python package"
"$PY" -m pip install --quiet "$SRC_DIR/prjuray-tools" \
  > "$LOG_DIR/pip-prjuray-tools.log" 2>&1

# Note: the f4pga/prjuray repo itself (utils/fasm2frames.py, utils/bit2fasm.py,
# ...) has no setup.py -- it is not pip-installable. It is cloned above for
# Phase 6 (T6.x) use via a PYTHONPATH wrapper (its `utils` package needs
# the prjuray repo root on PYTHONPATH, alongside prjuray-tools for
# `prjuray.db`); T5.8 itself only needs the prjuray-tools Python package
# (prjxray-shaped API) and the Series7 tools above.

log "installing pytest"
"$PY" -m pip install --quiet pytest >> "$LOG_DIR/pip-prjuray-tools.log" 2>&1

# --- 6. Verify: fasm/prjxray/xc_fasm import from venv-xilinx, not the live repo. ---
log "verifying imports resolve inside venv-xilinx, not the live repository"
IMPORT_CHECK="$(cd "$LOG_DIR" && "$PY" -c '
import json
import fasm, prjxray, xc_fasm, prjuray
print(json.dumps({
    "fasm_file": fasm.__file__,
    "prjxray_file": prjxray.__file__,
    "xc_fasm_file": xc_fasm.__file__,
    "prjuray_file": prjuray.__file__,
}))
')"
LIVE_FASM_DIR="$REPO_ROOT/fasm"
FASM_FILE="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["fasm_file"])' "$IMPORT_CHECK")"
case "$FASM_FILE" in
  "$LIVE_FASM_DIR"/*)
    log "ERROR: venv-xilinx imported fasm from the live repository ($FASM_FILE)" >&2
    exit 1
    ;;
esac
log "imports OK: $IMPORT_CHECK"

SETUP_END=$(date +%s)

# --- 7. Record status.json. ---
STATUS_PY="
import json, os
status = {
    'prjxray_commit': os.environ['PRJXRAY_COMMIT'],
    'prjxray_commit_resolved': os.environ['PRJXRAY_RESOLVED'],
    'f4pga_xc_fasm_commit': os.environ['F4PGA_XC_FASM_COMMIT'],
    'f4pga_xc_fasm_commit_resolved': os.environ['F4PGA_XC_FASM_RESOLVED'],
    'prjuray_commit': os.environ['PRJURAY_COMMIT'],
    'prjuray_commit_resolved': os.environ['PRJURAY_RESOLVED'],
    'prjuray_tools_commit': os.environ['PRJURAY_TOOLS_COMMIT'],
    'prjuray_tools_commit_resolved': os.environ['PRJURAY_TOOLS_RESOLVED'],
    'fasm_install_mode': os.environ['FASM_INSTALL_MODE'],
    'prjxray_cpp_built': True,
    'prjxray_cpp_targets': os.environ['PRJXRAY_TARGETS'].split(),
    'prjuray_cpp_built': os.environ['PRJURAY_CPP_BUILT'] == '1',
    'prjuray_cpp_targets': os.environ['URAY_TARGETS'].split() if os.environ['PRJURAY_CPP_BUILT'] == '1' else [],
    'prjuray_cpp_error': os.environ.get('PRJURAY_CPP_ERROR', ''),
    'setup_seconds': int(os.environ['SETUP_END']) - int(os.environ['SETUP_START']),
    'bin_dir': os.environ['BIN_DIR'],
    'venv_dir': os.environ['VENV_DIR'],
}
with open(os.environ['STATUS_FILE'], 'w') as f:
    json.dump(status, f, indent=2, sort_keys=True)
    f.write('\n')
"
PRJXRAY_COMMIT="$PRJXRAY_COMMIT" PRJXRAY_RESOLVED="$PRJXRAY_RESOLVED" \
F4PGA_XC_FASM_COMMIT="$F4PGA_XC_FASM_COMMIT" F4PGA_XC_FASM_RESOLVED="$F4PGA_XC_FASM_RESOLVED" \
PRJURAY_COMMIT="$PRJURAY_COMMIT" PRJURAY_RESOLVED="$PRJURAY_RESOLVED" \
PRJURAY_TOOLS_COMMIT="$PRJURAY_TOOLS_COMMIT" PRJURAY_TOOLS_RESOLVED="$PRJURAY_TOOLS_RESOLVED" \
FASM_INSTALL_MODE="$FASM_INSTALL_MODE" PRJXRAY_TARGETS="$PRJXRAY_TARGETS" \
PRJURAY_CPP_BUILT="$PRJURAY_CPP_BUILT" URAY_TARGETS="$URAY_TARGETS" \
PRJURAY_CPP_ERROR="$PRJURAY_CPP_ERROR" SETUP_START="$SETUP_START" SETUP_END="$SETUP_END" \
BIN_DIR="$BIN_DIR" VENV_DIR="$VENV_DIR" STATUS_FILE="$STATUS_FILE" \
"$PY" -c "$STATUS_PY"

touch "$MARKER"
log "Xilinx oracle setup complete in $((SETUP_END - SETUP_START))s: $VENV_DIR"
log "binaries: $BIN_DIR"
cat "$STATUS_FILE"

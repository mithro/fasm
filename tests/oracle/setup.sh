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
# Sets up the "oracle": a venv containing the *original* (pre-Rust-rewrite)
# Python `fasm` package, used by the Rust rewrite's differential tests as a
# golden reference. See tests/oracle/README.md for details.
#
# Usage:
#   tests/oracle/setup.sh [--force]
#
#   --force   Delete any existing venv/build state and rebuild from scratch.
#             Without --force the script is a fast no-op once the oracle has
#             been set up successfully.
set -euo pipefail

FORCE=0
for arg in "$@"; do
  case "$arg" in
    --force)
      FORCE=1
      ;;
    -h | --help)
      sed -n '2,26p' "$0" | sed -e 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "setup.sh: unknown argument: $arg" >&2
      echo "usage: $0 [--force]" >&2
      exit 2
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORACLE_DIR="$SCRIPT_DIR"
VENV_DIR="$ORACLE_DIR/venv"
BUILD_DIR="$ORACLE_DIR/build"
STATUS_FILE="$BUILD_DIR/status.json"
MARKER="$VENV_DIR/.oracle-setup-ok"

log() {
  echo "[oracle setup] $*"
}

if [[ "$FORCE" -eq 1 ]]; then
  log "--force given: removing existing venv and build state"
  rm -rf "$VENV_DIR" "$BUILD_DIR"
fi

mkdir -p "$BUILD_DIR"

# Idempotent fast path: if a previous run completed successfully, just print
# the recorded status and exit. Rebuilding the ANTLR C++ extension takes
# minutes, so we only do it once unless --force is given.
if [[ -f "$MARKER" ]]; then
  log "oracle venv already set up at $VENV_DIR (pass --force to rebuild)"
  if [[ -f "$STATUS_FILE" ]]; then
    cat "$STATUS_FILE"
  fi
  exit 0
fi

log "creating virtualenv at $VENV_DIR"
python3 -m venv "$VENV_DIR"
PY="$VENV_DIR/bin/python"

log "upgrading pip/setuptools/wheel"
"$PY" -m pip install --quiet --upgrade pip setuptools wheel

log "installing base requirements (textX, pytest, Cython) needed by both parsers"
"$PY" -m pip install --quiet 'textx' 'pytest' 'Cython'

# --- Best effort: get everything the ANTLR C++ parser extension needs. ---
#
# None of the steps in this section may fail the script: if they don't
# succeed, `pip install` below still succeeds with the textX-only parser
# (setup.py's AntlrCMakeBuild catches build failures itself and falls back).

log "fetching git submodules needed for the ANTLR C++ build (read only, from GitHub)"
if git -C "$REPO_ROOT" submodule update --init --depth 1 -- \
    third_party/antlr4 third_party/googletest \
    > "$BUILD_DIR/submodule.log" 2>&1; then
  log "submodules ready (third_party/antlr4, third_party/googletest)"
else
  log "WARNING: submodule update failed (see $BUILD_DIR/submodule.log);" \
    "ANTLR parser build will likely fall back to textX only"
fi

if command -v apt-get >/dev/null 2>&1; then
  log "attempting to install system packages needed by the ANTLR build" \
    "(uuid-dev, pkg-config; best effort, never fatal)"
  APT_CMD=(apt-get)
  if [[ "$(id -u)" -ne 0 ]] && command -v sudo >/dev/null 2>&1; then
    APT_CMD=(sudo -n apt-get)
  fi
  if ! { "${APT_CMD[@]}" update -qq && \
         "${APT_CMD[@]}" install -y -qq uuid-dev pkg-config; } \
      > "$BUILD_DIR/apt.log" 2>&1; then
    log "WARNING: apt-get install failed or unavailable" \
      "(see $BUILD_DIR/apt.log); continuing, ANTLR build may fail"
  fi
else
  log "no apt-get available; skipping system package install"
fi

# --- Install the original `fasm` package from the current source tree. ---
#
# setup.py's `AntlrCMakeBuild.run()` already implements the "attempt the
# ANTLR C++ build, fall back to textX-only on any failure" behaviour we
# want, so a single install call is all that is needed. `--no-build-isolation`
# makes it use the Cython/setuptools/wheel already installed in this venv
# instead of downloading its own isolated build environment.
log "installing the original fasm package (editable) with the ANTLR build attempted"
set +e
"$PY" -m pip install --no-build-isolation -e "$REPO_ROOT" \
  > "$BUILD_DIR/install.log" 2>&1
INSTALL_STATUS=$?
set -e

if [[ "$INSTALL_STATUS" -ne 0 ]]; then
  log "ERROR: pip install failed even for the textX-only fallback;" \
    "see $BUILD_DIR/install.log"
  tail -n 40 "$BUILD_DIR/install.log" >&2 || true
  exit 1
fi

log "installing pytest is already done above; verifying parser availability"
AVAILABLE_JSON="$("$PY" -c '
import json
import fasm.parser
print(json.dumps(sorted(fasm.parser.available)))
')"

ANTLR_OK=0
case "$AVAILABLE_JSON" in
  *antlr*) ANTLR_OK=1 ;;
esac

cat > "$STATUS_FILE" <<EOF
{
  "available_parsers": $AVAILABLE_JSON,
  "antlr_built": $( [[ "$ANTLR_OK" -eq 1 ]] && echo true || echo false )
}
EOF

if [[ "$ANTLR_OK" -eq 1 ]]; then
  log "ANTLR C++ parser built successfully; available parsers: $AVAILABLE_JSON"
else
  log "WARNING: ANTLR C++ parser was NOT built; falling back to textX only." \
    "See $BUILD_DIR/install.log for details. Available parsers: $AVAILABLE_JSON"
fi

touch "$MARKER"
log "oracle setup complete: $VENV_DIR"

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
# Python `fasm` package, pinned to an immutable commit, used by the Rust
# rewrite's differential tests as a golden reference. See
# tests/oracle/README.md for details.
#
# The oracle is deliberately NOT an editable/in-place install of this
# worktree's live `fasm/` directory: Phase 3 of the rewrite replaces that
# directory's contents (fasm/parser/rust.py, a maturin based setup.py, ...),
# and an editable install would silently turn the "golden reference" into
# the very code it is supposed to be diffed against. Instead this script
# checks out a pinned commit (ORACLE_COMMIT, default the last pre-rewrite
# commit) into its own `git worktree` under tests/oracle/build/pristine-src
# and installs *that* immutable tree into the venv.
#
# Usage:
#   tests/oracle/setup.sh [--force]
#   ORACLE_COMMIT=<commit-ish> tests/oracle/setup.sh [--force]
#
#   --force   Delete any existing venv/build state (including the pristine
#             worktree) and rebuild from scratch.
#             Without --force the script is a fast no-op once the oracle has
#             been set up successfully for the requested ORACLE_COMMIT (if
#             ORACLE_COMMIT changed since the last successful run, a
#             rebuild is triggered automatically, no --force needed).
set -euo pipefail

FORCE=0
for arg in "$@"; do
  case "$arg" in
    --force)
      FORCE=1
      ;;
    -h | --help)
      sed -n '2,39p' "$0" | sed -e 's/^# \{0,1\}//'
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
PRISTINE_SRC="$BUILD_DIR/pristine-src"

# The commit the oracle is pinned to: the last commit before the Rust
# rewrite started touching fasm/ or setup.py (an upstream chipsalliance/fasm
# master merge). Override with the ORACLE_COMMIT environment variable to
# intentionally move the pin, e.g. to re-pin after a deliberate upstream
# sync: `ORACLE_COMMIT=<new-commit> tests/oracle/setup.sh`. A pin change is
# detected automatically (see below) and triggers a rebuild without needing
# --force; --force is only needed to rebuild the *same* pin from scratch.
ORACLE_COMMIT="${ORACLE_COMMIT:-ffafe82}"

log() {
  echo "[oracle setup] $*"
}

# Remove the pristine worktree's git-worktree registration (not just its
# directory: `rm -rf` alone leaves a stale entry in `git worktree list`
# that then blocks re-adding the same path). Safe to call when nothing is
# registered.
remove_pristine_worktree_registration() {
  git -C "$REPO_ROOT" worktree prune >/dev/null 2>&1 || true
  if git -C "$REPO_ROOT" worktree list --porcelain 2>/dev/null \
      | grep -qxF "worktree $PRISTINE_SRC"; then
    git -C "$REPO_ROOT" worktree remove --force "$PRISTINE_SRC" \
      >/dev/null 2>&1 || true
  fi
  git -C "$REPO_ROOT" worktree prune >/dev/null 2>&1 || true
}

# Auto-detect a moved pin: if a previous successful run recorded a
# different ORACLE_COMMIT in status.json, force a rebuild even without
# --force, so the oracle never silently stays pinned to the wrong commit.
if [[ "$FORCE" -eq 0 && -f "$MARKER" && -f "$STATUS_FILE" ]]; then
  RECORDED_COMMIT="$(python3 -c '
import json, sys
try:
    print(json.load(open(sys.argv[1])).get("oracle_commit", ""))
except Exception:
    print("")
' "$STATUS_FILE" 2>/dev/null || true)"
  if [[ -n "$RECORDED_COMMIT" && "$RECORDED_COMMIT" != "$ORACLE_COMMIT" ]]; then
    log "ORACLE_COMMIT changed ($RECORDED_COMMIT -> $ORACLE_COMMIT);" \
      "forcing a rebuild"
    FORCE=1
  fi
fi

if [[ "$FORCE" -eq 1 ]]; then
  log "--force (or a pin change): removing existing venv and build state"
  remove_pristine_worktree_registration
  rm -rf "$VENV_DIR" "$BUILD_DIR"
fi

mkdir -p "$BUILD_DIR"

# Idempotent fast path: if a previous run completed successfully for this
# ORACLE_COMMIT, just print the recorded status and exit. Rebuilding the
# ANTLR C++ extension takes a minute or two, so we only do it once.
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

# --- Pin the oracle source: a detached git worktree of ORACLE_COMMIT. ---
#
# This is a real, separate checkout (its own files on disk, sharing objects
# with $REPO_ROOT's .git), not a symlink or an editable install pointing
# back into $REPO_ROOT: later commits in this worktree (Phase 3 rewriting
# fasm/ and setup.py) cannot change what gets installed here.
remove_pristine_worktree_registration
if [[ -e "$PRISTINE_SRC" ]]; then
  # Stale directory left over from an interrupted previous run (not a
  # registered worktree, or remove_pristine_worktree_registration above
  # would already have cleaned it up).
  log "removing stale $PRISTINE_SRC before re-adding the pristine worktree"
  rm -rf "$PRISTINE_SRC"
fi
log "creating pristine git worktree at $PRISTINE_SRC pinned to $ORACLE_COMMIT"
git -C "$REPO_ROOT" worktree add --detach "$PRISTINE_SRC" "$ORACLE_COMMIT" \
  > "$BUILD_DIR/worktree.log" 2>&1
ORACLE_COMMIT_RESOLVED="$(git -C "$PRISTINE_SRC" rev-parse HEAD)"
log "pristine worktree HEAD: $ORACLE_COMMIT_RESOLVED"

# --- Best effort: get everything the ANTLR C++ parser extension needs. ---
#
# None of the steps in this section may fail the script: if they don't
# succeed, `pip install` below still succeeds with the textX-only parser
# (setup.py's AntlrCMakeBuild catches build failures itself and falls back).

log "fetching git submodules needed for the ANTLR C++ build (read only, from GitHub)"
if git -C "$PRISTINE_SRC" submodule update --init --depth 1 -- \
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

# --- Install the pinned, immutable `fasm` package into the venv. ---
#
# setup.py's `AntlrCMakeBuild.run()` already implements the "attempt the
# ANTLR C++ build, fall back to textX-only on any failure" behaviour we
# want, so a single install call is all that is needed. `--no-build-isolation`
# makes it use the Cython/setuptools/wheel already installed in this venv
# instead of downloading its own isolated build environment.
#
# A regular (non-editable) install is used so the venv's site-packages
# holds a standalone copy, independent of $PRISTINE_SRC after this point.
# If that fails outright (not just the ANTLR extension falling back, which
# setup.py already handles internally, but the whole `pip install` call
# failing) an editable install of the *pristine* worktree is tried as a
# fallback -- still immutable with respect to $REPO_ROOT's live fasm/, just
# not copied into site-packages. See README.md for when this path is taken.
log "installing the pinned fasm package into the venv (non-editable)"
set +e
"$PY" -m pip install --no-build-isolation "$PRISTINE_SRC" \
  > "$BUILD_DIR/install.log" 2>&1
INSTALL_STATUS=$?
set -e
INSTALL_MODE="non-editable"

if [[ "$INSTALL_STATUS" -ne 0 ]]; then
  log "WARNING: non-editable install failed; retrying as an editable" \
    "install of the pinned (immutable) worktree; see $BUILD_DIR/install.log"
  set +e
  "$PY" -m pip install --no-build-isolation -e "$PRISTINE_SRC" \
    >> "$BUILD_DIR/install.log" 2>&1
  INSTALL_STATUS=$?
  set -e
  INSTALL_MODE="editable-pristine-worktree"
fi

if [[ "$INSTALL_STATUS" -ne 0 ]]; then
  log "ERROR: pip install of the pinned oracle failed entirely" \
    "(both non-editable and editable-of-pristine-worktree);" \
    "see $BUILD_DIR/install.log"
  tail -n 40 "$BUILD_DIR/install.log" >&2 || true
  exit 1
fi

# Run every check that imports `fasm` from a directory that cannot itself
# contain a `fasm/` package (i.e. NOT $REPO_ROOT), so plain `python -c`'s
# well known "current directory shadows an installed package of the same
# name" behaviour can never make this check pass by picking up the live
# repository's fasm/ instead of the pinned install. See README.md.
log "verifying parser availability and that fasm was NOT imported from the live repo"
AVAILABLE_JSON="$(cd "$BUILD_DIR" && "$PY" -c '
import json
import fasm.parser
print(json.dumps(sorted(fasm.parser.available)))
')"
FASM_FILE="$(cd "$BUILD_DIR" && "$PY" -c 'import fasm; print(fasm.__file__)')"

LIVE_FASM_DIR="$REPO_ROOT/fasm"
case "$FASM_FILE" in
  "$LIVE_FASM_DIR"/*)
    log "ERROR: the oracle imported fasm from the live repository" \
      "($FASM_FILE) instead of the pinned build at $PRISTINE_SRC or the" \
      "venv's site-packages; this should never happen and is a bug in" \
      "setup.sh, not a machine-specific issue"
    exit 1
    ;;
esac

ANTLR_OK=0
case "$AVAILABLE_JSON" in
  *antlr*) ANTLR_OK=1 ;;
esac

# NOTE: the env var assignments below must come BEFORE the command (bash
# "prefix assignment" form) to actually be exported into its environment;
# placed after, they would just be extra argv entries to the -c script.
ORACLE_COMMIT="$ORACLE_COMMIT" \
ORACLE_COMMIT_RESOLVED="$ORACLE_COMMIT_RESOLVED" \
INSTALL_MODE="$INSTALL_MODE" \
AVAILABLE_JSON="$AVAILABLE_JSON" \
ANTLR_OK="$ANTLR_OK" \
FASM_FILE="$FASM_FILE" \
PRISTINE_SRC="$PRISTINE_SRC" \
STATUS_FILE="$STATUS_FILE" \
"$PY" -c '
import json
import os

status = {
    "oracle_commit": os.environ["ORACLE_COMMIT"],
    "oracle_commit_resolved": os.environ["ORACLE_COMMIT_RESOLVED"],
    "install_mode": os.environ["INSTALL_MODE"],
    "available_parsers": json.loads(os.environ["AVAILABLE_JSON"]),
    "antlr_built": os.environ["ANTLR_OK"] == "1",
    "fasm_file": os.environ["FASM_FILE"],
    "pristine_src": os.environ["PRISTINE_SRC"],
}
with open(os.environ["STATUS_FILE"], "w") as f:
    json.dump(status, f, indent=2, sort_keys=True)
    f.write("\n")
'

if [[ "$ANTLR_OK" -eq 1 ]]; then
  log "ANTLR C++ parser built successfully; available parsers: $AVAILABLE_JSON"
else
  log "WARNING: ANTLR C++ parser was NOT built; falling back to textX only." \
    "See $BUILD_DIR/install.log for details. Available parsers: $AVAILABLE_JSON"
fi
log "installed ($INSTALL_MODE) from $FASM_FILE, pinned to $ORACLE_COMMIT ($ORACLE_COMMIT_RESOLVED)"

touch "$MARKER"
log "oracle setup complete: $VENV_DIR"

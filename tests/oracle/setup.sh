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
#
#   ANTLR_BUILD_ATTEMPTS=N        (default 3) how many times to retry the
#                                 ANTLR C++ build before accepting a
#                                 textX-only fallback (T0.4b: that build is
#                                 flaky across otherwise identical runs --
#                                 see tests/oracle/README.md, "T0.4b").
#   CMAKE_BUILD_PARALLEL_LEVEL=N  (default min(nproc, 4)) caps the ANTLR/
#                                 cmake build's parallelism instead of the
#                                 unbounded `-j` setup.py otherwise adds.
set -euo pipefail

FORCE=0
for arg in "$@"; do
  case "$arg" in
    --force)
      FORCE=1
      ;;
    -h | --help)
      sed -n '2,51p' "$0" | sed -e 's/^# \{0,1\}//'
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
# want, so a single install call would in principle be all that is needed.
# `--no-build-isolation` makes it use the Cython/setuptools/wheel already
# installed in this venv instead of downloading its own isolated build
# environment.
#
# T0.4b: the ANTLR C++ build is flaky across otherwise identical runs
# (T5.8's implementer saw it succeed once and fall back to textX on
# another run). Root cause, found by reading the pinned commit's own
# build files (immutable at $PRISTINE_SRC -- never edited here):
#
#   1. `src/CMakeLists.txt` pulls in `third_party/antlr4/runtime/Cpp/
#      cmake/ExternalAntlr4Cpp.cmake`, which does its OWN, SEPARATE
#      `ExternalProject_Add(... GIT_REPOSITORY https://github.com/antlr/
#      antlr4.git GIT_TAG e4c1a74 ...)` -- a fresh network `git clone` of
#      the antlr4 runtime sources at *cmake build time*, into a temp build
#      directory. This ignores the `third_party/antlr4` git submodule
#      already checked out locally (used only for the ANTLR tool jar and
#      cmake modules, not this runtime clone), so every build refetches
#      from github.com regardless of the submodule step above having
#      succeeded. A transient network hiccup here fails the whole step.
#   2. setup.py's `AntlrCMakeBuild.build_extension()` adds an UNBOUNDED
#      `cmake --build . -- -j` (no job count) whenever
#      `CMAKE_BUILD_PARALLEL_LEVEL` is unset, for both this ExternalProject
#      antlr4 runtime build and the `parse_fasm` extension itself --
#      i.e. as many parallel compiler/linker jobs as there are
#      translation units, uncapped, on a container that may have as few
#      as 4 cores and be sharing them with other agents/builds. Resource
#      contention (OOM, or the runtime's own internal build racing the
#      extension's) can fail the build.
#   3. `AntlrCMakeBuild.run()` wraps ALL of the above -- the network
#      clone, the configure, the parallel build, `ctest` -- in a single
#      `except BaseException`, prints a one-line message plus a
#      traceback, and returns normally: `pip install` itself always
#      reports success (exit 0) whether or not ANTLR actually built, so
#      its own exit code can never be used to detect or retry the
#      failure -- only inspecting `fasm.parser.available` after the
#      install (done below) can.
#
# Neither of those files may be edited (they are the immutable pinned
# oracle source, third_party/antlr4 is a pinned submodule) -- the fixes
# below are all in this script instead:
#
#   * `CMAKE_BUILD_PARALLEL_LEVEL` is exported, bounded to at most 4
#     (min(nproc, 4)), before every install attempt: this is exactly the
#     escape hatch `build_extension()` checks
#     (`if CMAKE_BUILD_PARALLEL_LEVEL is None: build_args += ['--', '-j']`),
#     so it both serialises/bounds the racy parallel build (item 2) and
#     is honoured natively by `cmake --build` for the antlr4_runtime
#     ExternalProject step too.
#   * Up to $ANTLR_BUILD_ATTEMPTS (default 3) full install attempts, each
#     followed by an availability check (import fasm.parser); a run that
#     falls back to textX-only is retried with backoff (5s, 10s) rather
#     than accepted on the first try, to ride out a transient network
#     failure of the ExternalProject clone (item 1). Every attempt's
#     `pip install` output is appended to install.log under its own
#     "=== attempt N ===" header, so a persistent failure's actual cause
#     (network vs. compiler vs. something else) is still visible there
#     instead of only ever showing the first attempt.
#
# A regular (non-editable) install is used so the venv's site-packages
# holds a standalone copy, independent of $PRISTINE_SRC after this point.
# If that fails outright (not just the ANTLR extension falling back,
# which setup.py already handles internally, but the whole `pip install`
# call failing) an editable install of the *pristine* worktree is tried
# as a fallback -- still immutable with respect to $REPO_ROOT's live
# fasm/, just not copied into site-packages. See README.md for when this
# path is taken.
NPROC="$(nproc 2>/dev/null || echo 4)"
export CMAKE_BUILD_PARALLEL_LEVEL="${CMAKE_BUILD_PARALLEL_LEVEL:-$((NPROC < 4 ? NPROC : 4))}"
log "bounding the ANTLR/cmake build to CMAKE_BUILD_PARALLEL_LEVEL=$CMAKE_BUILD_PARALLEL_LEVEL" \
  "jobs (of $NPROC available) -- see T0.4b comment above for why"

ANTLR_BUILD_ATTEMPTS="${ANTLR_BUILD_ATTEMPTS:-3}"
INSTALL_MODE="non-editable"
INSTALL_STATUS=1
ATTEMPT_ANTLR_OK=0
for attempt in $(seq 1 "$ANTLR_BUILD_ATTEMPTS"); do
  log "installing the pinned fasm package into the venv (non-editable," \
    "attempt $attempt/$ANTLR_BUILD_ATTEMPTS)"
  {
    echo "=== attempt $attempt/$ANTLR_BUILD_ATTEMPTS" \
      "(CMAKE_BUILD_PARALLEL_LEVEL=$CMAKE_BUILD_PARALLEL_LEVEL) ==="
  } >> "$BUILD_DIR/install.log"
  set +e
  "$PY" -m pip install --no-build-isolation "$PRISTINE_SRC" \
    >> "$BUILD_DIR/install.log" 2>&1
  INSTALL_STATUS=$?
  set -e
  if [[ "$INSTALL_STATUS" -ne 0 ]]; then
    # A hard pip failure is not the ANTLR-fallback flakiness this task is
    # about (that path always exits 0, see the comment above) -- retrying
    # it here would just repeat the same packaging error, so break out to
    # the existing editable-install fallback below instead.
    break
  fi
  ATTEMPT_ANTLR_JSON="$(cd "$BUILD_DIR" && "$PY" -c '
import json
import fasm.parser
print(json.dumps(sorted(fasm.parser.available)))
' 2>/dev/null || echo '[]')"
  case "$ATTEMPT_ANTLR_JSON" in
    *antlr*)
      ATTEMPT_ANTLR_OK=1
      log "ANTLR C++ parser built on attempt $attempt/$ANTLR_BUILD_ATTEMPTS"
      break
      ;;
    *)
      if [[ "$attempt" -lt "$ANTLR_BUILD_ATTEMPTS" ]]; then
        backoff=$((attempt * 5))
        log "WARNING: attempt $attempt/$ANTLR_BUILD_ATTEMPTS fell back to" \
          "textX only (available: $ATTEMPT_ANTLR_JSON); retrying the ANTLR" \
          "build in ${backoff}s -- see $BUILD_DIR/install.log"
        sleep "$backoff"
      fi
      ;;
  esac
done

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

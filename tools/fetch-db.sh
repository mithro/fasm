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
# Fetches prjxray-db / prjuray-db family databases with sparse, blobless,
# shallow clones, so a differential test can pass
# `--db-root <cache>/prjxray-db/<family>` straight to a reference or Rust
# tool without a full (multi-GB, full-history) checkout of either database
# repository. See docs/rewrite/DESIGN-xilinx-db.md section 2 for the
# on-disk layout this relies on (family directory == db-root).
#
# Usage:
#   tools/fetch-db.sh prjxray <family> [<family> ...]
#   tools/fetch-db.sh prjuray <family> [<family> ...]
#   tools/fetch-db.sh all                 # every family of both databases
#
#   Families:
#     prjxray: artix7 kintex7 spartan7 zynq7
#     prjuray: zynqusp (the only family of upstream prjuray-db, 2 parts)
#
# Examples:
#   tools/fetch-db.sh prjxray artix7
#     -> $FASM_DB_CACHE/prjxray-db/artix7   (~181 MiB)
#   tools/fetch-db.sh prjuray zynqusp
#     -> $FASM_DB_CACHE/prjuray-db/zynqusp  (~217 MiB)
#
# Idempotent: a family already present is left alone; adding a new family
# to an existing sparse checkout uses `git sparse-checkout add` (does not
# re-fetch families already present). Re-running with the same families
# is a fast no-op.
#
# Cache directory: ${FASM_DB_CACHE:-tests/oracle/build/db} (gitignored;
# never committed -- each machine fetches its own). Override with the
# FASM_DB_CACHE environment variable to use a shared cache across worktrees:
#   FASM_DB_CACHE=/var/cache/fasm-db tools/fetch-db.sh prjxray artix7
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CACHE_DIR="${FASM_DB_CACHE:-$REPO_ROOT/tests/oracle/build/db}"

# --- Pinned commits (see docs/rewrite/DESIGN-xilinx-db.md section 1 for
# provenance; these match the commits prjxray/prjuray/f4pga-xc-fasm are
# pinned to by tests/oracle/setup-xilinx.sh at the time this was written,
# 2026-09-24). Override to intentionally re-pin. ---
PRJXRAY_DB_COMMIT="${PRJXRAY_DB_COMMIT:-0a0addedd73e7e4139d52a6d8db4258763e0f1f3}"
PRJURAY_DB_COMMIT="${PRJURAY_DB_COMMIT:-affbc5e555ebae16475f32e8fb2d6565d4204f3f}"

PRJXRAY_DB_URL="https://github.com/f4pga/prjxray-db.git"
PRJURAY_DB_URL="https://github.com/f4pga/prjuray-db.git"

PRJXRAY_FAMILIES="artix7 kintex7 spartan7 zynq7"
PRJURAY_FAMILIES="zynqusp"

log() {
  echo "[fetch-db] $*"
}

usage() {
  sed -n '2,48p' "$0" | sed -e 's/^# \{0,1\}//'
}

# fetch_repo REPO_NAME URL PIN DEST FAMILY...
fetch_repo() {
  local repo_name="$1" url="$2" pin="$3" dest="$4"
  shift 4
  local families=("$@")

  mkdir -p "$(dirname "$dest")"

  if [[ ! -d "$dest/.git" ]]; then
    log "cloning $repo_name (sparse, blobless, shallow) pinned to $pin -> $dest"
    git clone --quiet --filter=blob:none --sparse --depth 1 \
      --no-checkout "$url" "$dest"
    git -C "$dest" fetch --quiet --depth 1 origin "$pin"
    git -C "$dest" checkout --quiet "$pin"
    git -C "$dest" sparse-checkout init --cone
    # cone mode with no directories set checks out top-level files only
    # (LICENSE, README.md, ...); no family directory yet.
    git -C "$dest" sparse-checkout set
  else
    local current
    current="$(git -C "$dest" rev-parse HEAD)"
    if [[ "$current" != "$pin" ]]; then
      log "$repo_name pin changed ($current -> $pin); re-fetching"
      git -C "$dest" fetch --quiet --depth 1 origin "$pin"
      git -C "$dest" checkout --quiet "$pin"
    fi
  fi

  local existing
  existing="$(git -C "$dest" sparse-checkout list 2>/dev/null || true)"
  local to_add=()
  local fam
  for fam in "${families[@]}"; do
    if [[ -d "$dest/$fam" ]] && grep -qxF "$fam" <<<"$existing"; then
      log "$repo_name/$fam already fetched"
    else
      to_add+=("$fam")
    fi
  done

  if [[ "${#to_add[@]}" -gt 0 ]]; then
    log "fetching $repo_name families: ${to_add[*]}"
    git -C "$dest" sparse-checkout add "${to_add[@]}"
  fi

  for fam in "${families[@]}"; do
    if [[ ! -d "$dest/$fam" ]]; then
      log "ERROR: $repo_name has no family directory '$fam' after fetch" \
        "(known families for $repo_name: see 'tools/fetch-db.sh --help')" >&2
      exit 1
    fi
    local size
    size="$(du -sh "$dest/$fam" 2>/dev/null | cut -f1)"
    log "$repo_name/$fam ready at $dest/$fam ($size)"
  done
}

if [[ $# -eq 0 || "$1" == "-h" || "$1" == "--help" ]]; then
  usage
  exit 0
fi

REPO="$1"
shift

case "$REPO" in
  all)
    log "fetching every family of both databases into $CACHE_DIR"
    # shellcheck disable=SC2086
    fetch_repo prjxray-db "$PRJXRAY_DB_URL" "$PRJXRAY_DB_COMMIT" \
      "$CACHE_DIR/prjxray-db" $PRJXRAY_FAMILIES
    # shellcheck disable=SC2086
    fetch_repo prjuray-db "$PRJURAY_DB_URL" "$PRJURAY_DB_COMMIT" \
      "$CACHE_DIR/prjuray-db" $PRJURAY_FAMILIES
    ;;
  prjxray)
    if [[ $# -eq 0 ]]; then
      echo "fetch-db.sh: prjxray needs at least one family" \
        "(one of: $PRJXRAY_FAMILIES)" >&2
      exit 2
    fi
    for fam in "$@"; do
      case " $PRJXRAY_FAMILIES " in
        *" $fam "*) ;;
        *)
          echo "fetch-db.sh: unknown prjxray family '$fam'" \
            "(known: $PRJXRAY_FAMILIES)" >&2
          exit 2
          ;;
      esac
    done
    fetch_repo prjxray-db "$PRJXRAY_DB_URL" "$PRJXRAY_DB_COMMIT" \
      "$CACHE_DIR/prjxray-db" "$@"
    ;;
  prjuray)
    if [[ $# -eq 0 ]]; then
      echo "fetch-db.sh: prjuray needs at least one family" \
        "(one of: $PRJURAY_FAMILIES)" >&2
      exit 2
    fi
    for fam in "$@"; do
      case " $PRJURAY_FAMILIES " in
        *" $fam "*) ;;
        *)
          echo "fetch-db.sh: unknown prjuray family '$fam'" \
            "(known: $PRJURAY_FAMILIES)" >&2
          exit 2
          ;;
      esac
    done
    fetch_repo prjuray-db "$PRJURAY_DB_URL" "$PRJURAY_DB_COMMIT" \
      "$CACHE_DIR/prjuray-db" "$@"
    ;;
  *)
    echo "fetch-db.sh: unknown repo '$REPO' (expected prjxray, prjuray, or all)" >&2
    usage >&2
    exit 2
    ;;
esac

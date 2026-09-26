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
#   tools/fetch-db.sh openxc7 <family> [<family> ...]
#   tools/fetch-db.sh all                 # every family of both databases
#
#   Families:
#     prjxray: artix7 kintex7 spartan7 zynq7
#     prjuray: zynqusp (the only family of upstream prjuray-db, 2 parts)
#     openxc7: artix7 kintex7 spartan7 zynq7 (same names, a DIFFERENT,
#       independently pinned copy -- see "openxc7" below)
#
# Examples:
#   tools/fetch-db.sh prjxray artix7
#     -> $FASM_DB_CACHE/prjxray-db/artix7   (~181 MiB)
#   tools/fetch-db.sh prjuray zynqusp
#     -> $FASM_DB_CACHE/prjuray-db/zynqusp  (~217 MiB)
#   tools/fetch-db.sh openxc7 artix7
#     -> $FASM_DB_CACHE/prjxray-db-openxc7/artix7   (~188 MiB)
#
# openxc7: exposes the prjxray-db copy BUNDLED INSIDE the openXC7 snap
# (tools/e2e/setup-openxc7.sh's pin: openXC7 snap 0.8.2, sha256
# 6b2e07ce99ef33d3a4e41e2fd2eb916f26bb0ece34a97216ed840a0032e98587;
# Project X-Ray commit 4c157493, 2021-12-14 per the snap's own
# prjxray-db/Info.md -- see tools/e2e/README.md, "A note on prjxray-db
# provenance"). This is NOT the same commit as the "prjxray" pin above:
# the two are different, independently maintained snapshots of Project
# X-Ray and are not always identical (the snap db has STARTUP/CFG_CENTER
# ppips the pinned db lacks, T5.8b). Named "prjxray-db-openxc7" (not
# "prjxray-db") specifically so it never collides with, or gets confused
# for, the "prjxray" family's cache entry in the same $FASM_DB_CACHE.
#
# Getting at just the db, without installing the ~4 GiB openXC7 +
# OSS CAD Suite toolchain (tools/e2e/setup-openxc7.sh): reuses
# tools/e2e/build/openxc7/root (that script's own extracted snap) if
# already present; otherwise downloads only the snap (~200 MiB,
# sha256-verified against the pin above, deleted again once the db is
# extracted) and extracts only the requested family's db directory from
# it with `unsquashfs <snap> opt/nextpnr-xilinx/external/prjxray-db/
# <family>` (a targeted extraction -- the rest of the ~1.3 GiB snap
# content is never decompressed). Needs `unsquashfs` (squashfs-tools) and,
# unless tools/e2e/build/openxc7/root already exists, network access to
# github.com; skips each family with a clear message (exit 0) if either
# is unavailable, rather than failing the whole invocation. Every
# extracted file's sha256 is recorded in `<family>/.manifest.sha256`,
# checked on a later run to decide whether re-extraction is needed
# (skipped, like the git-based families above, once already present and
# verified).
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
LOG_DIR_OPENXC7="$CACHE_DIR/.fetch-db-logs"

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
OPENXC7_FAMILIES="artix7 kintex7 spartan7 zynq7"

# --- openXC7 snap pin (same as tools/e2e/setup-openxc7.sh; see
# tools/e2e/README.md, "A note on prjxray-db provenance"). Override to
# intentionally re-pin. ---
OPENXC7_SNAP_VERSION="${OPENXC7_SNAP_VERSION:-0.8.2}"
OPENXC7_SNAP_SHA256="${OPENXC7_SNAP_SHA256:-6b2e07ce99ef33d3a4e41e2fd2eb916f26bb0ece34a97216ed840a0032e98587}"
OPENXC7_SNAP_URL="${OPENXC7_SNAP_URL:-https://github.com/openXC7/openXC7-snap/releases/download/${OPENXC7_SNAP_VERSION}/openxc7_${OPENXC7_SNAP_VERSION}_amd64.snap}"
# tools/e2e/setup-openxc7.sh's own extraction of the same snap, reused
# here (copied, not re-downloaded/re-extracted) when present.
OPENXC7_E2E_ROOT="${OPENXC7_E2E_BUILD:-$SCRIPT_DIR/e2e/build}/openxc7/root"
OPENXC7_E2E_DB="$OPENXC7_E2E_ROOT/opt/nextpnr-xilinx/external/prjxray-db"

log() {
  echo "[fetch-db] $*"
}

usage() {
  sed -n '2,83p' "$0" | sed -e 's/^# \{0,1\}//'
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

sha256_of() {
  sha256sum "$1" | awk '{print $1}'
}

# manifest_ok DIR: true if DIR/.manifest.sha256 exists and every file it
# lists still has the recorded sha256 (extraction is complete and intact).
manifest_ok() {
  local dir="$1"
  [[ -f "$dir/.manifest.sha256" ]] || return 1
  (cd "$dir" && sha256sum -c .manifest.sha256 --quiet) >/dev/null 2>&1
}

write_manifest() {
  local dir="$1"
  (cd "$dir" && find . -type f ! -name '.manifest.sha256' -print0 |
    LC_ALL=C sort -z | xargs -0 sha256sum >.manifest.sha256)
}

# fetch_openxc7_db FAMILY...: extracts $FASM_DB_CACHE/prjxray-db-openxc7/
# <family> from the openXC7 snap's bundled prjxray-db (see the usage
# comment above, "openxc7"). Skips a family (exit 0, not 1) when the
# snap cannot be obtained -- network unreachable, or `unsquashfs`
# missing -- since this database is optional (only needed by the
# T7.1/T7.2 end-to-end tests that deliberately compare against it).
fetch_openxc7_db() {
  local families=("$@")
  local dest_root="$CACHE_DIR/prjxray-db-openxc7"
  mkdir -p "$dest_root" "$LOG_DIR_OPENXC7"

  local have_unsquashfs=1
  command -v unsquashfs >/dev/null 2>&1 || have_unsquashfs=0

  local fam fam_dest
  for fam in "${families[@]}"; do
    fam_dest="$dest_root/$fam"
    if manifest_ok "$fam_dest"; then
      log "openxc7/$fam already extracted and verified at $fam_dest"
      continue
    fi
    rm -rf "$fam_dest"

    if [[ -d "$OPENXC7_E2E_DB/$fam" ]]; then
      log "openxc7/$fam: copying from the already-extracted snap at" \
        "$OPENXC7_E2E_DB/$fam (tools/e2e/setup-openxc7.sh)"
      mkdir -p "$fam_dest"
      cp -a "$OPENXC7_E2E_DB/$fam/." "$fam_dest/"
    elif [[ "$have_unsquashfs" -eq 0 ]]; then
      log "skipping openxc7/$fam: unsquashfs not found (install" \
        "squashfs-tools), and no extracted snap at $OPENXC7_E2E_ROOT" \
        "(tools/e2e/setup-openxc7.sh)"
      continue
    else
      local tmp_snap tmp_extract got attempt
      tmp_snap="$(mktemp -t openxc7-snap.XXXXXX.snap)"
      got=""
      for attempt in 1 2 3; do
        log "downloading the openXC7 snap $OPENXC7_SNAP_VERSION" \
          "(attempt $attempt/3, needed for prjxray-db/$fam)"
        if curl -fsSL --connect-timeout 20 -o "$tmp_snap" \
          "$OPENXC7_SNAP_URL"; then
          got="$(sha256_of "$tmp_snap")"
          [[ "$got" == "$OPENXC7_SNAP_SHA256" ]] && break
          log "openXC7 snap sha256 mismatch (got $got, expected" \
            "$OPENXC7_SNAP_SHA256); retrying"
        else
          log "download failed; retrying"
        fi
        got=""
        sleep $((attempt * 3))
      done
      if [[ -z "$got" ]]; then
        log "skipping openxc7/$fam: could not download and verify the" \
          "openXC7 snap after 3 attempts (network unreachable, or the" \
          "release moved -- see tools/e2e/setup-openxc7.sh)"
        rm -f "$tmp_snap"
        continue
      fi
      tmp_extract="$(mktemp -d)"
      log "extracting prjxray-db/$fam only (unsquashfs, targeted" \
        "extraction -- the rest of the snap is not decompressed)"
      unsquashfs -f -d "$tmp_extract" "$tmp_snap" \
        "opt/nextpnr-xilinx/external/prjxray-db/$fam" \
        >"$LOG_DIR_OPENXC7/unsquashfs-$fam.log" 2>&1 || {
        log "ERROR: unsquashfs failed for $fam (see" \
          "$LOG_DIR_OPENXC7/unsquashfs-$fam.log)" >&2
        rm -rf "$tmp_extract"
        rm -f "$tmp_snap"
        exit 1
      }
      mkdir -p "$fam_dest"
      cp -a \
        "$tmp_extract/opt/nextpnr-xilinx/external/prjxray-db/$fam/." \
        "$fam_dest/"
      rm -rf "$tmp_extract"
      rm -f "$tmp_snap"
    fi

    write_manifest "$fam_dest"
    local size
    size="$(du -sh "$fam_dest" 2>/dev/null | cut -f1)"
    log "openxc7/$fam ready at $fam_dest ($size)"
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
  openxc7)
    if [[ $# -eq 0 ]]; then
      echo "fetch-db.sh: openxc7 needs at least one family" \
        "(one of: $OPENXC7_FAMILIES)" >&2
      exit 2
    fi
    for fam in "$@"; do
      case " $OPENXC7_FAMILIES " in
        *" $fam "*) ;;
        *)
          echo "fetch-db.sh: unknown openxc7 family '$fam'" \
            "(known: $OPENXC7_FAMILIES)" >&2
          exit 2
          ;;
      esac
    done
    fetch_openxc7_db "$@"
    ;;
  *)
    echo "fetch-db.sh: unknown repo '$REPO' (expected prjxray, prjuray," \
      "openxc7, or all)" >&2
    usage >&2
    exit 2
    ;;
esac

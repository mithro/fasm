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
# Installs the f4pga (Yosys + VPR) toolchain for Xilinx 7 series (T7.3)
# the way f4pga-examples documents it (docs/getting.rst of
# https://github.com/chipsalliance/f4pga-examples at F4PGA_EXAMPLES_COMMIT
# below):
#
#   1. a conda environment "xc7" from f4pga-examples' xc7/environment.yml
#      (yosys + symbiflow-yosys-plugins, vtr-optimized (VPR, genfasm),
#      prjxray-tools (xc7frames2bit, bitread, ...), prjxray-db, the RISC-V
#      GCC, openFPGALoader, python 3.7, ...);
#   2. the PyPI part of that environment (xc7/requirements.txt: the f4pga
#      python package, prjxray, f4pga-xc-fasm (xcfasm), fasm, ...);
#   3. the f4pga architecture definitions (symbiflow-arch-defs) packages
#      "install-xc7" and one "<device>_test" package per device, from
#      storage.googleapis.com.
#
# Layout (F4PGA_INSTALL_DIR=$ROOT, FPGA_FAM=xc7, as in the documentation):
#
#   $ROOT/xc7/conda/envs/xc7        the conda environment
#   $ROOT/xc7/share/f4pga/...       the architecture definitions
#   $ROOT/bin/micromamba            the conda package manager used
#   $ROOT/status.json               what was installed (pins, sha256s)
#
# $ROOT is $F4PGA_E2E_ROOT, default tools/e2e/build/f4pga (gitignored).
# Source tools/e2e/f4pga-env.sh afterwards to use the toolchain.
#
# Usage:
#   tools/e2e/setup-f4pga.sh [--devices D[,D...]] [--remove-devices D[,D...]]
#                            [--big-files-dir DIR] [--force]
#
#   --devices D,...     architecture definition packages to install
#                       (default: xc7a50t_test,xc7z010_test). Known:
#                       xc7a50t_test (Arty A7-35T, Basys3; 2.6 GiB
#                       extracted), xc7z010_test (Zybo; 1.5 GiB),
#                       xc7a100t_test (Arty A7-100T, Nexys4 DDR; 4.8 GiB),
#                       xc7a200t_test (Nexys Video; 10.5 GiB).
#   --remove-devices D  delete these installed devices (to make room).
#   --big-files-dir DIR install the devices given with --devices into
#                       DIR/<device> (arch/<device> is a symlink to it),
#                       e.g. a tmpfs like /dev/shm when the disk is short
#                       (almost all of a device is its VPR routing graph
#                       rr_graph_*.rr_graph.real.bin). Only the directory
#                       can be a symlink: VPR maps the graph with the
#                       size of lstat() of its path, so a symlinked file
#                       fails with "size_ 73 is not a multiple of
#                       capnp::word". A tmpfs is lost at reboot: the
#                       script reinstalls a device whose directory is
#                       gone when it is run again.
#   --force             reinstall the conda environment and the devices.
#
# Deviations from the documented procedure (docs/getting.rst), all
# forced by this machine and none changing what gets installed:
#
#   * micromamba (a single static binary) instead of the Miniconda
#     installer ("Miniconda3-latest", unpinned, ~200 MB): the environment
#     is created from tools/e2e/f4pga/xc7-conda-explicit.txt, the explicit
#     lock of what `conda env create -f xc7/environment.yml` resolved to
#     (channels litex-hub and defaults, as conda does), with md5s;
#   * the f4pga python package (common/requirements.txt names a
#     github.com/.../archive/<commit>.zip URL) is installed from a git
#     clone at the same commit: github.com archive downloads are refused
#     with HTTP 403 by this machine's egress proxy, git clones are not;
#   * the PyPI packages are pinned to what xc7/requirements.txt resolved
#     to on 2026-09-25 (tools/e2e/f4pga/xc7-pip-freeze.txt), instead of
#     "latest compatible";
#   * the downloaded architecture definition archives are verified
#     against the sha256s below, extracted, and deleted.
#
# --- Pins ----------------------------------------------------------------
#
#   f4pga-examples                     13f11197b33dae1cde3bf146f317d63f0134eacf
#   f4pga (python package)             e1cd038f06c7161b27afd0073fb507da2b8e5a9e
#   micromamba 2.3.2-0
#     https://github.com/mamba-org/micromamba-releases/releases/download/2.3.2-0/micromamba-linux-64
#     sha256 ffc3cb8d52d4d6b354bdbb979c407719c485392b74e462cbd50811aa88e58f85
#   symbiflow-arch-defs 20220920-124259 / 007d1c1
#     https://storage.googleapis.com/symbiflow-arch-defs/artifacts/prod/foss-fpga-tools/symbiflow-arch-defs/continuous/install/20220920-124259/symbiflow-arch-defs-<pkg>-007d1c1.tar.xz
#     sha256 (see ARCH_SHA256 below)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ROOT="${F4PGA_E2E_ROOT:-$REPO_ROOT/tools/e2e/build/f4pga}"
LOCK_DIR="$REPO_ROOT/tools/e2e/f4pga"
FAM=xc7
ENV_DIR="$ROOT/$FAM/conda/envs/$FAM"
DL="$ROOT/downloads"
STATUS_FILE="$ROOT/status.json"

F4PGA_EXAMPLES_COMMIT=13f11197b33dae1cde3bf146f317d63f0134eacf
F4PGA_COMMIT=e1cd038f06c7161b27afd0073fb507da2b8e5a9e
F4PGA_REPO=https://github.com/chipsalliance/f4pga.git
MICROMAMBA_VERSION=2.3.2-0
MICROMAMBA_URL="https://github.com/mamba-org/micromamba-releases/releases/download/$MICROMAMBA_VERSION/micromamba-linux-64"
MICROMAMBA_SHA256=ffc3cb8d52d4d6b354bdbb979c407719c485392b74e462cbd50811aa88e58f85
ARCH_TIMESTAMP=20220920-124259
ARCH_HASH=007d1c1
ARCH_BASE="https://storage.googleapis.com/symbiflow-arch-defs/artifacts/prod/foss-fpga-tools/symbiflow-arch-defs/continuous/install/$ARCH_TIMESTAMP"
declare -A ARCH_SHA256=(
  [install-xc7]=8d8ba213fab8d2a0b5e99da097ef78515d42878284254c294e8d47ec04ad8a65
  [xc7a50t_test]=7dafd8b08503afe8baa782218c5a703a8afc5b8c2601a3062307178a620d834d
  [xc7a100t_test]=7a64daaa04b8f0761a86d3c74d2bc1bcacc27a6680edc8e7672bc2e804a73bcf
  [xc7a200t_test]=1fc5bca8811923d91f94680e414969d94c92787f814ed919fa0d57de66cd18bf
  [xc7z010_test]=784976422428ab8f26c0e63414692cc7a4bdd29f0161141147ae78bd97ed666d
)

DEVICES=xc7a50t_test,xc7z010_test
REMOVE=
BIG_DIR=
FORCE=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --devices) DEVICES="$2"; shift 2 ;;
    --devices=*) DEVICES="${1#*=}"; shift ;;
    --remove-devices) REMOVE="$2"; shift 2 ;;
    --remove-devices=*) REMOVE="${1#*=}"; shift ;;
    --big-files-dir) BIG_DIR="$2"; shift 2 ;;
    --big-files-dir=*) BIG_DIR="${1#*=}"; shift ;;
    --force) FORCE=1; shift ;;
    -h|--help) sed -n '19,60p' "$0"; exit 0 ;;
    *) echo "setup-f4pga: unknown argument $1" >&2; exit 2 ;;
  esac
done

log() { echo "[setup-f4pga] $*" >&2; }

# download URL FILE SHA256: into $DL, verified.
download() {
  local url="$1" out="$DL/$2" sha="$3"
  mkdir -p "$DL"
  if [[ -f "$out" ]] && echo "$sha  $out" | sha256sum -c --quiet 2>/dev/null; then
    return 0
  fi
  log "downloading $url"
  rm -f "$out.part"
  timeout 1800 curl -sSfL --retry 3 -o "$out.part" "$url"
  echo "$sha  $out.part" | sha256sum -c --quiet || {
    log "sha256 mismatch for $url (expected $sha):"
    sha256sum "$out.part" >&2
    exit 1
  }
  mv "$out.part" "$out"
}

mkdir -p "$ROOT/bin" "$ROOT/$FAM"

# --- 1. micromamba ------------------------------------------------------
download "$MICROMAMBA_URL" "micromamba-$MICROMAMBA_VERSION" "$MICROMAMBA_SHA256"
install -m 755 "$DL/micromamba-$MICROMAMBA_VERSION" "$ROOT/bin/micromamba"
export MAMBA_ROOT_PREFIX="$ROOT/mamba"
MM="$ROOT/bin/micromamba"

# --- 2. conda environment ------------------------------------------------
if [[ $FORCE == 1 ]]; then
  rm -rf "$ENV_DIR" "$ROOT/.env-done"
fi
if [[ ! -f "$ROOT/.env-done" ]]; then
  rm -rf "$ENV_DIR"
  log "creating the conda environment $ENV_DIR"
  timeout 1800 "$MM" create -y -q -p "$ENV_DIR" \
    -f "$LOCK_DIR/xc7-conda-explicit.txt"

  # --- 3. PyPI packages (xc7/requirements.txt, pinned) ------------------
  src="$ROOT/src/f4pga"
  if [[ ! -d "$src/.git" ]]; then
    rm -rf "$src"
    mkdir -p "$ROOT/src"
    timeout 900 git clone -q "$F4PGA_REPO" "$src"
  fi
  git -C "$src" checkout -q "$F4PGA_COMMIT"
  log "installing the PyPI packages"
  PATH="$ENV_DIR/bin:$PATH" timeout 1800 "$ENV_DIR/bin/python" -m pip \
    install -q --no-cache-dir -r "$LOCK_DIR/xc7-pip-freeze.txt"
  PATH="$ENV_DIR/bin:$PATH" timeout 900 "$ENV_DIR/bin/python" -m pip \
    install -q --no-cache-dir --no-deps "$src/f4pga"
  rm -rf "$ROOT/src"
  "$MM" clean -a -y -q >/dev/null
  rm -rf "$ROOT/mamba/pkgs"
  touch "$ROOT/.env-done"
fi

# --- 4. architecture definitions ------------------------------------------
share="$ROOT/$FAM/share/f4pga"
IFS=, read -r -a remove <<<"$REMOVE"
for dev in "${remove[@]}"; do
  [[ -n "$dev" ]] || continue
  log "removing $dev"
  if [[ -L "$share/arch/$dev" ]]; then
    rm -rf "$(readlink "$share/arch/$dev")"
  fi
  rm -rf "$share/arch/$dev" "$ROOT/.arch-$dev-done"
done

install_pkg() {
  local pkg="$1"
  local done="$ROOT/.arch-$pkg-done"
  if [[ $FORCE == 1 ]]; then rm -f "$done"; fi
  if [[ $pkg != install-* && ! -e "$share/arch/$pkg" ]]; then
    rm -f "$done"  # e.g. its --big-files-dir tmpfs was lost
  fi
  [[ -f "$done" ]] && return 0
  if [[ -z "${ARCH_SHA256[$pkg]:-}" ]]; then
    log "unknown architecture definition package $pkg"
    exit 2
  fi
  local file="symbiflow-arch-defs-$pkg-$ARCH_HASH.tar.xz"
  download "$ARCH_BASE/$file" "$file" "${ARCH_SHA256[$pkg]}"
  log "extracting $file"
  if [[ $pkg != install-* ]]; then
    if [[ -L "$share/arch/$pkg" ]]; then
      rm -rf "$(readlink "$share/arch/$pkg")"
    fi
    rm -rf "$share/arch/$pkg"
    if [[ -n "$BIG_DIR" ]]; then
      mkdir -p "$BIG_DIR/$pkg" "$share/arch"
      ln -s "$(cd "$BIG_DIR/$pkg" && pwd)" "$share/arch/$pkg"
    fi
  fi
  tar -xJf "$DL/$file" -C "$ROOT/$FAM" --keep-directory-symlink
  rm -f "$DL/$file"
  touch "$done"
}

install_pkg install-xc7
IFS=, read -r -a devices <<<"$DEVICES"
for dev in "${devices[@]}"; do
  [[ -n "$dev" ]] || continue
  install_pkg "$dev"
done

# --- 5. status.json ---------------------------------------------------------
installed=$(cd "$share/arch" 2>/dev/null && ls -d -- *_test 2>/dev/null | paste -sd, || true)
python3 - "$STATUS_FILE" <<EOF
import json, sys
json.dump({
    'f4pga_examples_commit': '$F4PGA_EXAMPLES_COMMIT',
    'f4pga_commit': '$F4PGA_COMMIT',
    'micromamba': {'version': '$MICROMAMBA_VERSION', 'url': '$MICROMAMBA_URL',
                   'sha256': '$MICROMAMBA_SHA256'},
    'conda_lock': 'tools/e2e/f4pga/xc7-conda-explicit.txt',
    'pip_lock': 'tools/e2e/f4pga/xc7-pip-freeze.txt',
    'arch_defs': {'timestamp': '$ARCH_TIMESTAMP', 'hash': '$ARCH_HASH',
                  'base_url': '$ARCH_BASE',
                  'sha256': $(python3 -c "import json,sys; print(json.dumps(dict(a.split('=',1) for a in sys.argv[1:])))" $(for k in "${!ARCH_SHA256[@]}"; do echo "$k=${ARCH_SHA256[$k]}"; done))},
    'installed_devices': [d for d in '$installed'.split(',') if d],
}, open(sys.argv[1], 'w'), indent=2, sort_keys=True)
EOF
log "done: $(du -sh "$ROOT" | cut -f1) in $ROOT (devices: ${installed:-none})"

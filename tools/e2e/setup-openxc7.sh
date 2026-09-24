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
# Sets up an *open source* Xilinx 7 series synthesis + place-and-route
# toolchain on this machine (T7.1): yosys (OSS CAD Suite) + nextpnr-xilinx
# + bbasm + fasm2frames + xc7frames2bit + bit2fasm + bit2fasm's prjxray-db
# copy, all from the openXC7 project (https://github.com/openXC7). This is
# what turns the plain Verilog/LiteX designs in f4pga-examples and
# fpgas.online-test-designs (T7.2, T7.3) into FASM, frames and bitstreams
# that can be compared against the Rust rewrite's output.
#
# This is deliberately separate from tests/oracle/setup-xilinx.sh: that
# script builds the *reference* prjxray/prjuray C++ tools from source,
# pinned to exact commits, to serve as a byte-exact oracle for frames/
# bitstream differential tests (T5.9, T6.3). This script instead installs
# a pre-built, independent toolchain whose *job* is to synthesize and
# place-and-route Verilog into FASM in the first place -- something the
# oracle tools cannot do (they only convert FASM <-> frames <-> bitstream).
#
# Usage:
#   tools/e2e/setup-openxc7.sh [--force] [--parts DEVICE[,DEVICE...]]
#
#   --force            Delete tools/e2e/build/openxc7 and
#                       tools/e2e/build/oss-cad-suite and rebuild from
#                       scratch (same pins). Downloaded archives in
#                       tools/e2e/build/downloads are kept and reused if
#                       their sha256 still matches (pass --force twice, or
#                       rm -rf tools/e2e/build/downloads, to also redo the
#                       downloads).
#                       Without --force the script is a fast no-op once
#                       set up successfully for the requested pins/parts.
#   --parts DEVICE,...  Also build a nextpnr-xilinx chip database for each
#                       given prjxray-db device name (e.g.
#                       xc7a100tcsg324-1). The chip database for
#                       xc7a35tcsg324-1 (needed by run-counter.sh) is
#                       always built. Building the chipdb for larger parts
#                       is CPU/RAM heavy -- see tools/e2e/README.md
#                       ("Chip database sizes and timings") before
#                       requesting xc7a200t*/xc7a100t* on a small machine.
#
# Everything this script creates lives under the gitignored
# tools/e2e/build/ -- nothing here is committed to git.
#
# --- Pinned downloads (resolved 2026-09-24; see tools/e2e/README.md) ----
#
#   openXC7 snap (yosys NOT included -- see "no yosys" below):
#     https://github.com/openXC7/openXC7-snap/releases/download/0.8.2/openxc7_0.8.2_amd64.snap
#     sha256 6b2e07ce99ef33d3a4e41e2fd2eb916f26bb0ece34a97216ed840a0032e98587
#     (169226240 bytes)
#
#   OSS CAD Suite (yosys; the openXC7 snap explicitly ships without it --
#   see its meta/snap.yaml description):
#     https://github.com/YosysHQ/oss-cad-suite-build/releases/download/2026-09-21/oss-cad-suite-linux-x64-20260921.tgz
#     sha256 fc11a9c05c1de96b2821a5468ed02ba35253b278a78106cbe68167a5c419970c
#     (741984643 bytes)
#     api.github.com (used by fpgas.online-test-designs's own
#     scripts/setup_toolchains.py to find "latest") is not reachable from
#     this machine, so this script probes release tags directly with
#     `curl -sSIL` if the pinned one ever stops resolving -- see
#     find_oss_cad_suite_url() below.
#
# --------------------------------------------------------------------------
set -euo pipefail

FORCE=0
EXTRA_PARTS=()
for arg in "$@"; do
  case "$arg" in
    --force)
      FORCE=1
      ;;
    --parts)
      echo "setup-openxc7.sh: --parts requires an argument (e.g. --parts xc7a100tcsg324-1)" >&2
      exit 2
      ;;
    --parts=*)
      IFS=',' read -r -a _p <<<"${arg#--parts=}"
      EXTRA_PARTS+=("${_p[@]}")
      ;;
    -h | --help)
      sed -n '2,55p;58,76p' "$0" | sed -e 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "setup-openxc7.sh: unknown argument: $arg" >&2
      echo "usage: $0 [--force] [--parts DEVICE[,DEVICE...]]" >&2
      exit 2
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="$SCRIPT_DIR/build"
DL_DIR="$BUILD_DIR/downloads"
LOG_DIR="$BUILD_DIR/logs"
OPENXC7_DIR="$BUILD_DIR/openxc7"
OPENXC7_ROOT="$OPENXC7_DIR/root"
OPENXC7_BIN="$OPENXC7_DIR/bin"
CHIPDB_DIR="$OPENXC7_DIR/chipdb"
OSS_DIR="$BUILD_DIR/oss-cad-suite"
STATUS_FILE="$OPENXC7_DIR/status.json"
MARKER="$OPENXC7_DIR/.setup-ok"

OPENXC7_SNAP_VERSION="0.8.2"
OPENXC7_SNAP_URL="https://github.com/openXC7/openXC7-snap/releases/download/${OPENXC7_SNAP_VERSION}/openxc7_${OPENXC7_SNAP_VERSION}_amd64.snap"
OPENXC7_SNAP_SHA256="6b2e07ce99ef33d3a4e41e2fd2eb916f26bb0ece34a97216ed840a0032e98587"
OPENXC7_SNAP_FILE="$DL_DIR/openxc7_${OPENXC7_SNAP_VERSION}_amd64.snap"

OSS_CAD_SUITE_TAG="2026-09-21"
OSS_CAD_SUITE_DATE_COMPACT="20260921"
OSS_CAD_SUITE_URL="https://github.com/YosysHQ/oss-cad-suite-build/releases/download/${OSS_CAD_SUITE_TAG}/oss-cad-suite-linux-x64-${OSS_CAD_SUITE_DATE_COMPACT}.tgz"
OSS_CAD_SUITE_SHA256="fc11a9c05c1de96b2821a5468ed02ba35253b278a78106cbe68167a5c419970c"
OSS_CAD_SUITE_FILE="$DL_DIR/oss-cad-suite-linux-x64-${OSS_CAD_SUITE_DATE_COMPACT}.tgz"

DEFAULT_PART="xc7a35tcsg324-1"
ALL_PARTS=("$DEFAULT_PART")
for p in "${EXTRA_PARTS[@]}"; do
  if [[ "$p" != "$DEFAULT_PART" ]]; then
    ALL_PARTS+=("$p")
  fi
done

log() {
  echo "[setup-openxc7] $*"
}

sha256_of() {
  sha256sum "$1" | awk '{print $1}'
}

# download URL DEST EXPECTED_SHA256 DESCRIPTION
download() {
  local url="$1" dest="$2" expected="$3" desc="$4"
  mkdir -p "$(dirname "$dest")"
  if [[ -f "$dest" ]]; then
    local have
    have="$(sha256_of "$dest")"
    if [[ "$have" == "$expected" ]]; then
      log "$desc already downloaded and verified: $dest"
      return 0
    fi
    log "$desc cached file has the wrong sha256 ($have != $expected); re-downloading"
    rm -f "$dest"
  fi
  log "downloading $desc"
  log "  URL: $url"
  local tmp="$dest.part"
  rm -f "$tmp"
  curl -fSL --retry 3 --retry-delay 5 -o "$tmp" "$url"
  local got
  got="$(sha256_of "$tmp")"
  if [[ "$got" != "$expected" ]]; then
    rm -f "$tmp"
    echo "setup-openxc7.sh: ERROR: $desc: sha256 mismatch (got $got, expected $expected)" >&2
    exit 1
  fi
  mv "$tmp" "$dest"
  log "$desc downloaded and verified: $dest"
}

# find_oss_cad_suite_url: returns "URL SHA256_OR_EMPTY" on stdout. Uses the
# pinned tag if it still resolves (verified with curl -sSIL, per the T7.1
# brief); otherwise probes release tags going backwards from today so the
# script keeps working even if oss-cad-suite-build's release retention
# ever drops 2026-09-21 (api.github.com, which would otherwise answer
# "latest", is not reachable from this machine -- see header comment).
find_oss_cad_suite_url() {
  local code
  code="$(curl -sS -o /dev/null -w '%{http_code}' --max-time 20 -I -L "$OSS_CAD_SUITE_URL" || true)"
  if [[ "$code" == "200" ]]; then
    echo "$OSS_CAD_SUITE_URL $OSS_CAD_SUITE_SHA256"
    return 0
  fi
  log "WARNING: pinned OSS CAD Suite tag $OSS_CAD_SUITE_TAG returned HTTP $code; probing nearby dates"
  local today_epoch delta date_s compact url
  today_epoch="$(date -u +%s)"
  for delta in 0 1 2 3 4 5 6 7 10 14 21 30 45 60 90; do
    date_s="$(date -u -d "@$((today_epoch - delta * 86400))" +%Y-%m-%d 2>/dev/null || true)"
    [[ -z "$date_s" ]] && continue
    compact="${date_s//-/}"
    url="https://github.com/YosysHQ/oss-cad-suite-build/releases/download/${date_s}/oss-cad-suite-linux-x64-${compact}.tgz"
    code="$(curl -sS -o /dev/null -w '%{http_code}' --max-time 20 -I -L "$url" || true)"
    if [[ "$code" == "200" ]]; then
      log "found a working OSS CAD Suite release: $date_s"
      # No pinned sha256 for a probed date: the caller downloads it and
      # records whatever sha256 it actually got in status.json instead of
      # verifying against a constant.
      echo "$url -"
      return 0
    fi
  done
  echo "setup-openxc7.sh: ERROR: could not find any working OSS CAD Suite release (tried $OSS_CAD_SUITE_TAG and a range of nearby dates)" >&2
  exit 1
}

# ---------------------------------------------------------------------

if [[ "$FORCE" -eq 0 && -f "$MARKER" && -f "$STATUS_FILE" ]]; then
  RECORDED_PARTS="$(python3 -c '
import json, sys
try:
    s = json.load(open(sys.argv[1]))
    print(" ".join(sorted(s.get("chipdb", {}).keys())))
except Exception:
    print("")
' "$STATUS_FILE" 2>/dev/null || true)"
  WANTED_PARTS="$(printf '%s\n' "${ALL_PARTS[@]}" | sort | tr '\n' ' ' | sed -e 's/ $//')"
  MISSING=0
  for p in "${ALL_PARTS[@]}"; do
    case " $RECORDED_PARTS " in
      *" $p "*) ;;
      *) MISSING=1 ;;
    esac
  done
  if [[ "$MISSING" -eq 0 ]]; then
    log "already set up at $OPENXC7_DIR (pass --force to rebuild, or --parts to add chip databases)"
    cat "$STATUS_FILE"
    exit 0
  fi
  log "requested parts ($WANTED_PARTS) are not all built yet; building the missing ones"
fi

if [[ "$FORCE" -eq 1 ]]; then
  log "--force: removing $OPENXC7_DIR and $OSS_DIR (keeping $DL_DIR cache)"
  rm -rf "$OPENXC7_DIR" "$OSS_DIR"
fi

mkdir -p "$DL_DIR" "$LOG_DIR" "$OPENXC7_DIR" "$OPENXC7_BIN" "$CHIPDB_DIR"

SETUP_START=$(date +%s)

# --- 0. System prerequisites (best effort; apt-get for small deps only). --
NEEDED_APT_PKGS=()
command -v unsquashfs >/dev/null 2>&1 || NEEDED_APT_PKGS+=(squashfs-tools)
command -v patchelf >/dev/null 2>&1 || NEEDED_APT_PKGS+=(patchelf)
if [[ "${#NEEDED_APT_PKGS[@]}" -gt 0 ]]; then
  log "installing system packages: ${NEEDED_APT_PKGS[*]}"
  if command -v apt-get >/dev/null 2>&1; then
    APT_CMD=(apt-get)
    if [[ "$(id -u)" -ne 0 ]] && command -v sudo >/dev/null 2>&1; then
      APT_CMD=(sudo -n apt-get)
    fi
    "${APT_CMD[@]}" update -qq > "$LOG_DIR/apt.log" 2>&1 || true
    "${APT_CMD[@]}" install -y -qq "${NEEDED_APT_PKGS[@]}" >> "$LOG_DIR/apt.log" 2>&1 \
      || { echo "setup-openxc7.sh: ERROR: apt-get install ${NEEDED_APT_PKGS[*]} failed; see $LOG_DIR/apt.log" >&2; exit 1; }
  else
    echo "setup-openxc7.sh: ERROR: missing ${NEEDED_APT_PKGS[*]} and no apt-get available" >&2
    exit 1
  fi
fi

# --- 1. Download and extract the openXC7 snap (squashfs image). ---------
if [[ ! -d "$OPENXC7_ROOT" ]] || [[ ! -x "$OPENXC7_ROOT/usr/bin/nextpnr-xilinx" ]]; then
  download "$OPENXC7_SNAP_URL" "$OPENXC7_SNAP_FILE" "$OPENXC7_SNAP_SHA256" "openXC7 snap $OPENXC7_SNAP_VERSION"
  log "extracting the snap (squashfs) with unsquashfs -- this is NOT installed"
  log "as an actual snap (no snapd involved); it is just a squashfs image"
  rm -rf "$OPENXC7_ROOT"
  unsquashfs -f -d "$OPENXC7_ROOT" "$OPENXC7_SNAP_FILE" > "$LOG_DIR/unsquashfs.log" 2>&1
else
  log "openXC7 snap already extracted at $OPENXC7_ROOT"
fi

# --- 2. Make the extracted ELF binaries runnable without snapd/core20. --
#
# The snap's base is core20 (classic confinement): its binaries carry
# `/snap/core20/current/lib64/ld-linux-x86-64.so.2` as their ELF
# interpreter (PT_INTERP). That path does not exist on this machine (no
# snapd, no core20 snap installed) so the kernel refuses to exec them
# ("cannot execute: required file not found"). `ldd` on them still
# resolves every OTHER shared library fine because they carry a $ORIGIN
# relative RPATH into the snap's own usr/lib/x86_64-linux-gnu -- only the
# interpreter path itself is the problem. Fix: patchelf --set-interpreter
# to the host's own (glibc-compatible; both are recent Ubuntu/glibc)
# dynamic linker. This avoids downloading core20 (another ~100+ MB snap)
# entirely. Verified against Ubuntu 24.04 glibc 2.39; nextpnr-xilinx
# --test, xc7frames2bit --help, bbasm --help and the full yosys ->
# nextpnr-xilinx -> fasm2frames -> xc7frames2bit pipeline all work after
# this patch (see tools/e2e/README.md, "How the snap was made runnable").
HOST_INTERP="/lib64/ld-linux-x86-64.so.2"
if [[ ! -e "$HOST_INTERP" ]]; then
  echo "setup-openxc7.sh: ERROR: host dynamic linker $HOST_INTERP not found" >&2
  exit 1
fi
PATCHED_BINS=()
for f in "$OPENXC7_ROOT"/usr/bin/*; do
  [[ -f "$f" && -x "$f" ]] || continue
  interp="$(patchelf --print-interpreter "$f" 2>/dev/null || true)"
  case "$interp" in
    /snap/*)
      patchelf --set-interpreter "$HOST_INTERP" "$f"
      PATCHED_BINS+=("$(basename "$f")")
      ;;
  esac
done
log "patched ELF interpreter (core20 -> host) on: ${PATCHED_BINS[*]}"

# --- 3. Rewrite the snap's own python wrapper scripts (fasm2frames, ------
# bit2fasm, fasm) so they work standalone: their shebang line AND their
# sys.path.append() calls are hardcoded to /snap/openxc7/current/... (the
# path snapd would have mounted this at). A single substitution of that
# prefix for $OPENXC7_ROOT fixes both -- the shebang line itself starts
# with "/snap/openxc7/current/usr/bin/python3.8".
mkdir -p "$OPENXC7_BIN"
for w in fasm2frames bit2fasm fasm; do
  src="$OPENXC7_ROOT/bin/$w"
  dst="$OPENXC7_BIN/$w"
  if [[ -f "$src" ]]; then
    sed -e "s#/snap/openxc7/current#$OPENXC7_ROOT#g" "$src" > "$dst"
    chmod +x "$dst"
  fi
done
# Symlink the plain ELF tools (already runnable after step 2) into the
# same overlay bin/ directory so openxc7-env.sh only needs one PATH entry.
for t in nextpnr-xilinx bbasm xc7frames2bit bitread bittool \
  frame_address_decoder gen_part_base_yaml xc7patch segmatch; do
  if [[ -x "$OPENXC7_ROOT/usr/bin/$t" ]]; then
    ln -sf "$OPENXC7_ROOT/usr/bin/$t" "$OPENXC7_BIN/$t"
  fi
done
log "overlay bin/ ready: $OPENXC7_BIN"

# --- 4. Inventory: what's bundled in the snap. ---------------------------
# (checked purely inside the pristine extracted snap, independent of
# anything this script itself has added, so this is correct on every run)
HAS_YOSYS=0
if find "$OPENXC7_ROOT" -maxdepth 4 -type f -iname yosys 2>/dev/null | grep -q .; then
  HAS_YOSYS=1
fi
PRJXRAY_DB_DIR="$OPENXC7_ROOT/opt/nextpnr-xilinx/external/prjxray-db"
FAMILIES=()
if [[ -d "$PRJXRAY_DB_DIR" ]]; then
  for d in "$PRJXRAY_DB_DIR"/*/; do
    [[ -d "$d" ]] || continue
    fam="$(basename "$d")"
    case "$fam" in .git) continue ;; esac
    FAMILIES+=("$fam")
  done
fi
log "bundled prjxray-db families: ${FAMILIES[*]}"
log "snap includes yosys: $([[ "$HAS_YOSYS" -eq 1 ]] && echo yes || echo no)"

# --- 5. yosys: the snap does not bundle it (see meta/snap.yaml -------
# description: "This package does not include Yosys, which needs to be
# installed separately."); get it from OSS CAD Suite.
if [[ "$HAS_YOSYS" -eq 0 ]]; then
  if [[ ! -x "$OSS_DIR/oss-cad-suite/bin/yosys" ]]; then
    read -r OSS_URL OSS_SHA256 <<<"$(find_oss_cad_suite_url)"
    OSS_FILE="$DL_DIR/$(basename "$OSS_URL")"
    if [[ "$OSS_SHA256" == "-" ]]; then
      log "downloading OSS CAD Suite from a probed (unpinned) URL: $OSS_URL"
      mkdir -p "$DL_DIR"
      curl -fSL --retry 3 --retry-delay 5 -o "$OSS_FILE" "$OSS_URL"
      OSS_SHA256="$(sha256_of "$OSS_FILE")"
      log "recorded sha256 for this run: $OSS_SHA256"
    else
      download "$OSS_URL" "$OSS_FILE" "$OSS_SHA256" "OSS CAD Suite ($OSS_CAD_SUITE_TAG)"
    fi
    log "extracting OSS CAD Suite (this is ~2.4 GiB uncompressed; only yosys and its direct needs are used, but nothing is pruned so sby/nextpnr-* stay available for later tasks)"
    mkdir -p "$OSS_DIR"
    tar xzf "$OSS_FILE" -C "$OSS_DIR"
    OSS_RESOLVED_URL="$OSS_URL"
    OSS_RESOLVED_SHA256="$OSS_SHA256"
  else
    log "OSS CAD Suite already extracted at $OSS_DIR"
    OSS_RESOLVED_URL="$OSS_CAD_SUITE_URL"
    OSS_RESOLVED_SHA256="$OSS_CAD_SUITE_SHA256"
  fi
  # NOT symlinked into $OPENXC7_BIN: oss-cad-suite/bin/yosys is itself a
  # bash wrapper that derives its library directory from
  # `dirname "${BASH_SOURCE[0]}"` -- when invoked through a symlink living
  # in a different directory, BASH_SOURCE is the symlink's own path, so
  # the wrapper computes the wrong lib dir and fails
  # ("...: No such file or directory"). openxc7-env.sh instead adds
  # $OSS_DIR/oss-cad-suite/bin to PATH directly, ahead of $OPENXC7_BIN.
  YOSYS_BIN="$OSS_DIR/oss-cad-suite/bin/yosys"
else
  YOSYS_BIN=""
  OSS_RESOLVED_URL=""
  OSS_RESOLVED_SHA256=""
fi

# --- 6. Chip databases (bbaexport.py -> .bba text -> bbasm -> .bin). -----
#
# The snap ships NO prebuilt chipdb .bin files at all (only the prjxray-db
# + nextpnr-xilinx-meta source data + the bbaexport.py/bbasm tools to
# build one per device on demand) -- see tools/e2e/README.md ("Chip
# database sizes and timings") for measured bba/bin sizes and build time
# per part.
BBAEXPORT="$OPENXC7_ROOT/opt/nextpnr-xilinx/python/bbaexport.py"
PYTHON38="$OPENXC7_ROOT/usr/bin/python3.8"
BUILT_CHIPDBS=()
for device in "${ALL_PARTS[@]}"; do
  bin_out="$CHIPDB_DIR/$device.bin"
  if [[ -f "$bin_out" ]]; then
    log "chipdb for $device already built: $bin_out"
    BUILT_CHIPDBS+=("$device")
    continue
  fi
  bba_out="$CHIPDB_DIR/$device.bba"
  log "building chipdb for $device (bbaexport.py -> bbasm)"
  t0=$(date +%s)
  "$PYTHON38" "$BBAEXPORT" --device "$device" --bba "$bba_out" \
    > "$LOG_DIR/bbaexport-$device.log" 2>&1
  "$OPENXC7_BIN/bbasm" --l "$bba_out" "$bin_out" \
    > "$LOG_DIR/bbasm-$device.log" 2>&1
  t1=$(date +%s)
  rm -f "$bba_out"
  log "chipdb for $device built in $((t1 - t0))s: $bin_out ($(du -h "$bin_out" | cut -f1))"
  BUILT_CHIPDBS+=("$device")
done

SETUP_END=$(date +%s)

# --- 7. status.json -------------------------------------------------------
STATUS_PY="
import json, os, subprocess

def du(path):
    if not os.path.exists(path):
        return None
    out = subprocess.run(['du', '-sh', path], capture_output=True, text=True).stdout
    return out.split()[0] if out else None

def ver(cmd):
    # Some of these tools print their version banner to stderr, not
    # stdout (nextpnr-xilinx does); check both.
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=20)
        lines = (r.stdout + r.stderr).strip().splitlines()
        return lines[0] if lines else '<no output>'
    except Exception as e:
        return f'<error: {e}>'

def split_env_list(name):
    return [x for x in os.environ.get(name, '').split(chr(31)) if x]

chipdb = {}
for d in split_env_list('BUILT_CHIPDBS_JSON'):
    p = os.path.join(os.environ['CHIPDB_DIR'], d + '.bin')
    chipdb[d] = {'path': p, 'bytes': os.path.getsize(p) if os.path.exists(p) else None}

status = {
    'openxc7_snap_version': os.environ['OPENXC7_SNAP_VERSION'],
    'openxc7_snap_url': os.environ['OPENXC7_SNAP_URL'],
    'openxc7_snap_sha256': os.environ['OPENXC7_SNAP_SHA256'],
    'openxc7_root': os.environ['OPENXC7_ROOT'],
    'openxc7_bin_overlay': os.environ['OPENXC7_BIN'],
    'elf_interpreter_patched_from': '/snap/core20/current/lib64/ld-linux-x86-64.so.2',
    'elf_interpreter_patched_to': os.environ['HOST_INTERP'],
    'patched_binaries': split_env_list('PATCHED_BINS_JSON'),
    'bundled_prjxray_db_families': split_env_list('FAMILIES_JSON'),
    'prjxray_db_dir': os.environ['PRJXRAY_DB_DIR'],
    'snap_includes_yosys': bool(int(os.environ['HAS_YOSYS'])),
    'oss_cad_suite_url': os.environ.get('OSS_RESOLVED_URL', ''),
    'oss_cad_suite_sha256': os.environ.get('OSS_RESOLVED_SHA256', ''),
    'oss_cad_suite_dir': os.environ['OSS_DIR'] if os.environ.get('OSS_RESOLVED_URL') else None,
    'yosys_bin': os.environ.get('YOSYS_BIN', ''),
    'yosys_version': ver([os.environ['YOSYS_BIN'], '-V']) if os.environ.get('YOSYS_BIN') and os.path.exists(os.environ['YOSYS_BIN']) else None,
    'nextpnr_xilinx_version': ver([os.path.join(os.environ['OPENXC7_BIN'], 'nextpnr-xilinx'), '--version']),
    'chipdb': chipdb,
    'disk_usage': {
        'openxc7': du(os.environ['OPENXC7_DIR']),
        'oss_cad_suite': du(os.environ['OSS_DIR']),
        'downloads': du(os.environ['DL_DIR']),
    },
    'setup_seconds': int(os.environ['SETUP_END']) - int(os.environ['SETUP_START']),
}
with open(os.environ['STATUS_FILE'], 'w') as f:
    json.dump(status, f, indent=2, sort_keys=True)
    f.write('\n')
"
PATCHED_BINS_JSON="$(printf '%s\37' "${PATCHED_BINS[@]}")"
FAMILIES_JSON="$(printf '%s\37' "${FAMILIES[@]}")"
BUILT_CHIPDBS_JSON="$(printf '%s\37' "${BUILT_CHIPDBS[@]}")"
OPENXC7_DIR="$OPENXC7_DIR" \
OPENXC7_SNAP_VERSION="$OPENXC7_SNAP_VERSION" OPENXC7_SNAP_URL="$OPENXC7_SNAP_URL" \
OPENXC7_SNAP_SHA256="$OPENXC7_SNAP_SHA256" OPENXC7_ROOT="$OPENXC7_ROOT" \
OPENXC7_BIN="$OPENXC7_BIN" HOST_INTERP="$HOST_INTERP" \
PATCHED_BINS_JSON="$PATCHED_BINS_JSON" FAMILIES_JSON="$FAMILIES_JSON" \
PRJXRAY_DB_DIR="$PRJXRAY_DB_DIR" HAS_YOSYS="$HAS_YOSYS" \
OSS_RESOLVED_URL="${OSS_RESOLVED_URL:-}" OSS_RESOLVED_SHA256="${OSS_RESOLVED_SHA256:-}" \
OSS_DIR="$OSS_DIR" CHIPDB_DIR="$CHIPDB_DIR" BUILT_CHIPDBS_JSON="$BUILT_CHIPDBS_JSON" \
YOSYS_BIN="${YOSYS_BIN:-}" \
DL_DIR="$DL_DIR" SETUP_START="$SETUP_START" SETUP_END="$SETUP_END" STATUS_FILE="$STATUS_FILE" \
python3 -c "$STATUS_PY"

touch "$MARKER"
log "openXC7 toolchain setup complete in $((SETUP_END - SETUP_START))s"
cat "$STATUS_FILE"

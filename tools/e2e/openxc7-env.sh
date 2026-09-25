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
# Source this (do not execute it) to put the openXC7 end-to-end toolchain
# (T7.1) on PATH and export a couple of convenience variables:
#
#   source tools/e2e/openxc7-env.sh
#
# Requires tools/e2e/setup-openxc7.sh to have been run first.
#
# OPENXC7_E2E_BUILD (optional) names the build directory to use instead of
# this script's own tools/e2e/build, e.g. the main checkout's from a second
# working tree that has no toolchain of its own:
#
#   OPENXC7_E2E_BUILD=/home/user/fasm/tools/e2e/build source tools/e2e/openxc7-env.sh
#
# Exports:
#   PATH               adds, in order:
#                        1. tools/e2e/build/oss-cad-suite/oss-cad-suite/bin
#                           (yosys and the rest of OSS CAD Suite, if it was
#                           installed -- kept as a real directory on PATH,
#                           not symlinked into (2), because yosys's own
#                           launcher script derives its library directory
#                           from its symlink-resolved location and breaks
#                           if invoked through a symlink elsewhere);
#                        2. tools/e2e/build/openxc7/bin: nextpnr-xilinx,
#                           bbasm, xc7frames2bit, bitread, bittool,
#                           frame_address_decoder, gen_part_base_yaml,
#                           xc7patch, segmatch (symlinks into the patched
#                           snap), and fasm2frames / bit2fasm / fasm
#                           (rewritten standalone copies of the snap's
#                           python wrapper scripts -- see
#                           tools/e2e/setup-openxc7.sh step 3).
#   OPENXC7_ROOT        tools/e2e/build/openxc7/root -- the extracted snap
#   PRJXRAY_DB_DIR      the prjxray-db copy bundled INSIDE the snap
#                       ($OPENXC7_ROOT/opt/nextpnr-xilinx/external/
#                       prjxray-db); has artix7, kintex7, spartan7, zynq7.
#                       Pass "$PRJXRAY_DB_DIR/<family>" as --db-root /
#                       --xray to fasm2frames / bbaexport.py.
#   CHIPDB_DIR          tools/e2e/build/openxc7/chipdb -- nextpnr-xilinx
#                       chip database .bin files built by
#                       setup-openxc7.sh, one per device name (e.g.
#                       xc7a35tcsg324-1.bin). Pass
#                       "$CHIPDB_DIR/<device>.bin" as --chipdb to
#                       nextpnr-xilinx.
#   OPENXC7_PYTHON3     the snap's own python3.8 interpreter (has the
#                       pinned fasm/prjxray/textx it was built with); use
#                       this to run opt/nextpnr-xilinx/python/bbaexport.py
#                       directly for a device not already in $CHIPDB_DIR.

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  echo "openxc7-env.sh: this must be sourced, not executed:" >&2
  echo "  source ${BASH_SOURCE[0]}" >&2
  exit 1
fi

_OPENXC7_ENV_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_OPENXC7_ENV_BUILD="${OPENXC7_E2E_BUILD:-$_OPENXC7_ENV_SCRIPT_DIR/build}"
_OPENXC7_ENV_DIR="$_OPENXC7_ENV_BUILD/openxc7"

if [[ ! -d "$_OPENXC7_ENV_DIR/bin" ]]; then
  echo "openxc7-env.sh: $_OPENXC7_ENV_DIR/bin not found;" >&2
  echo "  run tools/e2e/setup-openxc7.sh first" >&2
  return 1 2>/dev/null || exit 1
fi

_OPENXC7_ENV_OSS_BIN="$_OPENXC7_ENV_BUILD/oss-cad-suite/oss-cad-suite/bin"
if [[ -d "$_OPENXC7_ENV_OSS_BIN" ]]; then
  export PATH="$_OPENXC7_ENV_OSS_BIN:$_OPENXC7_ENV_DIR/bin:$PATH"
else
  export PATH="$_OPENXC7_ENV_DIR/bin:$PATH"
fi
export OPENXC7_ROOT="$_OPENXC7_ENV_DIR/root"
export PRJXRAY_DB_DIR="$OPENXC7_ROOT/opt/nextpnr-xilinx/external/prjxray-db"
export CHIPDB_DIR="$_OPENXC7_ENV_DIR/chipdb"
export OPENXC7_PYTHON3="$OPENXC7_ROOT/usr/bin/python3.8"

unset _OPENXC7_ENV_SCRIPT_DIR _OPENXC7_ENV_BUILD _OPENXC7_ENV_DIR _OPENXC7_ENV_OSS_BIN

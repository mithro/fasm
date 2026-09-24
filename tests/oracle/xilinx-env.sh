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
# Source this (do not execute it) to put the Xilinx oracle's C++ tools on
# PATH and export a couple of convenience variables:
#
#   source tests/oracle/xilinx-env.sh
#
# This matters for f4pga-xc-fasm's `xcfasm`, which shells out to
# `xc7frames2bit` *by bare name* (see xc_fasm/xc_fasm.py:74-78,
# `subprocess.check_output(..., shell=True)` with `--frm2bit` defaulting to
# the literal string "xc7frames2bit") rather than an absolute path -- it
# has to be resolvable on PATH for `xcfasm-oracle` to work.
#
# Requires tests/oracle/setup-xilinx.sh to have been run first.
#
# Exports:
#   PATH                adds tests/oracle/build/xilinx/bin (xc7frames2bit,
#                        bitread, frame_address_decoder, gen_part_base_yaml,
#                        bittool, xc7patch, and the prjuray-tools uray-*
#                        equivalents, if that best-effort build succeeded)
#   PRJXRAY_DB_ROOT      the root directory fetched databases live under
#                         (${FASM_DB_CACHE:-tests/oracle/build/db}/prjxray-db);
#                         combine with a family name for --db-root, e.g.
#                         "$PRJXRAY_DB_ROOT/artix7". Not read directly by
#                         any reference tool (they take --db-root/--part
#                         explicitly, or XRAY_DATABASE_DIR+XRAY_DATABASE /
#                         XRAY_PART -- see docs/rewrite/DESIGN-xilinx-db.md
#                         section 2.1); this is only this repo's own
#                         convenience variable for finding fetched db's.
#   PRJURAY_DB_ROOT      same, for prjuray-db
#                         (${FASM_DB_CACHE:-tests/oracle/build/db}/prjuray-db)

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  echo "xilinx-env.sh: this must be sourced, not executed:" >&2
  echo "  source ${BASH_SOURCE[0]}" >&2
  exit 1
fi

_XILINX_ENV_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_XILINX_ENV_REPO_ROOT="$(cd "$_XILINX_ENV_SCRIPT_DIR/../.." && pwd)"

export PATH="$_XILINX_ENV_SCRIPT_DIR/build/xilinx/bin:$PATH"

_XILINX_ENV_DB_CACHE="${FASM_DB_CACHE:-$_XILINX_ENV_SCRIPT_DIR/build/db}"
export PRJXRAY_DB_ROOT="$_XILINX_ENV_DB_CACHE/prjxray-db"
export PRJURAY_DB_ROOT="$_XILINX_ENV_DB_CACHE/prjuray-db"

unset _XILINX_ENV_SCRIPT_DIR _XILINX_ENV_REPO_ROOT _XILINX_ENV_DB_CACHE

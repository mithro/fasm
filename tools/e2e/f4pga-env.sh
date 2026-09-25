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
# Source this (do not execute it) to use the f4pga toolchain installed by
# tools/e2e/setup-f4pga.sh (T7.3); the equivalent of f4pga-examples'
# "conda activate xc7" with F4PGA_INSTALL_DIR/FPGA_FAM set
# (docs/building-examples.rst):
#
#   source tools/e2e/f4pga-env.sh
#
# Exports:
#   F4PGA_INSTALL_DIR   $F4PGA_E2E_ROOT (default tools/e2e/build/f4pga)
#   FPGA_FAM            xc7
#   F4PGA_ENV           the conda environment ($F4PGA_INSTALL_DIR/xc7/conda/envs/xc7)
#   F4PGA_PRJXRAY_DB    the flow's prjxray-db (conda package prjxray-db
#                       0.0_257_g0a0adde, what `prjxray-config` prints and
#                       symbiflow_write_bitstream uses)
#   PATH                $F4PGA_ENV/bin first (yosys, vpr, genfasm, xcfasm,
#                       xc7frames2bit, bitread, f4pga, symbiflow_*, ...)
#   CONDA_PREFIX        $F4PGA_ENV (some wrappers look for it)

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  echo "f4pga-env.sh: source this file, do not execute it" >&2
  exit 1
fi

_f4pga_repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export F4PGA_INSTALL_DIR="${F4PGA_E2E_ROOT:-$_f4pga_repo/tools/e2e/build/f4pga}"
export FPGA_FAM=xc7
export F4PGA_ENV="$F4PGA_INSTALL_DIR/xc7/conda/envs/xc7"
export F4PGA_PRJXRAY_DB="$F4PGA_ENV/share/symbiflow/prjxray-db"
export CONDA_PREFIX="$F4PGA_ENV"
if [[ ! -x "$F4PGA_ENV/bin/vpr" ]]; then
  echo "f4pga-env.sh: $F4PGA_ENV not set up (tools/e2e/setup-f4pga.sh)" >&2
else
  case ":$PATH:" in
    *":$F4PGA_ENV/bin:"*) ;;
    *) export PATH="$F4PGA_ENV/bin:$PATH" ;;
  esac
fi
unset _f4pga_repo

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
# Runs the RapidWright cross-checks of T7.5 (tools/e2e/rapidwright/
# rwcheck.py; see docs/rewrite/DESIGN-rapidwright.md and
# docs/rewrite/DESIGN-xilinx-db.md §8.15):
#
#   layout  RapidWright's configuration array of every database part
#           against the Rust part walk and part.json;
#   bits    every reference bitstream (Vivado ones of prjxray-db, prjxray
#           and prjuray-tools; the corpus; f4pga / openXC7 flow outputs
#           found under $RAPIDWRIGHT_FLOW_OUT, colon separated, default
#           tools/e2e/build/out) and every corpus .frm through the
#           RapidWright and the Rust readers and writers;
#   fasm    (only with setup-rapidwright.sh --with-interchange and the
#           reference tools of tests/oracle/setup-xilinx.sh) FASM of
#           RapidWright-built designs through the Rust and the reference
#           fasm2frames.
#
# Usage:
#   tools/e2e/run-rapidwright-checks.sh [--quick] [CHECK...] [rwcheck.py options]
#
# Needs: tools/e2e/setup-rapidwright.sh, `cargo build --release`, the
# fetched databases ($FASM_DB_CACHE, default tests/oracle/build/db) and,
# for the Vivado test bitstreams, the reference sources of
# tests/oracle/setup-xilinx.sh ($FASM_ORACLE_SRC). The work directory
# (tools/e2e/build/rapidwright-work) holds at most a few hundred MB at a
# time; the report is tools/e2e/build/rapidwright-work/report.json.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="${RAPIDWRIGHT_WORK:-$REPO_ROOT/tools/e2e/build/rapidwright-work}"
mkdir -p "$WORK"

if [ ! -f "$REPO_ROOT/tools/e2e/build/rapidwright/classes/RwCheck.class" ]; then
    echo "run-rapidwright-checks: run tools/e2e/setup-rapidwright.sh first" >&2
    exit 2
fi

FLOW_ARGS=()
IFS=':' read -r -a FLOW_DIRS <<<"${RAPIDWRIGHT_FLOW_OUT:-$REPO_ROOT/tools/e2e/build/out}"
for d in "${FLOW_DIRS[@]}"; do
    if [ -n "$d" ] && [ -d "$d" ]; then
        FLOW_ARGS+=(--flow-out "$d")
    fi
done

exec timeout 14400 python3 "$REPO_ROOT/tools/e2e/rapidwright/rwcheck.py" \
    --work-dir "$WORK" --report "$WORK/report.json" \
    "${FLOW_ARGS[@]}" "$@"

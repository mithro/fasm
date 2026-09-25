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
# Checks that genfasm finished writing the FASM of an f4pga build:
#
#   tools/e2e/f4pga/check-genfasm.sh BUILD_DIR [BUILD_LOG]
#
# Exit status 0 if it did; otherwise 1 and the reason on stdout.
#
# Needed because the flow does not check it: symbiflow_write_fasm (the
# make and LiteX flows) runs genfasm in `/bin/bash -c` followed by more
# commands and without `set -e`, so a genfasm killed by a signal
# (Killed, Bus error, Terminated, ...) or failing with an error still
# "succeeds", with a truncated FASM, and the bitstream is written from it.
#
# genfasm (like VPR) writes its log to vpr_stdout.log in the build
# directory as it goes, and ends a successful run with
#
#   Writing Implementation FASM: <top>.fasm
#   The entire flow of VPR took <N> seconds.
#
# symbiflow_write_fasm renames it to fasm.log afterwards (only when bash
# returns); `f4pga build` (counter_test/arty_35) leaves it as
# vpr_stdout.log, genfasm being the last VPR tool it runs. So the log is
# BUILD_DIR/fasm.log, else BUILD_DIR/vpr_stdout.log, and it must end with
# those two lines. Additionally BUILD_LOG (the captured output of the
# build) must not hold bash's report of a signal killing genfasm
# ("line N: PID <signal> ... genfasm ...").
#
# tests/e2e/test_f4pga_examples.py runs this on fake genfasm runs (killed
# by SIGKILL, SIGBUS, SIGTERM, SIGSEGV, exiting non-zero after an error,
# and succeeding) run through `/bin/bash -c` the way the flow runs them.

dir="$1"
build_log="${2:-}"
if [[ -z "$dir" ]]; then
  echo "usage: $0 BUILD_DIR [BUILD_LOG]" >&2
  exit 2
fi
log="$dir/fasm.log"
[[ -f "$log" ]] || log="$dir/vpr_stdout.log"
if [[ ! -f "$log" ]]; then
  echo "genfasm log not found (no fasm.log or vpr_stdout.log in $dir)"
  exit 1
fi
if [[ -n "$build_log" && -f "$build_log" ]]; then
  report=$(grep -E 'line [0-9]+: +[0-9]+ .*genfasm' "$build_log" | head -1)
  if [[ -n "$report" ]]; then
    echo "genfasm killed: $(sed -E 's/^.*line [0-9]+: +[0-9]+ +([A-Za-z][A-Za-z ()-]*[a-z)]) .*$/\1/' <<<"$report")"
    exit 1
  fi
fi
last=$(grep -v '^[[:space:]]*$' "$log" | tail -n 2)
if ! grep -qE '^Writing Implementation FASM: ' <<<"$last" \
   || ! grep -qE '^The entire flow of VPR took [0-9.e+-]+ seconds' <<<"$(tail -n 1 <<<"$last")"; then
  echo "genfasm did not finish ($(basename "$log") ends with: $(tail -n 1 <<<"$last" | cut -c1-200))"
  exit 1
fi
exit 0

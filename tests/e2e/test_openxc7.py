#!/usr/bin/env python3
# -*- coding: utf-8 -*-
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
"""Smoke tests for the T7.1 end-to-end toolchain (tools/e2e/setup-openxc7.sh
and tools/e2e/run-counter.sh). Skips cleanly when the toolchain has not
been set up (or, for the FASM parse check, when the plain FASM oracle from
tests/oracle/setup.sh has not been set up), so this is safe to include in
the normal `pytest tests/` run on a machine that has not run either setup
script.

Does not touch the Rust rewrite; it only checks that the open source
Xilinx synthesis + place-and-route flow this task installs actually works
and that the FASM it produces (checked into the corpus) is valid FASM.
"""
import subprocess
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
E2E_DIR = REPO_ROOT / 'tools' / 'e2e'
OPENXC7_DIR = E2E_DIR / 'build' / 'openxc7'
OPENXC7_MARKER = OPENXC7_DIR / '.setup-ok'
OPENXC7_BIN = OPENXC7_DIR / 'bin'
OSS_CAD_SUITE_BIN = E2E_DIR / 'build' / 'oss-cad-suite' / 'oss-cad-suite' / 'bin'

ORACLE_PYTHON = REPO_ROOT / 'tests' / 'oracle' / 'venv' / 'bin' / 'python'
ORACLE_DUMP = REPO_ROOT / 'tests' / 'oracle' / 'dump.py'

CORPUS_FASM = (
    REPO_ROOT / 'tests' / 'corpus' / 'xilinx' / 'artix7' / 'designs' /
    'f4pga-examples' / 'counter_test' / 'arty_35' / 'top.fasm')

require_openxc7 = pytest.mark.skipif(
    not OPENXC7_MARKER.exists(),
    reason=(
        "tools/e2e/build/openxc7 not set up; "
        "run tools/e2e/setup-openxc7.sh first"))

require_oracle = pytest.mark.skipif(
    not ORACLE_PYTHON.exists(),
    reason=(
        "tests/oracle/venv not set up; run tests/oracle/setup.sh first"))


def _yosys_path():
    for candidate in (OSS_CAD_SUITE_BIN / 'yosys', OPENXC7_BIN / 'yosys'):
        if candidate.exists():
            return candidate
    return None


@require_openxc7
def test_yosys_runs():
    yosys = _yosys_path()
    assert yosys is not None, (
        "neither OSS CAD Suite nor the openXC7 snap provided a yosys "
        "binary; see tools/e2e/README.md")
    result = subprocess.run(
        [str(yosys), '-V'], capture_output=True, text=True, timeout=30)
    assert result.returncode == 0, result.stderr
    assert 'Yosys' in result.stdout


@require_openxc7
def test_nextpnr_xilinx_runs():
    nextpnr = OPENXC7_BIN / 'nextpnr-xilinx'
    assert nextpnr.exists(), (
        f"{nextpnr} not found; run tools/e2e/setup-openxc7.sh")
    result = subprocess.run(
        [str(nextpnr), '--version'], capture_output=True, text=True,
        timeout=30)
    assert result.returncode == 0, result.stderr
    # nextpnr-xilinx prints its version banner to stderr, not stdout.
    banner = result.stdout + result.stderr
    assert 'nextpnr-xilinx' in banner
    assert '0.8.2' in banner


@require_oracle
def test_counter_fasm_exists_and_is_substantial():
    assert CORPUS_FASM.exists(), (
        f"{CORPUS_FASM} not found; run tools/e2e/run-counter.sh and copy "
        "its top.fasm into the corpus (see that directory's README.md)")
    lines = CORPUS_FASM.read_text().splitlines()
    assert len(lines) >= 200, (
        f"expected a few hundred lines of FASM, got {len(lines)}")


@require_oracle
def test_counter_fasm_parses_with_original_oracle():
    assert CORPUS_FASM.exists(), (
        f"{CORPUS_FASM} not found; run tools/e2e/run-counter.sh first")
    result = subprocess.run(
        [str(ORACLE_PYTHON), str(ORACLE_DUMP), str(CORPUS_FASM)],
        capture_output=True, text=True, timeout=60)
    assert result.returncode == 0, (
        f"tests/oracle/dump.py failed on {CORPUS_FASM}:\n{result.stderr}")
    assert result.stdout.strip(), "dump.py produced no output"

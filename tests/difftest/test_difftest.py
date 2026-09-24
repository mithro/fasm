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
"""Pytest wrapper around `tools/difftest.py` (T1.5): runs the Rust vs.
oracle differential test over the committed corpus and asserts there are
zero UNEXPLAINED differences.

Skips cleanly (does not fail) when either prerequisite is missing, so a
plain `pytest` run in a checkout that has not built the Rust workspace or
set up the oracle venv is unaffected:

* the `fasm-dump` example binary
  (`cargo build --release --example dump -p fasm`);
* an oracle venv (`tests/oracle/setup.sh`, in this checkout or, like
  `tools/difftest.py` itself, the main checkout's).

Run directly with `pytest tests/difftest/test_difftest.py -v -s`, or via
`make difftest`.
"""
import os
import subprocess
import sys

import pytest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(REPO_ROOT, "tools"))

import importlib.util

_spec = importlib.util.spec_from_file_location(
    "difftest", os.path.join(REPO_ROOT, "tools", "difftest.py"))
difftest = importlib.util.module_from_spec(_spec)
# Register under its real name *before* exec'ing: `difftest.main()` uses a
# `multiprocessing.Pool`, which pickles `process_one` by qualified name
# ("difftest.process_one") and re-resolves it through `sys.modules` on the
# worker side; without this, the worker's plain `import difftest` (found
# via tools/ on sys.path below) would get a second, distinct module object
# and pickling would fail with "not the same object as difftest.process_one".
sys.modules["difftest"] = difftest
_spec.loader.exec_module(difftest)


def _prerequisites():
    rust_dump = difftest.DEFAULT_RUST_DUMP
    oracle_python = difftest.find_oracle_python(None)
    missing = []
    if not os.path.exists(rust_dump):
        missing.append(
            "fasm-dump not built (cargo build --release --example dump "
            "-p fasm) at {}".format(rust_dump))
    if oracle_python is None:
        missing.append(
            "no oracle venv found (tests/oracle/setup.sh) in {}".format(
                difftest.DEFAULT_ORACLE_PYTHON_CANDIDATES))
    return rust_dump, oracle_python, missing


def test_difftest_zero_unexplained(tmp_path):
    rust_dump, oracle_python, missing = _prerequisites()
    if missing:
        pytest.skip("; ".join(missing))

    report_path = str(tmp_path / "difftest-report.txt")
    argv = [
        "--jobs", str(min(4, os.cpu_count() or 1)),
        "--rust-dump", rust_dump,
        "--oracle-python", oracle_python,
        "--report", report_path,
    ]
    rc = difftest.main(argv)

    report = ""
    if os.path.exists(report_path):
        with open(report_path) as f:
            report = f.read()

    assert rc == difftest.EXIT_OK, (
        "tools/difftest.py found unexplained differences (exit code {}); "
        "full report at {}:\n\n{}".format(
            rc, report_path,
            "\n".join(line for line in report.splitlines()
                      if "[unexplained]" in line or "  " in line)[:8000]))


def test_difftest_cli_subprocess_smoke():
    """Also exercise `tools/difftest.py` as an actual subprocess (not just
    imported), over a small filtered slice of the corpus, the way
    `make difftest` and a human on the command line would run it."""
    rust_dump, oracle_python, missing = _prerequisites()
    if missing:
        pytest.skip("; ".join(missing))

    result = subprocess.run(
        [sys.executable, os.path.join(REPO_ROOT, "tools", "difftest.py"),
         "--jobs", "2",
         "--rust-dump", rust_dump,
         "--oracle-python", oracle_python,
         "--filter", "tests/corpus/fasm-examples/*"],
        capture_output=True, text=True)
    assert result.returncode == difftest.EXIT_OK, (
        result.stdout + result.stderr)

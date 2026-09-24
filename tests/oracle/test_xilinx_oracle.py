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
"""Smoke tests for the Xilinx oracle (tests/oracle/setup-xilinx.sh). Run
with the Xilinx oracle venv's *installed pytest entry point*, not
`python -m pytest` (see tests/oracle/README.md, "Avoiding the
current-directory shadowing pitfall" -- the same CWD-shadowing hazard
tests/oracle/test_oracle.py guards against applies here too):

    tests/oracle/venv-xilinx/bin/pytest tests/oracle/test_xilinx_oracle.py -v

Two independent checks:

(a) f4pga-xc-fasm's own test suite (tests/test_fasm2frames.py, using its
    miniature bundled database under tests/test_data/db) passes inside
    venv-xilinx, against the pinned prjxray/xc_fasm/fasm packages -- this
    needs no database fetch (tools/fetch-db.sh) and is skipped only if
    tests/oracle/setup-xilinx.sh has not been run.

(b) The oracle wrapper scripts (fasm2frames-oracle, xc7frames2bit-oracle,
    bitread-oracle) reproduce the checked-in golden
    tests/corpus/xilinx/artix7/smoke_x1y0.{frm,bit,bitread.txt} from
    tests/corpus/xilinx/artix7/smoke_x1y0.fasm against a real fetched
    artix7 database -- skipped if tools/fetch-db.sh prjxray artix7 has not
    been run (in addition to setup-xilinx.sh).

Neither test touches the Rust rewrite; they only check the oracle/corpus
setup this task (T5.8) is responsible for.
"""
import filecmp
import os
import subprocess
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parent.parent
ORACLE_DIR = HERE
VENV_XILINX = ORACLE_DIR / 'venv-xilinx'
XILINX_BUILD = ORACLE_DIR / 'build' / 'xilinx'
XILINX_MARKER = VENV_XILINX / '.oracle-xilinx-setup-ok'
F4PGA_XC_FASM_SRC = XILINX_BUILD / 'src' / 'f4pga-xc-fasm'

FASM_DB_CACHE = Path(
    os.environ.get('FASM_DB_CACHE', str(ORACLE_DIR / 'build' / 'db')))
ARTIX7_DB_ROOT = FASM_DB_CACHE / 'prjxray-db' / 'artix7'

CORPUS_DIR = REPO_ROOT / 'tests' / 'corpus' / 'xilinx' / 'artix7'
SMOKE_FASM = CORPUS_DIR / 'smoke_x1y0.fasm'
GOLDEN_FRM = CORPUS_DIR / 'smoke_x1y0.frm'
GOLDEN_BIT = CORPUS_DIR / 'smoke_x1y0.bit'
GOLDEN_BITREAD = CORPUS_DIR / 'smoke_x1y0.bitread.txt'

PART = 'xc7a35tcsg324-1'

require_xilinx_oracle = pytest.mark.skipif(
    not XILINX_MARKER.exists(),
    reason=(
        "tests/oracle/venv-xilinx not set up; "
        "run tests/oracle/setup-xilinx.sh first"))

require_artix7_db = pytest.mark.skipif(
    not (ARTIX7_DB_ROOT / 'mapping' / 'parts.yaml').exists(),
    reason=(
        "artix7 database not fetched at {}; "
        "run tools/fetch-db.sh prjxray artix7 first".format(ARTIX7_DB_ROOT)))


# --------------------------------------------------------------------------
# (a) f4pga-xc-fasm's own test suite, run inside venv-xilinx.
# --------------------------------------------------------------------------


@require_xilinx_oracle
def test_f4pga_xc_fasm_test_suite_passes():
    """`tests/test_fasm2frames.py` from the pinned f4pga-xc-fasm checkout,
    run with venv-xilinx's pytest, against its own bundled miniature
    database (tests/test_data/db) -- no external database fetch needed.

    One upstream test, `test_badkey`, is deselected: it does
    `except TextXSyntaxError` around a call that (like every other call in
    this test file) goes through the top level `fasm.parse_fasm_filename`,
    whose *implementation* (antlr vs. textx) is decided once, at fasm
    package build/import time, by whether the antlr4 C++ extension built
    successfully -- see fasm/parser/__init__.py. tests/oracle/setup-xilinx.sh
    installs the exact same pinned `fasm` build tests/oracle/setup.sh does
    (see its "installing the pinned fasm package into venv-xilinx" step),
    so whichever implementation that produced also governs here; on a
    machine where the antlr extension built (the common case in this
    project's containers -- see tests/oracle/build/status.json), a bad-key
    parse error surfaces as a generic antlr4 C++ binding `Exception`
    instead of `textx.exceptions.TextXSyntaxError`, which `test_badkey`'s
    narrow `except` does not catch. This is a pre-existing assumption in
    f4pga-xc-fasm's own test, not something tests/oracle/setup-xilinx.sh or
    the Rust rewrite introduces; it is deselected here (not silenced or
    patched in the pinned checkout) so the rest of the suite is still a
    real, verifiable pass. See tests/oracle/README.md ("Xilinx reference
    tools") for the exact pass/skip/deselect counts observed."""
    if not F4PGA_XC_FASM_SRC.exists():
        pytest.skip(
            "{} not cloned; run tests/oracle/setup-xilinx.sh first".format(
                F4PGA_XC_FASM_SRC))

    test_file = F4PGA_XC_FASM_SRC / 'tests' / 'test_fasm2frames.py'
    result = subprocess.run(
        [
            str(VENV_XILINX / 'bin' / 'pytest'),
            str(test_file),
            '-v',
            '-k',
            'not test_badkey',
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    print(result.stdout)
    assert result.returncode == 0, result.stdout


# --------------------------------------------------------------------------
# (b) fasm2frames-oracle / xc7frames2bit-oracle / bitread-oracle vs. the
#     checked-in golden files, over a real fetched artix7 database.
# --------------------------------------------------------------------------


def _run_wrapper(name, args):
    wrapper = ORACLE_DIR / name
    result = subprocess.run(
        [str(wrapper)] + args,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    assert result.returncode == 0, (
        "{} {} failed:\nstdout: {}\nstderr: {}".format(
            name, args, result.stdout, result.stderr))
    return result


@require_xilinx_oracle
@require_artix7_db
def test_smoke_fasm_matches_golden_frm(tmp_path):
    """fasm2frames-oracle on the tiny hand written smoke FASM reproduces
    the checked-in golden .frm byte for byte."""
    out_frm = tmp_path / 'smoke_x1y0.frm'
    _run_wrapper(
        'fasm2frames-oracle', [
            '--sparse',
            '--db-root',
            str(ARTIX7_DB_ROOT),
            '--part',
            PART,
            str(SMOKE_FASM),
            str(out_frm),
        ])
    assert filecmp.cmp(str(out_frm), str(GOLDEN_FRM), shallow=False), (
        "fasm2frames-oracle output does not match {}; "
        "if this is expected (e.g. a deliberate re-pin), see "
        "tests/corpus/xilinx/artix7/README.md for how to regenerate "
        "the golden files".format(GOLDEN_FRM))


@require_xilinx_oracle
@require_artix7_db
def test_smoke_bit_bitread_matches_golden(tmp_path):
    """The full chain -- fasm2frames-oracle -> xc7frames2bit-oracle ->
    bitread-oracle -- reproduces the checked-in golden .bitread.txt (the
    set of programmed configuration bits) for the tiny hand written smoke
    FASM.

    This deliberately does NOT byte-compare the freshly produced .bit
    against the checked-in golden smoke_x1y0.bit: xc7frames2bit's
    BitstreamWriter embeds the current UTC date/time and the literal
    --frm_file path it was given into the bitstream header (see
    lib/include/prjxray/xilinx/bitstream_writer.h,
    absl::FormatTime(..., absl::Now(), ...) and tools/xc7frames2bit.cc's
    writeBitstream(..., FLAGS_frm_file, ...) call), so two runs on
    different days, or with the frame file at a different path/tmpdir,
    never produce byte-identical bitstreams even with identical frame
    contents -- see tests/corpus/xilinx/artix7/README.md. bitread's
    machine readable `-o`/`-y` output carries no such header info, so it
    is what's actually compared here (and in
    test_smoke_bitread_lists_expected_bits below)."""
    part_file = ARTIX7_DB_ROOT / PART / 'part.yaml'
    out_frm = tmp_path / 'smoke_x1y0.frm'
    out_bit = tmp_path / 'smoke_x1y0.bit'
    out_bitread = tmp_path / 'smoke_x1y0.bitread.txt'

    _run_wrapper(
        'fasm2frames-oracle', [
            '--sparse',
            '--db-root',
            str(ARTIX7_DB_ROOT),
            '--part',
            PART,
            str(SMOKE_FASM),
            str(out_frm),
        ])
    assert filecmp.cmp(str(out_frm), str(GOLDEN_FRM), shallow=False), (
        "fasm2frames-oracle output does not match {}".format(GOLDEN_FRM))

    _run_wrapper(
        'xc7frames2bit-oracle', [
            '--frm_file',
            str(out_frm),
            '--output_file',
            str(out_bit),
            '--part_name',
            PART,
            '--part_file',
            str(part_file),
        ])
    assert out_bit.exists() and out_bit.stat().st_size > 0

    _run_wrapper(
        'bitread-oracle', [
            '-z',
            '-y',
            '--part_file',
            str(part_file),
            '-o',
            str(out_bitread),
            str(out_bit),
        ])
    assert out_bitread.read_text() == GOLDEN_BITREAD.read_text(), (
        "bitread-oracle output does not match {}".format(GOLDEN_BITREAD))


@require_xilinx_oracle
@require_artix7_db
def test_smoke_bitread_lists_expected_bits():
    """Sanity check independent of the golden file byte-compare above:
    the 4 expected (frame, word, bit) entries -- 2 for the CLBLL_L
    ALUT.INIT[00]/[01] bits, 2 for the INT_L EL1BEG_N3.LOGIC_OUTS_L0 pip's
    2-bit encoding -- are exactly what bitread reports set. See
    tests/corpus/xilinx/artix7/README.md."""
    expected = {
        'bit_0040010b_000_05',
        'bit_0040010e_000_05',
        'bit_00400120_000_15',
        'bit_00400121_000_15',
    }
    actual = set(GOLDEN_BITREAD.read_text().split())
    assert actual == expected

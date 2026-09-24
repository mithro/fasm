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
"""Tests for the T7.2 fpgas.online-test-designs corpus
(tests/corpus/xilinx/artix7/designs/fpgas.online-test-designs/<design>/<board>/,
produced by tools/e2e/run-fpgas-online.sh +
tools/e2e/install-fpgas-online-corpus.sh).

For every committed design/board:

1. The FASM (top.fasm or top.fasm.xz) parses cleanly. Tries, in order:
   the Rust CLI (`target/release/fasm` under the repo root, if built),
   then the installed `fasm` Python package if it reports a working
   `rust`/`textx` implementation, then the pristine oracle
   (tests/oracle/venv/bin/python tests/oracle/dump.py, from
   tests/oracle/setup.sh). Skipped cleanly if none of the three is
   available.

2. When the Rust `fasm-xilinx` frame assembler exists as a CLI binary
   (`target/release/fasm2frames`) *and* the corresponding prjxray-db
   database is available, its output on the corpus FASM is compared byte
   for byte against the committed dense `top.frm.xz` (regenerated with
   the oracle `fasm2frames-oracle` by tools/e2e/run-fpgas-online.sh, see
   each design's README.md). As of this session `target/release/fasm2frames`
   does not exist yet in this checkout (T5.4/T5.5, the Rust frame
   assembler + fasm2frames CLI, is `[r]` in docs/rewrite/TASKS.md -- in
   review, not yet merged into this branch) so this comparison is skipped
   cleanly everywhere; the test is written to pick it up automatically
   once that binary lands, without needing to be edited.

3. If tools/difftest-xilinx.py (T5.9's frames differential test driver)
   exists in this checkout, it is run over every corpus FASM file found
   here and its outcome is asserted; skipped cleanly if the script is not
   present (it lives on a branch not yet merged here either -- see
   tools/e2e/README.md, "fpgas.online-test-designs corpus (T7.2)").

Does not touch the openXC7/LiteX build flow itself (that's
tests/e2e/test_openxc7.py's job for the shared toolchain, and
tools/e2e/run-fpgas-online.sh, run by hand, for producing new corpus
entries); this only checks what's already committed.
"""
import lzma
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
CORPUS_ROOT = (
    REPO_ROOT / 'tests' / 'corpus' / 'xilinx' / 'artix7' / 'designs' /
    'fpgas-online-test-designs' if False else
    REPO_ROOT / 'tests' / 'corpus' / 'xilinx' / 'artix7' / 'designs' /
    'fpgas.online-test-designs')

ORACLE_PYTHON = REPO_ROOT / 'tests' / 'oracle' / 'venv' / 'bin' / 'python'
ORACLE_DUMP = REPO_ROOT / 'tests' / 'oracle' / 'dump.py'
RUST_FASM_BIN = REPO_ROOT / 'target' / 'release' / 'fasm'
RUST_FASM2FRAMES_BIN = REPO_ROOT / 'target' / 'release' / 'fasm2frames'
DIFFTEST_XILINX = REPO_ROOT / 'tools' / 'difftest-xilinx.py'

PRJXRAY_DB_DIR = (
    REPO_ROOT / 'tests' / 'oracle' / 'build' / 'db' / 'prjxray-db')


def _discover_designs():
    """Yield (design, board, fasm_path) for every committed corpus entry."""
    if not CORPUS_ROOT.is_dir():
        return
    for design_dir in sorted(CORPUS_ROOT.iterdir()):
        if not design_dir.is_dir():
            continue
        for board_dir in sorted(design_dir.iterdir()):
            if not board_dir.is_dir():
                continue
            fasm_plain = board_dir / 'top.fasm'
            fasm_xz = board_dir / 'top.fasm.xz'
            if fasm_plain.exists():
                yield (design_dir.name, board_dir.name, fasm_plain)
            elif fasm_xz.exists():
                yield (design_dir.name, board_dir.name, fasm_xz)


DESIGNS = list(_discover_designs())

require_designs = pytest.mark.skipif(
    not DESIGNS,
    reason=(
        "no committed fpgas.online-test-designs corpus entries found under "
        f"{CORPUS_ROOT} (run tools/e2e/run-fpgas-online.sh + "
        "tools/e2e/install-fpgas-online-corpus.sh first)"))


def _read_maybe_xz(path):
    if path.suffix == '.xz':
        with lzma.open(path, 'rt') as f:
            return f.read()
    return path.read_text()


def _extract_to(path, tmp_path, name):
    """Return a plain-text path for *path* (decompressing .xz if needed)."""
    if path.suffix != '.xz':
        return path
    out = tmp_path / name
    out.write_text(_read_maybe_xz(path))
    return out


def _pick_fasm_parser():
    """Return (kind, callable(path) -> CompletedProcess-like) or None."""
    if RUST_FASM_BIN.exists():
        def run_rust(path):
            return subprocess.run(
                [str(RUST_FASM_BIN), 'parse', str(path)],
                capture_output=True, text=True, timeout=60)
        return ('rust-cli', run_rust)

    try:
        import fasm as fasm_pkg  # noqa: F401
        from fasm.parser import get_available_implementations
        impls = get_available_implementations()
        if impls:
            def run_pkg(path):
                result = subprocess.run(
                    [sys.executable, '-c',
                     'import sys, fasm; list(fasm.parse_fasm_filename(sys.argv[1]))',
                     str(path)],
                    capture_output=True, text=True, timeout=60)
                return result
            return ('python-package', run_pkg)
    except Exception:
        pass

    if ORACLE_PYTHON.exists() and ORACLE_DUMP.exists():
        def run_oracle(path):
            return subprocess.run(
                [str(ORACLE_PYTHON), str(ORACLE_DUMP), str(path)],
                capture_output=True, text=True, timeout=120)
        return ('oracle', run_oracle)

    return None


PARSER = _pick_fasm_parser()

require_parser = pytest.mark.skipif(
    PARSER is None,
    reason=(
        "no FASM parser available: neither target/release/fasm, an "
        "installed `fasm` package with a working implementation, nor "
        "tests/oracle/venv (run tests/oracle/setup.sh) was found"))


@require_designs
@require_parser
@pytest.mark.parametrize(
    'design,board,fasm_path', DESIGNS,
    ids=[f'{d}-{b}' for d, b, _ in DESIGNS])
def test_corpus_fasm_parses(design, board, fasm_path, tmp_path):
    """Every committed fpgas.online-test-designs FASM parses cleanly."""
    kind, run = PARSER
    plain = _extract_to(fasm_path, tmp_path, f'{design}-{board}.fasm')
    result = run(plain)
    assert result.returncode == 0, (
        f"{kind} parser failed on {fasm_path} ({design}/{board}):\n"
        f"stdout: {result.stdout}\nstderr: {result.stderr}")


@require_designs
@pytest.mark.parametrize(
    'design,board,fasm_path', DESIGNS,
    ids=[f'{d}-{b}' for d, b, _ in DESIGNS])
def test_corpus_fasm_is_substantial(design, board, fasm_path):
    """Sanity check: the committed FASM is not empty / truncated."""
    text = _read_maybe_xz(fasm_path)
    lines = [line for line in text.splitlines() if line.strip()]
    assert len(lines) >= 10, (
        f"{fasm_path} ({design}/{board}) has suspiciously few FASM lines "
        f"({len(lines)})")


def _part_for(design, board):
    """Read the part name for design/board from run-fpgas-online.sh's own
    table via --config, so this test never duplicates it."""
    script = REPO_ROOT / 'tools' / 'e2e' / 'run-fpgas-online.sh'
    if not script.exists():
        return None
    result = subprocess.run(
        ['bash', str(script), '--config', design, board],
        capture_output=True, text=True, timeout=10)
    if result.returncode != 0 or not result.stdout.strip():
        return None
    return result.stdout.strip().split('|')[1]


require_rust_fasm2frames = pytest.mark.skipif(
    not RUST_FASM2FRAMES_BIN.exists(),
    reason=(
        f"{RUST_FASM2FRAMES_BIN} not found -- the Rust fasm-xilinx frame "
        "assembler / fasm2frames CLI (T5.4/T5.5) is not merged into this "
        "checkout yet (docs/rewrite/TASKS.md marks it '[r]', in review); "
        "this test will start comparing automatically once it lands"))


@require_designs
@require_rust_fasm2frames
@pytest.mark.parametrize(
    'design,board,fasm_path', DESIGNS,
    ids=[f'{d}-{b}' for d, b, _ in DESIGNS])
def test_corpus_frames_match_rust_fasm2frames(design, board, fasm_path, tmp_path):
    """Rust fasm2frames output matches the committed reference .frm."""
    part = _part_for(design, board)
    assert part, f"could not resolve the part for {design}/{board}"
    db_root = PRJXRAY_DB_DIR / 'artix7'
    if not db_root.is_dir():
        pytest.skip(f"{db_root} not found; run tests/oracle/fetch the db first")

    board_dir = fasm_path.parent
    frm_xz = board_dir / 'top.frm.xz'
    assert frm_xz.exists(), f"{frm_xz} missing (every corpus entry should have one)"
    golden_frm = _extract_to(frm_xz, tmp_path, f'{design}-{board}-golden.frm')

    plain_fasm = _extract_to(fasm_path, tmp_path, f'{design}-{board}.fasm')
    rust_frm = tmp_path / f'{design}-{board}-rust.frm'
    result = subprocess.run(
        [str(RUST_FASM2FRAMES_BIN), '--db-root', str(db_root), '--part', part,
         str(plain_fasm), str(rust_frm)],
        capture_output=True, text=True, timeout=60)
    assert result.returncode == 0, (
        f"Rust fasm2frames failed on {design}/{board}:\n{result.stderr}")
    assert rust_frm.read_bytes() == golden_frm.read_bytes(), (
        f"Rust fasm2frames output differs from the committed reference "
        f".frm for {design}/{board}")


@pytest.mark.skipif(
    not DIFFTEST_XILINX.exists(),
    reason=(
        f"{DIFFTEST_XILINX} not found in this checkout (T5.9's frames "
        "differential test driver lives on a branch not yet merged here; "
        "per the T7.2 brief this step is skipped when it is absent -- see "
        "tools/e2e/README.md)"))
@require_designs
def test_difftest_xilinx_over_corpus():
    """Run tools/difftest-xilinx.py over the fpgas.online-test-designs
    corpus, if it exists in this checkout."""
    result = subprocess.run(
        [sys.executable, str(DIFFTEST_XILINX), str(CORPUS_ROOT)],
        capture_output=True, text=True, timeout=600)
    assert result.returncode == 0, (
        f"tools/difftest-xilinx.py reported failures over {CORPUS_ROOT}:\n"
        f"stdout: {result.stdout}\nstderr: {result.stderr}")

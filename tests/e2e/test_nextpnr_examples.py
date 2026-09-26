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
"""Tests for the T7.6 nextpnr-xilinx examples corpus
(tests/corpus/xilinx/<family>/designs/{nextpnr-xilinx,openxc7-demo-projects,
openxc7-primitive-tests}/<example>/<board>/top.fasm[.xz], built with the
openXC7 snap by tools/e2e/run-nextpnr-examples.sh and installed by
tools/e2e/install-nextpnr-examples-corpus.py).

Without any toolchain:

* every entry has its difftest.json and README; the FASM's and the
  frames' sha256 are the ones the README records; the entry is in
  run-nextpnr-examples.sh's table with the same part;
* tools/difftest-xilinx.py's corpus mode (`make xilinx-difftest`) picks
  every entry up with the part of its difftest.json;
* run-nextpnr-examples.sh --list works.

With the Rust tools built (`cargo build --release`):

* the Rust `fasm` parses every FASM;
* with the openXC7 snap's prjxray-db (tools/e2e/setup-openxc7.sh, found
  through $OPENXC7_E2E_BUILD or tools/e2e/build), the Rust `fasm2frames`
  writes the flow's frames (top.frm.xz) byte for byte;
* with the pinned prjxray-db ($FASM_DB_CACHE or tests/oracle/build/db),
  it writes the same frames too, or fails with the FasmLookupError the
  README records (the snap db has features the pinned db lacks).

With the toolchain and the pinned nextpnr-xilinx checkout
($NEXTPNR_XILINX_DIR, or tools/e2e/build/nextpnr-examples-src, made by
`run-nextpnr-examples.sh --fetch`): nextpnr-xilinx/blinky/arty-a35 is
rebuilt end to end (seconds), its FASM must be the committed one (the
flow is deterministic), and tools/e2e/compare-nextpnr-examples.py must
find the Rust tools identical to the snap's.
"""
import hashlib
import importlib.util
import json
import lzma
import os
import re
import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(REPO_ROOT / 'tools' / 'e2e'))
import snap_prjxray_db  # noqa: E402
CORPUS = REPO_ROOT / 'tests' / 'corpus' / 'xilinx'
RUST = REPO_ROOT / 'target' / 'release'
RUN = REPO_ROOT / 'tools' / 'e2e' / 'run-nextpnr-examples.sh'
DB_CACHE = Path(
    os.environ.get('FASM_DB_CACHE',
                   REPO_ROOT / 'tests' / 'oracle' / 'build' / 'db'))
E2E_BUILD = Path(
    os.environ.get('OPENXC7_E2E_BUILD',
                   REPO_ROOT / 'tools' / 'e2e' / 'build'))
# The snap's own bundled prjxray-db (T5.8b): tools/e2e/snap_prjxray_db.py
# resolves either tools/fetch-db.sh's lean cache or setup-openxc7.sh's full
# extraction. May be None if neither is set up; guarded at each use below.
SNAP_DB_CACHE = snap_prjxray_db.db_cache(REPO_ROOT)
NEXTPNR_XILINX_DIR = Path(
    os.environ.get(
        'NEXTPNR_XILINX_DIR',
        REPO_ROOT.joinpath('tools', 'e2e', 'build', 'nextpnr-examples-src',
                           'nextpnr-xilinx')))
SOURCES = ('nextpnr-xilinx', 'openxc7-demo-projects',
           'openxc7-primitive-tests')


def _entries():
    out = []
    for family_dir in sorted(CORPUS.iterdir()):
        for source in SOURCES:
            root = family_dir / 'designs' / source
            if root.is_dir():
                out += sorted(root.glob('*/*/top.fasm*'))
    return out


ENTRIES = _entries()
IDS = ['/'.join(p.parts[-4:-1]) for p in ENTRIES]


def _read(path):
    data = path.read_bytes()
    return lzma.decompress(data) if path.suffix == '.xz' else data


def _config(fasm):
    return json.loads((fasm.parent / 'difftest.json').read_text())


def _recorded(fasm, name):
    readme = (fasm.parent / 'README.md').read_text()
    m = re.search(r'sha256  %s\s+([0-9a-f]{64})' % re.escape(name), readme)
    return m.group(1) if m else None


def test_corpus_not_empty():
    assert len(ENTRIES) >= 10, ENTRIES
    assert any('nextpnr-xilinx' in i for i in IDS), IDS


@pytest.mark.parametrize('fasm', ENTRIES, ids=IDS)
def test_entry_metadata(fasm):
    config = _config(fasm)
    assert set(config) == {'part', 'family'}
    assert fasm.parts[-6] == config['family']
    readme = (fasm.parent / 'README.md').read_text()
    assert '`%s`' % config['part'] in readme
    assert _recorded(fasm, 'top.fasm') == hashlib.sha256(
        _read(fasm)).hexdigest()
    frm = lzma.decompress((fasm.parent / 'top.frm.xz').read_bytes())
    assert _recorded(fasm, 'top.frm') == hashlib.sha256(frm).hexdigest()
    assert _recorded(fasm, 'top.bit') is not None


@pytest.mark.parametrize('fasm', ENTRIES, ids=IDS)
def test_entry_in_run_script_table(fasm):
    ident = '/'.join(fasm.parts[-4:-1])
    r = subprocess.run(['bash', str(RUN), '--config', ident],
                       stdout=subprocess.PIPE,
                       timeout=60)
    assert r.returncode == 0, ident
    fields = r.stdout.decode().strip().split('|')
    assert fields[0] == ident
    assert fields[1] == _config(fasm)['part']
    assert fields[3] == _config(fasm)['family']


def test_run_script_list():
    r = subprocess.run(['bash', str(RUN), '--list'],
                       stdout=subprocess.PIPE,
                       timeout=60)
    assert r.returncode == 0
    listed = r.stdout.decode()
    for ident in IDS:
        assert ident in listed
    assert 'skip: ' in listed


def test_difftest_xilinx_covers_the_corpus():
    spec = importlib.util.spec_from_file_location(
        'difftest_xilinx', REPO_ROOT / 'tools' / 'difftest-xilinx.py')
    dx = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(dx)
    cases, notes = dx.corpus(str(DB_CACHE), '*/designs/*/*/*/top.fasm*')
    notes = [n for n in notes if any(s in n for s in SOURCES)]
    if notes:
        pytest.skip('prjxray-db missing: %s' % notes)
    seen = {}
    for name, fasm, db, part, flags in cases:
        seen.setdefault(name.rsplit('[', 1)[0], set()).add((part, db))
    for fasm in ENTRIES:
        rel = str(fasm.relative_to(REPO_ROOT))
        config = _config(fasm)
        assert seen.get(rel) == {
            (config['part'],
             str(DB_CACHE / 'prjxray-db' / config['family']))
        }, rel


need_rust = pytest.mark.skipif(
    not (RUST / 'fasm2frames').exists() or not (RUST / 'fasm').exists(),
    reason='the Rust tools are not built (cargo build --release)')


@need_rust
@pytest.mark.parametrize('fasm', ENTRIES, ids=IDS)
def test_rust_fasm_parses(fasm, tmp_path):
    path = tmp_path / 'top.fasm'
    path.write_bytes(_read(fasm))
    r = subprocess.run([str(RUST / 'fasm'), str(path)],
                       stdout=subprocess.DEVNULL,
                       stderr=subprocess.PIPE,
                       timeout=300)
    assert r.returncode == 0, r.stderr.decode()


def _fasm2frames(fasm, tmp_path, db_cache):
    if db_cache is None:
        pytest.skip('no prjxray-db cache available')
    config = _config(fasm)
    db = db_cache / 'prjxray-db' / config['family']
    if not (db / config['part']).is_dir():
        pytest.skip('%s has no %s' % (db, config['part']))
    path = tmp_path / 'top.fasm'
    path.write_bytes(_read(fasm))
    out = tmp_path / 'top.frm'
    argv = [
        str(RUST / 'fasm2frames'), '--db-root',
        str(db), '--part', config['part'],
        str(path),
        str(out)
    ]
    r = subprocess.run(argv, stderr=subprocess.PIPE, timeout=600)
    return r.returncode, r.stderr.decode(), out.read_bytes()


@need_rust
@pytest.mark.parametrize('fasm', ENTRIES, ids=IDS)
def test_rust_fasm2frames_matches_the_flow(fasm, tmp_path):
    code, err, frm = _fasm2frames(fasm, tmp_path, SNAP_DB_CACHE)
    assert code == 0, err
    assert frm == lzma.decompress((fasm.parent / 'top.frm.xz').read_bytes())


@need_rust
@pytest.mark.parametrize('fasm', ENTRIES, ids=IDS)
def test_rust_fasm2frames_pinned_db(fasm, tmp_path):
    code, err, frm = _fasm2frames(fasm, tmp_path, DB_CACHE)
    readme = (fasm.parent / 'README.md').read_text()
    if '* pinned db = snap db frames: yes' in readme:
        assert code == 0, err
        assert frm == lzma.decompress(
            (fasm.parent / 'top.frm.xz').read_bytes())
    else:
        # The README's note names the error the pinned db gives.
        assert code != 0 or frm != lzma.decompress(
            (fasm.parent / 'top.frm.xz').read_bytes())
        if code != 0:
            last = err.strip().splitlines()[-1]
            assert 'pinned db: exit code %d, %s' % (code, last) in readme, last


SNAP_BIN = E2E_BUILD / 'openxc7' / 'bin'
need_flow = pytest.mark.skipif(
    not (SNAP_BIN / 'nextpnr-xilinx').exists()
    or not E2E_BUILD.joinpath('openxc7', 'chipdb',
                              'xc7a35tcsg324-1.bin').exists(),
    reason='the openXC7 toolchain is not set up (tools/e2e/setup-openxc7.sh)')


@need_flow
@need_rust
@pytest.mark.skipif(not (NEXTPNR_XILINX_DIR / '.git').exists(),
                    reason='no nextpnr-xilinx checkout (%s); '
                    'tools/e2e/run-nextpnr-examples.sh --fetch' %
                    NEXTPNR_XILINX_DIR)
def test_build_blinky_arty_a35(tmp_path):
    ident = 'nextpnr-xilinx/blinky/arty-a35'
    env = dict(os.environ,
               NEXTPNR_EXAMPLES_OUT=str(tmp_path),
               OPENXC7_E2E_BUILD=str(E2E_BUILD),
               NEXTPNR_XILINX_DIR=str(NEXTPNR_XILINX_DIR))
    r = subprocess.run(['bash', str(RUN), ident],
                       env=env,
                       stdout=subprocess.PIPE,
                       stderr=subprocess.STDOUT,
                       timeout=1800)
    assert r.returncode == 0, r.stdout.decode()[-4000:]
    info = json.loads((tmp_path / ident / 'info.json').read_text())
    assert info['status'] == 'built', info
    built = (tmp_path / ident / 'top.fasm').read_bytes()
    committed = CORPUS / 'artix7' / 'designs' / ident / 'top.fasm'
    assert built == committed.read_bytes()
    argv = [
        'python3',
        str(REPO_ROOT / 'tools' / 'e2e' / 'compare-nextpnr-examples.py'),
        '--out',
        str(tmp_path)
    ]
    r = subprocess.run(argv,
                       env=env,
                       stdout=subprocess.PIPE,
                       stderr=subprocess.STDOUT,
                       timeout=1800)
    assert r.returncode == 0, r.stdout.decode()[-4000:]

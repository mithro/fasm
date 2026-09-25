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
"""Tests for the T7.4 VTR genfasm corpus: tests/corpus/vtr/test_fasm_arch/
(generic FASM of VTR's genfasm test architecture) and
tests/corpus/xilinx/<family>/designs/vtr/<circuit>/<board>/ (Xilinx FASM
of VTR's symbiflow benchmarks), made by tools/e2e/run-vtr-genfasm.sh and
tools/e2e/install-vtr-genfasm-corpus.py.

Without any toolchain:

* every built circuit of test_fasm_arch/genfasm.json.xz is stored (in its
  own file or in its directory's genfasm-all.fasm[.xz]) with the recorded
  sha256 and line count, and nothing else is stored;
* test_fasm.cpp's rr edge metadata variant of wire.eblif is the plain
  FASM plus routing features, and its expected-errors.json names the line
  of its first routing feature;
* tools/difftest.py discovers every generic file (and the expected error)
  and tools/difftest-xilinx.py every Xilinx entry, with its part;
* every Xilinx entry has its difftest.json and a README recording the
  sha256 of the FASM (and of the reference frames and bitstream).

With the Rust tools built (`cargo build --release`): the Rust `fasm`
parses every generic FASM file (and rejects the expected error at its
line); with the pinned prjxray-db as well, the Rust `fasm2frames --sparse
--emit_pudc_b_pullup` writes the reference frames of every Xilinx entry.

With the f4pga toolchain (tools/e2e/setup-f4pga.sh, $F4PGA_E2E_ROOT) and
a VTR checkout ($VTR_ROOT, see run-vtr-genfasm.sh; only used when it
exists): a few test_fasm_arch circuits are rerun and must give the
committed FASM (genfasm is deterministic); with $VTR_GENFASM_XC7=1 also
counter_basys3 on xc7a50t_test (about a minute and 4 GB of memory).
"""
import hashlib
import importlib.util
import json
import lzma
import os
import re
import subprocess
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
GENERIC = REPO_ROOT / 'tests' / 'corpus' / 'vtr' / 'test_fasm_arch'
XILINX = REPO_ROOT / 'tests' / 'corpus' / 'xilinx'
RUST = REPO_ROOT / 'target' / 'release'
DB_CACHE = Path(
    os.environ.get('FASM_DB_CACHE',
                   REPO_ROOT / 'tests' / 'oracle' / 'build' / 'db'))
F4PGA_ROOT = Path(
    os.environ.get('F4PGA_E2E_ROOT',
                   REPO_ROOT / 'tools' / 'e2e' / 'build' / 'f4pga'))
F4PGA_ENV = F4PGA_ROOT / 'xc7' / 'conda' / 'envs' / 'xc7'
VTR_ROOT = Path(
    os.environ.get('VTR_ROOT', REPO_ROOT / 'tools' / 'e2e' / 'build' / 'vtr'))
RUN = REPO_ROOT / 'tools' / 'e2e' / 'run-vtr-genfasm.sh'
SECTION = re.compile(rb'^# circuit (\S+) sha256 ([0-9a-f]{64}) lines (\d+)$',
                     re.M)


def _read(path):
    data = path.read_bytes()
    return lzma.decompress(data) if path.suffix == '.xz' else data


def _sha(data):
    return hashlib.sha256(data).hexdigest()


INDEX = json.loads(_read(GENERIC / 'genfasm.json.xz'))
CIRCUITS = INDEX['circuits']
BUILT = sorted(c for c, e in CIRCUITS.items() if e['status'] == 'built')


def _sections(path):
    """{circuit: FASM} of an aggregated genfasm-all.fasm[.xz]."""
    data = _read(path)
    marks = list(SECTION.finditer(data))
    assert marks, path
    out = {}
    for i, m in enumerate(marks):
        end = marks[i + 1].start() if i + 1 < len(marks) else len(data)
        body = data[m.end() + 1:end]
        assert _sha(body) == m.group(2).decode(), m.group(1)
        assert body.count(b'\n') == int(m.group(3))
        out[m.group(1).decode()] = body
    return out


def _stored(circuit, name='genfasm.fasm'):
    stored = GENERIC / CIRCUITS[circuit][name]['stored']
    if 'genfasm-all.fasm' in stored.name:
        return _sections(stored)[circuit]
    return _read(stored)


def _xilinx_entries():
    out = []
    for family_dir in sorted(XILINX.iterdir()):
        root = family_dir / 'designs' / 'vtr'
        if root.is_dir():
            out += sorted(root.glob('*/*/genfasm.fasm*'))
    return [p for p in out if not p.name.endswith('.frm.xz')]


XILINX_ENTRIES = _xilinx_entries()
XILINX_IDS = ['/'.join(p.parts[-3:-1]) for p in XILINX_ENTRIES]


def _recorded(readme, name):
    m = re.search(r'sha256  %s\s+([0-9a-f]{64})' % re.escape(name), readme)
    return m.group(1) if m else None


def test_index():
    assert INDEX['vtr_commit'] == '25e723a24aa0ae7a0061cd89dd84b1fb62afcc09'
    assert INDEX['route_chan_width'] == 100
    assert INDEX['arch'] == 'utils/fasm/test/test_fasm_arch.xml'
    assert len(CIRCUITS) == 1502
    assert len(BUILT) == 428
    for circuit, entry in CIRCUITS.items():
        status = entry['status']
        assert status == 'built' or status.startswith(
            ('unimplementable: ', 'not run: ')), (circuit, status)
        assert ('genfasm.fasm' in entry) == (status == 'built'), circuit


@pytest.mark.parametrize('circuit', BUILT)
def test_stored_fasm_matches_index(circuit):
    entry = CIRCUITS[circuit]['genfasm.fasm']
    data = _stored(circuit)
    assert _sha(data) == entry['sha256']
    assert data.count(b'\n') == entry['lines']
    assert len(data) == entry['bytes']


def test_nothing_else_stored():
    stored = {'genfasm.json.xz', 'fasm-test/wire/expected-errors.json'}
    for entry in CIRCUITS.values():
        for name in ('genfasm.fasm', 'genfasm-rr-metadata.fasm'):
            if name in entry:
                stored.add(entry[name]['stored'])
    files = {
        str(p.relative_to(GENERIC))
        for p in GENERIC.rglob('*') if p.is_file()
    }
    assert files == stored
    for path in GENERIC.rglob('genfasm-all.fasm*'):
        group = str(path.parent.relative_to(GENERIC))
        members = {
            c
            for c in BUILT if c.rsplit('/', 1)[0] == group
            and CIRCUITS[c]['genfasm.fasm']['stored'].startswith(group + '/')
        }
        assert set(_sections(path)) == members, group


def test_rr_metadata_variant():
    wire = CIRCUITS['fasm-test/wire']
    plain = _stored('fasm-test/wire')
    meta = _stored('fasm-test/wire', 'genfasm-rr-metadata.fasm')
    assert _sha(meta) == wire['genfasm-rr-metadata.fasm']['sha256']
    lines = meta.decode().splitlines(True)
    routing = [
        i for i, line in enumerate(lines)
        if line[:1].isdigit() or line.startswith('PIN_')
    ]
    # The plain FASM plus the edge features, in genfasm's order.
    assert ''.join(line for i, line in enumerate(lines)
                   if i not in set(routing)).encode() == plain
    assert len(routing) > 0
    expected = json.loads(
        (GENERIC / 'fasm-test' / 'wire' / 'expected-errors.json').read_text())
    assert expected['genfasm-rr-metadata.fasm']['line'] == routing[0] + 1


def _load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_difftest_discovers_the_generic_corpus():
    dt = _load('difftest', REPO_ROOT / 'tools' / 'difftest.py')
    found = {
        rel: category
        for _, rel, category in dt.discover_corpus('tests/corpus/vtr/*')
    }
    rel = str(GENERIC.relative_to(REPO_ROOT))
    for p in GENERIC.rglob('*.fasm*'):
        assert found.get(str(p.relative_to(REPO_ROOT))) == 'plain', p
    expected = dt.load_expected_errors()
    key = rel + '/fasm-test/wire/genfasm-rr-metadata.fasm'
    assert expected[key]['line'] > 1
    assert dt._class_all_three_reject(
        {'error': 'Parse error at 334:0 - x'},
        {'error': 'Parse error at 334:0 - y'},
        {'error': '/a/b.fasm:334:1: Expected z'}, 334)
    assert not dt._class_all_three_reject(
        {'error': 'Parse error at 334:0 - x'},
        {'error': 'Parse error at 335:0 - y'},
        {'error': '/a/b.fasm:334:1: Expected z'}, 334)
    assert not dt._class_all_three_reject({'lines': []}, {'lines': []},
                                          {'lines': []}, 334)


def test_xilinx_corpus_not_empty():
    assert len(XILINX_ENTRIES) >= 5, XILINX_ENTRIES


@pytest.mark.parametrize('fasm', XILINX_ENTRIES, ids=XILINX_IDS)
def test_xilinx_entry_metadata(fasm):
    config = json.loads((fasm.parent / 'difftest.json').read_text())
    assert set(config) == {'part', 'family'}
    assert fasm.parts[-6] == config['family']
    readme = (fasm.parent / 'README.md').read_text()
    assert '`%s`' % config['part'] in readme
    assert _recorded(readme, 'top.fasm') == _sha(_read(fasm))
    assert _recorded(readme, 'top.frm') is not None
    assert _recorded(readme, 'top.bit') is not None


def test_difftest_xilinx_covers_the_corpus():
    dx = _load('difftest_xilinx', REPO_ROOT / 'tools' / 'difftest-xilinx.py')
    cases, notes = dx.corpus(str(DB_CACHE), '*designs/vtr/*')
    notes = [n for n in notes if '/vtr/' in n]
    if notes:
        pytest.skip('prjxray-db missing: %s' % notes)
    seen = {}
    for name, _, db, part, _ in cases:
        seen.setdefault(name.rsplit('[', 1)[0], set()).add((part, db))
    for fasm in XILINX_ENTRIES:
        rel = str(fasm.relative_to(REPO_ROOT))
        config = json.loads((fasm.parent / 'difftest.json').read_text())
        assert seen.get(rel) == {
            (config['part'],
             str(DB_CACHE / 'prjxray-db' / config['family']))
        }, rel


need_rust = pytest.mark.skipif(
    not (RUST / 'fasm2frames').exists() or not (RUST / 'fasm').exists(),
    reason='the Rust tools are not built (cargo build --release)')


@need_rust
def test_rust_fasm_parses_the_generic_corpus(tmp_path):
    expected = json.loads(
        (GENERIC / 'fasm-test' / 'wire' / 'expected-errors.json').read_text())
    for path in sorted(GENERIC.rglob('*.fasm*')):
        data = _read(path)
        f = tmp_path / 'x.fasm'
        f.write_bytes(data)
        r = subprocess.run([str(RUST / 'fasm'), str(f)],
                           stdout=subprocess.PIPE,
                           stderr=subprocess.PIPE,
                           timeout=300)
        # Like the original `fasm` tool, a parse error is printed as
        # `Error: Parse error at L:C - ...` (exit status 0).
        out = (r.stdout + r.stderr).decode()
        if path.name in expected:
            line = expected[path.name]['line']
            assert out.startswith('Error: Parse error at %d:' % line), path
        else:
            assert r.returncode == 0 and not out.startswith('Error'), \
                (path, out[:500])


@need_rust
@pytest.mark.parametrize('fasm', XILINX_ENTRIES, ids=XILINX_IDS)
def test_rust_fasm2frames_matches_the_reference(fasm, tmp_path):
    config = json.loads((fasm.parent / 'difftest.json').read_text())
    db = DB_CACHE / 'prjxray-db' / config['family']
    if not (db / config['part']).is_dir():
        pytest.skip('%s not fetched (tools/fetch-db.sh prjxray %s)' %
                    (db, config['family']))
    path = tmp_path / 'genfasm.fasm'
    path.write_bytes(_read(fasm))
    out = tmp_path / 'genfasm.frm'
    argv = [
        str(RUST / 'fasm2frames'), '--db-root',
        str(db), '--part', config['part'], '--sparse',
        '--emit_pudc_b_pullup',
        str(path),
        str(out)
    ]
    r = subprocess.run(argv, stderr=subprocess.PIPE, timeout=600)
    assert r.returncode == 0, r.stderr.decode()
    frm = out.read_bytes()
    committed = fasm.parent / 'genfasm.frm.xz'
    if committed.exists():
        assert frm == lzma.decompress(committed.read_bytes())
    assert _sha(frm) == _recorded((fasm.parent / 'README.md').read_text(),
                                  'top.frm')


need_toolchain = pytest.mark.skipif(
    not (F4PGA_ENV / 'bin' / 'genfasm').exists()
    or not (VTR_ROOT / 'utils/fasm/test/test_fasm_arch.xml').exists(),
    reason='the f4pga toolchain (tools/e2e/setup-f4pga.sh) or a VTR '
    'checkout ($VTR_ROOT) is missing')

RERUN = ['fasm-test/wire', 'microbenchmarks/mult_4x4', 'blif/6/b1',
         'blif/clock_aliases', 'tests/conn_order']


def _run(tmp_path, *args):
    env = dict(os.environ,
               F4PGA_E2E_ROOT=str(F4PGA_ROOT),
               VTR_ROOT=str(VTR_ROOT),
               VTR_GENFASM_OUT=str(tmp_path / 'out'))
    r = subprocess.run(['bash', str(RUN)] + list(args),
                       env=env,
                       stdout=subprocess.PIPE,
                       stderr=subprocess.STDOUT,
                       timeout=1800)
    assert r.returncode == 0, r.stdout.decode()[-3000:]
    return tmp_path / 'out'


@need_toolchain
def test_rerun_test_fasm_arch(tmp_path):
    out = _run(tmp_path, 'test_fasm_arch', *RERUN)
    for circuit in RERUN:
        info = json.loads(
            (out / 'test_fasm_arch' / circuit / 'info.json').read_text())
        for name in ('genfasm.fasm', 'genfasm-rr-metadata.fasm'):
            if name in CIRCUITS[circuit]:
                assert info[name]['sha256'] == \
                    CIRCUITS[circuit][name]['sha256'], (circuit, name)


@need_toolchain
@pytest.mark.skipif(os.environ.get('VTR_GENFASM_XC7') != '1',
                    reason='set VTR_GENFASM_XC7=1 to run VPR on '
                    'xc7a50t_test (about a minute, 4 GB)')
def test_rerun_counter_basys3(tmp_path):
    out = _run(tmp_path, 'xc7a50t_test', 'counter_basys3')
    info = json.loads(
        (out / 'xc7a50t_test/counter_basys3/basys3/info.json').read_text())
    entry = XILINX / 'artix7' / 'designs' / 'vtr' / 'counter_basys3' / \
        'basys3'
    readme = (entry / 'README.md').read_text()
    assert info['top.fasm']['sha256'] == _recorded(readme, 'top.fasm')
    assert info['top.frm']['sha256'] == _recorded(readme, 'top.frm')

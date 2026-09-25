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
"""Tests for the T7.3 f4pga-examples corpus
(tests/corpus/xilinx/<family>/designs/f4pga-examples/<design>/<board>/
vpr.fasm[.xz], built with the f4pga Yosys + VPR flow by
tools/e2e/run-f4pga-examples.sh and installed by
tools/e2e/install-f4pga-examples-corpus.py).

Without any toolchain:

* every entry has its difftest.json and README, and the FASM's sha256 is
  the one the README records;
* tools/difftest-xilinx.py's corpus mode (`make xilinx-difftest`) picks
  every entry up with the part of its difftest.json.

With the Rust tools built (`cargo build --release`) and the pinned
prjxray-db (tools/fetch-db.sh, $FASM_DB_CACHE or tests/oracle/build/db;
the flow's own prjxray-db is identical to it, see tools/e2e/README.md):

* the Rust `fasm` parses every FASM;
* the Rust `fasm2frames --sparse --emit_pudc_b_pullup` (the flow's xcfasm
  options) writes exactly the flow's frames: the committed `vpr.frm.xz`,
  or the sha256 the README records for `top.frm`.

With the f4pga toolchain (tools/e2e/setup-f4pga.sh, device xc7a50t_test)
and an f4pga-examples checkout (tools/e2e/build/f4pga-examples or
$F4PGA_EXAMPLES_DIR; cloned by run-f4pga-examples.sh, so only when
$F4PGA_EXAMPLES_BUILD=1): counter_test/arty_35 is built end to end (about
a minute), its FASM must be the committed one (the flow is
deterministic), and tools/e2e/compare-f4pga-examples.py must find the Rust
tools identical to the flow's outputs and tools.
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
CORPUS = REPO_ROOT / 'tests' / 'corpus' / 'xilinx'
RUST = REPO_ROOT / 'target' / 'release'
DB_CACHE = Path(
    os.environ.get('FASM_DB_CACHE',
                   REPO_ROOT / 'tests' / 'oracle' / 'build' / 'db'))
F4PGA_ROOT = Path(
    os.environ.get('F4PGA_E2E_ROOT',
                   REPO_ROOT / 'tools' / 'e2e' / 'build' / 'f4pga'))
F4PGA_ENV = F4PGA_ROOT / 'xc7' / 'conda' / 'envs' / 'xc7'
EXAMPLES = Path(
    os.environ.get('F4PGA_EXAMPLES_DIR',
                   REPO_ROOT / 'tools' / 'e2e' / 'build' / 'f4pga-examples'))


def _entries():
    out = []
    for family_dir in sorted(CORPUS.iterdir()):
        root = family_dir / 'designs' / 'f4pga-examples'
        if not root.is_dir():
            continue
        for fasm in sorted(root.glob('*/*/vpr.fasm*')):
            out.append(fasm)
    return out


ENTRIES = _entries()
IDS = ['/'.join(p.parts[-3:-1]) for p in ENTRIES]


def _read(path):
    data = path.read_bytes()
    return lzma.decompress(data) if path.suffix == '.xz' else data


def _readme(fasm):
    d = fasm.parent
    for name in ('README.vpr.md', 'README.md'):
        if (d / name).exists():
            return (d / name).read_text()
    return None


def _recorded(readme, name):
    m = re.search(r'sha256  %s\s+([0-9a-f]{64})' % re.escape(name), readme)
    return m.group(1) if m else None


def test_corpus_not_empty():
    assert len(ENTRIES) >= 20, ENTRIES


@pytest.mark.parametrize('fasm', ENTRIES, ids=IDS)
def test_entry_metadata(fasm):
    config = json.loads((fasm.parent / 'difftest.json').read_text())
    assert set(config) == {'part', 'family'}
    assert fasm.parts[-6] == config['family']
    readme = _readme(fasm)
    assert readme is not None
    assert '`%s`' % config['part'] in readme
    assert _recorded(readme, 'top.fasm') == hashlib.sha256(
        _read(fasm)).hexdigest()
    assert _recorded(readme, 'top.frm') is not None
    assert _recorded(readme, 'top.bit') is not None


def test_difftest_xilinx_covers_the_corpus():
    spec = importlib.util.spec_from_file_location(
        'difftest_xilinx', REPO_ROOT / 'tools' / 'difftest-xilinx.py')
    dx = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(dx)
    cases, notes = dx.corpus(str(DB_CACHE), '*f4pga-examples*vpr.fasm*')
    notes = [n for n in notes if 'f4pga-examples' in n]
    if notes:
        pytest.skip('prjxray-db missing: %s' % notes)
    seen = {}
    for name, fasm, db, part, flags in cases:
        seen.setdefault(name.rsplit('[', 1)[0], set()).add((part, db))
    for fasm in ENTRIES:
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
@pytest.mark.parametrize('fasm', ENTRIES, ids=IDS)
def test_rust_fasm_parses(fasm, tmp_path):
    path = tmp_path / 'vpr.fasm'
    path.write_bytes(_read(fasm))
    r = subprocess.run([str(RUST / 'fasm'), str(path)],
                       stdout=subprocess.DEVNULL,
                       stderr=subprocess.PIPE,
                       timeout=300)
    assert r.returncode == 0, r.stderr.decode()


@need_rust
@pytest.mark.parametrize('fasm', ENTRIES, ids=IDS)
def test_rust_fasm2frames_matches_the_flow(fasm, tmp_path):
    config = json.loads((fasm.parent / 'difftest.json').read_text())
    db = DB_CACHE / 'prjxray-db' / config['family']
    if not (db / config['part']).is_dir():
        pytest.skip('%s not fetched (tools/fetch-db.sh prjxray %s)' %
                    (db, config['family']))
    path = tmp_path / 'vpr.fasm'
    path.write_bytes(_read(fasm))
    out = tmp_path / 'vpr.frm'
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
    committed = fasm.parent / 'vpr.frm.xz'
    if committed.exists():
        assert frm == lzma.decompress(committed.read_bytes())
    assert hashlib.sha256(frm).hexdigest() == _recorded(
        _readme(fasm), 'top.frm')


CHECK_GENFASM = REPO_ROOT / 'tools' / 'e2e' / 'f4pga' / 'check-genfasm.sh'

# A fake genfasm: writes its log like genfasm does, then ends as told.
FAKE_GENFASM = """#!/bin/bash
echo "VPR FPGA Placement and Routing." > vpr_stdout.log
echo "Loading rr graph ..." >> vpr_stdout.log
echo "CLBLM_R_X1Y1.SLICEM_X0.AFF.ZINI" > top.fasm
case "$1" in
  ok)
    echo "Writing Implementation FASM: top.fasm" >> vpr_stdout.log
    echo "The entire flow of VPR took 1.5 seconds." >> vpr_stdout.log ;;
  error)
    echo "Error 1: something failed" >> vpr_stdout.log
    exit 1 ;;
  *)
    echo "Writing Implementation FASM: top.fasm" >> vpr_stdout.log
    kill -"$1" $$ ;;
esac
"""


@pytest.mark.parametrize('ending,ok', [('ok', True), ('KILL', False),
                                       ('BUS', False), ('TERM', False),
                                       ('SEGV', False), ('ABRT', False),
                                       ('error', False)])
@pytest.mark.parametrize('flow', ['write_fasm', 'f4pga_build'])
def test_check_genfasm(tmp_path, ending, ok, flow):
    """tools/e2e/f4pga/check-genfasm.sh on fake genfasm runs, run like
    symbiflow_write_fasm runs genfasm (`/bin/bash -c` with more commands
    after it, no `set -e`; fasm.log renamed from vpr_stdout.log when bash
    returns 0) or like `f4pga build` does (vpr_stdout.log kept)."""
    genfasm = tmp_path / 'genfasm'
    genfasm.write_text(FAKE_GENFASM)
    genfasm.chmod(0o755)
    script = ("\n'%s' %s\nTOP=top\necho \"writing final fasm (extra: "
              "${TOP}_fasm_extra.fasm)\"\n" % (genfasm, ending))
    r = subprocess.run(['/bin/bash', '-c', script],
                       cwd=str(tmp_path),
                       stdout=subprocess.PIPE,
                       stderr=subprocess.STDOUT,
                       timeout=60)
    # The flow itself does not notice: bash returns 0 in every case.
    assert r.returncode == 0, r.stdout
    (tmp_path / 'build.log').write_bytes(r.stdout)
    if flow == 'write_fasm':
        (tmp_path / 'vpr_stdout.log').rename(tmp_path / 'fasm.log')
    c = subprocess.run(
        [str(CHECK_GENFASM),
         str(tmp_path),
         str(tmp_path / 'build.log')],
        stdout=subprocess.PIPE,
        timeout=60)
    assert (c.returncode == 0) == ok, (r.stdout, c.stdout)
    if not ok:
        assert c.stdout.strip(), 'no reason given'


def test_check_genfasm_reports_the_signal(tmp_path):
    (tmp_path / 'vpr_stdout.log').write_text(
        'Writing Implementation FASM: top.fasm\n'
        'The entire flow of VPR took 1 seconds.\n')
    (tmp_path / 'build.log').write_text(
        "/bin/bash: line 2: 22904 Bus error               (core dumped) "
        "'/x/bin/genfasm' ${ARCH_DEF} ${EBLIF}\n")
    c = subprocess.run(
        [str(CHECK_GENFASM),
         str(tmp_path),
         str(tmp_path / 'build.log')],
        stdout=subprocess.PIPE,
        timeout=60)
    assert c.returncode == 1
    assert c.stdout.decode().startswith('genfasm killed: Bus error'), c.stdout


ARCH_A50T = F4PGA_ROOT / 'xc7' / 'share' / 'f4pga' / 'arch' / 'xc7a50t_test'
need_flow = pytest.mark.skipif(
    not (F4PGA_ENV / 'bin' / 'vpr').exists() or not ARCH_A50T.is_dir(),
    reason='the f4pga toolchain (device xc7a50t_test) is not installed '
    '(tools/e2e/setup-f4pga.sh)')


@need_flow
def test_flow_tools_run(tmp_path):
    for argv in (['yosys', '-V'], ['vpr', '--version'],
                 ['xcfasm', '--help'], ['xc7frames2bit', '--helpshort']):
        r = subprocess.run([str(F4PGA_ENV / 'bin' / argv[0])] + argv[1:],
                           cwd=str(tmp_path),
                           stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT,
                           timeout=60)
        assert r.returncode in (0, 1), (argv, r.stdout[-2000:])
        assert r.stdout, argv


@need_flow
@need_rust
@pytest.mark.skipif(
    not (EXAMPLES / '.git').exists()
    and os.environ.get('F4PGA_EXAMPLES_BUILD') != '1',
    reason='no f4pga-examples checkout (%s); set F4PGA_EXAMPLES_BUILD=1 to '
    'let tools/e2e/run-f4pga-examples.sh clone it' % EXAMPLES)
def test_build_counter_test_arty_35(tmp_path):
    env = dict(os.environ, F4PGA_EXAMPLES_OUT=str(tmp_path))
    argv = [
        str(REPO_ROOT / 'tools' / 'e2e' / 'run-f4pga-examples.sh'),
        'counter_test', 'arty_35'
    ]
    r = subprocess.run(argv,
                       env=env,
                       stdout=subprocess.PIPE,
                       stderr=subprocess.STDOUT,
                       timeout=1800)
    assert r.returncode == 0, r.stdout.decode()[-4000:]
    built = (tmp_path / 'counter_test' / 'arty_35' / 'top.fasm').read_bytes()
    committed = (CORPUS / 'artix7' / 'designs' / 'f4pga-examples'
                 / 'counter_test' / 'arty_35' / 'vpr.fasm').read_bytes()
    assert built == committed
    argv = [
        'python3',
        str(REPO_ROOT / 'tools' / 'e2e' / 'compare-f4pga-examples.py'),
        '--out',
        str(tmp_path)
    ]
    r = subprocess.run(argv,
                       stdout=subprocess.PIPE,
                       stderr=subprocess.STDOUT,
                       timeout=1800)
    assert r.returncode == 0, r.stdout.decode()[-4000:]

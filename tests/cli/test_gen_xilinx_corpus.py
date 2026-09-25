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
"""`tools/gen-xilinx-corpus.py` on the test databases of
`rust/fasm-xilinx/testdata` (no reference tools, no fetched database):
the output is deterministic, every feature is placed, and the Rust
`fasm2frames --sparse` output of every features file equals the frames
the generator's model of prjxray predicts (`--expected-frm`); the error
files fail. With the prjxray-db artix7 database, the same on one real
part (`--tiles first`). Skipped without the Rust binary
(`FASM2FRAMES_RUST`, default `target/release/fasm2frames`)."""
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
GENERATOR = ROOT / 'tools' / 'gen-xilinx-corpus.py'
TESTDATA = ROOT / 'rust' / 'fasm-xilinx' / 'testdata'
RUST = Path(
    os.environ.get('FASM2FRAMES_RUST',
                   ROOT / 'target' / 'release' / 'fasm2frames'))

if not RUST.exists():
    pytest.skip('Rust fasm2frames missing (cargo build --release -p '
                'fasm-cli): {}'.format(RUST),
                allow_module_level=True)


def real_db():
    for base in (os.environ.get('FASM_DB_CACHE'),
                 ROOT.joinpath('tests', 'oracle', 'build', 'db')):
        if base and (Path(base) / 'prjxray-db' / 'artix7').is_dir():
            return Path(base) / 'prjxray-db' / 'artix7'
    return None


def generate(db, part, out, *extra):
    argv = [
        sys.executable,
        str(GENERATOR), '--db-root',
        str(db), '--part', part, '--out-dir',
        str(out), '--expected-frm'
    ]
    subprocess.run(argv + list(extra), check=True, capture_output=True)
    return json.loads((out / 'manifest.json').read_text())


def fasm2frames(db, part, fasm, frm, tmp_path, flags=('--sparse', )):
    env = dict(os.environ)
    env['FASM_XDB_CACHE'] = str(tmp_path / 'xdb')
    argv = [str(RUST), '--db-root', str(db), '--part', part]
    argv += list(flags) + [str(fasm), str(frm)]
    return subprocess.run(argv, env=env, capture_output=True)


def check_coverage(manifest):
    """Every unit of every group placed or listed as uncovered (counted
    distinct per group, not per placement)."""
    coverage = manifest['coverage']
    assert coverage and manifest['features_total'] > 0
    for name, c in coverage.items():
        assert c['placed'] + c['uncovered'] == c['units'], (name, c)
    assert sum(c['units'] for c in coverage.values()) == \
        manifest['features_total']
    assert manifest['features_distinct_placed'] + len(
        manifest['uncovered']) == manifest['features_total']


CASES = [
    (TESTDATA / 'mini-db', 'xc7', ['--tiles', 'sample', '2']),
    (TESTDATA / 'mini-db', 'xc7', ['--tiles', 'first']),
    (TESTDATA / 'synthetic-db', 'xc7test-1', ['--tiles', 'sample', '3']),
    (TESTDATA / 'synthetic-db', 'xc7test-1', ['--tiles', 'all', '--seed',
                                              '5']),
]


@pytest.mark.parametrize('db,part,options', CASES)
def test_model_matches_rust(db, part, options, tmp_path):
    manifest = generate(db, part, tmp_path / 'a', *options)
    check_coverage(manifest)
    assert not manifest['uncovered']
    for name in manifest['files']:
        fasm = tmp_path / 'a' / name
        result = fasm2frames(db, part, fasm, tmp_path / 'out.frm', tmp_path)
        assert result.returncode == 0, result.stderr
        expected = fasm.with_suffix('.expected.frm').read_bytes()
        assert (tmp_path / 'out.frm').read_bytes() == expected, name
        result = fasm2frames(db, part, fasm, tmp_path / 'out.frm', tmp_path,
                             ['--emit_pudc_b_pullup'])
        assert result.returncode in (0, 1), result.stderr
    for name in manifest['errors']:
        result = fasm2frames(db, part, tmp_path / 'a' / 'errors' / name,
                             tmp_path / 'out.frm', tmp_path)
        assert result.returncode == 1, name


def test_deterministic(tmp_path):
    db = TESTDATA / 'mini-db'
    generate(db, 'xc7', tmp_path / 'a', '--seed', '3')
    generate(db, 'xc7', tmp_path / 'b', '--seed', '3')
    generate(db, 'xc7', tmp_path / 'c', '--seed', '4')
    names = sorted(p.name for p in (tmp_path / 'a').iterdir())
    assert names == sorted(p.name for p in (tmp_path / 'b').iterdir())
    for name in names:
        if name.endswith('.fasm'):
            a = (tmp_path / 'a' / name).read_bytes()
            assert a == (tmp_path / 'b' / name).read_bytes()
    a = (tmp_path / 'a' / 'features.fasm').read_bytes()
    assert a != (tmp_path / 'c' / 'features.fasm').read_bytes()


def test_list_parts():
    argv = [
        sys.executable,
        str(GENERATOR), '--db-root',
        str(TESTDATA / 'synthetic-db'), '--list-parts'
    ]
    out = subprocess.run(argv, check=True, capture_output=True,
                         text=True).stdout.split()
    assert out == ['xc7test-1', 'xc7nodev-1']


@pytest.mark.skipif(real_db() is None, reason='prjxray-db artix7 not fetched')
def test_model_matches_rust_real_part(tmp_path):
    db = real_db()
    part = 'xc7a35tcsg324-1'
    manifest = generate(db, part, tmp_path / 'a', '--tiles', 'first',
                        '--no-errors')
    check_coverage(manifest)
    assert not manifest['uncovered']
    assert not manifest['unreachable']
    # Both alias groups of both _SING IOB types have a STEPDOWN host.
    assert len(manifest['stepdown_hosts']) == 6
    for name in manifest['files']:
        fasm = tmp_path / 'a' / name
        result = fasm2frames(db, part, fasm, tmp_path / 'out.frm', tmp_path)
        assert result.returncode == 0, result.stderr
        expected = fasm.with_suffix('.expected.frm').read_bytes()
        assert (tmp_path / 'out.frm').read_bytes() == expected, name

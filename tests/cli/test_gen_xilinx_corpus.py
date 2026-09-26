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

# Same fallback chain as test_uray_corpus.find_db: $FASM_DB_CACHE, else
# <URAY_ORACLE_DIR or --uray-oracle-dir default>/build/db, else this
# checkout's tests/oracle/build/db.
ORACLE_DIR = Path(
    os.environ.get('URAY_ORACLE_DIR', ROOT / 'tests' / 'oracle'))


def real_db():
    for base in (os.environ.get('FASM_DB_CACHE'),
                 ORACLE_DIR.joinpath('build', 'db'),
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


# prjuray-db layout (T6.3): the model of prjuray's utils/fasm2frames.py
# (16-bit words, 186 per frame, bits past the frame end kept) against the
# Rust uray-fasm2frames.
URAY_RUST = RUST.parent / 'uray-fasm2frames'
USP_DB = TESTDATA / 'synthetic-usp-db'


def real_uray_db():
    for base in (os.environ.get('FASM_DB_CACHE'),
                 ORACLE_DIR.joinpath('build', 'db'),
                 ROOT.joinpath('tests', 'oracle', 'build', 'db')):
        if base and (Path(base) / 'prjuray-db' / 'zynqusp').is_dir():
            return Path(base) / 'prjuray-db' / 'zynqusp'
    return None


def uray_fasm2frames(db, part, fasm, frm, tmp_path, flags=('--sparse', )):
    env = dict(os.environ)
    env['FASM_XDB_CACHE'] = str(tmp_path / 'xdb')
    argv = [str(URAY_RUST), '--db-root', str(db), '--part', part]
    argv += list(flags) + [str(fasm), str(frm)]
    return subprocess.run(argv, env=env, capture_output=True)


@pytest.mark.skipif(not URAY_RUST.exists(),
                    reason='Rust uray-fasm2frames missing')
@pytest.mark.parametrize('options', [['--tiles', 'sample', '3'],
                                     ['--tiles', 'first'],
                                     ['--tiles', 'all', '--seed', '5']])
def test_uray_model_matches_rust(options, tmp_path):
    part = 'xcusptest-1'
    manifest = generate(USP_DB, part, tmp_path / 'a', *options)
    check_coverage(manifest)
    assert manifest['layout'] == 'prjuray'
    assert manifest['fabric'] is None
    # EDGE.OUT sets 16-bit word 186 of EDGE_X0Y0 (the only EDGE tile):
    # IndexError in prjuray's get_frames, never placed; EDGE.CLEAR_OUT
    # clears that word (stored, no error) and is placed.
    assert manifest['uncovered'] == [[
        'EDGE', 'EDGE_X0Y0', 'OUT',
        'sets a bit past the frame end on every tile'
    ]]
    assert not manifest['unreachable']
    assert not manifest['stepdown_hosts'] and manifest['pudc_b'] is None
    assert 'past_frame_end.fasm' in manifest['errors']
    text = ''.join((tmp_path / 'a' / name).read_text()
                   for name in manifest['files'])
    assert 'EDGE_X0Y0.CLEAR_OUT' in text
    assert 'RCLK_INT_L_X2Y29.' in text
    if options[1] != 'first':
        # The bottom half tile.
        assert 'CLEM_X1Y60.' in text
    for name in manifest['files']:
        fasm = tmp_path / 'a' / name
        result = uray_fasm2frames(USP_DB, part, fasm, tmp_path / 'out.frm',
                                  tmp_path)
        assert result.returncode == 0, result.stderr
        expected = fasm.with_suffix('.expected.frm').read_bytes()
        assert (tmp_path / 'out.frm').read_bytes() == expected, name
        # Each frame: 186 16-bit words.
        line = expected.decode().splitlines()[0]
        assert len(line.split(' ')[1].split(',')) == 186
    for name in manifest['errors']:
        result = uray_fasm2frames(USP_DB, part,
                                  tmp_path / 'a' / 'errors' / name,
                                  tmp_path / 'out.frm', tmp_path)
        assert result.returncode == 1, name
    past_end = tmp_path / 'a' / 'errors' / 'past_frame_end.fasm'
    result = uray_fasm2frames(USP_DB, part, past_end, tmp_path / 'out.frm',
                              tmp_path)
    assert result.stderr.decode().endswith(
        'IndexError: list index out of range\n'), result.stderr


def test_uray_list_parts():
    argv = [
        sys.executable,
        str(GENERATOR), '--db-root',
        str(USP_DB), '--list-parts'
    ]
    out = subprocess.run(argv, check=True, capture_output=True,
                         text=True).stdout.split()
    assert out == ['xcusptest-1']


def test_uray_deterministic(tmp_path):
    generate(USP_DB, 'xcusptest-1', tmp_path / 'a', '--seed', '3')
    generate(USP_DB, 'xcusptest-1', tmp_path / 'b', '--seed', '3')
    for name in sorted(p.name for p in (tmp_path / 'a').iterdir()):
        if name.endswith('.fasm'):
            assert (tmp_path / 'a' / name).read_bytes() == (
                tmp_path / 'b' / name).read_bytes()


@pytest.mark.skipif(real_uray_db() is None or not URAY_RUST.exists(),
                    reason='prjuray-db zynqusp not fetched')
def test_uray_model_matches_rust_real_part(tmp_path):
    db = real_uray_db()
    part = 'xczu3eg-sbva484-1-e'
    manifest = generate(db, part, tmp_path / 'a', '--tiles', 'first',
                        '--no-errors')
    check_coverage(manifest)
    assert not manifest['uncovered']
    # 34 segbits keys of BRAM, INT_INTF_LEFT_TERM_PSS and XIPHY_BYTE_RIGHT
    # have a part that starts with a digit (e.g. READ_WIDTH_A.36): not
    # FASM feature names.
    assert len(manifest['unreachable']) == 34
    assert len([c for c in manifest['coverage'].values()
                if c['placed']]) == 27
    for name in manifest['files']:
        fasm = tmp_path / 'a' / name
        result = uray_fasm2frames(db, part, fasm, tmp_path / 'out.frm',
                                  tmp_path)
        assert result.returncode == 0, result.stderr
        expected = fasm.with_suffix('.expected.frm').read_bytes()
        assert (tmp_path / 'out.frm').read_bytes() == expected, name

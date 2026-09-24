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
"""Command line compatibility test of the Rust `xcfasm` binary against
f4pga-xc-fasm's `xcfasm` (`xc_fasm.xc_fasm`, the oracle
`tests/oracle/xcfasm-oracle`, which runs the reference `xc7frames2bit`).

Runs both with the same arguments and environment (in a fresh working
directory each) and asserts identical exit codes, stdout, stderr and
files written (`--frm_out`, `--bit_out`, and the file named `None` the
reference writes without `--bit_out`), for argparse cases (help at many
terminal widths, missing, abbreviated and ambiguous options, the
`XRAY_*` defaults) and for runs on the artix7 prjxray-db (skipped
without it), including error cases.

The accepted differences (the `xcfasm` section of
`docs/rewrite/COMPAT.md`) are applied by `normalise()`: the oracle's
traceback frames are removed, parse error messages are compared up to
the message and database errors only have to fail on both sides (as for
`fasm2frames`); the `.bit` header time of the Rust tool is set to the
reference's with `SOURCE_DATE_EPOCH`. The in process bitstream writer
(`--frm2bit` ignored, no temporary `.frm` without `--frm_out`) is
checked on the Rust tool alone.

Run with the oracle venv's pytest after `cargo build --release -p
fasm-cli` (`make cli-difftest`); `XCFASM_ORACLE` and `XCFASM_RUST_CLI`
override the tools, `FASM_DB_CACHE` the database directory.
"""
import calendar
import os
import re
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = Path(
    os.environ.get('XCFASM_ORACLE',
                   ROOT / 'tests' / 'oracle' / 'xcfasm-oracle'))
RUST_CLI = Path(
    os.environ.get('XCFASM_RUST_CLI',
                   ROOT / 'target' / 'release' / 'xcfasm'))
ORACLE_BIN_DIR = ORACLE.parent / 'build' / 'xilinx' / 'bin'

if not (ORACLE.parent / 'venv-xilinx' / 'bin' / 'xcfasm').exists() or \
        not (ORACLE_BIN_DIR / 'xc7frames2bit').exists():
    pytest.skip('oracle xcfasm or xc7frames2bit missing (run '
                'tests/oracle/setup-xilinx.sh): {}'.format(ORACLE),
                allow_module_level=True)
if not RUST_CLI.exists():
    pytest.skip('Rust xcfasm binary missing (cargo build --release -p '
                'fasm-cli): {}'.format(RUST_CLI),
                allow_module_level=True)


def find_db():
    for base in (os.environ.get('FASM_DB_CACHE'), ORACLE.parent / 'build' /
                 'db', ROOT / 'tests' / 'oracle' / 'build' / 'db'):
        if base and (Path(base) / 'prjxray-db' / 'artix7').is_dir():
            return Path(base) / 'prjxray-db' / 'artix7'
    return None


DB = find_db()
PART = 'xc7a35tcsg324-1'
PART_FILE = str(DB / PART / 'part.yaml') if DB else '/nonexistent/part.yaml'
CORPUS = ROOT / 'tests' / 'corpus' / 'xilinx' / 'artix7'
SMOKE = str(CORPUS / 'smoke_x1y0.fasm')
SYNTHETIC = CORPUS / 'synthetic'
MINI_DB = str(ROOT / 'rust' / 'fasm-xilinx' / 'testdata' / 'mini-db')
LUT_INT = str(ROOT / 'tests' / 'corpus' / 'f4pga-xc-fasm' / 'lut_int.fasm')
# Replaced by a path in the fresh working directory of each tool.
TMP = '{tmp}'

XRAY_VARIABLES = ('XRAY_DATABASE_DIR', 'XRAY_DATABASE', 'XRAY_PART')

needs_db = pytest.mark.skipif(DB is None,
                              reason='prjxray-db artix7 not fetched')


def run(tool, argv, tmp_path, env=None, columns=None):
    full_env = dict(os.environ)
    for name in XRAY_VARIABLES + ('COLUMNS', 'LINES', 'SOURCE_DATE_EPOCH'):
        full_env.pop(name, None)
    full_env['PATH'] = str(ORACLE_BIN_DIR) + os.pathsep + full_env['PATH']
    full_env.update(env or {})
    if columns is not None:
        full_env['COLUMNS'] = columns
    tmp_path.mkdir(parents=True, exist_ok=True)
    argv = [a.replace(TMP, str(tmp_path)) for a in argv]
    result = subprocess.run([str(tool)] + argv,
                            cwd=str(tmp_path),
                            env=full_env,
                            stdin=subprocess.DEVNULL,
                            capture_output=True,
                            timeout=600)
    files = {
        name: (tmp_path / name).read_bytes()
        for name in sorted(os.listdir(tmp_path))
    }
    return (result.returncode, result.stdout,
            result.stderr.decode('utf-8', 'surrogateescape'), files)


def bit_time(files):
    for data in files.values():
        m = re.search(
            rb'c\x00\x0b(\d{4})/(\d\d)/(\d\d)\x00d\x00\x09'
            rb'(\d\d):(\d\d):(\d\d)\x00', data[:4096])
        if m:
            return calendar.timegm(tuple(int(g) for g in m.groups()))
    return None


def strip_traceback(stderr):
    out = []
    in_traceback = False
    for line in stderr.split('\n'):
        if line == 'Traceback (most recent call last):':
            in_traceback = True
            continue
        if in_traceback and line.startswith(' '):
            continue
        in_traceback = False
        out.append(line)
    return '\n'.join(out)


PARSE_ERROR_RE = re.compile(r'Exception: Parse error at (\d+):(\d+) - .*',
                            re.S)
DB_ERROR_RE = re.compile(r'^(AssertionError|fasm_xilinx\.DbError): ', re.M)


def normalise(result, tmp_path):
    code, stdout, stderr, files = result
    stderr = strip_traceback(stderr).replace(str(tmp_path), 'TMP')
    stderr = PARSE_ERROR_RE.sub(
        lambda m: 'Exception: Parse error at %s:%s - <message>\n' % m.groups(),
        stderr)
    if DB_ERROR_RE.search(stderr):
        stderr = '<database error>'
    tmp = str(tmp_path).encode()
    files = {k: v.replace(tmp, b'TMP') for k, v in files.items()}
    return code, stdout.replace(tmp, b'TMP'), stderr, files


def check(tmp_path, argv, env=None, columns=None):
    o_path, r_path = tmp_path / 'o', tmp_path / 'r'
    oracle = run(ORACLE, argv, o_path, env, columns)
    r_env = dict(env or {})
    epoch = bit_time(oracle[3])
    if epoch is not None:
        r_env['SOURCE_DATE_EPOCH'] = str(epoch)
    rust = run(RUST_CLI, argv, r_path, r_env, columns)
    oracle = normalise(oracle, o_path)
    rust = normalise(rust, r_path)
    assert rust[0] == oracle[0], 'exit code'
    assert rust[2] == oracle[2], 'stderr'
    assert rust[1] == oracle[1], 'stdout'
    assert rust[3] == oracle[3], 'files'


ARGPARSE_CASES = [
    [],
    ['-h'],
    ['--help'],
    ['--he'],
    ['--bogus'],
    ['--db-root', 'x'],
    ['--db-root', 'x', '--part', 'p'],
    ['--db-root', 'x', '--part_file', 'f'],
    ['--par', 'x'],
    ['--part_f', 'f', '--db', 'x', '--part', 'p', '-h'],
    ['--frm2bit'],
    ['--fn_in'],
    ['--sparse=1'],
    ['positional'],
    ['--db-root=x', '--part=p', '--part_file=f', 'extra'],
    ['--', '--sparse'],
    ['--debug', '--debug', '--sparse', '--sparse'],
]


@pytest.mark.parametrize('argv', ARGPARSE_CASES, ids=repr)
def test_argparse(tmp_path, argv):
    check(tmp_path, argv)


@pytest.mark.parametrize('columns',
                         ['1', '20', '40', '60', '79', '80', '100', 'abc'])
@pytest.mark.parametrize('argv', [['-h'], []], ids=repr)
def test_terminal_width(tmp_path, argv, columns):
    check(tmp_path, argv, columns=columns)


ENV_CASES = [
    {'XRAY_PART': 'xc7'},
    {'XRAY_DATABASE_DIR': '/a', 'XRAY_DATABASE': 'b'},
    {'XRAY_DATABASE_DIR': '/a', 'XRAY_DATABASE': 'b', 'XRAY_PART': 'p'},
]


@pytest.mark.parametrize('env', ENV_CASES, ids=repr)
@pytest.mark.parametrize('argv', [['-h'], []], ids=repr)
def test_environment(tmp_path, argv, env):
    check(tmp_path, argv, env=env)


DB_ARGS = ['--db-root', str(DB), '--part', PART, '--part_file', PART_FILE]
OUTS = ['--frm_out', TMP + '/out.frm', '--bit_out', TMP + '/out.bit']

RUN_CASES = [
    DB_ARGS + ['--fn_in', SMOKE] + OUTS,
    DB_ARGS + ['--fn_in', SMOKE, '--sparse'] + OUTS,
    DB_ARGS + ['--fn_in', SMOKE, '--sparse', '--debug'] + OUTS,
    DB_ARGS + ['--fn_in', SMOKE, '--emit_pudc_b_pullup'] + OUTS,
    DB_ARGS + [
        '--fn_in',
        str(SYNTHETIC / 'multibit_stepdown.fasm'), '--sparse', '--roi',
        str(SYNTHETIC / 'multibit_stepdown.roi.json')
    ] + OUTS,
    # Without --bit_out: a file named None.
    DB_ARGS + ['--fn_in', SMOKE, '--frm_out', TMP + '/out.frm'],
    DB_ARGS + ['--fn_in', SMOKE, '--frm_out', TMP + '/out.frm', '--bit_out',
               '/nonexistent/out.bit'],
    ['--db-root', str(DB), '--part', PART, '--part_file', '/nonexistent',
     '--fn_in', SMOKE] + OUTS,
    ['--db-root', str(DB), '--part', PART, '--part_file', SMOKE, '--fn_in',
     SMOKE] + OUTS,
    # A directory as part file: xc7frames2bit aborts.
    ['--db-root', str(DB), '--part', PART, '--part_file', '/tmp', '--fn_in',
     SMOKE] + OUTS,
    DB_ARGS + OUTS,
    DB_ARGS + ['--fn_in', '/nonexistent.fasm'] + OUTS,
    DB_ARGS + ['--fn_in', SMOKE, '--frm_out', '/nonexistent/out.frm',
               '--bit_out', TMP + '/out.bit'],
    DB_ARGS + ['--fn_in', str(SYNTHETIC / 'errors' / 'unknown_feature.fasm')]
    + OUTS,
    DB_ARGS + ['--fn_in', str(SYNTHETIC / 'errors' / 'parse_error.fasm')] +
    OUTS,
    ['--db-root', MINI_DB, '--part', 'nope', '--part_file', PART_FILE,
     '--fn_in', LUT_INT] + OUTS,
    # The miniature database's frames with a real part file.
    ['--db-root', MINI_DB, '--part', 'xc7', '--part_file', PART_FILE,
     '--fn_in', LUT_INT, '--sparse'] + OUTS,
]


@needs_db
@pytest.mark.parametrize('argv', RUN_CASES, ids=repr)
def test_run(tmp_path, argv):
    check(tmp_path, argv)


@needs_db
def test_without_frm_out(tmp_path):
    """Documented difference: no temporary .frm, the header names the
    FASM file."""
    code, _, stderr, files = run(RUST_CLI, DB_ARGS + [
        '--fn_in', SMOKE, '--bit_out', TMP + '/out.bit', '--frm2bit',
        '/nonexistent/tool'
    ], tmp_path)
    assert (code, stderr) == (0, '')
    assert list(files) == ['out.bit']
    assert (SMOKE + ';Generator=xc7frames2bit\0').encode() in \
        files['out.bit'][:200]

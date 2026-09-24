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
"""Command line compatibility test of the Rust `fasm2frames` binary
against the original f4pga-xc-fasm tool (`xc_fasm.fasm2frames`, the
oracle `tests/oracle/fasm2frames-oracle`).

Runs both with the same arguments and environment and asserts identical
exit codes, stdout, output file contents and stderr, for argparse cases
(help at many terminal widths, missing, unknown, abbreviated and
ambiguous options, `--`, the `XRAY_DATABASE_DIR`/`XRAY_DATABASE`/
`XRAY_PART` defaults) and for runs on the miniature database
(`rust/fasm-xilinx/testdata/mini-db`), including error cases.

The oracle runs as `python -m xc_fasm.fasm2frames`, so argparse calls it
`fasm2frames.py`; the Rust binary is run through a symbolic link of that
name (argparse's program name is the base name of `argv[0]`).

The accepted differences (the `fasm2frames` section of
`docs/rewrite/COMPAT.md`) are applied by `normalise()`: the oracle's
traceback frames are removed (the Rust tool prints only the last line,
`<exception>: <message>`), parse error messages are compared up to the
message, and database errors (which the oracle reports with assorted
exceptions, the Rust tool as `fasm_xilinx.DbError`) only have to fail on
both sides.

Run with the oracle venv's pytest after `cargo build --release -p
fasm-cli` (`make cli-difftest` runs it with the `fasm` CLI test);
`FASM2FRAMES_ORACLE` and `FASM2FRAMES_RUST_CLI` override the tools.
"""
import os
import re
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = Path(
    os.environ.get('FASM2FRAMES_ORACLE',
                   ROOT / 'tests' / 'oracle' / 'fasm2frames-oracle'))
RUST_CLI = Path(
    os.environ.get('FASM2FRAMES_RUST_CLI',
                   ROOT / 'target' / 'release' / 'fasm2frames'))

if not (ORACLE.parent / 'venv-xilinx' / 'bin' / 'python').exists():
    pytest.skip(
        'oracle venv-xilinx missing (run tests/oracle/setup-xilinx.sh): '
        '{}'.format(ORACLE),
        allow_module_level=True)
if not RUST_CLI.exists():
    pytest.skip(
        'Rust fasm2frames binary missing (cargo build --release -p '
        'fasm-cli): {}'.format(RUST_CLI),
        allow_module_level=True)

MINI_DB = str(ROOT / 'rust' / 'fasm-xilinx' / 'testdata' / 'mini-db')
LUT_INT = str(ROOT / 'tests' / 'corpus' / 'f4pga-xc-fasm' / 'lut_int.fasm')
STEPDOWN = str(ROOT / 'tests' / 'corpus' / 'f4pga-xc-fasm' / 'iob' /
               'liob_stepdown.fasm')
DB = ['--db-root', MINI_DB, '--part', 'xc7']
# Replaced by a path in a fresh directory for each tool.
OUT = '{out}'
BAD = '{bad}'

XRAY_VARIABLES = ('XRAY_DATABASE_DIR', 'XRAY_DATABASE', 'XRAY_PART')


@pytest.fixture(scope='module')
def rust_tool(tmp_path_factory):
    """The Rust binary, as `fasm2frames.py`."""
    link = tmp_path_factory.mktemp('bin') / 'fasm2frames.py'
    link.symlink_to(RUST_CLI)
    return link


def run(tool, argv, tmp_path, env=None, columns=None):
    full_env = dict(os.environ)
    for name in XRAY_VARIABLES + ('COLUMNS', 'LINES'):
        full_env.pop(name, None)
    full_env.update(env or {})
    if columns is not None:
        full_env['COLUMNS'] = columns
    tmp_path.mkdir(parents=True, exist_ok=True)
    out = tmp_path / 'out.frm'
    bad = tmp_path / 'bad.fasm'
    bad.write_text('CLBLM_L_X10Y102.SLICEM_X0.NOPE\nX_X1Y1.FOO\n')
    argv = [
        a.replace(OUT, str(out)).replace(BAD, str(bad)) for a in argv
    ]
    result = subprocess.run([str(tool)] + argv,
                            cwd=str(ROOT),
                            env=full_env,
                            stdin=subprocess.DEVNULL,
                            capture_output=True,
                            timeout=600)
    frm = out.read_bytes() if out.exists() else None
    return (result.returncode, result.stdout,
            result.stderr.decode('utf-8', 'surrogateescape'), frm)


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


def normalise(result):
    code, stdout, stderr, frm = result
    stderr = strip_traceback(stderr)
    stderr = PARSE_ERROR_RE.sub(
        lambda m: 'Exception: Parse error at %s:%s - <message>\n' % m.groups(),
        stderr)
    if DB_ERROR_RE.search(stderr):
        stderr = '<database error>'
    return code, stdout, stderr, frm


def check(rust_tool, tmp_path, argv, env=None, columns=None):
    oracle = normalise(run(ORACLE, argv, tmp_path / 'o', env, columns))
    rust = normalise(run(rust_tool, argv, tmp_path / 'r', env, columns))
    assert rust[0] == oracle[0], 'exit code'
    assert rust[2] == oracle[2], 'stderr'
    assert rust[1] == oracle[1], 'stdout'
    assert rust[3] == oracle[3], 'output file'


ARGPARSE_CASES = [
    [],
    ['-h'],
    ['--help'],
    ['--h'],
    ['--he'],
    ['-h', '--bogus'],
    ['--bogus', '-h'],
    ['--db-root'],
    ['--db-root', 'x'],
    ['--db-root=x', '--part=y'],
    ['--part', 'p', 'a'],
    ['--db-root', 'x', '--part', 'p'],
    ['--d', 'x'],
    ['--de'],
    ['--db', 'x', '--pa', 'p'],
    ['--db_root', 'x', '--part', 'p', 'a'],
    ['--bogus'],
    ['-x'],
    ['-p', 'x'],
    ['--sparse=1'],
    ['--sparse='],
    ['--roi'],
    ['--roi', '--sparse'],
    ['--roi', '-h'],
    ['a', 'b', 'c'],
    ['--db-root', 'x', '--part', 'p', 'a', '--sparse', 'b'],
    ['--db-root', 'x', '--part', 'p', 'a', 'b', 'c'],
    ['--db-root', 'x', '--part', 'p', '--', '--sparse'] + [OUT],
    ['--db-root', 'x', '--part', 'p', '--'],
    ['--', '-h'],
    ['--emit', 'a'],
    ['--em=1', 'a'],
    ['-hx'],
    ['--debug', '--debug', '--sparse', '--sparse'],
]


@pytest.mark.parametrize('argv', ARGPARSE_CASES, ids=repr)
def test_argparse(rust_tool, tmp_path, argv):
    check(rust_tool, tmp_path, argv)


@pytest.mark.parametrize('columns',
                         ['1', '10', '20', '30', '40', '50', '60', '70', '79',
                          '80', '100', '200', 'abc'])
@pytest.mark.parametrize('argv', [['-h'], []], ids=repr)
def test_terminal_width(rust_tool, tmp_path, argv, columns):
    check(rust_tool, tmp_path, argv, columns=columns)


ENV_CASES = [
    {'XRAY_PART': 'xc7'},
    {'XRAY_PART': ''},
    {'XRAY_DATABASE_DIR': '/a', 'XRAY_DATABASE': 'b'},
    {'XRAY_DATABASE_DIR': '/a/', 'XRAY_DATABASE': '/b'},
    {'XRAY_DATABASE_DIR': '', 'XRAY_DATABASE': 'b'},
    {'XRAY_DATABASE': 'b'},
    {'XRAY_DATABASE_DIR': '/a'},
    {'XRAY_DATABASE_DIR': '/a', 'XRAY_DATABASE': 'b', 'XRAY_PART': 'p'},
]


@pytest.mark.parametrize('env', ENV_CASES, ids=repr)
@pytest.mark.parametrize('argv', [['-h'], [], ['a']], ids=repr)
def test_environment(rust_tool, tmp_path, argv, env):
    check(rust_tool, tmp_path, argv, env=env)


def test_environment_defaults_run(rust_tool, tmp_path):
    """`--db-root` and `--part` from the environment."""
    parent, name = os.path.split(MINI_DB)
    env = {
        'XRAY_DATABASE_DIR': parent,
        'XRAY_DATABASE': name,
        'XRAY_PART': 'xc7'
    }
    check(rust_tool, tmp_path, [LUT_INT, OUT], env=env)


RUN_CASES = [
    DB + [LUT_INT, OUT],
    DB + ['--sparse', LUT_INT, OUT],
    DB + ['--sparse', STEPDOWN, OUT],
    DB + ['--emit_pudc_b_pullup', '--sparse', STEPDOWN, OUT],
    DB + ['--sparse', '--debug', STEPDOWN, OUT],
    # fn_out defaults to /dev/stdout.
    DB + ['--sparse', LUT_INT],
    DB + ['--sparse', '--', LUT_INT, OUT],
    DB + ['--roi', '', '--sparse', LUT_INT, OUT],
    # Errors.
    DB + [BAD, OUT],
    DB + ['/nonexistent/file.fasm', OUT],
    DB + [LUT_INT, '/nonexistent/dir/out.frm'],
    DB + [LUT_INT, str(ROOT)],
    DB + ['--roi', '/nonexistent/roi.json', LUT_INT, OUT],
    ['--db-root', MINI_DB, '--part', 'nope', LUT_INT, OUT],
    ['--db-root', '/nonexistent', '--part', 'xc7', LUT_INT, OUT],
]


@pytest.mark.parametrize('argv', RUN_CASES, ids=repr)
def test_run(rust_tool, tmp_path, argv):
    check(rust_tool, tmp_path, argv)

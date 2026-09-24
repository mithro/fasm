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
"""Command line compatibility test of the Rust `xc7frames2bit` binary
against prjxray's C++ tool (the oracle `tests/oracle/xc7frames2bit-oracle`,
built by `tests/oracle/setup-xilinx.sh`).

Runs both with the same arguments and asserts identical exit codes,
stdout, stderr and output file, for the gflags command line (help flags,
unknown flags, missing and illegal values, `--undefok`, `--fromenv`,
permutation, `--`) and for runs on the artix7 `part.yaml` of the fetched
prjxray-db (skipped without it) with good and malformed `.frm` files.

The accepted differences (the `xc7frames2bit` section of
`docs/rewrite/COMPAT.md`) are applied by `normalise()`: the program path
the reference prints (`argv[0]`) and the absolute source paths of its
help (`Flags from /.../prjxray/tools/xc7frames2bit.cc`) are replaced; a
SIGABRT of the reference (an uncaught `std::stoul` exception) is exit
code 134 like the Rust tool's. The `.bit` header time of the Rust tool
is set to the reference's with `SOURCE_DATE_EPOCH`, so the `.bit` files
are compared byte for byte.

Run with the oracle venv's pytest after `cargo build --release -p
fasm-cli` (`make cli-difftest`); `XC7FRAMES2BIT_ORACLE` and
`XC7FRAMES2BIT_RUST_CLI` override the tools, `FASM_DB_CACHE` the
database directory.
"""
import calendar
import lzma
import os
import re
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = Path(
    os.environ.get('XC7FRAMES2BIT_ORACLE',
                   ROOT / 'tests' / 'oracle' / 'xc7frames2bit-oracle'))
RUST_CLI = Path(
    os.environ.get('XC7FRAMES2BIT_RUST_CLI',
                   ROOT / 'target' / 'release' / 'xc7frames2bit'))
ORACLE_BIN = ORACLE.parent / 'build' / 'xilinx' / 'bin' / 'xc7frames2bit'

if not ORACLE_BIN.exists():
    pytest.skip('oracle xc7frames2bit missing (run '
                'tests/oracle/setup-xilinx.sh): {}'.format(ORACLE_BIN),
                allow_module_level=True)
if not RUST_CLI.exists():
    pytest.skip('Rust xc7frames2bit binary missing (cargo build --release -p '
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
SMOKE = str(CORPUS / 'smoke_x1y0.frm')
COUNTER = CORPUS / 'designs' / 'f4pga-examples' / 'counter_test' / 'arty_35'
# Replaced by paths in a fresh directory for each tool.
OUT = '{out}'
TMP = '{tmp}'

needs_db = pytest.mark.skipif(DB is None,
                              reason='prjxray-db artix7 not fetched')

WORDS = ','.join(['0x00000000'] * 101)
ONE = ','.join(['0x00000001'] * 101)
FRM_FILES = {
    'bad_count.frm': '0x00400100 0x1,0x2\n0x00400101 ' + ONE + '\n',
    'bad_hex.frm': '0x00400100 ' + ONE + '\nzz ' + WORDS + '\n',
    'bad_word.frm': '0x00400100 ' + ONE.replace('0x00000001', 'q', 1) + '\n',
    'overflow.frm': '0x10000000000000000 ' + WORDS + '\n',
    'empty_line.frm': '0x00400100 ' + ONE + '\n\n0x00400101 ' + ONE + '\n',
    'comments.frm': '# comment\n0x00400100 ' + ONE + '\r\n# end\n',
    'extra_frames.frm': ('0x0FFFFFFF ' + ONE + '\n0x00000010 ' + ONE +
                         '\n0x00400100 ' + ONE + '\n0x00400100 ' + WORDS +
                         '\n'),
    'ecc_bits.frm': '0x00400100 ' + ','.join(['0xFFFFFFFF'] * 101) + '\n',
    'empty.frm': '',
}


def run(tool, argv, tmp_path, env=None):
    tmp_path.mkdir(parents=True, exist_ok=True)
    for name, text in FRM_FILES.items():
        (tmp_path / name).write_text(text)
    with lzma.open(COUNTER / 'top.sparse.frm.xz') as f:
        (tmp_path / 'counter.frm').write_bytes(f.read())
    out = tmp_path / 'out.bit'
    argv = [
        a.replace(OUT, str(out)).replace(TMP, str(tmp_path)) for a in argv
    ]
    full_env = dict(os.environ)
    full_env.pop('SOURCE_DATE_EPOCH', None)
    full_env.update(env or {})
    result = subprocess.run([str(tool)] + argv,
                            cwd=str(tmp_path),
                            env=full_env,
                            stdin=subprocess.DEVNULL,
                            capture_output=True,
                            timeout=600)
    bit = out.read_bytes() if out.exists() else None
    code = 134 if result.returncode == -6 else result.returncode
    return code, result.stdout, result.stderr, bit


def bit_time(bit):
    m = re.search(
        rb'c\x00\x0b(\d{4})/(\d\d)/(\d\d)\x00d\x00\x09(\d\d):(\d\d):(\d\d)\x00',
        bit or b'')
    return calendar.timegm(tuple(int(g)
                                 for g in m.groups())) if m else None


def normalise(result, tmp_path):
    code, stdout, stderr, bit = result
    tmp = str(tmp_path).encode()

    def text(b):
        b = b.replace(tmp, b'TMP')
        b = re.sub(rb'(Flags from |<file>)/[^\n<]*?/prjxray/', rb'\1', b)
        b = re.sub(rb"/[^\s:'<]*/xc7frames2bit\b", b'PROG', b)
        return b

    if bit is not None:
        bit = bit.replace(tmp, b'TMP')
    return code, text(stdout), text(stderr), bit


def check(tmp_path, argv, env=None):
    o_path, r_path = tmp_path / 'o', tmp_path / 'r'
    oracle = run(ORACLE, argv, o_path, env)
    epoch = bit_time(oracle[3])
    r_env = dict(env or {})
    if epoch is not None:
        r_env['SOURCE_DATE_EPOCH'] = str(epoch)
    rust = run(RUST_CLI, argv, r_path, r_env)
    oracle = normalise(oracle, o_path)
    rust = normalise(rust, r_path)
    assert rust[0] == oracle[0], 'exit code'
    assert rust[2] == oracle[2], 'stderr'
    assert rust[1] == oracle[1], 'stdout'
    assert rust[3] == oracle[3], 'output file'


FLAG_CASES = [
    [],
    ['--help'],
    ['-help'],
    ['--helpful'],
    ['--helpshort'],
    ['--helpshort', '--part_name', 'x', '--frm_file=y'],
    ['--helpon=xc7frames2bit'],
    ['--helpon=gflags'],
    ['--helpon=nothing'],
    ['--helpmatch=reporting'],
    ['--helpmatch=zzz'],
    ['--helppackage'],
    ['--helpxml'],
    ['--version'],
    ['--help=false', '--version'],
    ['--nohelp', '--nohelpshort'],
    ['--bogus'],
    ['--zz', '--aa', '--part_file'],
    ['--part_file'],
    ['--nopart_file'],
    ['--noflag'],
    ['--help=foo'],
    ['--help=YES'],
    ['--tab_completion_columns=abc'],
    ['--tab_completion_columns', '2147483648'],
    ['--undefok=bogus', '--bogus'],
    ['--undefok=nobogus', '--nobogus'],
    ['--undefok=,'],
    ['--undefok=-x'],
    ['--fromenv=part_file'],
    ['--tryfromenv=part_file'],
    ['--fromenv=nothing'],
    ['-', '--', '--bogus'],
    ['--part_file=/nonexistent'],
    ['--architecture=Foo', '--part_file=/nonexistent'],
    ['positional', '--part_file', '/nonexistent'],
]


@pytest.mark.parametrize('argv', FLAG_CASES, ids=repr)
def test_flags(tmp_path, argv):
    check(tmp_path, argv)


def test_fromenv_values(tmp_path):
    check(tmp_path, ['--fromenv=part_file,part_name'],
          env={
              'FLAGS_part_file': '/nonexistent',
              'FLAGS_part_name': 'x'
          })
    check(tmp_path, ['--tryfromenv=part_file'],
          env={'FLAGS_part_file': 'fromenv'})


BASE = ['--part_file=' + PART_FILE, '--output_file=' + OUT]

RUN_CASES = [
    BASE + ['--frm_file=' + SMOKE, '--part_name=' + PART],
    BASE + ['--frm_file', SMOKE, '--part_name', PART, '--architecture=Foo'],
    BASE + ['--frm_file=' + TMP + '/counter.frm', '--part_name=x'],
    ['--frm_file=' + SMOKE, 'extra', '--part_file', PART_FILE, 'args',
     '--output_file', OUT],
    BASE,
    BASE + ['--frm_file=/nonexistent.frm'],
    BASE + ['--frm_file=' + TMP],
    ['--part_file=' + PART_FILE, '--frm_file=' + SMOKE],
    ['--part_file=' + PART_FILE, '--frm_file=' + SMOKE,
     '--output_file=/nonexistent/out.bit'],
    ['--part_file=' + SMOKE, '--frm_file=' + SMOKE, '--output_file=' + OUT],
] + [BASE + ['--frm_file=' + TMP + '/' + name] for name in sorted(FRM_FILES)]


@needs_db
@pytest.mark.parametrize('argv', RUN_CASES, ids=repr)
def test_run(tmp_path, argv):
    check(tmp_path, argv)


@needs_db
def test_other_part_file(tmp_path):
    """A part.yaml of another part (no IDCODE check in the reference)."""
    other = str(DB / 'xc7a200tffg1156-1' / 'part.yaml')
    check(tmp_path, [
        '--part_file=' + other, '--frm_file=' + SMOKE, '--output_file=' + OUT
    ])


@needs_db
@pytest.mark.parametrize('arch', ['UltraScale', 'UltraScalePlus', 'Spartan6'])
def test_unsupported_architecture(tmp_path, arch):
    """Documented difference: only Series7 is implemented."""
    code, _, stderr, bit = run(
        RUST_CLI, BASE + ['--frm_file=' + SMOKE, '--architecture=' + arch],
        tmp_path)
    assert code == 1
    assert b'not supported yet' in stderr
    assert bit is None

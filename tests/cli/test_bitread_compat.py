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
"""Command line compatibility test of the Rust `bitread` binary against
prjxray's C++ tool (the oracle `tests/oracle/bitread-oracle`, built by
`tests/oracle/setup-xilinx.sh`).

Runs both with the same arguments (and stdin) and asserts identical exit
codes, stdout, stderr and output files (`-o`, `--aux`), for the gflags
command line and for every output mode (`-x`, `-y`, `-p`, the default
hex dump, with `-o`, `-z`, `-C`, `-f`, `-F`, `--aux`) on the golden
`tests/corpus/xilinx/artix7/smoke_x1y0.bit` with the artix7 `part.yaml`
of the fetched prjxray-db (skipped without it), and on malformed input.

The accepted differences (the `bitread` section of
`docs/rewrite/COMPAT.md`) are applied by `normalise()`: the program path
(`argv[0]`) and the absolute source paths of the help are replaced, and
the section of the Rust only `--frm_out` flag is removed from the full
help. The other documented differences (trailing bytes after the last
whole word, the architectures other than Series7) are checked on the
Rust tool alone.

Run with the oracle venv's pytest after `cargo build --release -p
fasm-cli` (`make cli-difftest`); `BITREAD_ORACLE` and
`BITREAD_RUST_CLI` override the tools, `FASM_DB_CACHE` the database
directory.
"""
import os
import re
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = Path(
    os.environ.get('BITREAD_ORACLE',
                   ROOT / 'tests' / 'oracle' / 'bitread-oracle'))
RUST_CLI = Path(
    os.environ.get('BITREAD_RUST_CLI',
                   ROOT / 'target' / 'release' / 'bitread'))
ORACLE_BIN = ORACLE.parent / 'build' / 'xilinx' / 'bin' / 'bitread'

if not ORACLE_BIN.exists():
    pytest.skip('oracle bitread missing (run tests/oracle/setup-xilinx.sh): '
                '{}'.format(ORACLE_BIN),
                allow_module_level=True)
if not RUST_CLI.exists():
    pytest.skip('Rust bitread binary missing (cargo build --release -p '
                'fasm-cli): {}'.format(RUST_CLI),
                allow_module_level=True)


def find_db():
    for base in (os.environ.get('FASM_DB_CACHE'), ORACLE.parent / 'build' /
                 'db', ROOT / 'tests' / 'oracle' / 'build' / 'db'):
        if base and (Path(base) / 'prjxray-db' / 'artix7').is_dir():
            return Path(base) / 'prjxray-db' / 'artix7'
    return None


DB = find_db()
PART_FILE = str(DB / 'xc7a35tcsg324-1' /
                'part.yaml') if DB else '/nonexistent/part.yaml'
OTHER_PART_FILE = str(DB / 'xc7a200tffg1156-1' /
                      'part.yaml') if DB else '/nonexistent/part.yaml'
SMOKE = ROOT / 'tests' / 'corpus' / 'xilinx' / 'artix7' / 'smoke_x1y0.bit'
# Replaced by paths in a fresh directory for each tool.
TMP = '{tmp}'

needs_db = pytest.mark.skipif(DB is None,
                              reason='prjxray-db artix7 not fetched')


def inputs():
    smoke = SMOKE.read_bytes()
    sync = smoke.index(b'\xaa\x99\x55\x66')
    return {
        'smoke.bit': smoke,
        'empty.bit': b'',
        'garbage.bit': b'hello world\n' * 10,
        # Only a sync word and a few packets.
        'short.bit': smoke[:sync + 4 + 4 * 40],
        # A truncated FDRI packet: no frames.
        'truncated.bit': smoke[:sync + 4 + (len(smoke) - sync) // 8 * 4],
        # Header only, the sync word at an odd offset.
        'odd.bit': b'x' + smoke[sync:sync + 4 + 4 * 4],
        # Words of header type 3 end the packets.
        'type3.bit': smoke[:sync + 4 + 4 * 30] + b'\x60\x00\x00\x00' +
        smoke[sync + 4 + 4 * 30:],
    }


def run(tool, argv, tmp_path, stdin=None):
    tmp_path.mkdir(parents=True, exist_ok=True)
    for name, data in inputs().items():
        (tmp_path / name).write_bytes(data)
    argv = [a.replace(TMP, str(tmp_path)) for a in argv]
    stdin_data = (tmp_path / stdin).read_bytes() if stdin else b''
    result = subprocess.run([str(tool)] + argv,
                            cwd=str(tmp_path),
                            input=stdin_data,
                            capture_output=True,
                            timeout=600)
    files = {}
    for name in sorted(os.listdir(tmp_path)):
        if name.startswith('out'):
            files[name] = (tmp_path / name).read_bytes()
    code = 134 if result.returncode == -6 else result.returncode
    return code, result.stdout, result.stderr, files


EXTENSION = re.compile(
    rb'\n  Flags from rust/fasm-cli/src/bitread_extensions\.rs:\n'
    rb'(?:    .*\n|      .*\n)*\n\n')
EXTENSION_XML = re.compile(
    rb'<flag><file>rust/fasm-cli/src/bitread_extensions\.rs</file>.*\n')


def normalise(result, tmp_path, rust):
    code, stdout, stderr, files = result
    tmp = str(tmp_path).encode()

    def text(b):
        b = b.replace(tmp, b'TMP')
        b = re.sub(rb'(Flags from |<file>)/[^\n<]*?/prjxray/', rb'\1', b)
        b = re.sub(rb"/[^\s:'<]*/bitread\b", b'PROG', b)
        if rust:
            b = EXTENSION.sub(b'', b)
            b = EXTENSION_XML.sub(b'', b)
        return b

    return code, text(stdout), text(stderr), files


def check(tmp_path, argv, stdin=None):
    o_path, r_path = tmp_path / 'o', tmp_path / 'r'
    oracle = normalise(run(ORACLE, argv, o_path, stdin), o_path, False)
    rust = normalise(run(RUST_CLI, argv, r_path, stdin), r_path, True)
    assert rust[0] == oracle[0], 'exit code'
    assert rust[2] == oracle[2], 'stderr'
    assert rust[1] == oracle[1], 'stdout'
    assert rust[3] == oracle[3], 'output files'


FLAG_CASES = [
    ['--help'],
    ['--helpshort'],
    ['--helpshort', '-x', '-f', '12', '--part_file=abc', '-F='],
    ['--helpon=bitread'],
    ['--helpxml'],
    ['--helppackage'],
    ['--version'],
    ['-f=abc'],
    ['-f', '0x80000000'],
    ['-f'],
    ['-x=maybe'],
    ['-nox', '-noy', '--bogus'],
    ['-noF'],
]


@pytest.mark.parametrize('argv', FLAG_CASES, ids=repr)
def test_flags(tmp_path, argv):
    check(tmp_path, argv)


P = '--part_file=' + PART_FILE
SMOKE_T = TMP + '/smoke.bit'

RUN_CASES = [
    [P, '-z', '-y', SMOKE_T],
    [P, '-z', '-y', '-o', TMP + '/out.txt', SMOKE_T],
    [P, '-z', '-x', '-o', TMP + '/out.txt', SMOKE_T],
    [P, '-z', '-x', '-F', '0x00400100:0x00400121', SMOKE_T],
    [P, '-x', '-F', '0x00400100', SMOKE_T],
    [P, '-y', '-C', '-F', '0x00400100:0x0040011f', '-o', TMP + '/out.txt',
     SMOKE_T],
    [P, '-y', '-F', '0x00400000:', SMOKE_T],
    [P, '-y', '-F', '4194560:4194570:9', SMOKE_T],
    [P, '-y', '-F', '010:0x', SMOKE_T],
    [P, '-f', '0x0040010b', SMOKE_T],
    [P, '-f', '4194571', '-C', SMOKE_T],
    [P, '-f', '4194571', '-o', TMP + '/out.txt', SMOKE_T],
    [P, '-f', '-2', '-z', '-o', TMP + '/out.txt', SMOKE_T],
    [P, '-z', '-o', TMP + '/out.txt', SMOKE_T],
    [P, '-z', '-p', '-f', '4194571', SMOKE_T],
    [P, '-p', '-F', '0x00400100:0x00400180', '-o', TMP + '/out.pgm', SMOKE_T],
    [P, '-p', '-z', '-o', TMP + '/out.pgm', SMOKE_T],
    [P, '-z', '-y', '--aux', TMP + '/out.aux', SMOKE_T],
    [P, '-z', '-y', '--aux', '/nonexistent/aux.txt', SMOKE_T],
    [P, '-z', '-o', '/nonexistent/out.txt', SMOKE_T],
    [P, '-c', '-z', '-y', SMOKE_T, '--architecture=Foo'],
    [P, SMOKE_T, SMOKE_T],
    [P],
    [P, '/nonexistent.bit'],
    [P, TMP],
    ['--part_file=/nonexistent', SMOKE_T],
    # A directory: the reference aborts (std::__ios_failure).
    ['--part_file=/tmp', SMOKE_T],
    # Files whose size is 0 (/proc) read as empty.
    [P, '/proc/version'],
    ['--part_file=' + SMOKE_T, SMOKE_T],
    ['--part_file=' + OTHER_PART_FILE, '-z', '-y', SMOKE_T],
    ['-y', PART_FILE],
] + [[P, '-z', '-y', '--aux', TMP + '/out.aux', TMP + '/' + name]
     for name in sorted(inputs()) if name not in ('smoke.bit', )]


@needs_db
@pytest.mark.parametrize('argv', RUN_CASES, ids=repr)
def test_run(tmp_path, argv):
    check(tmp_path, argv)


@needs_db
@pytest.mark.parametrize('flags', [['-z', '-y'], ['-f', '4194571', '-C']],
                         ids=repr)
def test_stdin(tmp_path, flags):
    check(tmp_path, [P] + flags, stdin='smoke.bit')


@needs_db
def test_trailing_bytes(tmp_path):
    """Documented difference: bytes after the last whole word are ignored
    (the reference aborts with std::out_of_range)."""
    (tmp_path / 'in.bit').write_bytes(SMOKE.read_bytes()[:-1])
    full = run(RUST_CLI, [P, '-z', '-y', SMOKE_T], tmp_path / 'full')
    cut = subprocess.run(
        [str(RUST_CLI), P, '-z', '-y',
         str(tmp_path / 'in.bit')],
        capture_output=True)
    assert cut.returncode == 0
    assert full[0] == 0
    # The last word (a NOP) is incomplete: one word less, the same frames.
    assert cut.stdout == full[1].replace(b'2192122 bytes',
                                         b'2192121 bytes').replace(
                                             b'547990 words', b'547989 words')


@needs_db
def test_frm_out_extension(tmp_path):
    """`--frm_out`: the non zero frames without the ECC bits, i.e. the
    frames of the golden sparse .frm that have bits set."""
    frm = tmp_path / 'out.frm'
    result = subprocess.run(
        [str(RUST_CLI), P, '-z', '--frm_out=' + str(frm),
         str(SMOKE)],
        capture_output=True)
    assert result.returncode == 0
    golden = (SMOKE.parent / 'smoke_x1y0.frm').read_text().splitlines()
    zero = ','.join(['0x00000000'] * 101)
    expected = [line for line in golden if not line.endswith(' ' + zero)]
    assert frm.read_text().splitlines() == expected


@pytest.mark.parametrize('arch', ['UltraScale', 'UltraScalePlus', 'Spartan6'])
def test_unsupported_architecture(tmp_path, arch):
    """Documented difference: only Series7 is implemented."""
    result = subprocess.run(
        [str(RUST_CLI), '--architecture=' + arch,
         str(SMOKE)],
        capture_output=True)
    assert result.returncode == 1
    assert b'not supported yet' in result.stderr

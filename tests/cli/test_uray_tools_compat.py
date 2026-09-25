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
"""Command line compatibility test of the Rust prjuray tools against the
prjuray references (T6.2):

* `xcframes2bit` against prjuray-tools' C++ `xcframes2bit`
  (`tests/oracle/uray-xcframes2bit-oracle`);
* `uray-bitread` against prjuray-tools' C++ `bitread`
  (`tests/oracle/uray-bitread-oracle`);
* `uray-fasm2frames` against prjuray's `utils/fasm2frames.py`
  (`tests/oracle/uray-fasm2frames-oracle`).

Both sides run with the same arguments; exit codes, stdout, stderr and
the output files must be identical, on the gflags / argparse command
lines and on runs with the synthetic UltraScale+ database
(`rust/fasm-xilinx/testdata/synthetic-usp-db`, no download needed),
including malformed `.frm` and `.bit` inputs and the ECC verification.

Accepted differences (the `xcframes2bit`, `uray-bitread` and
`uray-fasm2frames` sections of `docs/rewrite/COMPAT.md`), applied by
`normalise()`: the program path and the absolute source paths of the
gflags help are replaced, a SIGABRT is exit code 134 on both sides, the
`.bit` header time of the Rust tool is the reference's
(`SOURCE_DATE_EPOCH`), and the Python traceback frames of the reference
`fasm2frames.py` are removed (the Rust tool prints the last line).

Run with the oracle venv's pytest after `cargo build --release -p
fasm-cli`. `URAY_ORACLE_DIR` selects the `tests/oracle` directory whose
`build/` and `venv-xilinx/` the oracle wrappers use (e.g. the main
checkout's, from a worktree), `URAY_RUST_DIR` the Rust binaries.
"""
import calendar
import os
import re
import shutil
import struct
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE_WRAPPERS = ROOT / 'tests' / 'oracle'
ORACLE_DIR = Path(os.environ.get('URAY_ORACLE_DIR', ORACLE_WRAPPERS))
RUST_DIR = Path(
    os.environ.get('URAY_RUST_DIR', ROOT / 'target' / 'release'))
USP_DB = ROOT / 'rust' / 'fasm-xilinx' / 'testdata' / 'synthetic-usp-db'
PART = 'xcusptest-1'
PART_FILE = str(USP_DB / PART / 'part.yaml')

if not (ORACLE_DIR / 'build' / 'xilinx' / 'bin' / 'uray-bitread').exists():
    pytest.skip(
        'prjuray oracle tools missing (tests/oracle/setup-xilinx.sh): '
        '{}'.format(ORACLE_DIR),
        allow_module_level=True)
for tool in ('xcframes2bit', 'uray-bitread', 'uray-fasm2frames'):
    if not (RUST_DIR / tool).exists():
        pytest.skip(
            'Rust {} missing (cargo build --release -p fasm-cli)'.format(tool),
            allow_module_level=True)

TOOLS = {
    'xcframes2bit': ('uray-xcframes2bit-oracle', 'xcframes2bit'),
    'bitread': ('uray-bitread-oracle', 'uray-bitread'),
    'fasm2frames': ('uray-fasm2frames-oracle', 'uray-fasm2frames'),
}

# Replaced by paths in a fresh directory for each side.
TMP = '{tmp}'
OUT = '{tmp}/out'

ZERO = ','.join(['0x00000000'] * 93)
ONE = ','.join(['0x00000001'] * 93)
FRM_FILES = {
    'good.frm': ''.join('%s %s\n' % (address, ONE)
                        for address in ('0x00000100', '0x01000005',
                                        '0x00800100')),
    'invalid_frame.frm': '0x00000100 ' + ONE + '\n0x000009FF ' + ONE + '\n',
    'invalid_zero.frm': '0x00000000 ' + ONE + '\n0x07000000 ' + ZERO + '\n',
    'count.frm': '0x00000100 0x1,0x2\n0x00000101 ' + ONE + '\n',
    'bad_hex.frm': '0x00000100 ' + ONE + '\nzz ' + ZERO + '\n',
    'bad_word.frm': '0x00000100 ' + ONE.replace('0x00000001', 'q', 1) + '\n',
    'empty_line.frm': '0x00000100 ' + ONE + '\n\n',
    'ecc_bits.frm': '0x00000101 ' + ','.join(['0xFFFFFFFF'] * 93) + '\n',
    'comments.frm': '# c\n0x00000102 ' + ONE + '\r\n',
    's7.frm': '0x00000100 ' + ','.join(['0x00000001'] * 101) + '\n',
    'empty.frm': '',
}

FASM_FILES = {
    'design.fasm': ("CLEM_X1Y0.ALUT.INIT[15:0] = 16'hA5C3\n"
                    'CLEM_X1Y1.ABCDFF.CEUSED.V1\n'
                    "BRAM_X2Y0.RAMB18E2_L.INIT_00[7:0] = 8'hFF\n"
                    'BRAM_X2Y0.RAMB18E2_L.CLKARDCLKINV.V1\n'
                    'RCLK_INT_L_X2Y29.BUFCE_LEAF_X0Y0.BUFCE_LEAF.'
                    'DELAY_TAP.V0\n'
                    'EDGE_X0Y0.OK\n'),
    'out_of_frame.fasm': 'EDGE_X0Y0.OUT\n',
    'clear_out.fasm': 'EDGE_X0Y0.CLEAR_OUT\n',
    'conflict.fasm': 'CLEM_X1Y1.AFF.INIT.V0\nCLEM_X1Y1.AFF.INIT.V1\n',
    'lookup.fasm': 'CLEM_X1Y1.NOPE\nCLEM_X1Y1.ALUT.INIT[40]\n',
    'tile.fasm': 'CLEM_X1Y1.NOPE\nFOO_X1.BAR\n',
    'syntax.fasm': 'CLEM_X1Y1.AFF.INIT.V0 = = 1\n',
    'roi.json': ('{"info": {"GRID_X_MIN": 2, "GRID_X_MAX": 3, '
                 '"GRID_Y_MIN": 0, "GRID_Y_MAX": 60}, '
                 '"required_features": '
                 '["BRAM_X2Y0.RAMB18E2_L.INIT_00[3]"]}\n'),
}


def make_inputs(tmp_path):
    tmp_path.mkdir(parents=True, exist_ok=True)
    for name, text in list(FRM_FILES.items()) + list(FASM_FILES.items()):
        (tmp_path / name).write_text(text)
    for name, data in BIT_FILES.items():
        (tmp_path / name).write_bytes(data)


def run(tool, side, argv, tmp_path, env=None):
    make_inputs(tmp_path)
    oracle, rust = TOOLS[tool]
    if side == 'o':
        exe = str(ORACLE_WRAPPERS / oracle)
    else:
        exe = str(RUST_DIR / rust)
    full_env = dict(os.environ)
    for name in ('SOURCE_DATE_EPOCH', 'URAY_DATABASE_DIR', 'URAY_DATABASE',
                 'URAY_PART', 'COLUMNS', 'LINES'):
        full_env.pop(name, None)
    full_env['URAY_ORACLE_DIR'] = str(ORACLE_DIR)
    full_env.update(env or {})
    argv = [a.replace(TMP, str(tmp_path)) for a in argv]
    prog = [exe]
    # The program name is the base name of argv[0] (argparse; gflags'
    # help and --helppackage): the Rust tools run through links with the
    # names of the reference binaries (`uray-xcframes2bit` is prjuray's
    # `xcframes2bit` as installed by tests/oracle/setup-xilinx.sh).
    link_name = {
        'fasm2frames': 'fasm2frames.py',
        'xcframes2bit': 'uray-xcframes2bit'
    }.get(tool)
    if side == 'r' and link_name:
        link = tmp_path / link_name
        if not link.exists():
            link.symlink_to(exe)
        prog = [str(link)]
    result = subprocess.run(prog + argv,
                            cwd=str(tmp_path),
                            env=full_env,
                            stdin=subprocess.DEVNULL,
                            capture_output=True,
                            timeout=600)
    files = {}
    for name in sorted(os.listdir(tmp_path)):
        if name.startswith('out'):
            files[name] = (tmp_path / name).read_bytes()
    code = 134 if result.returncode == -6 else result.returncode
    return code, result.stdout, result.stderr, files


def bit_time(files):
    for data in files.values():
        m = re.search(
            rb'c\x00\x0b(\d{4})/(\d\d)/(\d\d)\x00d\x00\x09'
            rb'(\d\d):(\d\d):(\d\d)\x00', data)
        if m:
            return calendar.timegm(tuple(int(g) for g in m.groups()))
    return None


def strip_traceback(stderr):
    out = []
    in_traceback = False
    for line in stderr.split(b'\n'):
        if line == b'Traceback (most recent call last):':
            in_traceback = True
            continue
        if in_traceback and line.startswith(b' '):
            continue
        in_traceback = False
        out.append(line)
    return b'\n'.join(out)


EXTENSION = re.compile(
    rb'\n  Flags from rust/fasm-cli/src/bitread_extensions\.rs:\n'
    rb'(?:    .*\n|      .*\n)*\n\n')
EXTENSION_XML = re.compile(
    rb'<flag><file>rust/fasm-cli/src/bitread_extensions\.rs</file>.*\n')
PARSE_ERROR_RE = re.compile(rb'Exception: Parse error at (\d+):(\d+) - .*',
                            re.S)


def normalise(result, tmp_path, tool):
    code, stdout, stderr, files = result
    tmp = str(tmp_path).encode()

    def text(b):
        b = re.sub(rb"/[^\s:'<]*/(uray-)?(xcframes2bit|bitread)\b(?!\.cc)",
                   b'PROG',
                   b)
        b = b.replace(tmp, b'TMP')
        b = re.sub(rb'(Flags from |<file>)/[^\n<]*?/prjuray-tools/', rb'\1',
                   b)
        b = EXTENSION.sub(b'', b)
        b = EXTENSION_XML.sub(b'', b)
        return b

    if tool == 'fasm2frames':
        stderr = strip_traceback(stderr)
        stderr = PARSE_ERROR_RE.sub(
            lambda m: b'Exception: Parse error at %s:%s - <message>\n' % m.
            groups(), stderr)
    files = {k: v.replace(tmp, b'TMP') for k, v in files.items()}
    return code, text(stdout), text(stderr), files


def check(tool, tmp_path, argv, env=None):
    o_path, r_path = tmp_path / 'o', tmp_path / 'r'
    oracle = run(tool, 'o', argv, o_path, env)
    r_env = dict(env or {})
    epoch = bit_time(oracle[3])
    if epoch is not None:
        r_env['SOURCE_DATE_EPOCH'] = str(epoch)
    rust = run(tool, 'r', argv, r_path, r_env)
    oracle = normalise(oracle, o_path, tool)
    rust = normalise(rust, r_path, tool)
    assert rust[0] == oracle[0], 'exit code'
    assert rust[2] == oracle[2], 'stderr'
    assert rust[1] == oracle[1], 'stdout'
    assert rust[3] == oracle[3], 'output files'


def reference_bit(frm_text):
    """The reference xcframes2bit's .bit of the synthetic part."""
    tmp = Path(os.environ.get('TMPDIR', '/tmp')) / (
        'uray-compat-%d' % os.getpid())
    tmp.mkdir(parents=True, exist_ok=True)
    (tmp / 'in.frm').write_text(frm_text)
    env = dict(os.environ, URAY_ORACLE_DIR=str(ORACLE_DIR))
    argv = [
        str(ORACLE_WRAPPERS / 'uray-xcframes2bit-oracle'),
        '--architecture=UltraScalePlus', '--part_file=' + PART_FILE,
        '--part_name=xcusptest', '--frm_file=in.frm', '--output_file=out.bit'
    ]
    subprocess.run(argv,
                   cwd=str(tmp),
                   env=env,
                   check=True,
                   stdin=subprocess.DEVNULL)
    data = (tmp / 'out.bit').read_bytes()
    shutil.rmtree(str(tmp))
    return data


def bit_files():
    good = reference_bit(FRM_FILES['good.frm'])
    sync = good.index(b'\xaa\x99\x55\x66')
    # Frame 0x100 is the first frame of the payload: flip a data bit of
    # its word 3 (the ECC no longer matches).
    fdri = good.index(b'\x30\x00\x40\x00', sync)
    word3 = fdri + 8 + 3 * 4
    ecc = bytearray(good)
    ecc[word3 + 3] ^= 0x02
    # A Type2 FDRI packet of 93 + 20 words: the second frame is short
    # (verifyECC throws std::out_of_range, the output still buffered is
    # lost); the rest of the frame data is read as packets.
    count = struct.unpack('>I', good[fdri + 4:fdri + 8])[0]
    count = (count & ~0x07FFFFFF) | (93 + 20)
    short = good[:fdri + 4] + struct.pack('>I', count) + good[fdri + 8:]
    return {
        'good.bit': good,
        'ecc.bit': bytes(ecc),
        'short.bit': short,
        'garbage.bit': b'hello\n' * 10,
    }


BIT_FILES = {}


@pytest.fixture(scope='module', autouse=True)
def bitstreams():
    BIT_FILES.update(bit_files())
    yield


GFLAGS_CASES = [
    [],
    ['--help'],
    ['--helpfull'],
    ['--helpful'],
    ['--helpshort'],
    ['--helpxml'],
    ['--helppackage'],
    ['--version'],
    ['--bogus'],
    ['--undefok=bogus', '--bogus'],
    ['--part_file'],
]


@pytest.mark.parametrize('argv', GFLAGS_CASES, ids=repr)
def test_xcframes2bit_flags(tmp_path, argv):
    check('xcframes2bit', tmp_path, argv)


@pytest.mark.parametrize('argv', GFLAGS_CASES + [['-E=maybe'], ['-f', 'x']],
                         ids=repr)
def test_bitread_flags(tmp_path, argv):
    check('bitread', tmp_path, argv)


F2B = ['--part_file=' + PART_FILE, '--output_file=' + OUT + '.bit']
USP = '--architecture=UltraScalePlus'

XCFRAMES2BIT_CASES = [
    F2B + [USP, '--frm_file=' + TMP + '/' + name, '--part_name=p']
    for name in sorted(FRM_FILES)
] + [
    F2B + ['--frm_file=' + TMP + '/good.frm', '--architecture=Foo'],
    F2B + ['--frm_file=' + TMP + '/good.frm', '--architecture=UltraScale'],
    F2B + ['--frm_file=' + TMP + '/good.frm'],
    F2B + [USP, '--frm_file=/nonexistent.frm'],
    F2B + [USP, '--frm_file=' + TMP],
    F2B + [USP],
    [USP, '--part_file=/nonexistent', '--frm_file=x'],
    [USP, '--part_file=/tmp', '--frm_file=x'],
    [
        USP, '--part_file=' + PART_FILE, '--frm_file=' + TMP + '/good.frm',
        '--output_file=/nonexistent/out.bit'
    ],
    [
        USP, '--part_file=' + PART_FILE, '--frm_file=' + TMP + '/good.frm',
        '--output_file=/dev/stdout'
    ],
]


@pytest.mark.parametrize('argv', XCFRAMES2BIT_CASES, ids=repr)
def test_xcframes2bit_run(tmp_path, argv):
    check('xcframes2bit', tmp_path, argv)


BR = ['--part_file=' + PART_FILE, USP]
GOOD = TMP + '/good.bit'
BITREAD_CASES = [
    BR + ['-z', '-y', GOOD],
    BR + ['-x', GOOD],
    BR + ['-x', '-C', GOOD],
    BR + ['-z', '-C', '-y', GOOD],
    BR + ['-z', '-o', OUT + '.txt', GOOD],
    BR + ['-o', OUT + '.txt', '-C', GOOD],
    BR + ['-p', '-o', OUT + '.pgm', GOOD],
    BR + ['-z', '-y', '--aux', OUT + '.aux', GOOD],
    BR + ['-f', '0x00000100', GOOD],
    BR + ['-F', '0x00000100:0x00000180', '-y', GOOD],
    BR + ['-z', '-y', TMP + '/ecc.bit'],
    BR + ['-z', '-y', '-E', TMP + '/ecc.bit'],
    BR + ['-z', '-E', '-o', OUT + '.txt', TMP + '/ecc.bit'],
    BR + ['-z', '-y', '-f', '0x00000101', TMP + '/ecc.bit'],
    BR + ['-z', '-y', TMP + '/garbage.bit'],
    BR + ['-y', TMP + '/short.bit', '-F', '0x0:0x1'],
    BR + ['-y', TMP + '/short.bit'],
    BR + ['-y', '-o', OUT + '.txt', TMP + '/short.bit'],
    ['--part_file=' + PART_FILE, '--architecture=Foo', GOOD],
    ['--part_file=' + PART_FILE, '--architecture=UltraScale', GOOD],
    ['--part_file=' + PART_FILE, GOOD],
    BR + ['/nonexistent.bit'],
]


@pytest.mark.parametrize('argv', BITREAD_CASES, ids=repr)
def test_bitread_run(tmp_path, argv):
    check('bitread', tmp_path, argv)


ARGPARSE_CASES = [
    [],
    ['-h'],
    ['--help'],
    ['--bogus', '-h'],
    ['--bogus'],
    ['--db-root', 'x'],
    ['--db-root', 'x', '--part', 'p', 'a', 'b', 'c'],
    ['--dump', 'a'],
    ['--d', 'x'],
]


@pytest.mark.parametrize('argv', ARGPARSE_CASES, ids=repr)
def test_fasm2frames_argparse(tmp_path, argv):
    check('fasm2frames', tmp_path, argv)


DB = ['--db-root', str(USP_DB), '--part', PART]
DESIGN = TMP + '/design.fasm'
FASM2FRAMES_CASES = [
    DB + [DESIGN, OUT + '.frm'],
    DB + ['--sparse', DESIGN, OUT + '.frm'],
    DB + ['--sparse', '--debug', DESIGN, OUT + '.frm'],
    DB + ['--dump_bits', DESIGN, OUT + '.bits'],
    DB + ['--sparse', '--roi', TMP + '/roi.json', DESIGN, OUT + '.frm'],
    DB + ['--part', 'nope', DESIGN, OUT + '.frm'],
    DB + ['/nonexistent.fasm', OUT + '.frm'],
    DB + [DESIGN, '/nonexistent/out.frm'],
] + [
    DB + ['--sparse', TMP + '/' + name, OUT + '.frm']
    for name in sorted(FASM_FILES) if name.endswith('.fasm')
]


@pytest.mark.parametrize('argv', FASM2FRAMES_CASES, ids=repr)
def test_fasm2frames_run(tmp_path, argv):
    if '--part' in argv[4:]:
        # An unknown part: a database error on both sides (the reference
        # raises FileNotFoundError for its tilegrid.json).
        o = run('fasm2frames', 'o', argv, tmp_path / 'o')
        r = run('fasm2frames', 'r', argv, tmp_path / 'r')
        assert (o[0], r[0]) == (1, 1)
        assert r[2].startswith(b'fasm_xilinx.DbError: ')
        return
    check('fasm2frames', tmp_path, argv)


def test_fasm2frames_environment(tmp_path):
    env = {
        'URAY_DATABASE_DIR': str(USP_DB.parent),
        'URAY_DATABASE': USP_DB.name,
        'URAY_PART': PART
    }
    check('fasm2frames', tmp_path, [DESIGN, OUT + '.frm'], env=env)

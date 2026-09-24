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
"""Differential test of the Rust `fasm` binary against the original tool.

Runs the original Python `fasm` command line tool (the oracle,
`tests/oracle/fasm-oracle`, see `tests/oracle/README.md`) and the Rust
binary (`target/release/fasm`) with the same arguments and asserts that
their stdout, stderr and exit code are identical, byte for byte, for

* every FASM file of `examples/`, `tests/corpus/` (not the `.xz` ones) and
  `tests/cli/fixtures/`, with every option combination of `PER_FILE_ARGS`;
* the file independent command lines of `GLOBAL_CASES` (argparse edge
  cases: missing, unknown, abbreviated, repeated and `=` options, `--`,
  `-h` anywhere, non UTF-8 arguments, ...);
* the help and usage messages at many terminal widths (`COLUMNS`).

Run it with the oracle venv's pytest entry point (not `python -m pytest`,
see `tests/oracle/test_oracle.py`), after `cargo build --release -p
fasm-cli`, or use `make cli-difftest`:

    tests/oracle/venv/bin/pytest tests/cli

`FASM_ORACLE` and `FASM_RUST_CLI` override the paths of the two tools (for
example to use the oracle of another checkout from a git worktree). The
tests are skipped if either tool is missing.

The only accepted differences are the documented ones (the CLI section of
`docs/rewrite/COMPAT.md`), implemented by `normalise()`, which is applied
to both results before comparing them:

1. Parse error messages: `Error: Parse error at L:C - <message>` is
   compared up to the message (`<message>`): the position must be
   identical, the message texts are the Rust parser's own.
2. Value range errors with the ANTLR parser: the original prints
   `Error: 'NoneType' object is not iterable` (the parser's `assert` fails
   inside a ctypes callback, which prints a traceback on stderr); the Rust
   tool prints a parse error with the position of the value. Both become
   `Error: <rejected>` (and the oracle's stderr traceback is dropped); the
   Rust stderr must be empty.
3. Errors of the textX parser (`--parser textx`): textX reports errors
   (including a missing file) in its own format; the Rust tool (which has
   one parser for all `--parser` names) prints its parse error. Both
   become `Error: <rejected>`.

A result is only normalised by rule 2 or 3 when the other side is a Rust
parse error, so every file that the original tool accepts must be
accepted with the same output, and every rejected file must be rejected.
"""
import os
import re
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = Path(
    os.environ.get('FASM_ORACLE', ROOT / 'tests' / 'oracle' / 'fasm-oracle'))
RUST_CLI = Path(
    os.environ.get('FASM_RUST_CLI', ROOT / 'target' / 'release' / 'fasm'))

if not (ORACLE.parent / 'venv' / 'bin' / 'python').exists():
    pytest.skip(
        'oracle venv missing (run tests/oracle/setup.sh): {}'.format(ORACLE),
        allow_module_level=True)
if not RUST_CLI.exists():
    pytest.skip(
        'Rust fasm binary missing (cargo build --release -p fasm-cli): {}'.
        format(RUST_CLI),
        allow_module_level=True)


def fasm_files():
    """The FASM files to run both tools on, relative to ROOT."""
    files = sorted((ROOT / 'examples').glob('*.fasm'))
    files += sorted((ROOT / 'tests' / 'corpus').glob('**/*.fasm'))
    files += sorted((ROOT / 'tests' / 'cli' / 'fixtures').glob('*.fasm'))
    return [str(f.relative_to(ROOT)) for f in files]


# Option combinations run with every file (the file is appended), and the
# parser each one selects.
PER_FILE_ARGS = [
    ([], 'antlr'),
    (['--canonical'], 'antlr'),
    (['--parser', 'antlr'], 'antlr'),
    (['--parser', 'textx'], 'textx'),
    (['--canonical', '--parser', 'textx'], 'textx'),
    (['--parser'], 'antlr'),
    (['--bogus'], 'antlr'),
    (['--canon'], 'antlr'),
    (['--parser=antlr'], 'antlr'),
    (['--canonical', '--canonical'], 'antlr'),
    (['--'], 'antlr'),
]

BLANK = 'examples/blank.fasm'
MANY = 'examples/many.fasm'

# File independent command lines (argv without the program name; str or
# bytes items) and the parser they select.
GLOBAL_CASES = [
    ([], 'antlr'),
    (['/nonexistent/file.fasm'], 'antlr'),
    (['--parser', 'textx', '/nonexistent/file.fasm'], 'textx'),
    ([''], 'antlr'),
    (['-'], 'antlr'),
    (['-h'], 'antlr'),
    (['--help'], 'antlr'),
    (['--h'], 'antlr'),
    (['--he'], 'antlr'),
    ([MANY, '-h'], 'antlr'),
    (['--bogus', '-h'], 'antlr'),
    (['-h', '--parser'], 'antlr'),
    (['-hx'], 'antlr'),
    (['-hh'], 'antlr'),
    (['-hhx'], 'antlr'),
    (['-hé'], 'antlr'),
    (['-h=x'], 'antlr'),
    (['-h='], 'antlr'),
    (['-h-'], 'antlr'),
    (['-h=-'], 'antlr'),
    (['-hh=x'], 'antlr'),
    (['-hh='], 'antlr'),
    (['--help='], 'antlr'),
    (['--help=x'], 'antlr'),
    (['--=x'], 'antlr'),
    (['-h', '--=x'], 'antlr'),
    (['--canonical=1', BLANK], 'antlr'),
    (['--canonical=', BLANK], 'antlr'),
    (['--canonical=it\'s', BLANK], 'antlr'),
    (['--canonical=x\n\x01\'"é\xa0 \U0001F600', BLANK], 'antlr'),
    (['--c=x', BLANK], 'antlr'),
    (['--c', BLANK], 'antlr'),
    (['--parser'], 'antlr'),
    (['--parser', BLANK], 'antlr'),
    (['--parser', '-x', BLANK], 'antlr'),
    (['--parser', '-h'], 'antlr'),
    (['--parser', '--', BLANK], 'antlr'),
    (['--parser', '-1', BLANK], 'antlr'),
    (['--parser', '-1.5', BLANK], 'antlr'),
    (['--parser', '-.5', BLANK], 'antlr'),
    (['--parser', '-1\n', BLANK], 'antlr'),
    (['--parser', '-٣', BLANK], 'antlr'),
    (['--parser', '-1x', BLANK], 'antlr'),
    (['--parser', 'a b', BLANK], 'antlr'),
    (['--parser', '-a b', BLANK], 'antlr'),
    (['--parser', '', MANY], 'antlr'),
    (['--parser=', MANY], 'antlr'),
    (['--parser=a=b', BLANK], 'antlr'),
    (['--parser', 'foo', '/nonexistent/file.fasm'], 'antlr'),
    (['--parser', 'ANTLR', BLANK], 'antlr'),
    (['--parser', 'textx ', BLANK], 'antlr'),
    (['--parser', 'foo', '--parser', 'textx', MANY], 'textx'),
    (['--parser', 'textx', '--parser', 'antlr', MANY], 'antlr'),
    (['--pars', 'textx', MANY], 'textx'),
    (['--p=textx', MANY], 'textx'),
    (['--canon', '--pars', 'antlr', MANY], 'antlr'),
    ([MANY, '--canonical'], 'antlr'),
    ([MANY, '--parser', 'textx', '--canonical'], 'textx'),
    (['a', 'b', 'c'], 'antlr'),
    (['--bogus'], 'antlr'),
    (['--bogus', BLANK], 'antlr'),
    (['-x', BLANK], 'antlr'),
    ([BLANK, '-x'], 'antlr'),
    (['---', BLANK], 'antlr'),
    (['-c', BLANK], 'antlr'),
    (['-p', BLANK], 'antlr'),
    (['x', '--bogus=1', 'y'], 'antlr'),
    (['--', MANY], 'antlr'),
    ([MANY, '--'], 'antlr'),
    (['--', '--'], 'antlr'),
    (['--', '-h'], 'antlr'),
    (['--', '--canonical'], 'antlr'),
    (['--'], 'antlr'),
    (['--', 'a', 'b'], 'antlr'),
    (['a', '--', 'b'], 'antlr'),
    (['a', '--', '--'], 'antlr'),
    (['--canonical', '--', MANY, '--'], 'antlr'),
    (['a', '--', 'b', '--', 'c'], 'antlr'),
    (['--canonical', '--', MANY], 'antlr'),
    ([b'--parser', b'x\xff', BLANK], 'antlr'),
    ([b'--parser', b'x\xff\n\x01\'"', BLANK], 'antlr'),
    ([b'--canonical=x\xff', BLANK], 'antlr'),
    ([BLANK, b'x\xff'], 'antlr'),
    ([b'--x\xff', BLANK], 'antlr'),
    (['--parser', 'textx', b'/nonexistent/x\xff'], 'textx'),
]

# The widths help and usage messages are checked at.
HELP_COLUMNS = [str(c) for c in range(1, 101)] + [
    '120', '200', '0', '-5', 'abc', ' 70 ', '+70', '7_0', '٧٠', '',
    # `int()` converts at most `sys.int_max_str_digits` (4300) digits,
    # leading zeros included: more is a ValueError (so 80 columns).
    '0' * 4298 + '50', '0' * 4299 + '50', '1' * 4300, '1' * 4301,
    '9' * 5000, ' -' + '1' * 4300, '1_' * 4299 + '1'
]


def columns_id(columns):
    if len(columns) <= 20:
        return repr(columns)
    return '{!r}...({} chars)'.format(columns[:8], len(columns))


def run(tool, argv, columns=None):
    env = dict(os.environ)
    env.pop('COLUMNS', None)
    env.pop('LINES', None)
    if columns is not None:
        env['COLUMNS'] = columns
    argv = [os.fsencode(a) if isinstance(a, str) else a for a in argv]
    result = subprocess.run(
        [str(tool)] + argv,
        cwd=str(ROOT),
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=600)
    return result.stdout, result.stderr, result.returncode


PARSE_ERROR_RE = re.compile(rb'Error: Parse error at (\d+):(\d+) - .*\n', re.S)
REJECTED = b'Error: <rejected>\n'
NONE_TYPE_ERROR = b"Error: 'NoneType' object is not iterable\n"
CTYPES_TRACEBACK = b'Exception ignored on calling ctypes callback function'

# How often each normalisation rule was applied, for the summary.
RULE_COUNTS = {'1-message': 0, '2-antlr-value-error': 0, '3-textx-error': 0}


def is_parse_error(stdout):
    return PARSE_ERROR_RE.fullmatch(stdout) is not None


def normalise(oracle, rust, parser):
    """Applies the documented differences (see the module docstring) to
    the (stdout, stderr, returncode) results of both tools."""
    (o_out, o_err, o_code), (r_out, r_err, r_code) = oracle, rust
    if is_parse_error(r_out) and o_code == 0 and r_code == 0:
        # Rule 2: ANTLR value range errors.
        if (parser == 'antlr' and o_out == NONE_TYPE_ERROR
                and o_err.startswith(CTYPES_TRACEBACK)):
            RULE_COUNTS['2-antlr-value-error'] += 1
            return (REJECTED, b'', o_code), (REJECTED, r_err, r_code)
        # Rule 3: textX errors.
        if (parser == 'textx' and o_out.startswith(b'Error: ')
                and o_out.endswith(b'\n')
                and not o_out.startswith(b"Error: Parser '")):
            RULE_COUNTS['3-textx-error'] += 1
            return (REJECTED, o_err, o_code), (REJECTED, r_err, r_code)

    # Rule 1: parse error message texts.
    def strip_message(stdout):
        m = PARSE_ERROR_RE.fullmatch(stdout)
        if m is None:
            return stdout
        return b'Error: Parse error at %s:%s - <message>\n' % m.groups()

    if is_parse_error(o_out) and is_parse_error(r_out):
        RULE_COUNTS['1-message'] += 1
    return ((strip_message(o_out), o_err, o_code),
            (strip_message(r_out), r_err, r_code))


def check(argv, parser, columns=None):
    oracle = run(ORACLE, argv, columns)
    rust = run(RUST_CLI, argv, columns)
    oracle, rust = normalise(oracle, rust, parser)
    assert rust[2] == oracle[2], 'exit code'
    assert rust[0] == oracle[0], 'stdout'
    assert rust[1] == oracle[1], 'stderr'


def case_id(argv):
    return ' '.join(
        repr(a) if isinstance(a, bytes) or not a or ' ' in a else a
        for a in argv) or '(no arguments)'


FILE_CASES = [(args + [f], parser) for f in fasm_files()
              for args, parser in PER_FILE_ARGS]


@pytest.mark.parametrize(
    'argv,parser', FILE_CASES, ids=[case_id(a) for a, _ in FILE_CASES])
def test_file(argv, parser):
    check(argv, parser)


@pytest.mark.parametrize(
    'argv,parser', GLOBAL_CASES, ids=[case_id(a) for a, _ in GLOBAL_CASES])
def test_command_line(argv, parser):
    check(argv, parser)


@pytest.mark.parametrize(
    'columns', HELP_COLUMNS, ids=[columns_id(c) for c in HELP_COLUMNS])
@pytest.mark.parametrize('argv', [['--help'], [], ['--bogus', BLANK]])
def test_terminal_width(argv, columns):
    check(argv, 'antlr', columns)


def test_directory():
    """A directory as the file: the ANTLR parser aborts the process (a C++
    exception), the textX parser reports an `IsADirectoryError`; the Rust
    tool reports an error like for any unreadable file (documented)."""
    check(['--parser', 'textx', 'examples'], 'textx')
    stdout, stderr, code = run(RUST_CLI, ['examples'])
    assert stdout.startswith(
        b"Error: Parse error at 0:0 - Couldn't open file examples: ")
    assert (stderr, code) == (b'', 0)


def test_broken_pipe(tmp_path):
    """Both tools exit with 1 when the reader of their stdout goes away
    before they could write it all (the original prints a traceback; the
    Rust tool prints nothing)."""
    big = tmp_path / 'big.fasm'
    # More output than a pipe buffers.
    big.write_text(''.join(
        "A.B{}.C[3:0] = 4'hA\n".format(i) for i in range(20000)))
    for tool in (ORACLE, RUST_CLI):
        proc = subprocess.Popen(
            [str(tool), str(big)],
            cwd=str(ROOT),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE)
        proc.stdout.close()
        _, stderr = proc.communicate(timeout=600)
        assert proc.returncode == 1, (tool, stderr)
        if tool == RUST_CLI:
            assert stderr == b''


@pytest.mark.parametrize('argv', [['-h'], ['--help', '--bogus']])
def test_help_with_closed_stdout(argv):
    """`fasm -h >&-` (documented difference): Python's `sys.stdout` is None,
    so argparse prints the help to stderr; the Rust runtime reopens a
    closed stdout as /dev/null, so the help is discarded. Both exit with
    0."""

    def run_closed(tool):
        env = dict(os.environ)
        env.pop('COLUMNS', None)
        env.pop('LINES', None)
        result = subprocess.run(
            [str(tool)] + argv,
            cwd=str(ROOT),
            env=env,
            stdin=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            preexec_fn=lambda: os.close(1),
            timeout=600)
        return result.stderr, result.returncode

    help_text = run(RUST_CLI, ['--help'])[0]
    assert run_closed(ORACLE) == (help_text, 0)
    assert run_closed(RUST_CLI) == (b'', 0)

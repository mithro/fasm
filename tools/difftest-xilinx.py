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
"""Differential test of the Rust `fasm2frames` against the reference
(f4pga-xc-fasm's `xc_fasm.fasm2frames` on prjxray, the oracle
`tests/oracle/fasm2frames-oracle`), v1 (T5.4/T5.5).

For every FASM file of the Xilinx corpus

* `tests/corpus/xilinx/<family>/**/*.fasm` with the prjxray-db family
  `<db cache>/prjxray-db/<family>` and the part of `FAMILY_PARTS`
  (skipped with a note when the database has not been fetched,
  `tools/fetch-db.sh prjxray <family>`);
* `tests/corpus/f4pga-xc-fasm/**/*.fasm` with the miniature database
  `rust/fasm-xilinx/testdata/mini-db` (part `xc7`);

both tools are run with each flag set of `VARIANTS` (dense, `--sparse`,
`--emit_pudc_b_pullup`, `--sparse --debug`, and `--sparse --roi
<stem>.roi.json` when that file exists next to the FASM file), writing
the `.frm` to a file, and the results are compared:

* the exit codes must be equal;
* the `.frm` files must be identical, byte for byte (also when the tools
  fail: both create the output file first, so it must be empty);
* stdout (the `--debug` dump) must be identical;
* stderr must be identical after this normalisation (documented in the
  `fasm2frames` section of `docs/rewrite/COMPAT.md`):
  1. the oracle's traceback (`Traceback (most recent call last):` and
     the indented frame lines after it) is removed: the Rust tool prints
     only the last line(s), `<exception type>: <message>`;
  2. parse errors (`Exception: Parse error at L:C - <message>`) are
     compared up to the message: the position must be identical, the
     message texts are the Rust parser's own (like the `fasm` CLI);
  3. database errors: when the oracle fails with an exception outside
     `EXACT_EXCEPTIONS` (it reports a database that cannot be opened with
     assorted Python exceptions, the Rust tool with
     `fasm_xilinx.DbError`), and the Rust tool with
     `fasm_xilinx.DbError`, only the exit codes (1 on both sides) are
     compared;
  4. value range errors (`a = 2`, `a[3:0] = 5'h10`, `a[0:1]`) of the
     reference's ANTLR parser: its assertion fails inside a ctypes
     callback (`Exception ignored on calling ctypes callback function`,
     an `AssertionError` traceback) and the tool then dies with
     `TypeError: 'NoneType' object is not iterable`; the Rust tool reports
     `Exception: Parse error at L:C - <message>` at the value. Both become
     `<value range error>` (like rule 2 of the `fasm` CLI difftest).
  Any other difference of stderr is a failure.

Exit status: 0 if every run matches, 1 if any differs, 3 if a tool is
missing. `make xilinx-difftest` builds the Rust tool and runs this.
"""
import argparse
import concurrent.futures
import fnmatch
import os
import re
import subprocess
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_ORACLE = os.path.join(REPO_ROOT, 'tests', 'oracle',
                              'fasm2frames-oracle')
DEFAULT_RUST = os.path.join(REPO_ROOT, 'target', 'release', 'fasm2frames')
DEFAULT_DB_CACHE = os.environ.get(
    'FASM_DB_CACHE', os.path.join(REPO_ROOT, 'tests', 'oracle', 'build',
                                  'db'))
MINI_DB = os.path.join(REPO_ROOT, 'rust', 'fasm-xilinx', 'testdata',
                       'mini-db')

# The part used for the FASM files of each prjxray-db family directory.
FAMILY_PARTS = {
    'artix7': 'xc7a35tcsg324-1',
}

VARIANTS = [
    ('dense', []),
    ('sparse', ['--sparse']),
    ('pudc', ['--emit_pudc_b_pullup']),
    ('debug', ['--sparse', '--debug']),
]

# Exceptions whose message the Rust tool reproduces exactly.
EXACT_EXCEPTIONS = {
    'prjxray.fasm_assembler.FasmLookupError',
    'prjxray.fasm_assembler.FasmInconsistentBits',
    'KeyError',
    'IndexError',
    'ValueError',
    'Exception',
    'FileNotFoundError',
    'IsADirectoryError',
    'PermissionError',
    'NotADirectoryError',
}

EXIT_OK = 0
EXIT_DIFFERENCES = 1
EXIT_NOT_SET_UP = 3

RULES = ('1-traceback', '2-parse-message', '3-db-error', '4-value-range')
CTYPES_MARKER = 'Exception ignored on calling ctypes callback function'
NONE_TYPE = "TypeError: 'NoneType' object is not iterable"

PARSE_ERROR_RE = re.compile(r'^Exception: Parse error at (\d+):(\d+) - .*$',
                            re.S)


def strip_traceback(stderr):
    """Rule 1: removes the traceback header and frames."""
    lines = stderr.split('\n')
    out = []
    in_traceback = False
    for line in lines:
        if line == 'Traceback (most recent call last):':
            in_traceback = True
            continue
        if in_traceback and line.startswith(' '):
            continue
        in_traceback = False
        out.append(line)
    return '\n'.join(out)


def exception_type(stderr):
    """The exception type of the last line(s) of a traceback, if any."""
    for line in stderr.split('\n'):
        m = re.match(r'^([A-Za-z_][\w.]*): ', line)
        if m and (m.group(1) in EXACT_EXCEPTIONS or '.' in m.group(1)
                  or m.group(1).endswith('Error')):
            return m.group(1), line
    return None, None


def normalise(stderr, rules):
    """Rule 2: parse error messages."""
    head, sep, tail = stderr.partition('Exception: Parse error at ')
    if sep:
        m = PARSE_ERROR_RE.match(sep + tail)
        if m:
            rules['2-parse-message'] += 1
            return head + 'Exception: Parse error at %s:%s - <message>\n' % (
                m.group(1), m.group(2))
    return stderr


def run(tool, args, out_path):
    result = subprocess.run([tool] + args + [out_path],
                            cwd=REPO_ROOT,
                            stdin=subprocess.DEVNULL,
                            capture_output=True,
                            timeout=3600)
    try:
        with open(out_path, 'rb') as f:
            frm = f.read()
    except OSError:
        frm = None
    return result.returncode, result.stdout, result.stderr.decode(
        'utf-8', 'surrogateescape'), frm


def compare(case, oracle, rust, tmpdir):
    """Runs one case, returns (ok, message, rules applied)."""
    name, fasm, db, part, flags = case
    args = ['--db-root', db, '--part', part] + flags + [fasm]
    tag = re.sub(r'[^\w.-]', '_', name)
    o = run(oracle, args, os.path.join(tmpdir, tag + '.oracle.frm'))
    r = run(rust, args, os.path.join(tmpdir, tag + '.rust.frm'))
    rules = dict.fromkeys(RULES, 0)
    problems = []
    if o[0] != r[0]:
        problems.append('exit code %d (oracle) != %d (rust)' % (o[0], r[0]))
    if o[3] != r[3]:
        problems.append('.frm output differs (%s vs %s bytes)' %
                        (None if o[3] is None else len(o[3]),
                         None if r[3] is None else len(r[3])))
    if o[1] != r[1]:
        problems.append('stdout differs')
    o_err = o[2]
    if 'Traceback (most recent call last):' in o_err:
        rules['1-traceback'] += 1
        o_err = strip_traceback(o_err)
    o_type, _ = exception_type(o_err)
    if (CTYPES_MARKER in o_err and o_err.rstrip('\n').endswith(NONE_TYPE)
            and PARSE_ERROR_RE.match(r[2])):
        # Rule 4: ANTLR value range error.
        rules['4-value-range'] += 1
        if o[0] != 1 or r[0] != 1:
            problems.append('value range error: expected exit code 1')
    elif (o_type is not None and o_type not in EXACT_EXCEPTIONS
          and r[2].startswith('fasm_xilinx.DbError: ')):
        # Rule 3: database errors.
        rules['3-db-error'] += 1
        if o[0] != 1 or r[0] != 1:
            problems.append('database error: expected exit code 1')
    elif normalise(o_err, rules) != normalise(r[2], dict.fromkeys(RULES, 0)):
        problems.append('stderr differs:\n--- oracle\n%s--- rust\n%s' %
                        (o_err, r[2]))
    return (not problems, '%s: %s' % (name, '; '.join(problems)), rules)


def corpus(db_cache, pattern):
    """The (name, fasm, db, part, flags) cases and skipped notes."""
    cases = []
    notes = []
    sources = []
    xilinx = os.path.join(REPO_ROOT, 'tests', 'corpus', 'xilinx')
    for family in sorted(os.listdir(xilinx)):
        if not os.path.isdir(os.path.join(xilinx, family)):
            continue
        db = os.path.join(db_cache, 'prjxray-db', family)
        part = FAMILY_PARTS.get(family)
        if part is None:
            notes.append('no part for family %s' % family)
            continue
        if not os.path.isdir(db):
            notes.append('skipping %s: %s not found (tools/fetch-db.sh '
                         'prjxray %s)' % (family, db, family))
            continue
        sources.append((os.path.join(xilinx, family), db, part))
    sources.append((os.path.join(REPO_ROOT, 'tests', 'corpus',
                                 'f4pga-xc-fasm'), MINI_DB, 'xc7'))
    for root, db, part in sources:
        for dirpath, _, files in sorted(os.walk(root)):
            for f in sorted(files):
                if not f.endswith('.fasm'):
                    continue
                fasm = os.path.join(dirpath, f)
                rel = os.path.relpath(fasm, REPO_ROOT)
                if pattern and not fnmatch.fnmatch(rel, pattern):
                    continue
                variants = list(VARIANTS)
                roi = fasm[:-len('.fasm')] + '.roi.json'
                if os.path.exists(roi):
                    variants.append(('roi', ['--sparse', '--roi', roi]))
                for vname, flags in variants:
                    cases.append(('%s[%s]' % (rel, vname), rel, db, part,
                                  flags))
    return cases, notes


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--oracle',
                        default=os.environ.get('FASM2FRAMES_ORACLE',
                                               DEFAULT_ORACLE),
                        help='reference fasm2frames (default: %(default)s)')
    parser.add_argument('--rust',
                        default=os.environ.get('FASM2FRAMES_RUST',
                                               DEFAULT_RUST),
                        help='Rust fasm2frames (default: %(default)s)')
    parser.add_argument('--db-cache',
                        default=DEFAULT_DB_CACHE,
                        help='directory of fetched databases '
                        '(default: $FASM_DB_CACHE or %(default)s)')
    parser.add_argument('--filter', help='only FASM files matching GLOB')
    parser.add_argument('--jobs', type=int, default=os.cpu_count() or 1)
    parser.add_argument('-v', '--verbose', action='store_true')
    args = parser.parse_args()

    for tool in (args.oracle, args.rust):
        if not os.access(tool, os.X_OK):
            print('difftest-xilinx: %s not found' % tool, file=sys.stderr)
            return EXIT_NOT_SET_UP

    cases, notes = corpus(args.db_cache, args.filter)
    for note in notes:
        print(note)
    totals = dict.fromkeys(RULES, 0)
    failures = []
    files = set(case[1] for case in cases)
    with tempfile.TemporaryDirectory(prefix='difftest-xilinx-') as tmpdir:
        with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
            results = pool.map(
                lambda c: compare(c, args.oracle, args.rust, tmpdir), cases)
            for (ok, message, rules), case in zip(results, cases):
                for k, v in rules.items():
                    totals[k] += v
                if ok:
                    if args.verbose:
                        print('ok   %s' % case[0])
                else:
                    failures.append(message)
                    print('FAIL %s' % message)
    print('difftest-xilinx: %d FASM files, %d runs, %d identical, %d '
          'different' % (len(files), len(cases), len(cases) - len(failures),
                         len(failures)))
    for rule, count in sorted(totals.items()):
        print('  normalisation rule %s: applied %d time(s)' % (rule, count))
    return EXIT_DIFFERENCES if failures else EXIT_OK


if __name__ == '__main__':
    sys.exit(main())

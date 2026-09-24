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

Bitstream tools (T5.6/T5.7), for the prjxray-db families (whose parts
have a `part.yaml`):

* for the `BITSTREAM_VARIANTS` runs that succeed, the reference
  `xc7frames2bit` (`tests/oracle/xc7frames2bit-oracle`) and the Rust
  `xc7frames2bit` both turn the oracle's `.frm` into a `.bit`; the Rust
  tool gets the reference header's date and time through
  `SOURCE_DATE_EPOCH`, so the two files must be identical byte for byte
  (as must stdout, stderr and the exit codes); then the reference and
  the Rust `bitread` read the reference `.bit` with each flag set of
  `BITREAD_FLAGS` and must print the same (stdout, stderr, exit code and
  the `-o` / `--aux` files);
* the reference `xcfasm` (`tests/oracle/xcfasm-oracle`, with `--frm2bit`
  pointing at the reference `xc7frames2bit`) and the Rust `xcfasm` run
  on every FASM file with `XCFASM_VARIANTS`, writing the same `--frm_out`
  and `--bit_out` paths: the `.frm` and `.bit` files (again with the
  reference time injected), stdout, exit codes and stderr (normalised
  like for fasm2frames) must be identical;
* `bitread` also runs on the reference bitstreams of
  `reference_bitstreams()` (the golden smoke bitstream and, from the
  oracle's prjxray checkout, `lib/test_data/configuration_test*.bit` and
  the Series7 bitstreams of `ToolsTestData.tar.gz`, Vivado outputs) with
  every flag set.

The reference gflags tools print their own path (`argv[0]`) in some
messages; it is replaced by `PROG` on both sides.

Exit status: 0 if every run matches, 1 if any differs, 3 if a tool is
missing. `make xilinx-difftest` builds the Rust tools and runs this.
"""
import argparse
import calendar
import concurrent.futures
import fnmatch
import os
import re
import subprocess
import sys
import tarfile
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_ORACLE = os.path.join(REPO_ROOT, 'tests', 'oracle',
                              'fasm2frames-oracle')
DEFAULT_RUST = os.path.join(REPO_ROOT, 'target', 'release', 'fasm2frames')
DEFAULT_RUST_DIR = os.path.join(REPO_ROOT, 'target', 'release')
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

# The fasm2frames variants whose .frm also goes through xc7frames2bit and
# bitread, and the xcfasm variants.
BITSTREAM_VARIANTS = ('dense', 'sparse', 'pudc', 'roi')
XCFASM_VARIANTS = [
    ('dense', []),
    ('sparse', ['--sparse']),
    ('debug', ['--sparse', '--debug', '--emit_pudc_b_pullup']),
]

# bitread flag sets (`-o` and `--aux` get a file name appended).
BITREAD_FLAGS = [
    ['-z', '-y', '-o'],
    ['-z', '-x'],
    ['-z', '-C', '-o'],
    ['-y', '-F', '0x00400000:0x004000ff', '--aux'],
    ['-f', '0x00400000', '-C'],
    ['-p', '-F', '0x00000000:0x0000003f', '-o'],
    ['-C', '-x', '-z', '-o'],
    ['-y', '-F', '0:0xffffffff'],
    ['-y', '-F', '010:0x20'],
    ['-y', '-C', '-F', '0x00400100'],
    ['-f', '0x0040010b', '-o'],
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


def compare(case, oracle, rust, tmpdir, tools=None):
    """Runs one case, returns (ok, message, rules applied, bitstream tool
    runs)."""
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
    bitstream_runs = 0
    variant = name.rsplit('[', 1)[-1].rstrip(']')
    part_file = os.path.join(db, part, 'part.yaml')
    if (tools and not problems and o[0] == 0 and variant in BITSTREAM_VARIANTS
            and os.path.exists(part_file)):
        frm = os.path.join(tmpdir, tag + '.oracle.bitstream.frm')
        with open(frm, 'wb') as f:
            f.write(o[3])
        more, bitstream_runs = compare_bitstream(frm, part_file, part, tools,
                                                 tmpdir, tag)
        problems += more
        os.remove(frm)
    return (not problems, '%s: %s' % (name, '; '.join(problems)), rules,
            bitstream_runs)


def bit_time(path):
    """The seconds since the epoch of a .bit header's date and time, or
    None."""
    try:
        with open(path, 'rb') as f:
            data = f.read(4096)
    except OSError:
        return None
    m = re.search(
        rb'c\x00\x0b(\d{4})/(\d\d)/(\d\d)\x00d\x00\x09'
        rb'(\d\d):(\d\d):(\d\d)\x00', data)
    if not m:
        return None
    return calendar.timegm(tuple(int(g) for g in m.groups()))


def run_tool(argv, env=None):
    """(exit code, stdout, stderr); a SIGABRT is exit code 134."""
    full_env = dict(os.environ)
    full_env.update(env or {})
    result = subprocess.run(argv,
                            cwd=REPO_ROOT,
                            env=full_env,
                            stdin=subprocess.DEVNULL,
                            capture_output=True,
                            timeout=3600)
    code = result.returncode
    if code == -6:
        code = 134
    return code, result.stdout, result.stderr


def read_and_remove(paths):
    out = {}
    for path in paths:
        try:
            with open(path, 'rb') as f:
                out[os.path.basename(path)] = f.read()
            os.remove(path)
        except OSError:
            out[os.path.basename(path)] = None
    return out


def normalise_gflags_paths(data, tool):
    """The reference prints its own path (`argv[0]`); both become PROG."""
    return re.sub(rb"/[^\s:']*/" + tool.encode() + rb'\b', b'PROG', data)


def compare_runs(what, o, r):
    """Problems between (code, stdout, stderr, files) of the oracle and
    of the Rust tool."""
    problems = []
    for i, name in enumerate(('exit code', 'stdout', 'stderr')):
        if o[i] != r[i]:
            problems.append('%s: %s differs:\n--- oracle\n%r\n--- rust\n%r' %
                            (what, name, o[i][:2000], r[i][:2000]))
    for name in sorted(set(o[3]) | set(r[3])):
        a, b = o[3].get(name), r[3].get(name)
        if a != b:
            first = None
            if a is not None and b is not None:
                first = next((i for i in range(min(len(a), len(b)))
                              if a[i] != b[i]), min(len(a), len(b)))
            problems.append('%s: file %s differs (%s vs %s bytes, first '
                            'difference at %s)' %
                            (what, name, None if a is None else len(a),
                             None if b is None else len(b), first))
    return problems


def run_bitread(tools, part_file, bit, flags, tmpdir, tag):
    """Runs both bitreads with `flags` on `bit`; returns the problems."""
    results = []
    for side in ('oracle', 'rust'):
        args = list(flags)
        files = []
        for option in ('-o', '--aux'):
            if option in args:
                path = os.path.join(tmpdir,
                                    '%s.%s.txt' % (tag, option.strip('-')))
                args.insert(args.index(option) + 1, path)
                files.append(path)
        code, out, err = run_tool([tools[side + '_bitread'],
                                   '--part_file=' + part_file] + args + [bit])
        out = normalise_gflags_paths(out, 'bitread')
        results.append((code, out, err, read_and_remove(files)))
    return compare_runs('bitread %s' % ' '.join(flags), *results)


def compare_bitstream(frm, part_file, part, tools, tmpdir, tag):
    """xc7frames2bit on the oracle's .frm, then bitread on the .bit;
    returns (problems, runs)."""
    bit = os.path.join(tmpdir, tag + '.bit')
    base = [
        '--frm_file=' + frm, '--output_file=' + bit, '--part_name=' + part,
        '--part_file=' + part_file
    ]
    o = run_tool([tools['oracle_frames2bit']] + base)
    epoch = bit_time(bit)
    o_files = read_and_remove([bit])
    env = {'SOURCE_DATE_EPOCH': str(epoch)} if epoch is not None else {}
    r = run_tool([tools['rust_frames2bit']] + base, env=env)
    r_files = read_and_remove([bit])
    problems = compare_runs('xc7frames2bit', o + (o_files, ), r + (r_files, ))
    data = o_files[tag + '.bit']
    if problems or o[0] != 0 or data is None:
        return problems, 1
    with open(bit, 'wb') as f:
        f.write(data)
    for i, flags in enumerate(BITREAD_FLAGS):
        problems += run_bitread(tools, part_file, bit, flags, tmpdir,
                                '%s.%d' % (tag, i))
    os.remove(bit)
    return problems, 1 + len(BITREAD_FLAGS)


def compare_xcfasm(case, tools, tmpdir):
    """xcfasm-oracle vs the Rust xcfasm; (ok, message)."""
    name, fasm, db, part, flags = case
    part_file = os.path.join(db, part, 'part.yaml')
    tag = re.sub(r'[^\w.-]', '_', name)
    frm = os.path.join(tmpdir, tag + '.xcfasm.frm')
    bit = os.path.join(tmpdir, tag + '.xcfasm.bit')
    args = [
        '--db-root', db, '--part', part, '--part_file', part_file,
        '--frm2bit', tools['oracle_frames2bit'], '--fn_in', fasm,
        '--frm_out', frm, '--bit_out', bit
    ] + flags
    # The wrapper warns when no xc7frames2bit is on PATH (the tool itself
    # is given with --frm2bit): put the reference binaries there.
    bin_dir = os.path.join(
        os.path.dirname(os.path.abspath(tools['oracle_xcfasm'])), 'build',
        'xilinx', 'bin')
    o = run_tool([tools['oracle_xcfasm']] + args,
                 env={'PATH': bin_dir + os.pathsep + os.environ['PATH']})
    epoch = bit_time(bit)
    o_files = read_and_remove([frm, bit])
    env = {'SOURCE_DATE_EPOCH': str(epoch)} if epoch is not None else {}
    r = run_tool([tools['rust_xcfasm']] + args, env=env)
    r_files = read_and_remove([frm, bit])
    o_err = strip_traceback(o[2].decode('utf-8', 'surrogateescape'))
    r_err = r[2].decode('utf-8', 'surrogateescape')
    rules = dict.fromkeys(RULES, 0)
    o_type, _ = exception_type(o_err)
    if (CTYPES_MARKER in o_err and o_err.rstrip('\n').endswith(NONE_TYPE)
            and PARSE_ERROR_RE.match(r_err)):
        rules['4-value-range'] += 1
        o_err = r_err = '<value range error>'
    elif (o_type is not None and o_type not in EXACT_EXCEPTIONS
          and o_type != 'subprocess.CalledProcessError'
          and r_err.startswith('fasm_xilinx.DbError: ')):
        rules['3-db-error'] += 1
        o_err = r_err = '<database error>'
    else:
        o_err = normalise(o_err, rules)
        r_err = normalise(r_err, dict.fromkeys(RULES, 0))
    problems = compare_runs('xcfasm', (o[0], o[1], o_err.encode(), o_files),
                            (r[0], r[1], r_err.encode(), r_files))
    return not problems, '%s: %s' % (name, '; '.join(problems))


def reference_bitstreams(bitread_oracle, db_cache, tmpdir):
    """(name, part_file, bit) of the reference bitstreams that exist."""
    out = []
    db = os.path.join(db_cache, 'prjxray-db', 'artix7')
    smoke = os.path.join(REPO_ROOT, 'tests', 'corpus', 'xilinx', 'artix7',
                         'smoke_x1y0.bit')
    if os.path.isdir(db):
        out.append(('smoke_x1y0.bit',
                    os.path.join(db, FAMILY_PARTS['artix7'],
                                 'part.yaml'), smoke))
    test_data = os.path.join(os.path.dirname(os.path.abspath(bitread_oracle)),
                             'build', 'xilinx', 'src', 'prjxray', 'lib',
                             'test_data')
    if os.path.isdir(test_data):
        yaml = os.path.join(test_data, 'configuration_test.yaml')
        for f in ('configuration_test.bit', 'configuration_test.debug.bit',
                  'configuration_test.perframecrc.bit'):
            out.append((f, yaml, os.path.join(test_data, f)))
        tools = os.path.join(test_data, 'ToolsTestData.tar.gz')
        if os.path.exists(tools):
            with tarfile.open(tools) as tar:
                for member in ('Series7/part.yaml', 'Series7/design.bit',
                               'Series7/bram.bit'):
                    tar.extract(member, tmpdir)
            s7 = os.path.join(tmpdir, 'Series7')
            for f in ('design.bit', 'bram.bit'):
                out.append(('ToolsTestData/Series7/' + f,
                            os.path.join(s7, 'part.yaml'),
                            os.path.join(s7, f)))
    return out


def compare_reference_bit(item, tools, tmpdir):
    name, part_file, bit = item
    tag = re.sub(r'[^\w.-]', '_', name)
    problems = []
    for i, flags in enumerate(BITREAD_FLAGS):
        problems += run_bitread(tools, part_file, bit, flags, tmpdir,
                                '%s.%d' % (tag, i))
    return not problems, '%s: %s' % (name, '; '.join(problems))


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
    oracle_dir = os.path.join(REPO_ROOT, 'tests', 'oracle')
    parser.add_argument(
        '--frames2bit-oracle',
        default=os.environ.get('XC7FRAMES2BIT_ORACLE',
                               os.path.join(oracle_dir,
                                            'xc7frames2bit-oracle')),
        help='reference xc7frames2bit (default: %(default)s)')
    parser.add_argument('--bitread-oracle',
                        default=os.environ.get(
                            'BITREAD_ORACLE',
                            os.path.join(oracle_dir, 'bitread-oracle')),
                        help='reference bitread (default: %(default)s)')
    parser.add_argument('--xcfasm-oracle',
                        default=os.environ.get(
                            'XCFASM_ORACLE',
                            os.path.join(oracle_dir, 'xcfasm-oracle')),
                        help='reference xcfasm (default: %(default)s)')
    parser.add_argument('--rust-dir',
                        default=os.environ.get('FASM_RUST_DIR',
                                               DEFAULT_RUST_DIR),
                        help='directory of the Rust xc7frames2bit, bitread '
                        'and xcfasm (default: %(default)s)')
    parser.add_argument('--no-bitstream',
                        action='store_true',
                        help='only compare fasm2frames')
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
    tools = None
    if not args.no_bitstream:
        tools = {
            'oracle_frames2bit': args.frames2bit_oracle,
            'oracle_bitread': args.bitread_oracle,
            'oracle_xcfasm': args.xcfasm_oracle,
            'rust_frames2bit': os.path.join(args.rust_dir, 'xc7frames2bit'),
            'rust_bitread': os.path.join(args.rust_dir, 'bitread'),
            'rust_xcfasm': os.path.join(args.rust_dir, 'xcfasm'),
        }
        rust_missing = [
            t for k, t in tools.items()
            if k.startswith('rust') and not os.access(t, os.X_OK)
        ]
        if rust_missing:
            print('difftest-xilinx: %s not found' % ', '.join(rust_missing),
                  file=sys.stderr)
            return EXIT_NOT_SET_UP
        # The oracle wrappers exist in the source tree; the binaries they
        # run only after tests/oracle/setup-xilinx.sh.
        probe = run_tool([tools['oracle_frames2bit'], '--version'])
        if probe[0] != 0:
            print('skipping the bitstream tools: %s does not run '
                  '(tests/oracle/setup-xilinx.sh)' % tools['oracle_frames2bit'])
            tools = None

    cases, notes = corpus(args.db_cache, args.filter)
    for note in notes:
        print(note)
    totals = dict.fromkeys(RULES, 0)
    failures = []
    files = set(case[1] for case in cases)
    bitstream_runs = 0
    xcfasm_cases = []
    if tools:
        seen = set()
        for name, fasm, db, part, flags in cases:
            key = (fasm, db, part)
            if key in seen or not os.path.exists(
                    os.path.join(db, part, 'part.yaml')):
                continue
            seen.add(key)
            variants = list(XCFASM_VARIANTS)
            roi = os.path.join(REPO_ROOT, fasm)[:-len('.fasm')] + '.roi.json'
            if os.path.exists(roi):
                variants.append(('roi', ['--sparse', '--roi', roi]))
            for vname, vflags in variants:
                xcfasm_cases.append(('%s[xcfasm-%s]' % (fasm, vname), fasm,
                                     db, part, vflags))
    xcfasm_failures = []
    bit_failures = []
    with tempfile.TemporaryDirectory(prefix='difftest-xilinx-') as tmpdir:
        references = []
        if tools and not args.filter:
            references = reference_bitstreams(args.bitread_oracle,
                                              args.db_cache, tmpdir)
        with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
            results = pool.map(
                lambda c: compare(c, args.oracle, args.rust, tmpdir, tools),
                cases)
            for (ok, message, rules, runs), case in zip(results, cases):
                bitstream_runs += runs
                for k, v in rules.items():
                    totals[k] += v
                if ok:
                    if args.verbose:
                        print('ok   %s' % case[0])
                else:
                    failures.append(message)
                    print('FAIL %s' % message)
            for (ok, message), case in zip(
                    pool.map(lambda c: compare_xcfasm(c, tools, tmpdir),
                             xcfasm_cases), xcfasm_cases):
                if ok:
                    if args.verbose:
                        print('ok   %s' % case[0])
                else:
                    xcfasm_failures.append(message)
                    print('FAIL %s' % message)
            for (ok, message), item in zip(
                    pool.map(lambda b: compare_reference_bit(b, tools, tmpdir),
                             references), references):
                if ok:
                    if args.verbose:
                        print('ok   bitread %s' % item[0])
                else:
                    bit_failures.append(message)
                    print('FAIL bitread %s' % message)
    print('difftest-xilinx: %d FASM files, %d runs, %d identical, %d '
          'different' % (len(files), len(cases), len(cases) - len(failures),
                         len(failures)))
    for rule, count in sorted(totals.items()):
        print('  normalisation rule %s: applied %d time(s)' % (rule, count))
    if tools:
        print('  xc7frames2bit + bitread runs on the oracle .frm files of '
              'these cases: %d (differences are counted in the cases above)' %
              bitstream_runs)
        print('difftest-xilinx: xcfasm: %d runs, %d identical, %d different' %
              (len(xcfasm_cases), len(xcfasm_cases) - len(xcfasm_failures),
               len(xcfasm_failures)))
        print('difftest-xilinx: bitread on %d reference bitstreams x %d flag '
              'sets: %d identical, %d different' %
              (len(references), len(BITREAD_FLAGS),
               len(references) - len(bit_failures), len(bit_failures)))
    any_failure = failures or xcfasm_failures or bit_failures
    return EXIT_DIFFERENCES if any_failure else EXIT_OK


if __name__ == '__main__':
    sys.exit(main())

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

prjuray mode (`--prjuray`, T6.2; `make uray-difftest`), for every part
of prjuray-db `zynqusp`:

* a corpus is generated from the part's tilegrid and segbits (seed
  `--seed`): `--uray-files` designs of random features (plain, `= 1`,
  `= 0`, annotated, multi-bit values in hex and binary; one or three per
  tile, the latter often conflicting) and error cases (unknown feature,
  unknown tile, syntax error, value out of range, an empty file), plus an
  ROI around a random tile for some designs;
* the reference prjuray `utils/fasm2frames.py`
  (`tests/oracle/uray-fasm2frames-oracle`) and the Rust
  `uray-fasm2frames` run on each with `URAY_VARIANTS` (dense, `--sparse`,
  `--sparse --debug`, `--dump_bits`, ROI): exit codes, `.frm` (16-bit
  words), stdout and stderr (rules 1 to 4 above, with prjuray's
  `utils.fasm_assembler.*` exceptions) must be identical;
* for the successful dense, sparse and ROI runs, the oracle's `.frm` is
  converted to 32-bit words (prjuray's `fasm2bit.py`), which must equal
  the Rust `fasm2frames` output for the same arguments; the reference
  (`tests/oracle/uray-xcframes2bit-oracle`) and the Rust `xcframes2bit`
  turn it into a `.bit` (`--architecture=UltraScalePlus`, identical with
  the reference time injected), and both `uray-bitread`s read that with
  `URAY_BITREAD_FLAGS`;
* both `uray-bitread`s read the Vivado bitstreams of prjuray-tools'
  `ToolsTestData.tar.gz` (Series7, UltraScale, UltraScale+) with every
  flag set, and each goes round trip: Rust `uray-bitread --frm_out`,
  both `xcframes2bit`s, both `uray-bitread`s.

`--uray-oracle-dir` (`$URAY_ORACLE_DIR`) is the `tests/oracle` directory
whose `build/` and `venv-xilinx/` the wrappers use.

Exit status: 0 if every run matches, 1 if any differs, 3 if a tool is
missing. `make xilinx-difftest` builds the Rust tools and runs this.
"""
import argparse
import atexit
import calendar
import concurrent.futures
import fnmatch
import os
import re
import shutil
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


# ---------------------------------------------------------------------------
# prjuray mode (T6.2): `--prjuray`
# ---------------------------------------------------------------------------

# The prjuray-db family and the Rust / reference tools of the mode.
URAY_FAMILY = 'zynqusp'
URAY_ARCH = 'UltraScalePlus'

URAY_VARIANTS = [
    ('dense', []),
    ('sparse', ['--sparse']),
    ('debug', ['--sparse', '--debug']),
    ('bits', ['--dump_bits']),
]
# The variants whose .frm also goes through fasm2frames (32-bit words),
# xcframes2bit and uray-bitread.
URAY_BITSTREAM_VARIANTS = ('dense', 'sparse', 'roi')

# uray-bitread flag sets (`-o` and `--aux` get a file name appended).
URAY_BITREAD_FLAGS = [
    ['-z', '-y', '-o'],
    ['-z', '-x'],
    ['-z', '-C', '-o'],
    ['-y', '-F', '0x00000000:0x000100ff', '--aux'],
    ['-f', '0x00000100', '-C'],
    ['-p', '-F', '0x00000000:0x0000003f', '-o'],
    ['-C', '-x', '-z', '-o'],
    ['-E', '-z', '-y'],
    ['-y', '-F', '0x01000000:0x010000ff'],
]

URAY_EXACT_EXCEPTIONS = EXACT_EXCEPTIONS | {
    'utils.fasm_assembler.FasmLookupError',
    'utils.fasm_assembler.FasmInconsistentBits',
}

IDENT_RE = re.compile(r'^[A-Za-z_][A-Za-z0-9_]*$')


def read_segbits(path):
    """{feature: [(is_set, word_column, word_bit)]} of a segbits file."""
    out = {}
    if not os.path.exists(path):
        return out
    with open(path) as f:
        for line in f:
            parts = line.split()
            if len(parts) < 2:
                continue
            bits = []
            for b in parts[1:]:
                m = re.match(r'^(!?)(\d+)_(\d+)$', b)
                if m:
                    bits.append((not m.group(1), int(m.group(2)),
                                 int(m.group(3))))
            out[parts[0]] = bits
    return out


def fasm_name_ok(feature):
    """A segbits feature (without the tile type) that is a FASM name."""
    base = re.sub(r'\[\d+\]$', '', feature)
    return all(IDENT_RE.match(p) for p in base.split('.'))


class UrayCorpus(object):
    """A deterministic random FASM corpus for a prjuray-db part: valid
    designs (features, multi-bit values, comments and annotations, also
    conflicting ones) and error cases."""

    def __init__(self, db, part, seed):
        import json
        import random
        self.rng = random.Random(seed)
        with open(os.path.join(db, part, 'tilegrid.json')) as f:
            grid = json.load(f)
        self.grid = grid
        segbits = {}
        self.tiles = []
        for name in sorted(grid):
            tile_type = grid[name]['type']
            if tile_type not in segbits:
                lower = tile_type.lower()
                entries = read_segbits(
                    os.path.join(db, 'segbits_%s.db' % lower))
                entries.update(
                    read_segbits(
                        os.path.join(db, 'segbits_%s.block_ram.db' % lower)))
                prefix = tile_type + '.'
                segbits[tile_type] = sorted(
                    k[len(prefix):] for k in entries
                    if k.startswith(prefix) and fasm_name_ok(k[len(prefix):]))
            if segbits[tile_type] and grid[name].get('bits'):
                self.tiles.append(name)
        self.segbits = segbits

    def feature_line(self, tile):
        features = self.segbits[self.grid[tile]['type']]
        feature = self.rng.choice(features)
        m = re.match(r'^(.*)\[(\d+)\]$', feature)
        r = self.rng.random()
        if m and r < 0.4:
            # A multi-bit value over the addresses that exist.
            base = m.group(1)
            present = sorted(
                int(re.match(r'^.*\[(\d+)\]$', f).group(1)) for f in features
                if f.startswith(base + '['))
            lo = self.rng.choice(present)
            hi = lo
            while hi + 1 in present and hi - lo < 31 and self.rng.random(
            ) < 0.9:
                hi += 1
            value = self.rng.getrandbits(hi - lo + 1)
            if self.rng.random() < 0.5:
                return "%s.%s[%d:%d] = %d'h%X" % (tile, base, hi, lo,
                                                  hi - lo + 1, value)
            return "%s.%s[%d:%d] = %d'b%s" % (tile, base, hi, lo, hi - lo + 1,
                                              format(value, 'b'))
        if r < 0.5:
            return '%s.%s = 1' % (tile, feature)
        if r < 0.55:
            return '%s.%s = 0' % (tile, feature)
        if r < 0.6:
            return '%s.%s { src = "difftest" }' % (tile, feature)
        return '%s.%s' % (tile, feature)

    def design(self, n_tiles, per_tile):
        lines = ['# difftest-xilinx --prjuray design']
        for tile in self.rng.sample(self.tiles, min(n_tiles,
                                                    len(self.tiles))):
            for _ in range(per_tile):
                lines.append(self.feature_line(tile))
        return '\n'.join(lines) + '\n'

    def files(self, count):
        """[(name, text)]: `count` designs and the error cases."""
        out = []
        for i in range(count):
            # Few features per tile rarely conflict; many often do.
            per_tile = 1 if i % 3 else 3
            out.append(('design_%02d.fasm' % i,
                        self.design(20 + 40 * (i % 5), per_tile)))
        tile = self.tiles[0]
        tile_type = self.grid[tile]['type']
        out += [
            ('unknown_feature.fasm', '%s.NO_SUCH_FEATURE\n%s.OTHER[3:0] = '
             "4'hF\n" % (tile, tile)),
            ('unknown_tile.fasm', 'NO_SUCH_TILE_X0Y0.%s\n' %
             self.segbits[tile_type][0]),
            ('syntax_error.fasm', '%s.%s = = 1\n' %
             (tile, self.segbits[tile_type][0])),
            ('value_range.fasm', "%s.%s = 2'b111\n" %
             (tile, self.segbits[tile_type][0])),
            ('empty.fasm', ''),
        ]
        return out

    def roi(self):
        """A `design.json` ROI around a random tile."""
        import json
        tile = self.grid[self.rng.choice(self.tiles)]
        x, y = tile['grid_x'], tile['grid_y']
        return json.dumps({
            'info': {
                'GRID_X_MIN': max(0, x - 3),
                'GRID_X_MAX': x + 3,
                'GRID_Y_MIN': max(0, y - 10),
                'GRID_Y_MAX': y + 10
            }
        })


def uray_parts(db):
    return sorted(p for p in os.listdir(db)
                  if os.path.exists(os.path.join(db, p, 'part.yaml')))


def to_32bit_frm(data):
    """prjuray's fasm2frames .frm (16-bit words) as the .frm of prjuray's
    fasm2bit.py (32-bit words, what xcframes2bit reads)."""
    out = []
    for line in data.decode().splitlines():
        address, words = line.split(' ')
        words = [int(w, 16) for w in words.split(',')]
        full = [(hi << 16) | lo for lo, hi in zip(words[::2], words[1::2])]
        words = ','.join('0x%08X' % w for w in full)
        out.append('0x%08X %s\n' % (int(address, 16), words))
    return ''.join(out).encode()


def uray_normalise(stderr, rules):
    """Rules 1 and 2 for prjuray's fasm2frames.py."""
    if 'Traceback (most recent call last):' in stderr:
        rules['1-traceback'] += 1
        stderr = strip_traceback(stderr)
    return normalise(stderr, rules)


def compare_uray(case, tools, tmpdir):
    """uray-fasm2frames oracle vs Rust, then the bitstream tools; returns
    (ok, message, rules, bitstream runs)."""
    name, fasm, db, part, flags = case
    tag = re.sub(r'[^\w.-]', '_', name)
    args = ['--db-root', db, '--part', part] + flags + [fasm]
    o = run(tools['oracle_fasm2frames'], args,
            os.path.join(tmpdir, tag + '.oracle.frm'))
    r = run(tools['rust_fasm2frames'], args,
            os.path.join(tmpdir, tag + '.rust.frm'))
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
    o_err = uray_normalise(o[2], rules)
    o_type, _ = exception_type(o_err)
    if (CTYPES_MARKER in o_err and o_err.rstrip('\n').endswith(NONE_TYPE)
            and PARSE_ERROR_RE.match(r[2])):
        rules['4-value-range'] += 1
        if o[0] != 1 or r[0] != 1:
            problems.append('value range error: expected exit code 1')
    elif (o_type is not None and o_type not in URAY_EXACT_EXCEPTIONS
          and r[2].startswith('fasm_xilinx.DbError: ')):
        rules['3-db-error'] += 1
        if o[0] != 1 or r[0] != 1:
            problems.append('database error: expected exit code 1')
    elif o_err != normalise(r[2], dict.fromkeys(RULES, 0)):
        problems.append('stderr differs:\n--- oracle\n%s--- rust\n%s' %
                        (o_err, r[2]))
    runs = 0
    variant = name.rsplit('[', 1)[-1].rstrip(']')
    if not problems and o[0] == 0 and variant in URAY_BITSTREAM_VARIANTS:
        frm32 = to_32bit_frm(o[3])
        # The Rust fasm2frames (xc_fasm's command line) writes the 32-bit
        # frames directly for a prjuray-db part.
        path = os.path.join(tmpdir, tag + '.fasm2frames.frm')
        x = run(tools['rust_xc_fasm2frames'], args, path)
        runs += 1
        if x[0] != 0 or x[3] != frm32:
            problems.append('fasm2frames (32-bit words) differs from the '
                            'converted oracle .frm (exit code %d)' % x[0])
        frm = os.path.join(tmpdir, tag + '.frm32')
        with open(frm, 'wb') as f:
            f.write(frm32)
        more, n = compare_uray_bitstream(frm,
                                         os.path.join(db, part, 'part.yaml'),
                                         part, tools, tmpdir, tag)
        problems += more
        runs += n
        os.remove(frm)
    return (not problems, '%s: %s' % (name, '; '.join(problems)), rules,
            runs)


def uray_frames2bit(tools, side, frm, bit, part, part_file, arch, env=None):
    argv = [
        tools[side + '_xcframes2bit'], '--architecture=' + arch,
        '--frm_file=' + frm, '--output_file=' + bit, '--part_name=' + part,
        '--part_file=' + part_file
    ]
    return run_tool(argv, env=env)


def compare_uray_bitstream(frm,
                           part_file,
                           part,
                           tools,
                           tmpdir,
                           tag,
                           arch=URAY_ARCH):
    """xcframes2bit (oracle, Rust) on `frm`, then uray-bitread (oracle,
    Rust) on the reference .bit; returns (problems, runs)."""
    bit = os.path.join(tmpdir, tag + '.bit')
    o = uray_frames2bit(tools, 'oracle', frm, bit, part, part_file, arch)
    epoch = bit_time(bit)
    o_files = read_and_remove([bit])
    env = {'SOURCE_DATE_EPOCH': str(epoch)} if epoch is not None else {}
    r = uray_frames2bit(tools, 'rust', frm, bit, part, part_file, arch, env)
    r_files = read_and_remove([bit])
    problems = compare_runs('xcframes2bit', o + (o_files, ), r + (r_files, ))
    data = o_files[tag + '.bit']
    if problems or o[0] != 0 or data is None:
        return problems, 1
    with open(bit, 'wb') as f:
        f.write(data)
    for i, flags in enumerate(URAY_BITREAD_FLAGS):
        problems += run_uray_bitread(tools, part_file, bit,
                                     ['--architecture=' + arch] + flags,
                                     tmpdir, '%s.%d' % (tag, i))
    os.remove(bit)
    return problems, 1 + len(URAY_BITREAD_FLAGS)


def run_uray_bitread(tools, part_file, bit, flags, tmpdir, tag):
    """Both uray-bitreads with `flags` on `bit`; returns the problems."""
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
        out = normalise_gflags_paths(out, 'uray-bitread')
        results.append((code, out, err, read_and_remove(files)))
    return compare_runs('uray-bitread %s' % ' '.join(flags), *results)


def uray_reference_bitstreams(oracle_dir, tmpdir):
    """(name, arch, part_file, bit) of the Vivado bitstreams of
    prjuray-tools' ToolsTestData.tar.gz (all three architectures)."""
    tools = os.path.join(oracle_dir, 'build', 'xilinx', 'src',
                         'prjuray-tools', 'lib', 'test_data',
                         'ToolsTestData.tar.gz')
    if not os.path.exists(tools):
        return []
    members = [
        ('Series7', 'part.yaml', ('design.bit', 'bram.bit')),
        ('UltraScale', 'part.yaml', ('design.bit', )),
        ('UltraScalePlus', 'part.yaml', ('design.bit', )),
        ('UltraScalePlus', 'test.yaml', ('test.bit', )),
    ]
    out = []
    with tarfile.open(tools) as tar:
        for arch, yaml, bits in members:
            for member in (yaml, ) + bits:
                tar.extract('%s/%s' % (arch, member), tmpdir)
            for bit in bits:
                out.append(('ToolsTestData/%s/%s' % (arch, bit), arch,
                            os.path.join(tmpdir, arch, yaml),
                            os.path.join(tmpdir, arch, bit)))
    return out


def compare_uray_reference_bit(item, tools, tmpdir):
    """Every uray-bitread flag set on a reference bitstream, then the
    round trip: Rust uray-bitread --frm_out, both xcframes2bit (and the
    bitread flag sets on the result)."""
    name, arch, part_file, bit = item
    tag = re.sub(r'[^\w.-]', '_', name)
    problems = []
    for i, flags in enumerate(URAY_BITREAD_FLAGS):
        problems += run_uray_bitread(tools, part_file, bit,
                                     ['--architecture=' + arch] + flags,
                                     tmpdir, '%s.%d' % (tag, i))
    frm = os.path.join(tmpdir, tag + '.frm')
    code, _, err = run_tool([
        tools['rust_bitread'], '--part_file=' + part_file,
        '--architecture=' + arch, '--frm_out=' + frm, bit
    ])
    if code != 0:
        problems.append('uray-bitread --frm_out failed: %r' % err)
    else:
        more, _ = compare_uray_bitstream(frm, part_file, 'part', tools,
                                         tmpdir, tag + '.rt', arch)
        problems += more
        os.remove(frm)
    return not problems, '%s: %s' % (name, '; '.join(problems))


def main_prjuray(args):
    """The prjuray mode: returns the exit status."""
    oracle_dir = args.uray_oracle_dir
    wrappers = os.path.join(REPO_ROOT, 'tests', 'oracle')
    tools = {
        'oracle_fasm2frames': os.path.join(wrappers,
                                           'uray-fasm2frames-oracle'),
        'oracle_xcframes2bit': os.path.join(wrappers,
                                            'uray-xcframes2bit-oracle'),
        'oracle_bitread': os.path.join(wrappers, 'uray-bitread-oracle'),
        'rust_fasm2frames': os.path.join(args.rust_dir, 'uray-fasm2frames'),
        'rust_xc_fasm2frames': os.path.join(args.rust_dir, 'fasm2frames'),
        'rust_xcframes2bit': os.path.join(args.rust_dir, 'xcframes2bit'),
        'rust_bitread': os.path.join(args.rust_dir, 'uray-bitread'),
    }
    os.environ['URAY_ORACLE_DIR'] = oracle_dir
    missing = [t for t in tools.values() if not os.access(t, os.X_OK)]
    if not os.path.exists(
            os.path.join(oracle_dir, 'build', 'xilinx', 'bin',
                         'uray-bitread')):
        missing.append(os.path.join(oracle_dir, 'build', 'xilinx', 'bin'))
    if missing:
        print('difftest-xilinx: %s not found' % ', '.join(missing),
              file=sys.stderr)
        return EXIT_NOT_SET_UP
    db = os.path.join(args.db_cache, 'prjuray-db', URAY_FAMILY)
    if not os.path.isdir(db):
        print('difftest-xilinx: %s not found (tools/fetch-db.sh prjuray %s)' %
              (db, URAY_FAMILY),
              file=sys.stderr)
        return EXIT_NOT_SET_UP
    totals = dict.fromkeys(RULES, 0)
    failures = []
    ref_failures = []
    bitstream_runs = 0
    with tempfile.TemporaryDirectory(prefix='difftest-uray-') as tmpdir:
        cases = []
        parts = uray_parts(db)
        for i, part in enumerate(parts):
            corpus = UrayCorpus(db, part, args.seed + i)
            part_dir = os.path.join(tmpdir, 'corpus', part)
            os.makedirs(part_dir)
            roi = os.path.join(part_dir, 'roi.json')
            with open(roi, 'w') as f:
                f.write(corpus.roi())
            for name, text in corpus.files(args.uray_files):
                fasm = os.path.join(part_dir, name)
                with open(fasm, 'w') as f:
                    f.write(text)
                variants = list(URAY_VARIANTS)
                if name.startswith('design_0'):
                    variants.append(('roi', ['--sparse', '--roi', roi]))
                for vname, flags in variants:
                    case_name = '%s/%s[%s]' % (part, name, vname)
                    if args.filter and not fnmatch.fnmatch(
                            case_name, args.filter):
                        continue
                    cases.append((case_name, fasm, db, part, flags))
        references = []
        if not args.filter:
            references = uray_reference_bitstreams(oracle_dir, tmpdir)
        with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
            for (ok, message, rules, runs), case in zip(
                    pool.map(lambda c: compare_uray(c, tools, tmpdir), cases),
                    cases):
                bitstream_runs += runs
                for k, v in rules.items():
                    totals[k] += v
                if ok:
                    if args.verbose:
                        print('ok   %s' % case[0])
                else:
                    failures.append(message)
                    print('FAIL %s' % message)
            for (ok, message), item in zip(
                    pool.map(
                        lambda b: compare_uray_reference_bit(b, tools, tmpdir
                                                             ), references),
                    references):
                if ok:
                    if args.verbose:
                        print('ok   uray-bitread %s' % item[0])
                else:
                    ref_failures.append(message)
                    print('FAIL uray-bitread %s' % message)
    print('difftest-xilinx --prjuray: %d parts, %d FASM files, %d '
          'uray-fasm2frames runs, %d identical, %d different' %
          (len(parts), len(set(c[1] for c in cases)), len(cases),
           len(cases) - len(failures), len(failures)))
    for rule, count in sorted(totals.items()):
        print('  normalisation rule %s: applied %d time(s)' % (rule, count))
    print('  fasm2frames + xcframes2bit + uray-bitread runs on the oracle '
          '.frm files of these cases: %d (differences are counted in the '
          'cases above)' % bitstream_runs)
    print('difftest-xilinx --prjuray: uray-bitread on %d reference '
          'bitstreams x %d flag sets, and their bit -> frm -> bit round '
          'trip: %d identical, %d different' %
          (len(references), len(URAY_BITREAD_FLAGS),
           len(references) - len(ref_failures), len(ref_failures)))
    return EXIT_DIFFERENCES if failures or ref_failures else EXIT_OK


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
    parser.add_argument('--prjuray',
                        action='store_true',
                        help='the prjuray mode (UltraScale+, prjuray-db %s): '
                        'uray-fasm2frames, fasm2frames, xcframes2bit and '
                        'uray-bitread on a generated corpus and on the '
                        'ToolsTestData bitstreams' % URAY_FAMILY)
    parser.add_argument('--uray-oracle-dir',
                        default=os.environ.get('URAY_ORACLE_DIR',
                                               oracle_dir),
                        help='the tests/oracle directory with the prjuray '
                        'tools build and venv (default: %(default)s)')
    parser.add_argument('--uray-files',
                        type=int,
                        default=20,
                        help='generated designs per part (default: '
                        '%(default)s)')
    parser.add_argument('--seed',
                        type=int,
                        default=1,
                        help='seed of the generated corpus (default: '
                        '%(default)s)')
    parser.add_argument('--jobs', type=int, default=os.cpu_count() or 1)
    parser.add_argument('-v', '--verbose', action='store_true')
    args = parser.parse_args()

    # The Rust tools' binary database cache (FASM_XDB_CACHE, see
    # docs/rewrite/DESIGN-xilinx-db.md §8.8): a temporary directory of
    # this run unless set, so that the cached path is covered (written by
    # the first runs, loaded by the others) without writing into ~/.cache.
    if 'FASM_XDB_CACHE' not in os.environ:
        cache_dir = tempfile.mkdtemp(prefix='fasm-xdb-cache-')
        atexit.register(shutil.rmtree, cache_dir, True)
        os.environ['FASM_XDB_CACHE'] = cache_dir

    if args.prjuray:
        return main_prjuray(args)

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

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
  `tools/fetch-db.sh prjxray <family>`); a directory with a
  `difftest.json` (`{"part": ..., "family": ...}`) names its own part
  (and family), and its `*.fasm.xz` files are compared too (the
  f4pga-examples designs of T7.3); `--corpus-root DIR` compares the
  FASM files under DIR instead (e.g. the outputs of
  `tools/e2e/run-f4pga-examples.sh`);
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

All parts of a family (T5.9): with `--family artix7` (`--families
artix7,kintex7,spartan7,zynq7`, a family missing from `--db-cache` is
fetched with `tools/fetch-db.sh` after checking the free space) and
`--all-parts` or `--parts GLOB[,GLOB]`, the corpus of each part is
generated by `tools/gen-xilinx-corpus.py` (`--tiles`, `--seed`,
`--density`, `--max-per-tile`) into `--work-dir` (reused while the
generator, its options and the database are the same) and compared: the
first features file with every `VARIANTS` flag set (dense, sparse and
pudc also through xc7frames2bit and every `BITREAD_FLAGS` set), once more
`--sparse` with `FASM_XDB_CACHE=0` for the Rust tool (every other Rust run
uses a per part cache directory, written by the first run), the other
features files `--sparse` (through xc7frames2bit and two bitread flag
sets), the error files `--sparse`, and xcfasm with `XCFASM_VARIANTS`.
Parts run in parallel (`--jobs`), each part's runs in sequence; a table
per part and the totals are printed (`--json-report` writes them). The
reference results are cached in `<work-dir>/results`, keyed by the
command line, the content of its input files, the reference tools
(wrappers, binaries, venv packages) and the database commit, so a rerun
only runs the Rust tools (`--no-result-cache` runs everything); large
cached outputs are kept as a SHA-256 only. Differences where rule 3 or 4
applied are counted as "explained".

The reference gflags tools print their own path (`argv[0]`) in some
messages; it is replaced by `PROG` on both sides.

prjuray mode (`--prjuray`, T6.2, T6.3; `make uray-difftest-all`), for
every part of every prjuray-db family found in `--db-cache` (the
directories of `prjuray-db/` with a `tile_types/`; `zynqusp` is fetched
with `tools/fetch-db.sh` when there is none), or of `--families` /
`--parts GLOB`, parts in parallel with `--jobs`, a table per part and the
totals (`--json-report`), the reference results cached in `--work-dir`
like in the all-parts mode:

* the every-feature corpus of `tools/gen-xilinx-corpus.py` (same
  options) goes through the reference prjuray `utils/fasm2frames.py`
  (`tests/oracle/uray-fasm2frames-oracle`) and the Rust
  `uray-fasm2frames`: the first features file with `URAY_VARIANTS`
  (dense, `--sparse`, `--sparse --debug`, `--dump_bits`) and an ROI,
  once more `--sparse` with `FASM_XDB_CACHE=0` for the Rust tool, the
  other features files and the error files `--sparse`;
* the random corpus of T6.2 (seed `--uray-seed` plus the index of the
  part): `--uray-files` designs of random features (plain, `= 1`,
  `= 0`, annotated, multi-bit values in hex and binary; one or three per
  tile, the latter often conflicting) and error cases (unknown feature,
  unknown tile, syntax error, value out of range, an empty file), with
  every `URAY_VARIANTS` flag set, the first ten designs also with the ROI;
* exit codes, `.frm` (16-bit words), stdout and stderr (rules 1 to 4
  above, with prjuray's `utils.fasm_assembler.*` exceptions) must be
  identical;
* for the successful dense, sparse and ROI runs (and the other features
  files), the oracle's `.frm` is converted to 32-bit words (prjuray's
  `fasm2bit.py`), which must equal the Rust `fasm2frames` output for the
  same arguments; the reference (`tests/oracle/uray-xcframes2bit-oracle`)
  and the Rust `xcframes2bit` turn it into a `.bit`
  (`--architecture=UltraScalePlus`, identical with the reference time
  injected), and both `uray-bitread`s read that with `URAY_BITREAD_FLAGS`
  (the other features files: the first two sets);
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
import base64
import calendar
import concurrent.futures
import fnmatch
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import zlib

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_ORACLE = os.path.join(REPO_ROOT, 'tests', 'oracle',
                              'fasm2frames-oracle')
DEFAULT_RUST = os.path.join(REPO_ROOT, 'target', 'release', 'fasm2frames')
DEFAULT_RUST_DIR = os.path.join(REPO_ROOT, 'target', 'release')
DEFAULT_DB_CACHE = os.environ.get(
    'FASM_DB_CACHE', os.path.join(REPO_ROOT, 'tests', 'oracle', 'build',
                                  'db'))
DEFAULT_WORK_DIR = os.path.join(REPO_ROOT, 'tests', 'oracle', 'build',
                                'difftest-xilinx')
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

# Version of the oracle result cache entries (Runner).
CACHE_FORMAT = 1
# Larger cached outputs (compressed, base64) are kept as a hash only.
CACHE_KEEP_LIMIT = 32 * 1024

EXIT_OK = 0
EXIT_DIFFERENCES = 1
EXIT_NOT_SET_UP = 3

RULES = ('1-traceback', '2-parse-message', '3-db-error', '4-value-range')
CTYPES_MARKER = 'Exception ignored on calling ctypes callback function'
NONE_TYPE = "TypeError: 'NoneType' object is not iterable"

PARSE_ERROR_RE = re.compile(r'^Exception: Parse error at (\d+):(\d+) - .*$',
                            re.S)


# The Rust parser's messages for the value range errors of rule 4 (a value
# wider than its range or declared width, an address range whose end is
# before its start).
VALUE_RANGE_MESSAGE_RE = re.compile(r'does not fit|before its start')


def is_rust_value_range_error(stderr):
    """The Rust side of rule 4: a parse error about a value range."""
    return bool(
        PARSE_ERROR_RE.match(stderr)
        and VALUE_RANGE_MESSAGE_RE.search(stderr.split('\n', 1)[0]))


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


class Runner(object):
    """Runs the tools; with a cache directory, the oracle's results are
    kept on disk, keyed by the command line, the environment given, the
    content of every file argument and `salt` (the reference tools and
    the databases), so that a rerun only runs the Rust tools."""

    def __init__(self, cache_dir=None, salt=''):
        self.cache_dir = cache_dir
        self.salt = salt
        self.lock = threading.Lock()
        self.counts = {'oracle': 0, 'cached': 0, 'rust': 0}

    def count(self, what):
        with self.lock:
            self.counts[what] += 1

    def key(self, argv, env, outputs):
        h = hashlib.sha256()
        h.update(json.dumps([CACHE_FORMAT, self.salt, argv,
                             sorted((env or {}).items()),
                             list(outputs)]).encode())
        for arg in argv:
            path = arg.split('=', 1)[-1] if arg.startswith('--') else arg
            if path not in outputs and os.path.isfile(path):
                h.update(path.encode() + b'\0')
                with open(path, 'rb') as f:
                    for block in iter(lambda: f.read(1 << 20), b''):
                        h.update(block)
        return h.hexdigest()

    def run(self, argv, env=None, outputs=(), oracle=False, keep=()):
        """(exit code, stdout, stderr, {basename: bytes or None}) of the
        outputs, which are removed; a SIGABRT is exit code 134. The cache
        keeps stdout and the outputs larger than CACHE_KEEP_LIMIT
        compressed bytes as a Digest (which compares equal to the bytes
        it was made from), except the outputs named in `keep`."""
        path = None
        if oracle and self.cache_dir:
            path = os.path.join(self.cache_dir,
                                self.key(argv, env, outputs) + '.json')
            try:
                with open(path) as f:
                    data = json.load(f)
                self.count('cached')
                files = dict(
                    (k, unpack(v)) for k, v in data['files'].items())
                if all(not isinstance(files[os.path.basename(k)], Digest)
                       for k in keep):
                    return (data['code'], unpack(data['stdout']),
                            unpack(data['stderr']), files)
            except (OSError, ValueError, KeyError):
                pass
        full_env = dict(os.environ)
        full_env.update(env or {})
        result = subprocess.run(argv,
                                cwd=REPO_ROOT,
                                env=full_env,
                                stdin=subprocess.DEVNULL,
                                capture_output=True,
                                timeout=3600)
        self.count('oracle' if oracle else 'rust')
        code = result.returncode
        if code == -6:
            code = 134
        files = read_and_remove(outputs)
        if path is not None:
            os.makedirs(self.cache_dir, exist_ok=True)
            tmp = '%s.%d.%d.tmp' % (path, os.getpid(), threading.get_ident())
            packed = {}
            for k in outputs:
                name = os.path.basename(k)
                packed[name] = pack(files[name], k not in keep)
            with open(tmp, 'w') as f:
                json.dump(
                    {
                        'argv': argv,
                        'code': code,
                        'stdout': pack(result.stdout, True),
                        'stderr': pack(result.stderr),
                        'files': packed
                    }, f)
            os.replace(tmp, path)
        return code, result.stdout, result.stderr, files


class Digest(object):
    """The SHA-256 and size of a cached output: equal to bytes with that
    hash."""

    def __init__(self, sha, size):
        self.sha = sha
        self.size = size

    def __eq__(self, other):
        if isinstance(other, Digest):
            return self.sha == other.sha
        if isinstance(other, bytes):
            return hashlib.sha256(other).hexdigest() == self.sha
        return NotImplemented

    def __ne__(self, other):
        equal = self.__eq__(other)
        return equal if equal is NotImplemented else not equal

    def __len__(self):
        return self.size

    def __getitem__(self, index):
        return ('<cached output, sha256 %s, rerun with --no-result-cache '
                'to see it>' % self.sha).encode()

    def __repr__(self):
        return 'Digest(%r, %d)' % (self.sha, self.size)


def pack(data, digest=False):
    if data is None:
        return None
    text = base64.b64encode(zlib.compress(data, 6)).decode('ascii')
    if digest and len(text) > CACHE_KEEP_LIMIT:
        return {'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data)}
    return text


def unpack(value):
    if value is None:
        return None
    if isinstance(value, dict):
        return Digest(value['sha256'], value['size'])
    return zlib.decompress(base64.b64decode(value))


PLAIN_RUNNER = Runner()


def run(runner, tool, args, out_path, env=None, oracle=False, keep=False):
    """fasm2frames: (exit code, stdout, stderr text, .frm bytes or None)."""
    code, out, err, files = runner.run([tool] + args + [out_path],
                                       env=env,
                                       outputs=[out_path],
                                       oracle=oracle,
                                       keep=[out_path] if keep else [])
    return (code, out, err.decode('utf-8', 'surrogateescape'),
            files[os.path.basename(out_path)])


def compare(case,
            oracle,
            rust,
            tmpdir,
            tools=None,
            runner=PLAIN_RUNNER,
            rust_env=None,
            bitstream=None,
            bitread_flags=None):
    """Runs one case, returns (ok, message, rules applied, bitstream tool
    runs). `rust_env`: extra environment of the Rust tool (e.g.
    FASM_XDB_CACHE=0); `bitstream`: whether the oracle's .frm also goes
    through the bitstream tools (default: the variants of
    BITSTREAM_VARIANTS), with `bitread_flags` (default BITREAD_FLAGS)."""
    name, fasm, db, part, flags = case
    args = ['--db-root', db, '--part', part] + flags + [fasm]
    tag = re.sub(r'[^\w.-]', '_', name)
    if bitstream is None:
        variant = name.rsplit('[', 1)[-1].rstrip(']')
        bitstream = variant in BITSTREAM_VARIANTS
    o = run(runner,
            oracle,
            args,
            os.path.join(tmpdir, tag + '.oracle.frm'),
            oracle=True,
            keep=bool(tools and bitstream))
    r = run(runner,
            rust,
            args,
            os.path.join(tmpdir, tag + '.rust.frm'),
            env=rust_env)
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
            and is_rust_value_range_error(r[2])):
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
                        (o_err[:4000], r[2][:4000]))
    bitstream_runs = 0
    part_file = os.path.join(db, part, 'part.yaml')
    if (tools and not problems and o[0] == 0 and bitstream
            and os.path.exists(part_file)):
        frm = os.path.join(tmpdir, tag + '.oracle.bitstream.frm')
        with open(frm, 'wb') as f:
            f.write(o[3])
        more, bitstream_runs = compare_bitstream(frm, part_file, part, tools,
                                                 tmpdir, tag, runner,
                                                 bitread_flags)
        problems += more
        os.remove(frm)
    return (not problems, '%s: %s' % (name, '; '.join(problems)), rules,
            bitstream_runs)


def bit_time(data):
    """The seconds since the epoch of a .bit header's date and time, or
    None."""
    if data is None:
        return None
    m = re.search(
        rb'c\x00\x0b(\d{4})/(\d\d)/(\d\d)\x00d\x00\x09'
        rb'(\d\d):(\d\d):(\d\d)\x00', data[:4096])
    if not m:
        return None
    return calendar.timegm(tuple(int(g) for g in m.groups()))


def run_tool(argv, env=None, runner=PLAIN_RUNNER, oracle=False):
    """(exit code, stdout, stderr); a SIGABRT is exit code 134."""
    return runner.run(argv, env=env, oracle=oracle)[:3]


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
            if (a is not None and b is not None
                    and not isinstance(a, Digest)):
                first = next((i for i in range(min(len(a), len(b)))
                              if a[i] != b[i]), min(len(a), len(b)))
            problems.append('%s: file %s differs (%s vs %s bytes, first '
                            'difference at %s)' %
                            (what, name, None if a is None else len(a),
                             None if b is None else len(b), first))
    return problems


def run_bitread(tools,
                part_file,
                bit,
                flags,
                tmpdir,
                tag,
                runner=PLAIN_RUNNER,
                tool='bitread'):
    """Runs both bitreads (`tool`: bitread or uray-bitread, the name the
    reference prints) with `flags` on `bit`; returns the problems."""
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
        argv = [tools[side + '_bitread'], '--part_file=' + part_file]
        code, out, err, outputs = runner.run(
            argv + args + [bit],
            outputs=files,
            oracle=side == 'oracle')
        if not isinstance(out, Digest) and not isinstance(
                results[0][1] if results else None, Digest):
            # (A cached large output has no path to normalise.)
            out = normalise_gflags_paths(out, tool)
        results.append((code, out, err, outputs))
    return compare_runs('%s %s' % (tool, ' '.join(flags)), *results)


def compare_bitstream(frm,
                      part_file,
                      part,
                      tools,
                      tmpdir,
                      tag,
                      runner=PLAIN_RUNNER,
                      bitread_flags=None):
    """xc7frames2bit on the oracle's .frm, then bitread on the .bit;
    returns (problems, runs)."""
    if bitread_flags is None:
        bitread_flags = BITREAD_FLAGS
    bit = os.path.join(tmpdir, tag + '.bit')
    base = [
        '--frm_file=' + frm, '--output_file=' + bit, '--part_name=' + part,
        '--part_file=' + part_file
    ]
    o = runner.run([tools['oracle_frames2bit']] + base,
                   outputs=[bit],
                   oracle=True,
                   keep=[bit])
    data = o[3][tag + '.bit']
    epoch = bit_time(data)
    env = {'SOURCE_DATE_EPOCH': str(epoch)} if epoch is not None else {}
    r = runner.run([tools['rust_frames2bit']] + base, env=env, outputs=[bit])
    problems = compare_runs('xc7frames2bit', o, r)
    if problems or o[0] != 0 or data is None:
        return problems, 1
    with open(bit, 'wb') as f:
        f.write(data)
    for i, flags in enumerate(bitread_flags):
        problems += run_bitread(tools, part_file, bit, flags, tmpdir,
                                '%s.%d' % (tag, i), runner)
    os.remove(bit)
    return problems, 1 + len(bitread_flags)


def compare_xcfasm(case, tools, tmpdir, runner=PLAIN_RUNNER, rust_env=None):
    """xcfasm-oracle vs the Rust xcfasm; (ok, message, rules applied)."""
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
    o = runner.run([tools['oracle_xcfasm']] + args,
                   env={'PATH': bin_dir + os.pathsep + os.environ['PATH']},
                   outputs=[frm, bit],
                   oracle=True,
                   keep=[bit])
    epoch = bit_time(o[3][os.path.basename(bit)])
    env = dict(rust_env or {})
    if epoch is not None:
        env['SOURCE_DATE_EPOCH'] = str(epoch)
    r = runner.run([tools['rust_xcfasm']] + args, env=env, outputs=[frm, bit])
    o_full = o[2].decode('utf-8', 'surrogateescape')
    o_err = strip_traceback(o_full)
    r_err = r[2].decode('utf-8', 'surrogateescape')
    rules = dict.fromkeys(RULES, 0)
    if o_err != o_full:
        rules['1-traceback'] += 1
    o_type, _ = exception_type(o_err)
    if (CTYPES_MARKER in o_err and o_err.rstrip('\n').endswith(NONE_TYPE)
            and is_rust_value_range_error(r_err)):
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
    problems = compare_runs('xcfasm', (o[0], o[1], o_err.encode(), o[3]),
                            (r[0], r[1], r_err.encode(), r[3]))
    return not problems, '%s: %s' % (name, '; '.join(problems)), rules


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


def compare_reference_bit(item, tools, tmpdir, runner=PLAIN_RUNNER):
    name, part_file, bit = item
    tag = re.sub(r'[^\w.-]', '_', name)
    problems = []
    for i, flags in enumerate(BITREAD_FLAGS):
        problems += run_bitread(tools, part_file, bit, flags, tmpdir,
                                '%s.%d' % (tag, i), runner)
    return not problems, '%s: %s' % (name, '; '.join(problems))


# A directory with this file holds FASM files of one part: a JSON object
# with "part" and optionally "family" (the prjxray-db family; default: the
# first directory under the corpus root). Its `*.fasm.xz` files are also
# compared (decompressed into a temporary directory); elsewhere only
# `*.fasm`.
DIFFTEST_JSON = 'difftest.json'
_XZ_DIR = []


def decompressed(path):
    """A decompressed copy of the `.fasm.xz` file `path`."""
    import lzma
    if not _XZ_DIR:
        _XZ_DIR.append(tempfile.mkdtemp(prefix='difftest-xilinx-xz-'))
        atexit.register(shutil.rmtree, _XZ_DIR[0], True)
    digest = hashlib.sha256(path.encode()).hexdigest()[:16]
    out = os.path.join(_XZ_DIR[0], digest,
                       os.path.basename(path)[:-len('.xz')])
    if not os.path.exists(out):
        os.makedirs(os.path.dirname(out), exist_ok=True)
        with lzma.open(path) as f, open(out, 'wb') as g:
            shutil.copyfileobj(f, g)
    return out


def corpus(db_cache, pattern, root=None):
    """The (name, fasm, db, part, flags) cases and skipped notes.

    `root`: the corpus (default tests/corpus/xilinx, plus
    tests/corpus/f4pga-xc-fasm with the miniature database)."""
    cases = []
    notes = []
    sources = []
    default = root is None
    xilinx = os.path.join(REPO_ROOT, 'tests', 'corpus',
                          'xilinx') if default else os.path.abspath(root)
    for dirpath, _, files in sorted(os.walk(xilinx)):
        fasms = sorted(
            f for f in files if f.endswith('.fasm') or (
                f.endswith('.fasm.xz') and DIFFTEST_JSON in files))
        if not fasms:
            continue
        rel_dir = os.path.relpath(dirpath, xilinx)
        family = rel_dir.split(os.sep)[0]
        part = FAMILY_PARTS.get(family)
        if DIFFTEST_JSON in files:
            with open(os.path.join(dirpath, DIFFTEST_JSON)) as f:
                config = json.load(f)
            part = config['part']
            family = config.get('family', family)
        if part is None:
            notes.append('no part for %s' % rel_dir)
            continue
        db = os.path.join(db_cache, 'prjxray-db', family)
        if not os.path.isdir(db):
            notes.append('skipping %s: %s not found (tools/fetch-db.sh '
                         'prjxray %s)' % (rel_dir, db, family))
            continue
        sources.append((dirpath, fasms, db, part))
    if default:
        mini = os.path.join(REPO_ROOT, 'tests', 'corpus', 'f4pga-xc-fasm')
        for dirpath, _, files in sorted(os.walk(mini)):
            fasms = sorted(f for f in files if f.endswith('.fasm'))
            if fasms:
                sources.append((dirpath, fasms, MINI_DB, 'xc7'))
    for dirpath, fasms, db, part in sources:
        for f in fasms:
            path = os.path.join(dirpath, f)
            rel = os.path.relpath(path, REPO_ROOT)
            if rel.startswith('..'):
                rel = path
            if pattern and not fnmatch.fnmatch(rel, pattern):
                continue
            fasm = decompressed(path) if f.endswith('.xz') else path
            variants = list(VARIANTS)
            stem = path[:-len('.fasm.xz' if f.endswith('.xz') else '.fasm')]
            roi = stem + '.roi.json'
            if os.path.exists(roi):
                variants.append(('roi', ['--sparse', '--roi', roi]))
            for vname, flags in variants:
                cases.append(('%s[%s]' % (rel, vname), fasm, db, part,
                              flags))
    return cases, notes


# ---------------------------------------------------------------------
# All parts of a family (T5.9): a generated corpus per part.
# ---------------------------------------------------------------------
GENERATOR = os.path.join(REPO_ROOT, 'tools', 'gen-xilinx-corpus.py')
FETCH_DB = os.path.join(REPO_ROOT, 'tools', 'fetch-db.sh')
PRJXRAY_FAMILIES = ('artix7', 'kintex7', 'spartan7', 'zynq7')
# Free space wanted before fetching a family (each is 180-250 MiB checked
# out, plus the git objects).
FETCH_MIN_FREE = 1 << 30
# Mean seconds (reference and Rust tools, generation) per part of the
# default corpus, from the first full run (docs/rewrite/DESIGN-xilinx-db.md
# 8.9): for the up front estimate.
SECONDS_PER_PART = 130
EXPLAINED_RULES = ('3-db-error', '4-value-range')


def file_sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for block in iter(lambda: f.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()


def find_family(db_dirs, family):
    for d in db_dirs:
        path = os.path.join(d, 'prjxray-db', family)
        if os.path.isdir(os.path.join(path, 'mapping')):
            return path
    return None


def fetch_family(db_dirs, family, repo='prjxray'):
    """tools/fetch-db.sh REPO FAMILY into the first database directory,
    if there is room; returns the family directory or None."""
    target = db_dirs[0]
    os.makedirs(target, exist_ok=True)
    free = shutil.disk_usage(target).free
    print('difftest-xilinx: %s has %.1f GiB free' % (target, free / 2.0**30))
    if free < FETCH_MIN_FREE:
        print('difftest-xilinx: not fetching %s: less than %.1f GiB free' %
              (family, FETCH_MIN_FREE / 2.0**30))
        return None
    env = dict(os.environ)
    env['FASM_DB_CACHE'] = target
    result = subprocess.run([FETCH_DB, repo, family], env=env)
    if result.returncode != 0:
        print('difftest-xilinx: fetching %s failed' % family)
        return None
    if repo == 'prjuray':
        return find_uray_families([target]).get(family)
    return find_family([target], family)


def db_commit(family_dir):
    """The commit of the prjxray-db checkout (a detached HEAD, or the
    branch HEAD names, loose or packed), or None."""
    git = os.path.join(os.path.dirname(family_dir), '.git')
    try:
        with open(os.path.join(git, 'HEAD')) as f:
            head = f.read().strip()
    except OSError:
        return None
    if not head.startswith('ref: '):
        return head if re.match(r'^[0-9a-f]{40}$', head) else None
    ref = head[len('ref: '):]
    try:
        with open(os.path.join(git, ref)) as f:
            return f.read().strip()
    except OSError:
        pass
    try:
        with open(os.path.join(git, 'packed-refs')) as f:
            for line in f:
                parts = line.split()
                if len(parts) == 2 and parts[1] == ref:
                    return parts[0]
    except OSError:
        pass
    return None


ORACLE_SUBDIRS = ('build/xilinx/bin', 'venv-xilinx/lib')
ORACLE_PACKAGES = ('/prjxray', '/xc_fasm', '/fasm')


def oracle_identity(paths,
                    oracle_dirs=None,
                    subdirs=ORACLE_SUBDIRS,
                    packages=ORACLE_PACKAGES):
    """Identifies the reference tools: the wrappers' content, the size and
    modification time of the binaries they run and of the Python packages
    of the oracle venv (`subdirs` of the directory of each wrapper, or of
    `oracle_dirs`; of the venv only the `packages`)."""
    h = hashlib.sha256()

    def walk(oracle_dir):
        for sub in subdirs:
            top = os.path.join(oracle_dir, sub)
            for dirpath, dirnames, files in os.walk(top):
                dirnames[:] = sorted(d for d in dirnames
                                     if d not in ('__pycache__', 'tests'))
                # Match `packages` against the path relative to the oracle
                # (venv) root, not the absolute path: an absolute checkout
                # path containing e.g. "/fasm" (as .../fasm/tests/oracle/...
                # does) would otherwise match every directory and hash the
                # whole venv.
                rel = '/' + os.path.relpath(dirpath, oracle_dir).replace(
                    os.sep, '/')
                if sub.endswith('lib') and not any(p in rel
                                                   for p in packages):
                    continue
                for f in sorted(files):
                    if f.endswith('.pyc'):
                        continue
                    st = os.stat(os.path.join(dirpath, f))
                    h.update(('%s %d %d\n' % (os.path.join(dirpath, f),
                                              st.st_size,
                                              st.st_mtime_ns)).encode())

    for path in paths:
        if path is None:
            continue
        if os.path.isfile(path):
            h.update(file_sha256(path).encode())
        if oracle_dirs is None:
            walk(os.path.dirname(os.path.abspath(path)))
    for oracle_dir in oracle_dirs or ():
        walk(oracle_dir)
    return h.hexdigest()


def make_runner(args, commits, identity, prog='difftest-xilinx'):
    """The Runner of an all-parts run: the oracle results cached in
    <work-dir>/results unless --no-result-cache or a database commit is
    unknown; `identity` (oracle_identity) and the commits are the salt."""
    salt = json.dumps([identity, sorted(commits.items())])
    cache_dir = None
    unknown = sorted(db for db, commit in commits.items() if commit is None)
    if unknown and not args.no_result_cache:
        print('%s: not using the result cache: unknown database commit of '
              '%s' % (prog, ', '.join(unknown)))
    elif not args.no_result_cache:
        cache_dir = os.path.join(args.work_dir, 'results')
    return Runner(cache_dir, salt)


def tally(row, counts, ok, rules):
    """Counts a compared run in `counts` (runs, identical, explained,
    different) and its normalisation rules in the row."""
    counts[0] += 1
    explained = any(rules.get(r) for r in EXPLAINED_RULES)
    if not ok:
        counts[3] += 1
    elif explained:
        counts[2] += 1
    else:
        counts[1] += 1
    for k, v in rules.items():
        row['rules'][k] += v


def generator_options(args):
    return [
        '--tiles',
    ] + args.tiles + [
        '--seed',
        str(args.seed), '--density',
        str(args.density), '--max-per-tile',
        str(args.max_per_tile)
    ]


def generate(db, part, options, corpus_dir, commit):
    """Generates the corpus of a part unless the manifest of the same
    generator, options and database is there (never reused when the
    database commit is unknown); returns the manifest."""
    key = hashlib.sha256(
        json.dumps([file_sha256(GENERATOR), options, commit,
                    db]).encode()).hexdigest()
    manifest = os.path.join(corpus_dir, 'manifest.json')
    stamp = os.path.join(corpus_dir, 'key')
    try:
        with open(stamp) as f:
            if commit is not None and f.read().strip() == key:
                with open(manifest) as m:
                    return json.load(m)
    except (OSError, ValueError):
        pass
    argv = [
        sys.executable, GENERATOR, '--db-root', db, '--part', part,
        '--out-dir', corpus_dir
    ]
    result = subprocess.run(argv + options, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError('gen-xilinx-corpus.py failed for %s:\n%s' %
                           (part, result.stderr))
    with open(stamp, 'w') as f:
        f.write(key + '\n')
    with open(manifest) as m:
        return json.load(m)


def part_cases(corpus_dir, manifest, db, part):
    """[(case, options)] of a part: the first features file with every
    VARIANTS flag set (dense, sparse and pudc also through the bitstream
    tools), once more sparse without the Rust database cache
    (FASM_XDB_CACHE=0), the other features files and the error files
    sparse (the others through xc7frames2bit and the first two bitread
    flag sets)."""
    out = []
    name = manifest['files'][0]
    first = os.path.join(corpus_dir, name)
    for vname, flags in VARIANTS:
        case = ('%s[%s]' % (name, vname), first, db, part, flags)
        out.append((case, {'bitstream': vname in BITSTREAM_VARIANTS}))
    case = (name + '[sparse,FASM_XDB_CACHE=0]', first, db, part, ['--sparse'])
    out.append((case, {
        'bitstream': False,
        'rust_env': {
            'FASM_XDB_CACHE': '0'
        }
    }))
    for name in manifest['files'][1:]:
        case = (name + '[sparse]', os.path.join(corpus_dir, name), db, part,
                ['--sparse'])
        out.append((case, {
            'bitstream': True,
            'bitread_flags': BITREAD_FLAGS[:2]
        }))
    for name in manifest['errors']:
        case = ('errors/%s[sparse]' % name,
                os.path.join(corpus_dir, 'errors', name), db, part,
                ['--sparse'])
        out.append((case, {'bitstream': False}))
    return out


def run_part(family, db, part, args, tools, runner, commit):
    """Generates the corpus of one part and compares every run; returns
    the part's result row."""
    start = time.time()
    options = generator_options(args)
    slug = re.sub(r'[^\w.-]', '_',
                  '-'.join(args.tiles) + '-s' + str(args.seed))
    corpus_dir = os.path.join(args.work_dir, 'corpus', family, part, slug)
    row = {
        'family': family,
        'part': part,
        'fabric': '',
        'lines': 0,
        'files': 0,
        'fasm2frames': [0, 0, 0, 0],  # runs, identical, explained, different
        'bitstream': 0,
        'xcfasm': [0, 0, 0, 0],
        'failures': [],
        'rules': dict.fromkeys(RULES, 0),
    }
    try:
        manifest = generate(db, part, options, corpus_dir, commit)
    except RuntimeError as e:
        row['failures'].append(str(e))
        row['seconds'] = time.time() - start
        return row
    row['fabric'] = manifest['fabric']
    row['lines'] = manifest['lines']
    row['files'] = len(manifest['files']) + len(manifest['errors'])
    run_dir = os.path.join(args.work_dir, 'run', family, part)
    shutil.rmtree(run_dir, ignore_errors=True)
    os.makedirs(run_dir)
    xdb = None
    rust_env_base = {}
    if args.xdb_cache != '0':
        xdb = os.path.join(args.xdb_cache or run_dir, 'xdb')
        rust_env_base['FASM_XDB_CACHE'] = xdb

    for case, opts in part_cases(corpus_dir, manifest, db, part):
        rust_env = dict(rust_env_base)
        rust_env.update(opts.get('rust_env', {}))
        ok, message, rules, runs = compare(
            case,
            args.oracle,
            args.rust,
            run_dir,
            tools,
            runner,
            rust_env=rust_env,
            bitstream=opts.get('bitstream'),
            bitread_flags=opts.get('bitread_flags'))
        tally(row, row['fasm2frames'], ok, rules)
        row['bitstream'] += runs
        if not ok:
            row['failures'].append('%s %s' % (part, message))
    if tools and os.path.exists(os.path.join(db, part, 'part.yaml')):
        first = os.path.join(corpus_dir, manifest['files'][0])
        for vname, vflags in XCFASM_VARIANTS:
            case = ('%s[xcfasm-%s]' % (manifest['files'][0], vname), first, db,
                    part, vflags)
            ok, message, rules = compare_xcfasm(case, tools, run_dir, runner,
                                                rust_env_base)
            tally(row, row['xcfasm'], ok, rules)
            if not ok:
                row['failures'].append('%s %s' % (part, message))
    if not args.keep_run_dir:
        shutil.rmtree(run_dir, ignore_errors=True)
    row['seconds'] = time.time() - start
    return row


PRJXRAY_COLUMNS = [
    ('part', lambda r: r['part']),
    ('fabric', lambda r: r['fabric']),
    ('lines', lambda r: str(r['lines'])),
    ('files', lambda r: str(r['files'])),
    ('fasm2frames i/e/d', lambda r: '%d/%d/%d' % tuple(r['fasm2frames'][1:])),
    ('bit+bitread', lambda r: str(r['bitstream'])),
    ('xcfasm i/e/d', lambda r: '%d/%d/%d' % tuple(r['xcfasm'][1:])),
    ('seconds', lambda r: '%.0f' % r['seconds']),
]


def print_table(rows, columns=PRJXRAY_COLUMNS):
    """The per part table: (header, function of the row) columns."""
    header = tuple(c[0] for c in columns)
    table = [header]
    for r in rows:
        table.append(tuple(c[1](r) for c in columns))
    widths = [max(len(row[i]) for row in table) for i in range(len(header))]
    for i, row in enumerate(table):
        print('  '.join(c.ljust(w) for c, w in zip(row, widths)).rstrip())
        if i == 0:
            print('  '.join('-' * w for w in widths))


def sample_parts(db, parts, count):
    """`count` parts of a family, as many fabrics as possible: the first
    part of each fabric (in mapping/parts.yaml order), then the second,
    ...; within a fabric, parts of a new (device, package) first (a speed
    grade does not change the corpus)."""
    info = generator_module().read_simple_yaml(
        os.path.join(db, 'mapping', 'parts.yaml'))
    by_fabric = {}
    for part in parts:
        by_fabric.setdefault(fabric_of(db, part), []).append(part)
    for fabric, members in by_fabric.items():
        seen = set()
        first, rest = [], []
        for part in members:
            key = (info[part].get('device'), info[part].get('package'))
            (rest if key in seen else first).append(part)
            seen.add(key)
        by_fabric[fabric] = first + rest
    out = []
    depth = 0
    while len(out) < min(count, len(parts)):
        for fabric in sorted(by_fabric):
            if depth < len(by_fabric[fabric]) and len(out) < count:
                out.append(by_fabric[fabric][depth])
        depth += 1
    return [p for p in parts if p in out]


def fabric_of(db, part):
    """The fabric of a part (mapping/parts.yaml and devices.yaml), with the
    generator's reader."""
    return generator_module().fabric_of(db, part)


def generator_module():
    """tools/gen-xilinx-corpus.py as a module."""
    global _GENERATOR_MODULE
    if _GENERATOR_MODULE is None:
        spec = importlib.util.spec_from_file_location('gen_xilinx_corpus',
                                                      GENERATOR)
        _GENERATOR_MODULE = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(_GENERATOR_MODULE)
    return _GENERATOR_MODULE


_GENERATOR_MODULE = None


def format_seconds(seconds):
    seconds = int(seconds + 0.5)
    if seconds < 60:
        return '%d s' % seconds
    if seconds < 3600:
        return '%d min %02d s' % (seconds // 60, seconds % 60)
    return '%d h %02d min' % (seconds // 3600, seconds % 3600 // 60)


def families_main(args, tools):
    """--family/--families: every (or --parts GLOB) part of the families,
    generated corpus; per part table and totals."""
    families = []
    for item in args.families:
        families += [f for f in item.split(',') if f]
    db_dirs = args.db_cache.split(os.pathsep)
    selected = []
    for family in families:
        if family not in PRJXRAY_FAMILIES:
            print('difftest-xilinx: unknown family %s (known: %s)' %
                  (family, ', '.join(PRJXRAY_FAMILIES)),
                  file=sys.stderr)
            return EXIT_NOT_SET_UP
        db = find_family(db_dirs, family)
        # --list only reports what --db-cache already has; it never
        # fetches (same as --no-fetch).
        if db is None and not args.no_fetch and not args.list:
            db = fetch_family(db_dirs, family)
        if db is None:
            print('difftest-xilinx: database of %s not found in %s' %
                  (family, args.db_cache),
                  file=sys.stderr)
            return EXIT_NOT_SET_UP
        parts = subprocess.run(
            [sys.executable, GENERATOR, '--db-root', db, '--list-parts'],
            capture_output=True,
            text=True,
            check=True).stdout.split()
        if args.parts:
            globs = args.parts.split(',')
            parts = [
                p for p in parts if any(fnmatch.fnmatch(p, g) for g in globs)
            ]
            if not parts and len(families) == 1:
                print('difftest-xilinx: no part of %s matches %s' %
                      (family, args.parts),
                      file=sys.stderr)
                return EXIT_NOT_SET_UP
        elif args.parts_sample:
            parts = sample_parts(db, parts, args.parts_sample)
        elif not args.all_parts:
            parts = parts[:1]
        selected += [(family, db, p) for p in parts]
    if not selected:
        print('difftest-xilinx: no part selected (--parts %s)' % args.parts,
              file=sys.stderr)
        return EXIT_NOT_SET_UP
    if args.list:
        for family, db, part in selected:
            print('%s %s' % (family, part))
        return EXIT_OK
    commits = dict((db, db_commit(db)) for _, db, _ in selected)
    runner = make_runner(
        args, commits,
        oracle_identity([
            args.oracle, args.frames2bit_oracle, args.bitread_oracle,
            args.xcfasm_oracle
        ]))

    def work(family, db, part):
        return run_part(family, db, part, args, tools, runner, commits[db])

    def progress(row):
        f, x = row['fasm2frames'], row['xcfasm']
        return 'fasm2frames %d/%d/%d  xcfasm %d/%d/%d' % (f[1], f[2], f[3],
                                                          x[1], x[2], x[3])

    rows, start = run_selected(args, selected, work, 'difftest-xilinx',
                               families, SECONDS_PER_PART,
                               '46-271 s, mean 130 s', progress)
    print()
    print_table(rows)
    totals = [0] * 4
    xtotals = [0] * 4
    rules = dict.fromkeys(RULES, 0)
    for r in rows:
        for i in range(4):
            totals[i] += r['fasm2frames'][i]
            xtotals[i] += r['xcfasm'][i]
        for k, v in r['rules'].items():
            rules[k] += v
    wall = time.time() - start
    print()
    print('difftest-xilinx: %d parts, %d FASM files, %d lines; fasm2frames '
          '%d runs: %d identical, %d explained, %d different; '
          'xc7frames2bit + bitread %d runs; xcfasm %d runs: %d identical, '
          '%d explained, %d different' %
          (len(rows), sum(r['files'] for r in rows), sum(
              r['lines'] for r in rows), totals[0], totals[1], totals[2],
           totals[3], sum(r['bitstream'] for r in rows), xtotals[0],
           xtotals[1], xtotals[2], xtotals[3]))
    for rule, count in sorted(rules.items()):
        print('  normalisation rule %s: applied %d time(s)' % (rule, count))
    print_runner_summary('difftest-xilinx', runner, wall)
    if args.json_report:
        with open(args.json_report, 'w') as f:
            json.dump({'rows': rows, 'wall_seconds': wall}, f, indent=1)
    failed = any(r['failures'] for r in rows)
    return EXIT_DIFFERENCES if failed else EXIT_OK


def run_selected(args, selected, work, prog, families, seconds_per_part,
                 measured, progress):
    """Runs `work(family, db, part)` (a result row) for every selected
    part, --jobs in parallel, and prints a line (`progress(row)`, the
    ETA) after each; returns (the rows in the order of `selected`, the
    start time)."""
    start = time.time()
    print('%s: %d parts of %s, corpus --tiles %s --seed %s, '
          '%d jobs, work directory %s' %
          (prog, len(selected), ', '.join(families), ' '.join(
              args.tiles), args.seed, args.jobs, args.work_dir))
    estimate = len(selected) * seconds_per_part / max(
        1, min(args.jobs, len(selected)))
    print('%s: estimated wall time without cached results: '
          '%s (about %d s of work per part, measured for the default '
          'corpus: %s)' %
          (prog, format_seconds(estimate), seconds_per_part, measured),
          flush=True)
    rows = []
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        futures = [
            pool.submit(work, family, db, part)
            for family, db, part in selected
        ]
        for future in concurrent.futures.as_completed(futures):
            row = future.result()
            rows.append(row)
            done = len(rows)
            elapsed = time.time() - start
            eta = elapsed / done * (len(selected) - done)
            print('%-4s %-22s %6d lines %3d files  %s  %.0f s  [%d/%d, '
                  'ETA %s]' % ('FAIL' if row['failures'] else 'ok',
                               row['part'], row['lines'], row['files'],
                               progress(row), row['seconds'], done,
                               len(selected), format_seconds(eta)),
                  flush=True)
            for failure in row['failures']:
                print('  FAIL %s' % failure)
    order = dict(((f, p), i) for i, (f, _, p) in enumerate(selected))
    rows.sort(key=lambda r: order[(r['family'], r['part'])])
    return rows, start


def print_runner_summary(prog, runner, wall):
    print('%s: oracle runs %d (%d from the result cache), Rust '
          'runs %d, wall time %.0f s' %
          (prog, runner.counts['oracle'] + runner.counts['cached'],
           runner.counts['cached'], runner.counts['rust'], wall))


# ---------------------------------------------------------------------------
# prjuray mode (T6.2): `--prjuray`
# ---------------------------------------------------------------------------

URAY_ARCH = 'UltraScalePlus'
# The prjuray-db families fetched when none is found (tools/fetch-db.sh;
# the upstream prjuray-db has only this one, with two parts, see
# docs/rewrite/DESIGN-xilinx-db.md 8.12); every family directory found is
# tested.
URAY_FETCH_FAMILIES = ('zynqusp', )
# Mean seconds per part of the default corpus (every feature, the random
# designs; reference and Rust tools, generation), for the up front
# estimate (docs/rewrite/DESIGN-xilinx-db.md 8.12).
URAY_SECONDS_PER_PART = 400
URAY_MEASURED = '393-394 s for the two zynqusp parts'
# What identifies the reference prjuray tools (oracle_identity).
URAY_ORACLE_SUBDIRS = ('build/xilinx/bin', 'build/xilinx/src/prjuray/utils',
                       'venv-xilinx/lib')
URAY_ORACLE_PACKAGES = ('/prjuray', '/fasm')

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


def compare_uray(case,
                 tools,
                 tmpdir,
                 runner=PLAIN_RUNNER,
                 rust_env=None,
                 bitstream=None,
                 bitread_flags=None):
    """uray-fasm2frames oracle vs Rust, then the bitstream tools; returns
    (ok, message, rules, bitstream runs). `bitstream`: whether the
    oracle's .frm also goes through fasm2frames (32-bit words),
    xcframes2bit and uray-bitread (default: the variants of
    URAY_BITSTREAM_VARIANTS), with `bitread_flags` (default
    URAY_BITREAD_FLAGS)."""
    name, fasm, db, part, flags = case
    tag = re.sub(r'[^\w.-]', '_', name)
    args = ['--db-root', db, '--part', part] + flags + [fasm]
    if bitstream is None:
        variant = name.rsplit('[', 1)[-1].rstrip(']')
        bitstream = variant in URAY_BITSTREAM_VARIANTS
    o = run(runner,
            tools['oracle_fasm2frames'],
            args,
            os.path.join(tmpdir, tag + '.oracle.frm'),
            oracle=True,
            keep=bitstream)
    r = run(runner,
            tools['rust_fasm2frames'],
            args,
            os.path.join(tmpdir, tag + '.rust.frm'),
            env=rust_env)
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
            and is_rust_value_range_error(r[2])):
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
                        (o_err[:4000], r[2][:4000]))
    runs = 0
    if not problems and o[0] == 0 and bitstream:
        frm32 = to_32bit_frm(o[3])
        # The Rust fasm2frames (xc_fasm's command line) writes the 32-bit
        # frames directly for a prjuray-db part.
        path = os.path.join(tmpdir, tag + '.fasm2frames.frm')
        x = run(runner, tools['rust_xc_fasm2frames'], args, path, env=rust_env)
        runs += 1
        if x[0] != 0 or x[3] != frm32:
            problems.append('fasm2frames (32-bit words) differs from the '
                            'converted oracle .frm (exit code %d)' % x[0])
        frm = os.path.join(tmpdir, tag + '.frm32')
        with open(frm, 'wb') as f:
            f.write(frm32)
        more, n = compare_uray_bitstream(frm,
                                         os.path.join(db, part, 'part.yaml'),
                                         part, tools, tmpdir, tag, URAY_ARCH,
                                         runner, bitread_flags)
        problems += more
        runs += n
        os.remove(frm)
    return (not problems, '%s: %s' % (name, '; '.join(problems)), rules,
            runs)


def compare_uray_bitstream(frm,
                           part_file,
                           part,
                           tools,
                           tmpdir,
                           tag,
                           arch=URAY_ARCH,
                           runner=PLAIN_RUNNER,
                           bitread_flags=None):
    """xcframes2bit (oracle, Rust) on `frm`, then uray-bitread (oracle,
    Rust) on the reference .bit with `bitread_flags` (default
    URAY_BITREAD_FLAGS); returns (problems, runs)."""
    if bitread_flags is None:
        bitread_flags = URAY_BITREAD_FLAGS
    bit = os.path.join(tmpdir, tag + '.bit')
    base = [
        '--architecture=' + arch, '--frm_file=' + frm, '--output_file=' + bit,
        '--part_name=' + part, '--part_file=' + part_file
    ]
    o = runner.run([tools['oracle_xcframes2bit']] + base,
                   outputs=[bit],
                   oracle=True,
                   keep=[bit])
    data = o[3][tag + '.bit']
    epoch = bit_time(data)
    env = {'SOURCE_DATE_EPOCH': str(epoch)} if epoch is not None else {}
    r = runner.run([tools['rust_xcframes2bit']] + base, env=env, outputs=[bit])
    problems = compare_runs('xcframes2bit', o, r)
    if problems or o[0] != 0 or data is None:
        return problems, 1
    with open(bit, 'wb') as f:
        f.write(data)
    for i, flags in enumerate(bitread_flags):
        problems += run_bitread(tools, part_file, bit,
                                ['--architecture=' + arch] + flags, tmpdir,
                                '%s.%d' % (tag, i), runner, 'uray-bitread')
    os.remove(bit)
    return problems, 1 + len(bitread_flags)


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


def compare_uray_reference_bit(item, tools, tmpdir, runner=PLAIN_RUNNER):
    """Every uray-bitread flag set on a reference bitstream, then the
    round trip: Rust uray-bitread --frm_out, both xcframes2bit (and the
    bitread flag sets on the result)."""
    name, arch, part_file, bit = item
    tag = re.sub(r'[^\w.-]', '_', name)
    problems = []
    for i, flags in enumerate(URAY_BITREAD_FLAGS):
        problems += run_bitread(tools, part_file, bit,
                                ['--architecture=' + arch] + flags, tmpdir,
                                '%s.%d' % (tag, i), runner, 'uray-bitread')
    frm = os.path.join(tmpdir, tag + '.frm')
    code, _, err = run_tool([
        tools['rust_bitread'], '--part_file=' + part_file,
        '--architecture=' + arch, '--frm_out=' + frm, bit
    ])
    if code != 0:
        problems.append('uray-bitread --frm_out failed: %r' % err)
    else:
        more, _ = compare_uray_bitstream(frm, part_file, 'part', tools,
                                         tmpdir, tag + '.rt', arch, runner)
        problems += more
        os.remove(frm)
    return not problems, '%s: %s' % (name, '; '.join(problems))


def find_uray_families(db_dirs):
    """{family: directory} of the prjuray-db families (the directories
    with a tile_types/ subdirectory) in the database directories (the
    first directory with a family wins)."""
    out = {}
    for d in db_dirs:
        root = os.path.join(d, 'prjuray-db')
        if not os.path.isdir(root):
            continue
        for name in sorted(os.listdir(root)):
            path = os.path.join(root, name)
            if not name.startswith('.') and os.path.isdir(
                    os.path.join(path, 'tile_types')):
                out.setdefault(name, path)
    return out


def uray_part_cases(corpus_dir, manifest, random_dir, random_files, roi, db,
                    part):
    """[(case, options)] of a prjuray-db part: the first features file with
    every URAY_VARIANTS flag set and the ROI (dense, sparse and ROI also
    through fasm2frames, xcframes2bit and every URAY_BITREAD_FLAGS set),
    once more sparse without the Rust database cache (FASM_XDB_CACHE=0),
    the other features files sparse (through the bitstream tools with the
    first two uray-bitread flag sets), the error files sparse, and the
    random designs and error cases with every flag set (the first ten
    designs also with the ROI)."""
    out = []
    name = manifest['files'][0]
    first = os.path.join(corpus_dir, name)
    for vname, flags in URAY_VARIANTS + [('roi',
                                          ['--sparse', '--roi', roi])]:
        case = ('%s[%s]' % (name, vname), first, db, part, flags)
        out.append((case, {'bitstream': vname in URAY_BITSTREAM_VARIANTS}))
    case = (name + '[sparse,FASM_XDB_CACHE=0]', first, db, part, ['--sparse'])
    out.append((case, {
        'bitstream': False,
        'rust_env': {
            'FASM_XDB_CACHE': '0'
        }
    }))
    for name in manifest['files'][1:]:
        case = (name + '[sparse]', os.path.join(corpus_dir, name), db, part,
                ['--sparse'])
        out.append((case, {
            'bitstream': True,
            'bitread_flags': URAY_BITREAD_FLAGS[:2]
        }))
    for name in manifest['errors']:
        case = ('errors/%s[sparse]' % name,
                os.path.join(corpus_dir, 'errors', name), db, part,
                ['--sparse'])
        out.append((case, {'bitstream': False}))
    rel = os.path.basename(random_dir)
    for name in random_files:
        variants = list(URAY_VARIANTS)
        if name.startswith('design_0'):
            variants.append(('roi', ['--sparse', '--roi', roi]))
        for vname, flags in variants:
            case = ('%s/%s[%s]' % (rel, name, vname),
                    os.path.join(random_dir, name), db, part, flags)
            out.append((case, {}))
    return out


def uray_coverage(db, manifest):
    """Coverage of a generated prjuray-db corpus: the tile types of the
    part with a segbits file, those with a feature placed, and the
    features (units) placed, of all, unreachable and uncovered."""
    with_segbits = generator_module().family_tile_types(db)
    present = [t for t in manifest['tile_types'] if t in with_segbits]
    reached = set()
    for group, c in manifest['coverage'].items():
        if c['placed']:
            reached.add(group.split(' ', 1)[0])
    return {
        'tile_types_with_segbits': len(present),
        'tile_types_reached': len(reached & set(present)),
        'tile_types_not_reached': sorted(set(present) - reached),
        'features_total': manifest['features_total'],
        'features_placed': manifest['features_distinct_placed'],
        'features_placements': manifest['features_placed'],
        'uncovered': len(manifest['uncovered']),
        'unreachable': len(manifest['unreachable']),
    }


def run_uray_part(family, db, part, args, tools, runner, commit, seed):
    """Generates the corpora of one prjuray-db part (every feature, random
    designs) and compares every run; returns the part's result row."""
    start = time.time()
    options = generator_options(args)
    slug = re.sub(r'[^\w.-]', '_',
                  '-'.join(args.tiles) + '-s' + str(args.seed))
    corpus_dir = os.path.join(args.work_dir, 'corpus', family, part, slug)
    row = {
        'family': family,
        'part': part,
        'fabric': family,
        'lines': 0,
        'files': 0,
        # runs, identical, explained, different
        'uray_fasm2frames': [0, 0, 0, 0],
        'bitstream': 0,
        'coverage': {},
        'failures': [],
        'rules': dict.fromkeys(RULES, 0),
    }
    try:
        manifest = generate(db, part, options, corpus_dir, commit)
    except RuntimeError as e:
        row['failures'].append(str(e))
        row['seconds'] = time.time() - start
        return row
    # The random designs of T6.2 (--uray-files, --uray-seed) and the ROI.
    random_dir = os.path.join(args.work_dir, 'corpus', family, part,
                              'random-%d-s%d' % (args.uray_files, seed))
    os.makedirs(random_dir, exist_ok=True)
    corpus = UrayCorpus(db, part, seed)
    roi = os.path.join(random_dir, 'roi.json')
    with open(roi, 'w') as f:
        f.write(corpus.roi())
    random_files = []
    for name, text in corpus.files(args.uray_files):
        with open(os.path.join(random_dir, name), 'w') as f:
            f.write(text)
        random_files.append(name)
        row['lines'] += text.count('\n')
    row['lines'] += manifest['lines']
    row['files'] = len(manifest['files']) + len(manifest['errors'])
    row['files'] += len(random_files)
    row['coverage'] = uray_coverage(db, manifest)
    run_dir = os.path.join(args.work_dir, 'run', family, part)
    shutil.rmtree(run_dir, ignore_errors=True)
    os.makedirs(run_dir)
    rust_env_base = {}
    if args.xdb_cache != '0':
        rust_env_base['FASM_XDB_CACHE'] = os.path.join(
            args.xdb_cache or run_dir, 'xdb')
    for case, opts in uray_part_cases(corpus_dir, manifest, random_dir,
                                      random_files, roi, db, part):
        if args.filter and not fnmatch.fnmatch('%s/%s' % (part, case[0]),
                                               args.filter):
            continue
        rust_env = dict(rust_env_base)
        rust_env.update(opts.get('rust_env', {}))
        ok, message, rules, runs = compare_uray(
            case,
            tools,
            run_dir,
            runner,
            rust_env=rust_env,
            bitstream=opts.get('bitstream'),
            bitread_flags=opts.get('bitread_flags'))
        tally(row, row['uray_fasm2frames'], ok, rules)
        row['bitstream'] += runs
        if not ok:
            row['failures'].append('%s %s' % (part, message))
    if not args.keep_run_dir:
        shutil.rmtree(run_dir, ignore_errors=True)
    row['seconds'] = time.time() - start
    return row


def uray_progress(row):
    f, c = row['uray_fasm2frames'], row['coverage']
    return ('uray-fasm2frames %d/%d/%d  bitstream %d  tile types %d/%d  '
            'features %d/%d' %
            (f[1], f[2], f[3], row['bitstream'],
             c.get('tile_types_reached', 0),
             c.get('tile_types_with_segbits', 0), c.get('features_placed', 0),
             c.get('features_total', 0)))


URAY_COLUMNS = [
    ('part', lambda r: r['part']),
    ('family', lambda r: r['family']),
    ('lines', lambda r: str(r['lines'])),
    ('files', lambda r: str(r['files'])),
    ('uray-fasm2frames i/e/d',
     lambda r: '%d/%d/%d' % tuple(r['uray_fasm2frames'][1:])),
    ('f2f+bit+bitread', lambda r: str(r['bitstream'])),
    ('tile types', lambda r: '%d/%d' %
     (r['coverage'].get('tile_types_reached', 0), r['coverage'].get(
         'tile_types_with_segbits', 0))),
    ('features', lambda r: '%d/%d' % (r['coverage'].get(
        'features_placed', 0), r['coverage'].get('features_total', 0))),
    ('unreachable', lambda r: str(r['coverage'].get('unreachable', 0))),
    ('seconds', lambda r: '%.0f' % r['seconds']),
]


def main_prjuray(args):
    """The prjuray mode: every part of every prjuray-db family (or those
    of --families / --parts); returns the exit status."""
    prog = 'difftest-xilinx --prjuray'
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
    db_dirs = args.db_cache.split(os.pathsep)
    found = find_uray_families(db_dirs)
    wanted = []
    for item in args.families:
        wanted += [f for f in item.split(',') if f]
    # --list only reports what --db-cache already has; it never fetches
    # (same as --no-fetch).
    if not args.list:
        for family in wanted or ([] if found else URAY_FETCH_FAMILIES):
            if family not in found and not args.no_fetch:
                if fetch_family(db_dirs, family, 'prjuray') is not None:
                    found = find_uray_families(db_dirs)
    families = wanted or sorted(found)
    absent = [f for f in families if f not in found]
    if absent or not families:
        print('difftest-xilinx: prjuray-db %s not found in %s '
              '(tools/fetch-db.sh prjuray %s)' %
              (', '.join(absent) or 'families', args.db_cache, ' '.join(
                  absent or URAY_FETCH_FAMILIES)),
              file=sys.stderr)
        return EXIT_NOT_SET_UP
    selected = []
    seeds = {}
    for family in families:
        db = found[family]
        parts = generator_module().family_parts(db)
        for i, part in enumerate(parts):
            # The seed of the random designs: --uray-seed plus the index
            # of the part in its family, as in T6.2.
            seeds[(family, part)] = args.uray_seed + i
        if args.parts:
            globs = args.parts.split(',')
            parts = [
                p for p in parts if any(fnmatch.fnmatch(p, g) for g in globs)
            ]
        selected += [(family, db, p) for p in parts]
    if not selected:
        print('difftest-xilinx: no prjuray-db part selected (--parts %s)' %
              args.parts,
              file=sys.stderr)
        return EXIT_NOT_SET_UP
    if args.list:
        for family, db, part in selected:
            print('%s %s' % (family, part))
        return EXIT_OK
    commits = dict((db, db_commit(db)) for _, db, _ in selected)
    identity = oracle_identity(
        [tools[k] for k in sorted(tools) if k.startswith('oracle_')],
        oracle_dirs=[oracle_dir],
        subdirs=URAY_ORACLE_SUBDIRS,
        packages=URAY_ORACLE_PACKAGES)
    runner = make_runner(args, commits, identity)

    def work(family, db, part):
        return run_uray_part(family, db, part, args, tools, runner,
                             commits[db], seeds[(family, part)])

    rows, start = run_selected(args, selected, work, prog, families,
                               URAY_SECONDS_PER_PART, URAY_MEASURED,
                               uray_progress)
    print()
    print_table(rows, URAY_COLUMNS)
    references = []
    ref_failures = []
    if not args.filter:
        ref_dir = os.path.join(args.work_dir, 'run', 'uray-references')
        shutil.rmtree(ref_dir, ignore_errors=True)
        os.makedirs(ref_dir)
        references = uray_reference_bitstreams(oracle_dir, ref_dir)
        with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
            for (ok, message), item in zip(
                    pool.map(
                        lambda b: compare_uray_reference_bit(
                            b, tools, ref_dir, runner), references),
                    references):
                if ok:
                    if args.verbose:
                        print('ok   uray-bitread %s' % item[0])
                else:
                    ref_failures.append(message)
                    print('FAIL uray-bitread %s' % message)
        if not args.keep_run_dir:
            shutil.rmtree(ref_dir, ignore_errors=True)
    totals = [0] * 4
    rules = dict.fromkeys(RULES, 0)
    for r in rows:
        for i in range(4):
            totals[i] += r['uray_fasm2frames'][i]
        for k, v in r['rules'].items():
            rules[k] += v
    wall = time.time() - start
    print()
    print('%s: %d parts, %d FASM files, %d lines; uray-fasm2frames %d '
          'runs: %d identical, %d explained, %d different; fasm2frames + '
          'xcframes2bit + uray-bitread %d runs' %
          (prog, len(rows), sum(r['files'] for r in rows),
           sum(r['lines'] for r in rows), totals[0], totals[1], totals[2],
           totals[3], sum(r['bitstream'] for r in rows)))
    for rule, count in sorted(rules.items()):
        print('  normalisation rule %s: applied %d time(s)' % (rule, count))
    print('%s: uray-bitread on %d reference bitstreams x %d flag sets, and '
          'their bit -> frm -> bit round trip: %d identical, %d different' %
          (prog, len(references), len(URAY_BITREAD_FLAGS),
           len(references) - len(ref_failures), len(ref_failures)))
    print_runner_summary(prog, runner, wall)
    if args.json_report:
        with open(args.json_report, 'w') as f:
            json.dump(
                {
                    'rows': rows,
                    'wall_seconds': wall,
                    'references': {
                        'runs': len(references),
                        'failures': ref_failures
                    }
                },
                f,
                indent=1)
    failed = any(r['failures'] for r in rows) or ref_failures
    return EXIT_DIFFERENCES if failed else EXIT_OK


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
                        default=None,
                        help='directory of fetched databases, or several '
                        'separated by "%s" (searched in order; a family is '
                        'fetched into the first) (default: $FASM_DB_CACHE, '
                        'else <--uray-oracle-dir>/build/db if that exists, '
                        'else %s)' % (os.pathsep, DEFAULT_DB_CACHE))
    parser.add_argument('--filter', help='only FASM files matching GLOB')
    parser.add_argument('--prjuray',
                        action='store_true',
                        help='the prjuray mode (UltraScale+, every part of '
                        'every prjuray-db family, or of --families / '
                        '--parts): uray-fasm2frames, fasm2frames, '
                        'xcframes2bit and uray-bitread on the generated '
                        'corpora and on the ToolsTestData bitstreams')
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
    parser.add_argument('--uray-seed',
                        type=int,
                        default=1,
                        help='seed of the generated prjuray corpus '
                        '(default: %(default)s)')
    parser.add_argument('--corpus-root',
                        metavar='DIR',
                        help='compare the FASM files under DIR (laid out '
                        'like tests/corpus/xilinx: <family>/**/*.fasm, or '
                        'any layout with %s files) instead of the corpus '
                        '(e.g. the collected outputs of '
                        'tools/e2e/run-f4pga-examples.sh)' % DIFFTEST_JSON)
    parser.add_argument('--jobs', type=int, default=os.cpu_count() or 1)
    parser.add_argument('-v', '--verbose', action='store_true')
    group = parser.add_argument_group(
        'all parts of a family (generated corpus, tools/gen-xilinx-corpus.py)')
    group.add_argument('--family',
                       '--families',
                       dest='families',
                       action='append',
                       default=[],
                       metavar='F[,F...]',
                       help='prjxray-db families (%s); a family not found '
                       'in --db-cache is fetched with tools/fetch-db.sh '
                       '(with --prjuray: prjuray-db families, default all)' %
                       ', '.join(PRJXRAY_FAMILIES))
    group.add_argument('--all-parts',
                       action='store_true',
                       help='every part of mapping/parts.yaml (default: '
                       'the first)')
    group.add_argument('--parts',
                       metavar='GLOB[,GLOB...]',
                       help='the parts matching these globs')
    group.add_argument('--parts-sample',
                       type=int,
                       metavar='N',
                       help='N parts per family, covering as many fabrics '
                       'as possible (a quick run: --parts-sample 1 is 4 '
                       'parts, a few minutes with --jobs 4)')
    group.add_argument('--list',
                       action='store_true',
                       help='print the selected parts and exit')
    group.add_argument('--no-fetch',
                       action='store_true',
                       help='do not fetch missing families')
    group.add_argument('--tiles',
                       nargs='+',
                       default=['sample', '3'],
                       metavar='MODE',
                       help='generator --tiles: first | sample N | all '
                       '(default: sample 3)')
    group.add_argument('--seed', default='0', help='generator --seed')
    group.add_argument('--density',
                       default='0.5',
                       help='generator --density (default %(default)s)')
    group.add_argument('--max-per-tile',
                       default='0',
                       help='generator --max-per-tile (default: no bound)')
    group.add_argument('--work-dir',
                       default=os.environ.get('FASM_DIFFTEST_XILINX_WORK',
                                              DEFAULT_WORK_DIR),
                       help='generated corpora, oracle result cache and run '
                       'directories (default: $FASM_DIFFTEST_XILINX_WORK '
                       'or %(default)s)')
    group.add_argument('--no-result-cache',
                       action='store_true',
                       help='always run the reference tools')
    group.add_argument('--xdb-cache',
                       help='FASM_XDB_CACHE of the Rust tools (default: a '
                       'directory per part, removed with its run '
                       'directory; 0: no cache)')
    group.add_argument('--keep-run-dir',
                       action='store_true',
                       help='keep the run directories (outputs, xdb cache)')
    group.add_argument('--json-report', help='write the result rows here')
    args = parser.parse_args()

    # --db-cache default, resolved after parsing since it depends on
    # --uray-oracle-dir: $FASM_DB_CACHE, else <uray-oracle-dir>/build/db if
    # that exists, else tests/oracle/build/db of this checkout (same order
    # as test_uray_corpus.find_db / test_gen_xilinx_corpus.real_uray_db).
    if args.db_cache is None:
        env_cache = os.environ.get('FASM_DB_CACHE')
        uray_cache = os.path.join(args.uray_oracle_dir, 'build', 'db')
        if env_cache:
            args.db_cache = env_cache
        elif os.path.isdir(uray_cache):
            args.db_cache = uray_cache
        else:
            args.db_cache = DEFAULT_DB_CACHE

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
                  '(tests/oracle/setup-xilinx.sh)' %
                  tools['oracle_frames2bit'])
            tools = None
            args.frames2bit_oracle = args.bitread_oracle = None
            args.xcfasm_oracle = None

    if args.families:
        return families_main(args, tools)

    cases, notes = corpus(args.db_cache.split(os.pathsep)[0], args.filter,
                          args.corpus_root)
    for note in notes:
        print(note)
    totals = dict.fromkeys(RULES, 0)
    failures = []
    files = set(case[1] for case in cases)
    bitstream_runs = 0
    xcfasm_cases = []
    if tools:
        seen = {}
        for name, fasm, db, part, flags in cases:
            key = (fasm, db, part)
            if not os.path.exists(os.path.join(db, part, 'part.yaml')):
                continue
            if key not in seen:
                seen[key] = (name.rsplit('[', 1)[0], list(XCFASM_VARIANTS))
            if name.endswith('[roi]'):
                seen[key][1].append(('roi', flags))
        for (fasm, db, part), (rel, variants) in seen.items():
            for vname, vflags in variants:
                xcfasm_cases.append(('%s[xcfasm-%s]' % (rel, vname), fasm,
                                     db, part, vflags))
    xcfasm_failures = []
    bit_failures = []
    with tempfile.TemporaryDirectory(prefix='difftest-xilinx-') as tmpdir:
        references = []
        if tools and not args.filter and not args.corpus_root:
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
            for (ok, message, _), case in zip(
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

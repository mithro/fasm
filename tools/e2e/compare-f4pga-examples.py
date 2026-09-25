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
"""Compares the Rust tools with the f4pga flow's own outputs and tools
(T7.3), for the designs collected by tools/e2e/run-f4pga-examples.sh.

For every `<out>/<design>/<board>/` with a built `top.fasm`:

* `xcfasm`: the Rust `xcfasm` with the flow's command line
  (`--sparse --emit_pudc_b_pullup`, the flow's prjxray-db) must write the
  flow's frames (`top.frm`) byte for byte, and a bitstream identical to
  the flow's `top.bit` except for the header's design field (the path of
  the `.frm`, a temporary file of the flow's xcfasm) -- the header's date
  and time are those of `top.bit` (`SOURCE_DATE_EPOCH`);
* `fasm2frames`: the Rust `fasm2frames --sparse --emit_pudc_b_pullup`
  must write `top.frm`, with the flow's database and with the pinned
  database of tests/oracle (`--pinned-db-cache`), when that has the
  family;
* `xc7frames2bit`: the Rust tool on `top.frm` must write `top.bit` (same
  header rule);
* `bitread`: the Rust `bitread` and the flow's must print the same for
  `top.bit` with every flag set of tools/difftest-xilinx.py's
  `BITREAD_FLAGS`;
* `fasm`: the Rust `fasm` CLI and the flow's (the `fasm` PyPI package
  the flow installs) must print the same, with and without
  `--canonical`, and so must the oracle's (`tests/oracle/fasm-oracle`,
  when its venv is set up, tests/oracle/setup.sh);
* timings: the flow's `xcfasm`, `fasm2frames`, `xc7frames2bit`,
  `bitread` and `fasm` against the Rust tools (the Rust tools with a warm
  binary database cache, FASM_XDB_CACHE).

The dense and sparse variants of fasm2frames/xcfasm against the flow's
tools and against the oracle's are tools/difftest-xilinx.py's job
(`--corpus-root <out>`, see tools/e2e/README.md). Writes a JSON report
(`--json`) and prints a table; exit status 1 if anything differs.
"""
import argparse
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEFAULT_OUT = os.path.join(REPO_ROOT, 'tools', 'e2e', 'build', 'out',
                           'f4pga-examples')
# The environment's bin/fasm2frames does not run (tools/e2e/README.md).
FASM2FRAMES_FLOW = os.path.join(REPO_ROOT, 'tools', 'e2e', 'f4pga',
                                'fasm2frames-flow')
DEFAULT_ENV = os.path.join(
    os.environ.get('F4PGA_E2E_ROOT',
                   os.path.join(REPO_ROOT, 'tools', 'e2e', 'build',
                                'f4pga')), 'xc7', 'conda', 'envs', 'xc7')


def load_difftest():
    spec = importlib.util.spec_from_file_location(
        'difftest_xilinx', os.path.join(REPO_ROOT, 'tools',
                                        'difftest-xilinx.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


DX = load_difftest()


def bit_fields(data):
    """The header fields (a, b, c, d) and the configuration data of a
    .bit file."""
    pos = 2 + int.from_bytes(data[0:2], 'big')
    pos += 2
    fields = {}
    while pos < len(data):
        key = data[pos:pos + 1]
        pos += 1
        if key == b'e':
            size = int.from_bytes(data[pos:pos + 4], 'big')
            return fields, data[pos + 4:pos + 4 + size]
        size = int.from_bytes(data[pos:pos + 2], 'big')
        fields[key.decode()] = data[pos + 2:pos + 2 + size]
        pos += 2 + size
    raise ValueError('no configuration data')


def same_bit(a, b, any_time=False):
    """None if the .bit files `a` and `b` are identical but for the
    path in the design field ("<path>;Generator=...") and, with
    `any_time`, the date and time fields, else why not."""
    if a == b:
        return None
    try:
        fa, da = bit_fields(a)
        fb, db = bit_fields(b)
    except ValueError as e:
        return str(e)
    if da != db:
        first = next((i for i in range(min(len(da), len(db)))
                      if da[i] != db[i]), min(len(da), len(db)))
        return 'configuration data differs (%d vs %d bytes, first at %d)' % (
            len(da), len(db), first)
    for key in sorted(set(fa) | set(fb)):
        va, vb = fa.get(key), fb.get(key)
        if key == 'a' and va and vb:
            va = va.split(b';', 1)[-1]
            vb = vb.split(b';', 1)[-1]
        if any_time and key in ('c', 'd'):
            continue
        if va != vb:
            return 'header field %s differs: %r vs %r' % (key, fa.get(key),
                                                          fb.get(key))
    return None


def timed(argv, env=None, cwd=None):
    full_env = dict(os.environ)
    full_env.update(env or {})
    start = time.monotonic()
    p = subprocess.run(argv,
                       env=full_env,
                       cwd=cwd,
                       stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE)
    return p.returncode, p.stdout, p.stderr, time.monotonic() - start


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read(path):
    with open(path, 'rb') as f:
        return f.read()


class Design:
    def __init__(self, directory, info, args, tmp):
        self.dir = directory
        self.info = info
        self.args = args
        self.tmp = tmp
        self.part = info['part']
        self.family = info['family']
        self.fasm = os.path.join(directory, 'top.fasm')
        self.flow_db = os.path.join(args.env, 'share', 'symbiflow',
                                    'prjxray-db', self.family)
        self.pinned_db = os.path.join(args.pinned_db_cache, 'prjxray-db',
                                      self.family)
        self.flow_bin = os.path.join(args.env, 'bin')
        self.rust = args.rust_dir
        self.problems = []
        self.checks = {}
        self.times = {}

    def flow(self, tool):
        return os.path.join(self.flow_bin, tool)

    def rust_tool(self, tool):
        return os.path.join(self.rust, tool)

    def check(self, name, problem):
        self.checks[name] = problem is None
        if problem is not None:
            self.problems.append('%s: %s' % (name, problem))

    def flow_env(self):
        return {'PATH': self.flow_bin + os.pathsep + os.environ['PATH']}

    def run(self):
        frm = read(os.path.join(self.dir, 'top.frm'))
        bit = read(os.path.join(self.dir, 'top.bit'))
        epoch = DX.bit_time(bit)
        date_env = {'SOURCE_DATE_EPOCH': str(epoch)} if epoch else {}
        part_file = os.path.join(self.flow_db, self.part, 'part.yaml')
        t = os.path.join(self.tmp, 'x')

        # xcfasm with the flow's command line.
        def xcfasm(tool, env):
            args = [
                tool, '--db-root', self.flow_db, '--part', self.part,
                '--part_file', part_file, '--sparse',
                '--emit_pudc_b_pullup', '--fn_in', self.fasm, '--frm_out',
                t + '.frm', '--bit_out', t + '.bit', '--frm2bit',
                self.flow('xc7frames2bit')
            ]
            code, out, err, secs = timed(args, env=env)
            return code, err, secs, read(t + '.frm'), read(t + '.bit')

        env = dict(self.flow_env())
        env.update(date_env)
        xcfasm(self.rust_tool('xcfasm'), env)  # warm the database cache
        code, err, secs, r_frm, r_bit = xcfasm(self.rust_tool('xcfasm'), env)
        self.times['xcfasm rust'] = secs
        problem = None
        if code != 0:
            problem = 'exit code %d: %s' % (code, err[-2000:])
        elif r_frm != frm:
            problem = '.frm differs from the flow\'s top.frm'
        else:
            problem = same_bit(bit, r_bit)
        self.check('xcfasm = flow (frm, bit)', problem)
        code, err, secs, o_frm, o_bit = xcfasm(self.flow('xcfasm'),
                                               self.flow_env())
        self.times['xcfasm flow'] = secs
        if code != 0 or o_frm != frm or same_bit(bit, o_bit, True):
            self.problems.append('the flow\'s xcfasm rerun does not '
                                 'reproduce top.frm and top.bit (exit code '
                                 '%d)' % code)

        # fasm2frames, both databases.
        for name, db in (('flow db', self.flow_db), ('pinned db',
                                                     self.pinned_db)):
            if not os.path.isdir(db):
                self.checks['fasm2frames = flow (%s)' % name] = None
                continue
            args = [
                '--db-root', db, '--part', self.part, '--sparse',
                '--emit_pudc_b_pullup', self.fasm, t + '.f2f.frm'
            ]
            code, _, err, secs = timed(
                [self.rust_tool('fasm2frames')] + args)
            if name == 'flow db':
                self.times['fasm2frames rust'] = secs
            problem = None
            if code != 0:
                problem = 'exit code %d: %s' % (code, err[-2000:])
            elif read(t + '.f2f.frm') != frm:
                problem = '.frm differs from the flow\'s top.frm'
            self.check('fasm2frames = flow (%s)' % name, problem)
            if name == 'flow db':
                code, _, err, secs = timed([FASM2FRAMES_FLOW] + args)
                self.times['fasm2frames flow'] = secs
                if code != 0 or read(t + '.f2f.frm') != frm:
                    self.problems.append(
                        'the flow\'s fasm2frames does not reproduce top.frm '
                        '(exit code %d: %s)' % (code, err[-2000:]))

        # xc7frames2bit on the flow's frames.
        frm_path = os.path.join(self.dir, 'top.frm')
        base = [
            '--frm_file=' + frm_path, '--output_file=' + t + '.f2b.bit',
            '--part_name=' + self.part, '--part_file=' + part_file
        ]
        code, _, err, secs = timed([self.rust_tool('xc7frames2bit')] + base,
                                   env=date_env)
        self.times['xc7frames2bit rust'] = secs
        problem = ('exit code %d: %s' % (code, err[-2000:]) if code else
                   same_bit(bit, read(t + '.f2b.bit')))
        self.check('xc7frames2bit = flow', problem)
        _, _, _, secs = timed([self.flow('xc7frames2bit')] + base)
        self.times['xc7frames2bit flow'] = secs

        # bitread on the flow's bitstream.
        tools = {
            'oracle_bitread': self.flow('bitread'),
            'rust_bitread': self.rust_tool('bitread')
        }
        bit_path = os.path.join(self.dir, 'top.bit')
        problems = []
        for i, flags in enumerate(DX.BITREAD_FLAGS):
            problems += DX.run_bitread(tools, part_file, bit_path, flags,
                                       self.tmp, 'bitread.%d' % i)
        self.check('bitread = flow (%d flag sets)' % len(DX.BITREAD_FLAGS),
                   '; '.join(problems) if problems else None)
        argv = ['--part_file=' + part_file, '-z', '-y', '-o', t + '.bits',
                bit_path]
        self.times['bitread rust'] = timed(
            [self.rust_tool('bitread')] + argv)[3]
        self.times['bitread flow'] = timed([self.flow('bitread')] + argv)[3]

        # The fasm CLI.
        refs = [('flow', self.flow('fasm'))]
        oracle = self.args.fasm_oracle
        if oracle and os.access(oracle, os.X_OK) and os.path.exists(
                os.path.join(os.path.dirname(oracle), 'venv', 'bin',
                             'python')):
            refs.append(('oracle', oracle))
        for flags in ([], ['--canonical']):
            r = timed([self.rust_tool('fasm')] + flags + [self.fasm])
            if flags:
                self.times['fasm --canonical rust'] = r[3]
            for name, tool in refs:
                o = timed([tool] + flags + [self.fasm])
                if flags and name == 'flow':
                    self.times['fasm --canonical flow'] = o[3]
                problem = None
                if o[:3] != r[:3]:
                    problem = 'output differs (exit %d vs %d, %d vs %d ' \
                        'bytes)' % (o[0], r[0], len(o[1]), len(r[1]))
                self.check('fasm %s= %s' % (' '.join(flags) + ' ', name),
                           problem)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--out', default=DEFAULT_OUT)
    parser.add_argument('--env', default=DEFAULT_ENV,
                        help='the f4pga conda environment')
    parser.add_argument('--rust-dir',
                        default=os.path.join(REPO_ROOT, 'target', 'release'))
    parser.add_argument('--pinned-db-cache',
                        default=os.environ.get(
                            'FASM_DB_CACHE',
                            os.path.join(REPO_ROOT, 'tests', 'oracle',
                                         'build', 'db')))
    parser.add_argument('--fasm-oracle',
                        default=os.path.join(REPO_ROOT, 'tests', 'oracle',
                                             'fasm-oracle'))
    parser.add_argument('--json', help='write the results here')
    parser.add_argument('designs', nargs='*', help='DESIGN/BOARD (default: '
                        'all)')
    args = parser.parse_args()
    for tool in ('fasm', 'fasm2frames', 'xc7frames2bit', 'bitread',
                 'xcfasm'):
        if not os.access(os.path.join(args.rust_dir, tool), os.X_OK):
            print('compare-f4pga-examples: %s/%s not found (cargo build '
                  '--release)' % (args.rust_dir, tool),
                  file=sys.stderr)
            return 3
    if not os.access(os.path.join(args.env, 'bin', 'xcfasm'), os.X_OK):
        print('compare-f4pga-examples: %s not set up '
              '(tools/e2e/setup-f4pga.sh)' % args.env,
              file=sys.stderr)
        return 3
    if 'FASM_XDB_CACHE' not in os.environ:
        cache = tempfile.mkdtemp(prefix='fasm-xdb-cache-')
        os.environ['FASM_XDB_CACHE'] = cache
    else:
        cache = None
    results = []
    failed = False
    try:
        for design in sorted(os.listdir(args.out)):
            for board in sorted(os.listdir(os.path.join(args.out, design))):
                d = os.path.join(args.out, design, board)
                if args.designs and '%s/%s' % (design,
                                               board) not in args.designs:
                    continue
                info_path = os.path.join(d, 'info.json')
                if not os.path.exists(info_path):
                    continue
                with open(info_path) as f:
                    info = json.load(f)
                if info.get('status') != 'built':
                    results.append({'design': design, 'board': board,
                                    'status': info.get('status')})
                    continue
                with tempfile.TemporaryDirectory() as tmp:
                    x = Design(d, info, args, tmp)
                    x.run()
                results.append({
                    'design': design,
                    'board': board,
                    'status': 'built',
                    'part': x.part,
                    'fasm_lines': info['top.fasm']['lines'],
                    'checks': x.checks,
                    'problems': x.problems,
                    'times': {k: round(v, 3)
                              for k, v in sorted(x.times.items())},
                })
                ok = not x.problems
                failed |= not ok
                t = x.times
                print('%-4s %-22s %-12s %7d lines  xcfasm %6.2fs flow / '
                      '%5.2fs rust  fasm2frames %6.2fs / %5.2fs' %
                      ('ok' if ok else 'FAIL', design, board,
                       info['top.fasm']['lines'], t['xcfasm flow'],
                       t['xcfasm rust'], t['fasm2frames flow'],
                       t['fasm2frames rust']))
                for p in x.problems:
                    print('     %s' % p[:3000])
    finally:
        if cache:
            shutil.rmtree(cache, True)
    if args.json:
        with open(args.json, 'w') as f:
            json.dump(results, f, indent=2, sort_keys=True)
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())

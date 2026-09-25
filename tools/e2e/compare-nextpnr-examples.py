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
"""Compares the Rust tools with the openXC7 snap's tools and outputs
(T7.6), for the designs built by tools/e2e/run-nextpnr-examples.sh.

For every `<out>/<source>/<example>/<board>/` whose info.json says
`built` (top.fasm, and the flow's top.frm and top.bit, written by the
snap's fasm2frames and xc7frames2bit with the snap's bundled prjxray-db):

* `fasm2frames` with the snap db: the Rust tool writes the flow's
  top.frm byte for byte (dense, the flow's command line), and the same
  frames as the snap's `fasm2frames` with `--sparse` and with
  `--emit_pudc_b_pullup` (exit codes too);
* `fasm2frames` with the pinned db (tests/oracle, $FASM_DB_CACHE): the
  Rust tool and the oracle (`tests/oracle/fasm2frames-oracle`) write the
  same frames and exit codes (dense); whether those are the flow's frames
  is reported (`pinned db = snap db frames`) -- the two databases are
  different prjxray-db commits (tools/e2e/README.md), a difference there
  is explained by listing the frames and features involved;
* `xc7frames2bit`: the Rust tool on top.frm writes top.bit, byte for byte
  but for the `.frm` path in the header's design field (the time is
  injected with SOURCE_DATE_EPOCH);
* `xcfasm` (the snap has none): the Rust `xcfasm` with the snap db and
  the snap's xc7frames2bit writes top.frm and top.bit (same header rule);
* `bitread`: the Rust tool and the snap's print the same for top.bit with
  every flag set of tools/difftest-xilinx.py's BITREAD_FLAGS;
* `fasm`: the Rust CLI prints what the snap's `fasm` prints (which runs
  the textX parser: the snap's ANTLR extension does not load here) and
  what the oracle's prints (`tests/oracle/fasm-oracle`), with and without
  `--canonical`;
* timings: the snap's fasm2frames, xc7frames2bit, bitread and fasm
  against the Rust tools (the Rust tools with a warm FASM_XDB_CACHE).

Prints a table, writes the results as JSON (`--json`); exit status 1 if
anything differs that is not explained.
"""
import argparse
import importlib.util
import json
import os
import shutil
import sys
import tempfile

REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
E2E_BUILD = os.environ.get('OPENXC7_E2E_BUILD',
                           os.path.join(REPO_ROOT, 'tools', 'e2e', 'build'))
ORACLE_DIR = os.environ.get('ORACLE_DIR',
                            os.path.join(REPO_ROOT, 'tests', 'oracle'))
DEFAULT_OUT = os.environ.get(
    'NEXTPNR_EXAMPLES_OUT',
    os.path.join(REPO_ROOT, 'tools', 'e2e', 'build', 'out',
                 'nextpnr-examples'))


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


DX = load('difftest_xilinx', os.path.join(REPO_ROOT, 'tools',
                                          'difftest-xilinx.py'))
CF = load('compare_f4pga_examples',
          os.path.join(REPO_ROOT, 'tools', 'e2e',
                       'compare-f4pga-examples.py'))
timed = CF.timed
same_bit = CF.same_bit


# The feature the snap's fasm2frames --emit_pudc_b_pullup asks for.
PUDC_QUIRK = (b'LVCMOS12_LVCMOS15_LVCMOS18_LVCMOS25_LVCMOS33_LVTTL_SSTL135_'
              b'SSTL15.IN_ONLY not found')


def read(path):
    with open(path, 'rb') as f:
        return f.read()


def frames(data):
    """{address: line} of a .frm file."""
    out = {}
    for line in data.decode().splitlines():
        if line.strip():
            out[line.split()[0]] = line
    return out


def frame_diff(a, b):
    """The frame addresses whose lines differ between two .frm files."""
    fa, fb = frames(a), frames(b)
    return sorted(k for k in set(fa) | set(fb) if fa.get(k) != fb.get(k))


class Design:
    def __init__(self, directory, info, args, tmp):
        self.dir = directory
        self.info = info
        self.args = args
        self.tmp = tmp
        self.part = info['part']
        self.family = info['family']
        self.fasm = os.path.join(directory, 'top.fasm')
        self.snap_db = os.path.join(args.snap_db_cache, 'prjxray-db',
                                    self.family)
        self.pinned_db = os.path.join(args.pinned_db_cache, 'prjxray-db',
                                      self.family)
        self.problems = []
        self.notes = []
        self.explained = []
        self.checks = {}
        self.times = {}

    def snap(self, tool):
        return os.path.join(self.args.snap_bin, tool)

    def rust(self, tool):
        return os.path.join(self.args.rust_dir, tool)

    def check(self, name, problem):
        self.checks[name] = problem is None
        if problem is not None:
            self.problems.append('%s: %s' % (name, problem))

    def fasm2frames(self, tool, db, flags, out):
        argv = [tool, '--db-root', db, '--part', self.part
                ] + flags + [self.fasm, out]
        return timed(argv)

    def run(self):
        frm = read(os.path.join(self.dir, 'top.frm'))
        bit = read(os.path.join(self.dir, 'top.bit'))
        epoch = DX.bit_time(bit)
        date_env = {'SOURCE_DATE_EPOCH': str(epoch)} if epoch else {}
        part_file = os.path.join(self.snap_db, self.part, 'part.yaml')
        t = os.path.join(self.tmp, 'x')

        # fasm2frames, snap db: dense against the flow's frames, the
        # variants against the snap's tool.
        rust = self.rust('fasm2frames')
        self.fasm2frames(rust, self.snap_db, [], t + '.warm.frm')
        code, _, err, secs = self.fasm2frames(rust, self.snap_db, [],
                                              t + '.r.frm')
        self.times['fasm2frames rust'] = secs
        problem = None
        if code != 0:
            problem = 'exit code %d: %s' % (code, err[-2000:])
        elif read(t + '.r.frm') != frm:
            problem = 'differs from top.frm in frames %s' % frame_diff(
                read(t + '.r.frm'), frm)[:10]
        self.check('fasm2frames = flow (snap db, dense)', problem)
        code, _, err, secs = self.fasm2frames(self.snap('fasm2frames'),
                                              self.snap_db, [],
                                              t + '.s.frm')
        self.times['fasm2frames snap'] = secs
        if code != 0 or read(t + '.s.frm') != frm:
            self.problems.append('the snap\'s fasm2frames rerun does not '
                                 'reproduce top.frm (exit code %d)' % code)
        for name, flags in (('sparse', ['--sparse']),
                            ('pudc', ['--emit_pudc_b_pullup'])):
            rc = self.fasm2frames(rust, self.snap_db, flags, t + '.r.frm')
            refs = [('snap', self.snap('fasm2frames'))]
            if self.args.have_oracle:
                refs.append(('oracle', self.args.oracle))
            for ref, tool in refs:
                sc = self.fasm2frames(tool, self.snap_db, flags,
                                      t + '.s.frm')
                problem = None
                if (ref == 'snap' and name == 'pudc' and sc[0] == 1
                        and PUDC_QUIRK in sc[2] and rc[0] == 0):
                    # The snap's prjxray utils/fasm2frames.py names the
                    # PUDC_B IN_ONLY feature without LVDS_25/TMDS_33:
                    # neither database has it (COMPAT.md, "The openXC7
                    # snap's tools"). The oracle comparison below covers
                    # the variant.
                    self.checks['fasm2frames pudc = snap'] = None
                    self.explained.append(
                        'fasm2frames --emit_pudc_b_pullup: the snap\'s '
                        'fails (FasmLookupError, %s)' % PUDC_QUIRK.decode())
                    continue
                if rc[0] != sc[0]:
                    problem = 'exit codes %d (rust) / %d (%s): %s' % (
                        rc[0], sc[0], ref, (rc[2] + sc[2])[-2000:])
                elif read(t + '.r.frm') != read(t + '.s.frm'):
                    problem = 'frames %s differ' % frame_diff(
                        read(t + '.r.frm'), read(t + '.s.frm'))[:10]
                self.check('fasm2frames %s = %s (snap db)' % (name, ref),
                           problem)

        # fasm2frames, pinned db: Rust against the oracle, and against
        # the snap db's frames.
        if (self.args.have_oracle
                and os.path.isdir(os.path.join(self.pinned_db, self.part))):
            rc = self.fasm2frames(rust, self.pinned_db, [], t + '.rp.frm')
            oc = self.fasm2frames(self.args.oracle, self.pinned_db, [],
                                  t + '.op.frm')
            self.times['fasm2frames oracle (pinned db)'] = oc[3]
            problem = None
            if rc[0] != oc[0]:
                problem = 'exit codes %d (rust) / %d (oracle): %s / %s' % (
                    rc[0], oc[0], rc[2][-1000:], oc[2][-1000:])
            elif read(t + '.rp.frm') != read(t + '.op.frm'):
                problem = 'frames %s differ' % frame_diff(
                    read(t + '.rp.frm'), read(t + '.op.frm'))[:10]
            else:
                r_last = rc[2].strip().splitlines()[-1:]
                o_last = oc[2].strip().splitlines()[-1:]
                if rc[0] != 0 and r_last != o_last:
                    problem = 'error messages differ: %r / %r' % (r_last,
                                                                  o_last)
            self.check('fasm2frames = oracle (pinned db)', problem)
            same = rc[0] == 0 and read(t + '.rp.frm') == frm
            self.checks['pinned db = snap db frames'] = same
            if not same:
                if rc[0] != 0:
                    why = rc[2].decode(
                        errors='replace').strip().splitlines()[-1]
                    self.notes.append('pinned db: exit code %d, %s' %
                                      (rc[0], why))
                else:
                    self.notes.append(
                        'pinned db: frames %s differ from the snap db\'s' %
                        frame_diff(read(t + '.rp.frm'), frm)[:10])
        else:
            self.checks['fasm2frames = oracle (pinned db)'] = None

        # xc7frames2bit on the flow's frames.
        frm_path = os.path.join(self.dir, 'top.frm')
        base = [
            '--frm_file=' + frm_path, '--output_file=' + t + '.f2b.bit',
            '--part_name=' + self.part, '--part_file=' + part_file
        ]
        code, _, err, secs = timed([self.rust('xc7frames2bit')] + base,
                                   env=date_env)
        self.times['xc7frames2bit rust'] = secs
        problem = ('exit code %d: %s' % (code, err[-2000:]) if code else
                   same_bit(bit, read(t + '.f2b.bit')))
        self.check('xc7frames2bit = flow', problem)
        self.times['xc7frames2bit snap'] = timed([self.snap('xc7frames2bit')
                                                  ] + base)[3]

        # xcfasm: Rust, with the snap's xc7frames2bit.
        argv = [
            self.rust('xcfasm'), '--db-root', self.snap_db, '--part',
            self.part, '--part_file', part_file, '--fn_in', self.fasm,
            '--frm_out', t + '.xc.frm', '--bit_out', t + '.xc.bit',
            '--frm2bit',
            self.snap('xc7frames2bit')
        ]
        code, _, err, secs = timed(argv, env=date_env)
        self.times['xcfasm rust'] = secs
        problem = None
        if code != 0:
            problem = 'exit code %d: %s' % (code, err[-2000:])
        elif read(t + '.xc.frm') != frm:
            problem = '.frm differs from top.frm'
        else:
            problem = same_bit(bit, read(t + '.xc.bit'))
        self.check('xcfasm = flow (frm, bit)', problem)

        # bitread.
        tools = {
            'oracle_bitread': self.snap('bitread'),
            'rust_bitread': self.rust('bitread')
        }
        bit_path = os.path.join(self.dir, 'top.bit')
        problems = []
        for i, flags in enumerate(DX.BITREAD_FLAGS):
            problems += DX.run_bitread(tools, part_file, bit_path, flags,
                                       self.tmp, 'bitread.%d' % i)
        self.check('bitread = snap (%d flag sets)' % len(DX.BITREAD_FLAGS),
                   '; '.join(problems) if problems else None)
        argv = [
            '--part_file=' + part_file, '-z', '-y', '-o', t + '.bits',
            bit_path
        ]
        self.times['bitread rust'] = timed([self.rust('bitread')] + argv)[3]
        self.times['bitread snap'] = timed([self.snap('bitread')] + argv)[3]

        # The fasm CLI.
        refs = [('snap', self.snap('fasm'))]
        oracle = self.args.fasm_oracle
        if oracle and os.access(oracle, os.X_OK) and os.path.exists(
                os.path.join(os.path.dirname(oracle), 'venv', 'bin',
                             'python')):
            refs.append(('oracle', oracle))
        for flags in ([], ['--canonical']):
            r = timed([self.rust('fasm')] + flags + [self.fasm])
            if flags:
                self.times['fasm --canonical rust'] = r[3]
            for name, tool in refs:
                o = timed([tool] + flags + [self.fasm])
                if flags and name == 'snap':
                    self.times['fasm --canonical snap'] = o[3]
                problem = None
                if o[:2] != r[:2]:
                    problem = 'output differs (exit %d vs %d, %d vs %d ' \
                        'bytes)' % (o[0], r[0], len(o[1]), len(r[1]))
                self.check('fasm %s= %s' % (' '.join(flags) + ' ', name),
                           problem)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--out', default=DEFAULT_OUT)
    parser.add_argument('--snap-bin',
                        default=os.path.join(E2E_BUILD, 'openxc7', 'bin'),
                        help='the snap tools (tools/e2e/setup-openxc7.sh)')
    parser.add_argument('--snap-db-cache',
                        default=os.path.join(E2E_BUILD, 'openxc7', 'root',
                                             'opt', 'nextpnr-xilinx',
                                             'external'),
                        help='the directory with the snap\'s prjxray-db')
    parser.add_argument('--rust-dir',
                        default=os.path.join(REPO_ROOT, 'target', 'release'))
    parser.add_argument('--pinned-db-cache',
                        default=os.environ.get(
                            'FASM_DB_CACHE',
                            os.path.join(REPO_ROOT, 'tests', 'oracle',
                                         'build', 'db')))
    parser.add_argument('--oracle',
                        default=os.path.join(ORACLE_DIR,
                                             'fasm2frames-oracle'),
                        help='the oracle fasm2frames (default: in '
                        '$ORACLE_DIR, else tests/oracle)')
    parser.add_argument('--fasm-oracle',
                        default=os.path.join(ORACLE_DIR, 'fasm-oracle'))
    parser.add_argument('--json', help='write the results here')
    parser.add_argument('ids',
                        nargs='*',
                        help='SOURCE/EXAMPLE/BOARD (default: all)')
    args = parser.parse_args()
    for tool in ('fasm', 'fasm2frames', 'xc7frames2bit', 'bitread',
                 'xcfasm'):
        if not os.access(os.path.join(args.rust_dir, tool), os.X_OK):
            print('compare-nextpnr-examples: %s/%s not found (cargo build '
                  '--release)' % (args.rust_dir, tool),
                  file=sys.stderr)
            return 3
    if not os.access(os.path.join(args.snap_bin, 'fasm2frames'), os.X_OK):
        print('compare-nextpnr-examples: %s not set up '
              '(tools/e2e/setup-openxc7.sh)' % args.snap_bin,
              file=sys.stderr)
        return 3
    # The oracle (tests/oracle/setup-xilinx.sh): its wrappers run the
    # venv next to them.
    args.have_oracle = os.path.exists(
        os.path.join(os.path.dirname(args.oracle), 'venv-xilinx', 'bin',
                     'python'))
    if not args.have_oracle:
        print('compare-nextpnr-examples: %s is not set up '
              '(tests/oracle/setup-xilinx.sh, or ORACLE_DIR): the pinned db '
              'and oracle comparisons are skipped' % args.oracle,
              file=sys.stderr)
    if 'FASM_XDB_CACHE' not in os.environ:
        cache = tempfile.mkdtemp(prefix='fasm-xdb-cache-')
        os.environ['FASM_XDB_CACHE'] = cache
    else:
        cache = None
    results = []
    failed = False
    try:
        for dirpath, _, files in sorted(os.walk(args.out)):
            if 'info.json' not in files:
                continue
            with open(os.path.join(dirpath, 'info.json')) as f:
                info = json.load(f)
            ident = info['id']
            if args.ids and ident not in args.ids:
                continue
            if info.get('status') != 'built':
                results.append({'id': ident, 'status': info.get('status'),
                                'note': info.get('note')})
                continue
            with tempfile.TemporaryDirectory() as tmp:
                x = Design(dirpath, info, args, tmp)
                x.run()
            results.append({
                'id': ident,
                'status': 'built',
                'part': x.part,
                'fasm_lines': info['top.fasm']['lines'],
                'checks': x.checks,
                'problems': x.problems,
                'notes': x.notes,
                'explained': x.explained,
                'build_seconds': info.get('seconds', {}),
                'times': {k: round(v, 3)
                          for k, v in sorted(x.times.items())},
            })
            ok = not x.problems
            failed |= not ok
            t = x.times
            print('%-4s %-66s %7d lines  fasm2frames %6.2fs snap / %5.2fs '
                  'rust  pinned=snap: %s' %
                  ('ok' if ok else 'FAIL', ident, info['top.fasm']['lines'],
                   t['fasm2frames snap'], t['fasm2frames rust'],
                   x.checks.get('pinned db = snap db frames')))
            for p in x.problems:
                print('     %s' % p[:3000])
            for n in x.explained:
                print('     explained: %s' % n[:3000])
            for n in x.notes:
                print('     note: %s' % n[:3000])
    finally:
        if cache:
            shutil.rmtree(cache, True)
    if args.json:
        with open(args.json, 'w') as f:
            json.dump(results, f, indent=2, sort_keys=True)
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())

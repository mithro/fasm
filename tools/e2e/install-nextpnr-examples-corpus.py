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
"""Installs the designs built by tools/e2e/run-nextpnr-examples.sh (T7.6)
into the corpus:

  tests/corpus/xilinx/<family>/designs/<source>/<example>/<board>/
    top.fasm or top.fasm.xz   nextpnr-xilinx's FASM (xz -9 when over
                              COMPRESS_OVER bytes)
    top.frm.xz                the flow's frames (the snap's fasm2frames,
                              dense, the snap's prjxray-db)
    difftest.json             part and family (tools/difftest-xilinx.py)
    README.md                 provenance: source commit, example, part,
                              chipdb, tool versions, commands, build
                              times, sha256 of the FASM, frames and
                              bitstream, and the comparison results of
                              tools/e2e/compare-nextpnr-examples.py
                              (--compare-json)

Usage:
  tools/e2e/install-nextpnr-examples-corpus.py [--out DIR]
      [--compare-json FILE] [ID ...]
"""
import argparse
import hashlib
import json
import lzma
import os
import re
import sys

REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEFAULT_OUT = os.environ.get(
    'NEXTPNR_EXAMPLES_OUT',
    os.path.join(REPO_ROOT, 'tools', 'e2e', 'build', 'out',
                 'nextpnr-examples'))
CORPUS = os.path.join(REPO_ROOT, 'tests', 'corpus', 'xilinx')
COMPRESS_OVER = 256 * 1024

SNAP = """\
* Toolchain: openXC7 snap `0.8.2`
  (<https://github.com/openXC7/openXC7-snap/releases/download/0.8.2/openxc7_0.8.2_amd64.snap>,
  sha256 `6b2e07ce99ef33d3a4e41e2fd2eb916f26bb0ece34a97216ed840a0032e98587`,
  `tools/e2e/setup-openxc7.sh`): {nextpnr}, built from
  openXC7/nextpnr-xilinx tag `0.8.2`
  (`dea2f28c67fd1193ec72d0ba586800285e4c3648`); the snap's `fasm2frames`
  (prjxray `utils/fasm2frames.py`) and `xc7frames2bit`.
* Synthesis: {yosys} (OSS CAD Suite `2026-09-21`; the snap has no yosys).
* Database: the snap's bundled prjxray-db (`Info.md`: Project X-Ray
  `4c157493`), not the pinned `tools/fetch-db.sh` copy; see
  `tools/e2e/README.md`, "A note on prjxray-db provenance".
"""

SOURCES = {
    'nextpnr-xilinx':
    ('nextpnr-xilinx `xilinx/examples/{path}/{arg}.sh`',
     'https://github.com/openXC7/nextpnr-xilinx/tree/{commit}/xilinx/'
     'examples/{path}'),
    'openxc7-demo-projects':
    ('openXC7 demo-projects `{path}`',
     'https://github.com/openXC7/demo-projects/tree/{commit}/{path}'),
    'openxc7-primitive-tests':
    ('openXC7 primitive-tests `{path}`',
     'https://github.com/openXC7/primitive-tests/tree/{commit}/{path}'),
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def commands(lines):
    """The flow's commands (`make -n` output for the Makefile flows)
    without make's warnings and this machine's paths, the `$buf`
    workaround (run after synthesis) marked."""
    out = []
    for line in lines:
        if re.match(r'^\S*(Makefile|\.mk):\d+: warning:', line):
            continue
        line = re.sub(r'/\S*/chipdb/', '<chipdb dir>/', line)
        line = re.sub(r'/\S*/prjxray-db', '<snap prjxray-db>', line)
        line = re.sub(r'\s+$', '', line)
        line = line.replace('   # workaround, see run-nextpnr-examples.sh',
                            '   # after synthesis: the $buf workaround')
        out.append(line)
    return out


def readme(info, compare, fasm_name):
    src = info['source']
    source = info['id'].split('/')[0]
    path = src['path']
    if info['kind'] == 'regression':
        path = 'regression/' + path
    title, url = SOURCES[source]
    title = title.format(path=path, arg=src['arg'])
    url = url.format(commit=src['commit'], path=path)
    lines = [
        '# %s -- nextpnr-xilinx (openXC7) FASM (T7.6)' % info['id'], '',
        '`%s` is the FASM nextpnr-xilinx wrote for %s ([source](%s), commit '
        '`%s`), built by `tools/e2e/run-nextpnr-examples.sh %s` following '
        'the example\'s own %s (see `tools/e2e/README.md`, "nextpnr-xilinx '
        'examples corpus (T7.6)"). `top.frm.xz` is the flow\'s frames: the '
        'snap\'s `fasm2frames`, dense, with the snap\'s prjxray-db.' %
        (fasm_name, title, url, src['commit'], info['id'], {
            'nx-script': 'script',
            'make': 'Makefile',
            'regression': 'regression runner (`regression/run.sh`)'
        }[info['kind']]), '', '## Target', '',
        '* Part: `%s` (family `%s`)' % (info['part'], info['family']),
        '* Chip database: `%s.bin` (built from the snap\'s prjxray-db; %s)'
        % (info['chipdb_device'], 'the same package and database files, '
           'the speed grade does not change a chipdb' if
           info['chipdb_device'] != info['part'] else 'the design\'s part'),
        '* FASM: %d lines, %d bytes' %
        (info['top.fasm']['lines'], info['top.fasm']['bytes']),
        '* Flow times on this machine (4 cores, s): %s' % ', '.join(
            '%s %s' % (k, v) for k, v in info['seconds'].items()), ''
    ]
    if info.get('note'):
        lines += ['* Note: %s' % info['note'], '']
    lines += ['## Tools', '', SNAP.format(
        nextpnr=info['tools']['nextpnr-xilinx'],
        yosys=info['tools']['yosys']), '## Commands', '', '```']
    lines += commands(info.get('commands', []))
    lines += [
        '```', '', '## Reference outputs of the flow', '', '```',
        'sha256  top.fasm  %s' % info['top.fasm']['sha256'],
        'sha256  top.frm   %s  (%d bytes)' %
        (info['top.frm']['sha256'], info['top.frm']['bytes']),
        'sha256  top.bit   %s  (%d bytes)' %
        (info['top.bit']['sha256'], info['top.bit']['bytes']), '```', '',
        '`top.bit` is not committed: its header holds the build date and '
        'time and the `.frm` path.', ''
    ]
    if compare:
        lines += ['## Comparison (`tools/e2e/compare-nextpnr-examples.py`)',
                  '']
        for name, ok in sorted(compare['checks'].items()):
            lines.append('* %s: %s' % (name, {
                True: 'yes',
                False: 'no',
                None: 'not applicable'
            }[ok]))
        for n in compare.get('explained', []):
            lines.append('* explained: %s' % n)
        for n in compare.get('notes', []):
            lines.append('* note: %s' % n)
        for p in compare.get('problems', []):
            lines.append('* PROBLEM: %s' % p)
        lines.append('')
    return '\n'.join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--out', default=DEFAULT_OUT)
    parser.add_argument('--corpus', default=CORPUS)
    parser.add_argument('--compare-json')
    parser.add_argument('ids', nargs='*')
    args = parser.parse_args()
    compare = {}
    if args.compare_json:
        with open(args.compare_json) as f:
            compare = {r['id']: r for r in json.load(f)}
    installed = 0
    for dirpath, _, files in sorted(os.walk(args.out)):
        if 'info.json' not in files:
            continue
        with open(os.path.join(dirpath, 'info.json')) as f:
            info = json.load(f)
        if args.ids and info['id'] not in args.ids:
            continue
        if info['status'] != 'built':
            continue
        dest = os.path.join(args.corpus, info['family'], 'designs',
                            info['id'])
        os.makedirs(dest, exist_ok=True)
        for old in ('top.fasm', 'top.fasm.xz', 'top.frm.xz'):
            if os.path.exists(os.path.join(dest, old)):
                os.unlink(os.path.join(dest, old))
        with open(os.path.join(dirpath, 'top.fasm'), 'rb') as f:
            fasm = f.read()
        assert sha(fasm) == info['top.fasm']['sha256']
        if len(fasm) > COMPRESS_OVER:
            fasm_name = 'top.fasm.xz'
            data = lzma.compress(fasm, preset=9 | lzma.PRESET_EXTREME)
        else:
            fasm_name = 'top.fasm'
            data = fasm
        with open(os.path.join(dest, fasm_name), 'wb') as f:
            f.write(data)
        with open(os.path.join(dirpath, 'top.frm'), 'rb') as f:
            frm = f.read()
        assert sha(frm) == info['top.frm']['sha256']
        with open(os.path.join(dest, 'top.frm.xz'), 'wb') as f:
            f.write(lzma.compress(frm, preset=9 | lzma.PRESET_EXTREME))
        with open(os.path.join(dest, 'difftest.json'), 'w') as f:
            json.dump({'family': info['family'], 'part': info['part']}, f)
            f.write('\n')
        with open(os.path.join(dest, 'README.md'), 'w') as f:
            f.write(readme(info, compare.get(info['id']), fasm_name))
        installed += 1
        print('installed %s' % os.path.relpath(dest, REPO_ROOT))
    print('%d entries' % installed)
    return 0


if __name__ == '__main__':
    sys.exit(main())

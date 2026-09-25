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
"""Writes the info.json of one tools/e2e/run-vtr-genfasm.sh run (T7.4), or
prints the summary of a group's runs.

  write-info.py OUT ARCH CIRCUIT NETLIST CHAN_WIDTH STATUS VPR_S GENFASM_S
                NAMES [BOARD PART FAMILY IN_VTR_TASK]
  write-info.py --summary GROUP_DIR

For an Xilinx run (BOARD given) it also writes difftest.json (part and
family, for tools/difftest-xilinx.py --corpus-root) and the fields
tools/e2e/compare-f4pga-examples.py reads (design, board, part, family,
status, top.fasm lines).
"""
import hashlib
import json
import os
import sys

FILES = ('genfasm.fasm', 'genfasm-rr-metadata.fasm', 'top.fasm', 'top.frm',
         'top.bit')


def num(s):
    return None if s in ('-', '') else float(s)


def write(argv):
    out, arch, circuit, netlist, chan, status, vpr_s, genfasm_s, names = \
        argv[:9]
    info = {
        'arch': arch,
        'circuit': circuit,
        'netlist': netlist,
        'route_chan_width': int(chan),
        'status': status,
        'vpr_seconds': num(vpr_s),
        'genfasm_seconds': num(genfasm_s),
    }
    if names != '-':
        info['names'] = int(names)
    if len(argv) > 9:
        board, part, family, task = argv[9:13]
        info.update({
            'design': circuit,
            'board': board,
            'device': arch,
            'part': part,
            'family': family,
            'in_vtr_task': task == 'yes',
        })
        with open(os.path.join(out, 'difftest.json'), 'w') as f:
            json.dump({'part': part, 'family': family}, f, sort_keys=True)
    for name in FILES:
        path = os.path.join(out, name)
        if os.path.exists(path):
            with open(path, 'rb') as f:
                data = f.read()
            info[name] = {
                'bytes': len(data),
                'sha256': hashlib.sha256(data).hexdigest()
            }
            if name.endswith('.fasm'):
                info[name]['lines'] = data.count(b'\n')
    with open(os.path.join(out, 'info.json'), 'w') as f:
        json.dump(info, f, indent=2, sort_keys=True)
        f.write('\n')
    fasm = info.get('top.fasm') or info.get('genfasm.fasm')
    print('%s %s: %s%s' % (arch, circuit, status,
                           ' (%d lines, genfasm %.2f s)' %
                           (fasm['lines'], info['genfasm_seconds'])
                           if fasm else ''))


def summary(root):
    counts = {}
    for dirpath, _, files in sorted(os.walk(root)):
        if 'info.json' not in files:
            continue
        with open(os.path.join(dirpath, 'info.json')) as f:
            info = json.load(f)
        key = info['status'].split(':')[0]
        counts[key] = counts.get(key, 0) + 1
    print('%s: %s' % (root, ', '.join('%d %s' % (v, k)
                                      for k, v in sorted(counts.items()))))


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--summary':
        summary(sys.argv[2])
    elif len(sys.argv) in (10, 14):
        write(sys.argv[1:])
    else:
        sys.exit(__doc__.split('\n\n')[1])

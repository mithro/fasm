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
"""Copies the FASM that tools/e2e/run-vtr-genfasm.sh collected into the
corpus (T7.4):

  tests/corpus/vtr/test_fasm_arch/        (generic FASM, VTR's genfasm
                                           test architecture)
    genfasm.json.xz                        every circuit run: netlist,
                                           status, times, and the sha256,
                                           lines and corpus location of
                                           its FASM
    <circuit>/genfasm.fasm                 one file per circuit, except
    blif/<K>/genfasm-all.fasm[.xz]         the MCNC circuits of
                                           vtr_flow/benchmarks/blif/<K>/:
                                           one file per directory, each
                                           circuit's FASM unchanged after
                                           a `# circuit <name> sha256 <hex>
                                           lines <n>` line
    fasm-test/wire/genfasm-rr-metadata.fasm  test_fasm.cpp's variant (not
                                           valid FASM), with its
                                           expected-errors.json
  tests/corpus/xilinx/<family>/designs/vtr/<circuit>/<board>/
    genfasm.fasm or genfasm.fasm.xz        genfasm's FASM (xz -9e when
                                           over COMPRESS_OVER bytes)
    genfasm.frm.xz                         the reference frames, when
                                           their xz is under FRM_LIMIT
    difftest.json                          part and family
    README.md                              provenance and sha256s

  install-vtr-genfasm-corpus.py [--out DIR] [--xilinx CIRCUIT/BOARD ...]

`--xilinx` limits the Xilinx designs installed (default: every built
one); the generic group is always installed completely.
"""
import argparse
import hashlib
import json
import lzma
import os
import re
import shutil
import sys

REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEFAULT_OUT = os.path.join(REPO_ROOT, 'tools', 'e2e', 'build', 'out',
                           'vtr-genfasm')
GENERIC = os.path.join(REPO_ROOT, 'tests', 'corpus', 'vtr', 'test_fasm_arch')
XILINX = os.path.join(REPO_ROOT, 'tests', 'corpus', 'xilinx')
COMPRESS_OVER = 256 * 1024
FRM_LIMIT = 64 * 1024
VTR_COMMIT = '25e723a24aa0ae7a0061cd89dd84b1fb62afcc09'
TOOLCHAIN = 'vtr-optimized 8.0.0_5699_g25e723a24 (conda, f4pga toolchain)'
VERSION = '8.1.0-dev+25e723a24-dirty (revision 8.0.0-5699-g25e723a24-dirty)'
AGGREGATED = re.compile(r'^(blif/[^/]+)/[^/]+$')
BOARDS = {
    'arty_35': 'Digilent Arty A7-35T',
    'arty_100': 'Digilent Arty A7-100T',
    'basys3': 'Digilent Basys 3',
}
XC7_OPTIONS = ('--max_router_iterations 500 --routing_failure_predictor off '
               '--router_high_fanout_threshold 1000 --constant_net_method '
               'route --route_chan_width 500 --router_heap bucket '
               '--clock_modeling route '
               '--place_delta_delay_matrix_calculation_method dijkstra '
               '--place_delay_model delta_override --router_lookahead '
               'extended_map --check_route quick --strict_checks off '
               '--allow_dangling_combinational_nodes on --disable_errors '
               'check_unbuffered_edges:check_route '
               '--congested_routing_iteration_threshold 0.8 '
               '--incremental_reroute_delay_ripup off --base_cost_type '
               'delay_normalized_length_bounded --bb_factor 10 '
               '--initial_pres_fac 4.0 --check_rr_graph off')


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def read(path):
    with open(path, 'rb') as f:
        return f.read()


def write(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    if path.endswith('.xz'):
        data = lzma.compress(data, preset=9 | lzma.PRESET_EXTREME)
    with open(path, 'wb') as f:
        f.write(data)


def infos(root):
    for dirpath, _, files in sorted(os.walk(root)):
        if 'info.json' in files:
            with open(os.path.join(dirpath, 'info.json')) as f:
                yield dirpath, json.load(f)


def install_generic(out):
    src_root = os.path.join(out, 'test_fasm_arch')
    if not os.path.isdir(src_root):
        print('no %s: generic group not installed' % src_root)
        return
    if os.path.isdir(GENERIC):
        shutil.rmtree(GENERIC)
    circuits = {}
    groups = {}
    for d, info in infos(src_root):
        circuit = info['circuit']
        entry = {
            'netlist': info['netlist'],
            'status': info['status'],
            'vpr_seconds': info['vpr_seconds'],
            'genfasm_seconds': info['genfasm_seconds'],
        }
        if 'names' in info:
            entry['names'] = info['names']
        entry = {k: v for k, v in entry.items() if v is not None}
        for name in ('genfasm.fasm', 'genfasm-rr-metadata.fasm'):
            if name not in info:
                continue
            data = read(os.path.join(d, name))
            f = {
                'sha256': sha256(data),
                'lines': data.count(b'\n'),
                'bytes': len(data)
            }
            m = AGGREGATED.match(circuit)
            if m and name == 'genfasm.fasm':
                groups.setdefault(m.group(1), []).append((circuit, data))
                f['stored'] = '%s/genfasm-all.fasm' % m.group(1)
            else:
                f['stored'] = '%s/%s' % (circuit, name)
                write(os.path.join(GENERIC, circuit, name), data)
            entry[name] = f
        circuits[circuit] = entry
    for group, members in sorted(groups.items()):
        text = [
            b'# VTR genfasm output (T7.4) of every circuit of',
            b'# vtr_flow/benchmarks/%s/ that fits utils/fasm/test/'
            b'test_fasm_arch.xml,' % group.encode(),
            b'# see tests/corpus/vtr/README.md: each circuit\'s FASM follows'
            b' its',
            b'# "# circuit" line unchanged (sha256 and lines of that FASM).',
        ]
        body = b'\n'.join(text) + b'\n'
        for circuit, data in sorted(members):
            body += b'# circuit %s sha256 %s lines %d\n' % (
                circuit.encode(), sha256(data).encode(), data.count(b'\n'))
            body += data
        name = 'genfasm-all.fasm'
        if len(body) > COMPRESS_OVER:
            name += '.xz'
        for circuit, _ in members:
            stored = '%s/%s' % (group, name)
            circuits[circuit]['genfasm.fasm']['stored'] = stored
        write(os.path.join(GENERIC, group, name), body)
    wire = circuits.get('fasm-test/wire', {})
    if 'genfasm-rr-metadata.fasm' in wire:
        data = read(
            os.path.join(src_root, 'fasm-test', 'wire',
                         'genfasm-rr-metadata.fasm'))
        line = next(i + 1 for i, text in enumerate(data.split(b'\n'))
                    if text[:1].isdigit())
        with open(
                os.path.join(GENERIC, 'fasm-test', 'wire',
                             'expected-errors.json'), 'w') as f:
            json.dump(
                {
                    'genfasm-rr-metadata.fasm': {
                        'line':
                        line,
                        'source':
                        'VTR genfasm with test_fasm.cpp\'s rr edge metadata '
                        '(<src>_<sink>_<switch> features start with a digit, '
                        'not a FASM identifier), T7.4'
                    }
                },
                f,
                indent=2,
                sort_keys=True)
            f.write('\n')
    # One circuit per line (1500 circuits).
    head = {
        'vtr_commit': VTR_COMMIT,
        'genfasm': TOOLCHAIN,
        'genfasm_version': VERSION,
        'arch': 'utils/fasm/test/test_fasm_arch.xml',
        'route_chan_width': 100,
    }
    lines = ['{']
    for key, value in sorted(head.items()):
        lines.append(' %s: %s,' % (json.dumps(key), json.dumps(value)))
    lines.append(' "circuits": {')
    for i, (circuit, entry) in enumerate(sorted(circuits.items())):
        lines.append('  %s: %s%s' %
                     (json.dumps(circuit), json.dumps(entry, sort_keys=True),
                      ',' if i + 1 < len(circuits) else ''))
    lines.append(' }')
    lines.append('}')
    write(os.path.join(GENERIC, 'genfasm.json.xz'),
          ('\n'.join(lines) + '\n').encode())
    built = sum(1 for c in circuits.values() if c['status'] == 'built')
    print('test_fasm_arch: %d circuits, %d built -> %s' %
          (len(circuits), built, os.path.relpath(GENERIC, REPO_ROOT)))


def install_xilinx(src, info):
    circuit, board = info['circuit'], info['board']
    dst = os.path.join(XILINX, info['family'], 'designs', 'vtr', circuit,
                       board)
    if os.path.isdir(dst):
        shutil.rmtree(dst)
    os.makedirs(dst)
    fasm = read(os.path.join(src, 'top.fasm'))
    frm = read(os.path.join(src, 'top.frm'))
    bit = read(os.path.join(src, 'top.bit'))
    fasm_name = 'genfasm.fasm' + ('.xz' if len(fasm) > COMPRESS_OVER else '')
    write(os.path.join(dst, fasm_name), fasm)
    frm_xz = lzma.compress(frm, preset=9 | lzma.PRESET_EXTREME)
    if len(frm_xz) <= FRM_LIMIT:
        with open(os.path.join(dst, 'genfasm.frm.xz'), 'wb') as f:
            f.write(frm_xz)
        frm_note = ('`genfasm.frm.xz` is the reference frames (`xz -9e`), '
                    'compared byte for byte with the Rust `fasm2frames '
                    '--sparse --emit_pudc_b_pullup` by '
                    '`tests/e2e/test_vtr_genfasm.py`.')
    else:
        frm_note = ('The reference frames are not committed (%d bytes after '
                    'xz, over the %d byte limit of this corpus); their '
                    'sha256 is below.' % (len(frm_xz), FRM_LIMIT))
    with open(os.path.join(dst, 'difftest.json'), 'w') as f:
        json.dump({'part': info['part'], 'family': info['family']}, f,
                  sort_keys=True)
        f.write('\n')
    readme = """# {circuit} / {board} -- VTR genfasm FASM (T7.4)

`{fasm_name}` is the FASM VTR's `genfasm` writes for the `{circuit}`
benchmark of VTR's nightly `symbiflow` regression task{task} on the f4pga
toolchain's `{device}` architecture, by `tools/e2e/run-vtr-genfasm.sh
xc7a50t_test {circuit}` (see `tools/e2e/README.md`, "VTR genfasm
designs (T7.4)"): VPR packs, places and routes the benchmark's eblif
with the task's options, then genfasm writes the FASM. {frm_note}

## Target

* Board: {board_name} (`{board}`), part `{part}` (family `{family}`),
  VPR device `{device}`
* Netlist, SDC and placement constraints:
  `benchmarks/circuits/{circuit}.eblif`, `benchmarks/sdc/{circuit}.sdc`,
  `benchmarks/place_constr/{circuit}.place`
  of the symbiflow-arch-defs benchmark tarball `fb1b251a` (sha256
  `2f5fed77c069e7e787f909e75f8aaf2db6ec1ea669a17a4f13d196c55931cc3d`,
  what VTR's `vtr_flow/scripts/download_symbiflow.py` downloads)
* VPR: `vpr arch.timing.xml {circuit}.eblif --read_rr_graph
  rr_graph_{device}.rr_graph.real.bin <options> --read_router_lookahead
  rr_graph_{device}.lookahead.bin --read_placement_delay_lookup
  rr_graph_{device}.place_delay.bin --sdc_file {circuit}.sdc
  --fix_clusters {circuit}.place`, {vpr_s:.0f} s
* genfasm: `genfasm arch.timing.xml {circuit}.eblif --read_rr_graph
  rr_graph_{device}.rr_graph.real.bin <options>`, {genfasm_s:.1f} s
* `<options>` (the task's `script_params`): `{options}`
* FASM: {lines} lines, {size} bytes

## Tools

* VPR and genfasm: {toolchain}, `vpr --version` {version}
  (VTR [`25e723a2`]({vtr_url})).
* Architecture: symbiflow-arch-defs `20220920-124259`/`007d1c1` (conda
  package `{device}` of `tools/e2e/setup-f4pga.sh`); the benchmarks were
  made for symbiflow-arch-defs `fb1b251a`, and VPR reads them with this
  one.
* Reference frames and bitstream: the f4pga flow's `xcfasm --sparse
  --emit_pudc_b_pullup` (f4pga-xc-fasm `25dc605c`, prjxray-tools
  `0.1_3015_gae546d6b`) with the flow's prjxray-db `0a0added`, identical
  to the pinned `tools/fetch-db.sh` copy.

## Reference outputs

```
sha256  top.fasm  {fasm_sha}
sha256  top.frm   {frm_sha}  ({frm_size} bytes)
sha256  top.bit   {bit_sha}  ({bit_size} bytes)
```

`top.bit` holds the build date and time and the `.frm` path in its
header; see `docs/rewrite/DESIGN-xilinx-db.md` §8.14.
""".format(circuit=circuit,
           board=board,
           board_name=BOARDS.get(board, board),
           task=(' (listed in its `config.txt`)' if info['in_vtr_task'] else
                 ' (in the task\'s benchmark tarball, not in its '
                 '`config.txt` list)'),
           fasm_name=fasm_name,
           frm_note=frm_note,
           part=info['part'],
           family=info['family'],
           device=info['device'],
           vpr_s=info['vpr_seconds'],
           genfasm_s=info['genfasm_seconds'],
           options=XC7_OPTIONS,
           lines=fasm.count(b'\n'),
           size=len(fasm),
           toolchain=TOOLCHAIN,
           version=VERSION,
           vtr_url='https://github.com/verilog-to-routing/'
           'vtr-verilog-to-routing/commit/' + VTR_COMMIT,
           fasm_sha=sha256(fasm),
           frm_sha=sha256(frm),
           frm_size=len(frm),
           bit_sha=sha256(bit),
           bit_size=len(bit))
    with open(os.path.join(dst, 'README.md'), 'w') as f:
        f.write(readme)
    return dst


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--out', default=DEFAULT_OUT)
    parser.add_argument('--xilinx', nargs='*', default=None,
                        help='CIRCUIT/BOARD to install (default: all)')
    args = parser.parse_args()
    install_generic(args.out)
    root = os.path.join(args.out, 'xc7a50t_test')
    if not os.path.isdir(root):
        return 0
    for d, info in infos(root):
        key = '%s/%s' % (info['circuit'], info['board'])
        if args.xilinx is not None and key not in args.xilinx:
            continue
        if info['status'] != 'built':
            print('skipping %s: %s' % (key, info['status']))
            continue
        dst = install_xilinx(d, info)
        print('%s -> %s' % (key, os.path.relpath(dst, REPO_ROOT)))
    return 0


if __name__ == '__main__':
    sys.exit(main())

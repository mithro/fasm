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
"""Copies the f4pga-examples designs built by
tools/e2e/run-f4pga-examples.sh into the corpus (T7.3):

  tests/corpus/xilinx/<family>/designs/f4pga-examples/<design>/<board>/
    vpr.fasm or vpr.fasm.xz   the flow's FASM (VPR genfasm + extra FASM;
                              xz -9 when over COMPRESS_OVER bytes)
    vpr.frm.xz                the flow's frames (xcfasm --sparse
                              --emit_pudc_b_pullup), when their xz is
                              under FRM_LIMIT bytes
    difftest.json             part and family (tools/difftest-xilinx.py)
    README.md                 provenance: commits, tool versions, board,
                              part, build command and time, sha256 of the
                              flow's FASM, frames and bitstream

(`vpr.*` rather than `top.*`: counter_test/arty_35 also holds the
openXC7 flow's `top.fasm` of T7.1.)
"""
import argparse
import hashlib
import json
import lzma
import os
import sys

REPO_ROOT = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEFAULT_OUT = os.path.join(REPO_ROOT, 'tools', 'e2e', 'build', 'out',
                           'f4pga-examples')
CORPUS = os.path.join(REPO_ROOT, 'tests', 'corpus', 'xilinx')
COMPRESS_OVER = 256 * 1024
FRM_LIMIT = 64 * 1024

F4PGA_EXAMPLES_COMMIT = '13f11197b33dae1cde3bf146f317d63f0134eacf'
TOOLS = """\
* f4pga-examples
  [`13f11197`](https://github.com/chipsalliance/f4pga-examples/commit/13f11197b33dae1cde3bf146f317d63f0134eacf)
  (submodules at the commits it records).
* Toolchain: `tools/e2e/setup-f4pga.sh` (conda environment `xc7`,
  `tools/e2e/f4pga/xc7-conda-explicit.txt` and `xc7-pip-freeze.txt`):
  f4pga python package `e1cd038f`, yosys `0.27_29_g0f5e7c244`,
  symbiflow-yosys-plugins `1.0.0_7_1260_ge7070ca`, vtr-optimized (VPR,
  genfasm) `8.0.0_5699_g25e723a24`, prjxray-tools (xc7frames2bit,
  bitread) `0.1_3015_gae546d6b`, prjxray `ae546d6b` (python), f4pga-xc-fasm
  (xcfasm) `25dc605c`, fasm `0.0.2.post88`, symbiflow-arch-defs
  `20220920-124259`/`007d1c1` (package `{device}`, sha256 `{device_sha}`).
* Database: the flow's prjxray-db is the conda package
  `prjxray-db 0.0_257_g0a0adde`, prjxray-db commit `0a0added`: every
  database file is identical to the pinned `tools/fetch-db.sh` copy
  (`0a0addedd73e7e4139d52a6d8db4258763e0f1f3`), so the reference outputs
  below are also those of the pinned database.
"""
ARCH_SHA256 = {
    'xc7a50t_test':
    '7dafd8b08503afe8baa782218c5a703a8afc5b8c2601a3062307178a620d834d',
    'xc7a100t_test':
    '7a64daaa04b8f0761a86d3c74d2bc1bcacc27a6680edc8e7672bc2e804a73bcf',
    'xc7a200t_test':
    '1fc5bca8811923d91f94680e414969d94c92787f814ed919fa0d57de66cd18bf',
    'xc7z010_test':
    '784976422428ab8f26c0e63414692cc7a4bdd29f0161141147ae78bd97ed666d',
}
BOARDS = {
    'arty_35': 'Digilent Arty A7-35T',
    'arty_100': 'Digilent Arty A7-100T',
    'basys3': 'Digilent Basys 3',
    'nexys4ddr': 'Digilent Nexys 4 DDR',
    'nexys_video': 'Digilent Nexys Video',
    'zybo': 'Digilent Zybo Z7-10',
}


def command(design, board):
    """The documented build command (f4pga-examples, in xc7/)."""
    if design.startswith('litex_demo_'):
        cpu = design[len('litex_demo_'):]
        variant = 'a7-100' if board == 'arty_100' else 'a7-35'
        return ('cd litex_demo && ./src/litex/litex/boards/targets/arty.py '
                '--toolchain=symbiflow --cpu-type=%s --sys-clk-freq 80e6 '
                '--output-dir build/%s/%s --variant %s --build' %
                (cpu, cpu, board, variant))
    if design.startswith('hello_'):
        return ('TARGET="%s" make -C ../projf-makefiles/hello/hello-arty/%s' %
                (board, design[-1].upper()))
    directory = {
        'button_controller': 'additional_examples/button_controller'
    }.get(design, design)
    return 'TARGET="%s" make -C %s' % (board, directory)


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def install(src, design, board, info):
    family = info['family']
    dst = os.path.join(CORPUS, family, 'designs', 'f4pga-examples', design,
                       board)
    os.makedirs(dst, exist_ok=True)
    fasm = open(os.path.join(src, 'top.fasm'), 'rb').read()
    frm = open(os.path.join(src, 'top.frm'), 'rb').read()
    bit = open(os.path.join(src, 'top.bit'), 'rb').read()
    for name in ('vpr.fasm', 'vpr.fasm.xz', 'vpr.frm.xz'):
        if os.path.exists(os.path.join(dst, name)):
            os.remove(os.path.join(dst, name))
    if len(fasm) > COMPRESS_OVER:
        fasm_name = 'vpr.fasm.xz'
        with open(os.path.join(dst, fasm_name), 'wb') as f:
            f.write(lzma.compress(fasm, preset=9 | lzma.PRESET_EXTREME))
    else:
        fasm_name = 'vpr.fasm'
        with open(os.path.join(dst, fasm_name), 'wb') as f:
            f.write(fasm)
    frm_xz = lzma.compress(frm, preset=9 | lzma.PRESET_EXTREME)
    frm_note = ('`vpr.frm.xz` is the flow\'s frames (`xz -9e`), compared '
                'byte for byte with the Rust `fasm2frames --sparse '
                '--emit_pudc_b_pullup` by `tests/e2e/test_f4pga_examples.py`.')
    if len(frm_xz) <= FRM_LIMIT:
        with open(os.path.join(dst, 'vpr.frm.xz'), 'wb') as f:
            f.write(frm_xz)
    else:
        frm_note = ('The frames are not committed (%d bytes after xz, over '
                    'the %d byte limit of this corpus); their sha256 is '
                    'below.' % (len(frm_xz), FRM_LIMIT))
    with open(os.path.join(dst, 'difftest.json'), 'w') as f:
        json.dump({'part': info['part'], 'family': family}, f,
                  sort_keys=True)
        f.write('\n')
    device = info['device']
    readme = """# {design} / {board} -- f4pga (VPR) flow FASM (T7.3)

`{fasm_name}` is the FASM of f4pga-examples' `{design}` for the
{board_name} (`{board}`), built with the f4pga Yosys + VPR flow exactly as
f4pga-examples documents it, by `tools/e2e/run-f4pga-examples.sh {design}
{board}` (see `tools/e2e/README.md`, "f4pga-examples corpus (T7.3)"):
`top.fasm` of the flow's build directory, i.e. VPR's `genfasm` output with
the flow's extra FASM appended. {frm_note}

## Target

* Part: `{part}` (family `{family}`), VPR device `{device}`
* Build command (in f4pga-examples' `xc7/`): `{command}`
* Build time on this machine (4 cores, one build at a time): {seconds:.0f} s
* FASM: {lines} lines, {size} bytes

## Tools

{tools}
## Reference outputs of the flow

The flow writes the bitstream with `xcfasm --sparse --emit_pudc_b_pullup`
(frames to a temporary file it does not keep, then `xc7frames2bit`);
`top.frm` is that same xcfasm command line rerun with `--frm_out`.

```
sha256  top.fasm  {fasm_sha}
sha256  top.frm   {frm_sha}  ({frm_size} bytes)
sha256  top.bit   {bit_sha}  ({bit_size} bytes)
```

`top.bit` is not byte reproducible: its header holds the build date and
time and the path of the temporary `.frm` file. The Rust `xcfasm`,
`fasm2frames` and `xc7frames2bit` reproduce `top.frm` byte for byte and
`top.bit` up to that path (with the header's date and time given through
`SOURCE_DATE_EPOCH`); see `docs/rewrite/DESIGN-xilinx-db.md` §8.11.
""".format(design=design,
           board=board,
           board_name=BOARDS.get(board, board),
           fasm_name=fasm_name,
           frm_note=frm_note,
           part=info['part'],
           family=family,
           device=device,
           command=command(design, board),
           seconds=info['build_seconds'],
           lines=fasm.count(b'\n'),
           size=len(fasm),
           tools=TOOLS.format(device=device,
                              device_sha=ARCH_SHA256.get(device, '?')),
           fasm_sha=sha256(fasm),
           frm_sha=sha256(frm),
           frm_size=len(frm),
           bit_sha=sha256(bit),
           bit_size=len(bit))
    readme_path = os.path.join(dst, 'README.md')
    if design == 'counter_test' and board == 'arty_35':
        # Shared with the openXC7 flow's top.fasm (T7.1): its own README
        # stays, this one goes next to it.
        readme_path = os.path.join(dst, 'README.vpr.md')
    with open(readme_path, 'w') as f:
        f.write(readme)
    return dst


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--out', default=DEFAULT_OUT)
    parser.add_argument('designs', nargs='*', help='DESIGN/BOARD (default: '
                        'every built one)')
    args = parser.parse_args()
    for design in sorted(os.listdir(args.out)):
        for board in sorted(os.listdir(os.path.join(args.out, design))):
            if args.designs and '%s/%s' % (design,
                                           board) not in args.designs:
                continue
            src = os.path.join(args.out, design, board)
            try:
                with open(os.path.join(src, 'info.json')) as f:
                    info = json.load(f)
            except FileNotFoundError:
                continue
            if info.get('status') != 'built':
                print('skipping %s/%s: %s' % (design, board,
                                              info.get('status')))
                continue
            dst = install(src, design, board, info)
            print('%s/%s -> %s' % (design, board,
                                   os.path.relpath(dst, REPO_ROOT)))
    return 0


if __name__ == '__main__':
    sys.exit(main())

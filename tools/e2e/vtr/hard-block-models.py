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
"""Writes the Verilog models of VTR's hard blocks that a VTR Verilog
benchmark instantiates (T7.4).

Several of VTR's Verilog benchmarks instantiate VTR's architecture hard
blocks `single_port_ram`, `dual_port_ram`, `multiply` and `adder`, which
VTR's own flows map to its architectures' RAM, DSP and carry chain
blocks. For another architecture they need a model: VTR's
`vtr_flow/primitives.v` has one for each (its simulation models). This
copies those of the modules BENCHMARK.v uses and does not define itself,
verbatim except for comments and the `(* keep_hierarchy *)` attribute
(Yosys would keep them as separate modules, which the f4pga flow cannot
pack), so the f4pga synthesis maps them to Xilinx block RAM, DSP48 and
CARRY4 cells or logic.

  hard-block-models.py PRIMITIVES.v BENCHMARK.v OUT.v

Writes nothing (and exits 1) when the benchmark needs none of them.
"""
import re
import sys

HARD_BLOCKS = ('single_port_ram', 'dual_port_ram', 'multiply', 'adder')


def strip_comments(text):
    return re.sub(r'//[^\n]*|/\*.*?\*/', '', text, flags=re.S)


def modules(text):
    """{name: source} of the modules of a Verilog file."""
    out = {}
    pattern = r'(?:\(\*[^*]*\*\)\s*)?\bmodule\s+(\w+).*?\bendmodule\b'
    for m in re.finditer(pattern, text, re.S):
        out[m.group(1)] = m.group(0)
    return out


def main(primitives, benchmark, out):
    with open(primitives) as f:
        models = modules(strip_comments(f.read()))
    with open(benchmark) as f:
        text = strip_comments(f.read())
    defined = set(modules(text))
    used = [
        name for name in HARD_BLOCKS
        # An instance: `name #(...) inst (` or `name inst (`.
        if name not in defined and re.search(
            r'\b%s\s*(?:#\s*\(|\w+\s*\()' % name, text)
    ]
    if not used:
        return 1
    with open(out, 'w') as f:
        f.write('// VTR hard block models from vtr_flow/primitives.v '
                '(tools/e2e/vtr/hard-block-models.py)\n')
        for name in used:
            source = re.sub(r'^\(\*\s*keep_hierarchy\s*\*\)\s*', '',
                            models[name])
            f.write(source + '\n\n')
    print(' '.join(used))
    return 0


if __name__ == '__main__':
    if len(sys.argv) != 4:
        sys.exit(__doc__.split('\n\n')[-2])
    sys.exit(main(*sys.argv[1:]))

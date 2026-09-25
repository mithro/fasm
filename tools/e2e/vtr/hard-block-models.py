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
blocks. For another architecture they need a model. This writes one for
each of those the benchmark uses and does not define itself:

* `multiply` and `adder`: VTR's `vtr_flow/primitives.v` (its simulation
  models), verbatim except for comments;
* `single_port_ram` and `dual_port_ram`: the behaviour of the
  `primitives.v` models (a synchronous write, then a synchronous read of
  the addressed word that returns the data just written; a word per
  port), written with non-blocking assignments so that Yosys infers block
  RAM: `primitives.v`'s blocking `Mem[addr] = data; out = Mem[addr];`
  makes Yosys replace the memory with registers (a 4096 x 32 memory then
  takes Yosys more than 15 minutes, or it runs out of memory). What
  happens when both ports of a `dual_port_ram` access the same word is
  not modelled (VTR's architectures leave it undefined too).

`(* keep_hierarchy *)` (on the RAMs of `primitives.v`) is dropped: Yosys
would keep them as separate modules, which the f4pga flow cannot pack.

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


RAMS = {
    'single_port_ram':
    """module single_port_ram #(
    parameter ADDR_WIDTH = 1,
    parameter DATA_WIDTH = 1
) (
    input clk,
    input [ADDR_WIDTH-1:0] addr,
    input [DATA_WIDTH-1:0] data,
    input we,
    output reg [DATA_WIDTH-1:0] out
);
    reg [DATA_WIDTH-1:0] Mem[(2 ** ADDR_WIDTH)-1:0];
    always @(posedge clk) begin
        if (we) begin
            Mem[addr] <= data;
            out <= data;
        end else begin
            out <= Mem[addr];
        end
    end
endmodule""",
    'dual_port_ram':
    """module dual_port_ram #(
    parameter ADDR_WIDTH = 1,
    parameter DATA_WIDTH = 1
) (
    input clk,
    input [ADDR_WIDTH-1:0] addr1,
    input [ADDR_WIDTH-1:0] addr2,
    input [DATA_WIDTH-1:0] data1,
    input [DATA_WIDTH-1:0] data2,
    input we1,
    input we2,
    output reg [DATA_WIDTH-1:0] out1,
    output reg [DATA_WIDTH-1:0] out2
);
    reg [DATA_WIDTH-1:0] Mem[(2 ** ADDR_WIDTH)-1:0];
    always @(posedge clk) begin
        if (we1) begin
            Mem[addr1] <= data1;
            out1 <= data1;
        end else begin
            out1 <= Mem[addr1];
        end
    end
    always @(posedge clk) begin
        if (we2) begin
            Mem[addr2] <= data2;
            out2 <= data2;
        end else begin
            out2 <= Mem[addr2];
        end
    end
endmodule""",
}


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
        f.write('// Models of VTR hard blocks '
                '(tools/e2e/vtr/hard-block-models.py)\n')
        for name in used:
            source = RAMS.get(name) or re.sub(
                r'^\(\*\s*keep_hierarchy\s*\*\)\s*', '', models[name])
            f.write(source + '\n\n')
    print(' '.join(used))
    return 0


if __name__ == '__main__':
    if len(sys.argv) != 4:
        sys.exit(__doc__.split('\n\n')[-2])
    sys.exit(main(*sys.argv[1:]))

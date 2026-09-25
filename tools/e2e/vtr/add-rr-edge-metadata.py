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
"""Adds `fasm_features` metadata to every edge of a VPR rr graph (XML), the
way VTR's utils/fasm/test/test_fasm.cpp (`fasm_integration_test`, VTR
25e723a24) does in memory through `vpr::add_rr_edge_metadata` before it
writes the rr graph that genfasm then reads (T7.4):

* every edge gets `<src node>_<sink node>_<switch id>`;
* an edge into an IPIN also gets a second line,
  `PIN_<xlow>_<ylow>_<sub tile>_<port>_<pin>` (the test's
  `get_pin_feature`: the sub tile type, its port and the pin index in the
  port, found from the IPIN's pin number).

The rr graph XML names each pin of a block type `<sub tile>[<instance>].
<port>[<pin>]` (or `<sub tile>.<port>[<pin>]` for a capacity 1 sub
tile), which gives the same sub tile, port and pin as the test's lookup.

  add-rr-edge-metadata.py IN.xml OUT.xml
"""
import re
import sys
import xml.etree.ElementTree as ET

PIN_RE = re.compile(r'^([^\[\].]+)(?:\[\d+\])?\.([^\[\]]+)\[(\d+)\]$')


def main(src, dst):
    tree = ET.parse(src)
    root = tree.getroot()
    pins = {}
    for bt in root.iter('block_type'):
        pins[int(bt.get('id'))] = {
            int(pin.get('ptc')): pin.text
            for pin in bt.iter('pin')
        }
    grid = {}
    for gl in root.iter('grid_loc'):
        grid[(int(gl.get('x')), int(gl.get('y')))] = int(
            gl.get('block_type_id'))
    nodes = {}
    for n in root.find('rr_nodes'):
        loc = n.find('loc')
        nodes[int(n.get('id'))] = (n.get('type'), int(loc.get('xlow')),
                                   int(loc.get('ylow')), int(loc.get('ptc')))
    for e in root.find('rr_edges'):
        sink = int(e.get('sink_node'))
        value = '%s_%d_%s' % (e.get('src_node'), sink, e.get('switch_id'))
        kind, x, y, ptc = nodes[sink]
        if kind == 'IPIN':
            name = pins[grid[(x, y)]][ptc]
            m = PIN_RE.match(name)
            if not m:
                raise SystemExit('cannot parse the pin name %r' % name)
            value += '\nPIN_%d_%d_%s_%s_%d' % (x, y, m.group(1), m.group(2),
                                               int(m.group(3)))
        metadata = ET.SubElement(e, 'metadata')
        ET.SubElement(metadata, 'meta', name='fasm_features').text = value
    tree.write(dst, encoding='unicode')


if __name__ == '__main__':
    if len(sys.argv) != 3:
        sys.exit(__doc__.rsplit('\n\n', 1)[1])
    main(sys.argv[1], sys.argv[2])

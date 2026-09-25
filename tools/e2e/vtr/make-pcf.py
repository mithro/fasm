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
"""Writes a PCF that puts every top level port bit of a synthesised eblif
on a package pin, in the pin map's order (T7.4): the VTR Verilog
benchmarks come without pin constraints, and the f4pga flow's placer
needs a PCF. Ports are assigned in the pin map's order, clock ports (a
name containing `clk` or `clock`, any case) first, to pins the pin map
marks `is_clock` (in the xc7a50t_test pin maps that is every pin, so this
only puts the clocks first). Only IOB sites are used (not the XADC's
analog input pads, which the pin map lists too).

  make-pcf.py EBLIF PINMAP.csv OUT.pcf

Exit status 2 when the design has more port bits than the package has
pins (the message says how many).
"""
import csv
import re
import sys


def ports(eblif):
    names = []
    with open(eblif) as f:
        text = f.read().replace('\\\n', ' ')
    for line in text.splitlines():
        words = line.split()
        if words and words[0] in ('.inputs', '.outputs'):
            names += words[1:]
    return names


def main(eblif, pinmap, out):
    names = ports(eblif)
    with open(pinmap) as f:
        # Only the IOB sites: the XADC's analog inputs (VP/VN, site
        # IPAD_*) are in the pin map too, but cannot take a port.
        pins = [r for r in csv.DictReader(f) if r['iob'].startswith('IOB_')]
    clocks = [n for n in names if re.search('clk|clock', n, re.I)]
    others = [n for n in names if n not in clocks]
    if len(names) > len(pins):
        print('%d port bits, the package has %d pins' %
              (len(names), len(pins)))
        return 2
    clock_pins = [p['name'] for p in pins if p['is_clock'] == '1']
    free = [p['name'] for p in pins]
    lines = []
    for n in clocks:
        pin = clock_pins.pop(0) if clock_pins else free[0]
        free.remove(pin)
        if pin in clock_pins:
            clock_pins.remove(pin)
        lines.append('set_io %s %s' % (n, pin))
    for n in others:
        lines.append('set_io %s %s' % (n, free.pop(0)))
    with open(out, 'w') as f:
        f.write('\n'.join(lines) + '\n')
    return 0


if __name__ == '__main__':
    if len(sys.argv) != 4:
        sys.exit(__doc__.split('\n\n')[-2])
    sys.exit(main(*sys.argv[1:]))

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
"""Runs a python-fpga-interchange command line module with pycapnp 1.3
and RapidWright 2026 physical netlists.

    pfi_run.py MODULE ARGS...     (e.g. fasm_generator, patch, convert)

Two adaptations, nothing else (tools/e2e/setup-rapidwright.sh
--with-interchange, docs/rewrite/DESIGN-rapidwright.md):

* python-fpga-interchange 0.0.18 pins pycapnp 1.1.0, which does not build
  with Cython 3; in pycapnp 1.3 `from_bytes` returns a context manager
  instead of the message. It is entered (the message stays alive for the
  process).
* RapidWright's `PhysNetlistWriter` never writes the site of a PIP
  (`noSite`), but the xc7 FASM generator needs the site of a pseudo PIP
  (a route-thru: a LUT, an ILOGIC bypass...) and fails with `assert site`.
  The site is derived from the device: the one site of the PIP's tile
  whose pins are wired to both of the PIP's wires (a route-thru enters
  and leaves the same site). A PIP for which that is not exactly one
  site is left alone (the generator's assertion then reports it).
"""
import importlib
import sys

import fpga_interchange.interchange_capnp as interchange_capnp

_read_capnp_file = interchange_capnp.read_capnp_file


def read_capnp_file(*args, **kwargs):
    message = _read_capnp_file(*args, **kwargs)
    if hasattr(message, '__enter__') and not hasattr(message, 'which'):
        message = message.__enter__()
    return message


def pseudo_pip_site(device, tile_name, wire0, wire1):
    """The site of TILE_NAME whose pins connect to WIRE0 and WIRE1."""
    tile = device.tile_name_to_tile[tile_name]
    capnp = device.device_resource_capnp
    tile_type = capnp.tileTypeList[tile.tile_type_index]
    w0 = device.string_index[wire0]
    w1 = device.string_index[wire1]
    found = []
    for site in capnp.tileList[tile.tile_index].sites:
        wires = set(tile_type.siteTypes[site.type].primaryPinsToTileWires)
        if w0 in wires and w1 in wires:
            found.append(device.strs[site.name])
    return found[0] if len(found) == 1 else None


def add_pseudo_pip_sites(generator):
    from fpga_interchange.physical_netlist import PhysicalPip
    device = generator.device_resources
    for segments in generator.flattened_nets.values():
        for segment in segments:
            if isinstance(segment, PhysicalPip) and segment.site is None:
                tile = device.tile_name_to_tile[segment.tile]
                tile_type = device.get_tile_type(tile.tile_type_index)
                pip = tile_type.pip(device.string_index[segment.wire0],
                                    device.string_index[segment.wire1])
                if pip.which() == 'pseudoCells':
                    segment.site = pseudo_pip_site(device, segment.tile,
                                                   segment.wire0,
                                                   segment.wire1)


def main():
    interchange_capnp.read_capnp_file = read_capnp_file
    module = importlib.import_module('fpga_interchange.' + sys.argv[1])
    if hasattr(module, 'read_capnp_file'):
        module.read_capnp_file = read_capnp_file
    if sys.argv[1] == 'fasm_generator':
        from fpga_interchange.fasm_generators.generic import FasmGenerator
        fill = FasmGenerator.fill_pip_features

        def fill_pip_features(self, *args, **kwargs):
            add_pseudo_pip_sites(self)
            return fill(self, *args, **kwargs)

        FasmGenerator.fill_pip_features = fill_pip_features
    sys.argv = [sys.argv[1]] + sys.argv[2:]
    module.main()


if __name__ == '__main__':
    main()

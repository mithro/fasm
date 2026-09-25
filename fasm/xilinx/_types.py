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
""" Exceptions and result types of fasm.xilinx.

The extension module (fasm._fasm_rs.xilinx) imports this module to raise
these exceptions and to build these namedtuples, so they are the same
classes whether the extension is used directly or through fasm.xilinx.
"""

from collections import namedtuple

try:
    from fasm._fasm_rs import FasmParseError as _CoreFasmParseError
except ImportError:  # pragma: no cover - only without the extension

    class _CoreFasmParseError(Exception):
        pass


class Error(Exception):
    """ Base class of the errors of fasm.xilinx.

    ``reference_exception`` is the name of the exception the reference
    Python tools (prjxray, f4pga-xc-fasm) raise in the same situation; the
    command line tools print ``<reference_exception>: <str(error)>``.
    """
    reference_exception = 'Exception'


class DbError(Error):
    """ The database cannot be opened: a missing or malformed file, an
    unknown part, a directory that is not a prjxray-db / prjuray-db
    family, or a part file (part.yaml) that cannot be used. """
    reference_exception = 'fasm_xilinx.DbError'


class FasmLookupError(Error):
    """ Features that are not in the database
    (prjxray.fasm_assembler.FasmLookupError).

    ``messages`` holds one message per missing feature bit, in order;
    ``str()`` joins them with newlines.
    """
    reference_exception = 'prjxray.fasm_assembler.FasmLookupError'

    def __init__(self, message='', messages=None):
        super().__init__(message)
        if messages is None:
            messages = [message]
        self.messages = list(messages)


class FasmInconsistentBits(Error):
    """ Two FASM lines want a different value for one bit
    (prjxray.fasm_assembler.FasmInconsistentBits). """
    reference_exception = 'prjxray.fasm_assembler.FasmInconsistentBits'


class FasmKeyError(Error, KeyError):
    """ An unknown tile or tile type, a missing key of a ROI design.json,
    ... (a KeyError in the reference tools, and also a KeyError here:
    ``str()`` is the repr of the key). """
    reference_exception = 'KeyError'


class FasmParseError(Error, _CoreFasmParseError):
    """ The FASM text does not parse, or the FASM file cannot be read.

    ``str()`` is ``Parse error at L:C - message``; ``line`` and ``column``
    hold L and C (0 for a file that cannot be read). Also a subclass of
    ``fasm.parser.rust.FasmParseError``.
    """
    reference_exception = 'Exception'

    def __init__(self, message='', line=0, column=0):
        super().__init__(message)
        self.line = line
        self.column = column


class FrmError(Error, ValueError):
    """ A number of a .frm file that does not parse (where xc7frames2bit
    aborts). ``line`` is the 1-based line. """
    reference_exception = 'ValueError'

    def __init__(self, message='', line=0):
        super().__init__(message)
        self.line = line


class BitstreamError(Error):
    """ A bitstream cannot be written or read: the part is not of the
    format's architecture, the frames do not have the format's frame
    size, the data is not a bitstream, or its IDCODE is not the part's.
    """
    reference_exception = 'fasm_xilinx.BitstreamError'


Tile = namedtuple('Tile', 'name tile_type grid_x grid_y')
Tile.__doc__ = """ A tile of the part's grid (tilegrid.json). """

Roi = namedtuple('Roi', 'x1 x2 y1 y2')
Roi.__doc__ = """ A region of interest: the tiles with
x1 <= grid_x <= x2 and y1 <= grid_y <= y2 (prjxray.roi.Roi). """

RoiDesign = namedtuple('RoiDesign', 'roi required_features')
RoiDesign.__doc__ = """ A ROI design.json: its Roi and its
required_features as FASM text (None if it has none). """

FeatureBits = namedtuple(
    'FeatureBits', 'tile tile_type segbits_tile_type pseudo_pip block_type '
    'base_address frame_count offset bits')
FeatureBits.__doc__ = """ What one bit address of a FASM feature sets
(Database.lookup_feature).

tile, tile_type: the tile and its type. segbits_tile_type: the tile
type whose segbits were used (the alias target of an aliased tile).
pseudo_pip: None, or the pseudo PIP type ('always', 'default', 'hint'),
in which case the feature sets nothing (block_type, base_address,
frame_count and offset are None and bits is empty). block_type:
'CLB_IO_CLK' or 'BLOCK_RAM'. base_address, frame_count: the frames of
the bus of the tile (the assembler marks them in use). offset: the
effective word offset. bits: a tuple of (frame_address, word, bit,
value) tuples, value being True for a bit the feature sets and False for
a bit it clears (a '!' segbit); bits that cannot be placed in a frame
(which the assembler drops with a warning) are left out.
"""

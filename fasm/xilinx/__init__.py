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
""" Xilinx bitstream generation: FASM -> frames -> .bit, in Rust.

Python bindings of the fasm-xilinx crate (the fasm._fasm_rs.xilinx
extension submodule), mirroring the reference Python tools
(prjxray.db.Database, prjxray.fasm_assembler.FasmAssembler,
xc_fasm.fasm2frames.fasm2frames, xcfasm, xc7frames2bit, bitread) for
prjxray-db (Series7) and prjuray-db (UltraScale+) databases:

>>> import fasm.xilinx as fx                              # doctest: +SKIP
>>> db = fx.Database.open('prjxray-db/artix7', 'xc7a35tcsg324-1')
>>> asm = fx.FasmAssembler(db)
>>> asm.parse_fasm_filename('top.fasm')
>>> frames = asm.get_frames(sparse=True)
>>> frames.write_frm('top.frm')
>>> fx.write_bitstream(frames, db, 'top.bit')

or in one step, with the whole fasm2frames flow (ROI, required
features, PUDC_B pullup, STEPDOWN propagation):

>>> fx.fasm2bit('prjxray-db/artix7', 'xc7a35tcsg324-1',
...             'top.fasm', 'top.bit')                    # doctest: +SKIP

The output is byte for byte what the command line tools (fasm2frames,
xcfasm, xc7frames2bit / xcframes2bit) write. Opening a database,
assembling and writing bitstreams run with the GIL released. See
docs/rewrite/DESIGN-python.md.
"""

import collections.abc
import sys

from fasm.xilinx._types import (
    BitstreamError,
    DbError,
    Error,
    FasmInconsistentBits,
    FasmKeyError,
    FasmLookupError,
    FasmParseError,
    FeatureBits,
    FrmError,
    Roi,
    RoiDesign,
    Tile,
)

try:
    from fasm._fasm_rs import xilinx as _xilinx
except ImportError as e:  # pragma: no cover - only without the extension
    raise ImportError(
        'fasm.xilinx needs the fasm._fasm_rs extension module built with '
        'its "xilinx" cargo feature (the default): {}'.format(e)) from e

Database = _xilinx.Database
FasmAssembler = _xilinx.FasmAssembler
Frames = _xilinx.Frames
write_bitstream = _xilinx.write_bitstream
read_bitstream = _xilinx.read_bitstream
read_roi_design = _xilinx.read_roi_design
dump_frames_sparse = _xilinx.dump_frames_sparse
ARCHITECTURES = _xilinx.ARCHITECTURES

collections.abc.Mapping.register(Frames)

__all__ = [
    'ARCHITECTURES',
    'BitstreamError',
    'Database',
    'DbError',
    'Error',
    'FasmAssembler',
    'FasmInconsistentBits',
    'FasmKeyError',
    'FasmLookupError',
    'FasmParseError',
    'FeatureBits',
    'Frames',
    'FrmError',
    'Roi',
    'RoiDesign',
    'Tile',
    'dump_frames_sparse',
    'fasm2bit',
    'fasm2frames',
    'read_bitstream',
    'read_roi_design',
    'write_bitstream',
]


def _database(db_root, part, cache):
    if isinstance(db_root, Database):
        return db_root
    return Database.open(db_root, part, cache=cache)


def fasm2frames(
        db_root,
        part=None,
        filename_in=None,
        f_out=None,
        sparse=False,
        roi=None,
        debug=False,
        emit_pudc_b_pullup=False,
        fasm_text=None,
        cache=True):
    """ FASM -> frames, like ``xc_fasm.fasm2frames.fasm2frames`` (same
    arguments, same order) and the ``fasm2frames`` command line tool.

    Args:
        db_root: The database family directory (e.g.
            ``prjxray-db/artix7``), or an open ``Database`` (then ``part``
            and ``cache`` are ignored).
        part: The part, e.g. ``xc7a35tcsg324-1``.
        filename_in: The FASM file (a path).
        f_out: Where to write the frames as a ``.frm`` file: a file
            object or a path (optional).
        sparse: Only output the frames of the buses that were written
            (and of the ROI tiles) instead of every frame of the part.
        roi: A ROI ``design.json`` path (optional).
        debug: Print ``dump_frames_sparse(frames)`` to ``sys.stdout``.
        emit_pudc_b_pullup: Make the PUDC_B pin an input with a pullup if
            the FASM does not use its IOB.
        fasm_text: The FASM text (``str`` or ``bytes``) instead of
            ``filename_in``.
        cache: The binary database cache (see ``Database.open``).

    On an UltraScale+ (prjuray-db) database, prjuray's fasm2frames flow is
    used (no IO bank handling), like the ``fasm2frames`` tool. Warnings
    (bits beyond the end of a frame) are printed to ``sys.stderr`` like
    the reference.

    Returns:
        The ``Frames`` (compares equal to the reference's ``dict``).

    Raises:
        FasmParseError, FasmLookupError, FasmInconsistentBits,
        FasmKeyError, DbError, OSError: see ``fasm.xilinx.Error``.
    """
    db = _database(db_root, part, cache)
    frames = _xilinx.fasm2frames(
        db,
        filename_in,
        text=fasm_text,
        sparse=sparse,
        roi=roi,
        emit_pudc_b_pullup=emit_pudc_b_pullup)
    if debug:
        sys.stdout.write(dump_frames_sparse(frames))
    if f_out is not None:
        frames.write_frm(f_out)
    return frames


def fasm2bit(
        db_root,
        part,
        fn_in,
        bit_out,
        part_file=None,
        frm_out=None,
        sparse=False,
        roi=None,
        debug=False,
        emit_pudc_b_pullup=False,
        fasm_text=None,
        cache=True,
        format=None,
        source_date_epoch=None):
    """ FASM -> ``.bit`` in one step, like the ``xcfasm`` command line tool
    (``xc_fasm.xc_fasm``: ``fasm2frames`` then ``xc7frames2bit``), in
    process.

    Args:
        db_root, part, sparse, roi, debug, emit_pudc_b_pullup, fasm_text,
            cache: as for ``fasm2frames`` (``fn_in`` is its
            ``filename_in``).
        bit_out: Where to write the bitstream: a path or a binary file
            object; ``None`` returns the bitstream as ``bytes``.
        part_file: The ``part.yaml`` (``--part_file``); by default the
            database's own part data.
        frm_out: Also write the frames there as a ``.frm`` file.
        format: The bitstream format (see ``write_bitstream``); by
            default the database's architecture.
        source_date_epoch: The header date and time (see
            ``write_bitstream``).

    The header names ``frm_out`` (else ``fn_in``) as the design and
    ``part`` as the part, like ``xcfasm``.

    Returns:
        The ``Frames``, or ``(frames, bitstream bytes)`` when ``bit_out``
        is ``None``.
    """
    db = _database(db_root, part, cache)
    frames = fasm2frames(
        db,
        filename_in=fn_in,
        f_out=frm_out,
        sparse=sparse,
        roi=roi,
        debug=debug,
        emit_pudc_b_pullup=emit_pudc_b_pullup,
        fasm_text=fasm_text)
    design_name = frm_out if isinstance(frm_out, (str, bytes)) else fn_in
    if format is None:
        format = db.architecture
    bitstream = write_bitstream(
        frames,
        part_file if part_file is not None else db,
        bit_out,
        format=format,
        part_name=part if part is not None else db.part,
        design_name=design_name if design_name is not None else '',
        source_date_epoch=source_date_epoch)
    if bit_out is None:
        return frames, bitstream
    return frames

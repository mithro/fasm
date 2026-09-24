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
""" FASM parser implemented in Rust (the fasm._fasm_rs extension module).

Returns the same fasm.model namedtuples as the other parsers. Errors are
raised as FasmParseError, an Exception subclass whose str() is
'Parse error at LINE:COLUMN - message' like the ANTLR parser's exception.
Parsing runs with the GIL released.
"""

from fasm import _fasm_rs

implementation = 'rust'
"""
Module name of the default parser implementation, accessible as fasm.parser
"""

FasmParseError = _fasm_rs.FasmParseError


def parse_fasm_string(s):
    """ Parse FASM string, returning list of FasmLine named tuples.

    >>> parse_fasm_string('a.b.c = 1')[0].set_feature.feature
    'a.b.c'

    Args:
        s: The string containing FASM source to parse.

    Returns:
        A list of fasm.model.FasmLine.

    Raises:
        FasmParseError: s is not valid FASM.
    """
    return _fasm_rs.parse_fasm_string(s)


def parse_fasm_bytes(data):
    """ Parse FASM source given as bytes, returning list of FasmLine named
    tuples.

    Only comments and annotation values have to be valid UTF-8.

    >>> parse_fasm_bytes(b'a.b.c = 1')[0].set_feature.feature
    'a.b.c'

    Args:
        data: The bytes (or other buffer) containing FASM source to parse.

    Returns:
        A list of fasm.model.FasmLine.

    Raises:
        FasmParseError: data is not valid FASM.
    """
    return _fasm_rs.parse_fasm_bytes(data)


def parse_fasm_filename(filename):
    """ Parse FASM file, returning list of FasmLine named tuples.

    >>> parse_fasm_filename('examples/feature_only.fasm')[0]\
        .set_feature.feature
    'EXAMPLE_FEATURE.X0.Y0.BLAH'

    Args:
        filename: The file containing FASM source to parse (str, bytes or
            os.PathLike).

    Returns:
        A list of fasm.model.FasmLine.

    Raises:
        FasmParseError: the file is not valid FASM, or cannot be read
            ('Parse error at 0:0 - Couldn't open file ...').
    """
    return _fasm_rs.parse_fasm_filename(filename)

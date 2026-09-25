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
"""Parser implementation selection.

Imports and re-exports the best available FASM parser implementation as
``parse_fasm_filename``/``parse_fasm_string``, tried in the order
``'rust'`` (``fasm.parser.rust``, backed by the ``fasm._fasm_rs``
extension module), ``'antlr'`` (only present for a legacy ``setup.py``
build) and ``'textx'`` (``fasm.parser.textx``, pure Python, always
available). See :data:`available` and :data:`implementation` below, and
``docs/PYTHON.md`` for how to pick a parser explicitly.
"""

import importlib
from warnings import warn

available = []
""" List of parser submodules available. Strings should match module names.

In order of preference: 'rust' (the fasm._fasm_rs extension module, the
default) and 'textx' (pure Python, always available).
"""

_rust_import_error = None
try:
    importlib.import_module('fasm.parser.rust')
    available.append('rust')
except ImportError as e:
    # `except ... as e` implicitly deletes `e` at the end of the except
    # block, so it is copied to a variable that survives past it.
    _rust_import_error = e

if 'rust' in available:
    from fasm.parser.rust import \
        parse_fasm_filename, parse_fasm_string, implementation
else:
    warn(
        "Unable to import the fasm._fasm_rs Rust parser extension "
        "(ImportError: {}); falling back to the much slower pure Python "
        "textX based parser implementation.".format(_rust_import_error),
        RuntimeWarning)
    from fasm.parser.textx import \
        parse_fasm_filename, parse_fasm_string, implementation

# The textx parser is available as a fallback.
available.append('textx')

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

import importlib
from warnings import warn

available = []
""" List of parser submodules available. Strings should match module names.

In order of preference: 'rust' (the fasm._fasm_rs extension module), 'antlr'
(only present in an installation built with the legacy setup.py ANTLR
build) and 'textx' (pure Python, always available). The first one is the
default implementation exported by this module.
"""

_import_errors = {}
for _name in ('rust', 'antlr'):
    try:
        importlib.import_module('fasm.parser.' + _name)
        available.append(_name)
    except ImportError as e:
        _import_errors[_name] = e

if 'rust' in available:
    from fasm.parser.rust import \
        parse_fasm_filename, parse_fasm_string, implementation
elif 'antlr' in available:
    from fasm.parser.antlr import \
        parse_fasm_filename, parse_fasm_string, implementation
else:
    warn(
        """Unable to import fast Antlr4 parser implementation.
  ImportError: {}

  Falling back to the much slower pure Python textX based parser
  implementation.

  Getting the faster antlr parser can normally be done by installing the
  required dependencies and then reinstalling the fasm package with:
    pip uninstall
    pip install -v fasm

  The Rust parser extension (fasm._fasm_rs) is not available either.
  ImportError: {}
""".format(_import_errors['antlr'], _import_errors['rust']), RuntimeWarning)
    from fasm.parser.textx import \
        parse_fasm_filename, parse_fasm_string, implementation

# The textx parser is available as a fallback.
available.append('textx')

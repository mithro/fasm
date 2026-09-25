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
"""The ``fasm`` command line tool.

Implements the ``fasm`` console script: parses a FASM file with the
selected parser implementation (see :mod:`fasm.parser`) and prints it
back out, optionally in canonical form (``--canonical``). The Rust
``fasm`` binary (``rust/fasm-cli``) is a byte for byte compatible
reimplementation of this module's behaviour, including the ``Error: ...``
message printed to stdout (not stderr) on failure with exit code 0; see
``docs/rewrite/COMPAT.md`` ("Command line tool") for the exact,
documented scope of that compatibility.
"""

import argparse
import importlib
import fasm.parser
from fasm import fasm_tuple_to_string


def nullable_string(val):
    """``argparse`` type for ``--parser``: an empty string means "unset".

    Used as ``type=nullable_string`` so that ``--parser ''`` behaves the
    same as omitting ``--parser`` (falls back to the default parser).
    """
    if not val:
        return None
    return val


def get_fasm_parser(name=None):
    """Import and return the ``fasm.parser.*`` module for ``name``.

    ``name`` is one of :data:`fasm.parser.available` (``'rust'``,
    ``'textx'``, and ``'antlr'`` when a legacy ANTLR build is present),
    or ``None`` for the default parser (:mod:`fasm.parser` itself, which
    re-exports the first available implementation). ``'antlr'`` is
    accepted even when no ANTLR build exists, as long as the Rust parser
    is available: it is aliased to ``fasm.parser.rust``, which replaces
    it, so ``--parser antlr`` keeps working with a Rust-only install.

    :raises Exception: if ``name`` names a parser that is not available.
    """
    module_name = None
    if name is None:
        module_name = 'fasm.parser'
    elif name in fasm.parser.available:
        module_name = 'fasm.parser.' + name
    elif name == 'antlr' and 'rust' in fasm.parser.available:
        # The Rust parser replaces the ANTLR parser (which is only built by
        # the legacy setup.py build); keep --parser antlr working.
        module_name = 'fasm.parser.rust'
    else:
        raise Exception("Parser '{}' is not available.".format(name))
    return importlib.import_module(module_name)


def main():
    """Entry point for the ``fasm`` console script.

    Parses ``sys.argv`` (see ``fasm --help``), parses the named file with
    the selected parser, and prints the result via
    :func:`fasm.fasm_tuple_to_string` (canonical form with
    ``--canonical``). Any exception is caught and printed as
    ``Error: <message>`` to stdout, with the process still exiting 0 (the
    original tool's behaviour, kept for compatibility; see
    ``docs/rewrite/COMPAT.md``).
    """
    parser = argparse.ArgumentParser('FASM tool')
    parser.add_argument('file', help='Filename to process')
    parser.add_argument(
        '--canonical',
        action='store_true',
        help='Return canonical form of FASM.')
    parser.add_argument(
        '--parser',
        type=nullable_string,
        help='Select FASM parser to use. '
        'Default is to choose the best implementation available.')

    args = parser.parse_args()

    try:
        fasm_parser = get_fasm_parser(args.parser)
        fasm_tuples = fasm_parser.parse_fasm_filename(args.file)
        print(fasm_tuple_to_string(fasm_tuples, args.canonical))
    except Exception as e:
        print('Error: ' + str(e))


if __name__ == '__main__':
    main()

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
"""Print a machine readable JSON dump of a FASM file's parse tree.

Used by the Rust rewrite's differential tests to compare the Rust parser's
parse tree against the oracle (original Python `fasm` package) byte for
byte. Must be run with the oracle venv's interpreter:

    tests/oracle/venv/bin/python tests/oracle/dump.py [--parser textx|antlr] FILE

Output is the JSON encoding of `fasm.parser.parse_fasm_filename(FILE)`
(or a specific implementation's, if --parser is given):

    {"lines": [
        {"set_feature": {"feature": ..., "start": ..., "end": ...,
                          "value": "<decimal string>",
                          "value_format": "<ValueFormat name>" | null}
                         | null,
         "annotations": [{"name": ..., "value": ...}, ...] | null,
         "comment": "<string>" | null},
        ...
    ]}

`value` is emitted as a decimal string (not a JSON number) because FASM
feature values are arbitrary width bit vectors that can exceed what every
JSON consumer treats as a safe integer.

On any error (parse error, unknown parser, missing file, ...) this prints
`{"error": "<message>"}` and *still exits 0*: the error is the interesting,
comparable result, not a tool failure. The JSON is written with sorted
keys, no extra whitespace and a single trailing newline, so two runs over
identical input produce byte-identical output (e.g. `diff -u` between
parsers, or between runs on different machines).
"""
import argparse
import importlib
import json
import sys

import fasm.parser

PARSER_CHOICES = ('textx', 'antlr')


def dump_set_feature(set_feature):
    if set_feature is None:
        return None

    value_format = set_feature.value_format
    return {
        'feature': set_feature.feature,
        'start': set_feature.start,
        'end': set_feature.end,
        'value': str(set_feature.value),
        'value_format': value_format.name if value_format is not None else None,
    }


def dump_annotations(annotations):
    if annotations is None:
        return None

    return [
        {
            'name': annotation.name,
            'value': annotation.value,
        } for annotation in annotations
    ]


def dump_line(fasm_line):
    return {
        'set_feature': dump_set_feature(fasm_line.set_feature),
        'annotations': dump_annotations(fasm_line.annotations),
        'comment': fasm_line.comment,
    }


def get_parser_module(name):
    """ Returns the fasm.parser[.name] module for the requested implementation.

    name=None picks the best available implementation (fasm.parser itself).
    """
    if name is None:
        return fasm.parser

    if name not in PARSER_CHOICES:
        raise ValueError("Unknown parser '{}', expected one of {}".format(
            name, PARSER_CHOICES))

    if name not in fasm.parser.available:
        raise ImportError(
            "Parser '{}' is not available in this oracle venv "
            "(available: {})".format(name, sorted(fasm.parser.available)))

    return importlib.import_module('fasm.parser.' + name)


def dump_fasm_file(filename, parser_name):
    parser_module = get_parser_module(parser_name)
    lines = list(parser_module.parse_fasm_filename(filename))
    return {'lines': [dump_line(line) for line in lines]}


def main(argv=None):
    arg_parser = argparse.ArgumentParser(description=__doc__)
    arg_parser.add_argument('file', help='FASM file to parse')
    arg_parser.add_argument(
        '--parser',
        choices=PARSER_CHOICES,
        default=None,
        help='Parser implementation to use '
        '(default: the best implementation available).')
    args = arg_parser.parse_args(argv)

    try:
        doc = dump_fasm_file(args.file, args.parser)
    except Exception as e:
        doc = {'error': str(e)}

    json.dump(doc, sys.stdout, sort_keys=True, separators=(',', ':'))
    sys.stdout.write('\n')

    return 0


if __name__ == '__main__':
    sys.exit(main())

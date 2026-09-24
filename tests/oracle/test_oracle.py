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
"""Self tests for the oracle (tests/oracle). Run with the oracle venv's
pytest, e.g.:

    tests/oracle/venv/bin/python -m pytest tests/oracle/test_oracle.py -v

These are sanity checks on the oracle setup itself (that every parser it
reports as available actually parses the shared example corpus, and that
`dump.py`'s output does not depend on which parser produced it), not tests
of the Rust rewrite.
"""
import json
import subprocess
import sys
from pathlib import Path

import pytest

import fasm.parser

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parent.parent
MANY_FASM = REPO_ROOT / 'examples' / 'many.fasm'
DUMP_PY = HERE / 'dump.py'

AVAILABLE_PARSERS = sorted(fasm.parser.available)


def run_dump(parser_name, fasm_file=MANY_FASM):
    """ Runs dump.py --parser <parser_name> <fasm_file>, returns raw stdout. """
    result = subprocess.run(
        [sys.executable,
         str(DUMP_PY), '--parser', parser_name,
         str(fasm_file)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
        text=True,
    )
    assert result.stderr == '', result.stderr
    return result.stdout


def test_at_least_one_parser_available():
    assert AVAILABLE_PARSERS, "fasm.parser.available is empty"


def test_textx_always_available():
    # The oracle setup requires the textX parser to always work; the ANTLR
    # C++ parser is only available when its native extension could be built.
    assert 'textx' in AVAILABLE_PARSERS


@pytest.mark.parametrize('parser_name', AVAILABLE_PARSERS)
def test_examples_many_fasm_parses(parser_name):
    """ Every parser this oracle venv reports as available must be able to
    parse the shared examples/many.fasm corpus file without error, and
    produce at least one line. """
    stdout = run_dump(parser_name)
    doc = json.loads(stdout)
    assert 'error' not in doc, doc.get('error')
    assert 'lines' in doc
    assert len(doc['lines']) > 0


@pytest.mark.parametrize('parser_name', AVAILABLE_PARSERS)
def test_dump_output_is_deterministic(parser_name):
    """ dump.py's JSON output must be byte identical across repeated runs
    (sorted keys, no incidental whitespace/ordering differences), since
    differential tests diff it directly. """
    first = run_dump(parser_name)
    second = run_dump(parser_name)
    assert first == second
    assert first.endswith('\n')
    assert '\n' not in first[:-1]
    assert json.loads(first) == json.loads(second)


def test_dump_identical_across_parsers():
    """ When more than one parser implementation is available, they must
    agree exactly on the parse tree for examples/many.fasm: dump.py's JSON
    output must be byte for byte identical. """
    if len(AVAILABLE_PARSERS) < 2:
        pytest.skip(
            'only one parser implementation available in this oracle venv '
            '({}); nothing to compare'.format(AVAILABLE_PARSERS))

    outputs = {name: run_dump(name) for name in AVAILABLE_PARSERS}
    reference_name = AVAILABLE_PARSERS[0]
    reference = outputs[reference_name]
    for name, output in outputs.items():
        assert output == reference, (
            "dump.py output for examples/many.fasm differs between "
            "'{}' and '{}'".format(reference_name, name))


def test_dump_reports_parse_errors_without_failing():
    """ dump.py must report a parse error as {"error": ...} and still
    exit 0, rather than raising/crashing, so differential tests can diff
    errors like any other output. """
    import tempfile

    with tempfile.NamedTemporaryFile(
            mode='w', suffix='.fasm', delete=False) as f:
        # Not valid FASM syntax at all; rejected by both parser
        # implementations with a non-empty message and no traceback.
        f.write('not fasm at all ===\n')
        bad_file = f.name

    try:
        result = subprocess.run(
            [sys.executable,
             str(DUMP_PY), '--parser', AVAILABLE_PARSERS[0], bad_file],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    finally:
        Path(bad_file).unlink(missing_ok=True)

    assert result.returncode == 0, result.stderr
    doc = json.loads(result.stdout)
    assert 'error' in doc
    assert isinstance(doc['error'], str)
    assert doc['error']


if __name__ == '__main__':
    sys.exit(pytest.main([__file__, '-v']))

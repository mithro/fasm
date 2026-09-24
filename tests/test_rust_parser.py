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
""" Tests of the Rust parser (fasm.parser.rust, the fasm._fasm_rs module).

Needs the extension module to be built into the venv running the tests
(see tests/README.md):

* in the source tree: `maturin develop` (or `pip install -e .`), then
  `pytest tests/test_simple.py tests/test_rust_parser.py`; a plain
  `pip install .` does not work there, because pytest puts the repository
  root (whose fasm/ has no extension module) first on sys.path;
* against an installed package (`pip install .`): run pytest from outside
  the repository root with `--import-mode=importlib`.

Paths are absolute and subprocesses run in a temporary directory, so both
ways import the same package.
"""

import gc
import glob
import json
import os
import pathlib
import subprocess
import sys
import threading

import pytest

import fasm
import fasm.parser
from fasm import _fasm_rs
from fasm.model import Annotation, FasmLine, SetFasmFeature, ValueFormat
from fasm.parser import rust, textx

ROOT = os.path.normpath(
    os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))


def corpus_files():
    files = glob.glob(os.path.join(ROOT, 'examples', '*.fasm'))
    files += glob.glob(
        os.path.join(ROOT, 'tests', 'corpus', '**', '*.fasm'), recursive=True)
    assert files
    return sorted(files)


def corpus_id(path):
    return os.path.relpath(path, ROOT)


def normalise(lines):
    """ The textX parser returns annotations as a tuple, the Rust and ANTLR
    parsers as a list. """
    return [
        line._replace(annotations=list(line.annotations))
        if line.annotations is not None else line for line in lines
    ]


def textx_or_skip(parse, arg):
    try:
        return list(parse(arg))
    except Exception as e:
        # See docs/rewrite/COMPAT.md: inputs textX rejects but the Rust
        # (and ANTLR) parser accepts.
        pytest.skip('textX cannot parse this input: {!r}'.format(e))


# Copied from tests/oracle/dump.py (the JSON shape of the oracle dumps in
# tests/corpus/oracle/*.json).
def dump_set_feature(set_feature):
    if set_feature is None:
        return None

    value_format = set_feature.value_format
    return {
        'feature': set_feature.feature,
        'start': set_feature.start,
        'end': set_feature.end,
        'value': str(set_feature.value),
        'value_format':
        value_format.name if value_format is not None else None,
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


def dump(lines):
    doc = {'lines': [dump_line(line) for line in lines]}
    return json.dumps(doc, sort_keys=True, separators=(',', ':')) + '\n'


# Inputs every parser agrees on (see docs/rewrite/COMPAT.md for the ones
# where they differ).
SNIPPETS = [
    '',
    '\n\n',
    'a',
    'a.b.c\n',
    'a[0]',
    'a[7]',
    'a[7:0] = 8\'hA5',
    'a[7:0] = \'hA_5',
    'a[7:0] = 8\'b1010_0101',
    'a[7:0] = 8\'o245',
    'a[7:0] = 8\'d165',
    'a[7:0] = 165',
    'a[7:0] = 0',
    'a = 1',
    'a[31:0] = 4294967295',
    'a[255:0] = 256\'h' + 'F' * 64,
    'a[299:0] = ' + str(2**299 + 12345),
    'a { b = "c" }',
    'a { b = "c", d = "" } # comment',
    '{ b = "c" }',
    '# only a comment',
    '#',
    'a # c',
    'x.y[3:1] = 3\'b101 { n = "v" }\nz\n# end\n',
    'a\r\nb\r\n',
    '  a  [ 1 ]  ',
]


def rust_stricter_than_textx(path):
    """ True for corpus files that textX accepts but the Rust (and ANTLR)
    parser rejects: everything under tests/corpus/synthetic/invalid/ and
    the edge cases classified ``rust_stricter`` in the corpus manifest
    (see docs/rewrite/COMPAT.md and tools/gen-corpus.py). """
    rel = os.path.relpath(path, os.path.join(ROOT, 'tests', 'corpus'))
    if rel.startswith(os.path.join('synthetic', 'invalid') + os.sep):
        return True
    manifest = os.path.join(ROOT, 'tests', 'corpus', 'synthetic',
                            'edge-cases', 'manifest.json')
    if rel.startswith('synthetic' + os.sep) and os.path.exists(manifest):
        with open(manifest) as f:
            entries = json.load(f)
        key = rel[len('synthetic' + os.sep):]
        entry = entries.get(key)
        return entry is not None and entry.get('class') == 'rust_stricter'
    return False


def rust_parse_or_skip(path):
    try:
        return rust.parse_fasm_filename(path)
    except rust.FasmParseError as e:
        if rust_stricter_than_textx(path):
            pytest.skip('documented divergence, Rust rejects: {}'.format(e))
        raise


@pytest.mark.parametrize('path', corpus_files(), ids=corpus_id)
def test_corpus_parity_with_textx(path):
    expected = normalise(textx_or_skip(textx.parse_fasm_filename, path))
    assert rust_parse_or_skip(path) == expected


@pytest.mark.parametrize('text', SNIPPETS)
def test_snippet_parity_with_textx(text):
    expected = normalise(textx_or_skip(textx.parse_fasm_string, text))
    assert rust.parse_fasm_string(text) == expected
    assert rust.parse_fasm_bytes(text.encode('utf-8')) == expected


@pytest.mark.parametrize(
    'json_path',
    sorted(
        glob.glob(os.path.join(ROOT, 'tests', 'corpus', 'oracle', '*.json'))),
    ids=os.path.basename)
def test_oracle_antlr_dump(json_path):
    """ tests/corpus/oracle/NAME.json is the oracle's (ANTLR) parse of
    examples/NAME.fasm, dumped by tests/oracle/dump.py. """
    name = os.path.splitext(os.path.basename(json_path))[0]
    lines = rust.parse_fasm_filename(
        os.path.join(ROOT, 'examples', name + '.fasm'))
    with open(json_path) as f:
        assert dump(lines) == f.read()


def test_default_implementation():
    assert fasm.parser.available == ['rust', 'textx']
    assert fasm.parser.implementation == 'rust'
    assert fasm.parser.parse_fasm_string is rust.parse_fasm_string
    assert fasm.parse_fasm_filename is rust.parse_fasm_filename
    assert rust.implementation == 'rust'


def test_namedtuple_types():
    lines = rust.parse_fasm_string(
        'a[3:0] = 4\'hF { x = "y" } # c\nb\n# only\n')
    assert type(lines) is list
    for line in lines:
        assert type(line) is FasmLine
    set_feature = lines[0].set_feature
    assert type(set_feature) is SetFasmFeature
    assert type(set_feature.feature) is str
    assert type(set_feature.start) is int
    assert type(set_feature.end) is int
    assert type(set_feature.value) is int
    assert set_feature.value_format is ValueFormat.VERILOG_HEX
    assert type(lines[0].annotations) is list
    assert type(lines[0].annotations[0]) is Annotation
    assert type(lines[0].comment) is str


def test_value_formats():
    lines = rust.parse_fasm_string(
        'a[7:0] = 5\n'
        'a[7:0] = \'d5\n'
        'a[7:0] = \'h5\n'
        'a[7:0] = \'b101\n'
        'a[7:0] = \'o5\n'
        'a\n')
    assert [line.set_feature.value_format for line in lines] == [
        ValueFormat.PLAIN,
        ValueFormat.VERILOG_DECIMAL,
        ValueFormat.VERILOG_HEX,
        ValueFormat.VERILOG_BINARY,
        ValueFormat.VERILOG_OCTAL,
        None,
    ]
    assert all(line.set_feature.value == 5 for line in lines[:5])


def test_nones():
    assert rust.parse_fasm_string('a\n') == [
        FasmLine(
            set_feature=SetFasmFeature(
                feature='a', start=None, end=None, value=1, value_format=None),
            annotations=None,
            comment=None)
    ]
    assert rust.parse_fasm_string('a[5]') == [
        FasmLine(
            set_feature=SetFasmFeature(
                feature='a', start=5, end=None, value=1, value_format=None),
            annotations=None,
            comment=None)
    ]
    assert rust.parse_fasm_string('# c') == [
        FasmLine(set_feature=None, annotations=None, comment=' c')
    ]
    assert rust.parse_fasm_string('{ a = "b" }') == [
        FasmLine(
            set_feature=None,
            annotations=[Annotation(name='a', value='b')],
            comment=None)
    ]
    assert rust.parse_fasm_string('') == []


@pytest.mark.parametrize('bits', [63, 64, 65, 128, 255, 256, 257, 1000])
def test_wide_values(bits):
    for value in (2**bits - 1, 2**(bits - 1), 2**(bits - 1) + 1, 0, 1):
        texts = [
            '{}\'h{:X}'.format(bits, value),
            '{}\'b{:b}'.format(bits, value),
            '{}\'o{:o}'.format(bits, value),
            '{}\'d{}'.format(bits, value),
            str(value),
        ]
        for text in texts:
            line = 'a[{}:0] = {}'.format(bits - 1, text)
            (parsed, ) = rust.parse_fasm_string(line)
            assert parsed.set_feature.value == value, line
            assert type(parsed.set_feature.value) is int


def test_parse_error():
    with pytest.raises(rust.FasmParseError) as info:
        rust.parse_fasm_string('a\nb c\n')
    error = info.value
    assert isinstance(error, Exception)
    assert str(error).startswith('Parse error at 2:2 - ')
    assert (error.line, error.column) == (2, 2)
    assert type(error).__name__ == 'FasmParseError'
    assert type(error).__module__ == 'fasm.parser.rust'
    assert rust.FasmParseError is _fasm_rs.FasmParseError


def test_value_range_error():
    # The ANTLR parser printed an AssertionError to stderr and returned
    # None for these; the Rust parser raises (docs/rewrite/COMPAT.md).
    for text, position in (('a = 2', '1:4'), ('a[3:0] = 5\'h10', '1:9'),
                           ('a[0:1]', '1:1')):
        with pytest.raises(rust.FasmParseError,
                           match='^Parse error at {} - '.format(position)):
            rust.parse_fasm_string(text)


def test_missing_file(tmp_path):
    path = str(tmp_path / 'missing.fasm')
    with pytest.raises(rust.FasmParseError) as info:
        rust.parse_fasm_filename(path)
    assert str(info.value).startswith(
        "Parse error at 0:0 - Couldn't open file {}: ".format(path))
    assert (info.value.line, info.value.column) == (0, 0)


def test_directory(tmp_path):
    with pytest.raises(rust.FasmParseError,
                       match="^Parse error at 0:0 - Couldn't open file "):
        rust.parse_fasm_filename(str(tmp_path))


def test_filename_types(tmp_path):
    path = tmp_path / 'caf\u00e9.fasm'
    path.write_bytes(b'a.b\n')
    expected = [
        FasmLine(
            set_feature=SetFasmFeature(
                feature='a.b',
                start=None,
                end=None,
                value=1,
                value_format=None),
            annotations=None,
            comment=None)
    ]
    assert rust.parse_fasm_filename(str(path)) == expected
    assert rust_parse_or_skip(path) == expected
    assert rust.parse_fasm_filename(os.fsencode(str(path))) == expected
    assert rust.parse_fasm_filename(pathlib.PurePath(str(path))) == expected
    with pytest.raises(TypeError):
        rust.parse_fasm_filename(42)


def test_string_and_bytes_inputs():
    assert rust.parse_fasm_bytes(bytearray(b'a\n')) == \
        rust.parse_fasm_string('a\n')
    with pytest.raises(TypeError):
        rust.parse_fasm_bytes(memoryview(b'a\n'))
    with pytest.raises(TypeError):
        rust.parse_fasm_string(b'a\n')
    # Non-ASCII text is only allowed in comments and annotation values.
    assert rust.parse_fasm_string('a # caf\u00e9')[0].comment == ' caf\u00e9'
    data = 'a { x = "\u00e9" }'.encode('utf-8')
    annotations = rust.parse_fasm_bytes(data)[0].annotations
    assert annotations == [Annotation(name='x', value='\u00e9')]
    with pytest.raises(rust.FasmParseError, match='^Parse error at 1:4 - '):
        rust.parse_fasm_bytes(b'a # \xff')
    # A NUL does not end the input (it did for the ANTLR parse_fasm_string).
    with pytest.raises(rust.FasmParseError, match='^Parse error at 2:2 - '):
        rust.parse_fasm_string('a # c\x00d\nb c\n')


def big_fasm(lines):
    return ''.join(
        'TILE_X{0}Y{1}.SITE.BEL.INIT[63:0] = 64\'h{0:016X} '
        '{{ n = "{1}" }} # line {1}\n'.format(i * 7919, i)
        for i in range(lines))


def test_threads():
    """ Parsing releases the GIL: smoke test of concurrent parses. """
    text = big_fasm(20000)
    expected = rust.parse_fasm_string(text)
    assert len(expected) == 20000
    results = [None] * 4
    errors = []

    def work(i):
        try:
            for _ in range(3):
                results[i] = rust.parse_fasm_string(text)
                assert results[i] == expected
        except Exception as e:  # pragma: no cover
            errors.append(e)

    threads = [threading.Thread(target=work, args=(i, )) for i in range(4)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    assert not errors
    assert all(result == expected for result in results)


def test_textx_fallback_without_extension(tmp_path):
    """ Without the extension module, fasm.parser falls back to textX with
    the RuntimeWarning of the original package. """
    code = '\n'.join(
        [
            'import sys, warnings',
            'sys.modules["fasm._fasm_rs"] = None',
            'with warnings.catch_warnings(record=True) as w:',
            '    warnings.simplefilter("always")',
            '    import fasm.parser',
            'print(fasm.parser.available, fasm.parser.implementation)',
            'print([str(x.message).splitlines()[0] for x in w',
            '       if x.category is RuntimeWarning])',
        ])
    out = subprocess.check_output(
        [sys.executable, '-c', code],
        cwd=str(tmp_path),
        universal_newlines=True)
    assert out.splitlines() == [
        "['textx'] textx",
        "['Unable to import fast Antlr4 parser implementation.']",
    ]


def run_tool(cwd, *args):
    """ Runs `python -m fasm.tool` in `cwd` (not the repository root, whose
    fasm/ would shadow an installed package). """
    return subprocess.run(
        [sys.executable, '-m', 'fasm.tool'] + list(args),
        cwd=str(cwd),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        universal_newlines=True)


def test_tool(tmp_path):
    example = os.path.join(ROOT, 'examples', 'feature_only.fasm')
    for parser in ([], ['--parser', 'rust'], ['--parser', 'antlr'],
                   ['--parser', 'textx']):
        result = run_tool(tmp_path, example, *parser)
        assert result.stdout == 'EXAMPLE_FEATURE.X0.Y0.BLAH\n\n', parser
        assert result.stderr == ''

    bad = tmp_path / 'bad.fasm'
    bad.write_text('a b\n')
    result = run_tool(tmp_path, str(bad))
    assert result.stdout.startswith('Error: Parse error at 1:2 - ')
    assert result.stderr == ''


# fasm_tuple_to_string fast path (_fasm_rs.fasm_tuple_to_string).


@pytest.mark.parametrize('path', corpus_files(), ids=corpus_id)
@pytest.mark.parametrize('canonical', [False, True])
def test_fasm_tuple_to_string_corpus(path, canonical):
    for parser in (rust, textx):
        try:
            model = list(parser.parse_fasm_filename(path))
        except Exception:
            continue
        try:
            expected = fasm.fasm_tuple_to_string(model, canonical)
        except AssertionError:
            # The pure Python implementation asserts on models that only
            # textX produces (e.g. `a[0:1] = 0`, end before start); the fast
            # path must decline them so the caller sees the same exception.
            assert rust_stricter_than_textx(path), path
            assert _fasm_rs.fasm_tuple_to_string(model, canonical) is None
            continue
        fast = _fasm_rs.fasm_tuple_to_string(model, canonical)
        if fast is None:
            # The fast path declines models it cannot represent exactly
            # (e.g. addresses above 2**32 - 1 that only textX accepts) and
            # the caller falls back to the Python implementation.  That may
            # only happen for the synthetic divergence corpus.
            assert rust_stricter_than_textx(path), path
            continue
        assert fast == expected
        assert _fasm_rs.fasm_tuple_to_string(tuple(model),
                                             canonical) == expected


def test_fasm_tuple_to_string_oracle():
    model = rust.parse_fasm_filename(
        os.path.join(ROOT, 'examples', 'many.fasm'))
    oracle = os.path.join(ROOT, 'tests', 'corpus', 'oracle')
    for canonical, name in ((False, 'many.fasm.out.txt'),
                            (True, 'many.fasm.canonical.txt')):
        with open(os.path.join(oracle, name)) as f:
            expected = f.read()
        assert _fasm_rs.fasm_tuple_to_string(model, canonical) == expected
        assert _fasm_rs.fasm_tuple_to_string(
            model, canonical=canonical) == expected


@pytest.mark.parametrize('text', SNIPPETS + [big_fasm(100)])
@pytest.mark.parametrize('canonical', [False, True])
def test_fasm_tuple_to_string_snippets(text, canonical):
    model = rust.parse_fasm_string(text)
    assert _fasm_rs.fasm_tuple_to_string(model, canonical) == \
        fasm.fasm_tuple_to_string(model, canonical)


def feature(**kwargs):
    fields = dict(
        feature='a.b', start=None, end=None, value=1, value_format=None)
    fields.update(kwargs)
    return FasmLine(
        set_feature=SetFasmFeature(**fields), annotations=None, comment=None)


class StrSubclass(str):
    def __format__(self, spec):
        return 'custom'


class IntSubclass(int):
    pass


# Models the fast path handles: same result as Python.
HANDLED = [
    [],
    [FasmLine(set_feature=None, annotations=None, comment=None)],
    [FasmLine(set_feature=None, annotations=[], comment='')],
    [FasmLine(set_feature=None, annotations=(), comment=None)],
    [
        FasmLine(
            set_feature=None,
            annotations=(Annotation('a', 'b'), Annotation('c', '')),
            comment=' x')
    ],
    [feature(value=0)],
    [feature(start=0, end=7, value=0, value_format=None)],
    [feature(start=0, end=7, value=0xA5, value_format=None)],
    [
        feature(start=3, end=300, value=2**297 + 5, value_format=f)
        for f in ValueFormat
    ],
    [feature(start=0), feature(start=0),
     feature(start=1)],
    [
        feature(start=2, end=5, value=0b1010, value_format=f)
        for f in ValueFormat
    ],
    [feature(start=4294967295)],
    [
        feature(
            start=4294967000,
            end=4294967295,
            value=2**295 + 5,
            value_format=ValueFormat.VERILOG_HEX)
    ],
]


@pytest.mark.parametrize('model', HANDLED)
@pytest.mark.parametrize('canonical', [False, True])
def test_fasm_tuple_to_string_handled(model, canonical):
    result = _fasm_rs.fasm_tuple_to_string(model, canonical)
    assert result is not None
    assert result == fasm.fasm_tuple_to_string(model, canonical)


# Models the fast path does not handle (returns None): the caller then uses
# the Python implementation, which gives its own result or raises.
NOT_HANDLED = [
    'not a list',
    iter([]),
    [None],
    [(None, None, None)],
    [feature(value=True)],
    [feature(value=IntSubclass(1))],
    [feature(value=-1, value_format=ValueFormat.PLAIN)],
    [feature(value=2)],  # AssertionError in Python
    [feature(value=1, value_format=ValueFormat.PLAIN, feature=StrSubclass())],
    [feature(start=4294967296)],
    [feature(start=-1)],
    [feature(start=None, end=2, value=1)],
    [feature(value_format=0)],
    [feature(value=1.0)],
    [FasmLine(set_feature=None, annotations=[('a', 'b')], comment=None)],
    [
        FasmLine(
            set_feature=None, annotations={Annotation('a', 'b')}, comment=None)
    ],
    [FasmLine(set_feature=None, annotations=None, comment=b'x')],
    [FasmLine(set_feature=None, annotations=None, comment='\udcff')],
]


@pytest.mark.parametrize('model', NOT_HANDLED)
@pytest.mark.parametrize('canonical', [False, True])
def test_fasm_tuple_to_string_not_handled(model, canonical):
    assert _fasm_rs.fasm_tuple_to_string(model, canonical) is None


# Models on the edge of the Rust model: whatever the fast path does, it
# must never give a result different from the Python implementation's, and
# must return None where Python raises.
EDGE = [
    [feature(start=3, end=2, value=0, value_format=ValueFormat.PLAIN)],
    [feature(start=3, end=2, value=0)],
    [feature(start=5, end=None, value=0)],
    [feature(start=0, end=0, value=1, value_format=ValueFormat.VERILOG_HEX)],
    [feature(start=None, end=None, value=0, value_format=ValueFormat.PLAIN)],
    [feature(start=None, end=None, value=2)],
    [feature(start=7, end=None, value=2, value_format=ValueFormat.PLAIN)],
    [feature(feature='', start=1)],
    [feature(feature='a..b. c')],
    [FasmLine(set_feature=None, annotations=None, comment='\u00e9\n')],
]


@pytest.mark.parametrize('model', HANDLED + NOT_HANDLED[2:] + EDGE)
@pytest.mark.parametrize('canonical', [False, True])
def test_fasm_tuple_to_string_never_differs(model, canonical):
    try:
        expected = fasm.fasm_tuple_to_string(model, canonical)
    except Exception:
        expected = None
    result = _fasm_rs.fasm_tuple_to_string(model, canonical)
    if expected is None:
        assert result is None
    else:
        assert result in (None, expected)


def test_gc_state_restored():
    """ The cyclic GC is paused while a large result is built, and its
    state restored afterwards, also on error. """
    text = big_fasm(1000)
    assert gc.isenabled()
    assert len(rust.parse_fasm_string(text)) == 1000
    assert gc.isenabled()
    gc.disable()
    try:
        assert len(rust.parse_fasm_string(text)) == 1000
        assert not gc.isenabled()
    finally:
        gc.enable()
    with pytest.raises(rust.FasmParseError):
        rust.parse_fasm_string(text + 'a b\n')
    assert gc.isenabled()

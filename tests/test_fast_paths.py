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
""" Tests for the T3.3 Rust fast paths wired into the public Python API:

* fasm.fasm_tuple_to_string, backed by fasm._fasm_rs.fasm_tuple_to_string
  (T3.1); wiring only -- the fast path itself is exercised by
  tests/test_rust_parser.py.
* fasm.output.merge_and_sort, backed by fasm._fasm_rs.merge_and_sort (new
  in T3.3): differential tests against fasm.output._merge_and_sort_py (the
  pure Python implementation both the extension-missing case and a
  declining fast path fall back to), call count/order tests for
  zero_function/sort_key, and wiring tests.

Needs the extension module built into the venv running the tests (see
tests/README.md), like tests/test_rust_parser.py.
"""

import os
import random

import pytest

import fasm
import fasm.output
from fasm import _fasm_rs
from fasm.model import Annotation, FasmLine, SetFasmFeature, ValueFormat
from fasm.parser import rust

from tests.test_rust_parser import SNIPPETS, big_fasm, corpus_files, corpus_id

ROOT = os.path.normpath(
    os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))

# ---------------------------------------------------------------------------
# Random model generation.
#
# A small, reused pool of feature names (rather than unique names per line)
# so random models exercise merge_and_sort's grouping/merging: several
# lines sharing a feature (some mergeable, some not because they carry an
# annotation/comment or conflict), several distinct groups sharing a first
# `.` component, comments/annotations attaching to the next/previous group,
# and blank lines.
# ---------------------------------------------------------------------------

FEATURE_NAMES = [
    'A.X0.Y0.INIT',
    'A.X0.Y0.FLAG',
    'A.X1.Y0.INIT',
    'B.X0.Y0.INIT',
    'B.X0.Y1.INIT',
    'C.SOLO',
    'D.ZERO.BIT',
]


def random_value_format(rng):
    return rng.choice(list(ValueFormat) + [None])


def random_set_feature(rng, name=None):
    name = name or rng.choice(FEATURE_NAMES)
    kind = rng.choice(('implicit', 'single', 'range'))
    if kind == 'implicit':
        return SetFasmFeature(
            feature=name, start=None, end=None, value=1, value_format=None)
    if kind == 'single':
        return SetFasmFeature(
            feature=name,
            start=rng.randrange(0, 8),
            end=None,
            value=rng.randrange(0, 2),
            value_format=random_value_format(rng))
    start = rng.randrange(0, 8)
    width = rng.randrange(1, 6)
    return SetFasmFeature(
        feature=name,
        start=start,
        end=start + width - 1,
        value=rng.getrandbits(width),
        value_format=random_value_format(rng))


def random_annotations(rng):
    return [
        Annotation('n%d' % i, 'v%d' % rng.randrange(3))
        for i in range(rng.randrange(1, 3))
    ]


def random_line(rng):
    """ One random FasmLine: a (possibly annotated/commented) feature, a
    comment-only line, an annotation-only line, or a blank line. """
    choice = rng.random()
    if choice < 0.55:
        annotations = random_annotations(rng) if rng.random() < 0.2 else None
        comment = ' c%d' % rng.randrange(3) if rng.random() < 0.2 else None
        return FasmLine(
            set_feature=random_set_feature(rng),
            annotations=annotations,
            comment=comment)
    if choice < 0.7:
        return FasmLine(
            set_feature=None,
            annotations=None,
            comment=' comment %d' % rng.randrange(5))
    if choice < 0.85:
        return FasmLine(
            set_feature=None,
            annotations=random_annotations(rng),
            comment=None)
    return FasmLine(set_feature=None, annotations=None, comment=None)


def random_model(seed):
    """ A random model built from `seed`: deterministic (same seed, same
    model), reused across differential and oracle style comparisons. """
    rng = random.Random(seed)
    n = rng.randrange(1, 30)
    lines = [random_line(rng) for _ in range(n)]

    # Occasionally add an explicit duplicate/conflicting pair for the same
    # feature: two single line, unannotated, uncommented groups sharing a
    # name are eligible for merge_features (fasm/output.py); when their
    # bits conflict (one sets, the other clears the same bit) this raises
    # AssertionError in the pure Python implementation -- exercising the
    # OutputError -> decline -> Python-raises-the-same-error path (T3.3
    # point 3, docs/rewrite/DESIGN-python.md).
    if rng.random() < 0.2:
        name = rng.choice(FEATURE_NAMES)
        lines.append(
            FasmLine(
                set_feature=SetFasmFeature(name, 0, None, 1, None),
                annotations=None,
                comment=None))
        lines.append(
            FasmLine(
                set_feature=SetFasmFeature(name, 0, None, 0, None),
                annotations=None,
                comment=None))

    return lines


RANDOM_MODELS = [random_model(seed) for seed in range(300)]

# sort_key variants: int, tuple, string and a class with only __lt__
# defined (no __eq__/__gt__) -- the fast path must compare keys with a
# single PyAny::lt per pair (like CPython's own sort), never
# PyAny::compare (which would also need __eq__/__gt__ and could reject a
# pair a plain "<"-based sort accepts); see docs/rewrite/DESIGN-python.md.


class LtOnly:
    """ A key with only __lt__ (no __eq__/__gt__): default object identity
    __eq__ (never True for distinct instances), and no __gt__ at all. """

    def __init__(self, value):
        self.value = value

    def __lt__(self, other):
        return self.value < other.value

    def __repr__(self):
        return 'LtOnly(%r)' % (self.value, )


SORT_KEYS = [
    None,
    lambda name: len(name),  # int keys
    lambda name: tuple(name.split('.')),  # tuple keys
    lambda name: name[::-1],  # string keys
    lambda name: LtOnly(name),  # custom __lt__ only
]

ZERO_FUNCTIONS = [
    None,
    lambda name: False,  # never drops a group
    lambda name: 'ZERO' in name,  # sometimes drops a group
]


def sort_key_id(sort_key):
    return 'none' if sort_key is None else sort_key.__doc__ or repr(sort_key)


# Give the lambdas above readable ids in the parametrize output.
SORT_KEYS[1].__doc__ = 'len'
SORT_KEYS[2].__doc__ = 'tuple'
SORT_KEYS[3].__doc__ = 'reversed'
SORT_KEYS[4].__doc__ = 'ltonly'
ZERO_FUNCTIONS[1].__doc__ = 'never'
ZERO_FUNCTIONS[2].__doc__ = 'sometimes'


def zero_function_id(zero_function):
    return 'none' if zero_function is None else zero_function.__doc__


# ---------------------------------------------------------------------------
# Differential tests: fasm.output.merge_and_sort (fast path, falling back
# to Python when the fast path declines or is unavailable) must return the
# same lines, in the same order, as fasm.output._merge_and_sort_py (the
# pure Python implementation, also what a declining/missing fast path
# falls back to) -- or raise the same exception type.
# ---------------------------------------------------------------------------


def run_both(model, zero_function, sort_key):
    """ Returns (fast_result_or_None, py_result_or_None, exception_type). """
    try:
        fast = list(fasm.output.merge_and_sort(model, zero_function, sort_key))
    except Exception as e:
        fast_exc = type(e)
        fast = None
    else:
        fast_exc = None

    try:
        py = list(
            fasm.output._merge_and_sort_py(model, zero_function, sort_key))
    except Exception as e:
        py_exc = type(e)
        py = None
    else:
        py_exc = None

    assert fast_exc is py_exc, (fast_exc, py_exc)
    if fast_exc is None:
        assert fast == py
    return fast, py


@pytest.mark.parametrize('path', corpus_files(), ids=corpus_id)
def test_merge_and_sort_corpus(path):
    try:
        model = rust.parse_fasm_filename(path)
    except rust.FasmParseError:
        pytest.skip('does not parse')
    fast, py = run_both(model, None, None)
    if py is not None:
        # A model the pure Python implementation does not raise on is
        # always well formed enough for the fast path to run (not
        # decline) -- otherwise this would only be testing that the
        # fallback matches itself. A handful of corpus files (e.g.
        # examples/many.fasm) deliberately hit
        # fasm.output.merge_features's AssertionError (conflicting
        # merged bits, see docs/rewrite/DESIGN-output.md); those are
        # only checked for matching exception types above.
        assert _fasm_rs.merge_and_sort(model, None, None) is not None


@pytest.mark.parametrize('sort_key', SORT_KEYS, ids=sort_key_id)
@pytest.mark.parametrize('zero_function', ZERO_FUNCTIONS, ids=zero_function_id)
@pytest.mark.parametrize('path', corpus_files()[:12], ids=corpus_id)
def test_merge_and_sort_corpus_with_callbacks(path, zero_function, sort_key):
    try:
        model = rust.parse_fasm_filename(path)
    except rust.FasmParseError:
        pytest.skip('does not parse')
    run_both(model, zero_function, sort_key)


@pytest.mark.parametrize('model', RANDOM_MODELS, ids=range(len(RANDOM_MODELS)))
def test_merge_and_sort_random_models(model):
    run_both(model, None, None)


@pytest.mark.parametrize('sort_key', SORT_KEYS, ids=sort_key_id)
@pytest.mark.parametrize('zero_function', ZERO_FUNCTIONS, ids=zero_function_id)
@pytest.mark.parametrize('model', RANDOM_MODELS[:30], ids=range(30))
def test_merge_and_sort_random_models_with_callbacks(
        model, zero_function, sort_key):
    run_both(model, zero_function, sort_key)


def test_merge_and_sort_snippets():
    for text in SNIPPETS + [big_fasm(50)]:
        model = rust.parse_fasm_string(text)
        run_both(model, None, None)


# ---------------------------------------------------------------------------
# zero_function/sort_key call count and order.
# ---------------------------------------------------------------------------


def feature_line(name, annotations=None, comment=None):
    return FasmLine(
        set_feature=SetFasmFeature(name, None, None, 1, None),
        annotations=annotations,
        comment=comment)


def test_sort_key_called_once_per_group_id_not_per_line():
    # Three distinct feature groups ('A', 'B', 'C'), 'B' with two lines
    # (same group id, different full names) so its group id is only seen
    # once despite two lines sharing it.
    model = [
        feature_line('C.solo'),
        feature_line('B.b2'),
        feature_line('A.a1'),
        feature_line('B.b1'),
    ]
    calls = []

    def sort_key(name):
        calls.append(name)
        return name

    result = list(fasm.output.merge_and_sort(model, sort_key=sort_key))
    assert sorted(calls) == ['A', 'B', 'C']
    assert len(calls) == 3  # once per group id, not once per line (4)
    assert [line.set_feature.feature for line in result if line.set_feature
            ] == ['A.a1', 'B.b1', 'B.b2', 'C.solo']


def test_sort_key_called_in_first_seen_order_when_unsorted():
    # sort_key itself is called before any comparison; capture the call
    # order (not just the sorted order) via a key that also records it.
    model = [feature_line('C.x'), feature_line('A.x'), feature_line('B.x')]
    order = []

    def sort_key(name):
        order.append(name)
        return name

    list(fasm.output.merge_and_sort(model, sort_key=sort_key))
    # 'C', 'A', 'B': the order group ids are first seen in `model`, i.e.
    # Python dict insertion order of feature_groups.keys() -- matching
    # fasm.output._merge_and_sort_py (sorted(..., key=sort_key) computes
    # keys in the original iterable's order before comparing).
    assert order == ['C', 'A', 'B']


def test_zero_function_short_circuits_like_python_all():
    # A group with two features: zero_function returns False for the
    # first (in flattened/sorted-by-full-name order) so `all(...)` stops
    # there and never calls it for the second.
    model = [feature_line('B.b2'), feature_line('B.b1')]
    calls = []

    def zero_function(name):
        calls.append(name)
        return name != 'B.b1'  # False for 'B.b1' (sorts before 'B.b2')

    result = list(
        fasm.output.merge_and_sort(model, zero_function=zero_function))
    assert calls == ['B.b1']
    assert len(result) == 2  # not dropped: not all zero


def test_zero_function_called_for_every_feature_when_not_short_circuited():
    model = [feature_line('B.b2'), feature_line('B.b1'), feature_line('A.a')]
    calls = []

    def zero_function(name):
        calls.append(name)
        return True  # never short circuits: always "zero"

    result = list(
        fasm.output.merge_and_sort(model, zero_function=zero_function))
    # Group ids sort before zero_function is ever called ('A' < 'B', no
    # sort_key given): 'A.a' first, then 'B.b1'/'B.b2' (flattened group,
    # sorted by full feature name).
    assert calls == ['A.a', 'B.b1', 'B.b2']
    assert result == []  # every group was all-zero: dropped


def test_zero_function_and_sort_key_call_order_matches_python():
    """ sort_key runs to completion (once per group id) before
    zero_function is called for the first group, matching
    fasm.output._merge_and_sort_py's output_sorted_lines (sorts
    group_ids first, then iterates them calling zero_function per
    group). """
    model = [feature_line('B.x'), feature_line('A.x'), feature_line('C.x')]
    calls = []

    def sort_key(name):
        calls.append(('sort_key', name))
        return name

    def zero_function(name):
        calls.append(('zero_function', name))
        return False

    list(
        fasm.output.merge_and_sort(
            model, zero_function=zero_function, sort_key=sort_key))
    assert calls == [
        ('sort_key', 'B'),
        ('sort_key', 'A'),
        ('sort_key', 'C'),
        ('zero_function', 'A.x'),
        ('zero_function', 'B.x'),
        ('zero_function', 'C.x'),
    ]


@pytest.mark.parametrize('which', ['zero_function', 'sort_key'])
def test_callback_exception_propagates_directly(which):
    """ Once zero_function/sort_key has been called, an exception it
    raises propagates directly out of merge_and_sort: falling back to
    Python at that point would call the same callable again and
    duplicate the (call-recording) side effect. """
    model = [feature_line('A.a'), feature_line('B.b')]
    calls = []

    def raising(name):
        calls.append(name)
        raise ValueError('boom: ' + name)

    kwargs = {which: raising}
    with pytest.raises(ValueError, match='boom'):
        list(fasm.output.merge_and_sort(model, **kwargs))
    assert calls == [calls[0]]  # called exactly once, not retried


# ---------------------------------------------------------------------------
# Type identity: the fast path returns fasm.model's own namedtuple
# classes, like the Rust parser and fasm_tuple_to_string's fast path do
# (rust/fasm-python/src/convert.rs's PyModel).
# ---------------------------------------------------------------------------


def test_merge_and_sort_returns_exact_namedtuple_types():
    model = [
        feature_line('A.a', annotations=[Annotation('n', 'v')], comment=' c'),
        FasmLine(set_feature=None, annotations=None, comment=' only comment'),
    ]
    assert _fasm_rs.merge_and_sort(model, None, None) is not None
    result = list(fasm.output.merge_and_sort(model))
    assert result
    for line in result:
        assert type(line) is FasmLine
        if line.set_feature is not None:
            assert type(line.set_feature) is SetFasmFeature
        if line.annotations:
            for annotation in line.annotations:
                assert type(annotation) is Annotation


def test_merge_and_sort_always_returns_an_iterator():
    model = [feature_line('A.a')]
    # Fast path taken (model is well formed).
    result = fasm.output.merge_and_sort(model)
    assert iter(result) is result
    assert list(result) == list(fasm.output._merge_and_sort_py(model))
    # Fallback (extension unavailable): still an iterator (a generator).
    result = fasm.output._merge_and_sort_py(model)
    assert iter(result) is result


# ---------------------------------------------------------------------------
# Wiring: fasm.fasm_tuple_to_string and fasm.output.merge_and_sort call the
# fast path when it is available, use its result when not None, and fall
# back to the pure Python implementation when it returns None or the
# extension is unavailable (fasm._fasm_rs is None).
# ---------------------------------------------------------------------------


def test_fasm_tuple_to_string_uses_fast_path_result(monkeypatch):
    calls = []

    def fake(model, canonical=False):
        calls.append((model, canonical))
        return 'FAST PATH RESULT\n'

    monkeypatch.setattr(_fasm_rs, 'fasm_tuple_to_string', fake)
    model = []
    assert fasm.fasm_tuple_to_string(model, canonical=True) == \
        'FAST PATH RESULT\n'
    assert calls == [(model, True)]


def test_fasm_tuple_to_string_falls_back_when_fast_path_declines(monkeypatch):
    calls = []

    def fake(model, canonical=False):
        calls.append((model, canonical))
        return None

    monkeypatch.setattr(_fasm_rs, 'fasm_tuple_to_string', fake)
    model = [FasmLine(set_feature=None, annotations=None, comment=' x')]
    assert fasm.fasm_tuple_to_string(model) == '# x\n'
    assert calls == [(model, False)]


def test_fasm_tuple_to_string_falls_back_when_extension_missing(monkeypatch):
    monkeypatch.setattr(fasm, '_fasm_rs', None)
    model = [FasmLine(set_feature=None, annotations=None, comment=' x')]
    assert fasm.fasm_tuple_to_string(model) == '# x\n'


def test_merge_and_sort_uses_fast_path_result(monkeypatch):
    calls = []

    def fake(model, zero_function, sort_key):
        calls.append((model, zero_function, sort_key))
        return []

    monkeypatch.setattr(_fasm_rs, 'merge_and_sort', fake)
    model = []
    result = fasm.output.merge_and_sort(model)
    assert list(result) == []
    assert calls == [(model, None, None)]


def test_merge_and_sort_falls_back_when_fast_path_declines(monkeypatch):
    calls = []

    def fake(model, zero_function, sort_key):
        calls.append((model, zero_function, sort_key))
        return None

    monkeypatch.setattr(_fasm_rs, 'merge_and_sort', fake)
    model = [feature_line('A.a')]
    result = list(fasm.output.merge_and_sort(model))
    assert calls == [(model, None, None)]
    assert result == list(fasm.output._merge_and_sort_py(model))


def test_merge_and_sort_falls_back_when_extension_missing(monkeypatch):
    monkeypatch.setattr(fasm.output, '_fasm_rs', None)
    model = [feature_line('A.a')]
    result = list(fasm.output.merge_and_sort(model))
    assert result == list(fasm.output._merge_and_sort_py(model))

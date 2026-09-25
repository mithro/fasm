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
""" Audits fasm/xilinx/__init__.pyi against the runtime API (T8.3).

Introspects the public names of fasm, fasm.xilinx and fasm._fasm_rs (the
compiled extension module) and compares them, and the signatures of the
plain Python fasm.xilinx functions and the parameter *names* of the
extension's classes' methods (via ``inspect.signature``, which pyo3
supports through each function's ``text_signature``), against what
fasm/xilinx/__init__.pyi declares. This is the only .pyi in the
repository (fasm's other modules have no separate stub); it exists to
keep it from silently drifting from the extension it describes.

Skipped when the fasm._fasm_rs extension (or its "xilinx" feature) is
not built, since there is then nothing to compare the stub against
(`maturin develop --release`, or a full `pip install .`, builds it; see
docs/PYTHON.md "Install").

Note: dir(fasm) is broken by a pre-existing, unrelated bug (fasm/__init__.py
assigns a plain string to the module-level name ``__dir__``, which Python's
dir() protocol then tries to call as fasm.__dir__()); this module therefore
inspects modules with ``vars(...)`` instead of ``dir(...)`` throughout,
which does not trigger that protocol.
"""

import ast
import inspect
import os
import types

import pytest

try:
    import fasm
    import fasm._fasm_rs
    import fasm.xilinx as xilinx
except ImportError as e:
    pytest.skip(str(e), allow_module_level=True)

_HERE = os.path.dirname(os.path.abspath(__file__))
_STUB_PATH = os.path.join(
    os.path.dirname(_HERE), 'fasm', 'xilinx', '__init__.pyi')


def _public_names(module):
    """ Public (non-underscore-prefixed) top level names bound in module.

    Excludes names bound to other modules (e.g. an ``import os`` at the
    top of the file): those are implementation details of how the module
    is written, not part of its API, and a .pyi stub is not expected to
    re-declare them.
    """
    return {
        n
        for n, v in vars(module).items()
        if not n.startswith('_') and not isinstance(v, types.ModuleType)
    }


# Names collections.abc.Mapping provides for free from __getitem__/
# __iter__/__len__ alone (`get`, `__contains__`, `__eq__`, `__ne__`).
# fasm.xilinx.Frames's stub declares it as a Mapping[int, List[int]]
# subclass, so these are present at runtime (the extension implements
# some of them directly rather than relying on the mixin, which is an
# implementation detail) without needing their own stub entry; comparing
# them directly against the stub's explicit method list would be a false
# mismatch.
_MAPPING_MIXIN_NAMES = frozenset({'get'})


def _parse_stub():
    """ Parse fasm/xilinx/__init__.pyi; return its ast.Module. """
    with open(_STUB_PATH) as f:
        return ast.parse(f.read(), filename=_STUB_PATH)


def _stub_all(tree):
    """ The literal list assigned to __all__ in the stub, if any. """
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign):
            if any(isinstance(t, ast.Name) and t.id == '__all__'
                   for t in node.targets):
                return [
                    elt.value
                    for elt in node.value.elts
                    if isinstance(elt, ast.Constant)
                ]
    return None


def _stub_top_level_names(tree):
    """ Names the stub binds at module scope: class/function defs and
    imported-and-reexported names (``X as X``), which is how the
    exception classes are re-exported from fasm.xilinx._types. """
    names = set()
    for node in tree.body:
        if isinstance(node, (ast.ClassDef, ast.FunctionDef)):
            names.add(node.name)
        elif isinstance(node, ast.ImportFrom):
            for alias in node.names:
                if alias.asname is not None and alias.asname == alias.name:
                    names.add(alias.asname)
        elif isinstance(node, ast.AnnAssign) \
                and isinstance(node.target, ast.Name):
            names.add(node.target.id)
    return names


def _stub_class_methods(tree, class_name):
    """ Method/property names declared on a class in the stub. """
    for node in tree.body:
        if isinstance(node, ast.ClassDef) and node.name == class_name:
            return {
                n.name
                for n in node.body
                if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))
                and not (n.name.startswith('__') and n.name.endswith('__'))
            }
    raise AssertionError(
        'class {!r} not found in {}'.format(class_name, _STUB_PATH))


def _stub_function_params(tree, name, in_class=None):
    """ Positional/keyword parameter names of a function/method in the
    stub, in declaration order (self/cls dropped). """
    body = tree.body
    if in_class is not None:
        for node in body:
            if isinstance(node, ast.ClassDef) and node.name == in_class:
                body = node.body
                break
        else:
            raise AssertionError(
                'class {!r} not found in {}'.format(in_class, _STUB_PATH))

    for node in body:
        if isinstance(node, ast.FunctionDef) and node.name == name:
            args = node.args
            params = [a.arg for a in args.posonlyargs]
            params += [a.arg for a in args.args]
            params += [a.arg for a in args.kwonlyargs]
            if params and params[0] in ('self', 'cls'):
                params = params[1:]
            return params
    raise AssertionError(
        'function {!r} not found in {} (class {!r})'.format(
            name, _STUB_PATH, in_class))


def _runtime_params(func):
    """ Parameter names of a runtime callable, self/cls dropped. """
    params = list(inspect.signature(func).parameters)
    if params and params[0] in ('self', 'cls'):
        params = params[1:]
    return params


@pytest.fixture(scope='module')
def stub_tree():
    return _parse_stub()


def test_fasm_xilinx_module_names_match_stub_all(stub_tree):
    """ fasm.xilinx's public runtime names == the stub's __all__. """
    stub_all = _stub_all(stub_tree)
    assert stub_all is not None, '__all__ not found in ' + _STUB_PATH
    runtime_names = _public_names(xilinx)
    assert set(stub_all) == runtime_names, (
        'fasm.xilinx.__all__ (stub) vs runtime public names differ: '
        'stub only={}, runtime only={}'.format(
            sorted(set(stub_all) - runtime_names),
            sorted(runtime_names - set(stub_all))))
    # __all__ itself should have no duplicates and match the stub's own
    # top level class/function/re-export names.
    assert len(stub_all) == len(set(stub_all)), '__all__ has duplicates'
    assert set(stub_all) <= _stub_top_level_names(stub_tree), (
        '__all__ names not actually declared/imported in the stub: {}'.format(
            sorted(set(stub_all) - _stub_top_level_names(stub_tree))))


@pytest.mark.parametrize(
    'cls_name,runtime_cls', [
        ('Database', xilinx.Database),
        ('Frames', xilinx.Frames),
        ('FasmAssembler', xilinx.FasmAssembler),
    ])
def test_class_methods_match_stub(stub_tree, cls_name, runtime_cls):
    """ Public methods/properties of each xilinx class == the stub's. """
    stub_methods = _stub_class_methods(stub_tree, cls_name)
    runtime_methods = _public_names(runtime_cls) - _MAPPING_MIXIN_NAMES
    assert stub_methods == runtime_methods, (
        '{}: stub vs runtime methods differ: stub only={}, '
        'runtime only={}'.format(
            cls_name, sorted(stub_methods - runtime_methods),
            sorted(runtime_methods - stub_methods)))


@pytest.mark.parametrize(
    'name', [
        'write_bitstream',
        'read_bitstream',
        'read_roi_design',
        'dump_frames_sparse',
        'fasm2frames',
        'fasm2bit',
    ])
def test_module_function_params_match_stub(stub_tree, name):
    """ Parameter names/order of each fasm.xilinx function == the stub's. """
    stub_params = _stub_function_params(stub_tree, name)
    runtime_params = _runtime_params(getattr(xilinx, name))
    assert stub_params == runtime_params, (
        '{}: stub params {} != runtime params {}'.format(
            name, stub_params, runtime_params))


@pytest.mark.parametrize(
    'cls_name,method,runtime_cls', [
        ('Database', 'open', xilinx.Database),
        ('Database', 'tile_type_features', xilinx.Database),
        ('Database', 'pseudo_pips', xilinx.Database),
        ('Database', 'lookup_feature', xilinx.Database),
        ('FasmAssembler', 'parse_fasm_filename', xilinx.FasmAssembler),
        ('FasmAssembler', 'add_fasm_line', xilinx.FasmAssembler),
        ('FasmAssembler', 'mark_roi_frames', xilinx.FasmAssembler),
        ('FasmAssembler', 'set_feature_callback', xilinx.FasmAssembler),
        ('FasmAssembler', 'get_frames', xilinx.FasmAssembler),
        ('Frames', 'frame_bytes', xilinx.Frames),
        ('Frames', 'write_frm', xilinx.Frames),
        ('Frames', 'from_frm', xilinx.Frames),
        ('Frames', 'read_frm', xilinx.Frames),
    ])
def test_method_params_match_stub(stub_tree, cls_name, method, runtime_cls):
    """ Parameter names/order of a sample of extension methods == stub's.

    (Covers every method whose stub signature has more than a bare
    ``self``; property/no-argument methods are already covered by
    test_class_methods_match_stub above.)
    """
    stub_params = _stub_function_params(stub_tree, method, in_class=cls_name)
    runtime_params = _runtime_params(getattr(runtime_cls, method))
    assert stub_params == runtime_params, (
        '{}.{}: stub params {} != runtime params {}'.format(
            cls_name, method, stub_params, runtime_params))


def test_namedtuple_fields_match_stub():
    """ Tile/Roi/RoiDesign/FeatureBits fields == their stub declarations. """
    tree = _parse_stub()
    for name, runtime_type in [
        ('Tile', xilinx.Tile),
        ('Roi', xilinx.Roi),
        ('RoiDesign', xilinx.RoiDesign),
        ('FeatureBits', xilinx.FeatureBits),
    ]:
        for node in tree.body:
            if isinstance(node, ast.ClassDef) and node.name == name:
                stub_fields = [
                    n.target.id
                    for n in node.body
                    if isinstance(n, ast.AnnAssign)
                    and isinstance(n.target, ast.Name)
                ]
                break
        else:
            raise AssertionError('class {!r} not in stub'.format(name))
        assert stub_fields == list(runtime_type._fields), (
            '{}: stub fields {} != runtime fields {}'.format(
                name, stub_fields, list(runtime_type._fields)))


def test_fasm_rs_xilinx_submodule_present():
    """ fasm._fasm_rs.xilinx exists (the extension backing fasm.xilinx). """
    assert hasattr(fasm._fasm_rs, 'xilinx')

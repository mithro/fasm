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
"""Prints how often each documented difference of test_cli_compat.py's
`normalise()` was applied, and gives the Rust tools a private binary
database cache."""
import os
import sys

import pytest


@pytest.fixture(scope='session', autouse=True)
def xdb_cache(tmp_path_factory):
    """`FASM_XDB_CACHE` (the Rust fasm2frames/xcfasm database cache, see
    docs/rewrite/DESIGN-xilinx-db.md §8.8) in a temporary directory of
    this run unless it is set: the cached path stays covered (written by
    the first runs, loaded by the others) without writing into
    ~/.cache."""
    if 'FASM_XDB_CACHE' in os.environ:
        yield os.environ['FASM_XDB_CACHE']
        return
    path = str(tmp_path_factory.mktemp('xdb-cache'))
    os.environ['FASM_XDB_CACHE'] = path
    try:
        yield path
    finally:
        os.environ.pop('FASM_XDB_CACHE', None)


def pytest_terminal_summary(terminalreporter):
    module = sys.modules.get('test_cli_compat')
    counts = getattr(module, 'RULE_COUNTS', None)
    if not counts:
        return
    terminalreporter.section('fasm CLI differential test')
    for rule, count in sorted(counts.items()):
        terminalreporter.write_line(
            'normalisation rule {}: applied {} time(s)'.format(rule, count))

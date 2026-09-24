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
`normalise()` was applied."""
import sys


def pytest_terminal_summary(terminalreporter):
    module = sys.modules.get('test_cli_compat')
    counts = getattr(module, 'RULE_COUNTS', None)
    if not counts:
        return
    terminalreporter.section('fasm CLI differential test')
    for rule, count in sorted(counts.items()):
        terminalreporter.write_line(
            'normalisation rule {}: applied {} time(s)'.format(rule, count))

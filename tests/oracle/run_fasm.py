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
"""Run the oracle: the original (pre-Rust-rewrite) `fasm` command line tool.

This is a thin wrapper around `fasm.tool.main()` from the `fasm` package
installed in tests/oracle/venv by tests/oracle/setup.sh. It must be run with
that venv's interpreter, e.g.:

    tests/oracle/venv/bin/python tests/oracle/run_fasm.py --canonical file.fasm

or via the tests/oracle/fasm-oracle wrapper script, which does exactly that:

    tests/oracle/fasm-oracle --canonical file.fasm

Its command line, stdout/stderr behaviour and exit code are exactly those of
the original `fasm` console_script entry point (`fasm=fasm.tool:main`), since
that is what a real `pip install`-ed `fasm` script does: `sys.exit(main())`.
"""
import sys

from fasm.tool import main

if __name__ == '__main__':
    sys.exit(main())

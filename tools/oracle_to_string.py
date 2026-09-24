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
"""Prints ``fasm.fasm_tuple_to_string(fasm.parse_fasm_filename(FILE),
canonical)`` to stdout, using whatever parser implementation the oracle
venv this is run with (`tests/oracle/venv/bin/python -P`) has available.
The oracle-side half of the `fasm-dump --to-string [--canonical]`
comparison in `tools/difftest.py` (T1.5): the equivalent of the
`python -c` snippet mentioned in `docs/rewrite/TASKS.md`, as an actual
file so it does not depend on shell quoting.

On any exception, prints its message to stderr and exits 1 (mirroring
`fasm-dump --to-string`'s behaviour on a parse error).
"""
import argparse
import sys

import fasm


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('file')
    parser.add_argument('--canonical', action='store_true')
    args = parser.parse_args(argv)

    try:
        model = list(fasm.parse_fasm_filename(args.file))
        text = fasm.fasm_tuple_to_string(model, canonical=args.canonical)
    except Exception as e:  # noqa: BLE001 - reported, not swallowed
        sys.stderr.write(str(e))
        return 1

    sys.stdout.write(text)
    return 0


if __name__ == '__main__':
    sys.exit(main())

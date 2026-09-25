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
"""Runs a python-fpga-interchange command line module with pycapnp 1.3.

    pfi_run.py MODULE ARGS...     (e.g. fasm_generator, patch)

python-fpga-interchange 0.0.18 pins pycapnp 1.1.0, which does not build
with Cython 3; in pycapnp 1.3 `from_bytes` returns a context manager
instead of the message. This enters it (the message stays alive for the
process), which is all the difference the FASM generator and the device
patching need (tools/e2e/setup-rapidwright.sh --with-interchange).
"""
import importlib
import sys

import fpga_interchange.interchange_capnp as interchange_capnp

_read_capnp_file = interchange_capnp.read_capnp_file


def read_capnp_file(*args, **kwargs):
    message = _read_capnp_file(*args, **kwargs)
    if hasattr(message, '__enter__') and not hasattr(message, 'which'):
        message = message.__enter__()
    return message


def main():
    interchange_capnp.read_capnp_file = read_capnp_file
    module = importlib.import_module('fpga_interchange.' + sys.argv[1])
    if hasattr(module, 'read_capnp_file'):
        module.read_capnp_file = read_capnp_file
    sys.argv = [sys.argv[1]] + sys.argv[2:]
    module.main()


if __name__ == '__main__':
    main()

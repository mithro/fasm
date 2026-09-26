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
"""One place that resolves the openXC7 snap's own bundled prjxray-db
(T5.8b): a DIFFERENT, independently pinned copy of Project X-Ray from the
"prjxray" family `tools/fetch-db.sh` fetches for the rest of this repo's
Xilinx differential tests -- see tools/e2e/README.md, "A note on prjxray-db
provenance", and tools/fetch-db.sh's own "openxc7" usage comment.

Two places may hold it, checked in this order (first match wins) so tests
work whichever one was set up:

1. `tools/fetch-db.sh openxc7 <family>`'s lean, checksummed cache:
   `$FASM_DB_CACHE/prjxray-db-openxc7/<family>` -- just the db, no
   toolchain install.
2. `tools/e2e/setup-openxc7.sh`'s full extraction of the snap:
   `$OPENXC7_E2E_BUILD (default tools/e2e/build)/openxc7/root/opt/
   nextpnr-xilinx/external/prjxray-db/<family>` -- present once the whole
   end-to-end toolchain has been installed.

Used by tests/e2e/test_fpgas_online.py and tests/e2e/test_nextpnr_examples.py
(grep for `snap_prjxray_db` there); import it as
`sys.path.insert(0, str(REPO_ROOT / 'tools' / 'e2e'))` then
`import snap_prjxray_db` (there is no package __init__.py under tools/e2e,
by design -- these are standalone scripts, not an importable package).
"""
import os
from pathlib import Path

FAMILIES = ('artix7', 'kintex7', 'spartan7', 'zynq7')


def _repo_root():
    return Path(__file__).resolve().parent.parent.parent


def _fetch_db_cache(repo_root=None):
    """$FASM_DB_CACHE (default tests/oracle/build/db), same default as
    tools/fetch-db.sh."""
    repo_root = repo_root or _repo_root()
    return Path(
        os.environ.get('FASM_DB_CACHE',
                       repo_root / 'tests' / 'oracle' / 'build' / 'db'))


def _e2e_build(repo_root=None):
    repo_root = repo_root or _repo_root()
    default = repo_root / 'tools' / 'e2e' / 'build'
    return Path(os.environ.get('OPENXC7_E2E_BUILD', default))


def lean_cache_root(repo_root=None):
    """`tools/fetch-db.sh openxc7`'s cache directory (may not exist)."""
    return _fetch_db_cache(repo_root) / 'prjxray-db-openxc7'


def full_snap_prjxray_db(repo_root=None):
    """`tools/e2e/setup-openxc7.sh`'s extracted snap's bundled prjxray-db
    directory (may not exist)."""
    return _e2e_build(repo_root).joinpath(
        'openxc7', 'root', 'opt', 'nextpnr-xilinx', 'external', 'prjxray-db')


def db_root(family, repo_root=None):
    """The directory for one family (what fasm2frames/uray-fasm2frames
    take as `--db-root`), or None if neither source has it."""
    lean = lean_cache_root(repo_root) / family
    if lean.is_dir():
        return lean
    full = full_snap_prjxray_db(repo_root) / family
    if full.is_dir():
        return full
    return None


def _ensure_shim(lean):
    """Idempotently makes `lean.parent/.openxc7-db-cache/prjxray-db` a
    symlink to `lean`, so that shim directory is shaped the way
    `tools/difftest-xilinx.py --db-cache` expects. Returns the shim
    directory, or None if the symlink cannot be made safely (e.g. a real
    directory already sits at that exact path -- never touched)."""
    shim = lean.parent / '.openxc7-db-cache'
    link = shim / 'prjxray-db'
    try:
        if link.is_symlink():
            if os.readlink(link) != str(lean):
                link.unlink()
                link.symlink_to(lean)
        elif link.exists():
            # A real file/directory already occupies the shim path (not
            # one this function created) -- leave it alone rather than
            # deleting someone else's directory out from under them.
            return None
        else:
            shim.mkdir(parents=True, exist_ok=True)
            link.symlink_to(lean)
    except OSError:
        return None
    return shim


def db_cache(repo_root=None):
    """A directory shaped as `tools/difftest-xilinx.py --db-cache` expects
    (a directory whose child `prjxray-db/<family>` exists), or None if
    neither source has anything. Prefers the lean `tools/fetch-db.sh
    openxc7` cache (same order as `db_root()` above), creating
    (idempotently) a small directory under it holding a `prjxray-db`
    symlink pointing at it, since the lean cache's top-level directory is
    itself named `prjxray-db-openxc7`, not `prjxray-db`. Falls back to
    `tools/e2e/setup-openxc7.sh`'s full extraction (already shaped this
    way, no extra step needed) when the lean cache is absent or its shim
    cannot be created."""
    lean = lean_cache_root(repo_root)
    if lean.is_dir():
        shim = _ensure_shim(lean)
        if shim is not None:
            return shim
    full = full_snap_prjxray_db(repo_root)
    if full.is_dir():
        return full.parent  # .../external, whose child IS "prjxray-db"
    return None


if __name__ == '__main__':
    import sys
    for fam in FAMILIES:
        root = db_root(fam)
        print('%-10s %s' % (fam, root if root else '(not found)'))
    cache = db_cache()
    print('db_cache:  %s' % (cache if cache else '(not found)'))
    sys.exit(0)

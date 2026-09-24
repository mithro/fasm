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
"""Fast check of the Rust `fasm2frames` on the generated corpus of one
part against golden results of the reference (T5.9).

`tools/gen-xilinx-corpus.py --tiles sample 3` writes the synthetic corpus
of xc7a35tcsg324-1 (every segbits feature and pseudo PIP of the part, ~24
files; `tools/difftest-xilinx.py --family artix7 --all-parts` is the full
comparison), and the Rust `fasm2frames` runs on every file (the first
`features.fasm` dense and `--sparse`, the others `--sparse`), with the
binary database cache (`FASM_XDB_CACHE`, written by the first run) and
without it (`FASM_XDB_CACHE=0`). The exit code, the SHA-256 of the `.frm`
and stderr (normalised like `tools/difftest-xilinx.py`: without the
reference's traceback, parse error messages up to the position, value
range errors of the ANTLR parser as one token) must equal the reference's,
recorded in `GOLDEN` together with the SHA-256 of every generated file
(so a changed generator is reported as such, not as a Rust bug) and the
versions of the reference tools and the database.

Skipped without the prjxray-db artix7 database, the Rust binary or the
golden file. To (re)write the golden file with the reference tools
(`tests/oracle/setup-xilinx.sh`):

    python3 tests/cli/test_xilinx_corpus.py --write-goldens

`FASM_DB_CACHE` selects the database directory, `FASM2FRAMES_RUST` the
Rust binary, `FASM2FRAMES_ORACLE` the reference and `XILINX_CORPUS_GOLDEN`
the golden file.
"""
import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PART = 'xc7a35tcsg324-1'
TILES = ['sample', '3']
SEED = '0'
DEFAULT_GOLDEN = ROOT.joinpath('tests', 'corpus', 'xilinx', 'artix7',
                               'generated',
                               '{}-sample-3-s0.json'.format(PART))
GOLDEN = Path(os.environ.get('XILINX_CORPUS_GOLDEN', DEFAULT_GOLDEN))
# Longer normalised stderr texts are kept as a hash.
STDERR_LIMIT = 2000
GENERATOR = ROOT / 'tools' / 'gen-xilinx-corpus.py'
RUST = Path(
    os.environ.get('FASM2FRAMES_RUST',
                   ROOT / 'target' / 'release' / 'fasm2frames'))
ORACLE = Path(
    os.environ.get('FASM2FRAMES_ORACLE',
                   ROOT / 'tests' / 'oracle' / 'fasm2frames-oracle'))


def load_difftest():
    spec = importlib.util.spec_from_file_location(
        'difftest_xilinx', str(ROOT / 'tools' / 'difftest-xilinx.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def find_db():
    for base in (os.environ.get('FASM_DB_CACHE'),
                 ORACLE.parent.joinpath('build', 'db'),
                 ROOT.joinpath('tests', 'oracle', 'build', 'db')):
        if base and (Path(base) / 'prjxray-db' / 'artix7').is_dir():
            return Path(base) / 'prjxray-db' / 'artix7'
    return None


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def generate(db, out_dir):
    """Generates the corpus; returns (manifest, {relative path: sha256})."""
    argv = [
        sys.executable,
        str(GENERATOR), '--db-root',
        str(db), '--part', PART, '--out-dir',
        str(out_dir), '--tiles'
    ]
    subprocess.run(argv + TILES + ['--seed', SEED],
                   check=True,
                   capture_output=True)
    manifest = json.loads((out_dir / 'manifest.json').read_text())
    files = {}
    for name in manifest['files']:
        files[name] = sha256((out_dir / name).read_bytes())
    for name in manifest['errors']:
        data = (out_dir / 'errors' / name).read_bytes()
        files['errors/' + name] = sha256(data)
    return manifest, files


def cases(manifest):
    """(name, relative path, flags)."""
    out = []
    first = manifest['files'][0]
    out.append((first + '[dense]', first, []))
    out.append((first + '[sparse]', first, ['--sparse']))
    for name in manifest['files'][1:]:
        out.append((name + '[sparse]', name, ['--sparse']))
    for name in manifest['errors']:
        out.append(('errors/%s[sparse]' % name, 'errors/' + name,
                    ['--sparse']))
    return out


def normalised_stderr(difftest, stderr):
    """stderr as tools/difftest-xilinx.py compares it (rules 1, 2, 4)."""
    rules = dict.fromkeys(difftest.RULES, 0)
    stderr = difftest.strip_traceback(stderr)
    if (difftest.CTYPES_MARKER in stderr
            and stderr.rstrip('\n').endswith(difftest.NONE_TYPE)):
        return '<value range error>'
    if difftest.PARSE_ERROR_RE.match(stderr) and 'does not fit' in stderr:
        # The Rust side of a value range error.
        return '<value range error>'
    stderr = difftest.normalise(stderr, rules)
    if len(stderr) > STDERR_LIMIT:
        return 'sha256:' + sha256(stderr.encode('utf-8', 'surrogateescape'))
    return stderr


def run_tool(tool, db, corpus_dir, rel, flags, out, env=None):
    full_env = dict(os.environ)
    full_env.update(env or {})
    argv = [str(tool), '--db-root', str(db), '--part', PART] + flags
    result = subprocess.run(argv + [str(corpus_dir / rel), str(out)],
                            env=full_env,
                            stdin=subprocess.DEVNULL,
                            capture_output=True,
                            timeout=600)
    frm = out.read_bytes() if out.exists() else b''
    if out.exists():
        out.unlink()
    return (result.returncode, sha256(frm),
            result.stderr.decode('utf-8', 'surrogateescape'))


def reference_versions():
    versions = {}
    status = ORACLE.parent / 'build' / 'status.json'
    if status.exists():
        data = json.loads(status.read_text())
        for key in ('prjxray_commit', 'f4pga_xc_fasm_commit'):
            if key in data:
                versions[key] = data[key]
    return versions


def db_commit(db):
    head = db.parent / '.git' / 'HEAD'
    return head.read_text().strip() if head.exists() else 'unknown'


def write_goldens():
    db = find_db()
    if db is None:
        sys.exit('prjxray-db artix7 not found (tools/fetch-db.sh prjxray '
                 'artix7)')
    difftest = load_difftest()
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        manifest, files = generate(db, tmp / 'corpus')
        runs = {}
        for name, rel, flags in cases(manifest):
            code, frm, stderr = run_tool(ORACLE, db, tmp / 'corpus', rel,
                                         flags, tmp / 'out.frm')
            runs[name] = {
                'exit_code': code,
                'frm_sha256': frm,
                'stderr': normalised_stderr(difftest, stderr),
            }
            print('%s: exit %d' % (name, code))
    golden = {
        'comment': ('Reference results (tests/oracle/fasm2frames-oracle) '
                    'for the corpus of tools/gen-xilinx-corpus.py; written '
                    'by python3 tests/cli/test_xilinx_corpus.py '
                    '--write-goldens'),
        'reference': reference_versions(),
        'prjxray_db_commit': db_commit(db),
        'part': PART,
        'generator': {
            'tiles': TILES,
            'seed': SEED,
            'generator_version': manifest['generator_version'],
        },
        'files_sha256': files,
        'runs': runs,
    }
    GOLDEN.parent.mkdir(parents=True, exist_ok=True)
    GOLDEN.write_text(json.dumps(golden, indent=1, sort_keys=True) + '\n')
    print('wrote %s' % GOLDEN)


if __name__ == '__main__':
    if sys.argv[1:] == ['--write-goldens']:
        write_goldens()
        sys.exit(0)
    sys.exit('usage: %s --write-goldens (or run it with pytest)' %
             sys.argv[0])

import pytest  # noqa: E402

DB = find_db()
if DB is None:
    pytest.skip('prjxray-db artix7 not fetched (tools/fetch-db.sh prjxray '
                'artix7)',
                allow_module_level=True)
if not RUST.exists():
    pytest.skip('Rust fasm2frames missing (cargo build --release -p '
                'fasm-cli): {}'.format(RUST),
                allow_module_level=True)
if not GOLDEN.exists():
    pytest.skip('{} missing: python3 tests/cli/test_xilinx_corpus.py '
                '--write-goldens (needs tests/oracle/setup-xilinx.sh)'.format(
                    GOLDEN),
                allow_module_level=True)


@pytest.fixture(scope='module')
def corpus(tmp_path_factory):
    out = tmp_path_factory.mktemp('xilinx-corpus')
    manifest, files = generate(DB, out)
    return out, manifest, files


@pytest.fixture(scope='module')
def golden():
    return json.loads(GOLDEN.read_text())


def test_golden_header(golden):
    assert golden['part'] == PART
    assert golden['generator']['tiles'] == TILES
    assert golden['reference'], 'no reference tool versions recorded'


def test_generated_files_match_golden(corpus, golden):
    """The generator (and the database) made the files the goldens were
    made from; otherwise rewrite the goldens."""
    _, _, files = corpus
    assert files == golden['files_sha256'], (
        'generated corpus differs from the golden one: regenerate with '
        'python3 tests/cli/test_xilinx_corpus.py --write-goldens')


def test_covers_every_feature(corpus):
    _, manifest, _ = corpus
    assert manifest['features_placed'] >= manifest['features_total']
    reasons = set(u[3] for u in manifest['uncovered'])
    assert reasons <= {'STEPDOWN feature, no bonded tile'}, reasons
    assert len(manifest['errors']) >= 4


@pytest.mark.parametrize('xdb', ['cache', 'no-cache'])
def test_rust_matches_golden(corpus, golden, tmp_path, xdb):
    corpus_dir, manifest, _ = corpus
    difftest = load_difftest()
    env = {'FASM_XDB_CACHE': '0'}
    if xdb == 'cache':
        env = {'FASM_XDB_CACHE': str(tmp_path / 'xdb')}
    names = [c[0] for c in cases(manifest)]
    assert sorted(names) == sorted(golden['runs'])
    problems = []
    for name, rel, flags in cases(manifest):
        code, frm, stderr = run_tool(RUST, DB, corpus_dir, rel, flags,
                                     tmp_path / 'out.frm', env)
        want = golden['runs'][name]
        got = {
            'exit_code': code,
            'frm_sha256': frm,
            'stderr': normalised_stderr(difftest, stderr),
        }
        if got != want:
            problems.append('%s:\n  golden %r\n  rust   %r' %
                            (name, want, got))
    assert not problems, '\n'.join(problems)
    if xdb == 'cache':
        assert list((tmp_path / 'xdb').glob('*.fasmxdb'))

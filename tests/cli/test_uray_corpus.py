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
"""Fast check of the Rust `uray-fasm2frames` and `fasm2frames` on the
generated every-feature corpus of one prjuray-db part against golden
results of the reference (T6.3).

`tools/gen-xilinx-corpus.py --tiles sample 3` writes the synthetic corpus
of xczu3eg-sfvc784-1-e (every segbits feature of the 27 tile types with
segbits; `tools/difftest-xilinx.py --prjuray` is the full comparison), and
the Rust `uray-fasm2frames` runs on every file (the first `features.fasm`
dense and `--sparse`, the others `--sparse`), with the binary database
cache (`FASM_XDB_CACHE`) and without it. The exit code, the SHA-256 of
the `.frm` (16-bit words) and stderr (normalised like
`tools/difftest-xilinx.py`) must equal those of prjuray's
`utils/fasm2frames.py` recorded in `GOLDEN`, and for the runs that
succeed the Rust `fasm2frames` (32-bit words, what `xcframes2bit` reads)
must write the reference `.frm` converted to 32-bit words. `GOLDEN` also
records the SHA-256 of every generated file (a changed generator is
reported as such, not as a Rust bug) and the commits of the reference
tools and of prjuray-db.

Skipped without the prjuray-db zynqusp database, the Rust binaries or the
golden file. To (re)write the golden file with the reference
(`tests/oracle/setup-xilinx.sh`; `URAY_ORACLE_DIR` selects the
`tests/oracle` directory of the oracle build):

    python3 tests/cli/test_uray_corpus.py --write-goldens

(`--update-header` only refreshes the recorded reference commits, read
from `build/xilinx/status.json` of the oracle directory or the pinned
defaults of `tests/oracle/setup-xilinx.sh`, and keeps the results.)

`FASM_DB_CACHE` selects the database directory, `FASM_RUST_DIR` the
directory of the Rust binaries and `URAY_CORPUS_GOLDEN` the golden file.
"""
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FAMILY = 'zynqusp'
PART = 'xczu3eg-sfvc784-1-e'
TILES = ['sample', '3']
SEED = '0'
DEFAULT_GOLDEN = ROOT.joinpath('tests', 'corpus', 'prjuray', FAMILY,
                               'generated',
                               '{}-sample-3-s0.json'.format(PART))
GOLDEN = Path(os.environ.get('URAY_CORPUS_GOLDEN', DEFAULT_GOLDEN))
# Longer normalised stderr texts are kept as a hash.
STDERR_LIMIT = 2000
GENERATOR = ROOT / 'tools' / 'gen-xilinx-corpus.py'
RUST_DIR = Path(
    os.environ.get('FASM_RUST_DIR', ROOT / 'target' / 'release'))
RUST = RUST_DIR / 'uray-fasm2frames'
RUST_32 = RUST_DIR / 'fasm2frames'
ORACLE_DIR = Path(
    os.environ.get('URAY_ORACLE_DIR', ROOT / 'tests' / 'oracle'))
ORACLE = ROOT / 'tests' / 'oracle' / 'uray-fasm2frames-oracle'
REFERENCE_KEYS = ('prjuray_commit', 'prjuray_tools_commit')


def load_difftest():
    spec = importlib.util.spec_from_file_location(
        'difftest_xilinx', str(ROOT / 'tools' / 'difftest-xilinx.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def find_db():
    for base in (os.environ.get('FASM_DB_CACHE'),
                 ORACLE_DIR.joinpath('build', 'db'),
                 ROOT.joinpath('tests', 'oracle', 'build', 'db')):
        if base and (Path(base) / 'prjuray-db' / FAMILY / PART).is_dir():
            return Path(base) / 'prjuray-db' / FAMILY
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
    if difftest.is_rust_value_range_error(stderr):
        # The Rust side of a value range error.
        return '<value range error>'
    stderr = difftest.normalise(stderr, rules)
    if len(stderr) > STDERR_LIMIT:
        return 'sha256:' + sha256(stderr.encode('utf-8', 'surrogateescape'))
    return stderr


def run_tool(tool, db, corpus_dir, rel, flags, out, env=None):
    """(exit code, .frm bytes, stderr)."""
    full_env = dict(os.environ)
    full_env['URAY_ORACLE_DIR'] = str(ORACLE_DIR)
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
    return (result.returncode, frm,
            result.stderr.decode('utf-8', 'surrogateescape'))


def reference_versions():
    """The commits of the reference tools: the resolved commits recorded
    by tests/oracle/setup-xilinx.sh in build/xilinx/status.json, else the
    pinned defaults of setup-xilinx.sh (the tools are not run)."""
    versions = {}
    status = ORACLE_DIR / 'build' / 'xilinx' / 'status.json'
    if status.exists():
        data = json.loads(status.read_text())
        for key in REFERENCE_KEYS:
            value = data.get(key + '_resolved') or data.get(key)
            if value:
                versions[key] = value
        if versions:
            versions['source'] = 'tests/oracle/build/xilinx/status.json'
            return versions
    setup = ROOT / 'tests' / 'oracle' / 'setup-xilinx.sh'
    if setup.exists():
        text = setup.read_text()
        for key in REFERENCE_KEYS:
            m = re.search(r'%s="\$\{%s:-([0-9a-f]+)\}"' %
                          (key.upper(), key.upper()), text)
            if m:
                versions[key] = m.group(1)
        if versions:
            versions['source'] = 'tests/oracle/setup-xilinx.sh (pinned)'
    return versions


def db_commit(db):
    head = db.parent / '.git' / 'HEAD'
    return head.read_text().strip() if head.exists() else 'unknown'


def update_header():
    """Rewrites the golden file's header (reference versions, database
    commit) and keeps its hashes and results."""
    golden = json.loads(GOLDEN.read_text())
    golden['reference'] = reference_versions()
    db = find_db()
    if db is not None:
        golden['prjuray_db_commit'] = db_commit(db)
    GOLDEN.write_text(json.dumps(golden, indent=1, sort_keys=True) + '\n')
    print('updated the header of %s: %r' % (GOLDEN, golden['reference']))


def write_goldens():
    db = find_db()
    if db is None:
        sys.exit('prjuray-db %s not found (tools/fetch-db.sh prjuray %s)' %
                 (FAMILY, FAMILY))
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
                'frm_sha256': sha256(frm),
                'stderr': normalised_stderr(difftest, stderr),
            }
            if code == 0:
                runs[name]['frm32_sha256'] = sha256(
                    difftest.to_32bit_frm(frm))
            print('%s: exit %d' % (name, code))
    golden = {
        'comment': ('Reference results (tests/oracle/uray-fasm2frames-'
                    'oracle, prjuray utils/fasm2frames.py) for the corpus of '
                    'tools/gen-xilinx-corpus.py; written by python3 '
                    'tests/cli/test_uray_corpus.py --write-goldens'),
        'reference': reference_versions(),
        'prjuray_db_commit': db_commit(db),
        'part': PART,
        'generator': {
            'tiles': TILES,
            'seed': SEED,
            'generator_version': manifest['generator_version'],
        },
        'coverage': {
            'features_total': manifest['features_total'],
            'features_placed': manifest['features_distinct_placed'],
            'unreachable': len(manifest['unreachable']),
            'uncovered': len(manifest['uncovered']),
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
    if sys.argv[1:] == ['--update-header']:
        update_header()
        sys.exit(0)
    sys.exit('usage: %s --write-goldens | --update-header (or run it with '
             'pytest)' % sys.argv[0])

import pytest  # noqa: E402

DB = find_db()
if DB is None:
    pytest.skip('prjuray-db %s not fetched (tools/fetch-db.sh prjuray %s)' %
                (FAMILY, FAMILY),
                allow_module_level=True)
for _tool in (RUST, RUST_32):
    if not _tool.exists():
        pytest.skip('Rust %s missing (cargo build --release -p fasm-cli)' %
                    _tool,
                    allow_module_level=True)
if not GOLDEN.exists():
    pytest.skip('{} missing: python3 tests/cli/test_uray_corpus.py '
                '--write-goldens (needs tests/oracle/setup-xilinx.sh)'.format(
                    GOLDEN),
                allow_module_level=True)


@pytest.fixture(scope='module')
def corpus(tmp_path_factory):
    out = tmp_path_factory.mktemp('uray-corpus')
    manifest, files = generate(DB, out)
    return out, manifest, files


@pytest.fixture(scope='module')
def golden():
    return json.loads(GOLDEN.read_text())


def test_golden_header(golden):
    assert golden['part'] == PART
    assert golden['generator']['tiles'] == TILES
    for key in REFERENCE_KEYS:
        assert re.match(r'^[0-9a-f]{40}$', golden['reference'].get(key, '')), (
            'no %s recorded: python3 tests/cli/test_uray_corpus.py '
            '--update-header' % key)


REGENERATE = ('regenerate it with the reference tools: URAY_ORACLE_DIR='
              '<oracle>/tests/oracle FASM_DB_CACHE=<db cache> python3 '
              'tests/cli/test_uray_corpus.py --write-goldens')


def stale_golden(corpus, golden):
    """Why the golden file does not describe this corpus, or None."""
    _, _, files = corpus
    if golden.get('prjuray_db_commit') != db_commit(DB):
        return ('the golden file was made with prjuray-db %s, the fetched '
                'database %s is at %s' %
                (golden.get('prjuray_db_commit'), DB, db_commit(DB)))
    if files != golden['files_sha256']:
        changed = sorted(
            set(k for k in set(files) | set(golden['files_sha256'])
                if files.get(k) != golden['files_sha256'].get(k)))
        return ('tools/gen-xilinx-corpus.py now generates another corpus '
                'than the golden file was made from (changed: %s)' %
                ', '.join(changed))
    return None


def test_generated_files_match_golden(corpus, golden):
    """The generator (and the database) made the files the goldens were
    made from; otherwise rewrite the goldens."""
    reason = stale_golden(corpus, golden)
    if reason:
        pytest.fail('stale golden %s: %s; %s' % (GOLDEN, reason, REGENERATE))


def test_covers_every_feature(corpus):
    """Every unit of every tile type of the part is placed at least once;
    the only segbits keys not reached are those that are not FASM names
    (a part starting with a digit: 34 keys of BRAM,
    INT_INTF_LEFT_TERM_PSS and XIPHY_BYTE_RIGHT)."""
    _, manifest, _ = corpus
    coverage = manifest['coverage']
    for name, c in coverage.items():
        assert c['placed'] + c['uncovered'] == c['units'], (name, c)
    assert not manifest['uncovered']
    assert manifest['features_distinct_placed'] == manifest['features_total']
    reasons = set(u[2] for u in manifest['unreachable'])
    assert reasons == {'not a FASM feature name'}, reasons
    assert len(manifest['unreachable']) == 34
    types = set(t for t, c in coverage.items() if c['placed'])
    assert len(types) == 27, sorted(types)
    assert manifest['layout'] == 'prjuray'
    assert 'feature_name.fasm' in manifest['errors']


@pytest.mark.parametrize('xdb', ['cache', 'no-cache'])
def test_rust_matches_golden(corpus, golden, tmp_path, xdb):
    corpus_dir, manifest, _ = corpus
    difftest = load_difftest()
    env = {'FASM_XDB_CACHE': '0'}
    if xdb == 'cache':
        env = {'FASM_XDB_CACHE': str(tmp_path / 'xdb')}
    reason = stale_golden(corpus, golden)
    if reason:
        pytest.fail('stale golden %s, cannot compare: %s; %s' %
                    (GOLDEN, reason, REGENERATE))
    names = [c[0] for c in cases(manifest)]
    assert sorted(names) == sorted(golden['runs'])
    problems = []
    for name, rel, flags in cases(manifest):
        code, frm, stderr = run_tool(RUST, DB, corpus_dir, rel, flags,
                                     tmp_path / 'out.frm', env)
        want = golden['runs'][name]
        got = {
            'exit_code': code,
            'frm_sha256': sha256(frm),
            'stderr': normalised_stderr(difftest, stderr),
        }
        if code == 0:
            code32, frm32, stderr32 = run_tool(RUST_32, DB, corpus_dir, rel,
                                               flags, tmp_path / 'out.frm',
                                               env)
            got['frm32_sha256'] = sha256(frm32)
            if code32 != 0:
                problems.append('%s: fasm2frames failed: %s' %
                                (name, stderr32))
        if got != want:
            problems.append('%s:\n  golden %r\n  rust   %r' %
                            (name, want, got))
    assert not problems, '\n'.join(problems)
    if xdb == 'cache':
        assert list((tmp_path / 'xdb').glob('*.fasmxdb'))

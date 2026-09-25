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
""" Tests of fasm.xilinx (the fasm-xilinx bindings, T5.10).

* The API itself on the test databases of rust/fasm-xilinx/testdata
  (mini-db, synthetic-db, synthetic-usp-db).
* Equivalence with the Rust command line tools: byte identical .frm and
  .bit files and the same error messages (``<reference_exception>:
  <message>``, the last line the tools print) for fasm2frames, xcfasm,
  xc7frames2bit, xcframes2bit, bitread and uray-bitread. The tools are
  taken from $FASM_CLI_DIR, else target/{release,debug} of the checkout
  ($CARGO_TARGET_DIR is honoured; `cargo build -p fasm-cli`); these tests
  are skipped without them.
* With FASM_DB_CACHE pointing at the directory holding prjxray-db/ and
  prjuray-db/ (tools/fetch-db.sh, e.g. tests/oracle/build/db): the
  counter_test design of tests/corpus/xilinx on xc7a35tcsg324-1 (also
  against the reference .frm files of the corpus) and a design made of
  one feature of every tile type on xczu3eg; skipped otherwise.
* With the f4pga-xc-fasm oracle venv (tests/oracle/setup-xilinx.sh;
  $ORACLE_DIR, default tests/oracle): the reference Python API
  (xc_fasm.fasm2frames.fasm2frames) on the mini database; skipped
  otherwise.
"""

import io
import lzma
import os
import pickle
import re
import subprocess
import threading

import pytest

try:
    import fasm.xilinx as xilinx
except ImportError as e:  # the extension without its "xilinx" feature
    pytest.skip(str(e), allow_module_level=True)

from fasm.model import FasmLine, SetFasmFeature  # noqa: E402

fx = xilinx

ROOT = os.path.normpath(
    os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
TESTDATA = os.path.join(ROOT, 'rust', 'fasm-xilinx', 'testdata')
MINI_DB = os.path.join(TESTDATA, 'mini-db')
SYNTHETIC_DB = os.path.join(TESTDATA, 'synthetic-db')
USP_DB = os.path.join(TESTDATA, 'synthetic-usp-db')
XC_FASM_CORPUS = os.path.join(ROOT, 'tests', 'corpus', 'f4pga-xc-fasm')
MINI_FIXTURES = [
    'lut.fasm', 'ff_int.fasm', 'ff_int_0s.fasm', 'ff_int_op1.fasm',
    'lut_int.fasm', 'iob/liob_stepdown.fasm', 'iob/riob_stepdown.fasm'
]
SOURCE_DATE_EPOCH = 1700000000

SYNTHETIC_DESIGN = """\
INT_L_X6Y0.WW2BEG0.LOGIC_OUTS_L12
BRAM_L_X6Y0.RAMB18_Y0.INIT_00[4:0] = 5'b10011
BRAM_L_X6Y0.RAMB18_Y0.IN_USE
LIOB33_X0Y1.IOB_Y0.PULL
"""

USP_DESIGN = """\
CLEM_X1Y0.ALUT.INIT[15:0] = 16'hA5C3
CLEM_X1Y1.ABCDFF.CEUSED.V1
BRAM_X2Y0.RAMB18E2_L.INIT_00[7:0] = 8'hFF
RCLK_INT_L_X2Y29.BUFCE_LEAF_X0Y0.BUFCE_LEAF.DELAY_TAP.V0
EDGE_X0Y0.OK
"""

# ---------------------------------------------------------------------------
# Helpers.
# ---------------------------------------------------------------------------


def cli_dir():
    """ The directory of the Rust command line tools, or None. """
    candidates = []
    if os.environ.get('FASM_CLI_DIR'):
        candidates.append(os.environ['FASM_CLI_DIR'])
    target = os.environ.get('CARGO_TARGET_DIR', os.path.join(ROOT, 'target'))
    for profile in ('release', 'debug'):
        candidates.append(os.path.join(target, profile))
    found = [
        d for d in candidates if os.path.isfile(os.path.join(d, 'xcfasm'))
    ]
    if not found:
        return None
    if os.environ.get('FASM_CLI_DIR') in found:
        return os.environ['FASM_CLI_DIR']
    return max(
        found, key=lambda d: os.path.getmtime(os.path.join(d, 'xcfasm')))


CLI_DIR = cli_dir()
needs_cli = pytest.mark.skipif(
    CLI_DIR is None,
    reason='the Rust command line tools are not built (cargo build -p '
    'fasm-cli, or set FASM_CLI_DIR)')


def run_cli(tool, *args):
    """ Runs a Rust command line tool; returns (code, stdout, stderr). """
    env = dict(os.environ)
    env['FASM_XDB_CACHE'] = '0'
    env['SOURCE_DATE_EPOCH'] = str(SOURCE_DATE_EPOCH)
    result = subprocess.run(
        [os.path.join(CLI_DIR, tool)] + [str(a) for a in args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        timeout=600)
    return result.returncode, result.stdout, result.stderr.decode(
        'utf-8', 'replace')


def read(path):
    with open(path, 'rb') as f:
        return f.read()


def summary_to_frames(summary, words_per_frame=101):
    """ The frames of a mini-db-golden/frm summary (``frames <first>
    <count>`` and ``word <frame> <index> <value>`` lines, see its
    README.md). """
    frames = {}
    for line in summary.splitlines():
        fields = line.split(' ')
        if fields[0] == 'frames':
            start = int(fields[1], 16)
            for i in range(int(fields[2])):
                frames[start + i] = [0] * words_per_frame
        else:
            assert fields[0] == 'word', line
            frames[int(fields[1], 16)][int(fields[2])] = int(fields[3], 16)
    return fx.Frames(frames, words_per_frame)


def error_line(error):
    """ What the command line tools print for ``error``. """
    return '{}: {}'.format(error.reference_exception, error)


@pytest.fixture(scope='module')
def mini_db():
    return fx.Database.open(MINI_DB, 'xc7', cache=False)


@pytest.fixture(scope='module')
def synthetic_db():
    return fx.Database.open(SYNTHETIC_DB, 'xc7test-1', cache=False)


@pytest.fixture(scope='module')
def usp_db():
    return fx.Database.open(USP_DB, 'xcusptest-1', cache=False)


# ---------------------------------------------------------------------------
# Database.
# ---------------------------------------------------------------------------


def test_database_properties(mini_db, synthetic_db, usp_db):
    assert mini_db.part == 'xc7'
    assert mini_db.layout == 'prjxray'
    assert mini_db.architecture == 'Series7'
    assert mini_db.words_per_frame == 101
    assert mini_db.idcode is None
    assert mini_db.frame_addresses() is None
    assert os.fspath(mini_db.root) == MINI_DB
    assert 'CLBLM_L' in mini_db.tile_types()
    assert 'mini-db' in repr(mini_db)

    assert synthetic_db.idcode is not None
    addresses = synthetic_db.frame_addresses()
    assert len(addresses) == len(set(addresses)) > 0
    assert synthetic_db.required_features() == [
        'INT_L_X6Y0.IMUX_L1.EE2END0',
        'HCLK_L_X6Y26.ENABLE_BUFFER.HCLK_CK_BUFHCLK8',
    ]

    assert usp_db.layout == 'prjuray'
    assert usp_db.architecture == 'UltraScalePlus'
    assert usp_db.words_per_frame == 93
    assert fx.ARCHITECTURES == ('Series7', 'UltraScale', 'UltraScalePlus')


def test_database_constructor_and_errors(tmp_path):
    db = fx.Database(MINI_DB, 'xc7', cache=False)
    assert db.part == 'xc7'
    no_part = fx.Database.open(MINI_DB, cache=False)
    assert no_part.part is None and no_part.tiles() == []
    with pytest.raises(AttributeError):
        fx.FasmAssembler(no_part)
    with pytest.raises(fx.DbError) as e:
        fx.Database.open(MINI_DB, 'nope', cache=False)
    assert 'part "nope" not found' in str(e.value)
    with pytest.raises(fx.DbError) as e:
        fx.Database.open(str(tmp_path), 'xc7', cache=False)
    assert 'not a prjxray-db' in str(e.value)
    with pytest.raises(TypeError):
        fx.Database.open(MINI_DB, 'xc7', cache=3.5)


def test_database_cache(tmp_path, monkeypatch):
    uncached = fx.Database.open(SYNTHETIC_DB, 'xc7test-1', cache=False)
    cache_dir = tmp_path / 'explicit'
    for _ in range(2):  # build, then load
        db = fx.Database.open(SYNTHETIC_DB, 'xc7test-1', cache=cache_dir)
        assert db.frame_addresses() == uncached.frame_addresses()
    assert any(p.suffix == '.fasmxdb' for p in cache_dir.iterdir())
    # cache=True (and None) honour FASM_XDB_CACHE like the tools.
    env_dir = tmp_path / 'env'
    monkeypatch.setenv('FASM_XDB_CACHE', str(env_dir))
    fx.Database.open(SYNTHETIC_DB, 'xc7test-1')
    assert any(p.suffix == '.fasmxdb' for p in env_dir.iterdir())
    monkeypatch.setenv('FASM_XDB_CACHE', '0')
    fx.Database.open(MINI_DB, 'xc7', cache=True)
    fx.Database.open(MINI_DB, 'xc7', cache=None)
    assert len(list(env_dir.iterdir())) == 1


def test_tiles_and_features(mini_db, synthetic_db):
    tiles = mini_db.tiles()
    assert all(isinstance(t, fx.Tile) for t in tiles)
    clb = [t for t in tiles if t.tile_type == 'CLBLM_L'][0]
    assert clb.name == 'CLBLM_L_X10Y102'
    features = mini_db.tile_type_features('CLBLM_L')
    assert 'SLICEM_X0.A5FF.ZINI' in features
    with pytest.raises(KeyError):
        mini_db.tile_type_features('NOPE')
    ppips = dict(synthetic_db.pseudo_pips('INT_L'))
    assert ppips['BYP_ALT0.VCC_WIRE'] in ('always', 'default', 'hint')


def test_lookup_feature(mini_db, synthetic_db):
    bits = mini_db.lookup_feature('CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI')
    assert isinstance(bits, fx.FeatureBits)
    assert bits.tile == 'CLBLM_L_X10Y102'
    assert bits.tile_type == bits.segbits_tile_type == 'CLBLM_L'
    assert bits.pseudo_pip is None
    assert bits.block_type == 'CLB_IO_CLK'
    assert bits.frame_count > 0
    assert all(isinstance(b, tuple) and len(b) == 4 for b in bits.bits)
    # The bits are where the assembler puts them.
    asm = fx.FasmAssembler(mini_db)
    asm.add_fasm_line('CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI')
    set_bits = asm.get_frames(sparse=True).set_bits()
    assert set_bits == sorted(
        (f, w, b) for f, w, b, value in bits.bits if value)
    # A multi bit feature, a `!` bit, an alias tile.
    init = synthetic_db.lookup_feature(
        'BRAM_L_X6Y0.RAMB18_Y0.INIT_00', address=4)
    assert init.block_type == 'BLOCK_RAM'
    imux = synthetic_db.lookup_feature('INT_L_X6Y0.IMUX_L1.EE2END0')
    assert sorted(v for *_, v in imux.bits) == [False] * 3 + [True] * 2
    ppip = synthetic_db.lookup_feature('INT_L_X6Y0.BYP_ALT0.VCC_WIRE')
    assert ppip.pseudo_pip == dict(
        synthetic_db.pseudo_pips('INT_L'))['BYP_ALT0.VCC_WIRE']
    assert ppip.bits == () and ppip.block_type is None
    sing = synthetic_db.lookup_feature('LIOB33_SING_X0Y0.IOB_Y0.PULL')
    assert sing.segbits_tile_type == 'LIOB33'
    with pytest.raises(fx.FasmKeyError) as e:
        mini_db.lookup_feature('NOPE_X0Y0.A')
    assert error_line(e.value) == "KeyError: 'NOPE_X0Y0'"
    with pytest.raises(fx.FasmLookupError) as e:
        mini_db.lookup_feature('CLBLM_L_X10Y102.SLICEM_X0.NOPE', 3)
    assert str(e.value) == (
        'Segment DB CLBLM_L, key CLBLM_L.SLICEM_X0.NOPE[3] not found')


# ---------------------------------------------------------------------------
# FasmAssembler.
# ---------------------------------------------------------------------------


def test_assembler_matches_oracle_goldens(mini_db):
    """ The f4pga-xc-fasm oracle's .frm output of every mini db fixture
    (rust/fasm-xilinx/testdata/mini-db-golden, from xc_fasm itself). """
    for fixture in MINI_FIXTURES:
        stem = os.path.splitext(os.path.basename(fixture))[0]
        path = os.path.join(XC_FASM_CORPUS, fixture)
        for sparse, mode in [(False, 'dense'), (True, 'sparse')]:
            golden = os.path.join(
                TESTDATA, 'mini-db-golden', 'frm', '{}.{}.txt'.format(
                    stem, mode))
            expected = summary_to_frames(read(golden).decode()).to_frm()
            frames = fx.fasm2frames(mini_db, filename_in=path, sparse=sparse)
            assert frames.to_frm() == expected, (fixture, mode)
            # The same with the assembler step by step.
            asm = fx.FasmAssembler(mini_db)
            asm.parse_fasm_filename(path)
            asm.add_required_features()
            asm.propagate_stepdown()
            assert asm.get_frames(sparse=sparse) == frames
            # From the text.
            with open(path) as f:
                text = f.read()
            assert fx.fasm2frames(
                mini_db, fasm_text=text, sparse=sparse) == frames
            assert fx.fasm2frames(
                mini_db, fasm_text=text.encode(), sparse=sparse) == frames


def test_assembler_inputs(mini_db):
    text = 'CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI\n'
    expected = fx.fasm2frames(mini_db, fasm_text=text, sparse=True)
    line = FasmLine(
        set_feature=SetFasmFeature(
            feature='CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI',
            start=None,
            end=None,
            value=1,
            value_format=None),
        annotations=None,
        comment=None)

    class MyInt(int):
        pass

    odd = line._replace(set_feature=line.set_feature._replace(value=MyInt(1)))
    for add in [
            lambda a: a.parse_fasm_string(text),
            lambda a: a.parse_fasm_bytes(text.encode()),
            lambda a: a.parse_fasm_string('', extra_features=text),
            lambda a: a.parse_fasm_string('', extra_features=[line]),
            lambda a: a.add_fasm_line(line),
            lambda a: a.add_fasm_line(odd),
            lambda a: a.add_fasm_line(text.strip()),
            lambda a: a.add_fasm_lines([line]),
            lambda a: a.add_fasm_lines(text),
    ]:
        asm = fx.FasmAssembler(mini_db)
        add(asm)
        assert asm.get_frames(sparse=True) == expected
        assert len(asm) >= 1
    assert asm.database is not None
    assert 'lines' in repr(asm)


def test_assembler_errors(mini_db):
    asm = fx.FasmAssembler(mini_db)
    with pytest.raises(fx.FasmLookupError) as e:
        asm.parse_fasm_string(
            'CLBLM_L_X10Y102.SLICEM_X0.NOPE\n'
            'CLBLM_L_X10Y102.SLICEM_X0.NOPE2\n')
    assert len(e.value.messages) == 2
    assert str(e.value) == '\n'.join(e.value.messages)
    assert e.value.messages[0] == (
        "Segment DB CLBLM_L, key CLBLM_L.SLICEM_X0.NOPE not found from "
        "line 'CLBLM_L_X10Y102.SLICEM_X0.NOPE'")
    missing = []
    asm.add_fasm_line('CLBLM_L_X10Y102.SLICEM_X0.NOPE3', missing)
    assert len(missing) == 1
    with pytest.raises(fx.FasmLookupError):
        asm.add_fasm_line('CLBLM_L_X10Y102.SLICEM_X0.NOPE3')
    with pytest.raises(fx.FasmKeyError) as e:
        asm.add_fasm_line('NOPE_X1Y1.A')
    assert isinstance(e.value, KeyError)
    with pytest.raises(fx.FasmParseError) as e:
        asm.parse_fasm_string('A.B\nX Y\n')
    assert isinstance(e.value, fx.Error)
    from fasm.parser.rust import FasmParseError
    assert isinstance(e.value, FasmParseError)
    assert (e.value.line, e.value.column) == (2, 2)
    with pytest.raises(fx.FasmParseError) as e:
        fx.fasm2frames(mini_db, filename_in='/nonexistent.fasm')
    assert str(e.value) == "Parse error at 0:0 - Couldn't open file"
    with pytest.raises(fx.FasmParseError) as e:
        asm.parse_fasm_filename('/nonexistent.fasm')
    assert str(e.value) == "Parse error at 0:0 - Couldn't open file"
    with pytest.raises(FileNotFoundError) as e:
        fx.fasm2frames(mini_db, fasm_text='', roi='/nonexistent/design.json')
    assert error_line(e.value) == (
        "FileNotFoundError: [Errno 2] No such file or directory: "
        "'/nonexistent/design.json'")
    with pytest.raises(TypeError):
        fx.fasm2frames(mini_db)
    # Conflicting bits.
    asm = fx.FasmAssembler(mini_db)
    feature = 'CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI'
    bits = mini_db.lookup_feature(feature).bits
    frame, word, bit, _ = bits[0]
    other = [
        f for f in mini_db.tile_type_features('CLBLM_L')
        if f != 'SLICEM_X0.A5FF.ZINI'
    ]
    conflict = None
    for f in other:
        found = mini_db.lookup_feature('CLBLM_L_X10Y102.' + f)
        if any(b[:3] == (frame, word, bit) and not b[3] for b in found.bits):
            conflict = f
    if conflict is not None:
        with pytest.raises(fx.FasmInconsistentBits):
            asm.parse_fasm_string(
                '{}\nCLBLM_L_X10Y102.{}\n'.format(feature, conflict))


def test_feature_callback_and_reentrancy(mini_db):
    asm = fx.FasmAssembler(mini_db)
    seen = []
    asm.set_feature_callback(seen.append)
    asm.parse_fasm_string(
        '# c\nA_X0Y0.B = 0\nCLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI\n')
    assert [s.feature for s in seen] == [
        'A_X0Y0.B', 'CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI'
    ]
    assert isinstance(seen[0], SetFasmFeature)

    def raising(_):
        raise ZeroDivisionError('from the callback')

    asm.set_feature_callback(raising)
    with pytest.raises(ZeroDivisionError):
        asm.add_fasm_line('CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI')

    def reentrant(_):
        asm.get_frames()

    asm.set_feature_callback(reentrant)
    with pytest.raises(RuntimeError):
        asm.add_fasm_line('CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI')
    asm.set_feature_callback(None)
    asm.add_fasm_line('CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI')


def test_roi(synthetic_db, tmp_path):
    design = tmp_path / 'design.json'
    design.write_text(
        '{"info": {"GRID_X_MIN": 0, "GRID_X_MAX": 100, "GRID_Y_MIN": 0, '
        '"GRID_Y_MAX": 100}, "required_features": '
        '["LIOB33_X0Y1.IOB_Y0.PULL"]}')
    roi = fx.read_roi_design(design)
    assert isinstance(roi, fx.RoiDesign)
    assert roi.roi == fx.Roi(0, 100, 0, 100)
    assert roi.required_features == 'LIOB33_X0Y1.IOB_Y0.PULL'
    frames = fx.fasm2frames(
        synthetic_db, fasm_text='', sparse=True, roi=str(design))
    asm = fx.FasmAssembler(synthetic_db)
    asm.mark_roi_frames(roi.roi)
    asm.parse_fasm_string('', extra_features=roi.required_features)
    asm.add_required_features()
    asm.propagate_stepdown()
    assert asm.get_frames(sparse=True) == frames
    asm = fx.FasmAssembler(synthetic_db)
    asm.mark_roi_frames((0, 100, 0, 100))
    assert len(asm.get_frames(sparse=True)) > 0


def test_threads(mini_db):
    """ Several threads assembling at once (the GIL is released) get the
    same frames as one; one assembler shared by threads serialises. """
    path = os.path.join(XC_FASM_CORPUS, 'lut_int.fasm')
    expected = fx.fasm2frames(mini_db, filename_in=path)
    results = []
    shared = fx.FasmAssembler(mini_db)

    def work():
        results.append(fx.fasm2frames(mini_db, filename_in=path))
        shared.add_fasm_lines(['CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI'] * 10)

    threads = [threading.Thread(target=work) for _ in range(8)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert len(results) == 8 and all(r == expected for r in results)
    assert len(shared) == 80


# ---------------------------------------------------------------------------
# Frames.
# ---------------------------------------------------------------------------


def test_frames_mapping(mini_db, tmp_path):
    path = os.path.join(XC_FASM_CORPUS, 'ff_int.fasm')
    frames = fx.fasm2frames(mini_db, filename_in=path, sparse=True)
    import collections.abc
    assert isinstance(frames, collections.abc.Mapping)
    as_dict = frames.to_dict()
    assert frames == as_dict and as_dict == dict(frames.items())
    assert list(frames) == sorted(as_dict) == frames.keys()
    first = frames.keys()[0]
    assert first in frames and 0xFFFFFFFF not in frames
    assert 'x' not in frames
    assert frames[first] == as_dict[first] == frames.get(first)
    assert frames.get(1) is None and frames.get(1, 5) == 5
    with pytest.raises(KeyError):
        frames[1]
    assert len(frames) == len(as_dict)
    assert frames.values() == [as_dict[a] for a in frames]
    data = frames.to_bytes()
    assert len(data) == len(frames) * 101 * 4
    assert frames.frame_bytes(first) == data[:404]
    assert fx.Frames({a: frames.frame_bytes(a) for a in frames}) == frames
    assert fx.Frames(as_dict, words_per_frame=101) == frames
    assert fx.Frames(words_per_frame=93).words_per_frame == 93
    assert len(fx.Frames()) == 0
    with pytest.raises(ValueError):
        fx.Frames({1: [0] * 100}, words_per_frame=101)
    with pytest.raises(TypeError):
        fx.Frames([1, 2])
    assert pickle.loads(pickle.dumps(frames)) == frames
    assert frames != {} and frames != 3
    assert 'Frames' in repr(frames)
    bits = frames.set_bits()
    assert bits and all(as_dict[f][w] >> b & 1 for f, w, b in bits)


def test_frm_files(mini_db, tmp_path):
    path = os.path.join(XC_FASM_CORPUS, 'lut.fasm')
    frames = fx.fasm2frames(mini_db, filename_in=path)
    text = frames.to_frm()
    out = tmp_path / 'out.frm'
    frames.write_frm(out)
    assert out.read_text() == text
    frames.write_frm(str(out) + '2')
    assert read(str(out) + '2').decode() == text
    buf = io.StringIO()
    frames.write_frm(buf)
    assert buf.getvalue() == text
    with open(tmp_path / 'b.frm', 'wb') as f:
        frames.write_frm(f)
    assert read(tmp_path / 'b.frm').decode() == text
    with open(tmp_path / 't.frm', 'w') as f:
        fx.fasm2frames(mini_db, filename_in=path, f_out=f)
    assert read(tmp_path / 't.frm').decode() == text
    assert fx.Frames.read_frm(out) == frames
    assert fx.Frames.read_frm(str(out)) == frames
    with open(out) as f:
        assert fx.Frames.read_frm(f) == frames
    with open(out, 'rb') as f:
        assert fx.Frames.read_frm(f) == frames
    assert fx.Frames.from_frm(text) == frames
    assert fx.Frames.from_frm(text.encode()) == frames
    with pytest.warns(UserWarning, match='found 2 words instead of 101'):
        short = fx.Frames.from_frm('0x00000001 0x1,0x2\n')
    assert len(short) == 0
    with pytest.raises(fx.FrmError) as e:
        fx.Frames.from_frm('zz 0x1\n')
    assert e.value.line == 1 and isinstance(e.value, ValueError)
    with pytest.raises(FileNotFoundError):
        fx.Frames.read_frm(tmp_path / 'missing.frm')
    usp = fx.Frames.from_frm('0x00000001 ' + ','.join(['0x1'] * 93), 93)
    assert usp[1] == [1] * 93


def test_debug_output(mini_db, capsys):
    path = os.path.join(XC_FASM_CORPUS, 'lut.fasm')
    frames = fx.fasm2frames(mini_db, filename_in=path, sparse=True, debug=True)
    out = capsys.readouterr().out
    assert out == fx.dump_frames_sparse(frames)
    assert out.startswith('\nFrames: {}\n'.format(len(frames)))


# ---------------------------------------------------------------------------
# Bitstreams.
# ---------------------------------------------------------------------------


def test_bitstream_round_trip(synthetic_db, usp_db, tmp_path):
    for db, design in [(synthetic_db, SYNTHETIC_DESIGN), (usp_db, USP_DESIGN)]:
        frames = fx.fasm2frames(db, fasm_text=design, sparse=True)
        bit = fx.write_bitstream(frames, db, source_date_epoch=0)
        assert isinstance(bit, bytes) and bit[:2] == b'\x00\x09'
        back = fx.read_bitstream(bit, db)
        assert back.words_per_frame == db.words_per_frame
        assert len(back) == len(db.frame_addresses())
        if db is usp_db:
            # (On the synthetic Series7 part, whose rows have gaps in their
            # columns, the command line tools read back the frames of the
            # bottom half shifted too: see test_cli_synthetic_db.)
            for address in frames:
                assert back[address] == frames[address]
            sparse = fx.read_bitstream(bit, db, skip_zero=True)
            assert set(sparse) <= set(frames)
        # Output to a path, a file object; reading from both.
        fx.write_bitstream(frames, db, tmp_path / 'a.bit', source_date_epoch=0)
        with open(tmp_path / 'b.bit', 'wb') as f:
            fx.write_bitstream(frames, db, f, source_date_epoch=0)
        assert read(tmp_path / 'a.bit') == read(tmp_path / 'b.bit') == bit
        assert fx.read_bitstream(tmp_path / 'a.bit', db) == back
        with open(tmp_path / 'a.bit', 'rb') as f:
            assert fx.read_bitstream(f, db) == back
        assert fx.read_bitstream(bytearray(bit), db) == back
        # A mapping works as frames too.
        assert fx.write_bitstream(
            frames.to_dict(), db, source_date_epoch=0) == bit


def test_bitstream_header(synthetic_db, monkeypatch):
    frames = fx.fasm2frames(
        synthetic_db, fasm_text=SYNTHETIC_DESIGN, sparse=True)
    bit = fx.write_bitstream(
        frames,
        synthetic_db,
        part_name='mypart',
        design_name='design.frm',
        generator='gen',
        source_date_epoch=0)
    assert b'design.frm;Generator=gen\x00' in bit
    assert b'mypart\x00' in bit and b'1970/01/01\x00' in bit
    assert b'00:00:00\x00' in bit
    monkeypatch.setenv('SOURCE_DATE_EPOCH', '86400')
    assert b'1970/01/02\x00' in fx.write_bitstream(frames, synthetic_db)
    monkeypatch.setenv('SOURCE_DATE_EPOCH', 'soon')
    with pytest.warns(UserWarning, match='not an integer'):
        fx.write_bitstream(frames, synthetic_db)
    default = fx.write_bitstream(frames, synthetic_db, source_date_epoch=0)
    assert b'xc7test-1\x00' in default


def test_bitstream_errors(mini_db, synthetic_db, usp_db, tmp_path):
    frames = fx.fasm2frames(
        synthetic_db, fasm_text=SYNTHETIC_DESIGN, sparse=True)
    with pytest.raises(fx.DbError):
        fx.write_bitstream(frames, mini_db)  # no frame tree
    with pytest.raises(fx.BitstreamError):
        fx.write_bitstream(frames, usp_db)  # 101 words, UltraScale+ part
    with pytest.raises(fx.BitstreamError):
        fx.write_bitstream(frames, synthetic_db, format='UltraScalePlus')
    with pytest.raises(ValueError):
        fx.write_bitstream(frames, synthetic_db, format='Virtex2')
    with pytest.raises(TypeError):
        fx.write_bitstream(frames, 42)
    with pytest.raises(fx.DbError):
        fx.write_bitstream(frames, tmp_path / 'missing.yaml')
    with pytest.raises(fx.BitstreamError, match='look like a bitstream'):
        fx.read_bitstream(b'\x00' * 64, synthetic_db)
    usp = fx.write_bitstream(
        fx.fasm2frames(usp_db, fasm_text=USP_DESIGN),
        usp_db,
        source_date_epoch=0)
    with pytest.raises(fx.BitstreamError):
        fx.read_bitstream(usp, synthetic_db)


def test_fasm2bit(synthetic_db, tmp_path):
    fasm_file = tmp_path / 'design.fasm'
    fasm_file.write_text(SYNTHETIC_DESIGN)
    frames, bit = fx.fasm2bit(
        SYNTHETIC_DB,
        'xc7test-1',
        str(fasm_file),
        None,
        cache=False,
        source_date_epoch=0)
    assert fx.read_bitstream(bit, synthetic_db) == fx.read_bitstream(
        fx.write_bitstream(
            frames,
            synthetic_db,
            design_name=str(fasm_file),
            source_date_epoch=0), synthetic_db)
    assert str(fasm_file).encode() in bit


# ---------------------------------------------------------------------------
# Equivalence with the Rust command line tools.
# ---------------------------------------------------------------------------


@needs_cli
@pytest.mark.parametrize('fixture', MINI_FIXTURES)
@pytest.mark.parametrize('sparse', [False, True])
def test_cli_fasm2frames_mini_db(mini_db, tmp_path, fixture, sparse):
    path = os.path.join(XC_FASM_CORPUS, fixture)
    out = tmp_path / 'cli.frm'
    args = ['--db-root', MINI_DB, '--part', 'xc7', path, out]
    if sparse:
        args.insert(0, '--sparse')
    code, _, err = run_cli('fasm2frames', *args)
    assert (code, err) == (0, '')
    frames = fx.fasm2frames(MINI_DB, 'xc7', path, sparse=sparse, cache=False)
    assert frames.to_frm().encode() == read(out)


MINI_ERRORS = {
    'lookup': 'CLBLM_L_X10Y102.SLICEM_X0.NOPE\nCLBLM_L_X10Y102.X[3:2] = 3\n',
    'key': 'NOPE_X1Y1.A\n',
    'parse': 'CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI\nA B\n',
    'value': 'A[1:0] = 7\n',
}


@needs_cli
@pytest.mark.parametrize('name', sorted(MINI_ERRORS))
def test_cli_error_messages(mini_db, tmp_path, name):
    path = tmp_path / (name + '.fasm')
    path.write_text(MINI_ERRORS[name])
    code, _, err = run_cli(
        'fasm2frames', '--db-root', MINI_DB, '--part', 'xc7', path,
        tmp_path / 'out.frm')
    assert code == 1
    with pytest.raises(Exception) as e:
        fx.fasm2frames(mini_db, filename_in=str(path))
    assert err == error_line(e.value) + '\n'


@needs_cli
def test_cli_synthetic_db(synthetic_db, tmp_path):
    """ fasm2frames, xcfasm, xc7frames2bit (native and the prjxray
    UltraScale variant) and bitread vs fasm.xilinx on synthetic-db. """
    fasm_file = tmp_path / 'design.fasm'
    fasm_file.write_text(SYNTHETIC_DESIGN)
    part_file = os.path.join(SYNTHETIC_DB, 'xc7test-1', 'part.yaml')
    for sparse in [False, True]:
        frm = tmp_path / 'cli.frm'
        args = ['--db-root', SYNTHETIC_DB, '--part', 'xc7test-1']
        if sparse:
            args.append('--sparse')
        assert run_cli('fasm2frames', *args, fasm_file, frm)[0] == 0
        frames = fx.fasm2frames(
            synthetic_db, filename_in=str(fasm_file), sparse=sparse)
        assert frames.to_frm().encode() == read(frm)

        # xcfasm (fasm2bit) with and without --frm_out.
        bit = tmp_path / 'cli.bit'
        frm_out = tmp_path / 'xcfasm.frm'
        code, _, err = run_cli(
            'xcfasm', *args, '--part_file', part_file, '--fn_in', fasm_file,
            '--bit_out', bit, '--frm_out', frm_out)
        assert (code, err) == (0, '')
        py_bit = tmp_path / 'py.bit'
        py_frm = tmp_path / 'py.frm'
        fx.fasm2bit(
            SYNTHETIC_DB,
            'xc7test-1',
            str(fasm_file),
            str(py_bit),
            part_file=part_file,
            frm_out=str(frm_out),
            sparse=sparse,
            cache=False,
            source_date_epoch=SOURCE_DATE_EPOCH)
        assert read(py_bit) == read(bit)
        fx.fasm2bit(
            synthetic_db,
            'xc7test-1',
            str(fasm_file),
            str(py_bit),
            frm_out=str(py_frm),
            sparse=sparse,
            source_date_epoch=SOURCE_DATE_EPOCH)
        assert read(py_frm) == read(frm_out)
        code, _, err = run_cli(
            'xcfasm', *args, '--part_file', part_file, '--fn_in', fasm_file,
            '--bit_out', bit)
        assert (code, err) == (0, '')
        fx.fasm2bit(
            synthetic_db,
            'xc7test-1',
            str(fasm_file),
            str(py_bit),
            sparse=sparse,
            source_date_epoch=SOURCE_DATE_EPOCH)
        assert read(py_bit) == read(bit)

    # xc7frames2bit, from the .frm file.
    bit = tmp_path / 'frames2bit.bit'
    code, _, err = run_cli(
        'xc7frames2bit', '--part_file', part_file, '--part_name', 'xc7test-1',
        '--frm_file', frm, '--output_file', bit)
    assert (code, err) == (0, '')
    frames = fx.Frames.read_frm(frm)
    for part in [synthetic_db, part_file]:
        assert fx.write_bitstream(
            frames,
            part,
            part_name='xc7test-1',
            design_name=str(frm),
            source_date_epoch=SOURCE_DATE_EPOCH) == read(bit)

    # bitread --frm_out (ECC cleared), and -C (kept).
    for keep_ecc in [False, True]:
        back = tmp_path / 'back.frm'
        flags = ['-C'] if keep_ecc else []
        code, _, err = run_cli(
            'bitread', '--part_file', part_file, '--frm_out', back, *flags,
            bit)
        assert code == 0
        for part in [synthetic_db, part_file]:
            py = fx.read_bitstream(str(bit), part, clear_ecc=not keep_ecc)
            assert py.to_frm().encode() == read(back)

    # The plain prjxray tools' UltraScale format on a Series7 part
    # (xc7frames2bit --architecture=UltraScale).
    frames123 = fx.Frames(
        {a: [a & 0xFF] * 123
         for a in synthetic_db.frame_addresses()[:5]})
    frm123 = tmp_path / 'us.frm'
    frames123.write_frm(frm123)
    code, _, err = run_cli(
        'xc7frames2bit', '--architecture', 'UltraScale', '--part_file',
        part_file, '--part_name', 'xc7test-1', '--frm_file', frm123,
        '--output_file', bit)
    assert (code, err) == (0, '')
    assert fx.write_bitstream(
        frames123,
        synthetic_db,
        format='prjxray:UltraScale',
        design_name=str(frm123),
        source_date_epoch=SOURCE_DATE_EPOCH) == read(bit)
    back = tmp_path / 'us-back.frm'
    code, _, err = run_cli(
        'bitread', '--architecture', 'UltraScale', '--part_file', part_file,
        '--frm_out', back, bit)
    assert code == 0
    py = fx.read_bitstream(bit, synthetic_db, format='prjxray:UltraScale')
    assert py.to_frm().encode() == read(back)


@needs_cli
def test_cli_synthetic_usp_db(usp_db, tmp_path):
    """ fasm2frames (32-bit words), xcframes2bit and uray-bitread vs
    fasm.xilinx on synthetic-usp-db (UltraScale+). """
    fasm_file = tmp_path / 'design.fasm'
    fasm_file.write_text(USP_DESIGN)
    part_file = os.path.join(USP_DB, 'xcusptest-1', 'part.yaml')
    for sparse in [False, True]:
        frm = tmp_path / 'cli.frm'
        args = ['--db-root', USP_DB, '--part', 'xcusptest-1']
        if sparse:
            args.append('--sparse')
        assert run_cli('fasm2frames', *args, fasm_file, frm)[0] == 0
        frames = fx.fasm2frames(
            usp_db, filename_in=str(fasm_file), sparse=sparse)
        assert frames.words_per_frame == 93
        assert frames.to_frm().encode() == read(frm)
        # The assembler in prjuray mode (the default for UltraScale+).
        asm = fx.FasmAssembler(usp_db)
        asm.parse_fasm_filename(fasm_file)
        asm.add_required_features()
        assert asm.get_frames(sparse=sparse) == frames

    bit = tmp_path / 'cli.bit'
    code, _, err = run_cli(
        'xcframes2bit', '--architecture', 'UltraScalePlus', '--part_file',
        part_file, '--part_name', 'xcusptest-1', '--frm_file', frm,
        '--output_file', bit)
    assert (code, err) == (0, '')
    for part in [usp_db, part_file]:
        for format in [None, 'UltraScalePlus', 'native:UltraScalePlus']:
            assert fx.write_bitstream(
                frames,
                part,
                format=format,
                part_name='xcusptest-1',
                design_name=str(frm),
                source_date_epoch=SOURCE_DATE_EPOCH) == read(bit)
    back = tmp_path / 'back.frm'
    code, _, err = run_cli(
        'uray-bitread', '--architecture', 'UltraScalePlus', '--part_file',
        part_file, '--frm_out', back, bit)
    assert code == 0
    py = fx.read_bitstream(bit, usp_db)
    assert py.to_frm().encode() == read(back)


# ---------------------------------------------------------------------------
# Real databases (FASM_DB_CACHE).
# ---------------------------------------------------------------------------

DB_CACHE = os.environ.get('FASM_DB_CACHE')
ARTIX7 = os.path.join(DB_CACHE or '', 'prjxray-db', 'artix7')
ZYNQUSP = os.path.join(DB_CACHE or '', 'prjuray-db', 'zynqusp')
COUNTER = os.path.join(
    ROOT, 'tests', 'corpus', 'xilinx', 'artix7', 'designs', 'f4pga-examples',
    'counter_test', 'arty_35')
needs_artix7 = pytest.mark.skipif(
    not os.path.isdir(os.path.join(ARTIX7, 'xc7a35tcsg324-1')),
    reason='set FASM_DB_CACHE to a directory with prjxray-db/artix7 '
    '(tools/fetch-db.sh)')


@pytest.fixture(scope='module')
def artix7_db():
    return fx.Database.open(ARTIX7, 'xc7a35tcsg324-1', cache=False)


@needs_artix7
def test_counter_test_reference_frm(artix7_db):
    """ counter_test vs the reference .frm files of the corpus
    (f4pga-xc-fasm's output, xz compressed). """
    fasm_file = os.path.join(COUNTER, 'top.fasm')
    for name, kwargs in [
        ('top.frm.xz', {}),
        ('top.sparse.frm.xz', {'sparse': True}),
        ('top.pudc.frm.xz', {'emit_pudc_b_pullup': True}),
    ]:
        expected = lzma.decompress(read(os.path.join(COUNTER, name)))
        frames = fx.fasm2frames(artix7_db, filename_in=fasm_file, **kwargs)
        assert frames.to_frm().encode() == expected, name


@needs_artix7
@needs_cli
def test_counter_test_cli(artix7_db, tmp_path):
    fasm_file = os.path.join(COUNTER, 'top.fasm')
    part_file = os.path.join(ARTIX7, 'xc7a35tcsg324-1', 'part.yaml')
    for sparse in [False, True]:
        args = ['--db-root', ARTIX7, '--part', 'xc7a35tcsg324-1']
        if sparse:
            args.append('--sparse')
        bit = tmp_path / 'cli.bit'
        frm = tmp_path / 'cli.frm'
        code, _, err = run_cli(
            'xcfasm', *args, '--part_file', part_file, '--fn_in', fasm_file,
            '--bit_out', bit, '--frm_out', frm)
        assert (code, err) == (0, '')
        py_bit = tmp_path / 'py.bit'
        frames = fx.fasm2bit(
            artix7_db,
            'xc7a35tcsg324-1',
            fasm_file,
            str(py_bit),
            part_file=part_file,
            frm_out=str(frm),
            sparse=sparse,
            source_date_epoch=SOURCE_DATE_EPOCH)
        assert frames.to_frm().encode() == read(frm)
        assert read(py_bit) == read(bit)
        # And back.
        back = fx.read_bitstream(py_bit, artix7_db)
        assert all(back[a] == frames[a] for a in frames)


def uray_part():
    if not os.path.isdir(ZYNQUSP):
        return None
    parts = sorted(
        p for p in os.listdir(ZYNQUSP) if p.startswith('xczu3eg')
        and os.path.isfile(os.path.join(ZYNQUSP, p, 'tilegrid.json')))
    return parts[0] if parts else None


URAY_PART = uray_part() if DB_CACHE else None


@pytest.mark.skipif(
    URAY_PART is None,
    reason='set FASM_DB_CACHE to a directory with prjuray-db/zynqusp '
    '(tools/fetch-db.sh)')
@needs_cli
def test_xczu3eg_cli(tmp_path):
    """ One feature of every tile type (of up to 4 tiles each) of
    xczu3eg: fasm2frames and xcframes2bit vs fasm.xilinx. """
    db = fx.Database.open(ZYNQUSP, URAY_PART, cache=False)
    assert db.architecture == 'UltraScalePlus'
    lines = []
    count = {}
    for tile in db.tiles():
        if count.get(tile.tile_type, 0) >= 4:
            continue
        # (Some prjuray-db tags are not FASM feature names.)
        features = [
            f for f in db.tile_type_features(tile.tile_type)
            if re.match(r'^[A-Za-z_]\w*(\.[A-Za-z_]\w*)*$', f)
        ]
        if not features:
            continue
        count[tile.tile_type] = count.get(tile.tile_type, 0) + 1
        lines.append('{}.{}'.format(tile.name, features[0]))
    assert len(count) > 10
    fasm_file = tmp_path / 'design.fasm'
    fasm_file.write_text('\n'.join(lines) + '\n')
    frm = tmp_path / 'cli.frm'
    code, _, err = run_cli(
        'fasm2frames', '--db-root', ZYNQUSP, '--part', URAY_PART, '--sparse',
        fasm_file, frm)
    assert (code, err) == (0, '')
    frames = fx.fasm2frames(db, filename_in=str(fasm_file), sparse=True)
    assert frames.to_frm().encode() == read(frm)
    part_file = os.path.join(ZYNQUSP, URAY_PART, 'part.yaml')
    bit = tmp_path / 'cli.bit'
    code, _, err = run_cli(
        'xcframes2bit', '--architecture', 'UltraScalePlus', '--part_file',
        part_file, '--part_name', URAY_PART, '--frm_file', frm,
        '--output_file', bit)
    assert (code, err) == (0, '')
    assert fx.write_bitstream(
        frames, db, design_name=str(frm),
        source_date_epoch=SOURCE_DATE_EPOCH) == read(bit)


# ---------------------------------------------------------------------------
# The reference Python API (xc_fasm) on the mini database.
# ---------------------------------------------------------------------------

ORACLE_DIR = os.environ.get(
    'ORACLE_DIR', os.path.join(ROOT, 'tests', 'oracle'))
ORACLE_PYTHON = os.path.join(ORACLE_DIR, 'venv-xilinx', 'bin', 'python')

ORACLE_SCRIPT = """
from xc_fasm.fasm2frames import fasm2frames, dump_frm
frames = fasm2frames(
    db_root=sys.argv[1], part=sys.argv[2], filename_in=sys.argv[3],
    sparse=sys.argv[4] == '1')
dump_frm(sys.stdout, frames)
"""


@pytest.mark.skipif(
    not os.path.isfile(ORACLE_PYTHON),
    reason='no f4pga-xc-fasm oracle venv (tests/oracle/setup-xilinx.sh, '
    'or set ORACLE_DIR)')
@pytest.mark.parametrize('fixture', MINI_FIXTURES)
def test_xc_fasm_reference_api(mini_db, tmp_path, fixture):
    path = os.path.join(XC_FASM_CORPUS, fixture)
    for sparse in [False, True]:
        result = subprocess.run(
            [
                ORACLE_PYTHON, '-c', ORACLE_SCRIPT, MINI_DB, 'xc7', path,
                '1' if sparse else '0'
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=str(tmp_path),
            timeout=600)
        assert result.returncode == 0, result.stderr
        frames = fx.fasm2frames(mini_db, filename_in=path, sparse=sparse)
        assert frames.to_frm().encode() == result.stdout
        # The reference returns a dict of lists: equal to our Frames.
        reference = fx.Frames.from_frm(result.stdout)
        assert frames == reference.to_dict()

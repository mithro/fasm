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
"""RapidWright cross-checks of the Rust Xilinx tools (T7.5).

RapidWright (tools/e2e/setup-rapidwright.sh) has no FASM writer, but its
closed source `com.xilinx.rapidwright.bitstream` package knows the
configuration array of every part and reads and writes `.bit` files. This
script compares, through tools/e2e/rapidwright/RwCheck.java:

layout
    RapidWright's configuration array of every part of the fetched
    prjxray-db / prjuray-db (and of the ToolsTestData UltraScale part):
    frame address walk, columns and frame counts, words per frame, IDCODE,
    FDRI payload size, against the Rust `Part` (the walk of an empty
    bitstream written by `xc7frames2bit` / `xcframes2bit` and read back by
    `bitread` / `uray-bitread --aux`) and the database's `part.json`.
    `--write-golden` stores RapidWright's layouts under
    tests/corpus/{xilinx/<family>,prjuray/zynqusp}/rapidwright/, which
    tests/e2e/test_rapidwright.py checks without RapidWright.

bits
    Every reference bitstream (Vivado: prjxray-db harness designs, prjxray
    and prjuray-tools test data; f4pga flow outputs when present; the
    corpus `smoke_x1y0.bit`) read by RapidWright and by the Rust readers
    (frames per frame address, ECC kept; header; packet list), then the
    frames written by the Rust writer and read by RapidWright, written by
    RapidWright and read by the Rust reader, and the original rewritten by
    RapidWright and read by the Rust reader.

frm
    Every corpus `.frm` of tests/corpus/xilinx/*/designs (and
    `smoke_x1y0.frm`): Rust `xc7frames2bit` -> RapidWright reader, and
    RapidWright writer -> Rust `bitread` (so RapidWright's ECC is compared
    with ours too).

Every difference is classified: `identical`, `explained` (a documented
RapidWright behaviour that the check verifies exactly, see
docs/rewrite/DESIGN-rapidwright.md) or `different`. The exit status is 1
if anything is `different`.
"""
import argparse
import hashlib
import json
import lzma
import os
import shutil
import struct
import subprocess
import sys
import tarfile
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
RUST = Path(os.environ.get('FASM_RUST_BIN', REPO_ROOT / 'target' / 'release'))
DB_CACHE = Path(
    os.environ.get(
        'FASM_DB_CACHE', REPO_ROOT / 'tests' / 'oracle' / 'build' / 'db'))
ORACLE_SRC = Path(
    os.environ.get(
        'FASM_ORACLE_SRC',
        REPO_ROOT / 'tests' / 'oracle' / 'build' / 'xilinx' / 'src'))
RW_BUILD = Path(
    os.environ.get(
        'RAPIDWRIGHT_E2E_ROOT',
        REPO_ROOT / 'tools' / 'e2e' / 'build' / 'rapidwright'))
RW_TAG = 'v2026.1.0-beta'
RW_JAR = RW_BUILD / 'rapidwright-2026.1.0-standalone-lin64.jar'
CORPUS = REPO_ROOT / 'tests' / 'corpus'
SERIES7_FAMILIES = ('artix7', 'kintex7', 'spartan7', 'zynq7')
WORDS_PER_FRAME = {'Series7': 101, 'UltraScale': 123, 'UltraScalePlus': 93}
BUS = {'CLB_IO_CLK': 0, 'BLOCK_RAM': 1, 'CFG_CLB': 2}
SYNC = b'\xaa\x99\x55\x66'
CTL1_PER_FRAME_CRC = 1 << 21

# ---------------------------------------------------------------- helpers


def run(cmd, **kw):
    kw.setdefault('stdout', subprocess.DEVNULL)
    kw.setdefault('stderr', subprocess.PIPE)
    p = subprocess.run([str(c) for c in cmd], timeout=1800, **kw)
    if p.returncode != 0:
        err = (p.stderr or b'')[-2000:].decode(errors='replace')
        raise RuntimeError(
            '%s failed (%d): %s' %
            (' '.join(map(str, cmd)), p.returncode, err))
    return p


def rapidwright_available():
    return (
        RW_JAR.is_file()
        and (RW_BUILD / 'classes' / 'RwCheck.class').is_file())


def rw_batch(commands, work):
    """Runs RwCheck commands in one JVM; returns {index: None | error}."""
    if not commands:
        return {}
    src = Path(__file__).resolve().parent / 'RwCheck.java'
    cls = RW_BUILD / 'classes' / 'RwCheck.class'
    if src.stat().st_mtime > cls.stat().st_mtime:
        run(
            [
                'javac', '-nowarn', '-d', RW_BUILD / 'classes', '-cp', RW_JAR,
                src
            ])
    batch = work / ('rw-batch-%d.txt' % int(time.time() * 1000))
    batch.write_text(''.join('\t'.join(map(str, c)) + '\n' for c in commands))
    env = dict(os.environ, RAPIDWRIGHT_PATH=str(RW_BUILD))
    cmd = [
        'java', '-Xmx4g', '-cp',
        '%s:%s' % (RW_JAR, RW_BUILD / 'classes'), 'RwCheck', 'batch',
        str(batch)
    ]
    p = subprocess.run(
        cmd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=7200)
    status = {}
    for line in p.stdout.decode(errors='replace').splitlines():
        parts = line.split(' ', 2)
        if parts[0] in ('OK', 'FAIL') and len(parts) >= 2:
            status[int(parts[1]) - 1] = None if parts[0] == 'OK' else (
                parts[2] if len(parts) > 2 else 'failed')
    for i in range(len(commands)):
        status.setdefault(
            i, 'no result (java exit %d): %s' %
            (p.returncode, p.stderr.decode(errors='replace')[-500:]))
    batch.unlink()
    return status


def read_frm(path):
    """{address: [words]} of a .frm file (plain or .xz)."""
    data = path.read_bytes()
    if path.suffix == '.xz':
        data = lzma.decompress(data)
    frames = {}
    for line in data.decode().splitlines():
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        addr, words = line.split(' ', 1)
        frames[int(addr, 16)] = [int(w, 16) for w in words.split(',')]
    return frames


def sha256_lines(addresses):
    h = hashlib.sha256()
    for a in addresses:
        h.update(b'0x%08X\n' % a)
    return h.hexdigest()


def minor_bits(arch):
    return 8 if arch == 'UltraScalePlus' else 7


# ------------------------------------------------------------ .bit files


def bit_header(data):
    """The TLV fields of a .bit header: {'a': design, 'b': part, ...}."""
    fields = {}
    pos = 2 + struct.unpack('>H', data[:2])[0]
    pos += 2  # the 16-bit length (1) of the key below
    while pos < len(data):
        key = chr(data[pos])
        pos += 1
        if key == 'e':
            fields['e'] = struct.unpack('>I', data[pos:pos + 4])[0]
            break
        n = struct.unpack('>H', data[pos:pos + 2])[0]
        fields[key] = data[pos + 2:pos + 2 + n].rstrip(b'\0').decode(
            errors='replace')
        pos += 2 + n
    return fields


def bit_packets(data):
    """The packets after the first sync word, like RapidWright lists them:
    [header, word count, sha256 of the data words (big endian)]."""
    start = data.find(SYNC)
    if start < 0:
        return []
    start += 4
    n = (len(data) - start) // 4
    words = struct.unpack('>%dI' % n, data[start:start + 4 * n])
    out = []
    i = 0
    while i < len(words):
        h = words[i]
        t = h >> 29
        if t == 1:
            count = h & 0x7ff
        elif t == 2:
            count = h & 0x7ffffff
        else:
            count = 0
        body = words[i + 1:i + 1 + count]
        out.append(
            [
                '0x%08X' % h,
                len(body),
                hashlib.sha256(struct.pack('>%dI' % len(body),
                                           *body)).hexdigest()
            ])
        i += 1 + count
    return out


def fdri_sha256(data):
    """SHA-256 of all FDRI data words of a bitstream, in order."""
    h = hashlib.sha256()
    start = data.find(SYNC) + 4
    n = (len(data) - start) // 4
    words = struct.unpack('>%dI' % n, data[start:start + 4 * n])
    i = 0
    reg = None
    while i < len(words):
        w = words[i]
        t = w >> 29
        if t == 1:
            reg = (w >> 13) & 0x3fff
            count = w & 0x7ff
        elif t == 2:
            count = w & 0x7ffffff
        else:
            i += 1
            continue
        if reg == 2 and (w >> 27) & 3 == 2:
            h.update(data[start + 4 * (i + 1):start + 4 * (i + 1 + count)])
        i += 1 + count
    return h.hexdigest()


def packet_facts(data):
    """(per frame CRC: a FAR write between FDRI writes with CTL1 bit 21
    set, FDRI data words)."""
    start = data.find(SYNC) + 4
    n = (len(data) - start) // 4
    words = struct.unpack('>%dI' % n, data[start:start + 4 * n])
    i = 0
    reg = None
    ctl1 = 0
    fdri = 0
    seen_fdri = False
    per_frame = False
    while i < len(words):
        h = words[i]
        t = h >> 29
        if t == 1:
            reg = (h >> 13) & 0x3fff
            count = h & 0x7ff
        elif t == 2:
            count = h & 0x7ffffff
        else:
            i += 1
            continue
        op = (h >> 27) & 3
        if op == 2 and count:
            if reg == 24:
                ctl1 = words[i + 1]
            elif reg == 2:
                fdri += count
                seen_fdri = True
            elif reg == 1 and seen_fdri and ctl1 & CTL1_PER_FRAME_CRC:
                per_frame = True
        i += 1 + count
    return per_frame, fdri


# -------------------------------------------------------------- the parts


class Part:
    def __init__(self, family, name, part_file, arch, part_json=None):
        self.family = family
        self.name = name
        self.part_file = part_file
        self.arch = arch
        self.part_json = part_json

    def golden(self, device):
        if self.family == 'zynqusp':
            base = CORPUS / 'prjuray' / 'zynqusp'
        else:
            base = CORPUS / 'xilinx' / self.family
        return base / 'rapidwright' / ('%s.json' % device)


def db_parts():
    parts = []
    for fam in SERIES7_FAMILIES:
        root = DB_CACHE / 'prjxray-db' / fam
        if root.is_dir():
            for d in sorted(root.iterdir()):
                if (d / 'part.yaml').is_file():
                    parts.append(
                        Part(
                            fam, d.name, d / 'part.yaml', 'Series7',
                            d / 'part.json'))
    root = DB_CACHE / 'prjuray-db' / 'zynqusp'
    if root.is_dir():
        for d in sorted(root.iterdir()):
            if (d / 'part.yaml').is_file():
                parts.append(
                    Part(
                        'zynqusp', d.name, d / 'part.yaml', 'UltraScalePlus',
                        d / 'part.json'))
    return parts


def tools_test_data(work):
    """ToolsTestData.tar.gz of prjuray-tools (Vivado bitstreams of all
    three architectures), extracted into WORK; None if not built."""
    tar = ORACLE_SRC / 'prjuray-tools' / 'lib' / 'test_data'
    tar = tar / 'ToolsTestData.tar.gz'
    if not tar.is_file():
        return None
    out = work / 'ToolsTestData'
    if not (out / 'UltraScalePlus' / 'test.yaml').is_file():
        out.mkdir(parents=True, exist_ok=True)
        with tarfile.open(tar) as t:
            for m in t.getmembers():
                if m.isfile() and m.name.endswith(('.bit', '.yaml')):
                    m.name = m.name.replace('/', os.sep)
                    t.extract(m, out)
    return out


def extra_parts(work):
    """Parts of the ToolsTestData bitstreams without a database part."""
    ttd = tools_test_data(work)
    if ttd is None:
        return []
    return [
        Part(
            'kintexu', 'xcku035-sfva784-1-c', ttd / 'UltraScale' / 'part.yaml',
            'UltraScale')
    ]


def part_json_columns(part):
    """({frame address of minor 0: frame count}, idcode) from part.json."""
    d = json.loads(part.part_json.read_text())
    cols = {}
    if part.arch == 'Series7':
        for half, name in ((0, 'top'), (1, 'bottom')):
            gcr = d.get('global_clock_regions', {}).get(name) or {}
            for row, r in (gcr.get('rows') or {}).items():
                for bus, b in r['configuration_buses'].items():
                    for col, c in b['configuration_columns'].items():
                        far = (
                            BUS[bus] << 23 | half << 22 | int(row) << 17
                            | int(col) << 7)
                        cols[far] = c['frame_count']
    else:
        for row, r in d['rows'].items():
            for bus, b in r['configuration_buses'].items():
                for col, c in b['configuration_columns'].items():
                    far = BUS[bus] << 24 | int(row) << 18 | int(col) << 8
                    cols[far] = c['frame_count']
    return cols, d['idcode']


def rust_tools(arch):
    if arch == 'Series7':
        return RUST / 'xc7frames2bit', RUST / 'bitread'
    return RUST / 'xcframes2bit', RUST / 'uray-bitread'


def our_walk(part, work):
    """The Rust part walk (frames of an empty bitstream written by the
    Rust writer and read by the Rust reader) and the FDRI word count."""
    writer, reader = rust_tools(part.arch)
    d = work / 'walk' / part.name
    d.mkdir(parents=True, exist_ok=True)
    frm = d / 'empty.frm'
    frm.write_text('')
    bit = d / 'empty.bit'
    aux = d / 'empty.aux'
    run(
        [
            writer,
            '--part_file=%s' % part.part_file,
            '--architecture=%s' % part.arch,
            '--frm_file=%s' % frm,
            '--output_file=%s' % bit
        ],
        env=dict(os.environ, SOURCE_DATE_EPOCH='0'))
    run(
        [
            reader,
            '--part_file=%s' % part.part_file,
            '--architecture=%s' % part.arch,
            '--aux=%s' % aux, bit
        ])
    walk = None
    for line in aux.read_text().splitlines():
        if line.startswith('Frame addresses in bitstream:'):
            walk = [int(x, 16) for x in line.split(':', 1)[1].split()]
    _, fdri = packet_facts(bit.read_bytes())
    shutil.rmtree(d)
    return walk, fdri


def columns_of_walk(walk, arch):
    cols = {}
    mask = (1 << minor_bits(arch)) - 1
    for a in walk:
        cols[a & ~mask] = cols.get(a & ~mask, 0) + 1
    return cols


def load_rw_layout(path):
    d = json.loads(path.read_text())
    if 'columns' in d and d['columns'] and isinstance(d['columns'][0], dict):
        d['columns'] = [
            [c['far'], c['frames'], c['subtype'], c['tile_column']]
            for c in d['columns']
        ]
    return d


def compare_layout(part, rw, walk, fdri):
    """Problems (strings) of the RapidWright layout RW of PART against the
    Rust walk / FDRI word count and the part.json."""
    problems = []
    rw_cols = {int(c[0], 16): c[1] for c in rw['columns']}
    if rw['series'] != part.arch:
        problems.append('series %s, ours %s' % (rw['series'], part.arch))
    if rw['words_per_frame'] != WORDS_PER_FRAME[part.arch]:
        problems.append(
            'words per frame %d, ours %d' %
            (rw['words_per_frame'], WORDS_PER_FRAME[part.arch]))
    if rw['walk_frames'] != len(walk) or rw['walk_sha256'] != sha256_lines(
            walk):
        problems.append(
            'frame address walk differs: %d frames, ours %d' %
            (rw['walk_frames'], len(walk)))
    ours = columns_of_walk(walk, part.arch)
    if rw_cols != ours:
        only_rw = sorted(set(rw_cols) - set(ours))
        only_ours = sorted(set(ours) - set(rw_cols))
        count = sorted(
            a for a in set(rw_cols) & set(ours) if rw_cols[a] != ours[a])
        problems.append(
            'columns differ from the Rust walk: only RapidWright %s, only '
            'ours %s, frame counts %s' % (
                ['0x%08X' % a for a in only_rw[:8]],
                ['0x%08X' % a for a in only_ours[:8]], [
                    '0x%08X: %d/%d' % (a, rw_cols[a], ours[a])
                    for a in count[:8]
                ]))
    if part.part_json is not None and part.part_json.is_file():
        pj, idcode = part_json_columns(part)
        if pj != ours:
            problems.append('part.json columns differ from the Rust walk')
        rw_id = rw.get('idcode')
        if rw_id is None or int(rw_id, 16) != idcode:
            problems.append('idcode %s, part.json 0x%08X' % (rw_id, idcode))
    # (block type, half, row) groups: each is followed by the overhead
    # (pad) frames in the FDRI payload.
    row_shift = 18 if part.arch == 'UltraScalePlus' else 17
    segments = len({a >> row_shift for a in walk})
    expected = (len(walk) + rw['frame_overhead_count_per_row'] * segments
                ) * rw['words_per_frame']
    if rw['config_array_words'] != fdri:
        problems.append(
            'config array %d words, our FDRI payload %d (%d '
            'frames + %d x %d overhead frames: %d)' % (
                rw['config_array_words'], fdri, len(walk), segments,
                rw['frame_overhead_count_per_row'], expected))
    return problems


def write_golden(path, rw, parts):
    cols = ',\n'.join(
        '    ' + json.dumps(c, separators=(', ', ': ')) for c in rw['columns'])
    head = {
        'rapidwright': RW_TAG,
        'device': rw['device'],
        'series': rw['series'],
        'idcode': rw['idcode'],
        'words_per_frame': rw['words_per_frame'],
        'frame_overhead_count_per_row': rw['frame_overhead_count_per_row'],
        'config_array_words': rw['config_array_words'],
        'walk_frames': rw['walk_frames'],
        'walk_sha256': rw['walk_sha256'],
        'parts': sorted(parts),
    }
    text = json.dumps(head, indent=2)[:-2]
    text += ',\n  "columns": [\n%s\n  ]\n}\n' % cols
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def check_layout(args, work):
    parts = db_parts() + extra_parts(work)
    if args.parts:
        wanted = set(args.parts.split(','))
        parts = [p for p in parts if p.name in wanted]
    out = work / 'layout'
    out.mkdir(parents=True, exist_ok=True)
    cmds = [('layout', p.name, out / ('%s.json' % p.name)) for p in parts]
    status = rw_batch(cmds, work)
    results = []
    goldens = {}
    for i, part in enumerate(parts):
        res = {'part': part.name, 'family': part.family, 'arch': part.arch}
        if status[i] is not None:
            res.update(
                status='different', problems=['RapidWright: ' + status[i]])
            results.append(res)
            continue
        rw = load_rw_layout(out / ('%s.json' % part.name))
        walk, fdri = our_walk(part, work)
        problems = compare_layout(part, rw, walk, fdri)
        res.update(
            device=rw['device'],
            frames=rw['walk_frames'],
            columns=len(rw['columns']),
            problems=problems,
            status='different' if problems else 'identical')
        results.append(res)
        key = (part.golden(rw['device']), rw['walk_sha256'])
        goldens.setdefault(key, (rw, []))[1].append(part.name)
    if args.write_golden:
        paths = {}
        for (path, _), (rw, names) in goldens.items():
            if path in paths:
                raise SystemExit(
                    '%s: parts with different layouts: %s / %s' %
                    (path, paths[path], names))
            paths[path] = names
            write_golden(path, rw, names)
    return results


# ------------------------------------------------------ bitstream checks


class BitCase:
    def __init__(self, name, bit, part_file, arch, source):
        self.name = name
        self.bit = bit
        self.part_file = part_file
        self.arch = arch
        self.source = source


def db_part_file(hdr_part):
    """The prjxray-db / prjuray-db part.yaml of a .bit header part name."""
    name = hdr_part if hdr_part.startswith('xc') else 'xc' + hdr_part
    for root in (DB_CACHE / 'prjxray-db', DB_CACHE / 'prjuray-db'):
        if not root.is_dir():
            continue
        for fam in sorted(root.iterdir()):
            if not fam.is_dir():
                continue
            for cand in (name, name + '-1'):
                if (fam / cand / 'part.yaml').is_file():
                    arch = (
                        'UltraScalePlus'
                        if root.name == 'prjuray-db' else 'Series7')
                    return fam / cand / 'part.yaml', arch
    return None, None


def bit_cases(args, work):
    cases = []

    def add(name, bit, source, part_file=None, arch=None):
        if not bit.is_file():
            return
        if part_file is None:
            part_file, arch = db_part_file(
                bit_header(bit.read_bytes()[:512]).get('b', ''))
            if part_file is None:
                return
        cases.append(BitCase(name, bit, part_file, arch, source))

    harness = (DB_CACHE / 'prjxray-db').glob('*/harness/*/*/design.bit')
    for bit in sorted(harness):
        rel = bit.relative_to(DB_CACHE / 'prjxray-db')
        add('prjxray-db/%s' % rel, bit, 'vivado')
    td = ORACLE_SRC / 'prjxray' / 'lib' / 'test_data'
    for n in ('configuration_test.bit', 'configuration_test.debug.bit',
              'configuration_test.perframecrc.bit'):
        add(
            'prjxray/test_data/%s' % n, td / n, 'vivado',
            td / 'configuration_test.yaml', 'Series7')
    ttd = tools_test_data(work)
    if ttd is not None:
        for arch, files in (('Series7', (('design.bit', 'part.yaml'),
                                         ('bram.bit', 'part.yaml'))),
                            ('UltraScale', (('design.bit', 'part.yaml'), )),
                            ('UltraScalePlus', (('design.bit', 'part.yaml'),
                                                ('test.bit', 'test.yaml')))):
            for bit, yaml in files:
                add(
                    'ToolsTestData/%s/%s' % (arch, bit), ttd / arch / bit,
                    'vivado', ttd / arch / yaml, arch)
    add(
        'corpus/xilinx/artix7/smoke_x1y0.bit',
        CORPUS / 'xilinx' / 'artix7' / 'smoke_x1y0.bit', 'prjxray')
    for out in args.flow_out:
        root = Path(out)
        if not root.is_dir():
            continue
        for bit in sorted(root.rglob('*.bit')):
            if bit.name.endswith('.rerun.bit') or 'rapidwright' in bit.parts:
                continue
            add('flow/%s' % bit.relative_to(root), bit, 'flow')
    if args.quick:
        keep = (
            'prjxray-db/artix7/harness/arty-a7/swbut/design.bit',
            'prjxray/test_data/configuration_test.bit',
            'prjxray/test_data/configuration_test.perframecrc.bit',
            'ToolsTestData/Series7/bram.bit',
            'ToolsTestData/UltraScale/design.bit',
            'ToolsTestData/UltraScalePlus/design.bit',
            'corpus/xilinx/artix7/smoke_x1y0.bit')
        cases = [c for c in cases if c.name in keep]
    if args.cases:
        cases = [c for c in cases if args.cases in c.name]
    return cases


def rw_part_name(hdr_part):
    return hdr_part if hdr_part.startswith('xc') else 'xc' + hdr_part


def our_read(case_arch, part_file, bit, frm):
    _, reader = rust_tools(case_arch)
    run(
        [
            reader, '-C',
            '--part_file=%s' % part_file,
            '--architecture=%s' % case_arch,
            '--frm_out=%s' % frm, bit
        ])
    return read_frm(frm)


def our_write(case_arch, part_file, frm, bit, part_name):
    # RapidWright identifies the part by the header's part name: without
    # --part_name (empty field b) its reader fails.
    writer, _ = rust_tools(case_arch)
    run(
        [
            writer,
            '--part_file=%s' % part_file,
            '--part_name=%s' % part_name,
            '--architecture=%s' % case_arch,
            '--frm_file=%s' % frm,
            '--output_file=%s' % bit
        ],
        env=dict(os.environ, SOURCE_DATE_EPOCH='0'))


def compare_frames(rw_frm, ours, walk_words):
    """(status, detail): RW_FRM is RapidWright's dense walk (list of
    (address, words)), OURS the Rust reader's {address: words}."""
    zero = [0] * walk_words
    rw = dict(rw_frm)
    only_ours = sorted(set(ours) - set(rw))
    diff = [a for a, w in rw_frm if ours.get(a, zero) != w]
    if not diff and not only_ours:
        return 'identical', '%d frames' % len(rw_frm)
    return 'different', '%d of %d frames differ (first %s), %d frames ' \
        'only in ours' % (len(diff), len(rw_frm), ['0x%08X' % a for a in
                                                   diff[:4]], len(only_ours))


def per_frame_crc_model(order, ours, arch):
    """What RapidWright reads from a per frame CRC bitstream whose frames
    we read as OURS ({address: words}), ORDER being the part walk: in
    every row (block type, half, row) the frame at walk position i holds
    what we read at position i + 1, the last frame of the row holds the
    first pad frame after it (zero); the first frame of a row is lost."""
    zero = [0] * WORDS_PER_FRAME[arch]
    shift = 18 if arch == 'UltraScalePlus' else 17
    out = {}
    for i, a in enumerate(order):
        nxt = order[i + 1] if i + 1 < len(order) else None
        if nxt is not None and nxt >> shift == a >> shift:
            out[a] = ours.get(nxt, zero)
        else:
            out[a] = zero
    return out


def fdri_packets(data):
    """The data words of every FDRI write packet, in order."""
    start = data.find(SYNC) + 4
    n = (len(data) - start) // 4
    words = struct.unpack('>%dI' % n, data[start:start + 4 * n])
    out = []
    i = 0
    reg = None
    while i < len(words):
        w = words[i]
        t = w >> 29
        if t == 1:
            reg = (w >> 13) & 0x3fff
            count = w & 0x7ff
        elif t == 2:
            count = w & 0x7ffffff
        else:
            i += 1
            continue
        if reg == 2 and (w >> 27) & 3 == 2 and count:
            out.append(list(words[i + 1:i + 1 + count]))
        i += 1 + count
    return out


def explain_per_frame_crc(rw_frm, ours, arch, bit):
    """None if RapidWright's reading RW_FRM of the per frame CRC bitstream
    BIT is not explained by `per_frame_crc_model`, else the explanation.

    One more exception is verified exactly: the prjxray reader (and so
    the Rust reader) writes the pad frame that follows the part's last
    frame over that frame (`GetNextFrameAddress` has no next address, so
    the current address is not advanced for the next FDRI packet), while
    RapidWright has the lost frame at the address before."""
    order = [a for a, _ in rw_frm]
    model = per_frame_crc_model(order, ours, arch)
    bad = [i for i, (a, w) in enumerate(rw_frm) if w != model[a]]
    if not bad:
        return (
            'per frame CRC: RapidWright reads the frames of every row '
            'one frame address early (model verified on all %d frames)' %
            len(rw_frm))
    if bad != [len(order) - 2]:
        return None
    packets = fdri_packets(bit.read_bytes())
    if len(packets) < 2:
        return None
    lost, pad = packets[-2], packets[-1]
    zero = [0] * WORDS_PER_FRAME[arch]
    if (rw_frm[-2][1] == lost and ours.get(order[-1], zero) == pad
            and pad == zero and lost != zero):
        return (
            'per frame CRC: RapidWright reads the frames of every row '
            'one frame address early (model verified on all %d '
            'frames); the last frame of the part (0x%08X) is lost by the '
            'prjxray-compatible reader (overwritten by the trailing pad '
            'frame), RapidWright has it at 0x%08X' %
            (len(rw_frm), order[-1], order[-2]))
    return None


def read_rw_frm(path):
    out = []
    for line in path.read_text().splitlines():
        addr, words = line.split(' ', 1)
        out.append((int(addr, 16), [int(w, 16) for w in words.split(',')]))
    return out


def compare_header_packets(bit, rw_json):
    data = bit.read_bytes()
    hdr = bit_header(data[:1024])
    problems = []
    # RapidWright splits field a before the first ';' into the design
    # name and the options (which keep the ';').
    rw_a = rw_json['design_name'] + rw_json['options']
    for key, rw_value in (('a', rw_a), ('b', rw_json['part_name']),
                          ('c', rw_json['date']), ('d', rw_json['time'])):
        if hdr.get(key, '') != rw_value:
            problems.append(
                'header field %s: %r, RapidWright %r' %
                (key, hdr.get(key), rw_value))
    ours = bit_packets(data)
    theirs = [p[:3] for p in rw_json['packets']]
    if ours != theirs:
        first = next(
            (i for i, (a, b) in enumerate(zip(ours, theirs)) if a != b),
            min(len(ours), len(theirs)))
        problems.append(
            'packets: %d, RapidWright %d, first difference at %d: %s / %s' % (
                len(ours), len(theirs), first, ours[first:first + 1],
                theirs[first:first + 1]))
    return problems


def check_bits(args, work):
    cases = [('bit', c) for c in bit_cases(args, work)]
    cases += [('frm', c) for c in frm_corpus_cases(args)]
    results = []
    d = work / 'bits'
    # A few cases at a time: the frames of an xc7a200t bitstream are 25 MB
    # of .frm text per file, and each case has up to six of them.
    for start in range(0, len(cases), args.chunk):
        chunk = cases[start:start + args.chunk]
        results += _check_bit_chunk(chunk, d, work, start)
        shutil.rmtree(d, ignore_errors=True)
        done = start + len(chunk)
        print('bits: %d/%d cases' % (done, len(cases)), file=sys.stderr)
    return results


def _check_bit_chunk(chunk, d, work, start):
    # Phase 1 (Rust): read the originals, write our bitstreams.
    for i, (kind, c) in enumerate(chunk):
        c.dir = d / ('%04d' % (start + i))
        c.dir.mkdir(parents=True, exist_ok=True)
        if kind == 'bit':
            c.ours = our_read(c.arch, c.part_file, c.bit, c.dir / 'ours.frm')
            c.hdr_part = bit_header(c.bit.read_bytes()[:1024]).get('b', '')
            our_write(
                c.arch, c.part_file, c.dir / 'ours.frm', c.dir / 'ours.bit',
                c.hdr_part)
        else:
            src = c.dir / 'input.frm'
            data = c.frm.read_bytes()
            if c.frm.suffix == '.xz':
                data = lzma.decompress(data)
            src.write_bytes(data)
            our_write(c.arch, c.part_file, src, c.dir / 'ours.bit', c.rw_part)
            c.ours = our_read(
                c.arch, c.part_file, c.dir / 'ours.bit', c.dir / 'ours.frm')
    # Phase 2 (RapidWright): read everything, write from our frames.
    cmds = []
    for kind, c in chunk:
        c.cmd = len(cmds)
        if kind == 'bit':
            cmds += [
                ('read', c.bit, c.dir / 'rw.frm', c.dir / 'rw.json'),
                (
                    'read', c.dir / 'ours.bit', c.dir / 'rw-ours.frm',
                    c.dir / 'rw-ours.json'),
                (
                    'write', rw_part_name(c.hdr_part), c.dir / 'ours.frm',
                    c.dir / 'rw-written.bit'),
                ('rewrite', c.bit, c.dir / 'rw-rewritten.bit')
            ]
        else:
            cmds += [
                (
                    'read', c.dir / 'ours.bit', c.dir / 'rw-ours.frm',
                    c.dir / 'rw-ours.json'),
                (
                    'write', c.rw_part, c.dir / 'input.frm',
                    c.dir / 'rw-written.bit')
            ]
    status = rw_batch(cmds, work)
    # Phase 3: compare.
    results = []
    for kind, c in chunk:
        if kind == 'bit':
            results.append(compare_bit_case(c, status))
        else:
            results.append(compare_frm_case(c, status))
        c.ours = None
    return results


def _step(res, name, status, detail):
    res['checks'][name] = {'status': status, 'detail': detail}


def compare_bit_case(c, status):
    words = WORDS_PER_FRAME[c.arch]
    res = {'case': c.name, 'source': c.source, 'arch': c.arch, 'checks': {}}
    per_frame, _ = packet_facts(c.bit.read_bytes())
    res['per_frame_crc'] = per_frame
    # RapidWright reader on the original.
    if status[c.cmd] is None:
        rw = read_rw_frm(c.dir / 'rw.frm')
        st, det = compare_frames(rw, c.ours, words)
        if st != 'identical' and per_frame:
            why = explain_per_frame_crc(rw, c.ours, c.arch, c.bit)
            if why is not None:
                st, det = 'explained', why
        _step(res, 'read frames', st, det)
        rw_json = json.loads((c.dir / 'rw.json').read_text())
        probs = compare_header_packets(c.bit, rw_json)
        _step(
            res, 'read header+packets', 'different' if probs else 'identical',
            '; '.join(probs) or '%d packets' % len(rw_json['packets']))
    else:
        _step(res, 'read frames', 'different', status[c.cmd])
    # RapidWright reader on the Rust writer's bitstream of our frames.
    if status[c.cmd + 1] is None:
        rw = read_rw_frm(c.dir / 'rw-ours.frm')
        _step(
            res, 'rust writer -> rw reader', *compare_frames(
                rw, c.ours, words))
        probs = compare_header_packets(
            c.dir / 'ours.bit', json.loads(
                (c.dir / 'rw-ours.json').read_text()))
        _step(
            res, 'rust writer -> rw header+packets',
            'different' if probs else 'identical', '; '.join(probs))
    else:
        _step(res, 'rust writer -> rw reader', 'different', status[c.cmd + 1])
    # RapidWright writer (our frames) -> Rust reader.
    _rw_written(
        res, c, 'rw writer -> rust reader', status[c.cmd + 2],
        c.dir / 'rw-written.bit', c.ours, None)
    # RapidWright rewrite of the original -> Rust reader.
    _rw_written(
        res, c, 'rw rewrite -> rust reader', status[c.cmd + 3],
        c.dir / 'rw-rewritten.bit', c.ours, per_frame)
    _fdri(res, c.dir / 'ours.bit', c.dir / 'rw-written.bit')
    # Information only: do the Rust and the RapidWright writer give the
    # original's FDRI payload / packet list back?
    if (c.dir / 'rw-written.bit').is_file():
        orig = c.bit.read_bytes()
        rw_bit = (c.dir / 'rw-written.bit').read_bytes()
        ours_bit = (c.dir / 'ours.bit').read_bytes()
        res['original_fdri_equals'] = {
            'rust writer': fdri_sha256(ours_bit) == fdri_sha256(orig),
            'rw writer': fdri_sha256(rw_bit) == fdri_sha256(orig),
        }
        res['original_packets_equal'] = {
            'rust writer': bit_packets(ours_bit) == bit_packets(orig),
            'rw writer': bit_packets(rw_bit) == bit_packets(orig),
        }
    res['status'] = overall(res)
    return res


def _rw_written(res, c, name, err, bit, expect, per_frame):
    if err is not None:
        _step(res, name, 'different', err)
        return
    words = WORDS_PER_FRAME[c.arch]
    got = our_read(c.arch, c.part_file, bit, bit.with_suffix('.frm'))
    zero = [0] * words
    walk = sorted(set(got) | set(expect))
    diff = [a for a in walk if got.get(a, zero) != expect.get(a, zero)]
    if not diff:
        _step(res, name, 'identical', '%d frames' % len(got))
        return
    if per_frame:
        # RapidWright wrote what it read.
        order = [a for a, _ in read_rw_frm(c.dir / 'rw.frm')]
        shifted = per_frame_crc_model(order, expect, c.arch)
        if all(got.get(a, zero) == shifted[a] for a in order):
            _step(
                res, name, 'explained', 'per frame CRC: the frames '
                'RapidWright read one address early, written back')
            return
    _step(
        res, name, 'different', '%d frames differ (first %s)' %
        (len(diff), ['0x%08X' % a for a in diff[:4]]))


def _fdri(res, ours, rw_written):
    """The FDRI payloads (frames, pad frames, ECC) of the Rust and the
    RapidWright writer for the same frames must be equal."""
    if not rw_written.is_file():
        return
    a = fdri_sha256(ours.read_bytes())
    b = fdri_sha256(rw_written.read_bytes())
    _step(
        res, 'fdri payload rust writer = rw writer',
        'identical' if a == b else 'different',
        '' if a == b else 'sha256 %s / %s' % (a[:12], b[:12]))


def overall(res):
    sts = {s['status'] for s in res['checks'].values()}
    if 'different' in sts:
        return 'different'
    return 'explained' if 'explained' in sts else 'identical'


class FrmCase:
    def __init__(self, name, frm, part_file, arch, rw_part):
        self.name = name
        self.frm = frm
        self.part_file = part_file
        self.arch = arch
        self.rw_part = rw_part


def frm_corpus_cases(args):
    cases = []
    xilinx = CORPUS / 'xilinx'
    for dt in sorted(xilinx.glob('*/designs/**/difftest.json')):
        meta = json.loads(dt.read_text())
        part_file = DB_CACHE / 'prjxray-db' / meta['family'] / meta[
            'part'] / 'part.yaml'
        if not part_file.is_file():
            continue
        for frm in sorted(dt.parent.glob('*.frm*')):
            cases.append(
                FrmCase(
                    str(frm.relative_to(CORPUS)), frm, part_file, 'Series7',
                    meta['part']))
    smoke = xilinx / 'artix7' / 'smoke_x1y0.frm'
    pf = DB_CACHE / 'prjxray-db' / 'artix7' / 'xc7a35tcsg324-1' / 'part.yaml'
    if smoke.is_file() and pf.is_file():
        cases.append(
            FrmCase(
                str(smoke.relative_to(CORPUS)), smoke, pf, 'Series7',
                'xc7a35tcsg324-1'))
    if args.quick:
        cases = cases[:3] + cases[-1:]
    if args.cases:
        cases = [c for c in cases if args.cases in c.name]
    return cases


def compare_frm_case(c, status):
    words = WORDS_PER_FRAME[c.arch]
    res = {
        'case': c.name,
        'source': 'corpus .frm',
        'arch': c.arch,
        'checks': {}
    }
    if status[c.cmd] is None:
        rw = read_rw_frm(c.dir / 'rw-ours.frm')
        _step(
            res, 'rust writer -> rw reader', *compare_frames(
                rw, c.ours, words))
        probs = compare_header_packets(
            c.dir / 'ours.bit', json.loads(
                (c.dir / 'rw-ours.json').read_text()))
        _step(
            res, 'rust writer -> rw header+packets',
            'different' if probs else 'identical', '; '.join(probs))
    else:
        _step(res, 'rust writer -> rw reader', 'different', status[c.cmd])
    # RapidWright writes the .frm (computing its own ECC): our reader must
    # give what our writer (our ECC) gave.
    _rw_written(
        res, c, 'rw writer -> rust reader', status[c.cmd + 1],
        c.dir / 'rw-written.bit', c.ours, None)
    _fdri(res, c.dir / 'ours.bit', c.dir / 'rw-written.bit')
    res['status'] = overall(res)
    return res


# ------------------------------------------------------------------- main


def summarize(kind, results, key):
    counts = {}
    for r in results:
        counts[r['status']] = counts.get(r['status'], 0) + 1
    print(
        '%s: %d cases: %s' % (
            kind, len(results), ', '.join(
                '%s %d' % kv for kv in sorted(counts.items()))))
    for r in results:
        if r['status'] != 'identical':
            print('  %-9s %s' % (r['status'], r[key]))
            for name, s in r.get('checks', {}).items():
                if s['status'] != 'identical':
                    print(
                        '      %s: %s: %s' % (name, s['status'], s['detail']))
            for p in r.get('problems', []):
                print('      ' + p)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument(
        'what', nargs='*', help='layout and/or bits (default: '
        'both)')
    ap.add_argument(
        '--work-dir',
        default=str(
            REPO_ROOT / 'tools' / 'e2e' / 'build' / 'rapidwright-work'))
    ap.add_argument('--parts', help='comma separated part names (layout)')
    ap.add_argument(
        '--cases',
        help='only the bitstream / .frm cases whose name '
        'contains this')
    ap.add_argument(
        '--flow-out',
        action='append',
        default=[],
        help='directory with flow built top.bit files (f4pga / '
        'openXC7 outputs); may be repeated')
    ap.add_argument(
        '--quick', action='store_true', help='a few parts and bitstreams only')
    ap.add_argument(
        '--write-golden',
        action='store_true',
        help='store the RapidWright layouts in tests/corpus')
    ap.add_argument('--report', help='write the results as JSON here')
    ap.add_argument(
        '--chunk',
        type=int,
        default=4,
        help='bitstream cases per RapidWright run (disk use)')
    args = ap.parse_args(argv)
    args.what = args.what or ['layout', 'bits']
    if set(args.what) - {'layout', 'bits'}:
        ap.error('unknown check: %s' % ' '.join(args.what))
    if not rapidwright_available():
        print(
            'RapidWright is not set up: run tools/e2e/setup-rapidwright.sh',
            file=sys.stderr)
        return 2
    work = Path(args.work_dir)
    work.mkdir(parents=True, exist_ok=True)
    if args.quick and not args.parts:
        args.parts = 'xc7a35tcsg324-1,xc7z010clg400-1,xczu3eg-sfvc784-1-e'
    report = {'rapidwright': RW_TAG}
    if 'layout' in args.what:
        report['layout'] = check_layout(args, work)
        summarize('layout', report['layout'], 'part')
    if 'bits' in args.what:
        report['bits'] = check_bits(args, work)
        summarize('bits', report['bits'], 'case')
    if args.report:
        Path(args.report).write_text(json.dumps(report, indent=1) + '\n')
    bad = [
        r for v in report.values() if isinstance(v, list) for r in v
        if r['status'] == 'different'
    ]
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())

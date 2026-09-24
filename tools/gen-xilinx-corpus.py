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
"""Deterministic, stdlib-only synthetic FASM generator for a part of a
prjxray-db family (T5.9).

    tools/gen-xilinx-corpus.py --db-root <db>/artix7 --part xc7a35tcsg324-1 \\
        --out-dir OUT [--tiles first|sample N|all] [--seed S]

writes

* ``OUT/features.fasm``: FASM that sets every segbits feature
  (``segbits_<type>.db`` and ``segbits_<type>.block_ram.db``) and every
  pseudo PIP (``ppips_<type>.db``) of every tile type present in the part's
  ``tilegrid.json`` (alias tiles, e.g. ``LIOB33_SING`` or
  ``HCLK_L_BOT_UTURN``, with the aliased type's features under their own
  names) at least once, spread over tiles of that type, conflict free: no
  two features (nor the part's ``required_features.fasm``, nor the STEPDOWN
  features the reference adds for a bank) set and clear the same bit.
  Multi bit features (``INIT[63:0]``, block RAM ``INIT_xx[255:0]``) are
  written as ranges whose value has one bit per address placed on that
  tile, in several formats (``W'h``, ``'h``, ``W'b``, ``W'd``, plain
  decimal, short ``W'o``; single addresses as ``F[n]``, ``F[n] = 1``,
  ``F[n:n] = 1'b1``), plus ``= 0`` disables of features that do not fit
  on the tile (no bits, no lookup), annotations and comments.
* ``OUT/errors/*.fasm``: small files for the reference error paths:
  batched ``FasmLookupError`` (unknown features, addresses missing from the
  segbits, a range with bits beyond the last address), a tile absent from
  the part's grid after lookup errors (``KeyError``, earlier errors lost),
  a value that does not fit its range (the reference's ANTLR value range
  error), conflicting features (``FasmInconsistentBits``) and a STEPDOWN
  feature on an IOB tile without a package pin (``KeyError``).
* ``OUT/manifest.json``: the options, per tile type counts (features,
  tiles used, lines) and every feature that could not be placed, with the
  reason.

Feature lookup and bit positions follow prjxray (``tile_segbits.py``,
``tile_segbits_alias.py``, ``fasm_assembler.py``) and f4pga-xc-fasm's
``fasm2frames.py`` (STEPDOWN propagation, PUDC_B), see
docs/rewrite/DESIGN-xilinx-db.md §5 and §8.9. ``--tiles``:

* ``first``: the tiles of each type in ``tilegrid.json`` order; each
  feature goes on the first of them where it fits (the smallest file);
* ``sample N`` (default, N=3): N tiles of each type spread over the grid
  (one random tile in each of N equal slices of the type's tile list), each
  given a random conflict free subset of the type's features (each feature
  with probability ``--density``), then every feature not placed yet on the
  first of them (or of further random tiles) where it fits;
* ``all``: every tile of the grid, like ``sample`` (big: millions of lines
  for the large parts; ``--max-per-tile`` bounds the random subsets).

Tiles kept free of generated features: the PUDC_B tile (so
``--emit_pudc_b_pullup`` adds its pull-up) and the IOB/HCLK_IOI3 tiles of
the banks chosen for the STEPDOWN features (they receive the reference's
STEPDOWN propagation), unless a tile type has no other tile.

The same arguments always give byte identical output (``--seed``, default
0).
"""
import argparse
import csv
import json
import os
import random
import re
import sys

CLB_IO_CLK = 'CLB_IO_CLK'
BLOCK_RAM = 'BLOCK_RAM'
WORD_BITS = 32
FRAME_WORDS = 101
GENERATOR_VERSION = 1

PUDC_FEATURES = (
    'LVCMOS12_LVCMOS15_LVCMOS18_LVCMOS25_LVCMOS33_LVDS_25_LVTTL_SSTL135_'
    'SSTL15_TMDS_33.IN_ONLY', 'LVCMOS25_LVCMOS33_LVTTL.IN', 'PULLTYPE.PULLUP')

ANNOTATION_KEYS = ('src', 'net', 'cell', 'note')


# ---------------------------------------------------------------------
# Database files (a stdlib-only reader of what prjxray reads).
# ---------------------------------------------------------------------
def read_simple_yaml(path):
    """The two level mappings of mapping/parts.yaml and devices.yaml:
    {key: {subkey: value}} (quotes removed)."""
    out = {}
    current = None
    with open(path) as f:
        for line in f:
            if not line.strip() or line.lstrip().startswith('#'):
                continue
            key, _, value = line.strip().partition(':')
            key = key.strip().strip('"\'')
            value = value.strip().strip('"\'')
            if not line[0].isspace():
                current = out.setdefault(key, {})
            elif current is not None:
                current[key] = value
    return out


def family_parts(db_root):
    """The parts of mapping/parts.yaml, in file order."""
    return list(read_simple_yaml(os.path.join(db_root, 'mapping',
                                              'parts.yaml')))


def fabric_of(db_root, part):
    parts = read_simple_yaml(os.path.join(db_root, 'mapping', 'parts.yaml'))
    devices = read_simple_yaml(
        os.path.join(db_root, 'mapping', 'devices.yaml'))
    return devices[parts[part]['device']]['fabric']


def parse_bit(text):
    isset = True
    if text.startswith('!'):
        isset = False
        text = text[1:]
    column, bit = text.split('_')
    return (int(column), int(bit), isset)


def read_segbits(path):
    """{key: [(word_column, word_bit, isset)]}, key order kept."""
    out = {}
    with open(path) as f:
        for line in f:
            parts = line.split()
            if len(parts) < 2:
                continue
            out[parts[0]] = [parse_bit(p) for p in parts[1:]]
    return out


def read_ppips(path):
    out = {}
    with open(path) as f:
        for line in f:
            parts = line.split()
            if len(parts) == 2:
                out[parts[0]] = parts[1]
    return out


class TileTypeDb(object):
    """prjxray's TileSegbits of one tile type: the segbits of each block
    type, the pseudo PIPs and the addressed features."""

    def __init__(self, db_root, tile_type):
        lower = tile_type.lower()
        self.tile_type = tile_type
        self.segbits = {}
        self.ppips = {}
        path = os.path.join(db_root, 'ppips_%s.db' % lower)
        if os.path.isfile(path):
            self.ppips = read_ppips(path)
        path = os.path.join(db_root, 'segbits_%s.db' % lower)
        if os.path.isfile(path):
            self.segbits[CLB_IO_CLK] = read_segbits(path)
        path = os.path.join(db_root, 'segbits_%s.block_ram.db' % lower)
        if os.path.isfile(path):
            self.segbits[BLOCK_RAM] = read_segbits(path)
        self.feature_addresses = {}
        for block_type, table in self.segbits.items():
            for key in table:
                open_ = key.rfind('[')
                if open_ != -1:
                    address = int(key[open_ + 1:key.rfind(']')])
                    self.feature_addresses.setdefault(
                        key[:open_], {})[address] = (block_type, key)

    def feature_to_bits(self, key, address):
        """prjxray's TileSegbits.feature_to_bits: None for a pseudo PIP,
        else [(block_type, (word_column, word_bit, isset))]; KeyError if
        the feature does not exist."""
        if key in self.ppips:
            return None
        for block_type, table in self.segbits.items():
            if address == 0 and key in table:
                return [(block_type, bit) for bit in table[key]]
        block_type, name = self.feature_addresses[key][address]
        return [(block_type, bit) for bit in self.segbits[block_type][name]]


class Database(object):
    """The part: grid, tile types, package pins and IO banks."""

    def __init__(self, db_root, part):
        self.db_root = db_root
        self.part = part
        self.fabric = fabric_of(db_root, part)
        with open(os.path.join(db_root, self.fabric, 'tilegrid.json')) as f:
            self.grid = json.load(f)
        self.known_types = set()
        for f in os.listdir(db_root):
            if f.startswith('tile_type_') and f.endswith('.json'):
                name = f[len('tile_type_'):-len('.json')]
                self.known_types.add(name.upper())
        self.tile_dbs = {}
        self.tile_to_bank = {}
        self.bank_to_tiles = {}
        part_dir = os.path.join(db_root, part)
        pins = os.path.join(part_dir, 'package_pins.csv')
        part_json = os.path.join(part_dir, 'part.json')
        if os.path.isfile(pins) and os.path.isfile(part_json):
            with open(part_json) as f:
                for bank, loc in json.load(f).get('iobanks', {}).items():
                    tile = 'HCLK_IOI3_' + loc
                    self.bank_to_tiles.setdefault(bank, []).append(tile)
                    self.tile_to_bank[tile] = bank
            with open(pins) as f:
                for row in csv.DictReader(f):
                    tiles = self.bank_to_tiles.setdefault(row['bank'], [])
                    if row['tile'] not in tiles:
                        tiles.append(row['tile'])
                    self.tile_to_bank[row['tile']] = row['bank']
        self.required_features = []
        path = os.path.join(part_dir, 'required_features.fasm')
        if os.path.isfile(path):
            with open(path) as f:
                for line in f:
                    line = line.strip()
                    if line and line not in self.required_features:
                        self.required_features.append(line)

    def tile_db(self, tile_type):
        if tile_type not in self.tile_dbs:
            self.tile_dbs[tile_type] = TileTypeDb(self.db_root, tile_type)
        return self.tile_dbs[tile_type]

    def bits_blocks(self, tile):
        """{block_type: (base address, offset, frames, alias or None)}."""
        out = {}
        for block_type, b in self.grid[tile].get('bits', {}).items():
            out[block_type] = (int(b['baseaddr'], 0), b['offset'], b['frames'],
                               b.get('alias'))
        return out

    def pudc_b(self):
        """find_pudc_b: (tile, site) of the PUDC_B pin, None, or 'many'."""
        found = None
        for tile, info in self.grid.items():
            for site, function in info.get('pin_functions', {}).items():
                if 'PUDC_B' in function:
                    if found is not None:
                        return 'many'
                    found = (tile, 'IOB_Y%d' % (int(site[-1]) % 2))
        return found


# ---------------------------------------------------------------------
# Features of a tile and their bits.
# ---------------------------------------------------------------------
class Unit(object):
    """One FASM level feature bit: `name` (plain) or `name[address]`,
    with its resolved bits relative to the tile's bits blocks."""
    __slots__ = ('name', 'address', 'bits', 'order')

    def __init__(self, name, address, bits, order):
        self.name = name
        self.address = address
        self.bits = bits  # None: pseudo PIP; else [(block, col, bit, set)]
        self.order = order

    def label(self):
        if self.address is None:
            return self.name
        return '%s[%d]' % (self.name, self.address)


class TileFeatures(object):
    """The features a tile of a given type (and alias) accepts: prjxray's
    lookup (TileSegbits or TileSegbitsAlias) run for every segbits key and
    pseudo PIP. Shared by the tiles of the same (type, alias)."""

    def __init__(self, db, tile_type, alias):
        self.tile_type = tile_type
        self.units = []
        self.skipped = []  # (label, reason)
        own = db.tile_db(tile_type)
        if alias is None:
            source = own
            source_type = tile_type
            sites = {}
        else:
            source_type = alias['type']
            source = db.tile_db(source_type)
            sites = alias['sites']
        rev_sites = dict((v, k) for k, v in sites.items())
        seen = set()

        def to_own(key):
            """map_feature_from_segbits, checked with the forward map."""
            parts = key.split('.')
            if alias is not None and len(parts) > 1 and parts[1] in rev_sites:
                parts[1] = rev_sites[parts[1]]
            name = '.'.join(parts[1:])
            if alias is not None:
                forward = name.split('.')
                if forward[0] in sites:
                    forward[0] = sites[forward[0]]
                if '%s.%s' % (source_type, '.'.join(forward)) != key:
                    return None
            return name

        def resolve(name, address):
            own_key = '%s.%s' % (tile_type, name)
            if alias is not None:
                if own_key in own.ppips:
                    return None
                parts = own_key.split('.')
                parts[0] = source_type
                if len(parts) > 1 and parts[1] in sites:
                    parts[1] = sites[parts[1]]
                key = '.'.join(parts)
            else:
                key = own_key
            bits = source.feature_to_bits(key, address)
            if bits is None:
                return None
            return [(b, c, w, s) for b, (c, w, s) in bits]

        candidates = []
        for block_type in (CLB_IO_CLK, BLOCK_RAM):
            for key in source.segbits.get(block_type, {}):
                if not key.startswith(source_type + '.'):
                    self.skipped.append((key, 'segbits key of another type'))
                    continue
                name = to_own(key)
                if name is None:
                    self.skipped.append((key, 'unreachable through alias'))
                    continue
                open_ = name.rfind('[')
                if open_ == -1:
                    candidates.append((name, None, key))
                else:
                    candidates.append(
                        (name[:open_], int(name[open_ + 1:name.rfind(']')]),
                         key))
        for key in own.ppips:
            if key.startswith(tile_type + '.'):
                candidates.append((key[len(tile_type) + 1:], None, key))
        plain_names = set(n for n, a, _ in candidates if a is None)
        for name, address, key in candidates:
            if (name, address) in seen:
                continue
            seen.add((name, address))
            if address == 0 and name in plain_names:
                # prjxray looks `name[0]` up as the plain `name` first.
                self.skipped.append((key, 'shadowed by the plain feature'))
                continue
            try:
                bits = resolve(name, address or 0)
            except KeyError:
                self.skipped.append((key, 'lookup fails'))
                continue
            self.units.append(Unit(name, address, bits, len(self.units)))
        self.by_label = dict((u.label(), u) for u in self.units)
        self.max_address = {}
        for unit in self.units:
            if unit.address is not None:
                self.max_address[unit.name] = max(
                    self.max_address.get(unit.name, 0), unit.address)

    def find(self, name, address):
        """The unit prjxray's lookup of `name` (`name[address]`) gives."""
        if address is None or address == 0:
            unit = self.by_label.get(name)
            if unit is not None:
                return unit
            address = 0
        return self.by_label.get('%s[%d]' % (name, address))


def tile_features(db, cache, tile):
    info = db.grid[tile]
    alias = None
    for b in info.get('bits', {}).values():
        if 'alias' in b:
            alias = b['alias']
    key = (info['type'], json.dumps(alias, sort_keys=True))
    if key not in cache:
        cache[key] = TileFeatures(db, info['type'], alias)
    return cache[key]


def unit_positions(unit, blocks):
    """[(key, isset)] of the unit's bits on a tile: key = (frame, word,
    bit) with prjxray's unwrapped word (negative for alias tiles), bits
    past the frame end dropped like prjxray's frame_set. None if a bits
    block is missing (a FasmLookupError)."""
    out = []
    if not unit.bits:
        return out
    for block_type, column, word_bit, isset in unit.bits:
        block = blocks.get(block_type)
        if block is None:
            return None
        base, offset, _, alias = block
        if alias is not None:
            offset -= alias['start_offset']
        absolute = offset * WORD_BITS + word_bit
        word = absolute // WORD_BITS
        if word >= FRAME_WORDS:
            continue
        out.append(((base + column, word, absolute % WORD_BITS), isset))
    return out


# ---------------------------------------------------------------------
# The design being built.
# ---------------------------------------------------------------------
class Design(object):
    def __init__(self):
        self.bits = {}  # (frame, word, bit) -> isset
        self.owner = {}  # (frame, word, bit) -> (tile, unit label)

    def fits(self, positions):
        for key, isset in positions:
            if self.bits.get(key, isset) != isset:
                return False
        return True

    def conflict(self, positions):
        for key, isset in positions:
            if self.bits.get(key, isset) != isset:
                return key
        return None

    def add(self, positions, owner):
        for key, isset in positions:
            if key not in self.bits:
                self.bits[key] = isset
                self.owner[key] = owner

    def remove_owner(self, owner):
        for key in [k for k, v in self.owner.items() if v == owner]:
            del self.bits[key]
            del self.owner[key]


def parse_simple_feature(text):
    """TILE.FEATURE[a:b] = value of required_features / simple lines:
    (tile, feature, [addresses set]); None if not understood."""
    m = re.match(
        r'^\s*([^\s.\[]+)\.([^\s\[=]+)\s*(?:\[(\d+)(?::(\d+))?\])?\s*'
        r'(?:=\s*(\S+))?\s*$', text)
    if not m:
        return None
    tile, feature, hi, lo, value = m.groups()
    if hi is None:
        return tile, feature, [None] if value in (None, '1', "1'b1") else []
    hi = int(hi)
    lo = hi if lo is None else int(lo)
    if value is None:
        return tile, feature, [lo] if hi == lo else None
    v = parse_value(value)
    if v is None:
        return None
    return tile, feature, [lo + i for i in range(hi - lo + 1) if v >> i & 1]


def parse_value(text):
    m = re.match(r"^(\d*)'([hbdo])([0-9a-fA-F_]+)$", text)
    if m:
        return int(m.group(3).replace('_', ''), {
            'h': 16,
            'b': 2,
            'd': 10,
            'o': 8
        }[m.group(2)])
    if re.match(r'^\d+$', text):
        return int(text)
    return None


class Pass(object):
    """One output file: a conflict free design."""

    def __init__(self, index):
        self.index = index
        self.design = Design()
        self.placed = {}  # tile -> [Unit]
        self.tile_order = []


class Group(object):
    """The tiles of a type that accept the same features (same alias)."""

    def __init__(self, tile_type, tiles, features):
        self.tile_type = tile_type
        self.tiles = tiles
        self.features = features
        self.pool = []
        self.leftover = []


class Generator(object):
    def __init__(self, db, mode, count, seed, density, max_per_tile,
                 max_passes):
        self.db = db
        self.mode = mode
        self.count = count
        self.seed = seed
        self.rng = random.Random('%s-%s' % (seed, db.part))
        self.density = density
        self.max_per_tile = max_per_tile
        self.max_passes = max_passes
        self.cache = {}
        self.passes = []
        self.cur = None
        self.stats = {}
        self.uncovered = []
        self.notes = []
        self.conflict_example = None
        self.stepdown_hosts = {}
        self.stepdown_banks = []
        self.groups = []

    # -- helpers ------------------------------------------------------
    def features_of(self, tile):
        return tile_features(self.db, self.cache, tile)

    def new_pass(self):
        self.cur = Pass(len(self.passes) + 1)
        self.passes.append(self.cur)
        self.seed_required()

    def place(self, tile, unit, blocks):
        positions = unit_positions(unit, blocks)
        if positions is None:
            return False
        design = self.cur.design
        if not design.fits(positions):
            if self.conflict_example is None and unit.bits:
                other = design.owner.get(design.conflict(positions))
                if other is not None and other[0] == tile:
                    self.conflict_example = (tile, other[1], unit.label())
            return False
        design.add(positions, (tile, unit.label()))
        if tile not in self.cur.placed:
            self.cur.placed[tile] = []
            self.cur.tile_order.append(tile)
        self.cur.placed[tile].append(unit)
        return True

    @staticmethod
    def is_stepdown(tile, unit):
        """fasm2frames' test: a feature of 3 or more parts whose tag (the
        parts after the site) contains STEPDOWN."""
        parts = unit.name.split('.', 1)
        return len(parts) == 2 and 'STEPDOWN' in parts[1]

    # -- the steps ----------------------------------------------------
    def seed_required(self):
        """The part's required_features.fasm bits are in every design."""
        for text in self.db.required_features:
            parsed = parse_simple_feature(text)
            if parsed is None or parsed[0] not in self.db.grid:
                self.note('required feature not understood: %s' % text)
                continue
            tile, feature, addresses = parsed
            features = self.features_of(tile)
            blocks = self.db.bits_blocks(tile)
            for address in addresses:
                unit = features.find(feature, address)
                if unit is None:
                    self.note('required feature not found: %s' % text)
                    continue
                self.cur.design.add(
                    unit_positions(unit, blocks) or [],
                    (tile, 'required ' + text))

    def note(self, text):
        if text not in self.notes:
            self.notes.append(text)

    def choose_stepdown(self, tiles_by_type):
        """Host tiles for the STEPDOWN features (bonded, in as few banks as
        possible); returns the tiles kept free of generated features: the
        other tiles of those banks and the PUDC_B tile."""
        pudc = self.db.pudc_b()
        pudc_tile = pudc[0] if isinstance(pudc, tuple) else None
        wanted = {}
        for tile_type, tiles in sorted(tiles_by_type.items()):
            if tile_type not in self.db.known_types or not any(
                    self.is_stepdown(tiles[0], u)
                    for u in self.features_of(tiles[0]).units):
                continue
            banks = {}
            for t in tiles:
                bank = self.db.tile_to_bank.get(t)
                if bank is not None and t != pudc_tile:
                    banks.setdefault(bank, []).append(t)
            if banks:
                wanted[tile_type] = banks
            else:
                self.note('no bonded %s tile: its STEPDOWN features are not '
                          'used' % tile_type)
        while wanted:
            counts = {}
            for banks in wanted.values():
                for bank in banks:
                    counts[bank] = counts.get(bank, 0) + 1
            bank = sorted(counts, key=lambda b: (-counts[b], b))[0]
            self.stepdown_banks.append(bank)
            for tile_type in [t for t in wanted if bank in wanted[t]]:
                self.stepdown_hosts[tile_type] = wanted[tile_type][bank][0]
                del wanted[tile_type]
        reserved = set()
        for bank in self.stepdown_banks:
            reserved.update(self.db.bank_to_tiles.get(bank, []))
        reserved.difference_update(self.stepdown_hosts.values())
        if pudc_tile is not None:
            reserved.add(pudc_tile)
        return reserved

    def make_pool(self, group, reserved):
        """(the tiles in the order features are placed, the tiles given a
        random subset first)."""
        pool = [t for t in group.tiles if t not in reserved]
        if not pool:
            pool = list(group.tiles)
        if self.mode == 'first':
            return pool, []
        if self.mode == 'all':
            return pool, pool
        n = min(self.count, len(pool))
        chosen = []
        for i in range(n):
            lo = i * len(pool) // n
            hi = (i + 1) * len(pool) // n
            chosen.append(pool[self.rng.randrange(lo, hi)])
        rest = [t for t in pool if t not in chosen]
        self.rng.shuffle(rest)
        return chosen + rest, chosen

    def fill_tile(self, tile, units, blocks):
        """A random conflict free subset of `units` on `tile`."""
        order = list(units)
        self.rng.shuffle(order)
        placed = 0
        for unit in order:
            if self.max_per_tile and placed >= self.max_per_tile:
                break
            if self.rng.random() >= self.density:
                continue
            if self.place(tile, unit, blocks):
                placed += 1

    def cover(self, group, units):
        """Places each unit on the first tile of the pool where it fits;
        returns the units that fit nowhere."""
        for tile in group.pool:
            if not units:
                break
            blocks = self.db.bits_blocks(tile)
            units = [u for u in units if not self.place(tile, u, blocks)]
        return units

    def build(self):
        tiles_by_type = {}
        for tile, info in self.db.grid.items():
            tiles_by_type.setdefault(info['type'], []).append(tile)
        reserved = self.choose_stepdown(tiles_by_type)
        self.new_pass()
        for tile_type, host in sorted(self.stepdown_hosts.items()):
            blocks = self.db.bits_blocks(host)
            for unit in self.features_of(host).units:
                if self.is_stepdown(host, unit):
                    self.place(host, unit, blocks)
        for tile_type in sorted(tiles_by_type):
            tiles = tiles_by_type[tile_type]
            stat = self.stats.setdefault(tile_type, {
                'tiles_in_grid': len(tiles),
                'features': 0,
                'lines': 0,
                'tiles_used': 0,
            })
            if tile_type not in self.db.known_types:
                stat['note'] = 'no tile_type_%s.json' % tile_type
                continue
            by_features = {}
            for tile in tiles:
                features = self.features_of(tile)
                if id(features) not in by_features:
                    group = Group(tile_type, [], features)
                    by_features[id(features)] = group
                    self.groups.append(group)
                by_features[id(features)].tiles.append(tile)
        for group in self.groups:
            stat = self.stats[group.tile_type]
            stat['features'] += len(group.features.units)
            group.pool, chosen = self.make_pool(group, reserved)
            todo = [
                u for u in group.features.units
                if not self.is_stepdown(group.tiles[0], u)
            ]
            if self.mode != 'first':
                for tile in chosen:
                    self.fill_tile(tile, todo, self.db.bits_blocks(tile))
            placed = set()
            for tile in group.tiles:
                for unit in self.cur.placed.get(tile, ()):
                    placed.add(unit.label())
            group.leftover = self.cover(
                group, [u for u in todo if u.label() not in placed])
            for unit in group.features.units:
                if (self.is_stepdown(group.tiles[0], unit)
                        and group.tile_type not in self.stepdown_hosts):
                    self.uncovered.append(
                        (group.tile_type, group.tiles[0], unit.label(),
                         'STEPDOWN feature, no bonded tile'))
        self.propagate_stepdown()
        while any(g.leftover for g in self.groups):
            if len(self.passes) >= self.max_passes:
                break
            self.new_pass()
            progress = False
            for group in self.groups:
                if group.leftover:
                    before = len(group.leftover)
                    group.leftover = self.cover(group, group.leftover)
                    progress = progress or len(group.leftover) < before
            if not progress:
                self.passes.pop()
                break
        for group in self.groups:
            for unit in group.leftover:
                blocks = self.db.bits_blocks(group.tiles[0])
                if unit_positions(unit, blocks) is None:
                    reason = 'tile has no bits block for the feature'
                else:
                    reason = 'conflicts in every pass (--max-passes)'
                self.uncovered.append(
                    (group.tile_type, group.tiles[0], unit.label(), reason))
        for p in self.passes:
            for tile in p.placed:
                self.stats[self.db.grid[tile]['type']]['tiles_used'] += 1

    def used_iob_sites(self, extra=()):
        used = set(extra)
        for tile, units in self.cur.placed.items():
            if 'IOB33' not in tile:
                continue
            for unit in units:
                parts = unit.name.split('.')
                if len(parts) >= 2:
                    used.add((tile, parts[0]))
        return used

    def stepdown_features(self, extra_used=()):
        """The features fasm2frames adds for the STEPDOWN banks:
        [(tile, feature)]."""
        tags = {}
        for tile, units in self.cur.placed.items():
            for unit in units:
                if self.is_stepdown(tile, unit):
                    tag = unit.name.split('.', 1)[1]
                    bank = tags.setdefault(self.db.tile_to_bank[tile], [])
                    if tag not in bank:
                        bank.append(tag)
        used = self.used_iob_sites(extra_used)
        out = []
        for bank in tags:
            for tile in self.db.bank_to_tiles.get(bank, []):
                if 'IOB33' in tile and tile in self.db.grid:
                    for site in self.db.grid[tile]['sites']:
                        site = 'IOB_Y%d' % (int(site[-1]) % 2)
                        if (tile, site) in used:
                            continue
                        for tag in tags[bank]:
                            out.append((tile, '%s.%s' % (site, tag)))
                if 'HCLK_IOI3' in tile:
                    out.append((tile, 'STEPDOWN'))
        return out

    def propagate_stepdown(self):
        """Removes generated features that conflict with the STEPDOWN
        features the reference adds (with and without the PUDC_B pull-up,
        which uses its site), until none does; they go to the next pass."""
        pudc = self.db.pudc_b()
        extras = [()]
        if isinstance(pudc, tuple):
            extras.append((pudc, ))
        group_of = {}
        for group in self.groups:
            for tile in group.tiles:
                group_of[tile] = group
        while True:
            removed = False
            for extra in extras:
                trial = Design()
                trial.bits = dict(self.cur.design.bits)
                trial.owner = dict(self.cur.design.owner)
                for tile, feature in self.stepdown_features(extra):
                    if tile not in self.db.grid:
                        continue
                    unit = self.features_of(tile).find(feature, None)
                    if unit is None:
                        continue
                    positions = unit_positions(
                        unit, self.db.bits_blocks(tile)) or []
                    key = trial.conflict(positions)
                    if key is None:
                        trial.add(positions, (tile, 'stepdown ' + feature))
                        continue
                    owner = trial.owner[key]
                    if (owner[1].startswith('required ')
                            or owner[1].startswith('stepdown ')
                            or 'STEPDOWN' in owner[1]):
                        self.note('STEPDOWN feature %s.%s conflicts with '
                                  '%s.%s' % (tile, feature, owner[0],
                                             owner[1]))
                        continue
                    unit = self.unplace(owner)
                    group_of[owner[0]].leftover.append(unit)
                    removed = True
                    break
                if removed:
                    break
            if not removed:
                return

    def unplace(self, owner):
        tile, label = owner
        self.cur.design.remove_owner(owner)
        units = self.cur.placed[tile]
        unit = next(u for u in units if u.label() == label)
        units.remove(unit)
        return unit

    # -- output -------------------------------------------------------
    def annotation(self):
        keys = self.rng.sample(ANNOTATION_KEYS, self.rng.randrange(1, 3))
        return '{ %s }' % ', '.join('%s = "%s%d"' %
                                    (k, k, self.rng.randrange(1000))
                                    for k in keys)

    def decorate(self, line):
        r = self.rng.random()
        if r < 0.03:
            return '%s %s' % (line, self.annotation())
        if r < 0.05:
            return '%s # c%d' % (line, self.rng.randrange(1000))
        if r < 0.06:
            return '%s %s # both' % (line, self.annotation())
        return line

    def format_range(self, feature, lo, hi, value):
        width = hi - lo + 1
        choices = ['h', 'hw', 'b']
        if value < 2**31:
            choices.append('plain')
        if value < 2**32:
            choices.append('d')
        if width <= 30:
            choices.append('o')
        kind = self.rng.choice(choices)
        if kind == 'h':
            text = "%d'h%X" % (width, value)
        elif kind == 'hw':
            text = "'h%x" % value
        elif kind == 'b':
            text = "%d'b%s" % (width, format(value, 'b'))
        elif kind == 'plain':
            text = '%d' % value
        elif kind == 'd':
            text = "%d'd%d" % (width, value)
        else:
            text = "%d'o%o" % (width, value)
        return '%s[%d:%d] = %s' % (feature, hi, lo, text)

    def tile_lines(self, tile, units):
        features = self.features_of(tile)
        plain = []
        ranges = {}
        for unit in units:
            if unit.address is None:
                plain.append(unit)
            else:
                ranges.setdefault(unit.name, []).append(unit)
        lines = []
        for unit in sorted(plain, key=lambda u: u.order):
            r = self.rng.random()
            if r < 0.85:
                lines.append('%s.%s' % (tile, unit.name))
            elif r < 0.95:
                lines.append('%s.%s = 1' % (tile, unit.name))
            else:
                lines.append("%s.%s = 1'b1" % (tile, unit.name))
        for name in sorted(ranges,
                           key=lambda n: min(u.order for u in ranges[n])):
            addresses = sorted(u.address for u in ranges[name])
            top = features.max_address[name]
            i = 0
            while i < len(addresses):
                lo = addresses[i]
                single = i + 1 == len(addresses) or self.rng.random() < 0.05
                if single and self.rng.random() < 0.6:
                    r = self.rng.random()
                    if r < 0.5:
                        lines.append('%s.%s[%d]' % (tile, name, lo))
                    elif r < 0.8:
                        lines.append('%s.%s[%d] = 1' % (tile, name, lo))
                    else:
                        lines.append("%s.%s[%d:%d] = 1'b1" %
                                     (tile, name, lo, lo))
                    i += 1
                    continue
                span = self.rng.choice((8, 16, 32, 64, 64, 256, 1024))
                j = i
                while j < len(addresses) and addresses[j] - lo < span:
                    j += 1
                hi = addresses[j - 1]
                if self.rng.random() < 0.3:
                    # Zero bits past the last set address (up to two past
                    # the feature's last address: 0 bits are not looked
                    # up).
                    hi = min(hi + self.rng.randrange(1, 4), max(top, hi) + 2)
                value = 0
                for a in addresses[i:j]:
                    value |= 1 << (a - lo)
                lines.append(
                    self.format_range('%s.%s' % (tile, name), lo, hi, value))
                i = j
        # `= 0` disables of features not placed here: no bits, no lookup.
        placed = set(u.label() for u in units)
        others = [u for u in features.units if u.label() not in placed]
        for unit in self.rng.sample(others, min(len(others), 2)):
            if unit.address is None:
                lines.append('%s.%s = 0' % (tile, unit.name))
            else:
                lines.append("%s.%s[%d:%d] = 2'b00" %
                             (tile, unit.name, unit.address + 1, unit.address))
        if self.mode != 'first':
            self.rng.shuffle(lines)
        return [self.decorate(line) for line in lines]

    def expected_frames(self, p):
        """The frames prjxray's `get_frames(sparse=True)` gives for pass
        `p` without --emit_pudc_b_pullup, from this model: every frame of a
        bits block a feature has bits in (in use, zero filled) and every
        stored bit, with the required and the STEPDOWN features. A cross
        check of the model and of any assembler (--expected-frm)."""
        bits = dict(p.design.bits)
        in_use = set()

        def use(tile, unit):
            blocks = self.db.bits_blocks(tile)
            for block_type, _, _, _ in unit.bits or ():
                base, _, frames, _ = blocks[block_type]
                in_use.update(range(base, base + frames))

        for tile, units in p.placed.items():
            for unit in units:
                use(tile, unit)
        for text in self.db.required_features:
            parsed = parse_simple_feature(text)
            if parsed is not None and parsed[0] in self.db.grid:
                for address in parsed[2]:
                    unit = self.features_of(parsed[0]).find(parsed[1], address)
                    if unit is not None:
                        use(parsed[0], unit)
        if p is self.passes[0]:
            saved, self.cur = self.cur, p
            try:
                for tile, feature in self.stepdown_features():
                    unit = None
                    if tile in self.db.grid:
                        unit = self.features_of(tile).find(feature, None)
                    if unit is None:
                        continue
                    use(tile, unit)
                    for key, isset in unit_positions(
                            unit, self.db.bits_blocks(tile)) or ():
                        bits.setdefault(key, isset)
            finally:
                self.cur = saved
        frames = dict((f, [0] * FRAME_WORDS) for f in in_use)
        for (frame, word, bit), isset in bits.items():
            words = frames.setdefault(frame, [0] * FRAME_WORDS)
            if isset:
                words[word] |= 1 << bit
        return frames

    def write_expected(self, p, path):
        frames = self.expected_frames(p)
        with open(path, 'w') as f:
            for address in sorted(frames):
                words = ','.join('0x%08X' % w for w in frames[address])
                f.write('0x%08X %s\n' % (address, words))

    def write_pass(self, p, path):
        count = 0
        with open(path, 'w') as f:
            f.write('# Synthetic FASM for %s (fabric %s), pass %d of %d, '
                    'generated by tools/gen-xilinx-corpus.py\n' %
                    (self.db.part, self.db.fabric, p.index, len(self.passes)))
            f.write('# --tiles %s --seed %s\n' %
                    (self.describe_mode(), self.seed))
            f.write('{ generator = "gen-xilinx-corpus", version = "%d" }\n\n' %
                    GENERATOR_VERSION)
            order = list(p.tile_order)
            if self.mode != 'first':
                self.rng.shuffle(order)
            for tile in order:
                if not p.placed[tile]:
                    continue
                lines = self.tile_lines(tile, p.placed[tile])
                tile_type = self.db.grid[tile]['type']
                self.stats[tile_type]['lines'] += len(lines)
                count += len(lines)
                if self.rng.random() < 0.2:
                    f.write('# %s (%s)\n' % (tile, tile_type))
                for line in lines:
                    f.write(line + '\n')
                if self.rng.random() < 0.1:
                    f.write('\n')
        return count

    def describe_mode(self):
        if self.mode == 'sample':
            return 'sample %d' % self.count
        return self.mode


# ---------------------------------------------------------------------
# Error corpus.
# ---------------------------------------------------------------------
def write_errors(gen, out_dir, family_types):
    db = gen.db
    rng = random.Random('%s-%s-errors' % (gen.seed, db.part))
    os.makedirs(out_dir, exist_ok=True)
    used = set()
    for p in gen.passes:
        used.update(p.placed)
    groups = [g for g in gen.groups if g.features.units]
    sample = rng.sample(groups, min(6, len(groups)))

    def some_tile(group):
        """A tile of the group without generated features or IO bank."""
        for t in group.tiles:
            if t not in used and t not in db.tile_to_bank:
                return t
        return group.tiles[-1]

    def good_line(tile):
        units = gen.features_of(tile).units
        unit = next((u for u in units if u.address is None), units[0])
        return '%s.%s' % (tile, unit.label())

    written = []

    # 1. Batched lookup errors, every message in order.
    lines = ['# FasmLookupError: every message, in order']
    for group in sample:
        tile = some_tile(group)
        features = group.features
        lines.append('%s.NO_SUCH_FEATURE_%d' % (tile, rng.randrange(100)))
        lines.append('%s.NO_SUCH_FEATURE = 0' % tile)
        lines.append(good_line(tile))
        if features.max_address:
            names = sorted(features.max_address)
            name = names[rng.randrange(len(names))]
            top = features.max_address[name]
            lines.append('%s.%s[%d]' % (tile, name, top + 1))
            lines.append("%s.%s[%d:%d] = 3'b101" %
                         (tile, name, top + 3, top + 1))
            have = set(u.address for u in features.units if u.name == name)
            gaps = [a for a in range(1, top) if a not in have]
            if gaps:
                lines.append('%s.%s[%d] { note = "gap" }' %
                             (tile, name, gaps[0]))
        plain = [u for u in features.units if u.address is None]
        if plain:
            lines.append('%s.%s.EXTRA_SUFFIX' % (tile, plain[0].name))
    for group in groups:
        # A feature of a bits block the tile does not have.
        tile = group.tiles[0]
        blocks = db.bits_blocks(tile)
        unit = next((u for u in group.features.units
                     if u.bits and any(b not in blocks
                                       for b, _, _, _ in u.bits)), None)
        if unit is not None:
            lines.append('%s.%s # missing bits block' % (tile, unit.label()))
            break
    written.append(('lookup_errors.fasm', lines))

    # 2. A tile absent from this part after lookup errors: KeyError at
    # once (the batched errors are lost).
    present = set(g.tile_type for g in gen.groups)
    absent = sorted('%s_X0Y0' % t for t in family_types if t not in present)
    if not absent and sample:
        m = re.match(r'^(.*)_X\d+Y\d+$', sample[0].tiles[0])
        if m:
            absent = ['%s_X999Y999' % m.group(1)]
    if absent and sample:
        tile = some_tile(sample[0])
        written.append(('absent_tile.fasm', [
            '# KeyError for a tile that is not in this part',
            '%s.NO_SUCH_FEATURE' % tile,
            good_line(tile),
            '%s.SOME.FEATURE' % rng.choice(absent),
            '%s.NOT_REACHED' % tile,
        ]))

    # 3. A value that does not fit its range: the ANTLR value range error.
    if sample:
        tile = some_tile(sample[-1])
        units = sample[-1].features.units
        plain = [u for u in units if u.address is None]
        if plain:
            bad = '%s.%s = 2' % (tile, plain[0].name)
        else:
            unit = units[0]
            bad = "%s.%s[%d:%d] = 3'h9" % (tile, unit.name,
                                           unit.address + 2, unit.address)
        written.append(('value_range.fasm',
                        ['# value range error',
                         good_line(tile), bad]))

    # 4. Conflicting features: FasmInconsistentBits.
    if gen.conflict_example is not None:
        tile, first, second = gen.conflict_example
        written.append(('inconsistent.fasm', [
            '# FasmInconsistentBits: the first conflict wins',
            '%s.%s' % (tile, first),
            '%s.%s { why = "conflicts" }' % (tile, second),
        ]))

    # 5. STEPDOWN on an IOB tile without package pin: KeyError.
    for group in groups:
        unbonded = [t for t in group.tiles if t not in db.tile_to_bank]
        unit = next((u for u in group.features.units
                     if gen.is_stepdown(group.tiles[0], u)), None)
        if unbonded and unit is not None:
            written.append(('stepdown_unbonded.fasm', [
                '# KeyError: STEPDOWN on an IOB without IO bank',
                '%s.%s' % (unbonded[-1], unit.name)
            ]))
            break

    names = []
    for name, lines in written:
        with open(os.path.join(out_dir, name), 'w') as f:
            f.write('\n'.join(lines) + '\n')
        names.append(name)
    return names


def family_tile_types(db_root):
    """Tile types of the family with a segbits file."""
    types = set()
    for f in os.listdir(db_root):
        if (f.startswith('segbits_') and f.endswith('.db')
                and '.origin_info' not in f and '.block_ram' not in f):
            types.add(f[len('segbits_'):-len('.db')].upper())
    return types


def parse_tiles(parser, tiles):
    mode = tiles[0]
    if mode == 'sample':
        if len(tiles) != 2 or not tiles[1].isdigit() or int(tiles[1]) < 1:
            parser.error('--tiles sample needs a count >= 1')
        return mode, int(tiles[1])
    if mode not in ('first', 'all') or len(tiles) != 1:
        parser.error('--tiles must be first, all or sample N')
    return mode, 0


def main(argv=None):
    parser = argparse.ArgumentParser(
        description=__doc__.split('\n\n')[0],
        formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--db-root', required=True, help='family directory')
    parser.add_argument('--part')
    parser.add_argument('--list-parts',
                        action='store_true',
                        help='print the parts of the family and exit')
    parser.add_argument('--out-dir', help='output directory')
    parser.add_argument('--tiles',
                        nargs='+',
                        default=['sample', '3'],
                        metavar='MODE',
                        help='first | sample N | all (default: sample 3)')
    parser.add_argument('--seed', default='0', help='random seed (default 0)')
    parser.add_argument('--density',
                        type=float,
                        default=0.5,
                        help='probability of each feature in the random '
                        'subsets of sample/all (default %(default)s)')
    parser.add_argument('--max-per-tile',
                        type=int,
                        default=0,
                        help='bound of the random subsets (0: none)')
    parser.add_argument('--max-passes',
                        type=int,
                        default=64,
                        help='most files (features.fasm, features-2.fasm, '
                        '...) for features that conflict on every tile of '
                        'their type (default %(default)s)')
    parser.add_argument('--expected-frm',
                        action='store_true',
                        help='also write <file>.expected.frm, the sparse '
                        'frames of each features file (without the PUDC_B '
                        'pull-up) computed by this generator\'s model of '
                        'prjxray, a cross check')
    parser.add_argument('--no-errors',
                        action='store_true',
                        help='do not write errors/')
    args = parser.parse_args(argv)

    if args.list_parts:
        for part in family_parts(args.db_root):
            print(part)
        return 0
    if not args.part or not args.out_dir:
        parser.error('--part and --out-dir are required')
    mode, count = parse_tiles(parser, args.tiles)

    db = Database(args.db_root, args.part)
    gen = Generator(db, mode, count, args.seed, args.density,
                    args.max_per_tile, max(1, args.max_passes))
    gen.build()
    os.makedirs(args.out_dir, exist_ok=True)
    for d in (args.out_dir, os.path.join(args.out_dir, 'errors')):
        if os.path.isdir(d):
            for f in os.listdir(d):
                if f.endswith('.fasm') or f.endswith('.expected.frm'):
                    os.remove(os.path.join(d, f))
    files = []
    lines = 0
    for p in gen.passes:
        if p.index == 1:
            name = 'features.fasm'
        else:
            name = 'features-%d.fasm' % p.index
        lines += gen.write_pass(p, os.path.join(args.out_dir, name))
        files.append(name)
        if args.expected_frm:
            expected = name[:-len('.fasm')] + '.expected.frm'
            gen.write_expected(p, os.path.join(args.out_dir, expected))
    errors = []
    if not args.no_errors:
        errors = write_errors(gen, os.path.join(args.out_dir, 'errors'),
                              family_tile_types(args.db_root))
    placed = sum(
        len(units) for p in gen.passes for units in p.placed.values())
    manifest = {
        'generator_version': GENERATOR_VERSION,
        'part': args.part,
        'fabric': db.fabric,
        'tiles': gen.describe_mode(),
        'seed': args.seed,
        'density': args.density,
        'max_per_tile': args.max_per_tile,
        'files': files,
        'lines': lines,
        'features_placed': placed,
        'features_total': sum(len(g.features.units) for g in gen.groups),
        'stepdown_banks': gen.stepdown_banks,
        'stepdown_hosts': gen.stepdown_hosts,
        'pudc_b': db.pudc_b(),
        'errors': errors,
        'tile_types': gen.stats,
        'uncovered': [list(u) for u in gen.uncovered],
        'unreachable': sorted(
            set((g.tile_type, key, reason) for g in gen.groups
                for key, reason in g.features.skipped)),
        'notes': gen.notes,
    }
    with open(os.path.join(args.out_dir, 'manifest.json'), 'w') as f:
        json.dump(manifest, f, indent=1, sort_keys=True)
        f.write('\n')
    print('%s: %d files, %d lines, %d feature bits placed (%d distinct), '
          '%d not placed, %d error files' %
          (args.part, len(files), lines, placed, manifest['features_total'],
           len(gen.uncovered), len(errors)))
    return 0


if __name__ == '__main__':
    sys.exit(main())

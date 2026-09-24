// Copyright 2017-2022 F4PGA Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! [`Database`]: opening a prjxray-db / prjuray-db family directory for a
//! part, and the feature lookup used by the assembler.

use std::fmt;
use std::path::{Path, PathBuf};

use fasm::idstring::IdString;
use foldhash::{HashMap, HashSet};

use crate::arch::{Architecture, BitPosition, BitPositionError, BlockType};
use crate::error::{is_file, read_text, DbError};
use crate::part::{read_package_pins, BanksTilesRegistry, PackagePin, Part, PartJson};
use crate::segbits::{
    PpipType, SegBit, SegbitsEntry, SegbitsMatch, TileSegbits, TileSegbitsBuilder,
};
use crate::tilegrid::{BitsBlock, Grid, Tile};
use crate::yaml::{self, YamlError};

/// The directory structure of a database family (design document §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layout {
    /// prjxray-db: `mapping/{parts,devices}.yaml` map a part to a fabric
    /// directory holding `tilegrid.json`; `tile_type_*.json` are in the
    /// family directory.
    Prjxray,
    /// prjuray-db: no `mapping/`; `tilegrid.json` is in the part
    /// directory; `tile_type_*.json` are in `tile_types/`.
    Prjuray,
}

/// Which database files exist for a tile type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileTypeFiles {
    /// `segbits_<type>.db` (`CLB_IO_CLK` bus).
    pub segbits: bool,
    /// `segbits_<type>.block_ram.db` (`BLOCK_RAM` bus).
    pub block_ram_segbits: bool,
    /// `ppips_<type>.db`.
    pub ppips: bool,
    /// `mask_<type>.db` (recorded, never read: nothing in the FASM ->
    /// frames pipeline uses mask files).
    pub mask: bool,
}

/// One tile type of the family (`prjxray.tile.TileDbs` + its loaded
/// `TileSegbits`).
#[derive(Clone, Debug)]
pub struct TileType {
    /// Name, e.g. `CLBLL_L` (upper case, from `tile_type_<NAME>.json`).
    pub name: IdString,
    /// Segbits and pseudo PIPs (empty if the type has no files).
    pub segbits: TileSegbits,
    /// Which files exist.
    pub files: TileTypeFiles,
}

/// Part level data (the `<db_root>/<part>/` directory).
#[derive(Clone, Debug)]
pub struct PartInfo {
    /// Part name, e.g. `xc7a35tcsg324-1`.
    pub name: String,
    /// The part directory.
    pub directory: PathBuf,
    /// prjxray-db device (from `mapping/parts.yaml`), e.g. `xc7a35t`.
    pub device: Option<String>,
    /// The directory holding `tilegrid.json`: the fabric (prjxray-db, from
    /// `mapping/devices.yaml`, e.g. `xc7a50t`) or the part itself
    /// (prjuray-db).
    pub fabric: String,
    /// Frame tree and IDCODE (`part.yaml`, else `part.json`), if present.
    pub part: Option<Part>,
    /// IDCODE (`part.yaml`, else `part.json`), if present.
    pub idcode: Option<u32>,
    /// `part.json` `iobanks`: `(bank, location)` pairs in file order.
    pub iobanks: Option<Vec<(IdString, IdString)>>,
    /// `package_pins.csv`, if present.
    pub package_pins: Option<Vec<PackagePin>>,
    /// `required_features.fasm` lines (stripped, non-empty, first
    /// occurrence order), empty if the file does not exist.
    pub required_features: Vec<String>,
}

/// An opened database: the tile types (segbits, pseudo PIPs) of a family
/// and, for a part, its tile grid and part data.
///
/// Mirrors `prjxray.db.Database` (and `prjuray.db.Database`), loading
/// everything eagerly: [`Database::open`] reads `tilegrid.json`, every
/// `segbits_*.db` / `segbits_*.block_ram.db` / `ppips_*.db` of the
/// family's tile types, `part.yaml`, `part.json`, `package_pins.csv` and
/// `required_features.fasm`. It never reads `mask_*.db`,
/// `*.origin_info.db`, `tileconn.json`, `node_wires.json`,
/// `tile_type_*.json` (only their names) or `site_type_*.json`.
#[derive(Clone, Debug)]
pub struct Database {
    root: PathBuf,
    layout: Layout,
    architecture: Architecture,
    tile_types: Vec<TileType>,
    tile_type_index: HashMap<IdString, u32>,
    grid: Option<Grid>,
    part: Option<PartInfo>,
    banks: Option<BanksTilesRegistry>,
}

impl Database {
    /// Opens the family directory `db_root` (e.g.
    /// `prjxray-db/artix7`, `prjuray-db/zynqusp`) for `part`.
    ///
    /// * prjxray-db (a `mapping/` directory exists): the fabric is
    ///   `devices.yaml[parts.yaml[part].device].fabric`
    ///   (`prjxray.util.get_fabric_for_part`) and the grid is
    ///   `<db_root>/<fabric>/tilegrid.json`; the architecture is Series7.
    /// * prjuray-db (a `tile_types/` directory exists): the grid is
    ///   `<db_root>/<part>/tilegrid.json`; the architecture is taken from
    ///   the `part.yaml` tag, UltraScale+ by default.
    ///
    /// With `part == None` only the tile types are loaded (no grid, no
    /// part data), which is enough to inspect segbits.
    ///
    /// # Errors
    ///
    /// [`DbError::UnknownLayout`], [`DbError::UnknownPart`],
    /// [`DbError::UnknownDevice`], a missing `tilegrid.json`
    /// ([`DbError::MissingFile`]) or any parse error of the files listed
    /// above, with the file and line.
    pub fn open(db_root: &Path, part: Option<&str>) -> Result<Self, DbError> {
        let layout = if db_root.join("mapping").is_dir() {
            Layout::Prjxray
        } else if db_root.join("tile_types").is_dir() {
            Layout::Prjuray
        } else {
            return Err(DbError::UnknownLayout {
                root: db_root.to_path_buf(),
            });
        };
        let default_arch = match layout {
            Layout::Prjxray => Architecture::Series7,
            Layout::Prjuray => Architecture::UltraScalePlus,
        };
        let (tile_types, tile_type_index) = load_tile_types(db_root, layout)?;
        let mut db = Database {
            root: db_root.to_path_buf(),
            layout,
            architecture: default_arch,
            tile_types,
            tile_type_index,
            grid: None,
            part: None,
            banks: None,
        };
        let Some(part) = part else {
            return Ok(db);
        };
        let info = load_part_info(db_root, layout, part, default_arch)?;
        if let Some(p) = &info.part {
            db.architecture = p.architecture;
        }
        let grid_path = match layout {
            Layout::Prjxray => db_root.join(&info.fabric).join("tilegrid.json"),
            Layout::Prjuray => info.directory.join("tilegrid.json"),
        };
        let mut grid = Grid::from_file(&grid_path)?;
        db.resolve_tile_types(&mut grid);
        if let (Some(iobanks), Some(pins)) = (&info.iobanks, &info.package_pins) {
            db.banks = Some(BanksTilesRegistry::new(iobanks, pins));
        }
        db.grid = Some(grid);
        db.part = Some(info);
        Ok(db)
    }

    /// Sets each tile's tile type index (`self.tile_types[tile_type.upper()]`).
    fn resolve_tile_types(&self, grid: &mut Grid) {
        let mut cache: HashMap<IdString, u32> = HashMap::default();
        for tile in grid.tiles_mut() {
            tile.type_index = *cache.entry(tile.tile_type).or_insert_with(|| {
                let upper = tile.tile_type.with_str(str::to_ascii_uppercase);
                IdString::lookup(&upper)
                    .and_then(|id| self.tile_type_index.get(&id).copied())
                    .unwrap_or(u32::MAX)
            });
        }
    }

    /// The family directory given to [`Database::open`].
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory layout.
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// The architecture: from the `part.yaml` tag when a part with a
    /// `part.yaml` is open, else Series7 for prjxray-db and UltraScale+
    /// for prjuray-db.
    pub fn architecture(&self) -> Architecture {
        self.architecture
    }

    /// All tile types of the family, sorted by name.
    pub fn tile_types(&self) -> &[TileType] {
        &self.tile_types
    }

    /// The tile type named `name` (upper case).
    pub fn tile_type(&self, name: IdString) -> Option<&TileType> {
        self.tile_type_index
            .get(&name)
            .map(|&i| &self.tile_types[i as usize])
    }

    /// The tile type of a tile of [`Database::grid`].
    pub fn tile_type_of(&self, tile: &Tile) -> Option<&TileType> {
        tile.tile_type_index().map(|i| &self.tile_types[i])
    }

    /// The tile grid (`Database.grid()`), `None` if no part was given.
    pub fn grid(&self) -> Option<&Grid> {
        self.grid.as_ref()
    }

    /// The part data, `None` if no part was given.
    pub fn part_info(&self) -> Option<&PartInfo> {
        self.part.as_ref()
    }

    /// The part frame tree, if the part has a `part.yaml` or a `part.json`
    /// with a frame tree.
    pub fn part(&self) -> Option<&Part> {
        self.part.as_ref().and_then(|p| p.part.as_ref())
    }

    /// The IO bank <-> tile map for STEPDOWN propagation, if the part has
    /// both `part.json` `iobanks` and `package_pins.csv` (the files
    /// `fasm2frames.py` reads unconditionally for a part).
    pub fn banks_tiles_registry(&self) -> Option<&BanksTilesRegistry> {
        self.banks.as_ref()
    }

    /// `Database.get_required_fasm_features(part)`: the lines of
    /// `<db_root>/<part>/required_features.fasm` if `part` is the open
    /// part, else nothing (like prjxray, `None` gives nothing too).
    ///
    /// prjxray returns a `set` (arbitrary order); this returns the lines
    /// in file order without duplicates.
    pub fn get_required_fasm_features(&self, part: Option<&str>) -> &[String] {
        match (&self.part, part) {
            (Some(info), Some(part)) if part == info.name => &info.required_features,
            _ => &[],
        }
    }

    /// Looks up the FASM feature `<tile>.<feature>` with bit `address`
    /// (0 for a feature without `[N]`), following
    /// `FasmAssembler.enable_feature` / `Grid.get_tile_segbits_at_tilename`
    /// / `TileSegbits(Alias).feature_to_bits`:
    ///
    /// 1. `tile` -> grid tile -> tile type;
    /// 2. if any `bits` block of the tile has an `alias`: pseudo PIPs of
    ///    the tile's own type, then the aliased type's table with the
    ///    site (the first component of `feature`) mapped through the
    ///    alias `sites` and `offset - start_offset` as offset;
    /// 3. otherwise the tile type's table ([`TileSegbits::feature_to_bits`]:
    ///    pseudo PIP, exact name when `address == 0`, then `NAME[address]`).
    ///
    /// `feature` is the handle of the name *within* the tile (for
    /// `CLBLL_L_X2Y0.SLICEL_X0.A5FF.ZINI`, `tile` is `CLBLL_L_X2Y0` and
    /// `feature` is `SLICEL_X0.A5FF.ZINI`). The common path does two hash
    /// lookups and no allocation; only a site renamed by an alias builds a
    /// string.
    ///
    /// # Errors
    ///
    /// A [`LookupError`] naming what was not found. prjxray raises
    /// `FasmLookupError` for [`LookupError::UnknownFeature`] and
    /// [`LookupError::MissingBitsBlock`], and a bare `KeyError` for an
    /// unknown tile or tile type.
    pub fn lookup_feature(
        &self,
        tile: IdString,
        feature: IdString,
        address: u32,
    ) -> Result<FeatureLookup<'_>, LookupError> {
        let grid = self.grid.as_ref().ok_or(LookupError::NoGrid)?;
        let tile_index = grid
            .tile_index(tile)
            .ok_or(LookupError::UnknownTile { tile })?;
        let tile_ref = &grid.tiles()[tile_index];
        let own_type = self
            .tile_type_of(tile_ref)
            .ok_or(LookupError::UnknownTileType {
                tile,
                tile_type: tile_ref.tile_type,
            })?;
        let blocks = grid.bits(tile_ref);
        let unknown = || LookupError::UnknownFeature {
            tile,
            tile_type: tile_ref.tile_type,
            feature,
            address,
        };
        if !blocks.iter().any(BitsBlock::has_alias) {
            let found = own_type
                .segbits
                .feature_to_bits(feature, address)
                .ok_or_else(unknown)?;
            return self.finish_lookup(grid, tile_ref, own_type, found);
        }

        // TileSegbitsAlias.
        let mut alias_type = None;
        for block in blocks {
            let alias = grid
                .alias(block)
                .ok_or(LookupError::InconsistentAlias { tile })?;
            match alias_type {
                None => alias_type = Some(alias.tile_type),
                Some(t) if t != alias.tile_type => {
                    return Err(LookupError::InconsistentAlias { tile })
                }
                Some(_) => {}
            }
        }
        if let Some(ppip) = own_type.segbits.ppip(feature) {
            return Ok(FeatureLookup::PseudoPip(ppip));
        }
        let alias_type = alias_type.ok_or(LookupError::InconsistentAlias { tile })?;
        let target = self
            .tile_type(alias_type)
            .ok_or(LookupError::UnknownTileType {
                tile,
                tile_type: alias_type,
            })?;
        let mapped = Self::map_alias_site(grid, blocks, feature).ok_or_else(unknown)?;
        let found = target
            .segbits
            .feature_to_bits(mapped, address)
            .ok_or_else(unknown)?;
        self.finish_lookup(grid, tile_ref, target, found)
    }

    /// `TileSegbitsAlias.map_feature_to_segbits` for the site component
    /// (the first component of `feature`), `None` if the renamed feature
    /// cannot be in any table.
    fn map_alias_site(grid: &Grid, blocks: &[BitsBlock], feature: IdString) -> Option<IdString> {
        let resolved = feature.resolved();
        let site = resolved.first_component();
        let mut mapped: Option<String> = None;
        for block in blocks {
            let alias = grid.alias(block)?;
            let current = mapped.as_deref().unwrap_or(site);
            if let Some(&(_, to)) = grid
                .alias_sites(alias)
                .iter()
                .find(|(from, _)| *from == current)
            {
                mapped = Some(to.resolve());
            }
        }
        match mapped {
            Some(mapped) if mapped != site => resolved
                .with_str(|full| IdString::lookup(&format!("{mapped}{}", &full[site.len()..]))),
            _ => Some(feature),
        }
    }

    fn finish_lookup<'a>(
        &'a self,
        grid: &'a Grid,
        tile: &'a Tile,
        segbits_type: &'a TileType,
        found: SegbitsMatch<'a>,
    ) -> Result<FeatureLookup<'a>, LookupError> {
        let entry = match found {
            SegbitsMatch::PseudoPip(ppip) => return Ok(FeatureLookup::PseudoPip(ppip)),
            SegbitsMatch::Entry(entry) => entry,
        };
        let block =
            grid.bits_block(tile, entry.block_type)
                .ok_or(LookupError::MissingBitsBlock {
                    tile: tile.name,
                    block_type: entry.block_type,
                })?;
        Ok(FeatureLookup::Bits(FeatureBits {
            architecture: self.architecture,
            tile,
            segbits_type,
            entry,
            block,
            offset: grid.effective_offset(block),
            bits: segbits_type.segbits.bits(entry),
        }))
    }

    /// Looks up a whole FASM feature name (`<tile>.<feature>`), splitting
    /// it at the first `.` like `FasmAssembler.add_fasm_line`, then calls
    /// [`Database::lookup_feature`]. Costs a resolve and two
    /// [`IdString::lookup`]s more than `lookup_feature`; allocation free.
    ///
    /// # Errors
    ///
    /// See [`Database::lookup_feature`].
    pub fn lookup_fasm_feature(
        &self,
        feature: IdString,
        address: u32,
    ) -> Result<FeatureLookup<'_>, LookupError> {
        let (tile, rest) = feature.with_str(|s| {
            let (tile, rest) = s.split_once('.').unwrap_or((s, ""));
            // A name that `lookup` cannot produce is in no table (every
            // key was interned at load time): intern it only to report
            // the error with the right precedence (tile, type, feature).
            let tile = IdString::lookup(tile).unwrap_or_else(|| IdString::new(tile));
            let rest = IdString::lookup(rest).unwrap_or_else(|| IdString::new(rest));
            (tile, rest)
        });
        self.lookup_feature(tile, rest, address)
    }

    /// Checks that no segbit of any tile of the grid can land on a bit the
    /// bitstream writer overwrites with the frame ECC
    /// ([`Architecture::ecc_reserved_bits`]; design document §6.5 and
    /// §8.3 item 19). Every (segbits tile type, bus, effective offset)
    /// combination used by a tile is checked once.
    ///
    /// Positions are computed exactly like the assembler places them
    /// ([`Architecture::segbit_position`], including the Python style
    /// wrap-around of negative words). Bits that cannot be placed at all
    /// are reported separately in [`EccReport::unplaceable`]: with the
    /// real databases these are only bits of alias tiles whose region is
    /// shorter than the aliased type's (the top `LIOB33_SING`/`LIOI3_SING`
    /// tiles of a clock region, `offset 99`, for the other site's bits),
    /// which prjxray drops with a warning. Without a grid (no part)
    /// nothing is checked.
    pub fn check_ecc_invariant(&self) -> EccReport {
        let mut report = EccReport::default();
        let Some(grid) = &self.grid else {
            return report;
        };
        let arch = self.architecture;
        let mut seen: HashSet<(u32, BlockType, i64)> = HashSet::default();
        for tile in grid.tiles() {
            let blocks = grid.bits(tile);
            let Some(own) = tile.tile_type_index() else {
                continue;
            };
            for block in blocks {
                let segbits_index = match grid.alias(block) {
                    None => Some(own),
                    Some(alias) => self
                        .tile_type_index
                        .get(&alias.tile_type)
                        .map(|&i| i as usize),
                };
                let Some(segbits_index) = segbits_index else {
                    continue;
                };
                let offset = grid.effective_offset(block);
                if !seen.insert((segbits_index as u32, block.block_type, offset)) {
                    continue;
                }
                let segbits_type = &self.tile_types[segbits_index];
                for entry in segbits_type.segbits.entries() {
                    if entry.block_type != block.block_type {
                        continue;
                    }
                    for &bit in segbits_type.segbits.bits(entry) {
                        report.checked_bits += 1;
                        let finding = |error| EccFinding {
                            tile: tile.name,
                            tile_type: tile.tile_type,
                            segbits_tile_type: segbits_type.name,
                            feature: entry.feature,
                            bit,
                            result: error,
                        };
                        match arch.segbit_position(block.base_address, offset, bit) {
                            Ok(position) if arch.is_ecc_bit(position) => {
                                report.violations.push(finding(Ok(position)));
                            }
                            Ok(_) => {}
                            Err(error) => report.unplaceable.push(finding(Err(error))),
                        }
                    }
                }
            }
        }
        report
    }
}

/// The result of a successful feature lookup.
#[derive(Clone, Copy, Debug)]
pub enum FeatureLookup<'db> {
    /// A pseudo PIP: valid, sets no bits and does not mark any frame in
    /// use.
    PseudoPip(PpipType),
    /// A feature with bits.
    Bits(FeatureBits<'db>),
}

/// The bits of a feature on a tile.
#[derive(Clone, Copy, Debug)]
pub struct FeatureBits<'db> {
    architecture: Architecture,
    /// The tile.
    pub tile: &'db Tile,
    /// The tile type whose segbits were used (the alias target for an
    /// aliased tile).
    pub segbits_type: &'db TileType,
    /// The segbits entry.
    pub entry: &'db SegbitsEntry,
    /// The tile's `bits` block for the entry's bus.
    pub block: &'db BitsBlock,
    /// The offset to use (`block.offset`, minus the alias
    /// `start_offset` for an aliased tile).
    pub offset: i64,
    /// The bits.
    pub bits: &'db [SegBit],
}

impl FeatureBits<'_> {
    /// The bus of the feature.
    pub fn block_type(&self) -> BlockType {
        self.entry.block_type
    }

    /// The frames of the bus of this tile (`base_address ..
    /// base_address + frames`), which the assembler marks in use.
    pub fn frames(&self) -> std::ops::Range<u32> {
        self.block.base_address..self.block.base_address.saturating_add(self.block.frames)
    }

    /// Each bit with its position (see
    /// [`Architecture::segbit_position`]).
    pub fn positions(
        &self,
    ) -> impl Iterator<Item = (SegBit, Result<BitPosition, BitPositionError>)> + '_ {
        self.bits.iter().map(move |&bit| {
            (
                bit,
                self.architecture
                    .segbit_position(self.block.base_address, self.offset, bit),
            )
        })
    }
}

/// Why a feature lookup failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookupError {
    /// The database was opened without a part.
    NoGrid,
    /// No tile of that name (prjxray: `KeyError` in
    /// `gridinfo_at_tilename`).
    UnknownTile {
        /// The tile.
        tile: IdString,
    },
    /// The tile's type (or its alias target) has no
    /// `tile_type_<TYPE>.json` in the family (prjxray: `KeyError`).
    UnknownTileType {
        /// The tile.
        tile: IdString,
        /// The tile type.
        tile_type: IdString,
    },
    /// An aliased tile whose `bits` blocks do not all have an alias to
    /// the same tile type (prjxray: `AttributeError` / `AssertionError`).
    InconsistentAlias {
        /// The tile.
        tile: IdString,
    },
    /// The feature (or `feature[address]`) is not in the tile type's
    /// segbits or pseudo PIPs (prjxray: `FasmLookupError`).
    UnknownFeature {
        /// The tile.
        tile: IdString,
        /// The tile's type.
        tile_type: IdString,
        /// The feature within the tile.
        feature: IdString,
        /// The bit address.
        address: u32,
    },
    /// The feature's bus has no `bits` block in the tile (prjxray:
    /// `KeyError` -> `FasmLookupError`).
    MissingBitsBlock {
        /// The tile.
        tile: IdString,
        /// The bus.
        block_type: BlockType,
    },
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LookupError::NoGrid => f.write_str("the database was opened without a part"),
            LookupError::UnknownTile { tile } => write!(f, "unknown tile {tile}"),
            LookupError::UnknownTileType { tile, tile_type } => {
                write!(f, "tile {tile}: unknown tile type {tile_type}")
            }
            LookupError::InconsistentAlias { tile } => {
                write!(f, "tile {tile}: inconsistent bits aliases")
            }
            LookupError::UnknownFeature {
                tile_type,
                feature,
                address,
                ..
            } => {
                write!(f, "Segment DB {tile_type}, key {tile_type}.{feature}")?;
                if *address != 0 {
                    write!(f, "[{address}]")?;
                }
                f.write_str(" not found")
            }
            LookupError::MissingBitsBlock { tile, block_type } => {
                write!(f, "tile {tile} has no {block_type} bits")
            }
        }
    }
}

impl std::error::Error for LookupError {}

/// A segbit found by [`Database::check_ecc_invariant`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EccFinding {
    /// A tile where it happens (the first one found).
    pub tile: IdString,
    /// That tile's type.
    pub tile_type: IdString,
    /// The tile type whose segbits hold the bit (differs for aliases).
    pub segbits_tile_type: IdString,
    /// The feature within the tile.
    pub feature: IdString,
    /// The segbit.
    pub bit: SegBit,
    /// Its position (an ECC bit), or why it has none.
    pub result: Result<BitPosition, BitPositionError>,
}

/// The result of [`Database::check_ecc_invariant`].
#[derive(Clone, Debug, Default)]
pub struct EccReport {
    /// Number of (tile type, bus, offset, segbit) combinations checked.
    pub checked_bits: usize,
    /// Segbits that land on an ECC bit (must be empty).
    pub violations: Vec<EccFinding>,
    /// Segbits that cannot be placed in a frame (see
    /// [`Database::check_ecc_invariant`]).
    pub unplaceable: Vec<EccFinding>,
}

/// Lists and loads the tile types of a family.
fn load_tile_types(
    root: &Path,
    layout: Layout,
) -> Result<(Vec<TileType>, HashMap<IdString, u32>), DbError> {
    let dir = match layout {
        Layout::Prjxray => root.to_path_buf(),
        Layout::Prjuray => root.join("tile_types"),
    };
    let entries = std::fs::read_dir(&dir).map_err(|e| DbError::from_io(&dir, e))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| DbError::from_io(&dir, e))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if let Some(stem) = file_name
            .strip_prefix("tile_type_")
            .and_then(|s| s.strip_suffix(".json"))
        {
            // prjxray: tile_type = stem.lower(); key = tile_type.upper().
            names.push(stem.to_ascii_uppercase());
        }
    }
    names.sort();
    names.dedup();
    let mut tile_types = Vec::with_capacity(names.len());
    let mut index = HashMap::default();
    for name in names {
        let lower = name.to_ascii_lowercase();
        let segbits_path = root.join(format!("segbits_{lower}.db"));
        let block_ram_path = root.join(format!("segbits_{lower}.block_ram.db"));
        let ppips_path = root.join(format!("ppips_{lower}.db"));
        let files = TileTypeFiles {
            segbits: is_file(&segbits_path),
            block_ram_segbits: is_file(&block_ram_path),
            ppips: is_file(&ppips_path),
            mask: is_file(&root.join(format!("mask_{lower}.db"))),
        };
        let mut builder = TileSegbitsBuilder::new(&name);
        if files.ppips {
            builder.read_ppips(&ppips_path)?;
        }
        if files.segbits {
            builder.read_segbits(&segbits_path, BlockType::ClbIoClk)?;
        }
        if files.block_ram_segbits {
            builder.read_segbits(&block_ram_path, BlockType::BlockRam)?;
        }
        let id = IdString::new(&name);
        index.insert(id, tile_types.len() as u32);
        tile_types.push(TileType {
            name: id,
            segbits: builder.finish(),
            files,
        });
    }
    Ok((tile_types, index))
}

fn yaml_error(path: &Path) -> impl Fn(YamlError) -> DbError + '_ {
    move |e| DbError::Yaml {
        path: path.to_path_buf(),
        line: e.line,
        message: e.message,
    }
}

/// `get_part_information` + `get_fabric_for_part`.
fn prjxray_fabric(root: &Path, part: &str) -> Result<(String, String), DbError> {
    let parts_path = root.join("mapping").join("parts.yaml");
    let parts = yaml::parse(&read_text(&parts_path)?).map_err(yaml_error(&parts_path))?;
    let entry = parts
        .get(part)
        .map_err(yaml_error(&parts_path))?
        .ok_or_else(|| DbError::UnknownPart {
            part: part.to_owned(),
            path: parts_path.clone(),
        })?;
    let device = entry
        .require("device")
        .and_then(|d| d.as_str().map(str::to_owned))
        .map_err(yaml_error(&parts_path))?;

    let devices_path = root.join("mapping").join("devices.yaml");
    let devices = yaml::parse(&read_text(&devices_path)?).map_err(yaml_error(&devices_path))?;
    let entry = devices
        .get(&device)
        .map_err(yaml_error(&devices_path))?
        .ok_or_else(|| DbError::UnknownDevice {
            device: device.clone(),
            path: devices_path.clone(),
        })?;
    let fabric = entry
        .require("fabric")
        .and_then(|f| f.as_str().map(str::to_owned))
        .map_err(yaml_error(&devices_path))?;
    if fabric.is_empty() || fabric.contains(['/', '\\']) || fabric == ".." {
        return Err(DbError::invalid(
            &devices_path,
            format!("invalid fabric name {fabric:?}"),
        ));
    }
    Ok((device, fabric))
}

fn load_part_info(
    root: &Path,
    layout: Layout,
    part: &str,
    default_arch: Architecture,
) -> Result<PartInfo, DbError> {
    let directory = root.join(part);
    let (device, fabric) = match layout {
        Layout::Prjxray => {
            let (device, fabric) = prjxray_fabric(root, part)?;
            (Some(device), fabric)
        }
        Layout::Prjuray => {
            if !directory.is_dir() {
                return Err(DbError::UnknownPart {
                    part: part.to_owned(),
                    path: directory,
                });
            }
            (None, part.to_owned())
        }
    };

    let yaml_path = directory.join("part.yaml");
    let yaml_part = if is_file(&yaml_path) {
        Some(Part::from_yaml_file(&yaml_path, default_arch)?)
    } else {
        None
    };
    let json_path = directory.join("part.json");
    let json = if is_file(&json_path) {
        let flat_arch = yaml_part.as_ref().map_or(default_arch, |p| p.architecture);
        PartJson::from_file(&json_path, flat_arch)?
    } else {
        PartJson::default()
    };
    let pins_path = directory.join("package_pins.csv");
    let package_pins = if is_file(&pins_path) {
        Some(read_package_pins(&pins_path)?)
    } else {
        None
    };
    let required_path = directory.join("required_features.fasm");
    let mut required_features = Vec::new();
    if is_file(&required_path) {
        let mut seen = std::collections::HashSet::new();
        for line in read_text(&required_path)?.lines() {
            let line = line.trim();
            if !line.is_empty() && seen.insert(line.to_owned()) {
                required_features.push(line.to_owned());
            }
        }
    }
    let idcode = yaml_part.as_ref().map(|p| p.idcode).or(json.idcode);
    Ok(PartInfo {
        name: part.to_owned(),
        directory,
        device,
        fabric,
        part: yaml_part.or(json.part),
        idcode,
        iobanks: json.iobanks,
        package_pins,
        required_features,
    })
}

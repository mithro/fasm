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

//! Part data: the configuration frame tree and IDCODE (`part.yaml` /
//! `part.json`), IO banks (`part.json`), package pins
//! (`package_pins.csv`) and the tile <-> IO bank map used for STEPDOWN
//! (design document §3.5-§3.7, §5 step 9).

use std::path::Path;

use fasm::idstring::IdString;
use foldhash::HashMap;
use serde_json::Value as Json;

use crate::arch::{Architecture, BlockType, FrameAddress};
use crate::error::{read_file, read_text, DbError};
use crate::json::{JStr, OrderedMap};
use crate::yaml::{self, Node, YamlError};

/// The frame counts of one bus of one row: `(column, frame_count)` in
/// ascending column order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigBus {
    /// The bus.
    pub block_type: BlockType,
    columns: Vec<(u32, u32)>,
}

impl ConfigBus {
    /// `(column, frame_count)` pairs in ascending column order.
    pub fn columns(&self) -> &[(u32, u32)] {
        &self.columns
    }

    fn column_index(&self, column: u32) -> Option<usize> {
        self.columns.binary_search_by_key(&column, |c| c.0).ok()
    }
}

/// One configuration row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigRow {
    /// Bottom global clock region (Series7 only; always `false` for
    /// UltraScale/UltraScale+, whose row number includes the half bit).
    pub bottom: bool,
    /// Row number as keyed in `part.yaml` (see
    /// [`FrameAddress::row_index`]).
    pub row: u32,
    buses: Vec<ConfigBus>,
}

impl ConfigRow {
    /// The buses of the row, in block type order.
    pub fn buses(&self) -> &[ConfigBus] {
        &self.buses
    }

    /// The bus for `block_type`.
    pub fn bus(&self, block_type: u8) -> Option<&ConfigBus> {
        self.buses.iter().find(|b| b.block_type.raw() == block_type)
    }
}

/// The C++ `Part` of prjxray / prjuray-tools: IDCODE and the tree of
/// configuration rows, buses and columns with their frame counts, which
/// defines every valid frame address of the device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// The architecture (frame address layout).
    pub architecture: Architecture,
    /// The device IDCODE.
    pub idcode: u32,
    /// Rows sorted by `(bottom, row)`.
    rows: Vec<ConfigRow>,
}

type Tree = Vec<ConfigRow>;

impl Part {
    /// Builds a part from its rows (sorted and checked like the loaders
    /// do).
    ///
    /// # Errors
    ///
    /// Returns a message if a row, column or frame count does not fit the
    /// frame address fields of `architecture` or a row is repeated.
    pub fn new(
        architecture: Architecture,
        idcode: u32,
        mut rows: Vec<ConfigRow>,
    ) -> Result<Self, String> {
        rows.sort_by_key(|r| (r.bottom, r.row));
        let max_row = if architecture.has_global_clock_regions() {
            u32::from(architecture.max_row())
        } else {
            (u32::from(architecture.max_row()) << 1) | 1
        };
        for row in &mut rows {
            if row.row > max_row {
                return Err(format!(
                    "row {} does not fit the frame address (max {max_row})",
                    row.row
                ));
            }
            if row.bottom && !architecture.has_global_clock_regions() {
                return Err("bottom region rows are Series7 only".to_owned());
            }
            row.buses.sort_by_key(|b| b.block_type);
            for bus in &mut row.buses {
                bus.columns.sort_by_key(|c| c.0);
                for pair in bus.columns.windows(2) {
                    if pair[0].0 == pair[1].0 {
                        return Err(format!("column {} is listed twice", pair[0].0));
                    }
                }
                for &(column, frame_count) in &bus.columns {
                    if column > u32::from(architecture.max_column()) {
                        return Err(format!("column {column} does not fit the frame address"));
                    }
                    if frame_count > u32::from(architecture.max_minor()) + 1 {
                        return Err(format!(
                            "frame_count {frame_count} of column {column} does not fit the frame address minor field"
                        ));
                    }
                }
            }
            for pair in row.buses.windows(2) {
                if pair[0].block_type == pair[1].block_type {
                    return Err(format!("bus {} is listed twice", pair[0].block_type));
                }
            }
        }
        for pair in rows.windows(2) {
            if (pair[0].bottom, pair[0].row) == (pair[1].bottom, pair[1].row) {
                return Err(format!("row {} is listed twice", pair[0].row));
            }
        }
        Ok(Part {
            architecture,
            idcode,
            rows,
        })
    }

    /// The C++ `Part(idcode, addresses)` constructor (used by prjxray's
    /// unit tests): the rows, buses and columns of the given frame
    /// addresses, each column with `max(minor) + 1` frames.
    ///
    /// # Errors
    ///
    /// A message for an address with a reserved block type, or see
    /// [`Part::new`].
    pub fn from_frame_addresses(
        architecture: Architecture,
        idcode: u32,
        addresses: impl IntoIterator<Item = FrameAddress>,
    ) -> Result<Self, String> {
        use std::collections::BTreeMap;
        type Buses = BTreeMap<BlockType, BTreeMap<u32, u32>>;
        let mut tree: BTreeMap<(bool, u32), Buses> = BTreeMap::new();
        for address in addresses {
            let block_type = address
                .block_type(architecture)
                .ok_or_else(|| format!("{address}: reserved block type"))?;
            let key = if architecture.has_global_clock_regions() {
                (
                    address.is_bottom_half(architecture),
                    u32::from(address.row(architecture)),
                )
            } else {
                (false, u32::from(address.row_index(architecture)))
            };
            let count = tree
                .entry(key)
                .or_default()
                .entry(block_type)
                .or_default()
                .entry(u32::from(address.column(architecture)))
                .or_insert(0);
            *count = (*count).max(u32::from(address.minor(architecture)) + 1);
        }
        let rows = tree
            .into_iter()
            .map(|((bottom, row), buses)| ConfigRow {
                bottom,
                row,
                buses: buses
                    .into_iter()
                    .map(|(block_type, columns)| ConfigBus {
                        block_type,
                        columns: columns.into_iter().collect(),
                    })
                    .collect(),
            })
            .collect();
        Part::new(architecture, idcode, rows)
    }

    /// Reads a `part.yaml` file. The architecture is taken from the tag
    /// (`!<xilinx/xc7series/part>`, `xcuseries`, `xcupseries`), or is
    /// `default_arch` for an untagged document.
    ///
    /// The nested `global_clock_regions` (Series7) / `rows` (UltraScale/+)
    /// form written by `gen_part_base_yaml` is supported, and for Series7
    /// also the `configuration_ranges` form the C++ decoder accepts (a
    /// list of `[begin, end)` frame address ranges, used by prjxray's
    /// `lib/test_data/configuration_test.yaml`), which is turned into rows
    /// like the C++ `Part(idcode, addresses)` constructor
    /// ([`Part::from_frame_addresses`]).
    ///
    /// # Errors
    ///
    /// [`DbError::Yaml`] with the line of the problem.
    pub fn from_yaml_file(path: &Path, default_arch: Architecture) -> Result<Self, DbError> {
        let text = read_text(path)?;
        Self::from_yaml_str(&text, default_arch).map_err(|e| DbError::Yaml {
            path: path.to_path_buf(),
            line: e.line,
            message: e.message,
        })
    }

    pub(crate) fn from_yaml_str(text: &str, default_arch: Architecture) -> Result<Self, YamlError> {
        let doc = yaml::parse(text)?;
        let arch = match doc.tag.as_deref() {
            None => default_arch,
            Some(tag) => tag
                .strip_prefix("xilinx/")
                .and_then(|t| t.strip_suffix("/part"))
                .and_then(Architecture::from_yaml_namespace)
                .ok_or_else(|| YamlError {
                    line: doc.line,
                    message: format!("unknown part tag {tag:?}"),
                })?,
        };
        let idcode = doc.require("idcode")?.as_u32()?;
        let mut rows = Vec::new();
        if arch.has_global_clock_regions() {
            let Some(regions) = doc.get("global_clock_regions")? else {
                if let Some(ranges) = doc.get("configuration_ranges")? {
                    let addresses = configuration_ranges(arch, ranges)?;
                    return Part::from_frame_addresses(arch, idcode, addresses).map_err(
                        |message| YamlError {
                            line: doc.line,
                            message,
                        },
                    );
                }
                return Err(unsupported_form(&doc));
            };
            for (name, bottom) in [("top", false), ("bottom", true)] {
                let region = regions.require(name)?;
                if let Some(region_rows) = region.get("rows")? {
                    yaml_rows(region_rows, bottom, &mut rows)?;
                }
            }
        } else {
            let Some(part_rows) = doc.get("rows")? else {
                return Err(unsupported_form(&doc));
            };
            yaml_rows(part_rows, false, &mut rows)?;
        }
        Part::new(arch, idcode, rows).map_err(|message| YamlError {
            line: doc.line,
            message,
        })
    }

    /// Reads the frame tree and IDCODE of a `part.json` file, or `None`
    /// if the file has no frame tree (like the miniature test database's
    /// `part.json`, which only has `iobanks`). Series7 files have
    /// `global_clock_regions`, UltraScale/+ files have `rows`
    /// (architecture `flat_arch`).
    ///
    /// # Errors
    ///
    /// [`DbError::Json`] for invalid JSON, [`DbError::Invalid`] for an
    /// unexpected shape (or a frame tree without `idcode`).
    pub fn from_json_file(path: &Path, flat_arch: Architecture) -> Result<Option<Self>, DbError> {
        let data = read_file(path)?;
        let json: Json = serde_json::from_slice(&data).map_err(|e| DbError::json(path, &e))?;
        Self::from_json(&json, flat_arch).map_err(|m| DbError::invalid(path, m))
    }

    pub(crate) fn from_json(json: &Json, flat_arch: Architecture) -> Result<Option<Self>, String> {
        let mut rows = Vec::new();
        let arch = if let Some(regions) = json.get("global_clock_regions") {
            for (name, bottom) in [("top", false), ("bottom", true)] {
                let region = regions
                    .get(name)
                    .ok_or_else(|| format!("global_clock_regions has no {name:?}"))?;
                if let Some(region_rows) = region.get("rows") {
                    json_rows(region_rows, bottom, &mut rows)?;
                }
            }
            Architecture::Series7
        } else if let Some(part_rows) = json.get("rows") {
            json_rows(part_rows, false, &mut rows)?;
            flat_arch
        } else {
            return Ok(None);
        };
        let idcode = json_idcode(json)?.ok_or("part.json has a frame tree but no idcode")?;
        Part::new(arch, idcode, rows).map(Some)
    }

    /// The rows, sorted by `(bottom, row)`.
    pub fn rows(&self) -> &[ConfigRow] {
        &self.rows
    }

    /// Total number of frames (sum of all `frame_count`s).
    pub fn frame_count(&self) -> usize {
        self.rows
            .iter()
            .flat_map(|r| &r.buses)
            .flat_map(|b| &b.columns)
            .map(|&(_, n)| n as usize)
            .sum()
    }

    /// The `(bottom, row)` key of the row holding `address`.
    fn row_key(&self, address: FrameAddress) -> (bool, u32) {
        let arch = self.architecture;
        if arch.has_global_clock_regions() {
            (address.is_bottom_half(arch), u32::from(address.row(arch)))
        } else {
            (false, u32::from(address.row_index(arch)))
        }
    }

    fn row_position(&self, key: (bool, u32)) -> Option<usize> {
        self.rows
            .binary_search_by_key(&key, |r| (r.bottom, r.row))
            .ok()
    }

    /// `Part::IsValidFrameAddress`: the row, bus and column exist and the
    /// minor is below the column's frame count.
    pub fn is_valid_frame_address(&self, address: FrameAddress) -> bool {
        self.row_position(self.row_key(address))
            .is_some_and(|i| self.row_is_valid(&self.rows[i], address))
    }

    fn row_is_valid(&self, row: &ConfigRow, address: FrameAddress) -> bool {
        let arch = self.architecture;
        let Some(bus) = row.bus(address.block_type_raw(arch)) else {
            return false;
        };
        let Some(j) = bus.column_index(u32::from(address.column(arch))) else {
            return false;
        };
        u32::from(address.minor(arch)) < bus.columns[j].1
    }

    /// `Part::GetNextFrameAddress` of prjxray (`xc7series/part.cc` and its
    /// helpers) / prjuray-tools (`xcupseries/part.cc`), ported literally:
    ///
    /// 1. the next minor of the column, else minor 0 of the next column
    ///    of the bus (if valid), else column 0 of the next row of the
    ///    region with the same block type (if valid);
    /// 2. (Series7) from the top region, row 0 of the bottom region;
    /// 3. `BLOCK_RAM`, then `CFG_CLB`, row 0 column 0 of the top region.
    ///
    /// Returns `None` after the last frame. Addresses that are not in the
    /// part get the reference's answer too: a minor beyond its column
    /// continues with the next column, an unknown row, bus or column with
    /// steps 2 and 3.
    pub fn next_frame_address(&self, address: FrameAddress) -> Option<FrameAddress> {
        let arch = self.architecture;
        let block_type = u32::from(address.block_type_raw(arch));
        let bottom = self.row_key(address).0;
        if let Some(next) = self.region_next(address) {
            return Some(next);
        }
        if arch.has_global_clock_regions() && !bottom {
            let next = FrameAddress::compose_masked(arch, block_type, true, 0, 0, 0);
            if self.is_valid_frame_address(next) {
                return Some(next);
            }
        }
        for later in [BlockType::BlockRam, BlockType::CfgClb] {
            if block_type < u32::from(later.raw()) {
                let next =
                    FrameAddress::compose_masked(arch, u32::from(later.raw()), false, 0, 0, 0);
                if self.is_valid_frame_address(next) {
                    return Some(next);
                }
            }
        }
        None
    }

    /// `GlobalClockRegion::GetNextFrameAddress` (Series7) /
    /// the row part of `Part::GetNextFrameAddress` (UltraScale/+).
    fn region_next(&self, address: FrameAddress) -> Option<FrameAddress> {
        let arch = self.architecture;
        let key = self.row_key(address);
        let i = self.row_position(key)?;
        if let Some(next) = self.row_next(&self.rows[i], address) {
            return Some(next);
        }
        let next_row = self.rows.get(i + 1).filter(|r| r.bottom == key.0)?;
        let next = FrameAddress::compose_row_index(
            arch,
            u32::from(address.block_type_raw(arch)),
            key.0,
            next_row.row,
            0,
            0,
        );
        self.row_is_valid(next_row, next).then_some(next)
    }

    /// `Row::GetNextFrameAddress` + `ConfigurationBus::GetNextFrameAddress`
    /// + `ConfigurationColumn::GetNextFrameAddress`.
    fn row_next(&self, row: &ConfigRow, address: FrameAddress) -> Option<FrameAddress> {
        let arch = self.architecture;
        let bus = row.bus(address.block_type_raw(arch))?;
        let j = bus.column_index(u32::from(address.column(arch)))?;
        let minor = u32::from(address.minor(arch));
        let frame_count = bus.columns[j].1;
        // `ConfigurationColumn::GetNextFrameAddress` returns nothing for a
        // minor beyond the column, and the bus then tries the next column
        // like for the last minor.
        if minor + 1 < frame_count {
            return Some(FrameAddress(address.0 + 1));
        }
        let &(column, _) = bus.columns.get(j + 1)?;
        let next = FrameAddress::compose_row_index(
            arch,
            u32::from(address.block_type_raw(arch)),
            self.row_key(address).0,
            row.row,
            column,
            0,
        );
        self.row_is_valid(row, next).then_some(next)
    }

    /// Every frame address `Frames::addMissingFrames` visits, in
    /// ascending order: address 0 (always, even if the part does not
    /// declare it, like prjxray), then [`Part::next_frame_address`] until
    /// it returns `None`.
    ///
    /// These are exactly the frames `xc7frames2bit` writes to a
    /// bitstream.
    pub fn iter_frame_addresses(&self) -> impl Iterator<Item = FrameAddress> + '_ {
        let mut current = Some(FrameAddress(0));
        std::iter::from_fn(move || {
            let address = current?;
            // Defensive: `next_frame_address` is strictly increasing for
            // parts accepted by `Part::new`; stop rather than loop if not.
            current = self
                .next_frame_address(address)
                .filter(|next| *next > address);
            Some(address)
        })
    }
}

/// The frame addresses of a `configuration_ranges` sequence: every
/// address in `[begin, end)` of each `configuration_frame_range`
/// (`YAML::convert<xc7series::Part>::decode`).
fn configuration_ranges(arch: Architecture, ranges: &Node) -> Result<Vec<FrameAddress>, YamlError> {
    let address = |node: &Node| -> Result<u32, YamlError> {
        let tag_ok = matches!(
            node.tag.as_deref(),
            Some("xilinx/xc7series/frame_address" | "xilinx/xc7series/configuration_frame_address")
        );
        let bad = |message: &str| YamlError {
            line: node.line,
            message: message.to_owned(),
        };
        if !tag_ok {
            return Err(bad(
                "expected a !<xilinx/xc7series/configuration_frame_address>",
            ));
        }
        let block_type = BlockType::from_name(node.require("block_type")?.as_str()?)
            .ok_or_else(|| bad("unknown block_type"))?;
        let bottom = match node.require("row_half")?.as_str()? {
            "top" => false,
            "bottom" => true,
            _ => return Err(bad("row_half is neither top nor bottom")),
        };
        Ok(FrameAddress::compose_masked(
            arch,
            u32::from(block_type.raw()),
            bottom,
            node.require("row")?.as_u32()?,
            node.require("column")?.as_u32()?,
            node.require("minor")?.as_u32()?,
        )
        .0)
    };
    let mut addresses = Vec::new();
    for range in ranges.as_seq()? {
        let begin = address(range.require("begin")?)?;
        let end = address(range.require("end")?)?;
        if end.saturating_sub(begin) > 1 << 26 {
            return Err(YamlError {
                line: range.line,
                message: "configuration range too large".to_owned(),
            });
        }
        addresses.extend((begin..end).map(FrameAddress));
    }
    Ok(addresses)
}

fn unsupported_form(doc: &Node) -> YamlError {
    let message = if doc.get("configuration_ranges").ok().flatten().is_some() {
        "the configuration_ranges form of part.yaml is only supported for Series7"
    } else {
        "part.yaml has no global_clock_regions / rows"
    };
    YamlError {
        line: doc.line,
        message: message.to_owned(),
    }
}

fn key_u32(key: &str, line: usize) -> Result<u32, YamlError> {
    yaml::parse_u32(key).ok_or_else(|| YamlError {
        line,
        message: format!("key {key:?} is not an unsigned integer"),
    })
}

fn yaml_rows(rows_node: &Node, bottom: bool, out: &mut Tree) -> Result<(), YamlError> {
    for (row_key, row) in rows_node.as_map()? {
        let mut buses = Vec::new();
        if let Some(buses_node) = row.get("configuration_buses")? {
            for (bus_name, bus) in buses_node.as_map()? {
                let block_type = BlockType::from_name(bus_name).ok_or_else(|| YamlError {
                    line: bus.line,
                    message: format!("unknown block type {bus_name:?}"),
                })?;
                let mut columns = Vec::new();
                if let Some(columns_node) = bus.get("configuration_columns")? {
                    for (column_key, column) in columns_node.as_map()? {
                        columns.push((
                            key_u32(column_key, column.line)?,
                            column.require("frame_count")?.as_u32()?,
                        ));
                    }
                }
                buses.push(ConfigBus {
                    block_type,
                    columns,
                });
            }
        }
        out.push(ConfigRow {
            bottom,
            row: key_u32(row_key, row.line)?,
            buses,
        });
    }
    Ok(())
}

fn json_map<'a>(value: &'a Json, what: &str) -> Result<&'a serde_json::Map<String, Json>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{what} is not an object"))
}

fn json_key(key: &str) -> Result<u32, String> {
    crate::segbits::parse_u32(key).ok_or_else(|| format!("key {key:?} is not an unsigned integer"))
}

fn json_u32(value: &Json, what: &str) -> Result<u32, String> {
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| format!("{what} is not an unsigned 32-bit integer"))
}

fn json_rows(rows_value: &Json, bottom: bool, out: &mut Tree) -> Result<(), String> {
    for (row_key, row) in json_map(rows_value, "rows")? {
        let mut buses = Vec::new();
        if let Some(buses_value) = row.get("configuration_buses") {
            for (bus_name, bus) in json_map(buses_value, "configuration_buses")? {
                let block_type = BlockType::from_name(bus_name)
                    .ok_or_else(|| format!("unknown block type {bus_name:?}"))?;
                let mut columns = Vec::new();
                if let Some(columns_value) = bus.get("configuration_columns") {
                    for (column_key, column) in json_map(columns_value, "configuration_columns")? {
                        let frame_count = column
                            .get("frame_count")
                            .ok_or_else(|| format!("column {column_key} has no frame_count"))?;
                        columns
                            .push((json_key(column_key)?, json_u32(frame_count, "frame_count")?));
                    }
                }
                buses.push(ConfigBus {
                    block_type,
                    columns,
                });
            }
        }
        out.push(ConfigRow {
            bottom,
            row: json_key(row_key)?,
            buses,
        });
    }
    Ok(())
}

fn json_idcode(json: &Json) -> Result<Option<u32>, String> {
    json.get("idcode")
        .map(|v| json_u32(v, "idcode"))
        .transpose()
}

/// The part level data of `part.json` other than the frame tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PartJson {
    pub(crate) part: Option<Part>,
    pub(crate) idcode: Option<u32>,
    pub(crate) iobanks: Option<Vec<(IdString, IdString)>>,
}

impl PartJson {
    pub(crate) fn from_file(path: &Path, flat_arch: Architecture) -> Result<Self, DbError> {
        let data = read_file(path)?;
        let json: Json = serde_json::from_slice(&data).map_err(|e| DbError::json(path, &e))?;
        // A second, typed pass keeps `iobanks` in file order (a
        // `serde_json::Value` map is sorted).
        #[derive(serde::Deserialize)]
        struct Banks<'a> {
            #[serde(default, borrow)]
            iobanks: Option<OrderedMap<JStr<'a>, JStr<'a>>>,
        }
        let banks: Banks<'_> =
            serde_json::from_slice(&data).map_err(|e| DbError::json(path, &e))?;
        let parse = || -> Result<Self, String> {
            let iobanks = banks.iobanks.map(|banks| {
                banks
                    .0
                    .iter()
                    .map(|(bank, loc)| (IdString::new(bank.as_str()), IdString::new(loc.as_str())))
                    .collect()
            });
            Ok(PartJson {
                part: Part::from_json(&json, flat_arch)?,
                idcode: json_idcode(&json)?,
                iobanks,
            })
        };
        parse().map_err(|m| DbError::invalid(path, m))
    }
}

/// One row of `package_pins.csv`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackagePin {
    /// Package pin, e.g. `A1`.
    pub pin: IdString,
    /// IO bank, e.g. `35` (kept as a string, like prjxray).
    pub bank: IdString,
    /// Site, e.g. `IOB_X1Y81`.
    pub site: IdString,
    /// Tile, e.g. `RIOB33_X43Y81`.
    pub tile: IdString,
    /// Pin function, e.g. `IO_L9N_T1_DQS_AD7N_35`.
    pub pin_function: IdString,
}

/// Splits one CSV record (RFC 4180 quoting, no embedded newlines).
fn csv_fields(line: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    let mut at_start = true;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' && at_start {
            quoted = true;
            at_start = false;
        } else if c == ',' {
            fields.push(std::mem::take(&mut field));
            at_start = true;
        } else {
            field.push(c);
            at_start = false;
        }
    }
    if quoted {
        return Err("unterminated quoted field".to_owned());
    }
    fields.push(field);
    Ok(fields)
}

/// Reads `package_pins.csv` (`csv.DictReader`: columns by header name;
/// `bank` and `tile` are required, the others default to empty).
///
/// # Errors
///
/// [`DbError::Csv`] with the line of a malformed record.
pub fn read_package_pins(path: &Path) -> Result<Vec<PackagePin>, DbError> {
    let text = read_text(path)?;
    parse_package_pins(&text).map_err(|(line, message)| DbError::Csv {
        path: path.to_path_buf(),
        line,
        message,
    })
}

fn parse_package_pins(text: &str) -> Result<Vec<PackagePin>, (usize, String)> {
    let mut lines = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .filter(|(_, l)| !l.trim().is_empty());
    let Some((header_line, header)) = lines.next() else {
        return Ok(Vec::new());
    };
    let header = csv_fields(header).map_err(|m| (header_line, m))?;
    let column = |name: &str| header.iter().position(|h| h == name);
    let (Some(bank), Some(tile)) = (column("bank"), column("tile")) else {
        return Err((
            header_line,
            "header must have `bank` and `tile` columns".to_owned(),
        ));
    };
    let (pin, site, function) = (column("pin"), column("site"), column("pin_function"));
    let empty = IdString::new("");
    let mut pins = Vec::new();
    for (line, record) in lines {
        let fields = csv_fields(record).map_err(|m| (line, m))?;
        if fields.len() != header.len() {
            return Err((
                line,
                format!("{} fields, the header has {}", fields.len(), header.len()),
            ));
        }
        let get = |i: Option<usize>| i.map_or(empty, |i| IdString::new(&fields[i]));
        pins.push(PackagePin {
            pin: get(pin),
            bank: get(Some(bank)),
            site: get(site),
            tile: get(Some(tile)),
            pin_function: get(function),
        });
    }
    Ok(pins)
}

/// The IO bank <-> tile map of `fasm2frames.py` (`bank_to_tile`,
/// `tile_to_bank`), used for STEPDOWN propagation: for every `iobanks`
/// entry the tile `HCLK_IOI3_<loc>`, then the tile of every package pin.
/// A tile's bank is the last one assigned; tiles of a bank are kept in
/// first seen order without duplicates.
#[derive(Clone, Debug, Default)]
pub struct BanksTilesRegistry {
    banks: Vec<(IdString, Vec<IdString>)>,
    bank_index: HashMap<IdString, u32>,
    tile_to_bank: HashMap<IdString, IdString>,
}

impl BanksTilesRegistry {
    /// Builds the registry from `part.json` `iobanks` and the package pins.
    pub fn new(iobanks: &[(IdString, IdString)], package_pins: &[PackagePin]) -> Self {
        let mut registry = BanksTilesRegistry::default();
        for &(bank, loc) in iobanks {
            let tile = IdString::new(&format!("HCLK_IOI3_{loc}"));
            registry.add(bank, tile);
        }
        for pin in package_pins {
            registry.add(pin.bank, pin.tile);
        }
        registry
    }

    fn add(&mut self, bank: IdString, tile: IdString) {
        let index = *self.bank_index.entry(bank).or_insert_with(|| {
            self.banks.push((bank, Vec::new()));
            (self.banks.len() - 1) as u32
        });
        let tiles = &mut self.banks[index as usize].1;
        if !tiles.contains(&tile) {
            tiles.push(tile);
        }
        self.tile_to_bank.insert(tile, bank);
    }

    /// The bank of a tile.
    pub fn bank_of_tile(&self, tile: IdString) -> Option<IdString> {
        self.tile_to_bank.get(&tile).copied()
    }

    /// The tiles of a bank (empty for an unknown bank).
    pub fn tiles_of_bank(&self, bank: IdString) -> &[IdString] {
        self.bank_index
            .get(&bank)
            .map_or(&[], |&i| &self.banks[i as usize].1)
    }

    /// All banks, in first seen order.
    pub fn banks(&self) -> impl Iterator<Item = IdString> + '_ {
        self.banks.iter().map(|(bank, _)| *bank)
    }
}

#[cfg(test)]
mod tests;

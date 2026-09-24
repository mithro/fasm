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

//! Xilinx bitstream support for FASM.
//!
//! This crate provides, per `docs/rewrite/PLAN.md`:
//!
//! * a database loader for prjxray-db (Series7) and prjuray-db
//!   (UltraScale+) family directories: [`Database::open`] reads the
//!   tile grid (`tilegrid.json`), the segbits and pseudo PIPs of every
//!   tile type (`segbits_*.db`, `segbits_*.block_ram.db`, `ppips_*.db`)
//!   and the part data (`part.yaml`, `part.json`, `package_pins.csv`,
//!   `required_features.fasm`), and resolves FASM features to
//!   configuration bits ([`Database::lookup_feature`]);
//! * frame address arithmetic for Series7, UltraScale and UltraScale+
//!   ([`FrameAddress`], [`Architecture::segbit_position`],
//!   [`Part::iter_frame_addresses`]).
//!
//! Still to come (tasks T5.3-T5.6, T6.x): a binary cache of a loaded
//! database, the `FasmAssembler` / `fasm2frames` equivalent and the
//! bitstream writer and reader.
//!
//! The file formats and the reference behaviour are described in
//! `docs/rewrite/DESIGN-xilinx-db.md`.
//!
//! ```no_run
//! use std::path::Path;
//! use fasm::idstring::IdString;
//! use fasm_xilinx::{Database, FeatureLookup};
//!
//! let db = Database::open(Path::new("prjxray-db/artix7"), Some("xc7a35tcsg324-1"))?;
//! let feature = IdString::new("CLBLM_R_X33Y38.SLICEL_X1.A5FF.ZINI");
//! if let FeatureLookup::Bits(bits) = db.lookup_fasm_feature(feature, 0)? {
//!     for (segbit, position) in bits.positions() {
//!         println!("{segbit} -> {:?}", position);
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod arch;
mod db;
mod error;
mod json;
mod part;
mod segbits;
mod tilegrid;
mod yaml;

pub use arch::{
    Architecture, BitPosition, BitPositionError, BlockType, FrameAddress, FrameAddressFields,
};
pub use db::{
    Database, EccFinding, EccReport, FeatureBits, FeatureLookup, Layout, LookupError, PartInfo,
    TileType, TileTypeFiles,
};
pub use error::DbError;
pub use part::{read_package_pins, BanksTilesRegistry, ConfigBus, ConfigRow, PackagePin, Part};
pub use segbits::{PpipType, SegBit, SegbitsEntry, SegbitsMatch, TileSegbits};
pub use tilegrid::{BitAlias, BitsBlock, ClockRegion, Grid, Tile};

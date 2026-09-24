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

//! Xilinx bitstream support for FASM (work in progress, task T5.2):
//! frame addresses, the segbits / pseudo PIP tables, tile grid and part
//! files of prjxray-db and prjuray-db. See `docs/rewrite/DESIGN-xilinx-db.md`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod arch;
mod error;
mod json;
mod part;
mod segbits;
mod tilegrid;
mod yaml;

pub use arch::{
    Architecture, BitPosition, BitPositionError, BlockType, FrameAddress, FrameAddressFields,
};
pub use error::DbError;
pub use part::{read_package_pins, BanksTilesRegistry, ConfigBus, ConfigRow, PackagePin, Part};
pub use segbits::{PpipType, SegBit, SegbitsEntry, SegbitsMatch, TileSegbits};
pub use tilegrid::{BitAlias, BitsBlock, ClockRegion, Grid, Tile};

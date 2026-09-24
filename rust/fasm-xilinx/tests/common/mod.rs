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

//! Paths shared by the integration tests.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use fasm::idstring::IdString;
use fasm_xilinx::{BitPosition, Database, FeatureLookup};

/// `rust/fasm-xilinx/testdata/<name>`.
pub fn testdata(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join(name)
}

/// The repository root.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A real database family fetched by `tools/fetch-db.sh`
/// (`$FASM_DB_CACHE/<database>/<family>`, else
/// `tests/oracle/build/db/<database>/<family>`), or `None` (with a
/// message) if it is not present.
pub fn real_db(database: &str, family: &str) -> Option<PathBuf> {
    let cache = std::env::var_os("FASM_DB_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("tests/oracle/build/db"));
    let path = cache.join(database).join(family);
    if path.is_dir() {
        Some(path)
    } else {
        eprintln!(
            "skipping: {} not found (run `tools/fetch-db.sh {} {family}` or set FASM_DB_CACHE)",
            path.display(),
            database.trim_end_matches("-db"),
        );
        None
    }
}

/// The positions of the bits of a FASM feature (with `!` bits), panicking
/// on lookup errors.
pub fn positions(db: &Database, feature: &str, address: u32) -> Vec<(bool, BitPosition)> {
    match db.lookup_fasm_feature(IdString::new(feature), address) {
        Ok(FeatureLookup::Bits(bits)) => bits
            .positions()
            .map(|(bit, pos)| (bit.is_set, pos.unwrap_or_else(|e| panic!("{feature}: {e}"))))
            .collect(),
        Ok(FeatureLookup::PseudoPip(_)) => Vec::new(),
        Err(e) => panic!("{feature}[{address}]: {e}"),
    }
}

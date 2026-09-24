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

//! Build script: computes `FASM_XILINX_LOADER_FINGERPRINT`, a 64-bit
//! FNV-1a hash of every file under `src/` (paths and contents, in sorted
//! order) and the package version.
//!
//! The binary database cache (`src/cache/`) stores it in every cache file
//! and rebuilds a cache file written by a build with another fingerprint,
//! so a change to the text loader (or to the cache code) can never be
//! hidden by a cache file written by an older build. It only has to change
//! when the sources change, so a simple non-cryptographic hash is enough;
//! a spurious change (e.g. an edited comment) costs one rebuild of each
//! cache file.

use std::fs;
use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("read_dir entry").path();
        if path.is_dir() {
            collect(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn fnv1a(hash: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *hash ^= u64::from(b);
        *hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
}

fn main() {
    let src = Path::new("src");
    println!("cargo:rerun-if-changed=src");
    let mut files = Vec::new();
    collect(src, &mut files);
    files.sort();
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    fnv1a(&mut hash, env!("CARGO_PKG_VERSION").as_bytes());
    for file in files {
        let name = file.strip_prefix(src).unwrap_or(&file);
        // Separators are normalised so that the value does not depend on
        // the host.
        let name = name.to_string_lossy().replace('\\', "/");
        fnv1a(&mut hash, &(name.len() as u64).to_le_bytes());
        fnv1a(&mut hash, name.as_bytes());
        let data = fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        fnv1a(&mut hash, &(data.len() as u64).to_le_bytes());
        fnv1a(&mut hash, &data);
    }
    println!("cargo:rustc-env=FASM_XILINX_LOADER_FINGERPRINT={hash:016x}");
}

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

//! The binary database cache on the real prjxray-db `artix7` and
//! prjuray-db `zynqusp` databases: a part loaded from its cache file
//! equals the part loaded from the text files, and the cache file
//! verifies. Skipped (passes with a message) when the database has not
//! been fetched (`tools/fetch-db.sh`; `FASM_DB_CACHE` for a cache outside
//! this checkout). The times printed (`--nocapture`) are those of the
//! test profile; `cargo bench -p fasm-xilinx --bench db` measures the
//! release build in fresh processes.

mod common;

use std::path::Path;
use std::time::Instant;

use fasm_xilinx::cache::{self, CacheOptions, CacheOutcome};
use fasm_xilinx::Database;

use common::real_db;

fn round_trip(root: &Path, part: &str) {
    let dir = std::env::temp_dir().join(format!(
        "fasm-xilinx-cache-real-{}-{part}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let options = CacheOptions::in_directory(&dir);

    let start = Instant::now();
    let text = Database::open(root, Some(part)).unwrap();
    let t_text = start.elapsed();

    let start = Instant::now();
    let (built, outcome) = cache::open(root, Some(part), &options).unwrap();
    let t_build = start.elapsed();
    assert!(
        matches!(
            outcome,
            CacheOutcome::Rebuilt {
                write_error: None,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert!(built == text);

    let start = Instant::now();
    let (cached, outcome) = cache::open(root, Some(part), &options).unwrap();
    let t_cached = start.elapsed();
    let CacheOutcome::Hit {
        path,
        restat: false,
    } = outcome
    else {
        panic!("{outcome:?}");
    };
    assert!(cached == text, "{part}: the cached database differs");

    let report = cache::verify_file(&path);
    assert_eq!(report.problem, None);
    let info = report.info.unwrap();
    eprintln!(
        "{part}: text {:.1} ms, text + write {:.1} ms, cached {:.1} ms; file {:.1} MiB, {} sources ({:.1} MiB)",
        t_text.as_secs_f64() * 1e3,
        t_build.as_secs_f64() * 1e3,
        t_cached.as_secs_f64() * 1e3,
        info.file_len as f64 / (1024.0 * 1024.0),
        info.sources.len(),
        info.source_bytes() as f64 / (1024.0 * 1024.0),
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn artix7_parts_round_trip() {
    let Some(root) = real_db("prjxray-db", "artix7") else {
        return;
    };
    for part in ["xc7a35tcsg324-1", "xc7a200tffg1156-1"] {
        round_trip(&root, part);
    }
}

#[test]
fn zynqusp_part_round_trips() {
    let Some(root) = real_db("prjuray-db", "zynqusp") else {
        return;
    };
    round_trip(&root, "xczu3eg-sfvc784-1-e");
}

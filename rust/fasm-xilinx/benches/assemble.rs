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

//! Assembler benchmark (`cargo bench -p fasm-xilinx --bench assemble`).
//!
//! Assembles a FASM file for a part of the prjxray-db `artix7` database
//! (from `$FASM_DB_CACHE` or `tests/oracle/build/db`) and reports the time
//! of each phase: opening the database, parsing, assembling
//! ([`FasmAssembler::parse_fasm_bytes`] minus the parse), dense and sparse
//! [`FasmAssembler::get_frames`] and writing the `.frm` text, plus the
//! resident memory.
//!
//! `FASM_BENCH_FASM` and `FASM_BENCH_PART` select the FASM file and the
//! part (default: the counter_test design of the corpus on
//! xc7a35tcsg324-1). Skipped when the database is missing.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fasm_xilinx::{Database, FasmAssembler};

/// Resident set size in bytes (Linux), 0 elsewhere.
fn rss() -> usize {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| {
            s.split_whitespace()
                .nth(1)
                .and_then(|p| p.parse::<usize>().ok())
        })
        .map_or(0, |pages| pages * 4096)
}

fn mib(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn main() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("FASM_DB_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("tests/oracle/build/db"));
    let root = cache.join("prjxray-db/artix7");
    if !root.is_dir() {
        println!("skipping: {} not found", root.display());
        return;
    }
    let fasm = std::env::var_os("FASM_BENCH_FASM")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            repo.join(
                "tests/corpus/xilinx/artix7/designs/f4pga-examples/counter_test/arty_35/top.fasm",
            )
        });
    let part = std::env::var("FASM_BENCH_PART").unwrap_or_else(|_| "xc7a35tcsg324-1".into());
    println!("== {} on {part}", fasm.display());

    let start = Instant::now();
    let db = Database::open(&root, Some(&part)).unwrap();
    println!("  open database      {:8.1} ms", ms(start.elapsed()));
    let data = std::fs::read(&fasm).unwrap();

    let start = Instant::now();
    let lines = fasm::parse_fasm_bytes(&data).unwrap();
    let parse = start.elapsed();
    println!(
        "  parse              {:8.1} ms ({} lines)",
        ms(parse),
        lines.len()
    );
    drop(lines);

    let rss_before = rss();
    let mut assembler = FasmAssembler::new(&db).unwrap();
    let start = Instant::now();
    assembler.parse_fasm_bytes(&data, Vec::new()).unwrap();
    let total = start.elapsed();
    println!(
        "  assemble           {:8.1} ms (parse + assemble {:.1} ms)",
        ms(total.saturating_sub(parse)),
        ms(total)
    );
    for sparse in [false, true] {
        let start = Instant::now();
        let frames = assembler.get_frames(sparse).unwrap();
        let get = start.elapsed();
        let start = Instant::now();
        let mut out = Vec::with_capacity(frames.len() * 1122);
        frames.write_frm(&mut out).unwrap();
        println!(
            "  get_frames({sparse:5}) {:8.1} ms, write_frm {:6.1} ms ({} frames, {:.1} MiB)",
            ms(get),
            ms(start.elapsed()),
            frames.len(),
            mib(out.len())
        );
    }
    println!(
        "  RSS growth of the assembler {:.1} MiB",
        mib(rss().saturating_sub(rss_before))
    );
}

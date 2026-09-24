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

//! Database loader benchmark (`cargo bench -p fasm-xilinx --bench db`).
//!
//! Opens real prjxray-db / prjuray-db parts (from `$FASM_DB_CACHE` or
//! `tests/oracle/build/db`, see `tools/fetch-db.sh`) and reports:
//!
//! * the time to open the database fully (first open in a fresh process,
//!   so the interner starts empty, then the best of a few re-opens, which
//!   only hit the interner) and a breakdown (tile types, tilegrid);
//! * resident memory growth of the first open (`/proc/self/statm`) and the
//!   interner's heap;
//! * [`Database::open_cached`] (the binary cache, T5.3): writing the cache
//!   file in a fresh process, then loading it in fresh processes (empty
//!   interner, like the command line tools), best of three;
//! * feature lookup cost: with pre-split (tile, feature) handles
//!   ([`Database::lookup_feature`]), from the whole FASM feature handle
//!   ([`Database::lookup_fasm_feature`], which splits with `IdString::lookup`)
//!   and, for comparison, interning the feature remainder with
//!   `IdString::new`.
//!
//! Falls back to the miniature test database when no real one is found.

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fasm::idstring::{IdString, GLOBAL};
use fasm_xilinx::cache::{self, CacheOptions, CacheOutcome};
use fasm_xilinx::{Database, Grid};

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

fn cache_dir() -> PathBuf {
    std::env::var_os("FASM_DB_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/oracle/build/db")
        })
}

/// Builds `(full feature, tile, feature in tile, address)` for up to
/// `per_tile` segbits entries of every tile without an alias.
fn features(db: &Database, per_tile: usize) -> Vec<(IdString, IdString, IdString, u32)> {
    let grid: &Grid = db.grid().unwrap();
    let mut out = Vec::new();
    for tile in grid.tiles() {
        if grid.bits(tile).iter().any(|b| b.has_alias()) {
            continue;
        }
        let Some(tile_type) = db.tile_type_of(tile) else {
            continue;
        };
        for entry in tile_type.segbits.entries().iter().take(per_tile) {
            let name = entry.feature.to_string();
            let (base, address) = match name.rfind('[') {
                Some(open) => (
                    &name[..open],
                    name[open + 1..name.len() - 1].parse().unwrap(),
                ),
                None => (name.as_str(), 0),
            };
            let full = IdString::new(&format!("{}.{base}", tile.name));
            out.push((full, tile.name, IdString::new(base), address));
        }
    }
    out
}

fn time_per_op(n: usize, mut f: impl FnMut()) -> f64 {
    let start = Instant::now();
    f();
    start.elapsed().as_secs_f64() * 1e9 / n as f64
}

fn bench_part(root: &Path, part: &str) {
    println!("== {} {part}", root.display());
    let rss_before = rss();
    let interner_before = GLOBAL.stats().heap_bytes;
    let start = Instant::now();
    let db = Database::open(root, Some(part)).unwrap();
    let first = start.elapsed();
    let rss_after = rss();
    let interner_after = GLOBAL.stats().heap_bytes;
    let grid = db.grid().unwrap();
    let entries: usize = db.tile_types().iter().map(|t| t.segbits.len()).sum();
    let bits: usize = db.tile_types().iter().map(|t| t.segbits.bit_count()).sum();
    println!(
        "  {} tiles, {} tile types, {entries} segbits entries, {bits} bits, {} frames",
        grid.len(),
        db.tile_types().len(),
        db.part().map_or(0, |p| p.frame_count())
    );
    println!(
        "  first open: {:.1} ms; RSS +{:.1} MiB (interner heap +{:.1} MiB)",
        ms(first),
        mib(rss_after.saturating_sub(rss_before)),
        mib(interner_after.saturating_sub(interner_before)),
    );

    let mut best = Duration::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        black_box(Database::open(root, Some(part)).unwrap());
        best = best.min(start.elapsed());
    }
    let mut types = Duration::MAX;
    let mut tilegrid = Duration::MAX;
    let fabric = &db.part_info().unwrap().fabric;
    let grid_path = if root.join("mapping").is_dir() {
        root.join(fabric).join("tilegrid.json")
    } else {
        root.join(part).join("tilegrid.json")
    };
    let grid_bytes = std::fs::metadata(&grid_path).map_or(0, |m| m.len() as usize);
    for _ in 0..3 {
        let start = Instant::now();
        black_box(Database::open(root, None).unwrap());
        types = types.min(start.elapsed());
        let start = Instant::now();
        black_box(Grid::from_file(&grid_path).unwrap());
        tilegrid = tilegrid.min(start.elapsed());
    }
    println!(
        "  re-open (warm interner): {:.1} ms = tile types/segbits {:.1} ms + tilegrid.json ({:.1} MiB) {:.1} ms + part files",
        ms(best),
        ms(types),
        mib(grid_bytes),
        ms(tilegrid),
    );

    let start = Instant::now();
    let report = db.check_ecc_invariant();
    println!(
        "  ECC invariant check: {:.1} ms, {} bits, {} violations",
        ms(start.elapsed()),
        report.checked_bits,
        report.violations.len()
    );

    let list = features(&db, 16);
    let n = list.len();
    let reps = (2_000_000 / n.max(1)).max(1);
    let split = time_per_op(n * reps, || {
        for _ in 0..reps {
            for &(_, tile, feature, address) in &list {
                black_box(db.lookup_feature(tile, feature, address).is_ok());
            }
        }
    });
    let whole = time_per_op(n * reps, || {
        for _ in 0..reps {
            for &(full, _, _, address) in &list {
                black_box(db.lookup_fasm_feature(full, address).is_ok());
            }
        }
    });
    let strings: Vec<String> = list.iter().map(|&(_, _, f, _)| f.to_string()).collect();
    let intern = time_per_op(n * reps, || {
        for _ in 0..reps {
            for s in &strings {
                black_box(IdString::new(s));
            }
        }
    });
    let resolve = time_per_op(n * reps, || {
        for _ in 0..reps {
            for &(full, _, _, _) in &list {
                black_box(full.with_str(|s| s.len()));
            }
        }
    });
    println!(
        "  lookup ({n} features x {reps}): lookup_feature {split:.1} ns, lookup_fasm_feature {whole:.1} ns \
         (resolve of the full name alone {resolve:.1} ns); IdString::new(remainder) {intern:.1} ns"
    );
}

/// One [`cache::open`] in this (fresh) process: prints the outcome and
/// the time in ms, and checks the result against [`Database::open`].
fn cached_open(root: &Path, part: &str, dir: &Path) {
    let options = CacheOptions {
        verbose: std::env::var_os(cache::VERBOSE_ENV).is_some(),
        ..CacheOptions::in_directory(dir)
    };
    let start = Instant::now();
    let (db, outcome) = cache::open(root, Some(part), &options).unwrap();
    let time = ms(start.elapsed());
    let kind = match outcome {
        CacheOutcome::Hit { .. } => "hit",
        CacheOutcome::Rebuilt { .. } => "rebuilt",
        _ => "other",
    };
    assert!(db == Database::open(root, Some(part)).unwrap());
    println!("{kind} {time:.3}");
}

/// Runs [`cached_open`] in a fresh process.
fn cached_open_process(exe: &Path, root: &Path, part: &str, dir: &Path) -> (String, f64) {
    let out = std::process::Command::new(exe)
        .arg("--cached")
        .arg(root)
        .arg(part)
        .arg(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{part}: {}", out.status);
    let text = String::from_utf8(out.stdout).unwrap();
    let (kind, time) = text.trim().split_once(' ').unwrap();
    (kind.to_owned(), time.parse().unwrap())
}

fn bench_cached(exe: &Path, root: &Path, part: &str) {
    let dir = std::env::temp_dir().join(format!("fasm-db-bench-{}", std::process::id()));
    let (kind, build) = cached_open_process(exe, root, part, &dir);
    assert_eq!(kind, "rebuilt");
    let mut best = f64::MAX;
    for _ in 0..3 {
        let (kind, time) = cached_open_process(exe, root, part, &dir);
        assert_eq!(kind, "hit");
        best = best.min(time);
    }
    let size: u64 = cache::cache_files(&dir)
        .unwrap()
        .iter()
        .map(|f| std::fs::metadata(f).unwrap().len())
        .sum();
    println!(
        "  open_cached (fresh process): {best:.1} ms from the cache file ({:.1} MiB); \
         {build:.1} ms to load the text files and write it",
        mib(size as usize)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Runs each part in a fresh process (`<exe> --part <root> <part>`) so
/// that the first open starts with an empty interner and its memory
/// growth is not hidden by a previous part.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--part") {
        bench_part(Path::new(&args[i + 1]), &args[i + 2]);
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--cached") {
        cached_open(
            Path::new(&args[i + 1]),
            &args[i + 2],
            Path::new(&args[i + 3]),
        );
        return;
    }
    let cache = cache_dir();
    let artix7 = cache.join("prjxray-db/artix7");
    let zynqusp = cache.join("prjuray-db/zynqusp");
    let mut runs: Vec<(PathBuf, &str)> = Vec::new();
    if artix7.is_dir() {
        // xc7a35t uses the xc7a50t fabric (18055 tiles).
        runs.push((artix7.clone(), "xc7a35tcsg324-1"));
        runs.push((artix7, "xc7a200tffg1156-1"));
    }
    if zynqusp.is_dir() {
        runs.push((zynqusp, "xczu3eg-sfvc784-1-e"));
    }
    if runs.is_empty() {
        println!(
            "no real database under {} (tools/fetch-db.sh); using the miniature one",
            cache.display()
        );
        runs.push((
            Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/mini-db"),
            "xc7",
        ));
    }
    let exe = std::env::current_exe().unwrap();
    for (root, part) in runs {
        let status = std::process::Command::new(&exe)
            .arg("--part")
            .arg(&root)
            .arg(part)
            .status()
            .unwrap();
        assert!(status.success(), "{part}: {status}");
        bench_cached(&exe, &root, part);
    }
}

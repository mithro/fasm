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

//! Micro benchmark of `fasm::idstring` (`cargo bench -p fasm --bench idstring`).
//!
//! The feature names of `examples/many.fasm` are repeated over a grid of
//! tile coordinates to get a realistic number of distinct names (tile names
//! are the most numerous component). Alternatively, set
//! `FASM_IDSTRING_BENCH_FILE` to a file with one feature name per line.
//!
//! Reports nanoseconds per operation for interning new names (miss) and
//! known names (hit, from `&str` and from bytes), lookups, resolving and
//! sorting, plus the heap bytes the interner uses per distinct name and per
//! distinct component.

use std::collections::{HashMap, HashSet};
use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use fasm::idstring::{IdString, Interner};

/// Tile grid used to multiply the example features.
const GRID_X: usize = 150;
const GRID_Y: usize = 150;
/// Threads of the concurrent hit benchmark.
const THREADS: usize = 8;

/// Extracts the feature names of a FASM file (first token of each line
/// that is not blank, a comment or an annotation, without `[..]`).
fn fasm_features(text: &str) -> Vec<String> {
    let mut features = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim_start();
        let end = line
            .find(|c: char| c.is_whitespace() || matches!(c, '=' | '{' | '['))
            .unwrap_or(line.len());
        let feature = &line[..end];
        if !feature.is_empty() && !features.iter().any(|f| f == feature) {
            features.push(feature.to_owned());
        }
    }
    features
}

/// Replaces the `_X<n>Y<m>` suffix of the tile name with every coordinate
/// of the grid.
fn multiply(features: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for feature in features {
        let (tile, rest) = feature.split_once('.').unwrap_or((feature, ""));
        let tile_type = tile.rsplit_once("_X").map_or(tile, |(t, _)| t);
        for x in 0..GRID_X {
            for y in 0..GRID_Y {
                names.push(format!("{tile_type}_X{x}Y{y}.{rest}"));
            }
        }
    }
    names
}

fn load_names() -> (String, Vec<String>) {
    if let Ok(path) = std::env::var("FASM_IDSTRING_BENCH_FILE") {
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let unique: HashSet<&str> = text.lines().collect();
        let names = unique.into_iter().map(str::to_owned).collect();
        return (path, names);
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/many.fasm");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let features = fasm_features(&text);
    let source = format!(
        "{} features of examples/many.fasm x {GRID_X}x{GRID_Y} tiles",
        features.len()
    );
    (source, multiply(&features))
}

fn ns_per(elapsed: Duration, operations: usize) -> f64 {
    elapsed.as_secs_f64() * 1e9 / operations as f64
}

/// Runs `f` (which performs `operations` operations) `rounds` times and
/// returns the best time per operation in nanoseconds.
fn best_of(rounds: usize, operations: usize, mut f: impl FnMut()) -> f64 {
    (0..rounds)
        .map(|_| {
            let start = Instant::now();
            f();
            ns_per(start.elapsed(), operations)
        })
        .fold(f64::INFINITY, f64::min)
}

fn main() {
    let (source, names) = load_names();
    let n = names.len();
    let text_bytes: usize = names.iter().map(String::len).sum();
    println!("idstring benchmark: {source}");
    println!(
        "  {n} distinct names, average {:.1} bytes",
        text_bytes as f64 / n as f64
    );

    // Miss: every name is new to a fresh interner.
    let interner = Interner::new();
    let start = Instant::now();
    let ids: Vec<IdString> = names.iter().map(|s| interner.intern(s)).collect();
    let miss = ns_per(start.elapsed(), n);

    // Hit: every name is already known.
    let hit = best_of(5, n, || {
        for s in &names {
            black_box(interner.intern(black_box(s)));
        }
    });
    // Hit from bytes (a parser's input buffer): no UTF-8 validation.
    let hit_bytes = best_of(5, n, || {
        for s in &names {
            black_box(interner.intern_bytes(black_box(s.as_bytes())).ok());
        }
    });
    // The same with a separate validation pass, for comparison.
    let hit_validated = best_of(5, n, || {
        for s in &names {
            let s = std::str::from_utf8(black_box(s.as_bytes())).ok();
            black_box(s.map(|s| interner.intern(s)));
        }
    });
    let lookup = best_of(5, n, || {
        for s in &names {
            black_box(interner.lookup(black_box(s)));
        }
    });
    let with_str = best_of(5, n, || {
        for &id in &ids {
            black_box(interner.with_str(id, str::len));
        }
    });
    let resolve = best_of(5, n, || {
        for &id in &ids {
            black_box(interner.resolve(id));
        }
    });
    let mut sorted = ids.clone();
    let sort_ids = best_of(3, n, || {
        sorted.clone_from(&ids);
        sorted.sort_unstable_by(|&a, &b| interner.cmp(a, b));
    });
    let mut sorted_strings = names.clone();
    let sort_strings = best_of(3, n, || {
        sorted_strings.clone_from(&names);
        sorted_strings.sort_unstable();
    });
    let equal = sorted
        .iter()
        .zip(&sorted_strings)
        .all(|(&id, s)| interner.resolved(id) == s.as_str());
    assert!(equal, "IdString order differs from str order");

    // Baseline: a whole string `HashMap<String, u32>` (std hasher), the
    // obvious alternative to per-level interning.
    let map: HashMap<String, u32> = names.iter().cloned().zip(0..).collect();
    let map_hit = best_of(5, n, || {
        for s in &names {
            black_box(map.get(black_box(s.as_str())));
        }
    });

    // Concurrent hits: THREADS threads intern all names.
    let start = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..THREADS {
            scope.spawn(|| {
                for s in &names {
                    black_box(interner.intern(black_box(s)));
                }
            });
        }
    });
    let concurrent = ns_per(start.elapsed(), n * THREADS);

    // Memory.
    let stats = interner.stats();
    let components: usize = stats.level_entries.iter().sum::<usize>() + stats.overflow_entries;
    let string_heap = names.iter().map(|s| s.capacity() + 24).sum::<usize>();

    println!(
        "  level entries {:?}, overflow entries {}",
        stats.level_entries, stats.overflow_entries
    );
    println!("  intern (miss)            {miss:8.1} ns/op");
    println!("  intern (hit)             {hit:8.1} ns/op");
    println!("  intern (hit, {THREADS} threads)  {concurrent:8.1} ns/op (wall clock / total ops)");
    println!("  intern_bytes (hit)       {hit_bytes:8.1} ns/op");
    println!("  from_utf8 + intern (hit) {hit_validated:8.1} ns/op");
    println!("  lookup (hit)             {lookup:8.1} ns/op");
    println!("  with_str                 {with_str:8.1} ns/op");
    println!("  resolve (to String)      {resolve:8.1} ns/op");
    println!("  sort IdString            {sort_ids:8.1} ns/element");
    println!("  sort String              {sort_strings:8.1} ns/element");
    println!("  HashMap<String,u32> get  {map_hit:8.1} ns/op (baseline)");
    println!(
        "  interner heap            {:8} bytes = {:.1} bytes/distinct name, {:.1} bytes/distinct table entry",
        stats.heap_bytes,
        stats.heap_bytes as f64 / n as f64,
        stats.heap_bytes as f64 / components as f64,
    );
    println!(
        "  per name: IdString 8 bytes vs String {:.1} bytes (24 + heap text, before allocator overhead)",
        string_heap as f64 / n as f64
    );
}

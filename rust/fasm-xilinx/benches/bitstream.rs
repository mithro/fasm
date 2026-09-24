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

//! Bitstream benchmark (`cargo bench -p fasm-xilinx --bench bitstream`).
//!
//! Writes and reads a Series7 bitstream with every frame of a part of the
//! prjxray-db `artix7` database (from `$FASM_DB_CACHE` or
//! `tests/oracle/build/db`; default part `xc7a200tffg1156-1`, 24060
//! frames, `FASM_BENCH_PART` selects another), once with zero frames and
//! once with random frames (30% of the words set), and reports the time of
//! each phase (best of 5): `.frm` parsing, the ECC, [`bitstream_bytes`],
//! the reader and `.frm` writing. Skipped when the database is missing.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fasm_xilinx::bitstream::{bitstream_bytes, ecc, BitstreamOptions, BitstreamReader};
use fasm_xilinx::{Architecture, Frames, Part};

fn best<T>(mut f: impl FnMut() -> T) -> (Duration, T) {
    let mut best = Duration::MAX;
    let mut out = None;
    for _ in 0..5 {
        let start = Instant::now();
        let value = f();
        best = best.min(start.elapsed());
        out = Some(value);
    }
    (best, out.expect("ran"))
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn main() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("FASM_DB_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("tests/oracle/build/db"));
    let part_name =
        std::env::var("FASM_BENCH_PART").unwrap_or_else(|_| "xc7a200tffg1156-1".to_owned());
    let path = cache
        .join("prjxray-db/artix7")
        .join(&part_name)
        .join("part.yaml");
    if !path.exists() {
        eprintln!("skipping: {} not found", path.display());
        return;
    }
    let part = Part::from_yaml_file(&path, Architecture::Series7).unwrap();
    let options = BitstreamOptions {
        design_name: b"bench.frm".to_vec(),
        part_name: part_name.as_bytes().to_vec(),
        date: Some("2026/01/01".into()),
        time: Some("00:00:00".into()),
        ..Default::default()
    };
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for random in [false, true] {
        let mut frames = Frames::zeroed(101, part.iter_frame_addresses().map(|a| a.0));
        if random {
            let addresses: Vec<u32> = frames.addresses().to_vec();
            for address in addresses {
                for word in frames.get_mut(address).unwrap() {
                    if next() % 10 < 3 {
                        *word = next() as u32;
                    }
                }
            }
        }
        let frm = frames.to_frm_string().into_bytes();
        let label = if random { "random" } else { "zero" };
        println!(
            "{part_name}, {} frames, {label} ({:.1} MiB .frm):",
            frames.len(),
            frm.len() as f64 / 1048576.0
        );
        let (t, read) = best(|| Frames::read_frm(&frm, 101, &mut |_| {}).unwrap());
        println!("  read_frm             {:8.2} ms", ms(t));
        let (t, _) = best(|| {
            let mut copy = read.clone();
            let addresses: Vec<u32> = copy.addresses().to_vec();
            for address in addresses {
                ecc::update_ecc(copy.get_mut(address).unwrap());
            }
            copy
        });
        println!("  ECC (incl. a copy)   {:8.2} ms", ms(t));
        let (t, bit) = best(|| bitstream_bytes(&part, &read, &options).unwrap());
        println!(
            "  bitstream_bytes      {:8.2} ms ({:.1} MiB)",
            ms(t),
            bit.len() as f64 / 1048576.0
        );
        let (t, n) = best(|| {
            let reader = BitstreamReader::from_bytes(&bit).unwrap();
            let config = reader.configuration(&part).unwrap();
            config.to_frames(true, false).len()
        });
        println!("  read + to_frames     {:8.2} ms ({n} frames)", ms(t));
        let (t, _) = best(|| read.to_frm_string().len());
        println!("  write_frm            {:8.2} ms", ms(t));
    }
}

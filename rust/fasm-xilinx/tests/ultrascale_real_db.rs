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

//! UltraScale / UltraScale+ bitstream tests on real data (T6.2):
//!
//! * every part of prjuray-db `zynqusp` (`tools/fetch-db.sh prjuray
//!   zynqusp`, `FASM_DB_CACHE`): a FASM file of one feature in every
//!   tile that has segbits is assembled with prjuray's semantics, written
//!   as an UltraScale+ bitstream and read back (frames, ECC); when the
//!   reference prjuray-tools `xcframes2bit` is built
//!   (`tests/oracle/setup-xilinx.sh`, found like the prjxray tools of
//!   `bitstream_real_db.rs`), its bitstream of the same frames is
//!   identical;
//! * the Vivado bitstreams of prjuray-tools' `ToolsTestData.tar.gz`
//!   (UltraScale and UltraScale+ `design.bit` with their `part.yaml`, from
//!   the oracle's prjuray-tools checkout, extracted with `tar` into a
//!   temporary directory): every frame's ECC verifies, and bit -> frames
//!   -> bit -> frames is the identity (the reference `xcframes2bit` makes
//!   the same `.bit` when it is built).
//!
//! Tests whose inputs are missing pass with a message.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use fasm_xilinx::bitstream::{bitstream_bytes, BitHeader, BitstreamOptions, BitstreamReader, Ecc};
use fasm_xilinx::{uray_fasm2frames, Architecture, Database, Fasm2FramesOptions, Frames, Part};

use common::{real_db, repo_root};

/// The part directories (with a `part.yaml`) of a prjuray-db family.
fn parts(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(root)
        .unwrap()
        .filter_map(|e| {
            let e = e.ok()?;
            e.path()
                .join("part.yaml")
                .is_file()
                .then(|| e.file_name().to_string_lossy().into_owned())
        })
        .collect();
    out.sort();
    out
}

/// The directory of the reference binaries (`uray-xcframes2bit`), if built.
fn oracle_bin() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os("PRJXRAY_BIN") {
        candidates.push(PathBuf::from(dir));
    }
    candidates.push(repo_root().join("tests/oracle/build/xilinx/bin"));
    if let Some(cache) = std::env::var_os("FASM_DB_CACHE") {
        candidates.push(PathBuf::from(cache).join("../xilinx/bin"));
    }
    let found = candidates
        .into_iter()
        .find(|dir| dir.join("uray-xcframes2bit").is_file());
    if found.is_none() {
        eprintln!("skipping the reference comparison: uray-xcframes2bit not found");
    }
    found
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fasm-ultrascale-real-{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs the reference `xcframes2bit` on `frames`; returns its `.bit`.
fn reference_bit(
    bin: &Path,
    arch: Architecture,
    part_yaml: &Path,
    frames: &Frames,
    dir: &Path,
) -> Vec<u8> {
    let frm = dir.join("in.frm");
    let bit = dir.join("out.bit");
    std::fs::write(&frm, frames.to_frm_string()).unwrap();
    let status = Command::new(bin.join("uray-xcframes2bit"))
        .arg(format!("--architecture={}", arch.name()))
        .arg(format!("--part_file={}", part_yaml.display()))
        .arg("--part_name=ref")
        .arg(format!("--frm_file={}", frm.display()))
        .arg(format!("--output_file={}", bit.display()))
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read(bit).unwrap()
}

/// The `BitstreamOptions` that reproduce the header of `bit`.
fn options_of(bit: &[u8]) -> BitstreamOptions {
    let header = BitHeader::parse(bit).expect("a .bit header");
    let marker = b";Generator=";
    let at = header
        .design
        .windows(marker.len())
        .position(|w| w == marker)
        .expect("a generator");
    let (date, time) = header.date_time();
    BitstreamOptions {
        design_name: header.design[..at].to_vec(),
        generator: header.design[at + marker.len()..].to_vec(),
        part_name: header.part.clone(),
        date: Some(date),
        time: Some(time),
    }
}

fn assert_same_bytes(actual: &[u8], expected: &[u8], what: &str) {
    if actual != expected {
        let first = actual
            .iter()
            .zip(expected)
            .position(|(a, b)| a != b)
            .unwrap_or(actual.len().min(expected.len()));
        panic!(
            "{what}: {} bytes, expected {}; first difference at byte {first}",
            actual.len(),
            expected.len()
        );
    }
}

/// One feature of every `step`-th tile with segbits (the entry picked by
/// a hash of the tile index), as FASM text.
fn one_feature_per_tile(db: &Database, step: usize) -> String {
    let grid = db.grid().unwrap();
    let mut text = String::new();
    for (i, tile) in grid.tiles().iter().enumerate().step_by(step) {
        let Some(tile_type) = db.tile_type_of(tile) else {
            continue;
        };
        let entries = tile_type.segbits.entries();
        if entries.is_empty() {
            continue;
        }
        let pick = (i.wrapping_mul(2_654_435_761)) % entries.len();
        let feature = entries[pick].feature.to_string();
        // Some segbits tags are not FASM identifiers (`OUTPUTS_ENABLED.0`).
        let valid = feature.split('.').all(|part| {
            part.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_[]".contains(c))
        });
        if valid {
            text.push_str(&format!("{}.{}\n", tile.name, feature));
        }
    }
    text
}

#[test]
fn every_zynqusp_part_round_trips() {
    let Some(root) = real_db("prjuray-db", "zynqusp") else {
        return;
    };
    let oracle = oracle_bin();
    let all = parts(&root);
    assert!(!all.is_empty());
    for name in all {
        let db = Database::open(&root, Some(&name)).unwrap();
        assert_eq!(db.architecture(), Architecture::UltraScalePlus);
        let part: Part = db.part().unwrap().clone();
        let walk = part.iter_frame_addresses().count();
        assert_eq!(walk, part.frame_count(), "{name}: the part walk");
        let dir = scratch(&name);
        let fasm = dir.join("design.fasm");
        let text = one_feature_per_tile(&db, 7);
        std::fs::write(&fasm, &text).unwrap();
        let frames = uray_fasm2frames(
            &db,
            &fasm,
            &Fasm2FramesOptions {
                sparse: true,
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(frames.set_bits().count() > 100, "{name}");
        let options = BitstreamOptions {
            design_name: b"design.frm".to_vec(),
            part_name: name.as_bytes().to_vec(),
            date: Some("2026/09/25".into()),
            time: Some("12:00:00".into()),
            ..Default::default()
        };
        let bytes = bitstream_bytes(&part, &frames, &options).unwrap();
        let reader = BitstreamReader::from_bytes(&bytes).unwrap();
        let config = reader.configuration(&part).unwrap();
        assert_eq!(config.len(), part.frame_count(), "{name}");
        for (address, words) in config.frames() {
            assert_eq!(
                Ecc::UltraScalePlus.verify(words),
                Some(true),
                "{name} {address:#010x}"
            );
        }
        let back = config.to_frames(true, true);
        let mut nonzero = Frames::new(93);
        for (address, words) in frames.iter() {
            if words.iter().any(|&w| w != 0) {
                nonzero.insert_if_absent(address, words);
            }
        }
        assert!(back.diff(&nonzero).is_empty(), "{name}");
        if let Some(bin) = &oracle {
            let part_yaml = root.join(&name).join("part.yaml");
            let reference =
                reference_bit(bin, Architecture::UltraScalePlus, &part_yaml, &frames, &dir);
            let ours = bitstream_bytes(&part, &frames, &options_of(&reference)).unwrap();
            assert_same_bytes(&ours, &reference, &name);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// The oracle's prjuray-tools `ToolsTestData.tar.gz`, extracted (only the
/// UltraScale and UltraScale+ `part.yaml` and `design.bit`), or `None`.
fn tools_test_data() -> Option<PathBuf> {
    let mut candidates = vec![repo_root().join("tests/oracle/build/xilinx/src/prjuray-tools")];
    if let Some(cache) = std::env::var_os("FASM_DB_CACHE") {
        candidates.push(PathBuf::from(cache).join("../xilinx/src/prjuray-tools"));
    }
    let tarball = candidates
        .into_iter()
        .map(|d| d.join("lib/test_data/ToolsTestData.tar.gz"))
        .find(|p| p.is_file());
    let Some(tarball) = tarball else {
        eprintln!("skipping: prjuray-tools ToolsTestData.tar.gz not found");
        return None;
    };
    let dir = scratch("ttd");
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&tarball)
        .arg("-C")
        .arg(&dir)
        .args([
            "UltraScale/part.yaml",
            "UltraScale/design.bit",
            "UltraScalePlus/part.yaml",
            "UltraScalePlus/design.bit",
        ])
        .stdin(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Some(dir),
        _ => {
            eprintln!("skipping: cannot extract {}", tarball.display());
            None
        }
    }
}

#[test]
fn vivado_bitstreams_round_trip() {
    let Some(dir) = tools_test_data() else {
        return;
    };
    let oracle = oracle_bin();
    for arch in [Architecture::UltraScale, Architecture::UltraScalePlus] {
        let sub = dir.join(arch.name());
        let part_yaml = sub.join("part.yaml");
        let part = Part::from_yaml_file(&part_yaml, arch).unwrap();
        assert_eq!(part.architecture, arch);
        let bit = std::fs::read(sub.join("design.bit")).unwrap();
        let reader = BitstreamReader::from_bytes(&bit).unwrap();
        let config = reader.configuration(&part).unwrap();
        assert!(config.len() > 10_000, "{arch}");
        let ecc = Ecc::of(arch);
        let mut nonzero = 0;
        for (address, words) in config.frames() {
            assert_eq!(ecc.verify(words), Some(true), "{arch} {address:#010x}");
            nonzero += usize::from(words.iter().any(|&w| w != 0));
        }
        assert!(nonzero > 100, "{arch}");
        let frames = config.to_frames(false, false);
        let options = BitstreamOptions {
            design_name: b"design.frm".to_vec(),
            part_name: b"part".to_vec(),
            date: Some("2020/06/15".into()),
            time: Some("20:47:00".into()),
            ..Default::default()
        };
        let ours = bitstream_bytes(&part, &frames, &options).unwrap();
        let again = BitstreamReader::from_bytes(&ours).unwrap();
        let config_again = again.configuration(&part).unwrap();
        assert!(
            config_again
                .to_frames(false, false)
                .diff(&frames)
                .is_empty(),
            "{arch}"
        );
        if let Some(bin) = &oracle {
            let reference = reference_bit(bin, arch, &part_yaml, &frames, &sub);
            let ours = bitstream_bytes(&part, &frames, &options_of(&reference)).unwrap();
            assert_same_bytes(&ours, &reference, arch.name());
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

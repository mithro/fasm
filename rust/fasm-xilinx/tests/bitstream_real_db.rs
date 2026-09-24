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

//! Bitstream writer and reader tests on real data:
//!
//! * the golden `tests/corpus/xilinx/artix7/smoke_x1y0.bit` (written by
//!   prjxray's `xc7frames2bit`) is reproduced byte for byte from
//!   `smoke_x1y0.frm` with the header fields of the golden file, and
//!   reading it back gives `smoke_x1y0.bitread.txt` (`bitread -z -y`);
//! * the counter design's dense and sparse `.frm` give the same
//!   bitstream, which reads back to the dense frames; when the reference
//!   `xc7frames2bit` is available (`tests/oracle/setup-xilinx.sh`, found
//!   through `$PRJXRAY_BIN`, `tests/oracle/build/xilinx/bin` or
//!   `$FASM_DB_CACHE/../xilinx/bin`), its output is identical;
//! * prjxray's own test bitstreams (`lib/test_data/configuration_test*.bit`,
//!   2 MB each, read from the oracle's prjxray checkout; not copied) give
//!   identical configurations, as in prjxray's `configuration_test.cc`.
//!
//! Tests that need the part database (`tools/fetch-db.sh prjxray
//! artix7`, or `FASM_DB_CACHE`), `xz` or the oracle build are skipped
//! (they pass with a message) when it is missing.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use fasm_xilinx::bitstream::{bitstream_bytes, ecc, BitHeader, BitstreamOptions, BitstreamReader};
use fasm_xilinx::{Architecture, FrameAddress, Frames, Part};

use common::{real_db, repo_root};

const PART: &str = "xc7a35tcsg324-1";

fn corpus(name: &str) -> PathBuf {
    repo_root().join("tests/corpus/xilinx/artix7").join(name)
}

fn part_yaml(root: &Path) -> PathBuf {
    root.join(PART).join("part.yaml")
}

fn load_part(root: &Path) -> Part {
    Part::from_yaml_file(&part_yaml(root), Architecture::Series7).unwrap()
}

/// `xz -dc`, or `None` without `xz`.
fn unxz(path: &Path) -> Option<Vec<u8>> {
    match Command::new("xz").arg("-dc").arg(path).output() {
        Ok(output) => {
            assert!(output.status.success(), "xz -dc {}", path.display());
            Some(output.stdout)
        }
        Err(e) => {
            eprintln!("skipping: cannot run xz: {e}");
            None
        }
    }
}

fn read_frm(data: &[u8]) -> Frames {
    Frames::read_frm(data, 101, &mut |w| panic!("warning: {w}")).unwrap()
}

/// The directory of the reference prjxray binaries, if built.
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
        .find(|dir| dir.join("xc7frames2bit").is_file());
    if found.is_none() {
        eprintln!("skipping: reference xc7frames2bit not found (tests/oracle/setup-xilinx.sh)");
    }
    found
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

/// `bitread -z -y` output: `bit_%08x_%03d_%02d` for every set bit of
/// every non zero frame, without the ECC bits.
fn bitread_zy(bit: &[u8], part: &Part) -> String {
    let reader = BitstreamReader::from_bytes(bit).unwrap();
    let config = reader.configuration(part).unwrap();
    let mut out = String::new();
    for (address, words) in config.frames() {
        if words.len() == 101 && words.iter().all(|&w| w == 0) {
            continue;
        }
        for (i, &word) in words.iter().enumerate() {
            for k in 0..32 {
                if (i != 50 || k > 12) && word & (1 << k) != 0 {
                    out.push_str(&format!("bit_{address:08x}_{i:03}_{k:02}\n"));
                }
            }
        }
    }
    out
}

#[test]
fn smoke_golden_bit_is_reproduced() {
    let Some(root) = real_db("prjxray-db", "artix7") else {
        return;
    };
    let part = load_part(&root);
    let golden = std::fs::read(corpus("smoke_x1y0.bit")).unwrap();
    let frames = read_frm(&std::fs::read(corpus("smoke_x1y0.frm")).unwrap());
    let options = options_of(&golden);
    assert_eq!(options.design_name, b"/tmp/smoke_x1y0.frm");
    assert_eq!(options.part_name, PART.as_bytes());
    let bytes = bitstream_bytes(&part, &frames, &options).unwrap();
    assert_same_bytes(&bytes, &golden, "smoke_x1y0.bit");
}

#[test]
fn smoke_golden_bitread() {
    let Some(root) = real_db("prjxray-db", "artix7") else {
        return;
    };
    let part = load_part(&root);
    let golden = std::fs::read(corpus("smoke_x1y0.bit")).unwrap();
    let expected = std::fs::read_to_string(corpus("smoke_x1y0.bitread.txt")).unwrap();
    assert_eq!(bitread_zy(&golden, &part), expected);
    let reader = BitstreamReader::from_bytes(&golden).unwrap();
    assert_eq!(reader.words().len(), 547_990);
    let config = reader.configuration(&part).unwrap();
    assert_eq!(config.len(), 5408);
    assert_eq!(config.len(), part.frame_count());
    // bit -> frames -> bit.
    let again = bitstream_bytes(&part, &config.to_frames(false, false), &options_of(&golden));
    assert_same_bytes(&again.unwrap(), &golden, "smoke_x1y0.bit rewritten");
    // bit -> frames gives the .frm's frames back (ECC bits cleared).
    let frames = read_frm(&std::fs::read(corpus("smoke_x1y0.frm")).unwrap());
    let back = config.to_frames(true, false);
    for (address, words) in frames.iter() {
        assert_eq!(back.get(address), Some(words), "frame 0x{address:08X}");
    }
}

fn fixed_options() -> BitstreamOptions {
    BitstreamOptions {
        design_name: b"top.frm".to_vec(),
        part_name: PART.as_bytes().to_vec(),
        date: Some("2026/01/02".into()),
        time: Some("03:04:05".into()),
        ..Default::default()
    }
}

#[test]
fn counter_dense_and_sparse_round_trip() {
    let Some(root) = real_db("prjxray-db", "artix7") else {
        return;
    };
    let dir = corpus("designs/f4pga-examples/counter_test/arty_35");
    let (Some(dense), Some(sparse)) = (
        unxz(&dir.join("top.frm.xz")),
        unxz(&dir.join("top.sparse.frm.xz")),
    ) else {
        return;
    };
    let part = load_part(&root);
    let dense = read_frm(&dense);
    let sparse = read_frm(&sparse);
    assert!(sparse.len() < dense.len());
    let from_dense = bitstream_bytes(&part, &dense, &fixed_options()).unwrap();
    let from_sparse = bitstream_bytes(&part, &sparse, &fixed_options()).unwrap();
    assert_same_bytes(&from_sparse, &from_dense, "sparse vs dense");
    let reader = BitstreamReader::from_bytes(&from_dense).unwrap();
    let config = reader.configuration(&part).unwrap();
    // Every frame of the dense .frm is in the part; reading back (ECC
    // cleared) gives the dense frames, and the other frames are zero.
    let back = config.to_frames(true, false);
    for (address, words) in dense.iter() {
        assert_eq!(back.get(address), Some(words), "frame 0x{address:08X}");
    }
    for (address, words) in back.iter() {
        if !dense.contains(address) {
            assert!(words.iter().all(|&w| w == 0), "frame 0x{address:08X}");
        }
    }
    // The ECC word of every frame is the reference's.
    for (address, words) in config.frames() {
        let mut expected = words.to_vec();
        ecc::update_ecc(&mut expected);
        assert_eq!(words, &expected[..], "ECC of 0x{address:08X}");
    }
}

#[test]
fn counter_matches_reference_xc7frames2bit() {
    let Some(root) = real_db("prjxray-db", "artix7") else {
        return;
    };
    let Some(bin) = oracle_bin() else {
        return;
    };
    let dir = corpus("designs/f4pga-examples/counter_test/arty_35");
    let part = load_part(&root);
    let tmp = std::env::temp_dir().join(format!("fasm-xilinx-bitstream-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    for name in ["top.frm.xz", "top.sparse.frm.xz", "top.pudc.frm.xz"] {
        let Some(frm) = unxz(&dir.join(name)) else {
            return;
        };
        let frm_path = tmp.join(name.trim_end_matches(".xz"));
        let bit_path = tmp.join("oracle.bit");
        std::fs::write(&frm_path, &frm).unwrap();
        let status = Command::new(bin.join("xc7frames2bit"))
            .arg(format!("--frm_file={}", frm_path.display()))
            .arg(format!("--output_file={}", bit_path.display()))
            .arg(format!("--part_name={PART}"))
            .arg(format!("--part_file={}", part_yaml(&root).display()))
            .status()
            .unwrap();
        assert!(status.success());
        let oracle = std::fs::read(&bit_path).unwrap();
        let options = options_of(&oracle);
        assert_eq!(options.design_name, frm_path.to_str().unwrap().as_bytes());
        let ours = bitstream_bytes(&part, &read_frm(&frm), &options).unwrap();
        assert_same_bytes(&ours, &oracle, name);
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The frame addresses of a `configuration_ranges` `part.yaml` (the form
/// of prjxray's `lib/test_data/configuration_test.yaml`, which the
/// database loader does not support): every address in `[begin, end)`
/// of each range, like `YAML::convert<Part>::decode`.
fn configuration_ranges_part(text: &str) -> Part {
    let mut idcode = 0;
    let mut addresses = Vec::new();
    let mut fields = [0u32; 5];
    let mut current: Vec<u32> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let set = |i: usize, v: u32, fields: &mut [u32; 5]| fields[i] = v;
        match key.trim_start_matches("- ") {
            "idcode" => idcode = u32::from_str_radix(value.trim_start_matches("0x"), 16).unwrap(),
            "block_type" => set(
                0,
                ["CLB_IO_CLK", "BLOCK_RAM", "CFG_CLB"]
                    .iter()
                    .position(|b| *b == value)
                    .unwrap() as u32,
                &mut fields,
            ),
            "row_half" => set(1, u32::from(value == "bottom"), &mut fields),
            "row" => set(2, value.parse().unwrap(), &mut fields),
            "column" => set(3, value.parse().unwrap(), &mut fields),
            "minor" => {
                set(4, value.parse().unwrap(), &mut fields);
                let [bt, half, row, column, minor] = fields;
                current.push((bt << 23) | (half << 22) | (row << 17) | (column << 7) | minor);
                if current.len() == 2 {
                    addresses.extend((current[0]..current[1]).map(FrameAddress));
                    current.clear();
                }
            }
            _ => {}
        }
    }
    Part::from_frame_addresses(Architecture::Series7, idcode, addresses).unwrap()
}

/// prjxray's `lib/test_data`, in the oracle's checkout.
fn prjxray_test_data() -> Option<PathBuf> {
    let mut candidates =
        vec![repo_root().join("tests/oracle/build/xilinx/src/prjxray/lib/test_data")];
    if let Some(cache) = std::env::var_os("FASM_DB_CACHE") {
        candidates.push(PathBuf::from(cache).join("../xilinx/src/prjxray/lib/test_data"));
    }
    let found = candidates
        .into_iter()
        .find(|d| d.join("configuration_test.bit").is_file());
    if found.is_none() {
        eprintln!("skipping: prjxray lib/test_data not found (tests/oracle/setup-xilinx.sh)");
    }
    found
}

/// `ConfigurationTest.DebugAndPerFrameCrcBitstreamsProduceEqualConfigurations`
/// and `DebugAndNormalBitstreamsProduceEqualConfigurations`: Vivado's
/// normal, debug (`DEBUGBITSTREAM`: one FAR write and Type0 padding per
/// frame) and per frame CRC bitstreams of one design read to the same
/// frames.
#[test]
fn prjxray_test_bitstreams_give_equal_configurations() {
    let Some(dir) = prjxray_test_data() else {
        return;
    };
    let part = configuration_ranges_part(
        &std::fs::read_to_string(dir.join("configuration_test.yaml")).unwrap(),
    );
    let read = |name: &str| std::fs::read(dir.join(name)).unwrap();
    let (normal, debug, perframecrc) = (
        read("configuration_test.bit"),
        read("configuration_test.debug.bit"),
        read("configuration_test.perframecrc.bit"),
    );
    let readers: Vec<BitstreamReader> = [&normal, &debug, &perframecrc]
        .iter()
        .map(|b| BitstreamReader::from_bytes(b).unwrap())
        .collect();
    let configs: Vec<_> = readers
        .iter()
        .map(|r| r.configuration(&part).unwrap())
        .collect();
    assert!(!configs[0].is_empty());
    assert_eq!(configs[0], configs[1]);
    assert_eq!(configs[0], configs[2]);
    // Rewriting the frames reads back to the same configuration.
    let frames = configs[0].to_frames(false, false);
    let bytes = bitstream_bytes(&part, &frames, &fixed_options()).unwrap();
    let reader = BitstreamReader::from_bytes(&bytes).unwrap();
    let again = reader.configuration(&part).unwrap();
    for (address, words) in configs[0].frames() {
        assert_eq!(again.get(address), Some(words), "frame 0x{address:08X}");
    }
}

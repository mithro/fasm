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

//! Tests on the real prjxray-db `artix7` and prjuray-db `zynqusp`
//! databases. Each test is skipped (passes with a message) when the
//! database has not been fetched (`tools/fetch-db.sh prjxray artix7`,
//! `tools/fetch-db.sh prjuray zynqusp`; set `FASM_DB_CACHE` to use a
//! cache outside this checkout).

mod common;

use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use fasm::idstring::IdString;
use fasm_xilinx::{
    Architecture, BitPositionError, BlockType, Database, EccReport, FeatureLookup, FrameAddress,
    Layout, Part,
};

use common::{positions, real_db};

/// `(count, last, sum)` of the frame addresses of a part.
fn frame_summary(part: &Part) -> (usize, FrameAddress, u64) {
    let frames: Vec<FrameAddress> = part.iter_frame_addresses().collect();
    assert!(frames.windows(2).all(|w| w[0] < w[1]));
    assert!(frames[1..].iter().all(|&f| part.is_valid_frame_address(f)));
    let sum = frames.iter().map(|f| u64::from(f.0)).sum();
    (frames.len(), *frames.last().unwrap(), sum)
}

/// Segbits that cannot be placed only come from alias tiles whose bits
/// region is shorter than the aliased type's (e.g. the top `_SING` IOB
/// tiles, offset 99 of 101 words, for the other site's bits); prjxray
/// drops such writes with a warning (`word_addr >= 101`).
fn check_unplaceable(report: &EccReport) {
    for finding in &report.unplaceable {
        assert_ne!(finding.tile_type, finding.segbits_tile_type, "{finding:?}");
        assert!(
            matches!(finding.result, Err(BitPositionError::WordOutOfFrame { .. })),
            "{finding:?}"
        );
    }
    let tiles: HashSet<IdString> = report.unplaceable.iter().map(|f| f.tile).collect();
    eprintln!(
        "{} unplaceable alias bits on {} tile(s): {:?}",
        report.unplaceable.len(),
        tiles.len(),
        tiles
    );
}

fn open(root: &Path, part: &str) -> Database {
    let start = Instant::now();
    let db = Database::open(root, Some(part)).unwrap();
    eprintln!("opened {part} in {:?}", start.elapsed());
    db
}

/// Every segbits entry of every (non aliased) tile type of the grid is
/// found by `lookup_feature` on a tile of that type, with prjxray's
/// address rules.
fn all_entries_resolve(db: &Database) -> usize {
    let grid = db.grid().unwrap();
    let mut done = HashSet::new();
    let mut count = 0;
    for tile in grid.tiles() {
        if grid.bits(tile).iter().any(|b| b.has_alias()) || !done.insert(tile.tile_type) {
            continue;
        }
        let tile_type = db.tile_type_of(tile).unwrap();
        for entry in tile_type.segbits.entries() {
            // Skip entries of a bus the tile has no bits block for.
            if grid.bits_block(tile, entry.block_type).is_none() {
                continue;
            }
            let name = entry.feature.to_string();
            let (feature, address) = match name.rfind('[') {
                Some(open) => (
                    IdString::new(&name[..open]),
                    name[open + 1..name.len() - 1].parse().unwrap(),
                ),
                None => (entry.feature, 0),
            };
            match db.lookup_feature(tile.name, feature, address) {
                Ok(FeatureLookup::Bits(bits)) => {
                    // Same bits unless an exact CLB_IO_CLK name shadows a
                    // BLOCK_RAM one (or a pseudo PIP shadows an entry).
                    if bits.entry.feature == entry.feature {
                        assert_eq!(bits.bits, tile_type.segbits.bits(entry));
                    }
                    assert!(bits.positions().all(|(_, p)| p.is_ok()), "{name}");
                }
                Ok(FeatureLookup::PseudoPip(_)) => {}
                Err(e) => panic!("{}: {name}: {e}", tile.name),
            }
            count += 1;
        }
    }
    count
}

#[test]
fn artix7_xc7a35t() {
    let Some(root) = real_db("prjxray-db", "artix7") else {
        return;
    };
    let db = open(&root, "xc7a35tcsg324-1");
    assert_eq!(db.layout(), Layout::Prjxray);
    assert_eq!(db.architecture(), Architecture::Series7);
    assert_eq!(db.tile_types().len(), 128);
    let info = db.part_info().unwrap();
    assert_eq!(info.device.as_deref(), Some("xc7a35t"));
    assert_eq!(info.fabric, "xc7a50t");
    assert_eq!(info.idcode, Some(0x362d093));
    assert_eq!(info.iobanks.as_ref().unwrap().len(), 6);
    assert_eq!(info.package_pins.as_ref().unwrap().len(), 212);
    assert!(info.required_features.is_empty());

    let grid = db.grid().unwrap();
    assert_eq!(grid.len(), 18055);
    let types: HashSet<IdString> = grid.tiles().iter().map(|t| t.tile_type).collect();
    assert_eq!(types.len(), 112);
    assert!(grid.tiles().iter().all(|t| t.tile_type_index().is_some()));
    assert_eq!(grid.iter_bits().filter(|(_, b)| b.has_alias()).count(), 22);

    // part.yaml and part.json agree; frame enumeration matches prjxray:
    // `xc7frames2bit` (empty .frm) + `bitread -o` list 5408 frames, the
    // last 0x00C0017F, addresses summing to 16844570088.
    let part = db.part().unwrap();
    assert_eq!(part.idcode, 0x362d093);
    let json = Part::from_json_file(&info.directory.join("part.json"), Architecture::Series7)
        .unwrap()
        .unwrap();
    assert_eq!(&json, part);
    assert_eq!(part.frame_count(), 5408);
    assert_eq!(
        frame_summary(part),
        (5408, FrameAddress(0x00C0_017F), 16_844_570_088)
    );

    // Spot checks against the segbits files.
    // CLBLM_R.SLICEL_X1.A5FF.ZINI 31_05 on CLBLM_R_X11Y100 (0x00020580, offset 0).
    let p = positions(&db, "CLBLM_R_X11Y100.SLICEL_X1.A5FF.ZINI", 0);
    assert_eq!(p.len(), 1);
    assert_eq!(
        (p[0].1.frame.0, p[0].1.word, p[0].1.bit),
        (0x0002_059F, 0, 5)
    );
    // BRAM_L.RAMB18_Y0.INIT_00[004] 00_80 and [255] 01_143 on BRAM_L_X6Y0
    // (BLOCK_RAM 0x00C00000, offset 0).
    let p = positions(&db, "BRAM_L_X6Y0.RAMB18_Y0.INIT_00", 4);
    assert_eq!(
        (p[0].1.frame.0, p[0].1.word, p[0].1.bit),
        (0x00C0_0000, 2, 16)
    );
    let p = positions(&db, "BRAM_L_X6Y0.RAMB18_Y0.INIT_00", 255);
    assert_eq!(
        (p[0].1.frame.0, p[0].1.word, p[0].1.bit),
        (0x00C0_0001, 4, 15)
    );
    // A pseudo PIP of INT_L.
    let int_l = grid
        .tiles()
        .iter()
        .find(|t| t.tile_type == "INT_L")
        .unwrap();
    assert!(matches!(
        db.lookup_feature(int_l.name, IdString::new("BYP_ALT0.VCC_WIRE"), 0),
        Ok(FeatureLookup::PseudoPip(_))
    ));
    // A `!` bit.
    let clbll = db.tile_type(IdString::new("CLBLL_L")).unwrap();
    let afmux = clbll
        .segbits
        .get(IdString::new("SLICEL_X0.AFFMUX.AX"))
        .unwrap();
    assert_eq!(
        clbll
            .segbits
            .bits(afmux)
            .iter()
            .map(|b| b.is_set)
            .collect::<Vec<_>>(),
        [false, true, false, false]
    );
    // LIOB33_SING (alias of LIOB33, start_offset 2) wraps to word 99+.
    let sing = grid
        .tiles()
        .iter()
        .find(|t| t.tile_type == "LIOB33_SING")
        .unwrap();
    let block = &grid.bits(sing)[0];
    assert_eq!(grid.effective_offset(block), -2);

    let bram = db.tile_type(IdString::new("BRAM_L")).unwrap();
    let max_word_bit = bram
        .segbits
        .entries()
        .iter()
        .filter(|e| e.block_type == BlockType::BlockRam)
        .flat_map(|e| bram.segbits.bits(e))
        .map(|b| b.word_bit)
        .max()
        .unwrap();
    assert_eq!(max_word_bit, 319);

    let resolved = all_entries_resolve(&db);
    assert!(resolved > 100_000, "{resolved}");

    let report = db.check_ecc_invariant();
    eprintln!("ECC check: {} bits", report.checked_bits);
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    check_unplaceable(&report);

    let banks = db.banks_tiles_registry().unwrap();
    assert_eq!(
        banks.bank_of_tile(IdString::new("HCLK_IOI3_X1Y78")),
        Some(IdString::new("15"))
    );
}

#[test]
fn artix7_xc7a200t() {
    let Some(root) = real_db("prjxray-db", "artix7") else {
        return;
    };
    let db = open(&root, "xc7a200tffg1156-1");
    let grid = db.grid().unwrap();
    assert_eq!(db.part_info().unwrap().fabric, "xc7a200t");
    assert_eq!(grid.len(), 69165);
    let types: HashSet<IdString> = grid.tiles().iter().map(|t| t.tile_type).collect();
    assert_eq!(types.len(), 117);
    let part = db.part().unwrap();
    assert_eq!(part.idcode, 0x3636093);
    // xc7frames2bit + bitread: 24060 frames, last 0x00C4047F, sum 111517082430.
    assert_eq!(
        frame_summary(part),
        (24060, FrameAddress(0x00C4_047F), 111_517_082_430)
    );
    let report = db.check_ecc_invariant();
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    check_unplaceable(&report);
}

/// Every artix7 `part.yaml` parses and matches its `part.json`.
#[test]
fn artix7_all_part_files() {
    let Some(root) = real_db("prjxray-db", "artix7") else {
        return;
    };
    let mut parts = 0;
    for entry in std::fs::read_dir(&root).unwrap() {
        let dir = entry.unwrap().path();
        let yaml = dir.join("part.yaml");
        if !yaml.is_file() {
            continue;
        }
        let from_yaml = Part::from_yaml_file(&yaml, Architecture::Series7).unwrap();
        let from_json = Part::from_json_file(&dir.join("part.json"), Architecture::Series7)
            .unwrap()
            .unwrap();
        assert_eq!(from_yaml, from_json, "{}", dir.display());
        assert_eq!(
            from_yaml.iter_frame_addresses().count(),
            from_yaml.frame_count(),
            "{}",
            dir.display()
        );
        parts += 1;
    }
    assert!(parts >= 80, "{parts}");
}

#[test]
fn zynqusp() {
    let Some(root) = real_db("prjuray-db", "zynqusp") else {
        return;
    };
    for (part_name, tiles) in [("xczu3eg-sfvc784-1-e", 66385), ("xczu3eg-sbva484-1-e", 0)] {
        let db = open(&root, part_name);
        assert_eq!(db.layout(), Layout::Prjuray);
        assert_eq!(db.architecture(), Architecture::UltraScalePlus);
        assert_eq!(db.tile_types().len(), 158);
        let info = db.part_info().unwrap();
        assert_eq!(info.fabric, part_name);
        assert!(info.iobanks.is_none());
        assert!(db.banks_tiles_registry().is_none());
        let grid = db.grid().unwrap();
        if tiles != 0 {
            assert_eq!(grid.len(), tiles);
        }
        assert!(grid.tiles().iter().all(|t| t.tile_type_index().is_some()));
        let part = db.part().unwrap();
        assert_eq!(part.architecture, Architecture::UltraScalePlus);
        assert_eq!(part.idcode, 0x4a42093);
        let json = Part::from_json_file(
            &info.directory.join("part.json"),
            Architecture::UltraScalePlus,
        )
        .unwrap()
        .unwrap();
        assert_eq!(&json, part);
        let (count, _, _) = frame_summary(part);
        assert_eq!(count, part.frame_count());
        let report = db.check_ecc_invariant();
        eprintln!("{part_name}: ECC check: {} bits", report.checked_bits);
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        check_unplaceable(&report);
        all_entries_resolve(&db);
    }
    let db = open(&root, "xczu3eg-sfvc784-1-e");
    assert_eq!(db.part().unwrap().frame_count(), 14952);
    // CLEM.ABCDFF.CEUSED.V1 14_07 on CLEM_X1Y1 (0x00000300, offset 3
    // 16-bit words): absolute bit 3 * 16 + 7 = 55 -> word 1, bit 23.
    let p = positions(&db, "CLEM_X1Y1.ABCDFF.CEUSED.V1", 0);
    assert_eq!(
        (p[0].1.frame.0, p[0].1.word, p[0].1.bit),
        (0x0000_030E, 1, 23)
    );
    // BRAM.RAMB18E2_L.INIT_00[2] 00_24 on BRAM_X2Y0 (BLOCK_RAM 0x01000000).
    let p = positions(&db, "BRAM_X2Y0.RAMB18E2_L.INIT_00", 2);
    assert_eq!(
        (p[0].1.frame.0, p[0].1.word, p[0].1.bit),
        (0x0100_0000, 0, 24)
    );
    assert_eq!(
        FrameAddress(0x0100_0000).block_type(Architecture::UltraScalePlus),
        Some(BlockType::BlockRam)
    );
}

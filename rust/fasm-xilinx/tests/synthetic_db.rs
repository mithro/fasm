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

//! Loader tests on the hand written `testdata/synthetic-db`: part.yaml,
//! block RAM segbits, pseudo PIPs, aliases, required features, the ECC
//! check and error reporting.

mod common;

use std::path::{Path, PathBuf};

use fasm::idstring::IdString;
use fasm_xilinx::{
    Architecture, BlockType, Database, DbError, FeatureLookup, FrameAddress, LookupError, Part,
    PpipType,
};

use common::{positions, testdata};

const PART: &str = "xc7test-1";

fn open() -> Database {
    Database::open(&testdata("synthetic-db"), Some(PART)).unwrap()
}

fn lookup<'db>(
    db: &'db Database,
    feature: &str,
    address: u32,
) -> Result<FeatureLookup<'db>, LookupError> {
    db.lookup_fasm_feature(IdString::new(feature), address)
}

/// `(is_set, frame, word, bit)` of a feature.
fn bits(db: &Database, feature: &str, address: u32) -> Vec<(bool, u32, u32, u32)> {
    positions(db, feature, address)
        .into_iter()
        .map(|(set, p)| (set, p.frame.0, p.word, p.bit))
        .collect()
}

#[test]
fn part_data() {
    let db = open();
    assert_eq!(db.architecture(), Architecture::Series7);
    let info = db.part_info().unwrap();
    assert_eq!(info.device.as_deref(), Some("xc7test"));
    assert_eq!(info.fabric, "xc7testfab");
    assert_eq!(info.idcode, Some(0x362d093));
    let part = db.part().unwrap();
    assert_eq!(part.idcode, 0x362d093);
    // part.yaml and part.json describe the same tree.
    let json = Part::from_json_file(&info.directory.join("part.json"), Architecture::Series7)
        .unwrap()
        .unwrap();
    assert_eq!(&json, part);
    assert_eq!(part.frame_count(), 440);
    let frames: Vec<FrameAddress> = part.iter_frame_addresses().collect();
    assert_eq!(frames.len(), 440);
    assert!(frames.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(frames[0], FrameAddress(0));
    // Column 6 of top row 1 follows column 0 directly.
    let arch = Architecture::Series7;
    let col6 = frames
        .iter()
        .position(|f| f.row(arch) == 1 && f.column(arch) == 6)
        .unwrap();
    assert_eq!(frames[col6 - 1].column(arch), 0);
    assert_eq!(frames[col6 - 1].minor(arch), 41);
    assert_eq!(
        frames.last().unwrap().block_type(arch),
        Some(BlockType::BlockRam)
    );

    assert_eq!(
        db.get_required_fasm_features(Some(PART)),
        [
            "INT_L_X6Y0.IMUX_L1.EE2END0",
            "HCLK_L_X6Y26.ENABLE_BUFFER.HCLK_CK_BUFHCLK8"
        ]
    );
    assert!(db.get_required_fasm_features(None).is_empty());
    assert!(db.get_required_fasm_features(Some("xc7other-1")).is_empty());

    let banks = db.banks_tiles_registry().unwrap();
    assert_eq!(
        banks.tiles_of_bank(IdString::new("14")),
        &[
            IdString::new("HCLK_IOI3_X1Y26"),
            IdString::new("LIOB33_SING_X0Y0"),
            IdString::new("LIOB33_X0Y1")
        ]
    );
    let grid = db.grid().unwrap();
    let liob = grid.tile(IdString::new("LIOB33_X0Y1")).unwrap();
    let pudc: Vec<_> = grid
        .pin_functions(liob)
        .iter()
        .filter(|(_, f)| f.to_string().contains("PUDC_B"))
        .collect();
    assert_eq!(pudc.len(), 1);
    let bram5 = grid.tile(IdString::new("BRAM_L_X6Y5")).unwrap();
    assert_eq!(
        grid.prohibited_sites(bram5),
        &[IdString::new("RAMB18_X0Y2")]
    );
    assert_eq!(bram5.clock_region.unwrap().name, "X0Y1");
    assert_eq!(grid.dims(), Some((0, 9, 1, 155)));
}

#[test]
fn tile_types_and_files() {
    let db = open();
    assert_eq!(db.tile_types().len(), 7);
    let bram = db.tile_type(IdString::new("BRAM_L")).unwrap();
    assert!(
        bram.files.segbits && bram.files.block_ram_segbits && bram.files.mask && !bram.files.ppips
    );
    // 4 CLB_IO_CLK + 6 BLOCK_RAM entries; the .origin_info.db (invalid as
    // segbits) and mask files were not read.
    assert_eq!(bram.segbits.len(), 10);
    let int = db.tile_type(IdString::new("INT_L")).unwrap();
    assert_eq!(int.segbits.ppips().len(), 3);
    assert!(db
        .tile_type(IdString::new("NOSEGBITS"))
        .unwrap()
        .segbits
        .is_empty());
    assert!(db.tile_type(IdString::new("MYSTERY")).is_none());
}

#[test]
fn block_ram_segbits() {
    let db = open();
    // BLOCK_RAM bus of BRAM_L_X6Y0: baseaddr 0x00800100, offset 0.
    assert_eq!(
        bits(&db, "BRAM_L_X6Y0.RAMB18_Y0.INIT_00", 0),
        [(true, 0x0080_0100, 0, 0)]
    );
    assert_eq!(
        bits(&db, "BRAM_L_X6Y0.RAMB18_Y0.INIT_00", 1),
        [(true, 0x0080_0100, 0, 16)]
    );
    // word_bit 80 -> word 2, bit 16.
    assert_eq!(
        bits(&db, "BRAM_L_X6Y0.RAMB18_Y0.INIT_00", 4),
        [(true, 0x0080_0100, 2, 16)]
    );
    // 03_319 -> frame +3, word 9, bit 31.
    assert_eq!(
        bits(&db, "BRAM_L_X6Y0.RAMB18_Y0.INIT_00", 255),
        [(true, 0x0080_0103, 9, 31)]
    );
    assert!(matches!(
        lookup(&db, "BRAM_L_X6Y0.RAMB18_Y0.INIT_00", 2),
        Err(LookupError::UnknownFeature { address: 2, .. })
    ));
    // An exact name on both buses: CLB_IO_CLK wins.
    assert_eq!(
        bits(&db, "BRAM_L_X6Y0.BOTH", 0),
        [(true, 0x0002_0300, 0, 5)]
    );
    // An addressed bit on both buses: BLOCK_RAM wins (inserted last).
    assert_eq!(
        bits(&db, "BRAM_L_X6Y0.ZRAMB18_Y0.WIDTH", 1),
        [(true, 0x0080_0102, 68, 28)]
    );
    assert_eq!(
        bits(&db, "BRAM_L_X6Y0.ZRAMB18_Y0.WIDTH", 2),
        [(true, 0x0002_0300 + 25, 0, 4)]
    );
    // `!` bit.
    assert_eq!(
        bits(&db, "BRAM_L_X6Y0.RAMB18_Y0.IN_USE", 0),
        [
            (true, 0x0002_0300 + 27, 3, 4),
            (false, 0x0002_0300 + 27, 3, 5)
        ]
    );
    // The CLB_IO_CLK bus of BRAM_L_X6Y5 has offset 10; it has no BLOCK_RAM
    // bus.
    assert_eq!(
        bits(&db, "BRAM_L_X6Y5.BOTH", 0),
        [(true, 0x0002_0300, 10, 5)]
    );
    assert_eq!(
        lookup(&db, "BRAM_L_X6Y5.RAMB18_Y0.INIT_00", 4).unwrap_err(),
        LookupError::MissingBitsBlock {
            tile: IdString::new("BRAM_L_X6Y5"),
            block_type: BlockType::BlockRam
        }
    );
    let FeatureLookup::Bits(found) = lookup(&db, "BRAM_L_X6Y0.RAMB18_Y0.INIT_00", 4).unwrap()
    else {
        panic!()
    };
    assert_eq!(found.block_type(), BlockType::BlockRam);
    assert_eq!(found.frames(), 0x0080_0100..0x0080_0180);
    assert_eq!(found.entry.feature, "RAMB18_Y0.INIT_00[004]");
}

#[test]
fn pseudo_pips() {
    let db = open();
    let pip = |f: &str| match lookup(&db, f, 0) {
        Ok(FeatureLookup::PseudoPip(t)) => Some(t),
        _ => None,
    };
    assert_eq!(
        pip("INT_L_X6Y0.BYP_BOUNCE0.BYP_ALT0"),
        Some(PpipType::Always)
    );
    assert_eq!(pip("INT_L_X6Y0.GFAN0.GND_WIRE"), Some(PpipType::Hint));
    // A pseudo PIP wins over a segbits entry of the same name.
    assert_eq!(pip("INT_L_X6Y0.BYP_ALT0.VCC_WIRE"), Some(PpipType::Default));
    // ... whatever the address.
    assert!(matches!(
        lookup(&db, "INT_L_X6Y0.BYP_BOUNCE0.BYP_ALT0", 3),
        Ok(FeatureLookup::PseudoPip(PpipType::Always))
    ));
    assert_eq!(bits(&db, "INT_L_X6Y0.WW2BEG0.LOGIC_OUTS_L12", 0).len(), 2);
}

#[test]
fn aliases() {
    let db = open();
    // HCLK_L_BOT_UTURN aliases HCLK_L with start_offset 0.
    assert_eq!(
        bits(
            &db,
            "HCLK_L_BOT_UTURN_X6Y130.ENABLE_BUFFER.HCLK_CK_BUFHCLK8",
            0
        ),
        [(true, 0x0042_0380, 50, 14)]
    );
    // LIOB33_SING: own pseudo PIPs first (unmapped name) ...
    assert!(matches!(
        lookup(&db, "LIOB33_SING_X0Y0.IOB_Y0.PULLTYPE.NONE", 0),
        Ok(FeatureLookup::PseudoPip(PpipType::Always))
    ));
    // ... then the site is renamed IOB_Y0 -> IOB_Y1 and LIOB33's segbits
    // are used at offset 0 - 2: IOB_Y1.PULL 00_65 -> bit 1 of word 0.
    assert_eq!(
        bits(&db, "LIOB33_SING_X0Y0.IOB_Y0.PULL", 0),
        [(true, 0x0040_0000, 0, 1)]
    );
    // The aliased type's pseudo PIPs apply to the renamed feature.
    assert!(matches!(
        lookup(&db, "LIOB33_SING_X0Y0.IOB_Y0.OTHER", 0),
        Ok(FeatureLookup::PseudoPip(PpipType::Default))
    ));
    // IOB_Y1 is not renamed; LIOB33.IOB_Y1.PULL at offset -2.
    assert_eq!(
        bits(&db, "LIOB33_SING_X0Y0.IOB_Y1.PULL", 0),
        [(true, 0x0040_0000, 0, 1)]
    );
    // A tile level feature before the alias start wraps to word 99.
    assert_eq!(
        bits(&db, "LIOB33_SING_X0Y0.LOWBIT", 0),
        [(true, 0x0040_0000, 99, 3)]
    );
    // Not in LIOB33 after renaming.
    assert!(matches!(
        lookup(&db, "LIOB33_SING_X0Y0.IOB_Y0.LOWBIT", 0),
        Err(LookupError::UnknownFeature { .. })
    ));
    // The plain LIOB33 tile (offset 2).
    assert_eq!(
        bits(&db, "LIOB33_X0Y1.IOB_Y0.PULL", 0),
        [(true, 0x0040_0000, 2, 3)]
    );
}

#[test]
fn lookup_errors() {
    let db = open();
    assert_eq!(
        lookup(&db, "MYSTERY_X9Y9.A.B", 0).unwrap_err(),
        LookupError::UnknownTileType {
            tile: IdString::new("MYSTERY_X9Y9"),
            tile_type: IdString::new("MYSTERY")
        }
    );
    assert!(matches!(
        lookup(&db, "NOSEGBITS_X1Y1.A.B", 0),
        Err(LookupError::UnknownFeature { .. })
    ));
    assert!(matches!(
        lookup(&db, "NOPE_X0Y0.A", 0),
        Err(LookupError::UnknownTile { .. })
    ));
    // A feature name never seen by the interner.
    assert!(matches!(
        lookup(&db, "INT_L_X6Y0.NEVER_INTERNED_BEFORE_X.Y.Z", 0),
        Err(LookupError::UnknownFeature { .. })
    ));
}

#[test]
fn ecc_check_finds_the_clash() {
    let db = open();
    let report = db.check_ecc_invariant();
    // HCLK_L.ECC_CLASH 01_12 lands on word 50 bit 12, seen once for the
    // (HCLK_L, CLB_IO_CLK, offset 50) combination shared by HCLK_L_X6Y26
    // and the alias tile HCLK_L_BOT_UTURN_X6Y130.
    assert_eq!(report.violations.len(), 1, "{:?}", report.violations);
    let v = &report.violations[0];
    assert_eq!(v.segbits_tile_type, "HCLK_L");
    assert_eq!(v.feature, "ECC_CLASH");
    let p = v.result.unwrap();
    assert_eq!((p.word, p.bit), (50, 12));
    assert!(report.unplaceable.is_empty(), "{:?}", report.unplaceable);
}

#[test]
fn open_errors() {
    let root = testdata("synthetic-db");
    let err = Database::open(&root, Some("xc7nodev-1")).unwrap_err();
    assert!(
        matches!(&err, DbError::UnknownDevice { device, .. } if device == "xc7nodev"),
        "{err}"
    );
    let err = Database::open(&root, Some("nope")).unwrap_err();
    assert!(matches!(err, DbError::UnknownPart { .. }), "{err}");
}

/// A copy of the synthetic database in a fresh temporary directory.
struct TempDb(PathBuf);

impl TempDb {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("fasm-xilinx-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&testdata("synthetic-db"), &dir);
        TempDb(dir)
    }

    fn write(&self, file: &str, contents: &str) {
        std::fs::write(self.0.join(file), contents).unwrap();
    }

    fn open_err(&self) -> DbError {
        Database::open(&self.0, Some(PART)).unwrap_err()
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn malformed_files_report_file_and_line() {
    let db = TempDb::new("segbits");
    db.write("segbits_int_l.db", "INT_L.A 01_02\nINT_L.B 01_x2\n");
    let err = db.open_err();
    assert!(
        matches!(&err, DbError::Segbits { path, line: 2, .. } if path.ends_with("segbits_int_l.db")),
        "{err}"
    );
    assert!(err.to_string().contains("segbits_int_l.db:2:"), "{err}");

    let db = TempDb::new("ppips");
    db.write("ppips_int_l.db", "INT_L.A always\nINT_L.B maybe\n");
    assert!(matches!(db.open_err(), DbError::Segbits { line: 2, .. }));

    let db = TempDb::new("partyaml");
    db.write("xc7test-1/part.yaml", "!<xilinx/xc7series/part>\nidcode: 0x1\nglobal_clock_regions:\n  top:\n    rows:\n      - 1\n");
    let err = db.open_err();
    assert!(matches!(&err, DbError::Yaml { line: 6, .. }), "{err}");

    let db = TempDb::new("tilegrid");
    db.write(
        "xc7testfab/tilegrid.json",
        "{\n  \"A_X0Y0\": {\"type\": \"A\",\n  \"grid_x\": \"zero\", \"grid_y\": 0}\n}\n",
    );
    let err = db.open_err();
    assert!(matches!(&err, DbError::Json { line: 3, .. }), "{err}");

    let db = TempDb::new("csv");
    db.write(
        "xc7test-1/package_pins.csv",
        "pin,bank,site,tile,pin_function\nA1,14,IOB,T\n",
    );
    assert!(matches!(db.open_err(), DbError::Csv { line: 2, .. }));

    let db = TempDb::new("partjson");
    db.write("xc7test-1/part.json", "{\"iobanks\": [1]}");
    assert!(matches!(db.open_err(), DbError::Json { .. }));

    let db = TempDb::new("mapping");
    db.write("mapping/parts.yaml", "xc7test-1:\n  device: [x]\n");
    assert!(matches!(db.open_err(), DbError::Yaml { line: 2, .. }));

    let db = TempDb::new("missing");
    std::fs::remove_file(db.0.join("xc7testfab/tilegrid.json")).unwrap();
    let err = db.open_err();
    assert!(
        matches!(&err, DbError::MissingFile { path } if path.ends_with("tilegrid.json")),
        "{err}"
    );

    let db = TempDb::new("fabric");
    db.write("mapping/devices.yaml", "xc7test:\n  fabric: ../x\n");
    assert!(matches!(db.open_err(), DbError::Invalid { .. }));

    // Optional part files may be absent.
    let db = TempDb::new("optional");
    for file in [
        "part.yaml",
        "part.json",
        "package_pins.csv",
        "required_features.fasm",
    ] {
        std::fs::remove_file(db.0.join("xc7test-1").join(file)).unwrap();
    }
    let opened = Database::open(&db.0, Some(PART)).unwrap();
    let info = opened.part_info().unwrap();
    assert!(info.part.is_none() && info.iobanks.is_none() && info.package_pins.is_none());
    assert!(opened.banks_tiles_registry().is_none());
    assert!(opened.get_required_fasm_features(Some(PART)).is_empty());
}

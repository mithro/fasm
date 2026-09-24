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

//! Loader tests on the miniature f4pga-xc-fasm database
//! (`testdata/mini-db`), including the golden bits of its FASM tests.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use fasm::idstring::IdString;
use fasm_xilinx::{
    Architecture, BlockType, Database, DbError, FeatureLookup, FrameAddress, Layout, LookupError,
};

use common::{positions, repo_root, testdata};

fn open() -> Database {
    Database::open(&testdata("mini-db"), Some("xc7")).unwrap()
}

#[test]
fn inventory() {
    let db = open();
    assert_eq!(db.layout(), Layout::Prjxray);
    assert_eq!(db.architecture(), Architecture::Series7);
    let names: Vec<String> = db.tile_types().iter().map(|t| t.name.to_string()).collect();
    assert_eq!(
        names,
        [
            "CLBLM_L",
            "HCLK_IOI3",
            "HCLK_L",
            "INT_L",
            "LIOB33",
            "LIOB33_SING",
            "RIOB33",
            "RIOB33_SING"
        ]
    );
    let clblm = db.tile_type(IdString::new("CLBLM_L")).unwrap();
    assert!(
        clblm.files.segbits
            && !clblm.files.block_ram_segbits
            && !clblm.files.ppips
            && !clblm.files.mask
    );
    assert_eq!(clblm.segbits.len(), 96);
    assert_eq!(clblm.segbits.foreign_lines(), 0);
    let sing = db.tile_type(IdString::new("LIOB33_SING")).unwrap();
    assert!(sing.segbits.is_empty());

    let info = db.part_info().unwrap();
    assert_eq!(info.name, "xc7");
    assert_eq!(info.device.as_deref(), Some("xc7"));
    assert_eq!(info.fabric, "xc7");
    // part.json only has iobanks: no frame tree, no idcode, no part.yaml.
    assert!(info.part.is_none() && info.idcode.is_none() && db.part().is_none());
    let iobanks = info.iobanks.as_ref().unwrap();
    assert_eq!(iobanks.len(), 2);
    assert_eq!(iobanks[0], (IdString::new("99"), IdString::new("X1Y26")));
    assert_eq!(info.package_pins.as_ref().unwrap().len(), 10);
    assert!(db.get_required_fasm_features(Some("xc7")).is_empty());

    let grid = db.grid().unwrap();
    assert_eq!(grid.len(), 11);
    let tile = grid.tile(IdString::new("CLBLM_L_X10Y102")).unwrap();
    assert_eq!((tile.grid_x, tile.grid_y), (30, 49));
    assert_eq!(db.tile_type_of(tile).unwrap().name, "CLBLM_L");
    let block = grid.bits_block(tile, BlockType::ClbIoClk).unwrap();
    assert_eq!(
        (block.base_address, block.frames, block.offset, block.words),
        (0x0002_0500, 36, 4, 2)
    );
    assert_eq!(grid.sites(tile).len(), 2);
    let sing = grid.tile(IdString::new("LIOB33_SING_X0Y0")).unwrap();
    let alias = grid.alias(&grid.bits(sing)[0]).unwrap();
    assert_eq!(
        (alias.tile_type, alias.start_offset),
        (IdString::new("LIOB33"), 2)
    );
    assert_eq!(grid.effective_offset(&grid.bits(sing)[0]), -2);
    assert_eq!(grid.pin_functions(sing).len(), 1);
    assert!(grid.tiles().iter().all(|t| t.tile_type_index().is_some()));

    let banks = db.banks_tiles_registry().unwrap();
    assert_eq!(
        banks.bank_of_tile(IdString::new("HCLK_IOI3_X1Y26")),
        Some(IdString::new("99"))
    );
    assert_eq!(
        banks.bank_of_tile(IdString::new("RIOB33_X43Y1")),
        Some(IdString::new("66"))
    );
    assert_eq!(banks.tiles_of_bank(IdString::new("99")).len(), 4);
}

#[test]
fn lookups() {
    let db = open();
    // `!` bits and word_bit > 31 in a 2-word tile (INT_L offset 4).
    let bits = positions(&db, "INT_L_X10Y102.IMUX_L4.EE2END2", 0);
    let expected: Vec<(bool, u32, u32, u32)> = vec![
        (false, 0x0002_0500 + 22, 5, 1),
        (false, 0x0002_0500 + 23, 5, 1),
        (false, 0x0002_0500 + 25, 5, 1),
        (true, 0x0002_0500 + 17, 5, 1),
        (true, 0x0002_0500 + 24, 5, 1),
    ];
    let got: Vec<_> = bits
        .iter()
        .map(|(set, p)| (*set, p.frame.0, p.word, p.bit))
        .collect();
    assert_eq!(got, expected);

    // Multi bit feature: address selects INIT[NN] (two digit names).
    let init10 = positions(&db, "CLBLM_L_X10Y102.SLICEM_X0.ALUT.INIT", 10);
    assert_eq!(init10.len(), 1);
    let entry = db
        .tile_type(IdString::new("CLBLM_L"))
        .unwrap()
        .segbits
        .get(IdString::new("SLICEM_X0.ALUT.INIT[10]"))
        .copied()
        .unwrap();
    let raw = db
        .tile_type(IdString::new("CLBLM_L"))
        .unwrap()
        .segbits
        .bits(&entry)[0];
    assert_eq!(
        init10[0].1.frame,
        FrameAddress(0x0002_0500 + raw.word_column)
    );
    assert_eq!(
        init10[0].1.word * 32 + init10[0].1.bit,
        4 * 32 + raw.word_bit
    );
    // Address 0 of a multi bit feature falls through to INIT[00].
    assert_eq!(
        positions(&db, "CLBLM_L_X10Y102.SLICEM_X0.ALUT.INIT", 0).len(),
        1
    );
    // A missing bit of a multi bit feature and a non zero address on a
    // single bit feature are errors.
    for (feature, address) in [
        ("CLBLM_L_X10Y102.SLICEM_X0.ALUT.INIT", 64),
        ("CLBLM_L_X10Y102.SLICEM_X0.AFF.ZINI", 1),
        ("CLBLM_L_X10Y102.SLICEM_X0.NOPE", 0),
        ("CLBLM_L_X10Y102", 0),
    ] {
        let err = db
            .lookup_fasm_feature(IdString::new(feature), address)
            .unwrap_err();
        assert!(
            matches!(err, LookupError::UnknownFeature { .. }),
            "{feature}: {err}"
        );
    }
    let err = db
        .lookup_fasm_feature(IdString::new("CLBLM_L_X10Y102.SLICEM_X0.NOPE"), 0)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Segment DB CLBLM_L, key CLBLM_L.SLICEM_X0.NOPE not found"
    );
    let err = db
        .lookup_fasm_feature(IdString::new("NO_SUCH_TILE_X1Y1.A.B"), 0)
        .unwrap_err();
    assert_eq!(
        err,
        LookupError::UnknownTile {
            tile: IdString::new("NO_SUCH_TILE_X1Y1")
        }
    );

    // The split API gives the same answer.
    let split = db
        .lookup_feature(
            IdString::new("HCLK_L_X31Y130"),
            IdString::new("ENABLE_BUFFER.HCLK_CK_BUFHCLK8"),
            0,
        )
        .unwrap();
    let FeatureLookup::Bits(bits) = split else {
        panic!("pseudo pip");
    };
    assert_eq!(bits.tile.name, "HCLK_L_X31Y130");
    assert_eq!(bits.entry.feature, "ENABLE_BUFFER.HCLK_CK_BUFHCLK8");
    assert_eq!(bits.block_type(), BlockType::ClbIoClk);
    assert_eq!(bits.frames(), 0x0002_0500..0x0002_0500 + 26);
    let (_, pos) = bits.positions().next().unwrap();
    assert_eq!((pos.unwrap().word, pos.unwrap().bit), (50, 14));

    // Alias tile: LIOB33_SING uses LIOB33's segbits at offset 0 - 2.
    let sing = positions(&db, "LIOB33_SING_X0Y0.IOB_Y0.SOMETHING.STEPDOWN", 0);
    assert_eq!(sing.len(), 1);
    assert_eq!(
        (sing[0].1.frame.0, sing[0].1.word, sing[0].1.bit),
        (0x0040_0000, 99, 3)
    );
    let FeatureLookup::Bits(bits) = db
        .lookup_fasm_feature(
            IdString::new("LIOB33_SING_X0Y0.IOB_Y0.SOMETHING.STEPDOWN"),
            0,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(bits.segbits_type.name, "LIOB33");
    assert_eq!(bits.offset, -2);
}

/// Every feature of every corpus FASM file of f4pga-xc-fasm resolves.
#[test]
fn corpus_features_resolve() {
    let db = open();
    let corpus = repo_root().join("tests/corpus/f4pga-xc-fasm");
    let mut files: Vec<_> = ["", "iob"]
        .iter()
        .flat_map(|dir| std::fs::read_dir(corpus.join(dir)).unwrap())
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "fasm"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 7, "{files:?}");
    let mut count = 0;
    for file in &files {
        for line in fasm::parse_fasm_filename(file).unwrap() {
            let Some(set_feature) = &line.set_feature else {
                continue;
            };
            for flat in fasm::canonical_features(set_feature) {
                let address = flat.start.unwrap_or(0);
                db.lookup_fasm_feature(flat.feature, address)
                    .unwrap_or_else(|e| {
                        panic!("{}: {} [{address}]: {e}", file.display(), flat.feature)
                    });
                count += 1;
            }
        }
    }
    assert!(count > 60, "{count}");
}

fn read_golden(name: &str) -> BTreeSet<(u32, u32, u32)> {
    let text = std::fs::read_to_string(testdata("mini-db-golden").join(name)).unwrap();
    text.lines()
        .map(|line| {
            let mut parts = line.strip_prefix("bit_").unwrap().split('_');
            let mut next = |radix| u32::from_str_radix(parts.next().unwrap(), radix).unwrap();
            (next(16), next(10), next(10))
        })
        .collect()
}

/// The set bits of a list of FASM features (value != 0, canonicalised).
fn set_bits(db: &Database, features: &[(IdString, u32)]) -> BTreeSet<(u32, u32, u32)> {
    let mut out = BTreeSet::new();
    for &(feature, address) in features {
        let FeatureLookup::Bits(bits) = db.lookup_fasm_feature(feature, address).unwrap() else {
            continue;
        };
        for (bit, position) in bits.positions() {
            let p = position.unwrap();
            if bit.is_set {
                out.insert((p.frame.0, p.word, p.bit));
            }
        }
    }
    out
}

fn fasm_features(path: &Path) -> Vec<(IdString, u32)> {
    let mut out = Vec::new();
    for line in fasm::parse_fasm_filename(path).unwrap() {
        if let Some(set_feature) = &line.set_feature {
            out.extend(
                fasm::canonical_features(set_feature).map(|f| (f.feature, f.start.unwrap_or(0))),
            );
        }
    }
    out
}

/// The STEPDOWN propagation of `xc_fasm/fasm2frames.py` (lines 216-272),
/// written out here only to check the tile <-> bank registry and the
/// grid sites against the golden output; the real implementation is
/// T5.4's.
fn stepdown_features(db: &Database, features: &[(IdString, u32)]) -> Vec<(IdString, u32)> {
    let banks = db.banks_tiles_registry().unwrap();
    let grid = db.grid().unwrap();
    let mut used = BTreeSet::new();
    let mut tags: Vec<(IdString, String)> = Vec::new();
    for &(feature, _) in features {
        let name = feature.to_string();
        let parts: Vec<&str> = name.splitn(3, '.').collect();
        if parts.len() < 3 {
            continue;
        }
        if parts[0].contains("IOB33") {
            used.insert((parts[0].to_owned(), parts[1].to_owned()));
        }
        if parts[2].contains("STEPDOWN") {
            let bank = banks.bank_of_tile(IdString::new(parts[0])).unwrap();
            tags.push((bank, parts[2].to_owned()));
        }
    }
    let mut out = Vec::new();
    for (bank, tag) in &tags {
        for &tile in banks.tiles_of_bank(*bank) {
            let tile_name = tile.to_string();
            if tile_name.contains("IOB33") {
                let t = grid.tile(tile).unwrap();
                for (site, _) in grid.sites(t) {
                    let last = site
                        .to_string()
                        .chars()
                        .last()
                        .unwrap()
                        .to_digit(10)
                        .unwrap();
                    let site = format!("IOB_Y{}", last % 2);
                    if !used.contains(&(tile_name.clone(), site.clone())) {
                        out.push((IdString::new(&format!("{tile_name}.{site}.{tag}")), 0));
                    }
                }
            }
            if tile_name.contains("HCLK_IOI3") {
                out.push((IdString::new(&format!("{tile_name}.STEPDOWN")), 0));
            }
        }
    }
    out
}

#[test]
fn golden_bits() {
    let db = open();
    let corpus = repo_root().join("tests/corpus/f4pga-xc-fasm");
    for (fasm_file, golden) in [
        ("lut_int.fasm", "lut_int.bits"),
        ("ff_int.fasm", "ff_int.bits"),
        ("ff_int_0s.fasm", "ff_int.bits"),
    ] {
        let features = fasm_features(&corpus.join(fasm_file));
        assert_eq!(set_bits(&db, &features), read_golden(golden), "{fasm_file}");
    }
    for (fasm_file, golden) in [
        ("iob/liob_stepdown.fasm", "liob_stepdown.bits"),
        ("iob/riob_stepdown.fasm", "riob_stepdown.bits"),
    ] {
        let mut features = fasm_features(&corpus.join(fasm_file));
        let extra = stepdown_features(&db, &features);
        assert!(!extra.is_empty());
        features.extend(extra);
        assert_eq!(set_bits(&db, &features), read_golden(golden), "{fasm_file}");
    }
}

#[test]
fn ecc_invariant_holds() {
    let db = open();
    let report = db.check_ecc_invariant();
    assert!(report.checked_bits > 100, "{}", report.checked_bits);
    assert!(report.violations.is_empty(), "{:?}", report.violations);
    assert!(report.unplaceable.is_empty(), "{:?}", report.unplaceable);
}

#[test]
fn open_errors() {
    let root = testdata("mini-db");
    let err = Database::open(&root, Some("xc7a35tcsg324-1")).unwrap_err();
    assert!(
        matches!(&err, DbError::UnknownPart { part, .. } if part == "xc7a35tcsg324-1"),
        "{err}"
    );
    assert!(err.to_string().contains("parts.yaml"), "{err}");
    let err = Database::open(&testdata(""), Some("xc7")).unwrap_err();
    assert!(matches!(err, DbError::UnknownLayout { .. }), "{err}");

    // No part: tile types only.
    let db = Database::open(&root, None).unwrap();
    assert!(db.grid().is_none() && db.part_info().is_none());
    assert_eq!(db.tile_types().len(), 8);
    assert_eq!(
        db.lookup_fasm_feature(IdString::new("CLBLM_L_X10Y102.SLICEM_X0.AFF.ZINI"), 0)
            .unwrap_err(),
        LookupError::NoGrid
    );
    assert!(db.check_ecc_invariant().violations.is_empty());
}

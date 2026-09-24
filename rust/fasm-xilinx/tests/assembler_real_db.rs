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

//! Assembler tests on the real prjxray-db `artix7` database: the `.frm`
//! output must be identical, byte for byte, to the reference's (checked
//! in under `tests/corpus/xilinx/`, see the READMEs there). Each test is
//! skipped (passes with a message) when the database has not been
//! fetched (`tools/fetch-db.sh prjxray artix7`, or set `FASM_DB_CACHE`);
//! the `.frm.xz` goldens are read with the `xz` tool (skipped without it).

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use fasm_xilinx::{fasm2frames, Database, Fasm2FramesOptions, Frames};

use common::{real_db, repo_root};

const PART: &str = "xc7a35tcsg324-1";

fn open() -> Option<Database> {
    let root = real_db("prjxray-db", "artix7")?;
    Some(Database::open(&root, Some(PART)).unwrap())
}

fn corpus(name: &str) -> PathBuf {
    repo_root().join("tests/corpus/xilinx/artix7").join(name)
}

/// The contents of a golden `.frm` (`.frm.xz` files are decompressed with
/// `xz -dc`), or `None` if `xz` is not available.
fn golden(name: &str) -> Option<String> {
    let path = corpus(name);
    if path.extension().is_some_and(|e| e == "xz") {
        let output = match Command::new("xz").arg("-dc").arg(&path).output() {
            Ok(output) => output,
            Err(e) => {
                eprintln!("skipping {name}: cannot run xz: {e}");
                return None;
            }
        };
        assert!(output.status.success(), "xz -dc {}", path.display());
        Some(String::from_utf8(output.stdout).unwrap())
    } else {
        Some(std::fs::read_to_string(path).unwrap())
    }
}

fn assemble(db: &Database, fasm: &Path, options: &Fasm2FramesOptions) -> (Frames, Vec<String>) {
    let start = Instant::now();
    let mut warnings = Vec::new();
    let frames = fasm2frames(db, fasm, options, &mut |w| warnings.push(w.to_owned()))
        .unwrap_or_else(|e| panic!("{}: {}", fasm.display(), e.traceback_line()));
    eprintln!(
        "{}: {} frames in {:?}",
        fasm.display(),
        frames.len(),
        start.elapsed()
    );
    (frames, warnings)
}

fn check(db: &Database, fasm: &str, options: &Fasm2FramesOptions, expected: &str) {
    let Some(expected) = golden(expected) else {
        return;
    };
    let (frames, warnings) = assemble(db, &corpus(fasm), options);
    assert!(warnings.is_empty(), "{warnings:?}");
    let actual = frames.to_frm_string();
    if actual != expected {
        let expected = Frames::read_frm(expected.as_bytes(), 101, &mut |w| panic!("{w}")).unwrap();
        let diff = expected.diff(&frames);
        let shown: Vec<String> = diff.iter().take(10).map(ToString::to_string).collect();
        panic!(
            "{fasm}: {} differences with the reference:\n{}",
            diff.len(),
            shown.join("\n")
        );
    }
}

fn sparse() -> Fasm2FramesOptions {
    Fasm2FramesOptions {
        sparse: true,
        ..Default::default()
    }
}

/// `smoke_x1y0.frm` was made with `--sparse` (its README).
#[test]
fn smoke_x1y0() {
    let Some(db) = open() else {
        return;
    };
    check(&db, "smoke_x1y0.fasm", &sparse(), "smoke_x1y0.frm");
}

const COUNTER: &str = "designs/f4pga-examples/counter_test/arty_35";

/// The f4pga-examples counter built by openXC7 (nextpnr-xilinx):
/// dense, sparse and with the PUDC_B pullup.
#[test]
fn counter_test() {
    let Some(db) = open() else {
        return;
    };
    let fasm = format!("{COUNTER}/top.fasm");
    check(
        &db,
        &fasm,
        &Default::default(),
        &format!("{COUNTER}/top.frm.xz"),
    );
    check(
        &db,
        &fasm,
        &sparse(),
        &format!("{COUNTER}/top.sparse.frm.xz"),
    );
    let pudc = Fasm2FramesOptions {
        emit_pudc_b_pullup: true,
        ..Default::default()
    };
    check(&db, &fasm, &pudc, &format!("{COUNTER}/top.pudc.frm.xz"));
}

/// Multi bit features of every value format, `!` bits, block RAM, a
/// pseudo PIP, STEPDOWN propagation, a wrapping alias tile, and the same
/// with a ROI.
#[test]
fn synthetic_multibit_stepdown() {
    let Some(db) = open() else {
        return;
    };
    let fasm = "synthetic/multibit_stepdown.fasm";
    check(
        &db,
        fasm,
        &sparse(),
        "synthetic/multibit_stepdown.sparse.frm.xz",
    );
    let roi = Fasm2FramesOptions {
        sparse: true,
        roi: Some(corpus("synthetic/multibit_stepdown.roi.json")),
        ..Default::default()
    };
    check(&db, fasm, &roi, "synthetic/multibit_stepdown.roi.frm.xz");

    // STEPDOWN reached every other IOB site of bank 14 and its HCLK_IOI3.
    let (frames, _) = assemble(&db, &corpus(fasm), &sparse());
    let set: BTreeSet<(u32, u32, u32)> = frames.set_bits().collect();
    let banks = db.banks_tiles_registry().unwrap();
    let bank = banks
        .bank_of_tile(fasm::idstring::IdString::new("LIOB33_X0Y1"))
        .unwrap();
    assert_eq!(bank.to_string(), "14");
    let hclk = fasm::idstring::IdString::new("HCLK_IOI3_X1Y26");
    assert!(banks.tiles_of_bank(bank).contains(&hclk));
    // HCLK_IOI3.STEPDOWN 38_15 39_14 39_15 39_16, offset 50 of 0x00400000.
    for (column, bit) in [(38, 15), (39, 14), (39, 15), (39, 16)] {
        assert!(set.contains(&(0x0040_0000 + column, 50, bit)));
    }
}

/// Bits past the end of the frame are dropped with prjxray's warning.
#[test]
fn sing_out_of_frame_warnings() {
    let Some(db) = open() else {
        return;
    };
    let (frames, warnings) = assemble(&db, &corpus("synthetic/sing_out_of_frame.fasm"), &sparse());
    assert_eq!(
        warnings,
        [
            "frame_clear: invalid word address 101 in line: \
             LIOB33_SING_X0Y49.IOB_Y0.PULLTYPE.PULLUP",
            "frame_set: invalid word address 101 in line: \
             LIOB33_SING_X0Y49.IOB_Y0.PULLTYPE.PULLUP",
            "frame_set: invalid word address 101 in line: \
             LIOB33_SING_X0Y49.IOB_Y0.PULLTYPE.PULLUP",
        ]
    );
    // The IOB_Y1 bits are written (word 100); the bus is in use.
    assert_eq!(frames.len(), 42);
    assert!(frames.set_bits().all(|(_, word, _)| word == 100));
}

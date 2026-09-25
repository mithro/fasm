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

//! Assembler tests on the miniature f4pga-xc-fasm database
//! (`testdata/mini-db`): every case of f4pga-xc-fasm's
//! `tests/test_fasm2frames.py`, the oracle's `.frm` output of every
//! fixture (dense and sparse, byte for byte) and the error behaviour.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fasm_xilinx::{
    fasm2frames, fasm2frames_from, propagate_stepdown, AssemblerError, Database,
    Fasm2FramesOptions, FasmAssembler, FasmInput, Frames,
};

use common::{repo_root, testdata};

fn open() -> Database {
    Database::open(&testdata("mini-db"), Some("xc7")).unwrap()
}

fn corpus(name: &str) -> PathBuf {
    repo_root().join("tests/corpus/f4pga-xc-fasm").join(name)
}

/// A FASM file in a fresh temporary directory, removed on drop.
struct TempFasm(PathBuf);

impl TempFasm {
    fn new(text: &str) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "fasm-xilinx-test-{}-{}.fasm",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, text).unwrap();
        TempFasm(path)
    }
}

impl Drop for TempFasm {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run_file(
    db: &Database,
    path: &Path,
    options: &Fasm2FramesOptions,
) -> (Result<Frames, AssemblerError>, Vec<String>) {
    let mut warnings = Vec::new();
    let result = fasm2frames(db, path, options, &mut |w| warnings.push(w.to_owned()));
    (result, warnings)
}

fn run_text(db: &Database, text: &str, sparse: bool) -> Result<Frames, AssemblerError> {
    let file = TempFasm::new(text);
    let options = Fasm2FramesOptions {
        sparse,
        ..Default::default()
    };
    let (result, warnings) = run_file(db, &file.0, &options);
    assert!(warnings.is_empty(), "{warnings:?}");
    result
}

/// The `.frm` text described by a frm summary of
/// `testdata/mini-db-golden/frm` (see the README there).
fn summary_to_frm(summary: &str, words_per_frame: usize) -> String {
    let mut frames = Frames::new(words_per_frame);
    let hex = |s: &str| u32::from_str_radix(s.trim_start_matches("0x"), 16).unwrap();
    for line in summary.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        match fields[0] {
            "frames" => {
                let start = hex(fields[1]);
                for i in 0..fields[2].parse::<u32>().unwrap() {
                    frames.get_or_insert_zeroed(start + i);
                }
            }
            "word" => {
                let words = frames.get_mut(hex(fields[1])).unwrap();
                words[fields[2].parse::<usize>().unwrap()] = hex(fields[3]);
            }
            other => panic!("bad summary line {other}"),
        }
    }
    frames.to_frm_string()
}

fn golden_frm(name: &str) -> String {
    let path = testdata("mini-db-golden/frm").join(name);
    summary_to_frm(&std::fs::read_to_string(path).unwrap(), 101)
}

/// `bitread2bits` of test_fasm2frames.py.
fn read_bits(name: &str) -> BTreeSet<(u32, u32, u32)> {
    let text = std::fs::read_to_string(testdata("mini-db-golden").join(name)).unwrap();
    text.lines()
        .map(|line| {
            let mut parts = line.strip_prefix("bit_").unwrap().split('_');
            let mut next = |radix| u32::from_str_radix(parts.next().unwrap(), radix).unwrap();
            (next(16), next(10), next(10))
        })
        .collect()
}

fn bits(frames: &Frames) -> BTreeSet<(u32, u32, u32)> {
    frames.set_bits().collect()
}

const FIXTURES: [&str; 7] = [
    "lut.fasm",
    "lut_int.fasm",
    "ff_int.fasm",
    "ff_int_0s.fasm",
    "ff_int_op1.fasm",
    "iob/liob_stepdown.fasm",
    "iob/riob_stepdown.fasm",
];

/// Every fixture gives exactly the oracle's `.frm` (dense and sparse).
#[test]
fn frm_matches_oracle() {
    let db = open();
    for fixture in FIXTURES {
        let stem = Path::new(fixture).file_stem().unwrap().to_str().unwrap();
        for (sparse, mode) in [(false, "dense"), (true, "sparse")] {
            let options = Fasm2FramesOptions {
                sparse,
                ..Default::default()
            };
            let (frames, warnings) = run_file(&db, &corpus(fixture), &options);
            assert!(warnings.is_empty());
            let frames = frames.unwrap_or_else(|e| panic!("{fixture}: {e}"));
            let expected = golden_frm(&format!("{stem}.{mode}.txt"));
            assert!(
                frames.to_frm_string() == expected,
                "{fixture} ({mode}) differs from the oracle"
            );
            // The reader gives the same frames back.
            let back = Frames::read_frm(expected.as_bytes(), 101, &mut |w| panic!("{w}")).unwrap();
            assert!(back.diff(&frames).is_empty());
        }
    }
}

/// `fasm2frames_from` with the FASM text in memory, and an assembler
/// sharing its database (`new_shared`) plus `propagate_stepdown`, give
/// the same frames as `fasm2frames` on the file.
#[test]
fn bytes_input_and_shared_assembler() {
    let db = std::sync::Arc::new(open());
    for fixture in FIXTURES {
        let path = corpus(fixture);
        let text = std::fs::read(&path).unwrap();
        for sparse in [false, true] {
            let options = Fasm2FramesOptions {
                sparse,
                ..Default::default()
            };
            let (expected, _) = run_file(&db, &path, &options);
            let expected = expected.unwrap();
            let from_bytes =
                fasm2frames_from(&db, FasmInput::Bytes(&text), &options, &mut |_| {}).unwrap();
            assert!(from_bytes == expected, "{fixture}");

            let mut assembler = FasmAssembler::new_shared(std::sync::Arc::clone(&db)).unwrap();
            assembler.parse_fasm_bytes(&text, Vec::new()).unwrap();
            propagate_stepdown(&db, &mut assembler).unwrap();
            assert!(
                assembler.get_frames(sparse).unwrap() == expected,
                "{fixture}"
            );
        }
    }
    let err = fasm2frames_from(
        &db,
        FasmInput::Bytes(b"NOPE_X0Y0.A\n"),
        &Default::default(),
        &mut |_| {},
    )
    .unwrap_err();
    assert_eq!(err.traceback_line(), "KeyError: 'NOPE_X0Y0'");
}

// The cases of f4pga-xc-fasm's tests/test_fasm2frames.py.

/// `test_lut`: simple smoke test on just the LUTs.
#[test]
fn test_lut() {
    let db = open();
    let (frames, _) = run_file(&db, &corpus("lut.fasm"), &Default::default());
    assert!(!bits(&frames.unwrap()).is_empty());
}

/// `test_lut_int`, `test_ff_int`, `test_ff_int_0s`, `test_stepdown_1`,
/// `test_stepdown_2`: the set bits equal the reference design's
/// (`bitread_frm_equals`).
#[test]
fn bitread_frm_equals() {
    let db = open();
    for (fixture, golden) in [
        ("lut_int.fasm", "lut_int.bits"),
        ("ff_int.fasm", "ff_int.bits"),
        ("ff_int_0s.fasm", "ff_int.bits"),
        ("iob/liob_stepdown.fasm", "liob_stepdown.bits"),
        ("iob/riob_stepdown.fasm", "riob_stepdown.bits"),
    ] {
        let (frames, _) = run_file(&db, &corpus(fixture), &Default::default());
        let frames = frames.unwrap();
        // frm2bits checks 101 words per frame.
        assert!(frames.iter().all(|(_, w)| w.len() == 101));
        assert_eq!(bits(&frames), read_bits(golden), "{fixture}");
    }
}

/// `test_ff_int_op1` (skipped upstream: "Omitted key set to"): the file
/// omits the optional `SRUSEDMUX` feature, so its bits are NOT the ones
/// of `ff_int/design.bits`; it assembles without error (no inconsistent
/// bits), exactly like the oracle (see `frm_matches_oracle`).
#[test]
fn test_ff_int_op1() {
    let db = open();
    let (frames, _) = run_file(&db, &corpus("ff_int_op1.fasm"), &Default::default());
    let op1 = bits(&frames.unwrap());
    let reference = read_bits("ff_int.bits");
    assert_ne!(op1, reference);
    let missing: Vec<_> = reference.difference(&op1).collect();
    // SRUSEDMUX (CLBLM_L.SLICEM_X0.SRUSEDMUX 01_37) is not set.
    assert_eq!(missing, [&(0x0002_0501, 5, 3)]);
    assert!(op1.is_subset(&reference));
}

/// `test_opkey_01_default`: an optional key with the value omitted.
#[test]
fn test_opkey_01_default() {
    let db = open();
    let frames = run_text(&db, "CLBLM_L_X10Y102.SLICEM_X0.SRUSEDMUX", true).unwrap();
    assert_eq!(bits(&frames), BTreeSet::from([(0x0002_0501, 5, 3)]));
}

/// `test_opkey_01_1` (skipped upstream): `FEATURE 1` is a syntax error.
#[test]
fn test_opkey_01_1() {
    let db = open();
    let e = run_text(&db, "CLBLM_L_X10Y102.SLICEM_X0.SRUSEDMUX 1", false).unwrap_err();
    assert!(matches!(e, AssemblerError::Parse(_)), "{e}");
    assert!(
        e.traceback_line()
            .starts_with("Exception: Parse error at 1:36 - "),
        "{}",
        e.traceback_line()
    );
}

/// `test_opkey_enum` (skipped upstream, it expects a syntax error that
/// no FASM parser reports): `AFFMUX.O6` is a plain feature.
#[test]
fn test_opkey_enum() {
    let db = open();
    let frames = run_text(&db, "CLBLM_L_X10Y102.SLICEM_X0.AFFMUX.O6", true).unwrap();
    assert_eq!(bits(&frames), BTreeSet::from([(0x0002_051E, 4, 3)]));
}

/// `test_badkey`: `FEATURE 2` is a syntax error.
#[test]
fn test_badkey() {
    let db = open();
    let e = run_text(&db, "CLBLM_L_X10Y102.SLICEM_X0.SRUSEDMUX 2", false).unwrap_err();
    assert!(e
        .traceback_line()
        .starts_with("Exception: Parse error at 1:36 - "));
}

/// `test_dupkey` (skipped upstream): its input is a syntax error (there
/// is no duplicate key detection, only bit level conflicts).
#[test]
fn test_dupkey() {
    let db = open();
    let e = run_text(
        &db,
        "CLBLM_L_X10Y102.SLICEM_X0.SRUSEDMUX 0\nCLBLM_L_X10Y102.SLICEM_X0.SRUSEDMUX 1\n",
        false,
    )
    .unwrap_err();
    assert!(e
        .traceback_line()
        .starts_with("Exception: Parse error at 1:36 - "));
    // The same feature twice is fine.
    let text = "CLBLM_L_X10Y102.SLICEM_X0.SRUSEDMUX\nCLBLM_L_X10Y102.SLICEM_X0.SRUSEDMUX = 1\n";
    run_text(&db, text, false).unwrap();
}

/// `test_sparse` (skipped upstream): the sparse and the dense output
/// have the same set bits. The upstream test also asserts that the dense
/// text is at least 4 times longer, which does not hold for the
/// miniature database (3.3 times, 134640 vs 40392 bytes).
#[test]
fn test_sparse() {
    let db = open();
    let run = |sparse| {
        let options = Fasm2FramesOptions {
            sparse,
            ..Default::default()
        };
        run_file(&db, &corpus("lut_int.fasm"), &options).0.unwrap()
    };
    let (sparse, dense) = (run(true), run(false));
    assert_eq!(bits(&sparse), bits(&dense));
    let (s, d) = (sparse.to_frm_string().len(), dense.to_frm_string().len());
    assert_eq!((s, d), (40392, 134640));
}

// Error behaviour; the expected messages are the oracle's.

#[test]
fn inconsistent_bits() {
    let db = open();
    let e = run_text(
        &db,
        "CLBLM_L_X10Y102.SLICEM_X0.AFFMUX.AX\nCLBLM_L_X10Y102.SLICEM_X0.AFFMUX.CY # c\n",
        false,
    )
    .unwrap_err();
    assert_eq!(
        e.traceback_line(),
        "prjxray.fasm_assembler.FasmInconsistentBits: FASM line \
         \"CLBLM_L_X10Y102.SLICEM_X0.AFFMUX.CY # c\" wanted to clear bit (132382, 4, 1) but \
         was set by FASM line \"CLBLM_L_X10Y102.SLICEM_X0.AFFMUX.AX\""
    );
    let e = run_text(
        &db,
        "CLBLM_L_X10Y102.SLICEM_X0.AFFMUX.CY\nCLBLM_L_X10Y102.SLICEM_X0.AFFMUX.AX { x = \"y\" }\n",
        false,
    )
    .unwrap_err();
    assert_eq!(
        e.traceback_line(),
        "prjxray.fasm_assembler.FasmInconsistentBits: FASM line \
         \"CLBLM_L_X10Y102.SLICEM_X0.AFFMUX.AX { x = \"y\" }\" wanted to clear bit \
         (132382, 4, 0) but was set by FASM line \"CLBLM_L_X10Y102.SLICEM_X0.AFFMUX.CY\""
    );
}

#[test]
fn lookup_errors_are_batched() {
    let db = open();
    let e = run_text(
        &db,
        "INT_L_X10Y102.NOPE.X\nCLBLM_L_X10Y102.SLICEM_X0.NOPE[5:0] = 6'h3F\n\
         CLBLM_L_X10Y102.SLICEM_X0.ALUT.INIT[70] = 1\nCLBLM_L_X10Y102\n",
        false,
    )
    .unwrap_err();
    let mut expected = vec![
        "prjxray.fasm_assembler.FasmLookupError: Segment DB INT_L, key INT_L.NOPE.X not \
         found from line 'INT_L_X10Y102.NOPE.X'"
            .to_owned(),
    ];
    for _ in 0..6 {
        expected.push(
            "Segment DB CLBLM_L, key CLBLM_L.SLICEM_X0.NOPE not found from line \
             'CLBLM_L_X10Y102.SLICEM_X0.NOPE[5:0] = 6'h3F'"
                .to_owned(),
        );
    }
    expected.push(
        "Segment DB CLBLM_L, key CLBLM_L.SLICEM_X0.ALUT.INIT not found from line \
         'CLBLM_L_X10Y102.SLICEM_X0.ALUT.INIT[70] = 1'"
            .to_owned(),
    );
    expected
        .push("Segment DB CLBLM_L, key CLBLM_L. not found from line 'CLBLM_L_X10Y102'".to_owned());
    assert_eq!(e.traceback_line(), expected.join("\n"));
}

/// An unknown tile is an immediate `KeyError` (earlier lookup errors are
/// lost), but only when a bit is enabled: `= 0` does nothing.
#[test]
fn unknown_tile_is_a_key_error() {
    let db = open();
    let e = run_text(&db, "CLBLM_L_X10Y102.SLICEM_X0.NOPE\nFOO_X0Y0.BAR\n", false).unwrap_err();
    assert_eq!(e.traceback_line(), "KeyError: 'FOO_X0Y0'");
    let frames = run_text(
        &db,
        "FOO_X1Y1.BAR = 0\nCLBLM_L_X10Y102.SLICEM_X0.ALUT.INIT[63:0] = 0\n",
        true,
    )
    .unwrap();
    assert!(frames.is_empty());
}

#[test]
fn missing_file() {
    let db = open();
    let (e, _) = run_file(
        &db,
        Path::new("/nonexistent/file.fasm"),
        &Default::default(),
    );
    assert_eq!(
        e.unwrap_err().traceback_line(),
        "Exception: Parse error at 0:0 - Couldn't open file"
    );
}

/// The `_SING` alias tiles write word 99 through a negative index, and
/// the wrapped bit does not conflict with a direct write of word 99.
#[test]
fn sing_tiles_wrap() {
    let db = open();
    let frames = run_text(&db, "LIOB33_SING_X0Y0.IOB_Y1.SOMETHING.IN\n", true).unwrap();
    assert_eq!(bits(&frames), BTreeSet::from([(0x0040_0000, 99, 1)]));
    // Same physical bit, set twice through different keys: no error.
    let mut assembler = FasmAssembler::new(&db).unwrap();
    assembler
        .parse_fasm_bytes(
            b"LIOB33_SING_X0Y0.IOB_Y1.SOMETHING.IN\nLIOB33_SING_X0Y0.IOB_Y0.SOMETHING.IN\n",
            Vec::new(),
        )
        .unwrap();
    let frames = assembler.get_frames(true).unwrap();
    assert_eq!(bits(&frames), BTreeSet::from([(0x0040_0000, 99, 1)]));
}

/// STEPDOWN goes to every unused IOB site of the bank and to its
/// `HCLK_IOI3` tile, also for a bank whose STEPDOWN features are all
/// written explicitly.
#[test]
fn stepdown() {
    let db = open();
    let frames = run_text(
        &db,
        "LIOB33_X0Y3.IOB_Y0.SOMETHING.STEPDOWN\nRIOB33_X43Y3.IOB_Y1.SOMETHING.STEPDOWN\n\
         LIOB33_X0Y3.IOB_Y1.SOMETHING.STEPDOWN = 0\n",
        true,
    )
    .unwrap();
    let expected = [
        (0x0040_0000, 2, 3),
        (0x0040_0000, 6, 3),
        (0x0040_0000, 50, 16),
        (0x0040_0000, 99, 3),
        (0x0040_1580, 2, 3),
        (0x0040_1580, 6, 3),
        (0x0040_1580, 50, 16),
        (0x0040_1580, 99, 3),
    ];
    assert_eq!(bits(&frames), BTreeSet::from(expected));

    let file = TempFasm::new("CLBLM_L_X10Y102.SLICEM_X0.A5FF.STEPDOWN_X\n");
    let (e, _) = run_file(&db, &file.0, &Default::default());
    // The feature does not exist: the lookup error comes first.
    assert!(matches!(e.unwrap_err(), AssemblerError::Lookup(_)));
}

/// The feature callback sees every feature, including `= 0` ones, and
/// can abort.
#[test]
fn feature_callback() {
    use std::sync::{Arc, Mutex};
    let db = open();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut assembler = FasmAssembler::new(&db).unwrap();
    let log = Arc::clone(&seen);
    assembler.set_feature_callback(Box::new(move |f| {
        log.lock().unwrap().push(f.feature.to_string());
        if f.feature.to_string().starts_with("STOP") {
            return Err(AssemblerError::KeyError("stop".to_owned()));
        }
        Ok(())
    }));
    let e = assembler
        .parse_fasm_bytes(
            b"# c\nA_X0Y0.B = 0\nCLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI\nSTOP.X\nNOT.SEEN\n",
            Vec::new(),
        )
        .unwrap_err();
    assert_eq!(e.traceback_line(), "KeyError: 'stop'");
    assert_eq!(
        *seen.lock().unwrap(),
        ["A_X0Y0.B", "CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI", "STOP.X"]
    );
}

/// ROI: the frames of the tiles in the rectangle are output even when
/// sparse, and its `required_features` are applied after the file.
#[test]
fn roi() {
    let db = open();
    let roi = TempFasm::new(
        r#"{"info": {"GRID_X_MIN": 0, "GRID_X_MAX": 0, "GRID_Y_MIN": 154, "GRID_Y_MAX": 155},
            "required_features": ["LIOB33_X0Y1.IOB_Y1.SOMETHING.OUT"]}"#,
    );
    let fasm = TempFasm::new("CLBLM_L_X10Y102.SLICEM_X0.AFF.ZINI\n");
    let options = Fasm2FramesOptions {
        sparse: true,
        roi: Some(roi.0.clone()),
        ..Default::default()
    };
    let (frames, _) = run_file(&db, &fasm.0, &options);
    let frames = frames.unwrap();
    // CLBLM_L (36 frames at 0x20500) and the LIOB33 column (42 frames).
    assert_eq!(frames.len(), 36 + 42);
    assert!(frames.contains(0x0040_0000) && frames.contains(0x0040_0029));
    assert!(bits(&frames).contains(&(0x0040_0000, 2, 2)));

    let bad = TempFasm::new(r#"{"info": {"GRID_X_MIN": 0}}"#);
    let options = Fasm2FramesOptions {
        roi: Some(bad.0.clone()),
        ..Default::default()
    };
    let (e, _) = run_file(&db, &fasm.0, &options);
    assert_eq!(e.unwrap_err().traceback_line(), "KeyError: 'GRID_X_MAX'");

    // An empty ROI path is no ROI (Python truthiness).
    let options = Fasm2FramesOptions {
        sparse: true,
        roi: Some(PathBuf::new()),
        ..Default::default()
    };
    let (frames, _) = run_file(&db, &fasm.0, &options);
    assert_eq!(frames.unwrap().len(), 36);
}

/// The mini database has no PUDC_B pin: `--emit_pudc_b_pullup` is a no-op.
#[test]
fn pudc_b_without_pin() {
    let db = open();
    assert_eq!(fasm_xilinx::find_pudc_b(&db).unwrap(), None);
    let options = Fasm2FramesOptions {
        emit_pudc_b_pullup: true,
        ..Default::default()
    };
    let (frames, _) = run_file(&db, &corpus("lut_int.fasm"), &options);
    assert_eq!(
        frames.unwrap().to_frm_string(),
        golden_frm("lut_int.dense.txt")
    );
}

/// Random lines (valid and unknown tiles and features, random ranges and
/// values, including `SetFasmFeature`s that break the model invariants)
/// never panic, whatever the outcome.
#[test]
fn random_lines_do_not_panic() {
    use fasm::idstring::IdString;
    use fasm::{FasmLine, FeatureValue, SetFasmFeature};

    let db = open();
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let tiles = [
        "CLBLM_L_X10Y102",
        "INT_L_X10Y102",
        "LIOB33_SING_X0Y0",
        "LIOB33_X0Y1",
        "RIOB33_X43Y3",
        "HCLK_IOI3_X1Y26",
        "HCLK_L_X31Y130",
        "NOPE_X0Y0",
        "",
    ];
    let features = [
        "SLICEM_X0.ALUT.INIT",
        "SLICEM_X0.AFFMUX.AX",
        "SLICEM_X0.AFFMUX.CY",
        "IMUX_L1.EE2END0",
        "IOB_Y0.SOMETHING.STEPDOWN",
        "IOB_Y1.SOMETHING.IN",
        "STEPDOWN",
        "ENABLE_BUFFER.HCLK_CK_BUFHCLK8",
        "NOPE",
        "",
    ];
    for _ in 0..300 {
        let mut assembler = FasmAssembler::new(&db).unwrap();
        let mut missing = Vec::new();
        let mut result = Ok(());
        for _ in 0..(next() % 8) {
            let tile = tiles[(next() % tiles.len() as u64) as usize];
            let feature = features[(next() % features.len() as u64) as usize];
            let name = if feature.is_empty() {
                tile.to_owned()
            } else {
                format!("{tile}.{feature}")
            };
            if name.is_empty() {
                continue;
            }
            let start = (next() % 3 != 0).then(|| (next() % 70) as u32);
            let end = (next() % 2 == 0).then(|| (next() % 80) as u32);
            let value = FeatureValue::from_u64(next() % 5);
            let set_feature =
                SetFasmFeature::new_unchecked(IdString::new(&name), start, end, value, None);
            let line = FasmLine {
                set_feature: Some(set_feature),
                annotations: None,
                comment: None,
            };
            result = assembler.add_fasm_line(line, &mut missing);
            if result.is_err() {
                break;
            }
        }
        match result {
            Ok(()) => {
                for sparse in [false, true] {
                    if let Ok(frames) = assembler.get_frames(sparse) {
                        assert!(frames.iter().all(|(_, w)| w.len() == 101));
                    }
                }
            }
            Err(e) => assert!(!e.traceback_line().is_empty()),
        }
    }
}

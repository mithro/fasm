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

//! The synthetic UltraScale+ database (`testdata/synthetic-usp-db`, see its
//! README): the 16-bit unit of prjuray-db, prjuray's assembler semantics
//! (`uray_fasm2frames`), and FASM -> frames -> `.bit` -> frames with the
//! UltraScale+ bitstream format. The expected `.frm` / dump texts were
//! checked against prjuray's `utils/fasm2frames.py` (the oracle
//! `tests/oracle/uray-fasm2frames-oracle`).

mod common;

use std::path::PathBuf;

use fasm_xilinx::bitstream::{
    bitstream_bytes, fdri_payload, BitstreamOptions, BitstreamReader, Ecc,
};
use fasm_xilinx::{
    dump_frames_sparse_halfwords, uray_fasm2frames, write_bits, write_frm_halfwords, Architecture,
    AssemblerError, Database, Fasm2FramesOptions, FrameAddress, Frames,
};

use common::{positions, testdata};

const PART: &str = "xcusptest-1";

fn open() -> Database {
    Database::open(&testdata("synthetic-usp-db"), Some(PART)).unwrap()
}

fn fasm_file(name: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fasm-usp-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn assemble(name: &str, text: &str, sparse: bool) -> Result<Frames, AssemblerError> {
    let db = open();
    let options = Fasm2FramesOptions {
        sparse,
        ..Default::default()
    };
    uray_fasm2frames(&db, &fasm_file(name, text), &options)
}

const DESIGN: &str = "\
CLEM_X1Y0.ALUT.INIT[15:0] = 16'hA5C3
CLEM_X1Y1.ABCDFF.CEUSED.V1
CLEM_X1Y1.AFF.INIT.V0
BRAM_X2Y0.RAMB18E2_L.INIT_00[7:0] = 8'hFF
BRAM_X2Y0.RAMB18E2_L.CLKARDCLKINV.V1
RCLK_INT_L_X2Y29.BUFCE_LEAF_X0Y0.BUFCE_LEAF.DELAY_TAP.V0
EDGE_X0Y0.OK
";

#[test]
fn part_and_positions() {
    let db = open();
    assert_eq!(db.architecture(), Architecture::UltraScalePlus);
    let bit = |feature, address| {
        positions(&db, feature, address)
            .into_iter()
            .map(|(set, p)| (set, p.frame.0, p.word, p.bit))
            .collect::<Vec<_>>()
    };
    // Offset 3 (16-bit words) + 12_02: 16-bit word 3, bit 2 = word 1 bit 18.
    assert_eq!(bit("CLEM_X1Y1.AFF.INIT.V0", 0), [(true, 0x10C, 1, 18)]);
    // Offset 93: the upper half of word 46, after the ECC.
    assert_eq!(
        bit("RCLK_INT_L_X2Y29.BUFCE_LEAF_X0Y0.BUFCE_LEAF.CEINV.V1", 0),
        [(true, 0x302, 46, 28)]
    );
    // BLOCK_RAM bus, 00_60: word 3 bit 12 of the 16-bit unit = word 1 bit 28.
    assert_eq!(
        bit("BRAM_X2Y0.RAMB18E2_L.INIT_00", 4),
        [(true, 0x0100_0000, 1, 28)]
    );
    // The bottom half: row index 32.
    let p = bit("CLEM_X1Y60.AFF.INIT.V0", 0);
    let frame = FrameAddress(p[0].1);
    assert!(frame.is_bottom_half(Architecture::UltraScalePlus));
    assert_eq!(frame.row_index(Architecture::UltraScalePlus), 32);
    // The last word of the frame (185 * 16 + 3).
    assert_eq!(bit("EDGE_X0Y0.OK", 0), [(true, 0x100, 92, 19)]);
    for (_, p) in positions(&db, "CLEM_X1Y0.ALUT.INIT", 5)
        .into_iter()
        .chain(positions(
            &db,
            "RCLK_INT_L_X2Y29.BUFCE_LEAF_X0Y0.BUFCE_LEAF.CEINV.V1",
            0,
        ))
    {
        assert!(!Architecture::UltraScalePlus.is_ecc_bit(p));
    }
}

#[test]
fn prjuray_frames_and_outputs() {
    let frames = assemble("design.fasm", DESIGN, true).unwrap();
    // The frames of every bus written (the bits' tiles; required features
    // add CLEM_X1Y60).
    assert_eq!(frames.words_per_frame(), 93);
    assert_eq!(frames.len(), 16 + 6 + 256 + 76 + 16);
    assert_eq!(frames.get(0x100).unwrap()[92], 1 << 19);
    assert_eq!(frames.get(0x302).unwrap()[46], 0);
    assert_eq!(frames.get(0x300).unwrap()[46], 1 << 29);
    let mut dump = Vec::new();
    dump_frames_sparse_halfwords(&frames, &mut dump).unwrap();
    let dump = String::from_utf8(dump).unwrap();
    assert!(dump.starts_with(
        "\nFrames: 370\nFrame @ 0x00000100\n   185: 0x00000008\nFrame @ 0x00000108\n    0: 0x00005000\n"
    ));
    assert!(dump.contains("Frame @ 0x00000300\n   93: 0x00002000\n"));
    let mut frm = Vec::new();
    write_frm_halfwords(&frames, &mut frm).unwrap();
    let frm = String::from_utf8(frm).unwrap();
    let first = frm.lines().next().unwrap();
    assert_eq!(first.split(',').count(), 186);
    assert!(first.ends_with(",0x00000008"));
    let mut bits = Vec::new();
    write_bits(&frames, &mut bits).unwrap();
    let bits = String::from_utf8(bits).unwrap();
    assert!(bits.starts_with("bit_00000100_092_19\n"));
    assert_eq!(bits.lines().count(), frames.set_bits().count());
    // Dense: every frame of every tile.
    let dense = assemble("design.fasm", DESIGN, false).unwrap();
    assert_eq!(dense.len(), frames.len());
}

#[test]
fn prjuray_assembler_errors() {
    // A set bit past the end of the frame: IndexError (xc_fasm drops it).
    let e = assemble("out.fasm", "EDGE_X0Y0.OUT\n", false).unwrap_err();
    assert_eq!(e.traceback_line(), "IndexError: list index out of range");
    // A cleared one is harmless, but its frame is output.
    let frames = assemble("clear.fasm", "EDGE_X0Y0.CLEAR_OUT\n", true).unwrap();
    assert!(frames.get(0x100).is_some());
    // Conflicts are reported in 16-bit words.
    let e = assemble(
        "conflict.fasm",
        "CLEM_X1Y1.AFF.INIT.V0\nCLEM_X1Y1.AFF.INIT.V1\n",
        false,
    )
    .unwrap_err();
    assert_eq!(
        e.to_string(),
        "FASM line \"CLEM_X1Y1.AFF.INIT.V1\" wanted to clear bit (268, 3, 2) but was set by FASM line \"CLEM_X1Y1.AFF.INIT.V0\""
    );
}

#[test]
fn bitstream_round_trip() {
    let db = open();
    let part = db.part_info().unwrap().part.clone().unwrap();
    assert_eq!(part.architecture, Architecture::UltraScalePlus);
    assert_eq!(part.idcode, 0x04A4_2093);
    // Every frame is on the walk of addMissingFrames.
    assert_eq!(part.iter_frame_addresses().count(), part.frame_count());
    let frames = assemble("design.fasm", DESIGN, false).unwrap();
    let options = BitstreamOptions {
        design_name: b"design.frm".to_vec(),
        part_name: b"xcusptest".to_vec(),
        date: Some("2026/01/02".into()),
        time: Some("03:04:05".into()),
        ..Default::default()
    };
    let bytes = bitstream_bytes(&part, &frames, &options).unwrap();
    // Two padding frames after each of the 4 (row, bus) groups.
    let payload = fdri_payload(&part, &frames).unwrap();
    assert_eq!(payload.len(), (part.frame_count() + 8) * 93);
    let reader = BitstreamReader::from_bytes(&bytes).unwrap();
    // 21 sync words, the 32-bit word after the sync word is a NOP.
    assert_eq!(reader.words()[0], 0x2000_0000);
    let config = reader.configuration(&part).unwrap();
    assert_eq!(config.len(), part.frame_count());
    for (address, words) in config.frames() {
        assert_eq!(
            Ecc::UltraScalePlus.verify(words),
            Some(true),
            "{address:#x}"
        );
    }
    let back = config.to_frames(true, false);
    for (address, words) in frames.iter() {
        assert_eq!(back.get(address), Some(words), "{address:#x}");
    }
    let nonzero = back
        .iter()
        .filter(|(_, w)| w.iter().any(|&x| x != 0))
        .count();
    assert_eq!(
        nonzero,
        frames
            .iter()
            .filter(|(_, w)| w.iter().any(|&x| x != 0))
            .count()
    );
}

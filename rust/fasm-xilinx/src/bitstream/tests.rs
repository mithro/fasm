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

//! Unit tests of the bitstream writer and reader, including ports of
//! prjxray's `configuration_test.cc` and `frames_test.cc`.

use super::packet::{
    command, register, type1_header, type2_header, Packet, NOP_HEADER, OPCODE_WRITE,
};
use super::*;
use crate::arch::{Architecture, BlockType, FrameAddress, FrameAddressFields};
use crate::frames::Frames;
use crate::part::Part;

const S7: Architecture = Architecture::Series7;

fn fa(block_type: BlockType, bottom: bool, row: u8, column: u16, minor: u16) -> FrameAddress {
    FrameAddress::compose(
        S7,
        FrameAddressFields {
            block_type: block_type.raw(),
            bottom,
            row,
            column,
            minor,
        },
    )
    .unwrap()
}

fn write(register: u32, data: &[u32]) -> Packet<'_> {
    Packet {
        header_type: 1,
        opcode: OPCODE_WRITE,
        register,
        data,
    }
}

fn frame_of(value: u32) -> Vec<u32> {
    vec![value; 101]
}

/// `ConfigurationTest.ConstructFromPacketsWithSingleFrame`.
#[test]
fn construct_from_packets_with_single_frame() {
    let part = Part::from_frame_addresses(S7, 0x1234, [FrameAddress(0x4567), FrameAddress(0x4568)])
        .unwrap();
    let frame = frame_of(0xAA);
    let packets = [
        write(register::IDCODE, &[0x1234]),
        write(register::FAR, &[0x4567]),
        write(register::CMD, &[0x0001]),
        write(register::FDRI, &frame),
    ];
    let config = Configuration::from_packets(&part, packets).unwrap();
    assert_eq!(config.len(), 1);
    assert_eq!(config.get(0x4567), Some(&frame[..]));
}

/// `ConfigurationTest.ConstructFromPacketsWithAutoincrement`.
#[test]
fn construct_from_packets_with_autoincrement() {
    let addresses = (0x4560..0x4570).chain(0x4580..0x4590).map(FrameAddress);
    let part = Part::from_frame_addresses(S7, 0x1234, addresses).unwrap();
    let mut frame = frame_of(0xAA);
    frame.extend(frame_of(0xBB));
    let packets = [
        write(register::IDCODE, &[0x1234]),
        write(register::FAR, &[0x456F]),
        write(register::CMD, &[0x0001]),
        write(register::FDRI, &frame),
    ];
    let config = Configuration::from_packets(&part, packets).unwrap();
    assert_eq!(config.len(), 2);
    assert_eq!(config.get(0x456F), Some(&frame_of(0xAA)[..]));
    assert_eq!(config.get(0x4580), Some(&frame_of(0xBB)[..]));
}

#[test]
fn idcode_mismatch() {
    let part = Part::from_frame_addresses(S7, 0x1234, [FrameAddress(0)]).unwrap();
    let packets = [write(register::IDCODE, &[0x1235])];
    assert_eq!(
        Configuration::from_packets(&part, packets),
        Err(ReadError::IdcodeMismatch {
            found: 0x1235,
            expected: 0x1234
        })
    );
}

fn padding_part() -> (Part, Vec<FrameAddress>) {
    let addresses = vec![
        fa(BlockType::ClbIoClk, false, 0, 0, 0),
        fa(BlockType::ClbIoClk, true, 0, 0, 0),
        fa(BlockType::ClbIoClk, true, 1, 0, 0),
        fa(BlockType::BlockRam, false, 0, 0, 0),
        fa(BlockType::BlockRam, false, 1, 0, 0),
    ];
    let part = Part::from_frame_addresses(S7, 0x1234, addresses.clone()).unwrap();
    (part, addresses)
}

/// `ConfigurationTest.CheckForPaddingFrames`.
#[test]
fn check_for_padding_frames() {
    let (part, addresses) = padding_part();
    let mut frames = Frames::new(101);
    for (address, value) in addresses.iter().zip([0xAA, 0xBB, 0xCC, 0xDD, 0xEE]) {
        frames.insert_if_absent(address.0, &frame_of(value));
    }
    let payload = fdri_payload(&part, &frames).unwrap();
    // 4 row/half/block type switches (2 frames each), 2 frames at the end.
    assert_eq!(payload.len(), 15 * 101);
    let packets = [
        write(register::IDCODE, &[0x1234]),
        write(register::FAR, &[0]),
        write(register::CMD, &[0x0001]),
        write(register::FDRI, &payload),
    ];
    let config = Configuration::from_packets(&part, packets).unwrap();
    assert_eq!(config.len(), 5);
    for (address, words) in config.frames() {
        let mut expected = frames.get(address).unwrap().to_vec();
        ecc::update_ecc(&mut expected);
        assert_eq!(words, &expected[..]);
    }
}

/// `FramesTest.FillInMissingFrames`: the payload has every frame of the
/// part, the missing ones zero filled.
#[test]
fn fill_in_missing_frames() {
    let addresses: Vec<FrameAddress> = (0..5)
        .map(|m| fa(BlockType::ClbIoClk, false, 0, 0, m))
        .collect();
    let part = Part::from_frame_addresses(S7, 0x1234, addresses).unwrap();
    let mut frames = Frames::new(101);
    frames.insert_if_absent(2, &frame_of(0xCC));
    frames.insert_if_absent(3, &frame_of(0xDD));
    frames.insert_if_absent(4, &frame_of(0xEE));
    let payload = fdri_payload(&part, &frames).unwrap();
    assert_eq!(payload.len(), 7 * 101);
    assert!(payload[..202].iter().all(|&w| w == 0));
    let mut cc = frame_of(0xCC);
    ecc::update_ecc(&mut cc);
    assert_eq!(&payload[202..303], &cc[..]);
    assert!(payload[5 * 101..].iter().all(|&w| w == 0));
}

/// Frames outside the part are written in address order; frames before
/// address 0 of the walk cannot exist, frames after the last one come
/// last.
#[test]
fn extra_frames_are_kept() {
    let (part, _) = padding_part();
    let mut frames = Frames::new(101);
    frames.insert_if_absent(0x10, &frame_of(1));
    frames.insert_if_absent(0x0FFF_FFFF, &frame_of(2));
    let payload = fdri_payload(&part, &frames).unwrap();
    // 5 part frames + 2 extra; two zero frames after 0x0, 0x10 (its next
    // address is the bottom half's 0x400000), 0x400000, 0x420000 and
    // 0x800000, none after 0x820000 (the last frame of the part) and
    // 0x0FFFFFFF (no next address), two at the end: 19 frames.
    assert_eq!(payload.len(), 19 * 101);
    assert_eq!(payload[101], 0);
    assert_eq!(payload[303], 1);
    assert_eq!(payload[16 * 101], 2);
}

#[test]
fn architecture_mismatch_and_word_count() {
    let part =
        Part::from_frame_addresses(Architecture::UltraScalePlus, 1, [FrameAddress(0)]).unwrap();
    assert!(matches!(
        fdri_payload_with(
            &part,
            &Frames::new(93),
            &BitstreamFormat::prjxray(Architecture::UltraScalePlus)
        ),
        Err(BitstreamError::ArchitectureMismatch {
            part: Architecture::UltraScalePlus,
            format: Architecture::Series7,
        })
    ));
    assert_eq!(fdri_payload(&part, &Frames::new(93)).unwrap().len(), 3 * 93);
    assert!(matches!(
        Configuration::from_packets_with(
            &part,
            &BitstreamFormat::native(Architecture::UltraScale),
            []
        ),
        Err(ReadError::ArchitectureMismatch { .. })
    ));
    let (part, _) = padding_part();
    assert!(matches!(
        fdri_payload(&part, &Frames::new(100)),
        Err(BitstreamError::WordsPerFrame {
            frames: 100,
            expected: 101
        })
    ));
}

type PacketSummary = (u32, u32, Vec<u32>);

/// `(opcode, register, data)` of every packet (the length for a Type2
/// packet).
fn packet_summary(words: &[u32]) -> Vec<PacketSummary> {
    packet::PacketIter::new(words)
        .map(|p| {
            let data = if p.header_type == 2 {
                vec![p.data.len() as u32]
            } else {
                p.data.to_vec()
            };
            (p.opcode, p.register, data)
        })
        .collect()
}

/// `createConfigurationPackage` of `configuration.cc`, written out
/// independently of the writer: Series7 (`ultrascale == false`) or the
/// UltraScale / UltraScale+ sequence.
fn expected_packets(ultrascale: bool, idcode: u32, payload_len: u32) -> Vec<PacketSummary> {
    let nop = (0, 0, vec![]);
    let w = |reg: u32, v: u32| (OPCODE_WRITE, reg, vec![v]);
    let mut expected = vec![nop.clone()];
    if ultrascale {
        expected.push(nop.clone());
    }
    expected.extend([
        w(register::TIMER, 0),
        w(register::WBSTAR, 0),
        w(register::CMD, command::NOP),
        nop.clone(),
        w(register::CMD, command::RCRC),
        nop.clone(),
        nop.clone(),
    ]);
    if ultrascale {
        expected.extend([
            w(register::FAR, 0),
            w(register::UNKNOWN, 0),
            w(register::COR0, 0x3800_3FE5),
            w(register::COR1, 0x0040_0000),
        ]);
    } else {
        expected.extend([
            w(register::UNKNOWN, 0),
            w(register::COR0, 0x0200_3FE5),
            w(register::COR1, 0),
        ]);
    }
    let (mask, ctl0, final_mask) = if ultrascale {
        (0x1, 0x101, 0x101)
    } else {
        (0x401, 0x501, 0x501)
    };
    expected.extend([
        w(register::IDCODE, idcode),
        w(register::CMD, command::SWITCH),
        nop.clone(),
        w(register::MASK, mask),
        w(register::CTL0, ctl0),
        w(register::MASK, 0),
        w(register::CTL1, 0),
    ]);
    expected.extend(std::iter::repeat_n(nop.clone(), 8));
    expected.extend([
        w(register::FAR, 0),
        w(register::CMD, command::WCFG),
        nop.clone(),
        (OPCODE_WRITE, register::FDRI, vec![]),
        (OPCODE_WRITE, register::FDRI, vec![payload_len]),
        w(register::CMD, command::RCRC),
        nop.clone(),
        nop.clone(),
        w(register::CMD, command::GRESTORE),
        nop.clone(),
        w(register::CMD, command::LFRM),
    ]);
    expected.extend(std::iter::repeat_n(nop.clone(), 100));
    expected.extend([
        w(register::CMD, command::START),
        nop.clone(),
        w(register::FAR, 0x03BE_0000),
        w(register::MASK, final_mask),
        w(register::CTL0, final_mask),
        w(register::CMD, command::RCRC),
        nop.clone(),
        nop.clone(),
        w(register::CMD, command::DESYNC),
    ]);
    expected.extend(std::iter::repeat_n(nop, 400));
    expected
}

/// The whole sequence of §6.3: the packets around the payload.
#[test]
fn packet_sequence() {
    let (part, _) = padding_part();
    let words = configuration_words(&part, &Frames::new(101)).unwrap();
    assert_eq!(&words[..13], &SERIES7_SYNC_HEADER);
    assert_eq!(
        packet_summary(&words[13..]),
        expected_packets(false, 0x1234, 15 * 101)
    );
    assert_eq!(SERIES7_COR0, 0x0200_3FE5);
    assert_eq!(words[13], NOP_HEADER);
}

/// The UltraScale and UltraScale+ sequences (§8.10): two leading NOPs, a
/// `FAR` write before `UNKNOWN`, constant `COR0`/`COR1`, `MASK`/`CTL0`
/// 0x1/0x101, and their own sync headers; the same with the prjxray
/// formats (Series7 parts).
#[test]
fn ultrascale_packet_sequences() {
    let (s7_part, _) = padding_part();
    for arch in [Architecture::UltraScale, Architecture::UltraScalePlus] {
        let part = Part::from_frame_addresses(arch, 0x0484_A093, [FrameAddress(0)]).unwrap();
        let wpf = arch.words_per_frame();
        for (part, format, payload_frames) in [
            (&part, BitstreamFormat::native(arch), 3),
            (&s7_part, BitstreamFormat::prjxray(arch), 15),
        ] {
            let words = configuration_words_with(part, &Frames::new(wpf), &format).unwrap();
            let sync = format.sync_header();
            let expected_sync = if arch == Architecture::UltraScale {
                &ULTRASCALE_SYNC_HEADER[..]
            } else {
                &ULTRASCALE_PLUS_SYNC_HEADER[..]
            };
            assert_eq!(sync, expected_sync);
            assert_eq!(&words[..sync.len()], sync);
            assert_eq!(
                packet_summary(&words[sync.len()..]),
                expected_packets(true, part.idcode, (payload_frames * wpf) as u32),
                "{arch} {format:?}"
            );
        }
    }
    assert_eq!(ULTRASCALE_SYNC_HEADER.len(), 6);
    assert_eq!(ULTRASCALE_PLUS_SYNC_HEADER.len(), 21);
    assert!(ULTRASCALE_PLUS_SYNC_HEADER[..16]
        .iter()
        .all(|&w| w == 0xFFFF_FFFF));
    assert_eq!(
        &ULTRASCALE_PLUS_SYNC_HEADER[16..],
        &SERIES7_SYNC_HEADER[8..]
    );
    assert_eq!(&ULTRASCALE_SYNC_HEADER[1..], &SERIES7_SYNC_HEADER[8..]);
}

fn random_frames(part: &Part, seed: u64, density: u64) -> Frames {
    random_frames_with(part, &BitstreamFormat::native(S7), seed, density)
}

/// Random frames of `format` for `density` percent of the frames of the
/// part, with the ECC bits clear (like `fasm2frames` output).
fn random_frames_with(part: &Part, format: &BitstreamFormat, seed: u64, density: u64) -> Frames {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut frames = Frames::new(format.words_per_frame);
    for address in part.iter_frame_addresses() {
        if next() % 100 >= density {
            continue;
        }
        let frame = frames.get_or_insert_zeroed(address.0);
        for (i, word) in frame.iter_mut().enumerate() {
            if next() % 4 == 0 {
                *word = next() as u32;
            }
            *word &= !format.ecc.ecc_mask(i);
        }
    }
    frames
}

/// A small UltraScale / UltraScale+ part: rows with and without the
/// bottom half bit (the row index includes it), all three buses, a column
/// with the largest minor count of the architecture.
fn ultrascale_part(arch: Architecture) -> Part {
    let max_frames = u32::from(arch.max_minor()) + 1;
    // Every bus in the same rows: `GetNextFrameAddress` only tries the
    // next row of the part, so a row without the bus would end the walk of
    // that bus (see `part_walk_skips_rows_after_a_row_without_the_bus`).
    let addresses: Vec<FrameAddress> = [
        (0u32, 0u32, 0u32, 16u32),
        (0, 0, 1, 76),
        (0, 1, 0, 12),
        (0, 33, 0, 16),
        (0, 33, 2, 6),
        (1, 0, 0, max_frames),
        (1, 1, 0, 128),
        (1, 33, 0, 128),
        (2, 0, 0, 4),
    ]
    .into_iter()
    .flat_map(|(bt, row, column, count)| {
        (0..count)
            .map(move |minor| FrameAddress::compose_row_index(arch, bt, false, row, column, minor))
    })
    .collect();
    Part::from_frame_addresses(arch, 0x0484_A093, addresses).unwrap()
}

/// Random frames -> bit -> frames for the UltraScale / UltraScale+
/// formats of prjuray-tools and prjxray: the frames come back (ECC
/// recomputed with the format's algorithm, padding between rows skipped),
/// rewriting them gives the same bitstream, sparse = dense.
#[test]
fn random_round_trip_ultrascale() {
    let (s7_part, _) = padding_part();
    for arch in [Architecture::UltraScale, Architecture::UltraScalePlus] {
        let us_part = ultrascale_part(arch);
        for (part, format) in [
            (&us_part, BitstreamFormat::native(arch)),
            (&s7_part, BitstreamFormat::prjxray(arch)),
        ] {
            let wpf = format.words_per_frame;
            for (seed, density) in [(11, 0), (12, 10), (13, 100)] {
                let frames = random_frames_with(part, &format, seed, density);
                let options = BitstreamOptions {
                    date: Some("2020/06/15".into()),
                    time: Some("12:34:56".into()),
                    ..Default::default()
                };
                let bytes = bitstream_bytes_with(part, &frames, &options, &format).unwrap();
                let reader = BitstreamReader::from_bytes(&bytes).unwrap();
                let config = reader.configuration_with(part, &format).unwrap();
                assert_eq!(config.len(), part.frame_count(), "{arch} {format:?}");
                assert_eq!(config.words_per_frame(), wpf);
                assert_eq!(config.ecc(), format.ecc);
                for (address, words) in config.frames() {
                    let mut expected = frames
                        .get(address)
                        .map_or_else(|| vec![0; wpf], <[u32]>::to_vec);
                    format.ecc.update(&mut expected);
                    assert_eq!(words, &expected[..]);
                    // The Series7 ECC of a 123-word frame is wider than its 13
                    // stored bits: `verifyECC` fails there (prjxray never
                    // verifies).
                    if format.addressing == arch {
                        assert_eq!(format.ecc.verify(words), Some(true));
                    }
                }
                let back = config.to_frames(true, true);
                let mut nonzero = Frames::new(wpf);
                for (address, words) in frames.iter() {
                    if words.iter().any(|&w| w != 0) {
                        nonzero.insert_if_absent(address, words);
                    }
                }
                assert!(back.diff(&nonzero).is_empty(), "{arch} seed {seed}");
                let again =
                    bitstream_bytes_with(part, &config.to_frames(false, false), &options, &format)
                        .unwrap();
                assert_eq!(again, bytes);
                let mut dense = Frames::zeroed(wpf, part.iter_frame_addresses().map(|a| a.0));
                for (address, words) in frames.iter() {
                    dense.get_mut(address).unwrap().copy_from_slice(words);
                }
                assert_eq!(
                    bitstream_bytes_with(part, &dense, &options, &format).unwrap(),
                    bytes
                );
            }
        }
    }
}

/// The two zero frames of padding of the UltraScale+ payload: after the
/// last frame of every row (the row including the half bit), bus and at
/// the end.
#[test]
fn ultrascale_plus_padding() {
    let arch = Architecture::UltraScalePlus;
    let part = ultrascale_part(arch);
    let payload = fdri_payload(&part, &Frames::new(93)).unwrap();
    // 7 (row, bus) groups: 6 separators of 2 frames and the final padding.
    assert_eq!(part.iter_frame_addresses().count(), part.frame_count());
    assert_eq!(payload.len(), (part.frame_count() + 14) * 93);
    let addresses: Vec<FrameAddress> = part.iter_frame_addresses().collect();
    let breaks = addresses
        .windows(2)
        .filter(|w| !writer_same_row(arch, w[0], w[1]))
        .count();
    assert_eq!(breaks, 6);
}

fn writer_same_row(arch: Architecture, a: FrameAddress, b: FrameAddress) -> bool {
    a.block_type_raw(arch) == b.block_type_raw(arch) && a.row_index(arch) == b.row_index(arch)
}

/// Reference behaviour (`xcupseries::Part::GetNextFrameAddress`): after the
/// last frame of a bus in a row, only the *next row of the part* is tried;
/// if it does not have the bus, the walk continues with the next bus, and
/// the later rows of the bus are never visited, so `xcframes2bit` does not
/// write them (not even zero filled; a frame of the `.frm` there is
/// written, followed by the frames the walk would visit from it).
#[test]
fn part_walk_skips_rows_after_a_row_without_the_bus() {
    let arch = Architecture::UltraScalePlus;
    let a = |bt, row, minor| FrameAddress::compose_row_index(arch, bt, false, row, 0, minor);
    let part =
        Part::from_frame_addresses(arch, 1, [a(0, 0, 0), a(0, 1, 0), a(1, 0, 0), a(1, 2, 0)])
            .unwrap();
    let walk: Vec<FrameAddress> = part.iter_frame_addresses().collect();
    assert_eq!(walk, [a(0, 0, 0), a(0, 1, 0), a(1, 0, 0)]);
    assert_eq!(part.frame_count(), 4);
    let mut frames = Frames::new(93);
    frames.insert_if_absent(a(1, 2, 0).0, &[5; 93]);
    let payload = fdri_payload(&part, &frames).unwrap();
    // 4 frames, separators after rows 0 and 1 of CLB, after BRAM row 0
    // (its next address is BRAM row 2), and at the end.
    assert_eq!(payload.len(), (4 + 6) * 93);
    assert_eq!(payload[(3 + 4) * 93], 5);
}

/// Property: random frames -> bit -> frames gives the frames back (every
/// frame of the part, ECC recomputed; with the ECC cleared and zero
/// frames skipped exactly the input).
#[test]
fn random_round_trip_small_part() {
    let addresses: Vec<FrameAddress> = [
        (BlockType::ClbIoClk, false, 0u8, 0u16, 36u16),
        (BlockType::ClbIoClk, false, 0, 1, 28),
        (BlockType::ClbIoClk, false, 1, 0, 36),
        (BlockType::ClbIoClk, true, 0, 0, 36),
        (BlockType::ClbIoClk, true, 0, 3, 2),
        (BlockType::BlockRam, false, 0, 0, 128),
        (BlockType::BlockRam, true, 0, 0, 128),
        (BlockType::CfgClb, false, 0, 0, 3),
    ]
    .into_iter()
    .flat_map(|(bt, bottom, row, column, count)| {
        (0..count).map(move |minor| fa(bt, bottom, row, column, minor))
    })
    .collect();
    let part = Part::from_frame_addresses(S7, 0x0362_D093, addresses).unwrap();
    for (seed, density) in [(1, 0), (2, 5), (3, 50), (4, 100)] {
        let frames = random_frames(&part, seed, density);
        let options = BitstreamOptions {
            design_name: b"random.frm".to_vec(),
            part_name: b"xc7test".to_vec(),
            date: Some("2000/01/01".into()),
            time: Some("00:00:00".into()),
            ..Default::default()
        };
        let bytes = bitstream_bytes(&part, &frames, &options).unwrap();
        let header = BitHeader::parse(&bytes).unwrap();
        assert_eq!(header.design, b"random.frm;Generator=xc7frames2bit");
        assert_eq!(
            header.data_length as usize,
            bytes.len() - header.header_length
        );
        let reader = BitstreamReader::from_bytes(&bytes).unwrap();
        assert_eq!(reader.trailing_bytes(), 0);
        let config = reader.configuration(&part).unwrap();
        assert_eq!(config.len(), part.frame_count());
        let back = config.to_frames(true, true);
        let mut nonzero = Frames::new(101);
        for (address, words) in frames.iter() {
            if words.iter().any(|&w| w != 0) {
                nonzero.insert_if_absent(address, words);
            }
        }
        assert!(back.diff(&nonzero).is_empty(), "seed {seed}");
        // The ECC is in the read frames.
        for (address, words) in config.frames() {
            let mut expected = frames
                .get(address)
                .map_or_else(|| vec![0; 101], <[u32]>::to_vec);
            ecc::update_ecc(&mut expected);
            assert_eq!(words, &expected[..]);
        }
        // Writing the read frames again gives the same bitstream.
        let again = bitstream_bytes(&part, &config.to_frames(false, false), &options).unwrap();
        assert_eq!(again, bytes);
        // Sparse and dense input give the same bitstream.
        let mut dense = Frames::zeroed(101, part.iter_frame_addresses().map(|a| a.0));
        for (address, words) in frames.iter() {
            dense.get_mut(address).unwrap().copy_from_slice(words);
        }
        assert_eq!(bitstream_bytes(&part, &dense, &options).unwrap(), bytes);
    }
}

/// The reader never panics on malformed input and never returns frames
/// longer than a frame.
#[test]
fn reader_fuzz() {
    let (part, _) = padding_part();
    let frames = random_frames(&part, 7, 100);
    let options = BitstreamOptions {
        date: Some(String::new()),
        time: Some(String::new()),
        ..Default::default()
    };
    let good = bitstream_bytes(&part, &frames, &options).unwrap();
    let mut state = 0x1234_5678_9ABC_DEF1_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for round in 0..3000 {
        let mut bytes = if round % 3 == 0 {
            // Random words after a sync word.
            let mut b = SYNC_WORD.to_vec();
            for _ in 0..(next() % 64) {
                let r = next() as u32;
                let w = match r % 4 {
                    0 => type1_header(OPCODE_WRITE, (r >> 8) % 32, (r >> 16) % 4),
                    1 => type2_header(OPCODE_WRITE, (r >> 8) % 300),
                    2 => r,
                    _ => 0,
                };
                b.extend_from_slice(&w.to_be_bytes());
            }
            b
        } else {
            good.clone()
        };
        // Mutations.
        for _ in 0..(next() % 8) {
            if bytes.is_empty() {
                break;
            }
            let i = (next() as usize) % bytes.len();
            match next() % 3 {
                0 => bytes[i] = next() as u8,
                1 => bytes.truncate(i),
                _ => {
                    bytes.remove(i);
                }
            }
        }
        let _ = BitHeader::parse(&bytes);
        if let Some(reader) = BitstreamReader::from_bytes(&bytes) {
            if let Ok(config) = reader.configuration(&part) {
                assert!(config.frames().all(|(_, w)| w.len() <= 101));
                let _ = config.to_frames(true, true);
            }
        }
    }
}

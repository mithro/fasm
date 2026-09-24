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
fn unsupported_architecture_and_word_count() {
    let part =
        Part::from_frame_addresses(Architecture::UltraScalePlus, 1, [FrameAddress(0)]).unwrap();
    assert!(matches!(
        fdri_payload(&part, &Frames::new(93)),
        Err(BitstreamError::UnsupportedArchitecture(
            Architecture::UltraScalePlus
        ))
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

/// The whole sequence of §6.3: the packets around the payload.
#[test]
fn packet_sequence() {
    let (part, _) = padding_part();
    let words = configuration_words(&part, &Frames::new(101)).unwrap();
    assert_eq!(&words[..13], &SERIES7_SYNC_HEADER);
    let packets: Vec<Packet<'_>> = packet::PacketIter::new(&words[13..]).collect();
    let summary: Vec<(u32, u32, Vec<u32>)> = packets
        .iter()
        .map(|p| {
            let data = if p.header_type == 2 {
                vec![p.data.len() as u32]
            } else {
                p.data.to_vec()
            };
            (p.opcode, p.register, data)
        })
        .collect();
    let nop = (0, 0, vec![]);
    let w = |reg: u32, v: u32| (OPCODE_WRITE, reg, vec![v]);
    let mut expected = vec![
        nop.clone(),
        w(register::TIMER, 0),
        w(register::WBSTAR, 0),
        w(register::CMD, command::NOP),
        nop.clone(),
        w(register::CMD, command::RCRC),
        nop.clone(),
        nop.clone(),
        w(register::UNKNOWN, 0),
        w(register::COR0, 0x0200_3FE5),
        w(register::COR1, 0),
        w(register::IDCODE, 0x1234),
        w(register::CMD, command::SWITCH),
        nop.clone(),
        w(register::MASK, 0x401),
        w(register::CTL0, 0x501),
        w(register::MASK, 0),
        w(register::CTL1, 0),
    ];
    expected.extend(std::iter::repeat_n(nop.clone(), 8));
    expected.extend([
        w(register::FAR, 0),
        w(register::CMD, command::WCFG),
        nop.clone(),
        (OPCODE_WRITE, register::FDRI, vec![]),
        (OPCODE_WRITE, register::FDRI, vec![15 * 101]),
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
        w(register::MASK, 0x501),
        w(register::CTL0, 0x501),
        w(register::CMD, command::RCRC),
        nop.clone(),
        nop.clone(),
        w(register::CMD, command::DESYNC),
    ]);
    expected.extend(std::iter::repeat_n(nop, 400));
    assert_eq!(summary, expected);
    assert_eq!(SERIES7_COR0, 0x0200_3FE5);
    assert_eq!(words[13], NOP_HEADER);
}

fn random_frames(part: &Part, seed: u64, density: u64) -> Frames {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut frames = Frames::new(101);
    for address in part.iter_frame_addresses() {
        if next() % 100 >= density {
            continue;
        }
        let frame = frames.get_or_insert_zeroed(address.0);
        for word in frame.iter_mut() {
            if next() % 4 == 0 {
                *word = next() as u32;
            }
        }
        frame[50] &= !0x1FFF;
    }
    frames
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

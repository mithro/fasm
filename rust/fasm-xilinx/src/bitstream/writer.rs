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

//! Frames -> `.bit`, exactly like prjxray's `xc7frames2bit` and
//! prjuray-tools' `xcframes2bit` (design document §6.1-§6.5, §8.10).

use std::fmt;
use std::io::{self, Write};

use super::ecc::Ecc;
use super::header::{create_header, now_utc_date_time};
use super::packet::{command, register, type1_header, type2_header, NOP_HEADER, OPCODE_WRITE};
use crate::arch::{Architecture, FrameAddress};
use crate::frames::Frames;
use crate::part::Part;

/// The words before the first packet (`BitstreamWriter<Series7>::header_`,
/// UG470 pg. 80: bus width auto detection and the sync word).
pub const SERIES7_SYNC_HEADER: [u32; 13] = [
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0x0000_00BB,
    0x1122_0044,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xAA99_5566,
];

/// `BitstreamWriter<UltraScale>::header_`: one `0xFFFFFFFF` word before
/// the bus width detection pattern.
pub const ULTRASCALE_SYNC_HEADER: [u32; 6] = [
    0xFFFF_FFFF,
    0x0000_00BB,
    0x1122_0044,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xAA99_5566,
];

/// `BitstreamWriter<UltraScalePlus>::header_`: sixteen `0xFFFFFFFF` words
/// before the bus width detection pattern.
pub const ULTRASCALE_PLUS_SYNC_HEADER: [u32; 21] = [
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0x0000_00BB,
    0x1122_0044,
    0xFFFF_FFFF,
    0xFFFF_FFFF,
    0xAA99_5566,
];

/// The `COR0` value of the reference sequence
/// (`ConfigurationOptions0Value` with `AddPipelineStageForDoneIn`, done
/// released in phase 4, no DCI/MMCM wait, GTS in phase 5, GWE in phase 6).
pub const SERIES7_COR0: u32 = (1 << 25) // AddPipelineStageForDoneIn
    | (3 << 12) // ReleaseDonePinAtStartupCycle = Phase4
    | (7 << 9) // StallAtStartupCycleUntilDciMatch = NoWait
    | (7 << 6) // StallAtStartupCycleUntilMmcmLock = NoWait
    | (4 << 3) // ReleaseGtsSignalAtStartupCycle = Phase5
    | 5; // ReleaseGweSignalAtStartupCycle = Phase6

/// The `COR0` value of the UltraScale and UltraScale+ sequences (a
/// constant in `configuration.cc`).
pub const ULTRASCALE_COR0: u32 = 0x3800_3FE5;

/// The `COR1` value of the UltraScale and UltraScale+ sequences.
pub const ULTRASCALE_COR1: u32 = 0x0040_0000;

/// How a bitstream is laid out: which `--architecture` of the reference
/// tools, and which of their implementations of it.
///
/// prjuray-tools (`xcframes2bit`, its `bitread`) has a part type, frame
/// address layout and ECC per architecture ([`BitstreamFormat::native`]).
/// The plain prjxray checkout (`xc7frames2bit`, `bitread`) declares
/// UltraScale and UltraScale+ with the Series7 part type, frame address
/// layout and ECC, only with their word counts, sync headers and packet
/// sequences ([`BitstreamFormat::prjxray`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BitstreamFormat {
    /// The `--architecture`: sync header and packet sequence.
    pub architecture: Architecture,
    /// The part type and frame address layout: the architecture the
    /// [`Part`] must have.
    pub addressing: Architecture,
    /// 32-bit words per frame.
    pub words_per_frame: usize,
    /// The frame ECC.
    pub ecc: Ecc,
}

impl BitstreamFormat {
    /// prjuray-tools' `arch` (and prjxray's for Series7): everything of
    /// `arch`.
    pub const fn native(arch: Architecture) -> Self {
        BitstreamFormat {
            architecture: arch,
            addressing: arch,
            words_per_frame: arch.words_per_frame(),
            ecc: Ecc::of(arch),
        }
    }

    /// prjxray's `arch`: the Series7 part type, frame address layout and
    /// ECC with the word count, sync header and packet sequence of `arch`
    /// (the same as [`BitstreamFormat::native`] for Series7).
    pub const fn prjxray(arch: Architecture) -> Self {
        BitstreamFormat {
            architecture: arch,
            addressing: Architecture::Series7,
            words_per_frame: arch.words_per_frame(),
            ecc: Ecc::Series7,
        }
    }

    /// The words before the first packet.
    pub const fn sync_header(&self) -> &'static [u32] {
        match self.architecture {
            Architecture::Series7 => &SERIES7_SYNC_HEADER,
            Architecture::UltraScale => &ULTRASCALE_SYNC_HEADER,
            Architecture::UltraScalePlus => &ULTRASCALE_PLUS_SYNC_HEADER,
        }
    }
}

/// The fields of the `.bit` header (design document §6.1).
///
/// `xc7frames2bit` (and prjuray's `xcframes2bit`) writes
/// `<frm_file>;Generator=xc7frames2bit` into field `a`, its `--part_name`
/// into `b` and the current UTC date and time into `c` and `d`.
/// `date`/`time` of `None` use the current UTC time (like the reference);
/// set both for reproducible output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitstreamOptions {
    /// The source name (the `.frm` file path for `xc7frames2bit`).
    pub design_name: Vec<u8>,
    /// The generator name after `;Generator=` (`xc7frames2bit`).
    pub generator: Vec<u8>,
    /// The part name (field `b`), written as given.
    pub part_name: Vec<u8>,
    /// Field `c`, `YYYY/MM/DD` (see [`super::utc_date_time`]).
    pub date: Option<String>,
    /// Field `d`, `HH:MM:SS`.
    pub time: Option<String>,
}

impl Default for BitstreamOptions {
    fn default() -> Self {
        BitstreamOptions {
            design_name: Vec::new(),
            generator: b"xc7frames2bit".to_vec(),
            part_name: Vec::new(),
            date: None,
            time: None,
        }
    }
}

/// Why a bitstream cannot be written.
#[derive(Debug)]
pub enum BitstreamError {
    /// The part is not of the format's part type
    /// ([`BitstreamFormat::addressing`]).
    ArchitectureMismatch {
        /// The part's architecture.
        part: Architecture,
        /// The part type of the format.
        format: Architecture,
    },
    /// The frames do not have the format's number of words per frame.
    WordsPerFrame {
        /// Words per frame of the frames.
        frames: usize,
        /// Words per frame of the format.
        expected: usize,
    },
    /// Writing the output failed.
    Io(io::Error),
}

impl fmt::Display for BitstreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BitstreamError::ArchitectureMismatch { part, format } => write!(
                f,
                "the part is a {part} part, the bitstream format needs a {format} part"
            ),
            BitstreamError::WordsPerFrame { frames, expected } => write!(
                f,
                "the frames have {frames} words per frame instead of {expected}"
            ),
            BitstreamError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BitstreamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BitstreamError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for BitstreamError {
    fn from(e: io::Error) -> Self {
        BitstreamError::Io(e)
    }
}

fn check(part: &Part, frames: &Frames, format: &BitstreamFormat) -> Result<(), BitstreamError> {
    if part.architecture != format.addressing {
        return Err(BitstreamError::ArchitectureMismatch {
            part: part.architecture,
            format: format.addressing,
        });
    }
    if frames.words_per_frame() != format.words_per_frame {
        return Err(BitstreamError::WordsPerFrame {
            frames: frames.words_per_frame(),
            expected: format.words_per_frame,
        });
    }
    Ok(())
}

/// `true` if the two addresses are in the same row of the same half and
/// block type (no zero frames between them).
pub(crate) fn same_row(arch: Architecture, a: FrameAddress, b: FrameAddress) -> bool {
    a.block_type_raw(arch) == b.block_type_raw(arch)
        && a.is_bottom_half(arch) == b.is_bottom_half(arch)
        && a.row_index(arch) == b.row_index(arch)
}

/// [`fdri_payload_with`] in the part's [`BitstreamFormat::native`] format.
///
/// # Errors
///
/// See [`fdri_payload_with`].
pub fn fdri_payload(part: &Part, frames: &Frames) -> Result<Vec<u32>, BitstreamError> {
    fdri_payload_with(part, frames, &BitstreamFormat::native(part.architecture))
}

/// The frame data of the `FDRI` Type2 packet, exactly like `xc7frames2bit`
/// (all architectures share this code, `configuration.h`):
///
/// 1. every frame of the part missing from `frames` is added zero filled
///    (`Frames::addMissingFrames`: the walk of
///    [`Part::iter_frame_addresses`]); frames of `frames` that are not in
///    the part are kept, in address order;
/// 2. the ECC of every frame is recomputed with the format's ECC
///    (`Frames::readFrames` does it for the frames of the `.frm` file; the
///    added zero frames have an ECC of zero);
/// 3. the frames are concatenated in ascending address order, with two
///    zero frames after every frame whose next frame
///    ([`Part::next_frame_address`]) is in another row, half or block
///    type, and two more zero frames at the end
///    (`createType2ConfigurationPacketData`).
///
/// # Errors
///
/// [`BitstreamError::ArchitectureMismatch`] if the part is not of the
/// format's part type, [`BitstreamError::WordsPerFrame`] if the frames do
/// not have the format's word count.
pub fn fdri_payload_with(
    part: &Part,
    frames: &Frames,
    format: &BitstreamFormat,
) -> Result<Vec<u32>, BitstreamError> {
    check(part, frames, format)?;
    let arch = part.architecture;
    let wpf = format.words_per_frame;
    let ecc = format.ecc;
    let separator = 2 * wpf;
    let mut payload: Vec<u32> = Vec::with_capacity((part.frame_count() + 64) * wpf);
    let push_frame = |payload: &mut Vec<u32>, address: u32, words: Option<&[u32]>| {
        let start = payload.len();
        match words {
            Some(words) => {
                payload.extend_from_slice(words);
                ecc.update(&mut payload[start..]);
            }
            None => payload.resize(start + wpf, 0),
        }
        let address = FrameAddress(address);
        if let Some(next) = part.next_frame_address(address) {
            if !same_row(arch, address, next) {
                payload.resize(payload.len() + separator, 0);
            }
        }
    };
    // Merge the part's addresses (ascending) with the frames' (ascending).
    let mut given = frames.iter().peekable();
    for address in part.iter_frame_addresses() {
        let address = address.0;
        while let Some(&(a, words)) = given.peek() {
            if a >= address {
                break;
            }
            given.next();
            push_frame(&mut payload, a, Some(words));
        }
        match given.peek() {
            Some(&(a, words)) if a == address => {
                given.next();
                push_frame(&mut payload, a, Some(words));
            }
            _ => push_frame(&mut payload, address, None),
        }
    }
    for (a, words) in given {
        push_frame(&mut payload, a, Some(words));
    }
    payload.resize(payload.len() + separator, 0);
    Ok(payload)
}

/// [`configuration_words_with`] in the part's [`BitstreamFormat::native`]
/// format.
///
/// # Errors
///
/// See [`fdri_payload_with`].
pub fn configuration_words(part: &Part, frames: &Frames) -> Result<Vec<u32>, BitstreamError> {
    configuration_words_with(part, frames, &BitstreamFormat::native(part.architecture))
}

/// The configuration words of a bitstream after the `.bit` header: the
/// sync header ([`BitstreamFormat::sync_header`]) and the packet sequence
/// of `Configuration<ArchType>::createConfigurationPackage` (design
/// document §6.3; UltraScale and UltraScale+ share theirs, §8.10) around
/// the [`fdri_payload_with`], with the part's IDCODE. No CRC is computed
/// (the sequences only reset it with `RCRC`).
///
/// # Errors
///
/// See [`fdri_payload_with`].
pub fn configuration_words_with(
    part: &Part,
    frames: &Frames,
    format: &BitstreamFormat,
) -> Result<Vec<u32>, BitstreamError> {
    let payload = fdri_payload_with(part, frames, format)?;
    let ultrascale = format.architecture != Architecture::Series7;
    let mut w: Vec<u32> = Vec::with_capacity(payload.len() + 700);
    w.extend_from_slice(format.sync_header());
    let write = |w: &mut Vec<u32>, reg: u32, value: u32| {
        w.push(type1_header(OPCODE_WRITE, reg, 1));
        w.push(value);
    };
    let nops = |w: &mut Vec<u32>, n: usize| w.extend(std::iter::repeat_n(NOP_HEADER, n));
    // Initialization sequence.
    nops(&mut w, if ultrascale { 2 } else { 1 });
    write(&mut w, register::TIMER, 0);
    write(&mut w, register::WBSTAR, 0);
    write(&mut w, register::CMD, command::NOP);
    nops(&mut w, 1);
    write(&mut w, register::CMD, command::RCRC);
    nops(&mut w, 2);
    if ultrascale {
        write(&mut w, register::FAR, 0);
    }
    write(&mut w, register::UNKNOWN, 0);
    if ultrascale {
        write(&mut w, register::COR0, ULTRASCALE_COR0);
        write(&mut w, register::COR1, ULTRASCALE_COR1);
    } else {
        write(&mut w, register::COR0, SERIES7_COR0);
        write(&mut w, register::COR1, 0);
    }
    write(&mut w, register::IDCODE, part.idcode);
    write(&mut w, register::CMD, command::SWITCH);
    nops(&mut w, 1);
    let (mask, ctl0) = if ultrascale {
        (0x1, 0x101)
    } else {
        (0x401, 0x501)
    };
    write(&mut w, register::MASK, mask);
    write(&mut w, register::CTL0, ctl0);
    write(&mut w, register::MASK, 0);
    write(&mut w, register::CTL1, 0);
    nops(&mut w, 8);
    write(&mut w, register::FAR, 0);
    write(&mut w, register::CMD, command::WCFG);
    nops(&mut w, 1);
    // Frame data write.
    w.push(type1_header(OPCODE_WRITE, register::FDRI, 0));
    w.push(type2_header(OPCODE_WRITE, payload.len() as u32));
    w.extend_from_slice(&payload);
    drop(payload);
    // Finalization sequence.
    write(&mut w, register::CMD, command::RCRC);
    nops(&mut w, 2);
    write(&mut w, register::CMD, command::GRESTORE);
    nops(&mut w, 1);
    write(&mut w, register::CMD, command::LFRM);
    nops(&mut w, 100);
    write(&mut w, register::CMD, command::START);
    nops(&mut w, 1);
    write(&mut w, register::FAR, 0x03BE_0000);
    let final_mask = if ultrascale { 0x101 } else { 0x501 };
    write(&mut w, register::MASK, final_mask);
    write(&mut w, register::CTL0, final_mask);
    write(&mut w, register::CMD, command::RCRC);
    nops(&mut w, 2);
    write(&mut w, register::CMD, command::DESYNC);
    nops(&mut w, 400);
    Ok(w)
}

/// [`bitstream_bytes_with`] in the part's [`BitstreamFormat::native`]
/// format.
///
/// # Errors
///
/// See [`fdri_payload_with`].
pub fn bitstream_bytes(
    part: &Part,
    frames: &Frames,
    options: &BitstreamOptions,
) -> Result<Vec<u8>, BitstreamError> {
    bitstream_bytes_with(
        part,
        frames,
        options,
        &BitstreamFormat::native(part.architecture),
    )
}

/// The complete `.bit` file: the header of `options` with the length of
/// the data in field `e`, then the [`configuration_words_with`]
/// big-endian. Byte for byte what `xc7frames2bit` / `xcframes2bit` write
/// for the same frames, part, part name, `.frm` file name, time and
/// format.
///
/// # Errors
///
/// See [`fdri_payload_with`].
pub fn bitstream_bytes_with(
    part: &Part,
    frames: &Frames,
    options: &BitstreamOptions,
    format: &BitstreamFormat,
) -> Result<Vec<u8>, BitstreamError> {
    let words = configuration_words_with(part, frames, format)?;
    let (date, time) = match (&options.date, &options.time) {
        (Some(date), Some(time)) => (date.clone(), time.clone()),
        (date, time) => {
            let (now_date, now_time) = now_utc_date_time();
            (
                date.clone().unwrap_or(now_date),
                time.clone().unwrap_or(now_time),
            )
        }
    };
    let mut out = create_header(
        &options.design_name,
        &options.generator,
        &options.part_name,
        date.as_bytes(),
        time.as_bytes(),
    );
    let header_len = out.len();
    let data_len = words.len() * 4;
    out.reserve(data_len);
    for word in &words {
        out.extend_from_slice(&word.to_be_bytes());
    }
    // `length_of_data` is a `uint32_t`.
    out[header_len - 4..header_len].copy_from_slice(&(data_len as u32).to_be_bytes());
    Ok(out)
}

/// Writes [`bitstream_bytes`] to `out`.
///
/// # Errors
///
/// See [`fdri_payload_with`]; [`BitstreamError::Io`] for write errors.
pub fn write_bitstream(
    part: &Part,
    frames: &Frames,
    options: &BitstreamOptions,
    out: &mut dyn Write,
) -> Result<(), BitstreamError> {
    let bytes = bitstream_bytes(part, frames, options)?;
    out.write_all(&bytes)?;
    Ok(())
}

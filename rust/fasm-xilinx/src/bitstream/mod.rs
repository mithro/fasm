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

//! Frames <-> `.bit` bitstreams for Series7 (design document §6):
//!
//! * the writer ([`bitstream_bytes`], [`write_bitstream`]) is prjxray's
//!   `xc7frames2bit`: the `.bit` header ([`BitstreamOptions`]), the sync
//!   words, the packet sequence with the part's IDCODE, and all the frames
//!   of the part (missing ones zero filled) with their ECC and the zero
//!   frame padding between rows, in one `FDRI` write;
//! * the reader ([`BitstreamReader`], [`Configuration`]) is prjxray's
//!   `BitstreamReader` + `Configuration::InitWithPackets`, used by
//!   `bitread`: packets are replayed on the part to find the frames;
//! * [`ecc`] is the Series7 frame ECC, [`packet`] the packet format.
//!
//! UltraScale and UltraScale+ (T6.2) are reported as unsupported.

pub mod ecc;
mod header;
pub mod packet;
mod reader;
mod writer;

pub use header::{now_utc_date_time, utc_date_time, BitHeader, BIT_HEADER_PREAMBLE};
pub use reader::{BitstreamReader, Configuration, ReadError, SYNC_WORD};
pub use writer::{
    bitstream_bytes, configuration_words, fdri_payload, write_bitstream, BitstreamError,
    BitstreamOptions, SERIES7_COR0, SERIES7_SYNC_HEADER,
};

#[cfg(test)]
mod tests;

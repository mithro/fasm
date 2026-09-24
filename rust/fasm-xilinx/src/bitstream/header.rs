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

//! The `.bit` file header (`BitstreamWriter::create_header`, design
//! document §6.1): a fixed preamble and the TLV fields `a` (source and
//! generator), `b` (part), `c` (date), `d` (time) and `e` (length of the
//! configuration data).

/// The fixed 14 byte start of the header, up to the `a` field tag.
pub const BIT_HEADER_PREAMBLE: [u8; 14] = [
    0x00, 0x09, 0x0f, 0xf0, 0x0f, 0xf0, 0x0f, 0xf0, 0x0f, 0xf0, 0x00, 0x00, 0x01, b'a',
];

/// The decoded fields of a `.bit` header.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BitHeader {
    /// Field `a` without its NUL: `<frames file>;Generator=<generator>`
    /// for files written by `xc7frames2bit`.
    pub design: Vec<u8>,
    /// Field `b`: the part name.
    pub part: Vec<u8>,
    /// Field `c`: the date, `YYYY/MM/DD`.
    pub date: Vec<u8>,
    /// Field `d`: the time, `HH:MM:SS`.
    pub time: Vec<u8>,
    /// Field `e`: the length in bytes of the data after the header.
    pub data_length: u32,
    /// The length of the header in bytes (the offset of the data).
    pub header_length: usize,
}

impl BitHeader {
    /// Parses the header at the start of `bytes`: the preamble of
    /// [`BIT_HEADER_PREAMBLE`] (the tag `a` included), then the fields `a`
    /// to `d` (a big-endian `u16` length including a trailing NUL, then
    /// the value) and `e` (a big-endian `u32`). Returns `None` if the bytes
    /// do not have this shape.
    pub fn parse(bytes: &[u8]) -> Option<BitHeader> {
        let rest = bytes.strip_prefix(&BIT_HEADER_PREAMBLE[..13])?;
        let mut rest = rest;
        let mut field = |tag: u8| -> Option<Vec<u8>> {
            let (&t, tail) = rest.split_first()?;
            if t != tag || tail.len() < 2 {
                return None;
            }
            let len = usize::from(u16::from_be_bytes([tail[0], tail[1]]));
            let value = tail.get(2..2 + len)?;
            rest = &tail[2 + len..];
            let value = value.strip_suffix(&[0]).unwrap_or(value);
            Some(value.to_vec())
        };
        let design = field(b'a')?;
        let part = field(b'b')?;
        let date = field(b'c')?;
        let time = field(b'd')?;
        let (&e, tail) = rest.split_first()?;
        if e != b'e' || tail.len() < 4 {
            return None;
        }
        let data_length = u32::from_be_bytes([tail[0], tail[1], tail[2], tail[3]]);
        let header_length = bytes.len() - (tail.len() - 4);
        Some(BitHeader {
            design,
            part,
            date,
            time,
            data_length,
            header_length,
        })
    }

    /// The `(date, time)` of the header as strings (lossy).
    pub fn date_time(&self) -> (String, String) {
        (
            String::from_utf8_lossy(&self.date).into_owned(),
            String::from_utf8_lossy(&self.time).into_owned(),
        )
    }
}

/// Appends one TLV field like `create_header`: the tag, the length of the
/// value plus its NUL as a big-endian `u16` (truncated like the
/// reference's `static_cast<uint8_t>`s) and the value with a NUL.
fn push_field(out: &mut Vec<u8>, tag: Option<u8>, value: &[u8]) {
    if let Some(tag) = tag {
        out.push(tag);
    }
    let len = value.len().wrapping_add(1);
    out.push((len >> 8) as u8);
    out.push(len as u8);
    out.extend_from_slice(value);
    out.push(0);
}

/// `BitstreamWriter::create_header`: the header bytes up to and including
/// the `e` tag and its (zero) 4 byte length placeholder.
pub(crate) fn create_header(
    source: &[u8],
    generator: &[u8],
    part: &[u8],
    date: &[u8],
    time: &[u8],
) -> Vec<u8> {
    let mut out = BIT_HEADER_PREAMBLE.to_vec();
    let mut build_source = source.to_vec();
    build_source.extend_from_slice(b";Generator=");
    build_source.extend_from_slice(generator);
    push_field(&mut out, None, &build_source);
    push_field(&mut out, Some(b'b'), part);
    push_field(&mut out, Some(b'c'), date);
    push_field(&mut out, Some(b'd'), time);
    out.extend_from_slice(&[b'e', 0, 0, 0, 0]);
    out
}

/// The header date and time of `unix_seconds` like the reference formats
/// `absl::Now()`: `("%E4Y/%m/%d", "%H:%M:%S")` in UTC.
pub fn utc_date_time(unix_seconds: i64) -> (String, String) {
    let days = unix_seconds.div_euclid(86_400);
    let secs = unix_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let year = if year < 0 {
        format!("-{:03}", -year)
    } else {
        format!("{year:04}")
    };
    (
        format!("{year}/{month:02}/{day:02}"),
        format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            (secs / 60) % 60,
            secs % 60
        ),
    )
}

/// The current time as [`utc_date_time`] formats it.
pub fn now_utc_date_time() -> (String, String) {
    let now = std::time::SystemTime::now();
    let seconds = match now.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX) - 1,
    };
    utc_date_time(seconds)
}

/// Howard Hinnant's `civil_from_days`: (year, month, day) of a day count
/// since 1970-01-01 in the proleptic Gregorian calendar.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(utc_date_time(0), ("1970/01/01".into(), "00:00:00".into()));
        assert_eq!(
            utc_date_time(951_782_400),
            ("2000/02/29".into(), "00:00:00".into())
        );
        // 2026-09-24 01:44:56 UTC, the smoke corpus header.
        assert_eq!(
            utc_date_time(1_790_214_296),
            ("2026/09/24".into(), "01:44:56".into())
        );
        assert_eq!(utc_date_time(-1), ("1969/12/31".into(), "23:59:59".into()));
        assert_eq!(
            utc_date_time(253_402_300_799),
            ("9999/12/31".into(), "23:59:59".into())
        );
    }

    #[test]
    fn header_round_trip() {
        let bytes = create_header(
            b"a.frm",
            b"xc7frames2bit",
            b"xc7a35t",
            b"2026/09/24",
            b"01:02:03",
        );
        let mut expected = BIT_HEADER_PREAMBLE.to_vec();
        expected.extend_from_slice(b"\x00\x1ea.frm;Generator=xc7frames2bit\x00");
        expected.extend_from_slice(b"b\x00\x08xc7a35t\x00");
        expected.extend_from_slice(b"c\x00\x0b2026/09/24\x00");
        expected.extend_from_slice(b"d\x00\x0901:02:03\x00");
        expected.extend_from_slice(b"e\x00\x00\x00\x00");
        assert_eq!(bytes, expected);
        let header = BitHeader::parse(&bytes).unwrap();
        assert_eq!(header.design, b"a.frm;Generator=xc7frames2bit");
        assert_eq!(header.part, b"xc7a35t");
        assert_eq!(header.date, b"2026/09/24");
        assert_eq!(header.time, b"01:02:03");
        assert_eq!(header.data_length, 0);
        assert_eq!(header.header_length, bytes.len());
        for len in 0..bytes.len() {
            assert_eq!(BitHeader::parse(&bytes[..len]), None);
        }
    }
}

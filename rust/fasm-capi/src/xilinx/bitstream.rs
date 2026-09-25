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

//! [`fasm_xilinx_part`] and the bitstream writer and reader.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

use fasm_xilinx::bitstream::{
    bitstream_bytes_with, utc_date_time, BitstreamFormat, BitstreamOptions, BitstreamReader,
};
use fasm_xilinx::{Architecture, Database, Part};

use super::database::{database_arg, fasm_xilinx_database};
use super::frames::{fasm_xilinx_frames, frames_arg, frames_into_raw};
use super::{
    bytes_into_raw, db_error, fasm_bytes, fasm_xilinx_architecture, opt_str, path_arg, set_out,
};
use crate::error::{fasm_error, fasm_status, CapiError};
use crate::ffi::{bytes, guard, run_status};

/// The frame tree and IDCODE of a part (`part.yaml`), which a bitstream is
/// written for and read with.
///
/// Created by `fasm_xilinx_part_from_database` or
/// `fasm_xilinx_part_read_yaml`, released with `fasm_xilinx_part_free`.
/// Immutable: any number of threads may use it at once.
pub struct fasm_xilinx_part {
    _private: [u8; 0],
}

/// The Rust side of a [`fasm_xilinx_part`].
enum PartInner {
    /// The part of a database (and its name, the default header part
    /// name).
    Database(Arc<Database>),
    File(Part),
}

impl PartInner {
    fn part(&self) -> &Part {
        match self {
            PartInner::Database(db) => db.part().expect("checked on creation"),
            PartInner::File(part) => part,
        }
    }

    fn name(&self) -> Option<&str> {
        match self {
            PartInner::Database(db) => db.part_info().map(|i| i.name.as_str()),
            PartInner::File(_) => None,
        }
    }
}

/// # Safety
///
/// `part` must be `NULL` or live.
unsafe fn part_arg<'a>(part: *const fasm_xilinx_part) -> Result<&'a PartInner, CapiError> {
    // SAFETY: a non-NULL `part` came from `Box::into_raw` and is live.
    unsafe { part.cast::<PartInner>().as_ref() }.ok_or_else(|| CapiError::null("part"))
}

/// Stores the part of `db` (its `part.yaml`, else `part.json` frame tree)
/// in `*out` (free with `fasm_xilinx_part_free`; it shares the ownership
/// of `db`). `FASM_ERR_DB` if the part has no frame tree.
///
/// # Safety
///
/// `db` must be `NULL` or live; `out` and `err` `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_part_from_database(
    db: *const fasm_xilinx_database,
    out: *mut *mut fasm_xilinx_part,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        set_out(out, ptr::null_mut());
        run_status(err, || {
            if out.is_null() {
                return Err(CapiError::null("out"));
            }
            let db = &database_arg(db)?.db;
            if db.part().is_none() {
                let name = db.part_info().map_or("", |i| i.name.as_str());
                return Err(CapiError::new(
                    fasm_status::FASM_ERR_DB,
                    format!(
                        "{}: part {name:?} has no frame tree (part.yaml or part.json)",
                        db.root().display()
                    ),
                )
                .with_kind("fasm_xilinx.DbError"));
            }
            let inner = Box::new(PartInner::Database(Arc::clone(db)));
            set_out(out, Box::into_raw(inner).cast::<fasm_xilinx_part>());
            Ok(())
        })
    }
}

/// Reads the part file `path` (a `part.yaml`, NUL terminated) like the
/// `--part_file` of `xc7frames2bit` / `xcframes2bit`: a file tagged with
/// an architecture (`!<xilinx/xc7series/part>`, `xcuseries`,
/// `xcupseries`) is of that architecture, an untagged one is read as
/// `architecture` (a `fasm_xilinx_architecture` value). The part is
/// stored in `*out`. `FASM_ERR_DB` if it cannot be read.
///
/// # Safety
///
/// `path` must be `NULL` or NUL terminated; `out` and `err` `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_part_read_yaml(
    path: *const c_char,
    architecture: i32,
    out: *mut *mut fasm_xilinx_part,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        set_out(out, ptr::null_mut());
        run_status(err, || {
            if out.is_null() {
                return Err(CapiError::null("out"));
            }
            let path = path_arg(path, "path")?;
            let arch = fasm_xilinx_architecture::rust_from_int(architecture)?;
            let part = Part::from_yaml_file(&path, arch).map_err(|e| db_error(&e))?;
            let inner = Box::new(PartInner::File(part));
            set_out(out, Box::into_raw(inner).cast::<fasm_xilinx_part>());
            Ok(())
        })
    }
}

/// Frees `part`. `NULL` is accepted (no-op).
///
/// # Safety
///
/// `part` must be `NULL` or live, and not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_part_free(part: *mut fasm_xilinx_part) {
    guard((), || {
        if !part.is_null() {
            // SAFETY: `part` came from `Box::into_raw` and is freed once.
            drop(unsafe { Box::from_raw(part.cast::<PartInner>()) });
        }
    });
}

/// Returns the architecture of `part` (`FASM_XILINX_SERIES7` for `NULL`).
///
/// # Safety
///
/// `part` must be `NULL` or live.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_part_architecture(
    part: *const fasm_xilinx_part,
) -> fasm_xilinx_architecture {
    guard(fasm_xilinx_architecture::FASM_XILINX_SERIES7, || {
        // SAFETY: forwarded from the caller.
        unsafe { part_arg(part) }.map_or(fasm_xilinx_architecture::FASM_XILINX_SERIES7, |p| {
            fasm_xilinx_architecture::from_rust(p.part().architecture)
        })
    })
}

/// The layout of a bitstream (`fasm_xilinx_bitstream_options.format`).
///
/// Values are stable (part of the ABI).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum fasm_xilinx_bitstream_format {
    /// The part's architecture (prjuray-tools' implementation, which is
    /// prjxray's for Series7).
    FASM_XILINX_FORMAT_DEFAULT = 0,
    /// Series7 (`xc7frames2bit`).
    FASM_XILINX_FORMAT_SERIES7 = 1,
    /// UltraScale (`xcframes2bit --architecture=UltraScale`).
    FASM_XILINX_FORMAT_ULTRASCALE = 2,
    /// UltraScale+ (`xcframes2bit --architecture=UltraScalePlus`).
    FASM_XILINX_FORMAT_ULTRASCALE_PLUS = 3,
    /// The plain prjxray `xc7frames2bit --architecture=UltraScale`: the
    /// Series7 part type, frame addresses and ECC with the UltraScale word
    /// count and packets.
    FASM_XILINX_FORMAT_PRJXRAY_ULTRASCALE = 4,
    /// The plain prjxray `xc7frames2bit --architecture=UltraScalePlus`.
    FASM_XILINX_FORMAT_PRJXRAY_ULTRASCALE_PLUS = 5,
}

/// The format of a C `int` value for `part`.
fn format_of(value: i32, part: &Part) -> Result<BitstreamFormat, CapiError> {
    Ok(match value {
        0 => BitstreamFormat::native(part.architecture),
        1 => BitstreamFormat::native(Architecture::Series7),
        2 => BitstreamFormat::native(Architecture::UltraScale),
        3 => BitstreamFormat::native(Architecture::UltraScalePlus),
        4 => BitstreamFormat::prjxray(Architecture::UltraScale),
        5 => BitstreamFormat::prjxray(Architecture::UltraScalePlus),
        _ => {
            return Err(CapiError::invalid_arg(format!(
                "{value} is not a fasm_xilinx_bitstream_format"
            )))
        }
    })
}

/// The options of the bitstream writer. A `NULL` options pointer is all
/// defaults (zero).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct fasm_xilinx_bitstream_options {
    /// A `fasm_xilinx_bitstream_format` value.
    pub format: i32,
    /// The header part name (field b), NUL terminated; `NULL`: the part of
    /// the database for a part from `fasm_xilinx_part_from_database`, else
    /// empty.
    pub part_name: *const c_char,
    /// The header design name (field a, before `;Generator=`;
    /// `xc7frames2bit` writes its `--frm_file`), NUL terminated; `NULL`:
    /// empty.
    pub design_name: *const c_char,
    /// The generator name after `;Generator=`, NUL terminated; `NULL`:
    /// `xc7frames2bit`.
    pub generator: *const c_char,
    /// Use `source_date_epoch` for the header date and time.
    pub has_source_date_epoch: bool,
    /// Seconds since the epoch of the header date and time (UTC). Without
    /// `has_source_date_epoch`, `$SOURCE_DATE_EPOCH` when it is set to an
    /// integer (like the command line tools), else the current time.
    pub source_date_epoch: i64,
}

/// # Safety
///
/// `part`, `frames`, `options` as for the callers.
unsafe fn write(
    part: *const fasm_xilinx_part,
    frames: *const fasm_xilinx_frames,
    options: *const fasm_xilinx_bitstream_options,
) -> Result<Vec<u8>, CapiError> {
    // SAFETY: forwarded from the caller.
    unsafe {
        let part = part_arg(part)?;
        let frames = frames_arg(frames)?;
        let options = options.as_ref();
        let format = format_of(options.map_or(0, |o| o.format), part.part())?;
        let get = |f: fn(&fasm_xilinx_bitstream_options) -> *const c_char,
                   what: &str|
         -> Result<Option<&str>, CapiError> {
            match options {
                Some(o) => opt_str(f(o), what),
                None => Ok(None),
            }
        };
        let part_name = get(|o| o.part_name, "part_name")?
            .or(part.name())
            .unwrap_or_default();
        let design_name = get(|o| o.design_name, "design_name")?.unwrap_or_default();
        let generator = get(|o| o.generator, "generator")?.unwrap_or("xc7frames2bit");
        let seconds = match options {
            Some(o) if o.has_source_date_epoch => Some(o.source_date_epoch),
            _ => std::env::var("SOURCE_DATE_EPOCH")
                .ok()
                .and_then(|v| v.trim().parse::<i64>().ok()),
        };
        let (date, time) = seconds.map(utc_date_time).unzip();
        let options = BitstreamOptions {
            design_name: design_name.as_bytes().to_vec(),
            generator: generator.as_bytes().to_vec(),
            part_name: part_name.as_bytes().to_vec(),
            date,
            time,
        };
        bitstream_bytes_with(part.part(), frames, &options, &format).map_err(|e| {
            CapiError::new(fasm_status::FASM_ERR_BITSTREAM, e.to_string())
                .with_kind("fasm_xilinx.BitstreamError")
        })
    }
}

/// Writes `frames` as a `.bit` bitstream for `part` into a new
/// `fasm_bytes` stored in `*out`, byte for byte like `xc7frames2bit`
/// (Series7) and prjuray-tools' `xcframes2bit` (UltraScale, UltraScale+):
/// the `.bit` header, the configuration packets with the part's IDCODE and
/// every frame of the part (missing frames zero filled) with its ECC.
///
/// Errors: `FASM_ERR_BITSTREAM` (a part of another architecture than the
/// format's, frames of another size), `FASM_ERR_INVALID_ARG`.
///
/// # Safety
///
/// `part` and `frames` must be `NULL` or live; `options` `NULL` or valid
/// (its strings `NULL` or NUL terminated); `out` and `err` `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_bitstream_write(
    part: *const fasm_xilinx_part,
    frames: *const fasm_xilinx_frames,
    options: *const fasm_xilinx_bitstream_options,
    out: *mut *mut fasm_bytes,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        set_out(out, ptr::null_mut());
        run_status(err, || {
            if out.is_null() {
                return Err(CapiError::null("out"));
            }
            let data = write(part, frames, options)?;
            set_out(out, bytes_into_raw(data));
            Ok(())
        })
    }
}

/// `fasm_xilinx_bitstream_write` to the file `path` (NUL terminated;
/// `FASM_ERR_IO` if it cannot be written).
///
/// # Safety
///
/// As for `fasm_xilinx_bitstream_write`; `path` `NULL` or NUL terminated.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_bitstream_write_file(
    part: *const fasm_xilinx_part,
    frames: *const fasm_xilinx_frames,
    options: *const fasm_xilinx_bitstream_options,
    path: *const c_char,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        run_status(err, || {
            let path = path_arg(path, "path")?;
            let data = write(part, frames, options)?;
            std::fs::write(&path, data).map_err(|e| super::io_error(&path, e))
        })
    }
}

/// # Safety
///
/// `part` must be `NULL` or live; `out` writable.
unsafe fn read_into(
    part: *const fasm_xilinx_part,
    data: &[u8],
    format: i32,
    clear_ecc: bool,
    skip_zero: bool,
    out: *mut *mut fasm_xilinx_frames,
) -> Result<(), CapiError> {
    // SAFETY: forwarded from the caller.
    let part = unsafe { part_arg(part) }?.part();
    let format = format_of(format, part)?;
    let error = |message: String| {
        CapiError::new(fasm_status::FASM_ERR_BITSTREAM, message)
            .with_kind("fasm_xilinx.BitstreamError")
    };
    let reader = BitstreamReader::from_bytes(data)
        .ok_or_else(|| error("Input doesn't look like a bitstream".to_owned()))?;
    let configuration = reader
        .configuration_with(part, &format)
        .map_err(|e| error(e.to_string()))?;
    let frames = configuration.to_frames(clear_ecc, skip_zero);
    // SAFETY: forwarded from the caller.
    unsafe { set_out(out, frames_into_raw(frames)) };
    Ok(())
}

/// Reads the frames of the bitstream `data[0..len]` (a `.bit` file, or
/// raw configuration data: everything after the first sync word) for
/// `part` in `format` (a `fasm_xilinx_bitstream_format` value), like
/// `bitread`, into new frames stored in `*out`: every frame the bitstream
/// writes, with the ECC bits cleared when `clear_ecc` (which gives back
/// the frames `fasm2frames` wrote, `bitread --frm_out`) and without the
/// all zero frames when `skip_zero`.
///
/// Errors: `FASM_ERR_BITSTREAM` (no sync word, an IDCODE that is not the
/// part's, a part of another architecture than the format's).
///
/// # Safety
///
/// `part` must be `NULL` or live; `data` valid for `len` bytes; `out` and
/// `err` `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_bitstream_read(
    part: *const fasm_xilinx_part,
    data: *const u8,
    len: usize,
    format: i32,
    clear_ecc: bool,
    skip_zero: bool,
    out: *mut *mut fasm_xilinx_frames,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        set_out(out, ptr::null_mut());
        run_status(err, || {
            if out.is_null() {
                return Err(CapiError::null("out"));
            }
            let data = bytes(data, len, "data")?;
            read_into(part, data, format, clear_ecc, skip_zero, out)
        })
    }
}

/// `fasm_xilinx_bitstream_read` for the file `path` (NUL terminated;
/// `FASM_ERR_IO` if it cannot be read).
///
/// # Safety
///
/// As for `fasm_xilinx_bitstream_read`; `path` `NULL` or NUL terminated.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_bitstream_read_file(
    part: *const fasm_xilinx_part,
    path: *const c_char,
    format: i32,
    clear_ecc: bool,
    skip_zero: bool,
    out: *mut *mut fasm_xilinx_frames,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        set_out(out, ptr::null_mut());
        run_status(err, || {
            if out.is_null() {
                return Err(CapiError::null("out"));
            }
            let path = path_arg(path, "path")?;
            let data = std::fs::read(&path).map_err(|e| super::io_error(&path, e))?;
            read_into(part, &data, format, clear_ecc, skip_zero, out)
        })
    }
}

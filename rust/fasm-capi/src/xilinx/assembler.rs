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

//! [`fasm_xilinx_assembler`] and `fasm_xilinx_fasm2frames_*`.

use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::Arc;

use fasm_xilinx::{
    Architecture, AssemblerError, Database, Fasm2FramesOptions, FasmAssembler, FasmInput, Roi,
};

use super::database::{database_arg, fasm_xilinx_database};
use super::frames::{fasm_xilinx_frames, frames_into_raw};
use super::{assembler_error, emit_warnings, fasm_xilinx_warning_fn, path_arg, set_out};
use crate::error::{fasm_error, fasm_status, CapiError};
use crate::ffi::{bytes, fasm_str, guard, run_status};
use crate::file::{fasm_file, file_ref};

/// A FASM -> frames assembler (Python: `fasm.xilinx.FasmAssembler`, a
/// port of `prjxray.fasm_assembler.FasmAssembler`).
///
/// Created by `fasm_xilinx_assembler_new` for a database opened with a
/// part, released with `fasm_xilinx_assembler_free`. It shares the
/// ownership of its database. A call that modifies it needs exclusive
/// access (one thread at a time).
pub struct fasm_xilinx_assembler {
    _private: [u8; 0],
}

/// The Rust side of a [`fasm_xilinx_assembler`].
struct AssemblerInner {
    db: Arc<Database>,
    assembler: FasmAssembler<'static>,
}

/// # Safety
///
/// `a` must be `NULL` or a live assembler not accessed by anything else
/// for `'a`.
unsafe fn assembler_mut<'a>(
    a: *mut fasm_xilinx_assembler,
) -> Result<&'a mut AssemblerInner, CapiError> {
    // SAFETY: a non-NULL `a` came from `Box::into_raw` in
    // `fasm_xilinx_assembler_new` and is live and exclusive (caller
    // contract).
    unsafe { a.cast::<AssemblerInner>().as_mut() }.ok_or_else(|| CapiError::null("assembler"))
}

/// # Safety
///
/// `a` must be `NULL` or a live assembler not modified for `'a`.
unsafe fn assembler_ref<'a>(a: *const fasm_xilinx_assembler) -> Option<&'a AssemblerInner> {
    // SAFETY: as for `assembler_mut`.
    unsafe { a.cast::<AssemblerInner>().as_ref() }
}

/// Creates an assembler for the part `db` was opened for, stored in
/// `*out` (free with `fasm_xilinx_assembler_free`). For an UltraScale or
/// UltraScale+ database it uses prjuray's semantics (see
/// `fasm_xilinx_assembler_set_prjuray`), like the `fasm2frames` tool.
///
/// Errors: `FASM_ERR_DB` if `db` was opened without a part.
///
/// # Safety
///
/// `db` must be `NULL` or a live database; `out` and `err` `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_assembler_new(
    db: *const fasm_xilinx_database,
    out: *mut *mut fasm_xilinx_assembler,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        set_out(out, ptr::null_mut());
        run_status(err, || {
            if out.is_null() {
                return Err(CapiError::null("out"));
            }
            let db = Arc::clone(&database_arg(db)?.db);
            let mut assembler = FasmAssembler::new_shared(Arc::clone(&db)).map_err(|e| {
                CapiError::new(fasm_status::FASM_ERR_DB, e.to_string()).with_kind("AttributeError")
            })?;
            assembler.set_prjuray(db.architecture() != Architecture::Series7);
            let inner = Box::new(AssemblerInner { db, assembler });
            set_out(out, Box::into_raw(inner).cast::<fasm_xilinx_assembler>());
            Ok(())
        })
    }
}

/// Frees `assembler`. `NULL` is accepted (no-op).
///
/// # Safety
///
/// `assembler` must be `NULL` or live, and not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_assembler_free(assembler: *mut fasm_xilinx_assembler) {
    guard((), || {
        if !assembler.is_null() {
            // SAFETY: `assembler` came from `Box::into_raw` and is freed
            // once (caller contract).
            drop(unsafe { Box::from_raw(assembler.cast::<AssemblerInner>()) });
        }
    });
}

/// Selects prjuray's assembler semantics (`true`: bits beyond the end of a
/// frame are kept, conflicts are reported in 16-bit words) or prjxray's
/// (`false`: such bits are dropped with a warning).
///
/// # Safety
///
/// `assembler` must be `NULL` or live and not used by anything else
/// during the call.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_assembler_set_prjuray(
    assembler: *mut fasm_xilinx_assembler,
    prjuray: bool,
) {
    guard((), || {
        // SAFETY: forwarded from the caller.
        if let Ok(a) = unsafe { assembler_mut(assembler) } {
            a.assembler.set_prjuray(prjuray);
        }
    });
}

/// Runs `f` on the assembler, mapping its error.
///
/// # Safety
///
/// As for [`assembler_mut`]; `err` `NULL` or writable.
unsafe fn with_assembler(
    assembler: *mut fasm_xilinx_assembler,
    err: *mut *mut fasm_error,
    f: impl FnOnce(&mut AssemblerInner) -> Result<(), AssemblerError>,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        run_status(err, || {
            let a = assembler_mut(assembler)?;
            f(a).map_err(|e| assembler_error(&e))
        })
    }
}

/// `FasmAssembler.parse_fasm_filename`: parses the whole FASM file `path`
/// (NUL terminated; a syntax error is reported before anything is
/// assembled) and adds its lines.
///
/// Errors: `FASM_ERR_IO`, `FASM_ERR_PARSE` (with a position),
/// `FASM_ERR_LOOKUP` (every feature missing from the database, one per
/// line of the message), `FASM_ERR_INCONSISTENT_BITS`,
/// `FASM_ERR_ASSEMBLER` (kind `KeyError`: an unknown tile or tile type).
///
/// # Safety
///
/// `assembler` must be `NULL` or live and exclusive; `path` `NULL` or NUL
/// terminated; `err` `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_assembler_parse_file(
    assembler: *mut fasm_xilinx_assembler,
    path: *const c_char,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        let path = match path_arg(path, "path") {
            Ok(path) => path,
            Err(e) => return run_status(err, || Err(e)),
        };
        with_assembler(assembler, err, |a| {
            a.assembler.parse_fasm_filename(&path, Vec::new())
        })
    }
}

/// `fasm_xilinx_assembler_parse_file` for the FASM text `text[0..len]`.
///
/// # Safety
///
/// `assembler` must be `NULL` or live and exclusive; `text` valid for
/// `len` bytes; `err` `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_assembler_parse_string(
    assembler: *mut fasm_xilinx_assembler,
    text: *const c_char,
    len: usize,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        let data = match bytes(text.cast::<u8>(), len, "text") {
            Ok(data) => data,
            Err(e) => return run_status(err, || Err(e)),
        };
        with_assembler(assembler, err, |a| {
            a.assembler.parse_fasm_bytes(data, Vec::new())
        })
    }
}

/// Adds the lines of the parsed model `file` (`fasm_parse_*`,
/// `fasm_file_push_line`, ...) like `FasmAssembler.add_fasm_line`; the
/// features missing from the database are reported together at the end
/// (`FASM_ERR_LOOKUP`).
///
/// # Safety
///
/// `assembler` must be `NULL` or live and exclusive; `file` `NULL` or a
/// live `fasm_file`; `err` `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_assembler_add_file(
    assembler: *mut fasm_xilinx_assembler,
    file: *const fasm_file,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        let Some(file) = file_ref(file) else {
            return run_status(err, || Err(CapiError::null("file")));
        };
        with_assembler(assembler, err, |a| {
            add_lines(&mut a.assembler, file.lines.iter().cloned())
        })
    }
}

fn add_lines(
    assembler: &mut FasmAssembler<'_>,
    lines: impl IntoIterator<Item = fasm::FasmLine>,
) -> Result<(), AssemblerError> {
    let mut missing = Vec::new();
    for line in lines {
        assembler.add_fasm_line(line, &mut missing)?;
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(AssemblerError::Lookup(missing))
    }
}

/// Adds the lines of the part's `required_features.fasm`, like
/// `fasm2frames` does after the FASM file.
///
/// # Safety
///
/// `assembler` must be `NULL` or live and exclusive; `err` `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_assembler_add_required_features(
    assembler: *mut fasm_xilinx_assembler,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        with_assembler(assembler, err, |a| {
            let text =
                a.db.part_info()
                    .map(|info| info.required_features.join("\n"))
                    .unwrap_or_default();
            let lines = fasm::parse_fasm_string(&text)?;
            add_lines(&mut a.assembler, lines)
        })
    }
}

/// `FasmAssembler.mark_roi_frames`: marks every frame of every bus of the
/// tiles with `x1 <= grid_x <= x2` and `y1 <= grid_y <= y2` in use, so
/// that sparse frames include them.
///
/// # Safety
///
/// `assembler` must be `NULL` or live and exclusive; `err` `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_assembler_mark_roi(
    assembler: *mut fasm_xilinx_assembler,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        with_assembler(assembler, err, |a| {
            a.assembler.mark_roi_frames(&Roi { x1, x2, y1, y2 });
            Ok(())
        })
    }
}

/// The STEPDOWN propagation of `fasm2frames` (call it after all features
/// have been added): if a used IOB of an IO bank sets a feature whose name
/// contains `STEPDOWN`, every unused IOB site of the bank gets the same
/// feature(s) and the bank's `HCLK_IOI3` tile gets `STEPDOWN`.
///
/// # Safety
///
/// `assembler` must be `NULL` or live and exclusive; `err` `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_assembler_propagate_stepdown(
    assembler: *mut fasm_xilinx_assembler,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        with_assembler(assembler, err, |a| {
            fasm_xilinx::propagate_stepdown(&a.db, &mut a.assembler)
        })
    }
}

/// Returns the number of warnings so far (bits beyond the end of a frame
/// that prjxray drops with `frame_set: invalid word address ...`; 0 for
/// `NULL`).
///
/// # Safety
///
/// `assembler` must be `NULL` or live.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_assembler_warning_count(
    assembler: *const fasm_xilinx_assembler,
) -> usize {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { assembler_ref(assembler) }.map_or(0, |a| a.assembler.warnings().len())
    })
}

/// Returns warning `index` (borrowed, not NUL terminated: valid until the
/// assembler is modified or freed; empty if out of range).
///
/// # Safety
///
/// `assembler` must be `NULL` or live.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_assembler_warning(
    assembler: *const fasm_xilinx_assembler,
    index: usize,
) -> fasm_str {
    guard(fasm_str::empty(), || {
        // SAFETY: forwarded from the caller.
        unsafe { assembler_ref(assembler) }
            .and_then(|a| a.assembler.warnings().get(index))
            .map_or(fasm_str::empty(), |w| fasm_str::from_str(w))
    })
}

/// `FasmAssembler.get_frames(sparse)`: every frame of the part (`sparse`
/// false, zero filled) or only the frames of the buses that were written
/// or marked, with the set bits applied, as new frames stored in `*out`.
///
/// # Safety
///
/// `assembler` must be `NULL` or live; `out` and `err` `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_assembler_get_frames(
    assembler: *const fasm_xilinx_assembler,
    sparse: bool,
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
            let a = assembler_ref(assembler).ok_or_else(|| CapiError::null("assembler"))?;
            let frames = a
                .assembler
                .get_frames(sparse)
                .map_err(|e| assembler_error(&e))?;
            set_out(out, frames_into_raw(frames));
            Ok(())
        })
    }
}

/// The options of `fasm_xilinx_fasm2frames_*` (the flags of the
/// `fasm2frames` tool). A `NULL` options pointer is all defaults (zero).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct fasm_xilinx_fasm2frames_options {
    /// `--sparse`: only the frames of the buses that were written (and of
    /// the ROI tiles) instead of every frame of the part.
    pub sparse: bool,
    /// `--emit_pudc_b_pullup`: make the PUDC_B pin an input with a pullup
    /// if the FASM does not use its IOB.
    pub emit_pudc_b_pullup: bool,
    /// `--roi`: a ROI `design.json` (NUL terminated path), or `NULL`.
    pub roi: *const c_char,
    /// Receives the warnings (bits beyond the end of a frame), or `NULL`.
    pub warning: fasm_xilinx_warning_fn,
    /// Passed to `warning`.
    pub user: *mut c_void,
}

/// # Safety
///
/// `db`, `options`, `out` and `err` as for the callers.
unsafe fn fasm2frames<'a>(
    db: *const fasm_xilinx_database,
    input: impl FnOnce() -> Result<(Option<std::path::PathBuf>, &'a [u8]), CapiError>,
    options: *const fasm_xilinx_fasm2frames_options,
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
            let db = database_arg(db)?;
            let (warning, user, sparse, emit, roi) = match options.as_ref() {
                None => (None, ptr::null_mut(), false, false, None),
                Some(o) => (
                    o.warning,
                    o.user,
                    o.sparse,
                    o.emit_pudc_b_pullup,
                    if o.roi.is_null() {
                        None
                    } else {
                        Some(path_arg(o.roi, "roi")?)
                    },
                ),
            };
            let options = Fasm2FramesOptions {
                sparse,
                roi,
                emit_pudc_b_pullup: emit,
            };
            let (path, data) = input()?;
            let input = match &path {
                Some(path) => FasmInput::File(path),
                None => FasmInput::Bytes(data),
            };
            let mut warnings = Vec::new();
            let result = if db.db.architecture() == Architecture::Series7 {
                fasm_xilinx::fasm2frames_from(&db.db, input, &options, &mut |w| {
                    warnings.push(w.to_owned());
                })
            } else {
                fasm_xilinx::uray_fasm2frames_from(&db.db, input, &options)
            };
            emit_warnings(warning, user, &warnings);
            let frames = result.map_err(|e| assembler_error(&e))?;
            set_out(out, frames_into_raw(frames));
            Ok(())
        })
    }
}

/// The whole FASM -> frames flow of the `fasm2frames` tool
/// (`xc_fasm.fasm2frames.fasm2frames`) for the part `db` was opened for,
/// on the FASM file `path` (NUL terminated): the ROI, the part's required
/// features, the PUDC_B pullup and the STEPDOWN propagation; on an
/// UltraScale(+) database, prjuray's flow (like the tool). The frames are
/// stored in `*out`; byte for byte the tool's `.frm` output through
/// `fasm_xilinx_frames_to_frm`.
///
/// Errors: as `fasm_xilinx_assembler_parse_file`, plus `FASM_ERR_IO` /
/// `FASM_ERR_ASSEMBLER` for the ROI file and `FASM_ERR_DB`.
///
/// # Safety
///
/// `db` must be `NULL` or a live database; `path` `NULL` or NUL
/// terminated; `options` `NULL` or valid (its `roi` `NULL` or NUL
/// terminated, `warning` `NULL` or callable with `user`); `out` and `err`
/// `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_fasm2frames_file(
    db: *const fasm_xilinx_database,
    path: *const c_char,
    options: *const fasm_xilinx_fasm2frames_options,
    out: *mut *mut fasm_xilinx_frames,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        fasm2frames(
            db,
            || Ok((Some(path_arg(path, "path")?), &[][..])),
            options,
            out,
            err,
        )
    }
}

/// `fasm_xilinx_fasm2frames_file` for the FASM text `text[0..len]`.
///
/// # Safety
///
/// As for `fasm_xilinx_fasm2frames_file`, with `text` valid for `len`
/// bytes.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_fasm2frames_string(
    db: *const fasm_xilinx_database,
    text: *const c_char,
    len: usize,
    options: *const fasm_xilinx_fasm2frames_options,
    out: *mut *mut fasm_xilinx_frames,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller (`text` is readable during the
    // call).
    unsafe {
        let data = bytes(text.cast::<u8>(), len, "text");
        fasm2frames(db, move || Ok((None, data?)), options, out, err)
    }
}

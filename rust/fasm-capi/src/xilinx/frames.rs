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

//! [`fasm_xilinx_frames`] and `.frm` files.

use std::ffi::{c_char, c_void};
use std::ptr;

use fasm_xilinx::{Frames, FrmError};

use super::{emit_warnings, fasm_xilinx_warning_fn, path_arg, set_out};
use crate::error::{fasm_error, fasm_status, CapiError};
use crate::ffi::{bytes, guard, run_status};
use crate::string::{fasm_string, OwnedString};

/// Configuration frames: frame address -> `words_per_frame` 32-bit
/// words, in ascending address order (Python: `fasm.xilinx.Frames`).
///
/// Created by `fasm_xilinx_frames_new`, `fasm_xilinx_assembler_get_frames`,
/// `fasm_xilinx_fasm2frames_*`, `fasm_xilinx_frames_read_frm` /
/// `_parse_frm` and `fasm_xilinx_bitstream_read*`; released with
/// `fasm_xilinx_frames_free`. Reading from several threads at once is
/// safe; `fasm_xilinx_frames_set` needs exclusive access.
pub struct fasm_xilinx_frames {
    _private: [u8; 0],
}

/// Moves `frames` into a new `fasm_xilinx_frames`.
pub(crate) fn frames_into_raw(frames: Frames) -> *mut fasm_xilinx_frames {
    Box::into_raw(Box::new(frames)).cast::<fasm_xilinx_frames>()
}

/// Borrows the frames behind `frames`.
///
/// # Safety
///
/// `frames` must be `NULL` or live and not modified for `'a`.
pub(crate) unsafe fn frames_ref<'a>(frames: *const fasm_xilinx_frames) -> Option<&'a Frames> {
    // SAFETY: a non-NULL `frames` came from `frames_into_raw` and is live.
    unsafe { frames.cast::<Frames>().as_ref() }
}

/// The frames behind `frames`, or an error for `NULL`.
///
/// # Safety
///
/// As for [`frames_ref`].
pub(crate) unsafe fn frames_arg<'a>(
    frames: *const fasm_xilinx_frames,
) -> Result<&'a Frames, CapiError> {
    // SAFETY: forwarded from the caller.
    unsafe { frames_ref(frames) }.ok_or_else(|| CapiError::null("frames"))
}

/// Returns a new, empty set of frames of `words_per_frame` words (101 for
/// Series7, 123 for UltraScale, 93 for UltraScale+), or `NULL` if
/// `words_per_frame` is 0. Free with `fasm_xilinx_frames_free`.
#[no_mangle]
pub extern "C" fn fasm_xilinx_frames_new(words_per_frame: usize) -> *mut fasm_xilinx_frames {
    guard(ptr::null_mut(), || {
        if words_per_frame == 0 {
            ptr::null_mut()
        } else {
            frames_into_raw(Frames::new(words_per_frame))
        }
    })
}

/// Frees `frames`. `NULL` is accepted (no-op).
///
/// # Safety
///
/// `frames` must be `NULL` or live, and not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_frames_free(frames: *mut fasm_xilinx_frames) {
    guard((), || {
        if !frames.is_null() {
            // SAFETY: `frames` came from `Box::into_raw` and is freed once.
            drop(unsafe { Box::from_raw(frames.cast::<Frames>()) });
        }
    });
}

/// Returns the number of frames (0 for `NULL`).
///
/// # Safety
///
/// `frames` must be `NULL` or live.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_frames_count(frames: *const fasm_xilinx_frames) -> usize {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { frames_ref(frames) }.map_or(0, Frames::len)
    })
}

/// Returns the number of 32-bit words per frame (0 for `NULL`).
///
/// # Safety
///
/// `frames` must be `NULL` or live.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_frames_words_per_frame(
    frames: *const fasm_xilinx_frames,
) -> usize {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { frames_ref(frames) }.map_or(0, Frames::words_per_frame)
    })
}

/// Returns the address of frame `index` (frames are in ascending address
/// order; 0 if `index` is out of range or `frames` is `NULL`).
///
/// # Safety
///
/// `frames` must be `NULL` or live.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_frames_address(
    frames: *const fasm_xilinx_frames,
    index: usize,
) -> u32 {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { frames_ref(frames) }
            .and_then(|f| f.addresses().get(index).copied())
            .unwrap_or(0)
    })
}

/// Returns the `words_per_frame` words of frame `index` (borrowed from
/// `frames`: valid until it is freed or modified), or `NULL` if `index` is
/// out of range or `frames` is `NULL`.
///
/// # Safety
///
/// `frames` must be `NULL` or live.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_frames_words(
    frames: *const fasm_xilinx_frames,
    index: usize,
) -> *const u32 {
    guard(ptr::null(), || {
        // SAFETY: forwarded from the caller.
        unsafe { frames_ref(frames) }
            .and_then(|f| f.addresses().get(index).and_then(|&a| f.get(a)))
            .map_or(ptr::null(), <[u32]>::as_ptr)
    })
}

/// Returns the words of the frame at `address` (borrowed, as
/// `fasm_xilinx_frames_words`), or `NULL` if there is none.
///
/// # Safety
///
/// `frames` must be `NULL` or live.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_frames_find(
    frames: *const fasm_xilinx_frames,
    address: u32,
) -> *const u32 {
    guard(ptr::null(), || {
        // SAFETY: forwarded from the caller.
        unsafe { frames_ref(frames) }
            .and_then(|f| f.get(address))
            .map_or(ptr::null(), <[u32]>::as_ptr)
    })
}

/// Sets the frame at `address` to `words[0..count]` (inserting it if it
/// does not exist). `count` must be the number of words per frame
/// (`FASM_ERR_INVALID_ARG` otherwise).
///
/// # Safety
///
/// `frames` must be `NULL` or live and not accessed by anything else
/// during the call; `words` valid for `count` elements; `err` `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_frames_set(
    frames: *mut fasm_xilinx_frames,
    address: u32,
    words: *const u32,
    count: usize,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        run_status(err, || {
            // SAFETY: exclusive access (caller contract).
            let frames = frames
                .cast::<Frames>()
                .as_mut()
                .ok_or_else(|| CapiError::null("frames"))?;
            if words.is_null() {
                return Err(CapiError::null("words"));
            }
            if count != frames.words_per_frame() {
                return Err(CapiError::invalid_arg(format!(
                    "{count} words for frames of {} words",
                    frames.words_per_frame()
                )));
            }
            let words = std::slice::from_raw_parts(words, count);
            frames.get_or_insert_zeroed(address).copy_from_slice(words);
            Ok(())
        })
    }
}

/// Returns `true` if `a` and `b` hold the same frames with the same words
/// (two `NULL`s are equal).
///
/// # Safety
///
/// `a` and `b` must be `NULL` or live.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_frames_equal(
    a: *const fasm_xilinx_frames,
    b: *const fasm_xilinx_frames,
) -> bool {
    guard(false, || {
        // SAFETY: forwarded from the caller.
        unsafe { frames_ref(a) == frames_ref(b) }
    })
}

/// Formats `frames` as `.frm` text (`0x%08X` address, a space, the words
/// as comma separated `0x%08X`, one frame per line; byte for byte what
/// `fasm2frames` writes) into a new `fasm_string` stored in `*out`.
///
/// # Safety
///
/// `frames` must be `NULL` or live; `out` and `err` `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_frames_to_frm(
    frames: *const fasm_xilinx_frames,
    out: *mut *mut fasm_string,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        set_out(out, ptr::null_mut());
        run_status(err, || {
            if out.is_null() {
                return Err(CapiError::null("out"));
            }
            let frames = frames_arg(frames)?;
            set_out(out, OwnedString::into_raw(frames.to_frm_string()));
            Ok(())
        })
    }
}

/// Writes `frames` as a `.frm` file to `path` (NUL terminated).
/// `FASM_ERR_IO` if it cannot be written.
///
/// # Safety
///
/// `frames` must be `NULL` or live; `path` `NULL` or NUL terminated; `err`
/// `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_frames_write_frm(
    frames: *const fasm_xilinx_frames,
    path: *const c_char,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        run_status(err, || {
            let frames = frames_arg(frames)?;
            let path = path_arg(path, "path")?;
            let io_error = |e: std::io::Error| super::io_error(&path, e);
            let file = std::fs::File::create(&path).map_err(io_error)?;
            let mut out = std::io::BufWriter::with_capacity(1 << 16, file);
            frames.write_frm(&mut out).map_err(io_error)?;
            std::io::Write::flush(&mut out).map_err(io_error)
        })
    }
}

fn frm_error(error: &FrmError) -> CapiError {
    CapiError::new(fasm_status::FASM_ERR_FRM, error.to_string())
        .with_kind("ValueError")
        .with_position(error.line, 0)
}

/// # Safety
///
/// `out` must be `NULL` or writable; `warning` callable with `user`.
unsafe fn read_into(
    data: &[u8],
    words_per_frame: usize,
    warning: fasm_xilinx_warning_fn,
    user: *mut c_void,
    out: *mut *mut fasm_xilinx_frames,
) -> Result<(), CapiError> {
    if words_per_frame == 0 {
        return Err(CapiError::invalid_arg("words_per_frame must not be 0"));
    }
    let mut warnings = Vec::new();
    let result = Frames::read_frm(data, words_per_frame, &mut |w| warnings.push(w.to_owned()));
    // SAFETY: forwarded from the caller.
    unsafe { emit_warnings(warning, user, &warnings) };
    let frames = result.map_err(|e| frm_error(&e))?;
    // SAFETY: forwarded from the caller.
    unsafe { set_out(out, frames_into_raw(frames)) };
    Ok(())
}

/// Parses `.frm` text `text[0..len]` with `words_per_frame` words per
/// frame, like `xc7frames2bit` reads its `--frm_file` (`#` lines are
/// comments; a line with another number of words is skipped with a
/// warning passed to `warning`, which may be `NULL`; the first of two
/// frames with the same address wins), into new frames stored in `*out`.
/// `FASM_ERR_FRM` (with `fasm_error_line`) for a number that does not
/// parse.
///
/// # Safety
///
/// `text` valid for `len` bytes; `warning` `NULL` or callable with
/// `user`; `out` and `err` `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_frames_parse_frm(
    text: *const c_char,
    len: usize,
    words_per_frame: usize,
    warning: fasm_xilinx_warning_fn,
    user: *mut c_void,
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
            let data = bytes(text.cast::<u8>(), len, "text")?;
            read_into(data, words_per_frame, warning, user, out)
        })
    }
}

/// `fasm_xilinx_frames_parse_frm` for the file `path` (NUL terminated;
/// `FASM_ERR_IO` if it cannot be read).
///
/// # Safety
///
/// `path` `NULL` or NUL terminated; otherwise as
/// `fasm_xilinx_frames_parse_frm`.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_frames_read_frm(
    path: *const c_char,
    words_per_frame: usize,
    warning: fasm_xilinx_warning_fn,
    user: *mut c_void,
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
            read_into(&data, words_per_frame, warning, user, out)
        })
    }
}

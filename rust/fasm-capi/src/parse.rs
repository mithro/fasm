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

//! Parsing entry points: into a [`fasm_file`], or streamed to a callback.

use std::ffi::{c_char, c_void, CStr};
use std::path::PathBuf;
use std::ptr;

use fasm::{parse_fasm_bytes, parse_lines, ParseError, ParseErrorKind};

use crate::error::{fasm_error, fasm_status, CapiError};
use crate::ffi::{bytes, run_status};
use crate::file::{fasm_file, fasm_line, line_ptr, FileInner};

/// Callback of the streaming parsers (`fasm_parse_string_cb`,
/// `fasm_parse_file_cb`), called once per FASM line in file order.
///
/// * `line`: the parsed line, valid **only during the call** (copy out
///   what you need to keep).
/// * `line_number`: the 1 based line number where the FASM line starts
///   (lines are counted at `\n`, like `fasm_error_line`).
/// * `user`: the `user` pointer given to the parse function.
///
/// Return `true` to continue, `false` to stop parsing (the parse function
/// then returns `FASM_OK` without looking at the rest of the input). The
/// callback must not unwind (throw a C++ exception) or `longjmp` out:
/// that is undefined behaviour.
pub type fasm_line_callback = Option<
    unsafe extern "C" fn(line: *const fasm_line, line_number: usize, user: *mut c_void) -> bool,
>;

/// Reads a whole file, reporting failures as [`fasm_status::FASM_ERR_IO`].
///
/// # Safety
///
/// `path` must be `NULL` or a NUL terminated string.
unsafe fn read_file(path: *const c_char) -> Result<Vec<u8>, CapiError> {
    if path.is_null() {
        return Err(CapiError::null("path"));
    }
    // SAFETY: `path` is a NUL terminated string (caller contract).
    let path = unsafe { CStr::from_ptr(path) };
    let path = path_buf(path)?;
    std::fs::read(&path).map_err(|e| {
        CapiError::from(ParseError::new(
            0,
            0,
            ParseErrorKind::Io,
            format!("Couldn't open file {}: {e}", path.display()),
        ))
    })
}

/// Converts a C path: arbitrary bytes on Unix, UTF-8 elsewhere.
#[cfg(unix)]
fn path_buf(path: &CStr) -> Result<PathBuf, CapiError> {
    use std::os::unix::ffi::OsStrExt;
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes())))
}

/// Converts a C path: arbitrary bytes on Unix, UTF-8 elsewhere.
#[cfg(not(unix))]
fn path_buf(path: &CStr) -> Result<PathBuf, CapiError> {
    path.to_str().map(PathBuf::from).map_err(|e| {
        CapiError::new(
            fasm_status::FASM_ERR_UTF8,
            format!("path is not valid UTF-8: {e}"),
        )
    })
}

/// Stores `file` in `*out` if `out` is not `NULL`.
///
/// # Safety
///
/// `out` must be `NULL` or valid for writing a pointer.
unsafe fn set_out(out: *mut *mut fasm_file, file: *mut fasm_file) {
    if !out.is_null() {
        // SAFETY: `out` is writable (caller contract).
        unsafe { out.write(file) };
    }
}

/// Parses `bytes` into a new `fasm_file` stored in `*out`.
///
/// # Safety
///
/// `out` must be valid for writing a pointer.
unsafe fn parse_into(bytes: &[u8], out: *mut *mut fasm_file) -> Result<(), CapiError> {
    let lines = parse_fasm_bytes(bytes)?;
    // SAFETY: forwarded from the caller.
    unsafe { set_out(out, FileInner::into_raw(lines)) };
    Ok(())
}

/// Parses the FASM text `text[0..len]` (UTF-8, need not be NUL terminated)
/// into a new `fasm_file`, stored in `*out` (Python:
/// `fasm.parse_fasm_string`).
///
/// A UTF-8 byte order mark at the start is skipped. Invalid UTF-8 in a
/// comment or annotation value is `FASM_ERR_UTF8`; any other invalid input
/// is `FASM_ERR_PARSE`. On failure `*out` is set to `NULL` and, if `err` is
/// not `NULL`, `*err` receives the error (with `fasm_error_line` /
/// `fasm_error_column`); on success `*out` owns the new file (free with
/// `fasm_file_free`) and `*err` is set to `NULL`.
///
/// Returns `FASM_ERR_INVALID_ARG` if `out` is `NULL`, or `text` is `NULL`
/// with a non-zero `len`.
///
/// # Safety
///
/// `text` must be `NULL` (with `len == 0`) or valid for reading `len`
/// bytes; `out` must be `NULL` or writable; `err` must be `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_parse_string(
    text: *const c_char,
    len: usize,
    out: *mut *mut fasm_file,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: `out` and `err` are NULL or writable (caller contract).
    unsafe {
        set_out(out, ptr::null_mut());
        run_status(err, || {
            if out.is_null() {
                return Err(CapiError::null("out"));
            }
            let input = bytes(text.cast::<u8>(), len, "text")?;
            parse_into(input, out)
        })
    }
}

/// Reads and parses the FASM file `path` (NUL terminated; any bytes on
/// Unix, UTF-8 on other systems) into a new `fasm_file`, stored in `*out`
/// (Python: `fasm.parse_fasm_filename`).
///
/// A file that cannot be read is `FASM_ERR_IO` (line and column 0);
/// otherwise as `fasm_parse_string`.
///
/// # Safety
///
/// `path` must be `NULL` or a NUL terminated string; `out` and `err` must
/// be `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_parse_file(
    path: *const c_char,
    out: *mut *mut fasm_file,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: `path` is NULL or NUL terminated, `out` and `err` are NULL or
    // writable (caller contract).
    unsafe {
        set_out(out, ptr::null_mut());
        run_status(err, || {
            if out.is_null() {
                return Err(CapiError::null("out"));
            }
            let data = read_file(path)?;
            parse_into(&data, out)
        })
    }
}

/// Parses `bytes`, calling `callback` for each line until it returns
/// `false`.
///
/// # Safety
///
/// `callback` must be safe to call with a line and `user`.
unsafe fn stream(
    bytes: &[u8],
    callback: unsafe extern "C" fn(*const fasm_line, usize, *mut c_void) -> bool,
    user: *mut c_void,
) -> Result<(), CapiError> {
    let mut lines = parse_lines(bytes);
    while let Some(line) = lines.next() {
        let line = line?;
        // SAFETY: `line` outlives the call; the rest is the caller's
        // contract.
        if !unsafe { callback(line_ptr(&line), lines.line_number(), user) } {
            break;
        }
    }
    Ok(())
}

/// Parses the FASM text `text[0..len]` like `fasm_parse_string`, but
/// instead of building a `fasm_file` calls `callback` for each line (see
/// `fasm_line_callback`), which avoids holding the whole model in memory.
///
/// Lines before a parse error are passed to `callback`; the error is then
/// returned (`FASM_ERR_PARSE` / `FASM_ERR_UTF8`, with `*err` set). Returns
/// `FASM_OK` when the whole input was parsed or `callback` returned
/// `false`, and `FASM_ERR_INVALID_ARG` for a `NULL` `callback`, or a
/// `NULL` `text` with a non-zero `len`.
///
/// # Safety
///
/// `text` must be `NULL` (with `len == 0`) or valid for reading `len`
/// bytes; `callback` must be `NULL` or a function that can be called with
/// `user`; `err` must be `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_parse_string_cb(
    text: *const c_char,
    len: usize,
    callback: fasm_line_callback,
    user: *mut c_void,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        run_status(err, || {
            let callback = callback.ok_or_else(|| CapiError::null("callback"))?;
            let input = bytes(text.cast::<u8>(), len, "text")?;
            stream(input, callback, user)
        })
    }
}

/// Reads the FASM file `path` (see `fasm_parse_file`) and streams its
/// lines to `callback` (see `fasm_parse_string_cb`).
///
/// # Safety
///
/// `path` must be `NULL` or a NUL terminated string; `callback` must be
/// `NULL` or a function that can be called with `user`; `err` must be
/// `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_parse_file_cb(
    path: *const c_char,
    callback: fasm_line_callback,
    user: *mut c_void,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        run_status(err, || {
            let callback = callback.ok_or_else(|| CapiError::null("callback"))?;
            let data = read_file(path)?;
            stream(&data, callback, user)
        })
    }
}

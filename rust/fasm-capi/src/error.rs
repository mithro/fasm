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

//! Status codes and the [`fasm_error`] object.

use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;

use fasm::{ModelError, OutputError, ParseError, ParseErrorKind};

use crate::ffi::guard;

/// Result code of a fallible function.
///
/// Values are stable (part of the ABI); new codes may be added at the end.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum fasm_status {
    /// Success.
    FASM_OK = 0,
    /// The FASM text does not match the grammar or its values do not fit
    /// (`fasm_error_line` / `fasm_error_column` locate the error).
    FASM_ERR_PARSE = 1,
    /// A file could not be read.
    FASM_ERR_IO = 2,
    /// An argument is invalid: a `NULL` pointer where an object is
    /// required, an out of range value, an invalid `SetFasmFeature` (end
    /// without start, value too wide for its address, ...).
    FASM_ERR_INVALID_ARG = 3,
    /// Text that must be UTF-8 is not (in an argument, or a comment /
    /// annotation value of parsed FASM text).
    FASM_ERR_UTF8 = 4,
    /// An internal error (a Rust panic caught at the C boundary). Please
    /// report it as a bug.
    FASM_ERR_PANIC = 5,
    /// Formatting or merging a model failed (for example conflicting bits
    /// in `fasm_file_merge_and_sort`).
    FASM_ERR_OUTPUT = 6,
    /// A Xilinx database cannot be opened: a missing or malformed file, an
    /// unknown part, not a prjxray-db / prjuray-db family directory, a
    /// part file that cannot be used, a part without a frame tree
    /// (`fasm_xilinx_*`).
    FASM_ERR_DB = 7,
    /// FASM features that are not in the database
    /// (`prjxray.fasm_assembler.FasmLookupError`; the message has one line
    /// per missing feature bit).
    FASM_ERR_LOOKUP = 8,
    /// Two FASM lines want a different value for one bit
    /// (`prjxray.fasm_assembler.FasmInconsistentBits`).
    FASM_ERR_INCONSISTENT_BITS = 9,
    /// Any other error of the Xilinx assembler: an unknown tile or tile type
    /// (`KeyError`), a malformed ROI `design.json`, ... (`fasm_error_kind`
    /// names the exception of the reference tools).
    FASM_ERR_ASSEMBLER = 10,
    /// A bitstream cannot be written or read: a part of another
    /// architecture than the format's, frames of another size, data
    /// without a sync word, an IDCODE that is not the part's.
    FASM_ERR_BITSTREAM = 11,
    /// A number of a `.frm` file does not parse.
    FASM_ERR_FRM = 12,
}

/// Details of a failed call: status, message and (for parse errors) the
/// position in the input.
///
/// Created by fallible functions in their `fasm_error **err` argument,
/// owned by the caller and released with `fasm_error_free`. Immutable, so
/// it can be read from any thread.
pub struct fasm_error {
    _private: [u8; 0],
}

/// The Rust side of a [`fasm_error`] (what a `*mut fasm_error` points to).
#[derive(Debug)]
pub(crate) struct CapiError {
    pub(crate) status: fasm_status,
    message: CString,
    line: usize,
    column: usize,
    /// The name of the exception the reference Python tools raise
    /// (`fasm_error_kind`), for the errors of the Xilinx functions.
    kind: Option<CString>,
}

impl CapiError {
    /// An error without a position.
    pub(crate) fn new(status: fasm_status, message: impl Into<String>) -> Self {
        let mut message = message.into();
        // A C string cannot hold NUL bytes.
        message.retain(|c| c != '\0');
        CapiError {
            status,
            message: CString::new(message).unwrap_or_default(),
            line: 0,
            column: 0,
            kind: None,
        }
    }

    /// Sets the name of the reference tools' exception
    /// (`fasm_error_kind`).
    pub(crate) fn with_kind(mut self, kind: &str) -> Self {
        self.kind = CString::new(kind.replace('\0', "")).ok();
        self
    }

    /// Sets the position (1 based line, 0 based column).
    pub(crate) fn with_position(mut self, line: usize, column: usize) -> Self {
        self.line = line;
        self.column = column;
        self
    }

    /// A [`fasm_status::FASM_ERR_INVALID_ARG`] error.
    pub(crate) fn invalid_arg(message: impl Into<String>) -> Self {
        Self::new(fasm_status::FASM_ERR_INVALID_ARG, message)
    }

    /// The error for a required pointer argument that is `NULL`.
    pub(crate) fn null(what: &str) -> Self {
        Self::invalid_arg(format!("{what} must not be NULL"))
    }
}

impl From<ParseError> for CapiError {
    fn from(e: ParseError) -> Self {
        let status = match e.kind {
            ParseErrorKind::Io => fasm_status::FASM_ERR_IO,
            ParseErrorKind::InvalidUtf8 => fasm_status::FASM_ERR_UTF8,
            _ => fasm_status::FASM_ERR_PARSE,
        };
        let mut error = CapiError::new(status, e.to_string());
        error.line = e.line;
        error.column = e.column;
        error
    }
}

impl From<ModelError> for CapiError {
    fn from(e: ModelError) -> Self {
        CapiError::invalid_arg(e.to_string())
    }
}

impl From<OutputError> for CapiError {
    fn from(e: OutputError) -> Self {
        CapiError::new(fasm_status::FASM_ERR_OUTPUT, e.to_string())
    }
}

/// Stores `error` (or `NULL`) in `*err`, if `err` is not `NULL`.
///
/// # Safety
///
/// `err` must be `NULL` or valid for writing a pointer.
pub(crate) unsafe fn set_error(err: *mut *mut fasm_error, error: Option<CapiError>) {
    if err.is_null() {
        return;
    }
    let value = match error {
        Some(e) => Box::into_raw(Box::new(e)).cast::<fasm_error>(),
        None => ptr::null_mut(),
    };
    // SAFETY: `err` is not NULL and writable (caller contract).
    unsafe { err.write(value) };
}

/// Borrows the [`CapiError`] behind `e`.
///
/// # Safety
///
/// `e` must be `NULL` or a live `fasm_error` from this library.
unsafe fn inner<'a>(e: *const fasm_error) -> Option<&'a CapiError> {
    // SAFETY: a non-NULL `e` was created by `set_error` from a
    // `Box<CapiError>` and is still live (caller contract).
    unsafe { e.cast::<CapiError>().as_ref() }
}

/// Returns the status code of `error`.
///
/// Returns `FASM_ERR_INVALID_ARG` for a `NULL` `error`.
///
/// # Safety
///
/// `error` must be `NULL` or a live `fasm_error` from this library.
#[no_mangle]
pub unsafe extern "C" fn fasm_error_status(error: *const fasm_error) -> fasm_status {
    guard(fasm_status::FASM_ERR_PANIC, || {
        // SAFETY: forwarded from the caller.
        unsafe { inner(error) }.map_or(fasm_status::FASM_ERR_INVALID_ARG, |e| e.status)
    })
}

/// Returns the message of `error`: NUL terminated UTF-8, owned by `error`
/// (valid until `fasm_error_free`).
///
/// For parse errors it has the form `Parse error at LINE:COLUMN - ...`.
/// Returns an empty string for a `NULL` `error`.
///
/// # Safety
///
/// `error` must be `NULL` or a live `fasm_error` from this library.
#[no_mangle]
pub unsafe extern "C" fn fasm_error_message(error: *const fasm_error) -> *const c_char {
    guard(c"".as_ptr(), || {
        // SAFETY: forwarded from the caller.
        unsafe { inner(error) }.map_or(c"".as_ptr(), |e| e.message.as_ptr())
    })
}

/// Returns the 1 based line of a parse error (lines are counted at `\n`),
/// or 0 when the error has no position (not a parse error, or `NULL`).
///
/// # Safety
///
/// `error` must be `NULL` or a live `fasm_error` from this library.
#[no_mangle]
pub unsafe extern "C" fn fasm_error_line(error: *const fasm_error) -> usize {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { inner(error) }.map_or(0, |e| e.line)
    })
}

/// Returns the 0 based column (in Unicode code points) of a parse error,
/// or 0 when the error has no position (not a parse error, or `NULL`).
///
/// # Safety
///
/// `error` must be `NULL` or a live `fasm_error` from this library.
#[no_mangle]
pub unsafe extern "C" fn fasm_error_column(error: *const fasm_error) -> usize {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { inner(error) }.map_or(0, |e| e.column)
    })
}

/// Returns the kind of `error`: for the errors of the `fasm_xilinx_*`
/// functions, the name of the exception the reference Python tools
/// (prjxray, f4pga-xc-fasm) raise in the same situation, which the
/// command line tools print before the message
/// (`prjxray.fasm_assembler.FasmLookupError`, `KeyError`,
/// `FileNotFoundError`, `Exception` for a parse error, ...); for other
/// errors, `fasm_status_string` of its status. NUL terminated, owned by
/// `error` (valid until `fasm_error_free`); an empty string for a `NULL`
/// `error`.
///
/// # Safety
///
/// `error` must be `NULL` or a live `fasm_error` from this library.
#[no_mangle]
pub unsafe extern "C" fn fasm_error_kind(error: *const fasm_error) -> *const c_char {
    guard(c"".as_ptr(), || {
        // SAFETY: forwarded from the caller.
        match unsafe { inner(error) } {
            None => c"".as_ptr(),
            Some(e) => match &e.kind {
                Some(kind) => kind.as_ptr(),
                None => fasm_status_string(e.status as c_int),
            },
        }
    })
}

/// Frees `error`. `NULL` is accepted (no-op).
///
/// # Safety
///
/// `error` must be `NULL` or a live `fasm_error` from this library that is
/// not used afterwards (in particular not freed twice).
#[no_mangle]
pub unsafe extern "C" fn fasm_error_free(error: *mut fasm_error) {
    guard((), || {
        if !error.is_null() {
            // SAFETY: `error` came from `Box::into_raw` in `set_error` and
            // is freed once (caller contract).
            drop(unsafe { Box::from_raw(error.cast::<CapiError>()) });
        }
    });
}

/// Returns a short static description of the status code `status` (NUL
/// terminated, never `NULL`, never freed), e.g. `"parse error"` for
/// `FASM_ERR_PARSE`, or `"unknown status"` for a value that is not a
/// `fasm_status`.
///
/// Takes an `int` rather than a `fasm_status` so that any integer can be
/// passed safely.
#[no_mangle]
pub extern "C" fn fasm_status_string(status: c_int) -> *const c_char {
    let s: &'static CStr = match status {
        0 => c"ok",
        1 => c"parse error",
        2 => c"I/O error",
        3 => c"invalid argument",
        4 => c"invalid UTF-8",
        5 => c"internal error (panic)",
        6 => c"output error",
        7 => c"database error",
        8 => c"feature not in the database",
        9 => c"inconsistent bits",
        10 => c"assembler error",
        11 => c"bitstream error",
        12 => c"frm file error",
        _ => c"unknown status",
    };
    s.as_ptr()
}

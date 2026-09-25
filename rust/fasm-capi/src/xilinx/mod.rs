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

//! The `fasm_xilinx_*` functions: the C API of the `fasm-xilinx` crate
//! (prjxray-db / prjuray-db databases, FASM -> frames, `.frm` files,
//! bitstreams; `docs/rewrite/DESIGN-capi.md`, "Xilinx").
//!
//! Handles: [`fasm_xilinx_database`] (an opened part database, immutable
//! and shareable), [`fasm_xilinx_assembler`], [`fasm_xilinx_frames`],
//! [`fasm_xilinx_part`] (the frame tree a bitstream is written for) and
//! the owned byte buffer [`fasm_bytes`] (a bitstream). Errors use the
//! status codes `FASM_ERR_DB` to `FASM_ERR_FRM` besides the core ones,
//! and [`crate::fasm_error_kind`] names the exception of the reference
//! Python tools.

use std::ffi::{c_char, c_void, CStr};

use fasm_xilinx::{AssemblerError, DbError};

use crate::error::{fasm_status, CapiError};
use crate::ffi::guard;
use crate::parse::path_buf;

pub(crate) mod assembler;
pub(crate) mod bitstream;
pub(crate) mod database;
pub(crate) mod frames;

/// The architecture of a Xilinx database or part.
///
/// Values are stable (part of the ABI).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum fasm_xilinx_architecture {
    /// 7 series (prjxray-db: artix7, kintex7, spartan7, zynq7).
    FASM_XILINX_SERIES7 = 0,
    /// UltraScale.
    FASM_XILINX_ULTRASCALE = 1,
    /// UltraScale+ (prjuray-db: zynqusp).
    FASM_XILINX_ULTRASCALE_PLUS = 2,
}

impl fasm_xilinx_architecture {
    pub(crate) fn from_rust(arch: fasm_xilinx::Architecture) -> Self {
        match arch {
            fasm_xilinx::Architecture::Series7 => Self::FASM_XILINX_SERIES7,
            fasm_xilinx::Architecture::UltraScale => Self::FASM_XILINX_ULTRASCALE,
            fasm_xilinx::Architecture::UltraScalePlus => Self::FASM_XILINX_ULTRASCALE_PLUS,
        }
    }

    /// The architecture of a C `int` value.
    pub(crate) fn rust_from_int(value: i32) -> Result<fasm_xilinx::Architecture, CapiError> {
        match value {
            0 => Ok(fasm_xilinx::Architecture::Series7),
            1 => Ok(fasm_xilinx::Architecture::UltraScale),
            2 => Ok(fasm_xilinx::Architecture::UltraScalePlus),
            _ => Err(CapiError::invalid_arg(format!(
                "{value} is not a fasm_xilinx_architecture"
            ))),
        }
    }
}

/// A callback receiving a warning of the reference tools (for example
/// `frame_set: invalid word address ...`, which prjxray prints to stderr):
/// `len` bytes of UTF-8 at `message` (also NUL terminated), valid during
/// the call. It must not unwind (throw a C++ exception) or `longjmp` out.
pub type fasm_xilinx_warning_fn =
    Option<unsafe extern "C" fn(message: *const c_char, len: usize, user: *mut c_void)>;

/// Passes `warnings` to `callback`.
///
/// # Safety
///
/// `callback` must be `None` or callable with `user`.
pub(crate) unsafe fn emit_warnings(
    callback: fasm_xilinx_warning_fn,
    user: *mut c_void,
    warnings: &[String],
) {
    let Some(callback) = callback else { return };
    for warning in warnings {
        let mut text = warning.replace('\0', "");
        let len = text.len();
        text.push('\0');
        // SAFETY: `text` is NUL terminated and lives during the call; the
        // callback and `user` are the caller's.
        unsafe { callback(text.as_ptr().cast::<c_char>(), len, user) };
    }
}

/// The error of a failed assembler call.
pub(crate) fn assembler_error(error: &AssemblerError) -> CapiError {
    let kind = error.python_exception();
    let base = match error {
        AssemblerError::Parse(e) => CapiError::from(e.clone()),
        AssemblerError::OpenFasm { .. } | AssemblerError::Io { .. } => {
            CapiError::new(fasm_status::FASM_ERR_IO, error.to_string())
        }
        AssemblerError::Lookup(_) => {
            CapiError::new(fasm_status::FASM_ERR_LOOKUP, error.to_string())
        }
        AssemblerError::InconsistentBits(_) => {
            CapiError::new(fasm_status::FASM_ERR_INCONSISTENT_BITS, error.to_string())
        }
        AssemblerError::Db(e) => return db_error(e),
        _ => CapiError::new(fasm_status::FASM_ERR_ASSEMBLER, error.to_string()),
    };
    base.with_kind(kind)
}

/// The error of a database that cannot be opened.
pub(crate) fn db_error(error: &DbError) -> CapiError {
    CapiError::new(fasm_status::FASM_ERR_DB, error.to_string()).with_kind("fasm_xilinx.DbError")
}

/// Converts a NUL terminated C path.
///
/// # Safety
///
/// `path` must be `NULL` or a NUL terminated string.
pub(crate) unsafe fn path_arg(
    path: *const c_char,
    what: &str,
) -> Result<std::path::PathBuf, CapiError> {
    if path.is_null() {
        return Err(CapiError::null(what));
    }
    // SAFETY: `path` is a NUL terminated string (caller contract).
    path_buf(unsafe { CStr::from_ptr(path) })
}

/// Converts an optional NUL terminated UTF-8 C string.
///
/// # Safety
///
/// `s` must be `NULL` or a NUL terminated string.
pub(crate) unsafe fn opt_str<'a>(
    s: *const c_char,
    what: &str,
) -> Result<Option<&'a str>, CapiError> {
    if s.is_null() {
        return Ok(None);
    }
    // SAFETY: `s` is a NUL terminated string (caller contract).
    let s = unsafe { CStr::from_ptr(s) };
    s.to_str().map(Some).map_err(|e| {
        CapiError::new(
            fasm_status::FASM_ERR_UTF8,
            format!("{what} is not valid UTF-8: {e}"),
        )
    })
}

/// Writes `value` to `*out` if `out` is not `NULL`.
///
/// # Safety
///
/// `out` must be `NULL` or writable.
pub(crate) unsafe fn set_out<T>(out: *mut *mut T, value: *mut T) {
    if !out.is_null() {
        // SAFETY: `out` is writable (caller contract).
        unsafe { out.write(value) };
    }
}

/// An owned, immutable byte buffer returned by the library (a bitstream).
/// Released with `fasm_bytes_free`.
pub struct fasm_bytes {
    _private: [u8; 0],
}

/// Moves `data` into a new `fasm_bytes`.
pub(crate) fn bytes_into_raw(data: Vec<u8>) -> *mut fasm_bytes {
    Box::into_raw(Box::new(data)).cast::<fasm_bytes>()
}

/// Borrows the bytes behind `b`.
///
/// # Safety
///
/// `b` must be `NULL` or a live `fasm_bytes` from this library.
unsafe fn bytes_ref<'a>(b: *const fasm_bytes) -> Option<&'a Vec<u8>> {
    // SAFETY: a non-NULL `b` came from `bytes_into_raw` and is live.
    unsafe { b.cast::<Vec<u8>>().as_ref() }
}

/// Returns the data of `b` (owned by `b`, valid until `fasm_bytes_free`;
/// `NULL` for a `NULL` or empty `b`).
///
/// # Safety
///
/// `b` must be `NULL` or a live `fasm_bytes` from this library.
#[no_mangle]
pub unsafe extern "C" fn fasm_bytes_data(b: *const fasm_bytes) -> *const u8 {
    guard(std::ptr::null(), || {
        // SAFETY: forwarded from the caller.
        match unsafe { bytes_ref(b) } {
            Some(data) if !data.is_empty() => data.as_ptr(),
            _ => std::ptr::null(),
        }
    })
}

/// Returns the length of `b` in bytes (0 for a `NULL` `b`).
///
/// # Safety
///
/// `b` must be `NULL` or a live `fasm_bytes` from this library.
#[no_mangle]
pub unsafe extern "C" fn fasm_bytes_len(b: *const fasm_bytes) -> usize {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { bytes_ref(b) }.map_or(0, Vec::len)
    })
}

/// Frees `b`. `NULL` is accepted (no-op).
///
/// # Safety
///
/// `b` must be `NULL` or a live `fasm_bytes` from this library that is not
/// used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fasm_bytes_free(b: *mut fasm_bytes) {
    guard((), || {
        if !b.is_null() {
            // SAFETY: `b` came from `Box::into_raw` in `bytes_into_raw` and
            // is freed once (caller contract).
            drop(unsafe { Box::from_raw(b.cast::<Vec<u8>>()) });
        }
    });
}

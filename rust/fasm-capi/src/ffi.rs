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

//! Shared FFI plumbing: the borrowed string view [`fasm_str`], panic
//! guards and raw pointer helpers.

use std::ffi::c_char;
use std::panic::{self, AssertUnwindSafe};
use std::ptr;

use crate::error::{fasm_error, fasm_status, set_error, CapiError};

/// A borrowed string view: `len` bytes of UTF-8 text starting at `ptr`.
///
/// **Not NUL terminated.** When filled in by the library (comments,
/// annotations), it points into the owning object (a `fasm_file`, or the
/// line passed to a streaming callback) and is valid as long as that object
/// is; for an empty string `ptr` may point to a zero length buffer (never
/// `NULL` when filled in by the library). When passed to the library,
/// `ptr` may be `NULL` only if `len` is 0; the bytes must be valid UTF-8.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct fasm_str {
    /// First byte of the text.
    pub ptr: *const c_char,
    /// Length of the text in bytes (not counting any terminator).
    pub len: usize,
}

impl fasm_str {
    /// A view of `s`; valid as long as `s` is.
    pub(crate) fn from_str(s: &str) -> Self {
        fasm_str {
            ptr: s.as_ptr().cast::<c_char>(),
            len: s.len(),
        }
    }

    /// The empty view (`NULL`, 0).
    pub(crate) fn empty() -> Self {
        fasm_str {
            ptr: ptr::null(),
            len: 0,
        }
    }
}

/// Runs `f`, returning `default` if it panics. For the entry points that
/// cannot report an error (accessors).
pub(crate) fn guard<T>(default: T, f: impl FnOnce() -> T) -> T {
    panic::catch_unwind(AssertUnwindSafe(f)).unwrap_or(default)
}

/// A human readable description of a panic payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    let what = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic payload");
    format!("internal error (Rust panic): {what}")
}

/// Runs the fallible `f`, turning a panic into a
/// [`fasm_status::FASM_ERR_PANIC`] error.
pub(crate) fn catch<T>(f: impl FnOnce() -> Result<T, CapiError>) -> Result<T, CapiError> {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(payload) => Err(CapiError::new(
            fasm_status::FASM_ERR_PANIC,
            panic_message(&*payload),
        )),
    }
}

/// Stores the outcome of `result` in `*err` (the error, or `NULL` on
/// success) and returns the value, or `failure` on error.
///
/// # Safety
///
/// `err` must be `NULL` or valid for writing a pointer.
pub(crate) unsafe fn report<T>(
    err: *mut *mut fasm_error,
    failure: T,
    result: Result<T, CapiError>,
) -> T {
    match result {
        Ok(value) => {
            // SAFETY: the caller guarantees `err` is NULL or writable.
            unsafe { set_error(err, None) };
            value
        }
        Err(error) => {
            // SAFETY: as above.
            unsafe { set_error(err, Some(error)) };
            failure
        }
    }
}

/// Runs the fallible `f` (see [`catch`]) and reports its outcome in `*err`
/// (see [`report`]), returning `failure` on error.
///
/// # Safety
///
/// `err` must be `NULL` or valid for writing a pointer.
pub(crate) unsafe fn run<T>(
    err: *mut *mut fasm_error,
    failure: T,
    f: impl FnOnce() -> Result<T, CapiError>,
) -> T {
    // SAFETY: forwarded from the caller.
    unsafe { report(err, failure, catch(f)) }
}

/// [`run`] for functions returning a [`fasm_status`].
///
/// # Safety
///
/// `err` must be `NULL` or valid for writing a pointer.
pub(crate) unsafe fn run_status(
    err: *mut *mut fasm_error,
    f: impl FnOnce() -> Result<(), CapiError>,
) -> fasm_status {
    let result = catch(f);
    let status = match &result {
        Ok(()) => fasm_status::FASM_OK,
        Err(e) => e.status,
    };
    // SAFETY: forwarded from the caller.
    unsafe { report(err, (), result) };
    status
}

/// Borrows the bytes `ptr[..len]`; `NULL` is accepted for an empty slice.
///
/// # Safety
///
/// Unless `ptr` is `NULL`, it must be valid for reading `len` bytes that
/// stay unmodified for `'a`.
pub(crate) unsafe fn bytes<'a>(
    ptr: *const u8,
    len: usize,
    what: &str,
) -> Result<&'a [u8], CapiError> {
    if ptr.is_null() {
        if len == 0 {
            return Ok(&[]);
        }
        return Err(CapiError::invalid_arg(format!(
            "{what}: NULL pointer with a non-zero length ({len})"
        )));
    }
    // SAFETY: the caller guarantees `ptr[..len]` is readable for `'a`.
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
}

/// Borrows `ptr[..len]` as UTF-8 text.
///
/// # Safety
///
/// As for [`bytes`].
pub(crate) unsafe fn utf8<'a>(
    ptr: *const u8,
    len: usize,
    what: &str,
) -> Result<&'a str, CapiError> {
    // SAFETY: forwarded from the caller.
    let b = unsafe { bytes(ptr, len, what) }?;
    std::str::from_utf8(b).map_err(|e| {
        CapiError::new(
            fasm_status::FASM_ERR_UTF8,
            format!("{what} is not valid UTF-8: {e}"),
        )
    })
}

/// Borrows the text of a [`fasm_str`] passed in by the caller.
///
/// # Safety
///
/// As for [`bytes`], with `s.ptr` / `s.len`.
pub(crate) unsafe fn str_arg<'a>(s: &fasm_str, what: &str) -> Result<&'a str, CapiError> {
    // SAFETY: forwarded from the caller.
    unsafe { utf8(s.ptr.cast::<u8>(), s.len, what) }
}

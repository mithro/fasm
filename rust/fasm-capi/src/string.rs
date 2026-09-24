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

//! The owned string type [`fasm_string`].

use std::ffi::c_char;

use crate::ffi::guard;

/// An owned, immutable UTF-8 string returned by the library.
///
/// Its text is NUL terminated (`fasm_string_data`) and its length is
/// known (`fasm_string_len`); the text itself may contain NUL bytes (for
/// example from a comment), in which case only the length is reliable.
/// Owned by the caller and released with `fasm_string_free`.
pub struct fasm_string {
    _private: [u8; 0],
}

/// The Rust side of a [`fasm_string`]: the text followed by one NUL byte.
pub(crate) struct OwnedString {
    /// The text plus a trailing `\0` (not counted in the length).
    data: String,
}

impl OwnedString {
    /// Moves `s` into a new `fasm_string` and returns it (never `NULL`).
    pub(crate) fn into_raw(mut s: String) -> *mut fasm_string {
        s.push('\0');
        Box::into_raw(Box::new(OwnedString { data: s })).cast::<fasm_string>()
    }

    /// The text, without the trailing NUL.
    pub(crate) fn as_str(&self) -> &str {
        &self.data[..self.data.len() - 1]
    }
}

/// Borrows the [`OwnedString`] behind `s`.
///
/// # Safety
///
/// `s` must be `NULL` or a live `fasm_string` from this library.
pub(crate) unsafe fn inner<'a>(s: *const fasm_string) -> Option<&'a OwnedString> {
    // SAFETY: a non-NULL `s` was created by `OwnedString::into_raw` and is
    // still live (caller contract).
    unsafe { s.cast::<OwnedString>().as_ref() }
}

/// Returns the NUL terminated text of `s`, owned by `s` (valid until
/// `fasm_string_free`). Returns an empty string for a `NULL` `s`.
///
/// # Safety
///
/// `s` must be `NULL` or a live `fasm_string` from this library.
#[no_mangle]
pub unsafe extern "C" fn fasm_string_data(s: *const fasm_string) -> *const c_char {
    guard(c"".as_ptr(), || {
        // SAFETY: forwarded from the caller.
        match unsafe { inner(s) } {
            Some(s) => s.data.as_ptr().cast::<c_char>(),
            None => c"".as_ptr(),
        }
    })
}

/// Returns the length of `s` in bytes, not counting the NUL terminator (0
/// for a `NULL` `s`).
///
/// # Safety
///
/// `s` must be `NULL` or a live `fasm_string` from this library.
#[no_mangle]
pub unsafe extern "C" fn fasm_string_len(s: *const fasm_string) -> usize {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { inner(s) }.map_or(0, |s| s.as_str().len())
    })
}

/// Frees `s`. `NULL` is accepted (no-op).
///
/// # Safety
///
/// `s` must be `NULL` or a live `fasm_string` from this library that is not
/// used afterwards (in particular not freed twice).
#[no_mangle]
pub unsafe extern "C" fn fasm_string_free(s: *mut fasm_string) {
    guard((), || {
        if !s.is_null() {
            // SAFETY: `s` came from `Box::into_raw` in
            // `OwnedString::into_raw` and is freed once (caller contract).
            drop(unsafe { Box::from_raw(s.cast::<OwnedString>()) });
        }
    });
}

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

//! The [`fasm_file`] (a list of lines) and [`fasm_line`] handles.

use std::ptr;

use fasm::FasmLine;

use crate::ffi::guard;

/// A FASM model: an ordered list of lines (Python: the list returned by
/// `fasm.parse_fasm_string`).
///
/// Created by `fasm_parse_string`, `fasm_parse_file`, `fasm_file_new` and
/// `fasm_file_merge_and_sort*`; owned by the caller and released with
/// `fasm_file_free`. Reading a `fasm_file` from several threads at once is
/// safe; modifying it (`fasm_file_push_line`) requires exclusive access.
pub struct fasm_file {
    _private: [u8; 0],
}

/// One line of a FASM model (Python: `fasm.FasmLine`): an optional
/// `SetFasmFeature`, optional annotations and an optional comment.
///
/// Always borrowed: from a `fasm_file` (`fasm_file_line`, valid until the
/// file is freed or modified), or passed to a `fasm_line_callback` (valid
/// only during the callback).
pub struct fasm_line {
    _private: [u8; 0],
}

/// The Rust side of a [`fasm_file`].
pub(crate) struct FileInner {
    pub(crate) lines: Vec<FasmLine>,
}

impl FileInner {
    /// Moves `lines` into a new `fasm_file` and returns it (never `NULL`).
    pub(crate) fn into_raw(lines: Vec<FasmLine>) -> *mut fasm_file {
        Box::into_raw(Box::new(FileInner { lines })).cast::<fasm_file>()
    }
}

/// Borrows the lines of `file`.
///
/// # Safety
///
/// `file` must be `NULL` or a live `fasm_file` from this library that is
/// not modified for `'a`.
pub(crate) unsafe fn file_ref<'a>(file: *const fasm_file) -> Option<&'a FileInner> {
    // SAFETY: a non-NULL `file` came from `FileInner::into_raw` and is live
    // (caller contract).
    unsafe { file.cast::<FileInner>().as_ref() }
}

/// Mutably borrows the lines of `file`.
///
/// # Safety
///
/// `file` must be `NULL` or a live `fasm_file` from this library that is
/// not accessed by anything else for `'a`.
pub(crate) unsafe fn file_mut<'a>(file: *mut fasm_file) -> Option<&'a mut FileInner> {
    // SAFETY: as for `file_ref`, plus exclusivity (caller contract).
    unsafe { file.cast::<FileInner>().as_mut() }
}

/// A `fasm_line` handle for `line` (valid as long as `line` is).
pub(crate) fn line_ptr(line: &FasmLine) -> *const fasm_line {
    ptr::from_ref(line).cast::<fasm_line>()
}

/// Borrows the `FasmLine` behind `line`.
///
/// # Safety
///
/// `line` must be `NULL` or a `fasm_line` handed out by this library that
/// is still valid (see [`fasm_line`]) for `'a`.
pub(crate) unsafe fn line_ref<'a>(line: *const fasm_line) -> Option<&'a FasmLine> {
    // SAFETY: a non-NULL `line` was made by `line_ptr` from a `&FasmLine`
    // that is still live (caller contract).
    unsafe { line.cast::<FasmLine>().as_ref() }
}

/// Returns the number of lines in `file` (0 for a `NULL` `file`).
///
/// # Safety
///
/// `file` must be `NULL` or a live `fasm_file` from this library.
#[no_mangle]
pub unsafe extern "C" fn fasm_file_line_count(file: *const fasm_file) -> usize {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { file_ref(file) }.map_or(0, |f| f.lines.len())
    })
}

/// Returns line `index` (0 based) of `file`, or `NULL` if `file` is `NULL`
/// or `index >= fasm_file_line_count(file)`.
///
/// The line is borrowed from `file`: it stays valid until `file` is freed
/// or modified (`fasm_file_push_line`).
///
/// # Safety
///
/// `file` must be `NULL` or a live `fasm_file` from this library.
#[no_mangle]
pub unsafe extern "C" fn fasm_file_line(file: *const fasm_file, index: usize) -> *const fasm_line {
    guard(ptr::null(), || {
        // SAFETY: forwarded from the caller.
        unsafe { file_ref(file) }
            .and_then(|f| f.lines.get(index))
            .map_or(ptr::null(), line_ptr)
    })
}

/// Frees `file` and everything borrowed from it (lines, set features,
/// strings). `NULL` is accepted (no-op).
///
/// # Safety
///
/// `file` must be `NULL` or a live `fasm_file` from this library that is
/// not used afterwards (in particular not freed twice).
#[no_mangle]
pub unsafe extern "C" fn fasm_file_free(file: *mut fasm_file) {
    guard((), || {
        if !file.is_null() {
            // SAFETY: `file` came from `Box::into_raw` in
            // `FileInner::into_raw` and is freed once (caller contract).
            drop(unsafe { Box::from_raw(file.cast::<FileInner>()) });
        }
    });
}

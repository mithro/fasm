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

//! `merge_and_sort` (Python: `fasm.output.merge_and_sort`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_void};
use std::ptr;

use fasm::idstring::IdString;
use fasm::{merge_and_sort, merge_and_sort_by_key};

use crate::error::{fasm_error, CapiError};
use crate::ffi::{catch, report};
use crate::file::{fasm_file, file_ref, FileInner};

/// Callback deciding whether a feature is a "zero" feature for
/// `fasm_file_merge_and_sort_ex` (Python: `zero_function`).
///
/// `feature` is the feature name (NUL terminated, `len` bytes, valid only
/// during the call); `user` is the `user` pointer given to
/// `fasm_file_merge_and_sort_ex`. Must not unwind or `longjmp`.
pub type fasm_zero_fn =
    Option<unsafe extern "C" fn(feature: *const c_char, len: usize, user: *mut c_void) -> bool>;

/// Callback computing the sort key of a feature group for
/// `fasm_file_merge_and_sort_ex` (Python: `sort_key`).
///
/// `group_id` is the group id (the first `.` separated component of the
/// feature names, e.g. the tile name; NUL terminated, `len` bytes, valid
/// only during the call). It is called exactly once per group (the result
/// is cached), so it need not be deterministic. Groups are sorted by
/// increasing key, groups with equal keys by `group_id`. Must not unwind or
/// `longjmp`.
pub type fasm_sort_key_fn =
    Option<unsafe extern "C" fn(group_id: *const c_char, len: usize, user: *mut c_void) -> i64>;

/// Calls `f` with a NUL terminated copy of `s` in `buf` (reused between
/// calls).
fn with_c_str<R>(buf: &RefCell<Vec<u8>>, s: &str, f: impl FnOnce(*const c_char, usize) -> R) -> R {
    let mut buf = buf.borrow_mut();
    buf.clear();
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
    f(buf.as_ptr().cast::<c_char>(), s.len())
}

/// Groups and sorts the lines of `file` into a new `fasm_file` (Python:
/// `fasm.output.merge_and_sort(model)` without `zero_function` and
/// `sort_key`), leaving `file` unchanged.
///
/// Features are grouped by their first name component (the tile) and
/// groups sorted by name; runs of comment lines and annotation lines stay
/// attached to the following feature; address only features of one name
/// are merged into a single multi bit feature where possible; groups are
/// separated by a blank line. Returns `NULL` on error (`FASM_ERR_OUTPUT`
/// when features cannot be merged, `FASM_ERR_INVALID_ARG` for a `NULL`
/// `file`). Free the result with `fasm_file_free`.
///
/// # Safety
///
/// `file` must be `NULL` or a live `fasm_file`; `err` must be `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_file_merge_and_sort(
    file: *const fasm_file,
    err: *mut *mut fasm_error,
) -> *mut fasm_file {
    // SAFETY: forwarded from the caller.
    unsafe { fasm_file_merge_and_sort_ex(file, None, None, ptr::null_mut(), err) }
}

/// `fasm_file_merge_and_sort` with the optional Python callbacks:
/// `zero_fn` (a feature group whose features all answer `true` is
/// dropped) and `sort_key_fn` (group sort key; see `fasm_sort_key_fn`).
/// Either may be `NULL`. `user` is passed to both.
///
/// # Safety
///
/// `file` must be `NULL` or a live `fasm_file`; the callbacks must be
/// `NULL` or functions that can be called with `user`; `err` must be
/// `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_file_merge_and_sort_ex(
    file: *const fasm_file,
    zero_fn: fasm_zero_fn,
    sort_key_fn: fasm_sort_key_fn,
    user: *mut c_void,
    err: *mut *mut fasm_error,
) -> *mut fasm_file {
    let result = catch(|| {
        // SAFETY: `file` is NULL or live (caller contract).
        let file = unsafe { file_ref(file) }.ok_or_else(|| CapiError::null("file"))?;
        let buf = RefCell::new(Vec::new());
        let zero = zero_fn.map(|f| {
            move |name: &str| {
                // SAFETY: the name is NUL terminated and valid during the
                // call; `f` accepts `user` (caller contract).
                with_c_str(&buf, name, |p, len| unsafe { f(p, len, user) })
            }
        });
        let zero = zero.as_ref().map(|z| z as &dyn Fn(&str) -> bool);
        let lines = file.lines.iter().cloned();
        let merged = match sort_key_fn {
            None => merge_and_sort(lines, zero)?,
            Some(key_fn) => {
                // The core sort asks for a key once per comparison, and a
                // C key need not be deterministic (Rust's sort may panic
                // on an inconsistent order): call `key_fn` once per group
                // and cache the result. Group ids are already interned by
                // the core, so `IdString::new` does not allocate, and
                // `IdString`'s `Ord` compares the strings (the tie break).
                let key_buf = RefCell::new(Vec::new());
                let cache: RefCell<HashMap<IdString, i64>> = RefCell::new(HashMap::new());
                let key = |group_id: &str| {
                    let id = IdString::new(group_id);
                    let cached = cache.borrow().get(&id).copied();
                    let key = cached.unwrap_or_else(|| {
                        // SAFETY: as above.
                        let key = with_c_str(&key_buf, group_id, |p, len| unsafe {
                            key_fn(p, len, user)
                        });
                        cache.borrow_mut().insert(id, key);
                        key
                    });
                    (key, id)
                };
                merge_and_sort_by_key(lines, zero, &key)?
            }
        };
        Ok(FileInner::into_raw(merged))
    });
    // SAFETY: `err` is NULL or writable (caller contract).
    unsafe { report(err, ptr::null_mut(), result) }
}

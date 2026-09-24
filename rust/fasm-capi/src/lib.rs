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

//! C ABI (`libfasm_capi`) for the [`fasm`] crate.
//!
//! This crate builds a C library (cdylib + staticlib, library name
//! `fasm_capi`) whose header, `include/fasm/fasm.h`, is generated from
//! this crate's sources with cbindgen (`make capi-header`). Every item
//! exported to C is prefixed with `fasm_` (types and functions) or `FASM_`
//! (enum constants). Rust code normally uses the [`fasm`] crate directly;
//! the items are public here for the header generator, the documentation
//! and the tests.
//!
//! The API covers:
//!
//! * parsing a string or a file into a [`fasm_file`] (an array of lines,
//!   [`fasm_parse_string`], [`fasm_parse_file`]), or streaming the lines to
//!   a callback without collecting them ([`fasm_parse_string_cb`],
//!   [`fasm_parse_file_cb`]);
//! * read only access to the lines ([`fasm_line`]), their `SetFasmFeature`
//!   ([`fasm_set_feature`]), annotations and comment;
//! * building a [`fasm_file`] from C ([`fasm_file_new`],
//!   [`fasm_file_push_line`]);
//! * formatting ([`fasm_file_to_string`], canonical or not,
//!   [`fasm_line_to_string`], [`fasm_set_feature_to_string`]) and
//!   [`fasm_file_merge_and_sort`].
//!
//! See `docs/rewrite/DESIGN-capi.md` for the ownership, lifetime, error and
//! thread safety rules, which are also given on each function.
//!
//! # Conventions
//!
//! * Handles are opaque pointers. Objects returned by `fasm_file_new`,
//!   `fasm_parse_*`, `*_to_string`, `fasm_set_feature_name_string` and
//!   `fasm_file_merge_and_sort*` are owned by the caller and released with
//!   the matching `fasm_*_free` function (which accepts `NULL`). Pointers
//!   returned by accessors (`fasm_file_line`, `fasm_line_set_feature`, the
//!   [`fasm_str`]s filled in by `fasm_line_comment` /
//!   `fasm_line_annotation`) borrow from their parent object.
//! * Fallible functions take a last `fasm_error **err` argument, which may
//!   be `NULL` when the caller does not want the details. On failure a new
//!   [`fasm_error`] is stored in `*err` (free it with [`fasm_error_free`]);
//!   on success `*err` is set to `NULL`. They return either a
//!   [`fasm_status`] or a pointer that is `NULL` on failure.
//! * No Rust panic crosses the C boundary: every entry point catches
//!   panics and reports them as [`fasm_status::FASM_ERR_PANIC`] (or
//!   returns the documented neutral value for functions that cannot fail).
//! * `NULL` handles are accepted everywhere: accessors return a neutral
//!   value (0, `false`, `NULL`), fallible functions return
//!   `FASM_ERR_INVALID_ARG`.
//! * Text passed in is UTF-8 given as pointer + length (not necessarily
//!   NUL terminated), except file paths, which are NUL terminated.
//!   Borrowed text handed out ([`fasm_str`]) is **not** NUL terminated;
//!   owned text ([`fasm_string`]) is.

#![deny(unsafe_op_in_unsafe_fn)]
// The exported items use their C spelling (`fasm_file`, `FASM_OK`, ...) so
// that the generated header needs no renaming rules.
#![allow(non_camel_case_types)]

use std::ffi::c_char;

// Declaration order is the order of the declarations in the generated
// header (cbindgen `sort_by = "None"`).
mod builder;
mod error;
mod ffi;
mod file;
mod merge;
mod model;
mod output;
mod parse;
mod string;

pub use builder::{fasm_file_new, fasm_file_push_line, fasm_set_feature_spec};
pub use error::{
    fasm_error, fasm_error_column, fasm_error_free, fasm_error_line, fasm_error_message,
    fasm_error_status, fasm_status, fasm_status_string,
};
pub use ffi::fasm_str;
pub use file::{fasm_file, fasm_file_free, fasm_file_line, fasm_file_line_count, fasm_line};
pub use merge::{
    fasm_file_merge_and_sort, fasm_file_merge_and_sort_ex, fasm_sort_key_fn, fasm_zero_fn,
};
pub use model::{
    fasm_annotation, fasm_line_annotation, fasm_line_annotation_count, fasm_line_comment,
    fasm_line_has_comment, fasm_line_has_set_feature, fasm_line_set_feature, fasm_set_feature,
    fasm_set_feature_end, fasm_set_feature_has_end, fasm_set_feature_has_start,
    fasm_set_feature_name, fasm_set_feature_name_len, fasm_set_feature_name_string,
    fasm_set_feature_start, fasm_set_feature_value_bit, fasm_set_feature_value_bits,
    fasm_set_feature_value_bytes_le, fasm_set_feature_value_format,
    fasm_set_feature_value_to_string, fasm_set_feature_value_u64, fasm_set_feature_width,
    fasm_value_format,
};
pub use output::{fasm_file_to_string, fasm_line_to_string, fasm_set_feature_to_string};
pub use parse::{
    fasm_line_callback, fasm_parse_file, fasm_parse_file_cb, fasm_parse_string,
    fasm_parse_string_cb,
};
pub use string::{fasm_string, fasm_string_data, fasm_string_free, fasm_string_len};

/// Returns the library version as a static NUL terminated string (never
/// `NULL`, never freed), e.g. `"0.1.0"`.
#[no_mangle]
pub extern "C" fn fasm_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0")
        .as_ptr()
        .cast::<c_char>()
}

#[cfg(test)]
mod tests;

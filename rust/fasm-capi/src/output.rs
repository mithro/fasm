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

//! Formatting models back to FASM text.

use std::ptr;

use fasm::output as fasm_output;
use fasm::{fasm_tuple_to_string, set_feature_to_str};

use crate::error::{fasm_error, CapiError};
use crate::ffi::run;
use crate::file::{fasm_file, fasm_line, file_ref, line_ref};
use crate::model::{fasm_set_feature, set_feature_ref};
use crate::string::{fasm_string, OwnedString};

/// Renders all lines of `file` as FASM text (Python:
/// `fasm.fasm_tuple_to_string(model, canonical)`): one line per
/// `fasm_line`, each ending in `\n` (an empty file gives `"\n"`), with
/// optional whitespace normalised to single spaces.
///
/// With `canonical`, every set feature is expanded to one line per set
/// bit (`FEATURE[bit]`, or `FEATURE` for bit 0 of a feature without
/// address), cleared bits, annotations and comments are dropped, and the
/// lines are sorted and deduplicated.
///
/// Returns a new `fasm_string` (free with `fasm_string_free`), or `NULL`
/// on error (`FASM_ERR_INVALID_ARG` for a `NULL` `file`, `FASM_ERR_OUTPUT`
/// if a feature cannot be formatted).
///
/// # Safety
///
/// `file` must be `NULL` or a live `fasm_file`; `err` must be `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_file_to_string(
    file: *const fasm_file,
    canonical: bool,
    err: *mut *mut fasm_error,
) -> *mut fasm_string {
    // SAFETY: forwarded from the caller.
    unsafe {
        run(err, ptr::null_mut(), || {
            let file = file_ref(file).ok_or_else(|| CapiError::null("file"))?;
            let text = fasm_tuple_to_string(&file.lines, canonical)?;
            Ok(OwnedString::into_raw(text))
        })
    }
}

/// Renders one line (Python: `fasm.fasm_line_to_string`, whose list of
/// strings is joined with `\n` here, without a trailing `\n`).
///
/// Without `canonical` this is exactly one line of text (possibly empty,
/// for a blank line). With `canonical` it is the canonical lines of the
/// line's set feature (none, i.e. an empty string, for a line without one
/// or with the value 0), in bit order and not deduplicated.
///
/// Returns a new `fasm_string`, or `NULL` on error (as
/// `fasm_file_to_string`).
///
/// # Safety
///
/// `line` must be `NULL` or a valid `fasm_line`; `err` must be `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_line_to_string(
    line: *const fasm_line,
    canonical: bool,
    err: *mut *mut fasm_error,
) -> *mut fasm_string {
    // SAFETY: forwarded from the caller.
    unsafe {
        run(err, ptr::null_mut(), || {
            let line = line_ref(line).ok_or_else(|| CapiError::null("line"))?;
            let lines = fasm_output::fasm_line_to_string(line, canonical)?;
            Ok(OwnedString::into_raw(lines.join("\n")))
        })
    }
}

/// Renders a set feature (Python: `fasm.set_feature_to_str(set_feature,
/// check_if_canonical)`), e.g. `A.B[7:0] = 8'hFF`.
///
/// With `check_if_canonical`, a feature that is not in canonical form
/// (wider than one bit, with an end address, a start address of 0 or a
/// written value) is `FASM_ERR_OUTPUT`.
///
/// Returns a new `fasm_string`, or `NULL` on error.
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`; `err` must be `NULL`
/// or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_to_string(
    sf: *const fasm_set_feature,
    check_if_canonical: bool,
    err: *mut *mut fasm_error,
) -> *mut fasm_string {
    // SAFETY: forwarded from the caller.
    unsafe {
        run(err, ptr::null_mut(), || {
            let sf = set_feature_ref(sf).ok_or_else(|| CapiError::null("set_feature"))?;
            Ok(OwnedString::into_raw(set_feature_to_str(
                sf,
                check_if_canonical,
            )?))
        })
    }
}

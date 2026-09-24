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

//! Building a [`fasm_file`] from C: [`fasm_file_new`] and
//! [`fasm_file_push_line`].

use fasm::idstring::IdString;
use fasm::{Annotation, FasmLine, FeatureValue, SetFasmFeature, ValueFormat};

use crate::error::{fasm_error, fasm_status, CapiError};
use crate::ffi::{bytes, fasm_str, guard, run_status, str_arg};
use crate::file::{fasm_file, file_mut, FileInner};
use crate::model::fasm_annotation;

/// Input description of a `SetFasmFeature` for `fasm_file_push_line`.
///
/// All pointers are only read during the call (the library copies what it
/// keeps).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct fasm_set_feature_spec {
    /// The feature name (UTF-8, e.g. `CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT`).
    /// Like the Python model, the name is not checked against the FASM
    /// grammar.
    pub feature: fasm_str,
    /// Whether `start` is given (`FEATURE[start]` or `FEATURE[end:start]`).
    pub has_start: bool,
    /// Start (low) address bit; ignored unless `has_start`.
    pub start: u32,
    /// Whether `end` is given (`FEATURE[end:start]`; requires `has_start`).
    pub has_end: bool,
    /// End (high) address bit; ignored unless `has_end`.
    pub end: u32,
    /// The value as `value_len` little endian bytes (any length; `NULL`
    /// with `value_len == 0` is the value 0).
    pub value_le: *const u8,
    /// Number of bytes at `value_le`.
    pub value_len: usize,
    /// One of the `fasm_value_format` values (`FASM_VALUE_FORMAT_NONE` for
    /// no written value, which is printed as the bare feature name and
    /// normally goes with the value 1). An `int32_t` so that any value can
    /// be checked.
    pub value_format: i32,
}

/// Builds a `FeatureValue` from little endian bytes.
fn value_from_le_bytes(b: &[u8]) -> FeatureValue {
    let limbs: Vec<u64> = b
        .chunks(8)
        .map(|chunk| {
            let mut limb = [0u8; 8];
            limb[..chunk.len()].copy_from_slice(chunk);
            u64::from_le_bytes(limb)
        })
        .collect();
    FeatureValue::from_le_limbs(&limbs)
}

/// Validates a [`fasm_set_feature_spec`].
///
/// # Safety
///
/// The pointers in `spec` must satisfy the [`fasm_set_feature_spec`]
/// contract.
unsafe fn build_set_feature(spec: &fasm_set_feature_spec) -> Result<SetFasmFeature, CapiError> {
    // SAFETY: forwarded from the caller.
    let name = unsafe { str_arg(&spec.feature, "set_feature.feature") }?;
    // SAFETY: forwarded from the caller.
    let value = unsafe { bytes(spec.value_le, spec.value_len, "set_feature.value_le") }?;
    let value_format = match spec.value_format {
        -1 => None,
        v => Some(
            u8::try_from(v)
                .ok()
                .and_then(|v| ValueFormat::try_from(v).ok())
                .ok_or_else(|| {
                    CapiError::invalid_arg(format!(
                        "set_feature.value_format: {v} is not a fasm_value_format (-1 to 4)"
                    ))
                })?,
        ),
    };
    Ok(SetFasmFeature::new(
        IdString::new(name),
        spec.has_start.then_some(spec.start),
        spec.has_end.then_some(spec.end),
        value_from_le_bytes(value),
        value_format,
    )?)
}

/// Creates a new, empty `fasm_file` (free with `fasm_file_free`). Never
/// returns `NULL` (except on memory exhaustion, which aborts).
#[no_mangle]
pub extern "C" fn fasm_file_new() -> *mut fasm_file {
    guard(std::ptr::null_mut(), || FileInner::into_raw(Vec::new()))
}

/// Appends a line to `file` (Python: appending a `fasm.FasmLine` to a
/// model).
///
/// * `set_feature`: the `SetFasmFeature`, or `NULL` for none. Validated
///   like `fasm.SetFasmFeature`: end without start, end before start and
///   a value wider than the address are `FASM_ERR_INVALID_ARG`.
/// * `annotations[0..annotation_count]`: the annotations (`NULL` with a
///   count of 0 for none). Names and values are copied verbatim.
/// * `comment`: the comment text after `#`, or `NULL` for no comment (a
///   non-`NULL` empty string is a bare `#`).
///
/// All strings must be UTF-8 (else `FASM_ERR_UTF8`). On any error nothing
/// is appended. Appending invalidates the `fasm_line` (and
/// `fasm_set_feature`, `fasm_str`) pointers previously obtained from
/// `file`.
///
/// # Safety
///
/// `file` must be `NULL` or a live `fasm_file` not accessed concurrently;
/// `set_feature` and `comment` must be `NULL` or readable (with the
/// strings they point to); `annotations` must be `NULL` or valid for
/// reading `annotation_count` entries; `err` must be `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_file_push_line(
    file: *mut fasm_file,
    set_feature: *const fasm_set_feature_spec,
    annotations: *const fasm_annotation,
    annotation_count: usize,
    comment: *const fasm_str,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: every pointer satisfies the contract above (caller).
    unsafe {
        run_status(err, || {
            let file = file_mut(file).ok_or_else(|| CapiError::null("file"))?;
            let set_feature = match set_feature.as_ref() {
                Some(spec) => Some(build_set_feature(spec)?),
                None => None,
            };
            let annotations = if annotation_count == 0 {
                None
            } else if annotations.is_null() {
                return Err(CapiError::invalid_arg(format!(
                    "annotations: NULL pointer with a non-zero count ({annotation_count})"
                )));
            } else {
                let specs = std::slice::from_raw_parts(annotations, annotation_count);
                let mut out = Vec::with_capacity(specs.len());
                for a in specs {
                    let name = str_arg(&a.name, "annotation name")?;
                    let value = str_arg(&a.value, "annotation value")?;
                    out.push(Annotation::new(name, value));
                }
                Some(out)
            };
            let comment = match comment.as_ref() {
                Some(c) => Some(Box::<str>::from(str_arg(c, "comment")?)),
                None => None,
            };
            file.lines.push(FasmLine {
                set_feature,
                annotations,
                comment,
            });
            Ok(())
        })
    }
}

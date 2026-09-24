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

//! Read only access to lines ([`fasm_line`]) and their `SetFasmFeature`
//! ([`fasm_set_feature`]).

use std::ffi::c_char;
use std::ptr;

use fasm::{SetFasmFeature, ValueFormat};

use crate::error::{fasm_error, CapiError};
use crate::ffi::{fasm_str, guard, run};
use crate::file::{fasm_line, line_ref};
use crate::string::{fasm_string, OwnedString};

/// The `SetFasmFeature` of a line (Python: `fasm.SetFasmFeature`): a
/// feature name, an optional `[end:start]` address, a value and the
/// format the value was written in.
///
/// Always borrowed from its `fasm_line` (`fasm_line_set_feature`) and
/// valid as long as that line is.
pub struct fasm_set_feature {
    _private: [u8; 0],
}

/// How the value of a `SetFasmFeature` is written (Python:
/// `fasm.ValueFormat`, whose values 0 to 4 are kept), or
/// `FASM_VALUE_FORMAT_NONE` when no value was written (`FEATURE` or
/// `FEATURE[3]`, an implicit 1; Python: `value_format is None`).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum fasm_value_format {
    /// No value written (implicit 1).
    FASM_VALUE_FORMAT_NONE = -1,
    /// A plain decimal number, e.g. `42`.
    FASM_VALUE_FORMAT_PLAIN = 0,
    /// A Verilog decimal number, e.g. `8'd42`.
    FASM_VALUE_FORMAT_VERILOG_DECIMAL = 1,
    /// A Verilog hexadecimal number, e.g. `8'h2A`.
    FASM_VALUE_FORMAT_VERILOG_HEX = 2,
    /// A Verilog binary number, e.g. `8'b00101010`.
    FASM_VALUE_FORMAT_VERILOG_BINARY = 3,
    /// A Verilog octal number, e.g. `8'o52`.
    FASM_VALUE_FORMAT_VERILOG_OCTAL = 4,
}

impl From<Option<ValueFormat>> for fasm_value_format {
    fn from(format: Option<ValueFormat>) -> Self {
        match format {
            None => fasm_value_format::FASM_VALUE_FORMAT_NONE,
            Some(ValueFormat::Plain) => fasm_value_format::FASM_VALUE_FORMAT_PLAIN,
            Some(ValueFormat::VerilogDecimal) => {
                fasm_value_format::FASM_VALUE_FORMAT_VERILOG_DECIMAL
            }
            Some(ValueFormat::VerilogHex) => fasm_value_format::FASM_VALUE_FORMAT_VERILOG_HEX,
            Some(ValueFormat::VerilogBinary) => fasm_value_format::FASM_VALUE_FORMAT_VERILOG_BINARY,
            Some(ValueFormat::VerilogOctal) => fasm_value_format::FASM_VALUE_FORMAT_VERILOG_OCTAL,
        }
    }
}

/// One `name = "value"` annotation of a line, as two borrowed strings
/// (not NUL terminated). The value is the raw text between the quotes:
/// escapes such as `\"` are not decoded.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct fasm_annotation {
    /// The annotation name.
    pub name: fasm_str,
    /// The annotation value (possibly empty).
    pub value: fasm_str,
}

/// A `fasm_set_feature` handle for `sf` (valid as long as `sf` is).
pub(crate) fn set_feature_ptr(sf: &SetFasmFeature) -> *const fasm_set_feature {
    ptr::from_ref(sf).cast::<fasm_set_feature>()
}

/// Borrows the `SetFasmFeature` behind `sf`.
///
/// # Safety
///
/// `sf` must be `NULL` or a `fasm_set_feature` handed out by this library
/// that is still valid for `'a`.
pub(crate) unsafe fn set_feature_ref<'a>(
    sf: *const fasm_set_feature,
) -> Option<&'a SetFasmFeature> {
    // SAFETY: a non-NULL `sf` was made by `set_feature_ptr` from a live
    // `&SetFasmFeature` (caller contract).
    unsafe { sf.cast::<SetFasmFeature>().as_ref() }
}

// ---------------------------------------------------------------------
// fasm_line
// ---------------------------------------------------------------------

/// Returns `true` if `line` has a `SetFasmFeature` (`false` for `NULL`).
///
/// # Safety
///
/// `line` must be `NULL` or a valid `fasm_line` (see `fasm_line`).
#[no_mangle]
pub unsafe extern "C" fn fasm_line_has_set_feature(line: *const fasm_line) -> bool {
    guard(false, || {
        // SAFETY: forwarded from the caller.
        unsafe { line_ref(line) }.is_some_and(|l| l.set_feature.is_some())
    })
}

/// Returns the `SetFasmFeature` of `line`, borrowed from `line`, or `NULL`
/// if it has none (or `line` is `NULL`).
///
/// # Safety
///
/// `line` must be `NULL` or a valid `fasm_line` (see `fasm_line`).
#[no_mangle]
pub unsafe extern "C" fn fasm_line_set_feature(line: *const fasm_line) -> *const fasm_set_feature {
    guard(ptr::null(), || {
        // SAFETY: forwarded from the caller.
        unsafe { line_ref(line) }
            .and_then(|l| l.set_feature.as_ref())
            .map_or(ptr::null(), set_feature_ptr)
    })
}

/// Returns the number of annotations of `line` (0 when it has no
/// `{ ... }` block, or `line` is `NULL`).
///
/// # Safety
///
/// `line` must be `NULL` or a valid `fasm_line` (see `fasm_line`).
#[no_mangle]
pub unsafe extern "C" fn fasm_line_annotation_count(line: *const fasm_line) -> usize {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { line_ref(line) }
            .and_then(|l| l.annotations.as_ref())
            .map_or(0, Vec::len)
    })
}

/// Stores annotation `index` (0 based, in file order) of `line` in `*out`
/// and returns `true`; the strings are borrowed from `line`.
///
/// Returns `false` (leaving `*out` unchanged) if `line` or `out` is `NULL`
/// or `index >= fasm_line_annotation_count(line)`.
///
/// # Safety
///
/// `line` must be `NULL` or a valid `fasm_line`; `out` must be `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_line_annotation(
    line: *const fasm_line,
    index: usize,
    out: *mut fasm_annotation,
) -> bool {
    guard(false, || {
        if out.is_null() {
            return false;
        }
        // SAFETY: forwarded from the caller.
        let Some(annotation) = unsafe { line_ref(line) }
            .and_then(|l| l.annotations.as_ref())
            .and_then(|a| a.get(index))
        else {
            return false;
        };
        // SAFETY: `out` is not NULL and writable (caller contract).
        unsafe {
            out.write(fasm_annotation {
                name: fasm_str::from_str(&annotation.name),
                value: fasm_str::from_str(&annotation.value),
            });
        }
        true
    })
}

/// Returns `true` if `line` has a comment (`#`, possibly with empty text).
/// `false` for `NULL`.
///
/// # Safety
///
/// `line` must be `NULL` or a valid `fasm_line` (see `fasm_line`).
#[no_mangle]
pub unsafe extern "C" fn fasm_line_has_comment(line: *const fasm_line) -> bool {
    guard(false, || {
        // SAFETY: forwarded from the caller.
        unsafe { line_ref(line) }.is_some_and(|l| l.comment.is_some())
    })
}

/// Stores the comment text of `line` (everything after `#`, verbatim,
/// borrowed from `line`, not NUL terminated) in `*out` and returns
/// `true`.
///
/// Returns `false` and stores an empty view (`NULL`, 0) if `line` has no
/// comment or is `NULL`; `out` may be `NULL` to only test for a comment.
///
/// # Safety
///
/// `line` must be `NULL` or a valid `fasm_line`; `out` must be `NULL` or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_line_comment(line: *const fasm_line, out: *mut fasm_str) -> bool {
    guard(false, || {
        // SAFETY: forwarded from the caller.
        let comment = unsafe { line_ref(line) }.and_then(|l| l.comment.as_deref());
        if !out.is_null() {
            let view = comment.map_or(fasm_str::empty(), fasm_str::from_str);
            // SAFETY: `out` is not NULL and writable (caller contract).
            unsafe { out.write(view) };
        }
        comment.is_some()
    })
}

// ---------------------------------------------------------------------
// fasm_set_feature
// ---------------------------------------------------------------------

/// Returns the length in bytes of the feature name of `sf` (0 for
/// `NULL`).
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_name_len(sf: *const fasm_set_feature) -> usize {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { set_feature_ref(sf) }.map_or(0, |sf| sf.feature.len())
    })
}

/// Copies the feature name of `sf` into `buf` with `snprintf` semantics
/// and returns its full length in bytes (not counting the NUL).
///
/// Feature names are stored interned and split at `.`, so there is no
/// contiguous string to borrow; this copies without allocating. If
/// `buf_len > 0`, at most `buf_len - 1` bytes are copied and a NUL is
/// appended; the name was truncated if the return value is `>= buf_len`.
/// `buf` may be `NULL` when `buf_len` is 0 (to query the length). Returns 0
/// for a `NULL` `sf` (and stores an empty string if `buf_len > 0`).
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`; `buf` must be valid
/// for writing `buf_len` bytes (or `NULL` with `buf_len == 0`).
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_name(
    sf: *const fasm_set_feature,
    buf: *mut c_char,
    buf_len: usize,
) -> usize {
    guard(0, || {
        let copy = |name: &str| {
            if !buf.is_null() && buf_len > 0 {
                let n = name.len().min(buf_len - 1);
                // SAFETY: `buf` is writable for `buf_len > n` bytes
                // (caller contract) and does not overlap the interned
                // name.
                unsafe {
                    ptr::copy_nonoverlapping(name.as_ptr(), buf.cast::<u8>(), n);
                    buf.add(n).write(0);
                }
            }
            name.len()
        };
        // SAFETY: forwarded from the caller.
        match unsafe { set_feature_ref(sf) } {
            Some(sf) => sf.feature.with_str(copy),
            None => copy(""),
        }
    })
}

/// Returns the feature name of `sf` as a new `fasm_string` (free with
/// `fasm_string_free`), or `NULL` if `sf` is `NULL`.
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_name_string(
    sf: *const fasm_set_feature,
) -> *mut fasm_string {
    guard(ptr::null_mut(), || {
        // SAFETY: forwarded from the caller.
        unsafe { set_feature_ref(sf) }.map_or(ptr::null_mut(), |sf| {
            OwnedString::into_raw(sf.feature.resolve())
        })
    })
}

/// Returns `true` if `sf` has a `FeatureAddress` (`FEATURE[start]` or
/// `FEATURE[end:start]`). `false` for `NULL`.
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_has_start(sf: *const fasm_set_feature) -> bool {
    guard(false, || {
        // SAFETY: forwarded from the caller.
        unsafe { set_feature_ref(sf) }.is_some_and(|sf| sf.start.is_some())
    })
}

/// Returns the start (low) bit of the address of `sf`, or 0 if it has no
/// address (check `fasm_set_feature_has_start`) or `sf` is `NULL`.
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_start(sf: *const fasm_set_feature) -> u32 {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { set_feature_ref(sf) }
            .and_then(|sf| sf.start)
            .unwrap_or(0)
    })
}

/// Returns `true` if `sf` has an end (high) bit (`FEATURE[end:start]`).
/// `false` for `NULL`.
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_has_end(sf: *const fasm_set_feature) -> bool {
    guard(false, || {
        // SAFETY: forwarded from the caller.
        unsafe { set_feature_ref(sf) }.is_some_and(|sf| sf.end.is_some())
    })
}

/// Returns the end (high) bit of the address of `sf`, or 0 if it has none
/// (check `fasm_set_feature_has_end`) or `sf` is `NULL`.
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_end(sf: *const fasm_set_feature) -> u32 {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { set_feature_ref(sf) }
            .and_then(|sf| sf.end)
            .unwrap_or(0)
    })
}

/// Returns the format the value of `sf` was written in, or
/// `FASM_VALUE_FORMAT_NONE` if no value was written (or `sf` is `NULL`).
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_value_format(
    sf: *const fasm_set_feature,
) -> fasm_value_format {
    guard(fasm_value_format::FASM_VALUE_FORMAT_NONE, || {
        // SAFETY: forwarded from the caller.
        unsafe { set_feature_ref(sf) }.map_or(fasm_value_format::FASM_VALUE_FORMAT_NONE, |sf| {
            sf.value_format.into()
        })
    })
}

/// Returns the width in bits of the address of `sf`: 1 without an address
/// or with a single bit address, `end - start + 1` for a range (Python:
/// `fasm.set_feature_width`). 0 for `NULL`.
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_width(sf: *const fasm_set_feature) -> u32 {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { set_feature_ref(sf) }.map_or(0, SetFasmFeature::width)
    })
}

/// Returns the number of bits needed to hold the value of `sf`: 0 for the
/// value 0, otherwise one more than the index of its highest set bit. 0
/// for `NULL`.
///
/// Values have no size limit (a 256 bit BRAM `INIT` value is common); use
/// `fasm_set_feature_value_bytes_le` or `fasm_set_feature_value_bit` for
/// values wider than 64 bits.
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_value_bits(sf: *const fasm_set_feature) -> u32 {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { set_feature_ref(sf) }.map_or(0, |sf| sf.value.bit_len())
    })
}

/// Stores the value of `sf` in `*out` and returns `true` if it fits in 64
/// bits; returns `false` (leaving `*out` unchanged) if it does not, or
/// `sf` or `out` is `NULL`.
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`; `out` must be `NULL`
/// or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_value_u64(
    sf: *const fasm_set_feature,
    out: *mut u64,
) -> bool {
    guard(false, || {
        // SAFETY: forwarded from the caller.
        let Some(value) = unsafe { set_feature_ref(sf) }.and_then(|sf| sf.value.to_u64()) else {
            return false;
        };
        if out.is_null() {
            return false;
        }
        // SAFETY: `out` is not NULL and writable (caller contract).
        unsafe { out.write(value) };
        true
    })
}

/// Returns bit `index` of the value of `sf` (bit 0 is the least
/// significant); `false` beyond `fasm_set_feature_value_bits` and for
/// `NULL`.
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_value_bit(
    sf: *const fasm_set_feature,
    index: u32,
) -> bool {
    guard(false, || {
        // SAFETY: forwarded from the caller.
        unsafe { set_feature_ref(sf) }.is_some_and(|sf| sf.value.bit(index))
    })
}

/// Copies the value of `sf` into `buf` as a little endian byte string and
/// returns the number of bytes the value needs: `(value_bits + 7) / 8`, so
/// 0 for the value 0 (and for `NULL`).
///
/// If `buf_len` is at least the returned size, the value is written to
/// `buf[0..size]` and the rest of `buf[0..buf_len]` is zero filled (so a
/// 32 byte buffer receives any value of up to 256 bits, zero extended).
/// Otherwise `buf` is not touched: call again with a large enough buffer.
/// `buf` may be `NULL` when `buf_len` is 0 (to query the size).
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`; `buf` must be valid
/// for writing `buf_len` bytes (or `NULL` with `buf_len == 0`).
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_value_bytes_le(
    sf: *const fasm_set_feature,
    buf: *mut u8,
    buf_len: usize,
) -> usize {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        let Some(sf) = (unsafe { set_feature_ref(sf) }) else {
            return 0;
        };
        let needed = sf.value.bit_len().div_ceil(8) as usize;
        if !buf.is_null() && buf_len >= needed {
            // SAFETY: `buf` is valid for writing `buf_len` bytes (caller
            // contract); a `u8` has no invalid bit patterns.
            let out = unsafe {
                ptr::write_bytes(buf, 0, buf_len);
                std::slice::from_raw_parts_mut(buf, buf_len)
            };
            for bit in sf.value.iter_set_bits() {
                out[(bit / 8) as usize] |= 1 << (bit % 8);
            }
        }
        needed
    })
}

/// Returns the value of `sf` as digits in `radix` (2, 8, 10 or 16), with no
/// prefix or width and without leading zeros (`"0"` for 0), hex digits in
/// upper case if `uppercase`. Free with `fasm_string_free`.
///
/// Returns `NULL` (and sets `*err` to a `FASM_ERR_INVALID_ARG` error) if
/// `sf` is `NULL` or `radix` is not supported.
///
/// # Safety
///
/// `sf` must be `NULL` or a valid `fasm_set_feature`; `err` must be `NULL`
/// or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_set_feature_value_to_string(
    sf: *const fasm_set_feature,
    radix: u32,
    uppercase: bool,
    err: *mut *mut fasm_error,
) -> *mut fasm_string {
    // SAFETY: forwarded from the caller.
    unsafe {
        run(err, ptr::null_mut(), || {
            let sf = set_feature_ref(sf).ok_or_else(|| CapiError::null("set_feature"))?;
            if !matches!(radix, 2 | 8 | 10 | 16) {
                return Err(CapiError::invalid_arg(format!(
                    "unsupported radix {radix} (must be 2, 8, 10 or 16)"
                )));
            }
            Ok(OwnedString::into_raw(
                sf.value.to_radix_string(radix, uppercase),
            ))
        })
    }
}

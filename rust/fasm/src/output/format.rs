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

//! [`fasm_value_to_str`] / [`set_feature_to_str`] and their allocation free
//! `fmt::Write` counterparts: the Rust equivalent of Python's
//! `fasm_value_to_str` and `set_feature_to_str` in `fasm/__init__.py`.

use std::fmt;

use super::super::model::{FeatureValue, SetFasmFeature, ValueFormat};
use super::error::OutputError;

/// Formats `value` (of the given `width`, only used by the Verilog formats)
/// in `value_format`.
///
/// Mirrors Python's `fasm_value_to_str` in `fasm/__init__.py` exactly:
///
/// * [`ValueFormat::Plain`]: plain decimal, e.g. `"42"` (`width` unused,
///   exactly like the Python `'{}'.format(value)`).
/// * [`ValueFormat::VerilogHex`]: `"{width}'h{value:X}"`, uppercase hex,
///   e.g. `"8'hFF"`.
/// * [`ValueFormat::VerilogDecimal`]: `"{width}'d{value}"`, e.g. `"8'd42"`.
/// * [`ValueFormat::VerilogOctal`]: `"{width}'o{value:o}"`, e.g. `"8'o52"`.
/// * [`ValueFormat::VerilogBinary`]: `"{width}'b{value:b}"`, e.g.
///   `"8'b00101010"`.
#[must_use]
pub fn fasm_value_to_str(value: &FeatureValue, width: u32, value_format: ValueFormat) -> String {
    let mut s = String::new();
    // `String`'s `fmt::Write` impl never fails.
    write_fasm_value(&mut s, value, width, value_format).expect("String write is infallible");
    s
}

/// Allocation-free (beyond what `w` itself needs) counterpart of
/// [`fasm_value_to_str`], writing directly into `w`.
///
/// # Errors
///
/// Propagates any error from `w`.
pub fn write_fasm_value(
    w: &mut impl fmt::Write,
    value: &FeatureValue,
    width: u32,
    value_format: ValueFormat,
) -> fmt::Result {
    match value_format {
        ValueFormat::Plain => write!(w, "{value}"),
        ValueFormat::VerilogHex => write!(w, "{width}'h{}", value.to_radix_string(16, true)),
        ValueFormat::VerilogDecimal => write!(w, "{width}'d{value}"),
        ValueFormat::VerilogOctal => write!(w, "{width}'o{}", value.to_radix_string(8, false)),
        ValueFormat::VerilogBinary => write!(w, "{width}'b{}", value.to_radix_string(2, false)),
    }
}

/// Converts `set_feature` to its FASM source text, e.g.
/// `"A.B[7:0] = 8'hFF"`, `"A.B[3]"`, `"A.B = 1"`.
///
/// Mirrors Python's `set_feature_to_str` in `fasm/__init__.py`. If
/// `check_if_canonical` is `true`, also validates that `set_feature` is in
/// canonical form (width 1, no `end`, `start` either absent or nonzero, no
/// `value_format`), matching the Python function's `assert`s under
/// `check_if_canonical`.
///
/// # Errors
///
/// * [`OutputError::EndWithoutStart`], [`OutputError::EndBeforeStart`],
///   [`OutputError::AddressRangeTooWide`] if `set_feature` violates the
///   `SetFasmFeature` invariants in a way that would otherwise make
///   [`super::super::model::SetFasmFeature::width`] panic (only reachable
///   for a `set_feature` built with
///   [`super::super::model::SetFasmFeature::new_unchecked`], since
///   [`super::super::model::SetFasmFeature::new`] rejects all three at
///   construction).
/// * [`OutputError::ValueTooWideForFeature`] if `set_feature.value` needs
///   more bits than `set_feature.width()` allows (Python: `assert
///   set_feature.value < 2**width`; likewise only reachable via
///   `new_unchecked`).
/// * [`OutputError::NotCanonicalWidth`], [`OutputError::NotCanonicalHasEnd`],
///   [`OutputError::NotCanonicalStartZero`],
///   [`OutputError::NotCanonicalHasValueFormat`] if `check_if_canonical` is
///   `true` and the corresponding Python `assert` would have failed.
pub fn set_feature_to_str(
    set_feature: &SetFasmFeature,
    check_if_canonical: bool,
) -> Result<String, OutputError> {
    let mut s = String::new();
    write_set_feature(&mut s, set_feature, check_if_canonical)?;
    Ok(s)
}

/// [`super::super::model::SetFasmFeature::width`], without the panics that
/// method documents: it assumes the `SetFasmFeature` invariants
/// [`super::super::model::SetFasmFeature::new`] enforces (`end.is_some()`
/// implies `start.is_some()`, `end >= start`, and `end - start + 1` fits in
/// a `u32`) instead of relying on the caller to have upheld them. Only a
/// `set_feature` built with
/// [`super::super::model::SetFasmFeature::new_unchecked`] from
/// inconsistent inputs can fail these checks.
fn checked_width(set_feature: &SetFasmFeature) -> Result<u32, OutputError> {
    match set_feature.end {
        None => Ok(1),
        Some(end) => {
            let start = set_feature.start.ok_or(OutputError::EndWithoutStart)?;
            if end < start {
                return Err(OutputError::EndBeforeStart { start, end });
            }
            end.checked_sub(start)
                .and_then(|diff| diff.checked_add(1))
                .ok_or(OutputError::AddressRangeTooWide { start, end })
        }
    }
}

/// Allocation-free (beyond what `w` itself needs) counterpart of
/// [`set_feature_to_str`], writing directly into `w`.
///
/// # Errors
///
/// See [`set_feature_to_str`]; also propagates any error from `w`.
pub fn write_set_feature(
    w: &mut impl fmt::Write,
    set_feature: &SetFasmFeature,
    check_if_canonical: bool,
) -> Result<(), OutputError> {
    let width = checked_width(set_feature)?;

    if !set_feature.value.fits_in_bits(width) {
        return Err(OutputError::ValueTooWideForFeature {
            width,
            bit_len: set_feature.value.bit_len(),
        });
    }

    if check_if_canonical {
        if width != 1 {
            return Err(OutputError::NotCanonicalWidth { width });
        }
        if set_feature.end.is_some() {
            return Err(OutputError::NotCanonicalHasEnd);
        }
        if set_feature.start == Some(0) {
            return Err(OutputError::NotCanonicalStartZero);
        }
        if set_feature.value_format.is_some() {
            return Err(OutputError::NotCanonicalHasValueFormat);
        }
    }

    write!(w, "{}", set_feature.feature)?;

    if let Some(start) = set_feature.start {
        if let Some(end) = set_feature.end {
            write!(w, "[{end}:{start}]")?;
        } else {
            write!(w, "[{start}]")?;
        }
    }

    if let Some(value_format) = set_feature.value_format {
        write!(w, " = ")?;
        write_fasm_value(w, &set_feature.value, width, value_format)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests;

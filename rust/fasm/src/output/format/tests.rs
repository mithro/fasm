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

//! Expected strings in this file were cross-checked against the oracle
//! (`tests/oracle/venv/bin/python`, `fasm.fasm_value_to_str`/
//! `fasm.set_feature_to_str`) — see the task's scratchpad scripts.

use super::*;
use crate::idstring::IdString;

fn feature(name: &str) -> IdString {
    IdString::new(name)
}

// --- fasm_value_to_str / write_fasm_value: every ValueFormat -------------

#[test]
fn plain_is_decimal_and_ignores_width() {
    let v = FeatureValue::from_u64(42);
    assert_eq!(fasm_value_to_str(&v, 8, ValueFormat::Plain), "42");
    // width is ignored for PLAIN, exactly like Python's '{}'.format(value).
    assert_eq!(fasm_value_to_str(&v, 1, ValueFormat::Plain), "42");
    assert_eq!(
        fasm_value_to_str(&FeatureValue::zero(), 8, ValueFormat::Plain),
        "0"
    );
}

#[test]
fn verilog_hex_is_uppercase_with_width_prefix() {
    let v = FeatureValue::from_u64(0xFF);
    assert_eq!(fasm_value_to_str(&v, 8, ValueFormat::VerilogHex), "8'hFF");
    // Lower case input digits still render upper case.
    let v = FeatureValue::from_hex_str("1f").unwrap();
    assert_eq!(fasm_value_to_str(&v, 8, ValueFormat::VerilogHex), "8'h1F");
}

#[test]
fn verilog_decimal_has_width_prefix() {
    let v = FeatureValue::from_u64(42);
    assert_eq!(
        fasm_value_to_str(&v, 8, ValueFormat::VerilogDecimal),
        "8'd42"
    );
}

#[test]
fn verilog_octal_is_lowercase_o_with_width_prefix() {
    let v = FeatureValue::from_u64(0o52);
    assert_eq!(fasm_value_to_str(&v, 8, ValueFormat::VerilogOctal), "8'o52");
}

#[test]
fn verilog_binary_has_width_prefix() {
    let v = FeatureValue::from_u64(0b0010_1010);
    assert_eq!(
        fasm_value_to_str(&v, 8, ValueFormat::VerilogBinary),
        "8'b101010"
    );
}

#[test]
fn zero_in_every_format() {
    let zero = FeatureValue::zero();
    assert_eq!(fasm_value_to_str(&zero, 8, ValueFormat::Plain), "0");
    assert_eq!(fasm_value_to_str(&zero, 8, ValueFormat::VerilogHex), "8'h0");
    assert_eq!(
        fasm_value_to_str(&zero, 8, ValueFormat::VerilogDecimal),
        "8'd0"
    );
    assert_eq!(
        fasm_value_to_str(&zero, 8, ValueFormat::VerilogOctal),
        "8'o0"
    );
    assert_eq!(
        fasm_value_to_str(&zero, 8, ValueFormat::VerilogBinary),
        "8'b0"
    );
}

/// `(1 << 256) - 1`: the widest value the model's inline representation
/// holds without a heap allocation. Expected strings from the oracle (see
/// module docs).
#[test]
fn value_256_bits_all_ones_every_format() {
    let mut v = FeatureValue::zero();
    for bit in 0..256 {
        v.set_bit(bit);
    }

    assert_eq!(
        fasm_value_to_str(&v, 256, ValueFormat::Plain),
        "115792089237316195423570985008687907853269984665640564039457584007913129639935"
    );
    assert_eq!(
        fasm_value_to_str(&v, 256, ValueFormat::VerilogHex),
        format!("256'h{}", "F".repeat(64))
    );
    assert_eq!(
        fasm_value_to_str(&v, 256, ValueFormat::VerilogDecimal),
        "256'd115792089237316195423570985008687907853269984665640564039457584007913129639935"
    );
    assert_eq!(
        fasm_value_to_str(&v, 256, ValueFormat::VerilogOctal),
        format!("256'o1{}", "7".repeat(85))
    );
    assert_eq!(
        fasm_value_to_str(&v, 256, ValueFormat::VerilogBinary),
        format!("256'b{}", "1".repeat(256))
    );
}

// --- set_feature_to_str ---------------------------------------------------

#[test]
fn no_address_no_format_is_bare_feature() {
    let f =
        SetFasmFeature::new(feature("A.B"), None, None, FeatureValue::from_u64(1), None).unwrap();
    assert_eq!(set_feature_to_str(&f, false).unwrap(), "A.B");
}

#[test]
fn single_bit_address_no_format() {
    let f = SetFasmFeature::new(
        feature("A.B"),
        Some(3),
        None,
        FeatureValue::from_u64(1),
        None,
    )
    .unwrap();
    assert_eq!(set_feature_to_str(&f, false).unwrap(), "A.B[3]");
}

#[test]
fn range_address_with_format() {
    let f = SetFasmFeature::new(
        feature("A.B"),
        Some(0),
        Some(7),
        FeatureValue::from_u64(0xFF),
        Some(ValueFormat::VerilogHex),
    )
    .unwrap();
    assert_eq!(set_feature_to_str(&f, false).unwrap(), "A.B[7:0] = 8'hFF");
}

#[test]
fn explicit_value_with_no_address() {
    let f = SetFasmFeature::new(
        feature("A.B"),
        None,
        None,
        FeatureValue::from_u64(1),
        Some(ValueFormat::Plain),
    )
    .unwrap();
    assert_eq!(set_feature_to_str(&f, false).unwrap(), "A.B = 1");
}

#[test]
fn value_too_wide_is_an_error_not_a_panic() {
    // Only reachable via new_unchecked: a single bit address ([3]) with a
    // value of 2 (needs 2 bits).
    let f = SetFasmFeature::new_unchecked(
        feature("A.B"),
        Some(3),
        None,
        FeatureValue::from_u64(2),
        None,
    );
    assert_eq!(
        set_feature_to_str(&f, false),
        Err(OutputError::ValueTooWideForFeature {
            width: 1,
            bit_len: 2
        })
    );
}

// --- set_feature_to_str(check_if_canonical = true) ------------------------

#[test]
fn canonical_bare_feature_ok() {
    let f =
        SetFasmFeature::new(feature("A.B"), None, None, FeatureValue::from_u64(1), None).unwrap();
    assert_eq!(set_feature_to_str(&f, true).unwrap(), "A.B");
}

#[test]
fn canonical_single_nonzero_bit_ok() {
    let f = SetFasmFeature::new(
        feature("A.B"),
        Some(3),
        None,
        FeatureValue::from_u64(1),
        None,
    )
    .unwrap();
    assert_eq!(set_feature_to_str(&f, true).unwrap(), "A.B[3]");
}

#[test]
fn canonical_rejects_width_other_than_one() {
    let f = SetFasmFeature::new(
        feature("A.B"),
        Some(0),
        Some(7),
        FeatureValue::from_u64(1),
        None,
    )
    .unwrap();
    assert_eq!(
        set_feature_to_str(&f, true),
        Err(OutputError::NotCanonicalWidth { width: 8 })
    );
}

#[test]
fn canonical_rejects_end_present() {
    // Only reachable via new_unchecked: width 1 (end == start) but `end`
    // is still `Some`.
    let f = SetFasmFeature::new_unchecked(
        feature("A.B"),
        Some(3),
        Some(3),
        FeatureValue::from_u64(1),
        None,
    );
    assert_eq!(
        set_feature_to_str(&f, true),
        Err(OutputError::NotCanonicalHasEnd)
    );
}

#[test]
fn canonical_rejects_explicit_start_zero() {
    let f = SetFasmFeature::new(
        feature("A.B"),
        Some(0),
        None,
        FeatureValue::from_u64(1),
        None,
    )
    .unwrap();
    assert_eq!(
        set_feature_to_str(&f, true),
        Err(OutputError::NotCanonicalStartZero)
    );
}

#[test]
fn canonical_rejects_value_format() {
    let f = SetFasmFeature::new(
        feature("A.B"),
        None,
        None,
        FeatureValue::from_u64(1),
        Some(ValueFormat::Plain),
    )
    .unwrap();
    assert_eq!(
        set_feature_to_str(&f, true),
        Err(OutputError::NotCanonicalHasValueFormat)
    );
}

// --- write_set_feature: no intermediate allocation path -------------------

#[test]
fn write_set_feature_matches_set_feature_to_str() {
    let f = SetFasmFeature::new(
        feature("A.B"),
        Some(0),
        Some(7),
        FeatureValue::from_u64(0xFF),
        Some(ValueFormat::VerilogHex),
    )
    .unwrap();

    let mut s = String::new();
    write_set_feature(&mut s, &f, false).unwrap();
    assert_eq!(s, set_feature_to_str(&f, false).unwrap());
}

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

use super::*;
use crate::idstring::IdString;
use crate::model::{SetFasmFeature, ValueFormat};
use crate::output::format::set_feature_to_str;

fn feature(name: &str) -> IdString {
    IdString::new(name)
}

fn render(set_feature: &SetFasmFeature) -> Vec<String> {
    try_canonical_features(set_feature)
        .unwrap()
        .iter()
        .map(|f| set_feature_to_str(f, true).unwrap())
        .collect()
}

#[test]
fn zero_value_yields_nothing() {
    let f =
        SetFasmFeature::new(feature("A.B"), None, None, FeatureValue::from_u64(0), None).unwrap();
    assert_eq!(render(&f), Vec::<String>::new());

    let f = SetFasmFeature::new(
        feature("A.B"),
        Some(0),
        Some(7),
        FeatureValue::from_u64(0),
        None,
    )
    .unwrap();
    assert_eq!(render(&f), Vec::<String>::new());
}

#[test]
fn no_address_yields_bare_feature() {
    let f =
        SetFasmFeature::new(feature("A.B"), None, None, FeatureValue::from_u64(1), None).unwrap();
    assert_eq!(render(&f), vec!["A.B".to_string()]);
}

#[test]
fn single_bit_address_zero_drops_the_address() {
    // ALUT.INIT[0] = 1 -> ALUT.INIT (from docs/specification/syntax.rst).
    let f = SetFasmFeature::new(
        feature("ALUT.INIT"),
        Some(0),
        None,
        FeatureValue::from_u64(1),
        None,
    )
    .unwrap();
    assert_eq!(render(&f), vec!["ALUT.INIT".to_string()]);
}

#[test]
fn single_bit_address_nonzero_keeps_the_address() {
    let f = SetFasmFeature::new(
        feature("A.B"),
        Some(5),
        None,
        FeatureValue::from_u64(1),
        None,
    )
    .unwrap();
    assert_eq!(render(&f), vec!["A.B[5]".to_string()]);
}

/// `ALUT.INIT[3:0] = 4'b1101` -> `ALUT.INIT`, `ALUT.INIT[2]`, `ALUT.INIT[3]`
/// (from `docs/specification/syntax.rst`'s canonicalisation example).
#[test]
fn range_expansion_matches_spec_example() {
    let f = SetFasmFeature::new(
        feature("ALUT.INIT"),
        Some(0),
        Some(3),
        FeatureValue::from_digits(b"1101", 2).unwrap(),
        Some(ValueFormat::VerilogBinary),
    )
    .unwrap();
    assert_eq!(
        render(&f),
        vec![
            "ALUT.INIT".to_string(),
            "ALUT.INIT[2]".to_string(),
            "ALUT.INIT[3]".to_string(),
        ]
    );
}

/// `[63:32] = 32'b11110000_11110000_11110000_11110000` (value
/// `4_042_322_160`) from `examples/many.fasm`: bit `i` (0-indexed from the
/// digit string's LSB) is set for `i` in `4..=7, 12..=15, 20..=23,
/// 28..=31`, so the canonical addresses are `32 + i`. This is one of
/// *four* differently valued assignments to this same address range in
/// `examples/many.fasm` (see `output::line::tests::many_fasm_canonical_matches_oracle`
/// for the full, oracle cross-checked union of all of them after dedup).
#[test]
fn range_63_32_matches_oracle() {
    let f = SetFasmFeature::new(
        feature("CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT"),
        Some(32),
        Some(63),
        FeatureValue::from_digits(b"11110000111100001111000011110000", 2).unwrap(),
        Some(ValueFormat::VerilogBinary),
    )
    .unwrap();
    assert_eq!(
        render(&f),
        vec![
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[36]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[37]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[38]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[39]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[44]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[45]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[46]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[47]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[52]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[53]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[54]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[55]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[60]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[61]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[62]",
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[63]",
        ]
    );
}

#[test]
fn canonical_features_iterator_matches_try_variant() {
    let f = SetFasmFeature::new(
        feature("A.B"),
        Some(0),
        Some(3),
        FeatureValue::from_digits(b"1101", 2).unwrap(),
        Some(ValueFormat::VerilogBinary),
    )
    .unwrap();
    let via_iter: Vec<_> = canonical_features(&f).collect();
    let via_try = try_canonical_features(&f).unwrap();
    assert_eq!(via_iter, via_try);
}

#[test]
fn invalid_end_without_start_is_an_error() {
    let f = SetFasmFeature::new_unchecked(
        feature("A.B"),
        None,
        Some(3),
        FeatureValue::from_u64(1),
        None,
    );
    assert_eq!(
        try_canonical_features(&f),
        Err(OutputError::CanonicalEndWithoutStart)
    );
}

#[test]
#[should_panic(expected = "invalid SetFasmFeature")]
fn canonical_features_panics_on_invalid_input() {
    let f = SetFasmFeature::new_unchecked(
        feature("A.B"),
        None,
        Some(3),
        FeatureValue::from_u64(1),
        None,
    );
    let _ = canonical_features(&f).collect::<Vec<_>>();
}

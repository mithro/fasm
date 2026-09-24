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

//! Whole-model integration tests: cross-type parity cases and the size
//! assertions called out in `docs/rewrite/TASKS.md` T1.2.

use super::*;
use crate::idstring::IdString;

#[test]
fn size_of_set_fasm_feature() {
    // Measured; see docs/rewrite/DESIGN-model.md (dominated by
    // FeatureValue's 40 bytes plus IdString, two Option<u32> addresses and
    // an Option<ValueFormat>, with alignment padding).
    assert_eq!(std::mem::size_of::<SetFasmFeature>(), 72);
}

#[test]
fn size_of_fasm_line() {
    // Measured; see docs/rewrite/DESIGN-model.md (an Option<SetFasmFeature>
    // has no spare niche to reuse for its own `None`, so it costs a full
    // discriminant on top of SetFasmFeature's 72 bytes; the other two
    // fields' pointers supply their own niche).
    assert_eq!(std::mem::size_of::<FasmLine>(), 112);
}

#[test]
fn size_of_annotation() {
    assert_eq!(std::mem::size_of::<Annotation>(), 32);
}

/// `CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[63:32] = 32'b1111...0000` from
/// `examples/many.fasm`: a range address with a multi-limb crossing value.
#[test]
fn python_parity_range_feature() {
    let value = FeatureValue::from_digits(b"11110000_11110000_11110000_11110000", 2).unwrap();
    assert_eq!(value, FeatureValue::from_u64(4_042_322_160));

    let feature = SetFasmFeature::new(
        IdString::new("CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT"),
        Some(32),
        Some(63),
        value,
        Some(ValueFormat::VerilogBinary),
    )
    .unwrap();
    assert_eq!(feature.width(), 32);
}

/// `INT_L_X10Y146.SW6BEG0.WW2END0` (implicit 1, no address, no value
/// format): the most common line shape.
#[test]
fn python_parity_implicit_feature() {
    let feature = SetFasmFeature::new(
        IdString::new("INT_L_X10Y146.SW6BEG0.WW2END0"),
        None,
        None,
        FeatureValue::from_u64(1),
        None,
    )
    .unwrap();
    assert_eq!(feature.width(), 1);

    let line = FasmLine {
        set_feature: Some(feature),
        annotations: None,
        comment: None,
    };
    assert!(!line.is_blank());
    assert!(!line.is_only_comment());
    assert!(!line.is_only_annotation());
}

/// A bare `#` comment, as in `examples/many.fasm`'s "Empty comment"
/// section: `comment` is `Some("")`, not `None`, and per Python's
/// truthiness rules that is *not* "only a comment" (an empty string is
/// falsy), it is a blank line.
#[test]
fn bare_hash_comment_is_some_empty_string_and_counts_as_blank() {
    let line = FasmLine {
        set_feature: None,
        annotations: None,
        comment: Some(String::new().into_boxed_str()),
    };
    assert_eq!(line.comment.as_deref(), Some(""));
    assert!(line.is_blank());
    assert!(!line.is_only_comment());
}

/// `# This is a comment` keeps the leading space verbatim.
#[test]
fn comment_text_keeps_leading_space() {
    let line = FasmLine {
        set_feature: None,
        annotations: None,
        comment: Some(" This is a comment".into()),
    };
    assert_eq!(line.comment.as_deref(), Some(" This is a comment"));
    assert!(line.is_only_comment());
}

/// `{ module = "top", file = "/a/b/d.txt", line_number = "123" }`: several
/// annotations attached to a feature-less line.
#[test]
fn multiple_annotations_on_one_line() {
    let line = FasmLine {
        set_feature: None,
        annotations: Some(vec![
            Annotation::new("module", "top"),
            Annotation::new("file", "/a/b/d.txt"),
            Annotation::new("line_number", "123"),
        ]),
        comment: None,
    };
    assert!(line.is_only_annotation());
    assert_eq!(line.annotations.as_ref().unwrap().len(), 3);
}

/// A `SetFasmFeature`, annotation and comment all on one line: none of the
/// `is_only_*`/`is_blank` helpers are true.
#[test]
fn feature_with_annotation_and_comment() {
    let line = FasmLine {
        set_feature: Some(SetFasmFeature::new_unchecked(
            IdString::new("INT_L_X10Y146.SW6BEG0.WW2END0"),
            None,
            None,
            FeatureValue::from_u64(1),
            None,
        )),
        annotations: Some(vec![Annotation::new("top_module", "/a/b/c/d.txt")]),
        comment: Some(" This is a comment".into()),
    };
    assert!(!line.is_blank());
    assert!(!line.is_only_comment());
    assert!(!line.is_only_annotation());
}

/// A fully blank line (blank line in the file): every field `None`.
#[test]
fn fully_blank_line() {
    let line = FasmLine::default();
    assert!(line.is_blank());
}

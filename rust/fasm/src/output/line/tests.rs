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
use crate::model::{Annotation, SetFasmFeature, ValueFormat};

fn feature(
    name: &str,
    start: Option<u32>,
    end: Option<u32>,
    value: u64,
    format: Option<ValueFormat>,
) -> SetFasmFeature {
    SetFasmFeature::new(
        IdString::new(name),
        start,
        end,
        crate::model::FeatureValue::from_u64(value),
        format,
    )
    .unwrap()
}

fn feature_line(set_feature: SetFasmFeature) -> FasmLine {
    FasmLine {
        set_feature: Some(set_feature),
        annotations: None,
        comment: None,
    }
}

fn comment_line(text: &str) -> FasmLine {
    FasmLine {
        set_feature: None,
        annotations: None,
        comment: Some(text.into()),
    }
}

// --- fasm_line_to_string: non canonical -----------------------------------

#[test]
fn blank_line_yields_one_empty_string() {
    let line = FasmLine::default();
    assert_eq!(
        fasm_line_to_string(&line, false).unwrap(),
        vec![String::new()]
    );
}

#[test]
fn bare_hash_comment_renders_as_bare_hash() {
    // A bare `#` parses to `comment: Some("")`: not "only a comment" by
    // `FasmLine::is_only_comment` (that's a truthiness check), but Python's
    // `fasm_line_to_string` still emits it (an `is not None` check).
    let line = comment_line("");
    assert_eq!(fasm_line_to_string(&line, false).unwrap(), vec!["#"]);
}

#[test]
fn feature_annotations_and_comment_are_space_joined() {
    let line = FasmLine {
        set_feature: Some(feature("A.B", None, None, 1, None)),
        annotations: Some(vec![Annotation::new("n", "v")]),
        comment: Some(" c".into()),
    };
    assert_eq!(
        fasm_line_to_string(&line, false).unwrap(),
        vec![r#"A.B { n = "v" } # c"#]
    );
}

#[test]
fn multiple_annotations_are_comma_joined() {
    let line = FasmLine {
        set_feature: None,
        annotations: Some(vec![
            Annotation::new("module", "top"),
            Annotation::new("file", "/a/b/d.txt"),
            Annotation::new("line_number", "123"),
        ]),
        comment: None,
    };
    assert_eq!(
        fasm_line_to_string(&line, false).unwrap(),
        vec![r#"{ module = "top", file = "/a/b/d.txt", line_number = "123" }"#]
    );
}

#[test]
fn empty_annotations_vec_is_falsy_like_none() {
    // `Some(vec![])` never comes from the parser, but is representable;
    // Python's `if fasm_line.annotations` is falsy for `[]` too.
    let line = FasmLine {
        set_feature: Some(feature("A.B", None, None, 1, None)),
        annotations: Some(vec![]),
        comment: None,
    };
    assert_eq!(fasm_line_to_string(&line, false).unwrap(), vec!["A.B"]);
}

// --- fasm_line_to_string: canonical -----------------------------------

#[test]
fn canonical_ignores_annotations_and_comment() {
    let line = FasmLine {
        set_feature: Some(feature("A.B", None, None, 1, None)),
        annotations: Some(vec![Annotation::new("n", "v")]),
        comment: Some(" c".into()),
    };
    assert_eq!(fasm_line_to_string(&line, true).unwrap(), vec!["A.B"]);
}

#[test]
fn canonical_with_no_set_feature_yields_nothing() {
    let line = FasmLine {
        set_feature: None,
        annotations: Some(vec![Annotation::new("n", "v")]),
        comment: None,
    };
    assert_eq!(
        fasm_line_to_string(&line, true).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn canonical_range_yields_several_strings() {
    let line = feature_line(feature(
        "A.B",
        Some(0),
        Some(3),
        0b1101,
        Some(ValueFormat::VerilogBinary),
    ));
    assert_eq!(
        fasm_line_to_string(&line, true).unwrap(),
        vec!["A.B", "A.B[2]", "A.B[3]"]
    );
}

// --- fasm_tuple_to_string --------------------------------------------------

#[test]
fn empty_model_is_a_single_newline() {
    assert_eq!(fasm_tuple_to_string(&[], false).unwrap(), "\n");
    assert_eq!(fasm_tuple_to_string(&[], true).unwrap(), "\n");
}

#[test]
fn canonical_dedupes_and_sorts() {
    let a = feature_line(feature("B", None, None, 1, None));
    let b = feature_line(feature("A", None, None, 1, None));
    let a_again = feature_line(feature("B", None, None, 1, None));
    let lines = [a, b, a_again];
    assert_eq!(fasm_tuple_to_string(&lines, true).unwrap(), "A\nB\n");
}

/// The full `examples/many.fasm` model, built by hand (the `parser` module,
/// T1.3, is a separate branch under review and not available to this
/// task), matching the `repr()` dump from the oracle
/// (`tests/oracle/venv/bin/python`) exactly. Compared byte for byte against
/// `tests/corpus/oracle/many.fasm.{out,canonical}.txt`; see
/// `tests/corpus/oracle/README.md` for how those were generated.
fn many_fasm_model() -> Vec<FasmLine> {
    vec![
        comment_line(" This file should have examples of all FASM lines that should parse."),
        comment_line(" If an example is missing, add it to the end with a comment when is being"),
        comment_line(" demostrated."),
        comment_line(""),
        comment_line(" Blank  line"),
        comment_line(" Empty comment"),
        comment_line(""),
        comment_line("    "),
        comment_line(" Set a single feature bit to 1"),
        comment_line(" Implicit 1"),
        feature_line(feature(
            "INT_L_X10Y146.SW6BEG0.WW2END0",
            None,
            None,
            1,
            None,
        )),
        feature_line(feature(
            "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT",
            Some(17),
            None,
            1,
            None,
        )),
        comment_line(" Explicit 1"),
        feature_line(feature(
            "INT_L_X10Y146.SW6BEG0.WW2END0",
            None,
            None,
            1,
            Some(ValueFormat::Plain),
        )),
        feature_line(feature(
            "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT",
            Some(17),
            None,
            1,
            Some(ValueFormat::Plain),
        )),
        comment_line(" Explicit bit range"),
        feature_line(feature(
            "INT_L_X10Y146.SW6BEG0.WW2END0",
            Some(0),
            Some(0),
            1,
            Some(ValueFormat::VerilogBinary),
        )),
        feature_line(feature(
            "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT",
            Some(17),
            Some(17),
            1,
            Some(ValueFormat::VerilogBinary),
        )),
        comment_line(" Set a single feature bit to 0"),
        comment_line(" Explicit 0"),
        feature_line(feature(
            "INT_L_X10Y146.SW6BEG0.WW2END0",
            None,
            None,
            0,
            Some(ValueFormat::Plain),
        )),
        feature_line(feature(
            "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT",
            Some(17),
            None,
            0,
            Some(ValueFormat::Plain),
        )),
        comment_line(" Explicit bit range to 0"),
        feature_line(feature(
            "INT_L_X10Y146.SW6BEG0.WW2END0",
            Some(0),
            Some(0),
            0,
            Some(ValueFormat::VerilogBinary),
        )),
        feature_line(feature(
            "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT",
            Some(17),
            Some(17),
            0,
            Some(ValueFormat::VerilogBinary),
        )),
        comment_line(" Set a bitarray"),
        feature_line(feature(
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT",
            Some(32),
            Some(63),
            4_042_322_160,
            Some(ValueFormat::VerilogBinary),
        )),
        feature_line(feature(
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT",
            Some(32),
            Some(63),
            4_042_322_160,
            Some(ValueFormat::VerilogBinary),
        )),
        feature_line(feature(
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT",
            Some(32),
            Some(63),
            31,
            Some(ValueFormat::VerilogHex),
        )),
        feature_line(feature(
            "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT",
            Some(32),
            Some(63),
            342_391,
            Some(ValueFormat::VerilogOctal),
        )),
        comment_line(" Annotation on a FASM feature"),
        FasmLine {
            set_feature: Some(feature(
                "INT_L_X10Y146.SW6BEG0.WW2END0",
                None,
                None,
                1,
                None,
            )),
            annotations: Some(vec![Annotation::new(".attr", "")]),
            comment: None,
        },
        FasmLine {
            set_feature: Some(feature(
                "INT_L_X10Y146.SW6BEG0.WW2END0",
                None,
                None,
                1,
                None,
            )),
            annotations: Some(vec![Annotation::new(".filename", "/a/b/c.txt")]),
            comment: None,
        },
        FasmLine {
            set_feature: Some(feature(
                "INT_L_X10Y146.SW6BEG0.WW2END0",
                None,
                None,
                1,
                None,
            )),
            annotations: Some(vec![
                Annotation::new("module", "top"),
                Annotation::new("file", "/a/b/d.txt"),
                Annotation::new("line_number", "123"),
            ]),
            comment: None,
        },
        FasmLine {
            set_feature: Some(feature(
                "INT_L_X10Y146.SW6BEG0.WW2END0",
                None,
                None,
                1,
                None,
            )),
            annotations: Some(vec![
                Annotation::new("module", "top"),
                Annotation::new("file", "/a/b/d.txt"),
                Annotation::new("line_number", "123"),
            ]),
            comment: None,
        },
        comment_line(" Annotation by itself"),
        FasmLine {
            set_feature: None,
            annotations: Some(vec![Annotation::new(".top_module", "/a/b/c/d.txt")]),
            comment: None,
        },
        comment_line(" Annotation with FASM feature and comment"),
        FasmLine {
            set_feature: Some(feature(
                "INT_L_X10Y146.SW6BEG0.WW2END0",
                None,
                None,
                1,
                None,
            )),
            annotations: Some(vec![Annotation::new(".top_module", "/a/b/c/d.txt")]),
            comment: Some(" This is a comment".into()),
        },
        comment_line(" Comment on the last line!"),
    ]
}

#[test]
fn many_fasm_non_canonical_matches_oracle() {
    let model = many_fasm_model();
    let expected = include_str!("../../../../../tests/corpus/oracle/many.fasm.out.txt");
    assert_eq!(fasm_tuple_to_string(&model, false).unwrap(), expected);
}

#[test]
fn many_fasm_canonical_matches_oracle() {
    let model = many_fasm_model();
    let expected = include_str!("../../../../../tests/corpus/oracle/many.fasm.canonical.txt");
    assert_eq!(fasm_tuple_to_string(&model, true).unwrap(), expected);
}

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

//! Expected outputs in this file were cross-checked against the oracle
//! (`tests/oracle/venv/bin/python`, `fasm.output.merge_features`/
//! `fasm.output.merge_and_sort`) — see the task's scratchpad scripts, in
//! particular the "faithfully reproduced quirk" duplicate-group trace on
//! [`super::MergeModel`]'s docs.

use super::*;
use crate::idstring::IdString;
use crate::model::{Annotation, FeatureValue};

fn feature(name: &str, start: Option<u32>, end: Option<u32>, value: u64) -> SetFasmFeature {
    SetFasmFeature::new(
        IdString::new(name),
        start,
        end,
        FeatureValue::from_u64(value),
        None,
    )
    .unwrap()
}

fn feature_line(name: &str, start: Option<u32>) -> FasmLine {
    FasmLine {
        set_feature: Some(feature(name, start, None, 1)),
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

fn annotation_line(name: &str, value: &str) -> FasmLine {
    FasmLine {
        set_feature: None,
        annotations: Some(vec![Annotation::new(name, value)]),
        comment: None,
    }
}

// --- merge_features: docstring examples -----------------------------------

#[test]
fn merge_features_adjacent_bits() {
    // A[0] = 1, A[1] = 1 -> A[1:0] = 2'b11
    let a = feature("A", Some(0), None, 1);
    let b = feature("A", Some(1), None, 1);
    let merged = merge_features(&[a, b]).unwrap();
    assert_eq!(merged.start, Some(0));
    assert_eq!(merged.end, Some(1));
    assert_eq!(merged.value, FeatureValue::from_u64(0b11));
    assert_eq!(merged.value_format, Some(ValueFormat::VerilogBinary));
}

#[test]
fn merge_features_sparse_bits() {
    // A[5] = 1, A[7] = 1 -> A[7:0] = 8'b10100000
    let a = feature("A", Some(5), None, 1);
    let b = feature("A", Some(7), None, 1);
    let merged = merge_features(&[a, b]).unwrap();
    assert_eq!(merged.start, Some(0));
    assert_eq!(merged.end, Some(7));
    assert_eq!(merged.value, FeatureValue::from_u64(0b1010_0000));
    assert_eq!(merged.value_format, Some(ValueFormat::VerilogBinary));
}

#[test]
fn merge_features_rejects_empty() {
    assert_eq!(
        merge_features(&[]),
        Err(OutputError::MergeFeaturesNotSingleFeature)
    );
}

#[test]
fn merge_features_rejects_mismatched_names() {
    let a = feature("A", Some(0), None, 1);
    let b = feature("B", Some(0), None, 1);
    assert_eq!(
        merge_features(&[a, b]),
        Err(OutputError::MergeFeaturesNotSingleFeature)
    );
}

#[test]
fn merge_features_rejects_conflicting_bit() {
    // Bit 0 set by one feature, cleared by another.
    let a = feature("A", Some(0), None, 1);
    let b = feature("A", Some(0), Some(1), 0b00); // bit 0 = 0, bit 1 = 0
    assert_eq!(
        merge_features(&[a, b]),
        Err(OutputError::MergeFeaturesConflictingBit { bit: 0 })
    );
}

#[test]
fn merge_features_no_address_defaults_to_bit_zero() {
    // Python quirk: `start=None` is treated as bit 0, `end=None` as
    // `end=start`.
    let a = feature("A", None, None, 1);
    let merged = merge_features(&[a]).unwrap();
    assert_eq!(merged.start, Some(0));
    assert_eq!(merged.end, Some(0));
    assert_eq!(merged.value, FeatureValue::from_u64(1));
}

// --- MergeModel::add_to_model / merge_addresses / output_sorted_lines -----

#[test]
fn merge_and_sort_merges_eligible_same_feature() {
    let lines = [
        feature_line("TILE.A", Some(0)),
        feature_line("TILE.A", Some(2)),
    ];
    let out = merge_and_sort(lines, None).unwrap();
    assert_eq!(out.len(), 1);
    let sf = out[0].set_feature.as_ref().unwrap();
    assert_eq!(sf.start, Some(0));
    assert_eq!(sf.end, Some(2));
    assert_eq!(sf.value, FeatureValue::from_u64(0b101));
    assert_eq!(sf.value_format, Some(ValueFormat::VerilogBinary));
}

/// A feature appearing in one ineligible group (here: with an annotation)
/// is not merged with another, otherwise-eligible, occurrence of the same
/// feature name — both are emitted unmerged, ineligible group first
/// (`self.groups` order: non-eligible groups, in original order, then the
/// eligible-but-bypassed ones).
#[test]
fn merge_and_sort_skips_merge_when_feature_also_in_ineligible_group() {
    let lines = [
        feature_line("TILE.A", Some(0)),
        FasmLine {
            set_feature: Some(feature("TILE.A", Some(2), None, 1)),
            annotations: Some(vec![Annotation::new("n", "v")]),
            comment: None,
        },
    ];
    let out = merge_and_sort(lines, None).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].set_feature.as_ref().unwrap().start, Some(2));
    assert!(out[0].annotations.is_some());
    assert_eq!(out[1].set_feature.as_ref().unwrap().start, Some(0));
    assert!(out[1].annotations.is_none());
}

#[test]
fn merge_and_sort_groups_by_first_component_and_sorts() {
    let lines = [
        feature_line("B_TILE.X", Some(0)),
        feature_line("A_TILE.Y", Some(0)),
    ];
    let out = merge_and_sort(lines, None).unwrap();
    // A_TILE sorts before B_TILE.
    assert_eq!(
        out[0].set_feature.as_ref().unwrap().feature,
        IdString::new("A_TILE.Y")
    );
    assert!(out[1].is_blank());
    assert_eq!(
        out[2].set_feature.as_ref().unwrap().feature,
        IdString::new("B_TILE.X")
    );
}

#[test]
fn merge_and_sort_blank_separators_between_output_groups() {
    let lines = [
        feature_line("A_TILE.X", Some(0)),
        feature_line("B_TILE.Y", Some(0)),
        comment_line(" trailing comment"),
    ];
    let out = merge_and_sort(lines, None).unwrap();
    // feature group A_TILE, blank, feature group B_TILE, blank, comment
    // group (non-feature groups are appended after feature groups).
    assert_eq!(out.len(), 5);
    assert_eq!(
        out[0].set_feature.as_ref().unwrap().feature,
        IdString::new("A_TILE.X")
    );
    assert!(out[1].is_blank());
    assert_eq!(
        out[2].set_feature.as_ref().unwrap().feature,
        IdString::new("B_TILE.Y")
    );
    assert!(out[3].is_blank());
    assert_eq!(out[4].comment.as_deref(), Some(" trailing comment"));
}

#[test]
fn merge_and_sort_zero_function_drops_all_zero_groups() {
    let lines = [
        feature_line("B_TILE.X", Some(0)),
        FasmLine {
            set_feature: Some(feature("A_TILE.Y", Some(0), None, 0)),
            annotations: None,
            comment: None,
        },
        FasmLine {
            set_feature: Some(feature("A_TILE.Z", Some(1), None, 0)),
            annotations: None,
            comment: None,
        },
        comment_line(" standalone comment"),
    ];
    let zero = |name: &str| name.starts_with("A_TILE");
    let out = merge_and_sort(lines, Some(&zero)).unwrap();
    // A_TILE group (all zero) dropped entirely; B_TILE kept; comment group
    // (non-feature) appended after, separated by one blank line.
    assert_eq!(out.len(), 3);
    assert_eq!(
        out[0].set_feature.as_ref().unwrap().feature,
        IdString::new("B_TILE.X")
    );
    assert!(out[1].is_blank());
    assert_eq!(out[2].comment.as_deref(), Some(" standalone comment"));
}

#[test]
fn merge_and_sort_by_key_custom_sort_key() {
    let lines = [
        feature_line("B_TILE.X", Some(0)),
        feature_line("A_TILE.Y", Some(0)),
    ];
    // Reverse alphabetical: negate each byte, compare as a Vec<i32> (an
    // arbitrary Ord type), matching the oracle's
    // `tuple(-ord(c) for c in name)` key.
    let key = |s: &str| -> Vec<i32> { s.bytes().map(|b| -i32::from(b)).collect() };
    let out = merge_and_sort_by_key(lines, None, &key).unwrap();
    assert_eq!(
        out[0].set_feature.as_ref().unwrap().feature,
        IdString::new("B_TILE.X")
    );
    assert!(out[1].is_blank());
    assert_eq!(
        out[2].set_feature.as_ref().unwrap().feature,
        IdString::new("A_TILE.Y")
    );
}

#[test]
fn merge_and_sort_of_empty_model_is_empty() {
    let out = merge_and_sort(std::iter::empty(), None).unwrap();
    assert!(out.is_empty());
}

/// [`MergeModel::groups`] (added for the Python bindings, T3.3 — see
/// `rust/fasm-python/src/merge.rs`): reflects the same groups
/// [`MergeModel::output_sorted_lines`] would sort and flatten, after
/// [`MergeModel::finish`] and [`MergeModel::merge_addresses`].
#[test]
fn groups_reflects_merge_addresses_output() {
    let mut merged = MergeModel::new();
    merged.add_to_model(feature_line("A", Some(0)));
    merged.add_to_model(feature_line("A", Some(1)));
    merged.add_to_model(feature_line("B", None));
    merged.finish();
    merged.merge_addresses().unwrap();

    // "A[0]"/"A[1]" merge into a single group; "B" is its own group; a
    // fresh MergeModel starts empty.
    assert_eq!(merged.groups().len(), 2);
    assert!(MergeModel::new().groups().is_empty());
}

/// Regression/correctness test (review finding on T1.4) for
/// `merge_addresses`'s order-preserving indexed map: many distinct
/// eligible feature names, each split across two single-bit lines that
/// must be merged, exercises the `HashMap<IdString, usize>` lookup (rather
/// than the earlier `O(n)` linear scan) finding the right, previously seen
/// entry for every one of them.
#[test]
fn merge_and_sort_handles_many_distinct_feature_names() {
    const N: usize = 500;

    let mut lines = Vec::with_capacity(2 * N);
    let mut names = Vec::with_capacity(N);
    for i in 0..N {
        let name = format!("TILE_{i}.A");
        lines.push(feature_line(&name, Some(0)));
        lines.push(feature_line(&name, Some(2)));
        names.push(name);
    }

    let out = merge_and_sort(lines, None).unwrap();

    // N merged feature lines, separated by N - 1 blank lines.
    assert_eq!(out.len(), 2 * N - 1);

    let mut seen = std::collections::HashSet::new();
    for (idx, line) in out.iter().enumerate() {
        if idx % 2 == 1 {
            assert!(line.is_blank());
            continue;
        }
        let sf = line.set_feature.as_ref().unwrap();
        // Bits 0 and 2 set, bit 1 absent (never mentioned): [2:0] = 3'b101.
        assert_eq!(sf.start, Some(0));
        assert_eq!(sf.end, Some(2));
        assert_eq!(sf.value, FeatureValue::from_u64(0b101));
        assert_eq!(sf.value_format, Some(ValueFormat::VerilogBinary));
        assert!(seen.insert(sf.feature));
    }

    // Every distinct name was merged exactly once, none dropped or
    // conflated with another.
    assert_eq!(seen.len(), N);
    for name in &names {
        assert!(seen.contains(&IdString::new(name)));
    }
}

// --- comment/annotation grouping, including the duplicate-group quirk -----

#[test]
fn consecutive_comments_are_grouped_and_attach_to_next_feature() {
    let lines = [
        comment_line(" a"),
        comment_line(" b"),
        feature_line("X", None),
    ];
    let out = merge_and_sort(lines, None).unwrap();
    // The comment group is not eligible for address merging (len > 1), so
    // it stays a single "non feature" group... but it has a set_feature
    // line in it (the feature line joined the comment group), making it a
    // *feature* group under `output_sorted_lines`.
    assert_eq!(out.len(), 3);
    assert_eq!(out[0].comment.as_deref(), Some(" a"));
    assert_eq!(out[1].comment.as_deref(), Some(" b"));
    assert_eq!(
        out[2].set_feature.as_ref().unwrap().feature,
        IdString::new("X")
    );
}

/// Verified against the oracle (`tests/oracle/venv`): a comment group that
/// ends because of a feature line, followed by another comment or
/// annotation group, causes the first group's lines to be duplicated in
/// the output. See [`super::MergeModel`]'s docs for the Python-side trace.
#[test]
fn comment_group_ended_by_feature_then_new_comment_duplicates() {
    let lines = [
        comment_line(" a"),
        feature_line("X", None),
        comment_line(" b"),
    ];
    let out = merge_and_sort(lines, None).unwrap();
    let rendered: Vec<String> = out
        .iter()
        .map(|l| {
            crate::output::fasm_line_to_string(l, false)
                .unwrap()
                .join("")
        })
        .collect();
    assert_eq!(
        rendered,
        vec![
            "# a".to_string(),
            "X".to_string(),
            "# a".to_string(),
            "X".to_string(),
            String::new(),
            "# b".to_string(),
        ]
    );
}

/// The same situation, but the new group is an annotation group instead of
/// a comment group: still duplicates (`start_annotation_group` has the
/// same `current_group is not None` check as `start_comment_group`).
/// Verified against the oracle.
#[test]
fn comment_group_ended_by_feature_then_new_annotation_duplicates() {
    let lines = [
        comment_line(" a"),
        feature_line("X", None),
        annotation_line("n", "v"),
    ];
    let out = merge_and_sort(lines, None).unwrap();
    let rendered: Vec<String> = out
        .iter()
        .map(|l| {
            crate::output::fasm_line_to_string(l, false)
                .unwrap()
                .join("")
        })
        .collect();
    assert_eq!(
        rendered,
        vec![
            "# a".to_string(),
            "X".to_string(),
            "# a".to_string(),
            "X".to_string(),
            String::new(),
            r#"{ n = "v" }"#.to_string(),
        ]
    );
}

/// An annotation group ended by a feature line, unlike a comment group,
/// does *not* leave a stale reference (Python resets `current_group` to
/// `None` in that branch): the following comment does not duplicate.
/// Verified against the oracle.
#[test]
fn annotation_group_ended_by_feature_then_new_comment_does_not_duplicate() {
    let lines = [
        annotation_line("n", "v"),
        feature_line("X", None),
        comment_line(" b"),
    ];
    let out = merge_and_sort(lines, None).unwrap();
    let rendered: Vec<String> = out
        .iter()
        .map(|l| {
            crate::output::fasm_line_to_string(l, false)
                .unwrap()
                .join("")
        })
        .collect();
    assert_eq!(
        rendered,
        vec![
            "X".to_string(),
            String::new(),
            r#"{ n = "v" }"#.to_string(),
            String::new(),
            "# b".to_string(),
        ]
    );
}

/// A comment group ended by a feature line, with nothing following it, is
/// simply dropped (not re-flushed at end of stream): the final flush only
/// re-appends `current_group` when the state is not `NoGroup`, and this
/// group already put the state back to `NoGroup`. Verified against the
/// oracle.
#[test]
fn comment_group_ended_by_feature_with_no_further_trigger_is_not_duplicated() {
    let lines = [
        comment_line(" a"),
        feature_line("X", None),
        feature_line("Y", None),
    ];
    let out = merge_and_sort(lines, None).unwrap();
    let rendered: Vec<String> = out
        .iter()
        .map(|l| {
            crate::output::fasm_line_to_string(l, false)
                .unwrap()
                .join("")
        })
        .collect();
    assert_eq!(
        rendered,
        vec![
            "# a".to_string(),
            "X".to_string(),
            String::new(),
            "Y".to_string()
        ]
    );
}

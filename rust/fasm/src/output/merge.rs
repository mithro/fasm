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

//! [`merge_features`] and [`MergeModel`] / [`merge_and_sort`]: the Rust
//! equivalent of `fasm/output.py`.

use std::collections::{HashMap, HashSet};

use crate::idstring::IdString;

use super::super::model::{FasmLine, FeatureValue, SetFasmFeature, ValueFormat};
use super::error::OutputError;

/// Combines several `SetFasmFeature`s for the same feature, with possibly
/// different (possibly overlapping) addresses, into one.
///
/// ```text
/// A[0] = 1
/// A[1] = 1
/// ```
/// becomes
/// ```text
/// A[1:0] = 2'b11
/// ```
/// and
/// ```text
/// A[5] = 1
/// A[7] = 1
/// ```
/// becomes
/// ```text
/// A[7:0] = 8'b10100000
/// ```
///
/// Mirrors Python's `merge_features` in `fasm/output.py` exactly, including
/// that a bit not covered by any input feature is simply absent from the
/// (`set_bits`/`cleared_bits`) accounting rather than defaulting to
/// cleared — see the second example above, where bits `0..=4` and `6` are
/// never mentioned and the result only spans up to the highest bit that
/// *was* mentioned (`max_bit`).
///
/// # Errors
///
/// * [`OutputError::MergeFeaturesNotSingleFeature`] if `features` is empty,
///   or its entries do not all share the same
///   [`super::super::model::SetFasmFeature::feature`] (Python: `assert
///   len(set(feature.feature for feature in features)) == 1`, which also
///   fails for an empty list).
/// * [`OutputError::MergeFeaturesConflictingBit`] if the same bit is set by
///   one feature and cleared by another (Python: `assert bit not in
///   cleared_bits` / `assert bit not in set_bits`).
/// * [`OutputError::MergeFeaturesEndWithoutStart`] /
///   [`OutputError::Model`] for a `features` entry that violates the
///   `SetFasmFeature` invariants; only reachable via `new_unchecked`.
pub fn merge_features(features: &[SetFasmFeature]) -> Result<SetFasmFeature, OutputError> {
    if features.is_empty() || features.iter().any(|f| f.feature != features[0].feature) {
        return Err(OutputError::MergeFeaturesNotSingleFeature);
    }

    let mut set_bits: HashSet<u32> = HashSet::new();
    let mut cleared_bits: HashSet<u32> = HashSet::new();

    for feature in features {
        let (start, end) = match (feature.start, feature.end) {
            (None, None) => (0, 0),
            (Some(start), None) => (start, start),
            (Some(start), Some(end)) => (start, end),
            (None, Some(_)) => return Err(OutputError::MergeFeaturesEndWithoutStart),
        };

        for bit in start..=end {
            let bit_is_set = feature.value.bit(bit - start);
            if bit_is_set {
                if cleared_bits.contains(&bit) {
                    return Err(OutputError::MergeFeaturesConflictingBit { bit });
                }
                set_bits.insert(bit);
            } else {
                if set_bits.contains(&bit) {
                    return Err(OutputError::MergeFeaturesConflictingBit { bit });
                }
                cleared_bits.insert(bit);
            }
        }
    }

    // `features` is non-empty and every feature's `start..=end` range is
    // non-empty, so `set_bits`/`cleared_bits` are never both empty.
    let max_bit = set_bits
        .iter()
        .chain(cleared_bits.iter())
        .copied()
        .max()
        .expect("at least one bit was recorded above");

    let mut final_value = FeatureValue::zero();
    for &bit in &set_bits {
        final_value.set_bit(bit);
    }

    Ok(SetFasmFeature::new(
        features[0].feature,
        Some(0),
        Some(max_bit),
        final_value,
        Some(ValueFormat::VerilogBinary),
    )?)
}

/// Grouping state for [`MergeModel`], mirroring Python's
/// `MergeModel.State` enum.
// The shared `*Group` suffix intentionally mirrors the Python variant names
// (`NoGroup`/`InCommentGroup`/`InAnnotationGroup`) one for one.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum GroupState {
    #[default]
    NoGroup,
    InCommentGroup,
    InAnnotationGroup,
}

/// Groups and merges `FasmLine`s, mirroring Python's `MergeModel` class in
/// `fasm/output.py`.
///
/// Grouping logic (see [`Self::add_to_model`]):
///  - Consecutive comments are grouped.
///  - Comment groups attach to the next non-comment entry.
///  - Consecutive annotations are grouped.
///  - Blank lines are discarded.
///  - Features are grouped by their first feature part.
///  - Features within the same feature with different addresses are
///    merged (see [`Self::merge_addresses`]).
///
/// If a feature has a comment in its group, it is not eligible for address
/// merging.
///
/// # A faithfully reproduced quirk
///
/// Python's `add_to_comment_group` pushes its `current_group` into
/// `self.groups` when the group ends because of a non-comment,
/// non-annotation line, but (unlike the equivalent branch in
/// `add_to_annotation_group`) does **not** reset `self.current_group` to
/// `None` afterwards. Because Python lists are mutable references, the
/// stale `current_group` is then pushed into `self.groups` a *second* time
/// if another comment or annotation group starts before anything else
/// reassigns it (`start_comment_group`/`start_annotation_group` only check
/// `is not None`, not "was already flushed"), duplicating that group's
/// lines in the final output. This was verified against the oracle
/// (`tests/oracle/venv`) with the input
/// `[comment(" a"), feature(X), comment(" b")]`, which renders `# a` and
/// `X` twice. See `docs/rewrite/DESIGN-output.md` for the full trace. This
/// implementation reproduces the same observable behaviour: pushing a
/// clone of the group (rather than aliasing a `Vec`) without clearing
/// `current_group` in `add_to_comment_group`'s equivalent branch, while
/// `add_to_annotation_group`'s does clear it, exactly as in Python.
#[derive(Debug, Default)]
pub struct MergeModel {
    state: GroupState,
    groups: Vec<Vec<FasmLine>>,
    current_group: Option<Vec<FasmLine>>,
}

impl MergeModel {
    /// A new, empty `MergeModel`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a new group of comments. Requires `line` to be
    /// [`FasmLine::is_only_comment`] and the state to not already be
    /// [`GroupState::InCommentGroup`].
    fn start_comment_group(&mut self, line: FasmLine) {
        debug_assert_ne!(self.state, GroupState::InCommentGroup);
        debug_assert!(line.is_only_comment());

        if let Some(group) = self.current_group.take() {
            self.groups.push(group);
        }

        self.state = GroupState::InCommentGroup;
        self.current_group = Some(vec![line]);
    }

    /// See the "faithfully reproduced quirk" section on [`MergeModel`]: the
    /// `else` branch here pushes a *clone* of `current_group` without
    /// clearing it, matching Python's stale-reference bug.
    fn add_to_comment_group(&mut self, line: FasmLine) {
        debug_assert_eq!(self.state, GroupState::InCommentGroup);

        if line.is_only_comment() {
            self.current_group
                .as_mut()
                .expect("InCommentGroup implies current_group is Some")
                .push(line);
        } else if line.is_only_annotation() {
            self.current_group
                .as_mut()
                .expect("InCommentGroup implies current_group is Some")
                .push(line);
            self.state = GroupState::InAnnotationGroup;
        } else {
            if !line.is_blank() {
                self.current_group
                    .as_mut()
                    .expect("InCommentGroup implies current_group is Some")
                    .push(line);
            }

            self.groups.push(
                self.current_group
                    .clone()
                    .expect("InCommentGroup implies current_group is Some"),
            );
            self.state = GroupState::NoGroup;
            // `current_group` is deliberately left as `Some(..)` (not
            // reset): see the "faithfully reproduced quirk" section above.
        }
    }

    /// Starts a new group of annotations. Requires `line` to be
    /// [`FasmLine::is_only_annotation`] and the state to not already be
    /// [`GroupState::InAnnotationGroup`].
    fn start_annotation_group(&mut self, line: FasmLine) {
        debug_assert_ne!(self.state, GroupState::InAnnotationGroup);
        debug_assert!(line.is_only_annotation());

        if let Some(group) = self.current_group.take() {
            self.groups.push(group);
        }

        self.state = GroupState::InAnnotationGroup;
        self.current_group = Some(vec![line]);
    }

    fn add_to_annotation_group(&mut self, line: FasmLine) {
        debug_assert_eq!(self.state, GroupState::InAnnotationGroup);

        if line.is_only_comment() {
            self.start_comment_group(line);
        } else if line.is_only_annotation() {
            self.current_group
                .as_mut()
                .expect("InAnnotationGroup implies current_group is Some")
                .push(line);
            self.state = GroupState::InAnnotationGroup;
        } else {
            self.groups.push(
                self.current_group
                    .take()
                    .expect("InAnnotationGroup implies current_group is Some"),
            );
            self.state = GroupState::NoGroup;
            self.add_to_model(line);
        }
    }

    /// Adds a line to the model. Stateful: the grouping rules on
    /// [`MergeModel`] depend on insertion order.
    pub fn add_to_model(&mut self, line: FasmLine) {
        match self.state {
            GroupState::NoGroup => {
                if line.is_only_comment() {
                    self.start_comment_group(line);
                } else if line.is_only_annotation() {
                    self.start_annotation_group(line);
                } else if !line.is_blank() {
                    self.groups.push(vec![line]);
                }
            }
            GroupState::InCommentGroup => self.add_to_comment_group(line),
            GroupState::InAnnotationGroup => self.add_to_annotation_group(line),
        }
    }

    /// Flushes a trailing in-progress group. Call once, after all lines
    /// have been added, before [`Self::merge_addresses`]. Mirrors the
    /// final-flush step in Python's module level `merge_and_sort` function
    /// (not a `MergeModel` method there, but moved onto `MergeModel` here
    /// for a self-contained API).
    pub fn finish(&mut self) {
        if self.state != GroupState::NoGroup {
            if let Some(group) = self.current_group.take() {
                self.groups.push(group);
            }
        }
    }

    /// The grouped lines accumulated so far.
    ///
    /// After [`Self::finish`] and [`Self::merge_addresses`], these are the
    /// final groups [`Self::output_sorted_lines`] sorts and flattens.
    /// Exposed (read only) for a caller that needs to reproduce
    /// `output_sorted_lines`'s grouping/sorting with its own per-group
    /// callbacks instead of the `K: Ord` generic `dyn Fn` closures this
    /// type's own method takes: the Python bindings
    /// (`rust/fasm-python/src/merge.rs`, the `fasm._fasm_rs.merge_and_sort`
    /// fast path) need `zero_function`/`sort_key` to be Python callables
    /// that may raise, which a `dyn Fn(&str) -> bool` / `-> K` cannot
    /// represent (it cannot return `Result`, and `K: Ord` cannot be an
    /// arbitrary Python object compared with rich comparison); see
    /// `docs/rewrite/DESIGN-python.md`.
    #[must_use]
    pub fn groups(&self) -> &[Vec<FasmLine>] {
        &self.groups
    }

    /// Merges address-only features when possible. Call after all lines
    /// have been added (and [`Self::finish`] called).
    ///
    /// An "eligible" group is a single line group with no annotations and
    /// no comment. Features whose name also appears in a group that is
    /// *not* eligible are left unmerged (each becomes its own single line
    /// group, unmodified). Eligible groups sharing a feature name are
    /// merged with [`merge_features`] when there is more than one of them.
    ///
    /// # Errors
    ///
    /// Propagates [`merge_features`] errors (bit conflicts between
    /// eligible features sharing a name).
    pub fn merge_addresses(&mut self) -> Result<(), OutputError> {
        for group in &self.groups {
            for line in group {
                debug_assert!(!line.is_blank());
            }
        }

        fn find_eligible_feature(group: &[FasmLine]) -> Option<&SetFasmFeature> {
            if group.len() > 1 {
                return None;
            }
            let line = &group[0];
            if line.annotations.as_ref().is_some_and(|a| !a.is_empty()) {
                return None;
            }
            if line.comment.as_deref().is_some_and(|c| !c.is_empty()) {
                return None;
            }
            line.set_feature.as_ref()
        }

        // `eligible_address_features` preserves insertion order (Python
        // dict semantics), matching `sorted(..., key=feature_group_key)`'s
        // stability when two groups share the same feature name (rare, but
        // the tie break depends on insertion order into this structure
        // exactly as in Python). `eligible_index` is an order-preserving
        // indexed map (`HashMap<IdString, usize>` alongside the `Vec`) so
        // finding an existing feature name is `O(1)` rather than the `O(n)`
        // linear scan an earlier version of this code used, which made
        // `merge_addresses` overall `O(G^2)` in the number of distinct
        // eligible feature names `G` — a real cost on a full-chip model
        // with thousands of distinct features.
        let mut eligible_address_features: Vec<(IdString, Vec<SetFasmFeature>)> = Vec::new();
        let mut eligible_index: HashMap<IdString, usize> = HashMap::new();
        let mut non_eligible_groups: Vec<Vec<FasmLine>> = Vec::new();
        let mut non_eligible_features: HashSet<IdString> = HashSet::new();

        for group in std::mem::take(&mut self.groups) {
            match find_eligible_feature(&group) {
                None => {
                    for line in &group {
                        if let Some(set_feature) = &line.set_feature {
                            non_eligible_features.insert(set_feature.feature);
                        }
                    }
                    non_eligible_groups.push(group);
                }
                Some(feature) => {
                    let feature_name = feature.feature;
                    match eligible_index.get(&feature_name) {
                        Some(&idx) => eligible_address_features[idx].1.push(feature.clone()),
                        None => {
                            eligible_index.insert(feature_name, eligible_address_features.len());
                            eligible_address_features.push((feature_name, vec![feature.clone()]));
                        }
                    }
                }
            }
        }

        self.groups = non_eligible_groups;

        for (feature_name, feature_group) in eligible_address_features {
            if non_eligible_features.contains(&feature_name) {
                for feature in feature_group {
                    self.groups.push(vec![FasmLine {
                        set_feature: Some(feature),
                        annotations: None,
                        comment: None,
                    }]);
                }
            } else if feature_group.len() > 1 {
                self.groups.push(vec![FasmLine {
                    set_feature: Some(merge_features(&feature_group)?),
                    annotations: None,
                    comment: None,
                }]);
            } else {
                for feature in feature_group {
                    self.groups.push(vec![FasmLine {
                        set_feature: Some(feature),
                        annotations: None,
                        comment: None,
                    }]);
                }
            }
        }

        Ok(())
    }

    /// Yields the grouped, sorted lines, with a blank [`FasmLine`] between
    /// each pair of output groups.
    ///
    /// `zero_function`, if given, is called with a feature name; a feature
    /// group is dropped entirely if every feature in it (after merging)
    /// answers `true` (e.g. to drop tiles with only zero bits set).
    ///
    /// `sort_key`, if given, is called with each group id (the first `.`
    /// separated component of a feature name) to produce its sort key;
    /// without one, group ids sort by plain string (`IdString`) order.
    /// Groups sharing a feature name (after merging, normally at most one
    /// group per name) sort by that feature name; this is a stable sort,
    /// matching Python's `sorted`.
    ///
    /// Call after [`Self::merge_addresses`].
    pub fn output_sorted_lines<K: Ord>(
        &self,
        zero_function: Option<&dyn Fn(&str) -> bool>,
        sort_key: Option<&dyn Fn(&str) -> K>,
    ) -> Vec<FasmLine> {
        let mut feature_groups: HashMap<IdString, Vec<&[FasmLine]>> = HashMap::new();
        let mut non_feature_groups: Vec<&[FasmLine]> = Vec::new();

        for group in &self.groups {
            let group_id = group
                .iter()
                .find_map(|line| line.set_feature.as_ref())
                .map(|set_feature| IdString::new(set_feature.feature.first_component()));

            match group_id {
                Some(group_id) => feature_groups.entry(group_id).or_default().push(group),
                None => non_feature_groups.push(group),
            }
        }

        fn feature_group_key(group: &[FasmLine]) -> IdString {
            group
                .iter()
                .find_map(|line| line.set_feature.as_ref())
                .expect("caller only calls this for a group with at least one set_feature line")
                .feature
        }

        let mut group_ids: Vec<IdString> = feature_groups.keys().copied().collect();
        match sort_key {
            Some(sort_key) => {
                group_ids.sort_by_key(|id| id.with_str(|s| sort_key(s)));
            }
            None => group_ids.sort(),
        }

        let mut output_groups: Vec<Vec<FasmLine>> = Vec::new();

        for group_id in group_ids {
            let mut groups = feature_groups.remove(&group_id).unwrap_or_default();
            groups.sort_by_key(|group| feature_group_key(group));

            let mut flattened: Vec<FasmLine> = Vec::new();
            for group in groups {
                flattened.extend(group.iter().cloned());
            }

            if let Some(zero_function) = zero_function {
                let all_zero = flattened
                    .iter()
                    .filter_map(|line| line.set_feature.as_ref())
                    .all(|set_feature| set_feature.feature.with_str(zero_function));
                if all_zero {
                    continue;
                }
            }

            output_groups.push(flattened);
        }

        output_groups.extend(non_feature_groups.into_iter().map(<[FasmLine]>::to_vec));

        let mut out = Vec::new();
        let last = output_groups.len().saturating_sub(1);
        for (idx, group) in output_groups.into_iter().enumerate() {
            out.extend(group);
            if idx != last {
                out.push(FasmLine::default());
            }
        }
        out
    }
}

/// Shared engine behind [`merge_and_sort`] and [`merge_and_sort_by_key`]:
/// builds a [`MergeModel`] from `model` and returns its
/// [`MergeModel::output_sorted_lines`].
fn merge_and_sort_impl<K: Ord>(
    model: impl IntoIterator<Item = FasmLine>,
    zero_function: Option<&dyn Fn(&str) -> bool>,
    sort_key: Option<&dyn Fn(&str) -> K>,
) -> Result<Vec<FasmLine>, OutputError> {
    let mut merged = MergeModel::new();
    for line in model {
        merged.add_to_model(line);
    }
    merged.finish();
    merged.merge_addresses()?;
    Ok(merged.output_sorted_lines(zero_function, sort_key))
}

/// Groups and sorts `model`'s entries; the free-function equivalent of
/// Python's module level `merge_and_sort` in `fasm/output.py` (called with
/// `sort_key=None`), built on top of [`MergeModel`].
///
/// `zero_function` is as in [`MergeModel::output_sorted_lines`]; group ids
/// sort by plain string (`IdString`) order. For a custom sort key, use
/// [`merge_and_sort_by_key`] — the split exists because Rust's `sort_key`
/// needs a concrete return type `K: Ord`, unlike Python's dynamically typed
/// key functions (see `docs/rewrite/DESIGN-output.md`).
///
/// # Errors
///
/// Propagates [`MergeModel::merge_addresses`] errors.
pub fn merge_and_sort(
    model: impl IntoIterator<Item = FasmLine>,
    zero_function: Option<&dyn Fn(&str) -> bool>,
) -> Result<Vec<FasmLine>, OutputError> {
    // `K` is unused (`sort_key` is `None`); `IdString` is an arbitrary `Ord`
    // placeholder to pin it down.
    merge_and_sort_impl::<IdString>(model, zero_function, None)
}

/// [`merge_and_sort`] with a custom sort key for feature group ids; see
/// [`MergeModel::output_sorted_lines`].
///
/// # Errors
///
/// Propagates [`MergeModel::merge_addresses`] errors.
pub fn merge_and_sort_by_key<K: Ord>(
    model: impl IntoIterator<Item = FasmLine>,
    zero_function: Option<&dyn Fn(&str) -> bool>,
    sort_key: &dyn Fn(&str) -> K,
) -> Result<Vec<FasmLine>, OutputError> {
    merge_and_sort_impl(model, zero_function, Some(sort_key))
}

#[cfg(test)]
mod tests;

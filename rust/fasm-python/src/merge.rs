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

//! `fasm._fasm_rs.merge_and_sort`: the fast path for
//! `fasm.output.merge_and_sort` (see `docs/rewrite/DESIGN-python.md`).
//!
//! Unlike `fasm_tuple_to_string` (`src/output.rs`), this does not call into
//! `fasm::output::merge_and_sort`/`merge_and_sort_by_key` and convert the
//! result: that generic engine takes `zero_function`/`sort_key` as
//! `dyn Fn(&str) -> bool` / `-> K` closures with `K: Ord`, which cannot
//! represent a Python callable (it may raise, and a `dyn Fn` cannot return
//! `Result`) or a `sort_key` result that is an arbitrary Python object
//! compared with rich comparison instead of a concrete Rust `Ord`. This
//! module reimplements `MergeModel::output_sorted_lines`
//! (`rust/fasm/src/output/merge.rs`) directly against
//! [`fasm::MergeModel::groups`] (an accessor added to that type for this),
//! calling `zero_function`/`sort_key` as Python callables and propagating
//! any exception they raise immediately: once either has been called, this
//! module never falls back to the pure Python implementation, which would
//! call the same callable again and duplicate any side effect.

use std::cmp::Ordering;
use std::collections::HashMap;

use fasm::idstring::IdString;
use fasm::{FasmLine, MergeModel};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyString};

use crate::convert::lines_to_list;
use crate::output::model_from_py;

/// The full feature name of the first `set_feature` line in `group`: the
/// tie-break key Python's `feature_group_key` uses to sort the groups that
/// share a group id (a pure Rust comparison, no Python call: unlike a
/// group's *id*, its full feature name is never handed to `sort_key`).
///
/// # Panics
///
/// If `group` has no line with a `set_feature`. Only called for a group
/// already known to have one (it came from `feature_groups`, whose entries
/// are only ever pushed there for such a group).
fn feature_group_key(group: &[FasmLine]) -> IdString {
    group
        .iter()
        .find_map(|line| line.set_feature.as_ref())
        .expect("only called for a group with at least one set_feature line")
        .feature
}

/// Sorts `order` (group ids, in the order they were first seen) the way
/// Python's `sorted(feature_groups.keys(), key=sort_key)` does:
/// `sort_key` is called exactly once per id, in `order` (`order` already
/// matches Python's dict insertion order, see
/// `docs/rewrite/DESIGN-output.md`'s "Insertion order" section), and the
/// ids are then stably sorted by the resulting Python objects.
///
/// Comparisons use a single `PyAny::lt` (`<`) per pair, like CPython's own
/// sort (`Py_LT` only) — not [`pyo3::types::PyAnyMethods::compare`], which
/// would also call `==`/`>` and can raise for a pair a plain `<`-based sort
/// accepts (e.g. two instances of a class that only defines `__lt__`:
/// `compare` requires every pair to be resolved by `<`, `==` or `>`, but
/// Python's `sorted` never calls `==`/`>` at all and just treats "neither
/// `a < b` nor `b < a`" as equal for ordering purposes).
fn sort_group_ids_by_key<'py>(
    sort_key: &Bound<'py, PyAny>,
    order: Vec<IdString>,
) -> PyResult<Vec<IdString>> {
    let mut keyed: Vec<(IdString, Bound<'py, PyAny>)> = Vec::with_capacity(order.len());
    for id in order {
        let name = id.with_str(|s| PyString::new(sort_key.py(), s));
        let key = sort_key.call1((name,))?;
        keyed.push((id, key));
    }

    // A stable sort (`Vec::sort_by` is documented stable) from `<` alone:
    // `a < b` is `Less`, `b < a` is `Greater`, otherwise `Equal` (neither
    // direction holds — including `a`/`b` incomparable — sorts as equal,
    // matching Python's `sorted`, see this function's doc comment). Any
    // error is recorded and further comparisons become no-ops (`Equal`,
    // never observed: the result is discarded once `error` is `Some`).
    let mut error: Option<PyErr> = None;
    keyed.sort_by(|(_, a), (_, b)| {
        if error.is_some() {
            return Ordering::Equal;
        }
        match a.lt(b) {
            Ok(true) => Ordering::Less,
            Ok(false) => match b.lt(a) {
                Ok(true) => Ordering::Greater,
                Ok(false) => Ordering::Equal,
                Err(e) => {
                    error = Some(e);
                    Ordering::Equal
                }
            },
            Err(e) => {
                error = Some(e);
                Ordering::Equal
            }
        }
    });
    if let Some(e) = error {
        return Err(e);
    }

    Ok(keyed.into_iter().map(|(id, _)| id).collect())
}

/// Reimplements [`MergeModel::output_sorted_lines`] against
/// [`MergeModel::groups`], calling `zero_function`/`sort_key` (if given) as
/// Python callables; see this module's doc comment for why.
fn output_sorted_lines_py<'py>(
    groups: &[Vec<FasmLine>],
    zero_function: Option<&Bound<'py, PyAny>>,
    sort_key: Option<&Bound<'py, PyAny>>,
) -> PyResult<Vec<FasmLine>> {
    // `order`: group ids in first-seen order (see `sort_group_ids_by_key`'s
    // doc comment); `feature_groups`: the groups sharing each id, in their
    // original relative order (`Vec::push`); `non_feature_groups`: the
    // groups with no `set_feature` line at all, in original order.
    let mut order: Vec<IdString> = Vec::new();
    let mut feature_groups: HashMap<IdString, Vec<&[FasmLine]>> = HashMap::new();
    let mut non_feature_groups: Vec<&[FasmLine]> = Vec::new();

    for group in groups {
        let group_id = group
            .iter()
            .find_map(|line| line.set_feature.as_ref())
            .map(|set_feature| IdString::new(set_feature.feature.first_component()));

        match group_id {
            Some(id) => {
                if !feature_groups.contains_key(&id) {
                    order.push(id);
                }
                feature_groups.entry(id).or_default().push(group.as_slice());
            }
            None => non_feature_groups.push(group.as_slice()),
        }
    }

    let sorted_ids = match sort_key {
        Some(sort_key) => sort_group_ids_by_key(sort_key, order)?,
        None => {
            let mut ids = order;
            // The order of `ids.sort()`, each id resolved once.
            fasm::idstring::sort_by_string(&mut ids, |&id| id);
            ids
        }
    };

    let mut output_groups: Vec<Vec<FasmLine>> = Vec::new();
    for id in sorted_ids {
        let mut member_groups = feature_groups.remove(&id).unwrap_or_default();
        // Stable, like `sort_by_key`, with each name resolved once.
        fasm::idstring::sort_by_string(&mut member_groups, |group| feature_group_key(group));

        let mut flattened: Vec<FasmLine> = Vec::new();
        for group in member_groups {
            flattened.extend(group.iter().cloned());
        }

        if let Some(zero_function) = zero_function {
            // Mirrors Python's `all(zero_function(line.set_feature.feature)
            // for line in flattened_group if line.set_feature)`: calls
            // `zero_function` once per `set_feature` line in
            // `flattened`'s order, stopping at the first `False` (`all`
            // short circuits) — any exception propagates immediately via
            // `?`, aborting the whole function (no further group is
            // considered, matching an exception raised inside Python's
            // `output_sorted_lines` generator, which also never resumes).
            let mut all_zero = true;
            for line in &flattened {
                if let Some(set_feature) = &line.set_feature {
                    let name = set_feature
                        .feature
                        .with_str(|s| PyString::new(zero_function.py(), s));
                    if !zero_function.call1((name,))?.is_truthy()? {
                        all_zero = false;
                        break;
                    }
                }
            }
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
    Ok(out)
}

/// `_fasm_rs.merge_and_sort(model, zero_function, sort_key)`: see
/// `src/lib.rs`'s `#[pyfunction]` wrapper for the public contract.
pub(crate) fn merge_and_sort_from_py<'py>(
    py: Python<'py>,
    model: &Bound<'py, PyAny>,
    zero_function: Option<&Bound<'py, PyAny>>,
    sort_key: Option<&Bound<'py, PyAny>>,
) -> PyResult<Option<Bound<'py, PyList>>> {
    let Some(lines) = model_from_py(py, model)? else {
        return Ok(None);
    };

    // Grouping and address merging are pure Rust (no Python calls), so run
    // them with the GIL released, like parsing and `fasm_tuple_to_string`.
    let merged = py.detach(|| {
        let mut merged = MergeModel::new();
        for line in lines {
            merged.add_to_model(line);
        }
        merged.finish();
        merged.merge_addresses().map(|()| merged)
    });
    // `model_from_py` builds every `SetFasmFeature` with `new_unchecked`
    // (`src/output.rs`), so a model built by hand (not by the parser) can
    // still violate the invariants `merge_features`'s Python `assert`s
    // check. Decline here (`zero_function`/`sort_key` were not called yet)
    // so the caller's Python fallback raises that `AssertionError` — see
    // `docs/rewrite/DESIGN-python.md`.
    let Ok(merged) = merged else {
        return Ok(None);
    };

    let out = output_sorted_lines_py(merged.groups(), zero_function, sort_key)?;
    lines_to_list(py, &out).map(Some)
}

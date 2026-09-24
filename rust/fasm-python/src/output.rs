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

//! Conversion of `fasm.model` namedtuples back into the Rust model, for
//! the `fasm_tuple_to_string` fast path.
//!
//! The conversion is strict: it only accepts the exact `fasm.model` types
//! with exact `str` / `int` / `None` / `ValueFormat` fields, whose Python
//! formatting is known to be what the Rust `output` module produces
//! (`str.format` of an `int` subclass such as `bool`, or of a `str`
//! subclass with its own `__format__`, could differ). Anything else makes
//! [`model_from_py`] return `None`, and the caller falls back to the pure
//! Python implementation. Python errors raised while inspecting the model
//! are treated the same way: the Python implementation then raises its
//! own error, if any.

use fasm::idstring::IdString;
use fasm::{Annotation, FasmLine, FeatureValue, SetFasmFeature, ValueFormat};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyInt, PyList, PyString, PyTuple};

use crate::convert::{le_limbs, PyModel, VALUE_FORMATS};

/// Converts `model` (a `list` or `tuple` of `fasm.model.FasmLine`) into
/// Rust [`FasmLine`]s, or `None` if it is outside of the fast path.
///
/// # Errors
///
/// Only if `fasm.model` cannot be imported.
pub(crate) fn model_from_py(
    py: Python<'_>,
    model: &Bound<'_, PyAny>,
) -> PyResult<Option<Vec<FasmLine>>> {
    let py_model = PyModel::get(py)?;
    let items = if let Ok(list) = model.cast_exact::<PyList>() {
        list.iter().collect::<Vec<_>>()
    } else if let Ok(tuple) = model.cast_exact::<PyTuple>() {
        tuple.iter().collect::<Vec<_>>()
    } else {
        return Ok(None);
    };
    Ok(items
        .iter()
        .map(|line| line_from_py(py_model, line))
        .collect())
}

/// The items of `obj` if it is an instance of exactly `class`.
fn fields<'py>(
    obj: &Bound<'py, PyAny>,
    class: &Py<pyo3::types::PyType>,
) -> Option<Bound<'py, PyTuple>> {
    if !obj.get_type().is(class) {
        return None;
    }
    obj.cast::<PyTuple>().ok().cloned()
}

/// An exact `str`.
fn string(obj: &Bound<'_, PyAny>) -> Option<Box<str>> {
    Some(obj.cast_exact::<PyString>().ok()?.to_cow().ok()?.into())
}

/// `None` (`Some(None)`), or an exact `str`.
fn optional_string(obj: &Bound<'_, PyAny>) -> Option<Option<Box<str>>> {
    if obj.is_none() {
        Some(None)
    } else {
        string(obj).map(Some)
    }
}

/// `None` (`Some(None)`), or an exact `int` in `0..2**32`.
fn optional_address(obj: &Bound<'_, PyAny>) -> Option<Option<u32>> {
    if obj.is_none() {
        Some(None)
    } else if obj.is_exact_instance_of::<PyInt>() {
        obj.extract::<u32>().ok().map(Some)
    } else {
        None
    }
}

/// A non negative exact `int`.
fn value(obj: &Bound<'_, PyAny>) -> Option<FeatureValue> {
    if !obj.is_exact_instance_of::<PyInt>() {
        return None;
    }
    if let Ok(v) = obj.extract::<u64>() {
        return Some(FeatureValue::from_u64(v));
    }
    if obj.lt(0).ok()? {
        return None;
    }
    let bits: usize = obj.call_method0("bit_length").ok()?.extract().ok()?;
    let bytes = obj.call_method1("to_bytes", (bits.div_ceil(8), "little")).ok()?;
    let bytes = bytes.cast::<PyBytes>().ok()?;
    Some(FeatureValue::from_le_limbs(&le_limbs(bytes.as_bytes())))
}

/// `None` (`Some(None)`), or one of the `fasm.model.ValueFormat` members.
fn value_format(py_model: &PyModel, obj: &Bound<'_, PyAny>) -> Option<Option<ValueFormat>> {
    if obj.is_none() {
        return Some(None);
    }
    py_model
        .value_formats
        .iter()
        .position(|member| obj.is(member))
        .map(|i| Some(VALUE_FORMATS[i]))
}

/// A `fasm.model.SetFasmFeature`.
///
/// Built with [`SetFasmFeature::new_unchecked`]: values the Python code
/// rejects with an `AssertionError` (a value wider than the address, `end`
/// before `start`, ...) make the Rust `output` functions return an error,
/// which also falls back to Python.
fn set_feature_from_py(py_model: &PyModel, obj: &Bound<'_, PyAny>) -> Option<SetFasmFeature> {
    let fields = fields(obj, &py_model.set_feature)?;
    let item = |i| fields.get_item(i).ok();
    let feature = item(0)?;
    let feature = feature.cast_exact::<PyString>().ok()?.to_cow().ok()?;
    Some(SetFasmFeature::new_unchecked(
        IdString::new(&feature),
        optional_address(&item(1)?)?,
        optional_address(&item(2)?)?,
        value(&item(3)?)?,
        value_format(py_model, &item(4)?)?,
    ))
}

/// A `fasm.model.Annotation`.
fn annotation_from_py(py_model: &PyModel, obj: &Bound<'_, PyAny>) -> Option<Annotation> {
    let fields = fields(obj, &py_model.annotation)?;
    Some(Annotation {
        name: string(&fields.get_item(0).ok()?)?,
        value: string(&fields.get_item(1).ok()?)?,
    })
}

/// `None` (`Some(None)`), or an exact `list` or `tuple` of
/// `fasm.model.Annotation`.
fn annotations_from_py(
    py_model: &PyModel,
    obj: &Bound<'_, PyAny>,
) -> Option<Option<Vec<Annotation>>> {
    if obj.is_none() {
        return Some(None);
    }
    let items = if let Ok(list) = obj.cast_exact::<PyList>() {
        list.iter().collect::<Vec<_>>()
    } else {
        obj.cast_exact::<PyTuple>().ok()?.iter().collect()
    };
    items
        .iter()
        .map(|annotation| annotation_from_py(py_model, annotation))
        .collect::<Option<Vec<_>>>()
        .map(Some)
}

/// A `fasm.model.FasmLine`.
fn line_from_py(py_model: &PyModel, obj: &Bound<'_, PyAny>) -> Option<FasmLine> {
    let fields = fields(obj, &py_model.fasm_line)?;
    let set_feature = fields.get_item(0).ok()?;
    let set_feature = if set_feature.is_none() {
        None
    } else {
        Some(set_feature_from_py(py_model, &set_feature)?)
    };
    Some(FasmLine {
        set_feature,
        annotations: annotations_from_py(py_model, &fields.get_item(1).ok()?)?,
        comment: optional_string(&fields.get_item(2).ok()?)?,
    })
}

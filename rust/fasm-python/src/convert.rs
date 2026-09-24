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

//! Conversion of the Rust model into the `fasm.model` namedtuples.
//!
//! The namedtuple classes and the `ValueFormat` members are imported from
//! `fasm.model` once (on first use) and cached in [`PyModel`], so the
//! objects handed to Python are instances of the very same classes the
//! pure Python parsers use (`type(line) is fasm.model.FasmLine`).
//!
//! Instances are built with `tuple.__new__(cls, items)`, which is what a
//! namedtuple's own `__new__` ends up calling; this skips the generated
//! Python level `__new__` (about twice as fast, and the result is
//! identical: namedtuples have `__slots__ = ()` and no other state).

use fasm::{Annotation, FasmLine, FeatureValue, SetFasmFeature, ValueFormat};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyBytes, PyInt, PyList, PyString, PyTuple, PyType};

/// The `fasm.model` classes and helpers the conversions need.
pub(crate) struct PyModel {
    /// `fasm.model.FasmLine`.
    pub(crate) fasm_line: Py<PyType>,
    /// `fasm.model.SetFasmFeature`.
    pub(crate) set_feature: Py<PyType>,
    /// `fasm.model.Annotation`.
    pub(crate) annotation: Py<PyType>,
    /// The `fasm.model.ValueFormat` members, indexed by their value (which
    /// is also the discriminant of the Rust [`ValueFormat`]).
    pub(crate) value_formats: [Py<PyAny>; 5],
    /// `tuple.__new__`.
    tuple_new: Py<PyAny>,
    /// `int.from_bytes`.
    int_from_bytes: Py<PyAny>,
}

static PY_MODEL: PyOnceLock<PyModel> = PyOnceLock::new();

/// All Rust [`ValueFormat`]s, in the order of their discriminants.
pub(crate) const VALUE_FORMATS: [ValueFormat; 5] = [
    ValueFormat::Plain,
    ValueFormat::VerilogDecimal,
    ValueFormat::VerilogHex,
    ValueFormat::VerilogBinary,
    ValueFormat::VerilogOctal,
];

impl PyModel {
    /// Returns the cached `fasm.model` objects, importing `fasm.model` on
    /// the first call.
    pub(crate) fn get(py: Python<'_>) -> PyResult<&'static PyModel> {
        PY_MODEL.get_or_try_init(py, || {
            let model = py.import("fasm.model")?;
            let class = |name: &str| -> PyResult<Py<PyType>> {
                Ok(model.getattr(name)?.cast_into::<PyType>()?.unbind())
            };
            let value_format = model.getattr("ValueFormat")?;
            let member = |format: ValueFormat| -> PyResult<Py<PyAny>> {
                Ok(value_format.getattr(format.python_name())?.unbind())
            };
            let builtins = py.import("builtins")?;
            Ok(PyModel {
                fasm_line: class("FasmLine")?,
                set_feature: class("SetFasmFeature")?,
                annotation: class("Annotation")?,
                value_formats: [
                    member(VALUE_FORMATS[0])?,
                    member(VALUE_FORMATS[1])?,
                    member(VALUE_FORMATS[2])?,
                    member(VALUE_FORMATS[3])?,
                    member(VALUE_FORMATS[4])?,
                ],
                tuple_new: builtins.getattr("tuple")?.getattr("__new__")?.unbind(),
                int_from_bytes: py.get_type::<PyInt>().getattr("from_bytes")?.unbind(),
            })
        })
    }

    /// Builds an instance of the namedtuple class `class` from `items`.
    fn make<'py>(
        &self,
        py: Python<'py>,
        class: &Py<PyType>,
        items: Bound<'py, PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.tuple_new.bind(py).call1((class.bind(py), items))
    }

    /// Converts a [`FeatureValue`] into a Python `int`.
    fn int<'py>(&self, py: Python<'py>, value: &FeatureValue) -> PyResult<Bound<'py, PyAny>> {
        if let Some(v) = value.to_u64() {
            return Ok(v.into_pyobject(py)?.into_any());
        }
        let bytes = PyBytes::new(py, &le_bytes(value.as_le_limbs()));
        self.int_from_bytes.bind(py).call1((bytes, "little"))
    }

    /// Converts a [`SetFasmFeature`] into a `fasm.model.SetFasmFeature`.
    fn set_feature<'py>(
        &self,
        py: Python<'py>,
        set_feature: &SetFasmFeature,
    ) -> PyResult<Bound<'py, PyAny>> {
        let feature = set_feature.feature.with_str(|name| PyString::new(py, name));
        let value_format = set_feature
            .value_format
            .map(|format| self.value_formats[format as usize].bind(py));
        let items = (
            feature,
            set_feature.start,
            set_feature.end,
            self.int(py, &set_feature.value)?,
            value_format,
        )
            .into_pyobject(py)?;
        self.make(py, &self.set_feature, items)
    }

    /// Converts an [`Annotation`] into a `fasm.model.Annotation`.
    fn annotation<'py>(
        &self,
        py: Python<'py>,
        annotation: &Annotation,
    ) -> PyResult<Bound<'py, PyAny>> {
        let items = (&*annotation.name, &*annotation.value).into_pyobject(py)?;
        self.make(py, &self.annotation, items)
    }

    /// Converts a [`FasmLine`] into a `fasm.model.FasmLine`.
    fn line<'py>(&self, py: Python<'py>, line: &FasmLine) -> PyResult<Bound<'py, PyAny>> {
        let set_feature = match &line.set_feature {
            None => None,
            Some(set_feature) => Some(self.set_feature(py, set_feature)?),
        };
        let annotations = match &line.annotations {
            None => None,
            Some(annotations) => {
                let items = annotations
                    .iter()
                    .map(|annotation| self.annotation(py, annotation))
                    .collect::<PyResult<Vec<_>>>()?;
                Some(PyList::new(py, items)?)
            }
        };
        let items = (set_feature, annotations, line.comment.as_deref()).into_pyobject(py)?;
        self.make(py, &self.fasm_line, items)
    }
}

/// Converts parsed lines into a Python list of `fasm.model.FasmLine`.
pub(crate) fn lines_to_list<'py>(
    py: Python<'py>,
    lines: &[FasmLine],
) -> PyResult<Bound<'py, PyList>> {
    let model = PyModel::get(py)?;
    let items = lines
        .iter()
        .map(|line| model.line(py, line))
        .collect::<PyResult<Vec<_>>>()?;
    PyList::new(py, items)
}

/// Little endian bytes of little endian `u64` limbs.
pub(crate) fn le_bytes(limbs: &[u64]) -> Vec<u8> {
    limbs.iter().flat_map(|limb| limb.to_le_bytes()).collect()
}

/// Little endian `u64` limbs of little endian bytes (the last limb is zero
/// padded).
pub(crate) fn le_limbs(bytes: &[u8]) -> Vec<u64> {
    bytes
        .chunks(8)
        .map(|chunk| {
            let mut limb = [0u8; 8];
            limb[..chunk.len()].copy_from_slice(chunk);
            u64::from_le_bytes(limb)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_formats_are_in_discriminant_order() {
        for (i, format) in VALUE_FORMATS.iter().enumerate() {
            assert_eq!(*format as usize, i);
        }
    }

    #[test]
    fn le_bytes_and_limbs_round_trip() {
        assert!(le_bytes(&[]).is_empty());
        assert_eq!(le_bytes(&[0x0102]), [2, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(le_limbs(&[2, 1]), [0x0102]);
        assert_eq!(le_limbs(&[0; 9]), [0, 0]);
        let limbs = [u64::MAX, 7, 1 << 63];
        assert_eq!(le_limbs(&le_bytes(&limbs)), limbs);
    }
}

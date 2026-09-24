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

//! Python bindings for the `fasm` crate: the `fasm._fasm_rs` extension
//! module, built with maturin (see `pyproject.toml`).
//!
//! The module is used by `fasm/parser/rust.py` (the `rust` parser of the
//! `fasm` Python package). It exposes:
//!
//! * `parse_fasm_string(s: str) -> list[FasmLine]`,
//!   `parse_fasm_bytes(b: bytes) -> list[FasmLine]` and
//!   `parse_fasm_filename(path: str | bytes | os.PathLike) -> list[FasmLine]`,
//!   returning the existing namedtuples of `fasm.model` (never parallel
//!   types; see [`convert`]);
//! * `fasm_tuple_to_string(model, canonical=False) -> str | None`, a fast
//!   path for `fasm.fasm_tuple_to_string` that returns `None` for any
//!   input it does not handle exactly like the Python function (see
//!   [`output`]);
//! * `FasmParseError`, the exception raised for parse and I/O errors,
//!   whose `str()` is `Parse error at L:C - message` (the format of the
//!   original ANTLR based parser's exception), with `line` and `column`
//!   attributes.
//!
//! Parsing and formatting run with the GIL released
//! ([`Python::detach`]). See `docs/rewrite/DESIGN-python.md`.
//!
//! The unit tests of this crate only cover pure Rust helpers: the
//! behaviour of the module is tested from Python (`tests/test_rust_parser.py`).

use std::path::PathBuf;

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::pybacked::{PyBackedBytes, PyBackedStr};
use pyo3::types::PyList;

mod convert;
mod output;

create_exception!(
    fasm.parser.rust,
    FasmParseError,
    PyException,
    "A FASM parse error or an I/O error reading a FASM file.\n\n\
     ``str()`` is ``Parse error at L:C - message`` (L and C are 0 for an \
     I/O error); the ``line`` and ``column`` attributes hold L and C."
);

/// Converts a Rust [`fasm::ParseError`] into a `FasmParseError`.
fn parse_error(py: Python<'_>, error: &fasm::ParseError) -> PyErr {
    let err = FasmParseError::new_err(error.to_string());
    let value = err.value(py);
    // Setting attributes on a fresh exception instance cannot fail short
    // of a MemoryError, in which case the exception is still raised,
    // without them.
    let _ = value.setattr("line", error.line);
    let _ = value.setattr("column", error.column);
    err
}

/// Converts a parse result into a Python list of `fasm.model.FasmLine`, or
/// a `FasmParseError`.
fn lines_to_py<'py>(
    py: Python<'py>,
    result: Result<Vec<fasm::FasmLine>, fasm::ParseError>,
) -> PyResult<Bound<'py, PyList>> {
    match result {
        Ok(lines) => convert::lines_to_list(py, &lines),
        Err(error) => Err(parse_error(py, &error)),
    }
}

/// Parses FASM text, returning a list of ``fasm.model.FasmLine``.
///
/// Raises ``FasmParseError`` on the first error of the input.
#[pyfunction]
fn parse_fasm_string(py: Python<'_>, s: PyBackedStr) -> PyResult<Bound<'_, PyList>> {
    let text: &str = &s;
    let result = py.detach(|| fasm::parse_fasm_string(text));
    lines_to_py(py, result)
}

/// Parses FASM text given as ``bytes`` (or another buffer), returning a
/// list of ``fasm.model.FasmLine``.
///
/// Only comments and annotation values must be valid UTF-8. Raises
/// ``FasmParseError`` on the first error of the input.
#[pyfunction]
fn parse_fasm_bytes(py: Python<'_>, b: PyBackedBytes) -> PyResult<Bound<'_, PyList>> {
    let data: &[u8] = &b;
    let result = py.detach(|| fasm::parse_fasm_bytes(data));
    lines_to_py(py, result)
}

/// Reads and parses a FASM file, returning a list of
/// ``fasm.model.FasmLine``.
///
/// ``filename`` is a ``str``, ``bytes`` or ``os.PathLike``. Raises
/// ``FasmParseError`` on the first error of the file, and
/// ``FasmParseError('Parse error at 0:0 - Couldn't open file <path>: <OS
/// error>')`` if it cannot be read.
#[pyfunction]
fn parse_fasm_filename<'py>(
    py: Python<'py>,
    filename: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyList>> {
    // `os.fsdecode` accepts str, bytes and os.PathLike (and raises
    // TypeError for anything else); the `PathBuf` conversion encodes the
    // str back losslessly (surrogateescape on Unix).
    let path: PathBuf = py
        .import("os")?
        .call_method1("fsdecode", (filename,))?
        .extract()?;
    let result = py.detach(|| fasm::parse_fasm_filename(&path));
    lines_to_py(py, result)
}

/// Returns the text of a FASM file for ``model``, exactly like
/// ``fasm.fasm_tuple_to_string(model, canonical)``, or ``None`` when
/// ``model`` is outside of what this fast path handles.
///
/// Handled: a ``list`` or ``tuple`` of ``fasm.model.FasmLine`` (exact
/// types) holding ``None`` or exact ``SetFasmFeature``, ``Annotation``,
/// ``str`` and ``int`` values with the value ranges of the Rust model
/// (addresses in ``0..2**32``, non negative values, ``ValueFormat``
/// members or ``None``). Anything else, and every input for which the
/// Python function raises, returns ``None``: the caller then runs the pure
/// Python implementation, which gives the reference result or exception.
#[pyfunction]
#[pyo3(signature = (model, canonical=false))]
fn fasm_tuple_to_string(
    py: Python<'_>,
    model: &Bound<'_, PyAny>,
    canonical: bool,
) -> PyResult<Option<String>> {
    let Some(lines) = output::model_from_py(py, model)? else {
        return Ok(None);
    };
    Ok(py
        .detach(|| fasm::fasm_tuple_to_string(&lines, canonical))
        .ok())
}

/// The `fasm._fasm_rs` extension module.
#[pymodule]
fn _fasm_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse_fasm_string, m)?)?;
    m.add_function(wrap_pyfunction!(parse_fasm_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(parse_fasm_filename, m)?)?;
    m.add_function(wrap_pyfunction!(fasm_tuple_to_string, m)?)?;
    m.add("FasmParseError", m.py().get_type::<FasmParseError>())?;
    m.add("__version__", fasm::VERSION)?;
    Ok(())
}

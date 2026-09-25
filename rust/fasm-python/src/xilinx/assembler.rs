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

//! `fasm.xilinx.FasmAssembler`, `fasm2frames` and `read_roi_design`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use fasm::FasmLine;
use fasm_xilinx::{
    Architecture, AssemblerError, Database, Fasm2FramesOptions, FasmAssembler, FasmInput, Roi,
};
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::pybacked::{PyBackedBytes, PyBackedStr};
use pyo3::types::{PyList, PyString};
use pyo3::{PyTraverseError, PyVisit};

use super::{assembler_error, fs_path, Locked, PyDatabase, PyFrames, XTypes};
use crate::convert::PyModel;
use crate::output::line_from_py;

/// Parses FASM text for the assembler (`FasmParseError` on error).
fn parse_text(py: Python<'_>, text: &[u8]) -> PyResult<Vec<FasmLine>> {
    fasm::parse_fasm_bytes(text).map_err(|e| assembler_error(py, &AssemblerError::Parse(e)))
}

/// FASM lines from Python: ``None`` (nothing), FASM text (``str`` or
/// ``bytes``), or an iterable of ``fasm.model.FasmLine`` and/or ``str``
/// lines.
fn lines_from_py(py: Python<'_>, obj: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<FasmLine>> {
    let Some(obj) = obj.filter(|o| !o.is_none()) else {
        return Ok(Vec::new());
    };
    if obj.cast::<PyString>().is_ok() {
        let text: PyBackedStr = obj.extract()?;
        return parse_text(py, text.as_bytes());
    }
    if let Ok(bytes) = obj.extract::<PyBackedBytes>() {
        return parse_text(py, &bytes);
    }
    let model = PyModel::get(py)?;
    let mut lines = Vec::new();
    for item in obj.try_iter()? {
        let item = item?;
        if item.cast::<PyString>().is_ok() {
            let text: PyBackedStr = item.extract()?;
            lines.extend(parse_text(py, text.as_bytes())?);
        } else if let Some(line) = line_from_py(model, &item) {
            lines.push(line);
        } else {
            // Not exactly the `fasm.model` types the fast conversion
            // handles (an `int` subclass, ...): through its text.
            let text: String = py
                .import("fasm")?
                .call_method1("fasm_tuple_to_string", (PyList::new(py, [&item])?,))?
                .extract()?;
            lines.extend(parse_text(py, text.as_bytes())?);
        }
    }
    Ok(lines)
}

/// Assembles FASM features into configuration frames, exactly like
/// ``prjxray.fasm_assembler.FasmAssembler`` (and, with ``prjuray=True``,
/// prjuray's ``utils/fasm_assembler.py``).
///
/// ``FasmAssembler(db, prjuray=None)``: ``db`` is a ``Database`` opened
/// for a part; ``prjuray`` selects prjuray's semantics (bits beyond the end
/// of a frame are kept, conflicts are reported in 16-bit words) and
/// defaults to ``True`` for UltraScale and UltraScale+ databases, like the
/// ``fasm2frames`` command line tool.
///
/// Features are added with ``parse_fasm_filename`` / ``parse_fasm_string``
/// / ``parse_fasm_bytes`` (which report every feature missing from the
/// database at the end, as one ``FasmLookupError``), ``add_fasm_line`` and
/// ``add_fasm_lines``; ``get_frames`` returns the frames. Assembly runs
/// with the GIL released. Methods of one assembler may be called from
/// several threads (calls are serialised); a feature callback must not
/// call its own assembler (``RuntimeError``).
///
/// The assembler takes part in Python's cyclic garbage collection: a
/// feature callback that refers back to its assembler (for example a
/// bound method of an object that owns the assembler) is collected with
/// it (``gc.collect()``), which removes the callback.
#[pyclass(frozen, weakref, name = "FasmAssembler", module = "fasm.xilinx")]
pub(crate) struct PyFasmAssembler {
    database: Py<PyDatabase>,
    db: Arc<Database>,
    state: Locked<FasmAssembler<'static>>,
    /// The Python feature callback. The Rust callback installed in the
    /// assembler only holds this slot, so that the garbage collector sees
    /// the reference (`__traverse__`) and can break cycles (`__clear__`).
    callback: Arc<Mutex<Option<Py<PyAny>>>>,
    /// The exception a feature callback raised during the current call
    /// (only used while the assembler's lock is held).
    callback_error: Arc<Mutex<Option<PyErr>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl PyFasmAssembler {
    /// Runs `f` on the assembler with the GIL released; an error becomes
    /// the Python exception (the feature callback's own exception if it
    /// raised one).
    fn run<R: Send>(
        &self,
        py: Python<'_>,
        f: impl FnOnce(&mut FasmAssembler<'static>) -> Result<R, AssemblerError> + Send,
    ) -> PyResult<R> {
        let errors = &self.callback_error;
        // The callback's exception is taken while the assembler is still
        // locked, so that concurrent calls never see each other's.
        let (result, callback_error) = self.state.with(py, move |a| {
            *lock(errors) = None;
            let result = f(a);
            let error = if result.is_err() {
                lock(errors).take()
            } else {
                None
            };
            (result, error)
        })?;
        result.map_err(|e| callback_error.unwrap_or_else(|| assembler_error(py, &e)))
    }
}

#[pymethods]
impl PyFasmAssembler {
    #[new]
    #[pyo3(signature = (db, prjuray=None))]
    fn new(py: Python<'_>, db: Bound<'_, PyDatabase>, prjuray: Option<bool>) -> PyResult<Self> {
        let shared = Arc::clone(&db.get().db);
        let prjuray = prjuray.unwrap_or(shared.architecture() != Architecture::Series7);
        let mut assembler =
            FasmAssembler::new_shared(Arc::clone(&shared)).map_err(|e| assembler_error(py, &e))?;
        assembler.set_prjuray(prjuray);
        Ok(PyFasmAssembler {
            database: db.unbind(),
            db: shared,
            state: Locked::new(assembler),
            callback: Arc::new(Mutex::new(None)),
            callback_error: Arc::new(Mutex::new(None)),
        })
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.database)?;
        // Never block in the garbage collector: the slot is only locked
        // briefly, with the GIL held, by the callback wrapper.
        if let Ok(callback) = self.callback.try_lock() {
            if let Some(callback) = callback.as_ref() {
                visit.call(callback)?;
            }
        }
        Ok(())
    }

    fn __clear__(&self) {
        let callback = lock(&self.callback).take();
        drop(callback);
    }

    /// The ``Database``.
    #[getter]
    fn database(&self, py: Python<'_>) -> Py<PyDatabase> {
        self.database.clone_ref(py)
    }

    /// ``FasmAssembler.parse_fasm_filename``: parses the whole FASM file
    /// (``str``, ``bytes`` or ``os.PathLike``; a syntax error is raised
    /// before anything is assembled), adds its lines then
    /// ``extra_features`` (FASM text, or ``fasm.model.FasmLine`` objects
    /// and/or text lines), and raises ``FasmLookupError`` for the features
    /// that are not in the database. Also raises ``FasmParseError``,
    /// ``FasmInconsistentBits``, ``FasmKeyError`` (unknown tile or tile
    /// type), and the feature callback's exceptions.
    #[pyo3(signature = (filename, extra_features=None))]
    fn parse_fasm_filename(
        &self,
        py: Python<'_>,
        filename: &Bound<'_, PyAny>,
        extra_features: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let path = fs_path(filename)?;
        let extra = lines_from_py(py, extra_features)?;
        self.run(py, move |a| a.parse_fasm_filename(&path, extra))
    }

    /// ``parse_fasm_filename`` for FASM text.
    #[pyo3(signature = (text, extra_features=None))]
    fn parse_fasm_string(
        &self,
        py: Python<'_>,
        text: PyBackedStr,
        extra_features: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let extra = lines_from_py(py, extra_features)?;
        let data: &str = &text;
        self.run(py, move |a| a.parse_fasm_bytes(data.as_bytes(), extra))
    }

    /// ``parse_fasm_filename`` for the contents of a FASM file.
    #[pyo3(signature = (data, extra_features=None))]
    fn parse_fasm_bytes(
        &self,
        py: Python<'_>,
        data: PyBackedBytes,
        extra_features: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let extra = lines_from_py(py, extra_features)?;
        let data: &[u8] = &data;
        self.run(py, move |a| a.parse_fasm_bytes(data, extra))
    }

    /// ``FasmAssembler.add_fasm_line(line, missing_features)``: runs the
    /// feature callback and enables every set bit of ``line`` (a
    /// ``fasm.model.FasmLine``, or a FASM text line). The message of each
    /// feature bit that is not in the database is appended to the list
    /// ``missing_features``; without that list, they are raised at once
    /// as a ``FasmLookupError``.
    #[pyo3(signature = (line, missing_features=None))]
    fn add_fasm_line(
        &self,
        py: Python<'_>,
        line: &Bound<'_, PyAny>,
        missing_features: Option<&Bound<'_, PyList>>,
    ) -> PyResult<()> {
        let lines = if line.cast::<PyString>().is_ok() {
            lines_from_py(py, Some(line))?
        } else {
            lines_from_py(py, Some(PyList::new(py, [line])?.as_any()))?
        };
        self.add_lines(py, lines, missing_features)
    }

    /// Adds several lines (FASM text, or an iterable of
    /// ``fasm.model.FasmLine`` and/or text lines) like ``add_fasm_line``;
    /// the features missing from the database are raised together at the
    /// end (or appended to ``missing_features``).
    #[pyo3(signature = (lines, missing_features=None))]
    fn add_fasm_lines(
        &self,
        py: Python<'_>,
        lines: &Bound<'_, PyAny>,
        missing_features: Option<&Bound<'_, PyList>>,
    ) -> PyResult<()> {
        let lines = lines_from_py(py, Some(lines))?;
        self.add_lines(py, lines, missing_features)
    }

    /// Adds the lines of the part's ``required_features.fasm``
    /// (``Database.required_features()``), like ``fasm2frames`` does after
    /// the FASM file.
    fn add_required_features(&self, py: Python<'_>) -> PyResult<()> {
        let text = self
            .db
            .part_info()
            .map(|info| info.required_features.join("\n"))
            .unwrap_or_default();
        let lines = parse_text(py, text.as_bytes())?;
        self.add_lines(py, lines, None)
    }

    /// ``FasmAssembler.mark_roi_frames``: marks every frame of every bus
    /// of the tiles inside ``roi`` (a ``Roi(x1, x2, y1, y2)`` or any
    /// sequence of the four grid coordinates) in use, so that
    /// ``get_frames(sparse=True)`` includes them.
    fn mark_roi_frames(&self, py: Python<'_>, roi: &Bound<'_, PyAny>) -> PyResult<()> {
        let (x1, x2, y1, y2): (f64, f64, f64, f64) = roi.extract()?;
        let roi = Roi { x1, x2, y1, y2 };
        self.run(py, move |a| {
            a.mark_roi_frames(&roi);
            Ok(())
        })
    }

    /// The STEPDOWN propagation of ``fasm2frames``: if a used IOB of an IO
    /// bank sets a feature whose name contains ``STEPDOWN``, every unused
    /// IOB site of the bank gets the same feature(s) and the bank's
    /// ``HCLK_IOI3`` tile gets ``STEPDOWN``. Call it after all features
    /// have been added.
    fn propagate_stepdown(&self, py: Python<'_>) -> PyResult<()> {
        let db = Arc::clone(&self.db);
        self.run(py, move |a| fasm_xilinx::propagate_stepdown(&db, a))
    }

    /// Sets the function called with the ``fasm.model.SetFasmFeature`` of
    /// every line added from now on, before its bits are looked up
    /// (``FasmAssembler.set_feature_callback``); an exception it raises
    /// aborts the call that added the line and propagates. ``None``
    /// removes it. A callback referring back to the assembler does not
    /// keep it alive: the cycle is collected by the garbage collector.
    fn set_feature_callback(&self, py: Python<'_>, callback: Option<Py<PyAny>>) -> PyResult<()> {
        let Some(callback) = callback else {
            let old = lock(&self.callback).take();
            drop(old);
            return self.run(py, |a| {
                a.clear_feature_callback();
                Ok(())
            });
        };
        let old = lock(&self.callback).replace(callback);
        drop(old);
        let slot = Arc::clone(&self.callback);
        let errors = Arc::clone(&self.callback_error);
        let function: fasm_xilinx::FeatureCallback<'static> = Box::new(move |set_feature| {
            Python::attach(|py| -> PyResult<()> {
                // Cleared by the garbage collector: nothing to call.
                let Some(callback) = lock(&slot).as_ref().map(|c| c.clone_ref(py)) else {
                    return Ok(());
                };
                let model = PyModel::get(py)?;
                let value = model.set_feature(py, set_feature)?;
                callback.bind(py).call1((value,))?;
                Ok(())
            })
            .map_err(|e| {
                *lock(&errors) = Some(e);
                AssemblerError::Python {
                    exception: "Exception",
                    message: "the feature callback raised an exception".to_owned(),
                }
            })
        });
        self.run(py, move |a| {
            a.set_feature_callback(function);
            Ok(())
        })
    }

    /// ``FasmAssembler.get_frames(sparse)``: every frame of the part
    /// (``sparse=False``, zero filled) or only the frames of the buses that
    /// were written or marked by ``mark_roi_frames``, with the set bits
    /// applied, as ``Frames``.
    #[pyo3(signature = (sparse=false))]
    fn get_frames(&self, py: Python<'_>, sparse: bool) -> PyResult<PyFrames> {
        self.run(py, move |a| a.get_frames(sparse))
            .map(PyFrames::new_from)
    }

    /// The warnings of the reference so far (bits beyond the end of a
    /// frame, which prjxray drops: ``frame_set: invalid word address ...``),
    /// in order.
    #[getter]
    fn warnings(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        self.state.with(py, |a| a.warnings().to_vec())
    }

    /// Removes and returns the warnings.
    fn take_warnings(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        self.state.with(py, FasmAssembler::take_warnings)
    }

    /// The number of lines given to the assembler so far.
    fn __len__(&self, py: Python<'_>) -> PyResult<usize> {
        self.state.with(py, |a| a.lines().len())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let lines = self.__len__(py)?;
        Ok(format!("<fasm.xilinx.FasmAssembler: {lines} lines>"))
    }
}

impl PyFasmAssembler {
    fn add_lines(
        &self,
        py: Python<'_>,
        lines: Vec<FasmLine>,
        missing_features: Option<&Bound<'_, PyList>>,
    ) -> PyResult<()> {
        let errors = &self.callback_error;
        let (result, missing, callback_error) = self.state.with(py, move |a| {
            *lock(errors) = None;
            let mut missing = Vec::new();
            let mut result = Ok(());
            for line in lines {
                result = a.add_fasm_line(line, &mut missing);
                if result.is_err() {
                    break;
                }
            }
            let error = if result.is_err() {
                lock(errors).take()
            } else {
                None
            };
            (result, missing, error)
        })?;
        let result = result.map_err(|e| callback_error.unwrap_or_else(|| assembler_error(py, &e)));
        match missing_features {
            Some(list) => {
                for message in &missing {
                    list.append(message)?;
                }
            }
            None if !missing.is_empty() && result.is_ok() => {
                return Err(assembler_error(py, &AssemblerError::Lookup(missing)));
            }
            None => {}
        }
        result
    }
}

/// The FASM -> frames flow of ``xc_fasm.fasm2frames.fasm2frames`` for the
/// part ``db`` was opened for (the ``fasm2frames`` command line tool; on an
/// UltraScale(+) database, prjuray's ``fasm2frames`` flow in 32-bit
/// words, like that tool): the FASM file ``fasm`` (a path) or the FASM
/// ``text`` (``str`` or ``bytes``), the ROI design ``roi`` (a path),
/// the part's required features, the PUDC_B pullup and the STEPDOWN
/// propagation. Returns the ``Frames``; the warnings of the reference
/// (bits beyond the end of a frame) are written to ``sys.stderr``. Runs
/// with the GIL released. Used by ``fasm.xilinx.fasm2frames``.
#[pyfunction]
#[pyo3(signature = (db, fasm=None, *, text=None, sparse=false, roi=None, emit_pudc_b_pullup=false))]
pub(crate) fn fasm2frames(
    py: Python<'_>,
    db: &Bound<'_, PyDatabase>,
    fasm: Option<&Bound<'_, PyAny>>,
    text: Option<&Bound<'_, PyAny>>,
    sparse: bool,
    roi: Option<&Bound<'_, PyAny>>,
    emit_pudc_b_pullup: bool,
) -> PyResult<PyFrames> {
    let db = Arc::clone(&db.get().db);
    let roi: Option<PathBuf> = match roi.filter(|r| !r.is_none()) {
        Some(roi) => Some(fs_path(roi)?),
        None => None,
    };
    let options = Fasm2FramesOptions {
        sparse,
        roi,
        emit_pudc_b_pullup,
    };
    enum Source {
        File(PathBuf),
        Text(PyBackedStr),
        Bytes(PyBackedBytes),
    }
    let source = match (fasm.filter(|f| !f.is_none()), text) {
        (Some(path), None) => Source::File(fs_path(path)?),
        (None, Some(text)) => match text.extract::<PyBackedStr>() {
            Ok(s) => Source::Text(s),
            Err(_) => Source::Bytes(text.extract()?),
        },
        _ => {
            return Err(PyTypeError::new_err(
                "fasm2frames needs either a FASM file or text=",
            ))
        }
    };
    let (result, warnings) = py.detach(|| {
        let input = match &source {
            Source::File(path) => FasmInput::File(path),
            Source::Text(text) => FasmInput::Bytes(text.as_bytes()),
            Source::Bytes(bytes) => FasmInput::Bytes(bytes),
        };
        let mut warnings = Vec::new();
        let result = if db.architecture() == Architecture::Series7 {
            fasm_xilinx::fasm2frames_from(&db, input, &options, &mut |w| {
                warnings.push(w.to_owned());
            })
        } else {
            fasm_xilinx::uray_fasm2frames_from(&db, input, &options)
        };
        (result, warnings)
    });
    // Printed like the reference (`print(..., file=sys.stderr)`), also
    // before an error.
    if !warnings.is_empty() {
        let stderr = py.import("sys")?.getattr("stderr")?;
        for warning in &warnings {
            stderr.call_method1("write", (format!("{warning}\n"),))?;
        }
    }
    match result {
        Ok(frames) => Ok(PyFrames::new_from(frames)),
        Err(e) => Err(assembler_error(py, &e)),
    }
}

/// Reads a ROI ``design.json`` (``info.GRID_X_MIN`` ... ``GRID_Y_MAX``
/// and the optional ``required_features`` list) as ``RoiDesign(roi,
/// required_features)`` (``required_features`` is the FASM text, or
/// ``None``).
#[pyfunction]
pub(crate) fn read_roi_design<'py>(
    py: Python<'py>,
    path: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let path = fs_path(path)?;
    let design = py
        .detach(|| fasm_xilinx::read_roi_design(&path))
        .map_err(|e| assembler_error(py, &e))?;
    let types = XTypes::get(py)?;
    let roi =
        types
            .roi
            .bind(py)
            .call1((design.roi.x1, design.roi.x2, design.roi.y1, design.roi.y2))?;
    types
        .roi_design
        .bind(py)
        .call1((roi, design.required_features))
}

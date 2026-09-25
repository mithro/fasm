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

//! The `fasm._fasm_rs.xilinx` submodule (cargo feature `xilinx`, on by
//! default): Python bindings for the `fasm-xilinx` crate, used by the
//! `fasm.xilinx` package (`fasm/xilinx/__init__.py`), which re-exports
//! everything here and adds `fasm2frames()` / `fasm2bit()`.
//!
//! * `Database` (`database.rs`): a prjxray-db / prjuray-db part database
//!   (optionally through the binary cache), feature lookups;
//! * `FasmAssembler` (`assembler.rs`): FASM -> frames;
//! * `Frames` (`frames.rs`): frame address -> words, `.frm` files;
//! * `write_bitstream` / `read_bitstream` (`bitstream.rs`).
//!
//! The exceptions and the namedtuples are the Python classes of
//! `fasm/xilinx/_types.py`, imported on first use ([`XTypes`]). Opening a
//! database, assembling and reading/writing frames and bitstreams run with
//! the GIL released. See `docs/rewrite/DESIGN-python.md`.

use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use fasm_xilinx::{AssemblerError, DbError, LookupError};
use pyo3::call::PyCallArgs;
use pyo3::exceptions::{
    PyAssertionError, PyAttributeError, PyIndexError, PyOverflowError, PyRuntimeError, PyTypeError,
    PyValueError,
};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::PyString;

mod assembler;
mod bitstream;
mod database;
mod frames;

pub(crate) use database::PyDatabase;
pub(crate) use frames::PyFrames;

/// The classes of `fasm.xilinx._types`.
pub(crate) struct XTypes {
    pub(crate) error: Py<PyAny>,
    pub(crate) db_error: Py<PyAny>,
    pub(crate) lookup_error: Py<PyAny>,
    pub(crate) inconsistent_bits: Py<PyAny>,
    pub(crate) key_error: Py<PyAny>,
    pub(crate) parse_error: Py<PyAny>,
    pub(crate) frm_error: Py<PyAny>,
    pub(crate) bitstream_error: Py<PyAny>,
    pub(crate) tile: Py<PyAny>,
    pub(crate) roi: Py<PyAny>,
    pub(crate) roi_design: Py<PyAny>,
    pub(crate) feature_bits: Py<PyAny>,
}

static XTYPES: PyOnceLock<XTypes> = PyOnceLock::new();

impl XTypes {
    /// The classes, importing `fasm.xilinx._types` on the first call.
    pub(crate) fn get(py: Python<'_>) -> PyResult<&'static XTypes> {
        XTYPES.get_or_try_init(py, || {
            let module = py.import("fasm.xilinx._types")?;
            let get = |name: &str| -> PyResult<Py<PyAny>> { Ok(module.getattr(name)?.unbind()) };
            Ok(XTypes {
                error: get("Error")?,
                db_error: get("DbError")?,
                lookup_error: get("FasmLookupError")?,
                inconsistent_bits: get("FasmInconsistentBits")?,
                key_error: get("FasmKeyError")?,
                parse_error: get("FasmParseError")?,
                frm_error: get("FrmError")?,
                bitstream_error: get("BitstreamError")?,
                tile: get("Tile")?,
                roi: get("Roi")?,
                roi_design: get("RoiDesign")?,
                feature_bits: get("FeatureBits")?,
            })
        })
    }
}

/// An exception of class `class` built with `args` (or the error of
/// building it).
pub(crate) fn make_err<'py>(
    py: Python<'py>,
    class: &Py<PyAny>,
    args: impl PyCallArgs<'py>,
) -> PyErr {
    match class.bind(py).call1(args) {
        Ok(value) => PyErr::from_value(value),
        Err(e) => e,
    }
}

/// An exception of one of the `fasm.xilinx._types` classes, chosen by
/// `class`, with `args`.
pub(crate) fn xerr<'py>(
    py: Python<'py>,
    class: impl FnOnce(&'static XTypes) -> &'static Py<PyAny>,
    args: impl PyCallArgs<'py>,
) -> PyErr {
    match XTypes::get(py) {
        Ok(types) => make_err(py, class(types), args),
        Err(e) => e,
    }
}

/// `fasm.xilinx.DbError(str(error))`.
pub(crate) fn db_error(py: Python<'_>, error: &DbError) -> PyErr {
    xerr(py, |t| &t.db_error, (error.to_string(),))
}

/// The `OSError` (subclass) Python raises for `open(path)` failing with
/// `error`: `[Errno N] strerror: 'path'`.
pub(crate) fn os_error(py: Python<'_>, path: &Path, error: &io::Error) -> PyErr {
    let text = error.to_string();
    // Rust appends " (os error N)" to the strerror text.
    let strerror = text
        .rfind(" (os error ")
        .map_or(text.as_str(), |i| &text[..i]);
    let result = (|| -> PyResult<PyErr> {
        let os_error = py.import("builtins")?.getattr("OSError")?;
        let value = match error.raw_os_error() {
            // `OSError(errno, strerror, filename)` gives the subclass of
            // the errno (FileNotFoundError, ...).
            Some(errno) => os_error.call1((errno, strerror, path.as_os_str()))?,
            None => os_error.call1((format!("{strerror}: {}", path.display()),))?,
        };
        Ok(PyErr::from_value(value))
    })();
    result.unwrap_or_else(|e| e)
}

/// The builtin exception named `name` (the reference tool's exception),
/// or `fasm.xilinx.Error` for any other name.
fn builtin_error(py: Python<'_>, name: &str, message: String) -> PyErr {
    match name {
        "AssertionError" => PyAssertionError::new_err(message),
        "IndexError" => PyIndexError::new_err(message),
        "ValueError" => PyValueError::new_err(message),
        "TypeError" => PyTypeError::new_err(message),
        "AttributeError" => PyAttributeError::new_err(message),
        "OverflowError" => PyOverflowError::new_err(message),
        _ => xerr(py, |t| &t.error, (message,)),
    }
}

/// Converts an [`AssemblerError`] into the Python exception documented in
/// `fasm/xilinx/_types.py`: `str()` is the message the command line
/// tools print after `<reference exception>: `, and the instance's
/// `reference_exception` attribute is that exception's name.
pub(crate) fn assembler_error(py: Python<'_>, error: &AssemblerError) -> PyErr {
    let message = error.to_string();
    let err = match error {
        AssemblerError::Parse(e) => xerr(py, |t| &t.parse_error, (message, e.line, e.column)),
        AssemblerError::OpenFasm { .. } => xerr(py, |t| &t.parse_error, (message, 0, 0)),
        AssemblerError::Lookup(messages) => {
            xerr(py, |t| &t.lookup_error, (message, messages.clone()))
        }
        AssemblerError::InconsistentBits(m) => xerr(py, |t| &t.inconsistent_bits, (m.clone(),)),
        AssemblerError::KeyError(key) => xerr(py, |t| &t.key_error, (key.clone(),)),
        AssemblerError::Io { path, source } => os_error(py, path, source),
        AssemblerError::Json { .. } => PyValueError::new_err(message),
        AssemblerError::Db(e) => db_error(py, e),
        AssemblerError::FrameAddressOverflow { .. } => PyOverflowError::new_err(message),
        AssemblerError::Python { exception, .. } => builtin_error(py, exception, message),
        _ => xerr(py, |t| &t.error, (message,)),
    };
    // Instances of builtin exceptions accept attributes too.
    let _ = err
        .value(py)
        .setattr("reference_exception", error.python_exception());
    err
}

/// The exception for a failed [`fasm_xilinx::Database::lookup_feature`]:
/// what the assembler raises for the same feature (without its line).
pub(crate) fn lookup_error(py: Python<'_>, error: &LookupError) -> PyErr {
    match error {
        LookupError::UnknownTile { tile } => xerr(py, |t| &t.key_error, (tile.to_string(),)),
        LookupError::UnknownTileType { tile_type, .. } => xerr(
            py,
            |t| &t.key_error,
            (tile_type.to_string().to_ascii_uppercase(),),
        ),
        LookupError::UnknownFeature { .. } | LookupError::MissingBitsBlock { .. } => {
            let message = error.to_string();
            xerr(py, |t| &t.lookup_error, (message.clone(), vec![message]))
        }
        _ => xerr(py, |t| &t.error, (error.to_string(),)),
    }
}

/// A path argument: `str`, `bytes` or `os.PathLike` (through
/// `os.fsdecode`, lossless on Unix).
pub(crate) fn fs_path(obj: &Bound<'_, PyAny>) -> PyResult<PathBuf> {
    obj.py()
        .import("os")?
        .call_method1("fsdecode", (obj,))?
        .extract()
}

/// `True` if `obj` is a path argument (`str`, `bytes` or `os.PathLike`)
/// rather than a file object or data.
pub(crate) fn is_path(obj: &Bound<'_, PyAny>) -> PyResult<bool> {
    if obj.is_instance_of::<PyString>() {
        return Ok(true);
    }
    let os = obj.py().import("os")?;
    obj.is_instance(&os.getattr("PathLike")?)
}

static NEXT_THREAD: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static THREAD: Cell<u64> = const { Cell::new(0) };
}

/// A small, never zero, id of the current thread.
fn thread_id() -> u64 {
    THREAD.with(|id| {
        if id.get() == 0 {
            id.set(NEXT_THREAD.fetch_add(1, Ordering::Relaxed));
        }
        id.get()
    })
}

/// A mutex taken with the GIL released (so that a thread waiting for it
/// never blocks the thread holding it, whose feature callback may need
/// the GIL), that detects a re-entrant use from the thread that holds it
/// (a feature callback calling back into its own assembler) instead of
/// deadlocking.
pub(crate) struct Locked<T> {
    mutex: Mutex<T>,
    owner: AtomicU64,
}

/// Clears [`Locked::owner`] when a [`Locked::with`] call ends, also on a
/// panic.
struct OwnerGuard<'a>(&'a AtomicU64);

impl Drop for OwnerGuard<'_> {
    fn drop(&mut self) {
        self.0.store(0, Ordering::Release);
    }
}

impl<T: Send> Locked<T> {
    pub(crate) fn new(value: T) -> Self {
        Locked {
            mutex: Mutex::new(value),
            owner: AtomicU64::new(0),
        }
    }

    /// Runs `f` on the value with the GIL released.
    ///
    /// # Errors
    ///
    /// `RuntimeError` for a re-entrant call.
    pub(crate) fn with<R: Send>(
        &self,
        py: Python<'_>,
        f: impl FnOnce(&mut T) -> R + Send,
    ) -> PyResult<R> {
        let me = thread_id();
        if self.owner.load(Ordering::Acquire) == me {
            return Err(PyRuntimeError::new_err(
                "the object is in use by this thread (called from its own feature callback?)",
            ));
        }
        Ok(py.detach(|| {
            // A panic while the value was borrowed leaves it in a
            // consistent state for the assembler (every change is
            // complete before the next can fail).
            let mut value = self
                .mutex
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.owner.store(me, Ordering::Release);
            let _guard = OwnerGuard(&self.owner);
            f(&mut value)
        }))
    }
}

/// Registers the classes and functions into the `xilinx` submodule `m`.
pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<database::PyDatabase>()?;
    m.add_class::<assembler::PyFasmAssembler>()?;
    m.add_class::<frames::PyFrames>()?;
    m.add_function(wrap_pyfunction!(bitstream::write_bitstream, m)?)?;
    m.add_function(wrap_pyfunction!(bitstream::read_bitstream, m)?)?;
    m.add_function(wrap_pyfunction!(assembler::fasm2frames, m)?)?;
    m.add_function(wrap_pyfunction!(assembler::read_roi_design, m)?)?;
    m.add_function(wrap_pyfunction!(frames::dump_frames_sparse, m)?)?;
    m.add(
        "ARCHITECTURES",
        (
            fasm_xilinx::Architecture::Series7.name(),
            fasm_xilinx::Architecture::UltraScale.name(),
            fasm_xilinx::Architecture::UltraScalePlus.name(),
        ),
    )?;
    Ok(())
}

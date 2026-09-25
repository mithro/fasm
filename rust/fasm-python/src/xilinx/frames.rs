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

//! `fasm.xilinx.Frames`.

use std::sync::Arc;

use fasm_xilinx::{Frames, FrmError};
use pyo3::exceptions::{PyKeyError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::{PyBackedBytes, PyBackedStr};
use pyo3::types::{PyBytes, PyDict, PyIterator, PyList, PyMapping, PyString, PyTuple};

use super::{fs_path, is_path, os_error, xerr};

/// The `.frm` error as `fasm.xilinx.FrmError`.
fn frm_error(py: Python<'_>, error: &FrmError) -> PyErr {
    xerr(py, |t| &t.frm_error, (error.to_string(), error.line))
}

/// `ValueError` for frames of no words.
fn check_words_per_frame(words_per_frame: usize) -> PyResult<()> {
    if words_per_frame == 0 {
        return Err(PyValueError::new_err("words_per_frame must not be 0"));
    }
    Ok(())
}

/// Emits `warnings` with `warnings.warn` (a `UserWarning`).
pub(crate) fn warn_all(py: Python<'_>, warnings: &[String]) -> PyResult<()> {
    if warnings.is_empty() {
        return Ok(());
    }
    let module = py.import("warnings")?;
    for warning in warnings {
        module.call_method1("warn", (warning.as_str(),))?;
    }
    Ok(())
}

/// Configuration frames: frame address -> ``words_per_frame`` 32-bit
/// words, in ascending address order (what ``FasmAssembler.get_frames``
/// and ``read_bitstream`` return, and ``write_bitstream`` takes).
///
/// A read only mapping (``collections.abc.Mapping``): ``frames[address]``
/// is a new ``list`` of ``int`` (like the ``dict`` of lists the reference
/// ``get_frames`` returns), iteration gives the addresses in ascending
/// order, and ``frames == {address: [words], ...}`` compares with any
/// mapping. ``Frames(mapping, words_per_frame=None)`` builds frames from a
/// mapping of address to a list of words (or ``bytes``, little endian);
/// ``words_per_frame`` defaults to the length of the first frame (101
/// when empty). All words of all frames are kept in one contiguous
/// buffer (``to_bytes()``).
#[pyclass(frozen, name = "Frames", module = "fasm.xilinx", mapping)]
pub(crate) struct PyFrames {
    pub(crate) frames: Arc<Frames>,
}

impl PyFrames {
    pub(crate) fn new_from(frames: Frames) -> Self {
        PyFrames {
            frames: Arc::new(frames),
        }
    }

    fn words(&self, address: u32) -> PyResult<&[u32]> {
        self.frames
            .get(address)
            .ok_or_else(|| PyKeyError::new_err(address))
    }

    /// Converts a mapping or an existing `Frames` into Rust frames.
    pub(crate) fn from_py(obj: &Bound<'_, PyAny>) -> PyResult<Arc<Frames>> {
        if let Ok(frames) = obj.cast::<PyFrames>() {
            return Ok(Arc::clone(&frames.get().frames));
        }
        Ok(Arc::new(Self::build(obj, None)?))
    }

    fn build(mapping: &Bound<'_, PyAny>, words_per_frame: Option<usize>) -> PyResult<Frames> {
        let items: Vec<(u32, Bound<'_, PyAny>)> = if let Ok(dict) = mapping.cast::<PyDict>() {
            dict.iter()
                .map(|(k, v)| Ok((k.extract()?, v)))
                .collect::<PyResult<_>>()?
        } else if let Ok(map) = mapping.cast::<PyMapping>() {
            map.items()?
                .iter()
                .map(|item| {
                    let (k, v): (u32, Bound<'_, PyAny>) = item.extract()?;
                    Ok((k, v))
                })
                .collect::<PyResult<_>>()?
        } else {
            return Err(PyTypeError::new_err(
                "frames must be a mapping of frame address to words",
            ));
        };
        let mut rows: Vec<(u32, Vec<u32>)> = Vec::with_capacity(items.len());
        for (address, value) in items {
            let words: Vec<u32> = if let Ok(bytes) = value.cast::<PyBytes>() {
                let data = bytes.as_bytes();
                if data.len() % 4 != 0 {
                    return Err(PyValueError::new_err(format!(
                        "frame 0x{address:08X}: {} bytes is not a whole number of words",
                        data.len()
                    )));
                }
                data.chunks_exact(4)
                    .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect()
            } else {
                value.extract()?
            };
            rows.push((address, words));
        }
        let wpf = words_per_frame
            .or_else(|| rows.first().map(|(_, w)| w.len()))
            .unwrap_or(fasm_xilinx::Architecture::Series7.words_per_frame());
        check_words_per_frame(wpf)?;
        let mut frames = Frames::new(wpf);
        for (address, words) in &rows {
            if words.len() != wpf {
                return Err(PyValueError::new_err(format!(
                    "frame 0x{address:08X} has {} words instead of {wpf}",
                    words.len()
                )));
            }
            frames.get_or_insert_zeroed(*address).copy_from_slice(words);
        }
        Ok(frames)
    }

    fn read<'py>(
        py: Python<'py>,
        data: &[u8],
        words_per_frame: usize,
    ) -> PyResult<Bound<'py, PyFrames>> {
        check_words_per_frame(words_per_frame)?;
        let (result, warnings) = py.detach(|| {
            let mut warnings = Vec::new();
            let result = Frames::read_frm(data, words_per_frame, &mut |w| {
                warnings.push(w.to_owned());
            });
            (result, warnings)
        });
        warn_all(py, &warnings)?;
        match result {
            Ok(frames) => Bound::new(py, PyFrames::new_from(frames)),
            Err(e) => Err(frm_error(py, &e)),
        }
    }
}

#[pymethods]
impl PyFrames {
    #[new]
    #[pyo3(signature = (frames=None, words_per_frame=None))]
    fn py_new(frames: Option<&Bound<'_, PyAny>>, words_per_frame: Option<usize>) -> PyResult<Self> {
        let frames = match frames {
            Some(mapping) => Self::build(mapping, words_per_frame)?,
            None => {
                let wpf =
                    words_per_frame.unwrap_or(fasm_xilinx::Architecture::Series7.words_per_frame());
                check_words_per_frame(wpf)?;
                Frames::new(wpf)
            }
        };
        Ok(PyFrames::new_from(frames))
    }

    /// 32-bit words per frame.
    #[getter]
    fn words_per_frame(&self) -> usize {
        self.frames.words_per_frame()
    }

    fn __len__(&self) -> usize {
        self.frames.len()
    }

    fn __contains__(&self, address: &Bound<'_, PyAny>) -> bool {
        address
            .extract::<u32>()
            .is_ok_and(|a| self.frames.contains(a))
    }

    fn __getitem__(&self, address: &Bound<'_, PyAny>) -> PyResult<Vec<u32>> {
        // Any key that is not a frame address (a negative or too large
        // integer, another type) is simply missing, as in a `dict`.
        match address.extract::<u32>() {
            Ok(a) => Ok(self.words(a)?.to_vec()),
            Err(_) => Err(PyKeyError::new_err(address.clone().unbind())),
        }
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyIterator>> {
        PyList::new(py, self.frames.addresses())?.try_iter()
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        if let Ok(other) = other.cast::<PyFrames>() {
            return Ok(*self.frames == *other.get().frames);
        }
        if other.cast::<PyMapping>().is_err() {
            return Ok(false);
        }
        self.to_dict(py)?.as_any().eq(other)
    }

    fn __repr__(&self) -> String {
        format!(
            "<fasm.xilinx.Frames: {} frames of {} words>",
            self.frames.len(),
            self.frames.words_per_frame()
        )
    }

    /// ``self[address]`` if present, else ``default``.
    #[pyo3(signature = (address, default=None))]
    fn get<'py>(
        &self,
        py: Python<'py>,
        address: &Bound<'py, PyAny>,
        default: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        match address
            .extract::<u32>()
            .ok()
            .and_then(|a| self.frames.get(a))
        {
            Some(words) => Ok(PyList::new(py, words)?.into_any()),
            None => Ok(default.unwrap_or_else(|| py.None().into_bound(py))),
        }
    }

    /// The frame addresses, in ascending order (a ``list``).
    fn keys(&self) -> Vec<u32> {
        self.frames.addresses().to_vec()
    }

    /// The words of every frame, in address order (a ``list`` of lists).
    fn values(&self) -> Vec<Vec<u32>> {
        self.frames.iter().map(|(_, w)| w.to_vec()).collect()
    }

    /// ``(address, words)`` for every frame, in address order.
    fn items(&self) -> Vec<(u32, Vec<u32>)> {
        self.frames.iter().map(|(a, w)| (a, w.to_vec())).collect()
    }

    /// A ``dict`` of address -> list of words (what the reference
    /// ``FasmAssembler.get_frames`` returns).
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (address, words) in self.frames.iter() {
            dict.set_item(address, PyList::new(py, words)?)?;
        }
        Ok(dict)
    }

    /// The words of the frame at ``address`` as little endian ``bytes``.
    fn frame_bytes<'py>(&self, py: Python<'py>, address: u32) -> PyResult<Bound<'py, PyBytes>> {
        let words = self.words(address)?;
        PyBytes::new_with(py, words.len() * 4, |buf| {
            for (chunk, word) in buf.chunks_exact_mut(4).zip(words) {
                chunk.copy_from_slice(&word.to_le_bytes());
            }
            Ok(())
        })
    }

    /// All the words of all frames, in address order, as little endian
    /// ``bytes`` (``len(frames) * words_per_frame * 4`` bytes; with
    /// ``keys()``, e.g. ``numpy.frombuffer(frames.to_bytes(),
    /// '<u4').reshape(len(frames), -1)``).
    fn to_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let frames = &self.frames;
        PyBytes::new_with(py, frames.len() * frames.words_per_frame() * 4, |buf| {
            let mut chunks = buf.chunks_exact_mut(4);
            for (_, words) in frames.iter() {
                for (chunk, word) in (&mut chunks).zip(words) {
                    chunk.copy_from_slice(&word.to_le_bytes());
                }
            }
            Ok(())
        })
    }

    /// Every set bit as a ``(frame_address, word, bit)`` tuple, in frame,
    /// word and bit order.
    fn set_bits(&self) -> Vec<(u32, u32, u32)> {
        self.frames.set_bits().collect()
    }

    /// The frames as ``.frm`` text: ``0x%08X`` address, a space, the
    /// words as comma separated ``0x%08X``, one frame per line, in
    /// address order (the ``dump_frm`` of ``xc_fasm.fasm2frames``).
    fn to_frm(&self, py: Python<'_>) -> String {
        let frames = &self.frames;
        py.detach(|| frames.to_frm_string())
    }

    /// Writes the frames as a ``.frm`` file to ``target``: a path (``str``,
    /// ``bytes`` or ``os.PathLike``; written with the GIL released) or a
    /// file object (text or binary). Byte for byte the output of
    /// ``fasm2frames``.
    fn write_frm(&self, py: Python<'_>, target: &Bound<'_, PyAny>) -> PyResult<()> {
        let frames = &self.frames;
        if is_path(target)? || target.cast::<PyBytes>().is_ok() {
            let path = fs_path(target)?;
            let result = py.detach(|| {
                let file = std::fs::File::create(&path)?;
                let mut out = std::io::BufWriter::with_capacity(1 << 16, file);
                frames.write_frm(&mut out)?;
                std::io::Write::flush(&mut out)
            });
            return result.map_err(|e| os_error(py, &path, &e));
        }
        let text = py.detach(|| frames.to_frm_string());
        let io = py.import("io")?;
        if target.is_instance(&io.getattr("TextIOBase")?)? {
            target.call_method1("write", (text,))?;
        } else if target.is_instance(&io.getattr("RawIOBase")?)?
            || target.is_instance(&io.getattr("BufferedIOBase")?)?
        {
            target.call_method1("write", (PyBytes::new(py, text.as_bytes()),))?;
        } else {
            target.call_method1("write", (text,))?;
        }
        Ok(())
    }

    /// Parses ``.frm`` text (``str`` or ``bytes``) with ``words_per_frame``
    /// words per frame, like ``xc7frames2bit`` reads its ``--frm_file``:
    /// ``#`` lines are comments, a line with another number of words is
    /// skipped with a warning (``warnings.warn``), the first of two frames
    /// with the same address wins. Raises ``FrmError`` for a number that
    /// does not parse (where ``xc7frames2bit`` aborts).
    #[staticmethod]
    #[pyo3(signature = (data, words_per_frame=101))]
    fn from_frm<'py>(
        py: Python<'py>,
        data: &Bound<'py, PyAny>,
        words_per_frame: usize,
    ) -> PyResult<Bound<'py, PyFrames>> {
        if data.cast::<PyString>().is_ok() {
            let text: PyBackedStr = data.extract()?;
            return Self::read(py, text.as_bytes(), words_per_frame);
        }
        let bytes: PyBackedBytes = data.extract()?;
        Self::read(py, &bytes, words_per_frame)
    }

    /// Reads a ``.frm`` file (a path, or a file object whose ``read()``
    /// returns ``str`` or ``bytes``); see ``from_frm``.
    #[staticmethod]
    #[pyo3(signature = (source, words_per_frame=101))]
    fn read_frm<'py>(
        py: Python<'py>,
        source: &Bound<'py, PyAny>,
        words_per_frame: usize,
    ) -> PyResult<Bound<'py, PyFrames>> {
        if is_path(source)? || source.cast::<PyBytes>().is_ok() {
            let path = fs_path(source)?;
            let data = py
                .detach(|| std::fs::read(&path))
                .map_err(|e| os_error(py, &path, &e))?;
            return Self::read(py, &data, words_per_frame);
        }
        let data = source.call_method0("read")?;
        Self::from_frm(py, &data, words_per_frame)
    }

    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyTuple>> {
        let py = slf.py();
        let this = slf.get();
        let args = (this.to_dict(py)?, this.frames.words_per_frame());
        (slf.get_type(), args).into_pyobject(py)
    }
}

/// The ``--debug`` output of ``fasm2frames`` for ``frames``
/// (``dump_frames_sparse``: a blank line, ``Frames: N``, then every non
/// zero word of every frame).
#[pyfunction]
pub(crate) fn dump_frames_sparse(frames: &Bound<'_, PyAny>) -> PyResult<String> {
    let frames = PyFrames::from_py(frames)?;
    let mut out = Vec::new();
    fasm_xilinx::dump_frames_sparse(&frames, &mut out)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

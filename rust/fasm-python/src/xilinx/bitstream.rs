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

//! `fasm.xilinx.write_bitstream` and `fasm.xilinx.read_bitstream`.

use std::sync::Arc;

use fasm_xilinx::bitstream::{
    bitstream_bytes_with, utc_date_time, BitstreamFormat, BitstreamOptions, BitstreamReader,
};
use fasm_xilinx::{Architecture, Database, Part};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::PyBytes;

use super::frames::warn_all;
use super::{db_error, fs_path, is_path, os_error, xerr, PyDatabase, PyFrames};

/// The `format` argument: `None`, `"<arch>"` / `"native:<arch>"`
/// (prjuray-tools' implementation of the architecture, also prjxray's for
/// Series7) or `"prjxray:<arch>"` (the plain prjxray tools: the Series7
/// part type, frame addresses and ECC with the word count and packets of
/// the architecture).
fn parse_format(format: Option<&str>) -> PyResult<Option<BitstreamFormat>> {
    let Some(format) = format else {
        return Ok(None);
    };
    let (flavor, name) = format.split_once(':').unwrap_or(("native", format));
    let arch = Architecture::from_name(name);
    match (flavor, arch) {
        ("native", Some(arch)) => Ok(Some(BitstreamFormat::native(arch))),
        ("prjxray", Some(arch)) => Ok(Some(BitstreamFormat::prjxray(arch))),
        _ => Err(PyValueError::new_err(format!(
            "unknown bitstream format {format:?} (expected Series7, UltraScale, UltraScalePlus, \
             optionally prefixed with native: or prjxray:)"
        ))),
    }
}

/// The part of a bitstream: a database's, or read from a `part.yaml`.
enum PartSource {
    Database(Arc<Database>),
    File(Part),
}

impl PartSource {
    fn part(&self) -> &Part {
        match self {
            PartSource::Database(db) => db.part().expect("checked by resolve_part"),
            PartSource::File(part) => part,
        }
    }
}

/// The part, its default name and the format of the `part` and `format`
/// arguments.
fn resolve_part(
    py: Python<'_>,
    part: &Bound<'_, PyAny>,
    format: Option<&str>,
) -> PyResult<(PartSource, Option<String>, BitstreamFormat)> {
    let format = parse_format(format)?;
    if let Ok(db) = part.cast::<PyDatabase>() {
        let db = Arc::clone(&db.get().db);
        let Some(the_part) = db.part() else {
            let name = db.part_info().map_or("", |i| i.name.as_str()).to_owned();
            return Err(xerr(
                py,
                |t| &t.db_error,
                (format!(
                    "{}: part {name:?} has no frame tree (part.yaml or part.json)",
                    db.root().display()
                ),),
            ));
        };
        let format = format.unwrap_or(BitstreamFormat::native(the_part.architecture));
        let name = db.part_info().map(|i| i.name.clone());
        return Ok((PartSource::Database(db), name, format));
    }
    if !is_path(part)? {
        return Err(PyTypeError::new_err(
            "part must be a fasm.xilinx.Database or the path of a part.yaml file",
        ));
    }
    let path = fs_path(part)?;
    // `ArchType::Part::FromFile`: an untagged part.yaml is read as the
    // format's part type.
    let default = format.map_or(Architecture::Series7, |f| f.addressing);
    let the_part = py
        .detach(|| Part::from_yaml_file(&path, default))
        .map_err(|e| db_error(py, &e))?;
    let format = format.unwrap_or(BitstreamFormat::native(the_part.architecture));
    Ok((PartSource::File(the_part), None, format))
}

/// `os.fsencode(value)` for a `str`, `bytes` or `os.PathLike`.
fn fs_bytes(value: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    value
        .py()
        .import("os")?
        .call_method1("fsencode", (value,))?
        .extract()
}

/// The header date and time of `source_date_epoch`, else of
/// `$SOURCE_DATE_EPOCH` (a warning if it is not an integer), else `None`
/// (the current time).
fn header_time(
    py: Python<'_>,
    source_date_epoch: Option<i64>,
) -> PyResult<(Option<String>, Option<String>)> {
    if let Some(seconds) = source_date_epoch {
        let (date, time) = utc_date_time(seconds);
        return Ok((Some(date), Some(time)));
    }
    let value: Option<String> = py
        .import("os")?
        .getattr("environ")?
        .call_method1("get", ("SOURCE_DATE_EPOCH",))?
        .extract()?;
    let Some(value) = value else {
        return Ok((None, None));
    };
    match value.trim().parse::<i64>() {
        Ok(seconds) => {
            let (date, time) = utc_date_time(seconds);
            Ok((Some(date), Some(time)))
        }
        Err(_) => {
            warn_all(
                py,
                &[format!(
                    "SOURCE_DATE_EPOCH={value:?} is not an integer, using the current time"
                )],
            )?;
            Ok((None, None))
        }
    }
}

/// Writes ``frames`` (``Frames`` or a mapping of address to words) as a
/// ``.bit`` bitstream for ``part``, byte for byte like ``xc7frames2bit``
/// (Series7) and prjuray-tools' ``xcframes2bit`` (UltraScale,
/// UltraScale+): the ``.bit`` header, the configuration packets with the
/// part's IDCODE and every frame of the part (missing frames zero filled)
/// with its ECC.
///
/// * ``part``: a ``Database`` (its ``part.yaml`` / ``part.json``; the
///   header part name defaults to its part) or the path of a
///   ``part.yaml`` (read like ``--part_file``).
/// * ``output``: ``None`` returns the ``bytes``; a path (written with the
///   GIL released) or a binary file object receives them.
/// * ``format``: ``None`` (the part's architecture), ``'Series7'``,
///   ``'UltraScale'``, ``'UltraScalePlus'`` (prjuray-tools'
///   implementation, ``--architecture``) or ``'prjxray:UltraScale'`` /
///   ``'prjxray:UltraScalePlus'`` (the plain prjxray ``xc7frames2bit``'s:
///   Series7 part type, frame addresses and ECC with the UltraScale(+)
///   word count and packets).
/// * header fields: ``part_name`` (field b), ``design_name`` (field a,
///   before ``;Generator=``: ``xc7frames2bit`` writes its ``--frm_file``),
///   ``generator`` (default ``'xc7frames2bit'``), and the date and time
///   of ``source_date_epoch`` (seconds since the epoch), else of
///   ``$SOURCE_DATE_EPOCH`` like the command line tools, else now (UTC).
///
/// Raises ``BitstreamError`` (a part of another architecture than the
/// format's, frames of another size), ``DbError`` (a part file that cannot
/// be read, a database without a frame tree).
#[pyfunction]
#[pyo3(signature = (frames, part, output=None, *, format=None, part_name=None, design_name=None, generator=None, source_date_epoch=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_bitstream<'py>(
    py: Python<'py>,
    frames: &Bound<'py, PyAny>,
    part: &Bound<'py, PyAny>,
    output: Option<&Bound<'py, PyAny>>,
    format: Option<&str>,
    part_name: Option<String>,
    design_name: Option<&Bound<'py, PyAny>>,
    generator: Option<String>,
    source_date_epoch: Option<i64>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let frames = PyFrames::from_py(frames)?;
    let (source, default_name, format) = resolve_part(py, part, format)?;
    let (date, time) = header_time(py, source_date_epoch)?;
    let options = BitstreamOptions {
        design_name: match design_name {
            Some(name) => fs_bytes(name)?,
            None => Vec::new(),
        },
        generator: generator
            .unwrap_or_else(|| "xc7frames2bit".to_owned())
            .into_bytes(),
        part_name: part_name.or(default_name).unwrap_or_default().into_bytes(),
        date,
        time,
    };
    let output = output.filter(|o| !o.is_none());
    let path = match output {
        Some(o) if is_path(o)? => Some(fs_path(o)?),
        _ => None,
    };
    let result = py.detach(|| {
        let bytes = bitstream_bytes_with(source.part(), &frames, &options, &format)?;
        if let Some(path) = &path {
            std::fs::write(path, &bytes)?;
        }
        Ok::<_, fasm_xilinx::bitstream::BitstreamError>(bytes)
    });
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(fasm_xilinx::bitstream::BitstreamError::Io(e)) => {
            return Err(os_error(
                py,
                path.as_deref().unwrap_or(std::path::Path::new("")),
                &e,
            ))
        }
        Err(e) => return Err(xerr(py, |t| &t.bitstream_error, (e.to_string(),))),
    };
    match output {
        None => Ok(Some(PyBytes::new(py, &bytes))),
        Some(_) if path.is_some() => Ok(None),
        Some(file) => {
            file.call_method1("write", (PyBytes::new(py, &bytes),))?;
            Ok(None)
        }
    }
}

/// Reads the frames of a ``.bit`` bitstream (or raw configuration data:
/// everything after the first sync word) for ``part``, like ``bitread``
/// (prjxray / prjuray-tools): the packets are replayed on the part's frame
/// tree.
///
/// * ``source``: the data (``bytes``, ``bytearray``), a path, or a binary
///   file object.
/// * ``part`` and ``format``: as for ``write_bitstream``.
/// * ``clear_ecc`` (default ``True``) clears the ECC bits of every frame,
///   which gives back the frames ``fasm2frames`` wrote (``bitread
///   --frm_out``; ``-C`` keeps them); ``skip_zero`` leaves all zero frames
///   out.
///
/// Returns ``Frames``. Raises ``BitstreamError`` (no sync word, an IDCODE
/// that is not the part's, a part of another architecture than the
/// format's), ``DbError``.
#[pyfunction]
#[pyo3(signature = (source, part, *, format=None, clear_ecc=true, skip_zero=false))]
pub(crate) fn read_bitstream(
    py: Python<'_>,
    source: &Bound<'_, PyAny>,
    part: &Bound<'_, PyAny>,
    format: Option<&str>,
    clear_ecc: bool,
    skip_zero: bool,
) -> PyResult<PyFrames> {
    let (part_source, _, format) = resolve_part(py, part, format)?;
    let data: Vec<u8> = if is_path(source)? {
        let path = fs_path(source)?;
        py.detach(|| std::fs::read(&path))
            .map_err(|e| os_error(py, &path, &e))?
    } else if let Ok(bytes) = source.extract::<PyBackedBytes>() {
        bytes.to_vec()
    } else {
        let bytes: PyBackedBytes = source.call_method0("read")?.extract()?;
        bytes.to_vec()
    };
    let result = py.detach(|| {
        let reader = BitstreamReader::from_bytes(&data)
            .ok_or_else(|| "Input doesn't look like a bitstream".to_owned())?;
        let configuration = reader
            .configuration_with(part_source.part(), &format)
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(configuration.to_frames(clear_ecc, skip_zero))
    });
    match result {
        Ok(frames) => Ok(PyFrames::new_from(frames)),
        Err(message) => Err(xerr(py, |t| &t.bitstream_error, (message,))),
    }
}

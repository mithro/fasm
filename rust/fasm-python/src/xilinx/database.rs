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

//! `fasm.xilinx.Database`.

use std::sync::Arc;

use fasm::idstring::IdString;
use fasm_xilinx::cache::CacheOptions;
use fasm_xilinx::{Database, FeatureLookup, Layout};
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyList, PyTuple};

use super::{db_error, fs_path, lookup_error, XTypes};

/// The cache options of the `cache` argument: ``True`` or ``None`` (the
/// command line tools' settings: ``$FASM_XDB_CACHE``, else
/// ``$XDG_CACHE_HOME/fasm/db``, else ``~/.cache/fasm/db``;
/// ``FASM_XDB_CACHE=0`` disables it), ``False`` (no cache) or a directory.
fn cache_options(cache: Option<&Bound<'_, PyAny>>) -> PyResult<CacheOptions> {
    let Some(cache) = cache else {
        return Ok(CacheOptions::from_env());
    };
    if let Ok(flag) = cache.cast::<PyBool>() {
        return Ok(if flag.is_true() {
            CacheOptions::from_env()
        } else {
            CacheOptions::disabled()
        });
    }
    let directory = fs_path(cache)
        .map_err(|_| PyTypeError::new_err("cache must be True, False, None or a directory path"))?;
    let mut options = CacheOptions::from_env();
    options.directory = Some(directory);
    Ok(options)
}

/// A prjxray-db (Series7) or prjuray-db (UltraScale+) database family
/// opened for one part: its tile types and segbits, the part's tile grid,
/// frame tree, IO banks and required features.
///
/// ``Database(db_root, part=None, cache=True)`` is ``Database.open(...)``.
/// A database is immutable and can be shared by any number of
/// ``FasmAssembler`` objects and threads.
#[pyclass(frozen, name = "Database", module = "fasm.xilinx")]
pub(crate) struct PyDatabase {
    pub(crate) db: Arc<Database>,
}

#[pymethods]
impl PyDatabase {
    #[new]
    #[pyo3(signature = (db_root, part=None, cache=None))]
    fn new(
        py: Python<'_>,
        db_root: &Bound<'_, PyAny>,
        part: Option<String>,
        cache: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Self::open(py, db_root, part, cache)
    }

    /// Opens the family directory ``db_root`` (e.g. ``prjxray-db/artix7``,
    /// ``prjuray-db/zynqusp``) for ``part`` (e.g. ``xc7a35tcsg324-1``;
    /// ``None`` loads only the tile types, which cannot assemble).
    ///
    /// ``cache``: ``True`` (the default; ``None`` is the same) loads the
    /// part through the binary database cache like the command line tools
    /// (directory ``$FASM_XDB_CACHE``, else ``$XDG_CACHE_HOME/fasm/db``,
    /// else ``~/.cache/fasm/db``; ``FASM_XDB_CACHE=0`` disables it),
    /// ``False`` loads the text files, and a path uses that cache
    /// directory. The result is the same either way. Runs with the GIL
    /// released.
    ///
    /// Raises ``DbError`` (a missing or malformed file, an unknown part,
    /// not a database directory).
    #[staticmethod]
    #[pyo3(signature = (db_root, part=None, cache=None))]
    fn open(
        py: Python<'_>,
        db_root: &Bound<'_, PyAny>,
        part: Option<String>,
        cache: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let root = fs_path(db_root)?;
        let options = cache_options(cache)?;
        let result = py.detach(|| Database::open_cached(&root, part.as_deref(), &options));
        match result {
            Ok(db) => Ok(PyDatabase { db: Arc::new(db) }),
            Err(e) => Err(db_error(py, &e)),
        }
    }

    /// The database root directory, as given.
    #[getter]
    fn root(&self) -> std::ffi::OsString {
        self.db.root().as_os_str().to_owned()
    }

    /// The part name, or ``None``.
    #[getter]
    fn part(&self) -> Option<String> {
        self.db.part_info().map(|info| info.name.clone())
    }

    /// ``'prjxray'`` (prjxray-db) or ``'prjuray'`` (prjuray-db).
    #[getter]
    fn layout(&self) -> &'static str {
        match self.db.layout() {
            Layout::Prjxray => "prjxray",
            Layout::Prjuray => "prjuray",
        }
    }

    /// ``'Series7'``, ``'UltraScale'`` or ``'UltraScalePlus'``.
    #[getter]
    fn architecture(&self) -> &'static str {
        self.db.architecture().name()
    }

    /// 32-bit words per configuration frame (101, 123 or 93).
    #[getter]
    fn words_per_frame(&self) -> usize {
        self.db.architecture().words_per_frame()
    }

    /// The part's IDCODE (``part.yaml`` / ``part.json``), or ``None``.
    #[getter]
    fn idcode(&self) -> Option<u32> {
        self.db.part_info().and_then(|info| info.idcode)
    }

    /// The names of the tile types of the family, in database order.
    fn tile_types(&self) -> Vec<String> {
        self.db
            .tile_types()
            .iter()
            .map(|t| t.name.to_string())
            .collect()
    }

    /// The segbits features of ``tile_type`` (names within a tile, e.g.
    /// ``SLICEL_X0.ALUT.INIT[00]``), in database order: ``CLB_IO_CLK`` ones
    /// first, then ``BLOCK_RAM`` ones. Raises ``KeyError`` for an unknown
    /// tile type.
    fn tile_type_features(&self, tile_type: &str) -> PyResult<Vec<String>> {
        let tile_type = self.find_tile_type(tile_type)?;
        Ok(tile_type
            .segbits
            .entries()
            .iter()
            .map(|e| e.feature.to_string())
            .collect())
    }

    /// The pseudo PIPs of ``tile_type`` as ``(feature, type)`` tuples,
    /// ``type`` being ``'always'``, ``'default'`` or ``'hint'``. Raises
    /// ``KeyError`` for an unknown tile type.
    fn pseudo_pips(&self, tile_type: &str) -> PyResult<Vec<(String, &'static str)>> {
        let tile_type = self.find_tile_type(tile_type)?;
        Ok(tile_type
            .segbits
            .ppips()
            .iter()
            .map(|(feature, kind)| (feature.to_string(), kind.name()))
            .collect())
    }

    /// Every tile of the part's grid as a ``Tile(name, tile_type, grid_x,
    /// grid_y)`` namedtuple, in ``tilegrid.json`` order (empty without a
    /// part).
    fn tiles<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let class = XTypes::get(py)?.tile.bind(py);
        let tiles = self.db.grid().map_or(&[][..], |grid| grid.tiles());
        let items = tiles
            .iter()
            .map(|t| {
                class.call1((
                    t.name.to_string(),
                    t.tile_type.to_string(),
                    t.grid_x,
                    t.grid_y,
                ))
            })
            .collect::<PyResult<Vec<_>>>()?;
        PyList::new(py, items)
    }

    /// The lines of the part's ``required_features.fasm`` (stripped,
    /// without blank lines and duplicates, in file order).
    fn required_features(&self) -> Vec<String> {
        self.db
            .part_info()
            .map(|info| info.required_features.clone())
            .unwrap_or_default()
    }

    /// Every configuration frame address of the part (``part.yaml`` /
    /// ``part.json`` frame tree), in bitstream order, or ``None`` if the
    /// part has no frame tree.
    fn frame_addresses(&self) -> Option<Vec<u32>> {
        self.db
            .part()
            .map(|part| part.iter_frame_addresses().map(|a| a.0).collect())
    }

    /// Looks up bit ``address`` of the FASM feature ``feature``
    /// (``TILE.SITE.FEATURE``; ``address`` is the ``N`` of
    /// ``FEATURE[N]``, 0 for a feature without one), like the assembler.
    ///
    /// Returns a ``FeatureBits`` namedtuple (see its documentation; a
    /// pseudo PIP has an empty ``bits``). Raises ``FasmKeyError`` for an
    /// unknown tile or tile type, ``FasmLookupError`` for a feature that
    /// is not in the tile's segbits (``str()`` is ``Segment DB <type>, key
    /// <type>.<feature> not found``), ``Error`` for a database opened
    /// without a part.
    #[pyo3(signature = (feature, address=0))]
    fn lookup_feature<'py>(
        &self,
        py: Python<'py>,
        feature: &str,
        address: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let (tile, rest) = feature.split_once('.').unwrap_or((feature, ""));
        // A name the interner does not know is in no table: interning it
        // keeps the error precedence (tile, type, feature).
        let intern = |s: &str| IdString::lookup(s).unwrap_or_else(|| IdString::new(s));
        let (tile, rest) = (intern(tile), intern(rest));
        let found = self
            .db
            .lookup_feature(tile, rest, address)
            .map_err(|e| lookup_error(py, &e))?;
        let class = XTypes::get(py)?.feature_bits.bind(py);
        match found {
            FeatureLookup::PseudoPip(kind) => {
                let tile_type = self
                    .db
                    .grid()
                    .and_then(|grid| grid.tile(tile))
                    .map(|t| t.tile_type.to_string());
                class.call1((
                    tile.to_string(),
                    tile_type.clone(),
                    tile_type,
                    kind.name(),
                    py.None(),
                    py.None(),
                    py.None(),
                    py.None(),
                    PyTuple::empty(py),
                ))
            }
            FeatureLookup::Bits(bits) => {
                let positions: Vec<(u32, u32, u32, bool)> = bits
                    .positions()
                    .filter_map(|(segbit, position)| {
                        position
                            .ok()
                            .map(|p| (p.frame.0, p.word, p.bit, segbit.is_set))
                    })
                    .collect();
                class.call1((
                    bits.tile.name.to_string(),
                    bits.tile.tile_type.to_string(),
                    bits.segbits_type.name.to_string(),
                    py.None(),
                    bits.block_type().name(),
                    bits.block.base_address,
                    bits.block.frames,
                    bits.offset,
                    PyTuple::new(py, positions)?,
                ))
            }
        }
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let root = self.root().into_pyobject(py)?.repr()?.to_string();
        let part = self.part().into_pyobject(py)?.repr()?.to_string();
        Ok(format!(
            "<fasm.xilinx.Database {root} part={part} ({})>",
            self.db.architecture()
        ))
    }
}

impl PyDatabase {
    fn find_tile_type(&self, name: &str) -> PyResult<&fasm_xilinx::TileType> {
        IdString::lookup(name)
            .and_then(|id| self.db.tile_type(id))
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(name.to_owned()))
    }
}

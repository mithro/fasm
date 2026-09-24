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

//! Errors of the database loader.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// An error while opening or parsing a prjxray-db / prjuray-db database.
///
/// Every variant names the file involved; parse errors also carry the
/// 1-based line (and, for JSON, column) of the problem.
#[derive(Debug)]
#[non_exhaustive]
pub enum DbError {
    /// A file the loader needs does not exist.
    MissingFile {
        /// The missing file.
        path: PathBuf,
    },
    /// Reading a file (or listing a directory) failed.
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// A JSON file is not valid JSON or does not have the expected shape.
    Json {
        /// The file.
        path: PathBuf,
        /// 1-based line (0 if unknown).
        line: usize,
        /// 1-based column (0 if unknown).
        column: usize,
        /// What is wrong.
        message: String,
    },
    /// A YAML file (`part.yaml`, `mapping/*.yaml`) is malformed or uses
    /// YAML the loader does not support.
    Yaml {
        /// The file.
        path: PathBuf,
        /// 1-based line (0 if unknown).
        line: usize,
        /// What is wrong.
        message: String,
    },
    /// A CSV file (`package_pins.csv`) is malformed.
    Csv {
        /// The file.
        path: PathBuf,
        /// 1-based line.
        line: usize,
        /// What is wrong.
        message: String,
    },
    /// A malformed line in a `segbits_*.db` or `ppips_*.db` file.
    Segbits {
        /// The file.
        path: PathBuf,
        /// 1-based line.
        line: usize,
        /// What is wrong.
        message: String,
    },
    /// The part is not listed in the database (`mapping/parts.yaml` for
    /// prjxray-db, no `<part>/` directory for prjuray-db).
    UnknownPart {
        /// The requested part.
        part: String,
        /// The file (or directory) that was searched.
        path: PathBuf,
    },
    /// The device of a part is not listed in `mapping/devices.yaml`.
    UnknownDevice {
        /// The device named by `mapping/parts.yaml`.
        device: String,
        /// `mapping/devices.yaml`.
        path: PathBuf,
    },
    /// The directory is neither a prjxray-db family (`mapping/`
    /// directory) nor a prjuray-db family (`tile_types/` directory).
    UnknownLayout {
        /// The database root that was given.
        root: PathBuf,
    },
    /// A well formed file whose content is inconsistent (for example a
    /// tilegrid tile with an unknown bus name, two tiles at the same grid
    /// location or a part frame tree that does not fit the frame address
    /// bit fields).
    Invalid {
        /// The file.
        path: PathBuf,
        /// What is wrong.
        message: String,
    },
}

impl DbError {
    /// The error for a failed file read: [`DbError::MissingFile`] when the
    /// file does not exist, [`DbError::Io`] otherwise.
    pub(crate) fn from_io(path: &Path, source: io::Error) -> Self {
        if source.kind() == io::ErrorKind::NotFound {
            DbError::MissingFile {
                path: path.to_path_buf(),
            }
        } else {
            DbError::Io {
                path: path.to_path_buf(),
                source,
            }
        }
    }

    pub(crate) fn json(path: &Path, err: &serde_json::Error) -> Self {
        DbError::Json {
            path: path.to_path_buf(),
            line: err.line(),
            column: err.column(),
            message: err.to_string(),
        }
    }

    pub(crate) fn invalid(path: &Path, message: impl Into<String>) -> Self {
        DbError::Invalid {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }

    /// The file (or directory) the error is about.
    pub fn path(&self) -> &Path {
        match self {
            DbError::MissingFile { path }
            | DbError::Io { path, .. }
            | DbError::Json { path, .. }
            | DbError::Yaml { path, .. }
            | DbError::Csv { path, .. }
            | DbError::Segbits { path, .. }
            | DbError::UnknownPart { path, .. }
            | DbError::UnknownDevice { path, .. }
            | DbError::Invalid { path, .. } => path,
            DbError::UnknownLayout { root } => root,
        }
    }
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::MissingFile { path } => write!(f, "{}: file not found", path.display()),
            DbError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            DbError::Json {
                path,
                line,
                column,
                message,
            } => write!(f, "{}:{line}:{column}: {message}", path.display()),
            DbError::Yaml {
                path,
                line,
                message,
            }
            | DbError::Csv {
                path,
                line,
                message,
            }
            | DbError::Segbits {
                path,
                line,
                message,
            } => write!(f, "{}:{line}: {message}", path.display()),
            DbError::UnknownPart { part, path } => {
                write!(f, "{}: part {part:?} not found", path.display())
            }
            DbError::UnknownDevice { device, path } => {
                write!(f, "{}: device {device:?} not found", path.display())
            }
            DbError::UnknownLayout { root } => write!(
                f,
                "{}: not a prjxray-db (mapping/) or prjuray-db (tile_types/) family directory",
                root.display()
            ),
            DbError::Invalid { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for DbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DbError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Reads a whole file, mapping errors to [`DbError`].
pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>, DbError> {
    std::fs::read(path).map_err(|e| DbError::from_io(path, e))
}

/// Reads a whole UTF-8 text file, mapping errors to [`DbError`].
pub(crate) fn read_text(path: &Path) -> Result<String, DbError> {
    let bytes = read_file(path)?;
    String::from_utf8(bytes).map_err(|e| DbError::invalid(path, format!("not UTF-8: {e}")))
}

/// Returns `true` if `path` is an existing regular file (following
/// symlinks), like Python's `os.path.isfile`.
pub(crate) fn is_file(path: &Path) -> bool {
    path.is_file()
}

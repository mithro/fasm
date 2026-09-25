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

//! [`FasmAssembler`]: FASM features -> configuration frames, a port of
//! `prjxray.fasm_assembler.FasmAssembler` (`prjxray/fasm_assembler.py`).

use std::collections::hash_map::Entry;
use std::fmt;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fasm::idstring::IdString;
use fasm::{FasmLine, ParseError, SetFasmFeature};
use foldhash::{HashMap, HashMapExt, HashSet, HashSetExt};

use crate::arch::Architecture;
use crate::db::{Database, FeatureLookup, LookupError};
use crate::error::DbError;
use crate::frames::Frames;
use crate::tilegrid::Grid;

/// The error of an assembler run, modelled on the exception the reference
/// Python tools raise (see [`AssemblerError::python_exception`]); the
/// [`fmt::Display`] text is the `str()` of that exception.
#[derive(Debug)]
#[non_exhaustive]
pub enum AssemblerError {
    /// A FASM syntax error (the ANTLR parser raises
    /// `Exception('Parse error at L:C - message')`).
    Parse(ParseError),
    /// The FASM file cannot be read (`Exception: Parse error at 0:0 -
    /// Couldn't open file`).
    OpenFasm {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// `prjxray.fasm_assembler.FasmLookupError`: features that are not in
    /// the database, one message per enabled bit, in order (batched over
    /// the whole file like `FasmAssembler.parse_fasm_filename`).
    Lookup(Vec<String>),
    /// `prjxray.fasm_assembler.FasmInconsistentBits`: two lines want a
    /// different value for one bit (raised at the first conflict).
    InconsistentBits(String),
    /// A Python `KeyError`: an unknown tile or tile type (prjxray looks
    /// them up outside of the `try` that makes `FasmLookupError`s), a
    /// missing key of the ROI JSON, a STEPDOWN tile without IO bank, ...
    /// The value is the missing key.
    KeyError(String),
    /// A file the reference opens with `open()` cannot be opened or read
    /// (`FileNotFoundError`, `PermissionError`, `IsADirectoryError`, ...).
    Io {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// The ROI `design.json` is not valid JSON
    /// (`json.decoder.JSONDecodeError`; the message is serde_json's).
    Json {
        /// The file.
        path: PathBuf,
        /// What is wrong.
        message: String,
    },
    /// Opening the database failed (the reference fails with various
    /// exceptions, mostly `AssertionError`, `FileNotFoundError` and
    /// `KeyError`).
    Db(DbError),
    /// `base_address + word_column` of a segbit does not fit in 32 bits
    /// (impossible with a valid database; Python would write a frame
    /// address wider than 8 hex digits).
    FrameAddressOverflow {
        /// The tile.
        tile: IdString,
        /// The feature within the tile.
        feature: String,
    },
    /// Any other exception the reference raises (`AssertionError`,
    /// `IndexError`, `ValueError`, `TypeError`), with its `str()`.
    Python {
        /// The exception type, e.g. `IndexError`.
        exception: &'static str,
        /// The message.
        message: String,
    },
}

impl AssemblerError {
    /// The (qualified) name of the Python exception the reference tool
    /// raises in the same situation, as printed on the last line of its
    /// traceback (`prjxray.fasm_assembler.FasmLookupError`, `KeyError`,
    /// `FileNotFoundError`, ...). For errors the reference reports with
    /// many different exceptions ([`AssemblerError::Db`]) this is
    /// `fasm_xilinx.DbError`.
    pub fn python_exception(&self) -> &'static str {
        match self {
            AssemblerError::Parse(_) | AssemblerError::OpenFasm { .. } => "Exception",
            AssemblerError::Lookup(_) => "prjxray.fasm_assembler.FasmLookupError",
            AssemblerError::InconsistentBits(_) => "prjxray.fasm_assembler.FasmInconsistentBits",
            AssemblerError::KeyError(_) => "KeyError",
            AssemblerError::Io { source, .. } => os_error_name(source),
            AssemblerError::Json { .. } => "json.decoder.JSONDecodeError",
            AssemblerError::Db(_) => "fasm_xilinx.DbError",
            AssemblerError::FrameAddressOverflow { .. } => "OverflowError",
            AssemblerError::Python { exception, .. } => exception,
        }
    }

    /// `python_exception(): message`, the last line(s) of the reference
    /// tool's traceback.
    pub fn traceback_line(&self) -> String {
        format!("{}: {self}", self.python_exception())
    }
}

impl fmt::Display for AssemblerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssemblerError::Parse(e) => write!(f, "{e}"),
            AssemblerError::OpenFasm { .. } => {
                f.write_str("Parse error at 0:0 - Couldn't open file")
            }
            AssemblerError::Lookup(messages) => f.write_str(&messages.join("\n")),
            AssemblerError::InconsistentBits(message) => f.write_str(message),
            AssemblerError::KeyError(key) => f.write_str(&py_repr(key)),
            AssemblerError::Io { path, source } => {
                let text = source.to_string();
                // Rust appends " (os error N)"; Python prints
                // "[Errno N] <strerror>: '<path>'".
                let strerror = text
                    .rfind(" (os error ")
                    .map_or(text.as_str(), |i| &text[..i]);
                match source.raw_os_error() {
                    Some(errno) => write!(
                        f,
                        "[Errno {errno}] {strerror}: {}",
                        py_repr(&path.to_string_lossy())
                    ),
                    None => write!(f, "{strerror}: {}", py_repr(&path.to_string_lossy())),
                }
            }
            AssemblerError::Json { path, message } => {
                write!(f, "{}: {message}", path.display())
            }
            AssemblerError::Db(e) => write!(f, "{e}"),
            AssemblerError::FrameAddressOverflow { tile, feature } => write!(
                f,
                "frame address of {tile}.{feature} does not fit in 32 bits"
            ),
            AssemblerError::Python { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for AssemblerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AssemblerError::Parse(e) => Some(e),
            AssemblerError::OpenFasm { source, .. } | AssemblerError::Io { source, .. } => {
                Some(source)
            }
            AssemblerError::Db(e) => Some(e),
            _ => None,
        }
    }
}

impl From<DbError> for AssemblerError {
    fn from(e: DbError) -> Self {
        AssemblerError::Db(e)
    }
}

impl From<ParseError> for AssemblerError {
    fn from(e: ParseError) -> Self {
        AssemblerError::Parse(e)
    }
}

/// The Python `OSError` subclass for an I/O error.
fn os_error_name(error: &io::Error) -> &'static str {
    match error.raw_os_error() {
        Some(2) => "FileNotFoundError",
        Some(1 | 13) => "PermissionError",
        Some(21) => "IsADirectoryError",
        Some(20) => "NotADirectoryError",
        Some(17) => "FileExistsError",
        _ => match error.kind() {
            io::ErrorKind::NotFound => "FileNotFoundError",
            io::ErrorKind::PermissionDenied => "PermissionError",
            _ => "OSError",
        },
    }
}

/// Python's `repr()` of a `str`.
pub(crate) fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || (0x7f..0xa0).contains(&(c as u32)) => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// A callback run on every `SetFasmFeature` the assembler sees, before
/// its bits are looked up (`FasmAssembler.feature_callback`).
pub type FeatureCallback<'a> = Box<dyn FnMut(&SetFasmFeature) -> Result<(), AssemblerError> + 'a>;

/// Bias of the word field of a packed bit key: prjxray keys its bits by
/// the *unwrapped* word (`absolute bit // 32`, negative for the `_SING`
/// alias tiles, see [`FasmAssembler`]).
const WORD_BIAS: i64 = 1 << 26;

fn pack_key(frame: u32, word: i64, bit: u32) -> u64 {
    // Words outside [-2^26, 2^26) cannot be written (they are either
    // dropped, `>= words_per_frame`, or an `IndexError` in `get_frames`),
    // clamping keeps them distinct from every writable word.
    let word = word.clamp(-WORD_BIAS, WORD_BIAS - 1) + WORD_BIAS;
    (u64::from(frame) << 32) | ((word as u64) << 5) | u64::from(bit & 31)
}

fn unpack_key(key: u64) -> (u32, i64, u32) {
    let frame = (key >> 32) as u32;
    let word = ((key >> 5) & ((1 << 27) - 1)) as i64 - WORD_BIAS;
    (frame, word, (key & 31) as u32)
}

/// Assembles FASM features into configuration frames, exactly like
/// `prjxray.fasm_assembler.FasmAssembler` (design document §5):
///
/// * [`FasmAssembler::add_fasm_line`] splits the feature at the first `.`
///   into tile and feature, runs the feature callback, and enables every
///   set bit of the value (`canonical_features`: a value of 0 does
///   nothing at all, not even a lookup); each enabled bit is looked up
///   with [`Database::lookup_feature`];
/// * the bits of a feature are set (or, for `!` segbits, cleared); two
///   lines that want a different value for one bit are a
///   [`AssemblerError::InconsistentBits`] error (raised immediately), and
///   writing the same value again is fine. Like prjxray, bits are keyed by
///   `(frame, absolute_bit // 32, absolute_bit % 32)` *before* the
///   negative word of the `_SING` alias tiles wraps (Python list index
///   `-2` is word 99), so a wrapped bit never conflicts with a direct one;
/// * a bit whose word is beyond the frame is dropped with the warning
///   `frame_set: invalid word address <word> in line: <line>` (or
///   `frame_clear`), see [`FasmAssembler::warnings`];
/// * features not in the database are collected and reported together by
///   [`FasmAssembler::parse_fasm_filename`] as an
///   [`AssemblerError::Lookup`]; an unknown tile or tile type is an
///   immediate [`AssemblerError::KeyError`] (prjxray looks those up before
///   its `try`);
/// * pseudo PIPs set nothing; every other feature marks all frames of
///   its bus of the tile "in use" (for sparse output);
/// * [`FasmAssembler::get_frames`] returns every frame of the grid
///   (dense) or only the frames in use (sparse), plus the frame of every
///   bit that was written, zero filled, with the set bits applied.
///
/// The assembler borrows its database ([`FasmAssembler::new`]) or shares
/// the ownership of it ([`FasmAssembler::new_shared`], a
/// `FasmAssembler<'static>` for bindings that cannot express the borrow,
/// such as the Python and C APIs).
pub struct FasmAssembler<'db> {
    core: Core<'db>,
    /// Every line given to the assembler, for error messages: bits refer
    /// to their line by index.
    lines: Vec<FasmLine>,
    callback: Option<FeatureCallback<'db>>,
}

/// The state of [`FasmAssembler`] other than the lines and the callback
/// (so that a line of `lines` can be processed while the state changes).
struct Core<'db> {
    db: DbRef<'db>,
    architecture: Architecture,
    words_per_frame: usize,
    /// Packed `(frame, unwrapped word, bit)` -> `line index << 1 | is_set`.
    bits: HashMap<u64, u32>,
    /// `(base_address, frames)` of the bus blocks in use.
    in_use: HashSet<(u32, u32)>,
    last_in_use: Option<(u32, u32)>,
    warnings: Vec<String>,
    /// prjuray's `utils/fasm_assembler.py` semantics
    /// ([`FasmAssembler::set_prjuray`]).
    prjuray: bool,
}

impl fmt::Debug for FasmAssembler<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FasmAssembler")
            .field("architecture", &self.core.architecture)
            .field("bits", &self.core.bits.len())
            .field("lines", &self.lines.len())
            .finish_non_exhaustive()
    }
}

/// The text of line `index` (`fasm.fasm_line_to_string(line)`).
fn line_str(lines: &[FasmLine], index: usize) -> String {
    lines
        .get(index)
        .and_then(|line| fasm::fasm_line_to_string(line, false).ok())
        .and_then(|mut v| v.pop())
        .unwrap_or_default()
}

fn assertion_error() -> AssemblerError {
    AssemblerError::Python {
        exception: "AssertionError",
        message: String::new(),
    }
}

/// Splits a FASM feature at the first `.` into the tile and the feature
/// within the tile, like `FasmAssembler.add_fasm_line`.
fn split_feature(feature: IdString) -> (IdString, IdString) {
    feature.with_str(|s| {
        let (tile, rest) = s.split_once('.').unwrap_or((s, ""));
        // A name the interner does not know is in no table; interning it
        // keeps the error precedence (tile, type, feature).
        let tile = IdString::lookup(tile).unwrap_or_else(|| IdString::new(tile));
        let rest = IdString::lookup(rest).unwrap_or_else(|| IdString::new(rest));
        (tile, rest)
    })
}

impl<'db> FasmAssembler<'db> {
    /// An assembler for the part `db` was opened for.
    ///
    /// # Errors
    ///
    /// [`AssemblerError::Python`] (`AttributeError`) if the database was
    /// opened without a part (it has no grid).
    pub fn new(db: &'db Database) -> Result<Self, AssemblerError> {
        Self::with_db(DbRef::Borrowed(db))
    }

    fn with_db(db: DbRef<'db>) -> Result<Self, AssemblerError> {
        if db.grid().is_none() {
            return Err(AssemblerError::Python {
                exception: "AttributeError",
                message: "the database was opened without a part".to_owned(),
            });
        }
        let architecture = db.architecture();
        Ok(FasmAssembler {
            core: Core {
                db,
                architecture,
                words_per_frame: architecture.words_per_frame(),
                bits: HashMap::new(),
                in_use: HashSet::new(),
                last_in_use: None,
                warnings: Vec::new(),
                prjuray: false,
            },
            lines: Vec::new(),
            callback: None,
        })
    }

    /// Switches to the semantics of prjuray's `utils/fasm_assembler.py`
    /// (a copy of prjxray's `fasm_assembler.py` before prjxray added its
    /// word check, working in 16-bit words):
    ///
    /// * a bit beyond the end of the frame is not dropped with a warning:
    ///   it is kept like any other bit (it takes part in the conflict
    ///   checks and its frame is output), and [`FasmAssembler::get_frames`]
    ///   fails with Python's `IndexError: list index out of range` if it
    ///   is set;
    /// * the bit of a [`AssemblerError::InconsistentBits`] message is
    ///   `(frame, 16-bit word, bit)`.
    pub fn set_prjuray(&mut self, prjuray: bool) {
        self.core.prjuray = prjuray;
    }

    /// The database.
    pub fn database(&self) -> &Database {
        &self.core.db
    }

    /// Sets the callback run on every feature
    /// (`FasmAssembler.set_feature_callback`); an error it returns aborts
    /// [`FasmAssembler::add_fasm_line`].
    pub fn set_feature_callback(&mut self, callback: FeatureCallback<'db>) {
        self.callback = Some(callback);
    }

    /// The warnings printed so far by the reference (dropped bits beyond
    /// the end of a frame), in order.
    pub fn warnings(&self) -> &[String] {
        &self.core.warnings
    }

    /// Removes and returns the warnings.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.core.warnings)
    }

    /// Every line given to the assembler so far, in order (the
    /// reference's `set_features` are their `set_feature`s).
    pub fn lines(&self) -> &[FasmLine] {
        &self.lines
    }

    /// `FasmAssembler.parse_fasm_filename`: parses the whole file (a
    /// syntax error is reported before anything is assembled), adds its
    /// lines then the `extra_features`, and reports the features not
    /// found in the database, if any, as one [`AssemblerError::Lookup`].
    ///
    /// # Errors
    ///
    /// [`AssemblerError::OpenFasm`], [`AssemblerError::Parse`], the
    /// errors of [`FasmAssembler::add_fasm_line`] and
    /// [`AssemblerError::Lookup`].
    pub fn parse_fasm_filename(
        &mut self,
        path: &Path,
        extra_features: Vec<FasmLine>,
    ) -> Result<(), AssemblerError> {
        let data = std::fs::read(path).map_err(|source| AssemblerError::OpenFasm {
            path: path.to_path_buf(),
            source,
        })?;
        self.parse_fasm_bytes(&data, extra_features)
    }

    /// [`FasmAssembler::parse_fasm_filename`] for the contents of a FASM
    /// file.
    ///
    /// # Errors
    ///
    /// See [`FasmAssembler::parse_fasm_filename`].
    pub fn parse_fasm_bytes(
        &mut self,
        data: &[u8],
        extra_features: Vec<FasmLine>,
    ) -> Result<(), AssemblerError> {
        let mut lines = fasm::parse_fasm_bytes(data)?;
        lines.extend(extra_features);
        let first = self.lines.len();
        if self.lines.is_empty() {
            self.lines = lines;
        } else {
            self.lines.append(&mut lines);
        }
        self.core.bits.reserve(self.lines.len() - first);
        let mut missing = Vec::new();
        for index in first..self.lines.len() {
            self.process_line(index, &mut missing)?;
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(AssemblerError::Lookup(missing))
        }
    }

    /// `FasmAssembler.add_fasm_line`: runs the feature callback and
    /// enables every set bit of the line's feature. The message of a
    /// feature (or `feature[address]`) that is not in the database is
    /// appended to `missing_features`.
    ///
    /// # Errors
    ///
    /// The callback's error, [`AssemblerError::KeyError`] for an unknown
    /// tile or tile type, [`AssemblerError::InconsistentBits`], and
    /// [`AssemblerError::Python`] (`AssertionError`) for a malformed
    /// `SetFasmFeature` (a single bit address with a value other than 1,
    /// only possible with `SetFasmFeature::new_unchecked`).
    pub fn add_fasm_line(
        &mut self,
        line: FasmLine,
        missing_features: &mut Vec<String>,
    ) -> Result<(), AssemblerError> {
        self.lines.push(line);
        self.process_line(self.lines.len() - 1, missing_features)
    }

    /// `add_fasm_line` for `self.lines[index]`.
    fn process_line(
        &mut self,
        index: usize,
        missing_features: &mut Vec<String>,
    ) -> Result<(), AssemblerError> {
        let Some(set_feature) = &self.lines[index].set_feature else {
            return Ok(());
        };
        if let Some(callback) = &mut self.callback {
            callback(set_feature)?;
        }
        // `canonical_features`: the addresses of the set bits (a value of
        // 0 yields nothing; without a range the value must be 1).
        let value = &set_feature.value;
        if value.is_zero() {
            return Ok(());
        }
        let (tile, feature) = split_feature(set_feature.feature);
        let lines = &self.lines;
        let mut enable = |address: u32| -> Result<(), AssemblerError> {
            match self
                .core
                .enable_feature(lines, tile, feature, address, index)
            {
                Err(AssemblerError::Lookup(mut messages)) => {
                    missing_features.append(&mut messages);
                    Ok(())
                }
                other => other,
            }
        };
        match (set_feature.start, set_feature.end) {
            (None, Some(_)) => Err(assertion_error()),
            (start, None) => {
                if value.is_one() {
                    enable(start.unwrap_or(0))
                } else {
                    Err(assertion_error())
                }
            }
            (Some(start), Some(end)) => {
                let span = end.checked_sub(start).ok_or_else(assertion_error)?;
                for bit in value.iter_set_bits().take_while(|&i| i <= span) {
                    enable(start + bit)?;
                }
                Ok(())
            }
        }
    }

    /// `FasmAssembler.mark_roi_frames`: marks every frame of every bus of
    /// the tiles inside `roi` in use.
    pub fn mark_roi_frames(&mut self, roi: &Roi) {
        let core = &mut self.core;
        let grid = core.db.grid().expect("checked by FasmAssembler::new");
        for tile in grid.tiles() {
            if roi.contains(tile.grid_x, tile.grid_y) {
                for block in grid.bits(tile) {
                    mark_in_use(
                        &mut core.in_use,
                        &mut core.last_in_use,
                        block.base_address,
                        block.frames,
                    );
                }
            }
        }
    }

    /// `FasmAssembler.get_frames(sparse)`.
    ///
    /// # Errors
    ///
    /// [`AssemblerError::Python`] (`IndexError`) for a set bit whose word
    /// is before the start of the frame even after the Python style
    /// wrap-around (impossible with a valid database).
    pub fn get_frames(&self, sparse: bool) -> Result<Frames, AssemblerError> {
        let core = &self.core;
        let blocks: HashSet<(u32, u32)> = if sparse {
            core.in_use.clone()
        } else {
            core.grid()
                .iter_bits()
                .map(|(_, b)| (b.base_address, b.frames))
                .collect()
        };
        let mut addresses: Vec<u32> = Vec::new();
        for (base, count) in blocks {
            addresses.extend(base..base.saturating_add(count));
        }
        addresses.sort_unstable();
        addresses.dedup();
        // Frames of bits outside of the blocks (a segbit beyond its
        // tile's frames): `init_frame_at_address` in the bit loop.
        let mut extra: Vec<u32> = core
            .bits
            .keys()
            .map(|&key| unpack_key(key).0)
            .filter(|frame| addresses.binary_search(frame).is_err())
            .collect();
        if !extra.is_empty() {
            addresses.append(&mut extra);
        }
        let mut frames = Frames::zeroed(core.words_per_frame, addresses);
        let words_per_frame = core.words_per_frame as i64;
        for (&key, &state) in &core.bits {
            if state & 1 == 0 {
                continue;
            }
            let (frame, mut word, bit) = unpack_key(key);
            if word < 0 {
                word += words_per_frame;
            }
            let words = frames
                .get_mut(frame)
                .expect("the frame of every bit was added");
            match usize::try_from(word).ok().and_then(|w| words.get_mut(w)) {
                Some(w) => *w |= 1 << bit,
                None => {
                    return Err(AssemblerError::Python {
                        exception: "IndexError",
                        message: "list index out of range".to_owned(),
                    })
                }
            }
        }
        Ok(frames)
    }
}

impl Core<'_> {
    /// `FasmAssembler.enable_feature` for one bit address of line
    /// `index` of `lines`.
    fn enable_feature(
        &mut self,
        lines: &[FasmLine],
        tile: IdString,
        feature: IdString,
        address: u32,
        index: usize,
    ) -> Result<(), AssemblerError> {
        let bits = match self.db.lookup_feature(tile, feature, address) {
            Ok(FeatureLookup::PseudoPip(_)) => return Ok(()),
            Ok(FeatureLookup::Bits(bits)) => bits,
            Err(LookupError::UnknownTile { tile }) => {
                return Err(AssemblerError::KeyError(tile.to_string()))
            }
            Err(LookupError::UnknownTileType { tile_type, .. }) => {
                return Err(AssemblerError::KeyError(
                    tile_type.to_string().to_ascii_uppercase(),
                ))
            }
            Err(LookupError::UnknownFeature { .. } | LookupError::MissingBitsBlock { .. }) => {
                let tile_type = self
                    .grid()
                    .tile(tile)
                    .map_or_else(String::new, |t| t.tile_type.to_string());
                return Err(AssemblerError::Lookup(vec![format!(
                    "Segment DB {tile_type}, key {tile_type}.{feature} not found from line '{}'",
                    line_str(lines, index)
                )]));
            }
            Err(e @ (LookupError::InconsistentAlias { .. } | LookupError::NoGrid)) => {
                return Err(AssemblerError::Python {
                    exception: "AssertionError",
                    message: e.to_string(),
                })
            }
        };
        let base = bits.block.base_address;
        let frames = bits.block.frames;
        let offset = bits.offset;
        let words_per_frame = self.words_per_frame as i64;
        let state_of_line = u32::try_from(index)
            .ok()
            .and_then(|i| i.checked_mul(2))
            .unwrap_or(!1);
        // The line text of the warnings, rendered once per call.
        let mut rendered: Option<String> = None;
        for &segbit in bits.bits {
            let frame = base.checked_add(segbit.word_column).ok_or_else(|| {
                AssemblerError::FrameAddressOverflow {
                    tile,
                    feature: feature.to_string(),
                }
            })?;
            let absolute = self.architecture.segbit_absolute_bit(offset, segbit);
            let word = absolute.div_euclid(32);
            let bit = absolute.rem_euclid(32) as u32;
            if word >= words_per_frame && !self.prjuray {
                let function = if segbit.is_set {
                    "frame_set"
                } else {
                    "frame_clear"
                };
                let line = rendered.get_or_insert_with(|| line_str(lines, index));
                let warning = format!("{function}: invalid word address {word} in line: {line}");
                self.warnings.push(warning);
                continue;
            }
            let state = state_of_line | u32::from(segbit.is_set);
            match self.bits.entry(pack_key(frame, word, bit)) {
                Entry::Vacant(e) => {
                    e.insert(state);
                }
                Entry::Occupied(e) => {
                    let previous = *e.get();
                    if previous & 1 != state & 1 {
                        let (wanted, was) = if segbit.is_set {
                            ("set", "cleared")
                        } else {
                            ("clear", "set")
                        };
                        let (word, bit) = if self.prjuray {
                            (absolute.div_euclid(16), absolute.rem_euclid(16) as u32)
                        } else {
                            (word, bit)
                        };
                        return Err(AssemblerError::InconsistentBits(format!(
                            "FASM line \"{}\" wanted to {wanted} bit ({frame}, {word}, {bit}) \
                             but was {was} by FASM line \"{}\"",
                            line_str(lines, index),
                            line_str(lines, (previous >> 1) as usize)
                        )));
                    }
                }
            }
        }
        // `any_bits`: the bus is in use if the feature has any bit (even
        // one dropped beyond the frame).
        if !bits.bits.is_empty() {
            self.mark_in_use(base, frames);
        }
        Ok(())
    }

    fn mark_in_use(&mut self, base: u32, frames: u32) {
        mark_in_use(&mut self.in_use, &mut self.last_in_use, base, frames);
    }

    fn grid(&self) -> &Grid {
        self.db.grid().expect("checked by FasmAssembler::new")
    }
}

/// Marks the bus block `(base, frames)` in use.
fn mark_in_use(
    in_use: &mut HashSet<(u32, u32)>,
    last_in_use: &mut Option<(u32, u32)>,
    base: u32,
    frames: u32,
) {
    if *last_in_use != Some((base, frames)) {
        in_use.insert((base, frames));
        *last_in_use = Some((base, frames));
    }
}

/// The database of an assembler: borrowed, or shared.
#[derive(Clone)]
enum DbRef<'db> {
    Borrowed(&'db Database),
    Shared(Arc<Database>),
}

impl Deref for DbRef<'_> {
    type Target = Database;

    fn deref(&self) -> &Database {
        match self {
            DbRef::Borrowed(db) => db,
            DbRef::Shared(db) => db,
        }
    }
}

impl FasmAssembler<'static> {
    /// [`FasmAssembler::new`] for a database whose ownership the assembler
    /// shares: the assembler does not borrow anything, so it can live in
    /// an object of a language binding next to other users of the same
    /// database.
    ///
    /// # Errors
    ///
    /// See [`FasmAssembler::new`].
    pub fn new_shared(db: Arc<Database>) -> Result<Self, AssemblerError> {
        Self::with_db(DbRef::Shared(db))
    }
}

/// A region of interest: a rectangle of grid coordinates, both ends
/// included (`prjxray.roi.Roi`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roi {
    /// Smallest grid X.
    pub x1: f64,
    /// Largest grid X.
    pub x2: f64,
    /// Smallest grid Y.
    pub y1: f64,
    /// Largest grid Y.
    pub y2: f64,
}

impl Roi {
    /// `Roi.tile_in_roi`.
    pub fn contains(&self, grid_x: i32, grid_y: i32) -> bool {
        let (x, y) = (f64::from(grid_x), f64::from(grid_y));
        self.x1 <= x && x <= self.x2 && self.y1 <= y && y <= self.y2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_packing_round_trips() {
        for &(frame, word, bit) in &[
            (0u32, 0i64, 0u32),
            (u32::MAX, 100, 31),
            (0x0040_0000, -2, 3),
            (7, -101, 17),
            (7, WORD_BIAS - 1, 1),
            (7, -WORD_BIAS, 1),
        ] {
            assert_eq!(unpack_key(pack_key(frame, word, bit)), (frame, word, bit));
        }
        assert_ne!(pack_key(1, -2, 3), pack_key(1, 99, 3));
    }

    #[test]
    fn python_repr() {
        assert_eq!(py_repr("abc"), "'abc'");
        assert_eq!(py_repr("it's"), "\"it's\"");
        assert_eq!(py_repr("a'\"b"), "'a\\'\"b'");
        assert_eq!(py_repr("a\\b\n\x01é"), "'a\\\\b\\n\\x01é'");
    }

    #[test]
    fn roi_bounds_are_inclusive() {
        let roi = Roi {
            x1: 1.0,
            x2: 3.0,
            y1: 5.0,
            y2: 5.0,
        };
        assert!(roi.contains(1, 5) && roi.contains(3, 5));
        assert!(!roi.contains(0, 5) && !roi.contains(2, 4) && !roi.contains(4, 5));
    }
}

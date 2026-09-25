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

//! [`fasm_xilinx_database`].

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use fasm::idstring::IdString;
use fasm_xilinx::cache::CacheOptions;
use fasm_xilinx::{AssemblerError, BlockType, Database, FeatureLookup, LookupError};

use super::{db_error, fasm_xilinx_architecture, path_arg, set_out};
use crate::error::{fasm_error, fasm_status, CapiError};
use crate::ffi::{bytes, guard, run_status};

/// A prjxray-db (Series7) or prjuray-db (UltraScale+) database family
/// opened for one part (Python: `fasm.xilinx.Database`).
///
/// Created by `fasm_xilinx_database_open` / `_open_cached`, released with
/// `fasm_xilinx_database_free`. Immutable: any number of threads may use
/// it at once. Assemblers and parts made from it share its ownership, so
/// it may be freed before them.
pub struct fasm_xilinx_database {
    _private: [u8; 0],
}

/// The Rust side of a [`fasm_xilinx_database`].
pub(crate) struct DatabaseInner {
    pub(crate) db: Arc<Database>,
    part: Option<CString>,
}

impl DatabaseInner {
    fn into_raw(db: Database) -> *mut fasm_xilinx_database {
        let part = db
            .part_info()
            .and_then(|info| CString::new(info.name.clone()).ok());
        Box::into_raw(Box::new(DatabaseInner {
            db: Arc::new(db),
            part,
        }))
        .cast::<fasm_xilinx_database>()
    }
}

/// Borrows the database behind `db`.
///
/// # Safety
///
/// `db` must be `NULL` or a live `fasm_xilinx_database`.
pub(crate) unsafe fn database_ref<'a>(
    db: *const fasm_xilinx_database,
) -> Option<&'a DatabaseInner> {
    // SAFETY: a non-NULL `db` came from `DatabaseInner::into_raw` and is
    // live (caller contract).
    unsafe { db.cast::<DatabaseInner>().as_ref() }
}

/// The database behind `db`, or an error for `NULL`.
///
/// # Safety
///
/// As for [`database_ref`].
pub(crate) unsafe fn database_arg<'a>(
    db: *const fasm_xilinx_database,
) -> Result<&'a DatabaseInner, CapiError> {
    // SAFETY: forwarded from the caller.
    unsafe { database_ref(db) }.ok_or_else(|| CapiError::null("db"))
}

/// Opens the database family directory `db_root` (e.g.
/// `prjxray-db/artix7`, `prjuray-db/zynqusp`; NUL terminated) for `part`
/// (e.g. `xc7a35tcsg324-1`, NUL terminated UTF-8; `NULL` loads only the
/// tile types, which cannot assemble) from its text files, and stores the
/// new database in `*out` (free with `fasm_xilinx_database_free`).
///
/// Errors: `FASM_ERR_DB` (a missing or malformed file, an unknown part,
/// not a database directory), `FASM_ERR_INVALID_ARG`, `FASM_ERR_UTF8`.
///
/// # Safety
///
/// `db_root` and `part` must be `NULL` or NUL terminated; `out` and `err`
/// must be `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_database_open(
    db_root: *const c_char,
    part: *const c_char,
    out: *mut *mut fasm_xilinx_database,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe { open(db_root, part, None, out, err) }
}

/// `fasm_xilinx_database_open` through the binary database cache: the
/// part is loaded from its cache file when none of its source files
/// changed, else from the text files (and the cache file is rewritten);
/// the result is the same. `cache_dir` (NUL terminated) is the cache
/// directory; `NULL` uses the settings of the command line tools
/// (`$FASM_XDB_CACHE`, else `$XDG_CACHE_HOME/fasm/db`, else
/// `~/.cache/fasm/db`; `FASM_XDB_CACHE=0` disables the cache). Problems
/// with the cache itself are never errors.
///
/// # Safety
///
/// As for `fasm_xilinx_database_open`; `cache_dir` must be `NULL` or NUL
/// terminated.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_database_open_cached(
    db_root: *const c_char,
    part: *const c_char,
    cache_dir: *const c_char,
    out: *mut *mut fasm_xilinx_database,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        let options = if cache_dir.is_null() {
            CacheOptions::from_env()
        } else {
            match path_arg(cache_dir, "cache_dir") {
                Ok(dir) => {
                    let mut options = CacheOptions::from_env();
                    options.directory = Some(dir);
                    options
                }
                Err(e) => {
                    set_out(out, ptr::null_mut());
                    return run_status(err, || Err(e));
                }
            }
        };
        open(db_root, part, Some(options), out, err)
    }
}

/// # Safety
///
/// As for `fasm_xilinx_database_open`.
unsafe fn open(
    db_root: *const c_char,
    part: *const c_char,
    cache: Option<CacheOptions>,
    out: *mut *mut fasm_xilinx_database,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        set_out(out, ptr::null_mut());
        run_status(err, || {
            if out.is_null() {
                return Err(CapiError::null("out"));
            }
            let root = path_arg(db_root, "db_root")?;
            let part = super::opt_str(part, "part")?;
            let options = cache.unwrap_or_else(CacheOptions::disabled);
            let db = Database::open_cached(&root, part, &options).map_err(|e| db_error(&e))?;
            set_out(out, DatabaseInner::into_raw(db));
            Ok(())
        })
    }
}

/// Frees `db`. `NULL` is accepted (no-op). Assemblers and parts made from
/// it stay valid.
///
/// # Safety
///
/// `db` must be `NULL` or a live `fasm_xilinx_database` that is not used
/// afterwards.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_database_free(db: *mut fasm_xilinx_database) {
    guard((), || {
        if !db.is_null() {
            // SAFETY: `db` came from `Box::into_raw` in
            // `DatabaseInner::into_raw` and is freed once (caller contract).
            drop(unsafe { Box::from_raw(db.cast::<DatabaseInner>()) });
        }
    });
}

/// Returns the architecture of `db` (`FASM_XILINX_SERIES7` for `NULL`).
///
/// # Safety
///
/// `db` must be `NULL` or a live `fasm_xilinx_database`.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_database_architecture(
    db: *const fasm_xilinx_database,
) -> fasm_xilinx_architecture {
    guard(fasm_xilinx_architecture::FASM_XILINX_SERIES7, || {
        // SAFETY: forwarded from the caller.
        unsafe { database_ref(db) }.map_or(fasm_xilinx_architecture::FASM_XILINX_SERIES7, |d| {
            fasm_xilinx_architecture::from_rust(d.db.architecture())
        })
    })
}

/// Returns the number of 32-bit words per frame of the architecture of
/// `db` (101, 123 or 93; 0 for `NULL`).
///
/// # Safety
///
/// `db` must be `NULL` or a live `fasm_xilinx_database`.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_database_words_per_frame(
    db: *const fasm_xilinx_database,
) -> usize {
    guard(0, || {
        // SAFETY: forwarded from the caller.
        unsafe { database_ref(db) }.map_or(0, |d| d.db.architecture().words_per_frame())
    })
}

/// Returns the part name of `db` (NUL terminated, owned by `db`), or
/// `NULL` if it was opened without a part (or for a `NULL` `db`).
///
/// # Safety
///
/// `db` must be `NULL` or a live `fasm_xilinx_database`.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_database_part(
    db: *const fasm_xilinx_database,
) -> *const c_char {
    guard(ptr::null(), || {
        // SAFETY: forwarded from the caller.
        unsafe { database_ref(db) }
            .and_then(|d| d.part.as_ref())
            .map_or(ptr::null(), |p| p.as_ptr())
    })
}

/// A bit a feature sets or clears (`fasm_xilinx_database_lookup`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct fasm_xilinx_bit {
    /// Frame address.
    pub frame: u32,
    /// 32-bit word within the frame.
    pub word: u32,
    /// Bit within the word (0 to 31).
    pub bit: u32,
    /// `true` if the feature sets the bit, `false` if it clears it (a `!`
    /// segbit).
    pub value: bool,
}

/// What a feature bit is (`fasm_xilinx_database_lookup`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct fasm_xilinx_feature_info {
    /// `true` for a pseudo PIP: valid, sets no bits (the other fields are
    /// 0, `block_type` is -1).
    pub pseudo_pip: bool,
    /// The bus: 0 `CLB_IO_CLK`, 1 `BLOCK_RAM`, 2 `CFG_CLB`; -1 for a
    /// pseudo PIP.
    pub block_type: i32,
    /// The first frame of the bus of the tile.
    pub base_address: u32,
    /// The number of frames of the bus of the tile.
    pub frame_count: u32,
    /// The effective word offset of the tile in its frames.
    pub offset: i64,
    /// The number of bits (the ones that cannot be placed in a frame,
    /// which the assembler drops with a warning, are not counted).
    pub bit_count: usize,
}

/// Looks up bit `address` of the FASM feature `feature[0..len]`
/// (`TILE.SITE.FEATURE`, UTF-8; `address` is the `N` of `FEATURE[N]`, 0
/// for a feature without one), like the assembler does. Fills `*info`
/// and the first `bits_capacity` bits into `bits` (`info->bit_count` is
/// the total, so a caller can size `bits` with a first call with
/// `bits_capacity` 0; `bits` may be `NULL` then).
///
/// Errors: `FASM_ERR_ASSEMBLER` (kind `KeyError`) for an unknown tile or
/// tile type, `FASM_ERR_LOOKUP` for a feature that is not in the tile's
/// segbits (`Segment DB <type>, key <type>.<feature> not found`),
/// `FASM_ERR_DB` for a database opened without a part.
///
/// # Safety
///
/// `db` must be `NULL` or a live database; `feature` valid for `len`
/// bytes; `info` `NULL` or writable; `bits` `NULL` or writable for
/// `bits_capacity` elements; `err` `NULL` or writable.
#[no_mangle]
pub unsafe extern "C" fn fasm_xilinx_database_lookup(
    db: *const fasm_xilinx_database,
    feature: *const c_char,
    len: usize,
    address: u32,
    info: *mut fasm_xilinx_feature_info,
    bits: *mut fasm_xilinx_bit,
    bits_capacity: usize,
    err: *mut *mut fasm_error,
) -> fasm_status {
    // SAFETY: forwarded from the caller.
    unsafe {
        run_status(err, || {
            let db = database_arg(db)?;
            if info.is_null() {
                return Err(CapiError::null("info"));
            }
            if bits.is_null() && bits_capacity > 0 {
                return Err(CapiError::null("bits"));
            }
            let name =
                std::str::from_utf8(bytes(feature.cast::<u8>(), len, "feature")?).map_err(|e| {
                    CapiError::new(
                        fasm_status::FASM_ERR_UTF8,
                        format!("feature is not valid UTF-8: {e}"),
                    )
                })?;
            let (tile, rest) = name.split_once('.').unwrap_or((name, ""));
            let intern = |s: &str| IdString::lookup(s).unwrap_or_else(|| IdString::new(s));
            let found = db
                .db
                .lookup_feature(intern(tile), intern(rest), address)
                .map_err(|e| lookup_error(&e))?;
            let mut result = fasm_xilinx_feature_info {
                block_type: -1,
                ..Default::default()
            };
            match found {
                FeatureLookup::PseudoPip(_) => result.pseudo_pip = true,
                FeatureLookup::Bits(found) => {
                    result.block_type = match found.block_type() {
                        BlockType::ClbIoClk => 0,
                        BlockType::BlockRam => 1,
                        BlockType::CfgClb => 2,
                    };
                    result.base_address = found.block.base_address;
                    result.frame_count = found.block.frames;
                    result.offset = found.offset;
                    for (segbit, position) in found.positions() {
                        let Ok(position) = position else { continue };
                        if result.bit_count < bits_capacity {
                            bits.add(result.bit_count).write(fasm_xilinx_bit {
                                frame: position.frame.0,
                                word: position.word,
                                bit: position.bit,
                                value: segbit.is_set,
                            });
                        }
                        result.bit_count += 1;
                    }
                }
            }
            info.write(result);
            Ok(())
        })
    }
}

/// The error of a failed lookup: what the assembler reports for the same
/// feature (without its line).
fn lookup_error(error: &LookupError) -> CapiError {
    match error {
        LookupError::UnknownTile { tile } => {
            super::assembler_error(&AssemblerError::KeyError(tile.to_string()))
        }
        LookupError::UnknownTileType { tile_type, .. } => super::assembler_error(
            &AssemblerError::KeyError(tile_type.to_string().to_ascii_uppercase()),
        ),
        LookupError::UnknownFeature { .. } | LookupError::MissingBitsBlock { .. } => {
            super::assembler_error(&AssemblerError::Lookup(vec![error.to_string()]))
        }
        LookupError::NoGrid => {
            CapiError::new(fasm_status::FASM_ERR_DB, error.to_string()).with_kind("AttributeError")
        }
        _ => CapiError::new(fasm_status::FASM_ERR_ASSEMBLER, error.to_string())
            .with_kind("AssertionError"),
    }
}

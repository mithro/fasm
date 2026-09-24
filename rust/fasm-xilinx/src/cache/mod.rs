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

//! A versioned, content validated binary cache of opened databases.
//!
//! [`Database::open`] parses `tilegrid.json` and every segbits file of the
//! family, which takes 90 ms (xc7a35t) to 190 ms (xc7a200t) and dominates
//! the run time of small designs. [`Database::open_cached`] stores the
//! loaded tables in one binary file per (database root, part) and loads
//! them from there on the next open, after checking that none of the
//! source files changed.
//!
//! * **Never stale.** A cache file records every file the loader read or
//!   probed for the part (path relative to the root, existence, size, a
//!   stat fingerprint and the BLAKE3 hash of the content), the list of
//!   tile types, the layout, the part and the canonical root, plus the
//!   fingerprint of the loader's own source code. On load the stat
//!   fingerprints are compared (Unix: device, inode, modification and
//!   status change times, size); a file whose fingerprint changed is
//!   hashed again and only a different content invalidates the cache (the
//!   new fingerprints are then written back). [`verify_file`] always
//!   hashes every file.
//! * **Never garbage.** Magic, format version, lengths and BLAKE3 hashes
//!   of the header and the payload cover every byte of the file; a
//!   truncated, corrupt, foreign or outdated file is ignored and rebuilt
//!   from the text files, like a stale one.
//! * **Never fatal.** Any cache problem (unreadable directory, full disk,
//!   ...) only means the database is loaded from the text files; errors
//!   of the text loader are returned exactly as [`Database::open`] returns
//!   them. Files are written to a temporary file and renamed, so
//!   concurrent processes never see a partial file.
//!
//! The location comes from the environment ([`CacheOptions::from_env`]):
//!
//! | variable | meaning |
//! |---|---|
//! | `FASM_XDB_CACHE` | cache directory; `0` or empty disables the cache; unset: `$XDG_CACHE_HOME/fasm/db`, else `~/.cache/fasm/db` |
//! | `FASM_XDB_CACHE_VERBOSE` | `1`: report hits, rebuilds and their reason on stderr |
//!
//! (`FASM_DB_CACHE` is already the directory of the *text* databases
//! fetched by `tools/fetch-db.sh`, hence the separate name.)
//!
//! ```no_run
//! use std::path::Path;
//! use fasm_xilinx::cache::CacheOptions;
//! use fasm_xilinx::Database;
//!
//! let db = Database::open_cached(
//!     Path::new("prjxray-db/artix7"),
//!     Some("xc7a35tcsg324-1"),
//!     &CacheOptions::from_env(),
//! )?;
//! # Ok::<(), fasm_xilinx::DbError>(())
//! ```
//!
//! The file format is described in `docs/rewrite/DESIGN-xilinx-db.md`
//! §8.8.

use std::ffi::OsString;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use crate::arch::Architecture;
use crate::db::{detect_layout, tile_type_names, Database, Layout};
use crate::error::{read_text, DbError};
use crate::yaml;

mod format;
mod sources;
#[cfg(test)]
mod tests;

pub use format::{Hash, FORMAT_VERSION, MAGIC};
pub use sources::{SourceFile, SourceKind, StatFingerprint};

use format::{Reader, Writer, PREFIX_LEN};
use sources::{Freshness, Plan};

/// The environment variable naming the cache directory.
pub const CACHE_DIR_ENV: &str = "FASM_XDB_CACHE";
/// The environment variable enabling messages on stderr.
pub const VERBOSE_ENV: &str = "FASM_XDB_CACHE_VERBOSE";
/// The extension of cache files.
pub const EXTENSION: &str = "fasmxdb";

/// Fingerprint of the sources of this crate (see `build.rs`): a cache
/// file written by a build with other loader sources is rebuilt.
pub const LOADER_FINGERPRINT: &str = env!("FASM_XILINX_LOADER_FINGERPRINT");

/// Where (and whether) [`Database::open_cached`] caches databases.
///
/// The default is a disabled cache.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheOptions {
    /// The cache directory (created when needed), `None` to disable the
    /// cache.
    pub directory: Option<PathBuf>,
    /// Hash every source file on every load instead of trusting unchanged
    /// stat fingerprints.
    pub verify_contents: bool,
    /// Report hits, rebuilds and cache errors on stderr.
    pub verbose: bool,
}

impl CacheOptions {
    /// A disabled cache ([`Database::open_cached`] is [`Database::open`]).
    pub fn disabled() -> Self {
        CacheOptions::default()
    }

    /// A cache in `directory`.
    pub fn in_directory(directory: impl Into<PathBuf>) -> Self {
        CacheOptions {
            directory: Some(directory.into()),
            ..CacheOptions::default()
        }
    }

    /// The options given by the environment of this process (see the
    /// [module documentation](self)).
    pub fn from_env() -> Self {
        Self::from_vars(|name| std::env::var_os(name))
    }

    /// The options given by the environment variables `get` returns:
    ///
    /// * `FASM_XDB_CACHE`: `0` or empty disables the cache, any other
    ///   value is the directory;
    /// * unset: `$XDG_CACHE_HOME/fasm/db` (when set to an absolute path),
    ///   else `$HOME/.cache/fasm/db`, else no cache;
    /// * `FASM_XDB_CACHE_VERBOSE`: any value but empty or `0` enables the
    ///   messages.
    pub fn from_vars(get: impl Fn(&str) -> Option<OsString>) -> Self {
        let set = |name: &str| get(name).filter(|v| !v.is_empty());
        let directory = match get(CACHE_DIR_ENV) {
            Some(v) if v.is_empty() || v == "0" => None,
            Some(v) => Some(PathBuf::from(v)),
            None => set("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| set("HOME").map(|home| PathBuf::from(home).join(".cache")))
                .map(|base| base.join("fasm").join("db")),
        };
        let verbose = get(VERBOSE_ENV).is_some_and(|v| !v.is_empty() && v != "0");
        CacheOptions {
            directory,
            verify_contents: false,
            verbose,
        }
    }

    fn log(&self, message: impl FnOnce() -> String) {
        if self.verbose {
            let _ = writeln!(io::stderr().lock(), "fasm-xilinx db cache: {}", message());
        }
    }
}

/// What [`open`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CacheOutcome {
    /// The cache is disabled (or no part was given): the text files were
    /// loaded.
    Disabled,
    /// The database was loaded from the cache file.
    Hit {
        /// The cache file.
        path: PathBuf,
        /// `true` if some stat fingerprints had changed (same contents)
        /// and the cache file was updated.
        restat: bool,
    },
    /// The text files were loaded and the cache file (re)written.
    Rebuilt {
        /// The cache file.
        path: PathBuf,
        /// Why the cache file could not be used.
        reason: String,
        /// Why the new cache file was not written, if it was not.
        write_error: Option<String>,
    },
}

/// Opens a database like [`Database::open`], through the cache described
/// by `options`, and says whether the cache was used.
///
/// # Errors
///
/// Exactly the errors of [`Database::open`]: cache problems are never
/// errors (they are reported in the [`CacheOutcome`] and, with
/// [`CacheOptions::verbose`], on stderr).
pub fn open(
    db_root: &Path,
    part: Option<&str>,
    options: &CacheOptions,
) -> Result<(Database, CacheOutcome), DbError> {
    let (Some(dir), Some(part)) = (&options.directory, part) else {
        return Ok((Database::open(db_root, part)?, CacheOutcome::Disabled));
    };
    let start = Instant::now();
    let Some(key) = CacheKey::new(dir, db_root, part) else {
        // Not a database directory: let the loader report it.
        return Ok((Database::open(db_root, Some(part))?, CacheOutcome::Disabled));
    };
    let reason = match load(&key, db_root, options.verify_contents) {
        Ok(Loaded {
            db,
            timings,
            restat,
        }) => {
            let restat = match restat {
                None => false,
                Some(rewrite) => {
                    if let Err(e) = rewrite() {
                        options.log(|| format!("{}: cannot update: {e}", key.file.display()));
                    }
                    true
                }
            };
            options.log(|| {
                format!(
                    "loaded {} in {:.1} ms ({timings}){}",
                    key.file.display(),
                    ms(start),
                    if restat {
                        "; stat fingerprints updated"
                    } else {
                        ""
                    }
                )
            });
            return Ok((
                db,
                CacheOutcome::Hit {
                    path: key.file,
                    restat,
                },
            ));
        }
        Err(reason) => reason,
    };
    options.log(|| format!("rebuilding {}: {reason}", key.file.display()));
    let (db, written) = build_from_text(&key, db_root)?;
    let write_error = written.err();
    match &write_error {
        Some(e) => options.log(|| format!("not written: {e}")),
        None => options.log(|| format!("wrote {} in {:.1} ms", key.file.display(), ms(start))),
    }
    Ok((
        db,
        CacheOutcome::Rebuilt {
            path: key.file,
            reason,
            write_error,
        },
    ))
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e3
}

impl Database {
    /// [`Database::open`] through the binary cache described by
    /// `options` (see [`crate::cache`]): loads the part from its cache file
    /// when none of its source files changed, else loads the text files
    /// and (re)writes the cache file. The result is always equal (`==`) to
    /// what [`Database::open`] returns.
    ///
    /// Without a part, or with [`CacheOptions::disabled`], this is
    /// [`Database::open`]. The command line tools use
    /// [`CacheOptions::from_env`].
    ///
    /// # Errors
    ///
    /// Exactly the errors of [`Database::open`]; problems with the cache
    /// itself are never errors.
    pub fn open_cached(
        db_root: &Path,
        part: Option<&str>,
        options: &CacheOptions,
    ) -> Result<Self, DbError> {
        open(db_root, part, options).map(|(db, _)| db)
    }
}

/// The identity of a cache file.
struct CacheKey {
    layout: Layout,
    canonical_root: PathBuf,
    part: String,
    file: PathBuf,
}

impl CacheKey {
    fn new(dir: &Path, db_root: &Path, part: &str) -> Option<Self> {
        let layout = detect_layout(db_root)?;
        let canonical_root = std::fs::canonicalize(db_root).ok()?;
        let file = dir.join(cache_file_name(layout, part, &canonical_root));
        Some(CacheKey {
            layout,
            canonical_root,
            part: part.to_owned(),
            file,
        })
    }
}

fn layout_name(layout: Layout) -> &'static str {
    match layout {
        Layout::Prjxray => "prjxray",
        Layout::Prjuray => "prjuray",
    }
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// The file name of the cache of `part` of the database whose canonical
/// root is `canonical_root`: `<layout>-<part>-<root hash>-v<format>.fasmxdb`
/// (characters of the part other than ASCII letters, digits, `-`, `_`
/// and `.` are replaced by `_`; the root hash is the first 8 bytes of the
/// BLAKE3 hash of the path, in hex).
pub fn cache_file_name(layout: Layout, part: &str, canonical_root: &Path) -> String {
    let part: String = part
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let root_hash = format::hash(&path_bytes(canonical_root));
    let hex: String = root_hash[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{part}-{hex}-v{FORMAT_VERSION}.{EXTENSION}",
        layout_name(layout)
    )
}

/// The cache file of `part` of the database `db_root` in `dir`, `None` if
/// `db_root` is not a database directory.
pub fn cache_file(dir: &Path, db_root: &Path, part: &str) -> Option<PathBuf> {
    CacheKey::new(dir, db_root, part).map(|k| k.file)
}

/// The header of a cache file: provenance and source fingerprints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheInfo {
    /// The layout version of the file ([`FORMAT_VERSION`] when readable).
    pub format_version: u32,
    /// [`LOADER_FINGERPRINT`] of the build that wrote the file.
    pub loader_fingerprint: String,
    /// The `fasm-xilinx` version that wrote the file.
    pub crate_version: String,
    /// The database layout.
    pub layout: Layout,
    /// The architecture of the part.
    pub architecture: Architecture,
    /// The part.
    pub part: String,
    /// The canonical database root.
    pub db_root: PathBuf,
    /// Creation time (seconds since the Unix epoch).
    pub created: u64,
    /// Hash of the tile type name list.
    pub listing_hash: Hash,
    /// Every source path.
    pub sources: Vec<SourceFile>,
    /// Size of the payload in bytes.
    pub payload_len: u64,
    /// Size of the file in bytes.
    pub file_len: u64,
}

impl CacheInfo {
    /// Combined hash of the recorded source contents and tile type list.
    pub fn source_hash(&self) -> Hash {
        sources::sources_hash(&self.listing_hash, &self.sources)
    }

    /// Total size of the recorded source files.
    pub fn source_bytes(&self) -> u64 {
        self.sources.iter().map(|s| s.size).sum()
    }
}

fn encode_header(info: &CacheInfo) -> Vec<u8> {
    let mut w = Writer::default();
    w.str(&info.loader_fingerprint);
    w.str(&info.crate_version);
    w.u8(format::layout_code(info.layout));
    w.u8(format::arch_code(info.architecture));
    w.str(&info.part);
    w.bytes(&path_bytes(&info.db_root));
    w.u64(info.created);
    w.hash(&info.listing_hash);
    sources::write_sources(&mut w, &info.sources);
    w.buf
}

/// The prefix (with the hashes of `header` and `payload`) followed by
/// `header`: everything before the payload.
fn prefix_and_header(header: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(PREFIX_LEN + header.len());
    prefix.extend_from_slice(&MAGIC);
    prefix.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    prefix.extend_from_slice(&(header.len() as u32).to_le_bytes());
    prefix.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    prefix.extend_from_slice(&format::hash(header));
    prefix.extend_from_slice(&format::hash(payload));
    prefix.extend_from_slice(header);
    prefix
}

/// A cache file split into its checked header and its payload.
struct Parsed<'a> {
    info: CacheInfo,
    payload: &'a [u8],
    payload_hash: Hash,
}

/// Checks the prefix and the header hash and decodes the header (the
/// payload hash is checked separately, it is the expensive part).
fn parse(data: &[u8]) -> Result<Parsed<'_>, String> {
    if data.len() < PREFIX_LEN {
        return Err("truncated file".to_owned());
    }
    let mut r = Reader::new(data);
    if r.take(8)? != MAGIC {
        return Err("not a cache file (wrong magic)".to_owned());
    }
    let version = r.u32()?;
    if version != FORMAT_VERSION {
        return Err(format!(
            "format version {version} (expected {FORMAT_VERSION})"
        ));
    }
    let header_len = r.u32()? as u64;
    let payload_len = r.u64()?;
    let header_hash = r.hash()?;
    let payload_hash = r.hash()?;
    if (PREFIX_LEN as u64)
        .checked_add(header_len)
        .and_then(|n| n.checked_add(payload_len))
        != Some(data.len() as u64)
    {
        return Err("truncated or extended file (lengths do not match)".to_owned());
    }
    let header = &data[PREFIX_LEN..PREFIX_LEN + header_len as usize];
    let payload = &data[PREFIX_LEN + header_len as usize..];
    if format::hash(header) != header_hash {
        return Err("corrupt header (hash mismatch)".to_owned());
    }
    let mut r = Reader::new(header);
    let loader_fingerprint = r.string()?;
    let crate_version = r.string()?;
    let layout = format::layout_from_code(r.u8()?)?;
    let architecture = format::arch_from_code(r.u8()?)?;
    let part = r.string()?;
    let db_root = path_from_bytes(r.bytes()?);
    let created = r.u64()?;
    let listing_hash = r.hash()?;
    let sources = sources::read_sources(&mut r)?;
    if !r.is_empty() {
        return Err("corrupt header (trailing bytes)".to_owned());
    }
    Ok(Parsed {
        info: CacheInfo {
            format_version: version,
            loader_fingerprint,
            crate_version,
            layout,
            architecture,
            part,
            db_root,
            created,
            listing_hash,
            sources,
            payload_len,
            file_len: data.len() as u64,
        },
        payload,
        payload_hash,
    })
}

/// Checks everything but the payload hash and the source contents: the
/// file belongs to `key`, was written by this loader, and the layout and
/// tile type list are unchanged.
fn check_identity(info: &CacheInfo, key: &CacheKey, db_root: &Path) -> Result<(), String> {
    if info.loader_fingerprint != LOADER_FINGERPRINT {
        return Err(format!(
            "written by another loader build ({} {}, this is {} {})",
            info.crate_version,
            info.loader_fingerprint,
            env!("CARGO_PKG_VERSION"),
            LOADER_FINGERPRINT
        ));
    }
    if info.part != key.part || info.db_root != key.canonical_root || info.layout != key.layout {
        return Err(format!(
            "file is for {} {} ({})",
            info.db_root.display(),
            info.part,
            layout_name(info.layout)
        ));
    }
    check_listing(info, db_root)
}

fn check_listing(info: &CacheInfo, db_root: &Path) -> Result<(), String> {
    if detect_layout(db_root) != Some(info.layout) {
        return Err("the database layout changed".to_owned());
    }
    let names = tile_type_names(db_root, info.layout).map_err(|e| e.to_string())?;
    if sources::listing_hash(&names) != info.listing_hash {
        return Err("the tile type list changed".to_owned());
    }
    Ok(())
}

/// A successful cache load.
struct Loaded {
    db: Database,
    /// Time of each step, for the verbose message.
    timings: String,
    /// Writes back the updated stat fingerprints, if they changed.
    restat: Option<Box<dyn FnOnce() -> io::Result<()>>>,
}

/// Loads `key` from its cache file, or says why it cannot.
fn load(key: &CacheKey, db_root: &Path, full: bool) -> Result<Loaded, String> {
    let start = Instant::now();
    let data = match read_file(&key.file) {
        Ok(data) => data,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err("no cache file".to_owned()),
        Err(e) => return Err(format!("cannot read: {e}")),
    };
    let t_read = ms(start);
    let parsed = parse(&data)?;
    check_identity(&parsed.info, key, db_root)?;
    let t_identity = ms(start) - t_read;

    // The payload hash, the source check and the decoding are
    // independent: for a large file they run concurrently and the
    // decoded database is dropped if either check fails.
    let timed = |f: &dyn Fn() -> bool| {
        let t = Instant::now();
        (f(), ms(t))
    };
    let check_hash = || format::hash(parsed.payload) == parsed.payload_hash;
    let check_sources = || sources::check(db_root, &parsed.info.sources, full);
    let parallel = parsed.payload.len() >= PARALLEL_MIN_BYTES;
    let decode = || {
        format::decode_payload(
            parsed.payload,
            db_root,
            parsed.info.layout,
            parsed.info.architecture,
            parallel,
        )
    };
    let decode_start = Instant::now();
    let ((hash_ok, t_hash), freshness, decoded) = if !parallel {
        let hash = timed(&check_hash);
        if !hash.0 {
            return Err("corrupt payload (hash mismatch)".to_owned());
        }
        (hash, check_sources(), decode())
    } else {
        std::thread::scope(|scope| {
            let hash = scope.spawn(|| timed(&check_hash));
            let freshness = scope.spawn(check_sources);
            let decoded = decode();
            let join_error = || "cache check panicked".to_owned();
            (
                hash.join().unwrap_or((false, 0.0)),
                freshness.join().unwrap_or_else(|_| Err(join_error())),
                decoded,
            )
        })
    };
    if !hash_ok {
        return Err("corrupt payload (hash mismatch)".to_owned());
    }
    let freshness = freshness?;
    let db = decoded.map_err(|e| format!("corrupt payload: {e}"))?;
    let timings = format!(
        "read {t_read:.1} ms ({:.1} MiB), header and tile type list {t_identity:.1} ms, \
         decode {:.1} ms (payload hash {t_hash:.1} ms and source check concurrently)",
        data.len() as f64 / (1024.0 * 1024.0),
        ms(decode_start)
    );
    let restat = match freshness {
        Freshness::Fresh => None,
        Freshness::FreshRestat(sources) => {
            let mut info = parsed.info.clone();
            info.sources = sources;
            let payload_range = data.len() - parsed.payload.len()..;
            let file = key.file.clone();
            let rewrite = move || -> io::Result<()> {
                let header = encode_header(&info);
                let prefix = prefix_and_header(&header, &data[payload_range.clone()]);
                write_atomic(&file, &[&prefix, &data[payload_range]])
            };
            Some(Box::new(rewrite) as Box<dyn FnOnce() -> io::Result<()>>)
        }
    };
    Ok(Loaded {
        db,
        timings,
        restat,
    })
}

/// Loads the text files and writes the cache file; the error of the
/// write (the database is returned either way).
fn build_from_text(
    key: &CacheKey,
    db_root: &Path,
) -> Result<(Database, Result<(), String>), DbError> {
    // The sources are fingerprinted (and hashed) *before* the load and
    // stat'ed again after it: a file changed in between makes the
    // fingerprints differ and the cache file is not written, so a cache
    // can never claim contents it was not built from.
    let before = Plan::new(db_root, &key.part).and_then(|plan| {
        let sources = plan
            .fingerprint(db_root, true)
            .map_err(|e| DbError::from_io(db_root, e))?;
        Ok((plan, sources))
    });
    let db = Database::open(db_root, Some(&key.part))?;
    let (plan, sources) = match before {
        Ok(x) => x,
        Err(e) => return Ok((db, Err(format!("cannot fingerprint the sources: {e}")))),
    };
    let result = (|| {
        let after = Plan::new(db_root, &key.part).map_err(|e| e.to_string())?;
        let after_sources = after
            .fingerprint(db_root, false)
            .map_err(|e| e.to_string())?;
        let same_types = db.tile_types.len() == plan.tile_types.len()
            && db
                .tile_types
                .iter()
                .zip(&plan.tile_types)
                .all(|(t, n)| t.name == n.as_str());
        let same_fabric =
            plan.fabric.is_none() || db.part.as_ref().map(|p| &p.fabric) == plan.fabric.as_ref();
        if after != plan
            || !sources::same_state(&sources, &after_sources)
            || !same_types
            || !same_fabric
        {
            return Err("the database changed while it was loaded".to_owned());
        }
        let info = CacheInfo {
            format_version: FORMAT_VERSION,
            loader_fingerprint: LOADER_FINGERPRINT.to_owned(),
            crate_version: env!("CARGO_PKG_VERSION").to_owned(),
            layout: key.layout,
            architecture: db.architecture,
            part: key.part.clone(),
            db_root: key.canonical_root.clone(),
            created: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            listing_hash: sources::listing_hash(&plan.tile_types),
            sources,
            payload_len: 0,
            file_len: 0,
        };
        let payload = format::encode_payload(&db);
        let header = encode_header(&info);
        let prefix = prefix_and_header(&header, &payload);
        write_atomic(&key.file, &[&prefix, &payload])
            .map_err(|e| format!("{}: {e}", key.file.display()))
    })();
    Ok((db, result))
}

/// Files and payloads smaller than this are read and checked on the
/// calling thread only.
const PARALLEL_MIN_BYTES: usize = 1 << 20;

/// Reads a whole file. Large files are read by several threads with
/// positional reads (Unix): the time goes mostly into the page faults of
/// the fresh buffer, which the kernel serves concurrently.
fn read_file(path: &Path) -> io::Result<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        let file = std::fs::File::open(path)?;
        let len = usize::try_from(file.metadata()?.len())
            .map_err(|_| io::Error::other("file too large"))?;
        if len >= PARALLEL_MIN_BYTES {
            let mut data = vec![0u8; len];
            let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(4));
            let chunk = len.div_ceil(threads);
            std::thread::scope(|scope| {
                let handles: Vec<_> = data
                    .chunks_mut(chunk)
                    .enumerate()
                    .map(|(i, buf)| {
                        let file = &file;
                        scope.spawn(move || file.read_exact_at(buf, (i * chunk) as u64))
                    })
                    .collect();
                handles.into_iter().try_for_each(|h| {
                    h.join()
                        .unwrap_or_else(|_| Err(io::Error::other("reader thread panicked")))
                })
            })?;
            return Ok(data);
        }
    }
    std::fs::read(path)
}

/// Writes `parts` to `path` through a temporary file in the same
/// directory and a rename.
fn write_atomic(path: &Path, parts: &[&[u8]]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.{}.{nanos}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        for part in parts {
            file.write_all(part)?;
        }
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

// ---------------------------------------------------------------------
// Maintenance (the `fasm-db-cache` tool).

/// An error of [`build`].
#[derive(Debug)]
#[non_exhaustive]
pub enum CacheError {
    /// The text loader failed.
    Db(DbError),
    /// The database root is not a database directory.
    NotADatabase(PathBuf),
    /// The cache file could not be written, or the sources changed
    /// while they were loaded.
    Write(String),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheError::Db(e) => e.fmt(f),
            CacheError::NotADatabase(root) => write!(
                f,
                "{}: not a prjxray-db (mapping/) or prjuray-db (tile_types/) family directory",
                root.display()
            ),
            CacheError::Write(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for CacheError {}

impl From<DbError> for CacheError {
    fn from(e: DbError) -> Self {
        CacheError::Db(e)
    }
}

/// Loads `part` of `db_root` from the text files and (re)writes its
/// cache file in `dir`, whatever state the existing file is in; returns
/// the header of the new file.
///
/// # Errors
///
/// The loader's error, or why the file could not be written.
pub fn build(dir: &Path, db_root: &Path, part: &str) -> Result<CacheInfo, CacheError> {
    let key = CacheKey::new(dir, db_root, part)
        .ok_or_else(|| CacheError::NotADatabase(db_root.to_path_buf()))?;
    let (_, written) = build_from_text(&key, db_root)?;
    written.map_err(CacheError::Write)?;
    read_info(&key.file).map_err(CacheError::Write)
}

/// Reads the header of a cache file (checking the prefix and the header
/// hash, not the payload or the sources).
///
/// # Errors
///
/// Why the file is not a readable cache file.
pub fn read_info(path: &Path) -> Result<CacheInfo, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    parse(&data).map(|p| p.info)
}

/// The result of [`verify_file`].
#[derive(Clone, Debug)]
pub struct VerifyReport {
    /// The header, if it could be read.
    pub info: Option<CacheInfo>,
    /// `None` if the file is valid and up to date, else why not.
    pub problem: Option<String>,
}

/// Fully checks a cache file: prefix, header and payload hashes, the
/// loader fingerprint, the content hash of every source file (whatever
/// the stat fingerprints say) and that the payload decodes.
pub fn verify_file(path: &Path) -> VerifyReport {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(e) => {
            return VerifyReport {
                info: None,
                problem: Some(e.to_string()),
            }
        }
    };
    let parsed = match parse(&data) {
        Ok(parsed) => parsed,
        Err(e) => {
            return VerifyReport {
                info: None,
                problem: Some(e),
            }
        }
    };
    let info = &parsed.info;
    let check = || -> Result<(), String> {
        if format::hash(parsed.payload) != parsed.payload_hash {
            return Err("corrupt payload (hash mismatch)".to_owned());
        }
        if info.loader_fingerprint != LOADER_FINGERPRINT {
            return Err(format!(
                "written by another loader build ({} {})",
                info.crate_version, info.loader_fingerprint
            ));
        }
        let root = &info.db_root;
        if !root.is_dir() {
            return Err(format!("{}: database root not found", root.display()));
        }
        let expected = cache_file_name(info.layout, &info.part, root);
        if path.file_name().and_then(|n| n.to_str()) != Some(expected.as_str()) {
            return Err(format!("misnamed file (expected {expected})"));
        }
        check_listing(info, root)?;
        sources::check(root, &info.sources, true)?;
        format::decode_payload(
            parsed.payload,
            root,
            info.layout,
            info.architecture,
            parsed.payload.len() >= PARALLEL_MIN_BYTES,
        )
        .map_err(|e| format!("corrupt payload: {e}"))?;
        Ok(())
    };
    let problem = check().err();
    VerifyReport {
        info: Some(parsed.info.clone()),
        problem,
    }
}

/// The cache files (`*.fasmxdb`) in `dir`, sorted; none if `dir` does
/// not exist.
///
/// # Errors
///
/// Listing `dir` failed.
pub fn cache_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with('.') && path.extension().is_some_and(|e| e == EXTENSION) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Removes the cache files of `dir` (and temporary files left by
/// interrupted writes); returns the number of files removed.
///
/// # Errors
///
/// Listing `dir` or removing a file failed.
pub fn clear(dir: &Path) -> io::Result<usize> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut removed = 0;
    let suffix = format!(".{EXTENSION}");
    for entry in entries {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let is_cache = !name.starts_with('.') && name.ends_with(&suffix);
        let is_tmp = name.starts_with('.') && name.contains(&suffix) && name.ends_with(".tmp");
        if (is_cache || is_tmp) && path.is_file() {
            std::fs::remove_file(&path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Every part of a database family: the keys of `mapping/parts.yaml`
/// (prjxray-db, file order) or the directories holding a
/// `tilegrid.json` (prjuray-db, sorted).
///
/// # Errors
///
/// An unknown layout, or reading `parts.yaml` / the directory failed.
pub fn family_parts(db_root: &Path) -> Result<Vec<String>, DbError> {
    match detect_layout(db_root) {
        None => Err(DbError::UnknownLayout {
            root: db_root.to_path_buf(),
        }),
        Some(Layout::Prjxray) => {
            let path = db_root.join("mapping").join("parts.yaml");
            let node = yaml::parse(&read_text(&path)?).map_err(|e| DbError::Yaml {
                path: path.clone(),
                line: e.line,
                message: e.message,
            })?;
            let map = node.as_map().map_err(|e| DbError::Yaml {
                path: path.clone(),
                line: e.line,
                message: e.message,
            })?;
            Ok(map.iter().map(|(k, _)| k.clone()).collect())
        }
        Some(Layout::Prjuray) => {
            let entries = std::fs::read_dir(db_root).map_err(|e| DbError::from_io(db_root, e))?;
            let mut parts = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|e| DbError::from_io(db_root, e))?;
                if let Some(name) = entry.file_name().to_str() {
                    if entry.path().join("tilegrid.json").is_file() {
                        parts.push(name.to_owned());
                    }
                }
            }
            parts.sort();
            Ok(parts)
        }
    }
}

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

//! The source files of a cached database and their fingerprints.
//!
//! The set of files [`crate::Database::open`] reads for a part is fixed by
//! the layout, the tile type names (from the `tile_type_*.json` file
//! names), the fabric (from `mapping/{parts,devices}.yaml`, which are
//! themselves sources) and the part name, so the cache records:
//!
//! * the layout (checked against the directory again on load);
//! * a hash of the sorted tile type name list (the directory is listed
//!   again on load);
//! * for every file the loader reads or probes: its path relative to the
//!   database root, whether it exists, its size, a stat fingerprint and
//!   the BLAKE3 hash of its content (`mask_*.db` files, which the loader
//!   only probes, are recorded by existence only).

use std::fs::Metadata;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::format::{self, Corrupt, Hash, Reader, Writer};
use crate::db::{detect_layout, prjxray_fabric, tile_type_file_names, tile_type_names, Layout};
use crate::error::DbError;

/// What the cache knows about one source path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// A file the loader reads: size, stat fingerprint and content hash
    /// are recorded.
    Content,
    /// A path the loader probes that did not exist (or was not a regular
    /// file): it must still not exist.
    Absent,
    /// A file whose existence only matters (`mask_*.db`).
    Present,
}

/// Cheap change detector of a file (Unix: device, inode, modification
/// and status change times). When it is unchanged the content hash is
/// trusted; when it differs the content is hashed again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatFingerprint {
    /// Device number.
    pub dev: u64,
    /// Inode number.
    pub ino: u64,
    /// Modification time (seconds, nanoseconds).
    pub mtime: (i64, u32),
    /// Status change time (seconds, nanoseconds): changes on any write,
    /// rename or `touch`, even one that restores the modification time.
    pub ctime: (i64, u32),
}

/// How recent a modification or status change time can be for the stat
/// fingerprint to be trusted (the "racy git" problem): with a coarse
/// timestamp granularity (1 s on ext3 and HFS+, 2 s on FAT, clock skew on
/// network file systems), a file changed again right after it was
/// fingerprinted can keep the same size and times.
///
/// * On load, such a fingerprint is not recorded: the file is hashed on
///   every load until a load finds it old enough.
/// * A cache file is not written at all when a source it reads changed
///   less than this before the build started (or during it, see
///   [`changed_recently`]): the text loader and the hashing read the
///   files at different moments, and only the timestamps could show a
///   same size rewrite in between.
///
/// Zero in the unit tests (the window is tested by itself with explicit
/// values), which rewrite files and reload them immediately.
#[cfg(not(test))]
pub(crate) const RACY_WINDOW: Duration = Duration::from_secs(5);
#[cfg(test)]
pub(crate) const RACY_WINDOW: Duration = Duration::ZERO;

/// Whether a stat fingerprint taken at `now` may miss a later change:
/// its modification or status change time is within `window` of `now`
/// (or later).
pub(crate) fn is_racy(stat: &StatFingerprint, now: SystemTime, window: Duration) -> bool {
    let nanos = |(s, ns): (i64, u32)| i128::from(s) * 1_000_000_000 + i128::from(ns);
    let limit = match now.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128 - window.as_nanos() as i128,
        Err(_) => return true,
    };
    nanos(stat.mtime) >= limit || nanos(stat.ctime) >= limit
}

/// Drops the stat fingerprints that are too recent to be trusted at
/// `now` (see [`RACY_WINDOW`]).
pub(crate) fn drop_racy(sources: &mut [SourceFile], now: SystemTime) {
    for source in sources {
        if source.stat.is_some_and(|s| is_racy(&s, now, RACY_WINDOW)) {
            source.stat = None;
        }
    }
}

/// The first `Content` source (in either fingerprint list) whose
/// modification or status change time is less than `window` before
/// `start` or later; without stat fingerprints (not Unix), the
/// modification time the file has now.
pub(crate) fn changed_recently<'a>(
    root: &Path,
    lists: [&'a [SourceFile]; 2],
    start: SystemTime,
    window: Duration,
) -> Option<&'a str> {
    let limit = start.checked_sub(window);
    lists.into_iter().flatten().find_map(|s| {
        if s.kind != SourceKind::Content {
            return None;
        }
        let recent = match &s.stat {
            Some(stat) => is_racy(stat, start, window),
            None => match (
                limit,
                std::fs::metadata(root.join(&s.path)).and_then(|m| m.modified()),
            ) {
                (Some(limit), Ok(modified)) => modified >= limit,
                _ => true,
            },
        };
        recent.then_some(s.path.as_str())
    })
}

#[cfg(unix)]
fn stat_fingerprint(meta: &Metadata) -> Option<StatFingerprint> {
    use std::os::unix::fs::MetadataExt;
    Some(StatFingerprint {
        dev: meta.dev(),
        ino: meta.ino(),
        mtime: (meta.mtime(), meta.mtime_nsec() as u32),
        ctime: (meta.ctime(), meta.ctime_nsec() as u32),
    })
}

/// Without inode numbers and status change times the fast path is not
/// trusted: every file is hashed on every load.
#[cfg(not(unix))]
fn stat_fingerprint(_meta: &Metadata) -> Option<StatFingerprint> {
    None
}

/// One source path of a cache file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    /// Path relative to the database root, `/` separated.
    pub path: String,
    /// What is recorded.
    pub kind: SourceKind,
    /// Size in bytes ([`SourceKind::Content`] only).
    pub size: u64,
    /// Stat fingerprint ([`SourceKind::Content`] on Unix only).
    pub stat: Option<StatFingerprint>,
    /// BLAKE3 hash of the content ([`SourceKind::Content`] only).
    pub hash: Option<Hash>,
}

/// How a planned path is recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// Read by the loader when it exists.
    Content,
    /// Only its existence matters.
    Presence,
}

/// The paths the loader reads for a part, computed without loading it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) layout: Layout,
    pub(crate) tile_types: Vec<String>,
    /// prjxray-db: the fabric directory of the part.
    pub(crate) fabric: Option<String>,
    pub(crate) paths: Vec<(String, Role)>,
}

impl Plan {
    /// Lists the source paths of `part` in `root` like
    /// [`crate::Database::open`] would read them.
    pub(crate) fn new(root: &Path, part: &str) -> Result<Self, DbError> {
        let layout = detect_layout(root).ok_or_else(|| DbError::UnknownLayout {
            root: root.to_path_buf(),
        })?;
        let tile_types = tile_type_names(root, layout)?;
        let mut paths = Vec::new();
        let fabric = match layout {
            Layout::Prjxray => {
                paths.push(("mapping/parts.yaml".to_owned(), Role::Content));
                paths.push(("mapping/devices.yaml".to_owned(), Role::Content));
                let (_, fabric) = prjxray_fabric(root, part)?;
                paths.push((format!("{fabric}/tilegrid.json"), Role::Content));
                Some(fabric)
            }
            Layout::Prjuray => {
                paths.push((format!("{part}/tilegrid.json"), Role::Content));
                None
            }
        };
        for file in [
            "part.yaml",
            "part.json",
            "package_pins.csv",
            "required_features.fasm",
        ] {
            paths.push((format!("{part}/{file}"), Role::Content));
        }
        for name in &tile_types {
            let [segbits, block_ram, ppips, mask] = tile_type_file_names(name);
            paths.push((segbits, Role::Content));
            paths.push((block_ram, Role::Content));
            paths.push((ppips, Role::Content));
            paths.push((mask, Role::Presence));
        }
        Ok(Plan {
            layout,
            tile_types,
            fabric,
            paths,
        })
    }

    /// Stats (and, with `hash`, hashes) every planned path.
    pub(crate) fn fingerprint(&self, root: &Path, hash: bool) -> io::Result<Vec<SourceFile>> {
        self.paths
            .iter()
            .map(|(path, role)| fingerprint(root, path, *role, hash))
            .collect()
    }
}

/// The hash of the tile type name list.
pub(crate) fn listing_hash(names: &[String]) -> Hash {
    let mut hasher = blake3::Hasher::new();
    for name in names {
        hasher.update(&(name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn file_metadata(path: &Path) -> Option<Metadata> {
    std::fs::metadata(path).ok().filter(Metadata::is_file)
}

fn fingerprint(root: &Path, rel: &str, role: Role, hash: bool) -> io::Result<SourceFile> {
    let path = root.join(rel);
    let absent = |kind| SourceFile {
        path: rel.to_owned(),
        kind,
        size: 0,
        stat: None,
        hash: None,
    };
    let Some(meta) = file_metadata(&path) else {
        return Ok(absent(SourceKind::Absent));
    };
    if role == Role::Presence {
        return Ok(absent(SourceKind::Present));
    }
    let hash = if hash {
        Some(format::hash(&std::fs::read(&path)?))
    } else {
        None
    };
    Ok(SourceFile {
        path: rel.to_owned(),
        kind: SourceKind::Content,
        size: meta.len(),
        stat: stat_fingerprint(&meta),
        hash,
    })
}

/// Whether two fingerprints (taken before and after loading) describe
/// the same state (the hashes are not compared: the second fingerprint
/// is taken without hashing).
pub(crate) fn same_state(a: &[SourceFile], b: &[SourceFile]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.path == b.path && a.kind == b.kind && a.size == b.size && a.stat == b.stat
        })
}

/// The result of checking recorded sources against the files.
pub(crate) enum Freshness {
    /// Everything matches.
    Fresh,
    /// The contents match, but some stat fingerprints changed; the
    /// updated list should be written back.
    FreshRestat(Vec<SourceFile>),
}

/// Checks `sources` against the files under `root`. With `full`, every
/// file is hashed; otherwise only those whose stat fingerprint changed.
/// `Err` is the reason the cache is stale.
pub(crate) fn check(root: &Path, sources: &[SourceFile], full: bool) -> Result<Freshness, String> {
    let now = SystemTime::now();
    let mut updated: Option<Vec<SourceFile>> = None;
    for (i, source) in sources.iter().enumerate() {
        let path = root.join(&source.path);
        let meta = file_metadata(&path);
        match (source.kind, meta) {
            (SourceKind::Absent, None) | (SourceKind::Present, Some(_)) => {}
            (SourceKind::Absent, Some(_)) => return Err(format!("{} appeared", source.path)),
            (SourceKind::Present | SourceKind::Content, None) => {
                return Err(format!("{} disappeared", source.path))
            }
            (SourceKind::Content, Some(meta)) => {
                if meta.len() != source.size {
                    return Err(format!("{} changed size", source.path));
                }
                let stat = stat_fingerprint(&meta);
                if !full && stat.is_some() && stat == source.stat {
                    continue;
                }
                let data = std::fs::read(&path)
                    .map_err(|e| format!("{}: cannot read: {e}", source.path))?;
                if data.len() as u64 != source.size || Some(format::hash(&data)) != source.hash {
                    return Err(format!("{} changed", source.path));
                }
                // Same content: record the new stat fingerprint, unless it
                // is too recent to be trusted.
                let stat = stat.filter(|s| !is_racy(s, now, RACY_WINDOW));
                if stat.is_some() && stat != source.stat {
                    let list = updated.get_or_insert_with(|| sources.to_vec());
                    list[i].stat = stat;
                }
            }
        }
    }
    Ok(match updated {
        Some(list) => Freshness::FreshRestat(list),
        None => Freshness::Fresh,
    })
}

fn kind_code(kind: SourceKind) -> u8 {
    match kind {
        SourceKind::Content => 0,
        SourceKind::Absent => 1,
        SourceKind::Present => 2,
    }
}

pub(crate) fn write_sources(w: &mut Writer, sources: &[SourceFile]) {
    w.len(sources.len());
    for s in sources {
        w.str(&s.path);
        w.u8(kind_code(s.kind));
        w.u64(s.size);
        w.u8(u8::from(s.stat.is_some()));
        let stat = s.stat.unwrap_or_default();
        w.u64(stat.dev);
        w.u64(stat.ino);
        w.i64(stat.mtime.0);
        w.u32(stat.mtime.1);
        w.i64(stat.ctime.0);
        w.u32(stat.ctime.1);
        w.u8(u8::from(s.hash.is_some()));
        w.hash(&s.hash.unwrap_or_default());
    }
}

pub(crate) fn read_sources(r: &mut Reader<'_>) -> Result<Vec<SourceFile>, Corrupt> {
    let n = r.len()?;
    let mut out = Vec::with_capacity(n.min(1 << 16));
    for _ in 0..n {
        let path = r.string()?;
        let kind = match r.u8()? {
            0 => SourceKind::Content,
            1 => SourceKind::Absent,
            2 => SourceKind::Present,
            v => return Err(format!("invalid source kind {v}")),
        };
        let size = r.u64()?;
        let has_stat = r.bool()?;
        let stat = StatFingerprint {
            dev: r.u64()?,
            ino: r.u64()?,
            mtime: (r.i64()?, r.u32()?),
            ctime: (r.i64()?, r.u32()?),
        };
        let has_hash = r.bool()?;
        let hash = r.hash()?;
        if (kind == SourceKind::Content) != has_hash {
            return Err(format!("{path}: inconsistent source record"));
        }
        out.push(SourceFile {
            path,
            kind,
            size,
            stat: has_stat.then_some(stat),
            hash: has_hash.then_some(hash),
        });
    }
    Ok(out)
}

/// Combined hash of the recorded contents (shown by `fasm-db-cache info`).
pub(crate) fn sources_hash(listing: &Hash, sources: &[SourceFile]) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(listing);
    for s in sources {
        hasher.update(&(s.path.len() as u64).to_le_bytes());
        hasher.update(s.path.as_bytes());
        hasher.update(&[kind_code(s.kind)]);
        hasher.update(&s.size.to_le_bytes());
        hasher.update(&s.hash.unwrap_or_default());
    }
    *hasher.finalize().as_bytes()
}

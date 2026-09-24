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

//! Tests of the binary database cache on the miniature test databases.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use super::sources::{is_racy, RACY_WINDOW};
use super::*;

fn testdata(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join(name)
}

/// A temporary directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "fasm-xilinx-cache-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A private copy of a test database (so that it can be modified).
fn copy_db(name: &str) -> TempDir {
    let dir = TempDir::new(name);
    copy_tree(&testdata(name), &dir.path().join(name));
    dir
}

const MINI_PART: &str = "xc7";
const SYNTHETIC_PART: &str = "xc7test-1";

fn open_with(root: &Path, part: &str, dir: &Path) -> (Database, CacheOutcome) {
    open(root, Some(part), &CacheOptions::in_directory(dir)).unwrap()
}

fn is_hit(outcome: &CacheOutcome) -> bool {
    matches!(outcome, CacheOutcome::Hit { .. })
}

fn rebuild_reason(outcome: &CacheOutcome) -> String {
    match outcome {
        CacheOutcome::Rebuilt {
            reason,
            write_error: None,
            ..
        } => reason.clone(),
        other => panic!("expected a rebuild, got {other:?}"),
    }
}

/// Opens through the cache and checks the result against the text
/// loader; returns the outcome.
fn open_checked(root: &Path, part: &str, dir: &Path) -> CacheOutcome {
    let (db, outcome) = open_with(root, part, dir);
    let expected = Database::open(root, Some(part)).unwrap();
    assert_databases_equal(&db, &expected);
    outcome
}

/// Field by field (for a readable failure), then `==`.
fn assert_databases_equal(a: &Database, b: &Database) {
    assert_eq!(a.root, b.root);
    assert_eq!(a.layout, b.layout);
    assert_eq!(a.architecture, b.architecture);
    assert_eq!(a.tile_types.len(), b.tile_types.len());
    for (x, y) in a.tile_types.iter().zip(&b.tile_types) {
        assert_eq!(x.name, y.name);
        assert_eq!(x.files, y.files);
        let (s, t) = (&x.segbits, &y.segbits);
        assert_eq!(s.entries, t.entries, "{}", x.name);
        assert_eq!(s.bits, t.bits, "{}", x.name);
        assert_eq!(s.by_name, t.by_name, "{}", x.name);
        assert_eq!(s.addressed, t.addressed, "{}", x.name);
        assert_eq!(s.ppips, t.ppips, "{}", x.name);
        assert_eq!(s.ppip_index, t.ppip_index, "{}", x.name);
        assert_eq!(s.foreign_lines, t.foreign_lines, "{}", x.name);
    }
    assert_eq!(a.tile_type_index, b.tile_type_index);
    match (&a.grid, &b.grid) {
        (Some(g), Some(h)) => {
            assert_eq!(g.tiles, h.tiles);
            assert_eq!(g.by_name, h.by_name);
            assert_eq!(g.by_loc, h.by_loc);
            assert_eq!(g.bits, h.bits);
            assert_eq!(g.aliases, h.aliases);
            assert_eq!(g.pairs, h.pairs);
            assert_eq!(g.names, h.names);
        }
        (g, h) => assert_eq!(g.is_some(), h.is_some()),
    }
    assert_eq!(a.part, b.part);
    assert_eq!(a.banks, b.banks);
    assert!(a == b);
}

#[test]
fn round_trip_of_every_field() {
    for (name, part) in [("mini-db", MINI_PART), ("synthetic-db", SYNTHETIC_PART)] {
        let root = testdata(name);
        let db = Database::open(&root, Some(part)).unwrap();
        let payload = format::encode_payload(&db);
        for parallel in [false, true] {
            let decoded =
                format::decode_payload(&payload, &root, db.layout, db.architecture, parallel)
                    .unwrap();
            assert_databases_equal(&decoded, &db);
        }
        // Without a part.
        let db = Database::open(&root, None).unwrap();
        let payload = format::encode_payload(&db);
        let decoded =
            format::decode_payload(&payload, &root, db.layout, db.architecture, true).unwrap();
        assert_databases_equal(&decoded, &db);
    }
}

#[test]
fn second_open_is_a_hit() {
    for (name, part) in [("mini-db", MINI_PART), ("synthetic-db", SYNTHETIC_PART)] {
        let cache = TempDir::new("hit");
        let root = testdata(name);
        let reason = rebuild_reason(&open_checked(&root, part, cache.path()));
        assert_eq!(reason, "no cache file");
        for _ in 0..2 {
            let outcome = open_checked(&root, part, cache.path());
            assert_eq!(
                outcome,
                CacheOutcome::Hit {
                    path: cache_file(cache.path(), &root, part).unwrap(),
                    restat: false
                }
            );
        }
        let files = cache_files(cache.path()).unwrap();
        assert_eq!(files.len(), 1);
        let name = files[0].file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with(&format!("prjxray-{part}-")), "{name}");
        assert!(
            name.ends_with(&format!("-v{FORMAT_VERSION}.fasmxdb")),
            "{name}"
        );
        // Nothing else (no temporary file) is left behind.
        assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 1);
    }
}

#[test]
fn open_cached_is_open() {
    let cache = TempDir::new("api");
    let root = testdata("synthetic-db");
    let options = CacheOptions::in_directory(cache.path());
    for _ in 0..2 {
        let db = Database::open_cached(&root, Some(SYNTHETIC_PART), &options).unwrap();
        assert!(db == Database::open(&root, Some(SYNTHETIC_PART)).unwrap());
    }
    // Disabled cache, or no part: the plain loader, nothing written.
    let (_, outcome) = open(&root, Some(SYNTHETIC_PART), &CacheOptions::disabled()).unwrap();
    assert_eq!(outcome, CacheOutcome::Disabled);
    let (db, outcome) = open(&root, None, &options).unwrap();
    assert_eq!(outcome, CacheOutcome::Disabled);
    assert!(db == Database::open(&root, None).unwrap());
    assert_eq!(cache_files(cache.path()).unwrap().len(), 1);
}

#[test]
fn loader_errors_are_unchanged() {
    let cache = TempDir::new("errors");
    let options = CacheOptions::in_directory(cache.path());
    let root = testdata("synthetic-db");
    let cases: [(PathBuf, &str); 4] = [
        (root.clone(), "xc7nosuchpart"),
        // Listed, but its device is not in devices.yaml.
        (root.clone(), "xc7nodev-1"),
        (testdata("mini-db-golden"), "xc7"),
        (cache.path().join("missing"), "xc7"),
    ];
    for (root, part) in cases {
        let plain = Database::open(&root, Some(part)).unwrap_err().to_string();
        let cached = Database::open_cached(&root, Some(part), &options)
            .unwrap_err()
            .to_string();
        assert_eq!(plain, cached);
    }
    assert!(cache_files(cache.path()).unwrap().is_empty());
}

/// Writes a cache file for the mini database and returns its bytes.
fn mini_cache(cache: &TempDir) -> (PathBuf, PathBuf, Vec<u8>) {
    let root = testdata("mini-db");
    open_with(&root, MINI_PART, cache.path());
    let file = cache_file(cache.path(), &root, MINI_PART).unwrap();
    let data = std::fs::read(&file).unwrap();
    (root, file, data)
}

#[test]
fn wrong_magic_version_or_length_is_rebuilt() {
    let cache = TempDir::new("header");
    let (root, file, data) = mini_cache(&cache);
    let mut cases: Vec<(Vec<u8>, &str)> = Vec::new();
    let mut bad = data.clone();
    bad[0] = b'X';
    cases.push((bad, "wrong magic"));
    let mut bad = data.clone();
    bad[8..12].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
    cases.push((bad, "format version"));
    cases.push((Vec::new(), "truncated"));
    cases.push((data[..PREFIX_LEN - 1].to_vec(), "truncated"));
    cases.push((data[..data.len() - 1].to_vec(), "lengths do not match"));
    let mut longer = data.clone();
    longer.push(0);
    cases.push((longer, "lengths do not match"));
    for (bytes, expected) in cases {
        std::fs::write(&file, &bytes).unwrap();
        let reason = rebuild_reason(&open_checked(&root, MINI_PART, cache.path()));
        assert!(reason.contains(expected), "{reason:?} vs {expected:?}");
        // The rebuilt file is valid again.
        assert_eq!(std::fs::read(&file).unwrap().len(), data.len());
        assert!(is_hit(&open_checked(&root, MINI_PART, cache.path())));
    }
}

#[test]
fn any_flipped_byte_is_detected() {
    let cache = TempDir::new("flip");
    let (_, _, data) = mini_cache(&cache);
    // Every position, without touching the files: the checks done before
    // decoding must reject it.
    for i in 0..data.len() {
        for mask in [0x01, 0x80] {
            let mut bad = data.clone();
            bad[i] ^= mask;
            let accepted = parse(&bad).is_ok_and(|p| format::hash(p.payload) == p.payload_hash);
            assert!(
                !accepted,
                "flipping bit {mask:#x} of byte {i} went unnoticed"
            );
        }
    }
}

#[test]
fn flipped_bytes_are_rebuilt_end_to_end() {
    let cache = TempDir::new("flip-open");
    let (root, file, data) = mini_cache(&cache);
    let header_end = PREFIX_LEN + u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
    // All of the prefix, some of the header and payload.
    let positions = (0..PREFIX_LEN)
        .chain((PREFIX_LEN..header_end).step_by(97))
        .chain((header_end..data.len()).step_by(251))
        .chain([data.len() - 1]);
    for i in positions {
        let mut bad = data.clone();
        bad[i] ^= 0x10;
        std::fs::write(&file, &bad).unwrap();
        let outcome = open_checked(&root, MINI_PART, cache.path());
        rebuild_reason(&outcome);
        assert_eq!(std::fs::read(&file).unwrap().len(), data.len());
    }
}

/// A cache file with `edit` applied to its header and valid hashes.
fn with_header(data: &[u8], edit: impl FnOnce(&mut CacheInfo)) -> Vec<u8> {
    let parsed = parse(data).unwrap();
    let mut info = parsed.info.clone();
    edit(&mut info);
    let header = encode_header(&info);
    let mut out = prefix_and_header(&header, parsed.payload);
    out.extend_from_slice(parsed.payload);
    out
}

#[test]
fn corrupt_payload_with_valid_hashes_is_an_error_not_a_panic() {
    let root = testdata("synthetic-db");
    let db = Database::open(&root, Some(SYNTHETIC_PART)).unwrap();
    let payload = format::encode_payload(&db);
    let decode =
        |p: &[u8], parallel| format::decode_payload(p, &root, db.layout, db.architecture, parallel);
    assert!(decode(&payload, false).unwrap() == db);
    let mut errors = 0;
    for i in 0..payload.len() {
        for value in [0x00, 0x01, 0xff, payload[i] ^ 0x04] {
            let mut bad = payload.clone();
            bad[i] = value;
            match decode(&bad, i % 2 == 0) {
                Ok(_) => {}
                Err(_) => errors += 1,
            }
        }
    }
    assert!(errors > 0);
    for len in 0..payload.len() {
        assert!(decode(&payload[..len], len % 2 == 0).is_err());
    }
}

#[test]
fn changed_sources_are_rebuilt() {
    let copy = copy_db("synthetic-db");
    let root = copy.path().join("synthetic-db");
    let cache = TempDir::new("stale");
    let dir = cache.path();
    rebuild_reason(&open_checked(&root, SYNTHETIC_PART, dir));
    assert!(is_hit(&open_checked(&root, SYNTHETIC_PART, dir)));

    // A segbits file with another size.
    let segbits = root.join("segbits_int_l.db");
    let text = std::fs::read_to_string(&segbits).unwrap();
    std::fs::write(&segbits, format!("{text}INT_L.NEW_FEATURE 01_02\n")).unwrap();
    let reason = rebuild_reason(&open_checked(&root, SYNTHETIC_PART, dir));
    assert!(reason.contains("segbits_int_l.db changed size"), "{reason}");
    assert!(is_hit(&open_checked(&root, SYNTHETIC_PART, dir)));

    // Same size, modification time restored: the status change time
    // (or, without one, the content hash) still catches it.
    let meta = std::fs::metadata(&segbits).unwrap();
    let mtime = meta.modified().unwrap();
    let text = std::fs::read_to_string(&segbits).unwrap();
    std::fs::write(&segbits, text.replace("01_02", "01_03")).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&segbits)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    let reason = rebuild_reason(&open_checked(&root, SYNTHETIC_PART, dir));
    assert!(reason.contains("segbits_int_l.db changed"), "{reason}");

    // The tilegrid (in the fabric directory).
    let tilegrid = root.join("xc7testfab/tilegrid.json");
    let text = std::fs::read_to_string(&tilegrid).unwrap();
    std::fs::write(&tilegrid, text.replace("\"grid_x\": 1,", "\"grid_x\": 11,")).unwrap();
    let reason = rebuild_reason(&open_checked(&root, SYNTHETIC_PART, dir));
    assert!(reason.contains("tilegrid.json"), "{reason}");

    // An optional file that did not exist appears.
    std::fs::write(root.join("ppips_hclk_l.db"), "HCLK_L.X.Y always\n").unwrap();
    let reason = rebuild_reason(&open_checked(&root, SYNTHETIC_PART, dir));
    assert!(reason.contains("ppips_hclk_l.db appeared"), "{reason}");

    // A probed-only file (mask) disappears.
    std::fs::remove_file(root.join("mask_bram_l.db")).unwrap();
    let reason = rebuild_reason(&open_checked(&root, SYNTHETIC_PART, dir));
    assert!(reason.contains("mask_bram_l.db disappeared"), "{reason}");

    // A part file disappears.
    std::fs::remove_file(root.join("xc7test-1/required_features.fasm")).unwrap();
    let reason = rebuild_reason(&open_checked(&root, SYNTHETIC_PART, dir));
    assert!(
        reason.contains("required_features.fasm disappeared"),
        "{reason}"
    );

    // A new tile type.
    std::fs::write(root.join("tile_type_NEWTYPE.json"), "{}\n").unwrap();
    let reason = rebuild_reason(&open_checked(&root, SYNTHETIC_PART, dir));
    assert!(reason.contains("tile type list changed"), "{reason}");

    // The part moves to another fabric.
    let devices = root.join("mapping/devices.yaml");
    let text = std::fs::read_to_string(&devices).unwrap();
    copy_tree(&root.join("xc7testfab"), &root.join("xc7testfab2"));
    std::fs::write(&devices, text.replace("xc7testfab", "xc7testfab2")).unwrap();
    let reason = rebuild_reason(&open_checked(&root, SYNTHETIC_PART, dir));
    assert!(reason.contains("devices.yaml"), "{reason}");
    assert!(is_hit(&open_checked(&root, SYNTHETIC_PART, dir)));
    assert_eq!(cache_files(dir).unwrap().len(), 1);
}

#[test]
fn touched_sources_are_rehashed_not_rebuilt() {
    let copy = copy_db("mini-db");
    let root = copy.path().join("mini-db");
    let cache = TempDir::new("touch");
    let dir = cache.path();
    rebuild_reason(&open_checked(&root, MINI_PART, dir));
    let file = cache_file(dir, &root, MINI_PART).unwrap();
    let before = read_info(&file).unwrap();

    // Same content, another modification time (like a fresh checkout).
    let path = root.join("xc7/tilegrid.json");
    let earlier = SystemTime::now() - Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(earlier)
        .unwrap();
    let outcome = open_checked(&root, MINI_PART, dir);
    if cfg!(unix) {
        assert_eq!(
            outcome,
            CacheOutcome::Hit {
                path: file.clone(),
                restat: true
            }
        );
        let after = read_info(&file).unwrap();
        assert_ne!(before.sources, after.sources);
        assert_eq!(before.source_hash(), after.source_hash());
        assert_eq!(
            open_checked(&root, MINI_PART, dir),
            CacheOutcome::Hit {
                path: file,
                restat: false
            }
        );
    } else {
        assert!(is_hit(&outcome));
    }
}

#[test]
fn header_identity_is_checked() {
    let cache = TempDir::new("identity");
    let (root, file, data) = mini_cache(&cache);
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (
            with_header(&data, |i| i.loader_fingerprint = "0000".to_owned()),
            "another loader build",
        ),
        (
            with_header(&data, |i| i.part = "xc7other".to_owned()),
            "file is for",
        ),
        (
            with_header(&data, |i| i.db_root = PathBuf::from("/elsewhere")),
            "file is for",
        ),
        (
            with_header(&data, |i| i.layout = Layout::Prjuray),
            "file is for",
        ),
        (
            with_header(&data, |i| i.listing_hash = [0; 32]),
            "tile type list changed",
        ),
        (
            with_header(&data, |i| {
                let s = i
                    .sources
                    .iter_mut()
                    .find(|s| s.kind == SourceKind::Content)
                    .unwrap();
                s.size += 1;
            }),
            "changed size",
        ),
    ];
    for (bytes, expected) in cases {
        std::fs::write(&file, &bytes).unwrap();
        let reason = rebuild_reason(&open_checked(&root, MINI_PART, cache.path()));
        assert!(reason.contains(expected), "{reason:?} vs {expected:?}");
    }
}

#[test]
fn verify_hashes_every_source() {
    let cache = TempDir::new("verify");
    let (root, file, data) = mini_cache(&cache);
    let report = verify_file(&file);
    assert_eq!(report.problem, None);
    let info = report.info.unwrap();
    assert_eq!(info.part, MINI_PART);
    assert_eq!(info.db_root, std::fs::canonicalize(&root).unwrap());
    assert_eq!(info.file_len, data.len() as u64);

    // A recorded hash that does not match while the stat fingerprints
    // do: the fast path trusts the fingerprints, `verify_file` does not.
    let tampered = with_header(&data, |i| {
        let s = i
            .sources
            .iter_mut()
            .find(|s| s.path.ends_with("tilegrid.json"))
            .unwrap();
        s.hash = Some([7; 32]);
    });
    std::fs::write(&file, &tampered).unwrap();
    let problem = verify_file(&file).problem.unwrap();
    assert!(problem.contains("tilegrid.json changed"), "{problem}");
    if cfg!(unix) {
        assert!(is_hit(&open_with(&root, MINI_PART, cache.path()).1));
    }

    // Corruption.
    let mut bad = data.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    std::fs::write(&file, &bad).unwrap();
    let problem = verify_file(&file).problem.unwrap();
    assert!(problem.contains("corrupt payload"), "{problem}");
    std::fs::write(&file, b"garbage").unwrap();
    let report = verify_file(&file);
    assert!(report.info.is_none());
    assert!(report.problem.unwrap().contains("truncated"));

    // Misnamed.
    std::fs::write(&file, &data).unwrap();
    let other = cache.path().join("other.fasmxdb");
    std::fs::copy(&file, &other).unwrap();
    let problem = verify_file(&other).problem.unwrap();
    assert!(problem.contains("misnamed"), "{problem}");
}

#[test]
fn unwritable_cache_directory_is_not_an_error() {
    let cache = TempDir::new("unwritable");
    // A file where the directory should be.
    let dir = cache.path().join("not-a-directory");
    std::fs::write(&dir, b"").unwrap();
    let root = testdata("mini-db");
    let (db, outcome) = open_with(&root, MINI_PART, &dir);
    assert!(db == Database::open(&root, Some(MINI_PART)).unwrap());
    match outcome {
        CacheOutcome::Rebuilt {
            write_error: Some(_),
            ..
        } => {}
        other => panic!("{other:?}"),
    }
    assert!(build(&dir, &root, MINI_PART).is_err());
}

#[test]
fn concurrent_opens() {
    let cache = TempDir::new("concurrent");
    let root = testdata("synthetic-db");
    let expected = Database::open(&root, Some(SYNTHETIC_PART)).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for _ in 0..5 {
                    let (db, _) = open_with(&root, SYNTHETIC_PART, cache.path());
                    assert!(db == expected);
                }
            });
        }
    });
    // One file, no temporary files left.
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 1);
    assert_eq!(
        verify_file(&cache_files(cache.path()).unwrap()[0]).problem,
        None
    );
}

#[test]
fn different_roots_get_different_files() {
    let a = copy_db("mini-db");
    let b = copy_db("mini-db");
    let cache = TempDir::new("roots");
    let root_a = a.path().join("mini-db");
    let root_b = b.path().join("mini-db");
    rebuild_reason(&open_checked(&root_a, MINI_PART, cache.path()));
    rebuild_reason(&open_checked(&root_b, MINI_PART, cache.path()));
    assert!(is_hit(&open_checked(&root_a, MINI_PART, cache.path())));
    assert_eq!(cache_files(cache.path()).unwrap().len(), 2);
    // The same root under another spelling is the same file.
    let dotted = a.path().join(".").join("mini-db");
    assert!(is_hit(&open_checked(&dotted, MINI_PART, cache.path())));
}

/// A miniature prjuray-db family (`tile_types/`, the grid in the part
/// directory) made from the synthetic database.
fn prjuray_db() -> TempDir {
    let dir = TempDir::new("prjuray");
    let src = testdata("synthetic-db");
    let root = dir.path().join("zynqtest");
    std::fs::create_dir_all(root.join("tile_types")).unwrap();
    std::fs::create_dir_all(root.join("xczutest")).unwrap();
    for entry in std::fs::read_dir(&src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if name.starts_with("tile_type_") {
            std::fs::copy(entry.path(), root.join("tile_types").join(&name)).unwrap();
        } else if name.ends_with(".db") {
            std::fs::copy(entry.path(), root.join(&name)).unwrap();
        }
    }
    std::fs::copy(
        src.join("xc7testfab/tilegrid.json"),
        root.join("xczutest/tilegrid.json"),
    )
    .unwrap();
    dir
}

#[test]
fn prjuray_layout() {
    let db_dir = prjuray_db();
    let root = db_dir.path().join("zynqtest");
    let cache = TempDir::new("prjuray-cache");
    rebuild_reason(&open_checked(&root, "xczutest", cache.path()));
    let outcome = open_checked(&root, "xczutest", cache.path());
    assert!(is_hit(&outcome));
    let file = cache_file(cache.path(), &root, "xczutest").unwrap();
    let info = read_info(&file).unwrap();
    assert_eq!(info.layout, Layout::Prjuray);
    assert_eq!(info.architecture, Architecture::UltraScalePlus);
    assert!(info
        .sources
        .iter()
        .any(|s| s.path == "xczutest/tilegrid.json" && s.kind == SourceKind::Content));
    assert_eq!(family_parts(&root).unwrap(), ["xczutest"]);

    // A new tile type in tile_types/.
    std::fs::write(root.join("tile_types/tile_type_OTHER.json"), "{}").unwrap();
    let reason = rebuild_reason(&open_checked(&root, "xczutest", cache.path()));
    assert!(reason.contains("tile type list"), "{reason}");
}

#[test]
fn maintenance_functions() {
    let cache = TempDir::new("maintenance");
    let dir = cache.path();
    assert!(cache_files(&dir.join("missing")).unwrap().is_empty());
    assert_eq!(clear(&dir.join("missing")).unwrap(), 0);

    let root = testdata("synthetic-db");
    let info = build(dir, &root, SYNTHETIC_PART).unwrap();
    assert_eq!(info.part, SYNTHETIC_PART);
    assert_eq!(info.architecture, Architecture::Series7);
    assert_eq!(info.loader_fingerprint, LOADER_FINGERPRINT);
    // Every file the loader reads is recorded.
    let paths: Vec<&str> = info.sources.iter().map(|s| s.path.as_str()).collect();
    for expected in [
        "mapping/parts.yaml",
        "mapping/devices.yaml",
        "xc7testfab/tilegrid.json",
        "xc7test-1/part.yaml",
        "xc7test-1/part.json",
        "xc7test-1/package_pins.csv",
        "xc7test-1/required_features.fasm",
        "segbits_bram_l.db",
        "segbits_bram_l.block_ram.db",
        "ppips_int_l.db",
        "mask_bram_l.db",
    ] {
        assert!(paths.contains(&expected), "{expected} not in {paths:?}");
    }
    let kind = |path: &str| info.sources.iter().find(|s| s.path == path).unwrap().kind;
    assert_eq!(kind("mask_bram_l.db"), SourceKind::Present);
    assert_eq!(kind("mask_int_l.db"), SourceKind::Absent);
    assert_eq!(kind("segbits_nosegbits.db"), SourceKind::Absent);
    assert_eq!(kind("segbits_int_l.db"), SourceKind::Content);
    // A build is a rebuild: the file is replaced.
    std::thread::sleep(Duration::from_millis(5));
    build(dir, &root, SYNTHETIC_PART).unwrap();
    assert!(is_hit(&open_with(&root, SYNTHETIC_PART, dir).1));

    build(dir, &testdata("mini-db"), MINI_PART).unwrap();
    // A leftover temporary file and an unrelated file.
    std::fs::write(dir.join(".prjxray-x-0-v1.fasmxdb.1.2.tmp"), b"").unwrap();
    std::fs::write(dir.join("README"), b"").unwrap();
    assert_eq!(cache_files(dir).unwrap().len(), 2);
    assert_eq!(clear(dir).unwrap(), 3);
    assert!(cache_files(dir).unwrap().is_empty());
    assert!(dir.join("README").exists());

    assert_eq!(
        family_parts(&root).unwrap(),
        ["xc7test-1", "xc7nodev-1"].map(String::from)
    );
    assert_eq!(family_parts(&testdata("mini-db")).unwrap(), ["xc7"]);
    assert!(family_parts(&testdata("mini-db-golden")).is_err());
    assert!(matches!(
        build(dir, &testdata("mini-db-golden"), "xc7"),
        Err(CacheError::NotADatabase(_))
    ));
    assert!(matches!(
        build(dir, &root, "xc7nosuchpart"),
        Err(CacheError::Db(DbError::UnknownPart { .. }))
    ));
}

#[test]
fn options_from_environment() {
    let vars = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| OsString::from(v))
        }
    };
    let dir = |o: CacheOptions| o.directory;
    assert_eq!(
        dir(CacheOptions::from_vars(vars(&[("FASM_XDB_CACHE", "/c")]))),
        Some(PathBuf::from("/c"))
    );
    for off in ["0", ""] {
        let pairs: &'static [(&str, &str)] = if off.is_empty() {
            &[("FASM_XDB_CACHE", ""), ("HOME", "/home/u")]
        } else {
            &[("FASM_XDB_CACHE", "0"), ("HOME", "/home/u")]
        };
        assert_eq!(dir(CacheOptions::from_vars(vars(pairs))), None);
    }
    assert_eq!(
        dir(CacheOptions::from_vars(vars(&[
            ("XDG_CACHE_HOME", "/xdg"),
            ("HOME", "/home/u")
        ]))),
        Some(PathBuf::from("/xdg/fasm/db"))
    );
    // A relative XDG_CACHE_HOME is ignored (XDG base directory spec).
    assert_eq!(
        dir(CacheOptions::from_vars(vars(&[
            ("XDG_CACHE_HOME", "rel"),
            ("HOME", "/home/u")
        ]))),
        Some(PathBuf::from("/home/u/.cache/fasm/db"))
    );
    assert_eq!(
        dir(CacheOptions::from_vars(vars(&[("HOME", "/home/u")]))),
        Some(PathBuf::from("/home/u/.cache/fasm/db"))
    );
    assert_eq!(dir(CacheOptions::from_vars(vars(&[]))), None);
    assert!(!CacheOptions::from_vars(vars(&[("FASM_XDB_CACHE_VERBOSE", "0")])).verbose);
    assert!(CacheOptions::from_vars(vars(&[("FASM_XDB_CACHE_VERBOSE", "1")])).verbose);
    assert!(!CacheOptions::from_vars(vars(&[])).verbose);
    assert_eq!(CacheOptions::default(), CacheOptions::disabled());
}

#[test]
fn recent_stat_fingerprints_are_not_trusted() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let window = Duration::from_secs(5);
    let stat = |mtime: i64, ctime: i64| StatFingerprint {
        mtime: (mtime, 500),
        ctime: (ctime, 0),
        ..StatFingerprint::default()
    };
    assert!(!is_racy(&stat(900_000, 999_994), now, window));
    assert!(is_racy(&stat(999_995, 900_000), now, window));
    assert!(is_racy(&stat(900_000, 999_995), now, window));
    assert!(is_racy(&stat(2_000_000, 900_000), now, window), "future");
    assert!(!is_racy(&stat(999_999, 999_999), now, Duration::ZERO));
    assert!(is_racy(&stat(1_000_000, 0), now, Duration::ZERO));

    // Built right after the files were written: no stat fingerprint is
    // recorded, every load hashes the files (and records the
    // fingerprints once they are old enough).
    let root = testdata("mini-db");
    let mut sources = Plan::new(&root, MINI_PART)
        .unwrap()
        .fingerprint(&root, false)
        .unwrap();
    let written = sources.iter().filter(|s| s.stat.is_some()).count();
    if cfg!(unix) {
        assert!(written > 0);
    }
    sources::drop_racy(&mut sources, SystemTime::now());
    assert_eq!(sources.iter().filter(|s| s.stat.is_some()).count(), written);
    let mut far_future = sources.clone();
    let future = SystemTime::now() - Duration::from_secs(1) + RACY_WINDOW;
    for s in &mut far_future {
        if let Some(stat) = &mut s.stat {
            stat.ctime = (i64::MAX / 2, 0);
        }
    }
    sources::drop_racy(&mut far_future, future);
    assert!(far_future.iter().all(|s| s.stat.is_none()));
}

#[test]
fn file_names() {
    let root = Path::new("/db/artix7");
    let name = cache_file_name(Layout::Prjxray, "xc7a35tcsg324-1", root);
    assert!(name.starts_with("prjxray-xc7a35tcsg324-1-"), "{name}");
    assert!(name.ends_with("-v1.fasmxdb"), "{name}");
    assert_eq!(
        name,
        cache_file_name(Layout::Prjxray, "xc7a35tcsg324-1", root)
    );
    assert_ne!(
        name,
        cache_file_name(Layout::Prjxray, "xc7a35tcsg324-1", Path::new("/db/other"))
    );
    let odd = cache_file_name(Layout::Prjuray, "a/b c", root);
    assert!(odd.starts_with("prjuray-a_b_c-"), "{odd}");
}

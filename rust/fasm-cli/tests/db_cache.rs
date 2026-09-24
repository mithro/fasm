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

//! The `fasm2frames` and `xcfasm` binaries give byte identical results
//! (output files, stdout, stderr, exit code) without the database cache
//! (`FASM_XDB_CACHE=0`), when they write it and when they load it; and
//! the `fasm-db-cache` maintenance tool.
//!
//! Runs on the miniature test databases; the prjxray-db `artix7` cases
//! (`tools/fetch-db.sh prjxray artix7`, or `FASM_DB_CACHE`) are skipped
//! with a message when it is not present.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn testdata(name: &str) -> PathBuf {
    repo_root().join("rust/fasm-xilinx/testdata").join(name)
}

fn artix7() -> Option<PathBuf> {
    let cache = std::env::var_os("FASM_DB_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("tests/oracle/build/db"));
    let path = cache.join("prjxray-db/artix7");
    if path.is_dir() {
        Some(path)
    } else {
        eprintln!("skipping: {} not found", path.display());
        None
    }
}

/// A temporary directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "fasm-cli-db-cache-{tag}-{}-{}",
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

/// Everything a run produces.
#[derive(Debug, PartialEq, Eq)]
struct Run {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    /// The contents of the output files (`None`: not written).
    outputs: Vec<Option<Vec<u8>>>,
}

/// Runs `exe` with `args` in `work` (output files `outputs`, removed
/// first), with `FASM_XDB_CACHE=cache`.
fn run(exe: &str, args: &[&str], work: &Path, outputs: &[&str], cache: &str, verbose: bool) -> Run {
    for out in outputs {
        let _ = std::fs::remove_file(work.join(out));
    }
    let mut command = Command::new(exe);
    command
        .args(args)
        .current_dir(work)
        .env("FASM_XDB_CACHE", cache)
        .env("SOURCE_DATE_EPOCH", "1790214296")
        .env_remove("FASM_XDB_CACHE_VERBOSE")
        .env_remove("XRAY_DATABASE_DIR")
        .env_remove("XRAY_DATABASE")
        .env_remove("XRAY_PART");
    if verbose {
        command.env("FASM_XDB_CACHE_VERBOSE", "1");
    }
    let out = command.output().unwrap();
    Run {
        code: out.status.code(),
        stdout: out.stdout,
        stderr: out.stderr,
        outputs: outputs
            .iter()
            .map(|o| std::fs::read(work.join(o)).ok())
            .collect(),
    }
}

/// Runs a command line without the cache, then twice with a fresh cache
/// directory (writing, then loading the cache file), and checks that all
/// three runs are identical; returns the uncached run.
fn check_identical(exe: &str, args: &[&str], work: &Path, outputs: &[&str]) -> Run {
    let cache = TempDir::new("cache");
    let cache_dir = cache.path().to_str().unwrap();
    let plain = run(exe, args, work, outputs, "0", false);
    let writing = run(exe, args, work, outputs, cache_dir, false);
    assert_eq!(plain, writing, "{exe} {args:?}: writing the cache");
    let loading = run(exe, args, work, outputs, cache_dir, false);
    assert_eq!(plain, loading, "{exe} {args:?}: loading the cache");
    // The last run really used the cache file (when the database could be
    // opened at all).
    let verbose = run(exe, args, work, outputs, cache_dir, true);
    let stderr = String::from_utf8_lossy(&verbose.stderr);
    let files = std::fs::read_dir(cache.path()).unwrap().count();
    if files > 0 {
        assert_eq!(files, 1);
        assert!(
            stderr.contains("fasm-xilinx db cache: loaded"),
            "{exe} {args:?}: {stderr}"
        );
    }
    plain
}

const FASM2FRAMES: &str = env!("CARGO_BIN_EXE_fasm2frames");
const XCFASM: &str = env!("CARGO_BIN_EXE_xcfasm");
const DB_CACHE: &str = env!("CARGO_BIN_EXE_fasm-db-cache");

#[test]
fn fasm2frames_mini_db() {
    let work = TempDir::new("mini");
    let root = testdata("mini-db");
    let root = root.to_str().unwrap();
    let corpus = repo_root().join("tests/corpus/f4pga-xc-fasm");
    let mut ok = 0;
    for fasm in [
        "lut_int.fasm",
        "ff_int.fasm",
        "ff_int_0s.fasm",
        "lut.fasm",
        "iob/liob_stepdown.fasm",
        "iob/riob_stepdown.fasm",
    ] {
        let fasm = corpus.join(fasm);
        let fasm = fasm.to_str().unwrap();
        for extra in [&[][..], &["--sparse"], &["--debug", "--emit_pudc_b_pullup"]] {
            let mut args = vec!["--db-root", root, "--part", "xc7"];
            args.extend_from_slice(extra);
            args.extend_from_slice(&[fasm, "out.frm"]);
            let run = check_identical(FASM2FRAMES, &args, work.path(), &["out.frm"]);
            ok += usize::from(run.code == Some(0));
        }
    }
    assert!(ok > 0);
}

/// A few features of the synthetic database (with a BRAM bus and a
/// `part.yaml`).
const SYNTHETIC_FASM: &str = "INT_L_X6Y0.WW2BEG0.LOGIC_OUTS_L12\n\
     BRAM_L_X6Y0.RAMB18_Y0.INIT_00[1:0] = 2'b11\n";

#[test]
fn fasm2frames_and_xcfasm_synthetic_db() {
    let work = TempDir::new("synthetic");
    std::fs::write(work.path().join("ok.fasm"), SYNTHETIC_FASM).unwrap();
    std::fs::write(work.path().join("bad.fasm"), "INT_L_X6Y0.NO_SUCH_FEATURE\n").unwrap();
    let root = testdata("synthetic-db");
    let part_file = root.join("xc7test-1/part.yaml");
    let (root, part_file) = (root.to_str().unwrap(), part_file.to_str().unwrap());
    let base = ["--db-root", root, "--part", "xc7test-1"];

    let run = check_identical(
        FASM2FRAMES,
        &[&base[..], &["ok.fasm", "out.frm"]].concat(),
        work.path(),
        &["out.frm"],
    );
    assert_eq!(run.code, Some(0), "{run:?}");
    let run = check_identical(
        FASM2FRAMES,
        &[&base[..], &["bad.fasm", "out.frm"]].concat(),
        work.path(),
        &["out.frm"],
    );
    assert_eq!(run.code, Some(1));
    assert!(String::from_utf8_lossy(&run.stderr).contains("NO_SUCH_FEATURE"));

    let xcfasm_args = [
        &base[..],
        &[
            "--part_file",
            part_file,
            "--fn_in",
            "ok.fasm",
            "--bit_out",
            "out.bit",
            "--frm_out",
            "out.frm",
        ],
    ]
    .concat();
    let run = check_identical(XCFASM, &xcfasm_args, work.path(), &["out.frm", "out.bit"]);
    assert_eq!(run.code, Some(0), "{run:?}");
    assert!(run.outputs.iter().all(Option::is_some));

    // Database errors are reported identically.
    let run = check_identical(
        FASM2FRAMES,
        &[
            "--db-root",
            root,
            "--part",
            "xc7nosuchpart",
            "ok.fasm",
            "out.frm",
        ],
        work.path(),
        &["out.frm"],
    );
    assert_eq!(run.code, Some(1));
}

#[test]
fn artix7_counter_test() {
    let Some(db) = artix7() else {
        return;
    };
    let work = TempDir::new("artix7");
    let design = repo_root()
        .join("tests/corpus/xilinx/artix7/designs/f4pga-examples/counter_test/arty_35/top.fasm");
    let db = db.to_str().unwrap();
    let part = "xc7a35tcsg324-1";
    let part_file = format!("{db}/{part}/part.yaml");
    let design = design.to_str().unwrap();
    let base = ["--db-root", db, "--part", part];
    for extra in [&[][..], &["--sparse"]] {
        let run = check_identical(
            FASM2FRAMES,
            &[&base[..], extra, &[design, "out.frm"]].concat(),
            work.path(),
            &["out.frm"],
        );
        assert_eq!(run.code, Some(0), "{run:?}");
    }
    let run = check_identical(
        XCFASM,
        &[
            &base[..],
            &[
                "--part_file",
                &part_file,
                "--fn_in",
                design,
                "--bit_out",
                "out.bit",
                "--frm_out",
                "out.frm",
            ],
        ]
        .concat(),
        work.path(),
        &["out.frm", "out.bit"],
    );
    assert_eq!(run.code, Some(0), "{run:?}");
}

/// Runs `fasm-db-cache` with `args`.
fn db_cache(args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(DB_CACHE)
        .args(args)
        .env_remove("FASM_XDB_CACHE")
        .env_remove("FASM_XDB_CACHE_VERBOSE")
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn fasm_db_cache_tool() {
    let cache = TempDir::new("tool");
    let dir = cache.path().to_str().unwrap();
    let synthetic = testdata("synthetic-db");
    let synthetic = synthetic.to_str().unwrap();
    let mini = testdata("mini-db");
    let mini = mini.to_str().unwrap();

    // Usage errors: exit code 2.
    for args in [
        &[][..],
        &["frobnicate"],
        &["build"],
        &["build", synthetic],
        &["--cache-dir"],
        &["build", "--all"],
        &["build", "--all", synthetic, "xc7test-1"],
        &["clear", "extra"],
        &["--bogus", "list"],
    ] {
        let (code, _, stderr) = db_cache(&[&["--cache-dir", dir], args].concat());
        assert_eq!(code, Some(2), "{args:?}: {stderr}");
        assert!(
            stderr.contains("usage: fasm-db-cache"),
            "{args:?}: {stderr}"
        );
    }
    let (code, stdout, _) = db_cache(&["--help"]);
    assert_eq!(code, Some(0));
    assert!(stdout.contains("usage: fasm-db-cache"));

    // Empty cache.
    let (code, stdout, _) = db_cache(&["--cache-dir", dir, "list"]);
    assert_eq!((code, stdout.as_str()), (Some(0), ""));

    let (code, stdout, stderr) = db_cache(&["--cache-dir", dir, "build", synthetic, "xc7test-1"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains("xc7test-1"), "{stdout}");
    // `--all`: every part of parts.yaml; the one whose device is missing
    // fails (exit 1) but the others are built.
    let (code, stdout, stderr) = db_cache(&["--cache-dir", dir, "build", "--all", mini]);
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    let (code, _, stderr) = db_cache(&["--cache-dir", dir, "build", "--all", synthetic]);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("xc7nodev"), "{stderr}");
    let (code, _, stderr) = db_cache(&["--cache-dir", dir, "build", synthetic, "xc7nosuchpart"]);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("xc7nosuchpart"), "{stderr}");

    let (code, stdout, _) = db_cache(&["--cache-dir", dir, "list"]);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.lines().count(), 2, "{stdout}");
    let (code, stdout, _) = db_cache(&["--cache-dir", dir, "info"]);
    assert_eq!(code, Some(0));
    assert!(stdout.contains("sources:"), "{stdout}");
    assert!(stdout.contains("xc7testfab/tilegrid.json"), "{stdout}");

    let (code, stdout, stderr) = db_cache(&["--cache-dir", dir, "verify"]);
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    assert_eq!(stdout.matches(": ok").count(), 2, "{stdout}");

    // A corrupt file fails verification (exit 1).
    let file = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_str().unwrap().contains("xc7test-1"))
        .unwrap();
    let mut data = std::fs::read(&file).unwrap();
    let last = data.len() - 1;
    data[last] ^= 1;
    std::fs::write(&file, &data).unwrap();
    let (code, stdout, _) = db_cache(&["--cache-dir", dir, "verify"]);
    assert_eq!(code, Some(1));
    assert!(stdout.contains("corrupt payload"), "{stdout}");

    let (code, stdout, _) = db_cache(&["--cache-dir", dir, "clear"]);
    assert_eq!(code, Some(0));
    assert!(stdout.contains("removed 2"), "{stdout}");
    let (code, stdout, _) = db_cache(&["--cache-dir", dir, "list"]);
    assert_eq!((code, stdout.as_str()), (Some(0), ""));

    // The directory comes from FASM_XDB_CACHE by default, and `0`
    // disables the cache (nothing to do: exit 1 with a message).
    let out = Command::new(DB_CACHE)
        .args(["build", mini, "xc7"])
        .env("FASM_XDB_CACHE", dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1);
    let out = Command::new(DB_CACHE)
        .args(["list"])
        .env("FASM_XDB_CACHE", "0")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("disabled"));
    let out = Command::new(DB_CACHE)
        .args(["list"])
        .env_remove("FASM_XDB_CACHE")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("HOME")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("are unset"));
    // Explicit files need no cache directory.
    let file = std::fs::read_dir(dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    for command in ["verify", "info"] {
        let out = Command::new(DB_CACHE)
            .arg(command)
            .arg(&file)
            .env("FASM_XDB_CACHE", "0")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{command}");
    }
}

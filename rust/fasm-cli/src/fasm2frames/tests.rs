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

//! Tests of the `fasm2frames` tool. Expected outputs come from the
//! original tool (`tests/oracle/fasm2frames-oracle`, which runs as
//! `fasm2frames.py`); `tests/cli/test_fasm2frames_compat.py` and
//! `tools/difftest-xilinx.py` compare the two binaries directly.

use std::path::{Path, PathBuf};

use super::*;

struct Output {
    code: u8,
    stdout: String,
    stderr: String,
}

fn run_with(args: &[&str], env: &Environment) -> Output {
    let args: Vec<PyStr> = args.iter().map(|a| PyStr::from_str(a)).collect();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(
        "fasm2frames.py",
        &args,
        env,
        || 80,
        &mut stdout,
        &mut stderr,
    );
    Output {
        code,
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
    }
}

fn run_tool(args: &[&str]) -> Output {
    run_with(args, &Environment::default())
}

const HELP_80: &str = r#"usage: fasm2frames.py [-h] --db-root DB_ROOT --part PART [--sparse]
                      [--roi ROI] [--emit_pudc_b_pullup] [--debug]
                      fn_in [fn_out]

Convert FPGA configuration description ("FPGA assembly") into binary frame
equivalent

positional arguments:
  fn_in                 Input FPGA assembly (.fasm) file
  fn_out                Output FPGA frame (.frm) file

options:
  -h, --help            show this help message and exit
  --db-root DB_ROOT     Database root.
  --part PART           Part name. When not given defaults to XRAY_PART env.
                        var.
  --sparse              Don't zero fill all frames
  --roi ROI             ROI design.json file defining which tiles are within
                        the ROI.
  --emit_pudc_b_pullup  Emit an IBUF and PULLUP on the PUDC_B pin if unused
  --debug               Print debug dump
"#;

const USAGE_80: &str = "usage: fasm2frames.py [-h] --db-root DB_ROOT --part PART [--sparse]
                      [--roi ROI] [--emit_pudc_b_pullup] [--debug]
                      fn_in [fn_out]
";

#[test]
fn help() {
    for args in [
        &["-h"][..],
        &["--help"],
        &["--he"],
        &["a", "b", "-h", "--x"],
    ] {
        let out = run_tool(args);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr.as_str()),
            (0, HELP_80, "")
        );
    }
}

#[test]
fn usage_errors() {
    let error = |args: &[&str]| {
        let out = run_tool(args);
        assert_eq!(out.code, 2, "{args:?}");
        assert!(out.stdout.is_empty());
        let message = out.stderr.strip_prefix(USAGE_80).unwrap();
        message
            .strip_prefix("fasm2frames.py: error: ")
            .unwrap()
            .trim_end()
            .to_owned()
    };
    assert_eq!(
        error(&[]),
        "the following arguments are required: --db-root, --part, fn_in"
    );
    assert_eq!(
        error(&["--db-root", "d", "a"]),
        "the following arguments are required: --part"
    );
    assert_eq!(
        error(&["--d", "x"]),
        "ambiguous option: --d could match --db-root, --debug"
    );
    assert_eq!(error(&["--roi"]), "argument --roi: expected one argument");
    // argparse consumes both positionals (fn_out with zero arguments)
    // before the option, so the last argument is unrecognized.
    assert_eq!(
        error(&["--db-root", "d", "--part", "p", "a", "--sparse", "b"]),
        "unrecognized arguments: b"
    );
    assert_eq!(
        error(&["--db-root", "d", "--part", "p", "a", "b", "c"]),
        "unrecognized arguments: c"
    );
    assert_eq!(
        error(&["--db_root", "d", "--part", "p", "a"]),
        "the following arguments are required: --db-root"
    );
}

#[test]
fn arguments() {
    let parse = |args: &[&str], env: &Environment| {
        let args: Vec<PyStr> = args.iter().map(|a| PyStr::from_str(a)).collect();
        match parser("p", env).parse_args(&args) {
            Outcome::Run(values) => values,
            other => panic!("{other:?}"),
        }
    };
    let s = |v: &Values, d| v.str(d).map(|s| s.to_os_string().into_string().unwrap());
    let v = parse(
        &[
            "--db=/d", "--pa", "x", "--sp", "--e", "--de", "--r", "r.json", "in",
        ],
        &Environment::default(),
    );
    assert_eq!(s(&v, "db_root").as_deref(), Some("/d"));
    assert_eq!(s(&v, "part").as_deref(), Some("x"));
    assert_eq!(s(&v, "roi").as_deref(), Some("r.json"));
    assert_eq!(s(&v, "fn_in").as_deref(), Some("in"));
    assert_eq!(s(&v, "fn_out").as_deref(), Some("/dev/stdout"));
    assert!(v.flag("sparse") && v.flag("emit_pudc_b_pullup") && v.flag("debug"));
    let v = parse(
        &["--db-root", "d", "--part", "p", "a", "b"],
        &Environment::default(),
    );
    assert_eq!(s(&v, "fn_out").as_deref(), Some("b"));
    assert!(!v.flag("sparse"));
    assert_eq!(s(&v, "roi"), None);

    // prjxray.util.db_root_arg / part_arg defaults.
    let env = Environment {
        xray_database_dir: Some(PyStr::from_str("/db/")),
        xray_database: Some(PyStr::from_str("artix7")),
        xray_part: Some(PyStr::from_str("xc7a35tcsg324-1")),
        ..Default::default()
    };
    let v = parse(&["a"], &env);
    assert_eq!(s(&v, "db_root").as_deref(), Some("/db/artix7"));
    assert_eq!(s(&v, "part").as_deref(), Some("xc7a35tcsg324-1"));
    let out = run_with(&["-h"], &env);
    assert!(out
        .stdout
        .starts_with("usage: fasm2frames.py [-h] [--db-root DB_ROOT] [--part PART] [--sparse]\n"));
    // Only one of the two database variables: --db-root is required.
    let env = Environment {
        xray_database: Some(PyStr::from_str("artix7")),
        ..Default::default()
    };
    let out = run_with(&["--part", "p", "a"], &env);
    assert!(out
        .stderr
        .ends_with("error: the following arguments are required: --db-root\n"));
}

#[test]
fn os_path_join() {
    let j = |a: &str, b: &str| {
        path_join(&PyStr::from_str(a), &PyStr::from_str(b))
            .to_os_string()
            .into_string()
            .unwrap()
    };
    assert_eq!(j("/a", "b"), "/a/b");
    assert_eq!(j("/a/", "b"), "/a/b");
    assert_eq!(j("/a", "/b"), "/b");
    assert_eq!(j("", "b"), "b");
    assert_eq!(j("a", ""), "a/");
}

#[test]
fn program_name() {
    use std::ffi::OsStr;
    assert_eq!(
        prog_name(Some(OsStr::new("/usr/bin/fasm2frames"))),
        "fasm2frames"
    );
    assert_eq!(
        prog_name(Some(OsStr::new("fasm2frames.py"))),
        "fasm2frames.py"
    );
    assert_eq!(prog_name(None), "");
}

fn mini_db() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fasm-xilinx/testdata/mini-db")
}

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/f4pga-xc-fasm")
        .join(name)
}

/// A fresh temporary directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "fasm2frames-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn assembles_to_file() {
    let dir = TempDir::new();
    let out = dir.0.join("out.frm");
    let db = mini_db();
    let fasm = corpus("lut_int.fasm");
    let result = run_tool(&[
        "--db-root",
        s(&db),
        "--part",
        "xc7",
        "--sparse",
        "--debug",
        s(&fasm),
        s(&out),
    ]);
    assert_eq!((result.code, result.stderr.as_str()), (0, ""));
    let frm = std::fs::read_to_string(&out).unwrap();
    assert_eq!(frm.lines().count(), 36);
    assert!(frm.starts_with("0x00020500 0x00000000,"));
    // `--debug`: `dump_frames_sparse` on stdout.
    assert!(result
        .stdout
        .starts_with("\nFrames: 36\nFrame @ 0x0002050B\n    4: 0x00004000\n"));
}

#[test]
fn errors() {
    let dir = TempDir::new();
    let out = dir.0.join("out.frm");
    let db = mini_db();
    let missing = dir.0.join("missing.fasm");
    let result = run_tool(&["--db-root", s(&db), "--part", "xc7", s(&missing), s(&out)]);
    assert_eq!(
        (result.code, result.stderr.as_str()),
        (1, "Exception: Parse error at 0:0 - Couldn't open file\n")
    );
    // The output file was created (opened before anything else).
    assert_eq!(std::fs::read(&out).unwrap(), b"");

    let bad = dir.0.join("bad.fasm");
    std::fs::write(&bad, "CLBLM_L_X10Y102.SLICEM_X0.NOPE\nX_X1Y1.Y\n").unwrap();
    let result = run_tool(&["--db-root", s(&db), "--part", "xc7", s(&bad), s(&out)]);
    assert_eq!(
        (result.code, result.stderr.as_str()),
        (1, "KeyError: 'X_X1Y1'\n")
    );

    // The output file cannot be created: nothing else is done.
    let result = run_tool(&[
        "--db-root",
        "/nonexistent",
        "--part",
        "xc7",
        s(&bad),
        "/nonexistent/out.frm",
    ]);
    assert_eq!(
        (result.code, result.stderr.as_str()),
        (
            1,
            "FileNotFoundError: [Errno 2] No such file or directory: '/nonexistent/out.frm'\n"
        )
    );
    // A database error.
    let result = run_tool(&[
        "--db-root",
        "/nonexistent",
        "--part",
        "xc7",
        s(&bad),
        s(&out),
    ]);
    assert_eq!(result.code, 1);
    assert!(
        result.stderr.starts_with("fasm_xilinx.DbError: "),
        "{}",
        result.stderr
    );
}

/// The reference's ANTLR parser checks the syntax of the whole text before
/// decoding values: a later syntax error wins over a value range error,
/// in the FASM file and in the ROI's `required_features`.
#[test]
fn parse_error_precedence() {
    let dir = TempDir::new();
    let out = dir.0.join("out.frm");
    let db = mini_db();
    let fasm = dir.0.join("prec.fasm");
    std::fs::write(&fasm, "CLBLM_L_X10Y102.SLICEM_X0.AFFMUX.CY = 2\nb c\n").unwrap();
    let result = run_tool(&["--db-root", s(&db), "--part", "xc7", s(&fasm), s(&out)]);
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .starts_with("Exception: Parse error at 2:2 - "),
        "{}",
        result.stderr
    );
    // Only a value range error: it is reported.
    std::fs::write(&fasm, "CLBLM_L_X10Y102.SLICEM_X0.AFFMUX.CY = 2\n").unwrap();
    let result = run_tool(&["--db-root", s(&db), "--part", "xc7", s(&fasm), s(&out)]);
    assert!(
        result.stderr.starts_with("Exception: Parse error at 1:"),
        "{}",
        result.stderr
    );

    let roi = dir.0.join("roi.json");
    std::fs::write(
        &roi,
        r#"{"info": {"GRID_X_MIN": 0, "GRID_X_MAX": 0, "GRID_Y_MIN": 0, "GRID_Y_MAX": 0},
            "required_features": ["X_X0Y0.A = 2", "b c"]}"#,
    )
    .unwrap();
    std::fs::write(&fasm, "CLBLM_L_X10Y102.SLICEM_X0.AFF.ZINI\n").unwrap();
    let result = run_tool(&[
        "--db-root",
        s(&db),
        "--part",
        "xc7",
        "--roi",
        s(&roi),
        s(&fasm),
        s(&out),
    ]);
    assert!(
        result
            .stderr
            .starts_with("Exception: Parse error at 2:2 - "),
        "{}",
        result.stderr
    );
}

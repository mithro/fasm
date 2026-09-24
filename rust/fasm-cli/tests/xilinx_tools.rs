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

//! Tests of the `xc7frames2bit`, `bitread` and `xcfasm` tools on the
//! golden files of `tests/corpus/xilinx/artix7` (`smoke_x1y0.*`, written
//! by the reference tools). The tests that need the prjxray-db `artix7`
//! database (`tools/fetch-db.sh prjxray artix7`, or `FASM_DB_CACHE`) are
//! skipped (pass with a message) without it; the command line itself is
//! compared with the reference tools by `tests/cli/test_*_compat.py`.

use std::path::{Path, PathBuf};

use fasm_cli::fasm2frames::Environment;
use fasm_cli::pystr::PyStr;
use fasm_cli::xc7frames2bit::Env;
use fasm_cli::{bitread, xc7frames2bit, xcfasm};
use fasm_xilinx::bitstream::BitHeader;

const PART: &str = "xc7a35tcsg324-1";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus(name: &str) -> PathBuf {
    repo_root().join("tests/corpus/xilinx/artix7").join(name)
}

fn db() -> Option<PathBuf> {
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

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fasm-cli-xilinx-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn bytes(p: &Path) -> Vec<u8> {
    p.to_str().unwrap().as_bytes().to_vec()
}

fn arg(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// The golden header time as `SOURCE_DATE_EPOCH` (2026/09/24 01:44:56 UTC).
fn golden_env() -> Env {
    Env {
        vars: vec![("SOURCE_DATE_EPOCH".into(), b"1790214296".to_vec())],
    }
}

/// `.bit` bytes with the design field (`a`) replaced by `design`.
fn with_design(bit: &[u8], design: &[u8]) -> Vec<u8> {
    let header = BitHeader::parse(bit).unwrap();
    let old_len = 2 + header.design.len() + 1;
    let mut out = bit[..14].to_vec();
    let len = design.len() + 1;
    out.extend_from_slice(&[(len >> 8) as u8, len as u8]);
    out.extend_from_slice(design);
    out.push(0);
    out.extend_from_slice(&bit[14 + old_len..]);
    out
}

fn run_xc7frames2bit(args: &[Vec<u8>], env: &Env) -> (u8, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = xc7frames2bit::run(b"xc7frames2bit", args, env, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn run_bitread(args: &[Vec<u8>]) -> (u8, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = bitread::run(
        b"bitread",
        args,
        &Env::default(),
        &mut std::io::empty(),
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn xc7frames2bit_reproduces_the_golden_bit() {
    let Some(db) = db() else {
        return;
    };
    let dir = scratch("frames2bit");
    let out = dir.join("out.bit");
    let frm = corpus("smoke_x1y0.frm");
    let (code, stdout, stderr) = run_xc7frames2bit(
        &[
            [
                b"--part_file=".to_vec(),
                bytes(&db.join(PART).join("part.yaml")),
            ]
            .concat(),
            arg("--frm_file"),
            bytes(&frm),
            [b"--output_file=".to_vec(), bytes(&out)].concat(),
            arg("-part_name"),
            arg(PART),
        ],
        &golden_env(),
    );
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "", ""));
    let golden = std::fs::read(corpus("smoke_x1y0.bit")).unwrap();
    let ours = std::fs::read(&out).unwrap();
    let design = [bytes(&frm), b";Generator=xc7frames2bit".to_vec()].concat();
    assert!(
        ours == with_design(&golden, &design),
        "smoke_x1y0.bit differs"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn bitread_reproduces_the_golden_text_and_frm() {
    let Some(db) = db() else {
        return;
    };
    let dir = scratch("bitread");
    let part_file = [
        b"--part_file=".to_vec(),
        bytes(&db.join(PART).join("part.yaml")),
    ]
    .concat();
    let text = dir.join("bits.txt");
    let frm = dir.join("out.frm");
    let (code, stdout, stderr) = run_bitread(&[
        arg("-z"),
        arg("-y"),
        part_file.clone(),
        arg("-o"),
        bytes(&text),
        [b"--frm_out=".to_vec(), bytes(&frm)].concat(),
        bytes(&corpus("smoke_x1y0.bit")),
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        "Bitstream size: 2192122 bytes\nConfig size: 547990 words\nNumber of configuration frames: 5408\nDONE\n"
    );
    assert_eq!(
        std::fs::read_to_string(&text).unwrap(),
        std::fs::read_to_string(corpus("smoke_x1y0.bitread.txt")).unwrap()
    );
    // bit -> .frm (non zero frames) -> bit gives the golden bitstream.
    let out = dir.join("again.bit");
    let (code, _, stderr) = run_xc7frames2bit(
        &[
            part_file,
            [b"--frm_file=".to_vec(), bytes(&frm)].concat(),
            [b"--output_file=".to_vec(), bytes(&out)].concat(),
            [b"--part_name=".to_vec(), arg(PART)].concat(),
        ],
        &golden_env(),
    );
    assert_eq!((code, stderr.as_str()), (0, ""));
    let golden = std::fs::read(corpus("smoke_x1y0.bit")).unwrap();
    let design = [bytes(&frm), b";Generator=xc7frames2bit".to_vec()].concat();
    assert!(std::fs::read(&out).unwrap() == with_design(&golden, &design));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn xcfasm_reproduces_the_golden_bit() {
    let Some(db) = db() else {
        return;
    };
    let dir = scratch("xcfasm");
    let (bit, frm) = (dir.join("x.bit"), dir.join("x.frm"));
    let args: Vec<PyStr> = [
        "--db-root",
        db.to_str().unwrap(),
        "--part",
        PART,
        "--part_file",
        db.join(PART).join("part.yaml").to_str().unwrap(),
        "--sparse",
        "--fn_in",
        corpus("smoke_x1y0.fasm").to_str().unwrap(),
        "--bit_out",
        bit.to_str().unwrap(),
        "--frm_out",
        frm.to_str().unwrap(),
    ]
    .iter()
    .map(|s| PyStr::from_str(s))
    .collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = xcfasm::run(
        "xcfasm",
        &args,
        &Environment::default(),
        &golden_env(),
        || 80,
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    assert!(out.is_empty() && err.is_empty());
    assert_eq!(
        std::fs::read(&frm).unwrap(),
        std::fs::read(corpus("smoke_x1y0.frm")).unwrap()
    );
    let golden = std::fs::read(corpus("smoke_x1y0.bit")).unwrap();
    let design = [bytes(&frm), b";Generator=xc7frames2bit".to_vec()].concat();
    assert!(std::fs::read(&bit).unwrap() == with_design(&golden, &design));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn errors_without_a_database() {
    let (code, _, stderr) = run_xc7frames2bit(&[arg("--part_file=/nonexistent")], &Env::default());
    assert_eq!(
        (code, stderr.as_str()),
        (1, "Part file /nonexistent not found or invalid\n")
    );
    let (code, _, stderr) =
        run_xc7frames2bit(&[arg("--bogus"), arg("--part_file")], &Env::default());
    assert_eq!(
        (code, stderr.as_str()),
        (
            1,
            "ERROR: unknown command line flag 'bogus'\nERROR: flag '--part_file' is missing its argument; flag description: Definition file for target 7-series part\n"
        )
    );
    let (code, _, stderr) = run_xc7frames2bit(&[arg("--architecture=UltraScale")], &Env::default());
    assert_eq!(code, 1);
    assert!(stderr.contains("not supported yet"));
    let (code, stdout, stderr) = run_bitread(&[arg("/nonexistent.bit")]);
    assert_eq!((code, stdout.as_str()), (1, ""));
    assert_eq!(
        stderr,
        "Can't open input file '/nonexistent.bit' for reading!\n"
    );
    let (code, stdout, stderr) = run_bitread(&[]);
    assert_eq!(
        (code, stdout.as_str(), stderr.as_str()),
        (
            1,
            "Bitstream size: 0 bytes\n",
            "Input doesn't look like a bitstream\n"
        )
    );
    let (code, stdout, stderr) = run_bitread(&[bytes(&corpus("smoke_x1y0.bit"))]);
    assert_eq!(
        (code, stdout.as_str(), stderr.as_str()),
        (
            1,
            "Bitstream size: 2192122 bytes\nConfig size: 547990 words\n",
            "Part file not found or invalid\n"
        )
    );
}

#[test]
fn helpshort_text() {
    let (code, stdout, _) = run_xc7frames2bit(&[arg("--helpshort")], &Env::default());
    assert_eq!(code, 1);
    assert_eq!(
        stdout,
        r#"xc7frames2bit: xc7frames2bit

  Flags from tools/xc7frames2bit.cc:
    -architecture (Architecture of the provided bitstream) type: string
      default: "Series7"
    -frm_file (File containing a list of frame deltas to be applied to the base
      bitstream.  Each line in the file is of the form: <frame_address>
      <word1>,...,<word101>.) type: string default: ""
    -output_file (Write bitstream to file) type: string default: ""
    -part_file (Definition file for target 7-series part) type: string
      default: ""
    -part_name (Name of the 7-series part) type: string default: ""
"#
    );
}

/// `bitread` never panics on malformed bitstreams, in any output mode
/// (a small synthetic part, no database needed).
#[test]
fn bitread_fuzz() {
    use fasm_xilinx::bitstream::{bitstream_bytes, BitstreamOptions};
    use fasm_xilinx::{Architecture, Frames, Part};
    let dir = scratch("fuzz");
    let yaml = "\
!<xilinx/xc7series/part>
idcode: 0x362d093
global_clock_regions:
  top:
    rows:
      0: {configuration_buses: {CLB_IO_CLK: {configuration_columns: {0: {frame_count: 3}, 1: {frame_count: 2}}}, BLOCK_RAM: {configuration_columns: {0: {frame_count: 2}}}}}
      1: {configuration_buses: {CLB_IO_CLK: {configuration_columns: {0: {frame_count: 2}}}}}
  bottom:
    rows:
      0: {configuration_buses: {CLB_IO_CLK: {configuration_columns: {0: {frame_count: 1}}}}}
";
    let part_path = dir.join("part.yaml");
    std::fs::write(&part_path, yaml).unwrap();
    let part = Part::from_yaml_file(&part_path, Architecture::Series7).unwrap();
    let mut frames = Frames::zeroed(101, part.iter_frame_addresses().map(|a| a.0));
    for (i, address) in frames.addresses().to_vec().into_iter().enumerate() {
        frames.get_mut(address).unwrap()[i % 101] = 0x8000_0001 | (i as u32) << 4;
    }
    let options = BitstreamOptions {
        date: Some(String::new()),
        time: Some(String::new()),
        ..Default::default()
    };
    let good = bitstream_bytes(&part, &frames, &options).unwrap();
    let part_flag = [b"--part_file=".to_vec(), bytes(&part_path)].concat();
    let modes: [&[&str]; 6] = [
        &["-z", "-y"],
        &["-x", "-C"],
        &["-p"],
        &["-p", "-z", "-F", "0:0x400000"],
        &[],
        &["-f", "0"],
    ];
    let mut state = 0x1234_5678_9ABC_DEF1_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for round in 0..300 {
        let mut input = good.clone();
        for _ in 0..(next() % 6) {
            let i = (next() as usize) % input.len();
            match next() % 3 {
                0 => input[i] = next() as u8,
                1 => input.truncate(i.max(1)),
                _ => {
                    input.remove(i);
                }
            }
        }
        let mode = modes[round % modes.len()];
        let mut args = vec![part_flag.clone()];
        args.extend(mode.iter().map(|a| arg(a)));
        let aux = dir.join("aux.txt");
        args.push([b"--aux=".to_vec(), bytes(&aux)].concat());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = bitread::run(
            b"bitread",
            &args,
            &Env::default(),
            &mut input.as_slice(),
            &mut out,
            &mut err,
        );
        assert!(code <= 1);
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Runs the `bitread` binary with stdout and stderr going to the same
/// file (`2>&1`) and returns what was written.
fn bitread_merged(args: &[&std::ffi::OsStr], dir: &Path) -> (i32, String) {
    let path = dir.join("merged.txt");
    let file = std::fs::File::create(&path).unwrap();
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_bitread"))
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .status()
        .unwrap();
    (
        status.code().unwrap_or(-1),
        std::fs::read_to_string(&path).unwrap(),
    )
}

/// With stdout and stderr merged, the lines come in the reference's
/// order: stdout is flushed after the `Bitstream size`, `Config size` and
/// `Number of configuration frames` lines (the reference's `std::endl`)
/// and before any message on stderr.
#[test]
fn bitread_merged_output_order() {
    let dir = scratch("merged");
    let input = dir.join("hostname");
    std::fs::write(&input, b"abc").unwrap();
    let (code, text) = bitread_merged(
        &["--part_file=/nonexistent".as_ref(), input.as_os_str()],
        &dir,
    );
    assert_eq!(code, 1);
    assert_eq!(
        text,
        "Bitstream size: 3 bytes\nInput doesn't look like a bitstream\n"
    );
    let smoke = corpus("smoke_x1y0.bit");
    let (code, text) = bitread_merged(
        &["--part_file=/nonexistent".as_ref(), smoke.as_os_str()],
        &dir,
    );
    assert_eq!(code, 1);
    assert_eq!(
        text,
        "Bitstream size: 2192122 bytes\nConfig size: 547990 words\nPart file not found or invalid\n"
    );
    let (code, text) = bitread_merged(&["--bogus".as_ref()], &dir);
    assert_eq!(code, 1);
    assert_eq!(text, "ERROR: unknown command line flag 'bogus'\n");
    let _ = std::fs::remove_dir_all(dir);
}

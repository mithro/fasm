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

//! Tests of the prjuray tools (`uray-fasm2frames`, `xcframes2bit`,
//! `uray-bitread`) and of `fasm2frames` on a prjuray-db part, with the
//! synthetic UltraScale+ database of `fasm-xilinx`
//! (`rust/fasm-xilinx/testdata/synthetic-usp-db`). The command lines are
//! compared with the reference tools by `tests/cli/test_uray_tools_compat.py`
//! and `tools/difftest-xilinx.py --prjuray`.

use std::path::{Path, PathBuf};

use fasm_cli::fasm2frames::Environment;
use fasm_cli::pystr::PyStr;
use fasm_cli::xc7frames2bit::{run_tool, Env, Tool, ABORT};
use fasm_cli::{bitread, fasm2frames, uray_fasm2frames};

const PART: &str = "xcusptest-1";

fn db() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fasm-xilinx/testdata/synthetic-usp-db")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fasm-cli-uray-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn s(p: &Path) -> String {
    p.to_str().unwrap().to_owned()
}

const DESIGN: &str = "\
CLEM_X1Y0.ALUT.INIT[15:0] = 16'hA5C3
CLEM_X1Y1.ABCDFF.CEUSED.V1
BRAM_X2Y0.RAMB18E2_L.INIT_00[7:0] = 8'hFF
RCLK_INT_L_X2Y29.BUFCE_LEAF_X0Y0.BUFCE_LEAF.DELAY_TAP.V0
EDGE_X0Y0.OK
";

fn run_f2f(uray: bool, args: &[String]) -> (u8, String, String) {
    let args: Vec<PyStr> = args.iter().map(|a| PyStr::from_str(a)).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let env = Environment::default();
    let code = if uray {
        uray_fasm2frames::run("fasm2frames.py", &args, &env, || 80, &mut out, &mut err)
    } else {
        fasm2frames::run("fasm2frames", &args, &env, || 80, &mut out, &mut err)
    };
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn run_frames2bit(args: &[String], env: &Env) -> (u8, String, String) {
    let args: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_tool(
        Tool::Xcframes2bit,
        b"xcframes2bit",
        &args,
        env,
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn run_bitread(args: &[String]) -> (u8, String, String) {
    let args: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = bitread::run_tool(
        bitread::Flavor::Prjuray,
        b"uray-bitread",
        &args,
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

/// `uray-fasm2frames` (16-bit words) and `fasm2frames` (32-bit words) on
/// the same design agree, and the `.frm` goes through `xcframes2bit` and
/// back through `uray-bitread --frm_out`.
#[test]
fn fasm_to_bit_and_back() {
    let dir = scratch("flow");
    let fasm = dir.join("design.fasm");
    std::fs::write(&fasm, DESIGN).unwrap();
    let base = |out: &Path| {
        vec![
            "--db-root".to_owned(),
            s(&db()),
            "--part".to_owned(),
            PART.to_owned(),
            "--sparse".to_owned(),
            s(&fasm),
            s(out),
        ]
    };
    let half = dir.join("half.frm");
    assert_eq!(run_f2f(true, &base(&half)).0, 0);
    let full = dir.join("full.frm");
    let (code, _, err) = run_f2f(false, &base(&full));
    assert_eq!((code, err.as_str()), (0, ""));
    let half = std::fs::read_to_string(&half).unwrap();
    let full = std::fs::read_to_string(&full).unwrap();
    assert_eq!(half.lines().count(), full.lines().count());
    for (h, f) in half.lines().zip(full.lines()) {
        let (ha, hw) = h.split_once(' ').unwrap();
        let (fa, fw) = f.split_once(' ').unwrap();
        assert_eq!(ha, fa);
        let hw: Vec<u32> = hw
            .split(',')
            .map(|w| u32::from_str_radix(&w[2..], 16).unwrap())
            .collect();
        let fw: Vec<u32> = fw
            .split(',')
            .map(|w| u32::from_str_radix(&w[2..], 16).unwrap())
            .collect();
        assert_eq!((hw.len(), fw.len()), (186, 93));
        for (i, w) in fw.iter().enumerate() {
            assert_eq!(*w, hw[2 * i] | (hw[2 * i + 1] << 16), "{fa} word {i}");
        }
    }
    let bits = dir.join("bits.txt");
    let mut args = base(&bits);
    args.insert(5, "--dump_bits".to_owned());
    assert_eq!(run_f2f(true, &args).0, 0);
    let bits = std::fs::read_to_string(&bits).unwrap();
    assert!(bits.starts_with("bit_00000100_092_19\n"));

    // .frm -> .bit
    let part_yaml = db().join(PART).join("part.yaml");
    let bit = dir.join("design.bit");
    let env = Env {
        vars: vec![("SOURCE_DATE_EPOCH".into(), b"0".to_vec())],
    };
    let args = vec![
        "--architecture=UltraScalePlus".to_owned(),
        format!("--part_file={}", s(&part_yaml)),
        "--part_name=xcusptest".to_owned(),
        format!("--frm_file={}", s(&dir.join("full.frm"))),
        format!("--output_file={}", s(&bit)),
    ];
    let (code, out, err) = run_frames2bit(&args, &env);
    assert_eq!((code, out.as_str(), err.as_str()), (0, "", ""));

    // .bit -> frames
    let back = dir.join("back.frm");
    let (code, out, err) = run_bitread(&[
        "--architecture=UltraScalePlus".to_owned(),
        format!("--part_file={}", s(&part_yaml)),
        "-z".to_owned(),
        "-y".to_owned(),
        format!("--frm_out={}", s(&back)),
        s(&bit),
    ]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("bit_00000100_092_19\n"), "{out}");
    assert!(out.ends_with("DONE\n"));
    let nonzero: Vec<&str> = full
        .lines()
        .filter(|l| {
            l.split_once(' ')
                .unwrap()
                .1
                .split(',')
                .any(|w| w != "0x00000000")
        })
        .collect();
    let back = std::fs::read_to_string(&back).unwrap();
    assert_eq!(back.lines().collect::<Vec<_>>(), nonzero);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn uray_fasm2frames_help_and_usage() {
    // argparse fails to expand the `%` of the --dump_bits help.
    let (code, out, err) = run_f2f(true, &["--help".to_owned()]);
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (1, "", uray_fasm2frames::HELP_ERROR)
    );
    let (code, _, err) = run_f2f(true, &[]);
    assert_eq!(code, 2);
    assert_eq!(
        err,
        "usage: fasm2frames.py [-h] --db-root DB_ROOT --part PART [--sparse]\n                      [--roi ROI] [--debug] [--dump_bits]\n                      fn_in [fn_out]\nfasm2frames.py: error: the following arguments are required: --db-root, --part, fn_in\n"
    );
    // A missing bit: utils.fasm_assembler.FasmLookupError.
    let dir = scratch("lookup");
    let fasm = dir.join("bad.fasm");
    std::fs::write(&fasm, "CLEM_X1Y0.NOPE\n").unwrap();
    let (code, _, err) = run_f2f(
        true,
        &[
            "--db-root".to_owned(),
            s(&db()),
            "--part".to_owned(),
            PART.to_owned(),
            s(&fasm),
            s(&dir.join("out.frm")),
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(
        err,
        "utils.fasm_assembler.FasmLookupError: Segment DB CLEM, key CLEM.NOPE not found from line 'CLEM_X1Y0.NOPE'\n"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn xcframes2bit_errors() {
    let dir = scratch("errors");
    let part_yaml = s(&db().join(PART).join("part.yaml"));
    let frm = dir.join("in.frm");
    let words = vec!["0x00000000"; 93].join(",");
    // Column 9 is not in the part.
    std::fs::write(&frm, format!("0x00000100 {words}\n0x000009FF {words}\n")).unwrap();
    let args = |arch: &str| {
        vec![
            format!("--architecture={arch}"),
            format!("--part_file={part_yaml}"),
            format!("--frm_file={}", s(&frm)),
            format!("--output_file={}", s(&dir.join("out.bit"))),
        ]
    };
    let (code, _, err) = run_frames2bit(&args("UltraScalePlus"), &Env::default());
    assert_eq!(code, 1);
    assert_eq!(
        err,
        format!(
            "Frames file contains an invalid frame: [     0x9ff]  Row= 0 Column= 9 Minor=255 Type=CLB/IO/CLK\nFrames file {} not found or invalid\n",
            s(&frm)
        )
    );
    // An UltraScale+ part.yaml is not an UltraScale part.
    let (code, _, err) = run_frames2bit(&args("UltraScale"), &Env::default());
    assert_eq!(code, 1);
    assert_eq!(err, format!("Part file {part_yaml} not found or invalid\n"));
    // Unknown architecture: prjuray-tools aborts.
    let (code, _, err) = run_frames2bit(&args("Foo"), &Env::default());
    assert_eq!(code, ABORT);
    assert!(err.contains("absl::bad_variant_access"));
    // gflags 2.2.2: --helpfull, not --helpful.
    let (code, out, _) = run_frames2bit(&["--helpfull".to_owned()], &Env::default());
    assert_eq!(code, 1);
    assert!(out.contains("Flags from tools/xcframes2bit.cc:"));
    assert!(out.contains("-helpfull (show help on all flags -- same as -help)"));
    let (code, _, err) = run_frames2bit(&["--helpful".to_owned()], &Env::default());
    assert_eq!(
        (code, err.as_str()),
        (1, "ERROR: unknown command line flag 'helpful'\n")
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn uray_bitread_verifies_the_ecc() {
    let dir = scratch("ecc");
    let part_yaml = s(&db().join(PART).join("part.yaml"));
    let frm = dir.join("in.frm");
    let mut words = vec!["0x00000000".to_owned(); 93];
    words[3] = "0x00000010".to_owned();
    std::fs::write(&frm, format!("0x00000105 {}\n", words.join(","))).unwrap();
    let bit = dir.join("in.bit");
    let (code, _, _) = run_frames2bit(
        &[
            "--architecture=UltraScalePlus".to_owned(),
            format!("--part_file={part_yaml}"),
            format!("--frm_file={}", s(&frm)),
            format!("--output_file={}", s(&bit)),
        ],
        &Env::default(),
    );
    assert_eq!(code, 0);
    let mut data = std::fs::read(&bit).unwrap();
    // Flip one data bit of frame 0x105 (word 3) without fixing the ECC.
    let needle = 0x10u32.to_be_bytes();
    let at = data
        .windows(4)
        .rposition(|w| w == needle)
        .expect("the data word");
    data[at + 3] = 0x30;
    std::fs::write(&bit, &data).unwrap();
    let args = |extra: &[&str]| {
        let mut v = vec![
            "--architecture=UltraScalePlus".to_owned(),
            format!("--part_file={part_yaml}"),
            "-z".to_owned(),
            "-y".to_owned(),
        ];
        v.extend(extra.iter().map(|e| (*e).to_owned()));
        v.push(s(&bit));
        v
    };
    let (code, out, err) = run_bitread(&args(&[]));
    assert_eq!(code, 1);
    assert_eq!(
        err,
        "ERROR: ECC verification of frame [     0x105]  Row= 0 Column= 1 Minor= 5 Type=CLB/IO/CLK failed.\n"
    );
    assert!(!out.contains("DONE"));
    let (code, out, err) = run_bitread(&args(&["-E"]));
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains(
        "WARNING: ECC verification of frame [     0x105]  Row= 0 Column= 1 Minor= 5 Type=CLB/IO/CLK failed.\nFrame 0x00000105 (Type=0 Top=0 Row=0 Column=1 Minor=5):\nbit_00000105_003_04\nbit_00000105_003_05\n"
    ));
    // The prjxray bitread has no -E.
    let (code, _, err) = {
        let a: Vec<Vec<u8>> = args(&["-E"])
            .iter()
            .map(|a| a.as_bytes().to_vec())
            .collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = bitread::run(
            b"bitread",
            &a,
            &Env::default(),
            &mut std::io::empty(),
            &mut out,
            &mut err,
        );
        (code, out, String::from_utf8(err).unwrap())
    };
    assert_eq!(
        (code, err.as_str()),
        (1, "ERROR: unknown command line flag 'E'\n")
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

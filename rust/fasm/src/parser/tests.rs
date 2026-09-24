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

//! Parser tests: the `examples/*.fasm` files against the oracle parse trees
//! in `tests/corpus/oracle/`, a hand written edge case table, the API
//! entry points and a random input robustness test.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::*;
use crate::model::{FeatureValue, SetFasmFeature, ValueFormat};

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The JSON document `tests/oracle/dump.py` prints for a successful parse.
fn dump(lines: &[FasmLine]) -> Value {
    let lines: Vec<Value> = lines
        .iter()
        .map(|line| {
            let set_feature = line.set_feature.as_ref().map(|f| {
                json!({
                    "feature": f.feature.to_string(),
                    "start": f.start,
                    "end": f.end,
                    "value": f.value.to_string(),
                    "value_format": f.value_format.map(ValueFormat::python_name),
                })
            });
            let annotations = line.annotations.as_ref().map(|annotations| {
                annotations
                    .iter()
                    .map(|a| json!({"name": &*a.name, "value": &*a.value}))
                    .collect::<Vec<_>>()
            });
            json!({
                "set_feature": set_feature,
                "annotations": annotations,
                "comment": line.comment.as_deref(),
            })
        })
        .collect();
    json!({ "lines": lines })
}

/// Compact rendering of a parse result used by the edge case table:
///
/// * lines are separated by ` | `;
/// * a feature is `name[end:start]=value/FORMAT` (`[start]` for a single
///   bit address, no brackets without address, `-` for no value format);
/// * annotations are `{name="value",...}` (value in Rust `{:?}` form);
/// * a comment is `#"text"` (Rust `{:?}` form);
/// * an error is `ERR line:column Kind`.
fn render(result: &Result<Vec<FasmLine>, ParseError>) -> String {
    match result {
        Err(e) => format!("ERR {}:{} {:?}", e.line, e.column, e.kind),
        Ok(lines) => lines
            .iter()
            .map(render_line)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

fn render_line(line: &FasmLine) -> String {
    let mut parts = Vec::new();
    if let Some(f) = &line.set_feature {
        let mut s = f.feature.to_string();
        match (f.start, f.end) {
            (Some(start), Some(end)) => write!(s, "[{end}:{start}]").unwrap(),
            (Some(start), None) => write!(s, "[{start}]").unwrap(),
            (None, None) => {}
            (None, Some(end)) => write!(s, "[{end}:?]").unwrap(),
        }
        let format = f.value_format.map_or("-", ValueFormat::python_name);
        write!(s, "={}/{format}", f.value).unwrap();
        parts.push(s);
    }
    if let Some(annotations) = &line.annotations {
        let inner: Vec<String> = annotations
            .iter()
            .map(|a| format!("{}={:?}", a.name, a.value))
            .collect();
        parts.push(format!("{{{}}}", inner.join(",")));
    }
    if let Some(comment) = &line.comment {
        parts.push(format!("#{comment:?}"));
    }
    parts.join(" ")
}

// ---------------------------------------------------------------------
// examples/*.fasm against the oracle
// ---------------------------------------------------------------------

/// Every `examples/*.fasm` file parses to exactly the parse tree the
/// original ANTLR parser produced (`tests/oracle/dump.py --parser antlr`,
/// checked in as `tests/corpus/oracle/<name>.json`).
#[test]
fn examples_match_oracle() {
    let root = repo_root();
    let mut examples: Vec<PathBuf> = std::fs::read_dir(root.join("examples"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "fasm"))
        .collect();
    examples.sort();
    assert!(examples.len() >= 4, "examples not found: {examples:?}");

    for example in examples {
        let name = example.file_stem().unwrap().to_str().unwrap();
        let oracle_path = root.join(format!("tests/corpus/oracle/{name}.json"));
        let oracle: Value = serde_json::from_str(
            &std::fs::read_to_string(&oracle_path)
                .unwrap_or_else(|e| panic!("{}: {e}", oracle_path.display())),
        )
        .unwrap();
        let lines = parse_fasm_filename(&example).unwrap();
        assert_eq!(dump(&lines), oracle, "{}", example.display());

        // The string and streaming APIs agree with the file API.
        let text = std::fs::read_to_string(&example).unwrap();
        assert_eq!(parse_fasm_string(&text).unwrap(), lines);
        let streamed: Vec<FasmLine> = parse_lines(text.as_bytes()).map(Result::unwrap).collect();
        assert_eq!(streamed, lines);
    }
}

// ---------------------------------------------------------------------
// Edge case table
// ---------------------------------------------------------------------

/// How the original ANTLR parser (`fasm.parser.antlr`) handles a case,
/// checked against the oracle with `dump_edge_cases_for_oracle`.
#[derive(Clone, Copy, Debug)]
enum Antlr {
    /// Same parse tree, or an error at the same line:column.
    Same,
    /// ANTLR rejects the input too, but with a Python `AssertionError` (or
    /// another exception) that carries no position.
    NoPos,
    /// ANTLR behaves differently; the reason is recorded in
    /// `docs/rewrite/COMPAT.md`.
    Differs(&'static str),
}

struct Case {
    input: Vec<u8>,
    expect: String,
    antlr: Antlr,
}

fn case(input: impl AsRef<[u8]>, expect: impl Into<String>, antlr: Antlr) -> Case {
    Case {
        input: input.as_ref().to_vec(),
        expect: expect.into(),
        antlr,
    }
}

fn edge_cases() -> Vec<Case> {
    use Antlr::{Differs, NoPos, Same};

    let ones_256 = "F".repeat(64);
    let two_pow_256_minus_1 =
        "115792089237316195423570985008687907853269984665640564039457584007913129639935";
    let two_pow_1023_hex = format!("8{}", "0".repeat(255));
    let two_pow_1023 = FeatureValue::from_hex_str(&two_pow_1023_hex)
        .unwrap()
        .to_string();
    let bin_1024 = "1".repeat(1024);
    let dec_1000_bits = FeatureValue::from_hex_str(&format!("1{}", "0".repeat(250)))
        .unwrap()
        .to_string();

    vec![
        // --- Blank input -------------------------------------------------
        case("", "", Same),
        case("\n\n  \t\n", "", Same),
        case("\r\n\r\n", "", Same),
        // --- Features ----------------------------------------------------
        case("a", "a=1/-", Same),
        case("a\n", "a=1/-", Same),
        case("a.b.c", "a.b.c=1/-", Same),
        case("a_b.c1_2.X0Y0", "a_b.c1_2.X0Y0=1/-", Same),
        case("A_ = 1", "A_=1/PLAIN", Same),
        case(
            "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT[17]",
            "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT[17]=1/-",
            Same,
        ),
        case("a[5]", "a[5]=1/-", Same),
        case("a[0:0]", "a[0:0]=1/-", Same),
        // --- Values ------------------------------------------------------
        case("a = 0", "a=0/PLAIN", Same),
        case("a = 1", "a=1/PLAIN", Same),
        case("a = 001", "a=1/PLAIN", Same),
        case("a[3:0] = 007", "a[3:0]=7/PLAIN", Same),
        case("a[3:0] = 4'hF", "a[3:0]=15/VERILOG_HEX", Same),
        case("a[15:0] = 16'hDead", "a[15:0]=57005/VERILOG_HEX", Same),
        case("a[7:0] = 8'b1010_0101", "a[7:0]=165/VERILOG_BINARY", Same),
        case("a[7:0] = 8'd255", "a[7:0]=255/VERILOG_DECIMAL", Same),
        case("a[8:0] = 9'o777", "a[8:0]=511/VERILOG_OCTAL", Same),
        case("a[3:0] = 'hA", "a[3:0]=10/VERILOG_HEX", Same),
        case("a = 1'b1", "a=1/VERILOG_BINARY", Same),
        case("a[0:0] = 1'b0", "a[0:0]=0/VERILOG_BINARY", Same),
        case("a = 'b_1_", "a=1/VERILOG_BINARY", Same),
        case("a = 'h_", "a=0/VERILOG_HEX", Same),
        case("a = 'd_", "a=0/VERILOG_DECIMAL", Same),
        case("a = 'o__", "a=0/VERILOG_OCTAL", Same),
        case("a = 0'b1", "a=1/VERILOG_BINARY", Same),
        case("a[3:0] = 0'hF", "a[3:0]=15/VERILOG_HEX", Same),
        case("a[0] = 3'b001", "a[0]=1/VERILOG_BINARY", Same),
        case("a[5:0] = 3'b111", "a[5:0]=7/VERILOG_BINARY", Same),
        case("a = 00'h1", "a=1/VERILOG_HEX", Same),
        case(
            "a[7:0] = 004'hF_F",
            "ERR 1:9 ValueExceedsDeclaredWidth",
            NoPos,
        ),
        case(
            "a[7:0] = 8'hF_F\t# c { x }",
            r##"a[7:0]=255/VERILOG_HEX #" c { x }""##,
            Same,
        ),
        case(
            format!("a[255:0] = 256'h{ones_256}"),
            format!("a[255:0]={two_pow_256_minus_1}/VERILOG_HEX"),
            Same,
        ),
        case(
            format!("a[1023:0] = 1024'h{two_pow_1023_hex}"),
            format!("a[1023:0]={two_pow_1023}/VERILOG_HEX"),
            Same,
        ),
        case(
            format!("a[1023:0] = 1024'b{bin_1024}"),
            format!(
                "a[1023:0]={}/VERILOG_BINARY",
                FeatureValue::from_bin_str(&bin_1024).unwrap()
            ),
            Same,
        ),
        case(
            format!("a[1023:0] = 'd{two_pow_1023}"),
            format!("a[1023:0]={two_pow_1023}/VERILOG_DECIMAL"),
            Differs("'d values above 2^64 - 1: 'Could not decode decimal number.'"),
        ),
        case(
            format!("a[1023:0] = {dec_1000_bits}"),
            format!("a[1023:0]={dec_1000_bits}/PLAIN"),
            Differs("plain values above 2^31 - 1: 'Could not decode decimal number.'"),
        ),
        case(
            "a[31:0] = 2147483648",
            "a[31:0]=2147483648/PLAIN",
            Differs("plain values above 2^31 - 1: 'Could not decode decimal number.'"),
        ),
        case(
            "a[63:0] = 'd4294967296",
            "a[63:0]=4294967296/VERILOG_DECIMAL",
            Differs("'d values are truncated to 32 bits (gives 0)"),
        ),
        case(
            "a[63:0] = 'o1234567012345670123",
            "a[63:0]=23528931761549395/VERILOG_OCTAL",
            Differs("octal values above 32 bits are decoded wrongly"),
        ),
        case(
            "a[3:0] = 99999999999'h1",
            "a[3:0]=1/VERILOG_HEX",
            Differs("widths above 2^31 - 1 abort the process (std::stoi)"),
        ),
        // --- Underscores in plain decimals and addresses ----------------
        case(
            "a[15:0] = 1_000",
            "a[15:0]=1000/PLAIN",
            Differs("'_' in plain decimals: textX and the spec accept it"),
        ),
        case("a = 1__0", "ERR 1:5 Syntax", Same),
        case("a = 1_", "ERR 1:5 Syntax", Same),
        case("a = 1_0'hF", "ERR 1:5 Syntax", Same),
        case(
            "a[1_0]",
            "a[10]=1/-",
            Differs("'_' in addresses: textX and the spec accept it"),
        ),
        case("a[1__0]", "ERR 1:3 Syntax", Same),
        case("a[_1]", "ERR 1:2 Syntax", Same),
        // --- Whitespace ---------------------------------------------------
        case("  a  =  1  ", "a=1/PLAIN", Same),
        case("\ta\t=\t1\t", "a=1/PLAIN", Same),
        case("a=1", "a=1/PLAIN", Same),
        case("a[ 3 : 0 ] = 4'hF", "a[3:0]=15/VERILOG_HEX", Same),
        case("a [3:0] = 4'hF", "a[3:0]=15/VERILOG_HEX", Same),
        case("\ta.b\t[\t1\t]", "a.b[1]=1/-", Same),
        case("a[3:0] = 4 'hF", "a[3:0]=15/VERILOG_HEX", Same),
        case(
            "a[3:0] = 4'h F",
            "a[3:0]=15/VERILOG_HEX",
            Differs("whitespace after 'h is decoded as a digit (garbage value)"),
        ),
        case(
            "a[63:0] = 'h F",
            "a[63:0]=15/VERILOG_HEX",
            Differs("whitespace after 'h is decoded as a digit (gives 4294967295)"),
        ),
        case(
            "a[7:0] = 'd 5",
            "a[7:0]=5/VERILOG_DECIMAL",
            Differs("whitespace after 'd: 'Could not decode decimal number.'"),
        ),
        case(
            "a[7:0] = 'o\t7",
            "a[7:0]=7/VERILOG_OCTAL",
            Differs("whitespace after 'o is decoded as a digit (garbage value)"),
        ),
        case("a[7:0] = 'b 101", "a[7:0]=5/VERILOG_BINARY", Same),
        // --- Line endings -------------------------------------------------
        case("a\r\nb\r\n", "a=1/- | b=1/-", Same),
        case("a\rb", "a=1/- | b=1/-", Same),
        case("a\n\r\nb", "a=1/- | b=1/-", Same),
        case("a\nb", "a=1/- | b=1/-", Same),
        case("a\r\nb c", "ERR 2:2 Syntax", Same),
        case("a\rb c", "ERR 1:4 Syntax", Same),
        case("x\n\ny z", "ERR 3:2 Syntax", Same),
        case("a = 1\nb = 1\nc = 3 3", "ERR 3:6 Syntax", Same),
        case("# a\rb\r# c", r#"#" a" | b=1/- | #" c""#, Same),
        // --- Comments -----------------------------------------------------
        case("#", r##"#"""##, Same),
        case("# c", r##"#" c""##, Same),
        case("a # c", r##"a=1/- #" c""##, Same),
        case("a=1#x", r##"a=1/PLAIN #"x""##, Same),
        case("#  x  \t", r##"#"  x  \t""##, Same),
        case("# {a = \"b\"}", r##"#" {a = \"b\"}""##, Same),
        case("a # c\x00d", r##"a=1/- #" c\0d""##, Same),
        case(
            "# comment \u{e9}",
            "#\" comment \u{e9}\"",
            Differs("non-ASCII text: ASCII decoding fails"),
        ),
        case(
            b"# \xff".as_slice(),
            "ERR 1:2 InvalidUtf8",
            Differs("invalid UTF-8"),
        ),
        // --- Annotations --------------------------------------------------
        case("{ a = \"b\" }", r#"{a="b"}"#, Same),
        case("a { .attr = \"\" }", r#"a=1/- {.attr=""}"#, Same),
        case(
            "{ module = \"top\", file = \"/a/b/d.txt\", line_number = \"123\" }",
            r#"{module="top",file="/a/b/d.txt",line_number="123"}"#,
            Same,
        ),
        case("a{b=\"c\"}#x", r##"a=1/- {b="c"} #"x""##, Same),
        case(
            "a[3:0]=4'hF{x=\"y\"}",
            r#"a[3:0]=15/VERILOG_HEX {x="y"}"#,
            Same,
        ),
        case("{ a=\"1\" , b=\"2\" }", r#"{a="1",b="2"}"#, Same),
        case(r#"{ a = "x\"y" }"#, r#"{a="x\\\"y"}"#, Same),
        case(r#"{ a = "x\\" }"#, r#"{a="x\\\\"}"#, Same),
        case("{ a = \"x#y\" }", r#"{a="x#y"}"#, Same),
        case("{ a = \"x\ny\" }", r#"{a="x\ny"}"#, Same),
        case("{ a = \"x\r\ny\" }\nb", r#"{a="x\r\ny"} | b=1/-"#, Same),
        case("{ a = \"x\ny\" } b", "ERR 2:5 Syntax", Same),
        case("{ a = \"x\" } # c { d }", r##"{a="x"} #" c { d }""##, Same),
        case("{ . = \"x\", .1_a = \"y\" }", r#"{.="x",.1_a="y"}"#, Same),
        case("  \t{a=\"\" , b=\"\"}\t#", r##"{a="",b=""} #"""##, Same),
        case(
            "{ a = \"\u{e9}\" }",
            "{a=\"\u{e9}\"}",
            Differs("non-ASCII text: ASCII decoding fails"),
        ),
        case(r#"{ a = "x\q" }"#, "ERR 1:6 Syntax", Same),
        case("{ a = \"b\"", "ERR 1:9 Syntax", Same),
        case("{ a = \"b\"\n}", "ERR 1:9 Syntax", Same),
        case("{ a = \"b", "ERR 1:6 Syntax", Same),
        case("{ }", "ERR 1:2 Syntax", Same),
        case("{ 1 = \"x\" }", "ERR 1:2 Syntax", Same),
        case("{ .a.b=\"x\" }", "ERR 1:4 Syntax", Same),
        case("{ a=\"1\",b=\"2\",}", "ERR 1:14 Syntax", Same),
        case("{ a=\"1\" b=\"2\" }", "ERR 1:8 Syntax", Same),
        case("{ a = b }", "ERR 1:6 Syntax", Same),
        case("{{ a = \"b\" }}", "ERR 1:1 Syntax", Same),
        case("{ a = \"b\" }}", "ERR 1:11 Syntax", Same),
        case("{ a = \"b\" } { c = \"d\" }", "ERR 1:12 Syntax", Same),
        case("{ a = \"x\" } b", "ERR 1:12 Syntax", Same),
        case("a = 1 {", "ERR 1:7 Syntax", Same),
        case("# c { a = \"b\" }\n{", "ERR 2:1 Syntax", Same),
        // --- Feature syntax errors ----------------------------------------
        case("a.", "ERR 1:1 Syntax", Same),
        case("a..b", "ERR 1:1 Syntax", Same),
        case("a.1", "ERR 1:1 Syntax", Same),
        case(".a = 1", "ERR 1:0 Syntax", Same),
        case("_a", "ERR 1:0 Syntax", Same),
        case("1abc", "ERR 1:0 Syntax", Same),
        case("a b", "ERR 1:2 Syntax", Same),
        case("a\tb", "ERR 1:2 Syntax", Same),
        case("a\x00", "ERR 1:1 Syntax", Same),
        // --- Value syntax errors ------------------------------------------
        case("a = 12abc", "ERR 1:6 Syntax", Same),
        // Syntax errors take precedence over range checks on the same line.
        case("a[0:1] = 1 2", "ERR 1:11 Syntax", Same),
        case("a[99999999999] = 1 2", "ERR 1:19 Syntax", Same),
        case("a = 2 { x = \"y\" } z", "ERR 1:18 Syntax", Same),
        case("a == 1", "ERR 1:3 Syntax", Same),
        case("a =", "ERR 1:3 Syntax", Same),
        case("a = ", "ERR 1:4 Syntax", Same),
        case("a =\n1", "ERR 1:3 Syntax", Same),
        case("= 1", "ERR 1:0 Syntax", Same),
        case("a = 1 = 2", "ERR 1:6 Syntax", Same),
        case("a = 1 1", "ERR 1:6 Syntax", Same),
        case("a = 1 }", "ERR 1:6 Syntax", Same),
        case("a = 'h", "ERR 1:4 Syntax", Same),
        case("a = 4'h", "ERR 1:5 Syntax", Same),
        case("a = 'hg", "ERR 1:4 Syntax", Same),
        case("a[3:0] = 'H1", "ERR 1:9 Syntax", Same),
        case("a = '_", "ERR 1:4 Syntax", Same),
        case("a = 1'b1 2", "ERR 1:9 Syntax", Same),
        case("a = 'b12", "ERR 1:7 Syntax", Same),
        case("a = 'o8", "ERR 1:4 Syntax", Same),
        case("a = +4'hF", "ERR 1:4 Syntax", Same),
        case("a = -1", "ERR 1:4 Syntax", Same),
        // --- Value width checks -------------------------------------------
        case("a = 2", "ERR 1:4 ValueExceedsAddressWidth", NoPos),
        case(
            format!("a[20000:0] = {}", "9".repeat(4300)),
            format!(
                "a[20000:0]={}/PLAIN",
                FeatureValue::from_digits("9".repeat(4300).as_bytes(), 10).unwrap()
            ),
            Differs("plain values above 2^31 - 1: 'Could not decode decimal number.'"),
        ),
        case(
            format!("a[20000:0] = {}", "9".repeat(4301)),
            "ERR 1:13 DecimalValueTooLong",
            Same,
        ),
        case(
            format!("a[20000:0] = 'd{}", "9".repeat(4301)),
            "ERR 1:13 DecimalValueTooLong",
            Same,
        ),
        case(
            format!("a = 'd{}1", "0".repeat(4301)),
            "a=1/VERILOG_DECIMAL",
            Same,
        ),
        case(format!("a = {}1", "0".repeat(4301)), "a=1/PLAIN", Same),
        case(
            format!("a[20000:0] = 'd{}1", "0_".repeat(4299)),
            "a[20000:0]=1/VERILOG_DECIMAL",
            Same,
        ),
        case(
            "a[3:0] = 'b10000",
            "ERR 1:9 ValueExceedsAddressWidth",
            NoPos,
        ),
        case("a[1:0] = 4", "ERR 1:9 ValueExceedsAddressWidth", NoPos),
        case(
            "a[5:0] = 3'b1111",
            "ERR 1:9 ValueExceedsDeclaredWidth",
            NoPos,
        ),
        case(
            format!("a[1023:0] = 256'h1{ones_256}"),
            "ERR 1:12 ValueExceedsDeclaredWidth",
            NoPos,
        ),
        case(
            "a = 2\nb c",
            "ERR 1:4 ValueExceedsAddressWidth",
            Differs("ANTLR reports the later syntax error first (2:2)"),
        ),
        // --- Address checks -----------------------------------------------
        case("a[4294967295]", "a[4294967295]=1/-", Same),
        case("a[4294967295:1] = 0", "a[4294967295:1]=0/PLAIN", Same),
        case(
            "a[4294967296] = 0",
            "ERR 1:2 AddressOutOfRange",
            Differs("addresses are truncated to 32 bits"),
        ),
        case(
            "a[18446744073709551616] = 0",
            "ERR 1:2 AddressOutOfRange",
            Differs("addresses above 2^64 - 1 abort the process (std::stoul)"),
        ),
        case(
            "a[4294967295:0]",
            "ERR 1:1 AddressOutOfRange",
            Differs("a 2^32 bit wide range is accepted"),
        ),
        case(
            "a[0:1] = 0",
            "ERR 1:1 AddressEndBeforeStart",
            Differs("end < start is accepted when the value is 0"),
        ),
        case("a[0:1]", "ERR 1:1 AddressEndBeforeStart", NoPos),
        case("a[3]:0]", "ERR 1:4 Syntax", Same),
        case("a[]", "ERR 1:2 Syntax", Same),
        case("a[3:]", "ERR 1:4 Syntax", Same),
        case("a[3", "ERR 1:3 Syntax", Same),
        case("a[3:0", "ERR 1:5 Syntax", Same),
        case("a[3][4]", "ERR 1:4 Syntax", Same),
        case("a[-1]", "ERR 1:2 Syntax", Same),
        // --- Non-ASCII outside of comments and annotation values ----------
        case(
            "\u{e9}",
            "ERR 1:0 Syntax",
            Differs("the error message is not ASCII: no position"),
        ),
        // A byte order mark is skipped at the start of the input only.
        case("\u{feff}a", "a=1/-", Same),
        case("\u{feff}a b", "ERR 1:2 Syntax", Same),
        case(
            "a\n\u{feff}b",
            "ERR 2:0 Syntax",
            Differs("the error message is not ASCII: no position"),
        ),
        case(
            "# \u{e9}\u{e9}\n{ a = \"\u{e9}\" } b",
            "ERR 2:12 Syntax",
            Differs("non-ASCII text: ASCII decoding fails"),
        ),
        case(
            b"a \xff".as_slice(),
            "ERR 1:2 Syntax",
            Differs("invalid UTF-8"),
        ),
    ]
}

#[test]
fn edge_case_table() {
    let cases = edge_cases();
    assert!(cases.len() >= 60);
    let mut failures = String::new();
    for c in &cases {
        let got = render(&parse_fasm_bytes(&c.input));
        if got != c.expect {
            writeln!(
                failures,
                "input {:?}\n  expected {}\n  got      {}",
                String::from_utf8_lossy(&c.input),
                c.expect,
                got
            )
            .unwrap();
        }
    }
    assert!(failures.is_empty(), "\n{failures}");
}

/// Writes the edge case table with the Rust parse trees as JSON lines to
/// the file named by `FASM_EDGE_CASES_OUT`, for comparison with the oracle
/// (`tests/oracle/dump.py`). Run with
/// `FASM_EDGE_CASES_OUT=/tmp/cases.jsonl cargo test -p fasm -- --ignored dump_edge_cases`.
#[test]
#[ignore = "writes a file for the oracle comparison"]
fn dump_edge_cases_for_oracle() {
    let Ok(out) = std::env::var("FASM_EDGE_CASES_OUT") else {
        return;
    };
    let mut text = String::new();
    for c in edge_cases() {
        let result = match parse_fasm_bytes(&c.input) {
            Ok(lines) => dump(&lines),
            Err(e) => json!({"error": format!("{}:{} {:?}", e.line, e.column, e.kind)}),
        };
        let antlr = match c.antlr {
            Antlr::Same => "same".to_string(),
            Antlr::NoPos => "nopos".to_string(),
            Antlr::Differs(why) => format!("differs: {why}"),
        };
        let input_hex: String = c.input.iter().map(|b| format!("{b:02x}")).collect();
        let line = json!({"input_hex": input_hex, "rust": result, "antlr": antlr});
        writeln!(text, "{line}").unwrap();
    }
    std::fs::write(out, text).unwrap();
}

// ---------------------------------------------------------------------
// API
// ---------------------------------------------------------------------

#[test]
fn display_matches_antlr_wrapper_format() {
    let e = parse_fasm_string("a = 1\nb c\n").unwrap_err();
    assert_eq!(e.line, 2);
    assert_eq!(e.column, 2);
    assert_eq!(e.kind, ParseErrorKind::Syntax);
    assert_eq!(
        e.to_string(),
        "Parse error at 2:2 - unexpected 'c', expected '[', '=', '{', '#' or end of line"
    );
}

#[test]
fn model_values() {
    let lines = parse_fasm_string("A.B[7:4] = 4'b1010 { x = \"y\" } # c\n").unwrap();
    assert_eq!(lines.len(), 1);
    let expected = SetFasmFeature::new(
        crate::idstring::IdString::new("A.B"),
        Some(4),
        Some(7),
        FeatureValue::from_u64(10),
        Some(ValueFormat::VerilogBinary),
    )
    .unwrap();
    assert_eq!(lines[0].set_feature.as_ref(), Some(&expected));
    assert_eq!(
        lines[0].annotations.as_deref(),
        Some([crate::model::Annotation::new("x", "y")].as_slice())
    );
    assert_eq!(lines[0].comment.as_deref(), Some(" c"));

    // No value: value 1, no value format.
    let lines = parse_fasm_string("A.B").unwrap();
    let f = lines[0].set_feature.as_ref().unwrap();
    assert_eq!(f.value, 1u64);
    assert_eq!(f.value_format, None);
    assert_eq!((f.start, f.end), (None, None));
}

#[test]
fn every_parsed_feature_satisfies_the_model_invariants() {
    // Features built with new_unchecked by the parser must be accepted by
    // the checked constructor too.
    for c in edge_cases() {
        if let Ok(lines) = parse_fasm_bytes(&c.input) {
            for f in lines.iter().filter_map(|l| l.set_feature.as_ref()) {
                let checked =
                    SetFasmFeature::new(f.feature, f.start, f.end, f.value.clone(), f.value_format);
                assert_eq!(checked.as_ref(), Ok(f));
                let _ = f.width();
            }
        }
    }
}

#[test]
fn parse_line_single_lines() {
    assert_eq!(parse_line(b"", 1), Ok(None));
    assert_eq!(parse_line(b"  \t", 1), Ok(None));
    assert_eq!(parse_line(b"\r\n", 1), Ok(None));
    for input in [
        &b"a = 1"[..],
        b"a = 1\n",
        b"a = 1\r\n",
        b"a = 1\r",
        b"a = 1\n\n",
    ] {
        let line = parse_line(input, 7).unwrap().unwrap();
        assert_eq!(render_line(&line), "a=1/PLAIN", "{input:?}");
    }
    let line = parse_line(b"\xEF\xBB\xBFa", 1).unwrap().unwrap();
    assert_eq!(render_line(&line), "a=1/-");
    let e = parse_line(b"\xEF\xBB\xBFa", 2).unwrap_err();
    assert_eq!((e.line, e.column, e.kind), (2, 0, ParseErrorKind::Syntax));
    let line = parse_line(b"{ a = \"x\ny\" }", 1).unwrap().unwrap();
    assert_eq!(render_line(&line), r#"{a="x\ny"}"#);
}

#[test]
fn parse_line_errors_use_line_number() {
    let e = parse_line(b"a = 1 1", 42).unwrap_err();
    assert_eq!((e.line, e.column), (42, 6));
    let e = parse_line(b"{ a = \"x\ny\" } b", 42).unwrap_err();
    assert_eq!((e.line, e.column), (43, 5));
}

#[test]
fn parse_line_rejects_several_lines() {
    let e = parse_line(b"a\nb", 3).unwrap_err();
    assert_eq!((e.line, e.column, e.kind), (4, 0, ParseErrorKind::Syntax));
    let e = parse_line(b"a\r  b", 3).unwrap_err();
    assert_eq!((e.line, e.column), (3, 2));
    // An error in the second line is reported as is.
    let e = parse_line(b"a\n\nb c", 1).unwrap_err();
    assert_eq!((e.line, e.column), (3, 2));
}

#[test]
fn lines_iterator_stops_after_error() {
    let mut lines = parse_lines(b"a\nb c\nd\n");
    assert!(lines.next().unwrap().is_ok());
    let e = lines.next().unwrap().unwrap_err();
    assert_eq!((e.line, e.column), (2, 2));
    assert!(lines.next().is_none());
    assert!(lines.next().is_none());
}

#[test]
fn lines_iterator_is_lazy() {
    // Lines before an error are returned before the error is seen.
    let first: Vec<_> = parse_lines(b"a\nb\n= bad").take(2).collect();
    assert!(first.iter().all(Result::is_ok));
}

/// Huge values are handled in time linear in their length, whether they
/// are accepted, rejected by a width check or rejected by the decimal
/// digit limit, and error messages stay short.
#[test]
fn huge_values_are_fast_and_errors_short() {
    let mb = 1 << 20;
    let cases: Vec<(String, Option<ParseErrorKind>)> = vec![
        // Decimal values: over the digit limit, or too wide.
        (
            format!("a = {}", "9".repeat(mb)),
            Some(ParseErrorKind::DecimalValueTooLong),
        ),
        (
            format!("a[4294967294:0] = 'd{}", "7_".repeat(mb)),
            Some(ParseErrorKind::DecimalValueTooLong),
        ),
        (
            format!("a = {}", "9".repeat(4300)),
            Some(ParseErrorKind::ValueExceedsAddressWidth),
        ),
        (
            format!("a[3:0] = 4'd{}", "9".repeat(4000)),
            Some(ParseErrorKind::ValueExceedsDeclaredWidth),
        ),
        // Leading zeros are not significant.
        (format!("a = {}1", "0".repeat(mb)), None),
        // Power of two radixes: accepted, and rejected before or after
        // conversion.
        (format!("a[{}:0] = 'h{}", 4 * mb, "F".repeat(mb)), None),
        (
            format!("a[3:0] = 4'h{}", "F".repeat(mb)),
            Some(ParseErrorKind::ValueExceedsDeclaredWidth),
        ),
        (
            format!("a[3:0] = 'b{}", "1".repeat(mb)),
            Some(ParseErrorKind::ValueExceedsAddressWidth),
        ),
        // The exact bit length (260) exceeds the address width (258) but
        // the digit count bound (257) does not.
        (
            format!("a[257:0] = 'hF{}", "0".repeat(64)),
            Some(ParseErrorKind::ValueExceedsAddressWidth),
        ),
        (
            format!("a[{}]", "9".repeat(mb)),
            Some(ParseErrorKind::AddressOutOfRange),
        ),
    ];
    for (input, expected) in cases {
        let start = std::time::Instant::now();
        let result = parse_fasm_string(&input);
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "{elapsed:?} for {}...",
            &input[..40.min(input.len())]
        );
        match (result, expected) {
            (Ok(_), None) => {}
            (Err(e), Some(kind)) => {
                assert_eq!(e.kind, kind, "{e}");
                assert!(e.message.len() < 200, "message too long: {}", e.message);
            }
            (r, _) => panic!("unexpected {:?} for {}...", r.map(|_| ()), &input[..40]),
        }
    }

    let e = parse_fasm_string(&format!("a[257:0] = 'hF{}", "0".repeat(64))).unwrap_err();
    assert_eq!(
        e.message,
        "260 bit value 0xf000000000000000... does not fit in the 258 bit(s) addressed by the \
         feature"
    );
    let e = parse_fasm_string("a[3:0] = 4'h1F").unwrap_err();
    assert_eq!(
        e.message,
        "value 31 does not fit in the declared width of 4 bit(s)"
    );
}

#[test]
fn filename_api() {
    let dir = std::env::temp_dir().join(format!("fasm-parser-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.fasm");
    std::fs::write(&path, b"a = 1\r\n{ b = \"c\" }\r\n").unwrap();
    let lines = parse_fasm_filename(&path).unwrap();
    assert_eq!(render(&Ok(lines)), r#"a=1/PLAIN | {b="c"}"#);
    std::fs::remove_dir_all(&dir).unwrap();

    let e = parse_fasm_filename(dir.join("missing.fasm")).unwrap_err();
    assert_eq!((e.line, e.column, e.kind), (0, 0, ParseErrorKind::Io));
    assert!(e
        .to_string()
        .starts_with("Parse error at 0:0 - Couldn't open file"));
}

#[test]
fn many_lines_and_line_numbers() {
    let mut text = String::new();
    for i in 0..1000 {
        writeln!(text, "T.X{i}Y{i}.F[{i}] = 1 # line {}", i + 1).unwrap();
    }
    text.push_str("\n\nbad = = 1\n");
    let result = parse_fasm_string(&text);
    let e = result.unwrap_err();
    assert_eq!((e.line, e.column), (1003, 6));

    let good = &text[..text.find("\n\nbad").unwrap()];
    let lines = parse_fasm_string(good).unwrap();
    assert_eq!(lines.len(), 1000);
    assert_eq!(
        render_line(&lines[999]),
        r##"T.X999Y999.F[999]=1/PLAIN #" line 1000""##
    );
}

// ---------------------------------------------------------------------
// Robustness
// ---------------------------------------------------------------------

/// Deterministic xorshift generator (no dev-dependency needed).
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Random bytes (biased towards FASM syntax characters) never make the
/// parser panic, and every API agrees on the result.
#[test]
fn random_bytes_never_panic() {
    const ALPHABET: &[u8] = b"aZ_09.[]:='hbdo {}\",#\\\n\r\t 1F7x\x00\xc3\xa9\xff";
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    for _ in 0..20_000 {
        let len = (rng.next() % 48) as usize;
        let input: Vec<u8> = (0..len)
            .map(|_| {
                let r = rng.next();
                if r.is_multiple_of(8) {
                    (r >> 8) as u8
                } else {
                    ALPHABET[((r >> 8) % ALPHABET.len() as u64) as usize]
                }
            })
            .collect();
        let whole = parse_fasm_bytes(&input);
        let streamed: Result<Vec<FasmLine>, ParseError> = parse_lines(&input).collect();
        assert_eq!(whole, streamed);
        let _ = parse_line(&input, 1);
        if let Ok(lines) = &whole {
            for f in lines.iter().filter_map(|l| l.set_feature.as_ref()) {
                let _ = f.width();
            }
        }
        if let Err(e) = &whole {
            assert!(e.line >= 1);
            let _ = e.to_string();
        }
    }
}

/// Mutations of valid lines (more likely to get deep into the grammar
/// than purely random bytes) never make the parser panic.
#[test]
fn mutated_lines_never_panic() {
    let seeds: &[&[u8]] = &[
        b"CLBLL_L_X12Y124.SLICEL_X0.ALUT.INIT[63:32] = 32'b11110000111100001111000011110000\n",
        b"a.b[7:0] = 8'hff { x = \"y\\\"z\", .q = \"\" } # comment\r\n",
        b"a[1023:0] = 'd123456789012345678901234567890 # c\n",
        b"{ a = \"multi\nline\" } # c\n",
        b"a[4294967295:1] = 'o777_777\n",
    ];
    let mut rng = XorShift(12345);
    for _ in 0..20_000 {
        let seed = seeds[(rng.next() % seeds.len() as u64) as usize];
        let mut input = seed.to_vec();
        for _ in 0..1 + rng.next() % 4 {
            let r = rng.next();
            let pos = (r % (input.len() as u64 + 1)) as usize;
            match (r >> 32) % 3 {
                0 if pos < input.len() => {
                    input.remove(pos);
                }
                1 => input.insert(pos, (r >> 40) as u8),
                _ if pos < input.len() => input[pos] = (r >> 40) as u8,
                _ => {}
            }
        }
        let whole = parse_fasm_bytes(&input);
        let streamed: Result<Vec<FasmLine>, ParseError> = parse_lines(&input).collect();
        assert_eq!(whole, streamed);
    }
}

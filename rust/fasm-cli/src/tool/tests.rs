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

//! Tests of the tool's behaviour. Expected outputs come from the original
//! tool (`tests/oracle/fasm-oracle`); `tests/cli/test_cli_compat.py`
//! compares the two binaries directly.

use super::*;

const MANY: &[u8] = include_bytes!("../../../../examples/many.fasm");
/// `fasm_tuple_to_string` of `examples/many.fasm` (without `print`'s
/// newline), from the oracle.
const MANY_OUT: &str = include_str!("../../../../tests/corpus/oracle/many.fasm.out.txt");
const MANY_CANONICAL: &str =
    include_str!("../../../../tests/corpus/oracle/many.fasm.canonical.txt");

struct Output {
    code: u8,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_tool(args: &[&str]) -> Output {
    let args: Vec<PyStr> = args.iter().map(|a| PyStr::from_str(a)).collect();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(&args, || 80, &mut stdout, &mut stderr);
    Output {
        code,
        stdout,
        stderr,
    }
}

/// A file in a fresh temporary directory, removed on drop.
struct TempFile {
    dir: std::path::PathBuf,
    path: std::path::PathBuf,
}

impl TempFile {
    fn new(name: &str, contents: &[u8]) -> Self {
        let unique = format!(
            "fasm-cli-test-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        TempFile { dir, path }
    }

    fn path(&self) -> &str {
        self.path.to_str().unwrap()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn render_ok(data: &[u8], canonical: bool) -> String {
    render(data, canonical).unwrap()
}

fn render_err(data: &[u8]) -> String {
    render(data, false).unwrap_err()
}

/// The `Parse error at L:C` prefix of an error.
fn position(message: &str) -> &str {
    message.split(" - ").next().unwrap()
}

#[test]
fn renders_like_the_oracle() {
    assert_eq!(render_ok(MANY, false), format!("{MANY_OUT}\n"));
    assert_eq!(render_ok(MANY, true), format!("{MANY_CANONICAL}\n"));
}

#[test]
fn empty_inputs() {
    for data in [&b""[..], b"\n\n", b"  \n"] {
        assert_eq!(render_ok(data, false), "\n\n");
        assert_eq!(render_ok(data, true), "\n\n");
    }
    // Lines without a set feature, or with a zero value, have no canonical
    // form.
    assert_eq!(render_ok(b"# c\n{ a = \"b\" }\nA = 0\n", true), "\n\n");
    assert_eq!(
        render_ok(b"# c\n{ a = \"b\" }\nA = 0\n", false),
        "# c\n{ a = \"b\" }\nA = 0\n\n"
    );
}

#[test]
fn line_parts() {
    assert_eq!(render_ok(b"#", false), "#\n\n");
    assert_eq!(
        render_ok(b"A.B[3:0] = 4'hA { x = \"1\", y = \"\" } # c\n", false),
        "A.B[3:0] = 4'hA { x = \"1\", y = \"\" } # c\n\n"
    );
    assert_eq!(render_ok(b"A # c\r\nB", false), "A # c\nB\n\n");
    assert_eq!(
        render_ok(b"{ a = \"b\" } #x", false),
        "{ a = \"b\" } #x\n\n"
    );
}

#[test]
fn canonical_is_sorted_and_deduplicated() {
    assert_eq!(
        render_ok(b"B[2:0] = 3'b101\nA\nB[2]\nA[0]\n", true),
        "A\nB\nB[2]\n\n"
    );
}

#[test]
fn syntax_errors() {
    let error = render_err(b"a b\n");
    assert!(error.starts_with("Parse error at 1:2 - "), "{error}");
    let error = render_err(b"A = 1\nB = 1\nC D\n");
    assert!(error.starts_with("Parse error at 3:2 - "), "{error}");
}

#[test]
fn range_error_reported_with_position() {
    // The original tool prints `Error: 'NoneType' object is not iterable`.
    let error = render_err(b"A = 1\nB = 2\n");
    assert!(error.starts_with("Parse error at 2:4 - "), "{error}");
    let error = render_err(b"A[3:0] = 5'h10\n");
    assert!(error.starts_with("Parse error at 1:9 - "), "{error}");
}

#[test]
fn later_syntax_error_wins_over_range_error() {
    // Positions from the original (ANTLR) parser.
    let cases: &[(&[u8], &str)] = &[
        (b"a = 2\nb c\n", "Parse error at 2:2"),
        (b"a = 2\rb c\n", "Parse error at 1:8"),
        (b"a = 2\r\nb c\n", "Parse error at 2:2"),
        (b"a = 2 # x\nb c\n", "Parse error at 2:2"),
        (b"a = 2 { x = \"1\ny\" } # q\nb c\n", "Parse error at 3:2"),
        (b"a = 2 { x = \"\\\"\n\" }\n\nb c", "Parse error at 4:2"),
        (b"a = 2\nb = 3\nc = 4\nd e\n", "Parse error at 4:2"),
        (b"\xEF\xBB\xBFa = 2\nb c\n", "Parse error at 2:2"),
        (b"\xEF\xBB\xBFa = 2\rb c\n", "Parse error at 1:8"),
        (b"a[0:1]\nb c\n", "Parse error at 2:2"),
        // Within a line the syntax error wins anyway.
        (b"a = 2 { x = \"y\" } z\n", "Parse error at 1:18"),
    ];
    for (data, expected) in cases {
        let error = render_err(data);
        assert_eq!(
            position(&error),
            *expected,
            "{:?}: {error}",
            String::from_utf8_lossy(data)
        );
    }
    // No later syntax error: the first range error is reported.
    for (data, expected) in [
        (&b"a = 2\nb = 3\n"[..], "Parse error at 1:4"),
        (b"a = 2", "Parse error at 1:4"),
        (b"x\na = 2\n# c\n", "Parse error at 2:4"),
    ] {
        let error = render_err(data);
        assert_eq!(position(&error), expected, "{error}");
    }
}

#[test]
fn logical_line_end_skips_annotation_values() {
    let data = b"a = 2 { x = \"1\n\\\"#\r2\" } # c\"\nrest";
    assert_eq!(logical_line_end(data, 4), 28);
    assert_eq!(logical_line_end(b"a = 2", 4), 5);
    assert_eq!(logical_line_end(b"a = 2 # c", 4), 9);
}

#[test]
fn byte_offsets() {
    assert_eq!(byte_offset(b"ab\ncd", 2, 1), Some(4));
    assert_eq!(byte_offset(b"\xEF\xBB\xBFab", 1, 1), Some(4));
    assert_eq!(byte_offset("é\u{1F600}x".as_bytes(), 1, 2), Some(6));
    assert_eq!(byte_offset(b"ab", 2, 0), None);
}

#[test]
fn prints_the_file_and_an_empty_line() {
    let file = TempFile::new("many.fasm", MANY);
    let out = run_tool(&[file.path()]);
    assert_eq!((out.code, out.stderr.as_slice()), (0, &b""[..]));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{MANY_OUT}\n")
    );
    let out = run_tool(&["--canonical", "--parser", "textx", file.path()]);
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{MANY_CANONICAL}\n")
    );
}

#[test]
fn errors_go_to_stdout_with_exit_code_0() {
    let file = TempFile::new("bad.fasm", b"a b\n");
    let out = run_tool(&[file.path()]);
    assert_eq!((out.code, out.stderr.as_slice()), (0, &b""[..]));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.starts_with("Error: Parse error at 1:2 - "),
        "{stdout}"
    );
    assert!(stdout.ends_with('\n') && stdout.lines().count() == 1);

    let out = run_tool(&["/nonexistent/x.fasm"]);
    assert_eq!(out.code, 0);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.starts_with("Error: Parse error at 0:0 - Couldn't open file /nonexistent/x.fasm: "),
        "{stdout}"
    );
}

#[test]
fn parser_selection() {
    let file = TempFile::new("a.fasm", b"A\n");
    for parser in ["antlr", "textx", "rust", ""] {
        let out = run_tool(&["--parser", parser, file.path()]);
        assert_eq!(out.stdout, b"A\n\n", "{parser:?}");
    }
    let out = run_tool(&["--parser", "foo", "/nonexistent"]);
    assert_eq!(
        (out.code, out.stdout.as_slice()),
        (0, &b"Error: Parser 'foo' is not available.\n"[..])
    );
    let args = vec![
        PyStr::from_str("--parser"),
        PyStr::from_bytes_surrogateescape(b"x\xff'"),
        PyStr::from_str("f"),
    ];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(run(&args, || 80, &mut stdout, &mut stderr), 0);
    assert_eq!(stdout, b"Error: Parser 'x\xff'' is not available.\n");
}

#[test]
fn help_and_usage_errors() {
    let out = run_tool(&["--help"]);
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout, argparse::format_help(80).as_bytes());
    assert!(out.stderr.is_empty());

    let out = run_tool(&[]);
    assert_eq!(out.code, 2);
    assert!(out.stdout.is_empty());
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "usage: FASM tool [-h] [--canonical] [--parser PARSER] file\n\
         FASM tool: error: the following arguments are required: file\n"
    );
}

/// A writer failing like a pipe whose reader has gone away.
struct BrokenPipe;

impl Write for BrokenPipe {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn broken_pipe_exits_with_1_quietly() {
    let file = TempFile::new("a.fasm", b"A\n");
    let args = [PyStr::from_str(file.path())];
    let mut stderr = Vec::new();
    assert_eq!(run(&args, || 80, &mut BrokenPipe, &mut stderr), 1);
    assert!(stderr.is_empty());
    // The help ignores write errors, like argparse.
    let args = [PyStr::from_str("-h")];
    assert_eq!(run(&args, || 80, &mut BrokenPipe, &mut stderr), 0);
}

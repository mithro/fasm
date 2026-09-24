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

//! Tests of the argparse emulation. Every expected value was produced by
//! the original tool (`tests/oracle/fasm-oracle`, Python 3.11.15); the
//! differential test `tests/cli/test_cli_compat.py` covers many more.

use super::*;

fn py(s: &str) -> PyStr {
    PyStr::from_str(s)
}

fn parse(args: &[&str]) -> Parsed {
    let args: Vec<PyStr> = args.iter().map(|a| py(a)).collect();
    parse_args(&args)
}

fn run(file: &str, canonical: bool, parser: Option<&str>) -> Parsed {
    Parsed::Run(Namespace {
        file: py(file),
        canonical,
        parser: parser.map(py),
    })
}

fn error(message: &str) -> Parsed {
    Parsed::Error(py(message))
}

#[test]
fn plain_arguments() {
    assert_eq!(parse(&["f"]), run("f", false, None));
    assert_eq!(parse(&["--canonical", "f"]), run("f", true, None));
    assert_eq!(parse(&["f", "--canonical"]), run("f", true, None));
    assert_eq!(
        parse(&["--parser", "antlr", "f"]),
        run("f", false, Some("antlr"))
    );
    assert_eq!(
        parse(&["f", "--parser", "textx", "--canonical"]),
        run("f", true, Some("textx"))
    );
}

#[test]
fn option_forms() {
    assert_eq!(
        parse(&["--parser=antlr", "f"]),
        run("f", false, Some("antlr"))
    );
    assert_eq!(parse(&["--parser=a=b", "f"]), run("f", false, Some("a=b")));
    assert_eq!(parse(&["--canon", "f"]), run("f", true, None));
    assert_eq!(parse(&["--c", "f"]), run("f", true, None));
    assert_eq!(parse(&["--pars", "x", "f"]), run("f", false, Some("x")));
    assert_eq!(parse(&["--p=x", "f"]), run("f", false, Some("x")));
    // Repeated options: the last one wins.
    assert_eq!(
        parse(&[
            "--parser",
            "a",
            "--canonical",
            "--canonical",
            "--parser",
            "b",
            "f"
        ]),
        run("f", true, Some("b"))
    );
    // `nullable_string`: an empty value is the default parser.
    assert_eq!(parse(&["--parser", "", "f"]), run("f", false, None));
    assert_eq!(parse(&["--parser=", "f"]), run("f", false, None));
    assert_eq!(
        parse(&["--parser", "x", "--parser=", "f"]),
        run("f", false, None)
    );
    // Values that look like negative numbers or contain a space.
    assert_eq!(parse(&["--parser", "-1", "f"]), run("f", false, Some("-1")));
    assert_eq!(
        parse(&["--parser", "-.5", "f"]),
        run("f", false, Some("-.5"))
    );
    assert_eq!(
        parse(&["--parser", "-1\n", "f"]),
        run("f", false, Some("-1\n"))
    );
    assert_eq!(
        parse(&["--parser", "-\u{0663}", "f"]),
        run("f", false, Some("-\u{0663}"))
    );
    assert_eq!(
        parse(&["--parser", "-a b", "f"]),
        run("f", false, Some("-a b"))
    );
    // A lone '-' is a positional argument.
    assert_eq!(parse(&["-"]), run("-", false, None));
    assert_eq!(parse(&["-1"]), run("-1", false, None));
}

#[test]
fn double_dash() {
    assert_eq!(parse(&["--", "f"]), run("f", false, None));
    assert_eq!(parse(&["f", "--"]), run("f", false, None));
    assert_eq!(parse(&["--", "--"]), run("--", false, None));
    assert_eq!(parse(&["--", "-h"]), run("-h", false, None));
    assert_eq!(
        parse(&["--", "--canonical"]),
        run("--canonical", false, None)
    );
    assert_eq!(parse(&["--canonical", "--", "f"]), run("f", true, None));
    assert_eq!(parse(&["--", "a", "b"]), error("unrecognized arguments: b"));
    assert_eq!(parse(&["a", "--", "b"]), error("unrecognized arguments: b"));
    assert_eq!(
        parse(&["a", "--", "--"]),
        error("unrecognized arguments: --")
    );
    assert_eq!(
        parse(&["--canonical", "--", "f", "--"]),
        error("unrecognized arguments: --")
    );
    assert_eq!(
        parse(&["a", "--", "b", "--", "c"]),
        error("unrecognized arguments: b -- c")
    );
    assert_eq!(
        parse(&["--"]),
        error("the following arguments are required: file")
    );
}

#[test]
fn help() {
    for args in [
        &["-h"][..],
        &["--help"],
        &["--h"],
        &["--he"],
        &["f", "-h"],
        &["--bogus", "-h"],
        &["-h", "--parser"],
        &["a", "b", "-h"],
        &["-hx"],
        &["-hh"],
        &["-hé"],
        &["-hhx"],
    ] {
        assert_eq!(parse(args), Parsed::Help, "{args:?}");
    }
}

#[test]
fn errors() {
    let cases: &[(&[&str], &str)] = &[
        (&[], "the following arguments are required: file"),
        (
            &["--canonical"],
            "the following arguments are required: file",
        ),
        (&["--bogus"], "the following arguments are required: file"),
        (
            &["--parser", "f"],
            "the following arguments are required: file",
        ),
        (&["--parser"], "argument --parser: expected one argument"),
        (
            &["--parser", "-x", "f"],
            "argument --parser: expected one argument",
        ),
        (
            &["--parser", "-h"],
            "argument --parser: expected one argument",
        ),
        (
            &["--parser", "--", "f"],
            "argument --parser: expected one argument",
        ),
        (&["a", "b", "c"], "unrecognized arguments: b c"),
        (&["--bogus", "f"], "unrecognized arguments: --bogus"),
        (&["-x", "f"], "unrecognized arguments: -x"),
        (&["f", "-x"], "unrecognized arguments: -x"),
        (&["---", "f"], "unrecognized arguments: ---"),
        (&["-c", "f"], "unrecognized arguments: -c"),
        (&["-p", "f"], "unrecognized arguments: -p"),
        (
            &["x", "--bogus=1", "y"],
            "unrecognized arguments: --bogus=1 y",
        ),
        (
            &["--=x"],
            "ambiguous option: --=x could match --help, --canonical, --parser",
        ),
        (
            &["-h", "--=x"],
            "ambiguous option: --=x could match --help, --canonical, --parser",
        ),
        (
            &["--canonical=1", "f"],
            "argument --canonical: ignored explicit argument '1'",
        ),
        (
            &["--canonical=", "f"],
            "argument --canonical: ignored explicit argument ''",
        ),
        (
            &["--c=x", "f"],
            "argument --canonical: ignored explicit argument 'x'",
        ),
        (
            &["-h=x"],
            "argument -h/--help: ignored explicit argument 'x'",
        ),
        (&["-h="], "argument -h/--help: ignored explicit argument ''"),
        (
            &["--help="],
            "argument -h/--help: ignored explicit argument ''",
        ),
        (
            &["-h-"],
            "argument -h/--help: ignored explicit argument '-'",
        ),
        (
            &["-h=-"],
            "argument -h/--help: ignored explicit argument '-'",
        ),
        (
            &["-hh=x"],
            "argument -h/--help: ignored explicit argument 'x'",
        ),
        (
            &["-hh="],
            "argument -h/--help: ignored explicit argument ''",
        ),
        (
            &["--canonical=it's", "f"],
            "argument --canonical: ignored explicit argument \"it's\"",
        ),
    ];
    for (args, message) in cases {
        assert_eq!(parse(args), error(message), "{args:?}");
    }
}

#[test]
fn non_utf8_arguments() {
    let bad = PyStr::from_bytes_surrogateescape(b"x\xff");
    let args = vec![py("--parser"), bad.clone(), py("f")];
    assert_eq!(
        parse_args(&args),
        Parsed::Run(Namespace {
            file: py("f"),
            canonical: false,
            parser: Some(bad.clone()),
        })
    );
    let mut arg = py("--canonical=");
    arg.0.extend_from_slice(&bad.0);
    let Parsed::Error(message) = parse_args(&[arg, py("f")]) else {
        panic!("expected an error");
    };
    assert_eq!(
        format_error(&message, 80),
        "usage: FASM tool [-h] [--canonical] [--parser PARSER] file\n\
         FASM tool: error: argument --canonical: ignored explicit argument 'x\\udcff'\n"
    );
    let Parsed::Error(message) = parse_args(&[py("f"), bad]) else {
        panic!("expected an error");
    };
    assert_eq!(
        format_error(&message, 80),
        "usage: FASM tool [-h] [--canonical] [--parser PARSER] file\n\
         FASM tool: error: unrecognized arguments: x\\udcff\n"
    );
}

const HELP_80: &str = "\
usage: FASM tool [-h] [--canonical] [--parser PARSER] file

positional arguments:
  file             Filename to process

options:
  -h, --help       show this help message and exit
  --canonical      Return canonical form of FASM.
  --parser PARSER  Select FASM parser to use. Default is to choose the best
                   implementation available.
";

const HELP_1: &str = "\
usage: FASM tool
       [-h]
       [--canonical]
       [--parser PARSER]
       file

positional arguments:
  file
    Filename to
    process

options:
  -h, --help
    show this
    help
    message and
    exit
  --canonical
    Return
    canonical
    form of
    FASM.
  --parser PARSER
    Select FASM
    parser to
    use.
    Default is
    to choose
    the best im
    plementatio
    n
    available.
";

const HELP_24: &str = "\
usage: FASM tool [-h]
                 [--canonical]
                 [--parser PARSER]
                 file

positional arguments:
  file
    Filename to
    process

options:
  -h, --help
    show this help
    message and exit
  --canonical
    Return canonical
    form of FASM.
  --parser PARSER
    Select FASM parser
    to use. Default is
    to choose the best
    implementation
    available.
";

const HELP_30: &str = "\
usage: FASM tool [-h]
                 [--canonical]
                 [--parser PARSER]
                 file

positional arguments:
  file  Filename to process

options:
  -h, --help
        show this help
        message and exit
  --canonical
        Return canonical
        form of FASM.
  --parser PARSER
        Select FASM parser
        to use. Default is
        to choose the best
        implementation
        available.
";

const HELP_45: &str = "\
usage: FASM tool [-h] [--canonical]
                 [--parser PARSER]
                 file

positional arguments:
  file             Filename to process

options:
  -h, --help       show this help message
                   and exit
  --canonical      Return canonical form of
                   FASM.
  --parser PARSER  Select FASM parser to
                   use. Default is to
                   choose the best
                   implementation
                   available.
";

const HELP_100: &str = "\
usage: FASM tool [-h] [--canonical] [--parser PARSER] file

positional arguments:
  file             Filename to process

options:
  -h, --help       show this help message and exit
  --canonical      Return canonical form of FASM.
  --parser PARSER  Select FASM parser to use. Default is to choose the best implementation
                   available.
";

#[test]
fn help_text() {
    assert_eq!(format_help(80), HELP_80);
    assert_eq!(format_help(1), HELP_1);
    assert_eq!(format_help(-5), HELP_1);
    assert_eq!(format_help(24), HELP_24);
    assert_eq!(format_help(30), HELP_30);
    assert_eq!(format_help(45), HELP_45);
    assert_eq!(format_help(100), HELP_100);
    assert_eq!(format_help(i64::MAX), format_help(1000));
}

#[test]
fn usage_text_wrapping() {
    assert_eq!(
        format_usage(80),
        "usage: FASM tool [-h] [--canonical] [--parser PARSER] file\n"
    );
    assert_eq!(
        format_usage(60),
        "usage: FASM tool [-h] [--canonical] [--parser PARSER] file\n"
    );
    assert_eq!(
        format_usage(59),
        "usage: FASM tool [-h] [--canonical] [--parser PARSER]\n                 file\n"
    );
    assert_eq!(
        format_usage(23),
        "usage: FASM tool\n       [-h]\n       [--canonical]\n       [--parser PARSER]\n       file\n"
    );
    assert_eq!(
        format_error(&py("the following arguments are required: file"), 45),
        "usage: FASM tool [-h] [--canonical]\n                 [--parser PARSER]\n                 \
         file\nFASM tool: error: the following arguments are required: file\n"
    );
}

#[test]
fn text_wrap() {
    assert_eq!(wrap("aaa bbb", 3), vec!["aaa", "bbb"]);
    assert_eq!(wrap("aaa bbb", 7), vec!["aaa bbb"]);
    assert_eq!(wrap("abcdefgh", 3), vec!["abc", "def", "gh"]);
    // The long word starts on the current line when there is room.
    assert_eq!(wrap("ab cdefgh", 4), vec!["ab c", "defg", "h"]);
    // A full line: `_handle_long_word` appends an empty head.
    assert_eq!(wrap("abc defgh", 3), vec!["abc", "def", "gh"]);
}

# Compatibility of the Rust rewrite with the original implementation

This file records every known behavioural difference between the Rust
rewrite and the original Python `fasm` package (the "oracle", pinned to
commit `ffafe82`, see `tests/oracle/`), and the rules used to decide them.
Each entry gives an example and what the textX parser
(`fasm.parser.textx`), the ANTLR parser (`fasm.parser.antlr`, the default
when built) and the Rust implementation do with it. All examples were run
through the oracle (`tests/oracle/dump.py --parser antlr|textx`).

## Parser (`rust/fasm/src/parser/`, T1.3)

### Rule

1. Everything the ANTLR parser accepts is accepted and produces the
   identical `FasmLine`s (same feature, start, end, value, value format,
   annotations and comment), except where the ANTLR implementation is
   plainly buggy (32 bit truncation, whitespace decoded as a digit,
   process aborts, ...), where the Rust parser does the sane thing (the
   textX result, when textX has one).
2. Additionally accepted: inputs that the textX parser **and** the
   specification (`docs/specification/syntax.rst`) accept but ANTLR
   rejects (`_` separators in plain decimals and addresses).
3. Everything both original parsers reject is rejected. Inputs that only
   textX accepts and that the specification does not allow are rejected
   (see "textX only" below).
4. Line and column of syntax errors are those ANTLR reports (verified for
   every syntax error case of the edge case table,
   `rust/fasm/src/parser/tests.rs`). Error message texts are the Rust
   parser's own (see "Errors").

The edge case table in `rust/fasm/src/parser/tests.rs` marks each case as
`Same` (identical to ANTLR), `NoPos` (both reject; ANTLR without a
position) or `Differs` (listed here). Its ignored test
`dump_edge_cases_for_oracle` writes the Rust results as JSON lines for a
comparison with the oracle.

### Accepted by Rust, rejected by ANTLR (textX and the spec accept)

| Example | textX | ANTLR | Rust |
|---|---|---|---|
| `a[15:0] = 1_000` | value 1000, `PLAIN` | `Parse error at 1:5 - mismatched input '_'` | value 1000, `PLAIN` |
| `a[1_0]` | start 10 | `Parse error at 1:3 - mismatched input '_'` | start 10 |

`_` is accepted between two digits only (Python `int()` rules, which is
what textX applies): `1__0`, `1_`, `_1` are rejected by all three. `_` is
not accepted in the width of a Verilog value (`1_0'hF`): textX rejects it
too.

### Accepted by Rust and ANTLR, rejected by textX (ANTLR followed)

| Example | textX | ANTLR | Rust |
|---|---|---|---|
| `{ a = "x\"y" }` | syntax error (its `[^\"]` alternative eats the `\`) | value `x\"y` | value `x\"y` (verbatim, escapes are not decoded) |
| `{ a = "x\\" }` | value `x\\` | value `x\\` | value `x\\` |
| `a[0] = 3'b001` (declared width > address width) | `AssertionError` | value 1 | value 1 |
| `a[ 3 : 0 ] = 4'hF`, `a [3:0]`, `\ta.b\t[\t1\t]` | syntax error | accepted (whitespace is skipped between tokens) | accepted |
| `{ a="1" , b="2" }` (whitespace before `,`) | syntax error | accepted | accepted |
| `a = 'h_`, `'d_`, `'o__` (no digit, only `_`) | `ValueError` | value 0 | value 0 |
| `﻿a` (UTF-8 byte order mark at the start of the file) | syntax error | accepted (the ANTLR input stream drops it) | accepted |
| `  \t` (whitespace only line) | yields a `FasmLine(None, None, None)` | no line | no line |
| `{ a = "x\r\ny" }` (CR LF inside an annotation value) | value `x\ny` (text mode file read) | value `x\r\ny` | value `x\r\ny` |

A declared width of 0 (`a[3:0] = 0'hF`) is not checked by either
original parser (`if width:`); the Rust parser does the same.

### ANTLR bugs: Rust does the sane thing

| Example | textX | ANTLR | Rust |
|---|---|---|---|
| `a[31:0] = 2147483648` (plain decimal > 2^31 - 1, `std::stoi`) | 2147483648 | `Parse error at 1:10 - Could not decode decimal number.` | 2147483648 |
| `a[63:0] = 'd4294967296` (`'d` value > 2^32 - 1) | 4294967296 | 0 (truncated to 32 bits) | 4294967296 |
| `a[1023:0] = 'd<2^1023 in decimal>` (`'d` value > 2^64 - 1) | exact | `Could not decode decimal number.` | exact |
| `a[63:0] = 'o1234567012345670123` (octal > 32 bits) | 23528931761549395 | 23528930687807571 (wrong) | 23528931761549395 |
| `a[63:0] = 'h F` (whitespace after `'h`, allowed by the ANTLR lexer and the spec) | 15 | 4294967295 (the space is decoded as the digit -1) | 15 |
| `a[3:0] = 4'h F` | 15 | `AssertionError` (garbage value) | 15 |
| `a[7:0] = 'd 5` | 5 | `Could not decode decimal number.` | 5 |
| `a[7:0] = 'o\t7` | 7 | `AssertionError` (garbage value) | 7 |
| `a[7:0] = 'b 101` | 5 | 5 (works by accident) | 5 |
| `a[4294967296] = 0` (address > 2^32 - 1) | start 4294967296 | start 0 (truncated to 32 bits) | `AddressOutOfRange` error |
| `a[18446744073709551616] = 0` (address > 2^64 - 1) | start 18446744073709551616 | process aborts (`std::out_of_range` from `stoul`) | `AddressOutOfRange` error |
| `a[3:0] = 99999999999'h1` (width > 2^31 - 1) | runs out of memory computing `2**width` | process aborts (`std::out_of_range` from `stoi`) | value 1 (a width of 2^32 or more never limits the value) |

Addresses are limited to `u32` (`SetFasmFeature::start`/`end` are `u32`,
see `DESIGN-model.md`); larger addresses are an error rather than being
truncated.

### Rejected by Rust, accepted by ANTLR (model invariants)

| Example | textX | ANTLR | Rust |
|---|---|---|---|
| `a[0:1] = 0` (end < start, value 0) | start 1, end 0 | start 1, end 0 | `AddressEndBeforeStart` error at 1:1 |
| `a[0:1]` (end < start, no value) | start 1, end 0, value 1 (no check without a value) | `AssertionError` | `AddressEndBeforeStart` error |
| `a[4294967295:0]` (a 2^32 bit wide range) | accepted | accepted | `AddressOutOfRange` error at 1:1 |

Both would build a `SetFasmFeature` whose width (`end - start + 1`) is not
a valid `u32` bit count; `SetFasmFeature::new` rejects them and
`SetFasmFeature::width()` would panic on them. With a non zero value, both
original parsers reject `end < start` (`value < 2**(end - start + 1)`
fails); only the value 0 slips through.

### textX only: rejected by Rust (ANTLR and the spec reject)

| Example | textX | ANTLR | Rust |
|---|---|---|---|
| `a b`, `{ a = "x" } b`, `{ a = "b" } { c = "d" }` | several `FasmLine`s from one line (its `FasmFile` rule does not need a newline between lines) | `Parse error at 1:2` / `1:12` / `1:12` | syntax error, same position as ANTLR |
| `{ a = "x\q" }` (`\` not followed by `\` or `"`) | value `x\q` | `Parse error at 1:6 - token recognition error` | syntax error at 1:6 |

### Line endings and positions

* `\n` and `\r` both end a line (ANTLR's `NEWLINE : [\n\r]`), so `\r\n` and
  lone `\r` line endings give the same `FasmLine`s in all three parsers
  (`a\r\nb`, `a\rb`, `a\n\r\nb` all give `a` and `b`).
* Error positions follow ANTLR: the line number counts `\n` only and the
  column (0 based, in Unicode code points) counts from the last `\n`, so a
  lone `\r` does not start a new line for error positions: `a\rb c` is an
  error at 1:4, `a\r\nb c` at 2:2.
* Annotation values may contain line terminators (all three parsers accept
  `{ a = "x\ny" }`); line numbers after them count the `\n` inside.
* A file without a trailing newline, an empty file and blank lines are
  handled like both original parsers (blank lines produce no `FasmLine`;
  a bare `#` produces `comment = ""`).

### Non-ASCII input and encodings

| Example | textX | ANTLR | Rust |
|---|---|---|---|
| `# comment é` | comment ` comment é` | fails: the Cython decoder decodes as ASCII; the exception is raised inside the ctypes callback, printed and swallowed, and `parse_fasm_filename` returns `None` (`'NoneType' object is not iterable`); `parse_fasm_string` raises `UnicodeEncodeError` | comment ` comment é` |
| `{ a = "é" }` | value `é` | fails as above | value `é` |
| `é` (outside comments/annotation values) | syntax error | fails as above (the error message is not ASCII) | syntax error at 1:0 |
| `# \xff` (invalid UTF-8) | `UnicodeDecodeError` | process aborts (`std::range_error` from `wstring_convert`) | `InvalidUtf8` error at the offending byte (1:2) |
| `a \xff` | `UnicodeDecodeError` | process aborts | syntax error at 1:2 |
| `a # c\0d` (NUL in a comment) | comment ` c\0d` | comment ` c\0d` from a file; `parse_fasm_string` stops at the first NUL (the C++ side takes a C string; from the code, not run) | comment ` c\0d` |

Input is processed as bytes; only comments and annotation values (the only
places where non-ASCII characters are allowed) must be valid UTF-8 and end
up as `Box<str>`.

### Errors

* `ParseError`'s `Display` is `Parse error at {line}:{column} - {message}`,
  the format of the exception raised by the ANTLR wrapper. ANTLR's own
  message texts (`mismatched input '_' expecting {<EOF>, NEWLINE}`, ...)
  depend on its ATN and error recovery strategy and are not reproduced;
  the Rust messages say what was found and what was expected
  (`unexpected 'c', expected '[', '=', '{', '#' or end of line`).
* Range errors (value wider than the address or the declared width, end
  before start) are `AssertionError`s without position in both original
  parsers. textX's message is the assert tuple (`(2, 1)`); the ANTLR
  parser's assertion is raised inside the ctypes callback, printed to
  stderr and swallowed, and the API returns `None`: the original CLI then
  prints `Error: 'NoneType' object is not iterable`. The Rust parser
  reports them as `ValueExceedsAddressWidth`, `ValueExceedsDeclaredWidth`
  and `AddressEndBeforeStart` errors located at the value or at the `[`.
  What the Rust CLI prints for them is decided in T2.1.
* Error precedence: the ANTLR parser parses the whole file before
  checking value ranges, so a syntax error anywhere in the file is
  reported before a range error on an earlier line (`a = 2\nb c`: ANTLR
  reports the syntax error at 2:2). The Rust parser streams and reports
  the first error in file order (1:4 for that example). Within one line,
  a syntax error takes precedence over a range error, like ANTLR
  (`a = 2 { x = "y" } z` is a syntax error at 1:18 in both).
* A file that cannot be read gives a `ParseErrorKind::Io` error at 0:0
  (`Parse error at 0:0 - Couldn't open file <path>: <OS error>`); ANTLR
  gives `Parse error at 0:0 - Couldn't open file`, textX a
  `FileNotFoundError`.

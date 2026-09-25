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
3. Everything both original parsers reject is rejected, **modulo the
   documented relaxations above**: an input can combine a relaxation that
   only textX has (rule 2) with one that only ANTLR has, or with an ANTLR
   bug that Rust fixes (so each original parser rejects it for a different
   reason), and Rust then accepts it. Examples, all rejected by both
   originals and accepted by Rust:
   `a[1_0] = 'h_` (`_` in an address: ANTLR rejects; `'h_`: textX
   rejects), `a [ 3 : 0 ] = 4 'h F` (whitespace around `[`: textX rejects;
   whitespace after `'h`: ANTLR decodes a garbage value and fails its
   width assert), `A.B=2'h\t0` (a tab after `'h`: ANTLR decodes a garbage value;
   a declared width of 2 on a 1 bit feature: textX rejects). Inputs that only textX accepts
   and that the specification does not allow are rejected (see "textX
   only" below).
4. Line and column of syntax errors are those ANTLR reports (verified for
   every syntax error case of the edge case table,
   `rust/fasm/src/parser/tests.rs`, and over the review corpus of 5092
   generated files), including where ANTLR's error recovery moves the
   error to a later lexer error (see "Error positions inside annotation
   blocks"). Error message texts are the Rust parser's own (see
   "Errors").

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
| `{ a = "x\r\ny" }` (CR LF inside an annotation value) | value `x\ny` (text mode file read: universal newlines) | value `x\r\ny` | value `x\r\ny` |
| `{ a = "x\ry" }` (lone CR inside an annotation value) | value `x\ny` (universal newlines) | value `x\ry` | value `x\ry` |

A declared width of 0 (`a[3:0] = 0'hF`) is not checked by either
original parser (`if width:`); the Rust parser does the same.

### ANTLR bugs: Rust does the sane thing

| Example | textX | ANTLR | Rust |
|---|---|---|---|
| `a[31:0] = 2147483648` (plain decimal > 2^31 - 1, `std::stoi`) | 2147483648 | `Parse error at 1:10 - Could not decode decimal number.` | 2147483648 |
| `a[63:0] = 'd4294967296` (`'d` value > 2^32 - 1) | 4294967296 | 0 (truncated to 32 bits) | 4294967296 |
| `a[1023:0] = 'd<2^1023 in decimal>` (`'d` value > 2^64 - 1) | exact | `Could not decode decimal number.` | exact |
| `a[63:0] = 'o1234567012345670123` (octal > 32 bits) | 23528931761549395 | 23528930687807571 (wrong) | 23528931761549395 |
| `a[30:0] = 'o00_017777777777` (more than 10 octal digits, leading zeros included, even when the value fits 32 bits) | 2147483647 | 1073741823 (wrong) | 2147483647 |
| `a[32:0] = 31'o14404165671` (11 digits) | 1678830521 | 605088697 (wrong) | 1678830521 |
| `a[31:0] = 'o37777777777` | 4294967295 | 1073741823 (wrong) | 4294967295 |
| `a[63:0] = 'h F` (whitespace after `'h`, allowed by the ANTLR lexer and the spec) | 15 | 4294967295 (the space is decoded as the digit -1) | 15 |
| `a[3:0] = 4'h F` | 15 | `AssertionError` (garbage value) | 15 |
| `a[7:0] = 'd 5` | 5 | `Could not decode decimal number.` | 5 |
| `a[7:0] = 'o\t7` | 7 | `AssertionError` (garbage value) | 7 |
| `a[7:0] = 'b 101` | 5 | 5 (works by accident) | 5 |
| `a[4294967296] = 0` (address > 2^32 - 1) | start 4294967296 | start 0 (truncated to 32 bits) | `AddressOutOfRange` error |
| `a[18446744073709551616] = 0` (address > 2^64 - 1) | start 18446744073709551616 | process aborts (`std::out_of_range` from `stoul`) | `AddressOutOfRange` error |
| `a[3:0] = 99999999999'h1` (width > 2^31 - 1) | runs out of memory computing `2**width` | process aborts (`std::out_of_range` from `stoi`) | value 1 (a width of 2^32 or more never limits the value) |
| `a[20000:0] = 9…9` (4300 significant digits) | exact value | `Could not decode decimal number.` (above 2^31 - 1) | exact value |
| `a[20000:0] = 9…9` (4301 significant digits, plain or `'d`) | `ValueError: Exceeds the limit (4300 digits) for integer string conversion` | `Parse error at 1:13 - Could not decode decimal number.` | `DecimalValueTooLong` error at 1:13 |
| `a = 0…01` (4301 leading zeros, plain or `'d`) | `ValueError` (Python counts leading zeros) | value 1 | value 1 |
| `a[20000:0] = 0…09…9` (100 leading zeros, then 4300 nines; plain or `'d`) | `ValueError: … value has 4400 digits` | `Could not decode decimal number.` | exact value: the limit counts significant digits (so rejected by both originals, accepted by Rust) |

The ANTLR octal decoder shifts its 64 bit accumulator right instead of
masking it once a 32 bit word is emitted, so every `'o` value written with
more than 10 digits (leading zeros and all) is decoded wrongly, whatever
its magnitude.

Decimal values (plain and `'d`) are limited to 4300 significant digits
(Python's default `int()` limit, which textX hits; ANTLR's limits are far
lower): converting a decimal string is quadratic in its significant
digits, and the limit keeps a malicious line cheap. Leading zeros and `_`
are skipped in linear time and take no part in the conversion (10 MB of
leading zeros in front of 4300 nines on a `[4294967294:0]` address parse
in a few milliseconds; `rust/fasm/src/parser/tests.rs`,
`huge_values_are_fast_and_errors_short`). Values in the power of two
radixes have no limit (their conversion is linear), like in both original
parsers.

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

### Error positions inside annotation blocks

When the ANTLR parser meets an unexpected token T it tries "single token
deletion": it lexes the token after T to see whether dropping T would
help, before reporting anything. Inside `{ ... }` the lexer is in
annotation mode, where many characters (digits, `#`, `\n`, `{`, ...)
start no token; lexing the token after T then fails, and that lexer error
is what gets reported, at the later position. The Rust parser emulates
this (`Scanner::unexpected_la` in `rust/fasm/src/parser/line.rs`):

| Example | ANTLR | Rust |
|---|---|---|
| `{ a = "b" c` + newline | 1:11 (`\n` after `c`) | 1:11 |
| `{ "x" 1 }` | 1:6 | 1:6 |
| `{ a = b 1 }` | 1:8 | 1:8 |
| `{ a = = 1` | 1:8 | 1:8 |
| `{ a = "b" "c" 1` | 1:14 | 1:14 |
| `{ a = "b" c #` | 1:12 | 1:12 |
| `{ a = "b" c "x\q"` (the token after T is a bad annotation value) | 1:12 | 1:12 |
| `{ a = "b" c = "d" }` (the token after T lexes) | 1:10 (at T) | 1:10 |
| `{ a } 1` (T is `}`: the next token is lexed in default mode) | 1:4 (at T) | 1:4 |
| `a = {1` (T is a `{` where a value is expected: the next token is lexed in annotation mode) | 1:5 | 1:5 |

ANTLR only does this where its error strategy calls single token deletion:
at every token match and at the entry of a `( ... )*` loop, but not at
the loop back. So after the second annotation of a block
(`{ a="1", b="2" c 1 }`: 1:15, at `c`) and at the end of every line but
the first (`x\n{ a="b" } {1`: 2:10, at `{`; the same on the first line,
`{ a="b" } {1`, gives 1:11) there is no lookahead; Rust does the same.
All cases of the review corpus (5092 files) and 112 targeted probes give
the ANTLR position; no residual difference is known.

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
| `a # c\0d` (NUL in a comment) | comment ` c\0d` | comment ` c\0d` from a file (`parse_fasm_filename`, the CLI); `parse_fasm_string` passes a C string, so everything from the NUL on is silently dropped: `parse_fasm_string('a # c\x00d\nb c\n')` returns one line with comment ` c` and no error, hiding the syntax error on line 2 | comment ` c\0d` (and `parse_fasm_string` sees the whole input: the error at 2:2 is reported) |

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
* Messages never embed long input: values wider than 256 bits are shown
  as their bit length and leading hex digits
  (`260 bit value 0xf000000000000000... does not fit in the 258 bit(s)
  addressed by the feature`), a value whose digit count alone shows it is
  too wide is described by that count, and long addresses are truncated.
* A file that cannot be read gives a `ParseErrorKind::Io` error at 0:0
  (`Parse error at 0:0 - Couldn't open file <path>: <OS error>`); ANTLR
  gives `Parse error at 0:0 - Couldn't open file`, textX a
  `FileNotFoundError`.

## Command line tool (`fasm`, `rust/fasm-cli/`, T2.1)

### Rule

The `fasm` binary is a drop in replacement for the original `fasm`
console script (`fasm/tool.py`): same arguments, and byte for byte the
same stdout, stderr and exit code, as the original running under Python
3.11 (the oracle, Python 3.11.15). Its arguments are parsed by an
emulation of Python 3.11's argparse (`rust/fasm-cli/src/argparse.rs`), not
by a Rust argument parser, so that every argparse behaviour carries over:
unambiguous prefixes (`--canon`, `--pars textx`, `--h`), `--opt=value`,
`--`, `-h` anywhere on the command line, repeated options (the last one
wins), values that look like negative numbers (`--parser -1`), every usage
error message (`the following arguments are required: file`,
`unrecognized arguments: ...`, `argument --parser: expected one
argument`, `ambiguous option: --=x could match --help, --canonical,
--parser`, `argument -h/--help: ignored explicit argument 'x'`, with
Python's `repr()`), non UTF-8 arguments (Python's `surrogateescape`
decoding; `\udcXX` on stderr, the raw bytes on stdout), and the help and
usage text wrapped at the terminal width (`COLUMNS`, or the width of the
terminal on stdout, or 80, like `shutil.get_terminal_size()`). The Unicode
properties this depends on (`str.isprintable()` for `repr()`, `\d` of the
negative number pattern, `int()` of `COLUMNS`) come from tables generated
with the oracle's Python (Unicode 14.0.0,
`rust/fasm-cli/tools/gen_unicode_tables.py`).

Like the original, the whole file is parsed before anything is printed:
an invalid file prints only its `Error: ...` line (on stdout, exit code
0), never part of the output.

`tests/cli/test_cli_compat.py` (`make cli-difftest`) runs both tools over
the corpus with every option combination, over argparse edge cases and at
100+ terminal widths, and compares stdout, stderr and exit codes byte for
byte, modulo the normalisation rules 1 to 3 below (`normalise()` in the
test).

### Differences

| Case | Original (`fasm/tool.py`) | Rust |
|---|---|---|
| Syntax error (rule 1) | `Error: Parse error at L:C - <ANTLR message>` | `Error: Parse error at L:C - <Rust message>`: same `L:C`, own message text (see "Errors" above) |
| Value range error: `a = 2`, `a[3:0] = 5'h10`, `a[0:1]` (rule 2) | `Error: 'NoneType' object is not iterable` on stdout, and a `ctypes` callback traceback with the `AssertionError` on stderr (default and `--parser antlr`) | `Error: Parse error at L:C - <message>` with the position of the value (or of the `[`), nothing on stderr |
| Error with `--parser textx` (rule 3) | textX's own messages: `Error: <path>:L:C: Expected ...`, `Error: (2, 1)` (range), `Error: [Errno 2] No such file or directory: 'x'` | the Rust parse error (the Rust tool has one parser for every `--parser` name) |
| Valid input where textX and ANTLR differ, with `--parser textx` (whitespace only lines, `\"` in annotations, `a b`, ...: see "Parser" above) | textX's result | the ANTLR compatible result |
| Input only the Rust parser accepts (see "Parser" above; e.g. `# café`) | `Error: 'NoneType' object is not iterable` (ANTLR) | the file is printed |
| `_` in a plain decimal value (accepted by Rust and textX, see "Parser" above) | `a[7:0] = 1_0`: `Error: Parse error at 1:10 - mismatched input '_' ...`; `a = 1_0`: `Error: Parse error at 1:5 - ...`; `a = 2_1F`: `Error: Parse error at 1:5 - ...` | `a[7:0] = 1_0`: the file is printed (`a[7:0] = 10`); `a = 1_0`: `Error: Parse error at 1:4 - value 10 does not fit in the 1 bit(s) ...`; `a = 2_1F`: `Error: Parse error at 1:7 - unexpected 'F', ...` |
| `--parser rust` | `Error: Parser 'rust' is not available.` | accepted (the name of the Rust parser in the Rust based Python package) |
| File that cannot be read | `Error: Parse error at 0:0 - Couldn't open file` | `Error: Parse error at 0:0 - Couldn't open file <path>: <OS error>` (rule 1) |
| A file that opens but cannot be read: a directory, `EIO` (e.g. `/proc/self/mem`) | the process aborts (SIGABRT, exit code 134, `terminate called after throwing an instance of 'std::__ios_failure'` on stderr); `--parser textx`: `Error: [Errno 21] Is a directory: '<absolute path>'` | `Error: Parse error at 0:0 - Couldn't open file <path>: <OS error>` (`Is a directory (os error 21)`, `Input/output error (os error 5)`), exit code 0 |
| Non-ASCII file name (`é.fasm`, or non UTF-8 bytes) | `Error: 'ascii' codec can't encode character '\xe9' in position 0: ordinal not in range(128)` (the ANTLR wrapper encodes the name as ASCII) | the file is read |
| stdout closed early (`fasm big.fasm \| head -1`) | `BrokenPipeError` traceback on stderr, exit code 1 | nothing on stderr, exit code 1. Whether a given run hits the broken pipe at all is timing dependent and differs between the two: the Rust tool writes its output with one `write`, the original writes the text and `print`'s `\n` separately, so with an output that fits in the pipe buffer either one may exit with 0 where the other exits with 1 |
| `-h`/`--help` with stdout closed (`fasm -h >&-`, `fasm --help --bogus >&-`) | `sys.stdout` is `None`, so argparse prints the help to stderr; exit code 0 | the Rust runtime reopens a closed stdout as `/dev/null`: the help is discarded, nothing on stderr; exit code 0 (not emulated: it depends on how the runtime handles closed standard descriptors; `test_help_with_closed_stdout` pins both behaviours) |
| Other error writing stdout (`ENOSPC`, ...) | traceback on stderr, exit code 1 | `fasm: error writing to stdout: <error>` on stderr, exit code 1 |
| Non UTF-8 bytes in a `--parser` value that is printed back (`Error: Parser '...' is not available.`) | written back raw in the C/POSIX locale (Python's UTF-8 mode, what the oracle runs with) and `C.UTF-8`; under another UTF-8 locale Python's stdout is strict and the tool dies with a `UnicodeEncodeError` traceback | always written back raw |
| Help/usage width when stdout is a terminal and `COLUMNS` is not set, on non Unix platforms | the terminal width | 80 columns |
| Python older than 3.10 | help heading `optional arguments:` | `options:` (Python 3.10+) |
| `COLUMNS` with more than 4300 digits (leading zeros included) | `int()` raises `ValueError` above `sys.int_max_str_digits` (default 4300; `PYTHONINTMAXSTRDIGITS` and `-X int_max_str_digits` change it), so `COLUMNS` is ignored | the default limit of 4300 is applied; `PYTHONINTMAXSTRDIGITS` is not honoured |

Precedence of errors: the original ANTLR parser checks the syntax of the
whole file before it decodes any value, so a syntax error anywhere in the
file is reported instead of an earlier value range error (`a = 2\nb c`:
`Parse error at 2:2`). The Rust parser stops at the first error in file
order; the tool emulates the original precedence: after a value range
error it resumes parsing after that line and reports the first later
error that is not a value range error, if there is one
(`tool::error_to_report` in `rust/fasm-cli/src/tool.rs`), and the first
value range error otherwise.

## `fasm2frames` (`rust/fasm-cli/src/fasm2frames.rs`, `rust/fasm-xilinx/`, T5.4/T5.5)

### Rule

The `fasm2frames` binary is a drop in replacement for f4pga-xc-fasm's
`xc_fasm.fasm2frames` (`python -m xc_fasm.fasm2frames`, on prjxray's
`fasm_assembler.py`; the oracle `tests/oracle/fasm2frames-oracle`):

* the same arguments, parsed by the argparse emulation of the `fasm` tool
  (all of its behaviour listed above carries over): `--db-root DB_ROOT`
  (required, unless `XRAY_DATABASE_DIR` and `XRAY_DATABASE` are both set:
  then it defaults to `os.path.join` of the two), `--part PART` (required
  unless `XRAY_PART` is set), `--sparse`, `--roi ROI`,
  `--emit_pudc_b_pullup`, `--debug`, `fn_in`, `fn_out` (default
  `/dev/stdout`); the same help text, usage errors (exit code 2) and
  argparse quirks (`fasm2frames a.fasm --sparse b.frm` is an error:
  argparse gives `fn_out` its default before `--sparse`). The program name
  is the base name of `argv[0]`, like argparse's: `fasm2frames` for the
  installed binary, `fasm2frames.py` for the original run with `-m`
  (`tests/cli/test_fasm2frames_compat.py` runs the binary through a link
  of that name);
* byte for byte the same `.frm` output (dense and `--sparse`, ROI,
  required features, PUDC_B pullup, STEPDOWN propagation), the same
  `--debug` dump, the same exit codes, and the same warnings on stderr
  (`frame_set: invalid word address 101 in line: ...` for the bits that
  the top `_SING` alias tiles place past the end of the frame);
* the output file is opened (created, truncated) first, before the
  database and the FASM file are read, like `open(args.fn_out, 'w')` in
  `main()`: after an error the output file exists and is empty.

`tools/difftest-xilinx.py` (`make xilinx-difftest`) runs both tools over
every FASM file of `tests/corpus/xilinx/` (with the real databases) and
`tests/corpus/f4pga-xc-fasm/` (miniature database) with the dense,
`--sparse`, `--emit_pudc_b_pullup`, `--sparse --debug` and ROI variants,
and compares the `.frm` files, stdout, exit codes and stderr modulo the
normalisation rules below; `tests/cli/test_fasm2frames_compat.py` covers
the command line (help at many widths, argparse errors, the `XRAY_*`
variables, error cases).
`make xilinx-difftest-all` (T5.9) runs the same comparison, and the
bitstream tools and `xcfasm`, for every part of the artix7, kintex7,
spartan7 and zynq7 databases on a generated corpus that sets every
segbits feature and pseudo PIP of the part (`tools/gen-xilinx-corpus.py`,
`DESIGN-xilinx-db.md` §8.9); `tests/cli/test_xilinx_corpus.py` checks
one part against golden reference results. First full run (125 parts,
2651 fasm2frames runs, 8814 xc7frames2bit/bitread runs, 375 xcfasm runs,
68.6 minutes with 4 jobs): no difference beyond the rules below; the only
"explained" runs are the 125 of `errors/value_range.fasm` (one per part),
which is rule 4, and rule 1 applied to the other error files
(`FasmLookupError`, `KeyError`, `FasmInconsistentBits`). The second full
run with generator version 2 (2252 fasm2frames runs, 7617
xc7frames2bit/bitread runs, 375 xcfasm runs) gave the same picture: 0
unexplained differences, 125 explained value range runs.

### Differences

| Case | Original (`xc_fasm.fasm2frames`) | Rust |
|---|---|---|
| Any error (rule 1) | an uncaught exception: exit code 1, a traceback on stderr ending with `<exception type>: <message>` | exit code 1, only the last line(s) of that traceback on stderr: `prjxray.fasm_assembler.FasmLookupError: Segment DB ...` (all messages, one per line), `prjxray.fasm_assembler.FasmInconsistentBits: FASM line "..." wanted to set bit (frame, word, bit) but was cleared by FASM line "..."`, `KeyError: 'TILE'`, `Exception: Parse error at L:C - ...`, `FileNotFoundError: [Errno 2] No such file or directory: 'path'`, ... with the original's message text |
| FASM syntax error (rule 2) | `Exception: Parse error at L:C - <ANTLR message>` | same `L:C`, the Rust parser's message (as for the `fasm` tool; the parser differences of "Parser" above apply, e.g. octal values and large decimal values the ANTLR parser misreads). The ANTLR precedence is emulated like in the `fasm` tool (`tool::error_to_report`): a syntax error anywhere in the text wins over an earlier value range error (`a = 2\nb c`: `2:2`), for the FASM file, the ROI's `required_features` and the part's `required_features.fasm` |
| Value range error (`a = 2`, `a[3:0] = 5'h10`, `a[0:1]`) without a later syntax error (rule 4) | the parser's assertion fails inside a ctypes callback: `Exception ignored on calling ctypes callback function: ...` with an `AssertionError: (2, None, None)` traceback, then the tool dies with `TypeError: 'NoneType' object is not iterable`, exit code 1 | `Exception: Parse error at L:C - value 2 does not fit ...` at the value, exit code 1 |
| FASM file with a non-ASCII comment or annotation value (`# café`) | the ANTLR wrapper returns `None`: `TypeError: 'NoneType' object is not iterable`, exit code 1 | assembled (exit code 0) |
| A directory (or an unreadable file, `EIO`) as `fn_in` | the C++ parser aborts: `terminate called after throwing an instance of 'std::__ios_failure'`, SIGABRT, exit code 134 | `Exception: Parse error at 0:0 - Couldn't open file`, exit code 1 |
| Database that cannot be opened: unknown part, missing or malformed files (rule 3) | various exceptions (`AssertionError: Part None not found in {...}`, `AssertionError: Mapping file ... does not exist`, `FileNotFoundError`, `KeyError`, `json.decoder.JSONDecodeError`, `yaml` errors, ...), some only when a tile type is first used (prjxray reads segbits lazily) | `fasm_xilinx.DbError: <file>:<line>: <message>`, when the database is opened (every tile type's segbits are read up front, so a malformed segbits file of an unused tile type is an error too) |
| ROI `design.json` that is not valid JSON | `json.decoder.JSONDecodeError: <Python message>` | `json.decoder.JSONDecodeError: <path>: <serde_json message>` |
| ROI bounds that are not numbers | a `TypeError` from the first comparison that fails, only if a tile is compared | `TypeError: '<=' not supported between instances of ...` when the ROI is read |
| Order in which STEPDOWN features are added (and so of the messages of a `FasmLookupError`, or which conflict is reported, for them) | Python `set` iteration order (banks, tiles of a bank, tags; changes from run to run with the string hash seed) | first seen order: banks and tags in the order of the FASM lines, tiles in `part.json` `iobanks` then `package_pins.csv` order |
| Several STEPDOWN features on IOB tiles without a package pin (unbonded IOBs, e.g. `LIOB33_X0Y101` on xc7a35tcsg324-1) | `KeyError` for the first such tile in `set` order (depends on `PYTHONHASHSEED`) | `KeyError` for the first such tile in file order |
| `required_features.fasm` of the part | a `set`: arbitrary order | file order, duplicates dropped |
| More than one PUDC_B pin in the part | `AssertionError: ((tile, site), (tile, site))` | the same text (`AssertionError: (('T1', 'IOB_Y0'), ('T2', 'IOB_X0Y1'))`) |
| A feature that is exactly the PUDC_B tile name (no `.`), with `--emit_pudc_b_pullup` | `IndexError: list index out of range` (in the feature callback) | the same |
| `--debug` with the `.frm` written to stdout (no `fn_out`) | the `.frm` goes through a second file object on `/dev/stdout`, the dump through `sys.stdout`: their order depends on Python's buffering (for a pipe: the `.frm` first unless the dump is larger than 8 KiB) | the `.frm` first, then the dump |
| `.frm` output larger than the disk (`ENOSPC`), broken pipe | a traceback, exit code 1 | `OSError: [Errno 28] ...` / `BrokenPipeError: [Errno 32] Broken pipe`, exit code 1 |
| Non-ASCII FASM file name | `UnicodeEncodeError` (the ANTLR wrapper encodes the name as ASCII) | the file is read |
| Segbit whose frame address does not fit in 32 bits, or whose word is more than one frame before the frame start (impossible with the prjxray databases) | a 9+ digit frame address in the `.frm` / `IndexError` | `OverflowError: ...` / `IndexError: list index out of range` |
| A prjuray-db (UltraScale+) part | `AssertionError: Mapping file <db>/mapping/devices.yaml does not exist` (prjxray's `Database` only knows prjxray-db) | assembled like prjuray's `utils/fasm2bit.py` (prjuray's assembler: no IO bank, STEPDOWN or PUDC_B handling, `--emit_pudc_b_pullup` ignored, the errors of the `uray-fasm2frames` section) and written as 93 32-bit words per frame, the `.frm` `xcframes2bit` reads. Checked by `tools/difftest-xilinx.py --prjuray`: identical to prjuray's 16-bit `.frm` converted to 32-bit words on every successful run |
| Database cache | none: every run parses the text database (lazily, per tile type) | the opened part is kept in a binary cache file in `$FASM_XDB_CACHE` (default `$XDG_CACHE_HOME/fasm/db` or `~/.cache/fasm/db`; `0` disables it), written (atomically) by the first run of a part and after any change of its source files. Same output, exit codes and messages with and without it (checked by `rust/fasm-cli/tests/db_cache.rs` and `make xilinx-difftest`); `FASM_XDB_CACHE_VERBOSE=1` adds messages on stderr. See `DESIGN-xilinx-db.md` §8.8 |

`tools/difftest-xilinx.py` applies rules 1 (drops the oracle's
`Traceback (most recent call last):` line and the indented frame lines),
2 (compares parse errors up to the message), 3 (a database error: the
oracle fails with an exception outside the reproduced ones and the Rust
tool with `fasm_xilinx.DbError`; only the exit codes, 1, are compared)
and 4 (an ANTLR value range error on the oracle side, a parse error on
the Rust side: both become `<value range error>`, exit code 1); every
other stderr difference fails. `tests/cli/test_fasm2frames_compat.py`
applies rules 1 to 3.

## `xc7frames2bit` and `bitread` (`rust/fasm-cli/src/{xc7frames2bit,bitread,gflags}.rs`, `rust/fasm-xilinx/src/bitstream/`, T5.6)

### Rule

The `xc7frames2bit` and `bitread` binaries are drop in replacements for
prjxray's C++ tools of the same name (`tools/xc7frames2bit.cc`,
`tools/bitread.cc`; the oracles `tests/oracle/xc7frames2bit-oracle` and
`tests/oracle/bitread-oracle`) for the Series7 architecture:

* the same flags, parsed by an emulation of the gflags version bundled
  with prjxray (`rust/fasm-cli/src/gflags.rs`): `-flag`/`--flag`,
  `=value` or the next argument, `-noflag` for booleans, the boolean and
  int32 value syntax, the permutation of non flag arguments, `--`, the
  error messages collected per flag and printed sorted by flag name
  (exit code 1), `--help`/`--helpful`/`--helpshort`/`--helpon`/
  `--helpmatch`/`--helppackage`/`--helpxml` (help on stdout, exit code 1)
  with gflags' line breaking, `--version` (exit code 0), `--undefok`,
  `--fromenv`, `--tryfromenv`; an unknown `--architecture` is Series7,
  like the reference's default variant;
* `--architecture=UltraScale` / `UltraScalePlus` as the plain prjxray
  checkout implements them (T6.2): the Series7 `part.yaml` types, frame
  address layout and ECC (13 bits in word 50, computed over the longer or
  shorter frame: no parity fold for 93 words) with 123 / 93 words per
  frame and the UltraScale sync header and packet sequence of
  `bitstream_writer.cc` / `configuration.cc`; `bitread` also compares
  `-z`'s frames with a 101 word zero frame and masks word 50 in `-x`, `-y`
  and the hex dump for them (prjuray-tools' own UltraScale support, with
  its own part types and ECC, is the `xcframes2bit` / `uray-bitread`
  section below);
* `xc7frames2bit`: the same messages and exit codes (`Part file X not
  found or invalid`, `Unable to open frm file: X` / `Frames file X not
  found or invalid`, the `Frame <address>: found <n> words instead of
  101` warnings, `Unable to open file for writting: X` / `Failed to
  write bitstream` / `Exitting` with exit code 0, an abort (SIGABRT) with
  `terminate called after throwing an instance of
  'std::invalid_argument'` (or `'std::out_of_range'`) / `what():  stoul`
  for a `.frm` number `std::stoul` rejects, an empty line included), and
  byte for byte the same `.bit` for the same `.frm`, part file, part name,
  `--frm_file` path and time (header, sync words, packet sequence with the
  part's IDCODE, every frame of the part in address order with its ECC,
  two zero frames between rows, frames of the `.frm` outside the part
  kept);
* `bitread`: the same stdout (`Bitstream size`, `Config size`, `Number of
  configuration frames`, `DONE`), stderr, exit codes and output for every
  flag (`-x`, `-y`, `-p`, the hex dump, `-o`, `-z`, `-C`, `-f`, `-F`,
  `--aux`; `-c` is accepted and ignored like in the reference), the input
  read from the file (exactly one positional argument) or stdin, and the
  reference's reader semantics (sync word searched anywhere, packet
  parsing that stops at an incomplete packet or a header type above 2,
  the `FAR`/`CMD`/`CTL1`/`MASK` register machine with the per frame CRC
  quirk, the two padding frames skipped between rows, `IDCODE` checked:
  `Bitstream does not appear to be for this part`);
* the reference's handling of unusual files: `bitread` maps the input
  with `mmap` after an `fstat`, so a file whose size is 0 (an empty
  file, a pipe, a `/proc` file) is an empty input (`Bitstream size: 0
  bytes`, `Input doesn't look like a bitstream`, exit code 1) and exactly
  `st_size` bytes are read; `xc7frames2bit` writing to an unseekable
  output (`--output_file=/dev/stdout | ...`) cannot seek back to fill in
  the header's data length, which stays 0 (the 4 bytes after the `e`
  tag); a `--part_file` that is a directory makes yaml-cpp's
  `std::ifstream` throw: `terminate called after throwing an instance of
  'std::__ios_failure'` / `what():  basic_filebuf::underflow error
  reading the file: Is a directory`, SIGABRT (both tools; the Rust
  binaries print the same and call `abort()`);
* `bitread` streams its output (frame by frame, through a buffer) and
  flushes stdout after the `Bitstream size`, `Config size` and `Number
  of configuration frames` lines (the reference's `std::endl`) and before
  anything goes to stderr, so stdout and stderr merged (`2>&1`) come out
  in the reference's order; peak memory stays small (22 MiB for
  `-x -o` on a dense random xc7a200t bitstream, a 340 MiB output; the
  reference: 68 MiB).

`part.yaml` files are read by the database loader's YAML subset; for
Series7 it now also accepts the `configuration_ranges` form the C++
decoder supports (prjxray's `lib/test_data/configuration_test.yaml`).

`tools/difftest-xilinx.py` (`make xilinx-difftest`) turns the oracle's
`.frm` of every successful dense, `--sparse`, `--emit_pudc_b_pullup`
and ROI run of the artix7 corpus into a `.bit` with both
`xc7frames2bit`s (identical files; the Rust tool gets the reference's
header time through `SOURCE_DATE_EPOCH`) and reads it with both
`bitread`s and eleven flag sets; it also runs both `bitread`s on the golden
`smoke_x1y0.bit` and prjxray's reference bitstreams
(`lib/test_data/configuration_test{,.debug,.perframecrc}.bit`, the
Series7 `design.bit` and `bram.bit` of `ToolsTestData.tar.gz`, Vivado
outputs). `tests/cli/test_xc7frames2bit_compat.py` and
`tests/cli/test_bitread_compat.py` compare the command lines (help, flag
errors, malformed `.frm`/`.bit` input, every output mode).

### Differences

| Case | Original (prjxray C++ tools) | Rust |
|---|---|---|
| Source file names in the help (`Flags from <file>:`, `<file>` of `--helpxml`) | the absolute paths of the build (`/…/prjxray/tools/xc7frames2bit.cc`, `/…/prjxray/third_party/gflags/src/gflags.cc`) | the same paths relative to the prjxray checkout (`tools/xc7frames2bit.cc`, `third_party/gflags/src/gflags.cc`), so `--helpmatch` with a part of the build directory matches nothing. The tests replace the reference's prefix |
| `--flagfile=FILE` | reads more flags from `FILE` | `ERROR: --flagfile is not supported by this implementation of the prjxray tools`, exit code 1 |
| `--tab_completion_word=WORD` | prints bash completions of `WORD`, exit code 0 | ignored |
| gflags' "Did you really mean to set flag ..." warning | only for string flags whose help mentions `true`/`false` (none in these tools) | not implemented |
| `--architecture=Spartan6` | supported (65 16-bit word frames, `FAR_MAJ`/`FAR_MIN`) | `xc7frames2bit: --architecture=Spartan6 is not supported yet (only Series7, UltraScale and UltraScalePlus)` (or `bitread: ...`), exit code 1 |
| A `part.yaml` of another architecture (`!<xilinx/xcupseries/part>`) given to these Series7 tools | depends on what yaml-cpp's Series7 decoder makes of it | `Part file ... not found or invalid` |
| `.bit` header date and time | the current UTC time | the same, or the time of `$SOURCE_DATE_EPOCH` (seconds since the epoch) when it is set: an extension for reproducible builds, used by the tests. A value that is not an integer is reported (`warning: SOURCE_DATE_EPOCH="..." is not an integer, using the current time` on stderr) and the current time is used |
| An error writing the `.bit` after the file was created (`ENOSPC`) | ignored: a truncated file, exit code 0 | `Error writing <file>: <error>`, `Failed to write bitstream`, `Exitting`, exit code 1 |
| `bitread` on a bitstream whose length after the sync word is not a multiple of 4 bytes | `terminate called after throwing an instance of 'std::out_of_range'` / `what():  pos > size()`, SIGABRT | the incomplete last word is ignored |
| `bitread --help`, `--helpxml` | | one more flag, `--frm_out=FILE` (listed under `rust/fasm-cli/src/bitread_extensions.rs`): writes the frames selected by `-z`, `-f` and `-F` as a `.frm` file (the ECC bits cleared unless `-C`), the inverse of `xc7frames2bit` |
| A huge `configuration_ranges` range in a `part.yaml` (more than 2^26 frames) | allocates every address | an error |

## `xcfasm` (`rust/fasm-cli/src/xcfasm.rs`, T5.7)

### Rule

The `xcfasm` binary is a drop in replacement for f4pga-xc-fasm's `xcfasm`
console script (`xc_fasm.xc_fasm.main`, the oracle
`tests/oracle/xcfasm-oracle`): the same arguments (argparse emulation:
`--db-root` and `--part` with the `XRAY_*` defaults, `--part_file`
(required), `--sparse`, `--roi`, `--emit_pudc_b_pullup`, `--debug`,
`--frm2bit`, `--fn_in`, `--bit_out`, `--frm_out`), the same help and
usage errors, the frames assembled exactly like `fasm2frames` (every rule
and difference of the `fasm2frames` section above applies, including
the error messages), the `.frm` written to `--frm_out`, and then the
bitstream written to `--bit_out` like `xc7frames2bit --frm_file
<frm_out> --output_file <bit_out> --part_name <part> --part_file
<part_file>` would, with its messages (`Part file X not found or
invalid`, `Unable to open file for writting: X` ... with exit code 0)
and, when that tool would fail, the last line of the reference's
traceback, `subprocess.CalledProcessError: Command '<frm2bit> --frm_file
... --part_file <part_file>' returned non-zero exit status 1.` (exit code
1). Like the reference, a missing `--fn_in` fails with `TypeError:
encoding without a string argument` after the database was opened, and a
missing `--bit_out` writes the bitstream to a file named `None`.

`tools/difftest-xilinx.py` runs both tools on every FASM file of the
artix7 corpus (dense, `--sparse`, `--sparse --debug
--emit_pudc_b_pullup`, ROI) and compares the `.frm` and `.bit` files
(the reference time injected), stdout, exit codes and the normalised
stderr; `tests/cli/test_xcfasm_compat.py` covers the command line and the
error cases.

### Differences

| Case | Original (`xc_fasm.xc_fasm`) | Rust |
|---|---|---|
| `--frm2bit TOOL` | the program run through the shell to write the `.bit` (`subprocess.check_output(..., shell=True)`): any tool; a missing one fails with `/bin/sh: 1: TOOL: not found` and `CalledProcessError ... exit status 127` | accepted and ignored: the bitstream is always written in process by the Rust `xc7frames2bit` code (byte for byte the reference's output) and `TOOL` only appears in the `CalledProcessError` message |
| No `--frm_out` | the `.frm` is written to a `tempfile.mkstemp()` file that is never deleted; its path is in the `.bit` header and in error messages | no `.frm` file is written; the `.bit` header (field `a`) and the messages name the `--fn_in` path instead |
| Paths with spaces or shell metacharacters | split or interpreted by the shell command line | used as given |
| Errors of the bitstream step | the `xc7frames2bit` messages, then a traceback ending with the `CalledProcessError` line | the same messages, then only the `CalledProcessError` line. For a `--part_file` that is a directory the reference's `xc7frames2bit` aborts and `/bin/sh` (dash) prints `Aborted` and exits with 134: the Rust tool prints the same text and `... returned non-zero exit status 134.` (with another `/bin/sh`, e.g. bash, the reference's text differs) |
| Anything `xc7frames2bit` prints on stdout | captured and discarded | nothing is printed |
| `.bit` header time | the current UTC time | the same, or `$SOURCE_DATE_EPOCH` (see `xc7frames2bit`) |
| Database cache | none | as for `fasm2frames` (`$FASM_XDB_CACHE`) |
| UltraScale / UltraScale+ | not supported: xc_fasm has no `--architecture` (it runs `xc7frames2bit` for Series7) and opens the database with prjxray's `Database` (`AssertionError: Mapping file .../mapping/devices.yaml does not exist` for prjuray-db) | a prjuray-db part is assembled like `fasm2frames` does (above), then the bitstream step is Series7 only: an `xcupseries` `--part_file` is `Part file X not found or invalid` and the `CalledProcessError` line, exit code 1 (use `fasm2frames` + `xcframes2bit --architecture=UltraScalePlus`) |

## `xcframes2bit` and `uray-bitread` (prjuray-tools, `rust/fasm-cli/src/{xc7frames2bit,bitread}.rs`, T6.2)

### Rule

The `xcframes2bit` and `uray-bitread` binaries are drop in replacements
for prjuray-tools' C++ `xcframes2bit` and `bitread`
(`tools/xcframes2bit.cc`, `tools/bitread.cc` of SymbiFlow/prjuray-tools;
the oracles `tests/oracle/uray-xcframes2bit-oracle` and
`tests/oracle/uray-bitread-oracle`, which run the binaries
`tests/oracle/setup-xilinx.sh` installs as `uray-xcframes2bit` and
`uray-bitread`). Everything of the `xc7frames2bit` and `bitread`
section above applies (the gflags emulation, the messages, exit codes
and aborts, the unusual files, the streaming), with these differences
of the prjuray-tools sources:

* gflags 2.2.2: `--helpfull` instead of `--helpful`, the tools' flags
  listed under `tools/xcframes2bit.cc` / `tools/bitread.cc`;
* `--architecture`: `Series7`, `UltraScale` and `UltraScalePlus` with
  prjuray-tools' own part types (`!<xilinx/xcuseries/part>`,
  `!<xilinx/xcupseries/part>`: flat `rows` whose number includes the
  half bit, or `configuration_ranges`), frame address layouts (UltraScale
  = Series7's; UltraScale+ one bit higher with an 8-bit minor) and frame
  ECC (48 bits in words 60/61 of the 123-word UltraScale frame, words
  45/46 of the 93-word UltraScale+ frame); a `part.yaml` with another
  architecture's tag is `Part file X not found or invalid`; an unknown
  name aborts (`terminate called after throwing an instance of
  'absl::bad_variant_access'` / `what():  Bad variant access`, SIGABRT;
  `uray-bitread` prints `Bitstream size` first);
* `xcframes2bit` checks every `.frm` frame address against the part
  (`readFrames(file, part)`): the first one that is not in the part ends
  the read with `Frames file contains an invalid frame: <address>` (the
  C++ `FrameAddress` `operator<<`: `[<%#10x>] ` then `TOP`/`BOTTOM` for
  Series7, ` Row=%2d Column=%2d Minor=%2d Type=<CLB/IO/CLK|Block
  RAM|Config CLB|>`), then `Frames file X not found or invalid`, exit
  code 1; the word count warnings of the lines before it are printed;
* the `.bit` of `xcframes2bit` (still `Generator=xc7frames2bit`) is byte
  for byte the reference's: the 6 (UltraScale) or 21 (UltraScale+) sync
  words, the UltraScale packet sequence (two leading NOPs, `FAR` before
  `UNKNOWN`, `COR0 = 0x38003FE5`, `COR1 = 0x400000`, `MASK`/`CTL0`
  `0x1`/`0x101`, final `MASK`/`CTL0` `0x101`), the ECC of each frame, two
  zero frames after each (row, bus) and at the end;
* `uray-bitread` verifies the ECC of every selected frame (after `-z`,
  `-f` and `-F`): a mismatch is `ERROR: ECC verification of frame
  <address> failed.` on stderr and exit code 1, or with `-E` `WARNING:
  ...` on stdout and the frame is printed; a frame too short for its ECC
  words (the last frame of an `FDRI` write whose length is not a multiple
  of the frame size) aborts (`std::out_of_range`, `what():  Span::at
  failed bounds check`), and like the reference's `abort()` the output
  still buffered (stdout, the `-o` file) is lost; `-x`/`-y` leave out the
  ECC bits of the architecture (`is_ecc_bit`) unless `-C`; the hex dump
  masks word 50 for Series7 only; `-z` compares with a zero frame of the
  architecture's size; `-C`'s help is `do not ignore the ECC bits in each
  frame`; `--aux` writes the same text as prjxray's (its `fseek(-1)`
  trick replaces the trailing space), except on an unseekable file (a
  pipe), where the reference's trailing spaces are kept too.

`tools/difftest-xilinx.py --prjuray` (`make uray-difftest`) runs both
`xcframes2bit`s on the frames of every successful run of its generated
prjuray-db corpus (converted from `uray-fasm2frames`' 16-bit words) and
both `uray-bitread`s with 9 flag sets on the result, and both
`uray-bitread`s on the Vivado bitstreams of prjuray-tools'
`ToolsTestData.tar.gz` (Series7 `design.bit`/`bram.bit`, UltraScale
`design.bit`, UltraScale+ `design.bit`/`test.bit`) with the round trip
bit -> `--frm_out` -> both `xcframes2bit`s -> both `uray-bitread`s;
`tests/cli/test_uray_tools_compat.py` compares the command lines (gflags,
malformed `.frm` / `.bit` inputs, ECC failures, aborts) on the synthetic
UltraScale+ database (`rust/fasm-xilinx/testdata/synthetic-usp-db`).

### Differences

| Case | Original (prjuray-tools C++ tools) | Rust |
|---|---|---|
| The program name | the tools are called `xcframes2bit` and `bitread` (the oracle build installs them as `uray-xcframes2bit` / `uray-bitread`) | `xcframes2bit` and `uray-bitread` (`bitread` is prjxray's); the name only shows in the help and `--helppackage` (gflags matches it against `tools/<name>.cc`: `uray-bitread --helppackage` prints `Unable to find a package for file=uray-bitread` like the oracle binary of that name) |
| Source file names in the help | absolute build paths (`/…/prjuray-tools/tools/xcframes2bit.cc`) | relative (`tools/xcframes2bit.cc`), like the prjxray tools |
| `--architecture=Spartan6` | supported | `... is not supported yet (only Series7, UltraScale and UltraScalePlus)`, exit code 1 |
| An `xcu(p)series` `part.yaml` whose values do not fit the frame address fields (a row key >= 64, a column >= 1024, an UltraScale+ `frame_count` > 256) | accepted: with `frame_count: 300` `xcframes2bit` writes a bitstream and exits 0; with row key 64 or column 1024 `addMissingFrames` loops forever (the masked address is found valid in row 0 again; timed out at 600 s) | `Part file X not found or invalid`, exit code 1 (the field check of `Part::new`, `rust/fasm-xilinx/src/part.rs`; `uray-bitread`: `Part file not found or invalid`). The hang is deliberately not reproduced. Series7 is unaffected (prjxray also rejects `frame_count: 200`) |
| `uray-bitread --frm_out=FILE` | | the Rust extension of `bitread` (the frames as a `.frm` with the ECC bits of the architecture cleared unless `-C`) |
| An abort in `uray-bitread` after frames were printed (a short frame) | the output of the stdio buffers is lost: 4 KiB blocks of it may have been written already | the unflushed part of a 64 KiB buffer is lost: the two tools lose the same when less than 4 KiB were printed since the last `std::endl` (the header lines), otherwise the amounts differ |
| `uray-bitread` without `-E` on a frame whose ECC fails, stdout and stderr merged | the `ERROR` line comes before the frames printed so far, which are still buffered, unless more than the stdio buffer was printed | the same up to 64 KiB of frames |
| The rest of the `xc7frames2bit` / `bitread` section (flagfile, tab completion, `SOURCE_DATE_EPOCH`, write errors, trailing bytes, huge `configuration_ranges`) | | the same |

## `uray-fasm2frames` (prjuray's `utils/fasm2frames.py`, `rust/fasm-cli/src/uray_fasm2frames.rs`, T6.2)

### Rule

The `uray-fasm2frames` binary is a drop in replacement for prjuray's
`utils/fasm2frames.py` (SymbiFlow/prjuray, on prjuray-tools' `prjuray`
package; the oracle `tests/oracle/uray-fasm2frames-oracle`):

* the same arguments (argparse emulation): `--db-root DB_ROOT` (required
  unless `URAY_DATABASE_DIR` and `URAY_DATABASE` are set), `--part PART`
  (required unless `URAY_PART` is set), `--sparse`, `--roi ROI`,
  `--debug`, `--dump_bits`, `fn_in`, `fn_out` (default `/dev/stdout`);
  the same usage errors (exit code 2); `-h`/`--help` fails like the
  reference, whose argparse expands the `%` of the `--dump_bits` help
  (`bit_%08x_%03d_%02d`): `TypeError: %x format: an integer is required,
  not dict`, exit code 1, no help;
* the output file is opened first, then the database (prjuray-db
  layout, `<db-root>/<part>/tilegrid.json`); the FASM file, the ROI's
  `required_features` and the part's `required_features.fasm` are
  assembled with prjuray's `utils/fasm_assembler.py` semantics (a copy
  of prjxray's before its word check): no IO bank, STEPDOWN or PUDC_B
  handling; bits are keyed and checked in 16-bit words; a bit beyond the
  end of the 186 16-bit word frame is kept (its frame is output) and a
  set one is `IndexError: list index out of range`; conflicts are
  `utils.fasm_assembler.FasmInconsistentBits: FASM line "..." wanted to
  set bit (frame, 16-bit word, bit) ...`, unknown features
  `utils.fasm_assembler.FasmLookupError: ...`;
* the `.frm` has the frames as 16-bit words (186 per UltraScale+ frame,
  `0x%08X` each, the low half of each 32-bit word first); `--dump_bits`
  writes `bit_%08x_%03d_%02d` (frame, 32-bit word, bit) lines instead;
  `--debug` prints the sparse dump in 16-bit words on stdout before the
  output is written;
* errors: the last line of the reference's traceback, exit code 1, as
  for `fasm2frames` (its rules 1 to 4 and database errors apply).

This `.frm` is not what `xcframes2bit` reads (it warns `found 186 words
instead of 93` and skips every line): prjuray's `utils/fasm2bit.py`
converts the frames to 32-bit words first; the Rust `fasm2frames` writes
those directly for a prjuray-db part (the `fasm2frames` section).

`tools/difftest-xilinx.py --prjuray` (`make uray-difftest-all`, T6.3,
`docs/rewrite/DESIGN-xilinx-db.md` §8.12) runs every part of every
prjuray-db family (upstream has only `zynqusp`, with its two xczu3eg
parts) on the every-feature corpus of `tools/gen-xilinx-corpus.py` (all
54542 reachable features of the 27 tile types with segbits), random
designs and error files, and compares both tools with the dense,
`--sparse`, `--sparse --debug`, `--dump_bits` and ROI variants, then
`fasm2frames`, `xcframes2bit` and `uray-bitread` on the results: no
difference beyond the normalisation rules;
`tests/cli/test_uray_corpus.py` checks one part against golden reference
results, `tests/cli/test_uray_tools_compat.py` the command line.
prjuray-db has no native UltraScale (`xcuseries`, non-plus) part, so
UltraScale is covered only by the `ToolsTestData` Vivado bitstreams and
the synthetic parts and unit tests of the `xcframes2bit` / `uray-bitread`
section.

**prjuray-db features that no FASM file can set** (a database
limitation, the same for both tools): 34 segbits keys have a name part
that starts with a digit, which is not a FASM identifier, e.g.
`BRAM.RAMB18E2_L.READ_WIDTH_A.36` (6 keys of `BRAM`, 2 of
`INT_INTF_LEFT_TERM_PSS`, `OUTPUTS_ENABLED.0/1`, 26 of
`XIPHY_BYTE_RIGHT`, `...ISERDESE3.DATA_WIDTH.4/8`). Repro (prjuray-db
`affbc5e5`):

```
$ printf 'BRAM_X8Y0.RAMB18E2_L.READ_WIDTH_A.36\n' > w.fasm
$ tests/oracle/uray-fasm2frames-oracle --db-root <db>/prjuray-db/zynqusp \
    --part xczu3eg-sfvc784-1-e w.fasm out.frm
... Exception: Parse error at 1:33 - mismatched input '.' expecting {<EOF>, NEWLINE}
$ target/release/uray-fasm2frames ... (same arguments)
Exception: Parse error at 1:33 - unexpected '.', expected '[', '=', '{', '#' or end of line
```

Both exit with 1 at the same position (rule 2).

### Differences

| Case | Original (`utils/fasm2frames.py`) | Rust |
|---|---|---|
| Every difference of the `fasm2frames` section that is not about the IO banks, STEPDOWN or PUDC_B (the ANTLR parser cases, error message forms, database errors, the order of `required_features.fasm`, the database cache) | | the same |
| `--debug` with the `.frm` written to stdout | the dump (`print`) and the `.frm` (a second file object on `/dev/stdout`) interleave by Python's buffering | the dump first, then the `.frm` |
| The oracle's environment | `utils/util.py` imports `jinja2` at the top (for templates `fasm2frames.py` never uses); the oracle venv has none, so `uray-fasm2frames-oracle` provides an empty stand-in module | not needed |

## The f4pga flow's outputs (f4pga-examples, T7.3)

### Rule

For every f4pga-examples design built with the f4pga Yosys + VPR flow
(`tools/e2e/run-f4pga-examples.sh`, `docs/rewrite/DESIGN-xilinx-db.md`
§8.11), the Rust tools reproduce what the flow wrote: `xcfasm` with the
flow's command line (`--sparse --emit_pudc_b_pullup`, the flow's
prjxray-db) writes the flow's frames byte for byte and the flow's `.bit`
byte for byte except for the `.frm` path in the header's design field;
`fasm2frames` with the same options writes the same frames (with the
flow's and with the pinned database, which are identical);
`xc7frames2bit` on those frames writes the flow's `.bit` (same header
rule); `bitread` prints what the flow's `bitread` prints; the `fasm` CLI
prints what the flow's `fasm` (PyPI `fasm` 0.0.2.post88) prints. The
dense, sparse and pudc variants also match the flow's tools (prjxray
`ae546d6b`) and the oracle (prjxray `c9f02d85`) through
`tools/difftest-xilinx.py --corpus-root`. No difference was found; no
Rust change was needed.

### Differences and quirks of the reference flow

| Case | f4pga flow | Rust / this repository |
|---|---|---|
| `.bit` header of the flow | names the flow's temporary `.frm` file (`/tmp/tmpXXXXXXXX`, xcfasm without `--frm_out`) and the build time | the comparison skips the path (the configuration data and the other header fields must be identical) and injects the time with `SOURCE_DATE_EPOCH` (see `xcfasm` above) |
| The environment's `bin/fasm2frames` | prjxray's console script, broken: `ModuleNotFoundError: No module named 'utils'` (prjxray's pip package does not install `utils/`) | the comparisons run the flow's `xc_fasm.fasm2frames` (`tools/e2e/f4pga/fasm2frames-flow`), which the flow's `xcfasm` uses |
| `genfasm` killed or crashing | `symbiflow_write_fasm` ignores its exit status: a truncated `top.fasm` (`genfasm` of an Arty A7-100T design was killed by the OOM killer here, leaving 320 lines without any routing), then a bitstream of it, and `make` succeeds | `run-f4pga-examples.sh` marks such a build as failed (`tools/e2e/f4pga/check-genfasm.sh`: genfasm's log, `fasm.log` or `vpr_stdout.log`, must end with `Writing Implementation FASM` and `The entire flow of VPR took`, and no bash signal report may name genfasm; a SIGTERM only shows as a bare `Terminated`, a non-zero exit not at all); the truncated FASM is valid FASM and the Rust tools reproduce the flow's frames and bitstream of it too |

## The openXC7 snap's tools (nextpnr-xilinx examples, T7.6)

### Rule

For every design built with the openXC7 snap `0.8.2` (nextpnr-xilinx
examples, openXC7 demo-projects and primitive-tests;
`tools/e2e/run-nextpnr-examples.sh`, `docs/rewrite/DESIGN-xilinx-db.md`
§8.13), with the snap's bundled prjxray-db: the Rust `fasm2frames` writes
the flow's frames (the snap's `fasm2frames`, dense) byte for byte, and
the same frames and exit codes as the snap's `fasm2frames --sparse`;
`--emit_pudc_b_pullup` gives the oracle's frames (the snap's tool fails
there, see below); `xc7frames2bit` on the flow's frames writes the flow's
`.bit` (up to the `.frm` path in the header's design field, the time
injected with `SOURCE_DATE_EPOCH`); `xcfasm` writes both; `bitread` prints
what the snap's `bitread` prints with all 11 `BITREAD_FLAGS` sets; the
`fasm` CLI prints what the snap's `fasm` (textX, see below) and the
oracle's print, with and without `--canonical`. With the pinned database
the Rust tools and the oracle agree too (frames, exit codes, last error
line); the frames are the snap database's except where the design uses
features only the snap database has. No difference was found; no Rust
change was needed.

The snap's `fasm2frames` is prjxray's `utils/fasm2frames.py` (the snap
builds prjxray `master` at its build time), not f4pga-xc-fasm's
`xc_fasm.fasm2frames` that the Rust tool reproduces; the two files differ
only in the licence header, formatting, `OpenSafeFile` (a lock file) for
`package_pins.csv`, `part.json` and the ROI, the function name, and the
PUDC_B feature below.

### Differences and quirks of the reference flow

| Case | openXC7 snap 0.8.2 | Rust / this repository |
|---|---|---|
| `fasm2frames --emit_pudc_b_pullup` on a design that does not use the PUDC_B pin (every design built here) | `prjxray.fasm_assembler.FasmLookupError: Segment DB LIOB33, key LIOB33.IOB_Y0.LVCMOS12_LVCMOS15_LVCMOS18_LVCMOS25_LVCMOS33_LVTTL_SSTL135_SSTL15.IN_ONLY not found ...`, exit code 1: prjxray's `utils/fasm2frames.py` still emits the IN_ONLY feature under its old name, which neither the snap's nor the pinned prjxray-db has (both name it `...LVCMOS33_LVDS_25_LVTTL_SSTL135_SSTL15_TMDS_33.IN_ONLY`) | the feature f4pga-xc-fasm emits (`..._LVDS_25_LVTTL_SSTL135_SSTL15_TMDS_33.IN_ONLY`, plus `LVCMOS25_LVCMOS33_LVTTL.IN` and `PULLTYPE.PULLUP`), like the oracle: the frames are the oracle's (`compare-nextpnr-examples.py` checks this and reports the snap's failure as explained). Repro: `fasm2frames --db-root $PRJXRAY_DB_DIR/artix7 --part xc7a35tcsg324-1 --emit_pudc_b_pullup tests/corpus/xilinx/artix7/designs/nextpnr-xilinx/blinky/arty-a35/top.fasm x.frm` after `source tools/e2e/openxc7-env.sh` |
| The snap's `fasm` package without snapd's `core20` base (tools/e2e/setup-openxc7.sh patches the interpreter, the libraries of `core20` are not there) | its ANTLR extension does not load (`ImportError: libffi.so.7`), so `fasm` and `fasm2frames` print a `RuntimeWarning: Unable to import fast Antlr4 parser implementation` on stderr and parse with textX | same output as the textX run; the comparisons check stdout, frames and exit codes, not this warning |
| Designs using `STARTUPE2`, `BSCANE2` or a GTP reference clock (`config-primitive-startupe2`, primitive-tests `startupe2`, `jtag-test`, `gtp_common/internal-refclk`) with the pinned database | -- (the snap flow uses the snap database, which has the `ppips_cfg_center_*.db` pseudo PIPs and `GTP_COMMON.GTPE2_COMMON.GTGREFCLK0_USED`, see `tools/e2e/README.md`, "A note on prjxray-db provenance") | with the pinned database both the Rust tool and the oracle fail with the same `FasmLookupError` (`Segment DB CFG_CENTER_MID, key CFG_CENTER_MID.CFG_CENTER_STARTUP_USRDONEO.CFG_CENTER_IMUX42_8 not found ...`, `... CFG_CENTER_STARTUP_USRCCLKO.CFG_CENTER_CLK1_7 ...`, `... CFG_CENTER_LOGIC_OUTS_B17_11.CFG_CENTER_BSCAN3_TDI ...`, `Segment DB GTP_COMMON, key GTP_COMMON.GTPE2_COMMON.GTGREFCLK0_USED ...`): a database difference, not a tool difference |
| `.bit` header | names the `.frm` file as given to `xc7frames2bit` (relative, e.g. `blinky.frames`) and the build time | the comparison skips the path and injects the time (as for the f4pga flow above) |

## C API (`libfasm_capi`, `rust/fasm-capi/`, T4.1)

The C API mirrors the Python functions (see `docs/rewrite/DESIGN-capi.md`);
where C cannot express the Python behaviour exactly:

| Case | Python | C API |
|---|---|---|
| `merge_and_sort(model, sort_key=...)` | `sort_key(group_id)` returns any comparable object; called once per group by `sorted()`; groups with equal keys keep their order of first appearance in the model (stable sort over dict insertion order) | `fasm_file_merge_and_sort_ex`: the key is an `int64_t`; called exactly once per group (cached); groups with equal keys are ordered by group id (string order), so the output never depends on hash map order |
| `merge_and_sort(model, zero_function=...)` | called with the feature name | `fasm_zero_fn` is called with a NUL terminated copy of the feature name (and its length) |
| Feature values | unbounded `int` | bit length, `uint64_t` when it fits, bits, little endian bytes or digit strings (`fasm_set_feature_value_*`) |
| `value_format` | `ValueFormat` or `None` | `fasm_value_format`: the same values 0 to 4, `FASM_VALUE_FORMAT_NONE` (-1) for `None` |

### C++ wrapper (`include/fasm/fasm.hpp`, T4.2)

The C++ wrapper is a thin, 1:1 layer over the C API above (see
`docs/rewrite/DESIGN-capi.md`, "C++ wrapper" section) and introduces no
further behavioural differences from Python beyond the C API's own: every
row of the table above applies unchanged (`fasm::File::merge_and_sort`'s
`SortKeyFn` is still an `int64_t` key called once per group;
`fasm::Value` is still exposed as bit length / `uint64_t` / bytes / a
digit string rather than an unbounded integer; `fasm::ValueFormat` is
`std::optional<ValueFormat>` rather than a Python `ValueFormat | None`,
`std::nullopt` standing for `None`). The one C++-specific difference from
both Python and the plain C API: failures are reported as a thrown
`fasm::Error` (a `std::runtime_error`) rather than a return value/`None`
or a status code, and a callback that throws propagates that same
exception out of `merge_and_sort` / `parse_each` instead of terminating
the process the way an uncaught exception crossing the Rust `extern "C"`
boundary otherwise would (see the exception trampoline rule in
`DESIGN-capi.md`).

## Python bindings (`fasm.parser.rust`, `rust/fasm-python/`, T3.1, T3.3)

### Rule

The `fasm` Python package keeps its API (`fasm`, `fasm.model`,
`fasm.output`, `fasm.parser`, `fasm.tool`); its default parser is now the
Rust parser (`fasm.parser.rust`), which returns the same `fasm.model`
namedtuples, with the field types of the ANTLR parser (a `list` of lines,
`annotations` a `list` or `None`), and parses exactly like the Rust library
(every entry of "Parser" above applies). The textX parser is unchanged. See
`DESIGN-python.md` for the design.

### Differences

| Case | Original (default parser: `antlr`) | Rust based package |
|---|---|---|
| `fasm.parser.available`, `fasm.parser.implementation` | `['antlr', 'textx']`, `'antlr'` | `['rust', 'textx']`, `'rust'` |
| Parse error | plain `Exception('Parse error at L:C - <ANTLR message>')` | `fasm.parser.rust.FasmParseError` (an `Exception` subclass) with the same `str()` format, the Rust message (see "Errors"), and `line` / `column` attributes |
| Value range error (`a = 2`, `a[3:0] = 5'h10`, `a[0:1]`) | `AssertionError` traceback printed on stderr, the function returns `None` | `FasmParseError` at the value or at the `[` |
| Syntax error after a value range error (`a = 2\nb c`) | the syntax error (`Parse error at 2:2`): the whole file is checked for syntax first | the first error in file order (`Parse error at 1:4 - value 2 does not fit ...`); the Rust `fasm` CLI emulates the original precedence (`2:2`, see "Command line tool") but the bindings do not, so the `fasm` console script of the Python package (`fasm/tool.py`) prints `1:4` where the Rust `fasm` binary prints `2:2` |
| File that cannot be read | `Exception('Parse error at 0:0 - Couldn't open file')` | `FasmParseError('Parse error at 0:0 - Couldn't open file <path>: <OS error>')`, the text of the Rust CLI |
| `parse_fasm_filename` argument | ASCII `str` only (`bytes(filename, 'ascii')`) | `str` (any), `bytes`, `os.PathLike` |
| Non-ASCII comment or annotation value (`# café`) | `parse_fasm_string`: `UnicodeEncodeError`; `parse_fasm_filename`: `None` (see "Non-ASCII input") | parsed |
| NUL in `parse_fasm_string` input | the rest of the input is dropped | parsed (see "Non-ASCII input") |
| `fasm --parser antlr` (`fasm/tool.py`) | the ANTLR parser | the Rust parser when the ANTLR one is not built (like the Rust CLI); `--parser rust` is accepted |
| Rust parser extension not importable | `RuntimeWarning` "Unable to import fast Antlr4 parser implementation. ..." and the textX parser (original: no `setup.py` ANTLR build) | `RuntimeWarning` "Unable to import the fasm._fasm_rs Rust parser extension (ImportError: ...); falling back to ..." and the textX parser |
| `fasm.__version__` | from `fasm/version.py` (`update_version.py`) | from the package metadata (`0.1.0.dev0` for now); `fasm/version.py` is no longer tracked or packaged (a locally generated one still takes precedence when importing from the source tree) |
| New API | | `fasm.parser.rust.parse_fasm_bytes`, `fasm.parser.rust.FasmParseError`, `fasm._fasm_rs.fasm_tuple_to_string`/`fasm._fasm_rs.merge_and_sort` (fast paths, return `None` when they cannot guarantee the Python result — used automatically by `fasm.fasm_tuple_to_string`/`fasm.output.merge_and_sort`, T3.3), `fasm.output._merge_and_sort_py` (the pure Python implementation `merge_and_sort` falls back to) |
| Cyclic garbage collector while building a result of 256 lines or more | runs | paused, then restored (`gc.callbacks` do not fire meanwhile) |
| `fasm.output.merge_and_sort`'s `zero_function`/`sort_key` calls, when the fast path runs (T3.3) | called lazily, as the caller consumes the returned generator | called eagerly, at the `merge_and_sort(...)` call itself (same count, arguments and order, just sooner) — the fast path returns a materialised `list` wrapped in `iter()`, not a generator; see `DESIGN-python.md`'s "Eager vs. lazy evaluation" |
| `fasm.output.merge_and_sort`'s `sort_key` result's `__lt__` call count for a **tied** pair of group ids (neither `a < b` nor `b < a`), when the fast path runs (T3.3) | `sorted(..., key=sort_key)` calls `__lt__` once per comparison decision (CPython's sort only ever tests one direction) | the fast path's comparator calls `__lt__` up to twice per pair (`a < b`, then, only if that is `False`, `b < a`, to build a 3-way `Ordering` for `Vec::sort_by`) — the resulting sorted order is identical (a tied pair keeps its original relative order either way), but a `sort_key` whose `__lt__` has a call-count-dependent side effect (e.g. raises on its Nth call) can behave differently between the two paths; see `DESIGN-python.md`'s "`sort_key`'s `__lt__` call count can differ for tied keys" and its regression test in `tests/test_fast_paths.py` |

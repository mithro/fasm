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

### Differences

| Case | Original (`xc_fasm.fasm2frames`) | Rust |
|---|---|---|
| Any error (rule 1) | an uncaught exception: exit code 1, a traceback on stderr ending with `<exception type>: <message>` | exit code 1, only the last line(s) of that traceback on stderr: `prjxray.fasm_assembler.FasmLookupError: Segment DB ...` (all messages, one per line), `prjxray.fasm_assembler.FasmInconsistentBits: FASM line "..." wanted to set bit (frame, word, bit) but was cleared by FASM line "..."`, `KeyError: 'TILE'`, `Exception: Parse error at L:C - ...`, `FileNotFoundError: [Errno 2] No such file or directory: 'path'`, ... with the original's message text |
| FASM syntax error (rule 2) | `Exception: Parse error at L:C - <ANTLR message>` | same `L:C`, the Rust parser's message (as for the `fasm` tool; the parser differences of "Parser" above apply, e.g. octal values and large decimal values the ANTLR parser misreads) |
| Database that cannot be opened: unknown part, missing or malformed files (rule 3) | various exceptions (`AssertionError: Part None not found in {...}`, `AssertionError: Mapping file ... does not exist`, `FileNotFoundError`, `KeyError`, `json.decoder.JSONDecodeError`, `yaml` errors, ...), some only when a tile type is first used (prjxray reads segbits lazily) | `fasm_xilinx.DbError: <file>:<line>: <message>`, when the database is opened (every tile type's segbits are read up front, so a malformed segbits file of an unused tile type is an error too) |
| ROI `design.json` that is not valid JSON | `json.decoder.JSONDecodeError: <Python message>` | `json.decoder.JSONDecodeError: <path>: <serde_json message>` |
| ROI bounds that are not numbers | a `TypeError` from the first comparison that fails, only if a tile is compared | `TypeError: '<=' not supported between instances of ...` when the ROI is read |
| Order in which STEPDOWN features are added (and so of the messages of a `FasmLookupError`, or which conflict is reported, for them) | Python `set` iteration order (banks, tiles of a bank, tags; changes from run to run with the string hash seed) | first seen order: banks and tags in the order of the FASM lines, tiles in `part.json` `iobanks` then `package_pins.csv` order |
| `required_features.fasm` of the part | a `set`: arbitrary order | file order, duplicates dropped |
| More than one PUDC_B pin in the part | `AssertionError: ((tile, site), (tile, site))` | the same text (`AssertionError: (('T1', 'IOB_Y0'), ('T2', 'IOB_X0Y1'))`) |
| A feature that is exactly the PUDC_B tile name (no `.`), with `--emit_pudc_b_pullup` | `IndexError: list index out of range` (in the feature callback) | the same |
| `--debug` with the `.frm` written to stdout (no `fn_out`) | the `.frm` goes through a second file object on `/dev/stdout`, the dump through `sys.stdout`: their order depends on Python's buffering (for a pipe: the `.frm` first unless the dump is larger than 8 KiB) | the `.frm` first, then the dump |
| `.frm` output larger than the disk (`ENOSPC`), broken pipe | a traceback, exit code 1 | `OSError: [Errno 28] ...` / `BrokenPipeError: [Errno 32] Broken pipe`, exit code 1 |
| Non-ASCII FASM file name | `UnicodeEncodeError` (the ANTLR wrapper encodes the name as ASCII) | the file is read |
| Segbit whose frame address does not fit in 32 bits, or whose word is more than one frame before the frame start (impossible with the prjxray databases) | a 9+ digit frame address in the `.frm` / `IndexError` | `OverflowError: ...` / `IndexError: list index out of range` |
| Architecture | Series7 only (101 words per frame; prjuray has its own `fasm2frames.py`) | the word count and the bit unit come from the database's architecture; UltraScale/UltraScale+ output is not verified yet (T6.x) |

`tools/difftest-xilinx.py` applies rules 1 (drops the oracle's
`Traceback (most recent call last):` line and the indented frame lines),
2 (compares parse errors up to the message) and 3 (for an exception type
the Rust tool does not reproduce, only checks that both fail with exit
code 1 and an error on stderr); `tests/cli/test_fasm2frames_compat.py`
does the same.

## C API (`libfasm_capi`, `rust/fasm-capi/`, T4.1)

The C API mirrors the Python functions (see `docs/rewrite/DESIGN-capi.md`);
where C cannot express the Python behaviour exactly:

| Case | Python | C API |
|---|---|---|
| `merge_and_sort(model, sort_key=...)` | `sort_key(group_id)` returns any comparable object; called once per group by `sorted()`; groups with equal keys keep their order of first appearance in the model (stable sort over dict insertion order) | `fasm_file_merge_and_sort_ex`: the key is an `int64_t`; called exactly once per group (cached); groups with equal keys are ordered by group id (string order), so the output never depends on hash map order |
| `merge_and_sort(model, zero_function=...)` | called with the feature name | `fasm_zero_fn` is called with a NUL terminated copy of the feature name (and its length) |
| Feature values | unbounded `int` | bit length, `uint64_t` when it fits, bits, little endian bytes or digit strings (`fasm_set_feature_value_*`) |
| `value_format` | `ValueFormat` or `None` | `fasm_value_format`: the same values 0 to 4, `FASM_VALUE_FORMAT_NONE` (-1) for `None` |

## Python bindings (`fasm.parser.rust`, `rust/fasm-python/`, T3.1)

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
| `fasm.parser.available`, `fasm.parser.implementation` | `['antlr', 'textx']`, `'antlr'` | `['rust', 'textx']`, `'rust'` (`['rust', 'antlr', 'textx']` if a legacy `setup.py` ANTLR build is present too) |
| Parse error | plain `Exception('Parse error at L:C - <ANTLR message>')` | `fasm.parser.rust.FasmParseError` (an `Exception` subclass) with the same `str()` format, the Rust message (see "Errors"), and `line` / `column` attributes |
| Value range error (`a = 2`, `a[3:0] = 5'h10`, `a[0:1]`) | `AssertionError` traceback printed on stderr, the function returns `None` | `FasmParseError` at the value or at the `[` |
| Syntax error after a value range error (`a = 2\nb c`) | the syntax error (`Parse error at 2:2`): the whole file is checked for syntax first | the first error in file order (`Parse error at 1:4 - value 2 does not fit ...`); the Rust `fasm` CLI emulates the original precedence (`2:2`, see "Command line tool") but the bindings do not, so the `fasm` console script of the Python package (`fasm/tool.py`) prints `1:4` where the Rust `fasm` binary prints `2:2` |
| File that cannot be read | `Exception('Parse error at 0:0 - Couldn't open file')` | `FasmParseError('Parse error at 0:0 - Couldn't open file <path>: <OS error>')`, the text of the Rust CLI |
| `parse_fasm_filename` argument | ASCII `str` only (`bytes(filename, 'ascii')`) | `str` (any), `bytes`, `os.PathLike` |
| Non-ASCII comment or annotation value (`# café`) | `parse_fasm_string`: `UnicodeEncodeError`; `parse_fasm_filename`: `None` (see "Non-ASCII input") | parsed |
| NUL in `parse_fasm_string` input | the rest of the input is dropped | parsed (see "Non-ASCII input") |
| `fasm --parser antlr` (`fasm/tool.py`) | the ANTLR parser | the Rust parser when the ANTLR one is not built (like the Rust CLI); `--parser rust` is accepted |
| Neither the Rust nor the ANTLR parser importable | `RuntimeWarning` "Unable to import fast Antlr4 parser implementation. ..." and the textX parser | the same warning text, followed by a paragraph with the Rust extension's `ImportError`, and the textX parser |
| `fasm.__version__` | from `fasm/version.py` (`update_version.py`) | from the package metadata (`0.1.0.dev0` for now); `fasm/version.py` is no longer tracked or packaged (a locally generated one still takes precedence when importing from the source tree) |
| New API | | `fasm.parser.rust.parse_fasm_bytes`, `fasm.parser.rust.FasmParseError`, `fasm._fasm_rs.fasm_tuple_to_string` (fast path, returns `None` when it cannot guarantee the Python result) |
| Cyclic garbage collector while building a result of 256 lines or more | runs | paused, then restored (`gc.callbacks` do not fire meanwhile) |

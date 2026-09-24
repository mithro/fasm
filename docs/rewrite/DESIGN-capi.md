# Design notes: the C API (`fasm-capi`, T4.1)

The `fasm-capi` crate (`rust/fasm-capi/`) exposes the `fasm` core crate to
C (and, through T4.2, C++) as `libfasm_capi` (cdylib + staticlib). Its
header, `include/fasm/fasm.h`, is generated from the crate's sources with
cbindgen and checked in. This file records the rules of the API and the
decisions behind them; each function's contract is also written on the
function itself (Rust doc comments, copied into the header).

## Overview

| Area | Functions |
|---|---|
| Version, status | `fasm_version`, `fasm_status_string` |
| Errors | `fasm_error_status`, `fasm_error_message`, `fasm_error_line`, `fasm_error_column`, `fasm_error_free` |
| Owned strings | `fasm_string_data`, `fasm_string_len`, `fasm_string_free` |
| Files (models) | `fasm_file_line_count`, `fasm_file_line`, `fasm_file_free` |
| Parsing | `fasm_parse_string`, `fasm_parse_file` (into a `fasm_file`), `fasm_parse_string_cb`, `fasm_parse_file_cb` (streaming) |
| Lines | `fasm_line_has_set_feature`, `fasm_line_set_feature`, `fasm_line_annotation_count`, `fasm_line_annotation`, `fasm_line_has_comment`, `fasm_line_comment` |
| Set features | `fasm_set_feature_name_len`, `fasm_set_feature_name`, `fasm_set_feature_name_string`, `fasm_set_feature_has_start`, `fasm_set_feature_start`, `fasm_set_feature_has_end`, `fasm_set_feature_end`, `fasm_set_feature_value_format`, `fasm_set_feature_width`, `fasm_set_feature_value_bits`, `fasm_set_feature_value_u64`, `fasm_set_feature_value_bit`, `fasm_set_feature_value_bytes_le`, `fasm_set_feature_value_to_string` |
| Output | `fasm_file_to_string`, `fasm_line_to_string`, `fasm_set_feature_to_string` |
| Merge | `fasm_file_merge_and_sort`, `fasm_file_merge_and_sort_ex` |
| Building | `fasm_file_new`, `fasm_file_push_line` |

They mirror the Python API: `parse_fasm_string`/`parse_fasm_filename`,
the `FasmLine`/`SetFasmFeature`/`Annotation` namedtuples,
`fasm_tuple_to_string`, `fasm_line_to_string`, `set_feature_to_str`,
`set_feature_width` and `fasm.output.merge_and_sort`.

## Handles and ownership

All objects are opaque (`typedef struct fasm_file fasm_file;`); the C side
only holds pointers. Two kinds of pointers are handed out:

* **Owned**: `fasm_file` (from `fasm_parse_string`, `fasm_parse_file`,
  `fasm_file_new`, `fasm_file_merge_and_sort*`), `fasm_string` (from the
  `*_to_string` functions and `fasm_set_feature_name_string`) and
  `fasm_error` (from any `fasm_error **err` argument). The caller releases
  them with `fasm_file_free`, `fasm_string_free` and `fasm_error_free`.
  Every free function accepts `NULL`. Freeing twice is undefined behaviour
  (it is not detected).
* **Borrowed**: `fasm_line` (`fasm_file_line`), `fasm_set_feature`
  (`fasm_line_set_feature`), and the `fasm_str` views filled in by
  `fasm_line_comment` and `fasm_line_annotation`. They point into the
  owning `fasm_file` and stay valid until it is freed **or modified**
  (`fasm_file_push_line` may reallocate the line array). A `fasm_line`
  passed to a streaming callback is valid only during that call.

Internally a `fasm_file *` is a `Box<Vec<FasmLine>>` (wrapped in a
struct), a `fasm_line *` is a `&FasmLine`, a `fasm_set_feature *` is a
`&SetFasmFeature`: accessors are plain field reads, no conversion or copy
happens until the C side asks for text.

## Errors

**Pattern: status return + optional out-parameter.** Every fallible
function takes a last `fasm_error **err` argument and either returns a
`fasm_status` (`fasm_parse_*`, `fasm_file_push_line`) or returns a pointer
that is `NULL` on failure (`*_to_string`, `fasm_file_merge_and_sort*`,
`fasm_set_feature_value_to_string`). On failure, if `err` is not `NULL`,
`*err` receives a new `fasm_error` holding the status, a NUL terminated
message and, for parse errors, the line (1 based, counted at `\n`) and
column (0 based, in code points: ANTLR's positions, see `COMPAT.md`); on
success `*err` is set to `NULL` (so an `err` variable must not hold an
unfreed error when it is reused). `err` may be `NULL` when only the status
matters.

Why not a thread local "last error" (`errno` style)? An explicit error
object is thread safe without hidden state, cannot be clobbered by an
intervening call (for example a call made from a callback), carries
structured data (line, column), has a clear owner, and costs nothing on
success. The price is one extra argument, which callers can pass as `NULL`.

Status codes (`fasm_status`, values fixed):

| Code | Meaning |
|---|---|
| `FASM_OK` (0) | success |
| `FASM_ERR_PARSE` (1) | the text does not match the grammar or a value does not fit (`ParseErrorKind` other than `Io`/`InvalidUtf8`) |
| `FASM_ERR_IO` (2) | a file could not be read (line and column 0) |
| `FASM_ERR_INVALID_ARG` (3) | `NULL` where an object is required, a bad radix or value format, an invalid `SetFasmFeature` (`ModelError`) |
| `FASM_ERR_UTF8` (4) | text that must be UTF-8 is not: an argument, or a comment/annotation value in parsed text (`ParseErrorKind::InvalidUtf8`, which still has a position) |
| `FASM_ERR_PANIC` (5) | a Rust panic was caught at the boundary (a bug) |
| `FASM_ERR_OUTPUT` (6) | formatting or merging failed (`OutputError`; for example a non canonical feature with `check_if_canonical`, or conflicting bits when merging) |

`FASM_ERR_OUTPUT` is an addition to the codes listed in the task brief:
`OutputError`s are neither argument errors nor parse errors, and a caller
may want to tell them apart. `fasm_status_string` takes an `int` (not a
`fasm_status`) so that any integer, including a code added later, can be
passed without undefined behaviour on the Rust side.

**Panics.** Every entry point runs inside `std::panic::catch_unwind`;
fallible functions turn a panic into `FASM_ERR_PANIC` (the message
includes the panic payload), infallible accessors return their neutral
value (0, `false`, `NULL`). No panic is expected: the core crate reports
all invalid input as errors, and the C API validates everything it passes
on (for example it builds `SetFasmFeature`s with the checking `new`, never
`new_unchecked`). A caught panic is still printed to stderr by Rust's
default panic hook (the library does not install its own hook, which is
process global state the host program may own). Running out of memory is
not caught: like the Rust standard library, the library aborts the
process on allocation failure.

**`NULL` handles.** Every function accepts `NULL` for every handle and
out-parameter: accessors return 0 / `false` / `NULL` / an empty string,
fallible functions return `FASM_ERR_INVALID_ARG`. Dangling or foreign
pointers cannot be detected and are undefined behaviour.

## Strings

* All text is UTF-8.
* **Input** text is pointer + length (`const char *text, size_t len`, or a
  `fasm_str`), not necessarily NUL terminated; `NULL` is accepted with a
  length of 0. File paths are NUL terminated `const char *`: any bytes on
  Unix (converted with `OsStr::from_bytes`), UTF-8 elsewhere.
* **Borrowed output** (`fasm_str`: comments, annotation names and values)
  is **not** NUL terminated: it points directly at the `Box<str>` inside
  the model (zero copy). Print it with `printf("%.*s", (int)s.len, s.ptr)`.
  When `len` is 0 the pointer must not be dereferenced.
* **Owned output** (`fasm_string`) is NUL terminated, and its length is
  also available (`fasm_string_len`); the text may contain NUL bytes only
  if the input did (a comment can hold any character), in which case the
  length is authoritative.
* **Error messages** are NUL terminated and owned by the `fasm_error`
  (NUL bytes are removed).

### Feature names

A feature name is an `IdString`: the `fasm::idstring` interner splits
`TILE.SITE.REST` into up to three separately interned pieces, so there is
no contiguous `&'static str` for a whole name to point C at (the pieces
are `'static`, the joined string is not; `IdString::with_str` joins into a
temporary buffer). Instead of adding a whole-name cache to the core crate
(which would double the interner's memory), the C API offers:

* `fasm_set_feature_name(sf, buf, buf_len)`: copies into a caller buffer
  with `snprintf` semantics (returns the full length; truncates and always
  NUL terminates when `buf_len > 0`) — no allocation;
* `fasm_set_feature_name_len(sf)`: the length, to size a buffer;
* `fasm_set_feature_name_string(sf)`: an owned `fasm_string`.

No change to the `fasm` crate was needed.

### Values

Values have no size limit (`FeatureValue`), so they are exposed as bits:
`fasm_set_feature_value_bits` (bit length, 0 for 0),
`fasm_set_feature_value_u64` (`false` when wider than 64 bits),
`fasm_set_feature_value_bit(i)`, `fasm_set_feature_value_bytes_le` (the
value as little endian bytes; returns the needed size, fills and zero
extends when the buffer is large enough, leaves a too small buffer
untouched) and `fasm_set_feature_value_to_string(radix, uppercase)`.
Input values (`fasm_set_feature_spec.value_le`) use the same little
endian byte format.

`fasm_value_format` keeps Python's `ValueFormat` values 0 to 4 and adds
`FASM_VALUE_FORMAT_NONE = -1` for Python's `value_format is None`. The
input struct holds the format as an `int32_t`, not as the enum, so that an
out of range value from C is rejected (`FASM_ERR_INVALID_ARG`) instead of
being undefined behaviour in Rust.

## Parsing: array and streaming

`fasm_parse_string` / `fasm_parse_file` build a `fasm_file` (`Vec` of
lines). `fasm_parse_string_cb` / `fasm_parse_file_cb` instead call a
`fasm_line_callback(const fasm_line *line, size_t line_number, void *user)`
for each line as it is parsed and drop it afterwards, so memory use does
not grow with the file (the file itself is read into memory by
`fasm_parse_file_cb`). The callback returns `false` to stop early (the
function then returns `FASM_OK`); lines before a parse error are
delivered, then the error is returned. Callbacks (also the merge
callbacks) must not unwind (throw a C++ exception) or `longjmp` through
the library: the callbacks are declared `extern "C"`, and a foreign
exception unwinding into (or a `longjmp` across) Rust frames through an
`extern "C"` boundary is undefined behaviour; it is not guaranteed to be
caught or to abort. C++ callers must catch everything inside the callback
(and can, for example, return `false` to stop parsing and rethrow after
the call returns).

## Building models

`fasm_file_new` and `fasm_file_push_line(file, set_feature_spec,
annotations, count, comment, err)` build a model from plain C structs
(`fasm_set_feature_spec`: name, `has_start`/`start`, `has_end`/`end`,
little endian value bytes, value format; `fasm_annotation`: two
`fasm_str`s; the comment: a `fasm_str *`, `NULL` for none). The set feature
is validated with `SetFasmFeature::new` (end without start, end before
start, value wider than the address are `FASM_ERR_INVALID_ARG`); like the
Python model, names are not checked against the grammar. A failed push
appends nothing. This is what bindings and tools that generate FASM need
(T4.2, T5.10).

## Merge and sort

`fasm_file_merge_and_sort(file)` returns a new `fasm_file` (the input is
cloned and left unchanged). `fasm_file_merge_and_sort_ex` adds Python's
optional callbacks: a `zero_fn(feature, len, user) -> bool` and a
`sort_key_fn(group_id, len, user) -> int64_t` (Python's `sort_key` returns
any comparable object; a C integer key covers the realistic uses, such as
sorting tiles by grid coordinates). The key callback is called exactly
once per group and its result cached (the core sort asks for a key at
every comparison), so it may be non-deterministic (a counter) without
upsetting the sort. Groups with equal keys are ordered by group id, so the
output does not depend on hash map order (Python keeps their order of
first appearance; recorded in `COMPAT.md`). The strings passed to the
callbacks are NUL terminated copies valid during the call.

## Thread safety

* The library has no global mutable state other than the feature name
  interner, which is thread safe (`RwLock` + lock free reads).
* A `fasm_file` (and everything borrowed from it) may be read from any
  number of threads at once. `fasm_file_push_line` and `fasm_file_free`
  need exclusive access.
* `fasm_string` and `fasm_error` are immutable; any thread may read or
  free them (once).
* Any function may be called concurrently on different objects.

## Memory checking and the interner

The C test runs under valgrind (`--leak-check=full
--errors-for-leak-kinds=definite,indirect,possible --error-exitcode=1`).
The global feature name interner (`fasm::idstring::GLOBAL`) never frees
its tables by design (interned names are `'static`); they stay reachable
from the static, so valgrind classifies them as "still reachable", which
is not counted as an error. No suppression file is needed. Everything the
C API allocates for the caller is freed by the matching `*_free`, and the
test frees everything, so any "definitely/indirectly/possibly lost" block
fails the test (checked by removing a `fasm_file_free` call).

## ABI stability

* The crate is at version 0.x: the ABI may still change between releases.
  Once it is declared stable: new functions and new status codes may be
  added; existing function signatures, the layout of the public structs
  (`fasm_str`, `fasm_annotation`, `fasm_set_feature_spec`) and the enum
  values do not change (a new struct field needs a new function or a
  versioned struct).
* Handles are opaque, so their representation can change freely.
* Enums are `repr(C)` (C `int`); functions take `bool` (C99 `_Bool`),
  fixed width integers and `size_t` (Rust `usize`).
* `FASM_API` marks every function: `__declspec(dllimport)` on Windows
  unless `FASM_STATIC` is defined (static linking), default visibility
  with GCC/Clang.
* `fasm_version()` returns the crate version.

## Header generation

`include/fasm/fasm.h` is generated, never edited by hand:

```
cargo install cbindgen --locked   # once
make capi-header                   # = cbindgen --config rust/fasm-capi/cbindgen.toml \
                                   #     --crate fasm-capi --output include/fasm/fasm.h
```

The configuration is `rust/fasm-capi/cbindgen.toml` (C language, include
guard `FASM_FASM_H`, `extern "C"` guards for C++, `stdbool.h` /
`stddef.h` / `stdint.h`, doc comments carried over, declarations in source
order, the `FASM_API` macro). Generation is an explicit step, not a
`build.rs`, so building the library never needs cbindgen. The test
`rust/fasm-capi/tests/header.rs` regenerates the header into
`target/tmp/` and fails if it differs from the checked in one; it is
skipped (with a message) when cbindgen is not installed, unless
`FASM_REQUIRE_CBINDGEN=1` is set, which `make capi-header-check` does.

## C tests

`rust/fasm-capi/tests/c/test_capi.c` (C99, no framework) exercises every
function: `examples/many.fasm` against values from
`tests/corpus/oracle/many.json`, output identical to
`tests/corpus/oracle/many.fasm.out.txt` / `.canonical.txt`, a round trip,
256 bit values, parse errors with positions, missing files, invalid UTF-8,
streaming with early stop, merge and sort (with and without callbacks),
building and printing a model, and `NULL` handling.
`rust/fasm-capi/tests/c/CMakeLists.txt` builds it against the shared and
the static library from the Cargo target directory (`-Wall -Wextra
-Werror -std=c99`) and registers ctest tests, plus valgrind variants when
valgrind is installed. `make capi-test` runs `cargo build -p fasm-capi`,
configures and builds in `target/capi-tests` (under `$CARGO_TARGET_DIR`
when set), and runs ctest. The same behaviour is also tested from Rust
(`rust/fasm-capi/src/tests.rs`, `cargo test -p fasm-capi`), including the
layout of the public structs (`struct_layout`, matched by the C test's
`sizeof`/`offsetof` checks).

The Rust tests also run under Miri, which checks the unsafe code (pointer
casts, borrowed views, the callbacks' `user` pointers) for undefined
behaviour:

```
cargo +nightly miri test -p fasm-capi --lib
```

Under Miri's default isolation there is no file system access: the tests
parse the embedded `examples/many.fasm` with `fasm_parse_string` instead
of `fasm_parse_file`, and skip the file error tests.

# C/C++ API guide

User guide for `libfasm_capi`: the C ABI in `include/fasm/fasm.h`
(generated from `rust/fasm-capi` with cbindgen) and the header only C++17
RAII wrapper `include/fasm/fasm.hpp`. It covers building/installing,
linking, the error pattern, ownership rules, threading, ABI stability,
and complete examples for parse/print/merge and for the Xilinx frame and
bitstream flow. The full design rationale (why each rule exists, and the
Xilinx bindings) lives in `docs/rewrite/DESIGN-capi.md`; this guide is
kept consistent with it and with `include/fasm/fasm.h`'s own doc comments
— regenerate the header with `make capi-header` after changing
`rust/fasm-capi`'s Rust doc comments, and check it is still in sync with
`make capi-header-check`.

## Build and install

```sh
make capi-header          # regenerate include/fasm/fasm.h with cbindgen
make capi-header-check     # verify it is up to date (used by CI)
make capi-test             # build+run the C and C++ test programs (CMake, ctest)
make capi-install PREFIX=/some/prefix
```

`capi-install` installs `fasm.h`, `fasm.hpp`, `libfasm_capi.{so,a}`
(release profile), a generated `lib/pkgconfig/fasm.pc` and a CMake
package config (`lib/cmake/fasm/{fasmConfig,fasmConfigVersion}.cmake`,
`rust/fasm-capi/cmake/fasmConfig.cmake.in`) into
`PREFIX/{include/fasm,lib}`. `rust/fasm-capi/examples/cpp/` is a minimal
example project that finds the installed library with pkg-config or
CMake's `PkgConfig` module (both shown below); a project built entirely
with CMake will more often want `find_package(fasm CONFIG)` instead (see
"With CMake's `find_package(fasm CONFIG)`" below).

## Linking

With pkg-config (dynamic, then static):

```sh
export PKG_CONFIG_PATH=/some/prefix/lib/pkgconfig

g++ -std=c++17 $(pkg-config --cflags fasm) example.cpp \
    $(pkg-config --libs fasm) -o example
LD_LIBRARY_PATH=/some/prefix/lib ./example

g++ -std=c++17 $(pkg-config --cflags fasm) example.cpp \
    -Wl,-Bstatic $(pkg-config --libs fasm) \
    -Wl,-Bdynamic $(pkg-config --static --libs-only-l fasm | sed 's/-lfasm_capi//') \
    -o example_static
```

With CMake's `PkgConfig` module:

```cmake
find_package(PkgConfig REQUIRED)
pkg_check_modules(fasm REQUIRED IMPORTED_TARGET fasm)
target_link_libraries(my_target PRIVATE PkgConfig::fasm)
```

For C, drop `-std=c++17` for your C compiler's flag (C99 is enough); the
library is usable from C alone with just `fasm.h`.

### With CMake's `find_package(fasm CONFIG)`

`make capi-install` also installs a CMake package config, so a project
built entirely with CMake can skip pkg-config and use `find_package`
directly. It defines two imported targets, one per library kind (there
is no default: pick whichever `fasm.pc`'s pkg-config flags would give
you above):

* `fasm::fasm_capi` — the shared library (`libfasm_capi.so`/`.dylib`/`.dll`
  at run time, resolved through `LD_LIBRARY_PATH`/rpath/ldconfig, an
  install name on macOS, or PATH on Windows, same as the pkg-config
  dynamic case above).
* `fasm::fasm_capi_static` — the static library (`libfasm_capi.a`/`.lib`);
  no run time dependency on `libfasm_capi.so` at all (adds the system
  libraries the Rust standard library needs and the `FASM_STATIC`
  compile definition automatically, same as the `-Wl,-Bstatic` pkg-config
  recipe above).

```cmake
cmake_minimum_required(VERSION 3.13)
project(fasm_example CXX)

find_package(fasm 0.1.0 CONFIG REQUIRED)

add_executable(fasm_example example.cpp)
set_target_properties(fasm_example PROPERTIES
    CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON)
target_link_libraries(fasm_example PRIVATE fasm::fasm_capi)        # or:
# target_link_libraries(fasm_example PRIVATE fasm::fasm_capi_static)
```

```sh
cmake -S . -B build -Dfasm_DIR=/some/prefix/lib/cmake/fasm
cmake --build build
LD_LIBRARY_PATH=/some/prefix/lib ./build/fasm_example    # only for fasm::fasm_capi
```

(`-Dfasm_DIR=...` is only needed when `/some/prefix` is not already on
`CMAKE_PREFIX_PATH`/a default search path such as `/usr/local`.) Verified
against `rust/fasm-capi/examples/cpp/example.cpp` (T8.4): both targets
build and run correctly, and — because the installed `libfasm_capi.so`
has no SONAME, which `fasmConfig.cmake.in` accounts for with
`IMPORTED_NO_SONAME` — the resulting binary's ELF `NEEDED` entry for it
is a plain `libfasm_capi.so`, not an unmovable absolute build-tree path
(`readelf -d build/fasm_example | grep NEEDED` to check).

## The error pattern

Every fallible function takes a trailing `fasm_error **err` and either
returns a `fasm_status` or a pointer that is `NULL` on failure:

```c
fasm_file *file = NULL;
fasm_error *err = NULL;
fasm_status st = fasm_parse_string(text, text_len, &file, &err);
if (st != FASM_OK) {
    /* fasm_error_line/column return size_t: print with %zu. */
    fprintf(stderr, "%s at %zu:%zu: %s\n", fasm_status_string(st),
            fasm_error_line(err), fasm_error_column(err),
            fasm_error_message(err));
    fasm_error_free(err);
    return 1;
}
```

`err` may be `NULL` when only the status matters. On success `*err` is
always set to `NULL`. Status codes (`fasm_status`, stable values):

| Code | Meaning |
|---|---|
| `FASM_OK` (0) | success |
| `FASM_ERR_PARSE` (1) | grammar/value error (has line/column) |
| `FASM_ERR_IO` (2) | a file could not be read |
| `FASM_ERR_INVALID_ARG` (3) | `NULL` where required, bad radix/format/model |
| `FASM_ERR_UTF8` (4) | invalid UTF-8 in an argument or parsed text |
| `FASM_ERR_PANIC` (5) | a Rust panic was caught at the boundary (a bug) |
| `FASM_ERR_OUTPUT` (6) | formatting/merging failed |
| `FASM_ERR_DB` … `FASM_ERR_FRM` (7-12) | `fasm_xilinx_*` errors, see below |

Every entry point runs inside `catch_unwind`: a panic becomes
`FASM_ERR_PANIC` on a fallible function, or the neutral value (`0` /
`false` / `NULL`) on an infallible accessor — it never unwinds into your
code. Every function accepts `NULL` for every handle and out-parameter.

The C++ wrapper turns this into exceptions instead: every failure throws
`fasm::Error : std::runtime_error`, with `what()`, `status()`, `line()`,
`column()` and `has_position()`.

## Ownership

* **Owned** handles (`fasm_file *` from `fasm_parse_*`/`fasm_file_new`/
  `fasm_file_merge_and_sort*`; `fasm_string *`; `fasm_error *`) are
  released by the caller with the matching `fasm_*_free`, which accepts
  `NULL`. Freeing twice is undefined behaviour.
* **Borrowed** handles (`fasm_line *`, `fasm_set_feature *`, `fasm_str`
  views) point into their owning `fasm_file` and are valid until it is
  freed or modified (`fasm_file_push_line` may reallocate); a `fasm_line`
  passed to a streaming callback is valid only during that call.
* **Strings**: input text is pointer+length, not necessarily NUL
  terminated. Borrowed output (`fasm_str`, e.g. comments) is **not** NUL
  terminated — print with `printf("%.*s", (int)s.len, s.ptr)`. Owned
  output (`fasm_string`) is NUL terminated and also carries its length.

In C++, `File` and `String` are move-only RAII types (like
`std::unique_ptr`); `Line`/`SetFeature`/`Value`/`std::string_view`s are
cheap, non-owning views with the same lifetime rules as their C
counterparts — do not let one escape a `parse_each`/`parse_each_file`
callback.

## Threading

* The library's only global mutable state is the feature-name interner,
  which is internally thread-safe.
* A `fasm_file` (and anything borrowed from it) may be **read** from any
  number of threads at once; `fasm_file_push_line`/`fasm_file_free` need
  exclusive access to that file. `fasm_string`/`fasm_error` are immutable
  and may be freed once from any thread.
* Xilinx: databases, parts and frames may be read from any number of
  threads; an assembler and `fasm_xilinx_frames_set` need exclusive
  access.
* Callbacks (`fasm_line_callback`, `fasm_zero_fn`, `fasm_sort_key_fn`,
  Xilinx warning callbacks) must not unwind or `longjmp` across the
  `extern "C"` boundary — undefined behaviour. The C++ wrapper's
  callback-taking functions handle this for you: they install a
  captureless trampoline that catches every exception, stores the first
  one, and rethrows it after the C call returns (see
  `docs/rewrite/DESIGN-capi.md` "Exception trampoline rule" for the exact
  first-exception-wins semantics under `merge_and_sort`).

## ABI stability

The crate is at version 0.x, so the ABI may still change before a stable
release. Once declared stable: new functions/status codes may be added
freely; existing function signatures, the layout of public structs
(`fasm_str`, `fasm_annotation`, `fasm_set_feature_spec`,
`fasm_xilinx_*_options`) and enum values will not change (a new field
needs a new struct/function). Handles are opaque and may change freely.
Enums passed in structs/arguments are plain integers (`int32_t`), so an
out-of-range value from an old build is a normal `FASM_ERR_INVALID_ARG`
rather than undefined behaviour. `fasm_version()` reports the crate
version at runtime.

## Example: parse, print, merge (C)

```c
#include <fasm/fasm.h>
#include <stdio.h>
#include <string.h>

int main(void) {
    const char *text = "TILE.FEATURE\nTILE.OTHER[3:0] = 4'b0101\n";
    fasm_file *file = NULL;
    fasm_error *err = NULL;

    if (fasm_parse_string(text, strlen(text), &file, &err) != FASM_OK) {
        fprintf(stderr, "parse error: %s\n", fasm_error_message(err));
        fasm_error_free(err);
        return 1;
    }

    /* Group/sort the model (not canonical form: pass canonical=true to
     * fasm_file_to_string below for that instead). */
    fasm_file *merged = fasm_file_merge_and_sort(file, &err);
    if (!merged) {
        fprintf(stderr, "merge error: %s\n", fasm_error_message(err));
        fasm_error_free(err);
        fasm_file_free(file);
        return 1;
    }

    fasm_string *out = fasm_file_to_string(merged, /* canonical = */ false, &err);
    if (out) {
        printf("%s", fasm_string_data(out));
        fasm_string_free(out);
    }

    fasm_file_free(merged);
    fasm_file_free(file);
    return 0;
}
```

## Example: parse, print, merge (C++)

```cpp
#include <fasm/fasm.hpp>
#include <iostream>

int main() {
    try {
        auto file = fasm::File::parse(
            "TILE.FEATURE\nTILE.OTHER[3:0] = 4'b0101\n");
        fasm::File merged = file.merge_and_sort();
        std::cout << merged.to_string();

        for (const fasm::Line &line : merged) {
            if (auto sf = line.set_feature()) {
                std::cout << sf->name() << " = " << sf->value().to_string(10)
                          << '\n';
            }
        }
    } catch (const fasm::Error &e) {
        std::cerr << e.what() << '\n';
        return 1;
    }
    return 0;
}
```

## Xilinx: FASM -> frames -> bitstream

The `fasm_xilinx_*` functions (and `namespace fasm::xilinx` in C++) mirror
the same rules — opaque handles, `*_free` accepting `NULL`,
`fasm_error **`/exceptions, `catch_unwind` at every entry point — with
their own status codes `FASM_ERR_DB` (7), `FASM_ERR_LOOKUP` (8),
`FASM_ERR_INCONSISTENT_BITS` (9), `FASM_ERR_ASSEMBLER` (10),
`FASM_ERR_BITSTREAM` (11), `FASM_ERR_FRM` (12), and
`fasm_error_kind(err)`, which names the reference Python tool's exception
class (e.g. `prjxray.fasm_assembler.FasmLookupError`) so that
`kind + ": " + message` matches `fasm2frames`' own stderr line exactly.

### C

```c
fasm_xilinx_database *db = NULL;
fasm_xilinx_frames *frames = NULL;
fasm_xilinx_part *part = NULL;
fasm_error *err = NULL;
fasm_xilinx_fasm2frames_options options = {0};
options.sparse = true;

if (fasm_xilinx_database_open_cached("prjxray-db/artix7",
        "xc7a35tcsg324-1", NULL, &db, &err) ||
    fasm_xilinx_fasm2frames_file(db, "top.fasm", &options, &frames, &err) ||
    fasm_xilinx_part_from_database(db, &part, &err) ||
    fasm_xilinx_bitstream_write_file(part, frames, NULL, "top.bit", &err)) {
    fprintf(stderr, "%s: %s\n", fasm_error_kind(err), fasm_error_message(err));
    fasm_error_free(err);
}

fasm_xilinx_part_free(part);
fasm_xilinx_frames_free(frames);
fasm_xilinx_database_free(db);
```

### C++

```cpp
namespace fx = fasm::xilinx;
try {
    auto db = fx::Database::open_cached("prjxray-db/artix7", "xc7a35tcsg324-1");
    fx::Frames frames = fx::fasm2frames(db, "top.fasm");
    frames.write_frm("top.frm");
    fx::write_bitstream_file(fx::Part::from_database(db), frames, "top.bit");
} catch (const fasm::Error &e) {
    std::cerr << e.kind() << ": " << e.what() << '\n';
    return 1;
}
```

prjuray-db (UltraScale/UltraScale+) parts work the same way; select the
bitstream variant with `fasm_xilinx_bitstream_options.format` /
`fx::BitstreamFormat` (`Series7`, `UltraScale`, `UltraScalePlus`, or the
`prjxray:` variants — see `docs/rewrite/DESIGN-python.md`'s equivalent
Python `format=` argument for the full list).

## Testing and memory checking

`make capi-test` builds and runs the C and C++ test programs (CMake +
ctest) against both the shared and static library, including valgrind
variants (`--leak-check=full
--errors-for-leak-kinds=definite,indirect,possible`) when valgrind is
installed; the feature-name interner is expected to show as "still
reachable" (by design — it never frees its tables), not as a leak. The
Rust side of the API is additionally checked with `cargo +nightly miri
test -p fasm-capi --lib` for undefined behaviour in the unsafe pointer
and callback code.

## Further reading

* `docs/rewrite/DESIGN-capi.md`: the full design record (every function
  listed by area, the reasoning behind the error/ownership/ABI rules, the
  C++ wrapper's exception-trampoline mechanics, build/packaging details).
* `docs/rewrite/COMPAT.md`: any documented behavioural divergence from
  the reference tools.
* `include/fasm/fasm.h`, `include/fasm/fasm.hpp`: the generated header
  and the wrapper, with a doc comment on every function/type.

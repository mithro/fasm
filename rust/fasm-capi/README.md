# fasm-capi

C ABI (`libfasm_capi`) for the `fasm` crate (in the same workspace, at
`rust/fasm`): a stable C header (`include/fasm/fasm.h`, generated with
cbindgen) and a header-only C++17 RAII wrapper (`include/fasm/fasm.hpp`),
covering parsing, the data model, output/merge, and (behind the same
library) the `fasm-xilinx` frame/bitstream functions.

This crate is one piece of the [chipsalliance/fasm](https://github.com/chipsalliance/fasm)
rewrite. See [docs/CAPI.md](https://github.com/chipsalliance/fasm/blob/main/docs/CAPI.md)
in the repository for the C/C++ usage guide and build/install
instructions (`make capi-install`, and the CMake package it installs:
`find_package(fasm CONFIG)`).

Not yet published to crates.io (see `docs/RELEASING.md`); once it would
be, this crate is meant for `cargo install`/vendoring convenience (its
docs.rs page would then be at `docs.rs/fasm-capi`, or its published
name if renamed alongside `fasm`) -- consumers linking C/C++ code should
use the CMake or pkg-config install described in docs/CAPI.md rather
than depending on this crate directly from Cargo.

Licensed under Apache-2.0, see
[LICENSE](https://github.com/chipsalliance/fasm/blob/main/LICENSE).

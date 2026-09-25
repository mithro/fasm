# fasm-capi

C ABI (`libfasm_capi`) for the [`fasm`](https://crates.io/crates/fasm)
crate: a stable C header (`include/fasm/fasm.h`, generated with cbindgen)
and a header-only C++17 RAII wrapper (`include/fasm/fasm.hpp`), covering
parsing, the data model, output/merge, and (behind the same library) the
`fasm-xilinx` frame/bitstream functions.

This crate is one piece of the [chipsalliance/fasm](https://github.com/chipsalliance/fasm)
rewrite. See [docs/CAPI.md](https://github.com/chipsalliance/fasm/blob/main/docs/CAPI.md)
in the repository for the C/C++ usage guide, build and install
instructions (`make capi-install`, CMake package), and
[docs.rs/fasm-capi](https://docs.rs/fasm-capi) for the Rust-side API this
library wraps.

This crate publishes the Rust source of the C ABI to crates.io for
`cargo install`/vendoring convenience; consumers linking C/C++ code
should use the CMake or pkg-config install described in docs/CAPI.md
rather than depending on this crate directly from Cargo.

Licensed under Apache-2.0, see
[LICENSE](https://github.com/chipsalliance/fasm/blob/main/LICENSE).

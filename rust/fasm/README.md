# fasm

Core Rust library for the FASM (FPGA Assembly) file format: idstring
interning, the data model, the parser, and output/merge support.

This crate is one piece of the [chipsalliance/fasm](https://github.com/chipsalliance/fasm)
rewrite. See the [repository README](https://github.com/chipsalliance/fasm#readme)
for the full picture (Python bindings, C/C++ API, Xilinx tooling).

Not yet published to crates.io (see `docs/RELEASING.md`, including a
crates.io name collision this crate's own name needs resolving first);
once it is, its docs.rs page is at `docs.rs/<published crate name>`.
Until then, build the API reference locally with `cargo doc --open -p
fasm` from a checkout.

Licensed under Apache-2.0, see
[LICENSE](https://github.com/chipsalliance/fasm/blob/main/LICENSE).

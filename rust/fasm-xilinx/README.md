# fasm-xilinx

Xilinx-specific FASM tooling: a prjxray/prjuray tile-grid database loader,
a FASM to configuration-frames assembler, and a frames to bitstream
writer, for Series-7, UltraScale and UltraScale+ parts.

This crate is one piece of the [chipsalliance/fasm](https://github.com/chipsalliance/fasm)
rewrite; it builds on the `fasm` crate (in the same workspace, at
`rust/fasm`). See the [repository README](https://github.com/chipsalliance/fasm#readme)
for the full picture.

Not yet published to crates.io (see `docs/RELEASING.md`); once it is,
its docs.rs page is at `docs.rs/fasm-xilinx` (or its published name, if
renamed alongside `fasm`, see `docs/RELEASING.md`). Until then, build
the API reference locally with `cargo doc --open -p fasm-xilinx` from a
checkout.

Licensed under Apache-2.0, see
[LICENSE](https://github.com/chipsalliance/fasm/blob/main/LICENSE).

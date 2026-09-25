# fasm-cli

Command line tools for FASM: the `fasm` binary (drop-in compatible with
the original Python `fasm/tool.py`), the Xilinx bitstream tools
(`fasm2frames`, `xcfasm`, `xc7frames2bit`, `bitread`, and prjuray's
`xcframes2bit` and its own `fasm2frames`/`bitread` as `uray-fasm2frames`/
`uray-bitread`), and the `fasm-db-cache` database cache tool.

This crate is one piece of the [chipsalliance/fasm](https://github.com/chipsalliance/fasm)
rewrite. See the [repository README](https://github.com/chipsalliance/fasm#readme)
for install and usage instructions (the binaries are the primary product
of this crate).

Not yet published to crates.io (see `docs/RELEASING.md`); once it is,
its docs.rs page is at `docs.rs/fasm-cli` (or its published name, if
renamed alongside `fasm`, see `docs/RELEASING.md`).

Install with `cargo install fasm-cli`, or build from a checkout with
`cargo build --release -p fasm-cli`.

Licensed under Apache-2.0, see
[LICENSE](https://github.com/chipsalliance/fasm/blob/main/LICENSE).

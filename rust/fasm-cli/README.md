# fasm-cli

Command line tools for FASM: the `fasm` binary (drop-in compatible with
the original Python `fasm/tool.py`), the Xilinx bitstream tools
(`fasm2frames`, `xcfasm`, `xc7frames2bit`, `bitread`, and prjuray's
`xcframes2bit`/`bitread` as `uray-xcframes2bit`/`uray-bitread`), and the
`fasm-db-cache` database cache tool.

This crate is one piece of the [chipsalliance/fasm](https://github.com/chipsalliance/fasm)
rewrite. See the [repository README](https://github.com/chipsalliance/fasm#readme)
for install and usage instructions and
[docs.rs/fasm-cli](https://docs.rs/fasm-cli) for the library API reference
(the binaries are the primary product of this crate).

Install with `cargo install fasm-cli`, or build from a checkout with
`cargo build --release -p fasm-cli`.

Licensed under Apache-2.0, see
[LICENSE](https://github.com/chipsalliance/fasm/blob/main/LICENSE).

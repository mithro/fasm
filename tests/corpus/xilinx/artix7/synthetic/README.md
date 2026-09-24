# Synthetic artix7 FASM for the `fasm2frames` differential tests

Hand written FASM for part `xc7a35tcsg324-1` (fabric `xc7a50t`) of the
pinned prjxray-db `artix7` database (`tools/fetch-db.sh prjxray artix7`,
commit `0a0addedd73e7e4139d52a6d8db4258763e0f1f3`), each exercising
assembler paths of prjxray's `fasm_assembler.py` and f4pga-xc-fasm's
`fasm2frames.py` on real tile types. `tools/difftest-xilinx.py` runs the
reference and the Rust `fasm2frames` on every file here (dense,
`--sparse`, `--emit_pudc_b_pullup`, `--sparse --debug`, and `--sparse
--roi <stem>.roi.json` when that file exists) and compares the results;
`rust/fasm-xilinx/tests/assembler_real_db.rs` compares the Rust output
with the checked in reference output.

* `multibit_stepdown.fasm`: multi bit `INIT` features (full, partial,
  single bit ranges; hex, binary and decimal values; an all zero range),
  features with `!` bits, block RAM `INIT_xx`/`INITP_xx` (the
  `BLOCK_RAM` bus), a pseudo PIP, a STEPDOWN feature on an IOB of bank 14
  (propagated to every unused IOB site of the bank and to
  `HCLK_IOI3_X1Y26`), and an alias tile (`LIOB33_SING_X0Y0`) whose bits
  wrap to the end of the frame.
  `multibit_stepdown.roi.json` is a ROI `design.json` for it (grid
  rectangle and `required_features`).
* `pudc_in_use.fasm`: uses the PUDC_B IOB site (with a feature whose value
  is 0), so `--emit_pudc_b_pullup` adds nothing; STEPDOWN on bank 16.
* `sing_out_of_frame.fasm`: the top alias tile of a clock region
  (`LIOB33_SING_X0Y49`, offset 99): the other site's bits are past the end
  of the frame and dropped with prjxray's `frame_set`/`frame_clear`
  warning on stderr.
* `errors/*.fasm`: the reference's error behaviour: batched
  `FasmLookupError`s (`unknown_feature.fasm`), a `KeyError` for an unknown
  tile (`unknown_tile.fasm`) or a STEPDOWN IOB tile without IO bank
  (`stepdown_no_bank.fasm`), `FasmInconsistentBits`
  (`inconsistent.fasm`) and a syntax error (`parse_error.fasm`).

No octal and no large decimal values are used: the reference's ANTLR
parser misreads them (see the parser section of `docs/rewrite/COMPAT.md`).

## Reference output

`multibit_stepdown.sparse.frm.xz` and `multibit_stepdown.roi.frm.xz`
(`xz -9`) are the reference's output, made with the tools built by
`tests/oracle/setup-xilinx.sh` (prjxray
`c9f02d8576042325425824647ab5555b1bc77833`, f4pga-xc-fasm
`25dc605c9c0896204f0c3425b52a332034cf5e5c`):

```sh
DB=tests/oracle/build/db/prjxray-db/artix7
tests/oracle/fasm2frames-oracle --db-root $DB --part xc7a35tcsg324-1 --sparse \
    multibit_stepdown.fasm multibit_stepdown.sparse.frm
tests/oracle/fasm2frames-oracle --db-root $DB --part xc7a35tcsg324-1 --sparse \
    --roi multibit_stepdown.roi.json multibit_stepdown.fasm multibit_stepdown.roi.frm
xz -9 multibit_stepdown.sparse.frm multibit_stepdown.roi.frm
```

sha256 of the uncompressed files:

```
c33d0e12d0a54bbc4b4f8de970fbf8267b62bf29d5917fd55aac812b7408a514  multibit_stepdown.sparse.frm
bd4ac2e4d6adc223106d11a5c4eb0296ec90e2ff1cdef15aba611d6f25825e2a  multibit_stepdown.roi.frm
```

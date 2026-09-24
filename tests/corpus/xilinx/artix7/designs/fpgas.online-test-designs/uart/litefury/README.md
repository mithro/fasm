# uart / litefury -- fpgas.online-test-designs FASM (T7.2)

FASM produced by building fpgas.online-test-designs' `uart` design for
the `litefury` board with LiteX + the openXC7 flow (T7.1/T7.2). `top.fasm.xz`
is real, working FASM (not hand-written).

Regenerate with:

```
tools/e2e/setup-openxc7.sh --parts xc7a100tfgg484-2   # from /home/user/fasm (main tree); once per part
tools/e2e/setup-litex.sh                    # once per machine
tools/e2e/run-fpgas-online.sh uart litefury
tools/e2e/install-fpgas-online-corpus.sh uart litefury
```

## Target

* Part: `xc7a100tfgg484-2`
* Design: fpgas.online-test-designs `designs/uart/` (see its own
  `README.md` in that repository for what the test verifies)
* Board: `litefury`
* Gateware script: `designs/uart/gateware/uart_soc_acorn.py`

## Source provenance

* fpgas.online-test-designs commit `37d24079b28179558632abc12fd92af4ff00a036`
  (<https://github.com/fpgas-online/fpgas.online-test-designs>, Apache-2.0)
* LiteX stack pinned to the same commits as that repository's own
  `uv.lock` -- see `tools/e2e/setup-litex.sh` header comment and
  `tools/e2e/build/litex-venv/status.json`.
* openXC7 toolchain: see
  `tests/corpus/xilinx/artix7/designs/f4pga-examples/counter_test/arty_35/README.md`
  for exact yosys/nextpnr-xilinx/openXC7 versions and provenance (same
  toolchain install, shared across all T7.x corpus designs on this
  machine).
* `.frm`/`.bit` regenerated from the FASM with the ORACLE tools
  (`tests/oracle/fasm2frames-oracle`, `tests/oracle/xc7frames2bit-oracle`,
  `tests/oracle/bitread-oracle` -- f4pga-xc-fasm + prjxray C++, built by
  `tests/oracle/setup-xilinx.sh`), **not** openXC7's own bundled copies of
  the same tools, so this is directly comparable with the rest of the Rust
  rewrite's differential tests.

## Commands (as run by tools/e2e/run-fpgas-online.sh)

```
# LiteX build (yosys synth_xilinx -> nextpnr-xilinx -> FASM; the exact
# commands are in the generated designs/uart/build/litefury/build_top.sh
# or */gateware/build_*.sh, which is not committed):
CHIPDB=<chipdb overlay dir> PRJXRAY_DB_DIR=<prjxray-db root> \
  tools/e2e/build/litex-venv/bin/python \
  tools/e2e/build/fpgas.online-test-designs/designs/uart/gateware/uart_soc_acorn.py \
  --toolchain openxc7 --build --variant cle-101 --no-compile-software

# Reference frames + bitstream (from the FASM above):
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a100tfgg484-2 top.fasm top.frm
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a100tfgg484-2 --sparse top.fasm top.sparse.frm
tests/oracle/xc7frames2bit-oracle -frm_file top.frm -output_file top.bit \
  -part_name xc7a100tfgg484-2 -part_file <prjxray-db>/artix7/xc7a100tfgg484-2/part.yaml
```

## Output checksums (this run; `.bit` is NOT committed)

```
sha256  top.fasm (uncompressed)   751a9a31b60ced87644dfa15825ed4cca0164b4748e1fa982a4b714d5ca8968d
sha256  top.frm (uncompressed)    759e4604a48929d6e94f76a48029517bea024c59a68002065784dc3fda4c0cbb
sha256  top.bit (not committed)   0301e4445991d9a2e922f85681ee62d35fd52213fbf4196c15e6d4d01725915e
```

top.fasm: 88358 lines / 3512436 bytes uncompressed, 335512 bytes as top.fasm.xz
top.frm: 10111464 bytes uncompressed, 107072 bytes as top.frm.xz
top.sparse.frm: 98408 bytes as top.sparse.frm.xz
top.bit: 3825992 bytes (not committed; regenerate to verify)

## Timings (this machine, this run)

* LiteX build (synth + PnR + FASM): 74s
* fasm2frames (dense): 8s
* fasm2frames (sparse): 8s
* xc7frames2bit: 0s

Generated 2026-09-24.

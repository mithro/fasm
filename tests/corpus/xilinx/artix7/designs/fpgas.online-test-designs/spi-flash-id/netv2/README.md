# spi-flash-id / netv2 -- fpgas.online-test-designs FASM (T7.2)

FASM produced by building fpgas.online-test-designs' `spi-flash-id` design for
the `netv2` board with LiteX + the openXC7 flow (T7.1/T7.2). `top.fasm.xz`
is real, working FASM (not hand-written).

Regenerate with:

```
tools/e2e/setup-openxc7.sh --parts xc7a35tfgg484-2   # from /home/user/fasm (main tree); once per part
tools/e2e/setup-litex.sh                    # once per machine
tools/e2e/run-fpgas-online.sh spi-flash-id netv2
tools/e2e/install-fpgas-online-corpus.sh spi-flash-id netv2
```

## Target

* Part: `xc7a35tfgg484-2`
* Design: fpgas.online-test-designs `designs/spi-flash-id/` (see its own
  `README.md` in that repository for what the test verifies)
* Board: `netv2`
* Gateware script: `designs/spi-flash-id/gateware/spiflash_soc_netv2.py`

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
# commands are in the generated designs/spi-flash-id/build/netv2/build_top.sh
# or */gateware/build_*.sh, which is not committed):
CHIPDB=<chipdb overlay dir> PRJXRAY_DB_DIR=<prjxray-db root> \
  tools/e2e/build/litex-venv/bin/python \
  tools/e2e/build/fpgas.online-test-designs/designs/spi-flash-id/gateware/spiflash_soc_netv2.py \
  --toolchain openxc7 --build --variant a7-35 --no-compile-software

# Reference frames + bitstream (from the FASM above):
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tfgg484-2 top.fasm top.frm
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tfgg484-2 --sparse top.fasm top.sparse.frm
tests/oracle/xc7frames2bit-oracle -frm_file top.frm -output_file top.bit \
  -part_name xc7a35tfgg484-2 -part_file <prjxray-db>/artix7/xc7a35tfgg484-2/part.yaml
```

## Output checksums (this run; `.bit` is NOT committed)

```
sha256  top.fasm (uncompressed)   06b34284fb888c0aa4e0ae076343dedea25d26bde23ee744dd3faed6a940b38a
sha256  top.frm (uncompressed)    315781cf3299785876278e28583674a4a0fe6dcce5c22bc398a13e8bc5a4dece
sha256  top.bit (not committed)   7347f6f942b5660b4a046064d206f657d458674b69890541d4c231b3bf8c2f3c
```

top.fasm: 59106 lines / 2114236 bytes uncompressed, 224760 bytes as top.fasm.xz
top.frm: 5580828 bytes uncompressed, 70268 bytes as top.frm.xz
top.sparse.frm: 64876 bytes as top.sparse.frm.xz
top.bit: 2192220 bytes (not committed; regenerate to verify)

## Timings (this machine, this run)

* LiteX build (synth + PnR + FASM): 63s
* fasm2frames (dense): 1s
* fasm2frames (sparse): 2s
* xc7frames2bit: 0s

Generated 2026-09-24.

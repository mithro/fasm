# pmod-pin-id / arty -- fpgas.online-test-designs FASM (T7.2)

FASM produced by building fpgas.online-test-designs' `pmod-pin-id` design for
the `arty` board with LiteX + the openXC7 flow (T7.1/T7.2). `top.fasm.xz`
is real, working FASM (not hand-written).

Regenerate with:

```
tools/e2e/setup-openxc7.sh --parts xc7a35tcsg324-1   # from /home/user/fasm (main tree); once per part
tools/e2e/setup-litex.sh                    # once per machine
tools/e2e/run-fpgas-online.sh pmod-pin-id arty
tools/e2e/install-fpgas-online-corpus.sh pmod-pin-id arty
```

## Target

* Part: `xc7a35tcsg324-1`
* Design: fpgas.online-test-designs `designs/pmod-pin-id/` (see its own
  `README.md` in that repository for what the test verifies)
* Board: `arty`
* Gateware script: `designs/pmod-pin-id/gateware/pmod_pin_id_arty.py`

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
# commands are in the generated designs/pmod-pin-id/build/arty/build_top.sh
# or */gateware/build_*.sh, which is not committed):
CHIPDB=<chipdb overlay dir> PRJXRAY_DB_DIR=<prjxray-db root> \
  tools/e2e/build/litex-venv/bin/python \
  tools/e2e/build/fpgas.online-test-designs/designs/pmod-pin-id/gateware/pmod_pin_id_arty.py \
  --toolchain openxc7 --build

# Reference frames + bitstream (from the FASM above):
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tcsg324-1 top.fasm top.frm
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tcsg324-1 --sparse top.fasm top.sparse.frm
tests/oracle/xc7frames2bit-oracle -frm_file top.frm -output_file top.bit \
  -part_name xc7a35tcsg324-1 -part_file <prjxray-db>/artix7/xc7a35tcsg324-1/part.yaml
```

## Output checksums (this run; `.bit` is NOT committed)

```
sha256  top.fasm (uncompressed)   90efb4fff6efb683e21487f8c11095dff26fbfa35668535017e8122e7d44ddf5
sha256  top.frm (uncompressed)    abf6990faf81392ecbbab9bce9145d55b5a70a8c7b6c34aa19523e5cacf8d834
sha256  top.bit (not committed)   a4f7c613b8323062fe9b2be3d2e4cb0b87e126dcdea4201898e9039d298b3a98
```

top.fasm: 34578 lines / 1226598 bytes uncompressed, 114468 bytes as top.fasm.xz
top.frm: 5580828 bytes uncompressed, 44644 bytes as top.frm.xz
top.sparse.frm: 39740 bytes as top.sparse.frm.xz
top.bit: 2192218 bytes (not committed; regenerate to verify)

## Timings (this machine, this run)

* LiteX build (synth + PnR + FASM): 22s
* fasm2frames (dense): 1s
* fasm2frames (sparse): 1s
* xc7frames2bit: 0s

Generated 2026-09-24.

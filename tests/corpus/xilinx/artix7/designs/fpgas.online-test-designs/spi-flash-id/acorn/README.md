# spi-flash-id / acorn -- fpgas.online-test-designs FASM (T7.2)

FASM produced by building fpgas.online-test-designs' `spi-flash-id` design for
the `acorn` board with LiteX + the openXC7 flow (T7.1/T7.2). `top.fasm.xz`
is real, working FASM (not hand-written).

Regenerate with:

```
tools/e2e/setup-openxc7.sh --parts xc7a200tfbg484-3   # from /home/user/fasm (main tree); once per part
tools/e2e/setup-litex.sh                    # once per machine
tools/e2e/run-fpgas-online.sh spi-flash-id acorn
tools/e2e/install-fpgas-online-corpus.sh spi-flash-id acorn
```

## Target

* Part: `xc7a200tfbg484-3`
* Design: fpgas.online-test-designs `designs/spi-flash-id/` (see its own
  `README.md` in that repository for what the test verifies)
* Board: `acorn`
* Gateware script: `designs/spi-flash-id/gateware/spiflash_soc_acorn.py`

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
# commands are in the generated designs/spi-flash-id/build/acorn/build_top.sh
# or */gateware/build_*.sh, which is not committed):
CHIPDB=<chipdb overlay dir> PRJXRAY_DB_DIR=<prjxray-db root> \
  tools/e2e/build/litex-venv/bin/python \
  tools/e2e/build/fpgas.online-test-designs/designs/spi-flash-id/gateware/spiflash_soc_acorn.py \
  --toolchain openxc7 --build --variant cle-215+ --no-compile-software

# Reference frames + bitstream (from the FASM above):
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a200tfbg484-3 top.fasm top.frm
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a200tfbg484-3 --sparse top.fasm top.sparse.frm
tests/oracle/xc7frames2bit-oracle -frm_file top.frm -output_file top.bit \
  -part_name xc7a200tfbg484-3 -part_file <prjxray-db>/artix7/xc7a200tfbg484-3/part.yaml
```

## Output checksums (this run; `.bit` is NOT committed)

```
sha256  top.fasm (uncompressed)   46ab9f8b08ce8e147b56e3270ceb238be4999ba7e6a94b2c6dd63deeb96c01cc
sha256  top.frm (uncompressed)    5f2a0ab0ad6aa2864a2dfd36653a23c08f60113e5f3a461ccef5ce3769324f75
sha256  top.bit (not committed)   f8abc918eab8f2b7a0fa98683ff67a3f2f77f8db462a2fe6b71a33eeb8bbad44
```

top.fasm: 56825 lines / 2073028 bytes uncompressed, 214400 bytes as top.fasm.xz
top.frm: 22698060 bytes uncompressed, 84444 bytes as top.frm.xz
top.sparse.frm: 62912 bytes as top.sparse.frm.xz
top.bit: 9730861 bytes (not committed; regenerate to verify)

## Timings (this machine, this run)

* LiteX build (synth + PnR + FASM): 71s
* fasm2frames (dense): 18s
* fasm2frames (sparse): 17s
* xc7frames2bit: 0s

Generated 2026-09-24.

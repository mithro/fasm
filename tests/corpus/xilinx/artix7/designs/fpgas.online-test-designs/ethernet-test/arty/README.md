# ethernet-test / arty -- fpgas.online-test-designs FASM (T7.2)

FASM produced by building fpgas.online-test-designs' `ethernet-test` design for
the `arty` board with LiteX + the openXC7 flow (T7.1/T7.2). `top.fasm.xz`
is real, working FASM (not hand-written).

Regenerate with:

```
tools/e2e/setup-openxc7.sh --parts xc7a35tcsg324-1   # from /home/user/fasm (main tree); once per part
tools/e2e/setup-litex.sh                    # once per machine
tools/e2e/run-fpgas-online.sh ethernet-test arty
tools/e2e/install-fpgas-online-corpus.sh ethernet-test arty
```

## Target

* Part: `xc7a35tcsg324-1`
* Design: fpgas.online-test-designs `designs/ethernet-test/` (see its own
  `README.md` in that repository for what the test verifies)
* Board: `arty`
* Gateware script: `designs/ethernet-test/gateware/ethernet_soc_arty.py`

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
# commands are in the generated designs/ethernet-test/build/arty/build_top.sh
# or */gateware/build_*.sh, which is not committed):
CHIPDB=<chipdb overlay dir> PRJXRAY_DB_DIR=<prjxray-db root> \
  tools/e2e/build/litex-venv/bin/python \
  tools/e2e/build/fpgas.online-test-designs/designs/ethernet-test/gateware/ethernet_soc_arty.py \
  --toolchain openxc7 --build --no-compile-software

# Reference frames + bitstream (from the FASM above):
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tcsg324-1 top.fasm top.frm
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tcsg324-1 --sparse top.fasm top.sparse.frm
tests/oracle/xc7frames2bit-oracle -frm_file top.frm -output_file top.bit \
  -part_name xc7a35tcsg324-1 -part_file <prjxray-db>/artix7/xc7a35tcsg324-1/part.yaml
```

## Output checksums (this run; `.bit` is NOT committed)

```
sha256  top.fasm (uncompressed)   f320b4b8435ff4a69694a7230d76086870d4ec8a09f433992d6224008eb887dc
sha256  top.frm (uncompressed)    3292fe363489080da09ffc035017c34bcc690abb9d3aa618ff7a62d5b2017ba4
sha256  top.bit (not committed)   4f2f57f71023544301aeee038d699652e49a1f4a362731c94a70487f6284be9d
```

top.fasm: 252344 lines / 9584626 bytes uncompressed, 941436 bytes as top.fasm.xz
top.frm: 5580828 bytes uncompressed, 273100 bytes as top.frm.xz
top.sparse.frm: 270828 bytes as top.sparse.frm.xz
top.bit: 2192220 bytes (not committed; regenerate to verify)

## Timings (this machine, this run)

* LiteX build (synth + PnR + FASM): 205s
* fasm2frames (dense): 9s
* fasm2frames (sparse): 10s
* xc7frames2bit: 0s

Generated 2026-09-24.

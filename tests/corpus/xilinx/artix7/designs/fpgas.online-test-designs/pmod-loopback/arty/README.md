# pmod-loopback / arty -- fpgas.online-test-designs FASM (T7.2)

FASM produced by building fpgas.online-test-designs' `pmod-loopback` design for
the `arty` board with LiteX + the openXC7 flow (T7.1/T7.2). `top.fasm`
is real, working FASM (not hand-written).

Regenerate with:

```
tools/e2e/setup-openxc7.sh --parts xc7a35tcsg324-1   # from /home/user/fasm (main tree); once per part
tools/e2e/setup-litex.sh                    # once per machine
tools/e2e/run-fpgas-online.sh pmod-loopback arty
tools/e2e/install-fpgas-online-corpus.sh pmod-loopback arty
```

## Target

* Part: `xc7a35tcsg324-1`
* Design: fpgas.online-test-designs `designs/pmod-loopback/` (see its own
  `README.md` in that repository for what the test verifies)
* Board: `arty`
* Gateware script: `designs/pmod-loopback/gateware/gpio_loopback_arty.py`

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
# commands are in the generated designs/pmod-loopback/build/arty/build_top.sh
# or */gateware/build_*.sh, which is not committed):
CHIPDB=<chipdb overlay dir> PRJXRAY_DB_DIR=<prjxray-db root> \
  tools/e2e/build/litex-venv/bin/python \
  tools/e2e/build/fpgas.online-test-designs/designs/pmod-loopback/gateware/gpio_loopback_arty.py \
  --toolchain openxc7 --build

# Reference frames + bitstream (from the FASM above):
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tcsg324-1 top.fasm top.frm
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tcsg324-1 --sparse top.fasm top.sparse.frm
tests/oracle/xc7frames2bit-oracle -frm_file top.frm -output_file top.bit \
  -part_name xc7a35tcsg324-1 -part_file <prjxray-db>/artix7/xc7a35tcsg324-1/part.yaml
```

## Output checksums (this run; `.bit` is NOT committed)

```
sha256  top.fasm (uncompressed)   53cbf03359d1aeadc28432845f4b0347fba23ac85f6d2ddf58a46cbdc8736161
sha256  top.frm (uncompressed)    1a4c9baab30b5d0953180832e945ed33369a18d13328c81b546bc507c3239f35
sha256  top.bit (not committed)   c0add5518be729bf70e74c16e363f2fadc9882b56def6b4c5a5611edbc9e5b61
```

top.fasm: 294 lines / 11295 bytes uncompressed
top.frm: 5580828 bytes uncompressed, 7792 bytes as top.frm.xz
top.sparse.frm: 1284 bytes as top.sparse.frm.xz
top.bit: 2192220 bytes (not committed; regenerate to verify)

## Timings (this machine, this run)

* LiteX build (synth + PnR + FASM): 6s
* fasm2frames (dense): 0s
* fasm2frames (sparse): 0s
* xc7frames2bit: 0s

Generated 2026-09-24.

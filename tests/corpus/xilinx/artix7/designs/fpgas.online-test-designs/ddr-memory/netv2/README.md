# ddr-memory / netv2 -- fpgas.online-test-designs FASM (T7.2)

FASM produced by building fpgas.online-test-designs' `ddr-memory` design for
the `netv2` board with LiteX + the openXC7 flow (T7.1/T7.2). `top.fasm.xz`
is real, working FASM (not hand-written).

Regenerate with:

```
tools/e2e/setup-openxc7.sh --parts xc7a35tfgg484-2   # from /home/user/fasm (main tree); once per part
tools/e2e/setup-litex.sh                    # once per machine
tools/e2e/run-fpgas-online.sh ddr-memory netv2
tools/e2e/install-fpgas-online-corpus.sh ddr-memory netv2
```

## Target

* Part: `xc7a35tfgg484-2`
* Design: fpgas.online-test-designs `designs/ddr-memory/` (see its own
  `README.md` in that repository for what the test verifies)
* Board: `netv2`
* Gateware script: `designs/ddr-memory/gateware/ddr_soc_netv2.py`

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
# commands are in the generated designs/ddr-memory/build/netv2/build_top.sh
# or */gateware/build_*.sh, which is not committed):
CHIPDB=<chipdb overlay dir> PRJXRAY_DB_DIR=<prjxray-db root> \
  tools/e2e/build/litex-venv/bin/python \
  tools/e2e/build/fpgas.online-test-designs/designs/ddr-memory/gateware/ddr_soc_netv2.py \
  --toolchain openxc7 --build --variant a7-35 --no-compile-software

# Reference frames + bitstream (from the FASM above):
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tfgg484-2 top.fasm top.frm
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tfgg484-2 --sparse top.fasm top.sparse.frm
tests/oracle/xc7frames2bit-oracle -frm_file top.frm -output_file top.bit \
  -part_name xc7a35tfgg484-2 -part_file <prjxray-db>/artix7/xc7a35tfgg484-2/part.yaml
```

## Output checksums (this run; `.bit` is NOT committed)

```
sha256  top.fasm (uncompressed)   fb8606c15eb5fba7c2da49a95fa1458b5ac68bf0265fea8fb79265e1da410e4e
sha256  top.frm (uncompressed)    47a5ced642cfa31791d6c1233f571af3ad18ae91144bf72240bf2751e458d568
sha256  top.bit (not committed)   b2259bf200c18713d34d17e94783ce8c702e913004c70e709585ad7c56a0493d
```

top.fasm: 273281 lines / 10499772 bytes uncompressed, 1025176 bytes as top.fasm.xz
top.frm: 5580828 bytes uncompressed, 290104 bytes as top.frm.xz
top.sparse.frm: 287556 bytes as top.sparse.frm.xz
top.bit: 2192218 bytes (not committed; regenerate to verify)

## Timings (this machine, this run)

* LiteX build (synth + PnR + FASM): 205s
* fasm2frames (dense): 12s
* fasm2frames (sparse): 10s
* xc7frames2bit: 1s

Generated 2026-09-24.

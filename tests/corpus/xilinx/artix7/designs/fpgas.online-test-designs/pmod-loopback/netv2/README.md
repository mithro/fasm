# pmod-loopback / netv2 -- fpgas.online-test-designs FASM (T7.2)

FASM produced by building fpgas.online-test-designs' `pmod-loopback` design for
the `netv2` board with LiteX + the openXC7 flow (T7.1/T7.2). `top.fasm`
is real, working FASM (not hand-written).

Regenerate with:

```
tools/e2e/setup-openxc7.sh --parts xc7a35tfgg484-2   # from /home/user/fasm (main tree); once per part
tools/e2e/setup-litex.sh                    # once per machine
tools/e2e/run-fpgas-online.sh pmod-loopback netv2
tools/e2e/install-fpgas-online-corpus.sh pmod-loopback netv2
```

## Target

* Part: `xc7a35tfgg484-2`
* Design: fpgas.online-test-designs `designs/pmod-loopback/` (see its own
  `README.md` in that repository for what the test verifies)
* Board: `netv2`
* Gateware script: `designs/pmod-loopback/gateware/gpio_loopback_netv2.py`

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
# commands are in the generated designs/pmod-loopback/build/netv2/build_top.sh
# or */gateware/build_*.sh, which is not committed):
CHIPDB=<chipdb overlay dir> PRJXRAY_DB_DIR=<prjxray-db root> \
  tools/e2e/build/litex-venv/bin/python \
  tools/e2e/build/fpgas.online-test-designs/designs/pmod-loopback/gateware/gpio_loopback_netv2.py \
  --toolchain openxc7 --build --variant a7-35

# Reference frames + bitstream (from the FASM above):
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tfgg484-2 top.fasm top.frm
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tfgg484-2 --sparse top.fasm top.sparse.frm
tests/oracle/xc7frames2bit-oracle -frm_file top.frm -output_file top.bit \
  -part_name xc7a35tfgg484-2 -part_file <prjxray-db>/artix7/xc7a35tfgg484-2/part.yaml
```

## Output checksums (this run; `.bit` is NOT committed)

```
sha256  top.fasm (uncompressed)   fee985d3975cbd9c1f8e66868f7b23a400714a0881fc640c891816b681ef9da0
sha256  top.frm (uncompressed)    6f4a99f94308f21b90d79f11ae07cf8243c1e9751422a0c04b7d08a9b0443d04
sha256  top.bit (not committed)   47f76855c7ffb37215a839abe20f62063f64eb013e90bd7923a303524f0cda82
```

top.fasm: 37 lines / 1485 bytes uncompressed
top.frm: 5580828 bytes uncompressed, 6912 bytes as top.frm.xz
top.sparse.frm: 556 bytes as top.sparse.frm.xz
top.bit: 2192221 bytes (not committed; regenerate to verify)

## Timings (this machine, this run)

* LiteX build (synth + PnR + FASM): 5s
* fasm2frames (dense): 0s
* fasm2frames (sparse): 1s
* xc7frames2bit: 0s

Generated 2026-09-24.

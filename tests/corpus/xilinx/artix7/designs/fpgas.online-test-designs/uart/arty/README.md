# uart / arty -- fpgas.online-test-designs FASM (T7.2)

FASM produced by building fpgas.online-test-designs' `uart` design for
the `arty` board with LiteX + the openXC7 flow (T7.1/T7.2). `top.fasm.xz`
is real, working FASM (not hand-written).

Regenerate with:

```
tools/e2e/setup-openxc7.sh --parts xc7a35tcsg324-1   # from /home/user/fasm (main tree); once per part
tools/e2e/setup-litex.sh                    # once per machine
tools/e2e/run-fpgas-online.sh uart arty
tools/e2e/install-fpgas-online-corpus.sh uart arty
```

## Target

* Part: `xc7a35tcsg324-1`
* Design: fpgas.online-test-designs `designs/uart/` (see its own
  `README.md` in that repository for what the test verifies)
* Board: `arty`
* Gateware script: `designs/uart/gateware/uart_soc_arty.py`

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
  the same tools -- but run against the **openXC7 snap's own bundled
  prjxray-db** (`tools/e2e/build/openxc7/root/opt/nextpnr-xilinx/external/prjxray-db`), the same database
  nextpnr-xilinx's chipdb and the whole LiteX openxc7 flow are built
  against for this design, **not** the independently pinned
  `f4pga/prjxray-db` that `tests/oracle/setup-xilinx.sh`'s own
  `tools/fetch-db.sh` fetches for the rest of this repo's Xilinx
  differential tests. openXC7 snap `0.8.2`
  (sha256 `6b2e07ce99ef33d3a4e41e2fd2eb916f26bb0ece34a97216ed840a0032e98587`); its bundled prjxray-db's own
  `Info.md` records "Created using Project X-Ray version
  [4c157493](https://github.com/SymbiFlow/prjxray/commit/4c157493ec9f13caea4ad3f0c02f8f318f198846),
  last updated Tue Dec 14 07:31:38 PM UTC 2021" (an earlier draft of
  tools/e2e/README.md claimed this database "carries no version marker
  the way `tools/fetch-db.sh` pins prjxray-db for the oracle" --
  corrected after T7.2 review: `Info.md` does record one, it is simply a
  different, independent pin from `tools/fetch-db.sh`'s). See
  tools/e2e/README.md ("A note on prjxray-db provenance") for the exact,
  verified tile/segbits/ppips differences against the pinned db

## Commands (as run by tools/e2e/run-fpgas-online.sh)

```
# LiteX build (yosys synth_xilinx -> nextpnr-xilinx -> FASM; the exact
# commands are in the generated designs/uart/build/arty/build_top.sh
# or */gateware/build_*.sh, which is not committed):
CHIPDB=tools/e2e/build/chipdb-overlay PRJXRAY_DB_DIR=tools/e2e/build/openxc7/root/opt/nextpnr-xilinx/external/prjxray-db \
  tools/e2e/build/litex-venv/bin/python \
  tools/e2e/build/fpgas.online-test-designs/designs/uart/gateware/uart_soc_arty.py \
  --toolchain openxc7 --build --no-compile-software

# Reference frames + bitstream (from the FASM above; --db-root is the
# snap's bundled prjxray-db -- see "Source provenance" above):
tests/oracle/fasm2frames-oracle --db-root tools/e2e/build/openxc7/root/opt/nextpnr-xilinx/external/prjxray-db/artix7 --part xc7a35tcsg324-1 top.fasm top.frm
tests/oracle/fasm2frames-oracle --db-root tools/e2e/build/openxc7/root/opt/nextpnr-xilinx/external/prjxray-db/artix7 --part xc7a35tcsg324-1 --sparse top.fasm top.sparse.frm
tests/oracle/xc7frames2bit-oracle -frm_file top.frm -output_file top.bit \
  -part_name xc7a35tcsg324-1 -part_file tools/e2e/build/openxc7/root/opt/nextpnr-xilinx/external/prjxray-db/artix7/xc7a35tcsg324-1/part.yaml
```

## Output checksums (this run; `.bit` is NOT committed)

```
sha256  top.fasm (uncompressed)   18393a4d39568cddabaf54b33d50864d08b80d166b674d0f3079670ef675186a
sha256  top.frm (uncompressed)    6d9faffb9ca04eaa806a87799e6f6b246bb2461c58c775252efb60da5914c5b4
sha256  top.bit (not committed)   0b5eccc7f31aa58a68c0615a86416bcec8e851e09c4948704c97516ad6bfd681
```

top.fasm: 85331 lines / 3385681 bytes uncompressed, 319792 bytes as top.fasm.xz
top.frm: 5580828 bytes uncompressed, 92544 bytes as top.frm.xz
top.sparse.frm: 88948 bytes as top.sparse.frm.xz
top.bit: 2192211 bytes (not committed; regenerate to verify)

## Timings (this machine, this run)

* LiteX build (synth + PnR + FASM): 62s
* fasm2frames (dense): 3s
* fasm2frames (sparse): 2s
* xc7frames2bit: 1s

Generated 2026-09-24.

SPDX-License-Identifier: Apache-2.0 (fpgas.online-test-designs sources,
this README) -- the openXC7 snap's bundled prjxray-db used to regenerate
`.frm`/`.bit` above is CC0-1.0 (see its own `README.md`/`COPYING`).

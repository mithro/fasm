# rapidwright/ring60-s2/xc7a35tcsg324-1 -- RapidWright + FPGA interchange FASM (T7.5)

`rw.fasm` is FASM for a design placed and routed by RapidWright alone (no
Vivado): `tools/e2e/rapidwright/RwDesign.java` (a ring of 60 LUT6 +
flip-flop stages on random slices, seed 2; routed by RapidWright's
`router.Router`, since RWRoute refuses 7 series parts), written as an FPGA
interchange logical and physical netlist and turned into FASM by
python-fpga-interchange's xc7 FASM generator with RapidWright's device
resources for the part (patched with python-fpga-interchange's Series7
constraints and LUT definitions). `rw.sparse.frm.xz` is the reference
`fasm2frames --sparse` (f4pga-xc-fasm, prjxray-db) of it: the Rust
`fasm2frames` gives the same bytes (`tests/e2e/test_rapidwright.py`).

There is no independent reference bitstream for this design (that would
need Vivado); what it adds is FASM from a third producer (RapidWright's
placement and routing, python-fpga-interchange's feature emission) that
both assemblers must agree on.

* Part: `xc7a35tcsg324-1` (family `artix7`)
* FASM: 5521 lines, sha256 `b6f71bdc9ec181771388487c12670a06599061afa05518c190878857c902b90a`
* Sparse frames: 3040, `.frm` sha256 `cbb0b9692c3fb731b69c078ad0f2b46c39bd2e7a96f5cee527ff14e55234622d`
* RapidWright: RwDesign: 60 stages, 5281 PIPs, 0 inter-site nets without PIPs; `FAILED TO ROUTE` lines of `Router`:
  0

## Tools

* RapidWright `v2026.1.0-beta` (`tools/e2e/setup-rapidwright.sh`)
* python-fpga-interchange `04a02101d1f7f03a2d33716192fb478e1e8605af` with pycapnp 1.3.0
  (`setup-rapidwright.sh --with-interchange`, `rapidwright/pfi_run.py`)
* Database: the pinned prjxray-db of `tools/fetch-db.sh`

## Commands

```
python3 tools/e2e/rapidwright/rwcheck.py fasm --install
```

which runs

```
java RwDesign xc7a35tcsg324-1 <prefix> 60 2
java com.xilinx.rapidwright.interchange.DeviceResourcesExample xc7a35tcsg324-1
pfi_run.py patch --patch_path constraints --patch_format pyyaml ...
pfi_run.py patch --patch_path lutDefinitions --patch_format pyyaml ...
pfi_run.py fasm_generator --family xc7 <device> \
    <prefix>.netlist <prefix>.phys rw.fasm
fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tcsg324-1 \
    --sparse rw.fasm rw.sparse.frm
```

## Licence

Generated from this repository's own driver (Apache-2.0); no third party
design sources.

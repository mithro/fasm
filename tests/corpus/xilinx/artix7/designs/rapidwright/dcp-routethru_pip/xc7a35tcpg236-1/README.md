# rapidwright/dcp-routethru_pip/xc7a35tcpg236-1 -- a Vivado DCP through RapidWright (T7.5)

`rw.fasm` is FASM for `routethru_pip.dcp` of RapidWright's test data
(Xilinx/RapidWrightDCP `f9625fc62d290926668c4955c3a76e9d2044e916`, sha256
`4f534ec63a7cf068906f5143d73d97eb9dd826fb92eaacc3a2ecbf901b5f9cce`; placed and routed by Vivado, with a readable EDIF), converted
without Vivado: RapidWright's `DcpToInterchange` (interchange logical and
physical netlists), then python-fpga-interchange's xc7 FASM generator with
RapidWright's device resources for the part (patched with
python-fpga-interchange's Series7 constraints and LUT definitions;
`pfi_run.py` supplies the pseudo PIP sites RapidWright does not write).
`rw.sparse.frm.xz` is the reference `fasm2frames --sparse` (f4pga-xc-fasm,
prjxray-db) of it: the Rust `fasm2frames` gives the same bytes
(`tests/e2e/test_rapidwright.py`). The DCP has no bitstream, and without
Vivado none can be made to compare with.

* Part: `xc7a35tcpg236-1` (family `artix7`)
* FASM: 136 lines, sha256 `cfbbc175d1e9ca27db817c9f49263c54c117bb0eefd9269f6124935e9d42d004`
* Sparse frames: 106, `.frm` sha256 `09731db011fcf50e4d970fa40baca6596822026020d9b24e6b75ad55ccc1b8d7`

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
java com.xilinx.rapidwright.interchange.DcpToInterchange routethru_pip.dcp
pfi_run.py fasm_generator --family xc7 <device> \\
    routethru_pip.netlist routethru_pip.phys rw.fasm
fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tcpg236-1 \\
    --sparse rw.fasm rw.sparse.frm
```

## Licence

RapidWrightDCP is Apache-2.0 (RapidWright's licence); the FASM is
derived from its DCP.

# rapidwright/dcp-routethru_luts/xc7a35tcpg236-1 -- a Vivado DCP through RapidWright (T7.5)

`rw.fasm` is FASM for `routethru_luts.dcp` of RapidWright's test data
(Xilinx/RapidWrightDCP `f9625fc62d290926668c4955c3a76e9d2044e916`, sha256
`e449fc87233d89474457513189f9ed61310f1d26555f398ffe246ffbbb128905`; placed and routed by Vivado, with a readable EDIF), converted
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
* FASM: 47 lines, sha256 `aa9c00a9b57eb02dd939c514862f6fd8cdb2e5e13f98b2750d370e38723ebfa1`
* Sparse frames: 106, `.frm` sha256 `b76e5abe10c30393aea8c193d7c59218d8e2dcec939628bd54349910edd801b0`

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
java com.xilinx.rapidwright.interchange.DcpToInterchange routethru_luts.dcp
pfi_run.py fasm_generator --family xc7 <device> \\
    routethru_luts.netlist routethru_luts.phys rw.fasm
fasm2frames-oracle --db-root <prjxray-db>/artix7 --part xc7a35tcpg236-1 \\
    --sparse rw.fasm rw.sparse.frm
```

## Licence

RapidWrightDCP is Apache-2.0 (RapidWright's licence); the FASM is
derived from its DCP.

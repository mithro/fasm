# RapidWright as a reference for the Rust Xilinx tools (T7.5)

What RapidWright can contribute as an independent reference for
`fasm2frames`, the bitstream writer and the bitstream reader, established
by running it on this machine (no Vivado), and what was done with it. The
results matrix is in `DESIGN-xilinx-db.md` §8.15; usage in
`tools/e2e/README.md`, "RapidWright cross-checks (T7.5)".

## 1. What was used

| Component | Pin | How |
|---|---|---|
| RapidWright | `v2026.1.0-beta` (tag commit `127f55cd704c277372697e699f1559e1cdc91f34`, released 2026-06-30), `rapidwright-2026.1.0-standalone-lin64.jar` from the GitHub release, sha256 `18f81833595ef8a5191a72f9431727e17602de2fe4dddbba2d281358801a1fc9`, 115 MB (contains the closed source `rapidwright-api-lib` 2026.1.0) | `tools/e2e/setup-rapidwright.sh` |
| Device files | `http://data.rapidwright.io/<container>/<md5>` as the tag's `DataVersions.java` lists them (RapidWright's own download scheme, `FileTools.downloadDataFile`); `parts.db` and xc7a35t, xc7a50t, xc7a100t, xc7a200t, xc7k70t, xc7s50, xc7z010, xc7z020, xcku035, xczu3eg: 2-5.6 MB each, 30 MB in total; with the `.md5` file next to each, RapidWright downloads nothing itself | same |
| Java | OpenJDK 21.0.10, `-Xmx4g` | |
| python-fpga-interchange | `04a02101d1f7f03a2d33716192fb478e1e8605af` (its last commit, 0.0.18), pycapnp 1.3.0, python-sat, PyYAML | `setup-rapidwright.sh --with-interchange` |
| FPGA interchange schema | `c985b4648e66414b250261c1ba4cbe45a2971b1c` (RapidWright's submodule at the tag) plus capnproto-java's `java.capnp` v0.1.16 (imported by `References.capnp`) | same |
| RapidWrightDCP | `f9625fc62d290926668c4955c3a76e9d2044e916` (RapidWright's `test/RapidWrightDCP` submodule at the tag): its nine DCPs for parts we have databases for | same |

The drivers are small and committed: `tools/e2e/rapidwright/RwCheck.java`
(the bitstream API), `RwDesign.java` (a design placed and routed by
RapidWright), `pfi_run.py` (runs python-fpga-interchange with two
adaptations, §4), `rwcheck.py` (the comparisons), and
`tools/e2e/run-rapidwright-checks.sh`.

## 2. (a) Can RapidWright emit FASM? No, not by itself

* The jar has no class whose name contains `fasm` (`unzip -l
  rapidwright-2026.1.0-standalone-lin64.jar | grep -ci fasm` is 0), and
  the open source tree has no mention of FASM (`grep -rli fasm` over the
  tag's checkout: nothing). RapidWright's outputs are DCPs (for Vivado),
  EDIF and the FPGA interchange format
  (`com.xilinx.rapidwright.interchange`: `PhysNetlistWriter`,
  `LogNetlistWriter`, `DeviceResourcesWriter`, `DcpToInterchange`).
* **The interchange route works without Vivado** for Series7:
  1. a placed and routed `Design` in RapidWright: either built by
     RapidWright itself, or read from a DCP whose EDIF is readable;
  2. `LogNetlistWriter` / `PhysNetlistWriter` (or `DcpToInterchange
     design.dcp`) write `<name>.netlist` and `<name>.phys`;
  3. `java com.xilinx.rapidwright.interchange.DeviceResourcesExample
     xc7a35tcsg324-1` writes the device resources (`.device`, 32 MB for
     xc7a35t, 120 MB for xc7a200t; about 35 s and 2.2 GB of heap for
     xc7a35t); `python -m fpga_interchange.patch --patch_path constraints
     --patch_format pyyaml ... test_data/series7_constraints.yaml` and the
     same with `lutDefinitions` / `series7_luts.yaml` add what the
     generator needs (without the LUT definitions it fails with
     `AssertionError: SLICEM` in `luts.py:find_lut_bel`);
  4. `python -m fpga_interchange.fasm_generator --schema_dir ... --family
     xc7 <device> <name>.netlist <name>.phys out.fasm` writes FASM with
     prjxray-db feature names; its `xc7` generator knows the prjxray-db
     conventions itself (no database needed at this step).
* Obstacles found and how they were handled:
  * python-fpga-interchange pins `pycapnp==1.1.0`, which no longer builds
    (`Error compiling Cython file` with Cython 3). With pycapnp 1.3.0,
    `from_bytes` returns a context manager: `AttributeError:
    '_GeneratorContextManager' object has no attribute 'strList'`.
    `pfi_run.py` enters it. (pycapnp 2.x has other API changes.)
  * `References.capnp` imports `/capnp/java.capnp`: `Import failed:
    /capnp/java.capnp` until `CAPNP_PATH` points at a copy.
  * The `yaml` patch format needs a rapidyaml fork that is not on PyPI
    (`No module named 'ryml'`); the `pyyaml` format works.
  * RapidWright's `PhysNetlistWriter` writes every PIP with `noSite`, but
    the xc7 generator needs the site of a pseudo PIP (a route-thru):
    `AssertionError` at `generic.py:242` (`assert site`) for every Vivado
    DCP with a LUT or ILOGIC route-thru. `pfi_run.py` derives it from the
    device: the one site of the PIP's tile wired to both PIP wires.
  * **RWRoute refuses Series7**: `ERROR: RWRoute does not support routing
    the xc7a35tcsg324-1 from the Series7 series. Please re-target the
    design to a part from a supported series: [UltraScale,
    UltraScalePlus, Versal]`. RapidWright's older `router.Router` routes
    Series7 (used by `RwDesign.java`; it does not route clocks, so the
    ring design leaves clock, CE and reset unconnected).
  * The generator needs `INIT` on every flip-flop cell (`KeyError:
    'INIT'` otherwise): `RwDesign.java` sets it.
* Designs that went through (all in `tests/corpus/xilinx/*/designs/
  rapidwright/`, each with its reference sparse frames):
  * three RapidWright-built rings of 60 LUT6 + flip-flop stages
    (xc7a35tcsg324-1 seeds 1 and 2, xc7z010clg400-1 seed 1; 4969-5521
    FASM lines, 2014-3324 sparse frames);
  * two Vivado DCPs of RapidWrightDCP, `routethru_luts` and
    `routethru_pip` (xc7a35tcpg236-1; 47 and 136 lines).
* Designs that did not (`rwcheck.py fasm` reports them as "not
  possible", with the error):
  * the prjxray-db harness DCPs (`artix7/harness/*/*/design.dcp`, Vivado
    2017.2), the only DCPs we have **with** a matching `design.bit`:
    their `top.edf` is encrypted (`XlxV37EB...`), and RapidWright stops
    with `ERROR: Unable to find a readable EDIF file for the DCP` (with
    `RW_AUTO_GENERATE_READABLE_EDIF` unset it tries to run Vivado:
    `Couldn't find vivado on PATH`). Vivado's `write_edif` is needed;
  * RapidWrightDCP `ramb18` (`TypeError: 'NoneType' object is not
    subscriptable` in the generator's `handle_brams`: its RAMB18 has no
    `RAM_MODE` property as the generator expects), `bug349` (`ValueError:
    invalid literal for int() with base 16: "64'h00000000ffff0000"`: a
    LUT INIT in Verilog syntax the generator cannot decode), `bug709`
    (`AssertionError: .../ram_reg_0_15_6_6/SP`: LUTRAM macros are not
    supported), `verilog_ethernet` (`AssertionError: ('VCC',
    dict_keys([...]))`: the generator looks for a cell instance `VCC` in
    the logical netlist, which Vivado's netlist does not have);
  * RapidWrightDCP `bug226` (xc7a35t) and `bug635` (xc7a200t) produce
    FASM (85400 and 710 lines) that **both** `fasm2frames` reject with
    the same `FasmLookupError` lines: the generator emits RAMB18 features
    prjxray-db does not have (`BRAM_L.RAMB18_Y0.ZINV_REGCLKARDRCLK_B`,
    `BRAM_R.RAMB18_Y0.WRITE_MODE_A_WRITE_FIRST`, `CLK_BUFG_REBUF...
    GCLK1_0_UP_TEST_RING_OUT`). Identical rejections are still a check
    of the Rust error path;
  * `bug701` (xczu3eg): python-fpga-interchange has FASM generators for
    `xc7` and `nexus` only, so there is **no FASM route for UltraScale+**.
* What is compared: the Rust `fasm2frames` against the reference
  (f4pga-xc-fasm) dense and sparse, byte for byte, and the FASM
  regenerated byte for byte against the committed file (RapidWright and
  the generator are deterministic). What is **not** possible: comparing
  against an original bitstream. RapidWright cannot write a bitstream
  for a design (only frames the caller supplies, §3), the RapidWrightDCP
  designs come without bitstreams, and the only DCPs with bitstreams
  (prjxray-db harness) cannot be read without Vivado. With Vivado, the
  harness DCPs (`write_edif`) would give FASM whose frames could be
  compared with their `design.bit`.

## 3. (b) Can RapidWright read or write bitstreams? Yes, all three architectures

`com.xilinx.rapidwright.bitstream` (in the closed source api-lib since
2020.2.1, `javap` of the 2026.1.0 jar) is public: `Bitstream`
(`readBitstream(Path)`, `writeBitstream(Path)`, `new Bitstream(design,
part)`, `getHeader()`, `getPackets()`, `configureArray()`,
`updatePacketsFromConfigArray()`, `checkIfDeviceSupported(part)`),
`ConfigArray` (`new ConfigArray(Device)`, `getConfigRows()`,
`getFrame(FAR)`, `getWordSize()`, `FRAME_OVERHEAD_COUNT_PER_ROW`),
`ConfigRow`, `Block` (`getAddress()`, `getFrameCount()`, `getSubType()`,
`getTileColumn()`), `FAR` (`setFAR`, `incrementFAR`, field accessors),
`Frame` (`getWords`, `setWords`, `updateECCBits`,
`getWordsPerFrame(Series)`), `Packet`, `BitstreamHeader`, `IDCode`
(`getIDCode(Device)`), `CRC`. `checkIfDeviceSupported` accepts every part
we have (Series7, UltraScale, UltraScale+).

* **Frame layout**: `ConfigArray` lists, per configuration row, the
  blocks (columns) with their frame address, frame count, tile type and
  tile column; `FAR.incrementFAR()` from 0 walks every frame address (it
  returns -1 after the last). Words per frame 101 / 123 / 93, two
  overhead (pad) frames per row, `getWordSize()` = the FDRI payload of a
  full bitstream.
* **Reading**: `Bitstream.readBitstream(path)` parses the header and the
  packets; `configureArray()` fills the frames (`getConfigArray()` alone
  returns the array with all frames zero). The part comes from the
  header's part name (field `b`; a Vivado name like `7a50tfgg484` works):
  a bitstream with an empty part name fails with
  `ArrayIndexOutOfBoundsException: Index -1 out of bounds for length 0`
  (so `rwcheck.py` gives the Rust writer `--part_name`).
* **Writing**: `new Bitstream(name, part)`, `configureArray()`, set the
  frames, `updatePacketsFromConfigArray()`, `writeBitstream()`: a Vivado
  style bitstream (Vivado's packet sequence with CRC) of those frames.
  RapidWright cannot turn a design into frames (no bit database): it only
  writes the frames it is given or read.
* Behaviours found (verified exactly by `rwcheck.py`, none a Rust
  difference):
  1. **Per frame CRC bitstreams** (Vivado `-g PerFrameCRC`, i.e. a `FAR`
     write and a `CRC` write around every 1-frame `FDRI` write, with
     `CTL1` bit 21 set; all ToolsTestData Vivado bitstreams and prjxray's
     `configuration_test.perframecrc.bit`): RapidWright reads the frames
     of every row one frame address early (the frame at walk position i
     holds what the prjxray reader, and so the Rust port, has at i + 1;
     the last frame of a row holds the first pad frame, zero). The prjxray
     reader is right here. In these bitstreams the `FAR` written before
     each 1-frame `FDRI` holds the address of the **previous** frame (a
     progress marker: `FAR=0, FDRI, FAR=0, FDRI, FAR=1, FDRI, ...` in
     `configuration_test.perframecrc.bit`; in `configuration_test.debug.bit`
     the `LOUT` write *after* each frame carries that frame's address).
     With `CTL1` bit 21 set the prjxray reader does not restart the write
     on these `FAR` writes and keeps counting from the first `FAR`;
     RapidWright applies each `FAR` to the frame that follows it, hence
     one address early. Evidence: the same design without per frame CRC
     (`configuration_test.bit`) reads identically with both tools and
     equals prjxray's reading of the per frame CRC variant.
  2. The other side of the same bitstreams: the prjxray reader (and the
     Rust reader, a literal port) loses the **last frame of the part**:
     after it, `GetNextFrameAddress` has no next address, so the trailing
     pad frame (the next 1-frame `FDRI` write) is stored over it. In
     ToolsTestData `Series7/bram.bit` that frame (0x00C0017F) is not zero
     and RapidWright has its contents (at 0x00C0017E, item 1). The
     reference `bitread` drops it too (`bitread -o`), so the Rust reader
     keeps the behaviour (COMPAT.md, "Bitstream readers compared with
     RapidWright"); `tests/e2e/test_rapidwright.py` pins it.
  3. `BitstreamHeader` splits field `a` at the first `;` into the design
     name and the options (which keep the `;`).
  4. For a bitstream without per frame CRC, RapidWright's writer gives
     back the original's packet list exactly (Vivado and xc7frames2bit
     bitstreams alike use one `FDRI` payload; RapidWright writes the
     Vivado sequence: equal to Vivado's, not to xc7frames2bit's), and
     the `FDRI` payload of the Rust writer and of RapidWright's writer is
     the same for the same frames (including the pad frames and the ECC).

## 4. (c) Example designs

* RapidWright ships no designs in the jar. Its test data,
  Xilinx/RapidWrightDCP (55 files, about 150 MB), has 38 DCPs; for parts
  we have databases for: xc7a35tcpg236-1 (`routethru_luts`,
  `routethru_pip`, `ramb18`, `bug226`, `bug349`), xc7a200tsbg484-1
  (`bug635`, `bug709`, `verilog_ethernet`), xczu3eg-sbva484-1-i
  (`bug701`); the rest are UltraScale+ (xcvu3p, xcvu9p, xczu1cg, xck26,
  xcau10p), UltraScale (xcku035) and Versal. None contains a bitstream.
  `setup-rapidwright.sh --with-interchange` fetches the nine (6.7 MB)
  from a blobless clone, pinned by sha256.
* The prjxray-db harness designs (four Vivado 2017.2 DCPs with their
  `design.bit`, xc7a35t) are the only DCP + bitstream pairs available,
  and their EDIF is encrypted (§2).
* The Vivado bitstreams we do have (prjxray-db harness, prjxray
  `configuration_test*.bit`, prjuray-tools ToolsTestData for xc7a50t,
  xcku035, xczu3eg x2) and the flow outputs of T7.1-T7.3 are what §3's
  checks read.

## 5. What was implemented

`rwcheck.py` (driven by `tools/e2e/run-rapidwright-checks.sh`):

* `layout`: RapidWright's `ConfigArray` of every part of prjxray-db and
  prjuray-db (127 parts, 9 devices) and of the ToolsTestData xcku035 part
  against the Rust `Part`: the frame address walk (SHA-256 of the whole
  sequence; the Rust walk is read back with `bitread --aux` from an empty
  bitstream written by `xc7frames2bit` / `xcframes2bit`), the columns and
  their frame counts (also against `part.json`), the words per frame, the
  IDCODE (`part.json`) and the FDRI payload size (frames plus two pad
  frames per row). `--write-golden` stores the layouts
  (`tests/corpus/{xilinx/<family>,prjuray/zynqusp}/rapidwright/*.json`,
  180 KB), which `tests/e2e/test_rapidwright.py` checks without
  RapidWright.
* `bits`: every reference bitstream is read by RapidWright and by the
  Rust reader (frames with ECC, header fields, the whole packet list as
  (header, length, SHA-256 of the data)); the Rust writer's bitstream of
  those frames is read by RapidWright; RapidWright's bitstream of the
  same frames and its rewrite of the original are read by the Rust
  reader; the Rust and the RapidWright writers' FDRI payloads are
  compared. Every corpus `.frm` (the T7.1-T7.6 designs and
  `smoke_x1y0.frm`) goes through the Rust writer -> RapidWright reader and
  the RapidWright writer (its own ECC) -> Rust reader.
* `fasm`: §2.

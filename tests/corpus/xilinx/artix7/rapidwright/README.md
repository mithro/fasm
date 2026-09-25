# RapidWright configuration array layouts (T7.5)

One JSON file per device: the configuration array of the device as
RapidWright's `com.xilinx.rapidwright.bitstream.ConfigArray` describes it
(RapidWright `v2026.1.0-beta`, standalone jar sha256
`18f81833595ef8a5191a72f9431727e17602de2fe4dddbba2d281358801a1fc9`, its
device files `data/devices/<family>/<device>_db.dat` pinned in
`tools/e2e/setup-rapidwright.sh`). The same kind of file lives under
`tests/corpus/xilinx/{artix7,kintex7,spartan7,zynq7,kintexu}/rapidwright/`
and `tests/corpus/prjuray/zynqusp/rapidwright/`.

Written by

```sh
tools/e2e/setup-rapidwright.sh
python3 tools/e2e/rapidwright/rwcheck.py layout --write-golden
```

which first checks every field against the Rust tools and the database
(all 128 parts identical, see `docs/rewrite/DESIGN-xilinx-db.md` §8.15).
Fields:

| field | RapidWright API |
|---|---|
| `device`, `series` | `Device.getName()`, `Device.getSeries()` |
| `idcode` | `IDCode.getIDCode(Device)` |
| `words_per_frame` | `Frame.getWordsPerFrame(Series)` |
| `frame_overhead_count_per_row` | `ConfigArray.FRAME_OVERHEAD_COUNT_PER_ROW` (the pad frames after each row) |
| `config_array_words` | `ConfigArray.getWordSize()` (= the FDRI payload of a full bitstream) |
| `walk_frames`, `walk_sha256` | the frame addresses from `FAR.setFAR(0)` through `FAR.incrementFAR()` until it returns -1; the SHA-256 of the lines `0x%08X\n` |
| `columns` | per `ConfigRow` / `Block`: `[frame address of minor 0, Block.getFrameCount(), Block.getSubType() ("?" where RapidWright has none), Block.getTileColumn()]` |
| `parts` | the prjxray-db / prjuray-db parts (or ToolsTestData part) whose Rust walk and `part.json` equal this layout |

`tests/e2e/test_rapidwright.py` compares every part of the fetched
databases with these files through the Rust tools (no RapidWright
needed). Generated data describing the devices; the RapidWright device
files themselves are not stored here (RapidWright's licence:
Apache-2.0 for its sources, the device files are Xilinx/AMD data
downloaded by the setup script).

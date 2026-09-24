# Design: `fasm-xilinx` — prjxray / prjuray database, FASM↔frames↔bitstream

Status: research complete (T5.1). This document is the sole input a later
agent needs to implement the `fasm-xilinx` crate (T5.2–T6.3) without
re-reading the reference Python/C++ tools. Every claim below is backed by a
quoted source line; when the source was ambiguous or something could not be
determined from the checked-out material, it is called out explicitly in
§9.

## 1. Scope and sources

Reference checkouts used (all read-only; commit hashes captured with
`git rev-parse HEAD` at the time of this research):

| Repo | Local path (scratchpad `refs/` unless noted) | Commit | License |
|---|---|---|---|
| `f4pga/prjxray` | `refs/prjxray` | `c9f02d8576042325425824647ab5555b1bc77833` | ISC (`LICENSE`) |
| `f4pga/prjxray-db` (sparse: `artix7` only) | `refs/prjxray-db` | `0a0addedd73e7e4139d52a6d8db4258763e0f1f3` | CC0-1.0 (`LICENSE`) |
| `chipsalliance/f4pga-xc-fasm` (checked out as `f4pga-xc-fasm`) | `refs/f4pga-xc-fasm` | `25dc605c9c0896204f0c3425b52a332034cf5e5c` | Apache-2.0 (`LICENSE`) |
| `lromor/fpga-assembler` | `refs/fpga-assembler` | `b234841263e07df5306bbfec7789a8e87d94a31e` | Apache-2.0 (`LICENSE`) |
| `f4pga/prjuray` | `scratchpad/prjuray` (cloned this session) | `c550b03a26b4c4a9c4453353bd642a21f710b3ec` | Apache-2.0 (`LICENSE`) |
| `SymbiFlow/prjuray-tools` (git submodule of prjuray, `third_party/prjuray-tools`; cloned separately because the submodule was not fetched) | `scratchpad/prjuray-tools` | `f53f07b8fe37721137a57e9bee3b2b13e7676f53` | Apache-2.0 (`LICENSE`) |
| `f4pga/prjuray-db` (sparse: `zynqusp` only — the only family this repo has) | `scratchpad/prjuray-db` | `affbc5e555ebae16475f32e8fb2d6565d4204f3f` | CC0-1.0 (`COPYING`) |

All paths quoted below are relative to one of these checkouts unless a full
scratchpad path is given.

**Key structural discovery** (drives the whole document): prjuray does **not**
have its own `db.py`/`grid.py`/`tile_segbits.py`/`fasm_assembler.py` Python
package inside the `prjuray` repo itself — those live in the separate
`SymbiFlow/prjuray-tools` repo under its `prjuray/` Python package
(`third_party/prjuray-tools` submodule, `PYTHONPATH` includes `URAY_DIR`
itself per `prjuray/utils/environment.python.sh:20`). The `prjuray/utils/`
directory only has `fasm_assembler.py`, `fasm2frames.py`, `bit2fasm.py`,
`bitstream.py` (constants), `roi.py`, `util.py` — thin, prjxray-shaped
wrappers around the real `prjuray.db`/`prjuray.grid`/`prjuray.tile_segbits`
package in `prjuray-tools/prjuray/`. Likewise the C++ core
(`lib/`, `tools/`) lives in `prjuray-tools`, not `prjuray`.

## 2. Database layout on disk

### 2.1 prjxray-db (Series7: artix7/kintex7/spartan7/zynq7)

```
<db_root>/                              # e.g. prjxray-db/
  <family>/                             # "artix7", "kintex7", ...
    settings.sh                         # `source .../settings/<family>.sh`
    mapping/
      devices.yaml                      # device -> {fabric}
      parts.yaml                        # part -> {device, package, speedgrade}
    <fabric>/                           # e.g. "xc7a50t" (shared by several parts)
      tilegrid.json                     # the whole grid, all tiles, `bits` blocks
      tileconn.json                     # inter-tile wire connectivity (routing only)
      node_wires.json                   # node model data (routing only)
    <part>/                             # e.g. "xc7a35tcsg324-1"
      part.yaml                         # !<xilinx/xc7series/part> — frame address ranges + idcode
      part.json                         # same data as JSON + `iobanks`
      package_pins.csv                  # pin -> bank/site/tile/pin_function
      required_features.fasm            # optional; part-specific always-on FASM lines
    tile_type_<TILE_TYPE>.json          # per tile type, whole family (not per part!)
    site_type_<SITE_TYPE>.json          # per site type, whole family
    segbits_<tile_type>.db              # CLB_IO_CLK bus segbits (lowercase tile type in filename)
    segbits_<tile_type>.block_ram.db    # BLOCK_RAM bus segbits (only for BRAM tile types)
    segbits_<tile_type>.origin_info.db  # segbits + `origin:<fuzzer>` provenance (NOT read by tools)
    ppips_<tile_type>.db                # pseudo-PIPs: feature -> always|default|hint
    mask_<tile_type>.db                 # "bit FF_WW" lines; loaded into `TileDbs.mask` but
                                         # **never read** by TileSegbits, FasmAssembler or
                                         # FasmDisassembler — dead weight for this crate.
```

Which tool reads which file (prjxray Python):

| File | Reader | Code |
|---|---|---|
| `mapping/parts.yaml` | `util.get_part_information` | `prjxray/util.py:86-94` |
| `mapping/devices.yaml` | `util.get_fabric_for_part` | `prjxray/util.py:124-134` |
| `<fabric>/tilegrid.json` | `Database._read_tilegrid` | `prjxray/db.py:133-138` |
| `<fabric>/tileconn.json` | `Database._read_tileconn` | `prjxray/db.py:140-145` (only needed for `connections()`/routing — **not** used by fasm2frames/xc7frames2bit) |
| `<fabric>/node_wires.json` | `Database._read_node_wires` | `prjxray/db.py:147-152` (routing model only) |
| `tile_type_<T>.json` | `Database._read_tile_types` / `_get_tile_wires` | `prjxray/db.py:159-172` (routing model only — **the assembler never opens this file**) |
| `site_type_<T>.json` | `Database.get_site_type` | `prjxray/db.py:209-213` (not used by fasm2frames) |
| `segbits_<t>.db`, `segbits_<t>.block_ram.db`, `ppips_<t>.db` | `TileSegbits.__init__` | `prjxray/tile_segbits.py:80-96` |
| `mask_<t>.db` | loaded into `TileDbs.mask` by `Database.__init__` (`prjxray/db.py:83-86`) but never opened again |
| `<part>/part.yaml` | `xc7frames2bit`/`bitread` C++ (`ArchType::Part::FromFile`) | `lib/xilinx/xc7series/part.cc:19-26` |
| `<part>/part.json` | `xc_fasm/fasm2frames.py` (`iobanks`), `bitstream.gen_part_base_addrs` (`.replace(".yaml",".json")`) | `xc_fasm/fasm2frames.py:146-152`, `prjxray/bitstream.py:108-109` |
| `<part>/package_pins.csv` | `xc_fasm/fasm2frames.py` (STEPDOWN bank mapping) | `xc_fasm/fasm2frames.py:142-144` |
| `<part>/required_features.fasm` | `Database.__init__` | `prjxray/db.py:107-117` |

`settings.sh` (e.g. `prjxray/settings/artix7.sh:9-11`) sets `XRAY_DATABASE`
(the family dir name), `XRAY_PART` (default part), and ROI variables; it is
a *fuzzer* convenience, not read by `fasm2frames`/`xc7frames2bit` — those
take `--db-root`/`--part` directly (`prjxray/util.py:243-266`,
`db_root_arg`/`part_arg`). `fasm-xilinx`'s CLI only needs to replicate
`db_root_arg`/`part_arg`'s env-var fallback (`XRAY_DATABASE_DIR`+
`XRAY_DATABASE`, `XRAY_PART`), not `settings.sh` itself.

**`get_fabric_for_part(db_root, part)`** (`prjxray/util.py:124-134`) is the
one indirection a loader must reproduce exactly:
```python
def get_fabric_for_part(db_root, part):
    filename = os.path.join(db_root, "mapping", "devices.yaml")
    part = get_part_information(db_root, part)   # parts.yaml: part -> {device,...}
    device_mapping = yaml.load(open(filename))    # devices.yaml: device -> {fabric}
    device = device_mapping.get(part['device'], None)
    return device['fabric']
```
i.e. `fabric = devices.yaml[parts.yaml[part]['device']]['fabric']`. Example
(artix7): `parts.yaml['xc7a35tcsg324-1'] = {device: xc7a35t, package: csg324,
speedgrade: '1'}` (`prjxray-db/artix7/mapping/parts.yaml:209-212`),
`devices.yaml['xc7a35t'] = {fabric: xc7a50t}`
(`prjxray-db/artix7/mapping/devices.yaml:8-9`) — **the fabric directory name
is not always the device name** (35T and 50T share the xc7a50t fabric).

### 2.2 prjuray-db (UltraScale+ / zynqusp)

Only one family (`zynqusp`) exists in the checked-out `prjuray-db`, and its
layout differs structurally from prjxray-db, matching `prjuray-tools/prjuray/db.py`:

```
<db_root>/
  zynqusp/                              # "family" == "database" dir; NO further fabric split
    tile_types/tile_type_<T>.json       # note: subdirectory (prjxray-db has these flat)
    site_types/site_type_<T>.json       # note: subdirectory
    segbits_<tile_type>.db
    segbits_<tile_type>.block_ram.db
    segbits_<tile_type>.origin_info.db (+ .block_ram.origin_info.db)
    <part>/                             # e.g. "xczu3eg-sfvc784-1-e"
      tilegrid.json                     # PER PART, not per fabric (unlike prjxray-db!)
      tileconn.json                     # PER PART
      part.yaml                         # !<xilinx/xcupseries/part> — flat `rows:`, no top/bottom split
      part.json                         # {"idcode":..., "rows": {...}} — NO "iobanks" key
      package_pins.csv
```

Confirmed absent from the whole `zynqusp` tree: **no `ppips_*.db` files, no
`mask_*.db` files, no `settings.sh`, no `mapping/` directory anywhere**
(`find ... -iname "ppips*" -o -iname "mask*"` returned nothing; `find
... -iname mapping -o -iname settings.sh` returned nothing). `prjuray-tools/prjuray/db.py`
confirms the code path matches: `Database.__init__` builds `TileDbs` the
same way as prjxray (`ppips = os.path.join(db_root, 'ppips_{}.db'...)`,
falls back to `None` if the file does not exist — `prjuray-tools/prjuray/db.py:81-89`)
so pseudo-PIP handling degrades to "no ppips known" for every zynqusp tile
type in this database (see §9 risk). `Database` also reads
`<db_root>/tile_types/tile_type_<T>.json` and `<db_root>/site_types/site_type_<T>.json`
(`prjuray-tools/prjuray/db.py:67,105` — note the extra path segment vs prjxray) and
`<db_root>/<part>/tilegrid.json` + `<db_root>/<part>/tileconn.json`
directly (`prjuray-tools/prjuray/db.py:141-150` — **uses `self.part`, not a fabric
indirection**: prjuray has no `get_fabric_for_part`/`mapping/devices.yaml`
concept at all). `required_features.fasm` path is identical in shape:
`<db_root>/<part>/required_features.fasm` (`prjuray-tools/prjuray/db.py:112-113`).

Because there is exactly one family directory and each part carries its own
tilegrid, a `fasm-xilinx` loader must treat "fabric" as **optional**: for
prjxray-db, `fabric = devices.yaml[parts.yaml[part].device].fabric`; for
prjuray-db there is no such lookup — `fabric` is simply the family dir name
and tilegrid/tileconn are read from `<db_root>/<family>/<part>/`. The loader
should probe for `mapping/devices.yaml` under `<db_root>/<family>` to decide
which addressing scheme applies (see §9).

`prjuray/settings/zynqusp.sh` → `zynq_usp_3eg.sh` sets
`URAY_DATABASE=zynqusp`, `URAY_PART=xczu3eg-sfvc784-1-e`,
`URAY_ARCH=UltraScalePlus` (`prjuray/settings/zynq_usp_3eg.sh:18-20`) —
**prjuray-db as checked out here only documents UltraScale+ (Zynq
UltraScale+), not plain UltraScale**; see §9.

## 3. File formats

### 3.1 `tilegrid.json`

One JSON object, keyed by tile instance name (`"CLBLL_L_X2Y0"`), value:

```json
{
  "bits": {
    "CLB_IO_CLK": {
      "baseaddr": "0x00400100",
      "frames": 36,
      "offset": 0,
      "words": 2
    }
  },
  "clock_region": "X0Y0",
  "grid_x": 10,
  "grid_y": 155,
  "pin_functions": {},
  "prohibited_sites": [],
  "sites": { "SLICE_X0Y0": "SLICEL", "SLICE_X1Y0": "SLICEL" },
  "type": "CLBLL_L"
}
```
(prjxray-db `artix7/xc7a50t/tilegrid.json`, tile `CLBLL_L_X2Y0`.)

EBNF-ish grammar of one tile entry (all optional keys absent ⇒ empty):
```
tile        := { "bits": bus_map, "clock_region": (string|null),
                  "grid_x": uint, "grid_y": uint,
                  "pin_functions": {site: pin_function_string, ...},
                  "prohibited_sites": [site, ...],
                  "sites": {site: site_type, ...}, "type": tile_type }
bus_map     := { bus_name: bits_block, ... }        ; bus_name ∈ {"CLB_IO_CLK","BLOCK_RAM"}
bits_block  := { "baseaddr": "0x" hex8, "frames": uint,
                  "offset": uint, "words": uint,
                  ["alias": alias_block] }
alias_block := { "type": tile_type, "start_offset": uint,
                  "sites": {site: aliased_site, ...} }
```
Loaded 1:1 into `grid_types.Bits(base_address, frames, offset, words, alias)`
and `grid_types.BitAlias(tile_type, start_offset, sites)`
(`prjxray/grid.py:44-62`); `prjxray/grid_types.py:29` (`Bits = namedtuple('Bits',
'base_address frames offset words alias')`).

Statistics gathered on `prjxray-db/artix7/xc7a50t/tilegrid.json` (18055
tiles): 112 distinct tile types; only two `bits` bus names ever occur —
`CLB_IO_CLK` and `BLOCK_RAM` (`CFG_CLB` exists as a `BlockType` enum value
in the C++ frame-address code but never appears in a real 7-series
tilegrid — see §9); max `frames` observed 128 (BRAM), max `words` observed
101 (equal to `FRAME_WORD_COUNT`, i.e. some tiles' bits span the *entire*
frame — HCLK row tiles, see below).

`alias` example — **`words`/`offset` describe the exact same frame region
under two different tile-type name spaces**, used for L/R mirrored or
"_SING" IOB variants and the HCLK bottom U-turn tiles:
```json
"HCLK_L_BOT_UTURN_X72Y130": {"bits": {"CLB_IO_CLK": {
  "alias": {"sites": {}, "start_offset": 0, "type": "HCLK_L"},
  "baseaddr": "0x00020E00", "frames": 26, "offset": 50, "words": 1}}}
"LIOB33_SING_X0Y0": {"bits": {"CLB_IO_CLK": {
  "alias": {"sites": {"IOB33_Y0": "IOB33_Y0"}, "start_offset": 2, "type": "LIOB33"},
  "baseaddr": "0x00400000", "frames": 42, "offset": 0, "words": 2}}}
```
`offset: 50` on an HCLK row tile is the **HCLK "middle word"**: HCLK tiles
occupy exactly the single 32-bit word in the middle of the 101-word frame
(word index 50) shared by both the tile above and below the horizontal
clock row; `words: 1` confirms only that one word is addressed. The alias
mechanism is implemented entirely in `TileSegbitsAlias`
(`prjxray/tile_segbits_alias.py`), see §4.

### 3.2 `segbits_*.db`

One feature per line, whitespace-separated:
```
segbits_line := tag SP bitlist
tag          := [A-Za-z0-9_.\[\]]+          ; e.g. "CLBLM_L.SLICEL_X1.ALUT.INIT[10]"
bitlist      := bit (SP bit)*
bit          := ["!"] word "_" bitidx       ; e.g. "29_14", "!30_00"
word         := [0-9]+                      ; "word_column": added to base_address -> frame addr
bitidx       := [0-9]+                      ; NOT clamped to 0..31 for block_ram buses (see below)
```
Parser: `prjxray/tile_segbits.py:39-77` (`Bit = namedtuple('Bit',
'word_column word_bit isset')`, `parsebit`, `read_segbits`). Real lines:
```
CLBLL_L.SLICEL_X0.A5FF.ZINI 31_06
CLBLL_L.SLICEL_X0.AFFMUX.AX !30_00 30_01 !30_02 !30_03
CLBLL_L.SLICEL_X0.ALUT.INIT[00] 32_15
```
(`prjxray-db/artix7/segbits_clbll_l.db`). `!` (present on 12,290 of 253,678
bit occurrences measured across all 56 non-`origin_info` artix7 segbits
files) means "this bit must be **clear** for the feature to be active /
must be **cleared** when the feature is enabled" — see `parse_tagbit`
(`prjxray/util.py:321-331`) and the assembler semantics in §5.

**`word`/`bitidx` are *not* a `(frame-word, bit-in-word)` pair in general.**
`word` ("word_column") is added to the bus's `baseaddr` to get the frame
address (`frame = bits.base_address + query_bit.word_column`,
`prjxray/tile_segbits.py:133`); `bitidx` ("word_bit") is a **raw bit offset
within the frame's bit vector**, only later split into
`word_addr = bit.word_bit // 32; bit_index = bit.word_bit % 32`
(`prjxray/fasm_assembler.py:132-133`, `bitstream.WORD_SIZE_BITS = 32`). For
ordinary CLB_IO_CLK-bus segbits `bitidx` is always < 32 (one 32-bit word,
matching `words: 2` etc. is handled by *multiple lines with different
`word`/frame*), but for `BLOCK_RAM`-bus (`.block_ram.db`) segbits `bitidx`
routinely exceeds 31 — measured max 319 in one `segbits_bram_l.block_ram.db`
file, max 2204 across all artix7 `.block_ram.db` files combined — because a
BRAM `INIT_xx[N]` bit lives many 32-bit words into that tile's `BLOCK_RAM`
bus region:
```
BRAM_L.RAMB18_Y0.INIT_00[000] 00_00
BRAM_L.RAMB18_Y0.INIT_00[001] 00_16
BRAM_L.RAMB18_Y0.INIT_00[004] 00_80     ; word 80 => 32-bit word index 2, bit 16
```
(`prjxray-db/artix7/segbits_bram_l.block_ram.db`). A Rust decoder must treat
`word_bit` as `u32` (not clamp to 5 bits) and do the `/32`, `%32` split at
the point of use, exactly as `prjxray/fasm_assembler.py:132-133` does.

**Multi-bit features** (`TAG[N]`): 99,450 of 120,544 non-origin-info
feature lines in artix7 use `[N]` addressing (block-RAM `INIT`/`INITP`
strings dominate this count). `TileSegbits.__init__`
(`prjxray/tile_segbits.py:98-112`) builds a second index,
`feature_addresses[base_feature][N] = (block_type, full_feature_name)`,
used only when `feature_to_bits` is called with `address != 0` (§5).

**`.block_ram.db` variant**: identical grammar, loaded into a *second*
segbits map keyed by `BlockType.BLOCK_RAM`
(`prjxray/tile_segbits.py:94-96`); a tile type has at most one
`segbits_<t>.db` (→ `BlockType.CLB_IO_CLK`) and one
`segbits_<t>.block_ram.db` (→ `BlockType.BLOCK_RAM`).

**`.origin_info.db` variant**: same grammar with an extra `origin:<id>`
token right after the tag (`prjxray/util.py:280-284`,
`parse_db_line`/`write_db_lines` with `track_origin=True`) — e.g.
`CLBLL_L.SLICEL_X0.A5FF.ZINI origin:011-clb-ffconfig 31_06`. **Not read by
`Database`/`TileSegbits`/`FasmAssembler`/`fasm2frames` at all** — provenance
metadata for prjxray's own fuzzer pipeline only; `fasm-xilinx` never opens
these files.

**`_ALIAS`**: there is no `*_ALIAS*.db` file format in prjxray-db (the task
brief's "`_ALIAS`/`ppips`/`mask` files" phrasing refers to the *mechanism*,
which is the tilegrid `bits.<bus>.alias` block, §3.1, consumed by
`TileSegbitsAlias`, §4/§5 — not a separate file naming convention).

### 3.3 `ppips_*.db`

```
ppips_line := feature SP ppip_type
ppip_type  := "always" | "default" | "hint"
```
Parser: `prjxray/tile_segbits.py:24-36` (`read_ppips`,
`class PsuedoPipType(enum.Enum)`, sic — "Psuedo" typo is in the real code,
keep for byte-compat with any tooling that greps for it — not part of the
file format itself though). Examples:
```
CLBLL_L.CLBLL_L_AMUX.CLBLL_L_A hint
CLBLL_L.CLBLL_L_AX.CLBLL_BYP0 always
INT_L.BYP_ALT0.VCC_WIRE default
INT_L.BYP_BOUNCE0.BYP_ALT0 always
```
(`prjxray-db/artix7/ppips_clbll_l.db`, `ppips_int_l.db`). Semantics for the
**assembler** (all three types are handled identically by
`FasmAssembler`/`fasm2frames` — the distinction only matters to fuzzers and
the disassembler's pretty-printer, which this crate does not need to
reproduce): a feature present in the tile's `ppips` map contributes **zero
bits** — `feature_to_bits` returns immediately
(`prjxray/tile_segbits.py:170-171`, `if feature in self.ppips: return`).
Consuming such a feature in a FASM file is legal and a no-op (not a
`FasmLookupError`); an *unlisted* feature is the error case. Meaning of the
three tags for completeness (from prjxray docs / naming convention, not
algorithmically load-bearing here): `always` — the pip is physically
present with no configuration bit (hard-wired); `default` — it is what a
routing mux defaults to when nothing else on that mux is set (e.g. tie-off
to VCC/GND wires); `hint` — informational grouping only.

### 3.4 `mask_*.db`

```
mask_line := "bit" SP position
position  := word "_" bitidx
```
e.g. `bit 00_00`, `bit 00_01`, … (`prjxray-db/artix7/mask_clbll_l.db`).
`prjxray/util.py:275-276` special-cases this: `if tag == 'bit': raise
ValueError("Wanted bits db but got mask db")` — i.e. `parse_db_line` (the
generic segbits-style line parser) explicitly refuses to parse a mask file,
confirming mask files use a structurally different (but visually similar)
grammar. **No code in `prjxray`, `xc_fasm`, or `fpga-assembler` ever opens a
`mask_*.db` file's contents** — `Database.__init__` records the path in
`TileDbs.mask` (`prjxray/db.py:83-86`, `prjxray/tile.py:18` namedtuple) but
`TileSegbits.__init__` never reads `tile_db.mask`
(`prjxray/tile_segbits.py:81-96` only touches `.ppips`, `.segbits`,
`.block_ram_segbits`). **`fasm-xilinx`'s loader can skip mask files
entirely for T5.2–T6.3**; keep the path around only if a later
disassembler/mask-based validator phase wants it.

### 3.5 `part.yaml` / `part.json` (Series7)

`part.yaml` is a YAML document with a custom tag naming the C++ struct it
deserializes to; parsed only by the C++ tools (`xc7frames2bit`, `bitread`,
`gen_part_base_yaml`) via `yaml-cpp`, never by the Python `prjxray` package
(which reads `part.json` instead, see §3.6). Full grammar (from
`lib/include/prjxray/xilinx/xc7series/{part,global_clock_region,configuration_row,configuration_bus,configuration_column,frame_address}.h`
and `.cc` `YAML::convert<...>::decode`):
```yaml
!<xilinx/xc7series/part>
idcode: 0x362d093                    # hex string, YAML::Node::as<uint32_t>
global_clock_regions:
  top: !<xilinx/xc7series/global_clock_region>
    rows:
      0: !<xilinx/xc7series/row>          # key: row number (uint)
        configuration_buses:
          CLB_IO_CLK: !<xilinx/xc7series/configuration_bus>   # key: BlockType name
            configuration_columns:
              0: !<xilinx/xc7series/configuration_column>     # key: column number (uint)
                frame_count: 42
              1: {frame_count: 30}
              ...
          BLOCK_RAM: !<xilinx/xc7series/configuration_bus>
            configuration_columns: {0: {frame_count: 128}, ...}
      1: !<xilinx/xc7series/row> {...}
  bottom: !<xilinx/xc7series/global_clock_region>
    rows: {0: !<xilinx/xc7series/row> {...}}
```
Decoders: `Part::decode` accepts either `global_clock_regions` (this
nested form) **or** a flat `configuration_ranges: [{begin: <FrameAddress>,
end: <FrameAddress>}, ...]` list which it expands into individual
addresses and re-derives the row/bus/column tree
(`lib/xilinx/xc7series/part.cc:92-121`) — both forms are accepted on read,
only the nested form is ever written by `gen_part_base_yaml`. `Row::decode`
reads `configuration_buses` keyed by `BlockType` enum name
(`lib/xilinx/xc7series/configuration_row.cc:59-68`); `BlockType::decode`
(`lib/include/prjxray/xilinx/xc7series/block_type.h:21-26` +
`.cc`, not separately quoted) maps the literal strings `CLB_IO_CLK`,
`BLOCK_RAM`, `CFG_CLB` to the 0/1/2 enum (§4). `ConfigurationColumn::decode`
reads only `frame_count` (`lib/xilinx/xc7series/configuration_column.cc:49-58`).
This is **exactly the same tree shape as `part.json`'s
`global_clock_regions`** (§3.6) — `gen_part_base_yaml.cc` (which produces
`part.yaml`) derives it from FAR/LOUT writes found in a *debug* bitstream
(`BITSTREAM.GENERAL.DEBUGBITSTREAM` or `PERFRAMECRC` must be `YES`,
`tools/gen_part_base_yaml.cc:94-100`), and `bitstream.gen_part_base_addrs`
(`prjxray/bitstream.py:93-116`, used only by fuzzers) derives the same
shape from `part.json` by replacing `.yaml` with `.json` in
`XRAY_PART_YAML`. **A Rust loader only strictly needs `part.json`** (below)
for the FASM→frames→bitstream pipeline; `part.yaml` is needed only if
`fasm-xilinx` also implements `bitread`/`xc7frames2bit`-equivalent tools
that parse YAML directly instead of deriving the same tree from JSON —
recommendation: parse **both** into the same internal `Part` struct (JSON
for the Rust-native path, YAML only for byte-identical `--part_file`
compatibility with the C++ CLIs, see §8) since the JSON is strictly easier
to parse and 1:1 in content.

`part.json` (Series7): flat JSON mirror of the same tree, plus the two keys
Python actually needs:
```json
{
  "global_clock_regions": {"top": {"rows": {"0": {"configuration_buses": {
      "CLB_IO_CLK": {"configuration_columns": {"0": {"frame_count": 42}, ...}},
      "BLOCK_RAM":  {"configuration_columns": {"0": {"frame_count": 128}, ...}}
  }}, "1": {...}}}, "bottom": {"rows": {"0": {...}}}},
  "idcode": 56807571,
  "iobanks": {"0": "X1Y78", "14": "X1Y26", "15": "X1Y78",
              "16": "X1Y130", "34": "X113Y26", "35": "X113Y78"}
}
```
(`prjxray-db/artix7/xc7a35tcsg324-1/part.json` — full file, 6 iobanks).
`iobanks` maps a numeric IO bank id (as a string key) to an
`X<n>Y<n>` location string used to build the tile name
`"HCLK_IOI3_" + loc` (`xc_fasm/fasm2frames.py:150`) for STEPDOWN
propagation (§5). `idcode` here is decimal (`56807571` = `0x0362D093`,
matches `part.yaml`'s hex `0x362d093`).

### 3.6 `part.yaml` / `part.json` (UltraScale+, `xcupseries`)

Structurally **flatter** — no `global_clock_regions.{top,bottom}` split at
all, just a single `rows:` map, and (in the checked-out `zynqusp` database)
**no `iobanks` key**:
```yaml
!<xilinx/xcupseries/part>
idcode: 0x4a42093
rows:
  0: !<xilinx/xcupseries/row>
    configuration_buses:
      CLB_IO_CLK: !<xilinx/xcupseries/configuration_bus>
        configuration_columns:
          0: !<xilinx/xcupseries/configuration_column>
            frame_count: 16
          1: {frame_count: 76}
          ...
```
(`prjuray-db/zynqusp/xczu3eg-sfvc784-1-e/part.yaml`);
`part.json` for the same part: `{"idcode": 77865107, "rows": {...}}` (keys
`idcode`, `rows` only — verified with `python3 -c "print(list(d.keys()))"`
→ `['idcode', 'rows']`, and `d['iobanks']` → `None`/absent). This confirms
§5's note that prjuray's `fasm2frames.py` has no PUDC_B/STEPDOWN logic: the
database it ships simply carries no `iobanks` data to drive it. The
`FrameAddress`/`Part`/`Row`/`ConfigurationBus`/`ConfigurationColumn` C++
types for `xcupseries` (and the plain, non-plus `xcuseries`) live in
`prjuray-tools/lib/include/prjxray/xilinx/{xcupseries,xcuseries}/*.h` and
are **structurally the same nesting as Series7's `Row`/`ConfigurationBus`/
`ConfigurationColumn`** (`configuration_row.h`, `configuration_bus.h`,
`configuration_column.h` are near-identical files, only the namespace
differs) — only `FrameAddress`'s bit layout changes (§4) and there is no
`GlobalClockRegion` level (rows are not partitioned into top/bottom halves
at the `Part` level; "half" lives inside the row's own address bits, §4).

### 3.7 `package_pins.csv`

```
pin,bank,site,tile,pin_function
A1,35,IOB_X1Y81,RIOB33_X43Y81,IO_L9N_T1_DQS_AD7N_35
```
(`prjxray-db/artix7/xc7a35tcsg324-1/package_pins.csv:1-2`). Read with
`csv.DictReader` (`xc_fasm/fasm2frames.py:142-144`); only `bank` and `tile`
columns are used, to build `bank_to_tile`/`tile_to_bank` maps
(`xc_fasm/fasm2frames.py:154-156`) for STEPDOWN propagation (§5). Same
5-column shape in prjuray-db.

### 3.8 `tileconn.json` / `tile_type_*.json` / `site_type_*.json`

Confirmed **not needed** by the FASM↔frames↔bitstream pipeline this task
covers. `tileconn.json` is a list of inter-tile wire-pair connections
(`{"grid_deltas": [dx, dy], "tile_types": [a, b], "wire_pairs": [[wa, wb],
...]}`, 748 entries in `xc7a50t/tileconn.json`) used only by
`prjxray.connections`/`NodeModel` (routing graph, not part of frame
assembly). `tile_type_*.json` (`{"pips": {...}, "sites": {...}, "tile_type":
str, "wires": [...]}`, e.g. `tile_type_CLBLL_L.json`) is used only by
`Database.connections()`/`_get_tile_wires` (routing) — `FasmAssembler`,
`fasm2frames`, and `xc7frames2bit` never open it, matching the task
brief's "probably nothing". `site_type_*.json` (`{"site_pins": ..., "site_pips":
..., "type": ...}`) is likewise unused by this pipeline. **`fasm-xilinx`'s
T5.2 loader should not parse any of these three formats.**


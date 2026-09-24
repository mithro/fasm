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

## 4. Frame addressing

### 4.1 Series7 (and, per the shim in the plain `prjxray` checkout, the
    naive UltraScale/UltraScale+ path — see the correction in §4.2)

32-bit frame address, from `lib/xilinx/xc7series/frame_address.cc:20-30`
(`bit_field_set(value, top_bit, bottom_bit, field)`, both bounds inclusive):

| Bits | Field | Width | Notes |
|---|---|---|---|
| 25:23 | `block_type` | 3 | `0=CLB_IO_CLK, 1=BLOCK_RAM, 2=CFG_CLB` (`prjxray/util.py:348-352`, `block_type_i2s`) |
| 22 | `is_bottom_half_rows` | 1 | 0 = top half, 1 = bottom half |
| 21:17 | `row` | 5 | up to 32 rows per half |
| 16:7 | `column` | 10 | up to 1024 columns |
| 6:0 | `minor` | 7 | up to 128 frames per column (matches observed max `frames: 128` for BRAM) |

Python has the same encoding, independently, in
`prjxray/util.py:360-370` (`addr2btype`) and `prjxray/bitstream.py:118-127`
(`addr_bits2word`):
```python
def addr_bits2word(block_type, top_bottom, cfg_row, cfg_col, minor_addr):
    ret = block_type_s2i[block_type] << 23
    ret |= {"top": 0, "bottom": 1}[top_bottom] << 22
    ret |= cfg_row << 17
    ret |= cfg_col << 7
    ret |= minor_addr
    return ret
```
`FRAME_WORD_COUNT = 101` (32-bit words per frame),
`WORD_SIZE_BITS = 32`, `FRAME_ALIGNMENT = 0x80`
(`prjxray/bitstream.py:16-22` — alignment is a fuzzer/allocator convention,
not part of the addressing math itself).

**tilegrid → segbit → (frame, word, bit)** — the whole point of the
`bits.<bus>` block (§3.1) is that a tile's segbits file stores small local
offsets (`word_column`, `word_bit`), and the tilegrid supplies the two
numbers that turn those into a real frame address and bit position:
```python
# prjxray/tile_segbits.py:161-167 (TileSegbits.map_bit_to_frame)
frame     = bits.base_address + bit.word_column      # absolute frame address
word_bit  = bits.offset * WORD_SIZE_BITS + bit.word_bit  # bit index in [0, frames*32)
# prjxray/fasm_assembler.py:131-133 (FasmAssembler.enable_feature.update_segbit)
frame_addr = bit.word_column          # (already absolute, see map_bit_to_frame above)
word_addr  = bit.word_bit // 32
bit_index  = bit.word_bit % 32
```
So: `frame_address = tilegrid.bits[bus].baseaddr + segbit.word_column`;
`absolute_bit = tilegrid.bits[bus].offset * 32 + segbit.word_bit`;
`word_index_in_frame = absolute_bit // 32`; `bit_in_word = absolute_bit %
32`. `offset` (from tilegrid) is the tile's starting **word** inside the
101-word frame (not a bit offset — it gets multiplied by 32 before adding
`word_bit`); `words` (from tilegrid) bounds how many consecutive 32-bit
words that bus occupies starting at `offset`, and is used by the
*disassembler* (`bits_info.bits.words`, checking `word_idx + offset in
bitdata[frame][0]`, `prjxray/fasm_disassembler.py:119-121`) and by
`TileSegbitsAlias.match_filter` to bounds-check aliased bits
(`prjxray/tile_segbits_alias.py:101-107`) — the assembler itself does not
range-check against `words` (it trusts the segbits file).

**HCLK row middle word**: as shown in §3.1, an HCLK tile's `bits` block has
`offset: 50, words: 1` — i.e. its one word is exactly word index 50 of the
101-word frame (the physical middle), shared between the tile logically
"above" and "below" it in the row; the segbits file's `word_bit` for HCLK
features is always < 32 (relative to that single word), so `absolute_bit =
50*32 + word_bit` lands in `[1600, 1631]`, i.e. word 50 always.

**Alias mechanism** (`TileSegbitsAlias`, `prjxray/tile_segbits_alias.py`) —
used when a tile type (e.g. `HCLK_L_BOT_UTURN`, `LIOB33_SING`) shares its
physical bit region with a different, more general tile type (`HCLK_L`,
`LIOB33`) but starts at a **word offset into that other type's segbits**:
```python
# __init__ (per block_type present in the tile's `bits` map)
alias_bits_map[block_type] = Bits(
    base_address=bits_map[block_type].base_address,
    frames=bits_map[block_type].frames,
    offset=bits_map[block_type].offset - alias.start_offset,   # <-- the whole trick
    words=bits_map[block_type].words, alias=None)
```
(`prjxray/tile_segbits_alias.py:46-55`). i.e. it *subtracts*
`start_offset` from this tile's own `offset` so that, when the aliased
tile type's segbits are looked up with the *aliased* feature name (built
by `map_feature_to_segbits`, which swaps `parts[0]` = alias tile_type and
remaps site names via `alias.sites`, `prjxray/tile_segbits_alias.py:75-86`),
the resulting `word_bit` (computed against the *alias* tile's own
`baseaddr`+`offset` convention) lands at the correct absolute frame
position for *this* tile. `feature_to_bits` on `TileSegbitsAlias` simply
delegates to the aliased tile type's `TileSegbits.feature_to_bits` with the
adjusted `alias_bits_map` (`prjxray/tile_segbits_alias.py:119-126`).
`Grid.get_tile_segbits_at_tilename` picks `TileSegbitsAlias` over the plain
per-type segbits whenever **any** `bits.<bus>.alias` key is present on that
tile (`prjxray/grid.py:137-149`).

### 4.2 UltraScale / UltraScale+ (`xcuseries` / `xcupseries`) — bit layout differs from Series7

**This is the single most important divergence for a from-scratch Rust
implementation.** The plain `prjxray` checkout's `architectures.h`
(`lib/include/prjxray/xilinx/architectures.h:68-78`) defines `UltraScale`
and `UltraScalePlus` as C++ classes that simply **inherit** `Series7`'s
`FrameAddress`/`Part` types unchanged (only overriding `words_per_frame`):
```cpp
class UltraScalePlus : public Series7 {
  public: UltraScalePlus() : Series7("UltraScalePlus") {}
  static constexpr int words_per_frame = 93;
};
class UltraScale : public Series7 {
  public: UltraScale() : Series7("UltraScale") {}
  static constexpr int words_per_frame = 123;
};
```
This is **incorrect/incomplete** for the real UltraScale/+ bit layout — it
was superseded in the separate `SymbiFlow/prjuray-tools` repo (the actual
prjuray C++ core, §1), which defines dedicated `xcuseries` (plain
UltraScale) and `xcupseries` (UltraScale+) namespaces with their own
`FrameAddress`, `Part`, `Row`, `ConfigurationBus`, `ConfigurationColumn`,
and correctly wires them into `architectures.h`:
```cpp
class UltraScale : public Series7 {
  public: UltraScale() : Series7("UltraScale") {}
  using Part = xcuseries::Part;
  using FrameAddress = xcuseries::FrameAddress;
  static constexpr int words_per_frame = 123;
};
class UltraScalePlus : public Series7 {
  public: UltraScalePlus() : Series7("UltraScalePlus") {}
  using Part = xcupseries::Part;
  using FrameAddress = xcupseries::FrameAddress;
  static constexpr int words_per_frame = 93;
};
```
(`prjuray-tools/lib/include/prjxray/xilinx/architectures.h:65-79`).
**`fasm-xilinx` must implement the `prjuray-tools` (xcuseries/xcupseries)
bit layout, not the plain-`prjxray` shim** — the shim reusing Series7's
`FrameAddress` for UltraScale/+ would decode wrong `column`/`minor` values
because the field widths differ (below). `ConfRegType` (config register
numbering) **is** shared with Series7 in both repos (`UltraScale`/
`UltraScalePlus` do not override `ConfRegType`) — see §6.

| | Series7 | UltraScale (`xcuseries`) | UltraScale+ (`xcupseries`) |
|---|---|---|---|
| `words_per_frame` | 101 (×32-bit) | 123 (×32-bit) | 93 (×32-bit) |
| block_type bits | `[25:23]` (3b) | `[25:23]` (3b, same as Series7) | **`[26:24]`** (3b, shifted up 1) |
| row+half bits | `[22:17]` (half=bit22, row=`[21:17]`, 5b) | `[22:17]` (same as Series7: half=bit22, row=`[21:17]`) | **`[23:18]`** (half=bit23, row=`[22:18]`, 5b) |
| column bits | `[16:7]` (10b) | `[16:7]` (same as Series7) | **`[17:8]`** (10b) |
| minor bits | `[6:0]` (**7b**, max 127) | `[6:0]` (same as Series7, **7b**) | **`[7:0]`** (**8b**, max 255) |
| total address width | 26 bits | 26 bits (numerically identical layout to Series7) | 27 bits |
| Python constant `FRAME_WORD_COUNT` | 101 | *(no separate constant found; the checked-out `prjuray` Python only ships `93*2`, see below)* | `93 * 2 = 186` (as 16-bit half-words) |
| Python `WORD_SIZE_BITS` | 32 | — | **16** |
| Python `FRAME_ALIGNMENT` | `0x80` | — | `0x100` |

Sources: `prjuray-tools/lib/include/prjxray/xilinx/xcuseries/frame_address.h:18-30`
(`BLOCK_TYPE_HIGH=25,LOW=23; ROW_HIGH=22,LOW=17; COLUMN_HIGH=16,LOW=7;
MINOR_HIGH=6,LOW=0` — numerically the *same* bit ranges as Series7, i.e.
plain UltraScale really does share Series7's address layout, just a
different word count); `prjuray-tools/lib/include/prjxray/xilinx/xcupseries/frame_address.h:18-27`
(`BLOCK_TYPE_HIGH=26,LOW=24; ROW_HIGH=23,LOW=18; COLUMN_HIGH=17,LOW=8;
MINOR_HIGH=7,LOW=0` — everything shifted up one bit and minor widened to 8
bits because UltraScale+ tiles can have up to 256 frames in one column,
confirmed by the observed `"frames": 256` on `BRAM_X2Y0`'s `BLOCK_RAM` bus,
§3.6); `prjuray-tools/prjuray/bitstream.py:20-27` (`WORD_SIZE_BITS = 16`,
`FRAME_WORD_COUNT = 93 * 2`, `FRAME_ALIGNMENT = 0x100`, comment "How many
16-bit words for frame in a US+ bitstream").

**The prjuray Python side (`prjuray-tools/prjuray/*`, used by
`fasm_assembler.py`/`fasm2frames.py`) models a frame as `186` **16-bit**
half-words**, not `93` 32-bit words — i.e. `bitstream.WORD_SIZE_BITS=16`
changes the `word_addr = bit.word_bit // WORD_SIZE_BITS; bit_index =
bit.word_bit % WORD_SIZE_BITS` split in `fasm_assembler.py:129-130` to
operate on 16-bit granularity. This is consistent with the `xcupseries` C++
side working in native 32-bit words (`words_per_frame = 93`) — the Python
`.frm`/assembler layer is simply using a finer-grained "word" unit than the
C++ bitstream layer; a Rust `fasm-xilinx` should pick **one** canonical
internal unit (32-bit words, matching the C++ bitstream format and Series7)
and convert: a prjuray "16-bit word index" `w16` maps to 32-bit word
`w16 / 2`, half-word position `w16 % 2` (upper/lower 16 bits) — this exact
conversion is spelled out in `prjuray/utils/fasm2frames.py:78-88`
(`output_bits`, converting frames-as-16-bit-words back to the `.bits`
32-bit-word text format: `bit32_idx = bit_idx + (word_idx & 0x1) * 16;
word32_idx = word_idx >> 1`). **Recommendation**: `fasm-xilinx`'s internal
`Frames` container should always be arrays of 32-bit words (one array
length per architecture: 101/123/93), and the loader for prjuray-style
segbits (which store bit offsets in 16-bit-word units per `WORD_SIZE_BITS
= 16`) should convert to 32-bit-word+bit at load time or at
lookup time, consistently with how the Series7 path already does the
`//32, %32` split — just parameterize the divisor per architecture
(32 for Series7/xcuseries-as-used-by-plain-prjxray-shim, but see next
paragraph: the *authoritative* prjuray Python explicitly uses 16).

Frame address ECC/data-word semantics for the two UltraScale+ ECC words
(word 45 + low 16 bits of word 46, replacing Series7's single-word ECC at
word 50) are covered in §6 (bitstream, not FASM assembly — the ECC is
computed only when writing/reading the actual `.bit`, not when building
`.frm`).

### 4.3 Citations for the code that implements addressing (all read; line
    numbers as of the commits in §1)

| What | prjxray (7 series) | prjuray-tools (UltraScale/+) |
|---|---|---|
| `FrameAddress` bit-field layout | `lib/xilinx/xc7series/frame_address.cc:20-49` | `lib/xilinx/xcuseries/frame_address.cc` (same ranges as Series7) and `lib/xilinx/xcupseries/frame_address.cc:11-41` |
| tilegrid `bits` → segbit → bit position | `prjxray/tile_segbits.py:161-167`, `prjxray/fasm_assembler.py:128-138` | `prjuray-tools/prjuray/tile_segbits.py` (verified structurally identical to prjxray's — same function names/line shapes), `prjuray-tools/prjuray/fasm_assembler.py` not separately re-derived (uses same `prjuray.bitstream.WORD_SIZE_BITS`) |
| next-frame-address iteration (row/column/minor rollover, used by frame padding, §6) | `lib/xilinx/xc7series/{part,global_clock_region,configuration_row,configuration_bus,configuration_column}.cc` | `lib/xilinx/xcupseries/{part,configuration_row,configuration_bus,configuration_column}.cc` (same shape, no `global_clock_region` level — `Part::rows_` is a flat `std::map<unsigned int, Row>`, `lib/include/prjxray/xilinx/xcupseries/part.h:44-45`) |

## 5. FASM → frames algorithm

This is `prjxray.fasm_assembler.FasmAssembler` (`prjxray/fasm_assembler.py`,
byte-identical in `prjuray-tools/prjuray/fasm_assembler.py` modulo the
import of a local `bitstream` module and the loss of the `word_addr >= 101`
sanity print — see the diff notes inline below) driven by
`xc_fasm.fasm2frames.fasm2frames` (`xc_fasm/fasm2frames.py:119-282`) /
`prjuray/utils/fasm2frames.py:91-139` (`run`). A Rust port must reproduce
this exactly:

1. **Parse** the FASM file with the `fasm` crate's parser
   (`fasm.parse_fasm_filename`, `xc_fasm/fasm2frames.py:194`/
   `prjuray/utils/fasm2frames.py:129`), yielding `FasmLine` records.
2. **Per line**, `FasmAssembler.add_fasm_line` (`prjxray/fasm_assembler.py:165-192`):
   - Skip lines with `set_feature is None` (comments/annotations only).
   - Invoke the `feature_callback` (used by `fasm2frames.py` to build
     `set_features`, the STEPDOWN/PUDC_B tracking set — **not** part of the
     bit-setting logic itself).
   - `line_str = fasm.fasm_line_to_string(line)` — reconstruct the
     canonical single-line text, used only for error messages / dedup keys
     (must byte-match the `fasm` crate's own `fasm_line_to_string`; this is
     covered by Phase 1's `output` module, not re-specified here).
   - Split `line.set_feature.feature` on `.`: `tile = parts[0]`, `feature =
     '.'.join(parts[1:])` (`prjxray/fasm_assembler.py:175-177`).
   - **Canonicalize**: `for flat_set_feature in fasm.canonical_features(line.set_feature):`
     — this is the `fasm` crate's `output::canonical_features` /
     `merge_features` machinery (Phase 1) applied to a *single* line: it
     expands a `TAG[a:b] = value` multi-bit assignment into one
     `flat_set_feature` per **set** bit (bits that are 0 in `value` are
     dropped — **`value == 0` bits are simply not emitted as
     `flat_set_feature`s at all**, so a multi-bit feature assigned all-zero
     produces zero `enable_feature` calls for that tag, i.e. is a complete
     no-op — matches `db_dev_process`/`segmaker` conventions that
     "clearing" a multi-bit field is inferred from the *absence* of a set
     bit, not from an explicit `!` list the way single-bit `!TAG` works).
     For a bare (single-bit, `start=None`) feature, `canonical_features`
     yields it unchanged (one flat feature, `address = 0` since
     `flat_set_feature.start is None`).
   - For each flat feature: `address = flat_set_feature.start or 0`; call
     `self.enable_feature(tile, feature, address, line_str)`, catching
     `FasmLookupError` into a `missing_features` list (**not** raised
     immediately — all lines are processed first, then a single combined
     `FasmLookupError('\n'.join(missing_features))` is raised at the very
     end of `parse_fasm_filename`**, `prjxray/fasm_assembler.py:194-203`**
     — i.e. one FASM file with 3 unknown features produces **one** raised
     exception whose message has 3 newline-joined lines, not 3 separate
     exceptions).
3. **`enable_feature(tile, feature, address, line)`** (`prjxray/fasm_assembler.py:125-163`):
   - `gridinfo = grid.gridinfo_at_tilename(tile)` — **KeyError here is not
     caught specially** (propagates as a raw `KeyError`, not
     `FasmLookupError`) if `tile` does not exist in the tilegrid at all.
   - `segbits = grid.get_tile_segbits_at_tilename(tile)` — picks
     `TileSegbitsAlias` or plain `TileSegbits` per §4.1.
   - `db_k = f"{gridinfo.tile_type}.{feature}"` — the lookup key is
     **tile-type-qualified**, e.g. `CLBLM_L.SLICEM_X0.ALUT.INIT[00]` with
     the tile-type prefix `CLBLM_L` substituted for the *tile instance*
     name.
   - `for block_type, bit in segbits.feature_to_bits(gridinfo.bits, db_k, address):`
     — iterates **all** `(block_type, Bit)` pairs the feature resolves to
     (usually exactly the bits on one bus, but a feature name could in
     principle exist verbatim on more than one bus — `feature_to_bits`
     (`prjxray/tile_segbits.py:169-184`) checks ppips first (return nothing,
     no error), then, if `address == 0`, checks every `block_type` in
     `self.segbits` for an exact-name match and returns on the **first**
     match found (dict iteration order = insertion order = `CLB_IO_CLK`
     before `BLOCK_RAM`, since that's the order `TileSegbits.__init__`
     populates `self.segbits`); only if `address != 0` (multi-bit `[N]`
     addressing) does it fall through to `self.feature_addresses[feature][address]`,
     which **raises `KeyError`** (→ `FasmLookupError` at the call site) if
     neither the feature nor that specific `[address]` slot exists.
   - Each yielded `Bit` is turned into `(frame_addr, word_addr, bit_index)`
     via the `//32,%32` split (§4.1) and passed to `frame_set`/`frame_clear`
     depending on `bit.isset` (the `!` flag from the segbits line, §3.2 —
     **not** the FASM line's own value; a `!` bit in the segbits DB means
     "clear this physical config bit when this feature is enabled",
     independent of whether the FASM feature itself was written with `=0`
     or bare).
   - Any `KeyError` from the whole `feature_to_bits` generator (either the
     "not in `feature_addresses`" case above, or the tile-type not being in
     the loaded `db.tile_types` at all, `KeyError` from
     `self.tile_types[tile_type.upper()]` inside `Database.get_tile_segbits`)
     is caught and re-raised as `FasmLookupError("Segment DB %s, key %s not
     found from line '%s'" % (gridinfo.tile_type, db_k, line))`
     (`prjxray/fasm_assembler.py:153-156`) — **this exact message format
     must be reproduced** if `fasm-xilinx`'s CLI is to be diff-compatible
     with `fasm2frames.py`'s stderr/exception text.
   - After the bit loop, **for every `block_type` that contributed at
     least one bit**, all frames of that bus are marked "in use":
     `for frame in range(bits.base_address, bits.base_address +
     bits.frames): frames_in_use.add(frame)` (`prjxray/fasm_assembler.py:158-163`)
     — this is what makes `--sparse` output still zero-fill *every* frame
     of a tile's bus once *any* bit on that bus was touched (§5, sparse
     semantics below), even frames whose bits were all left at their
     default 0.
4. **`frame_set`/`frame_clear`** (`prjxray/fasm_assembler.py:81-123`):
   maintain `self.frames: dict[(frame_addr, word_addr, bit_index), 0|1]`
   and `self.frames_line[key] = line` (the FASM line text that last touched
   that bit, for error messages). Both check `word_addr >= 101` first and,
   if true, **print a warning to stderr and silently drop the write**
   (`prjxray/fasm_assembler.py:86-88,108-110` — **not** an exception; this
   is dead code for Series7 since no segbit ever produces `word_addr>=101`
   from a 101-word frame, but is a real hazard if `fasm-xilinx` reuses this
   constant unmodified for UltraScale's 123-word frames — the check must be
   parameterized per architecture, and **is absent entirely** in the
   prjuray fork's copy of `frame_set`/`frame_clear`,
   `prjuray-tools/prjuray/fasm_assembler.py:84-120` has no such guard at
   all). If the key was already set to a *different* value by an earlier
   line, raise `FasmInconsistentBits('FASM line "{line}" wanted to
   {set|clear} bit {key} but was {cleared|set} by FASM line
   "{frames_line[key]}"')` (`prjxray/fasm_assembler.py:90-97,111-119`) —
   **this is how `!` vs. non-`!` conflicts across two different FASM
   features that happen to touch the same physical bit are detected**; if
   the same value is written again (idempotent), it's a silent no-op.
   `FasmInconsistentBits` **propagates uncaught** all the way out of
   `parse_fasm_filename` (unlike `FasmLookupError`, which is batched) —
   i.e. the *first* inconsistent-bit conflict aborts the whole run
   immediately with a Python traceback (`fasm2frames.py`'s CLI has no
   try/except around `assembler.parse_fasm_filename`, so this becomes an
   uncaught exception → non-zero exit + traceback on stderr; `fasm-xilinx`
   should reproduce "first conflict aborts immediately" but can choose a
   cleaner error type/message as long as behavior — abort, not batch — matches).
5. **Duplicate features / idempotent re-enable**: enabling the exact same
   feature+address twice (e.g. FASM has the same line twice, or two
   different lines that expand to the same segbits) is fine — `frame_set`/
   `frame_clear` treat re-writing the same value as a no-op (step 4). There
   is no separate "duplicate feature" detection beyond the bit-level
   consistency check — i.e. `fasm-xilinx` should **not** implement a
   feature-name-level duplicate check; the semantics are entirely bit-level.
6. **Extra/required features**: `fasm2frames()` builds `extra_features`
   from two independent sources, both parsed as extra `FasmLine`s and fed
   through the *same* `add_fasm_line` path (so they get the identical
   conflict-checking as user FASM lines):
   - ROI's `required_features` list, if `--roi design.json` was given and
     the JSON has a `"required_features"` key (list of FASM feature
     strings) — `xc_fasm/fasm2frames.py:184-187`.
   - `db.get_required_fasm_features(part)` — the part's
     `required_features.fasm` file (§2.1/§3, one feature per non-blank
     line) — `xc_fasm/fasm2frames.py:190-192`,
     `prjxray/db.py:222-233` (`get_required_fasm_features`, returns
     `set()` if the part has none — **the checked-out artix7 slice of
     prjxray-db has no `required_features.fasm` anywhere** —
     `find . -iname "required_features*"` found nothing — so this path is
     unexercised by the available test data; implement per spec but note
     as untested against real data, §9).
   `extra_features` are appended (not prepended) to `assembler.parse_fasm_filename(filename_in,
   extra_features=extra_features)` — order: **all lines from the input
   FASM file first, then all `extra_features`**
   (`xc_fasm/fasm2frames.py:194`, `FasmAssembler.parse_fasm_filename`,
   `prjxray/fasm_assembler.py:194-203`, loops `for line in
   fasm.parse_fasm_filename(filename): ...` then `for line in
   extra_features: ...`). This ordering matters for `FasmInconsistentBits`
   messages (which line is "the earlier" one) but not for the final bit
   values.
7. **ROI** (`prjxray.roi.Roi`, `xc_fasm/fasm2frames.py:175-183`): when
   `--roi design.json` is given, `Roi(db, x1, x2, y1, y2)` (grid coordinate
   bounding box, inclusive on both ends — `Roi.tile_in_roi`,
   `prjxray/roi.py:25-29`, `x1<=x<=x2 and y1<=y<=y2`) is built from the
   ROI JSON's `info.GRID_X_MIN/MAX/GRID_Y_MIN/MAX` keys, then
   `assembler.mark_roi_frames(roi)` is called **before** parsing the FASM
   file: for every tile inside the ROI box (regardless of whether the FASM
   file sets any feature on it), every frame of every bus that tile has is
   added to `frames_in_use` (`prjxray/fasm_assembler.py:205-213`,
   identical logic to the "mark bus frames in use" step in `enable_feature`,
   §step 3 above). **Effect**: with `--sparse`, ROI tiles' frames are
   always zero-filled in the output even if nothing in the FASM file
   touches them, but non-ROI tiles outside the box that also weren't
   touched are omitted entirely. `--roi` does **not** filter/reject FASM
   features whose tile lies outside the ROI box — it is purely additive to
   `frames_in_use` for sparse-output purposes; there is no "features
   outside ROI are errors" check anywhere in this code path.
8. **PUDC_B pullup** (`--emit_pudc_b_pullup`, Series7-only feature — absent
   from prjuray's `fasm2frames.py` entirely, confirmed by reading
   `prjuray/utils/fasm2frames.py:91-139` top to bottom, no PUDC_B mention):
   - `find_pudc_b(db)` (`xc_fasm/fasm2frames.py:82-104`) scans every tile's
     `gridinfo.pin_functions` for a site whose pin-function string contains
     `'PUDC_B'`; **asserts there is at most one such site in the whole
     part** (`assert pudc_b_tile_site == None`, i.e. a part with two PUDC_B
     pins would crash `fasm2frames.py` with an `AssertionError` — a latent
     bug/assumption to preserve or deliberately fix, §9). Computes `iob_y =
     int(site[-1]) % 2` and returns `(tile, f"IOB_Y{iob_y}")`.
   - If `--emit_pudc_b_pullup` is set and a PUDC_B site was found, the
     assembler's feature callback is wrapped
     (`check_for_pudc_b`, `xc_fasm/fasm2frames.py:162-172`) to notice if
     the user's own FASM already sets a feature on that exact
     `(tile, site)` — if so, `pudc_b_in_use = True` and the synthetic
     pullup lines below are **skipped**.
   - After parsing the main FASM file (and only if `pudc_b_in_use` is still
     `False` and a PUDC_B site exists), three synthetic FASM lines are fed
     through `assembler.add_fasm_line` (`xc_fasm/fasm2frames.py:196-214`),
     literal template (Artix-50T/Zynq-10 specific per the code's own
     comment, **known wrong for K70T**):
     ```
     {tile}.{site}.LVCMOS12_LVCMOS15_LVCMOS18_LVCMOS25_LVCMOS33_LVDS_25_LVTTL_SSTL135_SSTL15_TMDS_33.IN_ONLY
     {tile}.{site}.LVCMOS25_LVCMOS33_LVTTL.IN
     {tile}.{site}.PULLTYPE.PULLUP
     ```
     Any `FasmLookupError` from these synthetic lines is collected the same
     way and re-raised as a combined `FasmLookupError` immediately (not
     deferred with the main parse) — `xc_fasm/fasm2frames.py:213-214`.
9. **STEPDOWN propagation over IO banks** (`xc_fasm/fasm2frames.py:216-272`,
   Series7-only — absent from prjuray, no `iobanks`/STEPDOWN code in
   `prjuray/utils/fasm2frames.py`). Runs unconditionally (not gated by a
   flag) whenever `part is not None`, **after** the main FASM parse (so it
   sees `set_features`, the full set of `SetFasmFeature`s observed via the
   `feature_callback`, §step 2):
   - Build `used_iob_sites: set[(tile, site)]` — every `(tile, site)` pair
     seen in `set_features` where `set_feature.value != 0` and `"IOB33" in
     tile` (feature split on `.` with `maxsplit` implied by plain `.split(".")`
     then `tile, site, tag = feature.split(".", maxsplit=2)` only when
     `len(parts) >= 3`).
   - Build `stepdown_tags: dict[bank -> set[tag]]` and `stepdown_banks:
     set[bank]` from every `set_feature` (again `value != 0` only) whose
     3rd-level tag contains the substring `"STEPDOWN"`: `bank =
     tile_to_bank[tile]` (from the `package_pins.csv`+`iobanks` maps built
     at the top of `fasm2frames()`, §3.7); `stepdown_tags[bank].add(tag)`.
   - For every bank that had a STEPDOWN tag, for every tile in
     `bank_to_tile[bank]` (both IOB33 tiles from `package_pins.csv` *and*
     the synthetic `"HCLK_IOI3_" + iobanks[bank]` tile, §3.5's `iobanks`
     map):
     - If the tile name contains `"IOB33"`: for every site
       `get_iob_sites(db, tile)` yields (`IOB_Y{int(site[-1]) % 2}` for
       every site in that tile's `gridinfo.sites`,
       `xc_fasm/fasm2frames.py:107-116`) that is **not** already in
       `used_iob_sites`, and for every STEPDOWN tag recorded for that bank,
       synthesize the FASM feature string `f"{tile}.{site}.{tag}"` and feed
       it through `assembler.add_fasm_line` (parsed via
       `fasm.parse_fasm_string`).
     - If the tile name contains `"HCLK_IOI3"`: synthesize
       `f"{tile}.STEPDOWN"` (bare, tile-level feature, no site) and feed it
       through the same path.
   - Any `FasmLookupError`s from this synthetic-feature pass are batched
     and raised once at the end, same pattern as step 8.
   - **Effect in plain English**: if *any* used IOB in a bank sets a
     `STEPDOWN` feature, every *other*, otherwise-unused IOB33 site in the
     same bank (plus that bank's `HCLK_IOI3` tile) gets the same STEPDOWN
     tag(s) enabled too — because STEPDOWN is a per-bank analog trim that
     must be consistently configured across the whole bank even for pins
     the design doesn't otherwise use. Confirmed against the miniature test
     fixture `f4pga-xc-fasm/tests/test_data/iob/liob_stepdown.fasm`:
     ```
     LIOB33_X0Y1.IOB_Y0.SOMETHING.IN
     LIOB33_X0Y1.IOB_Y0.SOMETHING.STEPDOWN
     RIOB33_X43Y1.IOB_Y1.SOMETHING.OUT
     ```
     (only `LIOB33_X0Y1.IOB_Y0` explicitly sets STEPDOWN; the test's
     `.bits` golden file — not inspected byte-for-byte here, but the test
     `test_stepdown_1`/`test_stepdown_2` in `tests/test_fasm2frames.py:186-192`
     exists precisely to check the propagation reaches `RIOB33_X43Y1`'s
     *other* Y-site).
10. **`get_frames(sparse=False)`** (`prjxray/fasm_assembler.py:47-68`):
    - `sparse=False` (default): start from `frames_init()` — **every**
      frame of **every** tile in the whole grid, zero-filled
      (`for bits_info in grid.iter_all_frames(): for coli in
      range(bits_info.bits.frames): init_frame_at_address(frames,
      base_address+coli)` — this is O(whole part), i.e. the non-sparse
      output always has one entry per frame address that exists anywhere
      in the tilegrid, e.g. tens of thousands of frames for a real part).
    - `sparse=True`: start from `{}`, then zero-init only the frames in
      `self.frames_in_use` (populated by steps 3/7 above — every bus that
      had *any* bit touched, or every tile inside an ROI). **"Sparse" does
      not mean "only frames with a nonzero word"** — it means "only frames
      belonging to a tile-bus that was touched at all", still zero-filled
      in full (all `FRAME_WORD_COUNT` words) for that bus. A completely
      untouched tile contributes zero frames to sparse output; a
      partially-touched tile (one bit set) contributes **all** its bus's
      frames, still mostly zero.
    - Then, regardless of `sparse`, every `(frame_addr, word_addr,
      bit_index) -> is_set` entry in `self.frames` is applied:
      `init_frame_at_address(frames, frame_addr)` (defensive — ensures the
      frame exists even if it wasn't already, e.g. a bit set on a frame
      whose bus wasn't "in use" for some reason) then `if is_set:
      frames[frame_addr][word_addr] |= 1 << bit_index` (bits already
      default to 0, so only `is_set=1` entries do anything — explicit
      `frame_clear` calls are a no-op here, their only effect was the
      conflict-check in step 4). Returns `dict[frame_addr -> list[FRAME_WORD_COUNT
      ints]]`.
    - `xc_fasm/fasm2frames.py:62` prints a warning (not an error) to
      stderr, `f"get_frames: invalid word address {word_addr}..."` /
      `f"...invalid frame address {frame_addr:x8}"`, if a stored key
      somehow has `word_addr >= 101` or an address not in `frames` — dead
      code in practice for the reasons in step 4 above, but note the
      `{frame_addr:x8}` format spec is itself a bug (should be `:08x`;
      Python accepts `x8` as "hex, min-width 8 via the wrong flag order"
      and it silently does **not** zero-pad — reproduce the *behavior*
      byte-for-byte only if `fasm-xilinx`'s CLI needs identical stderr
      output for diff-testing, otherwise fix it).

### 5.1 `.frm` text format

Writer: `dump_frm(f, frames)` (`xc_fasm/fasm2frames.py:74-79` /
`prjuray/utils/fasm2frames.py:70-75`, identical): iterate `sorted(frames.keys())`
(numeric ascending frame address order), one line per frame:
```
0x%08X <word0>,<word1>,...,<wordN>\n
```
where each `<wordK>` is `0x%08X` (Series7: N=100, i.e. exactly 101 comma-
separated `0x`-prefixed 8-hex-digit words; prjuray's own `.frm` writer is
architecture-agnostic — it just writes `len(words)` words, whatever that
list's length is for the loaded architecture). No trailing comma; `\n`
after each line; file ends after the last frame's line (no trailing blank
line beyond the final `\n`).

Reader (C++, used by `xc7frames2bit`/`xcframes2bit`):
`Frames<ArchType>::readFrames` (`lib/xilinx/frames.h:53-109`, identical
in `prjuray-tools/lib/include/prjxray/xilinx/frames.h`): skip lines
starting with `#` (comment support the Python writer never emits but the
reader tolerates); split on the first space into `<addr> <csv-words>`;
`addr = std::stoul(addr_str, nullptr, 16)`; split the CSV on `,`; **for
every non-Spartan6 architecture, if the parsed word count doesn't exactly
equal `ArchType::words_per_frame`, the whole line is skipped with a stderr
warning** (`"Frame <hex>: found <n> words instead of <words_per_frame>"`,
`lib/xilinx/frames.h:79-90`) — i.e. a malformed `.frm` line is silently
dropped, not a hard error. Each word is `std::stoul(val, nullptr, 16)`.
After parsing a frame's words, `updateECC(frame_data)` is called
immediately (**the `.frm` reader always recomputes and overwrites the ECC
word(s) before storing the frame** — see §6; this means the ECC word(s) in
a hand-written `.frm` file are ignored/replaced, not validated).


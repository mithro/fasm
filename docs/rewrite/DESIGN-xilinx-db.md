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
UltraScale+), not plain UltraScale**; see §9. (T6.3 checked upstream: `zynqusp`
with the two xczu3eg parts is all there is, §8.12.)

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
50*32 + word_bit` lands in `[1600, 1631]`, i.e. word 50 always. **Word 50
is also the exact word Series7's per-frame ECC lives in** (`kECCFrameNumber
= 0x32 = 50`, §6.5) — every HCLK-row tile's segbits therefore necessarily
share a word with the ECC value. §6.5 verifies, across every real
artix7 tile type whose bus can reach word 50 (not just the three HCLK
types), that no segbit ever touches the 13 low bits `updateECC` reserves;
this is a database-content fact, not something the addressing math itself
guarantees, so it is re-verified there per-architecture rather than
asserted here.

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
| tilegrid `bits` → segbit → bit position | `prjxray/tile_segbits.py:161-167`, `prjxray/fasm_assembler.py:128-138` | `prjuray-tools/prjuray/tile_segbits.py` (verified structurally identical to prjxray's — same function names/line shapes), `prjuray/utils/fasm_assembler.py` (in the `prjuray` repo, not `prjuray-tools` — not separately re-derived, uses the same `prjuray.bitstream.WORD_SIZE_BITS`) |
| next-frame-address iteration (row/column/minor rollover, used by frame padding, §6) | `lib/xilinx/xc7series/{part,global_clock_region,configuration_row,configuration_bus,configuration_column}.cc` | `lib/xilinx/xcupseries/{part,configuration_row,configuration_bus,configuration_column}.cc` (same shape, no `global_clock_region` level — `Part::rows_` is a flat `std::map<unsigned int, Row>`, `lib/include/prjxray/xilinx/xcupseries/part.h:44-45`) |

## 5. FASM → frames algorithm

This is `prjxray.fasm_assembler.FasmAssembler` (`prjxray/fasm_assembler.py`,
byte-identical in `prjuray/utils/fasm_assembler.py` — in the `prjuray`
repo, not `prjuray-tools` — modulo the import of a local `bitstream`
module and the loss of the `word_addr >= 101` sanity print — see the diff
notes inline below) driven by
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
   prjuray fork's copy of `frame_set`/`frame_clear`
   (`prjuray/utils/fasm_assembler.py:84-120`, in the `prjuray` repo, not
   `prjuray-tools` — has no such guard at all). If the key was already set
   to a *different* value by an earlier
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

## 6. Frames → bitstream (`xc7frames2bit` / `xcframes2bit`) and the reader

### 6.1 `.bit` file header

TLV-ish format, `BitstreamWriter<ArchType>::create_header`
(`lib/xilinx/bitstream_writer.cc` template body,
`lib/include/prjxray/xilinx/bitstream_writer.h:200-249`; identical in
`prjuray-tools`), byte sequence:
```
00 09 0f f0 0f f0 0f f0 0f f0 00 00 01 'a'      # fixed 14-byte preamble
<u16 len><"<frames_file>;Generator=<generator_name>" NUL>   # field 'a' payload, big-endian u16 length INCLUDES the NUL
'b' <u16 len><part_name NUL>                     # field 'b': part name
'c' <u16 len><"YYYY/MM/DD" NUL>                  # field 'c': build date, UTC, via absl::FormatTime("%E4Y/%m/%d", ...)
'd' <u16 len><"HH:MM:SS" NUL>                    # field 'd': build time, UTC
'e' 00 00 00 00                                  # field 'e': 4-byte placeholder for the data length, PATCHED after writing
```
(`lib/include/prjxray/xilinx/bitstream_writer.h:205-247` — quoted almost
verbatim: `bit_header{0x0,0x9,0x0f,0xf0,0x0f,0xf0,0x0f,0xf0,0x0f,0xf0,0x00,0x00,0x01,'a'}`,
then field `a`'s payload is `frames_file_name + ";Generator=" + generator_name`
with length `size()+1` (the `+1` accounts for the trailing `0x0` NUL the
code appends after the string bytes), field `b` is `part_name` (also
NUL-terminated, length+1), fields `c`/`d` are the date/time strings). Field
`'e'`'s 4 zero bytes are **overwritten** after the whole configuration
stream is written: `writeBitstream` records the file offset right after
the header (`end_of_header_pos`), writes all config words, then seeks back
4 bytes before that position and writes the big-endian `u32` byte length of
everything written after the header (`lib/include/prjxray/xilinx/bitstream_writer.h:160-197`,
`length_of_data = out_file.tellp() - end_of_header_pos`). For
`xc7frames2bit`, `part_name` is `FLAGS_part_name` (verbatim CLI value, no
validation against `part.idcode`), `frames_file_name` is `FLAGS_frm_file`
(verbatim path string), `generator_name` is the hardcoded string
`"xc7frames2bit"` (`tools/xc7frames2bit.cc:77`). **`fasm-xilinx`'s
`xc7frames2bit`-compatible CLI must byte-match this header** including the
NUL terminators and the `+1`-inclusive length prefixes, but the
date/time fields inherently make a byte-for-byte bitstream diff
non-reproducible run-to-run — differential tests must mask/ignore bytes 14
onward through the end of field `'d'`'s payload (or compare everything
except that span).

After the header comes the **sync word preamble + configuration packet
stream**, written 1 `ArchType::WordType` at a time, **big-endian**, via
`BitstreamWriter<ArchType>::iterator` walking a fixed per-architecture
`header_` word list *then* every `ConfigurationPacket`'s header word (via
`packet2header`) followed by its data words:
```cpp
// lib/xilinx/bitstream_writer.cc
BitstreamWriter<Series7>::header_{0xFFFFFFFF ×8, 0x000000BB, 0x11220044,
                                   0xFFFFFFFF, 0xFFFFFFFF, 0xAA995566};
BitstreamWriter<UltraScale>::header_{0xFFFFFFFF, 0x000000BB, 0x11220044,
                                       0xFFFFFFFF, 0xFFFFFFFF, 0xAA995566};
BitstreamWriter<UltraScalePlus>::header_{0xFFFFFFFF ×16, 0x000000BB, 0x11220044,
                                           0xFFFFFFFF, 0xFFFFFFFF, 0xAA995566};
```
(word counts: Series7 13 words, UltraScale 6 words, UltraScalePlus 21 words
— all end in the same 5-word tail `BB 11220044 FFFFFFFF FFFFFFFF AA995566`;
the `0xAA995566` word is UG470's documented sync word, matched at read time
by `BitstreamReader<ArchType>::kSyncWord = {0xAA,0x99,0x55,0x66}` as raw
**bytes**, `lib/include/prjxray/xilinx/bitstream_reader.h:172-176` — i.e.
the *reader* only looks for the 4-byte sync pattern anywhere in the byte
stream and discards everything before+including it, so the exact
leading-`0xFFFFFFFF` padding word count the writer chose is not
round-trip-checked). **Note the values shown here are what the prjuray
`architectures.h`/`bitstream_writer.cc` shim has for UltraScale/+ — since
this file was not further specialized per xcuseries/xcupseries beyond
`words_per_frame` and `header_`, and is otherwise identical to the plain
prjxray version already read, no separate prjuray-tools copy was diffed
line-by-line for this table; treat it as verified for prjxray's own
UltraScale support (which is what `xc7frames2bit --architecture
UltraScale...` and prjuray-tools' `xcframes2bit` both implement, since the
tool itself is shared/near-identical in both repos).**

### 6.2 Configuration packet format (Type1/Type2)

`ConfigurationPacket<ConfigRegType>::InitWithWords` for
`Series7ConfigurationRegister` (`lib/xilinx/configuration_packet.cc:98-167`),
header word layout (big-endian 32-bit, matches UG470 pg. 108):

| Bits | Type1 | Type2 |
|---|---|---|
| 31:29 | header_type (`0`=NONE/pad, `1`=TYPE1, `2`=TYPE2) | same |
| 28:27 | opcode (`0`=NOP,`1`=Read,`2`=Write) | same |
| 26:13 | register address (14 bits, `ConfigurationRegister` enum) | *(none — inherits previous Type1 packet's address, `previous_packet->address()`)* |
| 10:0 | data word count (11 bits) | *(n/a)* |
| 26:0 | *(n/a)* | data word count (27 bits) |

Register addresses (Series7, shared by UltraScale/UltraScalePlus — no
override in either `xcuseries`/`xcupseries`), `lib/include/prjxray/xilinx/configuration_register.h:61-83`:
`CRC=0x00, FAR=0x01, FDRI=0x02, FDRO=0x03, CMD=0x04, CTL0=0x05, MASK=0x06,
STAT=0x07, LOUT=0x08, COR0=0x09, MFWR=0x0a, CBC=0x0b, IDCODE=0x0c,
AXSS=0x0d, COR1=0x0e, WBSTAR=0x10, TIMER=0x11, UNKNOWN=0x13, BOOTSTS=0x16,
CTL1=0x18, BSPI=0x1F`. Commands (`Command` enum,
`lib/include/prjxray/xilinx/xc7series/command.h:17-37`): `NOP=0x0, WCFG=0x1,
MFW=0x2, LFRM=0x3, RCFG=0x4, START=0x5, RCAP=0x6, RCRC=0x7, AGHIGH=0x8,
SWITCH=0x9, GRESTORE=0xA, SHUTDOWN=0xB, GCAPTURE=0xC, DESYNC=0xD, IPROG=0xF,
CRCC=0x10, LTIMER=0x11, BSPI_READ=0x12, FALL_EDGE=0x13`.

A **Type0** header (`header_type == 0`, i.e. top 3 bits all zero) is
treated as inert padding — consumes exactly one word, yields a synthetic
NOP packet, never emitted by the writer (only tolerated by the reader for
`BITSTREAM.GENERAL.DEBUGBITSTREAM`-style padding, `lib/xilinx/configuration_packet.cc:111-122`).

### 6.3 Configuration packet **sequence** (the full programming sequence)

This is exact, copy-pasteable from `lib/xilinx/configuration.cc`'s
`Configuration<ArchType>::createConfigurationPackage` template
specializations (`lib/xilinx/configuration.cc:300-470` for Series7,
`:472-631` for UltraScale, `:633-792` for UltraScalePlus — all three read
in full this session). Below, `Write(REG, [data...])` denotes a Type1 write
packet (`ConfigurationPacketWithPayload<N, ConfigurationRegister>`), `NOP`
a bare NOP packet, and `FDRI-DATA` the Type1-header-with-zero-length +
Type2-header-with-payload pair that carries the actual frame words.

**Series7**:
```
NOP
Write(TIMER, [0x0])
Write(WBSTAR, [0x0])
Write(CMD, [NOP])
NOP
Write(CMD, [RCRC])
NOP; NOP
Write(UNKNOWN, [0x0])
Write(COR0, [ConfigurationOptions0Value: AddPipelineStageForDoneIn=1,
             ReleaseDonePinAtStartupCycle=Phase4,
             StallAtStartupCycleUntilDciMatch=NoWait,
             StallAtStartupCycleUntilMmcmLock=NoWait,
             ReleaseGtsSignalAtStartupCycle=Phase5,
             ReleaseGweSignalAtStartupCycle=Phase6])
Write(COR1, [0x0])
Write(IDCODE, [part.idcode()])
Write(CMD, [SWITCH])
NOP
Write(MASK, [0x401]); Write(CTL0, [0x501])
Write(MASK, [0x0]); Write(CTL1, [0x0])
NOP ×8
Write(FAR, [0x0])
Write(CMD, [WCFG])
NOP
Type1(FDRI, opcode=Write, 0 words)      # "primer" packet, no payload
Type2(FDRI, opcode=Write, <all frame words + zero-frame separators>)
Write(CMD, [RCRC])
NOP; NOP
Write(CMD, [GRESTORE])
NOP
Write(CMD, [LFRM])
NOP ×100
Write(CMD, [START])
NOP
Write(FAR, [0x3be0000])
Write(MASK, [0x501]); Write(CTL0, [0x501])
Write(CMD, [RCRC])
NOP; NOP
Write(CMD, [DESYNC])
NOP ×400
```
(`lib/xilinx/configuration.cc:300-470`; `COR0` construction:
`lib/include/prjxray/xilinx/xc7series/configuration_options_0_value.h`
bit-field setters, values as listed).

**UltraScale**: identical shape with one extra leading `NOP` (two `NOP`s
total before the first `Write(TIMER,...)`, vs. Series7's single leading
`NOP` — verified against `lib/xilinx/configuration.cc:480-481` (two
`NopPacket` emplaces for `UltraScale`) vs. `:308` (one `NopPacket` emplace
for `Series7`)), `Write(COR0, [0x38003fe5])`,
`Write(COR1, [0x400000])` (fixed constants, not built via
`ConfigurationOptions0Value`), `Write(CTL0/MASK)` values `0x1`/`0x101`
instead of Series7's `0x401`/`0x501`, and an **extra `Write(FAR, [0x0])`
right before `Write(UNKNOWN, [0x0])`** that Series7 doesn't have. Otherwise
byte-identical structure (same NOP counts, same finalization sequence
`RCRC→GRESTORE→LFRM→100×NOP→START→FAR 0x3be0000→MASK/CTL0 0x101→RCRC→DESYNC→400×NOP`).
**UltraScalePlus**: byte-identical to the UltraScale sequence just
described (same COR0/COR1 constants, same MASK/CTL0 values, same extra FAR
write) — the only difference between the two is `words_per_frame`
(123 vs 93) and the `FrameAddress` bit layout (§4.2); the packet
*sequence* itself is the same for both. (All three sequences verified by
reading `lib/xilinx/configuration.cc:300-792` end to end this session —
see §1.)

**Frame data payload** (`createType2ConfigurationPacketData`,
`lib/include/prjxray/xilinx/configuration.h:78-105`, shared by all
architectures via the generic template — only Spartan6 has its own
specialization): iterate `frames` (a `std::map<FrameAddress,
vector<word>>`, i.e. **numeric frame-address ascending order** since
`FrameAddress`'s `operator uint32_t` makes the map's default `<` compare
raw addresses) and, for each frame, append its words to the packet data;
**after** each frame, ask `part.GetNextFrameAddress(frame.first)` — if the
*next* frame in address order does not have the same `block_type` **and**
`is_bottom_half_rows` **and** `row` as the current one (i.e. a row/block
boundary is being crossed), insert `words_per_frame * 2` zero words (**two
full zero frames**) as a separator. After the loop, unconditionally append
one more `words_per_frame * 2` zero-word block at the very end. This is the
**"2 dummy/zero frames per row boundary"** rule the task brief asks about
— confirmed exact: it is **2 frames' worth of zero words** (not 2 words),
inserted between consecutive rows/block-types **and** once at the very
end of the whole FDRI payload.

`addMissingFrames` (`lib/include/prjxray/xilinx/frames.h:111-129`): before
building the Type2 payload, `xc7frames2bit`/`xcframes2bit` call this to
walk `part.GetNextFrameAddress` from address 0 through **every valid frame
address the part's `part.yaml`/`part_file` declares** and insert an
all-zero frame for any address missing from the `.frm`-derived map — i.e.
**the final FDRI payload always contains every single frame the part has,
in address order, with zero-fill for anything the `.frm` file didn't
mention** (this happens regardless of whether the `.frm` was produced with
`--sparse` or not — `--sparse` only controls what `fasm2frames.py` writes
to the `.frm` text file; `addMissingFrames` re-densifies it before writing
the actual bitstream). **This means the final `.bit`'s FDRI payload size
is architecture/part-invariant given the same part** — sparse vs. non-sparse
`.frm` inputs to `xc7frames2bit` for the *same part* produce byte-identical
`.bit` output (module the header's timestamp fields), because
`addMissingFrames` fills in exactly what non-sparse `fasm2frames.py` would
have zero-filled anyway.

### 6.4 CRC — **not actually computed by the writer**

Despite `Command::RCRC` (Reset CRC) appearing three times in every packet
sequence above, and `lib/include/prjxray/xilinx/xc7series/crc.h` defining a
full `icap_crc(addr, data, prev)` CRC-32C(Castagnoli)-variant running CRC
function, **`createConfigurationPackage` never calls `icap_crc` and never
writes a `Write(CRC, [...])` packet with a computed value** (confirmed by
reading `configuration.cc:300-792` — no `CRC` register write appears
anywhere in any of the three sequences; only `CMD, [RCRC]` — a *command*,
not a register write, telling the device to reset/skip CRC checking). The
device-side CRC check is effectively disabled by never presenting a CRC
value and repeatedly re-issuing `RCRC`. **`icap_crc` exists in the codebase
only for `bittool`/interactive-ICAP-style tools** (not part of the
`xc7frames2bit` write path checked this session) — `fasm-xilinx`'s
bitstream writer does **not** need to implement CRC computation for
write-compatibility with `xc7frames2bit`'s actual output (just emit the
same `RCRC` command sequence); implement `icap_crc` only if a later task
needs an ICAP-style live-reconfiguration packet builder.

### 6.5 ECC — **is** computed (unlike CRC), per architecture

Series7/UltraScale/UltraScalePlus (all three, `lib/xilinx/frames.cc:15-30`)
share `xc7series::updateECC` (`lib/xilinx/xc7series/ecc.cc`): a single 13-bit
ECC value stored in the **low 13 bits of word index `0x32` = 50** of every
101-word (Series7) — or 123-word (UltraScale) / 93-word (UltraScale+, using
the **plain-prjxray shim's copy** of this algorithm, since that repo does
not special-case ECC per architecture either — see caveat below) — frame:
```cpp
constexpr size_t kECCFrameNumber = 0x32;   // = 50
uint32_t icap_ecc(uint32_t idx, uint32_t data, uint32_t ecc) {
  uint32_t val = idx * 32;
  if (idx > 0x25) val += 0x1360; else if (idx > 0x6) val += 0x1340; else val += 0x1320;
  if (idx == 0x32) data &= 0xFFFFE000;      // mask out the ECC word's own old value
  for (i in 0..32) if (data & 1) ecc ^= val + i, data >>= 1;
  if (idx == 0x64) { /* word 100, last word: fold ecc into a parity bit at position 12 */
    v = ecc & 0xFFF; v ^= v>>8; v ^= v>>4; v ^= v>>2; v ^= v>>1; ecc ^= (v&1) << 12;
  }
  return ecc;
}
void updateECC(vector<uint32_t>& data) {  // called once per whole frame
  data[50] = (data[50] & 0xFFFFE000) | (calculateECC(data) & 0x1FFF);
}
```
(`lib/xilinx/xc7series/ecc.cc:18-65`, exact). **The `updateECC` loop
hard-codes `idx == 0x64` (100) as "last word"**, i.e. this exact algorithm
is only correct for a 101-word frame (Series7) — the plain-`prjxray`
checkout applies it unmodified to UltraScale (123 words) and UltraScale+
(93 words) too (`lib/xilinx/frames.cc:15-30`, all three `Frames<T>::updateECC`
specializations call the same `xc7series::updateECC`), which is **wrong**
for UltraScale+ per the corrected algorithm found in `prjuray-tools` (next
paragraph) — flag as a known bug in the plain-prjxray UltraScale/+ support,
not something to replicate.

**`prjuray-tools/lib/include/prjxray/xilinx/xcupseries/ecc.{h,cc}`** has
the *correct*, architecture-specific UltraScale+ ECC: a **48-bit** ECC
value (not 13-bit) stored across **word 45 (all 32 bits) and the low 16
bits of word 46**:
```cpp
constexpr size_t kECCFrameNumber = 45;
// calculate_us_ecc(word, bit): nibble-expanded offset function, odd-parity
// encoded 12-bit offset expanded to a 48-bit mask, shifted by bit-in-nibble.
uint64_t get_us_ecc(idx, data, ecc) {
  if (idx == 45) data = 0;                 // ECC word itself excluded
  if (idx == 46) data &= 0xFFFF0000;        // only the upper half of word 46 is real data
  for (i in 0..32) if (data&1) ecc ^= calculate_us_ecc(idx, i); data >>= 1;
  return ecc;
}
void updateECC(data) {
  ecc = calculateECC(data);   // fold over ALL words (0..92 for a 93-word frame)
  data[45] = ecc;                                  // low 32 bits
  data[46] = (data[46] & 0xffff0000) | ((ecc>>32) & 0xffff);  // high 16 bits, low half of word 46 preserved
}
bool verifyECC(data) { /* recompute and compare against stored value */ }
```
(`prjuray-tools/lib/xilinx/xcupseries/ecc.cc`, full file read). **No
separate `xcuseries` (plain UltraScale) ECC file exists in the checked-out
`prjuray-tools/lib/include/prjxray/xilinx/xcuseries/` directory beyond
`ecc.h`/`.cc` being *listed*** — not read in full this session; treat plain
UltraScale's ECC algorithm/word-position as **unconfirmed** (§9) and, if it
turns out to reuse `xc7series::updateECC` unmodified (matching its
identical `FrameAddress` layout, §4.2), that would be consistent with the
pattern; a later agent must open
`prjuray-tools/lib/xilinx/xcuseries/ecc.cc` before implementing plain
UltraScale ECC.

**Recommendation for `fasm-xilinx`**: implement ECC per-architecture as a
trait method (`fn update_ecc(&self, frame: &mut [u32])`) with three
concrete implementations — Series7 (word 50, 13-bit, `idx*32 +
{0x1320,0x1340,0x1360}` banding, parity fold at word 100) and
UltraScale+ (word 45+46, 48-bit, `(word+(255-92))<<3|nibble` offset,
odd-parity + nibble-expansion) copied verbatim from the two `.cc` files
quoted above; UltraScale (xcuseries) needs its own file read before coding
— do not assume it matches either.

**No real segbit ever writes into the ECC-reserved bits — verified across
every tile type that can reach the ECC word, not just HCLK.** §4.1 already
notes that HCLK-row tiles (`offset: 50, words: 1`) share word 50 with
Series7's ECC value by construction; the collision-avoidance property that
makes this safe is a fact about the *segbits database contents*, not about
the addressing scheme, so it needed its own check rather than an assumed
argument from the tilegrid shape alone. Scanning the whole `xc7a50t`
tilegrid (`prjxray-db/artix7/xc7a50t/tilegrid.json`) for every `bits`
block whose `[offset, offset+words)` range includes word 50 — not only the
three tile types whose block *starts* at offset 50 — finds **13** tile
types: `HCLK_IOI3`, `HCLK_L`, `HCLK_R` (offset 50, words 1 — the
directly-owning types; `HCLK_L_BOT_UTURN`/`HCLK_R_BOT_UTURN` alias to
`HCLK_L`/`HCLK_R` with `start_offset: 0`, so they reuse the identical
segbits file at the identical effective offset and need no separate
check), `CLK_HROW_BOT_R`/`CLK_HROW_TOP_R` (offset 42, words 18, i.e. words
42–59), `HCLK_CMT`/`HCLK_CMT_L` (offset 45, words 10, i.e. words 45–54),
and `CFG_CENTER_MID`/`GTP_COMMON`/`MONITOR_BOT`/`PCIE_BOT` (offset 0, words
101 — these four span the *entire* frame, trivially including word 50).
Checking each corresponding `segbits_<type>.db` (computing, for every bit
entry, `absolute_word = offset + word_bit // 32` and
`bit_in_word = word_bit % 32`, and keeping only entries where
`absolute_word == 50`) gives, for the minimum `bit_in_word` observed among
hits — i.e. how close any real segbit comes to the 13 reserved low bits
(`0..12`, since `updateECC`'s `data[50] &= 0xFFFFE000` clears exactly
those):

| Tile type | segbits file | min `bit_in_word` at word 50 |
|---|---|---|
| `HCLK_IOI3` | `segbits_hclk_ioi3.db` | 14 |
| `HCLK_L` (+ `HCLK_L_BOT_UTURN` alias) | `segbits_hclk_l.db` | 14 |
| `HCLK_R` (+ `HCLK_R_BOT_UTURN` alias) | `segbits_hclk_r.db` | 14 |
| `CLK_HROW_BOT_R` | `segbits_clk_hrow_bot_r.db` | 14 |
| `CLK_HROW_TOP_R` | `segbits_clk_hrow_top_r.db` | 14 |
| `HCLK_CMT` | `segbits_hclk_cmt.db` | 14 |
| `HCLK_CMT_L` | `segbits_hclk_cmt_l.db` | 14 |
| `GTP_COMMON` | `segbits_gtp_common.db` | 13 (i.e. also clear of `0..12`) |
| `CFG_CENTER_MID` | `segbits_cfg_center_mid.db` | no entry lands on word 50 at all |
| `PCIE_BOT` | `segbits_pcie_bot.db` | no entry lands on word 50 at all |
| `MONITOR_BOT` | *(no `segbits_monitor_bot.db` exists)* | trivially safe — no segbits for this type at all |

**Every single real Series7 segbit that can possibly land on word 50
avoids bits 0–12** (minimum observed `bit_in_word` is 13, everything else
is ≥14) — the reviewer's spot-check of `segbits_hclk_l.db` alone
generalizes to the full set of 10 distinct segbits files (plus one type
with no segbits file, and two alias types that reuse an already-checked
file) that can reach the ECC word in this database.

The same check for **UltraScale+**: `xcupseries::updateECC`'s 48-bit ECC
occupies 32-bit words 45 (fully) and the low 16 bits of word 46. Since
`prjuray`'s Python layer (and the zynqusp tilegrid's `offset`/`words`
fields) work in **16-bit word units** (`WORD_SIZE_BITS = 16`, §4.2), that
48-bit span is exactly three consecutive 16-bit words: `2*45=90`,
`2*45+1=91` (word 45's low and high halves) and `2*46=92` (word 46's low
16 bits — the part `updateECC` actually writes); 16-bit word `93` (word
46's *high* 16 bits) is real data, outside the ECC span — confirmed by
`prjuray/utils/fasm2frames.py:78-88`'s `output_bits` conversion,
`bit32_idx = bit_idx + (word_idx & 0x1) * 16; word32_idx = word_idx >> 1`
(odd 16-bit word index ⇒ upper half of the 32-bit word). Scanning
`prjuray-db/zynqusp/xczu3eg-sfvc784-1-e/tilegrid.json` for every `bits`
block whose `[offset, offset+words)` range includes 16-bit word 90, 91 or
92 finds: `CMT_RIGHT` (offset 84, words 10, i.e. 16-bit words 84–93 —
checked `segbits_cmt_right.db` against words 90–92, **zero hits**);
`PSS_ALTO` (offset 0, words 186 — spans the whole frame, but **no
`segbits_pss_alto.db` exists in this database**, so trivially safe); and
the `RCLK_*` row tiles (`RCLK_INT_L`, `RCLK_INT_R`, `RCLK_CLEM_L`,
`RCLK_CLEM_R`, `RCLK_CLEL_L_L`, `RCLK_BRAM_INTF_L`, `RCLK_BRAM_INTF_TD_L`,
`RCLK_BRAM_INTF_TD_R`, `RCLK_DSP_INTF_L`, `RCLK_DSP_INTF_R`,
`RCLK_DSP_INTF_CLKBUF_L`, `RCLK_HDIO`, `RCLK_AMS_CFGIO`,
`RCLK_XIPHY_OUTER_RIGHT`, `RCLK_INTF_LEFT_TERM_ALTO`, all `offset: 93,
words: 3`, i.e. 16-bit words 93–95) which — unlike Series7's HCLK tiles —
**do not overlap the ECC span at all**: UltraScale+'s horizontal-clock-row
tiles are positioned to start exactly *after* the 3-word ECC block (at the
first free 16-bit word, 93), rather than sitting *on* the ECC word the way
Series7's HCLK tiles do. **No real segbit in either checked-out database
(prjxray-db/artix7 or prjuray-db/zynqusp) ever touches an ECC-reserved
bit.**

**ECC must be computed after every segbit write to a frame has landed.**
This is not just a performance ordering — it is required for correctness:
`updateECC` (both variants) folds over the *current* contents of every
data word in the frame (masking out the ECC word's own old value first,
so it never self-references), so it must run once, after the frame's
final word contents are known, not incrementally per-bit or before the
FASM assembler has finished setting bits into that frame. This matches
where the reference tools actually call it: `Frames<ArchType>::readFrames`
calls `updateECC(frame_data)` immediately after parsing **all** of a
frame's words from one `.frm` line (§5.1) — i.e. after `fasm2frames`/
`FasmAssembler` has already finished writing every bit for that frame
into the word array that got serialized to `.frm`. A `fasm-xilinx`
bitstream writer that instead computed ECC per-bit, or before all FASM
lines for a design had been processed, could compute a stale value if any
later-processed FASM line happens to touch the same frame — the per-frame
"finalize, then compute ECC once" ordering must be preserved exactly.

### 6.6 Bitstream reader (`bitread` / `BitstreamReader<ArchType>`)

`BitstreamReader<ArchType>::InitWithBytes` (`lib/include/prjxray/xilinx/bitstream_reader.h:144-170`):
search the raw byte stream for the 4-byte sync word `AA 99 55 66`
anywhere (not requiring it at a fixed offset — tolerates the header TLV
fields being variable length), discard everything up to and including it,
then reinterpret the remainder as big-endian `ArchType::WordType` (u32 for
Series7/UltraScale/UltraScalePlus, u16 for Spartan6) words via
`make_big_endian_span` (`lib/include/prjxray/big_endian_span.h`, not
separately quoted — a straightforward big-endian word reinterpretation
iterator). Then `begin()`/`end()` give an iterator over
`ConfigurationPacket<ConfRegType>` by repeatedly calling
`ConfigurationPacket::InitWithWords` (§6.2) on the remaining word span,
skipping consumed words each time (`lib/include/prjxray/xilinx/bitstream_reader.h:189-234`).
`Configuration<ArchType>::InitWithPackets` (`lib/include/prjxray/xilinx/configuration.h:231-360`,
the Series7/UltraScale/UltraScalePlus generic template — Spartan6 has its
own `FAR_MAJ`/`FAR_MIN` two-register variant not relevant here) replays the
packet stream as a tiny register-machine: tracks `command_register`,
`frame_address_register`, `mask_register`, `ctl1_register`, and a
`start_new_write` flag; on `Write(IDCODE, [v])`, if `v != part.idcode()`
the whole read **fails** (`return {}`, i.e. `bitread`/`xc7frames2bit`-style
consumers get "Bitstream does not appear to be for this part",
`tools/bitread.cc:96-100`); on `Write(CMD, [1 /*WCFG*/])`, set
`start_new_write = true` (also on `Write(FAR, [addr])` if the
undocumented `ctl1_register` bit 21 is clear — the "PERFRAMECRC" quirk,
`configuration.h:294-311`); on `Write(FDRI, [data...])`, chunk `data` into
`words_per_frame`-sized frames starting at `frame_address_register` (only
latched into `current_frame_address` the *first* time after a
`start_new_write`), auto-incrementing via `part.GetNextFrameAddress` for
each subsequent chunk, and **skipping `2*words_per_frame` words whenever
the next address crosses a row/block-type boundary** (mirroring the
writer's separator insertion, `configuration.h:335-351`) — this is how the
reader correctly re-syncs past the "2 zero frames per row" padding
without needing to specially detect all-zero frames.

`bitread`'s own CLI (`tools/bitread.cc`) layers a "skip word 50's low 13
ECC bits unless `-C`" convention on top when printing/exporting `.bits`
text (`(i != 50 || FLAGS_C)`, `tools/bitread.cc:184,252` — Series7-specific
magic number 50, **not parameterized per architecture in this file**,
another latent 7-series-only assumption a Rust reimplementation should fix
by using the per-architecture ECC word index from §6.5).
`prjuray-tools/tools/bitread.cc` adds an `-E` flag ("Ignore failing frame
ECC verification") not present in plain `prjxray`'s `bitread.cc`, implying
prjuray's `bitread` actively calls `xcupseries::verifyECC` and fails by
default if it doesn't match — worth mirroring in `fasm-xilinx`'s reader for
UltraScale+ specifically.

## 7. Reference test data for unit tests

| Source | Path | License | Contents |
|---|---|---|---|
| f4pga-xc-fasm | `tests/test_data/db/` | Apache-2.0 | Miniature prjxray-db: `mapping/{devices,parts}.yaml` (one fake part `"xc7"`, device `"xc7"`, fabric `"xc7"`); `xc7/{tilegrid.json (255 lines), part.json ({"iobanks":{"99":"X1Y26","66":"X113Y26"}}), package_pins.csv}`; `segbits_{clblm_l(96 lines),hclk_ioi3(2),hclk_l(2),int_l(13),liob33(6),riob33(6)}.db`; `tile_type_{CLBLM_L,HCLK_IOI3,HCLK_L,INT_L,LIOB33,LIOB33_SING,RIOB33,RIOB33_SING}.json`. No `part.yaml`, no `ppips_*`/`mask_*` files (tile types used have none). |
| f4pga-xc-fasm | `tests/test_data/{lut,ff_int_0s,ff_int_op1}.fasm`, `tests/test_data/{lut_int,ff_int}.fasm` + `tests/test_data/{lut_int,ff_int}/{design.bits,top.v}` | Apache-2.0 | FASM inputs + golden `.bits` (bitread-format) outputs for differential testing; e.g. `lut.fasm` sets 13 `ALUT.INIT[N]` bits on `CLBLM_L_X10Y102.SLICEM_X0`; `ff_int_0s.fasm` exercises explicit `= 0` assignment on `FFSYNC`/`LATCH` plus several pseudo-PIP features (`INT_L_X10Y102.BYP_ALT0.EE2END0` etc., all expected to be no-ops) plus `HCLK_L` `ENABLE_BUFFER`/leaf-clock features. |
| f4pga-xc-fasm | `tests/test_data/iob/{liob_stepdown,riob_stepdown}.{fasm,bits}` | Apache-2.0 | STEPDOWN-bank-propagation golden test (§5 step 9); `liob_stepdown.fasm` sets STEPDOWN on one `LIOB33` site and expects it propagated to `RIOB33_X43Y1`'s other site via the shared IO bank. |
| f4pga-xc-fasm | `tests/test_fasm2frames.py` | Apache-2.0 | The Python test harness itself — reusable as a spec for exact expected behavior (`frm2bits`/`bitread2bits` comparison helpers, `test_lut/_int/_ff_int/_ff_int_0s/_stepdown_1/_2`, plus `@unittest.skip`ped cases documenting **known-unimplemented/edge behavior**: `test_ff_int_op1` (omitted-key handling), `test_opkey_enum` (enumerated optional key should be a syntax error), `test_dupkey` (duplicate key detection — **confirms no duplicate-key error exists today**, consistent with §5 step 5), `test_sparse` (sparse vs full equivalence — confirms the semantics in §5 step 10 but is itself skipped, i.e. not CI-verified upstream either). |
| prjxray | `lib/test_data/` | ISC | C++ unit fixtures: `configuration_test.{yaml,bit,debug.bit,perframecrc.bit}` (tiny real Series7 bitstreams + matching `part.yaml`, used by `configuration_test.cc`/`bitstream_reader_test.cc`/`bitstream_writer_test.cc` — good candidates for a Rust reader/writer round-trip test), `{one_entry,one_entry_extra_whitespace,one_entry_missing_bit,one_entry_empty_tag,two_entries}.segbits` + `small_file`/`empty_file` (segbits-parser edge cases for `segbits_file_reader_test.cc`), `ToolsTestData.tar.gz` (not extracted/inspected this session). |
| prjuray-tools | `lib/test_data/` (present per the earlier `find`; not read in detail this session — same directory shape expected as prjxray's) | Apache-2.0 | Not yet inventoried — a later agent should `tar`/list this before relying on it; flagged in §9. |

All licenses above (ISC, Apache-2.0, CC0-1.0) are copy-friendly into this
repo's `tests/`; CC0-1.0 database slices (prjxray-db, prjuray-db) need no
attribution but keeping a short provenance note (repo + commit) in
`tests/corpus/README` is still good practice.

## 8. Proposed Rust data model for `fasm-xilinx`

### 8.1 Core structs

```rust
/// One architecture's fixed shape (word count, address bit layout, ECC).
trait XilinxArchitecture {
    const WORDS_PER_FRAME: usize;         // 101 / 123 / 93
    type FrameAddress: Copy + Ord + Into<u32> + From<u32>;
    fn decompose(addr: u32) -> (BlockType, bool /*bottom*/, u8 /*row*/, u16 /*col*/, u16 /*minor*/);
    fn compose(bt: BlockType, bottom: bool, row: u8, col: u16, minor: u16) -> u32;
    fn update_ecc(frame: &mut [u32; Self::WORDS_PER_FRAME]);
    fn config_register(name: &str) -> Option<ConfigRegister>; // shared Series7 table for all 3
}
struct Series7;      // 101 words, block[25:23] row[21:17]+half[22] col[16:7] minor[6:0]
struct UltraScale;   // 123 words, SAME bit layout as Series7 (verify xcuseries ECC before shipping)
struct UltraScalePlus; // 93 words, block[26:24] row[22:18]+half[23] col[17:8] minor[7:0]

/// Part: idcode + the row/bus/column frame-count tree (from part.json OR part.yaml).
struct Part<A: XilinxArchitecture> {
    idcode: u32,
    // top/bottom -> row -> block_type -> column -> frame_count (Series7/UltraScale)
    // row -> block_type -> column -> frame_count             (UltraScale+/xcupseries: no top/bottom split)
    rows: BTreeMap<(bool /*bottom, always false for xcupseries*/, u8), BTreeMap<BlockType, BTreeMap<u16, u16>>>,
    iobanks: Option<HashMap<u32, String>>,   // absent for prjuray-db today
    _arch: PhantomData<A>,
}
impl<A: XilinxArchitecture> Part<A> {
    fn is_valid_frame_address(&self, addr: A::FrameAddress) -> bool;
    fn next_frame_address(&self, addr: A::FrameAddress) -> Option<A::FrameAddress>;
}

/// One IdString-keyed tile in the grid.
struct TileGrid {
    tiles: HashMap<IdString /*tile instance name*/, TileInfo>,
    loc_index: HashMap<(i32, i32), IdString>,   // grid_x, grid_y -> tile
}
struct TileInfo {
    tile_type: IdString,
    grid_x: i32, grid_y: i32,
    clock_region: Option<IdString>,
    bits: SmallVec<[(BlockType, BitsBlock); 2]>,  // almost always 1-2 entries
    sites: HashMap<IdString, IdString>,           // site name -> site type
    pin_functions: HashMap<IdString, IdString>,
    prohibited_sites: Vec<IdString>,
}
struct BitsBlock {
    base_address: u32, frames: u32, offset: u32, words: u32,
    alias: Option<BitAlias>,
}
struct BitAlias { tile_type: IdString, start_offset: u32, sites: HashMap<IdString, IdString> }

/// Compact per-tile-type segbits table: feature id -> bit list.
/// feature id = IdString of "TILE_TYPE.rest.of.feature" (WITHOUT the "[N]" suffix
/// when the feature is multi-bit-addressed; the address int is a separate key).
struct TileTypeSegbits {
    // exact-name lookup (address == 0 path)
    by_name: HashMap<IdString, (BlockType, Box<[SegBit]>)>,
    // "[N]"-addressed lookup: base feature id -> address -> (block_type, full feature id)
    // (kept as a separate index only to preserve the exact 2-step lookup order from
    // prjxray/tile_segbits.py:169-184 — full segbits are still looked up via `by_name`
    // using the full (with-suffix) feature id once the address resolves it)
    addressed: HashMap<IdString, BTreeMap<u32, (BlockType, IdString)>>,
    ppips: HashMap<IdString, PpipType>,   // always | default | hint — all treated the same by the assembler
}
struct SegBit { word_column: u32, word_bit: u32, is_set: bool }  // word_bit NOT clamped to 0..32, see §3.2/§4.1

/// Frames container: BTreeMap keeps numeric-address order for free (needed by
/// the bitstream writer's row-boundary zero-frame insertion, §6.3).
type Frames<const N: usize> = BTreeMap<u32, [u32; N]>;   // N = Series7:101, UltraScale:123, UltraScalePlus:93

/// Bitstream writer trait, one impl per architecture (packet sequences differ, §6.3).
trait BitstreamWriter<A: XilinxArchitecture> {
    fn write(part: &Part<A>, frames: &Frames<{A::WORDS_PER_FRAME}>,
              part_name: &str, frm_file_name: &str, generator: &str,
              out: &mut impl Write) -> io::Result<()>;
}
/// Bitstream reader: bytes -> Frames, for verification / bit2fasm.
trait BitstreamReader<A: XilinxArchitecture> {
    fn read(part: &Part<A>, bytes: &[u8]) -> Result<Frames<{A::WORDS_PER_FRAME}>, ReadError>;
}
```

`IdString` is the Phase 1 `fasm` crate's interned-string type (per
`docs/rewrite/DESIGN-idstring.md`) — reuse it for tile names, tile types,
site names and feature names throughout `fasm-xilinx` so a fully-loaded
part database (tens of thousands of tiles, hundreds of thousands of
segbits) has no per-string heap allocation beyond the interner's own
tables, matching PLAN.md's "keyed by IdString, no per-feature heap
allocation in the hot path" goal.

### 8.2 Binary cache format (sketch)

Versioned, content-hashed, `mmap`-able single file per `(db_root, part)`:
```
struct CacheHeader {
    magic: [u8; 8],           // b"FASMXDB1"
    format_version: u32,      // bump on any layout change
    source_hash: [u8; 32],    // BLAKE3 (or similar) over: db_root path is NOT hashed,
                               // but the *content* of every file this loader opened
                               // for this part is (tilegrid.json, every segbits/ppips
                               // file for every tile type actually present in the
                               // tilegrid, part.json/.yaml, package_pins.csv,
                               // required_features.fasm if present) — content hash,
                               // not mtimes, so it's correct across git checkouts.
    part_name_len: u32, part_name: [u8],
    // then: fixed-size, mmap-friendly tables (offsets recorded here):
    //   tile_name_interner_blob, tile_table, bits_table,
    //   tile_type_interner_blob, segbits_tables (one per tile type present),
    //   part_frame_tree
}
```
Load path: hash the same file set, compare against `source_hash`; on match,
`mmap` and zero-copy-deserialize (e.g. via `rkyv` or a hand-rolled
`bytemuck`-based layout — pick during T5.3, out of scope for this design
doc beyond the shape above); on mismatch, fall back to the text-file loader
and rewrite the cache. `fasm-db-cache` subcommand (T5.3) exposes
`build`/`verify`/`clear` operations. Because prjuray-db has **no
`mapping/`** directory, the cache key must record which addressing scheme
(fabric-indirected vs. per-part) was used, not just assume prjxray-db's
shape (§2.2, §9).

### 8.3 Differential-test behaviours to cover (T5.9/T6.3)

Derived directly from §5/§6 above — a later differential-test-generator
task should assert Rust output matches the Python/C++ reference for each
of these, using the miniature `f4pga-xc-fasm/tests/test_data/db` fixture
(§7) plus synthetic per-segbit corpora for full artix7:

1. Plain single-bit feature enable/disable (`TAG` bare vs `TAG = 0`).
2. `!`-negated bits in a segbits entry (bit must end up **clear**).
3. Multi-bit `TAG[a:b] = value` — only bits that are 1 in `value` produce
   `enable_feature` calls; an all-zero value produces none (§5 step 2).
4. Multi-bit feature with a gap in the `[N]` indices present in the
   segbits DB (address not found → `FasmLookupError`, batched not immediate).
5. Unknown feature name entirely → `FasmLookupError`, batched, exact
   message format `"Segment DB %s, key %s not found from line '%s'"`.
6. A feature that is a listed `ppips` entry (any of always/default/hint) →
   silently zero bits, no error, tile's bus is **not** marked "in use" by
   this feature alone (only if some other bit on the same bus was touched).
7. Two FASM lines that set the same physical bit to the same value
   (idempotent, no error) vs. to conflicting values (`!` vs plain, or two
   different features overlapping) → `FasmInconsistentBits`, raised
   immediately (not batched), first conflict wins.
8. `--sparse` vs. non-sparse `get_frames` — sparse output must still
   zero-fill *every* frame of a touched tile's bus, and touched-but-not-set
   bits stay 0.
9. `--roi` marks ROI-box tiles' frames in-use even with no FASM feature on
   them; features outside the ROI box are still applied normally (no
   filtering).
10. `--emit_pudc_b_pullup`: synthetic lines only emitted if PUDC_B site
    unused; part with 0 PUDC_B sites → no-op, no crash (fix the upstream
    `assert` for >1 site into a clean error, §9).
11. STEPDOWN bank propagation across `package_pins.csv` + `part.json`
    `iobanks` (miniature fixture's `liob_stepdown`/`riob_stepdown` cases).
12. `required_features.fasm` extra features applied after the main file,
    same conflict-checking semantics (no real fixture available — write a
    synthetic one, §9).
13. `TileSegbitsAlias` bit remapping (HCLK U-turn / `_SING` IOB tiles) —
    needs a synthetic segbits+tilegrid fixture since the miniature f4pga
    fixture has none; use real artix7 data (`prjxray-db/artix7`) for this.
14. `.frm` writer: exact `0x%08X <csv 0x%08X words>\n` format, numeric
    address ascending order.
15. `.frm` reader: word-count mismatch → skip line with warning, not error;
    ECC word always recomputed on read (never trusted from the file).
16. Bitstream writer: exact packet sequence per architecture (§6.3), header
    TLV byte-for-byte (masking timestamp bytes), 2-zero-frame row-boundary
    separators, `addMissingFrames` densification regardless of `--sparse`.
17. Bitstream reader: IDCODE mismatch → clean error (not for the part);
    row-boundary zero-frame skip; per-architecture ECC verification
    (UltraScale+ `verifyECC`, optionally-ignorable via an `-E`-equivalent
    flag).
18. UltraScale/UltraScale+ frame-address bit layout (§4.2) — a synthetic
    round-trip test (`decompose(compose(bt,bottom,row,col,minor)) ==
    (bt,bottom,row,col,minor)`) for the boundary values (`minor=127` for
    Series7/UltraScale, `minor=255` for UltraScale+, `block_type` at the
    shifted bit position for UltraScale+).
19. **ECC-word collision invariant** (§6.5): for every loaded part database
    (every tile type, every segbits entry, for both prjxray-db and
    prjuray-db parts as they become available), assert that no segbit's
    computed `(absolute_word, bit_in_word)` position ever falls inside the
    architecture's ECC-reserved span — Series7/UltraScale: word 50, bits
    0–12; UltraScale+: 32-bit words 45 (all 32 bits) and the low 16 bits
    of word 46 (equivalently, 16-bit words 90–92 in the `prjuray`
    tilegrid's own units). §6.5 verified this holds for every tile type in
    the two databases checked out for this research pass (13 Series7 types
    that can reach word 50, 2 UltraScale+ types that can reach words
    45/46 — one of which has no segbits file at all); the reference Python
    tools never check this themselves (a future prjxray-db/prjuray-db
    update could introduce a colliding segbit without any upstream test
    catching it), so `fasm-xilinx` should run this as a standing
    build-time or CI-time assertion over the full loaded database — not
    just a one-off test against the corpus checked in today — so a
    database update that silently breaks the invariant is caught
    immediately rather than producing a bitstream with a corrupted or
    ECC-clobbered configuration bit.

### 8.4 CLI flags to reproduce (drop-in compatibility, PLAN.md goal)

All argparse/gflags declarations read verbatim this session:

**`fasm2frames` (from `xc_fasm/fasm2frames.py:285-320`, prjuray's
`prjuray/utils/fasm2frames.py:142-166` is the same set minus
`--emit_pudc_b_pullup`, plus `--dump_bits`):**

| Flag | Type | Default | Help (verbatim) |
|---|---|---|---|
| `--db-root` | str | required unless `XRAY_DATABASE_DIR`+`XRAY_DATABASE` env set (then defaults to `$XRAY_DATABASE_DIR/$XRAY_DATABASE`) | `"Database root."` |
| `--part` | str | required unless `XRAY_PART` env set | `"Part name. When not given defaults to XRAY_PART env. var."` (prjuray: `"...URAY_PART env. var."`) |
| `--sparse` | flag (`store_true`) | `False` | `"Don't zero fill all frames"` |
| `--roi` | str | `None` | `"ROI design.json file defining which tiles are within the ROI."` |
| `--emit_pudc_b_pullup` | flag | `False` | `"Emit an IBUF and PULLUP on the PUDC_B pin if unused"` (prjxray only) |
| `--debug` | flag | `False` | `"Print debug dump"` |
| `--dump_bits` | flag | `False` | `"Output in bits format (bit_%08x_%03d_%02d)"` (prjuray only) |
| `fn_in` | positional str | required | `"Input FPGA assembly (.fasm) file"` |
| `fn_out` | positional str, `nargs='?'` | `/dev/stdout` | `"Output FPGA frame (.frm) file"` |

**`xcfasm` (`xc_fasm/xc_fasm.py:29-56`)** — same `--db-root`/`--part`/
`--sparse`/`--roi`/`--emit_pudc_b_pullup`/`--debug` as above, plus:

| Flag | Type | Default | Help |
|---|---|---|---|
| `--part_file` | str | required | `"Part YAML file."` |
| `--frm2bit` | str | `"xc7frames2bit"` | `"xc7frames2bit tool."` |
| `--fn_in` | str (named flag, **not positional** here) | — | `"Input FPGA assembly (.fasm) file"` |
| `--bit_out` | str | — | `"Output FPGA bitstream (.bit) file"` |
| `--frm_out` | str | `None` (→ tempfile if unset) | `"Output FPGA frame (.frm) file"` |

Behavior: builds `.frm` via the same in-process `fasm2frames()` call, then
shells out (`subprocess.check_output`, `shell=True`) to
`"{frm2bit} --frm_file {frm_out} --output_file {bit_out} --part_name {part} --part_file {part_file}"`
— i.e. `xcfasm` itself never links the C++ writer; a Rust `xcfasm`-compatible
binary should call its own in-process bitstream writer instead of shelling
out, but must accept identical flags.

**`xc7frames2bit` (`tools/xc7frames2bit.cc:17-28`, gflags):**

| Flag | Type | Default | Help |
|---|---|---|---|
| `--part_name` | string | `""` | `"Name of the 7-series part"` |
| `--part_file` | string | `""` | `"Definition file for target 7-series part"` |
| `--frm_file` | string | `""` | `"File containing a list of frame deltas to be applied to the base bitstream.  Each line in the file is of the form: <frame_address> <word1>,...,<word101>."` |
| `--output_file` | string | `""` | `"Write bitstream to file"` |
| `--architecture` | string | `"Series7"` | `"Architecture of the provided bitstream"` (accepts `Series7`/`UltraScale`/`UltraScalePlus`/`Spartan6`) |

**`bitread` (`tools/bitread.cc:28-60`, gflags; prjuray-tools' copy is
identical plus one extra flag noted):**

| Flag | Type | Default | Help |
|---|---|---|---|
| `-c` | bool | `false` | `"output '*' for repeating patterns"` |
| `-C` | bool | `false` | `"do not ignore the checksum in each frame"` (prjxray) / `"do not ignore the ECC bits in each frame"` (prjuray) |
| `-f` | int32 | `-1` | `"only dump the specified frame (might be used more than once)"` |
| `-F` | string | `""` | `"<first_frame_address>:<last_frame_address> only dump frame in the specified range"` |
| `-o` | string | `""` | `"write machine-readable output file with config frames"` |
| `-p` | bool | `false` | `"output a binary netpgm image"` |
| `-x` | bool | `false` | `"use format 'bit_%%08x_%%03d_%%02d_t%%d_h%%d_r%%d_c%%d_m%%d'\n..."` (full multi-line help gives field meanings: complete frame id, word index, bit index, decoded block type, top/bottom, row, column, minor) |
| `-y` | bool | `false` | `"use format 'bit_%%08x_%%03d_%%02d'"` |
| `-z` | bool | `false` | `"skip zero frames (frames with all bits cleared) in o"` |
| `--part_file` | string | `""` | `"YAML file describing a Xilinx part"` |
| `--architecture` | string | `"Series7"` | `"Architecture of the provided bitstream"` |
| `--aux` | string | `""` | `"write machine-readable output file with auxiliary bitstream data"` |
| `-E` (prjuray-tools only) | bool | `false` | `"Ignore failing frame ECC verification"` |

Positional: optional single bitfile path arg (else reads stdin).

**`gen_part_base_yaml` (`tools/gen_part_base_yaml.cc:27`, produces
`part.yaml` from a debug bitstream — needed only if `fasm-xilinx` ever
regenerates `part.yaml` itself, not for the assemble/write pipeline):**

| Flag | Type | Default | Help |
|---|---|---|---|
| `-f` | bool | `false` | `"Use FAR registers instead of LOUT ones"` |

Positional: required bitfile path.

**`bit2fasm` (`xc_fasm/bit2fasm.py:69-96`):**

| Flag | Type | Default | Help |
|---|---|---|---|
| `--db-root`, `--part` | — | — | (same as above) |
| `--bits-file` | str | `None` (tempfile) | `"Output filename for bitread output (default: tempfile)"` |
| `--bitread` | str | `"bitread"` | `"Name of part being targetted"` (sic — help text is a copy-paste bug in the upstream tool, reproduce verbatim if byte-diffing `--help` output) |
| `--frame_range` | str | `None` | `"Frame range to use with bitread."` |
| `bit_file` | positional | required | `"Input bitstream file"` |
| `--verbose` | flag | `False` | `"Print lines for unknown tiles and bits"` |
| `--canonical` | flag | `False` | `"Output canonical bitstream."` |
| `--fasm_file` | `FileType('w')` | `sys.stdout` | `"Output FASM file"` |

Behavior: shells out to an external `bitread` binary (`subprocess.check_output`) to
turn the `.bit` into a `.bits` text file, then decodes that with
`prjxray.fasm_disassembler.FasmDisassembler` + `fasm.output.merge_and_sort`
— i.e. `bit2fasm` is a thin composition of `bitread` (C++) + the Python
disassembler; a Rust equivalent should call its own in-process bitstream
reader (§6.6) instead of shelling out, but keep the same flags.

### 8.5 Implementation notes (T5.2)

What the `fasm-xilinx` loader (`rust/fasm-xilinx/src/`) actually does,
where it deviates from the sketch above, and measurements.

**Modules.** `arch.rs` (`Architecture`, `BlockType`, `FrameAddress`,
`segbit_position`, ECC bit spans), `segbits.rs` (`SegBit`, `PpipType`,
`TileSegbits`), `tilegrid.rs` (`Grid`, `Tile`, `BitsBlock`, `BitAlias`),
`part.rs` (`Part` frame tree, `package_pins.csv`, `BanksTilesRegistry`),
`db.rs` (`Database::open`, `lookup_feature`, `check_ecc_invariant`),
`yaml.rs` (YAML subset), `json.rs` (serde helpers), `error.rs` (`DbError`).

**Data model (ready for the T5.3 cache).** Instead of the generic
`XilinxArchitecture` trait / `Part<A>` of §8.1, one `Architecture` enum is
passed where needed (the frame address layout is data, not a type; this
keeps `Database` a single non-generic type that a cache can store). No
`Rc`, no references between tables: every table is a few `Vec`s of plain
`Copy` structs plus `HashMap`s that can be rebuilt from them.

* `Grid`: `tiles: Vec<Tile>` (file order) + flat `bits`, `aliases`,
  `pairs` (sites, pin functions, alias site maps) and `names`
  (prohibited sites) arrays; a `Tile` holds `(start, len)` spans into
  them, its name/type `IdString`s, grid location, clock region and the
  index of its tile type. `by_name` / `by_loc` maps are derived.
* `TileSegbits`: `entries: Vec<SegbitsEntry { feature, block_type,
  start, len }>` over one `bits: Vec<SegBit>` pool; `by_name`
  (`IdString -> entry`), `addressed` (`(base IdString, N) -> entry`) and
  the pseudo PIP map are derived. `SegBit` keeps `word_column`/`word_bit`
  as `u32` (block RAM `word_bit` up to 2204 in artix7) and `is_set`.
* `Part`: architecture, IDCODE, rows sorted by `(bottom, row)`, each with
  buses sorted by block type and `(column, frame_count)` pairs.

**Layout detection and fabric.** `<root>/mapping/` present: prjxray-db;
the fabric is `devices.yaml[parts.yaml[part].device].fabric` exactly like
`get_fabric_for_part` (`part.json` has **no** `fabric` field in
prjxray-db; the task brief's suggestion to read it from there does not
match the data). `<root>/tile_types/` present: prjuray-db, grid from
`<root>/<part>/tilegrid.json`. The architecture is the `part.yaml` tag
namespace (`xc7series` / `xcuseries` / `xcupseries`), else Series7 for
prjxray-db and UltraScale+ for prjuray-db. `part == None` loads only the
tile types.

**Files read.** Tile types are enumerated from the `tile_type_*.json` file
*names* (as prjxray does; the JSON is never parsed); for each,
`segbits_<t>.db`, `segbits_<t>.block_ram.db`, `ppips_<t>.db` are loaded
**eagerly** (all 128 artix7 types, 38 ms). Part files `part.yaml`,
`part.json`, `package_pins.csv`, `required_features.fasm` are all
optional. Never read: `mask_*.db`, `*.origin_info.db`, `tileconn.json`,
`node_wires.json`, `site_type_*.json` (the synthetic test database has an
invalid `.origin_info.db` and a mask file to prove it). Because loading
is eager, a malformed segbits line in *any* tile type fails
`Database::open` (prjxray only fails when that tile type is first used).

**Parsers.**

* JSON: `serde` + `serde_json` (workspace dependencies). `tilegrid.json`
  is streamed through a `serde` visitor straight into the `Grid` (no
  `serde_json::Value` DOM, strings borrowed from the file buffer), so
  there is no need for a hand written streaming parser: 6.3 MiB (xc7a50t)
  in 18 ms, 24.4 MiB (xc7a200t) in 73 ms, 14.3 MiB (xczu3eg) in 51 ms,
  interning included. Errors carry line and column.
* YAML: a ~450 line (plus tests) hand written parser for the subset the files use
  (block mappings, plain/quoted scalars, `!<...>` tags, one line flow
  maps, comments); sequences, anchors, block scalars and multi line flow
  collections are errors with a line number, not guesses. Rationale:
  `serde_yaml` is deprecated/unmaintained, `serde_yml` had soundness and
  maintenance problems, `yaml-rust2` would work but pulls a general YAML
  1.2 implementation for three fixed shaped files. All 88 artix7
  `part.yaml` files parse and equal their `part.json`; the zynqusp ones
  too. The `configuration_ranges` form of `part.yaml` (accepted by the C++
  decoder, never written by `gen_part_base_yaml`) is rejected with a
  clear error.
* CSV (`package_pins.csv`): hand written, columns by header name like
  `csv.DictReader`, RFC 4180 quotes.
* segbits / ppips lines: fields split on any ASCII whitespace (prjxray
  crashes on two spaces), numbers must be ASCII digits; malformed lines
  are `DbError::Segbits { path, line }`. Tags that do not start with
  `<TILE_TYPE>.` can never be looked up (prjxray's key is
  `f"{tile_type}.{feature}"`) and are dropped and counted
  (`TileSegbits::foreign_lines`; zero in the real databases). A repeated
  tag keeps the last bits (Python dict).

**Semantics decided here.**

* §9 item 6: all three C++ block type names (`CLB_IO_CLK`, `BLOCK_RAM`,
  `CFG_CLB`) are accepted in `tilegrid.json` and `part.yaml`; any other
  bus name is an error.
* Duplicate tile names or two tiles at one grid location are errors
  (prjxray: last wins / `assert`). `sites` and `prohibited_sites`
  default to empty (prjuray-db has no `prohibited_sites`).
* prjuray-db `pin_functions` values can be maps (`"SYSMONE4_X0Y0": {"R13":
  "VP", "T12": "VN"}`); they are stored as one `(site, function)` pair per
  entry (package pin names dropped; unused by the pipeline).
* Clock regions `X<n>Y<m>` accept several digits (prjxray's regex allows
  one).
* `part.json` `iobanks` keep file order (a second typed pass, since a
  `serde_json::Value` map is sorted).

**Feature lookup.** Per tile type, the tables are keyed by the
`IdString` of the feature name *within the tile* (the segbits tag minus
`<TILE_TYPE>.`), which is the "two level map keyed by (tile_type,
feature IdString)": `lookup_feature(tile, feature, address)` = tile map
probe -> tile type index -> pseudo PIPs (first, any address) -> exact name
if `address == 0` -> `(base, address)`; for a tile with any aliased bits
block: own pseudo PIPs, then the aliased type's table with the site
renamed through `alias.sites` and `offset - start_offset`. Allocation
free except when an alias actually renames a site. Result:
`FeatureLookup::{PseudoPip, Bits(FeatureBits)}`, where `FeatureBits` has
the tile, the entry, the bits block (for `frames_in_use`), the effective
offset and `positions()`. Errors distinguish `UnknownTile`,
`UnknownTileType`, `UnknownFeature` (Display is prjxray's
`Segment DB %s, key %s not found`, without the line), `MissingBitsBlock`
and `InconsistentAlias`.

Measured (release, `cargo bench -p fasm-xilinx --bench db`, every
segbits entry of every non-alias tile, up to 16 per tile):

| | xc7a50t fabric | xc7a200t | xczu3eg |
|---|---|---|---|
| `lookup_feature` (pre-split tile + feature handles) | 23 ns | 27 ns | 21 ns |
| `lookup_fasm_feature` (whole `TILE.FEATURE` handle, split with `with_str` + two `IdString::lookup`) | 99 ns | 124 ns | 100 ns |
| resolving the whole name alone | 24 ns | 24 ns | 22 ns |
| `IdString::new(remainder)` of a known remainder | 24 ns | 25 ns | 29 ns |

So interning (or looking up) the remainder per FASM line costs about as
much as the lookup itself; `lookup_fasm_feature` at ~100 ns per feature is
~0.1 s per million features, acceptable next to parsing. A further 3x is
available with an additive `IdString` helper that splits off the first
component by manipulating the level fields of the handle (the first
component is exactly level 0, and levels are independent tables), giving
the tile handle and the remainder key without touching strings; not done
in T5.2 (no `rust/fasm` change needed yet), candidate for T8.2.

**Frame addresses.** `FrameAddress(u32)` with `fields(arch)` /
`compose(arch, fields)` and per field accessors; `row_index(arch)` is the
C++ `row()` (includes the half bit for UltraScale/+, whose `part.yaml`
rows are keyed that way). `Part::next_frame_address` is a literal port of
`Part::GetNextFrameAddress` and its row/bus/column helpers (prjxray
`xc7series`, prjuray-tools `xcupseries`), and
`Part::iter_frame_addresses` is `addMissingFrames`' walk (address 0
first, always). Checked against the reference tools: `xc7frames2bit`
with an empty `.frm` then `bitread -o` lists 5408 frames for
xc7a35tcsg324-1 (last `0x00C0017F`) and 24060 for xc7a200tffg1156-1 (last
`0x00C4047F`); the Rust enumeration has the same count, last address and
address sum. `Part::new` rejects rows/columns/frame counts that do not fit
the address fields, so the walk is strictly increasing.

**Bit positions and a finding for T5.4.** `segbit_position` computes
`frame = baseaddr + word_column`, `abs = offset * unit + word_bit`
(`unit` = 32 for Series7, 16 for UltraScale/+), `word = abs / 32`,
`bit = abs % 32`. **Negative `abs` wraps like a Python list index**:
`LIOB33_SING`/`RIOB33_SING` alias tiles have `offset 0, start_offset 2`,
so their `IOB_Y0` bits have `abs < 0`, and prjxray writes
`frame[word_addr]` with `word_addr = abs // 32 = -2`, i.e. word 99. The
f4pga-xc-fasm golden output `liob_stepdown.bits` contains exactly
`bit_00400000_099_03` from `LIOB33_SING_X0Y0.IOB_Y0.SOMETHING.STEPDOWN`
(`riob_stepdown.bits` has `099_02`/`099_03`); the Rust tests reproduce
all four f4pga-xc-fasm golden bit sets (`lut_int`, `ff_int`,
`ff_int_0s`, `liob/riob_stepdown` with the STEPDOWN pass re-done in the
test) from the miniature database. `segbit_absolute_bit` exposes the
unwrapped value. Beyond the frame end (`word >= words_per_frame`) is an
error (`WordOutOfFrame`); prjxray prints `frame_set: invalid word
address` and drops the write. In the real databases this only happens
for the other site's bits of the top `LIOB33_SING`/`LIOI3_SING`
(`offset 99`) alias tiles (2402 (type, bus, offset, bit) combinations on
4 tiles of xc7a35t and of xc7a200t, none on zynqusp).

**ECC invariant (§8.3 item 19).** `Database::check_ecc_invariant()`
checks every (segbits tile type, bus, effective offset) combination used
by the grid against `Architecture::ecc_reserved_bits` (Series7: word 50
bits 0-12; UltraScale: word 60 + low 16 bits of word 61; UltraScale+:
word 45 + low 16 bits of word 46). Results: **no violation** on
xc7a35t (2.32 M bits checked), xc7a200t (2.33 M) and both zynqusp parts
(2.17 M), 9-13 ms each; the synthetic test database has a deliberate
clash that is reported. This also settles §9 item 1 as far as the ECC
*position* goes: `prjuray-tools/lib/xilinx/xcuseries/ecc.cc` (plain
UltraScale) has `kECCFrameNumber = 60` and
`offset = (word + (255 - 122)) << 3 | nib` (a 48-bit ECC in word 60 and
the low half of word 61).

**Load time and memory** (release build, each part in a fresh process;
"first open" starts with an empty interner; RSS is the growth of the
resident set over the open, which includes freed-but-retained parse
buffers; re-open is the best of three with a warm interner):

| part (fabric) | tiles | first open | re-open | tile types + segbits | tilegrid.json | RSS growth | interner heap |
|---|---|---|---|---|---|---|---|
| xc7a35tcsg324-1 (xc7a50t) | 18055 | 91 ms | 60 ms | 37 ms | 18 ms (6.3 MiB) | 23.7 MiB | 5.7 MiB |
| xc7a200tffg1156-1 (xc7a200t) | 69165 | 171 ms | 133 ms | 38 ms | 73 ms (24.4 MiB) | 41.4 MiB | 14.2 MiB |
| xczu3eg-sfvc784-1-e | 66385 | 96 ms | 76 ms | 20 ms | 51 ms (14.3 MiB) | 26.4 MiB | 8.1 MiB |

(For comparison, on the same machine prjxray's Python `Database` +
`grid()` + `get_tile_segbits` of every tile type takes 0.53 s for
xc7a35tcsg324-1 (including the module import), and `grid()` alone 0.68 s for xc7a200tffg1156-1.) The artix7 family has 120544 segbits entries with
156888 bits over 128 tile types; zynqusp 54576 entries, 167508 bits, 158
types.

**Open questions for T5.4.**

1. prjxray keys `FasmAssembler.frames` by the *unwrapped* word
   (`(frame, -2, 3)`), so a wrapped `_SING` bit and a direct bit on word
   99 of the same frame never conflict (`FasmInconsistentBits`) in
   Python; keying by the wrapped position would. Reproduce with
   `segbit_absolute_bit` if exact error parity matters.
2. Error kinds: prjxray raises a bare `KeyError` (not `FasmLookupError`)
   for an unknown tile *and* for a tile whose type has no
   `tile_type_*.json`, because `get_tile_segbits_at_tilename` runs before
   the `try:` in `enable_feature` (§5 step 3 says the tile type case is
   caught; it is not). `MissingBitsBlock` (a feature on a bus the tile
   has no `bits` for) is inside the `try` and becomes `FasmLookupError`.
3. `WordOutOfFrame` positions must be dropped with the
   `frame_set: invalid word address` warning (Series7), not rejected.
4. `lookup_fasm_feature` or pre-split: the assembler gets whole
   `IdString` features from the parser; see the lookup numbers above.

### 8.6 Implementation notes (T5.4/T5.5)

What the assembler (`rust/fasm-xilinx/src/{assembler,fasm2frames,frames}.rs`)
and the `fasm2frames` binary (`rust/fasm-cli/src/fasm2frames.rs`,
`src/bin/fasm2frames.rs`) do, where they deviate, and measurements. The
user visible differences are in the `fasm2frames` section of `COMPAT.md`.

**Modules.**

* `frames.rs`: `Frames`, one contiguous `Vec<u32>` of words plus the
  sorted address list; the word count is a run time value
  (`Architecture::words_per_frame`, so T6.2 can reuse it for 123/93 word
  frames). `write_frm` is `dump_frm`; `read_frm` follows
  `Frames::readFrames` (`std::stoul(s, nullptr, 16)` semantics: leading
  space, sign, optional `0x`, trailing garbage ignored, 64-bit overflow is
  an error, truncation to 32 bits; a line split at spaces with only the
  first two fields used; a word count mismatch is skipped with prjxray's
  warning; the first frame of an address wins; `#` lines skipped; an
  empty line is an error because `stoul("")` throws). The reader returns
  the words as read: the ECC recomputation of `readFrames` is left to the
  bitstream writer (T5.6). `diff` and `set_bits` are comparison helpers.
* `assembler.rs`: `FasmAssembler` (`prjxray.fasm_assembler`) and
  `AssemblerError`, whose `python_exception()` names the exception the
  reference raises and whose `Display` is its `str()`.
* `fasm2frames.rs`: `fasm2frames()` (`xc_fasm.fasm2frames.fasm2frames`),
  `find_pudc_b`, `read_roi_design`, `dump_frames_sparse`.

**Bit state.** prjxray's `frames` dict (`(frame, word, bit) -> 0/1`) and
`frames_line` dict are one `HashMap<u64, u32>`: the key packs the frame,
the *unwrapped* word (`absolute_bit.div_euclid(32)`, negative for the
`_SING` alias tiles, clamped to ±2^26, which only merges keys that can
never be written) and the bit; the value is `line index << 1 | is_set`.
This answers open question 1 of §8.5: keys are unwrapped exactly like
prjxray's, so a wrapped `_SING` bit never conflicts with a direct write of
word 99 (tested). `get_frames` applies the Python list wrap (`word + 101`
for a negative word) and reports `IndexError` below `-101`. Frames in use
are kept as `(base_address, frames)` bus blocks and expanded in
`get_frames`; the frame of every stored bit (set *or cleared*) is added
like `init_frame_at_address` in the loop.

**Lines.** Every line given to the assembler is kept (`lines()`), the
parsed file's `Vec<FasmLine>` moved in once; error messages are rendered
from them (`fasm_line_to_string`) only when needed. Keeping the lines is
what makes `set_features` (STEPDOWN, PUDC_B) available without a second
data structure. The common path does not allocate: set bits of the value
are enabled straight from `FeatureValue::iter_set_bits` (not
`canonical_features`, which collects into a `Vec` and iterates over
every address of the range, T1.4b), with `canonical_features`' rules and
asserts (`AssertionError` for a width 1 value other than 1). The review
measured 0.125 allocations per line on average over generated corpora,
almost all on `_SING` alias tiles: `Database::lookup_feature` builds a
string when an alias renames the site, and every bit dropped past the
frame end gets its own warning string (the line text is now rendered
once per feature, not once per bit). Interning, growing the bit map and
the parser's own allocations (annotations, comments) come on top.

**Semantics checked against the oracle** (all covered by
`tests/assembler_mini_db.rs`, the difftest or the CLI test):

1. An unknown tile, or a tile whose type has no `tile_type_*.json`, is a
   `KeyError` raised at once (§8.5 item 2); lookup errors collected before
   it are lost, like in Python. `MissingBitsBlock` and unknown features
   are `FasmLookupError` messages `Segment DB <tilegrid tile type>, key
   <tile type>.<feature> not found from line '<line>'` (the key is the
   feature *without* `[address]`, one message per enabled bit, so a
   missing 6 bit range gives 6 identical messages).
2. A feature with value 0 (bare `= 0` or an all zero range) does nothing:
   no lookup, so no error even for an unknown tile; the feature callback
   still sees it (a `= 0` feature on the PUDC_B site counts as using it).
3. Pseudo PIPs (always/default/hint alike) set no bits and mark nothing
   in use. A feature marks its bus in use if it has any bit, even if all
   its bits are dropped past the frame end (`any_bits` is filled before
   `update_segbit`).
4. Bits past the frame end (§8.5 item 3): dropped with
   `frame_set: invalid word address <word> in line: <line>` (or
   `frame_clear`) on stderr, e.g. `LIOB33_SING_X0Y49.IOB_Y0.*` on
   xc7a35t (`tests/corpus/xilinx/artix7/synthetic/sing_out_of_frame.fasm`).
5. `ff_int_op1.fasm` (upstream's skipped `test_ff_int_op1`) assembles
   without any error in the oracle: it omits `SRUSEDMUX`, so its bits are
   those of `ff_int/design.bits` minus one; it does *not* raise
   `FasmInconsistentBits`. Upstream's skipped `test_sparse` would also
   fail on its size assertion (dense is 3.3 times the sparse text, not 4).
   `test_opkey_enum` expects a syntax error that no parser reports.
6. STEPDOWN: `set_features` includes the extra (ROI, required) and PUDC_B
   lines; an IOB tile that has a STEPDOWN feature but no package pin (an
   unbonded IOB of the fabric, e.g. `LIOB33_X0Y101` on xc7a35tcsg324-1)
   is a `KeyError`; `HCLK_IOI3_<loc>` tiles of `iobanks` get the bare
   `STEPDOWN` feature; a bank's STEPDOWN is added even when every site of
   the bank sets it itself. Python iterates sets here; the Rust order is
   first seen (only visible in error messages).
7. The output file is created before the database is opened; an error
   leaves an empty file (both tools).
8. The reference's ANTLR parser cannot read octal values and misreads
   large decimal values (`Could not decode decimal number`), see the
   parser section of `COMPAT.md`; the synthetic corpus avoids them.

**Deviations** (see `COMPAT.md`): only the last traceback line is
printed; database errors are `fasm_xilinx.DbError` with the Rust loader's
message, reported when the database is opened (the loader is eager); the
STEPDOWN order; `required_features.fasm` in file order; `--debug` with
the `.frm` on stdout is not interleaved like Python's buffers.

**argparse.** `rust/fasm-cli/src/argparse.rs` is now a declarative parser
(`ArgumentParser` of `Argument`s: help, `store_true`, `store`, nullable
`store`, positionals with `nargs=None` or `'?'`, required options,
defaults) shared by `fasm` and `fasm2frames`; it implements
`_match_arguments_partial` (the regular expressions of several
positionals, with Python's backtracking order) and the `part_regexp`
split of the usage (a required `--db-root DB_ROOT` wraps as two parts).
The modules moved into a `fasm_cli` library for the two binaries.

**Measurements** (release build, this machine, the same inputs for both
tools and byte identical output; time and peak RSS from `getrusage`; the
oracle is Python 3.11 with prjxray's lazy per-tile-type segbits loading):

| input | part | oracle | Rust | speed-up |
|---|---|---|---|---|
| counter_test (781 lines), dense | xc7a35tcsg324-1 | 0.37-0.40 s, 59 MiB | 0.09-0.13 s, 32 MiB | 3-4x |
| empty FASM, dense | xc7a35tcsg324-1 | 0.31 s, 52 MiB | 0.09 s, 32 MiB | 3.5x |
| synthetic 1M lines (every segbits feature of the first non alias tiles, conflict free, 36 MB), dense | xc7a200tffg1156-1 | 28.2 s, 2291 MiB | 0.90 s, 234 MiB | 31x |
| same, `--sparse` | xc7a200tffg1156-1 | 27.5 s, 2291 MiB | 0.91 s, 233 MiB | 30x |

For small designs the run is dominated by opening the database (the Rust
loader reads every tile type up front, 90 ms for xc7a35t, 170-190 ms for
xc7a200t, where Python only reads the tile types the design uses), so
the counter_test design is only 3-4x faster than the reference, short of
the 10x target. This is accepted for now: making the database load cheap
is exactly the job of the binary cache of T5.3 (a memory mapped, already
indexed database), which should bring small designs to a few ms plus
the assembly (1.4 ms for counter_test dense). Breakdown of the 1M line run
(`cargo bench -p fasm-xilinx --bench assemble`): open 170 ms, parse
150-170 ms, assemble 326 ms, `get_frames` dense 40-50 ms, `write_frm`
20-25 ms (21.6 MiB).

### 8.7 Implementation notes (T5.6/T5.7)

What the Series7 bitstream writer and reader
(`rust/fasm-xilinx/src/bitstream/`) and the `xc7frames2bit`, `bitread`
and `xcfasm` binaries (`rust/fasm-cli/src/{xc7frames2bit,bitread,
xcfasm,gflags}.rs`, `src/bin/`) do, and measurements. The user visible
differences are in the `xc7frames2bit`/`bitread` and `xcfasm` sections of
`COMPAT.md`.

**Modules** (`fasm_xilinx::bitstream`).

* `ecc`: `icap_ecc` (a literal port of `xc7series::icap_ecc`, checked
  with the vectors of prjxray's `ecc_test.cc`), `frame_ecc` and
  `update_ecc` (`calculateECC`/`updateECC`). `frame_ecc` does not loop
  over the 32 bits of a word: `val = idx * 32 + band` is a multiple of 32,
  so `val + i == val | i`, and the XOR over the set bits is `val` (odd
  number of set bits) XOR the XOR of their indexes, whose bit `k` is the
  parity of `data & M_k` (`M_0 = 0xAAAAAAAA`, ..., `M_4 = 0xFFFF0000`);
  equal to the literal port on random frames (test), 4 ms for the 24060
  frames of xc7a200t.
* `packet`: register and command numbers, Type1/Type2 header words
  (`packet2header`, values masked to their fields), and `PacketIter`,
  `BitstreamReader::iterator` + `ConfigurationPacket::InitWithWords`
  exactly: a Type0 word is a one word NOP, a Type2 packet takes the
  register of the previous *parsed* packet (none: its words are skipped
  without a packet), an incomplete packet or a header type above 2 ends
  the stream.
* `header`: `BitHeader::parse` (the TLV fields, for tests and tools),
  `create_header` (lengths include the NUL and are truncated to 16 bits
  like the reference's `uint8_t` casts), `utc_date_time` (`%E4Y/%m/%d`,
  `%H:%M:%S` in UTC; civil from days, no time zone library).
* `writer`: `fdri_payload` (`addMissingFrames` as a merge of the part's
  address walk with the given frames, frames outside the part kept at
  their address; the ECC recomputed for every frame, which is what
  `readFrames` does for the `.frm` frames and a no-op for the added zero
  frames; two zero frames after every frame whose
  `Part::next_frame_address` is in another block type, half or row; two
  at the end), `configuration_words` (the 13 sync words and the packet
  sequence of §6.3; `COR0` is `0x02003FE5`, checked against the golden
  bitstream), `bitstream_bytes` / `write_bitstream` (the header, then the
  words big-endian, the data length in field `e` computed up front
  instead of seeking back). `BitstreamOptions { design_name, generator,
  part_name, date, time }`: `None` date/time is the current UTC time.
* `reader`: `BitstreamReader::from_bytes` (the first `AA 99 55 66` at any
  byte offset; trailing bytes that do not make a word are dropped where
  the reference aborts), `Configuration::from_packets`
  (`InitWithPackets` literally: `MASK`, `CTL1 & MASK`, `CMD` (`WCFG`
  starts a write), `IDCODE` (mismatch: `ReadError::IdcodeMismatch`),
  `FAR` (restarts the write if `CMD` is `WCFG` and `CTL1` bit 21 is
  clear), `FDRI` cut into 101 word frames that follow
  `next_frame_address` and skip 202 words at row changes; the last frame
  of a packet can be short, a later write of an address replaces it),
  `Configuration::to_frames(clear_ecc, skip_zero)` (bit -> `Frames` ->
  `.frm`).

Only Series7 was implemented in T5.6; T6.2 added UltraScale and
UltraScale+, in prjxray's and in prjuray-tools' variants
(`BitstreamFormat`, §8.10).

**Reference behaviour found on the way.**

1. `Part::next_frame_address` of T5.2 returned `None` for a minor beyond
   its column; `ConfigurationColumn::GetNextFrameAddress` returns nothing
   there and `ConfigurationBus` then tries the next column. Only visible
   for `.frm` frames outside the part (the writer asks for the next
   address of every frame to place the zero frames); fixed and tested.
2. `ArchitectureFactory::create_architecture` returns a default
   constructed variant, i.e. Series7, for an unknown `--architecture`.
3. `Frames::readFrames` asserts a non empty file name, but the oracle is
   a release build: `--frm_file=` (or none) is `Unable to open frm file:`.
   A directory opens and reads as an empty file. An empty line is
   `std::stoul("")`: an uncaught `std::invalid_argument`, SIGABRT; the
   Rust binary prints the same `terminate called ...` text and calls
   `std::process::abort()`.
4. `writeBitstream` reports a `.bit` it cannot create with `Failed to
   write bitstream` but `xc7frames2bit` still exits with 0.
5. `bitread` reads stdin unless there is exactly one positional argument
   (two file names read stdin); `-c` is defined but unused; the `-p` image
   is ported literally; the `-z` test compares with a 101 word zero
   vector, so a short frame is never "zero".
6. `xcfasm` formats Python's `None` into its shell command: without
   `--bit_out` the bitstream goes to a file named `None`; without
   `--fn_in` it fails in the ANTLR wrapper with `TypeError: encoding
   without a string argument` (after opening the database).
7. The reference `part.yaml` decoder also accepts `configuration_ranges`
   (a sequence of `[begin, end)` frame address ranges, used by prjxray's
   own test data): the YAML subset of T5.2 now parses block sequences
   (`- item`, `- !<tag>` with a nested mapping, `- key: value`) and
   `Part::from_yaml_str` builds the rows with `Part::from_frame_addresses`
   (the C++ `Part(idcode, addresses)` constructor).

**gflags.** `rust/fasm-cli/src/gflags.rs` ports `ParseNewCommandLineFlags`
(permutation of non flags, `--`, `SplitArgumentLocked` with `-nox`,
`FlagValue::ParseFrom`, the per flag error map printed in name order
after the help flags were handled), `HandleCommandLineHelpFlags` and
`DescribeOneFlag`'s line breaking, `--undefok`, `--fromenv`,
`--tryfromenv`; flags are listed by (file, name) like `GetAllFlags`, the
gflags flags under `third_party/gflags/src/*.cc` and the tools' under
`tools/<tool>.cc` (relative paths: the reference prints its absolute
build paths). `--flagfile` is an error and `--tab_completion_word` is
ignored (COMPAT.md). The `.bit` header time can be fixed with
`SOURCE_DATE_EPOCH` (all three tools).

**xcfasm.** `fasm2frames.rs` now exposes the pieces `xcfasm` reuses
(`build_frames`: open the database and assemble with the reference's
error texts; `create_output`, `write_frm_file`); `xcfasm` then writes the
bitstream in process with the `xc7frames2bit` code and turns its failures
into the reference's `subprocess.CalledProcessError: Command '...'
returned non-zero exit status 1.` line. `--frm2bit` is ignored; without
`--frm_out` nothing is written to a temporary file and the header names
the FASM file.

**Tests.** Unit tests: ports of prjxray's `ecc_test`, `frames_test`
(`FillInMissingFrames`), `configuration_test` (single frame,
autoincrement, padding frames), the packet sequence, a random frames ->
bit -> frames round trip on a synthetic part (sparse and dense input give
the same bit; rewriting the read frames gives the same bit), a reader
fuzz test (mutated and random bitstreams: no panic; also through the
`bitread` command line in every output mode), packet parsing and
fuzzing, the header and dates, gflags (with a fuzz test). The random
round trip also runs on every frame of xc7a35tcsg324-1. Integration tests
(`rust/fasm-xilinx/tests/bitstream_real_db.rs`,
`rust/fasm-cli/tests/xilinx_tools.rs`, skipped without the database):
`smoke_x1y0.frm` -> byte for byte `smoke_x1y0.bit` (header fields taken
from the golden file), `bitread -z -y` of it = `smoke_x1y0.bitread.txt`,
bit -> frm -> bit, the counter design's dense, sparse and PUDC `.frm`
identical to the reference `xc7frames2bit` (run when built), dense =
sparse bitstreams, prjxray's `configuration_test` bitstreams (normal,
debug, per frame CRC) read to equal configurations, `xcfasm` on the smoke
FASM. Differential: `make xilinx-difftest` (87 fasm2frames runs + 264
xc7frames2bit/bitread runs on their `.frm` files, 45 xcfasm runs, 6
reference bitstreams x 11 bitread flag sets: all identical), and the CLI
tests `tests/cli/test_{xc7frames2bit,bitread,xcfasm}_compat.py` (63 + 57
+ 57 cases). After review: `bitread` streams its output (peak RSS 22
MiB for `-x -o` on a dense random xc7a200t bitstream, 340 MiB of text;
the reference 68 MiB) and flushes stdout where the reference's
`std::endl` does; size-0 inputs, unseekable `.bit` outputs and
directory part files behave like the reference (COMPAT.md).

**Measurements** (release, this machine; best of 3 runs of the binary,
including reading the `.frm` text; `cargo bench -p fasm-xilinx --bench
bitstream` for the library phases):

| input | reference `xc7frames2bit` | Rust | reference `bitread -z -y -o` | Rust |
|---|---|---|---|---|
| xc7a200tffg1156-1, dense `.frm` of an empty design (20230 frames, 24060 written) | 0.25-0.26 s | 0.12-0.14 s | 0.05 s | 0.02 s |
| same frames with 30% random words | 0.27-0.28 s | 0.16 s | 1.87-1.97 s | 0.55 s |
| `smoke_x1y0.frm` (sparse, xc7a35t) | 0.03 s | 0.006 s | 0.015 s | 0.005 s |

Library phases for xc7a200t, 24060 frames (zero / random): `Frames::read_frm`
of the 25.7 MiB text 94 / 113 ms (the bulk of the binary's time), ECC 4 /
13 ms, `bitstream_bytes` (payload, ECC, packets, 9.3 MiB of bytes) 8 /
17 ms, reading the bitstream back to frames 5 / 6 ms, `write_frm` 40 ms.
The `.bit` writing itself is far below the 0.5 s target; `bitread`'s
bit lines are formatted by hand (3.4x faster than `format!`).

### 8.8 Implementation notes (T5.3)

The binary cache of an opened part (`rust/fasm-xilinx/src/cache/`:
`mod.rs` API, header, validation and I/O; `format.rs` byte layout and
payload encoder/decoder; `sources.rs` source file set and fingerprints;
`tests.rs`), the `fasm-db-cache` tool (`rust/fasm-cli/src/db_cache.rs`,
`src/bin/fasm-db-cache.rs`) and where they deviate from the sketch of
§8.2.

**API.** `Database::open` is unchanged (no cache).
`Database::open_cached(db_root, part, &CacheOptions)` returns a database
equal (`==`, every field including the derived indexes; `Database`,
`Grid`, `TileSegbits`, ... now derive `PartialEq`) to what `open`
returns, and exactly `open`'s errors: a cache problem is never an error.
`cache::open` also returns a `CacheOutcome` (`Disabled`, `Hit { restat
}`, `Rebuilt { reason, write_error }`). `CacheOptions::from_env()`:

| variable | meaning |
|---|---|
| `FASM_XDB_CACHE` | cache directory; `0` or empty disables the cache; unset: `$XDG_CACHE_HOME/fasm/db` (if absolute), else `$HOME/.cache/fasm/db`, else disabled |
| `FASM_XDB_CACHE_VERBOSE` | non-empty and not `0`: hits (with the time of each step), rebuilds and their reason, write errors on stderr |

The task brief named the directory variable `FASM_DB_CACHE`, but that
name already means the directory of the *text* databases fetched by
`tools/fetch-db.sh` (read by the tests, benches, `tools/difftest-xilinx.py`
and `tests/oracle/xilinx-env.sh`): reusing it would have mixed cache
files into the fetched databases and made `FASM_DB_CACHE=0` break every
database lookup, hence `FASM_XDB_CACHE` (after the `FASM XDB` magic).
`fasm2frames` and `xcfasm` read it through `fasm2frames::Environment`
(new field `db_cache`, disabled in `Environment::default()`, so the
in-process tests do not touch the user's cache); there is no command line
flag, the command lines stay the reference ones. `xc7frames2bit` and
`bitread` never open a database. `open_cached` without a part is `open`.

**File.** `<dir>/<layout>-<part>-<h>-v<format>.fasmxdb`, `h` = first 8
bytes (hex) of the BLAKE3 hash of the canonical database root (characters
of the part outside `[A-Za-z0-9._-]` become `_`). Layout (all little
endian):

```
prefix (88 bytes): magic "FASMXDB1", format_version u32 (1), header_len u32,
                   payload_len u64, BLAKE3(header) [32], BLAKE3(payload) [32]
header:  loader fingerprint, crate version, layout, architecture, part,
         canonical db root, creation time, BLAKE3 of the tile type name
         list, source records (path, kind, size, stat fingerprint, BLAKE3)
payload: section table (count; kind u8 + length u64 each), then the
         sections: 4 groups of consecutive tile types (balanced by entry
         count), the grid, the part data
section: two string tables (count, u32 lengths, UTF-8 blob), then
         fixed size records referring to strings by index
```

Records mirror the loader's flat tables one to one (§8.5): tile types
(name, `TileTypeFiles` bits, `foreign_lines`, segbits entries 13 bytes,
bits 9 bytes, pseudo PIPs, the `addressed` map as `(base, address,
entry)`), grid tiles (65 bytes: name, type, location, clock region,
type index, the four spans), bits blocks, aliases, pairs, prohibited
site names, and the part data (frame tree rows/buses/columns, IDCODE,
`iobanks`, package pins, required features). The derived indexes are
rebuilt on load exactly as the loader builds them: `by_name` /
`ppip_index` of every tile type, the grid's `by_name` / `by_loc`,
`tile_type_index`, the `BanksTilesRegistry`; `PartInfo::directory` and
`Database::root` come from the `db_root` argument, like `open`. Every
index, span and enum value is bounds checked while decoding, so even a
crafted file with valid hashes can only produce an error (tested by
decoding every one-byte mutation and every truncation of a payload).

**Deviation from §8.2: no `mmap`, no zero copy.** Names are `IdString`s
of the process global interner, whose handle values depend on the
interning order, so the cache stores the names as text and interns them
on load; the crate is also `#![forbid(unsafe_code)]`. The serialisation
is hand written (a small `Writer`/`Reader` pair): `postcard`/`bincode`
would need `serde` derives on the internal types and change nothing about
the interning, `rkyv`'s zero copy does not apply to interned handles.
The only new dependency is `blake3` (workspace dependency, 1.8): content
hashes of the sources and integrity hashes of header and payload.
BLAKE3 was chosen over SHA-256 (`sha2`: more dependencies, 3-10x slower
without SHA-NI) and over a hand-rolled 64-bit hash (a content hash must
detect *any* change; the payload hash is on the load path, BLAKE3 does
14.9 MiB in 3 ms here with its SIMD code, which falls back to Rust
intrinsics without a C compiler). The loader fingerprint is FNV-1a 64
over `src/**` computed by `build.rs` (a change detector of our own
sources, not an adversarial setting: a collision would need two builds
whose sources hash alike), so a cache file written by a build with
different loader or cache code is rebuilt; `FORMAT_VERSION` is in the file name and header, so
builds with different layouts do not overwrite each other's files.

**Validation.** On load: the prefix (magic, version, lengths against the
file size) and the header hash; the header's loader fingerprint, part,
canonical root and layout; the layout detection and the tile type list
(the family directory is listed again, as the loader does; the set of
files the loader reads is a function of the layout, this list, the
fabric (itself from the recorded `mapping/*.yaml`) and the part); then
every recorded source:

* `Content` (tilegrid.json, `mapping/parts.yaml`, `mapping/devices.yaml`,
  `part.yaml`, `part.json`, `package_pins.csv`, `required_features.fasm`,
  every `segbits_*.db`, `segbits_*.block_ram.db`, `ppips_*.db` that
  exists): must still be a regular file of the same size; if its stat
  fingerprint (Unix: device, inode, `mtime` and `ctime` with nanoseconds)
  is the recorded one it is trusted, otherwise it is hashed and compared
  with the recorded BLAKE3 hash. Same content: the cache is used and its
  header rewritten with the new fingerprints (atomically, reusing the
  payload bytes), e.g. after a fresh checkout of the database.
* `Absent` (every probed file that did not exist): must still not be a
  regular file. `Present` (`mask_*.db`, only probed): must still exist.

Stat fingerprints younger than 5 s when taken are not recorded (the
"racy git" problem: with coarse timestamps a file rewritten right after
it was fingerprinted can keep size and times); those files are hashed on
every load until one finds them old enough. Without Unix metadata (no
inode, no `ctime`) no stat fingerprint is recorded at all: every load
hashes the sources (about 5-10 ms for 12-30 MiB). `fasm-db-cache verify`
(`cache::verify_file`) always hashes every source and also checks the
payload hash, the file name and that the payload decodes. Any mismatch,
unreadable or truncated file, wrong magic/version/fingerprint or corrupt
payload means: load the text files, write a new cache file. Writing
(`build_from_text`) stats the sources *before* the text load, hashes them
on a second thread *during* the load, and stats them (and lists the tile
types, re-derives the fabric) again *after* it; if anything differs the
file is not written. That alone is not enough: the loader and the hashing
thread read each file at different moments, and on a file system with
coarse timestamps (1 s on ext3) a same size rewrite within the same
second leaves both stats equal, so the recorded hash could be of other
bytes than the tables (the review reproduced this on an ext3 image with a
concurrent writer; later loads then re-hash, match, and serve stale
tables). So the file is also not written when any `Content` source's
modification or status change time (from either stat) is less than 5 s
before the build started, or later (`sources::changed_recently`; without
Unix stats, the modification time); the first open of a database changed
in the last 5 s just loads the text files, the next one writes the cache.
Files are written to `.<name>.<pid>.<nanos>.tmp` in the cache
directory and renamed; any error (read-only or missing directory, full
disk) is reported only with `FASM_XDB_CACHE_VERBOSE` and never fails the
tool.

**Loading.** The file is read with 4 positional reads in parallel (the
time is the page faults of the fresh 9-15 MiB buffer, which the kernel
serves concurrently), then the payload hash, the source check and the
decoding run concurrently (the decoded database is dropped if a check
fails), and the sections are decoded on scoped threads; the grid section
builds `by_loc` from the raw records on its own thread from the start and
the tiles/`by_name` on another as soon as the tile names are interned,
while the site names are interned. Files and payloads under 1 MiB (the
test databases) are handled on the calling thread, and so is any work
for which the operating system refuses a thread (`cache/task.rs`:
`std::thread::Scope::spawn` would panic); a worker that panics is a
cache error (rebuild, or no write), never a failure of the tool.

**Measurements** (release, this machine: 4 cores, Firecracker VM;
`cargo bench -p fasm-xilinx --bench db`, each open in a fresh process
with an empty interner, best of 3):

| part (fabric) | text files (first open) | cache hit | cache file | text load + cache write |
|---|---|---|---|---|
| xc7a35tcsg324-1 (xc7a50t) | 102-138 ms | 23 ms | 9.0 MiB, 519 sources (12.5 MiB) | 125-149 ms |
| xc7a200tffg1156-1 (xc7a200t) | 176-196 ms | 40-42 ms | 14.9 MiB, 519 sources (30.7 MiB) | 251-283 ms |
| xczu3eg-sfvc784-1-e | 100-124 ms | 24-25 ms | 10.0 MiB, 637 sources (17.4 MiB) | 140-187 ms |

`fasm2frames` wall time (binary, best of 7, output written to a file):

| input | oracle | no cache (`FASM_XDB_CACHE=0`) | first run (writes the cache) | cache hit |
|---|---|---|---|---|
| counter_test, xc7a35tcsg324-1, dense | 441 ms | 94-101 ms | 147-161 ms | 31-34 ms |
| counter_test, `--sparse` | | 87-95 ms | 127 ms | 23-27 ms |
| empty FASM, xc7a35tcsg324-1, dense | | 95 ms | 129-138 ms | 29-31 ms |
| empty FASM, xc7a200tffg1156-1, dense (25 MiB `.frm`) | | 199-206 ms | 300-331 ms | 70-75 ms |

So counter_test is now 14x faster than the reference (3-4x before), and
a cache hit costs about a fifth of a text load. It does not reach the
"few ms" hoped for in §8.6, and the reasons are measured: of the 23 ms
for xc7a35t (40 ms for xc7a200t), most is interning the 98 k (195 k)
distinct names (tile, site, feature and base names) into the global
`IdString` interner, which costs ~170 ns per *new* string here (~45 ns
for a known one; inserting from several threads into the same level
table is slower than from one, so the grid's 130 k level-0 names are
interned serially), and first-touch page faults, which cost ~1.8 us per
4 KiB page in this VM (6.5 ms per 14 MiB, several times bare metal) for
the file buffer, the interner tables and the database itself (~40 MiB
for xc7a200t). The rest (reading 2-4 ms, payload hash 2-3 ms, source
stats ~1 ms for ~520 paths, hash map rebuilding) runs concurrently. The
next steps, if needed, are outside this task: a bulk insertion API in
the interner (one lock per shard and one reservation per batch, T8.2),
or making the database lazy (per tile type segbits, or a grid that
interns names on first lookup), which changes `Database`'s data model.

Writing the cache costs 25-60% on top of the text load, once per part
and database change (encoding ~35 ms for xc7a200t, the grid section's
string table dominates; the source hashing is hidden behind the load).

**Tests.** `src/cache/tests.rs` (22, plus 1 in `task.rs`): every field round trips
(`mini-db`, `synthetic-db`, with and without a part, sequential and
parallel decoding), second open is a hit, `open_cached == open` and the
same errors (unknown part, unknown device, not a database, missing
root), wrong magic / version / truncation / extension, *every* bit 0 and
bit 7 flip of a cache file is rejected before decoding, end to end flips
(all of the prefix, a sample of header and payload) are rebuilt, every
one-byte mutation and truncation of a payload with valid hashes decodes
to an error or a database, never a panic; changed sources: size change,
same size with the modification time restored, tilegrid, an absent file
appearing, a mask file or a part file disappearing, a new tile type, the
part moving to another fabric; touched files are re-hashed and the
header updated, not rebuilt; header identity (loader fingerprint, part,
root, layout, tile type list, recorded size); `verify_file` (and
`CacheOptions::verify_contents`) hashes even when the stat fingerprints
match; unwritable cache directory; 8 threads
opening concurrently (one file, no temporary file left); two copies of a
database get two files, another spelling of a root the same one; a
synthetic prjuray-db layout; `build`/`clear`/`cache_files`/
`family_parts`; the environment; the racy window, and no cache file
written for sources changed within the window (explicit one hour window
on a fresh copy; then written with old enough timestamps). `tests/cache_real_db.rs`
(skipped without the databases): xc7a35tcsg324-1, xc7a200tffg1156-1 and
xczu3eg-sfvc784-1-e round trip and verify. `rust/fasm-cli/tests/db_cache.rs`:
`fasm2frames` (mini-db: 6 FASM files x 3 flag sets; synthetic-db
including an unknown feature and an unknown part) and `xcfasm`
(synthetic-db; artix7 counter_test dense/sparse/xcfasm when the database
is present) give identical output files, stdout, stderr and exit codes
with `FASM_XDB_CACHE=0`, when writing and when loading the cache, and
the last run is checked to be a hit; `fasm-db-cache` commands and exit
codes. `make xilinx-difftest` with `FASM_XDB_CACHE` set (cache written by
the first runs, 4 parallel jobs): 107 fasm2frames runs, 60 xcfasm runs
and 6 reference bitstreams x 11 bitread flag sets, all identical.

**`fasm-db-cache`.** `fasm-db-cache [--cache-dir DIR] COMMAND`: `build
DB_ROOT PART...` / `build --all DB_ROOT` (the keys of
`mapping/parts.yaml`, or every prjuray-db directory with a
`tilegrid.json`), `verify [FILE...]`, `info [FILE...]` (header and every
source record), `list`, `clear` (cache files and leftover temporary
files). The directory defaults to the `FASM_XDB_CACHE` rules (`verify`
and `info` with explicit files need none). Exit codes: 0 success, 1 a
failed build or verification, unreadable file, or no cache directory
(disabled, or `HOME` unset: the message says which), 2 usage error. Plain hand-written argument parsing (no Python
counterpart to be compatible with).

**Limitations.** One file per part repeats the family's segbits tables
(about 6 MiB of each artix7 file), so `build --all` for the 88 artix7
parts takes about 1 GiB; there is no size limit or eviction (`clear`).
In the parallel path the payload is decoded while its hash is checked,
so the names of a *corrupt* file may be interned (leaked) before it is
rejected. A cache file is trusted like the database it was built from:
anyone who can write the cache directory can make the tools use other
tables (the default directory is per user). The loader fingerprint also
changes on edits that do not change the loader's results (one rebuild).
On network file systems the stat fast path is only as good as the
client's attribute cache (NFS may report stale sizes and times for a few
seconds after another client writes) and inode numbers may not be stable
across remounts (then files are just re-hashed); use
`CacheOptions::verify_contents` / `fasm-db-cache verify` where that
matters.

### 8.9 All-parts differential testing (T5.9)

`tools/gen-xilinx-corpus.py` (generator), `tools/difftest-xilinx.py
--family/--families` (comparison), `make xilinx-difftest-all`,
`tests/cli/test_xilinx_corpus.py` (fast CI check). Usage and disk layout:
`tests/oracle/README.md`, "All-parts differential test".

**What is generated.** For one part of a prjxray-db family (stdlib only,
deterministic for given `--tiles`, `--seed`, `--density`,
`--max-per-tile`):

* The generator reads what prjxray reads (the fabric from
  `mapping/{parts,devices}.yaml`, `<fabric>/tilegrid.json`, the tile types
  from the `tile_type_*.json` names, `segbits_<t>.db`,
  `segbits_<t>.block_ram.db`, `ppips_<t>.db`, the part's `part.json`
  `iobanks`, `package_pins.csv` and `required_features.fasm`) and runs a
  Python model of prjxray's lookup (`TileSegbits.feature_to_bits`: pseudo
  PIPs first, the exact name only for address 0, then
  `feature_addresses`; `TileSegbitsAlias`: the aliased type's tables,
  sites renamed, offset minus `start_offset`, the alias tile's own pseudo
  PIPs) for every segbits key and pseudo PIP of every tile type (and
  alias) of the grid, including the aliased type's pseudo PIPs under the
  alias tile's names (e.g. `LIOB33_SING_*.IOB_DIFFI_IN0.IOB_PADOUT1` from
  `ppips_liob33.db`: no bits on either side). The tiles of a type with
  the same alias form a *group* (the bottom and top `_SING` IOB tiles of
  a clock region are two groups of one type: `start_offset` 2 or 0,
  different sites). Each result is a *unit* (`NAME` or `NAME[n]`) with
  its bits relative to the tile. Keys that no FASM feature reaches are
  listed in the manifest (`unreachable`: a `NAME[0]` shadowed by a plain
  `NAME`, a key of another type, a site the alias map renames away): none
  in the four families.
* Units are placed on tiles of their type with a global bit map keyed
  like prjxray's `frames` dict (frame, *unwrapped* word, bit): a unit fits
  if none of its bits is already stored with the other value (bits past
  the frame end, which prjxray drops, do not count). `--tiles first`: the
  tiles in `tilegrid.json` order, each unit on the first tile where it
  fits; `sample N` (default 3): N tiles per type, one random tile in each
  of N equal slices of the type's tile list, each first given a random
  conflict free subset (each unit with probability `--density`, default
  0.5), then the units not placed yet go on the first of those (then of
  further random tiles) where they fit; `all`: every tile gets a random
  subset (8.07 M lines for xc7a35t). Units that conflict on every tile of
  their type (exclusive options of types with few tiles, e.g. the
  `CLK_BUFG_*` muxes or the `DRIVE`/`IOSTANDARD` options of the `*_SING`
  IOB tiles) go to further files: `features-2.fasm`, ... (fresh bit map
  each), so **every reachable unit of every group of the part is set at
  least once**, except the STEPDOWN units below, over 5 to 22 files per
  part. The generator asserts this at the end: per group, the distinct
  units placed over all files plus those listed in `uncovered` are all
  its units (manifest `coverage`; the tests check it too).
* The part's `required_features.fasm` (zynq7) is stored first in every
  file's bit map. The PUDC_B tile is kept free (so `--emit_pudc_b_pullup`
  adds its pull-up). STEPDOWN: each group with a STEPDOWN feature gets
  a *bonded* host tile (a STEPDOWN feature of a tile without IO bank is a
  `KeyError` in `fasm2frames.py`), in as few banks as possible; the other
  tiles of those banks are kept free, and the model replays
  `fasm2frames.py`'s propagation (unused IOB sites of the banks get every
  tag of the bank, `HCLK_IOI3_<loc>` gets `STEPDOWN`, with and without the
  PUDC_B site in use) and moves any generated unit that would conflict
  with it to the next file. Groups without a bonded tile cannot have
  their STEPDOWN units placed (listed in `uncovered`, 4 units per part):
  both `RIOB33_SING` groups on xc7a35tcpg236 and xc7a50tcpg236 (4 speed
  grades each) and xc7z010clg225 (3), both `LIOB33_SING` groups on
  xc7z020clg400 (3): 14 parts. (Generator version 1 chose one STEPDOWN
  host per tile type, so the other `_SING` group's STEPDOWN units were
  neither placed nor listed: 2-6 units on 111 parts, found by the review;
  fixed in version 2.)
* Lines: plain units as `F`, `F = 1` or `F = 1'b1`; the addresses of a
  multi bit feature placed on a tile as ranges `F[hi:lo] = value` whose
  set bits are those addresses (0 bits are never looked up, so ranges may
  include unplaced addresses and up to two past the last one), in the
  formats `W'hX`, `'hx`, `W'bB`, `W'dD` (below 2^32), plain decimal (below
  2^31) and `W'oO` (at most 30 bits, i.e. 10 octal digits), which the
  reference's ANTLR parser decodes correctly (see "Parser" in
  `COMPAT.md`), single addresses as `F[n]`, `F[n] = 1` or
  `F[n:n] = 1'b1`; two `= 0` disables per tile (`F = 0`,
  `F[n+1:n] = 2'b00`) of units not placed there (no lookup); annotations
  (`{ src = "src12", net = "net7" }`), end of line comments, comment and
  blank lines. Block RAM `INIT_xx`/`INITP_xx` (`BLOCK_RAM` bus, up to 256
  bit ranges) and the `_SING` tiles' wrapped (negative word) and dropped
  (past the frame end, `frame_set: invalid word address` on stderr) bits
  are included.
* `errors/`: `lookup_errors.fasm` (unknown features, addresses past the
  last one, a range with 1 bits past it, a gap address, a suffix on a
  real feature, a feature of a bits block the tile lacks when a part has
  one: batched `FasmLookupError`, every message in order),
  `absent_tile.fasm` (a tile of a family type the part does not have, or
  `X999Y999`, after lookup errors: `KeyError`, the earlier errors lost),
  `value_range.fasm` (the ANTLR value range error, rule 4),
  `inconsistent.fasm` (the first real conflict the packing met:
  `FasmInconsistentBits`), `stepdown_unbonded.fasm` (`KeyError`).
* `manifest.json`: options, files, per tile type counts, `uncovered`,
  `unreachable`, the STEPDOWN banks and hosts, PUDC_B. `--expected-frm`
  also writes the sparse `.frm` the model predicts for each features file.

Default sizes (`sample 3`, density 0.5; generator version 2): 43-62 k
lines per part (52 k lines, 11 features files and 5 error files for
xc7a35tcsg324-1), 6.71 M lines and 1752 files over the 125 parts
(version 1: 6.60 M lines, 2151 files);
generation takes 2-6 s per part.

**What is compared, per part** (`tools/difftest-xilinx.py --families
artix7,kintex7,spartan7,zynq7 --all-parts`, parts in parallel with
`--jobs`, each part's runs in sequence): `features.fasm` with the dense,
`--sparse`, `--emit_pudc_b_pullup` and `--sparse --debug` flag sets (the
first three also through `xc7frames2bit` and the 11 `bitread` flag sets
on the reference `.bit`), once more `--sparse` with `FASM_XDB_CACHE=0` for
the Rust tool (every other Rust run uses a per part cache directory,
written by the first run and loaded by the others), the other features
files `--sparse` (through `xc7frames2bit` and two `bitread` flag sets),
the error files `--sparse`, and `xcfasm` with its three flag sets: the
`.frm`, `.bit`, `bitread` outputs, stdout, stderr and exit codes byte for
byte, with the normalisation rules of `make xilinx-difftest` (a run where
rule 3 or 4 applied counts as "explained"). The reference results are
cached on disk (key: command line, the contents of every input file, the
reference wrappers, binaries and venv packages, the database commit), so
a rerun only runs the Rust tools; large cached outputs (the `bitread`
dumps) are kept as a SHA-256.

**Run matrix** of the first full run (default options, all 125 parts,
generator version 1): 2151 FASM files, 6.60 M lines; 2651 fasm2frames
runs, 8814 xc7frames2bit/bitread runs and 375 xcfasm runs on each side.
Version 2 (above) has fewer, fuller files (1752 files, 6.71 M lines), so
fewer runs; its full run against the reference (see the second results
block below) is also clean:

| family | parts | files | lines | fasm2frames | bitstream tools | xcfasm |
|---|---|---|---|---|---|---|
| artix7 | 88 | 1484 | 4 854 990 | 1836 | 6096 | 264 |
| kintex7 | 16 | 184 | 687 111 | 248 | 864 | 48 |
| spartan7 | 9 | 213 | 447 912 | 249 | 810 | 27 |
| zynq7 | 12 | 270 | 610 462 | 318 | 1044 | 36 |

**Results** (`make xilinx-difftest-all`, `ORACLE_DIR` the shared oracle
of `tests/oracle/setup-xilinx.sh`, prjxray
`c9f02d8576042325425824647ab5555b1bc77833`, f4pga-xc-fasm
`25dc605c9c0896204f0c3425b52a332034cf5e5c`, prjxray-db
`0a0addedd73e7e4139d52a6d8db4258763e0f1f3`; default corpus, `--jobs 4`,
2026-09-24/25, run by the orchestrator):

* **0 unexplained differences.** fasm2frames: 2651 runs, 2526 identical,
  125 explained, 0 different; xc7frames2bit + bitread: 8814 runs, all
  identical; xcfasm: 375 runs, all identical. 11840 reference runs and
  11840 Rust runs.
* The 125 explained runs are exactly one per part: `errors/value_range.fasm
  [sparse]` (a feature value that does not fit its range, e.g. `F = 2` on
  a one bit feature). This is the value range error already listed in the
  `fasm2frames` section of `COMPAT.md` (rule 4): the reference's ANTLR
  parser fails its assertion inside a ctypes callback and the tool dies
  with `TypeError: 'NoneType' object is not iterable`, the Rust tool
  reports `Exception: Parse error at L:C - value 2 does not fit ...`; both
  exit with 1 and write an empty `.frm`. No other rule 3 or 4 case
  occurred. Rule 1 (the reference's traceback dropped, the last line
  compared exactly) applied 588 times, to every other error file:
  `lookup_errors.fasm` (125, `FasmLookupError` with every message in
  order), `absent_tile.fasm` (125, `KeyError`), `inconsistent.fasm` (125,
  `FasmInconsistentBits`) and `stepdown_unbonded.fasm` (88: the parts that
  have an unbonded IOB tile with a STEPDOWN feature, `KeyError`), plus the
  125 `value_range.fasm` runs. Rule 2 did not apply (no syntax errors in
  the generated corpus).
* No Rust bug and no new reference quirk was found: the reference
  behaviours the corpus reaches (lookup, alias tiles, wrapped and dropped
  `_SING` bits with their `frame_set` warnings, pseudo PIPs, block RAM,
  STEPDOWN propagation, PUDC_B pull-up including kintex7, required
  features of zynq7, sparse zero filling, `--debug`, the bitstream writer
  and reader on 125 parts) were already reproduced.
* **Second full run, generator version 2** (same oracle and database
  commits, default corpus, `--jobs 3` beside other work, 2026-09-25, run
  by the orchestrator): 125 parts, 1752 FASM files, 6 712 737 lines;
  fasm2frames 2252 runs, 2127 identical, 125 explained (again exactly
  `errors/value_range.fasm [sparse]`, one per part), 0 different;
  xc7frames2bit + bitread 7617 runs, all identical; xcfasm 375 runs, all
  identical; 10244 reference and 10244 Rust runs; **0 unexplained
  differences**; wall time 7039 s (117 min) with 3 jobs, all 125 per part
  lines `ok`. So the STEPDOWN units of both `_SING` alias groups and the
  aliased pseudo PIPs added in version 2 are byte identical too.
* **Wall time 4114 s (68.6 min)** with `--jobs 4` for the version 1 run. Per part (generation,
  reference and Rust runs of that part in sequence) 46-271 s, median 91 s,
  mean 130 s (16214 s in total); the Rust side and the generation are
  7-15 s of it, so the reference costs about 40-260 s per part: the
  xc7a200t parts (61 k lines, dense frames of 24 k addresses) take the
  longest, 271 s. The earlier estimate from the §8.6 numbers (15-25 min)
  was low because each part's large file goes through the reference
  `fasm2frames` seven times (four variants, three `xcfasm` runs) and the
  bitstream tools 3 x 12 times. A rerun with the result cache
  (`<work-dir>/results`; the whole work directory, with the generated
  corpora, was 549 MB) only runs the Rust tools: a few minutes. For a quick check, `make xilinx-difftest-quick` (or
  `--parts-sample N`: N parts per family over as many fabrics and
  packages as possible) runs 4 parts in about 2-3 minutes; the harness
  prints an up front estimate (130 s per part) and an ETA after each part.
* `tests/cli/test_xilinx_corpus.py` compares the Rust `fasm2frames` with
  golden reference results for xc7a35tcsg324-1 (`--tiles sample 3`; one
  run per file plus the dense run of `features.fasm`, with and without
  the database cache: for generator version 1, 24 files and 25 runs
  (`features.fasm` dense and sparse, 18 other features files, 5 error
  files); for version 2, 16 files and 17 runs), recorded in
  `tests/corpus/xilinx/artix7/generated/xc7a35tcsg324-1-sample-3-s0.json`
  with the reference commits above. The test fails with the command to
  regenerate the golden file when the generator or the fetched database
  no longer match it.
* The harness was also run over the whole matrix with the Rust tools on
  both sides (`--oracle target/release/fasm2frames ...`): 326 s wall time
  with `--jobs 4` (generation of all corpora included; 7-15 s per part);
  corrupted cache entries (a changed `.frm`, a changed digest of a
  `bitread` dump) are reported as differences.
* Model cross check (no reference involved): the Rust `fasm2frames
  --sparse` output equals the generator model's `--expected-frm` for
  every features file of every part with three generator configurations
  (`sample 3` density 0.5 seed 0: 1563 files; `first`; `sample 5` density
  0.8 seed 7), again with generator version 2 (`sample 3`, 1164 features
  files, dense `--emit_pudc_b_pullup` runs without error too), and for `--tiles all` on xc7a35tcsg324-1 (8.07 M lines, 19
  files); every generated file assembles without error with the Rust
  tool, dense and `--sparse --emit_pudc_b_pullup`, on all 125 parts
  (including kintex7, whose PUDC_B features exist although
  `fasm2frames.py` notes its IOSTANDARD choice is wrong for K70T). No
  Rust bug was found this way.
  `tests/cli/test_gen_xilinx_corpus.py` runs this cross check on the
  test databases of `rust/fasm-xilinx/testdata` (and on xc7a35tcsg324-1
  `--tiles first` when fetched), without reference tools.
* Database facts found on the way: no unreachable segbits key in the four
  families; the most exclusive options per tile type need up to 22 files
  for the parts with few `_SING` IOB tiles.

### 8.10 UltraScale/UltraScale+ bitstreams (T6.2)

What the UltraScale and UltraScale+ bitstream writer and reader
(`rust/fasm-xilinx/src/bitstream/`) and the prjuray tools
(`xcframes2bit`, `uray-bitread`, `uray-fasm2frames`) do, how they differ
from Series7, and measurements. Sources, read in full for this task:
prjuray-tools `f53f07b8fe37721137a57e9bee3b2b13e7676f53`
(`tools/{xcframes2bit,bitread}.cc`, `lib/include/prjxray/xilinx/
{architectures,configuration,frames,bitstream_reader,bitstream_writer,
ecc}.h`, `lib/xilinx/{configuration,frames,bitstream_writer}.cc`,
`lib/xilinx/{xcuseries,xcupseries}/*.cc`, `third_party/gflags`) and
prjuray `c550b03a26b4c4a9c4453353bd642a21f710b3ec`
(`utils/{fasm2frames,fasm2bit,fasm_assembler,util}.py`); the user visible
behaviour is in the `xcframes2bit` / `uray-bitread` and
`uray-fasm2frames` sections of `COMPAT.md`.

**Two implementations of `--architecture=UltraScale(Plus)`.** The plain
prjxray checkout (the `xc7frames2bit` / `bitread` of T5.6) declares
`UltraScale` and `UltraScalePlus` as `Series7` with other
`words_per_frame`, sync headers and packet sequences: Series7 `part.yaml`
types and frame addresses, and `Frames<UltraScale(Plus)>::updateECC` is
`xc7series::updateECC` (on 123 / 93 words: the parity fold at word 100
happens for 123 words and not for 93). prjuray-tools gives both their own
`xcuseries` / `xcupseries` `Part` and `FrameAddress` types and ECC.
`BitstreamFormat` (`writer.rs`) captures both: `architecture` (sync header
and packets), `addressing` (the `Part` type), `words_per_frame` and `ecc`;
`BitstreamFormat::native(arch)` is prjuray-tools' (and prjxray's Series7),
`BitstreamFormat::prjxray(arch)` prjxray's. `bitstream_bytes`,
`configuration_words`, `fdri_payload` and `Configuration::from_packets`
use the part's native format, the `_with` variants take one. The
`xc7frames2bit` and `bitread` binaries use `prjxray(arch)`, `xcframes2bit`
and `uray-bitread` `native(arch)`.

**Differences from Series7** (prjuray-tools; all confirmed byte for byte
against the reference tools, below):

| | Series7 | UltraScale (`xcuseries`) | UltraScale+ (`xcupseries`) |
|---|---|---|---|
| words per frame | 101 | 123 | 93 |
| frame address | block 25:23, half 22, row 21:17, column 16:7, minor 6:0 | the same | block 26:24, half 23, row 22:18, column 17:8, minor 7:0 |
| `FrameAddress::row()` / `part.yaml` rows | row within the half; `global_clock_regions: {top, bottom}` | the half bit is the top bit of `row()`; a flat `rows` map keyed by it (`configuration_ranges` addresses have no `row_half`) | the same |
| part walk (`GetNextFrameAddress`) | next minor, column, row of the half, bottom half, next bus | next minor, column, *next row of the part* (only tried for the current bus: a row without the bus ends that bus's walk, and its later rows are never visited, `part_walk_skips_rows_after_a_row_without_the_bus`), next bus | the same |
| sync words before the packets | 13 (8 x `FFFFFFFF`, `BB`, `11220044`, `FFFFFFFF` x 2, `AA995566`) | 6 (1 x `FFFFFFFF`) | 21 (16 x `FFFFFFFF`) |
| packet sequence | §6.3 | one more leading NOP, `FAR = 0` before `UNKNOWN`, `COR0 = 0x38003FE5`, `COR1 = 0x400000`, `MASK`/`CTL0` `0x1`/`0x101`, final `MASK`/`CTL0` `0x101` | the same as UltraScale |
| frame ECC | 13 bits, low bits of word 50 | 48 bits: word 60 and the low half of word 61 | 48 bits: word 45 and the low half of word 46 |
| `bitread` ECC bits (`is_ecc_bit`, left out of `-x`/`-y` unless `-C`) | word 50 bits 0-12 | word 60, word 61 bits 0-15 | 16-bit words 90-92 = word 45, word 46 bits 0-15 |
| `bitread` hex dump | word 50 masked with `0xFFFFE000` unless `-C` | not masked | not masked |
| zero frame padding, `FDRI` layout, reader register machine, `.bit` header, no CRC | §6.3-§6.6 | the same | the same |

The UltraScale(+) ECC (`xcu(p)series/ecc.cc`, `calculate_us_ecc`): every
set bit `i` of word `w` (the ECC words masked out: the first one fully,
the second one's low half) XORs a 48-bit value into the ECC: the 11-bit
offset `(w + 255 - last_word) << 3 | i / 4` (`last_word` 122 / 92) gets
an odd parity bit 11, each of its 12 bits becomes one bit per nibble (bit
`k` -> bit `4k`), shifted left by `i % 4`. `Ecc` (`ecc.rs`) keeps a
table of the 48-bit values per (word, bit) and walks the set bits
(`trailing_zeros`); a literal port checks it on random frames, and real
Vivado frames of both architectures verify. §9 item 1 is resolved:
UltraScale is neither Series7's algorithm nor at UltraScale+'s position.
`verifyECC` compares the stored ECC with the computed one
(`Ecc::verify`); Series7's `verifyECC` compares 13 stored bits with the
whole computed value (fine for 101 words, never true for most 123-word
frames of prjxray's UltraScale format, which is why only
`uray-bitread` verifies).

**prjuray-tools' tools, beyond the architecture** (`COMPAT.md`):
`xcframes2bit` validates the `.frm` addresses against the part
(`read_frm_checked`, the C++ `FrameAddress` `operator<<` in the message,
`cpp_frame_address`), aborts on an unknown `--architecture`
(`absl::bad_variant_access`; prjxray's defaults to Series7) and still
writes `Generator=xc7frames2bit`; its gflags is 2.2.2 (`--helpfull`, the
only difference to prjxray's copy); `bitread` verifies the ECC (`-E`
turns the error into a warning on stdout), sizes `-z`'s zero frame per
architecture (prjxray's is always 101 words, so its `-z` never skips an
UltraScale frame), and writes `--aux` with `fseek(-1)` tricks that give
prjxray's text except on unseekable files. Both prjuray tools also
support Spartan6, which is not implemented (`ArchitectureError::
Unsupported`).

**prjuray's `utils/fasm2frames.py`** (`uray-fasm2frames`,
`fasm_xilinx::uray_fasm2frames`): prjuray's `fasm_assembler.py` is
prjxray's without the `word_addr >= 101` check, working in 16-bit words
(`FasmAssembler::set_prjuray`: bits past the frame end are kept and a set
one fails in `get_frames` with `IndexError`; conflict messages give the
16-bit word); `run()` has no IO bank / STEPDOWN / PUDC_B steps and
writes the frames as 186 16-bit words (`write_frm_halfwords`), `.bits`
lines (`write_bits`) or the 16-bit sparse dump (`--debug`,
`dump_frames_sparse_halfwords`). Its `.frm` cannot be read by
`xcframes2bit` (186 words instead of 93); prjuray's `fasm2bit.py`
converts to 32-bit words first, and the Rust `fasm2frames` does the same
for a prjuray-db part (xc_fasm itself cannot open prjuray-db). Its
`--help` raises `TypeError` (argparse formats the `%` of the
`--dump_bits` help).

**Decisions.** The prjxray tools keep their names and behaviour
(`xc7frames2bit`, `bitread`, now with prjxray's UltraScale formats
instead of "not supported"); prjuray-tools' `bitread` is the separate
binary `uray-bitread` (same flags plus `-E`, `Flavor::Prjuray`);
`xcframes2bit` has prjuray's name; `uray-fasm2frames` is prjuray's
`fasm2frames.py`. `xc_fasm` (`xcfasm`) has no UltraScale support (no
`--architecture`, prjxray's `Database`), so `xcfasm` gets none either.

**Reference behaviour found on the way.**

1. The part walk skips the later rows of a bus after a row without that
   bus (table above); the real zynqusp parts have every bus in every row,
   so `iter_frame_addresses` visits `frame_count` frames for both of them
   (tested).
2. gflags' `LOG(WARNING)` is a bare `std::cerr` (no `WARNING: ` prefix,
   no newline): `--helppackage` of a tool whose name matches no flag file
   prints `Unable to find a package for file=<name>` (a Rust bug in the
   T5.6 gflags emulation, fixed).
3. `uray-bitread` aborts (`Span::at failed bounds check`) when it verifies
   a frame shorter than its ECC words (an `FDRI` write whose length is not
   a multiple of the frame size) and loses its buffered output.
4. `uray-fasm2frames --help` fails with `TypeError: %x format: an integer
   is required, not dict`; prjuray's `utils/util.py` imports `jinja2`,
   which the oracle venv lacks (the oracle wrapper provides a stub).
5. `ToolsTestData.tar.gz`'s Vivado bitstreams (Series7, UltraScale,
   UltraScale+) all pass `verifyECC`; bit -> frames -> `xcframes2bit`
   gives the same frames back, not the same bytes (Vivado's packet
   sequences differ from the tools').
6. An `xcu(p)series` `part.yaml` whose values do not fit the frame
   address fields (a row key >= 64, a column >= 1024, an UltraScale+
   `frame_count` > 256) is accepted by prjuray-tools: with
   `frame_count: 300` `xcframes2bit` writes a bitstream and exits 0; with
   row key 64 or column 1024 `addMissingFrames` loops forever (the masked
   address is found valid in row 0 again; timed out at 600 s in the
   review). `Part::new` rejects such parts (`Part file X not found or
   invalid`, exit code 1); the hang is deliberately not reproduced
   (`COMPAT.md`). Series7 is unaffected (prjxray also rejects
   `frame_count: 200`).

**Tests.** Unit tests: the UltraScale(+) ECC (literal port, the
`calculate_us_ecc` comment example, real Vivado frames, `is_ecc_bit`),
the packet sequences of all formats, random frames -> bit -> frames in
the four new formats, the padding and the part walk quirk,
`read_frm_checked`, `configuration_ranges` for `xcupseries`, the 16-bit
writers, the frame address printing, gflags `--helpfull` /
`--helppackage`. `tests/synthetic_usp_db.rs` (the new
`testdata/synthetic-usp-db` fixture: 16-bit offsets, the RCLK tile after
the ECC words, 256-frame `BLOCK_RAM` columns, a bottom half row, bits at
and past the frame end) and `fasm-cli/tests/uray_tools.rs` (the three
tools and `fasm2frames` on it). Real data (skipped without
`FASM_DB_CACHE`): `tests/ultrascale_real_db.rs` assembles one feature in
every 7th tile of both zynqusp parts of prjuray-db
(xczu3eg-sfvc784-1-e, xczu3eg-sbva484-1-e), round trips the bitstream
and compares it with the reference `xcframes2bit` (byte identical), and
checks the UltraScale / UltraScale+ `ToolsTestData` bitstreams (ECC,
round trip, identical to the reference).

**Differential runs** (this machine, 4 cores):

| run | cases | identical | different |
|---|---|---|---|
| `difftest-xilinx.py --prjuray` (2 parts, seed 1, 20 designs + 5 error files per part): `uray-fasm2frames` dense / sparse / debug / dump_bits / ROI | 220 | 220 | 0 |
| ... on the 98 successful dense / sparse / ROI runs: `fasm2frames` (32-bit) = converted oracle `.frm`, `xcframes2bit` `.bit`, `uray-bitread` x 9 flag sets | 1078 | 1078 | 0 |
| `uray-bitread` x 9 flag sets + bit -> frm -> bit round trip on the 5 `ToolsTestData` bitstreams | 5 | 5 | 0 |
| `tests/cli/test_uray_tools_compat.py` (gflags / argparse command lines, malformed inputs, ECC failures, aborts) | 92 | 92 | 0 |
| ToolsTestData by hand: `uray-bitread` 15 flag sets x 5 bitstreams (75), prjxray `bitread` with `--architecture=UltraScale(Plus)` 15 flag sets x 3 inputs (45), bit -> frm -> `xcframes2bit` / `xc7frames2bit` round trips in all 6 formats (6) | 126 | 126 | 0 |
| `difftest-xilinx.py` (prjxray mode, regression check): 107 fasm2frames + 372 bitstream runs, 60 xcfasm, 6 x 11 bitread | all | all | 0 |
| `tests/cli/test_{xc7frames2bit,bitread}_compat.py` (with the new prjxray UltraScale cases) | 128 | 128 | 0 |

**Measurements** (release, best of 3, including process start):

| input | reference | Rust |
|---|---|---|
| `xcframes2bit`, ToolsTestData UltraScale `design.bit` frames (32510 frames, 16 MB `.bit`) | 0.42 s | 0.21 s |
| `xcframes2bit`, ToolsTestData UltraScale+ (14952 frames, 5.6 MB) | 0.15 s | 0.07 s |
| `uray-bitread -z -y -o`, UltraScale / UltraScale+ `design.bit` | 0.14 / 0.058 s | 0.047 / 0.020 s |
| `uray-bitread -x -o`, UltraScale / UltraScale+ | 0.49 / 0.18 s | 0.063 / 0.023 s |
| `uray-fasm2frames`, dense `.frm` of xczu3eg (14898 frames, 30.6 MB) | 0.99 s | 0.33 s (0.29 s with the database cache) |

### 8.11 f4pga-examples (T7.3)

Every Xilinx 7 series example of f4pga-examples (`13f11197`) built with
the f4pga Yosys + VPR flow the examples are written for (toolchain:
`tools/e2e/setup-f4pga.sh`; flow, collection and comparison:
`tools/e2e/run-f4pga-examples.sh`, `compare-f4pga-examples.py`,
`install-f4pga-examples-corpus.py`; details and quirks:
`tools/e2e/README.md`, "f4pga-examples corpus (T7.3)"). The FASM of each
design/board is in the corpus
(`tests/corpus/xilinx/{artix7,zynq7}/designs/f4pga-examples/<design>/<board>/vpr.fasm[.xz]`,
8.1 MB for 30 designs/boards, with `difftest.json` and a README of the
provenance and the sha256 of the flow's FASM, frames and bitstream), so
`make xilinx-difftest` covers it.

**Databases.** The flow's prjxray-db (conda package `prjxray-db
0.0_257_g0a0adde`) is prjxray-db `0a0added`, the commit
`tools/fetch-db.sh` pins, and its database files are identical to the
pinned copy (`diff -r`), so "the flow's db" and "the pinned db" give the
same results by construction; both were run anyway. (This differs from
T7.2's openXC7 snap database, which is another prjxray-db commit.) The
VPR architecture (symbiflow-arch-defs `007d1c1`) only decides which
features VPR can emit.

**What was compared, per design/board** (the columns of the matrix):

* *Same as flow*: the Rust `xcfasm` with the flow's own command line
  (`--sparse --emit_pudc_b_pullup`, flow db) writes the flow's frames
  byte for byte (`top.frm`, from the flow's xcfasm command line rerun
  with `--frm_out`, whose `.bit` is checked to be the flow's but for the
  header's `.frm` path, date and time) and
  the flow's `top.bit` byte for byte except the `.frm` path in the header
  design field (the flow's xcfasm writes its frames to a `mkstemp` file;
  the header time is injected with `SOURCE_DATE_EPOCH`); the Rust
  `fasm2frames` with the same options writes the same frames; the Rust
  `xc7frames2bit` on the flow's frames writes the flow's `.bit` (same
  rule); the Rust `bitread` prints what the flow's `bitread` (prjxray
  `ae546d6b`) prints for the flow's `.bit` with the 11 flag sets of
  `BITREAD_FLAGS`; the Rust `fasm` CLI prints what the flow's `fasm` (PyPI
  0.0.2.post88) prints, with and without `--canonical`.
* *Same with pinned db / oracle*: `fasm2frames` with the pinned database
  writes the flow's frames, the `fasm` CLI matches `tests/oracle/fasm-oracle`.
* Timings: one run of each tool on this machine (4 cores, shared with
  other jobs; the Rust tools with a warm `FASM_XDB_CACHE`); the flow's
  `fasm2frames` is its `xc_fasm.fasm2frames`.

| Design | Board | Part | Built | FASM lines | Build | Same as flow (frm, bit, bitread, fasm) | Same with pinned db / oracle | xcfasm flow / Rust | fasm2frames flow / Rust | fasm --canonical flow / Rust |
|---|---|---|---|---|---|---|---|---|---|---|
| counter_test | arty_35 | xc7a35tcsg324-1 | yes | 803 | 66 s | yes | yes | 0.43 / 0.03 s | 0.38 / 0.02 s | 0.05 / 0.002 s |
| counter_test | arty_100 | xc7a100tcsg324-1 | yes | 837 | 102 s | yes | yes | 0.68 / 0.04 s | 0.63 / 0.03 s | 0.05 / 0.002 s |
| counter_test | nexys4ddr | xc7a100tcsg324-1 | yes | 795 | 107 s | yes | yes | 0.66 / 0.03 s | 0.60 / 0.03 s | 0.05 / 0.002 s |
| counter_test | basys3 | xc7a35tcpg236-1 | yes | 782 | 70 s | yes | yes | 0.45 / 0.03 s | 0.42 / 0.02 s | 0.05 / 0.002 s |
| counter_test | nexys_video | xc7a200tsbg484-1 | **no** (xc7a200t_test not installed) | | | | | | | |
| counter_test | zybo | xc7z010clg400-1 | yes | 7486 | 284 s | yes | yes | 0.49 / 0.04 s | 0.43 / 0.03 s | 0.11 / 0.007 s |
| picosoc_demo | arty_35 | xc7a35tcsg324-1 | yes | 97441 | 232 s | yes | yes | 3.59 / 0.14 s | 3.58 / 0.12 s | 1.17 / 0.066 s |
| picosoc_demo | arty_100 | xc7a100tcsg324-1 | yes | 96951 | 283 s | yes | yes | 3.94 / 0.12 s | 3.79 / 0.13 s | 1.19 / 0.071 s |
| picosoc_demo | nexys4ddr | xc7a100tcsg324-1 | yes | 97848 | 280 s | yes | yes | 3.95 / 0.13 s | 3.86 / 0.14 s | 1.28 / 0.068 s |
| picosoc_demo | basys3 | xc7a35tcpg236-1 | yes | 99269 | 239 s | yes | yes | 3.55 / 0.12 s | 3.84 / 0.12 s | 1.22 / 0.070 s |
| litex_demo_picorv32 | arty_35 | xc7a35tcsg324-1 | yes | 220114 | 413 s | yes | yes | 10.72 / 0.26 s | 10.53 / 0.26 s | 2.89 / 0.164 s |
| litex_demo_picorv32 | arty_100 | xc7a100tcsg324-1 | yes | 217907 | 498 s | yes | yes | 13.25 / 0.25 s | 13.41 / 0.26 s | 2.90 / 0.182 s |
| litex_demo_vexriscv | arty_35 | xc7a35tcsg324-1 | yes | 261029 | 479 s | yes | yes | 12.51 / 0.33 s | 12.23 / 0.29 s | 3.49 / 0.197 s |
| litex_demo_vexriscv | arty_100 | xc7a100tcsg324-1 | yes | 260033 | 530 s | yes | yes | 14.84 / 0.33 s | 14.70 / 0.32 s | 3.53 / 0.211 s |
| linux_litex_demo | arty_35 | xc7a35tcsg324-1 | yes | 344230 | 634 s | yes | yes | 15.07 / 0.42 s | 14.88 / 0.41 s | 4.33 / 0.238 s |
| linux_litex_demo | arty_100 | xc7a100tcsg324-1 | yes | 340958 | 664 s | yes | yes | 17.19 / 0.39 s | 17.55 / 0.37 s | 4.41 / 0.263 s |
| litex_sata_demo | nexys_video | xc7a200tsbg484-1 | **no** (xc7a200t_test not installed) | | | | | | | |
| timer | basys3 | xc7a35tcpg236-1 | yes | 2970 | 72 s | yes | yes | 0.50 / 0.04 s | 0.44 / 0.03 s | 0.09 / 0.004 s |
| pulse_width_led | arty_35 | xc7a35tcsg324-1 | yes | 1834 | 73 s | yes | yes | 0.47 / 0.04 s | 0.43 / 0.03 s | 0.06 / 0.003 s |
| button_controller | basys3 | xc7a35tcpg236-1 | yes | 2997 | 72 s | yes | yes | 0.47 / 0.03 s | 0.45 / 0.03 s | 0.07 / 0.004 s |
| hello_a | arty_35 | xc7a35tcsg324-1 | yes | 76 | 60 s | yes | yes | 0.40 / 0.03 s | 0.36 / 0.02 s | 0.04 / 0.001 s |
| hello_b | arty_35 | xc7a35tcsg324-1 | yes | 209 | 60 s | yes | yes | 0.40 / 0.03 s | 0.36 / 0.02 s | 0.04 / 0.002 s |
| hello_c | arty_35 | xc7a35tcsg324-1 | yes | 209 | 64 s | yes | yes | 0.40 / 0.03 s | 0.37 / 0.02 s | 0.05 / 0.002 s |
| hello_d | arty_35 | xc7a35tcsg324-1 | yes | 255 | 61 s | yes | yes | 0.40 / 0.03 s | 0.35 / 0.02 s | 0.04 / 0.002 s |
| hello_e | arty_35 | xc7a35tcsg324-1 | yes | 760 | 63 s | yes | yes | 0.45 / 0.03 s | 0.39 / 0.02 s | 0.05 / 0.002 s |
| hello_f | arty_35 | xc7a35tcsg324-1 | yes | 856 | 61 s | yes | yes | 0.43 / 0.03 s | 0.39 / 0.03 s | 0.05 / 0.002 s |
| hello_g | arty_35 | xc7a35tcsg324-1 | yes | 1991 | 61 s | yes | yes | 0.46 / 0.03 s | 0.42 / 0.03 s | 0.06 / 0.002 s |
| hello_h | arty_35 | xc7a35tcsg324-1 | yes | 457 | 60 s | yes | yes | 0.46 / 0.03 s | 0.38 / 0.02 s | 0.04 / 0.002 s |
| hello_i | arty_35 | xc7a35tcsg324-1 | yes | 1060 | 62 s | yes | yes | 0.45 / 0.03 s | 0.39 / 0.03 s | 0.05 / 0.002 s |
| hello_j | arty_35 | xc7a35tcsg324-1 | yes | 806 | 61 s | yes | yes | 0.41 / 0.03 s | 0.39 / 0.02 s | 0.06 / 0.002 s |
| hello_k | arty_35 | xc7a35tcsg324-1 | yes | 4395 | 70 s | yes | yes | 0.52 / 0.03 s | 0.49 / 0.03 s | 0.09 / 0.005 s |
| hello_l | arty_35 | xc7a35tcsg324-1 | yes | 3642 | 66 s | yes | yes | 0.53 / 0.03 s | 0.47 / 0.03 s | 0.08 / 0.004 s |

Build times are the whole documented flow (synthesis, pack, place,
route, genfasm, bitstream; LiteX designs include generating the SoC and
compiling its BIOS), one build at a time, on a machine shared with
other jobs. Summed over the 30 designs: the flow's xcfasm 108 s, the
Rust xcfasm 3.1 s (35x); the flow's fasm2frames 107 s, Rust 3.0 s (36x);
the flow's `fasm --canonical` 27.6 s, Rust 1.6 s (17x); xc7frames2bit
1.4 s / 0.5 s and bitread `-z -y -o` 1.1 s / 0.3 s (both C++ on the
flow side, dominated by process start and I/O at these sizes).

**Dense, sparse, pudc and debug variants** (`tools/difftest-xilinx.py
--corpus-root tools/e2e/build/out/f4pga-examples`, 30 FASM files; per
file 4 fasm2frames runs, 3 of them through xc7frames2bit and the 11
bitread flag sets, and 3 xcfasm runs): against the oracle (prjxray
`c9f02d85`, pinned db) 120/120 fasm2frames runs and 90/90 xcfasm runs
identical (1080 xc7frames2bit/bitread runs); against the flow's own
tools (prjxray `ae546d6b` C++ tools, xc_fasm `25dc605c`, flow db) the
same, 120/120 and 90/90. `make xilinx-difftest` (the committed corpus,
now with these 30 files): 56 FASM files (26 before), 227/227 fasm2frames runs, 150/150 xcfasm runs and the 6 reference bitstreams identical, 6.5 minutes with `--jobs 2`. `tools/difftest.py` (Rust
`fasm` parser, `to_string`, canonical form and round trip against the
original Python package, ANTLR and textX): 30/30 files identical (6.6 minutes, `--jobs 2`).

**Not built.** `counter_test/nexys_video` and `litex_sata_demo/nexys_video`
(the only designs of the Nexys Video, xc7a200tsbg484-1): the
`xc7a200t_test` architecture package is 10.5 GiB extracted (a single
VPR routing graph), more than this task's 6 GiB disk budget and, next to
VPR itself, this machine's 15 GiB of RAM for a tmpfs. Everything else
the f4pga-examples CI builds for xc7 was built, including
`linux_litex_demo` (its prebuilt gateware Verilog and memory images are
in the repository; no BIOS build is needed) and `litex_demo` for both
CPUs (with the LiteX packages its Arty targets import, see the README).

**Findings.** No difference between the Rust tools and the flow's or
the oracle's anywhere; no Rust bug found, no Rust change. Quirks of the
reference flow (`docs/rewrite/COMPAT.md`, "The f4pga flow's outputs"):
the environment's `bin/fasm2frames` does not run (prjxray's pip package
misses `utils/`); `symbiflow_write_fasm` ignores a failing `genfasm`
(`tools/e2e/f4pga/check-genfasm.sh` now checks genfasm's own log)
(an OOM-killed `genfasm` gave a truncated 320 line FASM for
`counter_test/arty_100` and a "successful" build; rebuilt, 837 lines);
the flow's `.bit` header names its temporary `.frm` file. The flow is
deterministic (`counter_test/arty_35` rebuilt gives the committed FASM
byte for byte).

### 8.12 prjuray-db all-parts differential testing (T6.3)

`tools/gen-xilinx-corpus.py` (generator, now for both layouts),
`tools/difftest-xilinx.py --prjuray` (comparison), `make
uray-difftest-all`, `tests/cli/test_uray_corpus.py` (fast CI check with a
golden file). Usage and disk layout: `tests/oracle/README.md`, "prjuray
all-parts differential test".

**What prjuray-db contains.** Upstream prjuray-db
(`https://github.com/f4pga/prjuray-db`, branch `master` =
`affbc5e555ebae16475f32e8fb2d6565d4204f3f`, the commit
`tools/fetch-db.sh` pins; checked with `git ls-remote` and the tree of a
blobless clone on 2026-09-25) has exactly one family directory,
`zynqusp`, with two parts, `xczu3eg-sbva484-1-e` and
`xczu3eg-sfvc784-1-e`, whose `tilegrid.json` files are identical (66385
tiles, 158 tile types; `part.yaml`, `part.json` and `tileconn.json` are
identical too, only `package_pins.csv` differs). Its only other
branch, `next_zynqusp_db` (`2bcdfdcf`, merged into `master` by PR #2),
has the same family and parts (a slightly different `zynqusp` tree). So
**prjuray-db has no native UltraScale (`xcuseries`, non-plus) part**: UltraScale is covered by the Vivado
bitstreams of prjuray-tools' `ToolsTestData.tar.gz` (below) and by the
synthetic parts and unit tests of §8.10 only. The harness nevertheless
discovers every family directory of `prjuray-db/` (a directory with
`tile_types/`) and every part of each (a directory with a
`tilegrid.json`), so a new family is tested without code changes.

Of the 158 tile types of the zynqusp parts, 27 have a `segbits_*.db`
(`BRAM` also `segbits_bram.block_ram.db`: the `BLOCK_RAM` bus, 256 frame
columns), 29 have tiles with a bits block (`INT_INTF_R_PCIE4`, 95 of its
360 tiles, and `PSS_ALTO` have bits but no segbits); there are no
`ppips_*.db`, `mask_*.db` or `required_features.fasm` files and no
aliases in the tilegrid. Every bits block ends at or before 16-bit word
186 (the largest `offset + words` is 186, `XIPHY_BYTE_RIGHT` and
`PSS_ALTO`), and every segbits bit of a tile lies inside it: no real
feature reaches past the frame end or into the ECC words 90-92 (the
`RCLK_*` tiles start at word 93).

**Generator, prjuray-db layout** (`is_prjuray_layout`: a family
directory with `tile_types/` and no `mapping/`). What changes, following
prjuray's `utils/fasm2frames.py` (`prjuray/db.py`, `grid.py`,
`tile_segbits.py`, `tile_segbits_alias.py`, `utils/fasm_assembler.py`,
all read for this task; the assembler is prjxray's without its word
checks, §8.10):

* parts: the directories with a `tilegrid.json` (sorted), the tilegrid
  `<part>/tilegrid.json`, tile types from `tile_types/`, no fabric
  (manifest `fabric: null`, `layout: "prjuray"`); segbits, block RAM
  segbits and ppips files as in prjxray-db (the lookup model,
  `TileSegbits` / `TileSegbitsAlias`, is the same code);
* positions in 16-bit words (`offset * 16 + word_bit`), 186 per frame;
  a bit past the frame end is not dropped: a cleared one is stored in
  the bit map (and conflicts), a set one would fail the whole file with
  `IndexError` in `get_frames`, so a unit is not placed on a tile where
  it sets one; a unit that does that on every tile of its group is
  listed in `uncovered` (`sets a bit past the frame end on every tile`)
  and one such line goes to `errors/past_frame_end.fasm`;
* no IO bank, STEPDOWN or PUDC_B step (prjuray's `run()` has none): no
  reserved tiles, no STEPDOWN hosts, `pudc_b: null`;
* `--expected-frm` writes the sparse `.frm` of `uray-fasm2frames`
  (186 16-bit words per frame);
* error files: those of §8.9 except `stepdown_unbonded.fasm`, plus two
  lookup errors in `lookup_errors.fasm` (a feature of a tile type
  without segbits with a bits block, `PSS_ALTO`, and one without,
  e.g. `AMS`: `FasmLookupError` either way), `feature_name.fasm` and
  `past_frame_end.fasm` (only when a unit sets a bit past the frame end,
  i.e. on the synthetic database).

In both layouts a segbits key with a part that is not a FASM identifier
(`[A-Za-z][A-Za-z0-9_]*`) is now listed in `unreachable` (`not a FASM
feature name`), and `errors/feature_name.fasm` writes one to show the
parse error (both parsers reject it at the `.` before the digit; rule 2).
prjxray-db has no such key, so the prjxray corpora are byte identical to
generator version 2 (checked on the test databases, xc7a35tcsg324-1 and
xc7z020clg400-1 `--tiles first`; only `manifest.json` gains `layout`),
and `GENERATOR_VERSION` stays 2. prjuray-db has 34:

| tile type | keys | e.g. |
|---|---|---|
| `BRAM` | 6 | `BRAM.RAMB18E2_L.READ_WIDTH_A.36`, `BRAM.RAMB36E2.WRITE_WIDTH_B.72` |
| `INT_INTF_LEFT_TERM_PSS` | 2 | `INT_INTF_LEFT_TERM_PSS.OUTPUTS_ENABLED.0` |
| `XIPHY_BYTE_RIGHT` | 26 | `XIPHY_BYTE_RIGHT.BITSLICE_RX_TX_X0Y0.ISERDES.ISERDESE3.DATA_WIDTH.4` |

These features cannot be set through FASM with any tool (a database
limitation, `COMPAT.md`). Value formats: prjuray's `fasm2frames.py`
parses with the same `fasm` package (the ANTLR parser of the oracle venv)
as f4pga-xc-fasm, and `canonical_features` / `add_fasm_line` are the
same, so the §8.9 restrictions (and no others) apply: every generated
line was accepted by the reference.

**Coverage** (`--tiles sample 3`, seed 0, both parts; the numbers are the
same for every configuration tried): **27 of 27 tile types with segbits
reached, 54542 of 54542 reachable units placed (features and addresses:
the 54576 segbits keys minus the 34 above), `uncovered` empty,
`unreachable` exactly the 34 keys**; per group coverage is asserted by
the generator (`check_coverage`) and checked by the tests. 14 features
files (the exclusive options of the six `HPIO_RIGHT`, four
`HDIO_*_RIGHT` and three `CMT_RIGHT` / `RCLK_*` tiles need them), 19644
lines, 81097 placements for xczu3eg-sfvc784-1-e (19604 lines, 81621
placements for xczu3eg-sbva484-1-e), 5 error files; 2.8 s per part.

| tile type | units | tiles | | tile type | units | tiles |
|---|---|---|---|---|---|---|
| BRAM | 37128 | 216 | | RCLK_BRAM_INTF_L | 160 | 6 |
| CLEL_L | 768 | 720 | | RCLK_BRAM_INTF_TD_L | 160 | 9 |
| CLEL_R | 768 | 4500 | | RCLK_BRAM_INTF_TD_R | 160 | 3 |
| CLEM | 829 | 2520 | | RCLK_CLEL_L_L | 40 | 12 |
| CLEM_R | 829 | 1080 | | RCLK_CLEM_L | 40 | 42 |
| CMT_RIGHT | 104 | 3 | | RCLK_CLEM_R | 40 | 18 |
| HDIO_BOT_RIGHT | 828 | 4 | | RCLK_DSP_INTF_CLKBUF_L | 288 | 3 |
| HDIO_TOP_RIGHT | 812 | 4 | | RCLK_DSP_INTF_L | 80 | 6 |
| HPIO_RIGHT | 1560 | 6 | | RCLK_DSP_INTF_R | 80 | 6 |
| INT | 3778 | 5940 | | RCLK_HDIO | 512 | 4 |
| INT_INTF_LEFT_TERM_PSS | 48 | 180 | | RCLK_INTF_LEFT_TERM_ALTO | 456 | 3 |
| RCLK_AMS_CFGIO | 144 | 1 | | RCLK_INT_L / RCLK_INT_R | 1576 / 1576 | 75 / 24 |
| RCLK_XIPHY_OUTER_RIGHT | 48 | 3 | | XIPHY_BYTE_RIGHT | 1730 | 12 |

(T6.2's random corpus reached 17 of the 27 types and about 1.6 k of the
features.)

**What is compared, per part** (`--prjuray`, parts in parallel with
`--jobs`, each part's runs in sequence; the driver, the result cache
setup, `tally` and the `bitread` comparison are shared with the prjxray
all-parts mode, whose output is unchanged: the old and the new script
print the same and write the same JSON rows on two parts with the Rust
tools on both sides): `uray-fasm2frames` (reference: prjuray
`utils/fasm2frames.py`) on `features.fasm` dense, `--sparse`, `--sparse
--debug`, `--dump_bits` and `--sparse --roi`, once more `--sparse` with
`FASM_XDB_CACHE=0` for the Rust tool, the other features files and the
error files `--sparse`, and T6.2's 20 random designs and 5 error cases
(`--uray-files`, seed `--uray-seed` + part index) with the four flag sets
(the first ten designs also with the ROI): 134 runs per part. For every
run that succeeds with the dense, sparse or ROI flags and for the other
features files, the reference `.frm` converted to 32-bit words must equal
the Rust `fasm2frames` output, then `xcframes2bit` (reference
prjuray-tools) turns it into a `.bit` that must be identical, and both
`uray-bitread`s read it with the 9 flag sets (2 for the other features
files). Finally the 5 `ToolsTestData` bitstreams (Series7, UltraScale,
UltraScale+) with every `uray-bitread` flag set and their round trip.
The Runner caches the reference results (key: command line, input file
contents, the wrappers, `build/xilinx/bin`, prjuray's `utils/`, the
`prjuray` and `fasm` packages of the oracle venv, the database commit).

**Results** (`ORACLE_DIR` the shared oracle, prjuray
`c550b03a26b4c4a9c4453353bd642a21f710b3ec`, prjuray-tools
`f53f07b8fe37721137a57e9bee3b2b13e7676f53`, prjuray-db
`affbc5e555ebae16475f32e8fb2d6565d4204f3f`; `--jobs 2`, next to a
3-job prjxray run of the orchestrator, 2026-09-25):

| run | uray-fasm2frames runs | identical | explained | different | fasm2frames + xcframes2bit + uray-bitread runs | wall time |
|---|---|---|---|---|---|---|
| default (`--tiles sample 3`), 2 parts, 88 files, 45938 lines | 268 | 258 | 10 | 0 | 1248 (all identical) | 406 s (393 / 394 s per part) |
| the same from the result cache | 268 | 258 | 10 | 0 | 1248 | 96 s |
| `--tiles first` (every feature once per file, 14 features files), 2 parts, 88 files, 39200 lines; random designs and `ToolsTestData` from the cache | 268 | 258 | 10 | 0 | 1248 (all identical) | 120 s |
| `ToolsTestData` bitstreams x 9 flag sets + round trip | 5 | 5 | | 0 | | |

* **0 unexplained differences, no Rust bug found.** The 10 explained
  runs are the value range error (rule 4; `errors/value_range.fasm` and
  the four flag sets of the random `value_range.fasm`, per part). Rule 1
  applied 52 times (every error file), rule 2 10 times (the random
  `syntax_error.fasm` and `errors/feature_name.fasm`).
* Model cross check (no reference involved): the Rust `uray-fasm2frames
  --sparse` equals the generator's `--expected-frm` for every features
  file of both parts with `--tiles first`, `sample 3`, `sample 5
  --density 0.8 --seed 7` and `all --max-per-tile 20` (300 k lines per
  part): 112 files; on `synthetic-usp-db` with `sample 3`, `first` and
  `all` (`tests/cli/test_gen_xilinx_corpus.py`, including the cleared
  bit past the frame end of `EDGE.CLEAR_OUT` and the `IndexError` of
  `EDGE.OUT`, both also checked against the reference by hand: identical).
* `tests/cli/test_uray_corpus.py`: the golden
  `tests/corpus/prjuray/zynqusp/generated/xczu3eg-sfvc784-1-e-sample-3-s0.json`
  (reference results of `uray-fasm2frames` for the 19 files, 20 runs:
  exit code, `.frm` SHA-256, normalised stderr, and the SHA-256 of the
  32-bit conversion for the successful runs; header: the prjuray and
  prjuray-tools commits from `tests/oracle/build/xilinx/status.json`, the
  prjuray-db commit); the Rust `uray-fasm2frames` and `fasm2frames` with
  and without the database cache; 9 s.
### 8.13 nextpnr-xilinx examples (T7.6)

The example designs of nextpnr-xilinx (`xilinx/examples`, openXC7 tag
`0.8.2` = `dea2f28c`, the source of the installed openXC7 snap `0.8.2`;
upstream gatecat/nextpnr-xilinx `8f178fc6` has the same examples) and of
the openXC7 organisation's demo repositories (demo-projects `c5246c58`,
the last commit written for this toolchain generation, including its
`regression/` cases; primitive-tests `d29ee7c5`), built with the openXC7
snap flow each example documents: yosys `synth_xilinx`, `nextpnr-xilinx
--fasm`, the snap's `fasm2frames` (prjxray `utils/fasm2frames.py`) and
`xc7frames2bit`, all with the snap's bundled prjxray-db (flow, collection
and comparison: `tools/e2e/run-nextpnr-examples.sh`,
`compare-nextpnr-examples.py`, `install-nextpnr-examples-corpus.py`;
details, workarounds and what was not built: `tools/e2e/README.md`,
"nextpnr-xilinx examples corpus (T7.6)"). Every Artix-7 design with a
chipdb here or one its Makefile would build (xc7a35tcpg236-1 and
xc7a100tfgg676-1 were built, 85 s and 152 s) was attempted; the 20 that
nextpnr-xilinx 0.8.2 places and routes are in the corpus
(`tests/corpus/xilinx/artix7/designs/{nextpnr-xilinx,openxc7-demo-projects,openxc7-primitive-tests}/<example>/<board>/`,
2.5 MB: `top.fasm[.xz]`, the flow's dense frames `top.frm.xz`,
`difftest.json`, a README of provenance, commands, sha256 of the flow's
FASM, frames and bitstream, and the comparison results), so `make
xilinx-difftest` covers them. The whole `--all` run took 27 minutes, 20
of them the one regression case whose router never finishes.

**What was compared, per design** (`compare-nextpnr-examples.py`; the
columns of the matrix):

* *Same as snap tools* (snap db): the Rust `fasm2frames` writes the
  flow's frames byte for byte (dense, the flow's command line) and the
  snap `fasm2frames`' frames with `--sparse`; with
  `--emit_pudc_b_pullup` it writes the oracle's (snap db) frames -- the
  snap's tool fails there on every design (a stale feature name,
  `COMPAT.md`, "The openXC7 snap's tools"); the Rust `xc7frames2bit` on
  the flow's frames writes the flow's `.bit` (up to the `.frm` path in
  the header's design field, time injected); the Rust `xcfasm` (the snap
  has none) with the snap's `xc7frames2bit` writes both; the Rust
  `bitread` prints what the snap's prints for the flow's `.bit` with the
  11 `BITREAD_FLAGS` sets; the Rust `fasm` CLI prints what the snap's
  `fasm` (textX: its ANTLR extension does not load here) and the
  oracle's print, with and without `--canonical`.
* *Pinned db*: the Rust `fasm2frames` and the oracle agree (frames, exit
  code, last error line); "same frames" when that is also the snap db's
  result, "both fail" when the design uses features only the snap
  database has (the `ppips_cfg_center_*` pseudo PIPs of `STARTUPE2` and
  `BSCANE2`, `GTP_COMMON.GTPE2_COMMON.GTGREFCLK0_USED`; see
  `tools/e2e/README.md`, "A note on prjxray-db provenance").
* Timings: one run of each tool on this machine (4 cores, shared with
  another agent's jobs; the Rust tools with a warm `FASM_XDB_CACHE`).
  *Flow* is synthesis, the `$buf` workaround and place and route.

| Source | Example | Board | Part | Built | FASM lines | Flow | Same as snap tools (frm, bit, xcfasm, bitread, fasm) | Pinned db = oracle | fasm2frames snap / Rust | xc7frames2bit snap / Rust | bitread snap / Rust | fasm --canonical snap / Rust |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| nextpnr-xilinx | attosoc | arty-a35 | `xc7a35tcsg324-1` | yes | 25718 | 12 s | yes | yes, same frames | 4.94 / 0.05 s | 0.06 / 0.03 s | 0.02 / 0.01 s | 3.94 / 0.027 s |
| nextpnr-xilinx | attosoc | xczu2cg | `xczu2cg-sbva484-1-e` | skipped: UltraScale+ (xczu2cg chipdb, RapidWright json2dcp + Vivado, no FASM) | | | | | | | | |
| nextpnr-xilinx | blinky | arty-a35 | `xc7a35tcsg324-1` | yes | 1046 | 5 s | yes | yes, same frames | 0.78 / 0.04 s | 0.06 / 0.03 s | 0.01 / 0.00 s | 0.29 / 0.003 s |
| nextpnr-xilinx | blinky | artyz7-20 | `xc7z020clg400-1` | skipped: Zynq-7000 xc7z020 (zynq7; no chipdb, out of scope) | | | | | | | | |
| nextpnr-xilinx | blinky | xczu2cg | `xczu2cg-sbva484-1-e` | skipped: UltraScale+ (xczu2cg chipdb, RapidWright json2dcp + Vivado, no FASM) | | | | | | | | |
| nextpnr-xilinx | blinky | zcu104 | `xczu7ev-ffvc1156-2-e` | skipped: UltraScale+ (xczu7ev chipdb, RapidWright json2dcp + Vivado, no FASM) | | | | | | | | |
| openxc7-demo-projects | blinky | digilent-arty | `xc7a35tcsg324-1` | yes | 738 | 5 s | yes | yes, same frames | 0.70 / 0.03 s | 0.06 / 0.03 s | 0.01 / 0.00 s | 0.22 / 0.002 s |
| openxc7-demo-projects | blinky | digilent-basys-3 | `xc7a35tcpg236-1` | yes | 728 | 5 s | yes | yes, same frames | 0.66 / 0.03 s | 0.06 / 0.03 s | 0.01 / 0.01 s | 0.24 / 0.002 s |
| openxc7-demo-projects | litex-ddr | qmtech-artix7 | `xc7a100tfgg676-1` | yes | 196344 | 71 s | yes | yes, same frames | 43.27 / 0.23 s | 0.11 / 0.05 s | 0.09 / 0.02 s | 35.66 / 0.178 s |
| openxc7-demo-projects | litex-sata | alientek-davincipro | `xc7a35tfgg484-2` | **no**: place and route failed (ERROR: IBUFDS_GTE2 instance IBUFDS_GTE2 output port must be connected to a GTPE2_COMMON instance, but is instead connected to an instance $auto$clkbufmap.cc:261) | | 31 s | | | | | | |
| openxc7-demo-projects | regression-bram-sdp-unused-port | xc7a200tfbg484 | `xc7a200tfbg484-2` | yes | 1522 | 9 s | yes | yes, same frames | 2.07 / 0.08 s | 0.27 / 0.14 s | 0.04 / 0.02 s | 0.38 / 0.005 s |
| openxc7-demo-projects | regression-bufg-fabric-driven | xc7a200tfbg484 | `xc7a200tfbg484-2` | yes | 872 | 8 s | yes | yes, same frames | 1.86 / 0.07 s | 0.25 / 0.14 s | 0.04 / 0.01 s | 0.23 / 0.002 s |
| openxc7-demo-projects | regression-bufh-clock-constraint | xc7a200tfbg484 | `xc7a200tfbg484-2` | yes | 817 | 8 s | yes | yes, same frames | 1.98 / 0.07 s | 0.26 / 0.14 s | 0.04 / 0.01 s | 0.23 / 0.002 s |
| openxc7-demo-projects | regression-bufio-in-use | xc7a35tcsg324 | `xc7a35tcsg324-1` | **no**: place and route failed (ERROR: Unable to place cell 'bufio_i', no Bels remaining of type 'BUFIO') | | 4 s | | | | | | |
| openxc7-demo-projects | regression-bufr-pad-site | xc7a200tfbg484 | `xc7a200tfbg484-2` | **no**: place and route failed (ERROR: Unable to place cell 'bufr_i', no Bels remaining of type 'BUFR') | | 4 s | | | | | | |
| openxc7-demo-projects | regression-bufr-sink-region | xc7a200tfbg484 | `xc7a200tfbg484-2` | **no**: place and route failed (ERROR: Unable to place cell 'bufr_i', no Bels remaining of type 'BUFR') | | 4 s | | | | | | |
| openxc7-demo-projects | regression-clock-srcc-bufg | xc7a200tfbg484 | `xc7a200tfbg484-2` | yes | 820 | 8 s | yes | yes, same frames | 2.03 / 0.07 s | 0.24 / 0.12 s | 0.05 / 0.01 s | 0.24 / 0.002 s |
| openxc7-demo-projects | regression-config-primitive-startupe2 | xc7a200tfbg484 | `xc7a200tfbg484-2` | yes | 875 | 8 s | yes | yes, both fail (snap-only features) | 1.93 / 0.07 s | 0.27 / 0.14 s | 0.04 / 0.01 s | 0.29 / 0.002 s |
| openxc7-demo-projects | regression-const-holdout | xc7a35tcsg324 | `xc7a35tcsg324-1` | yes | 29125 | 12 s | yes | yes, same frames | 5.24 / 0.06 s | 0.07 / 0.04 s | 0.02 / 0.01 s | 4.28 / 0.021 s |
| openxc7-demo-projects | regression-dsp-const-only-pins | xc7a200tfbg484 | `xc7a200tfbg484-2` | **no**: place and route failed (ERROR: Unrouteable $PACKER_GND_NET sink $mul$top.v:9$4.CARRYCASCIN (SITEWIRE/DSP48_X0Y98/CARRYCASCIN)) | | 9 s | | | | | | |
| openxc7-demo-projects | regression-dup-package-pin | xc7a200tfbg484 | `xc7a200tfbg484-2` | **no**: place and route failed (ERROR: Cell '$iopadmap$top.led1$intcell$OBUF' cannot be bound to bel 'IOB_X0Y233/IOB33/OUTBUF' since it is already bound to cell '$iopadmap$top.led2$intcell$OBU) | | 4 s | | | | | | |
| openxc7-demo-projects | regression-fdse-fdpe-undefined-init | xc7z010clg400 | `xc7z010clg400-1` | skipped: Zynq-7000 xc7z010 (zynq7; out of scope) | | | | | | | | |
| openxc7-demo-projects | regression-iddr-four-iff-flops | xc7a200tfbg484 | `xc7a200tfbg484-2` | **no**: place and route failed (ERROR: Invalid global constant node 'INT_L_X0Y113/GND_WIRE') | | 8 s | | | | | | |
| openxc7-demo-projects | regression-lut_shared_pin | xc7z010clg400 | `xc7z010clg400-1` | skipped: Zynq-7000 xc7z010 (zynq7; out of scope) | | | | | | | | |
| openxc7-demo-projects | regression-lutram-clkinv | xc7a200tfbg484 | `xc7a200tfbg484-2` | **no**: place and route failed (ERROR: Unable to place cell 'ram', no Bels remaining of type 'RAM64X1S') | | 4 s | | | | | | |
| openxc7-demo-projects | regression-lutram-ram64x1s | xc7a200tfbg484 | `xc7a200tfbg484-2` | **no**: place and route failed (ERROR: Unable to place cell 'mem.0.0.genblk1.genblk1[0].genblk1.slice', no Bels remaining of type 'RAM64X1S') | | 4 s | | | | | | |
| openxc7-demo-projects | regression-srl-init | xc7a200tfbg484 | `xc7a200tfbg484-2` | **no**: place and route failed (ERROR: Invalid global constant node 'INT_L_X0Y113/GND_WIRE') | | 8 s | | | | | | |
| openxc7-demo-projects | regression-srl-wemux | xc7a200tfbg484 | `xc7a200tfbg484-2` | **no**: place and route failed (timed out after 1200s;) | | 1204 s | | | | | | |
| openxc7-demo-projects | regression-xorigport-unknown-name | xc7a200tfbg484 | `xc7a200tfbg484-2` | yes | 63 | 8 s | yes | yes, same frames | 1.74 / 0.07 s | 0.25 / 0.14 s | 0.04 / 0.01 s | 0.17 / 0.003 s |
| openxc7-primitive-tests | gtp_channel | xc7a35tfgg484 | `xc7a35tfgg484-2` | yes | 1975 | 5 s | yes | yes, same frames | 2.96 / 0.04 s | 0.06 / 0.03 s | 0.01 / 0.00 s | 0.40 / 0.003 s |
| openxc7-primitive-tests | gtp_common-external-refclk | xc7a100tfgg484 | `xc7a100tfgg484-1` | **no**: place and route failed (ERROR: Invalid global constant node 'INT_L_X0Y173/VCC_WIRE') | | 5 s | | | | | | |
| openxc7-primitive-tests | gtp_common-internal-refclk | xc7a100tfgg484 | `xc7a100tfgg484-1` | yes | 301 | 6 s | yes | yes, both fail (snap-only features) | 4.78 / 0.05 s | 0.10 / 0.06 s | 0.01 / 0.01 s | 0.16 / 0.002 s |
| openxc7-primitive-tests | jtag-test | acorn-cle215 | `xc7a200tfbg484-3` | yes | 818 | 9 s | yes | yes, both fail (snap-only features) | 1.77 / 0.08 s | 0.25 / 0.13 s | 0.04 / 0.01 s | 0.23 / 0.002 s |
| openxc7-primitive-tests | mmcm-blinky-artix | xc7a100tfgg676 | `xc7a100tfgg676-1` | yes | 1283 | 9 s | yes | yes, same frames | 1.05 / 0.04 s | 0.12 / 0.06 s | 0.01 / 0.01 s | 0.30 / 0.003 s |
| openxc7-primitive-tests | mmcm-blinky-artixx | xc7a100tfgg676 | `xc7a100tfgg676-1` | yes | 1283 | 9 s | yes | yes, same frames | 1.06 / 0.04 s | 0.11 / 0.06 s | 0.01 / 0.01 s | 0.32 / 0.003 s |
| openxc7-primitive-tests | mmcm-reconfig | qmtech-artix7 | `xc7a100tfgg676-1` | yes | 4927 | 8 s | yes | yes, same frames | 1.68 / 0.04 s | 0.11 / 0.05 s | 0.02 / 0.01 s | 0.82 / 0.009 s |
| openxc7-primitive-tests | pll-reconfig | qmtech-artix7 | `xc7a100tfgg676-1` | yes | 4277 | 7 s | yes | yes, same frames | 1.57 / 0.04 s | 0.10 / 0.05 s | 0.02 / 0.01 s | 0.70 / 0.006 s |
| openxc7-primitive-tests | startupe2 | qmtech-artix7 | `xc7a100tfgg676-1` | yes | 799 | 6 s | yes | yes, both fail (snap-only features) | 0.91 / 0.05 s | 0.10 / 0.06 s | 0.01 / 0.01 s | 0.23 / 0.002 s |

Summed over the 20 designs: the snap's `fasm2frames` 83.0 s, the Rust
one 1.25 s (66x; the oracle `xc_fasm.fasm2frames` with the pinned db
27.4 s: the snap's runs its fasm parser with textX); the snap's `fasm
--canonical` 49.4 s, Rust 0.28 s (176x); `xc7frames2bit` 2.9 s / 1.5 s
and `bitread -z -y -o` 0.56 s / 0.20 s (C++ on the snap side, process
start and I/O at these sizes).

**Dense, sparse, pudc and debug variants against the oracle**
(`tools/difftest-xilinx.py` over the committed corpus, `--filter
'*designs/nextpnr-xilinx/*'` and `'*designs/openxc7-*'`, the oracle tools
of `tests/oracle`): with the pinned database 80/80 fasm2frames runs and
60/60 xcfasm runs identical (576 xc7frames2bit/bitread runs; the 4
designs with snap-only features fail identically on both sides, so they
have no bitstream runs), 2.2 minutes with `--jobs 2`; with the snap
database (`--db-cache <snap>/opt/nextpnr-xilinx/external`) 80/80 and
60/60 identical, 720 xc7frames2bit/bitread runs, 2.5 minutes.
`tools/difftest.py` (Rust parser, `to_string`, canonical form and round
trip against the original Python package, ANTLR and textX): 20/20 files
identical (1.4 minutes).

**Not built.** Other families: nextpnr-xilinx's `artyz7-20/blinky`
(xc7z020), `attosoc`/`blinky` for xczu2cg and `zcu104/blinky`
(UltraScale+, no FASM: RapidWright json2dcp and Vivado), the fork's
`counter25` (Virtex-7, only on its `main`), demo-projects' Kintex-7,
Spartan-7 and Zynq designs and its two xc7z010 regression cases,
iologic-tests and dsp-tests (Kintex-7). Not placed or routed by
nextpnr-xilinx 0.8.2 (errors in `tools/e2e/README.md`): demo-projects'
`litex-sata/alientek-davincipro` (`IBUFDS_GTE2` driving a `BUFG`),
primitive-tests' `gtp_common/external-refclk`, and 10 of the 17 Artix-7
regression cases, which guard nextpnr-xilinx fixes made after 0.8.2
(`srl-wemux` reached the 20 minute cap in the router).

**Findings.** No difference between the Rust tools and the snap's or the
oracle's; no Rust bug found, no Rust change. Quirks of the reference
flow (`docs/rewrite/COMPAT.md`, "The openXC7 snap's tools"): the snap's
`fasm2frames --emit_pudc_b_pullup` asks for a PUDC_B `IN_ONLY` feature
that neither database has and fails on every design; the snap's `fasm`
package falls back to textX here (its ANTLR extension needs `libffi.so.7`
from snapd's `core20`); the pinned database lacks features some designs
use. Flow notes (`tools/e2e/README.md`): this machine's Yosys needs the
T7.2 `$buf` workaround for nextpnr-xilinx 0.8.2, applied to the written
netlist; the flow is deterministic (two full runs gave the same 20 FASM
files).

## 9. Open questions / risks

1. **Resolved by T6.2 (§8.10):** plain UltraScale uses the UltraScale+
   algorithm with its own parameters (48 bits in word 60 and the low half
   of word 61, `255 - 122`), confirmed on Vivado bitstreams and against
   the reference tools. The original note:
   **UltraScale (plain, non-Plus) ECC algorithm is unconfirmed.**
   `prjuray-tools/lib/include/prjxray/xilinx/xcuseries/ecc.h` and
   `lib/xilinx/xcuseries/ecc.cc` exist (confirmed via `find`) but were not
   opened this session (token budget). Do **not** assume it matches either
   Series7's (word 50, 13-bit) or UltraScale+'s (words 45/46, 48-bit)
   algorithm before implementing — a later agent must read that file first.
   Plain UltraScale's `FrameAddress` layout is confirmed identical to
   Series7's (§4.2), which is *some* evidence its ECC might also match
   Series7's, but this is not verified.
2. **prjuray-db as checked out documents only UltraScale+ (`zynqusp`,
   `URAY_ARCH=UltraScalePlus`), not plain UltraScale.** No plain-UltraScale
   family directory exists in `prjuray-db`. `fasm-xilinx`'s T6.1/T6.2
   differential tests (T6.3) can only be run against real per-bit data for
   UltraScale+ unless another database source is found (e.g. building one
   with the prjuray fuzzers, out of scope here) or the corrected
   `xcuseries` C++ types are exercised only via synthetic/unit tests.
   **Confirmed by T6.3 (§8.12):** upstream prjuray-db (`master` =
   `affbc5e5`) has only `zynqusp` (two xczu3eg parts); UltraScale is
   covered by `ToolsTestData` and the synthetic parts only.
3. **prjuray-db has zero `ppips_*.db` and zero `mask_*.db` files.**
   Confirmed by exhaustive `find`. This means, for every zynqusp tile type
   in the current database, `TileSegbits.ppips` is always empty — any
   feature that *should* be a no-op pseudo-PIP but isn't yet documented
   will instead raise `FasmLookupError` (or, worse, silently match a
   real segbit if the tile-type happens to also have one under the same
   name — unlikely but not provably impossible without cross-checking).
   This is a database-completeness gap in the upstream project, not
   something `fasm-xilinx` can work around — document it in `COMPAT.md`
   when T6.x lands, and expect zynqusp differential tests to have a higher
   "expected FasmLookupError" rate for pip-only features than artix7's.
4. **No `required_features.fasm` file exists anywhere in the checked-out
   artix7 or zynqusp database slices.** The required-features code path
   (§5 step 6) is implemented per spec here but is **unexercised by any
   available real fixture** — T5.9's differential-test corpus generator
   should synthesize a fake `required_features.fasm` for at least one test
   part to cover this path, since neither reference database exercises it.
5. **`fasm-xilinx`'s loader must detect which of the two addressing
   schemes (§2.1 fabric-indirected vs §2.2 per-part-direct) a given
   `db_root/family` uses**, since nothing in `settings.sh`/the directory
   itself declares it explicitly beyond "does `mapping/devices.yaml`
   exist". Recommend probing for `<db_root>/<family>/mapping/devices.yaml`
   and falling back to the per-part-direct scheme if absent, matching how
   `prjuray-tools/prjuray/db.py` has no `get_fabric_for_part` call at all
   (it simply never needed one because prjuray-db was always structured
   this way) versus `prjxray/db.py`'s explicit `get_fabric_for_part` call.
6. **`CFG_CLB` (`BlockType` value `2`) never appears in any real artix7
   tilegrid `bits` block observed this session** (only `CLB_IO_CLK` and
   `BLOCK_RAM` were found across the whole `xc7a50t` tilegrid, 18055
   tiles). It is a legitimate enum value (`prjxray/util.py:348-352`,
   `lib/include/.../xc7series/block_type.h:21-26`) used by the frame
   address math and appears in `part.yaml`'s `configuration_buses` in
   principle, but no concrete example was found to quote. Do not special
   case it away — just note that it is untested against real data in this
   research pass. **Note the asymmetry this creates for a Rust loader**:
   the Python `grid_types.BlockType` enum (`prjxray/grid_types.py:15-21`)
   has **only** `CLB_IO_CLK` and `BLOCK_RAM` members — no `CFG_CLB` — while
   the C++ `xc7series::BlockType` enum (§3.5) has all three. A tilegrid
   `bits` block naming a bus the Python side doesn't know about would
   currently raise a Python `ValueError` from `grid_types.BlockType(k)`
   (`prjxray/grid.py:44`, `BlockType(k)` constructing the enum from the
   JSON key) if it ever appeared — i.e. today's Python pipeline cannot
   actually load a tilegrid with a `CFG_CLB` bus at all. `fasm-xilinx`'s
   tilegrid loader should decide **explicitly** whether to (a) mirror this
   and treat an unrecognized/`CFG_CLB` bus name as a hard load error, or
   (b) support all three block types uniformly (matching the C++ side,
   which the frame-address math and bitstream writer already do support)
   — rather than silently doing whichever a generic enum-from-string
   deserializer happens to do; either choice is defensible, but it must be
   a conscious one made and documented at T5.2 time, not an accident of
   whatever serde/enum crate is used.
7. **The PUDC_B `assert pudc_b_tile_site == None`
   (`xc_fasm/fasm2frames.py:98`) will crash on any part with more than one
   PUDC_B-labeled pin.** No such part was observed in the checked-out
   artix7 slice, but `fasm-xilinx` should turn this into a clean, testable
   error instead of blindly reproducing a Python `AssertionError` — this
   is a deliberate, documented behavior *change*, not a compatibility gap,
   since no CLI output/exit-code contract depends on the exact assertion
   text (`fasm2frames.py`'s CLI has no handling for `AssertionError`
   either way — it's an uncaught crash either way; a clean `Result::Err`
   with a descriptive message is strictly better and does not regress any
   observable compatibility surface).
8. **`prjuray-tools/lib/test_data/` was not inventoried** (only confirmed
   to exist via `find`). Before relying on it for T5.9/T6.3 fixtures, a
   later agent must `ls`/inspect it — treat §7's prjuray-tools row as a
   placeholder, not a verified fixture list.
9. **HCLK "middle word" (§4.1) is described from a single tilegrid
   example** (`HCLK_L_BOT_UTURN_X72Y130`, `offset: 50, words: 1`); it was
   not independently cross-checked against a *non-alias* HCLK tile's
   `bits` block (e.g. a plain `HCLK_L`/`HCLK_R` entry) to confirm `offset:
   50` is universal for all HCLK row tile types and not an artifact of the
   alias example chosen. A later agent implementing HCLK-specific logic
   should spot-check a few more HCLK tile instances in `xc7a50t/tilegrid.json`.
10. **Whether the UltraScale (xcuseries) `Row`/`ConfigurationBus`/
    `ConfigurationColumn` `.cc` files (`prjuray-tools/lib/xilinx/xcuseries/*.cc`)
    contain any further behavioral differences from Series7's beyond
    `FrameAddress`'s bit-field ranges was not verified** — only the header
    files' shapes were confirmed structurally similar by directory listing
    and the `frame_address.h` diff; the `.cc` implementations for
    `xcuseries`'s row/bus/column next-address logic were not opened this
    session (they are very likely byte-identical in *logic* to Series7's,
    given the plain-prjxray shim reused Series7's types wholesale and
    still produced a plausible bitstream — but "very likely" is not
    "confirmed", so flag before relying on it for UltraScale-specific
    padding/row-boundary logic in T6.2).

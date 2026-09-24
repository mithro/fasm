# artix7 smoke-test FASM + golden reference outputs

`smoke_x1y0.fasm` is a tiny, hand written FASM file exercising three real
features from the pinned `prjxray-db` `artix7` family (fabric `xc7a50t`,
part `xc7a35tcsg324-1`): two `CLBLL_L` LUT `INIT` bits and one `INT_L`
routing pip. It is used by `tests/oracle/test_xilinx_oracle.py` as an
offline smoke test of the Xilinx oracle wrappers, and is meant to double
as a first differential-test fixture for the Rust `fasm-xilinx` crate
(T5.9) once it exists -- the golden `.frm`/`.bit`/`.bitread.txt` files
below let that comparison run without a database fetch or the reference
tools being available.

## Provenance

* FASM features taken from `prjxray-db` commit
  `0a0addedd73e7e4139d52a6d8db4258763e0f1f3` (the `PRJXRAY_DB_COMMIT`
  pinned by `tools/fetch-db.sh`):
  * `segbits_clbll_l.db`: `CLBLL_L.SLICEL_X0.ALUT.INIT[00] 32_15` and
    `CLBLL_L.SLICEL_X0.ALUT.INIT[01] 33_15`.
  * `segbits_int_l.db`: `INT_L.EL1BEG_N3.LOGIC_OUTS_L0 11_05 14_05` (a
    2-bit pip encoding -- both listed bits must be set together).
  * Tile instances from `artix7/xc7a50t/tilegrid.json`: `CLBLL_L_X2Y0`
    (grid_x=10, grid_y=155) and its paired `INT_L_X2Y0` (grid_x=11,
    grid_y=155, same row).
* Golden outputs produced with the reference tools built by
  `tests/oracle/setup-xilinx.sh`, pinned to:
  * `prjxray` (C++ tools + Python package):
    `c9f02d8576042325425824647ab5555b1bc77833`
  * `f4pga-xc-fasm` (`xc_fasm.fasm2frames`):
    `25dc605c9c0896204f0c3425b52a332034cf5e5c`

## Exact commands used

```sh
tools/fetch-db.sh prjxray artix7
# -> tests/oracle/build/db/prjxray-db/artix7 (~181 MiB)

tests/oracle/setup-xilinx.sh

tests/oracle/fasm2frames-oracle --sparse \
  --db-root tests/oracle/build/db/prjxray-db/artix7 \
  --part xc7a35tcsg324-1 \
  tests/corpus/xilinx/artix7/smoke_x1y0.fasm \
  tests/corpus/xilinx/artix7/smoke_x1y0.frm

tests/oracle/xc7frames2bit-oracle \
  --frm_file tests/corpus/xilinx/artix7/smoke_x1y0.frm \
  --output_file tests/corpus/xilinx/artix7/smoke_x1y0.bit \
  --part_name xc7a35tcsg324-1 \
  --part_file tests/oracle/build/db/prjxray-db/artix7/xc7a35tcsg324-1/part.yaml

tests/oracle/bitread-oracle -z -y \
  --part_file tests/oracle/build/db/prjxray-db/artix7/xc7a35tcsg324-1/part.yaml \
  -o tests/corpus/xilinx/artix7/smoke_x1y0.bitread.txt \
  tests/corpus/xilinx/artix7/smoke_x1y0.bit
```

`--sparse` is used for `smoke_x1y0.frm` so the golden file only contains
the frames actually touched (36 non-zero frames -- required_features.fasm
for this part pulls in several always-on frames elsewhere on the device,
e.g. PS/config related bits, in addition to the 4 frames the 3 features
above touch directly) instead of every frame of the device zero-filled
(~5.3 MiB dense vs. ~40 KiB sparse); `xc7frames2bit` always emits a full,
dense bitstream regardless (`smoke_x1y0.bit` is a complete, valid
`xc7a35t` configuration bitstream, ~2.1 MiB, matching a real device image
for this part). `-z` on `bitread` skips all-zero frames in the machine
readable `-o` output; `-y` selects the `bit_%08x_%03d_%02d` (frame
address, word index, bit index) format.

**`smoke_x1y0.bit` is NOT byte-for-byte reproducible.** prjxray's
`BitstreamWriter` embeds the current UTC date/time
(`absl::FormatTime(..., absl::Now(), ...)`) and the literal `--frm_file`
path it was given into the bitstream header (see
`lib/include/prjxray/xilinx/bitstream_writer.h` and
`tools/xc7frames2bit.cc`'s `writeBitstream(..., FLAGS_frm_file, ...)`
call), so re-running the command above on a different day, or with the
`.frm` file at a different path, produces a bitstream that differs in its
header bytes even though the programmed configuration bits are identical.
`smoke_x1y0.frm` (fasm2frames-oracle's output) and `smoke_x1y0.bitread.txt`
(bitread's machine-readable dump of the *programmed bits*, which carries
no header/timestamp info) ARE exactly reproducible and are what
`tests/oracle/test_xilinx_oracle.py` actually byte-compares;
`smoke_x1y0.bit` is checked in as a real reference bitstream (e.g. for a
future Rust bitstream reader/writer round-trip test) rather than as
something a fresh run is expected to match byte for byte.

## Expected `smoke_x1y0.bitread.txt` bits

```
bit_0040010b_000_05
bit_0040010e_000_05
bit_00400120_000_15
bit_00400121_000_15
```

`0x00400100` is `INT_L_X2Y0`'s / `CLBLL_L_X2Y0`'s shared base frame
address for this part; `+0x0b`/`+0x0e` word 0 bit 5 is the `EL1BEG_N3.
LOGIC_OUTS_L0` pip's two segbits (`11_05`, `14_05` -- frame offset 11
(0xb) and 14 (0xe) decimal, bit 5 of word 0), and `+0x20`/`+0x21` word 0
bit 15 is `ALUT.INIT[00]`/`INIT[01]` (`32_15`, `33_15` -- frame offset 32
(0x20) and 33 (0x21) decimal, bit 15 of word 0).

## Regenerating

Re-run the commands above after `tests/oracle/setup-xilinx.sh --force`
(e.g. after intentionally moving `PRJXRAY_COMMIT`/`F4PGA_XC_FASM_COMMIT`
in `tests/oracle/setup-xilinx.sh`, or `PRJXRAY_DB_COMMIT` in
`tools/fetch-db.sh`) to refresh these golden files against the new pin.

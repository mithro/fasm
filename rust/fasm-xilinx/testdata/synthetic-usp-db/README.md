# Synthetic prjuray-db style database (UltraScale+)

Hand written for the UltraScale+ tests of `fasm-xilinx` and the
`uray-fasm2frames`, `xcframes2bit` and `uray-bitread` tools (Apache-2.0,
like the rest of this repository), in the prjuray-db layout (`zynqusp/`):
`tile_types/`, `site_types/`, `segbits_*.db` in the family directory, and
one part directory, `xcusptest-1/`, with `part.yaml`
(`!<xilinx/xcupseries/...>` tags), `tilegrid.json` and
`required_features.fasm`.

It covers:

* the 16-bit word unit of prjuray-db: tilegrid `offset`/`words` and the
  segbits `word_bit` count 16-bit words (`CLEM_X1Y1` at offset 3, the
  `RCLK_INT_L` tile at offset 93, the upper half of 32-bit word 46, next
  to the ECC bits of words 45 and 46);
* the `BLOCK_RAM` bus with 256 frames in a column (the 8-bit minor of
  UltraScale+ frame addresses) next to the `CLB_IO_CLK` bits of `BRAM`;
* a tile in the bottom half (`CLEM_X1Y60`, row 32 of `part.yaml`: the row
  number includes the half bit);
* a tile whose bits reach past the end of the 186 16-bit word frame
  (`EDGE_X0Y0`, offset 185: `EDGE.OUT` is 16-bit word 186, `EDGE.OK` the
  last word 185), where prjuray's assembler fails with `IndexError` and
  xc_fasm's drops the bit with a warning;
* a `required_features.fasm` for the part.

The segbits lines are copied from prjuray-db `zynqusp` (CC0-1.0,
<https://github.com/f4pga/prjuray-db>), except `EDGE`. The tile and site
type JSON files are empty stubs: the loaders only list them.

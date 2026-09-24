# Synthetic prjxray-db style database

Hand written for the `fasm-xilinx` loader tests (Apache-2.0, like the rest
of this repository). It covers what the miniature f4pga-xc-fasm database
(`../mini-db`) does not have:

* `part.yaml` (with `!<xilinx/xc7series/...>` tags) and a `part.json`
  with the same frame tree, `idcode` and `iobanks`;
* `required_features.fasm` (with blank lines, surrounding whitespace and a
  duplicate);
* `segbits_bram_l.block_ram.db` (`BLOCK_RAM` bus, `word_bit` up to 2204)
  next to `segbits_bram_l.db`, with a feature (`BOTH`) and a multi bit
  feature bit (`ZRAMB18_Y0.WIDTH[1]`) present on both buses to check the
  lookup precedence;
* `ppips_*.db` of all three types, including a pseudo PIP that also has a
  segbits entry (`INT_L.BYP_ALT0.VCC_WIRE`);
* an alias tile with a site rename and a negative effective offset
  (`LIOB33_SING_X0Y0`), and one without (`HCLK_L_BOT_UTURN_X6Y130`);
* a feature on the Series7 ECC bits (`HCLK_L.ECC_CLASH`, word 50 bit 12),
  which the ECC invariant check must report;
* files the loader must not read: `segbits_bram_l.origin_info.db` (not
  valid segbits) and `mask_bram_l.db`;
* a tile type without segbits (`NOSEGBITS`), a tile whose type has no
  `tile_type_*.json` (`MYSTERY`), a `BRAM_L` tile without a `BLOCK_RAM`
  bits block, and a part whose device is missing from
  `mapping/devices.yaml` (`xc7nodev-1`).

Some lines are copied from prjxray-db `artix7` (CC0-1.0, commit
`0a0addedd73e7e4139d52a6d8db4258763e0f1f3`), e.g. the `INT_L` segbits and
pseudo PIPs and the `BRAM_L` `INIT_00` bits.

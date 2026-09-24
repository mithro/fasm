# Miniature prjxray-db (from f4pga-xc-fasm)

A tiny prjxray-db style database (one fake part `xc7`, device `xc7`,
fabric `xc7`, eleven tiles) used by the `fasm-xilinx` loader unit tests.

## Origin

* Repository: <https://github.com/chipsalliance/f4pga-xc-fasm>
* Commit: `25dc605c9c0896204f0c3425b52a332034cf5e5c`
* Path: `tests/test_data/db/`
* Licence: Apache-2.0 (the `LICENSE` file of that repository; the
  `mapping/*.yaml` files carry the Apache-2.0 header themselves).

## Changes made when copying

Everything was copied verbatim (`cp -r tests/test_data/db/. mini-db/`),
except that the contents of five `tile_type_*.json` files
(`HCLK_IOI3`, `LIOB33`, `LIOB33_SING`, `RIOB33`, `RIOB33_SING`; 334 KB of
routing data) were replaced by `{}`: the loader, like prjxray's
`Database`, only uses the *existence* of `tile_type_<TYPE>.json` to
enumerate the tile types of a family and never reads these files (they
hold the routing model). The other three `tile_type_*.json` files were
already `{}` upstream. Total size: about 13 KB.

## Contents

* `mapping/{parts,devices}.yaml`: part `xc7` -> device `xc7` -> fabric `xc7`.
* `xc7/tilegrid.json`: 11 tiles (CLBLM_L, INT_L, HCLK_L, LIOB33,
  LIOB33_SING (alias of LIOB33), RIOB33, RIOB33_SING, HCLK_IOI3).
* `xc7/part.json`: only `iobanks` (no `idcode`, no frame tree);
  `xc7/package_pins.csv`: ten pins in banks 99 and 66.
* `segbits_*.db`: CLB_IO_CLK segbits for six tile types, including `!`
  bits (`segbits_int_l.db`) and multi bit `INIT[NN]` features
  (`segbits_clblm_l.db`). There are no `ppips_*`, `mask_*`,
  `.block_ram.db` files and no `part.yaml`; those are covered by the
  hand written `../synthetic-db`.

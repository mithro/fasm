# Golden bits for the miniature database

Expected set bits (`bit_<frame>_<word>_<bit>`, bitread `-y` style) of the
f4pga-xc-fasm `test_fasm2frames.py` tests, used by the `fasm-xilinx`
loader tests to check feature lookup and bit placement end to end on
`../mini-db`.

* Repository: <https://github.com/chipsalliance/f4pga-xc-fasm>
* Commit: `25dc605c9c0896204f0c3425b52a332034cf5e5c`
* Licence: Apache-2.0
* Copied (renamed only):
  * `tests/test_data/lut_int/design.bits` -> `lut_int.bits`
    (for `lut_int.fasm`);
  * `tests/test_data/ff_int/design.bits` -> `ff_int.bits`
    (for `ff_int.fasm` and `ff_int_0s.fasm`);
  * `tests/test_data/iob/{liob,riob}_stepdown.bits` (for
    `iob/{liob,riob}_stepdown.fasm`, including the STEPDOWN propagation
    of `fasm2frames.py` and the `_SING` alias bits that wrap to word 99).

The FASM inputs are in `tests/corpus/f4pga-xc-fasm/` at the repository
root.

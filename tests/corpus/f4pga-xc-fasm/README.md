# f4pga-xc-fasm corpus

`.fasm` input fixtures from the `f4pga-xc-fasm` (`xc_fasm`) test data
directory, used here purely as *realistic FASM text* for the parser/output
differential test (T1.5); the accompanying `.bits`/`.v`/db files (used by
`xc_fasm`'s own bits<->FASM tests) are not needed for that and are not
copied.

## Origin

* Repository: <https://github.com/chipsalliance/f4pga-xc-fasm>
* Commit: `25dc605c9c0896204f0c3425b52a332034cf5e5c` (from a local read-only
  checkout at session time; `git -C <checkout> rev-parse HEAD`).
* Licence: Apache-2.0 (see `LICENSE` in that repository; the same licence
  header appears at the top of `dump.py`/`difftest.py` in this repo).
* Path: `tests/test_data/*.fasm` and `tests/test_data/iob/*.fasm`.

Obtained with a plain file copy (no history, no binary siblings):

```sh
cp tests/test_data/{ff_int,ff_int_0s,ff_int_op1,lut,lut_int}.fasm \
   <repo>/tests/corpus/f4pga-xc-fasm/
cp tests/test_data/iob/{liob_stepdown,riob_stepdown}.fasm \
   <repo>/tests/corpus/f4pga-xc-fasm/iob/
```

## Files

* `ff_int.fasm`, `ff_int_0s.fasm`, `ff_int_op1.fasm`: FF (LDCE/FDPE/FDCE
  style) configuration FASM for an `xc7` `CLBLM_L` slice, with routing
  (`INT_L`) features -- realistic multi-line, multi-feature FASM with
  comments.
* `lut.fasm`, `lut_int.fasm`: LUT `INIT[]` bit array features (`ALUT`) plus
  routing, including `VERILOG_HEX`/bit-range value forms.
* `iob/liob_stepdown.fasm`, `iob/riob_stepdown.fasm`: three-line IOB
  (`LIOB33`/`RIOB33`/`RIOB33_SING`) feature-only lines exercising the
  `STEPDOWN` synthetic feature used by `xc_fasm`'s IOB test.

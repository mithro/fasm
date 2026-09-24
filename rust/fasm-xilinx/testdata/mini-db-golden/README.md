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

## Oracle `.frm` output (`frm/`)

`frm/<fixture>.{dense,sparse}.txt` describe, byte for byte, the `.frm`
files the reference `fasm2frames` (f4pga-xc-fasm
`25dc605c9c0896204f0c3425b52a332034cf5e5c` + prjxray
`c9f02d8576042325425824647ab5555b1bc77833`, run by
`tests/oracle/fasm2frames-oracle`) writes for the seven f4pga-xc-fasm
fixtures on `../mini-db`, without and with `--sparse`:

```sh
tests/oracle/fasm2frames-oracle --db-root rust/fasm-xilinx/testdata/mini-db \
    --part xc7 [--sparse] tests/corpus/f4pga-xc-fasm/<fixture>.fasm out.frm
```

Instead of the 40-135 KB of mostly zero words per file they are stored as
a compact summary (converted from the `.frm` with a throw away script):

* `frames 0x<first address> <count>`: `count` frames with consecutive
  addresses starting at `first address`, zero filled;
* `word 0x<frame address> <word index> 0x<value>`: a non zero word.

`tests/assembler_mini_db.rs` rebuilds the `.frm` text from the summary
(101 words per frame) and compares it with the Rust output as a string.

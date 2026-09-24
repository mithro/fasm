# counter_test (from f4pga-examples)

`counter.v` and `arty.xdc` are copied unmodified from
[chipsalliance/f4pga-examples](https://github.com/chipsalliance/f4pga-examples),
path `xc7/counter_test/`, commit `13f11197b33dae1cde3bf146f317d63f0134eacf`
(2024-03-27), so `tools/e2e/run-counter.sh` (T7.1) can build this design
without a checkout of that repository. `LICENSE` in this directory is that
repository's top-level `LICENSE` file (Apache License 2.0), copied
alongside them since neither source file carries its own per-file license
header. Both files are unmodified from upstream; do not hand edit them --
re-copy from f4pga-examples instead.

Target: Digilent Arty A7-35T, part `xc7a35tcsg324-1`, top module `top`.

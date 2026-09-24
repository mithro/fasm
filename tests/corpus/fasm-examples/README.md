# fasm-examples corpus

Verbatim copies of this repository's own `examples/*.fasm` files, kept here
so the differential test corpus is self contained (T1.5, `tools/difftest.py`
discovers `tests/corpus/**/*.fasm`; the canonical originals stay at
`examples/*.fasm` and are also picked up directly by `difftest.py`, so these
copies are redundant with them by design, not a second source of truth --
see `docs/rewrite/PLAN.md`, "Testing strategy", item 4).

## Origin

Same repository, same commit as this worktree (`chipsalliance/fasm`,
relicensed/maintained at `mithro/fasm`), licence Apache-2.0 (see the root
`LICENSE` file). Obtained with a plain file copy:

```sh
cp examples/blank.fasm examples/comment.fasm examples/feature_only.fasm \
   examples/many.fasm tests/corpus/fasm-examples/
```

## Files

* `blank.fasm`: empty file (0 lines).
* `comment.fasm`: a single comment-only line.
* `feature_only.fasm`: a single bare feature name line.
* `many.fasm`: a small file exercising several `SetFasmFeature`, annotation
  and comment combinations; also the fixture behind
  `tests/corpus/oracle/many.fasm.out.txt` / `many.fasm.canonical.txt`
  (T1.4).

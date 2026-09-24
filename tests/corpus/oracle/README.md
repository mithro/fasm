# Oracle golden output (T1.4, `output` module)

Golden output for `examples/many.fasm`, generated with the Python oracle
(`tests/oracle/venv/bin/python`) via `fasm.fasm_tuple_to_string` directly —
**not** through the `tests/oracle/fasm-oracle` CLI wrapper, so there is no
extra trailing newline from `print()` on top of the function's own trailing
`\n`.

* `many.fasm.out.txt`: `fasm.fasm_tuple_to_string(fasm.parse_fasm_filename('examples/many.fasm'), canonical=False)`.
* `many.fasm.canonical.txt`: the same call with `canonical=True`.

Regenerate with:

```
tests/oracle/venv/bin/python - <<'EOF'
import fasm
model = list(fasm.parse_fasm_filename('examples/many.fasm'))
open('tests/corpus/oracle/many.fasm.out.txt', 'w').write(
    fasm.fasm_tuple_to_string(model, canonical=False))
open('tests/corpus/oracle/many.fasm.canonical.txt', 'w').write(
    fasm.fasm_tuple_to_string(model, canonical=True))
EOF
```

Used by `rust/fasm/src/output/line/tests.rs` (T1.4), which builds the same
`examples/many.fasm` model by hand (the `parser` module, T1.3, is not
available to this task) and checks `fasm_tuple_to_string` against these
files byte for byte.

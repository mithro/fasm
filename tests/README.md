# Python tests

`test_simple.py` and `test_rust_parser.py` test the `fasm` Python package,
including the Rust parser (the `fasm._fasm_rs` extension module, built from
`rust/fasm-python` with maturin). They need the extension module installed
in the venv that runs pytest. There are two ways to run them.

## In the source tree (editable install)

```
python3 -m venv venv
venv/bin/pip install maturin pytest textx
VIRTUAL_ENV=$PWD/venv venv/bin/maturin develop --release  # or: venv/bin/pip install -e .
venv/bin/pytest tests/test_simple.py tests/test_rust_parser.py
```

`maturin develop` builds `fasm/_fasm_rs.abi3.so` into the source tree
(ignored by git), so the `fasm/` package that pytest imports from the
repository root has the extension. A plain `pip install .` does **not**
work here: pytest puts the repository root first on `sys.path`
(`tests/__init__.py` makes it the root of the test package), so it imports
the source tree's `fasm/`, which has no extension module:
`test_rust_parser.py` fails to import (`cannot import name '_fasm_rs'`)
and `test_simple.py` only tests the textX parser (and fails
`test_implementations`).

## Against an installed package (`pip install .`, a wheel, an sdist)

```
python3 -m venv venv
venv/bin/pip install pytest .          # or the wheel / sdist
cd "$(mktemp -d)"
/path/to/venv/bin/pytest --import-mode=importlib \
    /path/to/fasm/tests/test_simple.py /path/to/fasm/tests/test_rust_parser.py
```

`--import-mode=importlib` keeps pytest from adding the repository root to
`sys.path`, and running from another directory keeps the current
directory from shadowing the installed package. The tests use absolute
paths, and run their subprocesses (`python -m fasm.tool`) in a temporary
directory, so they import the installed package.

The other test directories have their own instructions: `cli/` (Rust
`fasm` CLI vs. the original, `make cli-difftest`), `oracle/` (the original
package, `oracle/README.md`) and `e2e/` (toolchain end-to-end tests).

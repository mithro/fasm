# FASM oracle

This directory sets up and runs the **oracle**: the *original*, pre-Rust-
rewrite Python `fasm` package (this repository's `fasm/` directory as it
stood before the rewrite, via `setup.py`), installed into its own venv so it
can be used as a golden reference for differential testing the Rust
rewrite. See `docs/rewrite/PLAN.md` ("Testing strategy") and
`docs/rewrite/TASKS.md` (T0.4, T1.5, T2.2, T3.2) for how it is used.

Nothing under `tests/oracle/venv/` or `tests/oracle/build/` is committed to
git (see the root `.gitignore`): every machine builds its own oracle venv.

## Setup

```sh
tests/oracle/setup.sh          # first run: creates tests/oracle/venv
tests/oracle/setup.sh          # re-run: fast no-op once already set up
tests/oracle/setup.sh --force  # rebuild from scratch
```

The script:

1. Creates a venv at `tests/oracle/venv` and installs `textX` and `pytest`
   into it (the pure Python textX parser is required to always work).
2. Best effort, never fatal: runs
   `git submodule update --init --depth 1 -- third_party/antlr4
   third_party/googletest` (needed by the ANTLR C++ extension's CMake
   build) and `apt-get install -y uuid-dev pkg-config` (system libraries the
   ANTLR build needs; requires root or passwordless `sudo`, and network
   access to the distro package mirror).
3. Installs this repository's `fasm` package
   (`pip install --no-build-isolation -e .`) into the venv. `setup.py`
   itself already attempts to build the ANTLR C++ parser extension via
   CMake and **falls back to the textX-only install on any build failure**
   (missing submodules, missing `uuid.h`, no C++17 compiler, ...); this
   script does not need to (and does not) duplicate that fallback logic, it
   just makes the prerequisites available on a best effort basis first.
4. Records which parsers ended up available in
   `tests/oracle/build/status.json` and touches
   `tests/oracle/venv/.oracle-setup-ok` as a completion marker so a plain
   re-run is a fast no-op (the ANTLR build alone takes roughly a minute
   the first time, since it compiles the antlr4 C++ runtime from source).
   Logs from each best-effort step are kept in `tests/oracle/build/`
   (`submodule.log`, `apt.log`, `install.log`) for debugging a failed or
   partial build.

Run `tests/oracle/setup.sh --force` any time you want to re-attempt the
ANTLR build (e.g. after installing a missing system dependency by hand).

## Parsers available on the machine this was last set up on

Both parser implementations built successfully in the container this oracle
was developed and verified in:

```json
{
  "available_parsers": ["antlr", "textx"],
  "antlr_built": true
}
```

(`uuid-dev`, `pkg-config`, `cmake`, and a JDK for the ANTLR code generator
jar were already present; `third_party/antlr4` and `third_party/googletest`
were fetched as shallow submodule checkouts by `setup.sh`.) Your machine may
differ — always check `tests/oracle/build/status.json`, or run:

```sh
tests/oracle/venv/bin/python -c "import fasm.parser as p; print(p.available, p.implementation)"
```

If only `['textx']` is available, the ANTLR C++ parser could not be built;
see `tests/oracle/build/install.log` for why (common causes: no network
access to fetch the submodules, missing `uuid.h` (`uuid-dev`), no C++17
compiler, missing `cmake`/`java`). The oracle is still fully usable with
just the textX parser — it is the reference implementation the FASM
specification is defined against and is what `fasm.parser` falls back to at
runtime whenever the ANTLR extension is unavailable, exactly like a real
`pip install fasm` on a machine without the ANTLR build prerequisites.

## Running the oracle CLI

`tests/oracle/fasm-oracle` is a drop-in stand-in for the original `fasm`
command line tool (same argument grammar, same stdout/stderr/exit code
behaviour as `fasm/tool.py`'s `main()`, since it just calls that function):

```sh
tests/oracle/fasm-oracle examples/many.fasm
tests/oracle/fasm-oracle --canonical examples/many.fasm
tests/oracle/fasm-oracle --parser textx examples/many.fasm
tests/oracle/fasm-oracle --parser antlr examples/many.fasm
```

It requires `tests/oracle/setup.sh` to have been run first. It is
implemented as `tests/oracle/fasm-oracle` execing
`tests/oracle/venv/bin/python tests/oracle/run_fasm.py "$@"`; `run_fasm.py`
can also be invoked directly with the venv interpreter if a wrapper script
is inconvenient (e.g. from a test harness that already knows the venv
path):

```sh
tests/oracle/venv/bin/python tests/oracle/run_fasm.py --canonical examples/many.fasm
```

## Dumping parse trees for differential testing

`tests/oracle/dump.py` prints a deterministic JSON dump of
`parse_fasm_filename(FILE)` for a given parser implementation, meant to be
diffed directly against an equivalent dump from the Rust implementation:

```sh
tests/oracle/venv/bin/python tests/oracle/dump.py examples/many.fasm
tests/oracle/venv/bin/python tests/oracle/dump.py --parser textx examples/many.fasm
tests/oracle/venv/bin/python tests/oracle/dump.py --parser antlr examples/many.fasm
```

Output shape (sorted keys, compact separators, one trailing newline, so
identical input always produces byte-identical output):

```json
{"lines":[{"annotations":null,"comment":null,"set_feature":{"end":null,"feature":"EXAMPLE_FEATURE.X0.Y0.BLAH","start":null,"value":"1","value_format":null}}]}
```

* `set_feature` is `null`, or an object with `feature` (string), `start`
  and `end` (int or `null`), `value` (a **decimal string**, not a JSON
  number — FASM feature values are arbitrary width bit vectors that can
  exceed the range every JSON consumer treats as a safe integer), and
  `value_format` (one of `fasm.model.ValueFormat`'s member names — `PLAIN`,
  `VERILOG_DECIMAL`, `VERILOG_HEX`, `VERILOG_BINARY`, `VERILOG_OCTAL` — or
  `null`).
* `annotations` is `null`, or a list of `{"name": ..., "value": ...}`
  objects.
* `comment` is `null`, or the comment string (without the leading `#`).

On any error (parse error, missing file, requested parser not built, ...)
`dump.py` prints `{"error":"<message>"}` **and exits 0**: the error text is
the result to be compared, not a tool failure.

## Tests

`tests/oracle/test_oracle.py` is a pytest file that sanity checks the
oracle itself: that every parser `fasm.parser.available` reports actually
parses `examples/many.fasm`, that `dump.py`'s output is deterministic
across repeated runs, that it agrees byte for byte between `textx` and
`antlr` when both are available, and that parse errors come back as
`{"error": ...}` with exit code 0. Run it with the oracle venv's pytest:

```sh
tests/oracle/venv/bin/python -m pytest tests/oracle/test_oracle.py -v
```

This is separate from the repository's own `tests/test_simple.py` (which
also exercises `fasm.parser.available` but is part of the original
package's own test suite, run with whatever Python environment the
developer normally uses — it is not part of the oracle and is unmodified
by this task).

## Known limitations

* Only tested on Linux (Ubuntu, with a JDK, CMake, and a C++17 compiler
  already installed). The ANTLR build attempt on other platforms is exactly
  what `setup.py` already does for a normal `pip install fasm` there; this
  script adds nothing platform specific beyond the `apt-get` step, which is
  itself skipped when `apt-get` is not on `PATH`.
* `setup.sh`'s system package install step assumes `apt-get` (Debian /
  Ubuntu) and either root or passwordless `sudo`. On a machine without
  either, the ANTLR build may fail for a missing system library even though
  `cmake`/`java` are present; the textX fallback still works.
* Submodule fetch and `apt-get` both need network access (to GitHub and to
  the distro package mirror respectively, both reachable through this
  repository's configured proxy in the environment this was built in). No
  attempt is made to vendor or cache them; `setup.sh --force` simply retries
  from scratch.
* The oracle only covers `fasm.parser` (`parse_fasm_filename`,
  `parse_fasm_string`), `fasm.model`, `fasm.output`
  (`fasm_tuple_to_string`), and the `fasm` CLI (`fasm.tool.main`) — nothing
  Xilinx/bitstream specific (that is `tests/oracle/setup-xilinx.sh`, a
  separate later task per `docs/rewrite/TASKS.md` T5.8).
* `tests/oracle/build/` holds logs and `status.json` from the most recent
  `setup.sh` run, not a full build log history; `setup.sh --force` starts
  it fresh each time.

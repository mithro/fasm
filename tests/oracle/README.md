# FASM oracle

This directory sets up and runs the **oracle**: the *original*, pre-Rust-
rewrite Python `fasm` package, pinned to an immutable commit and installed
into its own venv so it can be used as a golden reference for differential
testing the Rust rewrite. See `docs/rewrite/PLAN.md` ("Testing strategy")
and `docs/rewrite/TASKS.md` (T0.4, T1.5, T2.2, T3.2) for how it is used.

Nothing under `tests/oracle/venv/` or `tests/oracle/build/` is committed to
git (see the root `.gitignore`): every machine builds its own oracle venv.

## Why the oracle is pinned to a commit, not the live worktree

The oracle is **not** an editable/in-place install of this worktree's live
`fasm/` directory. Phase 3 of the rewrite replaces that directory's
contents (`fasm/parser/rust.py`, a maturin based `setup.py`, ...); an
editable install would silently turn the "golden reference" into the very
code it needs to be diffed against, defeating the entire point of a
differential test.

Instead `tests/oracle/setup.sh`:

1. Creates a **detached `git worktree`** of a pinned commit
   (`ORACLE_COMMIT`, default `ffafe82` — the last upstream
   chipsalliance/fasm merge before the Rust rewrite started, per
   `docs/rewrite/TASKS.md` T0.1/T0.2) at
   `tests/oracle/build/pristine-src`. This is a real, separate checkout on
   disk (sharing git objects with the main `.git`, but with its own
   `HEAD`), not a symlink into `$REPO_ROOT` — later commits in this
   worktree (or any other) cannot change what gets installed.
2. Initialises the submodules the ANTLR C++ build needs
   (`third_party/antlr4`, `third_party/googletest`) **inside that pristine
   worktree**, not the live one.
3. Installs `fasm` from the pristine worktree into `tests/oracle/venv`
   **non-editable** (`pip install tests/oracle/build/pristine-src`), so
   `venv`'s `site-packages` ends up with its own standalone copy,
   independent of `pristine-src` from that point on. If the non-editable
   install fails outright (not just the ANTLR extension falling back to
   textX, which `setup.py` already handles internally — see below — but
   the whole `pip install` call failing), it retries as an **editable**
   install of the pristine worktree as a fallback; that is still pinned
   and immutable with respect to `$REPO_ROOT`'s live `fasm/`, it just
   isn't copied into `site-packages`. `tests/oracle/build/status.json`'s
   `install_mode` field records which one actually happened
   (`"non-editable"` or `"editable-pristine-worktree"`).
4. `tests/oracle/test_oracle.py::test_fasm_module_is_the_pinned_oracle_not_the_live_repo`
   asserts `fasm.__file__` really did resolve to one of those two places
   and never to `$REPO_ROOT/fasm`, so a regression here fails loudly
   instead of silently testing the wrong thing.

### Moving the pin

```sh
ORACLE_COMMIT=<commit-ish> tests/oracle/setup.sh
```

A pin change is **detected automatically** by comparing the requested
`ORACLE_COMMIT` against the one recorded in `tests/oracle/build/status.json`
from the last successful run, and triggers a full rebuild even without
`--force`. To make a new pin the default, edit the `ORACLE_COMMIT="${ORACLE_COMMIT:-...}"`
line near the top of `tests/oracle/setup.sh`. Do this deliberately (e.g. to
track a real upstream `chipsalliance/fasm` change worth re-pinning to), not
casually — the whole point of the pin is that it normally never moves.

## Setup

```sh
tests/oracle/setup.sh          # first run: creates tests/oracle/venv
tests/oracle/setup.sh          # re-run: fast no-op once already set up
tests/oracle/setup.sh --force  # rebuild from scratch (same pin)
```

The script:

1. Creates a venv at `tests/oracle/venv` and installs `textX`, `pytest` and
   `Cython` into it (the pure Python textX parser is required to always
   work).
2. Creates the pinned `git worktree` at `tests/oracle/build/pristine-src`
   (see above).
3. Best effort, never fatal: inside that pristine worktree, runs
   `git submodule update --init --depth 1 -- third_party/antlr4
   third_party/googletest` (needed by the ANTLR C++ extension's CMake
   build) and, in the main environment, `apt-get install -y uuid-dev
   pkg-config` (system libraries the ANTLR build needs; requires root or
   passwordless `sudo`, and network access to the distro package mirror).
4. Installs the pinned `fasm` package from `pristine-src` into the venv
   (see above; non-editable, with an editable-of-`pristine-src` fallback).
   `setup.py` itself already attempts to build the ANTLR C++ parser
   extension via CMake and **falls back to the textX-only install on any
   build failure** (missing submodules, missing `uuid.h`, no C++17
   compiler, ...); this script does not need to (and does not) duplicate
   that fallback logic, it just makes the prerequisites available on a
   best effort basis first.
5. Records which parsers ended up available, the pin, and the install mode
   in `tests/oracle/build/status.json`, and touches
   `tests/oracle/venv/.oracle-setup-ok` as a completion marker so a plain
   re-run is a fast no-op (the ANTLR build alone takes roughly a minute
   the first time, since it compiles the antlr4 C++ runtime from source).
   Logs from each step are kept in `tests/oracle/build/` (`worktree.log`,
   `submodule.log`, `apt.log`, `install.log`) for debugging a failed or
   partial build.

Run `tests/oracle/setup.sh --force` any time you want to re-attempt the
ANTLR build for the current pin (e.g. after installing a missing system
dependency by hand).

## Parsers available on the machine this was last set up on

Both parser implementations built successfully in the container this oracle
was developed and verified in:

```json
{
  "oracle_commit": "ffafe82",
  "oracle_commit_resolved": "ffafe821bae68637fe46e36bcfd2a01b97cdf6f2",
  "install_mode": "non-editable",
  "available_parsers": ["antlr", "textx"],
  "antlr_built": true
}
```

(`uuid-dev`, `pkg-config`, `cmake`, and a JDK for the ANTLR code generator
jar were already present; `third_party/antlr4` and `third_party/googletest`
were fetched as shallow submodule checkouts by `setup.sh` into
`pristine-src`.) Your machine may differ — always check
`tests/oracle/build/status.json`, or run (see the CWD warning below for why
`-P` matters here):

```sh
cd /tmp && /path/to/repo/tests/oracle/venv/bin/python -c "import fasm.parser as p; print(p.available, p.implementation)"
# or, equivalently, from anywhere:
/path/to/repo/tests/oracle/venv/bin/python -P -c "import fasm.parser as p; print(p.available, p.implementation)"
```

If only `['textx']` is available, the ANTLR C++ parser could not be built;
see `tests/oracle/build/install.log` for why (common causes: no network
access to fetch the submodules, missing `uuid.h` (`uuid-dev`), no C++17
compiler, missing `cmake`/`java`). The oracle is still fully usable with
just the textX parser — it is the reference implementation the FASM
specification is defined against and is what `fasm.parser` falls back to at
runtime whenever the ANTLR extension is unavailable, exactly like a real
`pip install fasm` on a machine without the ANTLR build prerequisites.

## Avoiding the current-directory shadowing pitfall

Plain `python -c "import ..."` and `python -m some_module` both prepend the
**current directory** to `sys.path`. If you run either of those with the
repository root as your current directory, `import fasm` resolves to this
worktree's own `fasm/` directory (it shadows the installed package)
*before* it ever gets to `tests/oracle/venv`'s `site-packages` — silently
defeating the whole point of pinning the oracle to an immutable commit.

This does **not** affect `tests/oracle/fasm-oracle`, `run_fasm.py`,
`dump.py`, or the installed `tests/oracle/venv/bin/pytest` /
`tests/oracle/venv/bin/fasm` entry-point scripts: running an actual script
file (or an installed entry point) prepends *that script's own directory*
to `sys.path`, not the current directory, so none of them can be shadowed
this way regardless of where you run them from. Prefer those over ad hoc
`-c`/`-m` invocations. If you do need `-c` or `-m` (e.g. a one-off sanity
check), either run it from a directory with no `fasm/` subdirectory of its
own, or pass Python's `-P` flag (Python >= 3.11; `PYTHONSAFEPATH=1` works
the same way on any 3.x) to disable the unsafe prepend entirely.

`tests/oracle/test_oracle.py::test_fasm_module_is_the_pinned_oracle_not_the_live_repo`
asserts `fasm.__file__` is under `tests/oracle/venv` or
`tests/oracle/build/pristine-src` and never under the live repository root,
so this class of mistake fails loudly instead of silently testing the
wrong `fasm`.

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
oracle itself: that `fasm` really was imported from the pinned build and
not the live repository (see above), that every parser
`fasm.parser.available` reports actually parses `examples/many.fasm`, that
`dump.py`'s output is deterministic across repeated runs, that it agrees
byte for byte between `textx` and `antlr` when both are available, and
that parse errors come back as `{"error": ...}` with exit code 0. Run it
with the oracle venv's **installed pytest entry point**, not
`python -m pytest` (see "Avoiding the current-directory shadowing pitfall"
above for why):

```sh
tests/oracle/venv/bin/pytest tests/oracle/test_oracle.py -v
```

This is separate from the repository's own `tests/test_simple.py` (which
also exercises `fasm.parser.available` but is part of the original
package's own test suite, run with whatever Python environment the
developer normally uses — it is not part of the oracle and is unmodified
by this task).

## Known limitations

* `tests/oracle/build/pristine-src` is a real `git worktree` registered
  against this repository's `.git` (visible in `git worktree list`). It is
  never committed (it lives under the gitignored `tests/oracle/build/`),
  but it *is* a live piece of git metadata: don't `rm -rf` it by hand
  without also running `git worktree remove --force
  tests/oracle/build/pristine-src` (or just `tests/oracle/setup.sh --force`,
  which does this for you) — otherwise `git worktree list` keeps a stale
  entry until something runs `git worktree prune`.
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

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

## Xilinx reference tools

`tests/oracle/setup-xilinx.sh` sets up a **second, separate** oracle:
the reference Xilinx FASM<->frames<->bitstream tools (prjxray's C++
tools and Python package, f4pga-xc-fasm, and prjuray/prjuray-tools for
UltraScale/UltraScale+, best effort), pinned to immutable commits, used
by the Rust rewrite's Xilinx differential tests (T5.9, T6.3;
`docs/rewrite/DESIGN-xilinx-db.md` is the design reference these tools'
CLI flags and on-disk database layout are documented against). It is
**not** part of `tests/oracle/setup.sh`/`tests/oracle/venv` above -- that
venv stays a minimal, pristine `fasm`-only install on purpose (see "Why
the oracle is pinned to a commit" above); this is a second venv,
`tests/oracle/venv-xilinx`, that additionally carries `prjxray`,
`xc_fasm` and `prjuray` and their dependencies, but installs the exact
same pinned `fasm` package from the exact same pristine worktree
(`tests/oracle/build/pristine-src`, created by `setup.sh` and reused here
if present) so both oracles agree on what "the original fasm" means.

### Setup

```sh
tests/oracle/setup-xilinx.sh          # first run: ~2-2.5 minutes
tests/oracle/setup-xilinx.sh          # re-run: fast no-op (<0.1s)
tests/oracle/setup-xilinx.sh --force  # rebuild from scratch (same pins)
```

It creates, all under the gitignored `tests/oracle/build/xilinx/` and
`tests/oracle/venv-xilinx/` (never `tests/oracle/venv`, see above):

1. Calls `tests/oracle/setup.sh` first (a fast no-op if it already ran)
   to guarantee the pinned `fasm` pristine worktree exists.
2. Clones `prjxray`, `f4pga-xc-fasm`, `prjuray` and `prjuray-tools`,
   each pinned to an immutable commit (see "Pinned commits" below).
   Idempotent per repo: a clone already at the pinned commit is left
   alone. A pin change (any of the four `*_COMMIT` variables) is
   auto-detected against `tests/oracle/build/xilinx/status.json` and
   triggers a rebuild, the same scheme as `setup.sh`'s `ORACLE_COMMIT`.
3. Fetches the C++ build's git submodules (`third_party/{abseil-cpp,
   cctz,gflags,googletest,yaml-cpp,sanitizers-cmake}`, shallow) for
   `prjxray` and `prjuray-tools`. Deliberately **not** fetched:
   `third_party/{fasm,python-sdf-timing,yosys,display_port,edalize,
   embeddedsw}` -- unrelated to the C++ tools built here and, in
   `yosys`'s case, huge. `prjxray-db` is not a submodule of `prjxray`
   (it's a separate repo, see "Database fetch" below) so there was
   nothing to avoid fetching there.
4. Builds the prjxray C++ tools (CMake + Ninja, `Release`):
   `xc7frames2bit`, `bitread`, `frame_address_decoder`,
   `gen_part_base_yaml`, `bittool`, `xc7patch`. Copies them to
   `tests/oracle/build/xilinx/bin/`.
5. Best effort (budgeted at 20 minutes, via `timeout`; a failure here is
   recorded in `status.json` and never fails the rest of the script):
   builds prjuray-tools' C++ tools the same way --
   `xcframes2bit`, `bitread`, `xc7_frame_address_decoder`,
   `xcu_frame_address_decoder`, `gen_part_base_yaml`, `bittool` -- and
   copies them to `tests/oracle/build/xilinx/bin/` with a `uray-` prefix
   (e.g. `uray-xcframes2bit`). On the container this was developed in,
   this build succeeded too (no fallback was needed -- see "What
   worked" below).
6. Creates `tests/oracle/venv-xilinx` and installs, in this order (order
   matters -- see the comments in `tests/oracle/setup-xilinx.sh`):
   1. `textX`+`Cython` (needed to install the pinned `fasm` package
      below, same as `setup.sh`), then the pinned `fasm` package itself
      from `tests/oracle/build/pristine-src` (non-editable, with the
      same editable-of-pristine-worktree fallback as `setup.sh`).
   2. `prjxray`'s Python package, installed **`--no-deps`** (its
      `setup.py` declares `install_requires=['fasm', ...]`, which would
      otherwise silently pull a different `fasm` wheel from PyPI over
      the pinned one just installed), then its other dependencies
      explicitly (`intervaltree numpy pyjson5 pyyaml simplejson`).
   3. `f4pga-xc-fasm`, same story (`install_requires` includes both
      `prjxray` and `fasm` from PyPI) -- `--no-deps`, then
      `intervaltree simplejson textx` explicitly.
   4. `prjuray-tools`'s Python package (`prjuray.db`, `prjuray.grid`,
      `prjuray.tile_segbits`, ...) -- no `fasm`/`prjxray` pin to defend
      against, installed normally.
   5. `pytest`.
7. Verifies `fasm`/`prjxray`/`xc_fasm`/`prjuray` all import from inside
   `venv-xilinx`, never from the live repository's `fasm/` (same check
   as `setup.sh`, run from a directory with no `fasm/` subdirectory of
   its own so it cannot be defeated by the CWD-shadowing pitfall below).
8. Records `tests/oracle/build/xilinx/status.json` and touches
   `tests/oracle/venv-xilinx/.oracle-xilinx-setup-ok`.

The `f4pga/prjuray` repo itself (`utils/fasm2frames.py`,
`utils/bit2fasm.py`, ...) is cloned too, but has no `setup.py` -- it is
not pip-installable. It's kept for Phase 6 (T6.x) use via a `PYTHONPATH`
wrapper (its `utils` package needs the `prjuray` repo root on
`PYTHONPATH`, alongside `prjuray-tools` for `prjuray.db`); T5.8 itself
only needed the `prjuray-tools` Python package (prjxray-shaped API) and
the C++ tools above, both covered.

### Pinned commits

Resolved by cloning each repo fresh on 2026-09-24 (their default-branch
HEAD at the time); see `tests/oracle/setup-xilinx.sh`'s
`PRJXRAY_COMMIT`/`F4PGA_XC_FASM_COMMIT`/`PRJURAY_COMMIT`/
`PRJURAY_TOOLS_COMMIT` (override any to re-pin -- auto-detected, same as
`setup.sh`'s `ORACLE_COMMIT`) and `tools/fetch-db.sh`'s
`PRJXRAY_DB_COMMIT`/`PRJURAY_DB_COMMIT`:

| Repo | Commit |
|---|---|
| `f4pga/prjxray` | `c9f02d8576042325425824647ab5555b1bc77833` |
| `chipsalliance/f4pga-xc-fasm` | `25dc605c9c0896204f0c3425b52a332034cf5e5c` |
| `f4pga/prjuray` | `c550b03a26b4c4a9c4453353bd642a21f710b3ec` |
| `SymbiFlow/prjuray-tools` | `f53f07b8fe37721137a57e9bee3b2b13e7676f53` |
| `f4pga/prjxray-db` (database, `tools/fetch-db.sh`) | `0a0addedd73e7e4139d52a6d8db4258763e0f1f3` |
| `f4pga/prjuray-db` (database, `tools/fetch-db.sh`) | `affbc5e555ebae16475f32e8fb2d6565d4204f3f` |

These happen to match the commits `docs/rewrite/DESIGN-xilinx-db.md`
section 1 was researched against (its scratchpad checkouts, not
committed anywhere) -- both were resolved from each repo's HEAD only
days apart, and none of these repos moved in between.

### Wrappers

Thin `exec` wrappers in `tests/oracle/`, mirroring the `fasm-oracle` /
`run_fasm.py` pattern above (each requires `setup-xilinx.sh` to have run
first):

| Wrapper | Runs |
|---|---|
| `fasm2frames-oracle` | `xc_fasm.fasm2frames:main` in `venv-xilinx` (`python -P -m ...` -- see below for why `-P`) |
| `xcfasm-oracle` | `venv-xilinx`'s installed `xcfasm` console script |
| `bit2fasm-oracle` | `venv-xilinx`'s installed `bit2fasm` console script |
| `xc7frames2bit-oracle` | the built `xc7frames2bit` C++ binary |
| `bitread-oracle` | the built `bitread` C++ binary |
| `gen_part_base_yaml-oracle` | the built `gen_part_base_yaml` C++ binary |

`source tests/oracle/xilinx-env.sh` puts `tests/oracle/build/xilinx/bin`
on `PATH` and exports `PRJXRAY_DB_ROOT`/`PRJURAY_DB_ROOT` (this repo's
own convenience variables pointing at `tools/fetch-db.sh`'s cache --
**not** read by the reference tools themselves, which take
`--db-root`/`--part` explicitly or fall back to prjxray's own
`XRAY_DATABASE_DIR`+`XRAY_DATABASE`/`XRAY_PART`; see
`docs/rewrite/DESIGN-xilinx-db.md` section 2.1). This matters for
`xcfasm-oracle`: `xcfasm` shells out to `xc7frames2bit` **by bare name**
(`--frm2bit` defaults to the literal string `"xc7frames2bit"`, looked up
on `PATH`), so it needs the built tools on `PATH` to work at all; same
for `bit2fasm-oracle` and `bitread`.

`fasm2frames-oracle` has no installed console-script entry point to run
(f4pga-xc-fasm's `setup.py` only registers `xcfasm`/`bit2fasm`; the
`fasm2frames` script that *is* installed is prjxray's own, unrelated,
broken one -- `utils.fasm2frames:main`, which fails with
`ModuleNotFoundError: No module named 'utils'` because `prjxray`'s
`setup.py` only packages `prjxray/`, not the top-level `utils/` script
directory it lives in). So `fasm2frames-oracle` runs
`python -m xc_fasm.fasm2frames` directly, and passes `-P` to disable
Python's normal "prepend the current directory to `sys.path` for `-m`"
behaviour -- without it, running `fasm2frames-oracle` from the
repository root would shadow `venv-xilinx`'s pinned `fasm`/`xc_fasm`
with the live repository's own `fasm/` directory, exactly the
CWD-shadowing pitfall described above for the plain oracle. This was
caught during development of this script (the warning
`Falling back to the much slower pure Python textX based parser` with a
path pointing at this worktree's own `fasm/parser/__init__.py`, instead
of `venv-xilinx`) and is why `xcfasm-oracle`/`bit2fasm-oracle` use the
installed console scripts instead (a script file's own directory,
`venv-xilinx/bin`, gets prepended, never the caller's CWD -- immune by
construction, like `tests/oracle/fasm-oracle` above).

### Database fetch (`tools/fetch-db.sh`)

```sh
tools/fetch-db.sh prjxray artix7            # -> <cache>/prjxray-db/artix7   (~181 MiB, ~6s)
tools/fetch-db.sh prjxray kintex7 spartan7 zynq7   # add more families
tools/fetch-db.sh prjuray zynqusp           # -> <cache>/prjuray-db/zynqusp  (~217 MiB, ~3s)
tools/fetch-db.sh all                       # every family of both databases
```

`<cache>` defaults to `${FASM_DB_CACHE:-tests/oracle/build/db}`
(gitignored, same as everything else under `tests/oracle/build/`).
prjxray-db's four families are `artix7`, `kintex7`, `spartan7`, `zynq7`
(confirmed with `git ls-tree -d HEAD` on a `--filter=blob:none --sparse`
clone -- there is no way to list a remote's tree without cloning it
first); prjuray-db currently has exactly one, `zynqusp`.

Each database is a single sparse (`--filter=blob:none --sparse
--depth 1`), cone-mode clone per repo (`<cache>/prjxray-db`,
`<cache>/prjuray-db`); requesting a family checks out that top-level
directory (`git sparse-checkout add <family>`), which pulls in
everything a `--db-root <cache>/prjxray-db/<family>` invocation of a
reference or Rust tool needs directly -- `settings.sh`, `mapping/`,
every `<fabric>/` and `<part>/` under it, `segbits_*.db`, `ppips_*.db`,
`mask_*.db` -- since `--db-root` addresses the family directory itself,
not its parent (see `docs/rewrite/DESIGN-xilinx-db.md` section 2.1:
`get_fabric_for_part`/`get_part_information` join `db_root` directly
with `"mapping"`, so `db_root` in prjxray's own code *is* the family
directory, e.g. `prjxray-db/artix7`). No extra step was needed to make
`settings.sh` show up -- it's an ordinary file inside the family
directory cone-mode already checks out whole.

Idempotent: a family already present (checked out **and** currently
listed by `git sparse-checkout list`) is left alone and not re-fetched;
adding a new family to an already-cloned repo uses `git sparse-checkout
add` (only fetches the new family's objects, doesn't touch what's
already there). A pin change (`PRJXRAY_DB_COMMIT`/`PRJURAY_DB_COMMIT`)
is detected by comparing the clone's current `HEAD` against the pinned
commit and re-fetches/checks out in place.

Measured sizes on the container this was developed in (`du -sh`):

| Family | Size | Fetch time (cold) |
|---|---|---|
| `prjxray-db/artix7` | 181 MiB | ~6.3s |
| `prjxray-db/spartan7` (added to the same clone) | 64 MiB | ~1.4s |
| `prjuray-db/zynqusp` | 217 MiB | ~3.2s |

### Smoke tests (`tests/oracle/test_xilinx_oracle.py`)

```sh
tests/oracle/venv-xilinx/bin/pytest tests/oracle/test_xilinx_oracle.py -v
```

Run with `venv-xilinx`'s installed pytest entry point, for the same
CWD-shadowing reason as `tests/oracle/test_oracle.py` above. Two
independent checks, each skipped cleanly (`pytest.skip`, not a failure)
when its prerequisite hasn't been set up:

1. **`test_f4pga_xc_fasm_test_suite_passes`** -- runs f4pga-xc-fasm's own
   `tests/test_fasm2frames.py` (against its bundled miniature database,
   `tests/test_data/db` -- no database fetch needed) inside
   `venv-xilinx`. On the container this was developed in: **7 passed, 5
   skipped** (upstream `@unittest.skip`s documenting known-unimplemented
   behavior -- see `docs/rewrite/DESIGN-xilinx-db.md` section 7), with
   one upstream test, `test_badkey`, deselected (`-k 'not test_badkey'`)
   rather than silently left to fail: it does
   `except TextXSyntaxError` around a parse error, but which exception
   class a bad-key parse error actually raises is decided once, at
   `fasm` package build time, by whether the antlr4 C++ extension built
   (see `fasm/parser/__init__.py`) -- on this container it did (same as
   the plain oracle, see "Parsers available" above), so the same call
   raises a generic antlr4 C++ binding `Exception` instead, which
   `test_badkey`'s narrow `except` doesn't catch. This is a pre-existing
   assumption in f4pga-xc-fasm's own test (not something this task's
   scripts introduce) and is documented in detail in
   `test_xilinx_oracle.py`'s docstring.
2. **`test_smoke_fasm_matches_golden_frm`**,
   **`test_smoke_bit_bitread_matches_golden`**,
   **`test_smoke_bitread_lists_expected_bits`** -- run
   `fasm2frames-oracle` / `xc7frames2bit-oracle` / `bitread-oracle` over
   `tests/corpus/xilinx/artix7/smoke_x1y0.fasm` (a tiny hand written FASM
   using three real artix7 features -- see
   `tests/corpus/xilinx/artix7/README.md`) against a real fetched artix7
   database, and byte-compare against the checked-in golden
   `smoke_x1y0.frm`/`smoke_x1y0.bitread.txt` (**not** `smoke_x1y0.bit`
   itself -- see the "not byte-for-byte reproducible" note in that
   README for why). Skipped if `tools/fetch-db.sh prjxray artix7` hasn't
   been run. All four tests passed on the container this was developed
   in (`4 passed in 0.88s`).

### All-parts differential test (`make xilinx-difftest-all`, T5.9)

`make xilinx-difftest` compares the Rust tools with these reference tools
on the checked-in corpus (one artix7 part). `make xilinx-difftest-all`
does it for **every part** of the four prjxray-db families (88 artix7,
16 kintex7, 9 spartan7, 12 zynq7), on a synthetic corpus generated per
part by `tools/gen-xilinx-corpus.py` (every segbits feature, block RAM
segbits feature and pseudo PIP of every tile type of the part's grid,
conflict free, over as many files as exclusive features need, plus an
error corpus):

```sh
make xilinx-difftest-all                       # 4 families, all parts, 4 jobs
make xilinx-difftest-quick                     # one part per family
make xilinx-difftest-all XILINX_DIFFTEST_ALL_ARGS="--parts 'xc7a35t*'"
python3 tools/difftest-xilinx.py --family zynq7 --parts xc7z010clg400-1 \
    --oracle tests/oracle/fasm2frames-oracle ...   # one part
```

* Wall time (first run, `--jobs 4`, this container): **68.6 minutes** for
  the 125 parts (46-271 s per part, mean 130 s, almost all of it in the
  reference tools; xc7a200t parts are the slowest); a rerun from the
  result cache takes a few minutes. `make xilinx-difftest-quick`
  (`--parts-sample 1`: one part per family, different fabrics) takes
  about 2-3 minutes. The harness prints an estimate at the start and an
  ETA after each part.
* Result of the first run (generator version 1; version 2 fixed the
  STEPDOWN units of the second `_SING` alias group and adds the alias
  tiles' pseudo PIPs, see §8.9): 2651 fasm2frames runs (2526 identical, 125
  explained: the value range error of `errors/value_range.fasm`, rule 4
  of the `fasm2frames` section of `docs/rewrite/COMPAT.md`), 8814
  xc7frames2bit/bitread runs and 375 xcfasm runs, all identical; 0
  unexplained differences.
* Families missing from `$FASM_DB_CACHE` (default `tests/oracle/build/db`)
  are fetched with `tools/fetch-db.sh` (after a free space check; the
  four families take about 400 MiB checked out). `--db-cache` also takes
  several directories separated by `:`.
* Everything else goes to `XILINX_DIFFTEST_WORK` (default
  `tests/oracle/build/difftest-xilinx`): `corpus/<family>/<part>/<opts>/`
  (the generated files, reused while the generator, its options and the
  database commit are unchanged; about 365 MB for all parts with the
  default `--tiles sample 3`), `results/` (the reference results, keyed by
  command line, input file contents, the reference tools and the database
  commit; about 1.4 MB per part, 549 MB for the whole work directory
  of a full run; a rerun only runs the Rust tools; delete
  it or pass `--no-result-cache` after changing the oracle in a way its
  key does not see) and `run/` (per part scratch, removed when the part is
  done, including the part's Rust `FASM_XDB_CACHE` directory).
* The output is one line per part as it finishes, then a table (part,
  fabric, lines, files, fasm2frames and xcfasm runs identical / explained
  / different, bitstream tool runs, seconds) and the totals;
  `--json-report FILE` writes the rows. Exit status 1 on any unexplained
  difference.
* `tests/cli/test_xilinx_corpus.py` is the fast version for CI: the
  xc7a35tcsg324-1 corpus (`--tiles sample 3`) through the Rust
  `fasm2frames` with and without its database cache, against golden
  reference results (`tests/corpus/xilinx/artix7/generated/`, written by
  `python3 tests/cli/test_xilinx_corpus.py --write-goldens`).
  `tests/cli/test_gen_xilinx_corpus.py` needs no reference tools: the
  generator's own model of prjxray (`--expected-frm`) against the Rust
  `fasm2frames` on the test databases.

See `docs/rewrite/DESIGN-xilinx-db.md` §8.9 for what is generated and
compared, the run matrix and the timings.

### What worked / what didn't

On the container this was developed in (Ubuntu, `cmake` 3.28,
`ninja-build`, `g++`, already had `uuid-dev`/`pkg-config` from the plain
oracle's own best-effort apt step): **everything worked**, including the
best-effort prjuray-tools C++ build -- no extra system packages beyond
what `setup.sh` already ensures were needed for either C++ build.
`tests/oracle/setup-xilinx.sh` still runs its own best-effort
`apt-get install cmake ninja-build build-essential uuid-dev pkg-config`
step for portability to a machine missing one of these (never fatal; a
missing prerequisite surfaces as a clear cmake/ninja failure recorded in
`tests/oracle/build/xilinx/logs/` and `status.json`'s
`prjuray_cpp_error` field instead).

### Timings (container this was developed in)

| Step | Time |
|---|---|
| `tests/oracle/setup-xilinx.sh` (cold; includes calling `setup.sh` if needed, both C++ builds, all pip installs) | ~2m27s wall (`setup_seconds` in `status.json`: 147s excluding the `setup.sh` prerequisite step) |
| `tests/oracle/setup-xilinx.sh` (warm, no-op) | <0.1s |
| prjxray C++ build alone (`cmake` configure + `ninja` 6 targets) | ~1s configure + ~45s build |
| prjuray-tools C++ build alone (`cmake` configure + `ninja` 6 targets) | ~1s configure + ~47s build |
| `tools/fetch-db.sh prjxray artix7` (cold) | ~6.3s |
| `tools/fetch-db.sh prjuray zynqusp` (cold) | ~3.2s |
| smoke pytest suite | ~0.9s |

### Disk usage

| Path | Size |
|---|---|
| `tests/oracle/build/xilinx/src` (4 repo clones + C++ build trees) | 169 MiB |
| `tests/oracle/build/xilinx/bin` (12 copied binaries) | 11 MiB |
| `tests/oracle/venv-xilinx` | 141 MiB |
| `tests/oracle/build/db` (artix7 only) | 190 MiB |

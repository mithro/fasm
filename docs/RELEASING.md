# Releasing

How to cut a release of this repository: the Rust crates (`fasm`,
`fasm-xilinx`, `fasm-cli`, `fasm-capi`) and the `fasm` Python package. See
`docs/rewrite/TASKS.md` T8.4 for how this file came to be and
`AGENTS.md`/`docs/rewrite/WORKFLOW.md` for the agent workflow this
repository is otherwise developed under.

Nothing here has been published yet: T8.4 prepared the metadata and CI
workflows only. Publishing is a decision for a maintainer with the
necessary crates.io/PyPI access, not something an agent does on its own
initiative.

## Before anything: the crates.io name collision

The crate name `fasm` is already registered on crates.io by an unrelated
project ([zk2u/fasm](https://github.com/zk2u/fasm), "Fallible Async State
Machines", currently at 0.4.x). `fasm-xilinx`, `fasm-cli` and `fasm-capi`
all depend on `fasm` as a path dependency with `version = "0.1.0-dev"`
(required for a path dependency to be publishable, see
[the crates.io publish docs](https://doc.rust-lang.org/cargo/reference/publishing.html#dependencies-with-path-and-version));
that requirement can never resolve against the existing crate.

Before any of these crates can be published, pick one:

* Publish the core crate under a different name (e.g. `fasm-format`,
  `fasm-fpga`, `chipsalliance-fasm`) and update the `[package] name` and
  every dependent crate's `fasm = { path = "...", version = "..." }` to
  match. The library's Rust item names (`fasm::...`) and the `[lib] name
  = "fasm"` do not have to change — only the crates.io *package* name.
* Contact the existing crate's author/crates.io support about the name
  (unlikely to succeed for an actively used name, and not something to
  pursue without the user's direction).

This does not affect the Python package: PyPI's `fasm` project name is a
separate namespace and was not found to collide (verify again at release
time with `pip index versions fasm` or the PyPI web UI, since names can
be registered at any time).

## Version scheme

One pre-release version, `0.1.0-dev`, is set by hand in two places kept
in step manually (there is no automation tying them together, see the
comments at both sites):

| Where | Field | Current value |
|---|---|---|
| `Cargo.toml` | `[workspace.package] version` | `0.1.0-dev` (all 5 crates inherit it with `version.workspace = true`) |
| `pyproject.toml` | `[project] version` | `0.1.0.dev0` (PEP 440's spelling of the same semver pre-release) |

To cut a release:

1. Decide the new version (e.g. `0.1.0` for the first real release, or
   `0.1.1`/`0.2.0` after). Semver: `fasm`, `fasm-xilinx`, `fasm-cli`,
   `fasm-capi` and the Python package are versioned together (one number
   for the whole rewrite) since they are developed and released as one
   unit; there is no reason yet to let them drift apart.
2. Update `Cargo.toml`'s `[workspace.package] version` and
   `pyproject.toml`'s `[project] version` (PEP 440 spelling: no `-`, use
   `.` — e.g. Cargo `1.2.3-rc.1` is PyPI `1.2.3rc1`; a plain release like
   `0.1.0` is the same string in both files).
3. `cargo update -p fasm -p fasm-xilinx -p fasm-cli -p fasm-capi
   --precise <version>` is unnecessary (path dependencies always build
   from the workspace's own source); just confirm `cargo package -p fasm
   --list` and `cargo metadata` pick up the new version (`cargo check
   --workspace` regenerates `Cargo.lock`).
4. Add a `CHANGELOG.md` entry for the new version (see below).
5. Commit (`plan: release vX.Y.Z` or similar), following
   `docs/rewrite/WORKFLOW.md`'s review process like any other change.
6. Tag: `git tag -a vX.Y.Z -m "vX.Y.Z"` (the wheels workflow's disabled
   `publish` job and a would-be `crates.io` publish both key off a `v*`
   tag). Do not push the tag until ready to publish (tags are also how a
   human decides "publish now").

## Compatibility checks to run first

Every one of these must be clean before tagging a release (they are also
what CI runs; running them locally first catches anything CI's
allowances leave out, like the Xilinx oracle/database-backed suites):

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
make capi-header-check capi-test
make difftest              # core parser vs. the Python/ANTLR oracle
make cli-difftest           # fasm CLI vs. the Python fasm/tool.py
make xilinx-difftest        # fasm2frames/xcfasm/xc7frames2bit vs. prjxray oracle
make uray-difftest           # UltraScale(+) tools vs. prjuray oracle
```

`xilinx-difftest`/`uray-difftest` need the oracle Xilinx databases set up
first (`tests/oracle/setup-xilinx.sh`; see `tests/oracle/README.md` and
`docs/rewrite/DESIGN-xilinx-db.md` -- multi-gigabyte downloads, not run in
ordinary CI, see the comment at the top of `.github/workflows/rust.yml`).
`make difftest`/`cli-difftest` need `tests/oracle/setup.sh` (the ANTLR
Python oracle), also not built in ordinary CI for the same reason.

Also build and smoke-test what will actually be published:

```
cargo package -p fasm                       # full package + verify build
maturin build --release --sdist             # wheel + sdist, in a scratch venv
python -m twine check dist/*                # if twine is installed
pip install dist/*.whl && python -c "import fasm, fasm.xilinx"
make capi-install PREFIX=/tmp/scratch-fasm  # C API install
```

(All of the above were run manually as part of T8.4; see
`docs/rewrite/LOG.md` for the exact results at that point in time --
package/wheel/sdist sizes, `cargo audit` output, etc. Re-run them at
release time rather than trusting stale numbers.)

## Publishing order and steps

Publish lowest-level first, since each later step's path dependencies
need the previous one already on crates.io at a matching version:

1. **`fasm`** (after renaming if the collision above is not resolved
   another way): `cargo publish -p fasm` (or `-p <new-name>`).
2. **`fasm-xilinx`**: `cargo publish -p fasm-xilinx` (depends on `fasm`
   from step 1).
3. **`fasm-cli`** and **`fasm-capi`** (either order; both depend only on
   `fasm`/`fasm-xilinx`): `cargo publish -p fasm-cli`, `cargo publish -p
   fasm-capi`.
4. **Python wheels/sdist to PyPI**: configure PyPI trusted publishing
   first (one-time setup, see below), then either let the tag push
   trigger it (once the `wheels.yml` `publish` job's `if: false` guard is
   removed/flipped in a follow-up commit -- see that job's comment) or
   run it manually: `gh workflow run wheels.yml -f publish=true --ref
   vX.Y.Z` after pushing the tag.
5. **GitHub release** (optional, not automated by any workflow here):
   create one from the tag, e.g. `gh release create vX.Y.Z --generate-notes`,
   pointing at the `CHANGELOG.md` entry.

`fasm-python` (the pyo3 extension crate) is never published to
crates.io (`publish = false`, see its `Cargo.toml`); it only exists to be
built into the wheel by step 4.

### Configuring PyPI trusted publishing (one-time, by a PyPI project owner)

1. If the `fasm` project does not exist on PyPI yet, its first release
   must be uploaded once with a normal API token (trusted publishing can
   only be configured for a project that already exists, or through
   PyPI's "pending publisher" flow for a brand new project -- see
   <https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/>
   if starting from scratch).
2. On <https://pypi.org/manage/project/fasm/settings/publishing/> (or the
   pending-publisher form for a new project), add a trusted publisher
   with:
   * Owner: the GitHub org/user this repository is pushed to for release
     (this task's working branch is `mithro/fasm`'s
     `claude/epic-goldberg-uc7xqf`; the actual release remote is whatever
     the user designates).
   * Repository: `fasm`
   * Workflow: `wheels.yml`
   * Environment: `pypi`
3. No PyPI token needs to be stored as a GitHub secret: `id-token: write`
   in `wheels.yml`'s `publish` job is what lets GitHub Actions mint the
   short-lived OIDC token PyPI exchanges for an upload token.

### crates.io publishing credentials

`cargo publish` needs `cargo login` with a crates.io API token (crates.io
does not yet support GitHub Actions OIDC trusted publishing the way PyPI
does, as of this writing); run it interactively with a maintainer's own
token, or add `CARGO_REGISTRY_TOKEN` as a repository secret and a publish
step to a workflow if this should be automated later (not done as part of
T8.4: publishing is out of scope, see `AGENTS.md`).

## What T8.4 left for a maintainer to do

* Resolve the crates.io `fasm` name collision (rename or otherwise).
* Configure PyPI trusted publishing (needs a PyPI account with rights to
  create/administer the `fasm` project).
* Decide whether the parser-only wheel (`wheels.yml`'s
  `wheel-parser-only` job, `--no-default-features`) should ever be
  published, and if so, how it is distinguished from the default wheel
  (see that job's comment: PyPI allows only one wheel per
  platform/ABI/version per release).
* Get a crates.io API token and flip `wheels.yml`'s `publish` job's `if:
  false` guard (and add an equivalent `cargo publish` step, currently
  absent) when ready to automate releases end to end.
* `cargo-deny` was not installed in the environment T8.4 ran in and was
  not added; `cargo audit` was installed for a one-off check (0
  vulnerabilities across 55 dependencies at the time) but is not wired
  into CI. Consider adding either (or both) as a CI job if ongoing
  dependency auditing is wanted.

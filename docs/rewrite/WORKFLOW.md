# Agent workflow for the Rust rewrite

These rules apply to every agent working on this effort.

## Restarting from any point

1. Read `docs/rewrite/PLAN.md`, then `docs/rewrite/TASKS.md`, then the tail
   of `docs/rewrite/LOG.md`.
2. The first unchecked task in `TASKS.md` whose dependencies are done is the
   next thing to do. Tasks marked `IN PROGRESS` may have a partially merged
   branch; check `git branch -a` and `LOG.md` for the branch name.
3. Build and test with `cargo test --workspace` and `make -C tests` (see
   `TASKS.md` for what exists at the current point).

## Rules

* Work in **small, incremental commits**. One logical change per commit.
* **Merges, never rebases.** Feature work happens on a branch/worktree and is
  merged into `claude/epic-goldberg-uc7xqf` with `git merge --no-ff`.
* Every code change is **reviewed by an independent sub-agent** before it is
  merged. Reviewer findings are fixed before merging (or logged in `LOG.md`
  with a reason when deliberately deferred, and added to `TASKS.md`).
* At most **2 sub-agents run at any one time**. Cheaper models (Sonnet) for
  well specified implementation and review tasks, Opus for design heavy
  tasks.
* `TASKS.md` and `LOG.md` are updated and committed after every change to
  the state of the work (task started, review done, merged, blocked).
* No pushes to any repository other than `mithro/fasm`; nothing is sent
  upstream. Reference repositories are only read.
* Never reduce test coverage to make something pass. Compatibility gaps that
  cannot be closed are documented in `docs/rewrite/COMPAT.md`.
* Keep the Python package importable at all times (the textX fallback must
  keep working when the extension is not built).

## Per task procedure (orchestrator)

1. Mark task `IN PROGRESS` in `TASKS.md` with the branch name; commit.
2. Launch implementer sub-agent in an isolated worktree with a precise brief
   (inputs, outputs, tests, commit granularity, what NOT to touch).
3. Launch reviewer sub-agent on the resulting branch (read only). The review
   brief names the task, the acceptance criteria and asks for a verdict of
   `APPROVE` / `REQUEST CHANGES` with concrete findings.
4. Send findings back to the implementer (or a fresh fixer agent); repeat
   until `APPROVE`.
5. Merge with `--no-ff`, run the full test suite on the merged tree, update
   `TASKS.md`/`LOG.md`, commit, push.

## Commit message conventions

`<area>: <imperative summary>` where area is one of `plan`, `rust/fasm`,
`rust/cli`, `rust/xilinx`, `rust/capi`, `rust/python`, `python`, `tests`,
`tools`, `docs`, `ci`.

# Notes for agents working in this repository

This repository is in the middle of a rewrite of the FASM tooling in Rust.
All planning and progress state lives in `docs/rewrite/`:

* `docs/rewrite/PLAN.md`     the architecture and phase plan
* `docs/rewrite/WORKFLOW.md` the rules every agent must follow
* `docs/rewrite/TASKS.md`    the live task list (what is done, in progress, next)
* `docs/rewrite/LOG.md`      append only progress log with branch/commit refs

To resume the work: read those four files in that order, pick the next task
whose dependencies are complete, and follow WORKFLOW.md.

Working branch: `claude/epic-goldberg-uc7xqf` (repository `mithro/fasm`).
Never push anywhere else and never send changes upstream.

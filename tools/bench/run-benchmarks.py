#!/usr/bin/env python3
# -*- coding: utf-8 -*-
#
# Copyright 2017-2022 F4PGA Authors
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# SPDX-License-Identifier: Apache-2.0
"""End-to-end benchmark driver: the Rust tools vs. the Python/C++
reference tools they replace (T8.1).

stdlib only (no `pytest-benchmark`, no third party packages beyond what
the reference venvs already have). Runs each tool as a subprocess, `N`
times (`--repeats`, default 5), and reports wall clock time (median and
min, over the runs that exited 0 -- a failed run's time is not counted,
see `run_timed()`) and peak resident set size (`os.wait4`'s `ru_maxrss`,
a per-child measurement, no `/usr/bin/time` dependency -- it is not
installed on the container this was developed on -- but **not** a clean
per-program number either: see `measure_rss_floor()`'s docstring for why,
and the `(floor)` markers in the Markdown report).

Suites (each independently skippable with `--skip NAME`, all run by
default; a suite that needs a tool it cannot find prints why and is
skipped, not a hard failure):

* `parser`    -- `fasm` (parse/print/`--canonical`) vs. the oracle's ANTLR
                 and textX parsers (`tests/oracle/fasm-oracle`), on small
                 (`counter_test`), medium (`picosoc_demo`), large
                 (`linux_litex_demo`) corpus files and a generated 1M line
                 synthetic file (textX on that last one is bounded to a
                 few minutes and reported as "timed out" rather than run
                 to completion -- see `suite_parser()`).
* `xilinx7`   -- `fasm2frames`/`xcfasm` vs. `xc_fasm` (Python), on the
                 same three designs plus each pass file of a
                 `tools/gen-xilinx-corpus.py` "every feature" sample for
                 xc7a35t (and xc7a200t if `--full-corpus`; see
                 `run_every_feature_corpus()` for why each pass file is
                 timed separately rather than concatenated), with and
                 without the binary database cache (`--xdb-cache-dir`,
                 primed once, untimed, before any cached measurement --
                 see `prime_xdb_cache()`).
* `ultrascale`-- `uray-fasm2frames` vs. prjuray's `utils/fasm2frames.py`,
                 on xczu3eg, same every-feature-corpus treatment.
* `python`    -- `fasm.parse_fasm_string` / `fasm.xilinx.fasm2frames`
                 (built with maturin into a scratch venv) vs. the oracle's
                 pure Python paths.

Usage:

    tools/bench/run-benchmarks.py --out-json report.json --out-md report.md
    tools/bench/run-benchmarks.py --quick  # fewer repeats, small inputs
    tools/bench/run-benchmarks.py --skip python --skip ultrascale

Everything needed to reproduce a run is in the JSON report's "commands"
field for every measurement (the literal argv, cwd and environment
overrides).
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Optional

REPO_ROOT = Path(__file__).resolve().parents[2]
# The reference venvs/binaries under tests/oracle/{venv,venv-xilinx,build}
# are gitignored build output (tests/oracle/setup.sh, setup-xilinx.sh), so
# a git worktree of this repository does not have them even though its
# tests/oracle/*-oracle wrapper *scripts* are tracked and present. Point
# ORACLE_DIR at wherever they were actually built (default: this
# checkout's tests/oracle, overridable with $FASM_ORACLE_DIR or
# --oracle-dir, e.g. the main checkout's tests/oracle from a worktree).
ORACLE_DIR = Path(
    os.environ.get("FASM_ORACLE_DIR", str(REPO_ROOT / "tests" / "oracle"))
)
CORPUS_XILINX = REPO_ROOT / "tests" / "corpus" / "xilinx"


# ---------------------------------------------------------------------------
# Timing


@dataclasses.dataclass
class Measurement:
    """One command's repeated runs."""

    name: str
    argv: list
    cwd: Optional[str]
    env_overrides: dict
    wall_times_s: list
    peak_rss_kib: list
    returncode: int
    returncodes: list = dataclasses.field(default_factory=list)
    timed_out: bool = False
    skipped_reason: Optional[str] = None

    def median_s(self) -> Optional[float]:
        return (
            statistics.median(self.wall_times_s) if self.wall_times_s else None
        )

    def min_s(self) -> Optional[float]:
        return min(self.wall_times_s) if self.wall_times_s else None

    def median_rss_kib(self) -> Optional[float]:
        return (
            statistics.median(self.peak_rss_kib) if self.peak_rss_kib else None
        )

    def any_failed(self) -> bool:
        """True when any repeat exited non-zero (a timeout is tracked
        separately via `timed_out` and is not "failed" in this sense)."""
        return any(rc != 0 for rc in self.returncodes)

    def first_failure_rc(self) -> Optional[int]:
        for rc in self.returncodes:
            if rc != 0:
                return rc
        return None

    def to_json(self) -> dict:
        return {
            "name": self.name,
            "argv": self.argv,
            "cwd": self.cwd,
            "env_overrides": self.env_overrides,
            "wall_times_s": self.wall_times_s,
            "peak_rss_kib": self.peak_rss_kib,
            "median_s": self.median_s(),
            "min_s": self.min_s(),
            "median_rss_kib": self.median_rss_kib(),
            "returncode": self.returncode,
            "returncodes": self.returncodes,
            "any_failed": self.any_failed(),
            "timed_out": self.timed_out,
            "skipped_reason": self.skipped_reason,
        }


def run_timed(
    name: str,
    argv: list,
    *,
    cwd: Optional[Path] = None,
    env_overrides: Optional[dict] = None,
    repeats: int = 5,
    timeout_s: float = 1200.0,
    input_bytes: Optional[bytes] = None,
) -> Measurement:
    """Runs `argv` `repeats` times, discarding stdout/stderr, and returns
    wall clock time and peak RSS of each successful (exit code 0) run,
    plus every run's exit code.

    Peak RSS comes from `os.wait4`'s `resource.struct_rusage.ru_maxrss`
    (on Linux, per-child, not `resource.getrusage(RUSAGE_CHILDREN)`'s
    aggregate across every child the parent has ever reaped -- see
    `measure_rss_floor()` below for why it is still not a clean per-
    program number and how this script corrects for that. Falls back
    cleanly (0) on platforms without `os.wait4`, e.g. Windows; not
    expected here).

    A run whose exit code is non-zero still records that exit code (in
    `returncodes`) but its wall time/RSS are *not* added to
    `wall_times_s`/`peak_rss_kib`: a failed run's "timing" is meaningless
    (it may have exited before doing the work being measured at all), and
    mixing it into a median silently understates the real cost. Callers
    that need every run to succeed (e.g. a correctness-sensitive
    comparison) should check `Measurement.any_failed()`.
    """
    env = dict(os.environ)
    if env_overrides:
        env.update(env_overrides)
    wall_times = []
    peak_rss = []
    returncodes = []
    timed_out = False
    for _ in range(max(1, repeats)):
        start = time.monotonic()
        proc = subprocess.Popen(
            argv,
            cwd=str(cwd) if cwd else None,
            env=env,
            stdin=(
                subprocess.PIPE
                if input_bytes is not None
                else subprocess.DEVNULL
            ),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        run_timed_out = False
        status = None
        rusage = None
        try:
            if input_bytes is not None:
                proc.stdin.write(input_bytes)
                proc.stdin.close()
            deadline = start + timeout_s
            if hasattr(os, "wait4"):
                while True:
                    reaped_pid, status, rusage = os.wait4(proc.pid, os.WNOHANG)
                    if reaped_pid != 0:
                        break
                    if time.monotonic() > deadline:
                        proc.kill()
                        os.wait4(proc.pid, 0)
                        run_timed_out = True
                        status = -1
                        break
                    time.sleep(0.002)
            else:
                try:
                    proc.wait(timeout=timeout_s)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
                    run_timed_out = True
                status = proc.returncode if not run_timed_out else -1
        finally:
            elapsed = time.monotonic() - start
        if run_timed_out:
            timed_out = True
            returncodes.append(-1)
            break
        rc = os.waitstatus_to_exitcode(status)
        returncodes.append(rc)
        if rc == 0:
            wall_times.append(elapsed)
            if rusage is not None:
                peak_rss.append(float(rusage.ru_maxrss))
    last_rc = returncodes[-1] if returncodes else (-1 if timed_out else 0)
    return Measurement(
        name=name,
        argv=[str(a) for a in argv],
        cwd=str(cwd) if cwd else None,
        env_overrides=env_overrides or {},
        wall_times_s=wall_times,
        peak_rss_kib=peak_rss,
        returncode=last_rc,
        returncodes=returncodes,
        timed_out=timed_out,
    )


def skipped(name: str, argv: list, reason: str) -> Measurement:
    return Measurement(
        name=name,
        argv=[str(a) for a in argv],
        cwd=None,
        env_overrides={},
        wall_times_s=[],
        peak_rss_kib=[],
        returncode=0,
        skipped_reason=reason,
    )


def measure_rss_floor(repeats: int = 3) -> Measurement:
    """Measures this driver's current `ru_maxrss` floor: `/bin/true` (a
    minimal, near-instant program) run through the same `run_timed()`
    every real measurement uses.

    Why this is needed: `ru_maxrss` is not a clean measurement of the
    *benchmarked program's* memory. Linux's `fork()`/`posix_spawn()`
    briefly gives the child the parent's own pages via copy-on-write
    before `exec()` replaces the child's address space, and on this
    container that history leaks into the reported peak: `/bin/true`
    alone reports about 15 MiB, but immediately after this driver
    allocates and frees a 300 MB buffer, the same `/bin/true` call reports
    about 322 MiB -- a number entirely explained by this driver's own
    history, not by `/bin/true`. Shelling out to `xz` for decompression
    (see `materialize()`) removes one source of that growth but not the
    general problem: many small, fast measurements (`fasm` on
    `counter_test`, an ANTLR parse) come out indistinguishable from this
    floor and must not be read as real per-program RSS.

    Call this immediately before and after a suite (or a group of
    measurements) whose driver-side memory footprint might have grown
    (e.g. after `materialize()` has decompressed something, after a
    subprocess's captured stdout/stderr was read into a Python `str`), and
    report both: `annotate_rss_floor()` uses the *later, larger* of the
    samples given to it as the threshold below which a measurement's RSS
    is marked `(floor)` rather than trusted as real.
    """
    true_bin = shutil.which("true") or "/bin/true"
    return run_timed("(baseline) /bin/true", [true_bin], repeats=repeats)


def rss_floor_kib(floor_samples: list) -> Optional[float]:
    """Given the `measure_rss_floor()` measurements taken during a run
    (callers collect these at the points noted in that function's
    docstring), returns the largest median RSS among them: the floor
    `write_markdown` compares every other measurement's RSS against,
    marking one at or below it as reflecting that floor rather than the
    benchmarked program's own memory. `None` if none were successful.
    """
    values = [
        m.median_rss_kib() for m in floor_samples if m.median_rss_kib()
    ]
    return max(values) if values else None


# ---------------------------------------------------------------------------
# Machine / reference info


def sh(argv, **kw) -> str:
    try:
        return subprocess.run(
            argv, capture_output=True, text=True, timeout=30, **kw
        ).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        return ""


def machine_info() -> dict:
    load1, load5, load15 = os.getloadavg()
    info = {
        "nproc": os.cpu_count(),
        "lscpu_model": "",
        "mem_total_kib": 0,
        "kernel": platform.release(),
        "load_avg": {"1m": load1, "5m": load5, "15m": load15},
        "rustc_version": sh(["rustc", "--version"]),
        "python_version": sys.version.split()[0],
        "python_version_full": sys.version,
    }
    for line in sh(["lscpu"]).splitlines():
        if line.startswith("Model name:"):
            info["lscpu_model"] = line.split(":", 1)[1].strip()
    try:
        with open("/proc/meminfo") as f:
            for line in f:
                if line.startswith("MemTotal:"):
                    info["mem_total_kib"] = int(line.split()[1])
                    break
    except OSError:
        pass
    return info


def reference_commits() -> dict:
    out = {}
    for label, path in (
        ("fasm_oracle", ORACLE_DIR / "build" / "status.json"),
        ("xilinx_oracle", ORACLE_DIR / "build" / "xilinx" / "status.json"),
    ):
        if path.exists():
            try:
                out[label] = json.loads(path.read_text())
            except (OSError, json.JSONDecodeError):
                out[label] = {"error": f"could not read/parse {path}"}
        else:
            out[label] = {"error": f"{path} not found"}
    return out


# ---------------------------------------------------------------------------
# Corpus helpers


def materialize(path: Path, scratch: Path) -> Optional[Path]:
    """Returns a plain, uncompressed file for `path` (which may be
    `path.fasm` or `path.fasm.xz`), decompressing into `scratch` if
    needed. `None` if neither exists.

    Decompresses with the `xz` binary rather than `lzma.decompress`
    in-process, so this driver's own heap does not grow just to hold a
    (possibly tens of MB) decompressed file it otherwise has no further
    use for. This helps keep the driver small, which is worth doing on
    general principle -- but on its own it does **not** fix the `ru_maxrss`
    measurement floor described in `measure_rss_floor()`'s docstring: that
    floor tracks the driver's peak *lifetime* memory (any large allocation
    the driver ever made, even one already freed, via the fork/vfork
    copy-on-write sharing window before `exec()` replaces the child's
    address space), and other things this driver does (reading a
    `gen-xilinx-corpus.py` subprocess's captured stdout, building the
    JSON report) can raise that floor too. See `measure_rss_floor()`.
    """
    if path.exists():
        return path
    xz = path.with_suffix(path.suffix + ".xz")
    if xz.exists():
        # Corpus files share a basename across designs/boards (every
        # design's FASM is named e.g. "vpr.fasm"), so the output name
        # must include enough of the source path to stay unique in a
        # shared scratch directory -- otherwise a second design's
        # materialize() call would see the first design's same-named
        # output already there and silently reuse its (wrong) content.
        unique = "_".join(xz.parent.parts[-2:]) + "_" + xz.stem
        out = scratch / unique
        if not out.exists():
            with open(out, "wb") as f:
                subprocess.run(["xz", "-dc", str(xz)], stdout=f, check=True)
        return out
    return None


def find_design(
    family_dir: str, design: str, board: str, filename: str
) -> Optional[Path]:
    base = (
        CORPUS_XILINX
        / family_dir
        / "designs"
        / "f4pga-examples"
        / design
        / board
    )
    for candidate in (base / filename, base / (filename + ".xz")):
        if candidate.exists():
            return (
                candidate.with_suffix("")
                if candidate.suffix == ".xz"
                else candidate
            )
    return None


def generate_synthetic_fasm(path: Path, lines: int) -> None:
    """Writes a deterministic synthetic FASM file with `lines` feature
    lines (mixed plain features, 64 bit binary values, 256 bit hex
    values, annotations and comments), matching the shape described in
    `rust/fasm/benches/parser.rs`'s `generate`, so the 1M line file used
    here is reproducible without committing it.
    """
    lut64 = "1111000011110000111100001111000011110000111100001111000011110000"[
        :64
    ]
    bram256 = "0123456789ABCDEF" * 4
    with open(path, "w") as f:
        i = 0
        x = 0
        while i < lines:
            for y in range(200):
                f.write(f"INT_L_X{x}Y{y}.WW2BEG0.LOGIC_OUTS_L4\n")
                i += 1
                if i >= lines:
                    break
                f.write(f"INT_L_X{x}Y{y}.IMUX_L{y % 48}.GFAN0\n")
                i += 1
                if i >= lines:
                    break
                tile = f"CLBLM_R_X{x}Y{y}.SLICEM_X0"
                f.write(f"{tile}.ALUT.INIT[63:0] = 64'b{lut64}\n")
                i += 1
                if i >= lines:
                    break
                bram = f"BRAM_L_X{x}Y{y}.RAMB18_Y0"
                f.write(f"{bram}.INIT_00[255:0] = 256'h{bram256}\n")
                i += 1
                if i >= lines:
                    break
                f.write(f"# comment for tile X{x}Y{y}\n")
                i += 1
                if i >= lines:
                    break
                f.write(f'{tile}.CARRY4.ACY0 {{ .id = "{i}" }}\n')
                i += 1
            x += 1


# ---------------------------------------------------------------------------
# Suites


def rust_bin(cli_dir: Path, name: str) -> Optional[Path]:
    path = cli_dir / name
    return path if path.exists() else None


def prime_xdb_cache(
    cli_dir: Path, xdb_cache_dir: Path, db_root: Path, part: str
) -> None:
    """Runs `fasm-db-cache build` once, untimed, so a part's binary cache
    is already built (in `xdb_cache_dir`, explicitly -- never the
    environment/XDG default, so this run's cache state is reproducible
    and does not depend on, or pollute, anything left over on the machine
    from a previous run) before any *timed* "default-cache" measurement
    reads it. Without this, whichever measurement happens to run first
    against a fresh cache directory pays the one-time build cost itself
    and is not comparable to the steady-state cache-hit numbers every
    other run of the same part gets (T5.3's own numbers, and
    `docs/rewrite/DESIGN-xilinx-db.md` §8.8, both quote fresh-cache-file
    write time as noticeably slower than a hit). Never raises: a failed
    priming run just means the first timed run pays the build cost
    instead, which is still correct, only less clean; callers should
    still expect the "default-cache" numbers of a part whose priming
    failed to plausibly include a one-off build cost.
    """
    fasm_db_cache = rust_bin(cli_dir, "fasm-db-cache")
    if fasm_db_cache is None:
        return
    subprocess.run(
        [
            str(fasm_db_cache),
            "--cache-dir",
            str(xdb_cache_dir),
            "build",
            str(db_root),
            part,
        ],
        capture_output=True,
        timeout=120,
        check=False,
    )


def suite_parser(results: list, args, scratch: Path, cli_dir: Path):
    rust_fasm = rust_bin(cli_dir, "fasm")
    oracle = ORACLE_DIR / "fasm-oracle"
    inputs = []

    small = find_design("artix7", "counter_test", "arty_35", "top.fasm")
    if small:
        inputs.append(
            ("small (counter_test, 781 lines)", materialize(small, scratch))
        )
    medium = find_design("artix7", "picosoc_demo", "arty_35", "vpr.fasm")
    if medium:
        inputs.append(
            ("medium (picosoc_demo, ~97k lines)", materialize(medium, scratch))
        )
    large = find_design("artix7", "linux_litex_demo", "arty_35", "vpr.fasm")
    if large:
        inputs.append(
            (
                "large (linux_litex_demo, ~344k lines)",
                materialize(large, scratch),
            )
        )
    if not args.quick:
        synthetic = scratch / "synthetic_1m.fasm"
        if not synthetic.exists():
            generate_synthetic_fasm(synthetic, 1_000_000)
        inputs.append(("synthetic (1M lines)", synthetic))

    repeats = 3 if args.quick else args.repeats
    for label, path in inputs:
        if path is None or not path.exists():
            results.append(
                skipped(
                    f"parser/{label}/rust", [], f"input not found for {label}"
                )
            )
            continue
        if rust_fasm:
            results.append(
                run_timed(
                    f"parser/{label}/rust-fasm",
                    [str(rust_fasm), str(path)],
                    repeats=repeats,
                    timeout_s=args.reference_timeout,
                )
            )
            results.append(
                run_timed(
                    f"parser/{label}/rust-fasm-canonical",
                    [str(rust_fasm), "--canonical", str(path)],
                    repeats=repeats,
                    timeout_s=args.reference_timeout,
                )
            )
        else:
            results.append(
                skipped(
                    f"parser/{label}/rust-fasm",
                    [],
                    "target/release/fasm not built",
                )
            )
        if oracle.exists():
            for parser_name in ("antlr", "textx"):
                reps = (
                    1
                    if (args.quick or "large" in label or "synthetic" in label)
                    else min(repeats, 3)
                )
                # textX on the 1M-line synthetic file grows super-linearly
                # (its RSS was still climbing well past 2 GiB after
                # several minutes when this was first tried -- see
                # docs/rewrite/BENCHMARKS.md); bound it to a few minutes
                # instead of the full --reference-timeout budget so a run
                # reliably finishes with a clearly labeled "timed out" row
                # (write_markdown) rather than tying up the whole suite.
                timeout_s = args.reference_timeout
                if parser_name == "textx" and "synthetic" in label:
                    timeout_s = min(args.reference_timeout, 300.0)
                results.append(
                    run_timed(
                        f"parser/{label}/oracle-{parser_name}",
                        [str(oracle), "--parser", parser_name, str(path)],
                        repeats=reps,
                        timeout_s=timeout_s,
                    )
                )
        else:
            results.append(
                skipped(f"parser/{label}/oracle", [], f"{oracle} not found")
            )


def sum_measurement(name: str, parts: list) -> Measurement:
    """A synthetic "sum" row over several successful `run_timed()`
    results (e.g. one every-feature pass file each): its one "run" is the
    sum of each part's median wall time and median peak RSS. Only
    meaningful when every part in `parts` succeeded (callers check that
    before calling this); returns a `skipped` row with a clear reason if
    `parts` is empty (nothing to sum, e.g. the corpus generator produced
    no pass files).
    """
    if not parts:
        return skipped(name, [], "nothing to sum (no pass files)")
    total_s = sum(m.median_s() or 0.0 for m in parts)
    total_rss = [m.median_rss_kib() for m in parts if m.median_rss_kib()]
    names = ", ".join(m.name for m in parts)
    return Measurement(
        name=name,
        argv=[f"sum of {len(parts)} runs: {names}"],
        cwd=None,
        env_overrides={},
        wall_times_s=[total_s],
        peak_rss_kib=[max(total_rss)] if total_rss else [],
        returncode=0,
        returncodes=[0],
    )


def run_every_feature_corpus(
    results: list,
    args,
    scratch: Path,
    label_prefix: str,
    family_root: Path,
    part: str,
    rust_bin_path: Optional[Path],
    rust_tool_name: str,
    oracle_bin_path: Path,
    oracle_tool_name: str,
    xdb_cache_dir: Optional[Path] = None,
    oracle_env: Optional[dict] = None,
):
    """Generates the "every feature" corpus for `part` with
    `tools/gen-xilinx-corpus.py` and times each of its pass files
    (`manifest.json`'s `"files"`, e.g. `features.fasm`,
    `features-2.fasm`, ...) as a **separate** `fasm2frames`/
    `uray-fasm2frames` run, never concatenated.

    Concatenating the pass files (an earlier version of this script did)
    is invalid: they exist as *separate* files precisely because their
    features conflict with each other on the same tile when combined (the
    generator's own multi-pass design, see `tools/gen-xilinx-corpus.py`'s
    module doc comment and `manifest.json`'s `notes`/`uncovered` fields);
    concatenating them produces a FASM both tools correctly reject before
    doing most of the real assembly work, which is not a meaningful
    "every feature" throughput measurement.

    Every file is required to succeed (`rc == 0`) for both tools -- a
    pass file the generator emits is a specific claim that it is
    conflict-free, so a non-zero exit on it is a real problem worth
    seeing in the report as `failed (rc=N)`, not silently averaged away.
    Reports a row per file plus one `.../SUM(N files)` row per tool (the
    sum of the per-file medians), for both the total-corpus comparison
    and the per-file detail.
    """
    gen = REPO_ROOT / "tools" / "gen-xilinx-corpus.py"
    part_dir = family_root / part
    if not gen.exists() or not part_dir.exists():
        results.append(
            skipped(
                f"{label_prefix}/every-feature/{part}",
                [],
                "generator or db missing",
            )
        )
        return
    out_dir = scratch / f"every-feature-{label_prefix}-{part}"
    tiles_mode = ["all"] if args.full_corpus else ["sample", "20"]
    gen_argv = [
        sys.executable,
        str(gen),
        "--db-root",
        str(family_root),
        "--part",
        part,
        "--out-dir",
        str(out_dir),
        "--tiles",
        *tiles_mode,
        "--no-errors",
    ]
    proc = subprocess.run(
        gen_argv,
        capture_output=True,
        text=True,
        timeout=args.reference_timeout,
    )
    if proc.returncode != 0:
        results.append(
            skipped(
                f"{label_prefix}/every-feature/{part}",
                gen_argv,
                f"generator failed: {proc.stderr[-500:]}",
            )
        )
        return
    manifest_path = out_dir / "manifest.json"
    try:
        manifest = json.loads(manifest_path.read_text())
        pass_files = manifest["files"]
    except (OSError, json.JSONDecodeError, KeyError) as e:
        results.append(
            skipped(
                f"{label_prefix}/every-feature/{part}",
                gen_argv,
                f"could not read {manifest_path}: {e}",
            )
        )
        return
    if not pass_files:
        results.append(
            skipped(
                f"{label_prefix}/every-feature/{part}",
                gen_argv,
                "manifest.json lists no pass files",
            )
        )
        return

    rust_results = []
    oracle_results = []
    for pass_file in pass_files:
        src = out_dir / pass_file
        stem = Path(pass_file).stem
        if rust_bin_path:
            out_frm = out_dir / f"{stem}.rust.frm"
            m = run_timed(
                f"{label_prefix}/every-feature/{part}/{rust_tool_name}/"
                f"{pass_file}",
                [
                    str(rust_bin_path),
                    "--db-root",
                    str(family_root),
                    "--part",
                    part,
                    str(src),
                    str(out_frm),
                ],
                env_overrides=(
                    {"FASM_XDB_CACHE": str(xdb_cache_dir)}
                    if xdb_cache_dir
                    else None
                ),
                repeats=args.repeats,
                timeout_s=args.reference_timeout,
            )
            results.append(m)
            if not m.any_failed() and not m.timed_out:
                rust_results.append(m)
        if oracle_bin_path.exists():
            out_frm = out_dir / f"{stem}.oracle.frm"
            m = run_timed(
                f"{label_prefix}/every-feature/{part}/{oracle_tool_name}/"
                f"{pass_file}",
                [
                    str(oracle_bin_path),
                    "--db-root",
                    str(family_root),
                    "--part",
                    part,
                    str(src),
                    str(out_frm),
                ],
                env_overrides=oracle_env,
                repeats=min(args.repeats, 3),
                timeout_s=args.reference_timeout,
            )
            results.append(m)
            if not m.any_failed() and not m.timed_out:
                oracle_results.append(m)
    if rust_bin_path:
        results.append(
            sum_measurement(
                f"{label_prefix}/every-feature/{part}/{rust_tool_name}/"
                f"SUM({len(pass_files)} files)",
                rust_results,
            )
        )
    if oracle_bin_path.exists():
        results.append(
            sum_measurement(
                f"{label_prefix}/every-feature/{part}/{oracle_tool_name}/"
                f"SUM({len(pass_files)} files)",
                oracle_results,
            )
        )
    else:
        results.append(
            skipped(
                f"{label_prefix}/every-feature/{part}/{oracle_tool_name}",
                [],
                "oracle wrapper missing",
            )
        )


def suite_xilinx7(
    results: list,
    args,
    scratch: Path,
    cli_dir: Path,
    db_root: Path,
    xdb_cache_dir: Path,
):
    fasm2frames = rust_bin(cli_dir, "fasm2frames")
    xcfasm = rust_bin(cli_dir, "xcfasm")
    fasm2frames_oracle = ORACLE_DIR / "fasm2frames-oracle"
    xcfasm_oracle = ORACLE_DIR / "xcfasm-oracle"
    artix7_root = db_root / "prjxray-db" / "artix7"

    designs = [
        ("counter_test", "arty_35", "top.fasm", "xc7a35tcsg324-1"),
    ]
    if not args.quick:
        designs.append(
            ("picosoc_demo", "arty_35", "vpr.fasm", "xc7a35tcsg324-1")
        )
        designs.append(
            ("linux_litex_demo", "arty_35", "vpr.fasm", "xc7a35tcsg324-1")
        )

    for design, board, filename, part in designs:
        src = find_design("artix7", design, board, filename)
        fasm_path = materialize(src, scratch) if src else None
        label = f"{design}/{board}"
        if fasm_path is None:
            results.append(skipped(f"xilinx7/{label}", [], "input not found"))
            continue
        part_yaml = artix7_root / part / "part.yaml"
        if not part_yaml.exists():
            results.append(
                skipped(f"xilinx7/{label}", [], f"{part_yaml} not found")
            )
            continue
        prime_xdb_cache(cli_dir, xdb_cache_dir, artix7_root, part)
        for cache, cache_label in (
            ({"FASM_XDB_CACHE": str(xdb_cache_dir)}, "default-cache"),
            ({"FASM_XDB_CACHE": "0"}, "no-cache"),
        ):
            out_frm = scratch / f"{design}-{board}-{cache_label}.frm"
            if fasm2frames:
                results.append(
                    run_timed(
                        f"xilinx7/{label}/rust-fasm2frames/{cache_label}",
                        [
                            str(fasm2frames),
                            "--db-root",
                            str(artix7_root),
                            "--part",
                            part,
                            str(fasm_path),
                            str(out_frm),
                        ],
                        env_overrides=cache,
                        repeats=(
                            1 if cache_label == "no-cache" else args.repeats
                        ),
                        timeout_s=args.reference_timeout,
                    )
                )
            if xcfasm:
                out_bit = scratch / f"{design}-{board}-{cache_label}.bit"
                results.append(
                    run_timed(
                        f"xilinx7/{label}/rust-xcfasm/{cache_label}",
                        [
                            str(xcfasm),
                            "--db-root",
                            str(artix7_root),
                            "--part",
                            part,
                            "--part_file",
                            str(part_yaml),
                            "--fn_in",
                            str(fasm_path),
                            "--bit_out",
                            str(out_bit),
                        ],
                        env_overrides=cache,
                        repeats=(
                            1 if cache_label == "no-cache" else args.repeats
                        ),
                        timeout_s=args.reference_timeout,
                    )
                )
        if fasm2frames_oracle.exists():
            out_frm = scratch / f"{design}-{board}-oracle.frm"
            results.append(
                run_timed(
                    f"xilinx7/{label}/oracle-fasm2frames",
                    [
                        str(fasm2frames_oracle),
                        "--db-root",
                        str(artix7_root),
                        "--part",
                        part,
                        str(fasm_path),
                        str(out_frm),
                    ],
                    repeats=1 if "linux" in design else min(args.repeats, 3),
                    timeout_s=args.reference_timeout,
                )
            )
        else:
            results.append(
                skipped(
                    f"xilinx7/{label}/oracle-fasm2frames",
                    [],
                    "oracle wrapper missing",
                )
            )
        if xcfasm_oracle.exists():
            env = dict(os.environ)
            xilinx_bin = ORACLE_DIR / "build" / "xilinx" / "bin"
            env_override = {"PATH": f"{xilinx_bin}:{env.get('PATH', '')}"}
            out_bit = scratch / f"{design}-{board}-oracle.bit"
            results.append(
                run_timed(
                    f"xilinx7/{label}/oracle-xcfasm",
                    [
                        str(xcfasm_oracle),
                        "--db-root",
                        str(artix7_root),
                        "--part",
                        part,
                        "--part_file",
                        str(part_yaml),
                        "--fn_in",
                        str(fasm_path),
                        "--bit_out",
                        str(out_bit),
                    ],
                    env_overrides=env_override,
                    repeats=1 if "linux" in design else min(args.repeats, 3),
                    timeout_s=args.reference_timeout,
                )
            )
        else:
            results.append(
                skipped(
                    f"xilinx7/{label}/oracle-xcfasm",
                    [],
                    "oracle wrapper missing",
                )
            )

    # "Every feature" synthetic corpus, sampled (a full --tiles all run of
    # xc7a200t is multiple GB / tens of minutes -- see
    # docs/rewrite/DESIGN-xilinx-db.md 8.9 -- so this defaults to a sample
    # and only widens to --tiles all with --full-corpus). Each pass file
    # (manifest.json's "files") is timed on its own -- see
    # run_every_feature_corpus()'s docstring for why they must not be
    # concatenated.
    parts = [("xc7a35tcsg324-1", "artix7")]
    if args.full_corpus:
        parts.append(("xc7a200tffg1156-1", "artix7"))
    for part, family in parts:
        family_root = db_root / "prjxray-db" / family
        prime_xdb_cache(cli_dir, xdb_cache_dir, family_root, part)
        run_every_feature_corpus(
            results,
            args,
            scratch,
            "xilinx7",
            family_root,
            part,
            fasm2frames,
            "rust-fasm2frames",
            fasm2frames_oracle,
            "oracle-fasm2frames",
            xdb_cache_dir=xdb_cache_dir,
        )


def suite_ultrascale(
    results: list,
    args,
    scratch: Path,
    cli_dir: Path,
    db_root: Path,
    xdb_cache_dir: Path,
):
    uray_fasm2frames = rust_bin(cli_dir, "uray-fasm2frames")
    uray_fasm2frames_oracle = ORACLE_DIR / "uray-fasm2frames-oracle"
    family_root = db_root / "prjuray-db" / "zynqusp"
    part = "xczu3eg-sfvc784-1-e"
    part_dir = family_root / part
    if not part_dir.exists():
        results.append(
            skipped("ultrascale/xczu3eg", [], f"{part_dir} not found")
        )
        return
    prime_xdb_cache(cli_dir, xdb_cache_dir, family_root, part)
    run_every_feature_corpus(
        results,
        args,
        scratch,
        "ultrascale",
        family_root,
        part,
        uray_fasm2frames,
        "rust-uray-fasm2frames",
        uray_fasm2frames_oracle,
        "oracle-uray-fasm2frames",
        xdb_cache_dir=xdb_cache_dir,
    )


def build_python_bindings(scratch: Path) -> Optional[Path]:
    """Builds the `fasm-python` extension with maturin into a fresh venv
    under `scratch`, returns the venv's python or `None` on failure (with
    a message printed to stderr; this is never a hard failure of the
    whole run, only of the `python` suite).
    """
    venv_dir = scratch / "bindings-venv"
    if not venv_dir.exists():
        subprocess.run(
            [sys.executable, "-m", "venv", str(venv_dir)], check=True
        )
    venv_py = venv_dir / "bin" / "python"
    pip_install = subprocess.run(
        [
            str(venv_py),
            "-m",
            "pip",
            "install",
            "-q",
            "maturin>=1.9.4",
            "textx",
        ],
        capture_output=True,
        text=True,
        timeout=600,
    )
    if pip_install.returncode != 0:
        print(
            "python suite: pip install maturin failed:\n"
            f"{pip_install.stderr[-2000:]}",
            file=sys.stderr,
        )
        return None
    env = dict(os.environ)
    env["VIRTUAL_ENV"] = str(venv_dir)
    env["PATH"] = f"{venv_dir / 'bin'}:{env.get('PATH', '')}"
    build = subprocess.run(
        [str(venv_py), "-m", "maturin", "develop", "--release"],
        cwd=str(REPO_ROOT),
        env=env,
        capture_output=True,
        text=True,
        timeout=900,
    )
    if build.returncode != 0:
        print(
            f"python suite: maturin develop failed:\n{build.stderr[-2000:]}",
            file=sys.stderr,
        )
        return None
    return venv_py


def suite_python(
    results: list,
    args,
    scratch: Path,
    db_root: Path,
    xdb_cache_dir: Path,
    cli_dir: Path,
):
    venv_py = build_python_bindings(scratch)
    if venv_py is None:
        results.append(
            skipped(
                "python/build",
                [],
                "maturin develop failed or unavailable, see stderr",
            )
        )
        return

    small = find_design("artix7", "counter_test", "arty_35", "top.fasm")
    small_path = materialize(small, scratch) if small else None
    if small_path:
        script = scratch / "bench_parse_fasm_string.py"
        script.write_text(
            "import sys, time\n"
            "import fasm\n"
            "text = open(sys.argv[1]).read()\n"
            "list(fasm.parse_fasm_string(text))\n"
        )
        results.append(
            run_timed(
                "python/parse_fasm_string/rust-bindings",
                [str(venv_py), str(script), str(small_path)],
                repeats=args.repeats,
                timeout_s=args.reference_timeout,
            )
        )
        oracle_py = ORACLE_DIR / "venv" / "bin" / "python"
        if oracle_py.exists():
            results.append(
                run_timed(
                    "python/parse_fasm_string/oracle-textx",
                    [str(oracle_py), "-P", str(script), str(small_path)],
                    repeats=min(args.repeats, 3),
                    timeout_s=args.reference_timeout,
                )
            )

    artix7_root = db_root / "prjxray-db" / "artix7"
    part = "xc7a35tcsg324-1"
    if small_path and (artix7_root / part).exists():
        script = scratch / "bench_fasm2frames_py.py"
        script.write_text(
            "import sys\n"
            "import fasm.xilinx as fx\n"
            "db = fx.Database.open(sys.argv[1], sys.argv[2])\n"
            "asm = fx.FasmAssembler(db)\n"
            "asm.parse_fasm_filename(sys.argv[3])\n"
            "asm.get_frames(sparse=False)\n"
        )
        prime_xdb_cache(cli_dir, xdb_cache_dir, artix7_root, part)
        for cache_label, env in (
            ("default-cache", {"FASM_XDB_CACHE": str(xdb_cache_dir)}),
            ("no-cache", {"FASM_XDB_CACHE": "0"}),
        ):
            results.append(
                run_timed(
                    f"python/fasm2frames/rust-bindings/{cache_label}",
                    [
                        str(venv_py),
                        str(script),
                        str(artix7_root),
                        part,
                        str(small_path),
                    ],
                    env_overrides=env,
                    repeats=1 if cache_label == "no-cache" else args.repeats,
                    timeout_s=args.reference_timeout,
                )
            )
        venv_xilinx_py = ORACLE_DIR / "venv-xilinx" / "bin" / "python"
        if venv_xilinx_py.exists():
            script_oracle = scratch / "bench_fasm2frames_oracle.py"
            script_oracle.write_text(
                "import sys\n"
                "from xc_fasm.fasm2frames import fasm2frames\n"
                "fasm2frames(sys.argv[1], sys.argv[2], sys.argv[3], "
                "sys.argv[4], sparse=False)\n"
            )
            out_frm = scratch / "oracle-bindings.frm"
            results.append(
                run_timed(
                    "python/fasm2frames/oracle-xc_fasm",
                    [
                        str(venv_xilinx_py),
                        "-P",
                        str(script_oracle),
                        str(artix7_root),
                        part,
                        str(small_path),
                        str(out_frm),
                    ],
                    repeats=min(args.repeats, 3),
                    timeout_s=args.reference_timeout,
                )
            )


# ---------------------------------------------------------------------------
# Reporting


def write_json(
    results: list,
    machine: dict,
    refs: dict,
    out_path: Path,
    xdb_cache_dir: Optional[Path] = None,
    rss_floor_kib_value: Optional[float] = None,
):
    out_path.write_text(
        json.dumps(
            {
                "machine": machine,
                "reference_commits": refs,
                "xdb_cache_dir": (
                    str(xdb_cache_dir) if xdb_cache_dir else None
                ),
                "rss_floor_kib": rss_floor_kib_value,
                "measurements": [m.to_json() for m in results],
            },
            indent=2,
        )
    )


def fmt_s(v: Optional[float]) -> str:
    if v is None:
        return "-"
    if v < 1:
        return f"{v * 1000:.1f} ms"
    return f"{v:.2f} s"


def fmt_rss(v: Optional[float], floor: Optional[float] = None) -> str:
    if not v:
        return "-"
    text = f"{v / 1024:.1f} MiB"
    if floor is not None and v <= floor:
        text += " (floor)"
    return text


def write_markdown(
    results: list,
    machine: dict,
    refs: dict,
    out_path: Path,
    xdb_cache_dir: Optional[Path] = None,
    rss_floor_kib_value: Optional[float] = None,
):
    lines = []
    lines.append("# Benchmark report\n")
    cpu = machine["lscpu_model"] or "(unknown)"
    lines.append(f"* nproc: {machine['nproc']}, CPU: {cpu}")
    ram_gib = machine["mem_total_kib"] / 1048576.0
    lines.append(f"* RAM: {ram_gib:.1f} GiB, kernel: {machine['kernel']}")
    load = machine["load_avg"]
    lines.append(
        f"* load average at run start: {load['1m']:.2f} "
        f"{load['5m']:.2f} {load['15m']:.2f}"
    )
    lines.append(
        f"* {machine['rustc_version']}, Python {machine['python_version']}"
    )
    if xdb_cache_dir is not None:
        lines.append(f"* `FASM_XDB_CACHE` for this run: `{xdb_cache_dir}`")
    if rss_floor_kib_value is not None:
        lines.append(
            f"* peak-RSS measurement floor this run: "
            f"{rss_floor_kib_value / 1024:.1f} MiB (see "
            "`measure_rss_floor()`'s docstring in this script -- a "
            "measurement at or below this reflects the floor, not the "
            "benchmarked program's own memory, and is marked `(floor)`)"
        )
    lines.append("")
    lines.append("| Measurement | Median | Min | Peak RSS | Runs |")
    lines.append("|---|---:|---:|---:|---:|")
    for m in results:
        if m.skipped_reason:
            lines.append(f"| {m.name} | skipped | | | {m.skipped_reason} |")
            continue
        n_runs = len(m.returncodes) or len(m.wall_times_s)
        if m.timed_out:
            lines.append(
                f"| {m.name} | timed out | | | {n_runs} (1 timed out) |"
            )
            continue
        if m.any_failed():
            rc = m.first_failure_rc()
            lines.append(
                f"| {m.name} | failed (rc={rc}) | | | {n_runs} "
                f"({sum(1 for r in m.returncodes if r != 0)} failed) |"
            )
            continue
        lines.append(
            f"| {m.name} | {fmt_s(m.median_s())} | {fmt_s(m.min_s())} | "
            f"{fmt_rss(m.median_rss_kib(), rss_floor_kib_value)} | "
            f"{n_runs} |"
        )
    out_path.write_text("\n".join(lines) + "\n")


# ---------------------------------------------------------------------------


def main() -> int:
    global ORACLE_DIR
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--repeats",
        type=int,
        default=5,
        help="repetitions per measurement (default 5)",
    )
    parser.add_argument(
        "--reference-timeout",
        type=float,
        default=1200.0,
        help=(
            "per-run timeout in seconds for any single command "
            "(default 1200 = 20 min, per WORKFLOW)"
        ),
    )
    parser.add_argument(
        "--quick", action="store_true", help="fewer repeats, small inputs only"
    )
    parser.add_argument(
        "--full-corpus",
        action="store_true",
        help=(
            "use --tiles all for the every-feature corpus instead of a "
            "sample (slow, GBs of RAM)"
        ),
    )
    parser.add_argument(
        "--skip",
        action="append",
        default=[],
        choices=["parser", "xilinx7", "ultrascale", "python"],
        help="suite(s) to skip",
    )
    parser.add_argument(
        "--cli-dir",
        type=Path,
        default=REPO_ROOT / "target" / "release",
        help="Rust CLI binaries",
    )
    parser.add_argument(
        "--oracle-dir",
        type=Path,
        default=ORACLE_DIR,
        help=(
            "tests/oracle directory with the built venv/venv-xilinx/build "
            "(default: $FASM_ORACLE_DIR or this checkout's tests/oracle; "
            "from a git worktree, pass the main checkout's tests/oracle -- "
            "the built venvs are gitignored and not shared between "
            "worktrees)"
        ),
    )
    parser.add_argument(
        "--db-root",
        type=Path,
        default=None,
        help=(
            "prjxray-db/prjuray-db cache root (default: $FASM_DB_CACHE, "
            "else <oracle-dir>/build/db)"
        ),
    )
    parser.add_argument(
        "--scratch",
        type=Path,
        default=None,
        help="scratch directory (default: a temp dir)",
    )
    parser.add_argument(
        "--xdb-cache-dir",
        type=Path,
        default=None,
        help=(
            "binary database cache directory for every \"default-cache\" "
            "measurement (default: <scratch>/xdb-cache -- never the "
            "environment/XDG default, so a run's cache state does not "
            "depend on, or pollute, anything left on the machine by a "
            "previous run); primed with one untimed build per part before "
            "any timed measurement reads it"
        ),
    )
    parser.add_argument(
        "--out-json", type=Path, default=Path("bench-report.json")
    )
    parser.add_argument("--out-md", type=Path, default=Path("bench-report.md"))
    parser.add_argument("--keep-scratch", action="store_true")
    args = parser.parse_args()

    ORACLE_DIR = args.oracle_dir
    if args.db_root is None:
        args.db_root = Path(
            os.environ.get("FASM_DB_CACHE", str(ORACLE_DIR / "build" / "db"))
        )

    own_scratch = args.scratch is None
    scratch = args.scratch or Path(tempfile.mkdtemp(prefix="fasm-bench-"))
    scratch.mkdir(parents=True, exist_ok=True)
    xdb_cache_dir = args.xdb_cache_dir or (scratch / "xdb-cache")
    xdb_cache_dir.mkdir(parents=True, exist_ok=True)

    machine = machine_info()
    refs = reference_commits()
    print(
        f"machine: nproc={machine['nproc']} load={machine['load_avg']} "
        f"cpu={machine['lscpu_model']!r}",
        file=sys.stderr,
    )

    results: list = []
    # See measure_rss_floor()'s docstring: sampled before and after the
    # suites run, since this driver's own peak-memory history (which
    # leaks into every child's measured ru_maxrss) can only grow as it
    # goes -- the later, larger sample is the floor reported and used to
    # annotate every other measurement.
    floor_samples = [measure_rss_floor()]
    try:
        if "parser" not in args.skip:
            suite_parser(results, args, scratch, args.cli_dir)
        if "xilinx7" not in args.skip:
            suite_xilinx7(
                results,
                args,
                scratch,
                args.cli_dir,
                args.db_root,
                xdb_cache_dir,
            )
        if "ultrascale" not in args.skip:
            suite_ultrascale(
                results,
                args,
                scratch,
                args.cli_dir,
                args.db_root,
                xdb_cache_dir,
            )
        if "python" not in args.skip:
            suite_python(
                results,
                args,
                scratch,
                args.db_root,
                xdb_cache_dir,
                args.cli_dir,
            )
    finally:
        floor_samples.append(measure_rss_floor())
        if own_scratch and not args.keep_scratch:
            shutil.rmtree(scratch, ignore_errors=True)

    floor = rss_floor_kib(floor_samples)
    results = floor_samples + results
    write_json(
        results, machine, refs, args.out_json, xdb_cache_dir, floor
    )
    write_markdown(
        results, machine, refs, args.out_md, xdb_cache_dir, floor
    )
    print(f"wrote {args.out_json} and {args.out_md}", file=sys.stderr)

    failures = [
        m for m in results if not m.skipped_reason and m.returncode != 0
    ]
    if failures:
        print(
            f"{len(failures)} measurement(s) had a non-zero exit code "
            "(see JSON report)",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())

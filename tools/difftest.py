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
"""Differential test driver: Rust `fasm` crate vs. the original Python
`fasm` package (the oracle), over the FASM corpus (T1.5).

For every corpus file (`tests/corpus/**/*.fasm`, `tests/corpus/**/*.fasm.xz`
and `examples/*.fasm`) this compares, depending on which corpus
subdirectory the file is in (see "Corpus categories" below):

(a) parse tree: `fasm-dump FILE` (Rust) vs.
    `dump.py [--parser antlr|textx] FILE` (oracle), byte for byte JSON;
(b) `fasm_tuple_to_string`, canonical and non-canonical:
    `fasm-dump --to-string [--canonical] FILE` (Rust) vs.
    `oracle_to_string.py [--canonical] FILE` (oracle), byte for byte;
(c) round trip: Rust parse -> to_string -> Rust parse gives the same tree,
    and the oracle's parse of that same Rust-printed text agrees with it
    too.

Differences are classified into the divergence classes documented in
`docs/rewrite/COMPAT.md` (see `CLASSES` below); the tool exits non-zero
only when a file shows an UNEXPLAINED difference (a difference that does
not match any documented/expected class for that file) -- i.e. a
candidate real bug. A summary table is printed; full detail (every
mismatch, every command run) is written to a report file (`--report`,
default a temp file, path always printed).

One further class applies outside the `CLASSES`/manifest mechanism, to a
single directory rather than a per-file manifest entry:
`xilinx_error_corpus` (see `_class_xilinx_error_corpus`/
`_in_xilinx_error_corpus` below). `tests/corpus/xilinx/**/synthetic/errors/`
is T5.4's fasm2frames error-path corpus: files there are deliberately
invalid FASM whose job is to exercise `fasm2frames`' *own* error
reporting, not parser parity, and the ANTLR oracle's error handling for
several of them is itself broken (it raises an internal Python exception,
e.g. `'NoneType' object is not iterable`, instead of a clean parse error).
For a file under that directory whose parse trees differ, this class
applies -- and the difference is `xilinx_error_corpus`, not
`unexplained` -- only when Rust reports a parse error *and* the ANTLR
oracle reports an error of some form (its message is not compared) *and*
textX either errors too or its result exactly matches Rust's. Anything
else in that directory (including a file all three parsers actually
agree on, e.g. a valid-FASM fixture that is only an error case for
`fasm2frames`' own lookup, not for parsing) is compared exactly as any
other "plain" corpus file -- this directory is never blanket-skipped.

A second such class, `all_three_reject`, applies only to the files a
corpus directory lists in its `expected-errors.json` (a map from the
file's name in that directory to `{"line": N, "source": "..."}`): FASM
that a real tool wrote but that is not valid FASM, e.g. VTR genfasm's
output for its own test architecture with the rr graph edge metadata of
`test_fasm.cpp` (T7.4: features such as `533_557_0` that start with a
digit). Such a file is `all_three_reject`, not `unexplained`, only when
Rust, the ANTLR oracle and the textX oracle all report a parse error on
exactly the listed line (the messages and columns are not compared); a
listed file that parses, or fails elsewhere, is `unexplained`.

Requires the oracle venv (`tests/oracle/setup.sh`, or point `--oracle-python`
at another one, e.g. the main checkout's) and the `fasm-dump` example binary
(`cargo build --release --example dump -p fasm`, or `--rust-dump`).
Exits with status 3 (not 1) and a message on stderr if either is missing,
so callers (the pytest wrapper) can tell "not set up" apart from "found
bugs".
"""
import argparse
import fnmatch
import glob
import json
import lzma
import multiprocessing
import os
import re
import subprocess
import sys
import tempfile
import time

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DUMP_PY = os.path.join(REPO_ROOT, "tests", "oracle", "dump.py")
TO_STRING_PY = os.path.join(REPO_ROOT, "tools", "oracle_to_string.py")
GEN_CORPUS_PY = os.path.join(REPO_ROOT, "tools", "gen-corpus.py")
MANIFEST_PATH = os.path.join(
    REPO_ROOT, "tests", "corpus", "synthetic", "edge-cases", "manifest.json")

DEFAULT_RUST_DUMP = os.path.join(
    REPO_ROOT, "target", "release", "examples", "dump")
DEFAULT_ORACLE_PYTHON_CANDIDATES = [
    os.path.join(REPO_ROOT, "tests", "oracle", "venv", "bin", "python"),
    "/home/user/fasm/tests/oracle/venv/bin/python",
]

EXIT_OK = 0
EXIT_UNEXPLAINED = 1
EXIT_NOT_SET_UP = 3


# ---------------------------------------------------------------------
# Divergence classes (docs/rewrite/COMPAT.md). Each predicate takes the
# parsed JSON docs (python dicts, as `json.load` gives them) for the Rust
# `fasm-dump`, oracle `--parser antlr` and oracle `--parser textx` runs of
# the *same* file and returns whether the observed outcome matches that
# class. Used for `tests/corpus/synthetic/edge-cases/*`, where
# `manifest.json` (written by `tools/gen-corpus.py`) records which class
# each file is expected to hit.
# ---------------------------------------------------------------------
def _err(doc):
    return isinstance(doc, dict) and "error" in doc


def _class_same_all_three(rust, antlr, textx):
    return not _err(rust) and not _err(antlr) and not _err(textx) \
        and rust == antlr == textx


def _class_rust_relaxes_antlr(rust, antlr, textx):
    return _err(antlr) and not _err(rust) and not _err(textx) \
        and rust == textx


def _class_rust_follows_antlr_over_textx(rust, antlr, textx):
    return _err(textx) and not _err(rust) and not _err(antlr) \
        and rust == antlr


def _class_antlr_bug_wrong_decode(rust, antlr, textx):
    return not _err(rust) and not _err(antlr) and not _err(textx) \
        and rust == textx and rust != antlr


def _class_rust_stricter(rust, antlr, textx):
    return _err(rust) and not _err(antlr) and not _err(textx)


def _class_non_ascii_antlr_exception(rust, antlr, textx):
    return _err(antlr) and not _err(rust) and not _err(textx) \
        and rust == textx


# `tests/corpus/xilinx/**/synthetic/errors/`: T5.4's fasm2frames error-path
# corpus (see the module docstring). Not manifest driven like `CLASSES`
# above -- there is no `manifest.json` entry per file here -- so these two
# helpers are applied directly by `process_plain`, gated on the directory,
# rather than looked up through `CLASSES`.
_XILINX_ERROR_CORPUS_PREFIX = "tests/corpus/xilinx/"
_XILINX_ERROR_CORPUS_INFIX = "/synthetic/errors/"


def _in_xilinx_error_corpus(rel):
    """True for `tests/corpus/xilinx/**/synthetic/errors/*.fasm` (`rel` is
    repo-root relative, `/` separated, as `discover_corpus` produces)."""
    return rel.startswith(_XILINX_ERROR_CORPUS_PREFIX) \
        and _XILINX_ERROR_CORPUS_INFIX in rel


def _class_xilinx_error_corpus(rust, antlr, textx):
    """See the module docstring's `xilinx_error_corpus` paragraph: Rust
    must actually report a parse error; the ANTLR oracle just needs to
    have failed too, in any form (message not compared -- dump.py's ANTLR
    parser wraps some errors in this corpus as unhelpful Python
    exceptions rather than a clean parse error); textX must likewise have
    errored, or (in principle; not observed in practice, since Rust
    erroring makes an exact match here vanishingly unlikely) produced
    exactly the same result as Rust."""
    return _err(rust) and _err(antlr) and (_err(textx) or textx == rust)


# `expected-errors.json` (see the module docstring's `all_three_reject`
# paragraph): per corpus directory, the files that every parser must
# reject, and the line of the error.
EXPECTED_ERRORS_NAME = "expected-errors.json"


def load_expected_errors():
    """{repo relative file path: {"line": N, ...}} from every
    `tests/corpus/**/expected-errors.json`."""
    result = {}
    pattern = os.path.join(REPO_ROOT, "tests", "corpus", "**",
                           EXPECTED_ERRORS_NAME)
    for manifest in glob.glob(pattern, recursive=True):
        base = os.path.relpath(os.path.dirname(manifest), REPO_ROOT)
        with open(manifest) as f:
            for name, entry in json.load(f).items():
                rel = os.path.join(base, name).replace(os.sep, "/")
                result[rel] = entry
    return result


_ERROR_LINE_RE = re.compile(r"(?:Parse error at |:)(\d+):\d+(?::| - )")


def _error_line(doc):
    """The line number of a dump's parse error (Rust and ANTLR:
    `Parse error at L:C - ...`; textX: `<path>:L:C: ...`), or None."""
    if not _err(doc):
        return None
    m = _ERROR_LINE_RE.search(doc["error"])
    return int(m.group(1)) if m else None


def _class_all_three_reject(rust, antlr, textx, line):
    return all(_error_line(d) == line for d in (rust, antlr, textx))


CLASSES = {
    "same_all_three": _class_same_all_three,
    "rust_relaxes_antlr": _class_rust_relaxes_antlr,
    "rust_follows_antlr_over_textx": _class_rust_follows_antlr_over_textx,
    "antlr_bug_wrong_decode": _class_antlr_bug_wrong_decode,
    "rust_stricter": _class_rust_stricter,
    "non_ascii_antlr_exception": _class_non_ascii_antlr_exception,
}


# ---------------------------------------------------------------------
# Corpus discovery
# ---------------------------------------------------------------------
def discover_corpus(filter_glob=None):
    """Returns a sorted list of (abs_path, rel_path, category) triples.

    `category` is one of `"invalid"` (`tests/corpus/synthetic/invalid/`),
    `"edge_case"` (`tests/corpus/synthetic/edge-cases/`, has a
    `manifest.json` entry) or `"plain"` (everything else: realistic FASM,
    full 3-way identity expected).
    """
    patterns = [
        os.path.join(REPO_ROOT, "tests", "corpus", "**", "*.fasm"),
        os.path.join(REPO_ROOT, "tests", "corpus", "**", "*.fasm.xz"),
        os.path.join(REPO_ROOT, "examples", "*.fasm"),
    ]
    paths = set()
    for pattern in patterns:
        paths.update(glob.glob(pattern, recursive=True))

    invalid_dir = os.path.join(
        REPO_ROOT, "tests", "corpus", "synthetic", "invalid") + os.sep
    edge_dir = os.path.join(
        REPO_ROOT, "tests", "corpus", "synthetic", "edge-cases") + os.sep

    result = []
    for path in sorted(paths):
        rel = os.path.relpath(path, REPO_ROOT).replace(os.sep, "/")
        if filter_glob and not fnmatch.fnmatch(rel, filter_glob):
            continue
        if path.startswith(invalid_dir):
            category = "invalid"
        elif path.startswith(edge_dir):
            category = "edge_case"
        else:
            category = "plain"
        result.append((path, rel, category))
    return result


def load_manifest():
    if not os.path.exists(MANIFEST_PATH):
        return {}
    with open(MANIFEST_PATH) as f:
        return json.load(f)


# ---------------------------------------------------------------------
# Subprocess helpers
# ---------------------------------------------------------------------
def _run(cmd):
    p = subprocess.run(cmd, capture_output=True, text=True)
    return p.returncode, p.stdout, p.stderr


def rust_dump_json(rust_dump, path):
    _, out, err = _run([rust_dump, path])
    try:
        return json.loads(out), None
    except ValueError:
        return None, "fasm-dump produced non-JSON output: {!r} (stderr: {!r})".format(
            out, err)


def oracle_dump_json(python, parser, path):
    cmd = [python, "-P", DUMP_PY, path]
    if parser is not None:
        cmd[3:3] = ["--parser", parser]
    _, out, err = _run(cmd)
    try:
        return json.loads(out), None
    except ValueError:
        return None, "dump.py produced non-JSON output: {!r} (stderr: {!r})".format(
            out, err)


def rust_to_string(rust_dump, path, canonical):
    cmd = [rust_dump, "--to-string"]
    if canonical:
        cmd.append("--canonical")
    cmd.append(path)
    rc, out, err = _run(cmd)
    if rc != 0:
        return None, err
    return out, None


def oracle_to_string(python, path, canonical):
    cmd = [python, "-P", TO_STRING_PY]
    if canonical:
        cmd.append("--canonical")
    cmd.append(path)
    rc, out, err = _run(cmd)
    if rc != 0:
        return None, err
    return out, None


def maybe_decompress(path):
    """If `path` ends in `.xz`, decompresses it to a temp `.fasm` file and
    returns that path (caller must remove it); otherwise returns `path`
    unchanged (and `None` as the temp path)."""
    if not path.endswith(".xz"):
        return path, None
    with lzma.open(path, "rb") as f:
        data = f.read()
    fd, tmp_path = tempfile.mkstemp(suffix=".fasm")
    with os.fdopen(fd, "wb") as f:
        f.write(data)
    return tmp_path, tmp_path


# ---------------------------------------------------------------------
# Per-file processing
# ---------------------------------------------------------------------
class FileResult:
    def __init__(self, rel, category):
        self.rel = rel
        self.category = category
        self.status = None  # "identical" | class name | "unexplained" | "error"
        self.details = []  # list of str, appended to the report

    def note(self, msg):
        self.details.append(msg)


def process_invalid(rel, path, rust_dump, result):
    expected_path = path + ".expected"
    if not os.path.exists(expected_path):
        result.status = "unexplained"
        result.note("no .expected sidecar for invalid case {}".format(rel))
        return result
    with open(expected_path) as f:
        expected_lines = f.read().splitlines()
    if not expected_lines:
        result.status = "unexplained"
        result.note("{}.expected is empty".format(rel))
        return result
    expected_pos = expected_lines[0].strip()

    doc, err = rust_dump_json(rust_dump, path)
    if doc is None:
        result.status = "unexplained"
        result.note("fasm-dump failed on {}: {}".format(rel, err))
        return result
    if not _err(doc):
        result.status = "unexplained"
        result.note(
            "{}: expected a Rust parse error at {}, but fasm-dump parsed "
            "it successfully: {}".format(rel, expected_pos, doc))
        return result

    message = doc["error"]
    # "Parse error at {line}:{column} - {message}"
    prefix = "Parse error at "
    if not message.startswith(prefix):
        result.status = "unexplained"
        result.note(
            "{}: error message does not start with {!r}: {!r}".format(
                rel, prefix, message))
        return result
    actual_pos = message[len(prefix):].split(" - ", 1)[0]
    if actual_pos != expected_pos:
        result.status = "unexplained"
        result.note(
            "{}: expected Rust error at {}, got {} ({!r})".format(
                rel, expected_pos, actual_pos, message))
        return result

    result.status = "invalid_case_ok"
    result.note("{}: Rust rejected at {} as expected ({!r})".format(
        rel, expected_pos, message))
    return result


EDGE_CASE_PREFIX = "tests/corpus/synthetic/"


def process_edge_case(rel, path, rust_dump, oracle_python, manifest, result):
    manifest_key = rel
    if manifest_key.startswith(EDGE_CASE_PREFIX):
        manifest_key = manifest_key[len(EDGE_CASE_PREFIX):]
    entry = manifest.get(manifest_key)
    if entry is None:
        result.status = "unexplained"
        result.note(
            "{}: no manifest.json entry for {!r} (run tools/gen-corpus.py "
            "write-all)".format(rel, manifest_key))
        return result
    klass = entry["class"]
    check = CLASSES.get(klass)
    if check is None:
        result.status = "unexplained"
        result.note("{}: unknown class {!r} in manifest".format(rel, klass))
        return result

    if oracle_python is None:
        result.status = "unexplained"
        result.note("{}: no oracle python available".format(rel))
        return result

    rust, rust_err = rust_dump_json(rust_dump, path)
    antlr, antlr_err = oracle_dump_json(oracle_python, "antlr", path)
    textx, textx_err = oracle_dump_json(oracle_python, "textx", path)
    if rust is None or antlr is None or textx is None:
        result.status = "unexplained"
        result.note("{}: a dump failed: rust={} antlr={} textx={}".format(
            rel, rust_err, antlr_err, textx_err))
        return result

    if check(rust, antlr, textx):
        result.status = klass
        result.note(
            "{}: matches expected class {!r} ({})".format(
                rel, klass, entry.get("source", "")))
    else:
        result.status = "unexplained"
        result.note(
            "{}: does NOT match expected class {!r} ({})\n"
            "  rust:  {}\n  antlr: {}\n  textx: {}".format(
                rel, klass, entry.get("source", ""), rust, antlr, textx))
    return result


def process_plain(rel, path, rust_dump, oracle_python, result,
                  expected_error=None):
    if oracle_python is None:
        result.status = "unexplained"
        result.note("{}: no oracle python available".format(rel))
        return result

    rust, rust_err = rust_dump_json(rust_dump, path)
    antlr, antlr_err = oracle_dump_json(oracle_python, "antlr", path)
    textx, textx_err = oracle_dump_json(oracle_python, "textx", path)
    if rust is None or antlr is None or textx is None:
        result.status = "unexplained"
        result.note("{}: a dump failed: rust={} antlr={} textx={}".format(
            rel, rust_err, antlr_err, textx_err))
        return result

    if expected_error is not None:
        line = expected_error["line"]
        if _class_all_three_reject(rust, antlr, textx, line):
            result.status = "all_three_reject"
        else:
            result.status = "unexplained"
        result.note(
            "{}: listed in {} as rejected by every parser at line {} "
            "({}): {}\n  rust:  {}\n  antlr: {}\n  textx: {}".format(
                rel, EXPECTED_ERRORS_NAME, line,
                expected_error.get("source", ""),
                "matches" if result.status == "all_three_reject" else
                "does NOT match", rust, antlr, textx))
        return result

    if not (rust == antlr == textx):
        if _in_xilinx_error_corpus(rel) and _class_xilinx_error_corpus(
                rust, antlr, textx):
            result.status = "xilinx_error_corpus"
            result.note(
                "{}: parse tree differs, but matches the "
                "xilinx_error_corpus class (T5.4 fasm2frames error-path "
                "corpus; Rust and the ANTLR oracle both report a parse "
                "error, message not compared -- see the module "
                "docstring)\n"
                "  rust:  {}\n  antlr: {}\n  textx: {}".format(
                    rel, rust, antlr, textx))
            return result
        result.status = "unexplained"
        result.note(
            "{}: parse tree differs (expected identical: this is plain, "
            "realistic FASM, not a documented edge case)\n"
            "  rust:  {}\n  antlr: {}\n  textx: {}".format(
                rel, rust, antlr, textx))
        return result

    if _err(rust):
        # A "plain" corpus file that no parser accepts: still identical
        # across all three (same error-vs-not outcome), nothing further
        # to check (to-string/round-trip need a parsed model).
        result.status = "identical"
        result.note("{}: all three agree (parse error): {}".format(
            rel, rust["error"]))
        return result

    # (b) fasm_tuple_to_string, canonical and non-canonical.
    for canonical in (False, True):
        r_text, r_err = rust_to_string(rust_dump, path, canonical)
        o_text, o_err = oracle_to_string(oracle_python, path, canonical)
        if r_text is None or o_text is None:
            result.status = "unexplained"
            result.note(
                "{}: to_string(canonical={}) failed: rust_err={} "
                "oracle_err={}".format(rel, canonical, r_err, o_err))
            return result
        if r_text != o_text:
            result.status = "unexplained"
            result.note(
                "{}: to_string(canonical={}) differs\n  rust:   {!r}\n"
                "  oracle: {!r}".format(rel, canonical, r_text, o_text))
            return result

    # (c) round trip: rust parse -> to_string -> rust parse, and oracle
    # parse of the same rust-printed text.
    rt_text, rt_err = rust_to_string(rust_dump, path, False)
    if rt_text is None:
        result.status = "unexplained"
        result.note("{}: round-trip to_string failed: {}".format(rel, rt_err))
        return result
    fd, rt_path = tempfile.mkstemp(suffix=".fasm")
    try:
        with os.fdopen(fd, "w") as f:
            f.write(rt_text)
        rust2, rust2_err = rust_dump_json(rust_dump, rt_path)
        antlr2, antlr2_err = oracle_dump_json(oracle_python, "antlr", rt_path)
        if rust2 is None or antlr2 is None:
            result.status = "unexplained"
            result.note(
                "{}: round-trip dump failed: rust={} antlr={}".format(
                    rel, rust2_err, antlr2_err))
            return result
        if rust2 != rust:
            result.status = "unexplained"
            result.note(
                "{}: round trip changed the Rust parse tree\n"
                "  original: {}\n  after round trip: {}".format(
                    rel, rust, rust2))
            return result
        if antlr2 != rust:
            result.status = "unexplained"
            result.note(
                "{}: oracle parse of the Rust-printed text differs from "
                "the original Rust parse tree\n  original: {}\n"
                "  oracle(rust output): {}".format(rel, rust, antlr2))
            return result
    finally:
        os.remove(rt_path)

    result.status = "identical"
    result.note(
        "{}: parse tree, to_string (both modes) and round trip all "
        "match the oracle".format(rel))
    return result


def process_one(task):
    (abs_path, rel, category, rust_dump, oracle_python, manifest,
     expected_errors) = task
    result = FileResult(rel, category)
    work_path, tmp_path = maybe_decompress(abs_path)
    try:
        if category == "invalid":
            return process_invalid(rel, work_path, rust_dump, result)
        if category == "edge_case":
            return process_edge_case(
                rel, work_path, rust_dump, oracle_python, manifest, result)
        return process_plain(rel, work_path, rust_dump, oracle_python, result,
                             expected_errors.get(rel))
    finally:
        if tmp_path is not None:
            os.remove(tmp_path)


# ---------------------------------------------------------------------
# Stress file (generated on the fly, never committed)
# ---------------------------------------------------------------------
def run_stress(rust_dump, size_bytes, seed, report_lines):
    fd, path = tempfile.mkstemp(suffix=".fasm", prefix="fasm-stress-")
    os.close(fd)
    try:
        t0 = time.time()
        subprocess.run(
            [sys.executable, GEN_CORPUS_PY, "stress", "--out", path,
             "--size", str(size_bytes), "--seed", str(seed)],
            check=True)
        gen_time = time.time() - t0
        actual_size = os.path.getsize(path)

        t0 = time.time()
        rc, out, err = _run([rust_dump, path])
        parse_time = time.time() - t0
        try:
            doc = json.loads(out)
        except ValueError:
            doc = None

        ok = doc is not None and not _err(doc)
        report_lines.append(
            "stress: generated {} bytes in {:.2f}s (seed {}), fasm-dump "
            "parsed it in {:.2f}s -> {}".format(
                actual_size, gen_time, seed, parse_time,
                "OK ({} lines)".format(len(doc["lines"])) if ok else
                "FAILED: {}".format(err or doc)))

        rt_ok = True
        if ok:
            r_text, r_err = rust_to_string(rust_dump, path, False)
            if r_text is None:
                rt_ok = False
                report_lines.append(
                    "stress: to_string failed: {}".format(r_err))
            else:
                fd2, rt_path = tempfile.mkstemp(suffix=".fasm")
                try:
                    with os.fdopen(fd2, "w") as f:
                        f.write(r_text)
                    doc2, err2 = rust_dump_json(rust_dump, rt_path)
                    rt_ok = doc2 == doc
                    report_lines.append(
                        "stress: round trip (rust only; the oracle is not "
                        "run over stress files, it is far too slow) -> "
                        "{}".format("OK" if rt_ok else
                                     "MISMATCH: {}".format(err2)))
                finally:
                    os.remove(rt_path)
        return ok and rt_ok
    finally:
        os.remove(path)


# ---------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------
def find_oracle_python(explicit):
    if explicit:
        return explicit if os.path.exists(explicit) else None
    for candidate in DEFAULT_ORACLE_PYTHON_CANDIDATES:
        if os.path.exists(candidate):
            return candidate
    return None


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jobs", "-j", type=int,
                         default=os.cpu_count() or 1)
    parser.add_argument("--filter", default=None,
                         help="glob (fnmatch) over the repo-relative path")
    parser.add_argument("--rust-dump", default=DEFAULT_RUST_DUMP)
    parser.add_argument("--oracle-python", default=None,
                         help="defaults to tests/oracle/venv/bin/python in "
                         "this checkout, falling back to the main "
                         "checkout's")
    parser.add_argument("--report", default=None,
                         help="path to write full details to (default: a "
                         "temp file, path is always printed)")
    parser.add_argument("--size", type=int, default=None,
                         help="also generate and check an on-the-fly "
                         "stress file of this many bytes (see "
                         "tools/gen-corpus.py stress); not committed")
    parser.add_argument("--stress-seed", type=int, default=0)
    args = parser.parse_args(argv)

    rust_dump = args.rust_dump
    if not os.path.exists(rust_dump):
        print(
            "SKIP: fasm-dump not found at {} -- build it with "
            "`cargo build --release --example dump -p fasm`".format(
                rust_dump), file=sys.stderr)
        return EXIT_NOT_SET_UP

    oracle_python = find_oracle_python(args.oracle_python)
    if oracle_python is None:
        print(
            "SKIP: no oracle venv found (looked in {}) -- run "
            "tests/oracle/setup.sh, or pass --oracle-python".format(
                DEFAULT_ORACLE_PYTHON_CANDIDATES), file=sys.stderr)
        return EXIT_NOT_SET_UP

    manifest = load_manifest()
    files = discover_corpus(args.filter)
    if not files:
        print("no corpus files found (filter={!r})".format(args.filter),
              file=sys.stderr)
        return EXIT_NOT_SET_UP

    expected_errors = load_expected_errors()
    tasks = [
        (abs_path, rel, category, rust_dump, oracle_python, manifest,
         expected_errors)
        for abs_path, rel, category in files
    ]

    t0 = time.time()
    if args.jobs > 1:
        with multiprocessing.Pool(args.jobs) as pool:
            results = list(pool.imap_unordered(process_one, tasks))
    else:
        results = [process_one(t) for t in tasks]
    elapsed = time.time() - t0

    counts = {}
    unexplained = []
    report_lines = [
        "fasm differential test report",
        "corpus: {} files, {:.1f}s, {} jobs".format(
            len(results), elapsed, args.jobs),
        "rust-dump: {}".format(rust_dump),
        "oracle-python: {}".format(oracle_python),
        "",
    ]
    for r in sorted(results, key=lambda r: r.rel):
        counts[r.status] = counts.get(r.status, 0) + 1
        report_lines.append("[{}] {} ({})".format(r.status, r.rel, r.category))
        for line in r.details:
            report_lines.append("  " + line.replace("\n", "\n  "))
        if r.status == "unexplained":
            unexplained.append(r)

    stress_ok = True
    if args.size:
        stress_lines = []
        stress_ok = run_stress(
            rust_dump, args.size, args.stress_seed, stress_lines)
        report_lines.append("")
        report_lines.extend(stress_lines)

    report_path = args.report
    if report_path is None:
        fd, report_path = tempfile.mkstemp(
            suffix=".txt", prefix="fasm-difftest-report-")
        os.close(fd)
    with open(report_path, "w") as f:
        f.write("\n".join(report_lines) + "\n")

    # Summary table.
    print("fasm differential test: {} files in {:.1f}s ({} jobs)".format(
        len(results), elapsed, args.jobs))
    print("{:<32} {:>6}".format("status", "count"))
    print("-" * 40)
    for status in sorted(counts):
        print("{:<32} {:>6}".format(status, counts[status]))
    print("-" * 40)
    print("{:<32} {:>6}".format("TOTAL", len(results)))
    if args.size:
        print("stress ({} bytes): {}".format(
            args.size, "OK" if stress_ok else "FAILED"))
    print()
    print("full report: {}".format(report_path))

    if unexplained or not stress_ok:
        print()
        print("UNEXPLAINED differences ({}):".format(len(unexplained)))
        for r in unexplained:
            print("  {} ({})".format(r.rel, r.category))
        if not stress_ok:
            print("  stress file check failed, see report")
        return EXIT_UNEXPLAINED

    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main())

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
"""Deterministic, stdlib-only synthetic FASM corpus generator (T1.5).

Two independent jobs:

1. ``write-all``: (re)writes the committed synthetic corpus under
   ``tests/corpus/synthetic/`` --

   * ``edge-cases/<category>/<NNN>-<slug>.fasm``: one small FASM file per
     documented grammar production or ``docs/rewrite/COMPAT.md``
     divergence, plus ``edge-cases/manifest.json`` recording, for every
     file, the divergence ``class`` (see ``CLASSES`` below and
     ``tools/difftest.py``, which uses this to know what kind of
     difference between the Rust and oracle parse trees is *expected*
     for that file rather than a bug) and a human readable
     ``description``/``source`` (which ``docs/rewrite/COMPAT.md`` table
     row it came from).
   * ``invalid/<NNN>-<slug>.fasm`` + ``.expected``: one invalid FASM
     construct per file; the sidecar names the line:column and
     ``ParseErrorKind`` the Rust parser is expected to report (checked by
     ``tools/difftest.py`` against ``fasm-dump``'s output, independently of
     the oracle -- some of these inputs crash the reference ANTLR C++
     extension outright, so they are never run through it).
   * ``xilinx-like.fasm``: a synthetic but realistic 7-series-shaped FASM
     file (tile names, pips, a 64 bit LUT ``INIT``, a 256 bit BRAM
     ``INIT``, IOB features, annotations and comments), which all three
     parsers are expected to agree on exactly (class ``same_all_three``).

2. ``stress``: writes one large, deterministic FASM file to ``--out``
   (realistic pip/LUT/BRAM shaped lines, repeated/varied until the target
   size is reached). Used by ``tools/difftest.py --size`` to generate a
   stress file on the fly; **not** committed to the corpus (the plan caps
   the corpus at a few MB; a 100 MB file obviously does not belong in
   git).

Both are seeded (``--seed``, default 0) so re-running produces byte
identical output.
"""
import argparse
import json
import os
import random
import sys

# ---------------------------------------------------------------------
# edge-cases: (category, slug, class, source, content) tuples.
#
# `content` is either a `str` (UTF-8 encoded, a trailing "\n" is added if
# missing) or `bytes` (written verbatim, for cases about raw encoding/BOM
# that a `str` cannot represent exactly).
#
# `class` values (see `tools/difftest.py`'s `CLASSES` doc for exactly what
# each one checks for):
#   same_all_three            - rust == antlr == textx parse tree, exactly.
#   rust_relaxes_antlr        - antlr rejects; rust and textx agree.
#   rust_follows_antlr_over_textx - textx rejects; rust and antlr agree.
#   antlr_bug_wrong_decode    - antlr "succeeds" but decodes the value
#                                wrongly; rust matches textx.
#   rust_stricter             - antlr and textx both accept; rust rejects
#                                (a model invariant, e.g. end < start).
#   non_ascii_antlr_exception - antlr raises inside its ctypes callback
#                                (caught, not a crash); rust matches textx.
# ---------------------------------------------------------------------
EDGE_CASES = [
    # --- grammar: plain production coverage (all three agree) ----------
    ("grammar", "bare-feature", "same_all_three",
     "docs/specification/syntax.rst grammar", "A.B.C\n"),
    ("grammar", "implicit-one", "same_all_three",
     "docs/specification/syntax.rst grammar", "INT_L_X10Y146.SW6BEG0.WW2END0\n"),
    ("grammar", "explicit-address", "same_all_three",
     "docs/specification/syntax.rst grammar",
     "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT[17]\n"),
    ("grammar", "explicit-range", "same_all_three",
     "docs/specification/syntax.rst grammar",
     "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[63:32] = "
     "32'b11110000111100001111000011110000\n"),
    ("grammar", "explicit-one-value", "same_all_three",
     "docs/specification/syntax.rst grammar",
     "INT_L_X10Y146.SW6BEG0.WW2END0 = 1\n"),
    ("grammar", "explicit-zero-value", "same_all_three",
     "docs/specification/syntax.rst grammar",
     "INT_L_X10Y146.SW6BEG0.WW2END0 = 0\n"),
    ("grammar", "hex-value", "same_all_three", "grammar: VerilogValue",
     "A[7:0] = 8'hFF\n"),
    ("grammar", "bin-value", "same_all_three", "grammar: VerilogValue",
     "A[3:0] = 4'b1111\n"),
    ("grammar", "oct-value", "same_all_three", "grammar: VerilogValue",
     "A[7:0] = 8'o377\n"),
    ("grammar", "dec-value", "same_all_three", "grammar: VerilogValue",
     "A[7:0] = 8'd255\n"),
    ("grammar", "plain-decimal", "same_all_three", "grammar: VerilogValue",
     "A[7:0] = 255\n"),
    ("grammar", "annotation-single", "same_all_three",
     "docs/specification/syntax.rst Annotations example",
     'INT_L_X10Y146.SW6BEG0.WW2END0 { .attr = "" }\n'),
    ("grammar", "annotation-multi", "same_all_three",
     "docs/specification/syntax.rst Annotations example",
     'INT_L_X10Y146.SW6BEG0.WW2END0 { module = "top", '
     'file = "/a/b/d.txt", line_number = "123" }\n'),
    ("grammar", "annotation-only", "same_all_three",
     "docs/specification/syntax.rst Annotations example",
     '{ .top_module = "/a/b/c/d.txt" }\n'),
    ("grammar", "feature-annotation-comment", "same_all_three",
     "docs/specification/syntax.rst Annotations example",
     'INT_L_X10Y146.SW6BEG0.WW2END0 { .top_module = "/a/b/c/d.txt" } '
     '# This is a comment\n'),
    ("grammar", "comment-only", "same_all_three", "grammar: Comment",
     "# just a comment\n"),
    ("grammar", "bare-hash", "same_all_three", "grammar: Comment",
     "#\n"),
    ("grammar", "crlf-line-ending", "same_all_three",
     "COMPAT.md, Line endings and positions", b"a\r\nb\r\n"),
    ("grammar", "lone-cr-line-ending", "same_all_three",
     "COMPAT.md, Line endings and positions", b"a\rb\r\n"),

    # --- COMPAT.md: rust relaxes over ANTLR (textx + spec agree) -------
    ("compat", "underscore-plain-decimal", "rust_relaxes_antlr",
     "COMPAT.md table: 'Accepted by Rust, rejected by ANTLR'",
     "a[15:0] = 1_000\n"),
    ("compat", "underscore-address", "rust_relaxes_antlr",
     "COMPAT.md table: 'Accepted by Rust, rejected by ANTLR'",
     "a[1_0]\n"),

    # --- COMPAT.md: rust follows ANTLR over textX -----------------------
    ("compat", "escaped-quote-in-annotation", "rust_follows_antlr_over_textx",
     'COMPAT.md table: \'Accepted by Rust and ANTLR, rejected by textX\'',
     '{ a = "x\\"y" }\n'),
    ("compat", "declared-width-gt-address-width",
     "rust_follows_antlr_over_textx",
     "COMPAT.md table: 'Accepted by Rust and ANTLR, rejected by textX'",
     "a[0] = 3'b001\n"),
    ("compat", "whitespace-around-address", "rust_follows_antlr_over_textx",
     "COMPAT.md table: 'Accepted by Rust and ANTLR, rejected by textX'",
     "a[ 3 : 0 ] = 4'hF\n"),
    ("compat", "whitespace-before-comma", "rust_follows_antlr_over_textx",
     "COMPAT.md table: 'Accepted by Rust and ANTLR, rejected by textX'",
     '{ a="1" , b="2" }\n'),
    ("compat", "radix-no-digit", "rust_follows_antlr_over_textx",
     "COMPAT.md table: 'Accepted by Rust and ANTLR, rejected by textX'",
     "a = 'h_\n"),
    ("compat", "utf8-bom", "rust_follows_antlr_over_textx",
     "COMPAT.md table: 'Accepted by Rust and ANTLR, rejected by textX'",
     b"\xef\xbb\xbfa\n"),

    # --- COMPAT.md: ANTLR bugs, Rust matches textX ----------------------
    # (a[31:0] = 2147483648: ANTLR's stoi based decimal decoder actually
    # *rejects* this one outright ("Could not decode decimal number."),
    # verified against the live oracle -- so this is a rejection, not a
    # wrong-decode: rust_relaxes_antlr, not antlr_bug_wrong_decode.)
    ("compat", "decimal-gt-2-31", "rust_relaxes_antlr",
     "COMPAT.md table: 'ANTLR bugs: Rust does the sane thing'",
     "a[31:0] = 2147483648\n"),
    ("compat", "d-value-gt-2-32", "antlr_bug_wrong_decode",
     "COMPAT.md table: 'ANTLR bugs: Rust does the sane thing'",
     "a[63:0] = 'd4294967296\n"),
    ("compat", "octal-gt-10-digits", "antlr_bug_wrong_decode",
     "COMPAT.md table: 'ANTLR bugs: Rust does the sane thing'",
     "a[63:0] = 'o1234567012345670123\n"),
    ("compat", "octal-leading-zeros-gt-10-digits", "antlr_bug_wrong_decode",
     "COMPAT.md table: 'ANTLR bugs: Rust does the sane thing'",
     "a[30:0] = 'o00_017777777777\n"),
    ("compat", "octal-11-digits", "antlr_bug_wrong_decode",
     "COMPAT.md table: 'ANTLR bugs: Rust does the sane thing'",
     "a[32:0] = 31'o14404165671\n"),
    ("compat", "octal-32-bit-max", "antlr_bug_wrong_decode",
     "COMPAT.md table: 'ANTLR bugs: Rust does the sane thing'",
     "a[31:0] = 'o37777777777\n"),
    ("compat", "whitespace-after-hex-radix", "antlr_bug_wrong_decode",
     "COMPAT.md table: 'ANTLR bugs: Rust does the sane thing'",
     "a[63:0] = 'h F\n"),
    # ('d 5: same stoi rejection as decimal-gt-2-31 above; verified
    # against the live oracle.)
    ("compat", "whitespace-after-dec-radix", "rust_relaxes_antlr",
     "COMPAT.md table: 'ANTLR bugs: Rust does the sane thing'",
     "a[7:0] = 'd 5\n"),
    # ('b 101: ANTLR decodes this correctly "by accident" per COMPAT.md;
    # verified against the live oracle all three agree exactly.)
    ("compat", "whitespace-after-bin-radix", "same_all_three",
     "COMPAT.md table: 'ANTLR bugs: Rust does the sane thing'",
     "a[7:0] = 'b 101\n"),
    # (4300 nines: over ANTLR's much lower internal limit, so it rejects
    # outright; verified against the live oracle.)
    ("compat", "huge-decimal-4300-digits", "rust_relaxes_antlr",
     "COMPAT.md table: 'ANTLR bugs: Rust does the sane thing'",
     "a[20000:0] = " + ("9" * 4300) + "\n"),

    # --- COMPAT.md: rust stricter (model invariants) --------------------
    ("compat", "end-before-start-zero-value", "rust_stricter",
     "COMPAT.md table: 'Rejected by Rust, accepted by ANTLR'",
     "a[0:1] = 0\n"),

    # --- COMPAT.md: non-ASCII (ANTLR raises, caught, not a crash) -------
    ("compat", "non-ascii-comment", "non_ascii_antlr_exception",
     "COMPAT.md table: 'Non-ASCII input and encodings'",
     "# comment é\n"),
    ("compat", "non-ascii-annotation-value", "non_ascii_antlr_exception",
     "COMPAT.md table: 'Non-ASCII input and encodings'",
     '{ a = "é" }\n'),
]

# `invalid/`: (slug, content, expected_line, expected_column, expected_kind)
# `expected_kind` is the `ParseErrorKind` variant name fasm-dump's error
# text is expected to be consistent with (checked loosely: difftest.py
# only asserts line:column, `expected_kind` is recorded for humans and
# possible future stricter checks).
INVALID_CASES = [
    ("unexpected-token", "a b\n", 1, 2, "Syntax"),
    ("unterminated-annotation-value", '{ a = "b\n', 1, 6, "Syntax"),
    ("bad-annotation-escape", '{ a = "x\\q" }\n', 1, 6, "Syntax"),
    ("address-gt-u32", "a[4294967296] = 0\n", 1, 2, "AddressOutOfRange"),
    ("address-gt-u64", "a[18446744073709551616] = 0\n", 1, 2,
     "AddressOutOfRange"),
    ("full-u32-range", "a[4294967295:0]\n", 1, 1, "AddressOutOfRange"),
    ("end-before-start-nonzero", "a[0:1] = 3\n", 1, 1, "AddressEndBeforeStart"),
    ("value-exceeds-declared-width", "a[7:0] = 8'h1FF\n", 1, 9,
     "ValueExceedsDeclaredWidth"),
    ("value-exceeds-address-width", "a[3:0] = 5'hFF\n", 1, 9,
     "ValueExceedsAddressWidth"),
    ("decimal-too-long", "a[20000:0] = " + ("9" * 4301) + "\n", 1, 13,
     "DecimalValueTooLong"),
    ("invalid-utf8-comment", b"a # \xff\n", 1, 4, "InvalidUtf8"),
]

XILINX_TILE_ROWS = 8
XILINX_TILE_COLS = 6


def _write(path, content):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    mode = "wb" if isinstance(content, bytes) else "w"
    data = content
    if isinstance(content, str) and not content.endswith("\n"):
        data = content + "\n"
    with open(path, mode, newline="" if mode == "w" else None) as f:
        if mode == "w":
            f.write(data)
        else:
            f.write(data)


def write_edge_cases(out_dir):
    manifest = {}
    base = os.path.join(out_dir, "edge-cases")
    for i, (category, slug, klass, source, content) in enumerate(EDGE_CASES):
        name = "{:03d}-{}.fasm".format(i, slug)
        rel = os.path.join("edge-cases", category, name)
        _write(os.path.join(out_dir, rel), content)
        manifest[rel.replace(os.sep, "/")] = {
            "category": category,
            "class": klass,
            "source": source,
        }
    manifest_path = os.path.join(base, "manifest.json")
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2, sort_keys=True)
        f.write("\n")
    return len(EDGE_CASES)


def write_invalid_cases(out_dir):
    base = os.path.join(out_dir, "invalid")
    for i, (slug, content, line, column, kind) in enumerate(INVALID_CASES):
        name = "{:03d}-{}.fasm".format(i, slug)
        _write(os.path.join(base, name), content)
        expected = "{}:{}\n{}\n".format(line, column, kind)
        with open(os.path.join(base, name + ".expected"), "w") as f:
            f.write(expected)
    return len(INVALID_CASES)


def _lut_init(rng, bits):
    return "".join(rng.choice("01") for _ in range(bits))


def xilinx_like_lines(rng):
    """Yields realistic-shaped 7 series FASM lines (deterministic, given
    `rng`)."""
    yield "# Synthetic 7-series-shaped FASM, generated by tools/gen-corpus.py"
    yield "# for the T1.5 differential test corpus. Not real prjxray output."
    yield ""
    for row in range(XILINX_TILE_ROWS):
        for col in range(XILINX_TILE_COLS):
            x, y = col * 2, row * 2
            clb = "CLBLL_L_X{}Y{}".format(x, y)
            slice_ = rng.choice(["SLICEL_X0", "SLICEL_X1"])
            yield "{}.{}.ALUT.INIT[{:02d}]".format(
                clb, slice_, rng.randrange(64))
            lut_init = _lut_init(rng, 64)
            yield "{}.{}.BLUT.INIT[63:0] = 64'b{}".format(
                clb, slice_, lut_init)
            yield "{}.{}.AFFMUX.AX".format(clb, slice_)
            yield "{}.{}.CLKINV".format(clb, slice_)
            int_tile = "INT_L_X{}Y{}".format(x + 1, y)
            a = rng.randrange(20)
            b = rng.randrange(20)
            yield "{}.EL1BEG{}.LOGIC_OUTS_L{} {{ .pip = \"1\" }}".format(
                int_tile, a, b)
            if (row, col) == (0, 0):
                bram_init = _lut_init(rng, 256)
                yield ("RAMB18_X0Y{}.RAMB18E1.INIT_00[255:0] = "
                       "256'b{}".format(y, bram_init))
            if col == 0:
                iob = "LIOB33_X0Y{}".format(y)
                yield "{}.IOB_Y0.ISTANDARD.LVCMOS33".format(iob)
                yield "{}.IOB_Y0.OUTBUF_EN".format(iob)
            if col == XILINX_TILE_COLS - 1:
                iob = "RIOB33_X43Y{}".format(y)
                yield "{}.IOB_Y1.PULLTYPE.PULLUP".format(iob)
        yield "# row {} done".format(row)


def write_xilinx_like(out_dir, seed):
    rng = random.Random(seed)
    path = os.path.join(out_dir, "xilinx-like.fasm")
    with open(path, "w") as f:
        for line in xilinx_like_lines(rng):
            f.write(line + "\n")
    return path


def stress_lines(rng):
    """Endless generator of realistic-shaped FASM lines for `stress()`."""
    row = 0
    while True:
        for col in range(64):
            x, y = col * 2, row * 2
            clb = "CLBLL_L_X{}Y{}".format(x, y)
            slice_ = rng.choice(["SLICEL_X0", "SLICEL_X1"])
            yield "{}.{}.ALUT.INIT[{:02d}]\n".format(
                clb, slice_, rng.randrange(64))
            yield "{}.{}.BLUT.INIT[63:0] = 64'b{}\n".format(
                clb, slice_, _lut_init(rng, 64))
            int_tile = "INT_L_X{}Y{}".format(x + 1, y)
            a = rng.randrange(20)
            b = rng.randrange(20)
            yield "{}.EL1BEG{}.LOGIC_OUTS_L{}\n".format(int_tile, a, b)
        row += 1


def write_stress(out_path, size_bytes, seed):
    rng = random.Random(seed)
    written = 0
    with open(out_path, "w") as f:
        for line in stress_lines(rng):
            f.write(line)
            written += len(line)
            if written >= size_bytes:
                break
    return written


def cmd_write_all(args):
    out_dir = args.out_dir
    n_edge = write_edge_cases(out_dir)
    n_invalid = write_invalid_cases(out_dir)
    xilinx_path = write_xilinx_like(out_dir, args.seed)
    print("wrote {} edge-case files, {} invalid-case files, {}".format(
        n_edge, n_invalid, xilinx_path))


def cmd_stress(args):
    written = write_stress(args.out, args.size, args.seed)
    print("wrote {} bytes to {}".format(written, args.out), file=sys.stderr)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    p_all = sub.add_parser(
        "write-all", help="(re)write the committed synthetic corpus")
    p_all.add_argument(
        "--out-dir", default="tests/corpus/synthetic",
        help="directory to write edge-cases/, invalid/ and xilinx-like.fasm "
        "into (default: tests/corpus/synthetic)")
    p_all.add_argument("--seed", type=int, default=0)
    p_all.set_defaults(func=cmd_write_all)

    p_stress = sub.add_parser(
        "stress", help="write one large synthetic FASM file (not committed)")
    p_stress.add_argument("--out", required=True)
    p_stress.add_argument(
        "--size", type=int, required=True, help="target size in bytes")
    p_stress.add_argument("--seed", type=int, default=0)
    p_stress.set_defaults(func=cmd_stress)

    args = parser.parse_args(argv)
    args.func(args)
    return 0


if __name__ == "__main__":
    sys.exit(main())

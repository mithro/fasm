#!/usr/bin/env bash
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
#
# Copies the results of a successful `tools/e2e/run-fpgas-online.sh DESIGN
# BOARD` run into the corpus, at
#   tests/corpus/xilinx/artix7/designs/fpgas.online-test-designs/<design>/<board>/
# writing a README.md with the part, the fpgas.online-test-designs commit,
# reproduction commands, timings and checksums (T7.2).
#
# Usage:
#   tools/e2e/install-fpgas-online-corpus.sh DESIGN BOARD
#
# Requires tools/e2e/run-fpgas-online.sh DESIGN BOARD to have completed
# successfully first (reads its tools/e2e/build/out/fpgas-online/<design>-<board>/
# output).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FPGAS_SRC="$SCRIPT_DIR/build/fpgas.online-test-designs"
OUT_DIR_BASE="$SCRIPT_DIR/build/out/fpgas-online"

[[ $# -eq 2 ]] || { echo "usage: $0 DESIGN BOARD" >&2; exit 2; }
DESIGN="$1"
BOARD="$2"
OUT_DIR="$OUT_DIR_BASE/$DESIGN-$BOARD"
CORPUS_DIR="$REPO_ROOT/tests/corpus/xilinx/artix7/designs/fpgas.online-test-designs/$DESIGN/$BOARD"

[[ -f "$OUT_DIR/summary.txt" ]] || { echo "install-fpgas-online-corpus.sh: $OUT_DIR/summary.txt not found; run tools/e2e/run-fpgas-online.sh $DESIGN $BOARD first" >&2; exit 1; }

read -r FASM_SHA256 FASM_BYTES FRM_SHA256 FRM_BYTES SPARSE_FRM_SHA256 BIT_SHA256 BIT_BYTES FASM_LINES BUILD_SECONDS DENSE_SECONDS SPARSE_SECONDS BIT_SECONDS < "$OUT_DIR/summary.txt"

CFG="$(bash "$SCRIPT_DIR/run-fpgas-online.sh" --config "$DESIGN" "$BOARD")"
IFS='|' read -r SCRIPT_REL PART FAMILY EXTRA_ARGS KIND <<<"$CFG"
[[ "$EXTRA_ARGS" == "-" ]] && EXTRA_ARGS=""

COMMIT="$(cat "$FPGAS_SRC/.checkout-commit" 2>/dev/null || echo unknown)"

mkdir -p "$CORPUS_DIR"

# FASM: xz if > 1 MiB, else plain text (matches the f4pga-examples corpus
# convention documented in tools/e2e/README.md / the counter_test README).
FASM_NAME="top.fasm"
rm -f "$CORPUS_DIR/top.fasm" "$CORPUS_DIR/top.fasm.xz"
if [[ "$FASM_BYTES" -gt 1048576 ]]; then
  xz -9 -k -c "$OUT_DIR/top.fasm" > "$CORPUS_DIR/top.fasm.xz"
  FASM_NAME="top.fasm.xz"
else
  cp "$OUT_DIR/top.fasm" "$CORPUS_DIR/top.fasm"
fi

# .frm: always xz (dense .frm is typically several MiB of mostly-zero
# frame words; xz -9 shrinks it drastically, see the counter_test README).
xz -9 -k -c "$OUT_DIR/top.frm" > "$CORPUS_DIR/top.frm.xz"
xz -9 -k -c "$OUT_DIR/top.sparse.frm" > "$CORPUS_DIR/top.sparse.frm.xz"

# .bit is NEVER committed (not byte-reproducible -- embeds a build
# timestamp -- see the sha256 recorded in README.md below instead).

XZ_FASM_BYTES=""
if [[ -f "$CORPUS_DIR/top.fasm.xz" ]]; then
  XZ_FASM_BYTES=$(stat -c%s "$CORPUS_DIR/top.fasm.xz")
fi
XZ_FRM_BYTES=$(stat -c%s "$CORPUS_DIR/top.frm.xz")
XZ_SPARSE_FRM_BYTES=$(stat -c%s "$CORPUS_DIR/top.sparse.frm.xz")
DATE_GENERATED="$(date -u +%Y-%m-%d)"

cat > "$CORPUS_DIR/README.md" <<EOF
# $DESIGN / $BOARD -- fpgas.online-test-designs FASM (T7.2)

FASM produced by building fpgas.online-test-designs' \`$DESIGN\` design for
the \`$BOARD\` board with LiteX + the openXC7 flow (T7.1/T7.2). \`$FASM_NAME\`
is real, working FASM (not hand-written).

Regenerate with:

\`\`\`
tools/e2e/setup-openxc7.sh --parts $PART   # from /home/user/fasm (main tree); once per part
tools/e2e/setup-litex.sh                    # once per machine
tools/e2e/run-fpgas-online.sh $DESIGN $BOARD
tools/e2e/install-fpgas-online-corpus.sh $DESIGN $BOARD
\`\`\`

## Target

* Part: \`$PART\`
* Design: fpgas.online-test-designs \`designs/$DESIGN/\` (see its own
  \`README.md\` in that repository for what the test verifies)
* Board: \`$BOARD\`
* Gateware script: \`designs/$DESIGN/$SCRIPT_REL\`

## Source provenance

* fpgas.online-test-designs commit \`$COMMIT\`
  (<https://github.com/fpgas-online/fpgas.online-test-designs>, Apache-2.0)
* LiteX stack pinned to the same commits as that repository's own
  \`uv.lock\` -- see \`tools/e2e/setup-litex.sh\` header comment and
  \`tools/e2e/build/litex-venv/status.json\`.
* openXC7 toolchain: see
  \`tests/corpus/xilinx/artix7/designs/f4pga-examples/counter_test/arty_35/README.md\`
  for exact yosys/nextpnr-xilinx/openXC7 versions and provenance (same
  toolchain install, shared across all T7.x corpus designs on this
  machine).
* \`.frm\`/\`.bit\` regenerated from the FASM with the ORACLE tools
  (\`tests/oracle/fasm2frames-oracle\`, \`tests/oracle/xc7frames2bit-oracle\`,
  \`tests/oracle/bitread-oracle\` -- f4pga-xc-fasm + prjxray C++, built by
  \`tests/oracle/setup-xilinx.sh\`), **not** openXC7's own bundled copies of
  the same tools, so this is directly comparable with the rest of the Rust
  rewrite's differential tests.

## Commands (as run by tools/e2e/run-fpgas-online.sh)

\`\`\`
# LiteX build (yosys synth_xilinx -> nextpnr-xilinx -> FASM; the exact
# commands are in the generated designs/$DESIGN/build/$BOARD/build_top.sh
# or */gateware/build_*.sh, which is not committed):
CHIPDB=<chipdb overlay dir> PRJXRAY_DB_DIR=<prjxray-db root> \\
  tools/e2e/build/litex-venv/bin/python \\
  tools/e2e/build/fpgas.online-test-designs/designs/$DESIGN/$SCRIPT_REL \\
  --toolchain openxc7 --build${EXTRA_ARGS:+ $EXTRA_ARGS}

# Reference frames + bitstream (from the FASM above):
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/$FAMILY --part $PART top.fasm top.frm
tests/oracle/fasm2frames-oracle --db-root <prjxray-db>/$FAMILY --part $PART --sparse top.fasm top.sparse.frm
tests/oracle/xc7frames2bit-oracle -frm_file top.frm -output_file top.bit \\
  -part_name $PART -part_file <prjxray-db>/$FAMILY/$PART/part.yaml
\`\`\`

## Output checksums (this run; \`.bit\` is NOT committed)

\`\`\`
sha256  top.fasm (uncompressed)   $FASM_SHA256
sha256  top.frm (uncompressed)    $FRM_SHA256
sha256  top.bit (not committed)   $BIT_SHA256
\`\`\`

top.fasm: $FASM_LINES lines / $FASM_BYTES bytes uncompressed$( [[ -n "$XZ_FASM_BYTES" ]] && echo ", $XZ_FASM_BYTES bytes as top.fasm.xz" )
top.frm: $FRM_BYTES bytes uncompressed, $XZ_FRM_BYTES bytes as top.frm.xz
top.sparse.frm: $XZ_SPARSE_FRM_BYTES bytes as top.sparse.frm.xz
top.bit: $BIT_BYTES bytes (not committed; regenerate to verify)

## Timings (this machine, this run)

* LiteX build (synth + PnR + FASM): ${BUILD_SECONDS}s
* fasm2frames (dense): ${DENSE_SECONDS}s
* fasm2frames (sparse): ${SPARSE_SECONDS}s
* xc7frames2bit: ${BIT_SECONDS}s

Generated $DATE_GENERATED.
EOF

echo "installed $DESIGN/$BOARD into $CORPUS_DIR"
ls -la "$CORPUS_DIR"

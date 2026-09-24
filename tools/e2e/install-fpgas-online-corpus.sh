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
# output). If that output is gone but the design is already installed in
# the corpus, pass --regen-readme-only to rewrite just its README.md (e.g.
# after fixing a documentation bug here) from the data already recorded in
# the existing README.md, without touching top.fasm/top.frm.xz/etc -- see
# "Regenerating READMEs only" below.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FPGAS_SRC="$SCRIPT_DIR/build/fpgas.online-test-designs"
OUT_DIR_BASE="$SCRIPT_DIR/build/out/fpgas-online"

REGEN_ONLY=0
ARGS=()
for a in "$@"; do
  case "$a" in
    --regen-readme-only) REGEN_ONLY=1 ;;
    *) ARGS+=("$a") ;;
  esac
done
set -- "${ARGS[@]}"

[[ $# -eq 2 ]] || { echo "usage: $0 DESIGN BOARD [--regen-readme-only]" >&2; exit 2; }
DESIGN="$1"
BOARD="$2"
OUT_DIR="$OUT_DIR_BASE/$DESIGN-$BOARD"
CORPUS_DIR="$REPO_ROOT/tests/corpus/xilinx/artix7/designs/fpgas.online-test-designs/$DESIGN/$BOARD"

CFG="$(bash "$SCRIPT_DIR/run-fpgas-online.sh" --config "$DESIGN" "$BOARD")"
IFS='|' read -r SCRIPT_REL PART FAMILY EXTRA_ARGS KIND <<<"$CFG"
[[ "$EXTRA_ARGS" == "-" ]] && EXTRA_ARGS=""

COMMIT="$(cat "$FPGAS_SRC/.checkout-commit" 2>/dev/null || echo unknown)"

# --- Fixed, repo-root-relative locations of the databases this design's
# .frm/.bit were (and are, on every re-run) regenerated against. Both are
# deterministic given the layout tools/e2e/setup-openxc7.sh and
# run-fpgas-online.sh always use -- not per-machine absolute paths, so
# these are safe to hardcode into a committed README.
#
# IMPORTANT (fixed after T7.2 review): this is the openXC7 SNAP's OWN
# bundled prjxray-db (from opt/nextpnr-xilinx/external/prjxray-db inside
# the extracted snap -- the same copy nextpnr-xilinx's own chipdb and the
# LiteX openxc7 toolchain build against), NOT the independently pinned
# f4pga/prjxray-db that tests/oracle/setup-xilinx.sh's own tools/fetch-db.sh
# fetches for the rest of this repo's Xilinx differential tests (tests
# built by run-fpgas-online.sh always export PRJXRAY_DB_DIR from
# tools/e2e/openxc7-env.sh, which points here). See "Database provenance"
# below and tools/e2e/README.md's "fpgas.online-test-designs corpus"
# section ("A note on prjxray-db provenance") for the exact, verified
# differences between the two and which designs are sensitive to them.
SNAP_PRJXRAY_DB_REL="tools/e2e/build/openxc7/root/opt/nextpnr-xilinx/external/prjxray-db"
CHIPDB_OVERLAY_REL="tools/e2e/build/chipdb-overlay"
OPENXC7_SNAP_VERSION="0.8.2"
OPENXC7_SNAP_SHA256="6b2e07ce99ef33d3a4e41e2fd2eb916f26bb0ece34a97216ed840a0032e98587"
PRJXRAY_INFO_COMMIT="4c157493ec9f13caea4ad3f0c02f8f318f198846"
PRJXRAY_INFO_DATE="Tue Dec 14 07:31:38 PM UTC 2021"

mkdir -p "$CORPUS_DIR"

if [[ "$REGEN_ONLY" -eq 1 ]]; then
  OLD_README="$CORPUS_DIR/README.md"
  [[ -f "$OLD_README" ]] || { echo "install-fpgas-online-corpus.sh: --regen-readme-only but $OLD_README does not exist yet; run without it first" >&2; exit 1; }
  # Recover the recorded data from the existing README.md (checksums for
  # top.bit in particular can ONLY come from here -- .bit is never
  # committed, so there is nothing else to hash it from). None of
  # top.fasm/top.frm.xz/top.sparse.frm.xz/checksums change by this --
  # only the README's own prose (Commands/Source-provenance sections) was
  # wrong; this mode never re-derives or re-verifies the artifacts
  # themselves, it only re-renders the text around them.
  extract() { grep -oP "$1" "$OLD_README" | head -1; }
  FASM_SHA256="$(extract 'sha256\s+top\.fasm \(uncompressed\)\s+\K[0-9a-f]+')"
  FRM_SHA256="$(extract 'sha256\s+top\.frm \(uncompressed\)\s+\K[0-9a-f]+')"
  BIT_SHA256="$(extract 'sha256\s+top\.bit \(not committed\)\s+\K[0-9a-f]+')"
  FASM_LINES="$(extract 'top\.fasm: \K[0-9]+(?= lines)')"
  FASM_BYTES="$(extract 'top\.fasm: [0-9]+ lines / \K[0-9]+(?= bytes uncompressed)')"
  XZ_FASM_BYTES="$(extract 'top\.fasm:.*bytes uncompressed, \K[0-9]+(?= bytes as top\.fasm\.xz)' || true)"
  FRM_BYTES="$(extract 'top\.frm: \K[0-9]+(?= bytes uncompressed)')"
  XZ_FRM_BYTES="$(extract 'top\.frm: [0-9]+ bytes uncompressed, \K[0-9]+(?= bytes as top\.frm\.xz)')"
  XZ_SPARSE_FRM_BYTES="$(extract 'top\.sparse\.frm: \K[0-9]+(?= bytes as top\.sparse\.frm\.xz)')"
  BIT_BYTES="$(extract 'top\.bit: \K[0-9]+(?= bytes \(not committed)')"
  BUILD_SECONDS="$(extract 'LiteX build \(synth \+ PnR \+ FASM\): \K[0-9]+(?=s)')"
  DENSE_SECONDS="$(extract 'fasm2frames \(dense\): \K[0-9]+(?=s)')"
  SPARSE_SECONDS="$(extract 'fasm2frames \(sparse\): \K[0-9]+(?=s)')"
  BIT_SECONDS="$(extract 'xc7frames2bit: \K[0-9]+(?=s)')"
  DATE_GENERATED="$(extract 'Generated \K[0-9-]+(?=\.)')"
  for v in FASM_SHA256 FRM_SHA256 BIT_SHA256 FASM_LINES FASM_BYTES FRM_BYTES \
    XZ_FRM_BYTES XZ_SPARSE_FRM_BYTES BIT_BYTES BUILD_SECONDS DENSE_SECONDS \
    SPARSE_SECONDS BIT_SECONDS DATE_GENERATED; do
    [[ -n "${!v}" ]] || { echo "install-fpgas-online-corpus.sh: could not recover $v from $OLD_README" >&2; exit 1; }
  done
  FASM_NAME="top.fasm"; [[ -f "$CORPUS_DIR/top.fasm.xz" ]] && FASM_NAME="top.fasm.xz"
else
  [[ -f "$OUT_DIR/summary.txt" ]] || { echo "install-fpgas-online-corpus.sh: $OUT_DIR/summary.txt not found; run tools/e2e/run-fpgas-online.sh $DESIGN $BOARD first (or pass --regen-readme-only to only rewrite an existing README.md)" >&2; exit 1; }

  read -r FASM_SHA256 FASM_BYTES FRM_SHA256 FRM_BYTES SPARSE_FRM_SHA256 BIT_SHA256 BIT_BYTES FASM_LINES BUILD_SECONDS DENSE_SECONDS SPARSE_SECONDS BIT_SECONDS < "$OUT_DIR/summary.txt"

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
fi

PRJXRAY_INFO_COMMIT_SHORT="${PRJXRAY_INFO_COMMIT:0:8}"
SENSITIVE_NOTE=""
if [[ "$DESIGN" == "spi-flash-id" ]]; then
  SENSITIVE_NOTE=" -- **this design is one of the ones sensitive to it**: its SPI clock is routed through \`STARTUPE2\`'s \`USRCCLKO\` pin (see \`designs/spi-flash-id/gateware/*.py\`'s own docstring), which sets \`CFG_CENTER_MID.STARTUP.USRCCLKO_CONNECTED\` -- a tag present only in the snap db, not the pinned one (see tools/e2e/README.md)."
fi

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
  the same tools -- but run against the **openXC7 snap's own bundled
  prjxray-db** (\`$SNAP_PRJXRAY_DB_REL\`), the same database
  nextpnr-xilinx's chipdb and the whole LiteX openxc7 flow are built
  against for this design, **not** the independently pinned
  \`f4pga/prjxray-db\` that \`tests/oracle/setup-xilinx.sh\`'s own
  \`tools/fetch-db.sh\` fetches for the rest of this repo's Xilinx
  differential tests. openXC7 snap \`$OPENXC7_SNAP_VERSION\`
  (sha256 \`$OPENXC7_SNAP_SHA256\`); its bundled prjxray-db's own
  \`Info.md\` records "Created using Project X-Ray version
  [$PRJXRAY_INFO_COMMIT_SHORT](https://github.com/SymbiFlow/prjxray/commit/$PRJXRAY_INFO_COMMIT),
  last updated $PRJXRAY_INFO_DATE" (an earlier draft of
  tools/e2e/README.md claimed this database "carries no version marker
  the way \`tools/fetch-db.sh\` pins prjxray-db for the oracle" --
  corrected after T7.2 review: \`Info.md\` does record one, it is simply a
  different, independent pin from \`tools/fetch-db.sh\`'s). See
  tools/e2e/README.md ("A note on prjxray-db provenance") for the exact,
  verified tile/segbits/ppips differences against the pinned db${SENSITIVE_NOTE}

## Commands (as run by tools/e2e/run-fpgas-online.sh)

\`\`\`
# LiteX build (yosys synth_xilinx -> nextpnr-xilinx -> FASM; the exact
# commands are in the generated designs/$DESIGN/build/$BOARD/build_top.sh
# or */gateware/build_*.sh, which is not committed):
CHIPDB=$CHIPDB_OVERLAY_REL PRJXRAY_DB_DIR=$SNAP_PRJXRAY_DB_REL \\
  tools/e2e/build/litex-venv/bin/python \\
  tools/e2e/build/fpgas.online-test-designs/designs/$DESIGN/$SCRIPT_REL \\
  --toolchain openxc7 --build${EXTRA_ARGS:+ $EXTRA_ARGS}

# Reference frames + bitstream (from the FASM above; --db-root is the
# snap's bundled prjxray-db -- see "Source provenance" above):
tests/oracle/fasm2frames-oracle --db-root $SNAP_PRJXRAY_DB_REL/$FAMILY --part $PART top.fasm top.frm
tests/oracle/fasm2frames-oracle --db-root $SNAP_PRJXRAY_DB_REL/$FAMILY --part $PART --sparse top.fasm top.sparse.frm
tests/oracle/xc7frames2bit-oracle -frm_file top.frm -output_file top.bit \\
  -part_name $PART -part_file $SNAP_PRJXRAY_DB_REL/$FAMILY/$PART/part.yaml
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

SPDX-License-Identifier: Apache-2.0 (fpgas.online-test-designs sources,
this README) -- the openXC7 snap's bundled prjxray-db used to regenerate
\`.frm\`/\`.bit\` above is CC0-1.0 (see its own \`README.md\`/\`COPYING\`).
EOF

echo "installed $DESIGN/$BOARD into $CORPUS_DIR"
ls -la "$CORPUS_DIR"

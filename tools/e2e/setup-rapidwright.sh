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
# Sets up RapidWright (T7.5) for the RapidWright cross-checks of
# tools/e2e/run-rapidwright-checks.sh: the pinned standalone jar, the
# RapidWright device files of the parts we have prjxray-db / prjuray-db
# databases (or ToolsTestData bitstreams) for, and the compiled driver
# tools/e2e/rapidwright/RwCheck.java. No Vivado is needed (or used).
# See docs/rewrite/DESIGN-rapidwright.md for what RapidWright can and
# cannot do here.
#
# Usage:
#   tools/e2e/setup-rapidwright.sh [--force] [--with-interchange]
#
#   --force              delete tools/e2e/build/rapidwright and set it up
#                        again.
#   --with-interchange   also set up the FPGA interchange route to FASM
#                        (rwcheck.py fasm): a venv with
#                        python-fpga-interchange, the interchange schema
#                        RapidWright is built with, and capnproto-java's
#                        java.capnp (imported by that schema).
#
# Layout (gitignored, tools/e2e/build/rapidwright):
#   rapidwright-2026.1.0-standalone-lin64.jar
#   data/parts.db, data/devices/<family>/<device>_db.dat (+ .md5, the file
#       RapidWright itself checks: with it in place RapidWright does not
#       download anything; RAPIDWRIGHT_PATH points here)
#   classes/RwCheck.class, classes/RwDesign.class
#   venv-interchange/, fpga-interchange-schema/, capnp-include/,
#       pfi-data/, dcps/ (with --with-interchange)
#   status.json
#
# Requires: java/javac >= 11 (Java 21 used), curl, sha256sum, md5sum.
#
# --- Pins (resolved 2026-09-25) ------------------------------------------
#
#   RapidWright v2026.1.0-beta (tag commit
#   127f55cd704c277372697e699f1559e1cdc91f34, released 2026-06-30):
#     https://github.com/Xilinx/RapidWright/releases/download/v2026.1.0-beta/rapidwright-2026.1.0-standalone-lin64.jar
#     sha256 18f81833595ef8a5191a72f9431727e17602de2fe4dddbba2d281358801a1fc9
#     (115043629 bytes; contains rapidwright-api-lib 2026.1.0, the closed
#     source part with com.xilinx.rapidwright.bitstream)
#
#   Device files: http://data.rapidwright.io/<container>/<md5>, the
#   container names and md5 sums of that tag's
#   src/com/xilinx/rapidwright/util/DataVersions.java (the URL scheme of
#   FileTools.downloadDataFile); the sha256 sums below were recorded on
#   the first download.
#
#   --with-interchange:
#     chipsalliance/python-fpga-interchange
#       04a02101d1f7f03a2d33716192fb478e1e8605af (its last commit, 0.0.18),
#       installed without its pins: pycapnp==1.3.0 (1.1.0 does not build
#       with Cython 3; the 1.x API; rapidwright/pfi_run.py adapts
#       from_bytes's context manager), python-sat==1.9.dev15,
#       PyYAML==6.0.3 (its "pyyaml" patch format; the "yaml" one needs a
#       rapidyaml fork that is not installed)
#     chipsalliance/fpga-interchange-schema
#       c985b4648e66414b250261c1ba4cbe45a2971b1c (RapidWright's
#       interchange/fpga-interchange-schema submodule at the tag)
#     capnproto/capnproto-java v0.1.16 compiler/src/main/schema/capnp/java.capnp
#       sha256 abc48d859ffa06ac26c7dfe6020374fb0ee5efa4936707abc35bdac2233aefab
#     python-fpga-interchange's test_data/series7_{constraints,luts}.yaml
#       (the device patches; pip does not install test_data)
#     Xilinx/RapidWrightDCP f9625fc62d290926668c4955c3a76e9d2044e916
#       (RapidWright's test/RapidWrightDCP submodule at the tag): its nine
#       DCPs for 7 series parts and xczu3eg, sha256 below

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BUILD="${REPO_ROOT}/tools/e2e/build/rapidwright"

RW_VERSION="2026.1.0"
RW_TAG="v2026.1.0-beta"
RW_JAR="rapidwright-${RW_VERSION}-standalone-lin64.jar"
RW_JAR_URL="https://github.com/Xilinx/RapidWright/releases/download/${RW_TAG}/${RW_JAR}"
RW_JAR_SHA256="18f81833595ef8a5191a72f9431727e17602de2fe4dddbba2d281358801a1fc9"
DATA_URL="http://data.rapidwright.io"

# relative path | container | md5 (DataVersions.java) | sha256
DATA_FILES=(
    "data/parts.db|parts-db|58dd6f20c37798322b6904a8a786a3de|d3a8ff8d73d7f355c8307a2f25bbbfde260d47e5c464ed86d9de9efbec66d343"
    "data/devices/artix7/xc7a35t_db.dat|xc7a35t-db-dat|d3110fa703bbcfb846afe6872918eca9|beeaea64fc83f7e0dc73a2593899ca776465996f6f189eecd2761ccd6cac9c23"
    "data/devices/artix7/xc7a50t_db.dat|xc7a50t-db-dat|373fdc19e37ba22ea8fbc1594f158f54|bdd799f352e0166e7f8c4271d962d8fadaa21547b80020499c1ccf403989f6a4"
    "data/devices/artix7/xc7a100t_db.dat|xc7a100t-db-dat|608abdfbe87e5d09c802ac01546ed5f3|7e78931821ca68e59864e10258713e130d62cdcb728480a9ff00aafb19ead53c"
    "data/devices/artix7/xc7a200t_db.dat|xc7a200t-db-dat|8857943adfa294f75aca06e7c68a1c1b|b3c9ab322cbf3e1750718ea3552ec3802782d5e823dc5d2a97d0823bfc5f7f3b"
    "data/devices/kintex7/xc7k70t_db.dat|xc7k70t-db-dat|b972f5f5380ebcc74e87eb0a5217d576|1fab48f1805ce4bed3e43f5939ae77aa20ea124d00b415ad5ed67571bd1bd687"
    "data/devices/spartan7/xc7s50_db.dat|xc7s50-db-dat|1bc0c54720391f7ce7b9330d2d194189|16da009849189b6cc3f162ce6f184671a4b8d5f1ed5be7513e5a59580d6392e7"
    "data/devices/zynq/xc7z010_db.dat|xc7z010-db-dat|13278d81f49ab7cfb587f9b42745d830|ba10d6619a2695f7604af175f0e4df20bb88911da598062768e2aba1d0975883"
    "data/devices/zynq/xc7z020_db.dat|xc7z020-db-dat|92c72e3b19439f5ed1b38d0dae45dd03|5ee62a285d25538d095bb739d88633ac79e18fa2f04533ac34e1853df92c1df6"
    "data/devices/kintexu/xcku035_db.dat|xcku035-db-dat|18927636cd0611f9f651ba1a2e25e63d|06196bce733adfb299316211e6e9236fb7826e22636658aa24bac1836f109f86"
    "data/devices/zynquplus/xczu3eg_db.dat|xczu3eg-db-dat|7087560c8e4722d5d53af2d78c0ee250|4abb685b6acddeb4eb5b8db9e583aa1d54f10da41c5a86964de91142501ba74f"
)

FORCE=0
INTERCHANGE=0
for arg in "$@"; do
    case "$arg" in
        --force) FORCE=1 ;;
        --with-interchange) INTERCHANGE=1 ;;
        -h|--help) sed -n '18,40p' "$0"; exit 0 ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

if [ "$FORCE" = 1 ]; then
    rm -rf "$BUILD"
fi
mkdir -p "$BUILD"

for tool in java javac curl sha256sum md5sum; do
    command -v "$tool" >/dev/null || { echo "setup-rapidwright: $tool not found" >&2; exit 1; }
done

# fetch URL DEST SHA256 [MD5]
fetch() {
    local url="$1" dest="$2" sha="$3" md5="${4:-}"
    if [ -f "$dest" ] && [ "$(sha256sum "$dest" | cut -d' ' -f1)" = "$sha" ]; then
        return 0
    fi
    mkdir -p "$(dirname "$dest")"
    echo "setup-rapidwright: downloading $url"
    timeout 900 curl -fsSL --retry 3 -o "$dest.part" "$url"
    local got
    got="$(sha256sum "$dest.part" | cut -d' ' -f1)"
    if [ "$got" != "$sha" ]; then
        echo "setup-rapidwright: sha256 mismatch for $url: $got (expected $sha)" >&2
        rm -f "$dest.part"
        exit 1
    fi
    if [ -n "$md5" ] && [ "$(md5sum "$dest.part" | cut -d' ' -f1)" != "$md5" ]; then
        echo "setup-rapidwright: md5 mismatch for $url" >&2
        rm -f "$dest.part"
        exit 1
    fi
    mv "$dest.part" "$dest"
}

fetch "$RW_JAR_URL" "$BUILD/$RW_JAR" "$RW_JAR_SHA256"

for entry in "${DATA_FILES[@]}"; do
    IFS='|' read -r rel container md5 sha <<<"$entry"
    fetch "$DATA_URL/$container/$md5" "$BUILD/$rel" "$sha" "$md5"
    # The file RapidWright compares with DataVersions before downloading.
    printf '%s' "$md5" > "$BUILD/$rel.md5"
done

mkdir -p "$BUILD/classes"
javac -nowarn -d "$BUILD/classes" -cp "$BUILD/$RW_JAR" \
    "$REPO_ROOT/tools/e2e/rapidwright/RwCheck.java" \
    "$REPO_ROOT/tools/e2e/rapidwright/RwDesign.java" 2>&1 | grep -v '^Picked up' || true
test -f "$BUILD/classes/RwCheck.class"
test -f "$BUILD/classes/RwDesign.class"

PFI_COMMIT="04a02101d1f7f03a2d33716192fb478e1e8605af"
SCHEMA_COMMIT="c985b4648e66414b250261c1ba4cbe45a2971b1c"
JAVA_CAPNP_URL="https://raw.githubusercontent.com/capnproto/capnproto-java/v0.1.16/compiler/src/main/schema/capnp/java.capnp"
JAVA_CAPNP_SHA256="abc48d859ffa06ac26c7dfe6020374fb0ee5efa4936707abc35bdac2233aefab"
if [ "$INTERCHANGE" = 1 ]; then
    if [ ! -x "$BUILD/venv-interchange/bin/python" ]; then
        python3 -m venv "$BUILD/venv-interchange"
    fi
    V="$BUILD/venv-interchange/bin/pip"
    timeout 1800 "$V" install -q --only-binary=:all: pycapnp==1.3.0 \
        python-sat==1.9.dev15 PyYAML==6.0.3
    timeout 1800 "$V" install -q --no-deps \
        "git+https://github.com/chipsalliance/python-fpga-interchange.git@${PFI_COMMIT}"
    if [ ! -d "$BUILD/fpga-interchange-schema/.git" ]; then
        timeout 600 git clone -q https://github.com/chipsalliance/fpga-interchange-schema.git \
            "$BUILD/fpga-interchange-schema"
    fi
    git -C "$BUILD/fpga-interchange-schema" checkout -q "$SCHEMA_COMMIT"
    fetch "$JAVA_CAPNP_URL" "$BUILD/capnp-include/capnp/java.capnp" "$JAVA_CAPNP_SHA256"
    PFI_RAW="https://raw.githubusercontent.com/chipsalliance/python-fpga-interchange/${PFI_COMMIT}/test_data"
    fetch "$PFI_RAW/series7_constraints.yaml" "$BUILD/pfi-data/series7_constraints.yaml" \
        "c105d3f9f0d09dce5a78179fcc0a46dbcb4bcba66cafa42289f63133a55f4961"
    fetch "$PFI_RAW/series7_luts.yaml" "$BUILD/pfi-data/series7_luts.yaml" "5208272cccc3bf1a5a44a86a691f02d41da7079ef97f4f918daa48f0cfbe4a50"
    # The 7 series and xczu3eg DCPs of RapidWright's test data (Vivado
    # placed and routed, with readable EDIF), fetched one by one from a
    # blobless clone (the whole repository is about 150 MB).
    DCP_COMMIT="f9625fc62d290926668c4955c3a76e9d2044e916"
    DCPS=(
        "routethru_luts.dcp e449fc87233d89474457513189f9ed61310f1d26555f398ffe246ffbbb128905"
        "routethru_pip.dcp 4f534ec63a7cf068906f5143d73d97eb9dd826fb92eaacc3a2ecbf901b5f9cce"
        "ramb18.dcp 68aedfd03d08dbcecc0de0f2c6d7bab5e480b62de7ffc1c241e760c7ca3db89a"
        "bug226.dcp c416adb235bd72639ac57d1256a2ec8a99c33998c2a65b3585c0fac5f05d4215"
        "bug349.dcp b3f725d5b1d38a44ed5b2c6947ab6d43cc13e35fcbeac11d4c83cbb63b6518e0"
        "bug635.dcp 3f69b0a4161f7e70cae83ab19df613a19f32181cce9db492815100e0705fef39"
        "bug709.dcp 0879309ed25f2f19579159eb8adc14fbbc31eddf60115f222a1a30f300ca1d8a"
        "verilog_ethernet.dcp 01e01522036f2b1523342605983f69a20df20b58e0af8549824b726da9fc96de"
        "bug701.dcp 6cc11c37989346da02db26e5de7ab7277a7396ef330b4d61d26d6b2111658de9"
    )
    missing=0
    for entry in "${DCPS[@]}"; do
        read -r name sha <<<"$entry"
        if [ ! -f "$BUILD/dcps/$name" ] || \
                [ "$(sha256sum "$BUILD/dcps/$name" | cut -d' ' -f1)" != "$sha" ]; then
            missing=1
        fi
    done
    if [ "$missing" = 1 ]; then
        rm -rf "$BUILD/rapidwright-dcp.git"
        timeout 900 git clone -q --filter=blob:none --no-checkout \
            https://github.com/Xilinx/RapidWrightDCP.git "$BUILD/rapidwright-dcp.git"
        mkdir -p "$BUILD/dcps"
        for entry in "${DCPS[@]}"; do
            read -r name sha <<<"$entry"
            timeout 900 git -C "$BUILD/rapidwright-dcp.git" show "$DCP_COMMIT:$name" \
                > "$BUILD/dcps/$name"
            if [ "$(sha256sum "$BUILD/dcps/$name" | cut -d' ' -f1)" != "$sha" ]; then
                echo "setup-rapidwright: sha256 mismatch for $name" >&2
                exit 1
            fi
        done
        rm -rf "$BUILD/rapidwright-dcp.git"
    fi
fi

cat > "$BUILD/status.json" <<EOF
{
  "rapidwright_tag": "$RW_TAG",
  "jar": "$RW_JAR",
  "jar_sha256": "$RW_JAR_SHA256",
  "java": "$(java -version 2>&1 | grep -v '^Picked up' | head -1 | sed 's/"/\\"/g')"
}
EOF
echo "setup-rapidwright: ready in $BUILD"

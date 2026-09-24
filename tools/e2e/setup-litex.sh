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
# Sets up a Python venv with the LiteX SoC builder + peripheral cores
# (T7.2), pinned to the exact commits that
# https://github.com/fpgas-online/fpgas-online/fpgas.online-test-designs
# uses (its pyproject.toml `[project.optional-dependencies].build` /
# `uv.lock`), so `tools/e2e/run-fpgas-online.sh` can drive its LiteX SoC
# gateware scripts through the openXC7 flow installed by
# `tools/e2e/setup-openxc7.sh` (T7.1).
#
# This is deliberately separate from setup-openxc7.sh: that script installs
# the synthesis/place-and-route *toolchain* (yosys, nextpnr-xilinx,
# fasm2frames, xc7frames2bit); this one installs the *Python SoC builder*
# (LiteX/migen) whose job is to emit the Verilog those tools consume and
# then invoke them.
#
# Usage:
#   tools/e2e/setup-litex.sh [--force]
#
#   --force   Delete tools/e2e/build/litex-venv and reinstall from scratch
#             (same pins). Without --force this is a fast no-op once set
#             up successfully.
#
# Everything this script creates lives under the gitignored
# tools/e2e/build/ -- nothing here is committed to git.
#
# --- Pinned packages (resolved 2026-09-24 from fpgas.online-test-designs' ---
# --- uv.lock at commit 37d24079b28179558632abc12fd92af4ff00a036) -----------
#
#   litex                            git+https://github.com/enjoy-digital/litex.git@e2625f98d25920af46a2cdc3564145b48435e4e9
#   litex-boards                     git+https://github.com/litex-hub/litex-boards.git@dc89d1177ca63d220e253d0c2f9970221e21378e
#   migen                            git+https://github.com/m-labs/migen.git@e19524c963a8342952840983047557707fbe0b6a
#   litedram                         git+https://github.com/enjoy-digital/litedram.git@51de2b05e9b8e555cde8ff5508b5996945a2fd22
#   liteeth                          git+https://github.com/enjoy-digital/liteeth.git@5689d38172f2abcfc177b2714b7eb46c13231776
#   litepcie                         git+https://github.com/enjoy-digital/litepcie.git@aceef740dbfeae08c4349d2b497c917070d7bfe9
#   litespi                          git+https://github.com/litex-hub/litespi.git@4c04564178fb3314ed2e33c56afb8e313eda45e3
#   litescope                        git+https://github.com/enjoy-digital/litescope.git@2dafca2ddce94101fdacf45acde643573eeb5307
#   pythondata-cpu-vexriscv          git+https://github.com/litex-hub/pythondata-cpu-vexriscv.git@1979a644dbe64d8d32dfbdd970dccee6add63723
#   pythondata-software-compiler-rt  git+https://github.com/litex-hub/pythondata-software-compiler_rt.git@fcb03245613ccf3079cc833a701f13d0beaae09d
#   pythondata-software-picolibc     git+https://github.com/litex-hub/pythondata-software-picolibc.git@4dbc2c3ffb06454a91d3101e27fa625d3a5f2069
#   pyserial>=3.5, pyyaml (plain LiteX runtime deps, from PyPI, unpinned by
#     upstream's own lock beyond normal semver -- pip resolves the latest
#     compatible release)
#
# The RISC-V GCC cross-compiler needed to compile a BIOS/firmware for
# designs with a CPU (uart, spi-flash-id, ddr-memory, ethernet-test,
# pcie-enumeration) is NOT installed by this script by default: T7.2
# prefers building those designs with `--no-compile-software` /
# `--no-compile-gateware`-style flags that avoid needing it, since only
# the gateware/FASM matters for this task, not a working BIOS binary. Pass
# `--with-riscv-gcc` to also fetch it (xpack RISC-V GCC 14.2.0-3, ~100 MB,
# URL taken from fpgas.online-test-designs' own
# scripts/setup_toolchains.py) for designs where that turns out to be
# unavoidable (see tools/e2e/README.md for which ones needed it).
set -euo pipefail

FORCE=0
WITH_RISCV_GCC=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --force)
      FORCE=1
      shift
      ;;
    --with-riscv-gcc)
      WITH_RISCV_GCC=1
      shift
      ;;
    -h | --help)
      sed -n '2,60p' "$0" | sed -e 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "setup-litex.sh: unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="$SCRIPT_DIR/build"
VENV_DIR="$BUILD_DIR/litex-venv"
LOG_DIR="$BUILD_DIR/logs"
DL_DIR="$BUILD_DIR/downloads"
RISCV_DIR="$BUILD_DIR/riscv-gcc"
STATUS_FILE="$VENV_DIR/status.json"
MARKER="$VENV_DIR/.setup-ok"

# Pins (see header comment above for provenance).
LITEX_COMMIT="e2625f98d25920af46a2cdc3564145b48435e4e9"
LITEX_BOARDS_COMMIT="dc89d1177ca63d220e253d0c2f9970221e21378e"
MIGEN_COMMIT="e19524c963a8342952840983047557707fbe0b6a"
LITEDRAM_COMMIT="51de2b05e9b8e555cde8ff5508b5996945a2fd22"
LITEETH_COMMIT="5689d38172f2abcfc177b2714b7eb46c13231776"
LITEPCIE_COMMIT="aceef740dbfeae08c4349d2b497c917070d7bfe9"
LITESPI_COMMIT="4c04564178fb3314ed2e33c56afb8e313eda45e3"
LITESCOPE_COMMIT="2dafca2ddce94101fdacf45acde643573eeb5307"
VEXRISCV_COMMIT="1979a644dbe64d8d32dfbdd970dccee6add63723"
COMPILER_RT_COMMIT="fcb03245613ccf3079cc833a701f13d0beaae09d"
PICOLIBC_COMMIT="4dbc2c3ffb06454a91d3101e27fa625d3a5f2069"

RISCV_GCC_VERSION="14.2.0-3"
RISCV_GCC_URL="https://github.com/xpack-dev-tools/riscv-none-elf-gcc-xpack/releases/download/v${RISCV_GCC_VERSION}/xpack-riscv-none-elf-gcc-${RISCV_GCC_VERSION}-linux-x64.tar.gz"

log() {
  echo "[setup-litex] $*"
}

if [[ "$FORCE" -eq 1 ]]; then
  log "--force: removing $VENV_DIR"
  rm -rf "$VENV_DIR"
fi

mkdir -p "$BUILD_DIR" "$LOG_DIR" "$DL_DIR"

if [[ -f "$MARKER" && -f "$STATUS_FILE" ]] && { [[ "$WITH_RISCV_GCC" -eq 0 ]] || [[ -x "$RISCV_DIR"/*/bin/riscv-none-elf-gcc ]] 2>/dev/null; }; then
  log "already set up at $VENV_DIR (pass --force to reinstall)"
  cat "$STATUS_FILE"
  exit 0
fi

SETUP_START=$(date +%s)

log "creating venv at $VENV_DIR (python3 $(python3 --version 2>&1))"
python3 -m venv "$VENV_DIR"
VENV_PY="$VENV_DIR/bin/python"
"$VENV_PY" -m pip install --quiet --upgrade pip wheel setuptools > "$LOG_DIR/pip-bootstrap.log" 2>&1

log "installing pinned LiteX stack (this clones each repo at its pinned commit; several minutes)"
"$VENV_PY" -m pip install --quiet \
  "pyserial>=3.5" \
  "pyyaml" \
  "meson>=1.0" \
  "ninja" \
  "migen @ git+https://github.com/m-labs/migen.git@${MIGEN_COMMIT}" \
  "litex @ git+https://github.com/enjoy-digital/litex.git@${LITEX_COMMIT}" \
  "litex-boards @ git+https://github.com/litex-hub/litex-boards.git@${LITEX_BOARDS_COMMIT}" \
  "litedram @ git+https://github.com/enjoy-digital/litedram.git@${LITEDRAM_COMMIT}" \
  "liteeth @ git+https://github.com/enjoy-digital/liteeth.git@${LITEETH_COMMIT}" \
  "litepcie @ git+https://github.com/enjoy-digital/litepcie.git@${LITEPCIE_COMMIT}" \
  "litespi @ git+https://github.com/litex-hub/litespi.git@${LITESPI_COMMIT}" \
  "litescope @ git+https://github.com/enjoy-digital/litescope.git@${LITESCOPE_COMMIT}" \
  "pythondata-cpu-vexriscv @ git+https://github.com/litex-hub/pythondata-cpu-vexriscv.git@${VEXRISCV_COMMIT}" \
  "pythondata-software-compiler-rt @ git+https://github.com/litex-hub/pythondata-software-compiler_rt.git@${COMPILER_RT_COMMIT}" \
  "pythondata-software-picolibc @ git+https://github.com/litex-hub/pythondata-software-picolibc.git@${PICOLIBC_COMMIT}" \
  > "$LOG_DIR/pip-install.log" 2>&1 \
  || { echo "setup-litex.sh: ERROR: pip install failed; see $LOG_DIR/pip-install.log" >&2; tail -60 "$LOG_DIR/pip-install.log" >&2; exit 1; }

INSTALLED_VERSIONS="$("$VENV_PY" -m pip freeze 2>/dev/null | grep -Ei '^(litex|migen|liteeth|litedram|litepcie|litespi|litescope|pythondata)' || true)"
log "installed:"
echo "$INSTALLED_VERSIONS" | sed -e 's/^/  /'

RISCV_GCC_BIN=""
RISCV_GCC_SHA256=""
if [[ "$WITH_RISCV_GCC" -eq 1 ]]; then
  RISCV_FILE="$DL_DIR/xpack-riscv-none-elf-gcc-${RISCV_GCC_VERSION}-linux-x64.tar.gz"
  if [[ ! -f "$RISCV_FILE" ]]; then
    log "downloading RISC-V GCC (xpack ${RISCV_GCC_VERSION}, ~100 MB)"
    curl -fSL --retry 3 --retry-delay 5 -o "$RISCV_FILE.part" "$RISCV_GCC_URL"
    mv "$RISCV_FILE.part" "$RISCV_FILE"
  fi
  RISCV_GCC_SHA256="$(sha256sum "$RISCV_FILE" | awk '{print $1}')"
  mkdir -p "$RISCV_DIR"
  if [[ ! -x "$RISCV_DIR"/*/bin/riscv-none-elf-gcc ]] 2>/dev/null; then
    log "extracting RISC-V GCC"
    tar xzf "$RISCV_FILE" -C "$RISCV_DIR"
  fi
  RISCV_GCC_BIN="$(dirname "$(find "$RISCV_DIR" -maxdepth 3 -name 'riscv-none-elf-gcc' -type f | head -1)")"
  log "RISC-V GCC at $RISCV_GCC_BIN"
fi

SETUP_END=$(date +%s)

STATUS_PY="
import json, os, subprocess

status = {
    'venv_dir': os.environ['VENV_DIR'],
    'python': subprocess.run([os.environ['VENV_PY'], '--version'], capture_output=True, text=True).stdout.strip()
        or subprocess.run([os.environ['VENV_PY'], '--version'], capture_output=True, text=True).stderr.strip(),
    'pins': {
        'litex': os.environ['LITEX_COMMIT'],
        'litex-boards': os.environ['LITEX_BOARDS_COMMIT'],
        'migen': os.environ['MIGEN_COMMIT'],
        'litedram': os.environ['LITEDRAM_COMMIT'],
        'liteeth': os.environ['LITEETH_COMMIT'],
        'litepcie': os.environ['LITEPCIE_COMMIT'],
        'litespi': os.environ['LITESPI_COMMIT'],
        'litescope': os.environ['LITESCOPE_COMMIT'],
        'pythondata-cpu-vexriscv': os.environ['VEXRISCV_COMMIT'],
        'pythondata-software-compiler-rt': os.environ['COMPILER_RT_COMMIT'],
        'pythondata-software-picolibc': os.environ['PICOLIBC_COMMIT'],
    },
    'pinned_from': 'fpgas.online-test-designs uv.lock at commit 37d24079b28179558632abc12fd92af4ff00a036',
    'installed_versions': os.environ.get('INSTALLED_VERSIONS', '').splitlines(),
    'riscv_gcc': {
        'installed': bool(os.environ.get('RISCV_GCC_BIN')),
        'bin_dir': os.environ.get('RISCV_GCC_BIN') or None,
        'version': os.environ['RISCV_GCC_VERSION'],
        'url': os.environ['RISCV_GCC_URL'],
        'sha256_this_download': os.environ.get('RISCV_GCC_SHA256') or None,
    },
    'setup_seconds': int(os.environ['SETUP_END']) - int(os.environ['SETUP_START']),
}
with open(os.environ['STATUS_FILE'], 'w') as f:
    json.dump(status, f, indent=2, sort_keys=True)
    f.write('\n')
"
VENV_DIR="$VENV_DIR" VENV_PY="$VENV_PY" \
LITEX_COMMIT="$LITEX_COMMIT" LITEX_BOARDS_COMMIT="$LITEX_BOARDS_COMMIT" MIGEN_COMMIT="$MIGEN_COMMIT" \
LITEDRAM_COMMIT="$LITEDRAM_COMMIT" LITEETH_COMMIT="$LITEETH_COMMIT" LITEPCIE_COMMIT="$LITEPCIE_COMMIT" \
LITESPI_COMMIT="$LITESPI_COMMIT" LITESCOPE_COMMIT="$LITESCOPE_COMMIT" VEXRISCV_COMMIT="$VEXRISCV_COMMIT" \
COMPILER_RT_COMMIT="$COMPILER_RT_COMMIT" PICOLIBC_COMMIT="$PICOLIBC_COMMIT" \
INSTALLED_VERSIONS="$INSTALLED_VERSIONS" \
RISCV_GCC_BIN="$RISCV_GCC_BIN" RISCV_GCC_VERSION="$RISCV_GCC_VERSION" RISCV_GCC_URL="$RISCV_GCC_URL" \
RISCV_GCC_SHA256="$RISCV_GCC_SHA256" \
SETUP_START="$SETUP_START" SETUP_END="$SETUP_END" STATUS_FILE="$STATUS_FILE" \
"$VENV_PY" -c "$STATUS_PY"

touch "$MARKER"
log "LiteX venv setup complete in $((SETUP_END - SETUP_START))s"
cat "$STATUS_FILE"

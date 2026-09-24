#!/usr/bin/env bash
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
# T1.6: populates rust/fasm/fuzz/corpus/<target>/ (gitignored, see
# fuzz/.gitignore) with every `tests/corpus/**/*.fasm`,
# `tests/corpus/**/*.fasm.xz` and `examples/*.fasm` file in the repository
# (`.xz` ones decompressed, since a fuzz target consumes raw FASM bytes,
# not xz-compressed ones), so `cargo fuzz run` starts from real, parseable
# FASM source instead of empty/random inputs. What's committed to git is
# this script (i.e. the *list* of source files it draws from, expressed as
# the globs above) and the source corpus itself, never a copy of the
# corpus under fuzz/ nor anything libFuzzer discovers while fuzzing
# (fuzz/corpus/, fuzz/artifacts/ — see fuzz/.gitignore).
#
# Usage: rust/fasm/fuzz/seed-corpus.sh [target...]
# With no arguments, seeds every target under fuzz/fuzz_targets/.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"
FUZZ_DIR="$PWD"
REPO_ROOT="$(cd ../../.. && pwd)"

targets=("$@")
if [ ${#targets[@]} -eq 0 ]; then
    for f in "$FUZZ_DIR"/fuzz_targets/*.rs; do
        targets+=("$(basename "${f%.rs}")")
    done
fi

mapfile -t seeds < <(
    find "$REPO_ROOT/tests/corpus" "$REPO_ROOT/examples" -type f \
        \( -name '*.fasm' -o -name '*.fasm.xz' \) | sort
)

if [ ${#seeds[@]} -eq 0 ]; then
    echo "seed-corpus.sh: found no tests/corpus/**/*.fasm(.xz) or examples/*.fasm files" >&2
    exit 1
fi

# Decompress every `.xz` seed once (not once per target) into a scratch
# directory, content-hash-named like the destination files below so this
# stays cheap and idempotent; `content_of[seed]` then points every seed
# (plain or decompressed) at the actual FASM bytes to copy from.
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
declare -A content_of
for seed in "${seeds[@]}"; do
    if [[ "$seed" == *.xz ]]; then
        hash="$(xz -dc "$seed" | sha256sum | cut -c1-16)"
        decompressed="$scratch/$hash.fasm"
        [ -e "$decompressed" ] || xz -dc "$seed" > "$decompressed"
        content_of["$seed"]="$decompressed"
    else
        content_of["$seed"]="$seed"
    fi
done

echo "seed-corpus.sh: seeding ${#targets[@]} target(s) from ${#seeds[@]} corpus file(s)" >&2

for target in "${targets[@]}"; do
    dir="$FUZZ_DIR/corpus/$target"
    mkdir -p "$dir"
    n=0
    for seed in "${seeds[@]}"; do
        src="${content_of[$seed]}"
        # A content hash keeps this idempotent (same source file always
        # lands at the same destination name) without recreating every
        # source path's directory structure under corpus/, and without
        # colliding when two source directories both have e.g. `lut.fasm`.
        hash="$(sha256sum "$src" | cut -c1-16)"
        if [ ! -e "$dir/$hash.fasm" ]; then
            cp "$src" "$dir/$hash.fasm"
        fi
        n=$((n + 1))
    done
    echo "  $target: $n seed file(s) in $dir" >&2
done

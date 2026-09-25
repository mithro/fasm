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

# The top directory where environment will be created.
TOP_DIR := $(realpath $(dir $(lastword $(MAKEFILE_LIST))))

# Python package (fasm/, pyproject.toml, maturin build of rust/fasm-python;
# see docs/rewrite/DESIGN-python.md). These targets run in whatever Python
# venv is active (there is no bundled conda environment any more): create
# one with e.g. `python3 -m venv .venv && . .venv/bin/activate` and
# `pip install .[dev]` first.
# ------------------------------------------------------------------------

# Build sdist + wheel with maturin.
build:
	python3 -m maturin build --release

.PHONY: build

# Editable install of the Python package (builds the Rust extension in
# place with maturin).
install:
	python3 -m maturin develop --release

.PHONY: install

# Run the Python tests (see tests/README.md for the two ways to run them;
# this assumes an editable `maturin develop` install).
test:
	python3 -m pytest -s tests/test_simple.py tests/test_rust_parser.py tests/test_xilinx_python.py

.PHONY: test

# Find files to apply tools to while ignoring files.
define with_files
  git ls-files | grep -ve '^\.|^env' | grep -e $(1) | xargs -r -P $$(nproc) $(2)
endef

# Lint the python files
lint:
	$(call with_py_files, flake8)

.PHONY: lint

# Format the python files
define with_py_files
  $(call with_files, '.py$$', $(1))
endef

PYTHON_FORMAT ?= yapf
format-py:
	$(call with_py_files, yapf -p -i)

.PHONY: format-py

# Format all the files!
format: format-py
	true

# Check files have license headers.
check-license:
	@./.github/check_license.sh

.PHONY: check-license

# Check python scripts have the correct headers.
check-python-scripts:
	@./.github/check_python_scripts.sh

.PHONY: check-python-scripts

# Rust workspace (rust/, see docs/rewrite/PLAN.md).
# ------------------------------------------------------------------------

# Build every crate in the workspace.
rust-build:
	cargo build --workspace

.PHONY: rust-build

# Run every crate's tests.
rust-test:
	cargo test --workspace

.PHONY: rust-test

# Check formatting and lint with clippy, denying warnings.
rust-lint:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings

.PHONY: rust-lint

# Auto-format the Rust workspace.
rust-fmt:
	cargo fmt --all

.PHONY: rust-fmt

# Build the rustdoc, denying rustdoc warnings (broken doc links etc.).
rust-doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace

.PHONY: rust-doc

# Everything CI runs for the Rust workspace (see .github/workflows/rust.yml):
# lint, doc, test.
rust-check: rust-lint rust-doc rust-test

.PHONY: rust-check

# Differential test of the Rust `fasm` binary against the original Python
# tool (tests/cli/test_cli_compat.py): identical stdout, stderr and exit
# code over the FASM corpus and argparse edge cases; and of the Rust
# `fasm2frames`, `xcfasm`, `xc7frames2bit` and `bitread` command lines
# against f4pga-xc-fasm's and prjxray's, and of `uray-fasm2frames`,
# `xcframes2bit` and `uray-bitread` against prjuray's
# (tests/cli/test_*_compat.py, need tests/oracle/setup-xilinx.sh).
# Needs the oracle venv (tests/oracle/setup.sh); to use another checkout's
# oracle (e.g. from a worktree) set ORACLE_DIR to its tests/oracle
# directory.
ORACLE_DIR ?= $(TOP_DIR)/tests/oracle

cli-difftest:
	cargo build --release -p fasm-cli
	FASM_ORACLE=$(ORACLE_DIR)/fasm-oracle FASM2FRAMES_ORACLE=$(ORACLE_DIR)/fasm2frames-oracle \
	XC7FRAMES2BIT_ORACLE=$(ORACLE_DIR)/xc7frames2bit-oracle BITREAD_ORACLE=$(ORACLE_DIR)/bitread-oracle \
	XCFASM_ORACLE=$(ORACLE_DIR)/xcfasm-oracle URAY_ORACLE_DIR=$(ORACLE_DIR) \
		$(ORACLE_DIR)/venv/bin/pytest tests/cli

.PHONY: cli-difftest

# Differential test: Rust `fasm` crate vs. the original Python `fasm`
# package (the oracle), over tests/corpus/ (T1.5, tools/difftest.py).
# Requires tests/oracle/setup.sh to have been run once; DIFFTEST_ARGS can
# add e.g. `--jobs N` or `--filter GLOB`.
difftest: DIFFTEST_ARGS ?=
difftest:
	cargo build --release --example dump -p fasm
	python3 tools/difftest.py $(DIFFTEST_ARGS)

.PHONY: difftest

# Differential test of the Rust `fasm2frames` against the reference
# f4pga-xc-fasm/prjxray tool (tools/difftest-xilinx.py, T5.4/T5.5): identical
# .frm output, stdout, exit code and (normalised) stderr for every FASM file
# of tests/corpus/xilinx/ (with the databases fetched by tools/fetch-db.sh,
# under $FASM_DB_CACHE or tests/oracle/build/db) and tests/corpus/f4pga-xc-fasm/
# (miniature database); and of the Rust `xc7frames2bit`, `bitread` and
# `xcfasm` against prjxray's and f4pga-xc-fasm's (T5.6/T5.7: identical .bit,
# bitread output and xcfasm results). Needs tests/oracle/setup-xilinx.sh; set ORACLE_DIR
# to use another checkout's oracle. DIFFTEST_XILINX_ARGS can add e.g.
# `--jobs N`, `--filter GLOB` or `-v`. The Rust tools use the binary
# database cache (docs/rewrite/DESIGN-xilinx-db.md §8.8) in a temporary
# directory of the run (this target and cli-difftest's tests/cli) unless
# FASM_XDB_CACHE is set (FASM_XDB_CACHE=0: without the cache).
xilinx-difftest: DIFFTEST_XILINX_ARGS ?=
xilinx-difftest:
	cargo build --release -p fasm-cli
	python3 tools/difftest-xilinx.py --oracle $(ORACLE_DIR)/fasm2frames-oracle \
		--frames2bit-oracle $(ORACLE_DIR)/xc7frames2bit-oracle \
		--bitread-oracle $(ORACLE_DIR)/bitread-oracle \
		--xcfasm-oracle $(ORACLE_DIR)/xcfasm-oracle $(DIFFTEST_XILINX_ARGS)

.PHONY: xilinx-difftest

# The same comparison over every part of the four prjxray-db families
# (T5.9, docs/rewrite/DESIGN-xilinx-db.md 8.9): a synthetic corpus per part
# from tools/gen-xilinx-corpus.py (every segbits feature and pseudo PIP of
# every tile type, plus an error corpus), missing families fetched with
# tools/fetch-db.sh. Generated files, the cached reference results and the
# run directories go to $(XILINX_DIFFTEST_WORK); a rerun only runs the Rust
# tools. XILINX_DIFFTEST_ALL_ARGS can add e.g. `--parts 'xc7a35t*'`,
# `--tiles first` or `--no-result-cache`.
XILINX_DIFFTEST_WORK ?= $(TOP_DIR)/tests/oracle/build/difftest-xilinx
XILINX_DIFFTEST_FAMILIES ?= artix7,kintex7,spartan7,zynq7
XILINX_DIFFTEST_JOBS ?= 4
xilinx-difftest-all: XILINX_DIFFTEST_ALL_ARGS ?=
xilinx-difftest-all:
	cargo build --release -p fasm-cli
	python3 tools/difftest-xilinx.py --oracle $(ORACLE_DIR)/fasm2frames-oracle \
		--frames2bit-oracle $(ORACLE_DIR)/xc7frames2bit-oracle \
		--bitread-oracle $(ORACLE_DIR)/bitread-oracle \
		--xcfasm-oracle $(ORACLE_DIR)/xcfasm-oracle \
		--families $(XILINX_DIFFTEST_FAMILIES) --all-parts \
		--jobs $(XILINX_DIFFTEST_JOBS) --work-dir $(XILINX_DIFFTEST_WORK) \
		$(XILINX_DIFFTEST_ALL_ARGS)

.PHONY: xilinx-difftest-all

# Quick version: one part per family (4 parts, a different fabric each,
# about 2-3 minutes with 4 jobs instead of about 70).
xilinx-difftest-quick:
	$(MAKE) xilinx-difftest-all XILINX_DIFFTEST_ALL_ARGS="--parts-sample 1 $(XILINX_DIFFTEST_ALL_ARGS)"

.PHONY: xilinx-difftest-quick
# The prjuray mode of tools/difftest-xilinx.py (UltraScale+, T6.2/T6.3,
# docs/rewrite/DESIGN-xilinx-db.md 8.12): every part of every prjuray-db
# family (fetched with tools/fetch-db.sh when missing; upstream has only
# zynqusp with two parts), on the every-feature corpus of
# tools/gen-xilinx-corpus.py, random designs and error files, and the
# ToolsTestData bitstreams: uray-fasm2frames, fasm2frames, xcframes2bit and
# uray-bitread against prjuray's. Generated files, the cached reference
# results and the run directories go to $(URAY_DIFFTEST_WORK); a rerun
# only runs the Rust tools. URAY_DIFFTEST_ALL_ARGS can add e.g.
# `--parts 'xczu3eg-sfvc*'`, `--tiles first` or `--no-result-cache`.
URAY_DIFFTEST_WORK ?= $(TOP_DIR)/tests/oracle/build/difftest-uray
URAY_DIFFTEST_JOBS ?= 2
uray-difftest-all: URAY_DIFFTEST_ALL_ARGS ?=
uray-difftest-all:
	cargo build --release -p fasm-cli
	python3 tools/difftest-xilinx.py --prjuray \
		--uray-oracle-dir $(ORACLE_DIR) --jobs $(URAY_DIFFTEST_JOBS) \
		--work-dir $(URAY_DIFFTEST_WORK) $(URAY_DIFFTEST_ALL_ARGS)

.PHONY: uray-difftest-all

# The same with the default work directory and DIFFTEST_XILINX_ARGS.
uray-difftest: DIFFTEST_XILINX_ARGS ?=
uray-difftest:
	cargo build --release -p fasm-cli
	python3 tools/difftest-xilinx.py --prjuray \
		--uray-oracle-dir $(ORACLE_DIR) $(DIFFTEST_XILINX_ARGS)

.PHONY: uray-difftest

# Fuzzing (T1.6, rust/fasm/fuzz/; see its README.md). Needs `cargo-fuzz`
# (`cargo install cargo-fuzz`) and a nightly toolchain (`rustup toolchain
# install nightly`); rust/fasm/fuzz is excluded from the main workspace
# (see Cargo.toml's `[workspace] exclude`) so plain `rust-build`/`rust-test`
# never need either. FUZZ_SECONDS is the libFuzzer `-max_total_time` per
# target (default 600s = 10 minutes); FUZZ_JOBS is libFuzzer `-jobs`
# (default 2, per the task brief's 4-core guidance).
FUZZ_TARGETS := parse roundtrip merge
FUZZ_SECONDS ?= 600
FUZZ_JOBS ?= 2

# (Re)populate fuzz/corpus/<target>/ from tests/corpus/**/*.fasm and
# examples/*.fasm; see rust/fasm/fuzz/seed-corpus.sh.
fuzz-corpus:
	rust/fasm/fuzz/seed-corpus.sh

.PHONY: fuzz-corpus

# Run every fuzz target for FUZZ_SECONDS seconds each.
fuzz: fuzz-corpus
	@for target in $(FUZZ_TARGETS); do \
		echo "==> fuzzing $$target for $(FUZZ_SECONDS)s (-jobs=$(FUZZ_JOBS))"; \
		( cd rust/fasm && cargo +nightly fuzz run $$target -- \
			-max_total_time=$(FUZZ_SECONDS) -jobs=$(FUZZ_JOBS) ) || exit 1; \
	done

.PHONY: fuzz

# C API (rust/fasm-capi, include/fasm/fasm.h; see docs/rewrite/DESIGN-capi.md).
# ------------------------------------------------------------------------

# cbindgen (`cargo install cbindgen --locked`): from PATH, else from the
# default cargo bin directory.
CBINDGEN ?= $(shell command -v cbindgen 2>/dev/null || echo $${CARGO_HOME:-$$HOME/.cargo}/bin/cbindgen)
# The Cargo target directory (CARGO_TARGET_DIR when set, as cargo does).
CAPI_CARGO_TARGET_DIR := $(abspath $(or $(CARGO_TARGET_DIR),$(TOP_DIR)/target))
CAPI_BUILD_DIR ?= $(CAPI_CARGO_TARGET_DIR)/capi-tests

# Regenerate the checked in C header from the fasm-capi sources.
capi-header:
	cd $(TOP_DIR) && $(CBINDGEN) --config rust/fasm-capi/cbindgen.toml --crate fasm-capi --output include/fasm/fasm.h

.PHONY: capi-header

# Fail if include/fasm/fasm.h is not what cbindgen generates (also run,
# without requiring cbindgen, by `cargo test --workspace`).
capi-header-check:
	CBINDGEN=$(CBINDGEN) FASM_REQUIRE_CBINDGEN=1 cargo test -p fasm-capi --test header

.PHONY: capi-header-check

# Build libfasm_capi, build the C and C++ test programs against the shared
# and the static library with CMake, and run them (and under valgrind, when
# installed, failing on memory errors and leaks). The Xilinx tests compare
# their output with the command line tools of fasm-cli, built here too.
capi-test:
	cargo build -p fasm-capi -p fasm-cli
	cmake -S $(TOP_DIR)/rust/fasm-capi/tests/c -B $(CAPI_BUILD_DIR) -DFASM_CARGO_PROFILE=debug -DFASM_CARGO_TARGET_DIR=$(CAPI_CARGO_TARGET_DIR)
	cmake --build $(CAPI_BUILD_DIR)
	cd $(CAPI_BUILD_DIR) && ctest --output-on-failure

.PHONY: capi-test

# Installs the C header (fasm.h), the header-only C++ wrapper (fasm.hpp),
# the shared and static libraries (release profile) and a pkg-config file
# into PREFIX/{include/fasm,lib} (default PREFIX is /usr/local; DESTDIR is
# honoured for staged installs). `pkg-config --cflags --libs fasm` then
# gives the flags to build against the installed library (see
# rust/fasm-capi/fasm.pc.in and rust/fasm-capi/examples/cpp).
PREFIX ?= /usr/local
FASM_PC_VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' $(TOP_DIR)/Cargo.toml | head -1)
# The system libraries the Rust staticlib needs (see FASM_NATIVE_LIBS in
# rust/fasm-capi/tests/c/CMakeLists.txt, derived from `cargo rustc -p
# fasm-capi -- --print native-static-libs`); Linux only, like that file.
FASM_PC_LIBS_PRIVATE ?= -lpthread -ldl -lm

capi-install:
	cargo build --release -p fasm-capi
	install -d $(DESTDIR)$(PREFIX)/include/fasm $(DESTDIR)$(PREFIX)/lib/pkgconfig
	install -m 644 $(TOP_DIR)/include/fasm/fasm.h $(DESTDIR)$(PREFIX)/include/fasm/fasm.h
	install -m 644 $(TOP_DIR)/include/fasm/fasm.hpp $(DESTDIR)$(PREFIX)/include/fasm/fasm.hpp
	install -m 755 $(CAPI_CARGO_TARGET_DIR)/release/libfasm_capi.so $(DESTDIR)$(PREFIX)/lib/libfasm_capi.so
	install -m 644 $(CAPI_CARGO_TARGET_DIR)/release/libfasm_capi.a $(DESTDIR)$(PREFIX)/lib/libfasm_capi.a
	sed -e 's|@PREFIX@|$(PREFIX)|g' -e 's|@VERSION@|$(FASM_PC_VERSION)|g' -e 's|@LIBS_PRIVATE@|$(FASM_PC_LIBS_PRIVATE)|g' $(TOP_DIR)/rust/fasm-capi/fasm.pc.in > $(DESTDIR)$(PREFIX)/lib/pkgconfig/fasm.pc

.PHONY: capi-install

# PyPI publishing is done by .github/workflows/python.yml's `publish` job
# (trusted publishing on `v*` tags); there is no local upload target.

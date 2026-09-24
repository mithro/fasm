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

# A pip `requirements.txt` file.
# https://pip.pypa.io/en/stable/reference/pip_install/#requirements-file-format
REQUIREMENTS_FILE := requirements.txt

# A conda `environment.yml` file.
# https://docs.conda.io/projects/conda/en/latest/user-guide/tasks/manage-environments.html
ENVIRONMENT_FILE := environment.yml

# Rule to checkout the git submodule if it wasn't cloned.
$(TOP_DIR)/third_party/make-env/conda.mk: $(TOP_DIR)/.gitmodules
	cd $(TOP_DIR); git submodule update --init third_party/make-env
	touch $(TOP_DIR)/third_party/make-env/conda.mk

-include $(TOP_DIR)/third_party/make-env/conda.mk

# Update the version file
# ------------------------------------------------------------------------
fasm/version.py: update_version.py | $(CONDA_ENV_PYTHON)
	$(IN_CONDA_ENV) python ./update_version.py

setup.py: fasm/version.py
	touch setup.py --reference fasm/version.py

# Build/install into the conda environment.
# ------------------------------------------------------------------------
build-clean:
	rm -rf dist fasm.egg-info

.PHONY: build-clean

build: setup.py | $(CONDA_ENV_PYTHON)
	make build-clean
	$(IN_CONDA_ENV) python setup.py sdist bdist_wheel

.PHONY: build

# Install into environment
install: setup.py | $(CONDA_ENV_PYTHON)
	$(IN_CONDA_ENV) python setup.py develop

.PHONY: install


# Build/install locally rather than inside the environment.
# ------------------------------------------------------------------------
local-build: setup.py
	python setup.py build

.PHONY: local-build

local-build-shared: setup.py
	python setup.py build --antlr-runtime=shared

.PHONY: local-build-shared

local-install: setup.py
	python setup.py install

.PHONY: local-install


# Test, lint, auto-format.
# ------------------------------------------------------------------------

# Run the tests
test: fasm/version.py | $(CONDA_ENV_PYTHON)
	$(IN_CONDA_ENV) py.test -s tests

.PHONY: test

# Find files to apply tools to while ignoring files.
define with_files
  $(IN_CONDA_ENV) git ls-files | grep -ve '^third_party\|^\.|^env' | grep -e $(1) | xargs -r -P $$(nproc) $(2)
endef

# Lint the python files
lint: | $(CONDA_ENV_PYTHON)
	$(call with_py_files, flake8)

.PHONY: lint

# Format the python files
define with_py_files
  $(call with_files, '.py$$', $(1))
endef

PYTHON_FORMAT ?= yapf
format-py: | $(CONDA_ENV_PYTHON)
	$(call with_py_files, yapf -p -i)

.PHONY: format-py

# Format the C++ files
define with_cpp_files
  $(call with_files, '\.cpp$$\|\.h$$', $(1))
endef

format-cpp:
	$(call with_cpp_files, clang-format -style=file -i)

.PHONY: format-cpp

# Format all the files!
format: format-py format-cpp
	true

# Check - ???
check: setup.py | $(CONDA_ENV_PYTHON)
	$(IN_CONDA_ENV) python setup.py check -m -s

.PHONY: check

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
# `fasm2frames` command line against f4pga-xc-fasm's
# (tests/cli/test_fasm2frames_compat.py, needs tests/oracle/setup-xilinx.sh). Needs the oracle venv
# (tests/oracle/setup.sh); to use another checkout's oracle (e.g. from a git
# worktree) set ORACLE_DIR to its tests/oracle directory.
ORACLE_DIR ?= $(TOP_DIR)/tests/oracle

cli-difftest:
	cargo build --release -p fasm-cli
	FASM_ORACLE=$(ORACLE_DIR)/fasm-oracle FASM2FRAMES_ORACLE=$(ORACLE_DIR)/fasm2frames-oracle \
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
# (miniature database). Needs tests/oracle/setup-xilinx.sh; set ORACLE_DIR
# to use another checkout's oracle. DIFFTEST_XILINX_ARGS can add e.g.
# `--jobs N`, `--filter GLOB` or `-v`.
xilinx-difftest: DIFFTEST_XILINX_ARGS ?=
xilinx-difftest:
	cargo build --release -p fasm-cli
	python3 tools/difftest-xilinx.py --oracle $(ORACLE_DIR)/fasm2frames-oracle $(DIFFTEST_XILINX_ARGS)

.PHONY: xilinx-difftest

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

# Build libfasm_capi, build the C test program against the shared and the
# static library with CMake, and run both (and under valgrind, when
# installed, failing on memory errors and leaks).
capi-test:
	cargo build -p fasm-capi
	cmake -S $(TOP_DIR)/rust/fasm-capi/tests/c -B $(CAPI_BUILD_DIR) -DFASM_CARGO_PROFILE=debug -DFASM_CARGO_TARGET_DIR=$(CAPI_CARGO_TARGET_DIR)
	cmake --build $(CAPI_BUILD_DIR)
	cd $(CAPI_BUILD_DIR) && ctest --output-on-failure

.PHONY: capi-test


# Upload to PyPI servers
# ------------------------------------------------------------------------

# PYPI_TEST = --repository-url https://test.pypi.org/legacy/
PYPI_TEST = --repository testpypi

# Check before uploading
upload-check: build | $(CONDA_ENV_PYTHON)
	$(IN_CONDA_ENV) twine check dist/*

.PHONY: upload-check

# Upload to test.pypi.org
upload-test: check | $(CONDA_ENV_PYTHON)
	$(IN_CONDA_ENV) twine upload ${PYPI_TEST}  dist/*.tar.gz
	$(IN_CONDA_ENV) twine upload ${PYPI_TEST}  dist/*.whl

.PHONY: upload-test

# Upload to the real pypi.org
upload: check | $(CONDA_ENV_PYTHON)
	$(IN_CONDA_ENV) twine upload ${PYPI_TEST}  dist/*.tar.gz
	$(IN_CONDA_ENV) twine upload ${PYPI_TEST}  dist/*.whl

.PHONY: upload

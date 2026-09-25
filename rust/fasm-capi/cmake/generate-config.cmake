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
# Standalone script (no add_subdirectory/project() needed): writes
# fasmConfig.cmake and fasmConfigVersion.cmake for an install prefix.
# Not a CMake project in its own right -- the actual library build is
# `cargo build --release -p fasm-capi` (see ../../../Makefile's
# `capi-install` target, which runs this with `cmake -P` after that
# build). Kept separate from tests/c/CMakeLists.txt (which builds and
# runs the C/C++ tests against a debug build in the Cargo target
# directory, not an installed prefix).
#
# Required -D variables:
#   FASM_PREFIX       install prefix (Makefile PREFIX, without DESTDIR)
#   FASM_VERSION      package version (Makefile FASM_PC_VERSION)
#   FASM_SOURCE_DIR   this file's directory (holds fasmConfig.cmake.in)
#   FASM_OUT_DIR      where to write the two generated files (typically
#                     DESTDIR+PREFIX/lib/cmake/fasm)

cmake_minimum_required(VERSION 3.13)
include(CMakePackageConfigHelpers)

foreach(var FASM_PREFIX FASM_VERSION FASM_SOURCE_DIR FASM_OUT_DIR)
    if(NOT DEFINED ${var})
        message(FATAL_ERROR "generate-config.cmake: -D${var}=... is required")
    endif()
endforeach()

# CMake's version comparisons (and write_basic_package_version_file's
# generated compatibility checks) are numeric-component based; strip a
# semver pre-release suffix (e.g. "0.1.0-dev" -> "0.1.0") so `cmake
# --version` style VERSION_LESS/VERSION_EQUAL checks against this file
# behave as intended. The full string (with any suffix) still appears
# verbatim in the pkg-config file (../fasm.pc.in) and crate metadata.
string(REGEX MATCH "^[0-9]+\\.[0-9]+\\.[0-9]+" FASM_NUMERIC_VERSION "${FASM_VERSION}")
if(NOT FASM_NUMERIC_VERSION)
    message(FATAL_ERROR "FASM_VERSION='${FASM_VERSION}' does not start with X.Y.Z")
endif()

set(FASM_INCLUDE_DIR "${FASM_PREFIX}/include")
set(FASM_LIB_DIR "${FASM_PREFIX}/lib")

# SameMinorVersion, not SameMajorVersion: while the major version is 0
# (semver's "anything can break" range, which every 0.x version here is
# until a 1.0), SameMajorVersion would accept a `find_package(fasm
# 0.1.0)` request from an installed 0.0.x, which is wrong -- 0.x treats
# the minor version the way >=1 treats the major. Revisit when this
# reaches 1.0.0 (switch to SameMajorVersion then).
write_basic_package_version_file(
    "${FASM_OUT_DIR}/fasmConfigVersion.cmake"
    VERSION "${FASM_NUMERIC_VERSION}"
    COMPATIBILITY SameMinorVersion
    ARCH_INDEPENDENT)

# INSTALL_DESTINATION is where fasmConfig.cmake will live *relative to
# INSTALL_PREFIX at run time* (it is pure path arithmetic: configure_
# package_config_file does not need either directory to exist), so it
# must be the final, unstaged path ("${FASM_PREFIX}/lib/cmake/fasm"), not
# FASM_OUT_DIR -- which, for a staged/DESTDIR install (`make capi-install
# DESTDIR=... PREFIX=...`), has an extra DESTDIR prefix that does not
# mirror FASM_PREFIX's own depth and would throw off the relative offset
# @PACKAGE_INIT@ later uses to find FASM_PREFIX from the installed file's
# own location.
configure_package_config_file(
    "${FASM_SOURCE_DIR}/fasmConfig.cmake.in"
    "${FASM_OUT_DIR}/fasmConfig.cmake"
    INSTALL_DESTINATION "${FASM_PREFIX}/lib/cmake/fasm"
    INSTALL_PREFIX "${FASM_PREFIX}"
    PATH_VARS FASM_INCLUDE_DIR FASM_LIB_DIR)

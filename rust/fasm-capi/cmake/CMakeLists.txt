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
# CMake project that builds libfasm_capi by driving `cargo` and installs
# it together with the headers, the pkg-config file and the CMake package
# config (fasmConfig.cmake / fasmConfigVersion.cmake, so downstream
# projects can `find_package(fasm CONFIG)` -- see fasmConfig.cmake.in).
#
# This is an alternative, all-CMake entry point to the same install that
# `make capi-install PREFIX=...` at the repository root produces (see the
# Makefile's `capi-install` target); both end up with the same files
# under PREFIX. Prefer `make capi-install` when Make is available (it is
# what this repository's own tests and docs/CAPI.md use, and it needs no
# extra CMake ExternalProject/Corrosion machinery to shell out to cargo);
# this file is for projects that already drive their whole build with
# CMake and would rather configure+build+install it standalone, or pull
# it in with `ExternalProject_Add`/`FetchContent` (each runs this as its
# own, separate `cmake --build`/`--install`, which is the right fit for a
# `project()` of its own like this one; plain `add_subdirectory` is not
# recommended here -- it would fold this file's own `project(fasm
# LANGUAGES NONE)` into the parent build's directory scope instead of
# keeping it a separate configure/build/install, and gets no benefit
# from doing so since there is no CMake target here a parent build could
# usefully depend on directly, only the install step's files):
#
#   cmake -S rust/fasm-capi/cmake -B build -DCMAKE_INSTALL_PREFIX=/some/prefix \
#       -DCMAKE_BUILD_TYPE=Release
#   cmake --build build
#   cmake --install build
#
# Cargo (a Rust >= 1.88 toolchain, see the root Cargo.toml's
# workspace.package.rust-version) must be on PATH; this does not fetch or
# install one. It always builds fasm-capi in `--release` (there is no
# CMAKE_BUILD_TYPE-driven cargo profile switch: a C API consumer wants an
# optimized library regardless of how it built the CMake side of things).

# 3.18, not this repository's usual minimum of 3.13 (rust/fasm-capi/
# tests/c/CMakeLists.txt, generate-config.cmake): find_program()'s
# REQUIRED keyword below needs it.
cmake_minimum_required(VERSION 3.18)
project(fasm LANGUAGES NONE)

include(GNUInstallDirs)
include(CMakePackageConfigHelpers)

get_filename_component(FASM_REPO_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../../.." ABSOLUTE)

find_program(FASM_CARGO cargo REQUIRED)

set(FASM_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo-target")
set(FASM_RELEASE_DIR "${FASM_TARGET_DIR}/release")
set(FASM_SHARED_LIB "${FASM_RELEASE_DIR}/${CMAKE_SHARED_LIBRARY_PREFIX}fasm_capi${CMAKE_SHARED_LIBRARY_SUFFIX}")
set(FASM_STATIC_LIB "${FASM_RELEASE_DIR}/${CMAKE_STATIC_LIBRARY_PREFIX}fasm_capi${CMAKE_STATIC_LIBRARY_SUFFIX}")

# Re-run cargo (and this configure/build step) whenever the crate sources
# change: cargo itself is the thing that decides whether a rebuild is
# actually needed (incremental, near-instant when nothing changed), so
# this glob only needs to be "wide enough", not exact.
file(GLOB_RECURSE FASM_RUST_SOURCES CONFIGURE_DEPENDS
    "${FASM_REPO_ROOT}/rust/fasm/src/*.rs"
    "${FASM_REPO_ROOT}/rust/fasm-xilinx/src/*.rs"
    "${FASM_REPO_ROOT}/rust/fasm-capi/src/*.rs")
list(APPEND FASM_RUST_SOURCES
    "${FASM_REPO_ROOT}/Cargo.toml"
    "${FASM_REPO_ROOT}/rust/fasm/Cargo.toml"
    "${FASM_REPO_ROOT}/rust/fasm-xilinx/Cargo.toml"
    "${FASM_REPO_ROOT}/rust/fasm-xilinx/build.rs"
    "${FASM_REPO_ROOT}/rust/fasm-capi/Cargo.toml")

add_custom_command(
    OUTPUT "${FASM_SHARED_LIB}" "${FASM_STATIC_LIB}"
    COMMAND "${FASM_CARGO}" build --release -p fasm-capi
            --manifest-path "${FASM_REPO_ROOT}/Cargo.toml"
            --target-dir "${FASM_TARGET_DIR}"
    DEPENDS ${FASM_RUST_SOURCES}
    WORKING_DIRECTORY "${FASM_REPO_ROOT}"
    COMMENT "cargo build --release -p fasm-capi"
    VERBATIM)
add_custom_target(fasm_capi_cargo ALL DEPENDS "${FASM_SHARED_LIB}" "${FASM_STATIC_LIB}")

# The version installed here always matches this checkout (there is no
# separate versioning for the CMake wrapper); read it from the same place
# the Makefile's capi-install target does.
file(STRINGS "${FASM_REPO_ROOT}/Cargo.toml" FASM_VERSION_LINE REGEX "^version = \"")
string(REGEX REPLACE "^version = \"([^\"]*)\"" "\\1" FASM_VERSION "${FASM_VERSION_LINE}")

install(FILES "${FASM_SHARED_LIB}" DESTINATION "${CMAKE_INSTALL_LIBDIR}")
install(FILES "${FASM_STATIC_LIB}" DESTINATION "${CMAKE_INSTALL_LIBDIR}")
install(FILES
    "${FASM_REPO_ROOT}/include/fasm/fasm.h"
    "${FASM_REPO_ROOT}/include/fasm/fasm.hpp"
    DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}/fasm")

# pkg-config file (same template the Makefile's capi-install substitutes;
# see ../fasm.pc.in for Libs.private/why it is what it is).
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(FASM_PC_LIBS_PRIVATE "-lpthread -ldl -lm")
else()
    set(FASM_PC_LIBS_PRIVATE "")
endif()
set(FASM_PC_FILE "${CMAKE_CURRENT_BINARY_DIR}/fasm.pc")
configure_file("${CMAKE_CURRENT_SOURCE_DIR}/../fasm.pc.in" "${FASM_PC_FILE}.tmp" COPYONLY)
file(READ "${FASM_PC_FILE}.tmp" FASM_PC_CONTENT)
string(REPLACE "@PREFIX@" "${CMAKE_INSTALL_PREFIX}" FASM_PC_CONTENT "${FASM_PC_CONTENT}")
string(REPLACE "@VERSION@" "${FASM_VERSION}" FASM_PC_CONTENT "${FASM_PC_CONTENT}")
string(REPLACE "@LIBS_PRIVATE@" "${FASM_PC_LIBS_PRIVATE}" FASM_PC_CONTENT "${FASM_PC_CONTENT}")
file(GENERATE OUTPUT "${FASM_PC_FILE}" CONTENT "${FASM_PC_CONTENT}")
install(FILES "${FASM_PC_FILE}" DESTINATION "${CMAKE_INSTALL_LIBDIR}/pkgconfig")

# fasmConfig.cmake / fasmConfigVersion.cmake (see fasmConfig.cmake.in).
string(REGEX MATCH "^[0-9]+\\.[0-9]+\\.[0-9]+" FASM_NUMERIC_VERSION "${FASM_VERSION}")
write_basic_package_version_file(
    "${CMAKE_CURRENT_BINARY_DIR}/fasmConfigVersion.cmake"
    VERSION "${FASM_NUMERIC_VERSION}"
    COMPATIBILITY SameMinorVersion
    ARCH_INDEPENDENT)
set(FASM_INCLUDE_DIR "${CMAKE_INSTALL_FULL_INCLUDEDIR}")
set(FASM_LIB_DIR "${CMAKE_INSTALL_FULL_LIBDIR}")
configure_package_config_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/fasmConfig.cmake.in"
    "${CMAKE_CURRENT_BINARY_DIR}/fasmConfig.cmake"
    INSTALL_DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/fasm"
    PATH_VARS FASM_INCLUDE_DIR FASM_LIB_DIR)
install(FILES
    "${CMAKE_CURRENT_BINARY_DIR}/fasmConfig.cmake"
    "${CMAKE_CURRENT_BINARY_DIR}/fasmConfigVersion.cmake"
    DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/fasm")

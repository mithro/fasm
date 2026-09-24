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

# C test program of the fasm C API (rust/fasm-capi, include/fasm/fasm.h).
#
# Builds tests/c/test_capi.c twice: linked against the shared library
# (test_capi_shared) and against the static library (test_capi_static),
# both taken from the Cargo target directory, which must have been built
# first (`cargo build -p fasm-capi`, or `--release` with
# -DFASM_CARGO_PROFILE=release). `make capi-test` at the repository root
# does all of it and runs the tests with ctest, under valgrind too when it
# is installed.

cmake_minimum_required(VERSION 3.13)
project(fasm_capi_tests C)

get_filename_component(FASM_REPO_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../../../.." ABSOLUTE)

set(FASM_CARGO_PROFILE "debug" CACHE STRING "Cargo profile directory to take libfasm_capi from")
set(FASM_CARGO_TARGET_DIR "${FASM_REPO_ROOT}/target" CACHE PATH "Cargo target directory")
set(FASM_LIB_DIR "${FASM_CARGO_TARGET_DIR}/${FASM_CARGO_PROFILE}")
option(FASM_VALGRIND "Also run the tests under valgrind when it is installed" ON)

set(FASM_SHARED_LIB
    "${FASM_LIB_DIR}/${CMAKE_SHARED_LIBRARY_PREFIX}fasm_capi${CMAKE_SHARED_LIBRARY_SUFFIX}")
set(FASM_STATIC_LIB
    "${FASM_LIB_DIR}/${CMAKE_STATIC_LIBRARY_PREFIX}fasm_capi${CMAKE_STATIC_LIBRARY_SUFFIX}")
foreach(lib "${FASM_SHARED_LIB}" "${FASM_STATIC_LIB}")
    if(NOT EXISTS "${lib}")
        message(FATAL_ERROR "${lib} not found: run `cargo build -p fasm-capi` first")
    endif()
endforeach()

# The Rust cdylib has no SONAME: link it by name (-L dir -lfasm_capi) so the
# executables record `libfasm_capi.so`, found at run time through the
# build tree RPATH.
add_library(fasm_capi_shared SHARED IMPORTED)
set_target_properties(fasm_capi_shared PROPERTIES
    IMPORTED_LOCATION "${FASM_SHARED_LIB}"
    IMPORTED_NO_SONAME TRUE
    INTERFACE_INCLUDE_DIRECTORIES "${FASM_REPO_ROOT}/include")

# The Rust staticlib needs the system libraries the Rust standard library
# uses (`cargo rustc -p fasm-capi -- --print native-static-libs`).
find_package(Threads REQUIRED)
add_library(fasm_capi_static STATIC IMPORTED)
set_target_properties(fasm_capi_static PROPERTIES
    IMPORTED_LOCATION "${FASM_STATIC_LIB}"
    INTERFACE_INCLUDE_DIRECTORIES "${FASM_REPO_ROOT}/include"
    INTERFACE_COMPILE_DEFINITIONS "FASM_STATIC")
if(UNIX)
    set_property(TARGET fasm_capi_static PROPERTY INTERFACE_LINK_LIBRARIES
        Threads::Threads ${CMAKE_DL_LIBS} m)
endif()

foreach(variant shared static)
    set(exe "test_capi_${variant}")
    add_executable(${exe} test_capi.c)
    set_target_properties(${exe} PROPERTIES C_STANDARD 99 C_STANDARD_REQUIRED ON C_EXTENSIONS OFF)
    if(CMAKE_C_COMPILER_ID MATCHES "GNU|Clang")
        target_compile_options(${exe} PRIVATE -Wall -Wextra -Werror -std=c99)
    endif()
    target_link_libraries(${exe} PRIVATE fasm_capi_${variant})
endforeach()

enable_testing()
foreach(variant shared static)
    add_test(NAME capi_${variant} COMMAND test_capi_${variant} "${FASM_REPO_ROOT}")
endforeach()

# Memory checking: every error and every definitely or indirectly lost
# block fails the test. The global feature name interner (fasm::idstring)
# deliberately never frees its tables; they stay referenced from a static,
# so valgrind reports them as "still reachable", which is not an error
# here (see docs/rewrite/DESIGN-capi.md).
find_program(VALGRIND_EXECUTABLE valgrind)
if(FASM_VALGRIND AND VALGRIND_EXECUTABLE)
    foreach(variant shared static)
        add_test(NAME capi_${variant}_valgrind
            COMMAND "${VALGRIND_EXECUTABLE}"
                --error-exitcode=1
                --leak-check=full
                --show-leak-kinds=definite,indirect,possible
                --errors-for-leak-kinds=definite,indirect,possible
                --track-origins=yes
                $<TARGET_FILE:test_capi_${variant}> "${FASM_REPO_ROOT}")
    endforeach()
elseif(FASM_VALGRIND)
    message(STATUS "valgrind not found: the valgrind tests are skipped")
endif()

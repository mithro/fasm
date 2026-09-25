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

# C and C++ test programs of the fasm C API and C++ wrapper (rust/fasm-capi,
# include/fasm/fasm.h, include/fasm/fasm.hpp).
#
# Builds tests/c/test_capi.c and tests/cpp/test_capi.cpp (and the Xilinx
# tests tests/c/test_xilinx.c and tests/cpp/test_xilinx.cpp) twice each: linked
# against the shared library (test_capi_shared / test_capi_cpp_shared) and
# against the static library (test_capi_static / test_capi_cpp_static), both
# taken from the Cargo target directory, which must have been built first
# (`cargo build -p fasm-capi`, or `--release` with
# -DFASM_CARGO_PROFILE=release). It also compiles tests/cpp/header_only.cpp
# (a translation unit that only includes fasm.hpp) with every C++ compiler
# found among g++/clang++ at both -std=c++17 and -std=c++20, to guarantee
# the header is self contained. `make capi-test` at the repository root
# does all of it and runs the tests with ctest, under valgrind too when it
# is installed.

cmake_minimum_required(VERSION 3.13)
project(fasm_capi_tests C CXX)

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
if(WIN32)
    # Not exercised in CI: cargo names the import library fasm_capi.dll.lib.
    set_target_properties(fasm_capi_shared PROPERTIES
        IMPORTED_IMPLIB "${FASM_LIB_DIR}/fasm_capi.dll.lib")
endif()

# The Rust staticlib needs the system libraries the Rust standard library
# uses (`cargo rustc -p fasm-capi -- --print native-static-libs`).
find_package(Threads REQUIRED)
add_library(fasm_capi_static STATIC IMPORTED)
set_target_properties(fasm_capi_static PROPERTIES
    IMPORTED_LOCATION "${FASM_STATIC_LIB}"
    INTERFACE_INCLUDE_DIRECTORIES "${FASM_REPO_ROOT}/include"
    INTERFACE_COMPILE_DEFINITIONS "FASM_STATIC")
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(FASM_NATIVE_LIBS Threads::Threads ${CMAKE_DL_LIBS} m rt util)
elseif(WIN32)
    # Not exercised in CI.
    set(FASM_NATIVE_LIBS ws2_32 userenv ntdll bcrypt advapi32 kernel32)
else()
    set(FASM_NATIVE_LIBS Threads::Threads ${CMAKE_DL_LIBS} m)
endif()
set_property(TARGET fasm_capi_static PROPERTY INTERFACE_LINK_LIBRARIES ${FASM_NATIVE_LIBS})

foreach(variant shared static)
    set(exe "test_capi_${variant}")
    add_executable(${exe} test_capi.c)
    set_target_properties(${exe} PROPERTIES C_STANDARD 99 C_STANDARD_REQUIRED ON C_EXTENSIONS OFF)
    if(CMAKE_C_COMPILER_ID MATCHES "GNU|Clang")
        target_compile_options(${exe} PRIVATE -Wall -Wextra -Werror -std=c99)
    endif()
    target_link_libraries(${exe} PRIVATE fasm_capi_${variant})
endforeach()

# The C++ wrapper test (tests/cpp/test_capi.cpp), against the same shared
# and static libraries, with -Wpedantic added (fasm.hpp must also be clean
# under it) and the C++17 standard the header targets.
foreach(variant shared static)
    set(exe "test_capi_cpp_${variant}")
    add_executable(${exe} ../cpp/test_capi.cpp)
    set_target_properties(${exe} PROPERTIES CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON CXX_EXTENSIONS OFF)
    if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
        target_compile_options(${exe} PRIVATE -Wall -Wextra -Wpedantic -Werror -std=c++17)
    endif()
    target_link_libraries(${exe} PRIVATE fasm_capi_${variant})
endforeach()

# The Xilinx functions (fasm_xilinx_*, fasm::xilinx): tests/c/test_xilinx.c
# and tests/cpp/test_xilinx.cpp, which also compare their .frm/.bit output
# with the Rust command line tools of the same Cargo profile directory
# (`cargo build -p fasm-cli`; skipped when they are not built), run in a
# work directory of their own under the build directory.
foreach(variant shared static)
    set(exe "test_xilinx_${variant}")
    add_executable(${exe} test_xilinx.c)
    set_target_properties(${exe} PROPERTIES C_STANDARD 99 C_STANDARD_REQUIRED ON C_EXTENSIONS OFF)
    if(CMAKE_C_COMPILER_ID MATCHES "GNU|Clang")
        # system() is POSIX: -std=c99 alone hides nothing it needs.
        target_compile_options(${exe} PRIVATE -Wall -Wextra -Werror -std=c99)
    endif()
    target_link_libraries(${exe} PRIVATE fasm_capi_${variant})

    set(exe "test_xilinx_cpp_${variant}")
    add_executable(${exe} ../cpp/test_xilinx.cpp)
    set_target_properties(${exe} PROPERTIES CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON CXX_EXTENSIONS OFF)
    if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
        target_compile_options(${exe} PRIVATE -Wall -Wextra -Wpedantic -Werror -std=c++17)
    endif()
    target_link_libraries(${exe} PRIVATE fasm_capi_${variant})
endforeach()

set(FASM_TEST_EXES test_capi_shared test_capi_static test_capi_cpp_shared test_capi_cpp_static)
set(FASM_XILINX_TEST_EXES
    test_xilinx_shared test_xilinx_static test_xilinx_cpp_shared test_xilinx_cpp_static)

enable_testing()
foreach(exe ${FASM_TEST_EXES})
    add_test(NAME capi_${exe} COMMAND ${exe} "${FASM_REPO_ROOT}")
endforeach()
foreach(exe ${FASM_XILINX_TEST_EXES})
    set(work "${CMAKE_CURRENT_BINARY_DIR}/work-${exe}")
    file(MAKE_DIRECTORY "${work}")
    add_test(NAME capi_${exe} COMMAND ${exe} "${FASM_REPO_ROOT}" "${FASM_LIB_DIR}" "${work}")
endforeach()
if(WIN32)
    # There is no RPATH on Windows: find the DLL through PATH.
    foreach(exe test_capi_shared test_capi_cpp_shared test_xilinx_shared test_xilinx_cpp_shared)
        set_tests_properties(capi_${exe} PROPERTIES ENVIRONMENT "PATH=${FASM_LIB_DIR};$ENV{PATH}")
    endforeach()
endif()

# Memory checking: every error and every definitely or indirectly lost
# block fails the test. The global feature name interner (fasm::idstring)
# deliberately never frees its tables; they stay referenced from a static,
# so valgrind reports them as "still reachable", which is not an error
# here (see docs/rewrite/DESIGN-capi.md).
find_program(VALGRIND_EXECUTABLE valgrind)
if(FASM_VALGRIND AND VALGRIND_EXECUTABLE)
    foreach(exe ${FASM_TEST_EXES})
        add_test(NAME capi_${exe}_valgrind
            COMMAND "${VALGRIND_EXECUTABLE}"
                --error-exitcode=1
                --leak-check=full
                --show-leak-kinds=definite,indirect,possible
                --errors-for-leak-kinds=definite,indirect,possible
                --track-origins=yes
                "--suppressions=${CMAKE_CURRENT_SOURCE_DIR}/valgrind.supp"
                $<TARGET_FILE:${exe}> "${FASM_REPO_ROOT}")
    endforeach()
    # The command line tools the Xilinx tests run are not checked (only
    # the test program: --trace-children is off).
    foreach(exe ${FASM_XILINX_TEST_EXES})
        set(work "${CMAKE_CURRENT_BINARY_DIR}/work-${exe}-valgrind")
        file(MAKE_DIRECTORY "${work}")
        add_test(NAME capi_${exe}_valgrind
            COMMAND "${VALGRIND_EXECUTABLE}"
                --error-exitcode=1
                --leak-check=full
                --show-leak-kinds=definite,indirect,possible
                --errors-for-leak-kinds=definite,indirect,possible
                --track-origins=yes
                "--suppressions=${CMAKE_CURRENT_SOURCE_DIR}/valgrind.supp"
                $<TARGET_FILE:${exe}> "${FASM_REPO_ROOT}" "${FASM_LIB_DIR}" "${work}")
    endforeach()
elseif(FASM_VALGRIND)
    message(STATUS "valgrind not found: the valgrind tests are skipped")
endif()

# Header self-containment matrix: tests/cpp/header_only.cpp (which only
# includes <fasm/fasm.hpp>) is compiled on its own (not linked) with every
# C++ compiler found among g++ and clang++, at both -std=c++17 and
# -std=c++20, -Wall -Wextra -Wpedantic -Werror. Run directly (not through
# CMake's own CMAKE_CXX_COMPILER, which is only one of the two), so this
# also works when the project was configured with neither compiler as the
# default. A compiler that is not installed is skipped, like valgrind.
find_program(FASM_GXX_EXECUTABLE g++)
find_program(FASM_CLANGXX_EXECUTABLE clang++)
set(FASM_HEADER_ONLY_SRC "${CMAKE_CURRENT_SOURCE_DIR}/../cpp/header_only.cpp")
foreach(compiler_name "g++" "clang++")
    if(compiler_name STREQUAL "g++")
        set(compiler_exe "${FASM_GXX_EXECUTABLE}")
    else()
        set(compiler_exe "${FASM_CLANGXX_EXECUTABLE}")
    endif()
    if(compiler_exe)
        foreach(std 17 20)
            string(REPLACE "+" "x" compiler_tag "${compiler_name}")
            set(obj "${CMAKE_CURRENT_BINARY_DIR}/header_only_${compiler_tag}_cpp${std}.o")
            add_test(NAME capi_header_only_${compiler_tag}_cpp${std}
                COMMAND "${compiler_exe}" -std=c++${std} -Wall -Wextra -Wpedantic -Werror
                        -I "${FASM_REPO_ROOT}/include" -c "${FASM_HEADER_ONLY_SRC}" -o "${obj}")
        endforeach()
    else()
        message(STATUS "${compiler_name} not found: its header self-containment tests are skipped")
    endif()
endforeach()

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

# Minimal example building example.cpp against an *installed* libfasm_capi
# through pkg-config (`fasm.pc`, see ../../fasm.pc.in and `make
# capi-install PREFIX=...` at the repository root). Point PKG_CONFIG_PATH
# (an environment variable, not a CMake option) at
# PREFIX/lib/pkgconfig before configuring:
#
#   make capi-install PREFIX=/some/prefix
#   PKG_CONFIG_PATH=/some/prefix/lib/pkgconfig \
#       cmake -S rust/fasm-capi/examples/cpp -B /tmp/fasm-example-build
#   cmake --build /tmp/fasm-example-build
#   LD_LIBRARY_PATH=/some/prefix/lib /tmp/fasm-example-build/fasm_example

cmake_minimum_required(VERSION 3.13)
project(fasm_cpp_example CXX)

find_package(PkgConfig REQUIRED)
pkg_check_modules(FASM REQUIRED IMPORTED_TARGET fasm)

add_executable(fasm_example example.cpp)
set_target_properties(fasm_example PROPERTIES
    CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON CXX_EXTENSIONS OFF)
if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
    target_compile_options(fasm_example PRIVATE -Wall -Wextra -Wpedantic -Werror)
endif()
target_link_libraries(fasm_example PRIVATE PkgConfig::FASM)

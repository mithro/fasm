/*
 * Copyright 2017-2022 F4PGA Authors
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

/*
 * Minimal example of building against an installed libfasm_capi with
 * pkg-config, through the C++ wrapper (include/fasm/fasm.hpp).
 *
 *   make capi-install PREFIX=/some/prefix
 *   export PKG_CONFIG_PATH=/some/prefix/lib/pkgconfig
 *   g++ -std=c++17 $(pkg-config --cflags fasm) example.cpp \
 *       $(pkg-config --libs fasm) -o example
 *   LD_LIBRARY_PATH=/some/prefix/lib ./example "A.B[3:0] = 4'hA\n"
 *
 * (or `cmake -S . -B build && cmake --build build`, see CMakeLists.txt).
 */

#include <fasm/fasm.hpp>

#include <cstdio>
#include <string>

int main(int argc, char **argv) {
    std::string text = argc > 1 ? argv[1] : "CLB.SLICE.INIT[3:0] = 4'hA # example\n";

    try {
        fasm::File file = fasm::File::parse(text);
        std::printf("fasm %s: parsed %zu line(s)\n", std::string(fasm::version()).c_str(),
                    file.size());
        for (const fasm::Line &line : file) {
            if (auto sf = line.set_feature()) {
                std::printf("  %s\n", sf->to_string().c_str());
            }
        }
        std::printf("canonical:\n%s", file.to_string(/*canonical=*/true).c_str());
    } catch (const fasm::Error &e) {
        std::fprintf(stderr, "fasm error (%s) at %zu:%zu: %s\n",
                     fasm::to_c(e.status()) == FASM_ERR_PARSE ? "parse" : "other", e.line(),
                     e.column(), e.what());
        return 1;
    }
    return 0;
}

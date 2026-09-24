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
 * Guarantees that include/fasm/fasm.hpp is self contained: this
 * translation unit includes nothing else, and is only ever compiled
 * (`-c`), never linked (see CMakeLists.txt), so it exercises the header's
 * declarations and template bodies without needing libfasm_capi at all.
 * Built with both g++ and clang++, at -std=c++17 and -std=c++20, with
 * -Wall -Wextra -Wpedantic -Werror.
 */

#include <fasm/fasm.hpp>

int main() {
    // Touch one symbol from each of the header's public types so every
    // template in it that is instantiated by ordinary use gets compiled,
    // not just parsed.
    fasm::File file;
    fasm::Status status = fasm::Status::Ok;
    fasm::ValueFormat format = fasm::ValueFormat::Plain;
    (void)status;
    (void)format;
    (void)fasm::version();
    (void)file.size();
    for (const fasm::Line &line : file) {
        (void)line.set_feature();
    }
    return 0;
}

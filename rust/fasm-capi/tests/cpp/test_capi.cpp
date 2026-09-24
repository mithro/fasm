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
 * Test program of the fasm C++ wrapper (include/fasm/fasm.hpp), C++17, no
 * framework (mirrors tests/c/test_capi.c, but through the RAII wrapper).
 *
 * Usage: test_capi REPO_ROOT
 *
 * Reads REPO_ROOT/examples/many.fasm and the Python oracle outputs in
 * REPO_ROOT/tests/corpus/oracle/. Exits with 0 when every check passes;
 * every failed check is reported on stderr. Every object is RAII managed,
 * so the program is also a leak test when run under valgrind (see
 * CMakeLists.txt).
 */

#include <fasm/fasm.hpp>

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

namespace {

int checks = 0;
int failures = 0;

// Records the outcome of a check, printing a message if it failed. Like the
// C test's CHECK, the condition is evaluated once by the caller (via the
// CHECK/REQUIRE macros below); msg is only evaluated on failure.
bool check_impl(bool ok, const char *file, int line, const char *expr, const std::string &msg) {
    checks++;
    if (!ok) {
        failures++;
        std::fprintf(stderr, "%s:%d: CHECK(%s) failed: %s\n", file, line, expr, msg.c_str());
    }
    return ok;
}

} // namespace

// CHECK(condition, message): records a failure; message may be any
// stream-like expression built with FASM_MSG.
#define FASM_MSG(...) (std::ostringstream() << __VA_ARGS__).str()
#define CHECK(cond, ...) check_impl((cond), __FILE__, __LINE__, #cond, FASM_MSG(__VA_ARGS__))

// REQUIRE: like CHECK, but stops the program (for values used next).
#define REQUIRE(cond, ...)                                                                       \
    do {                                                                                          \
        if (!CHECK(cond, __VA_ARGS__)) {                                                          \
            std::fprintf(stderr, "stopping after a failed REQUIRE\n");                            \
            std::exit(1);                                                                         \
        }                                                                                          \
    } while (0)

namespace {

std::string repo_root = ".";

std::string repo_path(const std::string &rel) { return repo_root + "/" + rel; }

// Reads a whole file into a std::string.
std::string read_file(const std::string &path) {
    std::ifstream f(path, std::ios::binary);
    REQUIRE(f.good(), "cannot open " << path);
    std::ostringstream ss;
    ss << f.rdbuf();
    return ss.str();
}

// ------------------------------------------------------------------ //

void test_version_and_status() {
    CHECK(!fasm::version().empty(), "version string");
    CHECK(fasm::to_c(fasm::Status::Ok) == FASM_OK, "Status::Ok maps to FASM_OK");
    CHECK(fasm::to_c(fasm::Status::ParseError) == FASM_ERR_PARSE, "Status::ParseError");
    CHECK(fasm::to_c(fasm::Status::Io) == FASM_ERR_IO, "Status::Io");
    CHECK(fasm::to_c(fasm::Status::InvalidArg) == FASM_ERR_INVALID_ARG, "Status::InvalidArg");
    CHECK(fasm::to_c(fasm::Status::Utf8) == FASM_ERR_UTF8, "Status::Utf8");
    CHECK(fasm::to_c(fasm::Status::Panic) == FASM_ERR_PANIC, "Status::Panic");
    CHECK(fasm::to_c(fasm::Status::Output) == FASM_ERR_OUTPUT, "Status::Output");
    CHECK(fasm::from_c(FASM_OK) == fasm::Status::Ok, "from_c(FASM_OK)");
}

// Checks lines of examples/many.fasm, mirroring test_capi.c's
// check_many_lines.
void check_many_lines(const fasm::File &file) {
    REQUIRE(file.size() == 40, "many.fasm has 40 lines, got " << file.size());

    // 0: comment only.
    {
        fasm::Line line = file[0];
        CHECK(!line.set_feature().has_value(), "line 0 has no feature");
        CHECK(line.annotations().empty(), "line 0 has no annotations");
        auto comment = line.comment();
        REQUIRE(comment.has_value(), "line 0 has a comment");
        CHECK(*comment == " This file should have examples of all FASM lines that should parse.",
              "line 0 comment text: " << *comment);
    }

    // 3: bare '#', an empty comment. 7: whitespace comment.
    {
        auto comment = file[3].comment();
        CHECK(comment.has_value() && comment->empty(), "line 3 empty comment");
        auto comment7 = file[7].comment();
        CHECK(comment7.has_value() && *comment7 == "    ", "line 7 whitespace comment");
    }

    // 10: INT_L_X10Y146.SW6BEG0.WW2END0 (implicit 1).
    {
        fasm::Line line = file[10];
        CHECK(!line.comment().has_value(), "line 10 has no comment");
        auto sf = line.set_feature();
        REQUIRE(sf.has_value(), "line 10 feature");
        CHECK(sf->name() == "INT_L_X10Y146.SW6BEG0.WW2END0", "line 10 name: " << sf->name());
        CHECK(!sf->start().has_value(), "no start");
        CHECK(!sf->end().has_value(), "no end");
        CHECK(!sf->value_format().has_value(), "implicit value");
        CHECK(sf->width() == 1, "width 1");
        auto v = sf->value().u64();
        CHECK(v.has_value() && *v == 1, "value 1");
        CHECK(sf->value().bit_length() == 1, "1 bit");

        // name_into truncation semantics.
        char buf[8];
        std::size_t len = sf->name_into(buf, sizeof(buf));
        CHECK(len == std::string("INT_L_X10Y146.SW6BEG0.WW2END0").size(), "name_into full length");
        CHECK(len >= sizeof(buf), "name_into truncates");
        CHECK(std::string(buf, 7) == "INT_L_X", "name_into truncated content");
    }

    // 11: CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT[17].
    {
        auto sf = file[11].set_feature();
        REQUIRE(sf.has_value(), "line 11 feature");
        CHECK(sf->name() == "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT", "line 11 name");
        CHECK(sf->start() == std::optional<std::uint32_t>(17), "start 17");
        CHECK(!sf->end().has_value(), "line 11 no end");
        CHECK(sf->width() == 1, "line 11 width");
    }

    // 13: ... = 1 (Plain). 16: [0:0] = 1'b1.
    {
        auto sf13 = file[13].set_feature();
        CHECK(sf13->value_format() == fasm::ValueFormat::Plain, "line 13 plain");
        auto sf16 = file[16].set_feature();
        CHECK(sf16->start() == std::optional<std::uint32_t>(0) &&
                  sf16->end() == std::optional<std::uint32_t>(0),
              "line 16 [0:0]");
        CHECK(sf16->value_format() == fasm::ValueFormat::VerilogBinary, "line 16 binary");
    }

    // 20: ... = 0.
    {
        auto sf = file[20].set_feature();
        auto v = sf->value().u64();
        CHECK(v.has_value() && *v == 0, "line 20 value 0");
        CHECK(sf->value().bit_length() == 0, "0 needs 0 bits");
        CHECK(sf->value().bytes_le().empty(), "0 needs 0 bytes");
    }

    // 26: [63:32] = 32'b11110000_... (4042322160).
    {
        auto sf = file[26].set_feature();
        CHECK(sf->start() == std::optional<std::uint32_t>(32) &&
                  sf->end() == std::optional<std::uint32_t>(63),
              "[63:32]");
        CHECK(sf->width() == 32, "width 32");
        auto v = sf->value();
        auto u = v.u64();
        CHECK(u.has_value() && *u == 4042322160u, "line 26 value");
        CHECK(v.bit(31) && !v.bit(0) && !v.bit(5000), "line 26 bits");
        CHECK(v.to_string(16, true) == "F0F0F0F0", "hex");
        CHECK(v.to_string(10, false) == "4042322160", "dec");
        CHECK(v.to_string(8, false) == "36074170360", "oct");
        CHECK(v.to_string(2, false) == "11110000111100001111000011110000", "bin");
    }

    // 28: = 5'h1F (31, VerilogHex). 29: = 32'o1234567 (342391).
    {
        auto sf28 = file[28].set_feature();
        CHECK(sf28->value_format() == fasm::ValueFormat::VerilogHex, "line 28 hex");
        auto v28 = sf28->value().u64();
        CHECK(v28.has_value() && *v28 == 31, "line 28 value");
        auto sf29 = file[29].set_feature();
        CHECK(sf29->value_format() == fasm::ValueFormat::VerilogOctal, "line 29 octal");
        auto v29 = sf29->value().u64();
        CHECK(v29.has_value() && *v29 == 342391, "line 29 value");
    }

    // 31: { .attr = "" }.
    {
        fasm::Line line = file[31];
        REQUIRE(line.annotations().size() == 1, "line 31 one annotation");
        auto a = line.annotations()[0];
        CHECK(a.name == ".attr" && a.value.empty(), "line 31 annotation");
    }

    // 34: three annotations.
    {
        fasm::Line line = file[34];
        REQUIRE(line.annotations().size() == 3, "line 34 three annotations");
        auto annotations = line.annotations();
        CHECK(annotations[0].name == "module" && annotations[0].value == "top",
              "line 34 annotation 0");
        CHECK(annotations[1].name == "file" && annotations[1].value == "/a/b/d.txt",
              "line 34 annotation 1");
        CHECK(annotations[2].name == "line_number" && annotations[2].value == "123",
              "line 34 annotation 2");
        // Random access iteration over the range (used again in
        // test_iterators_and_algorithms for the whole file).
        std::vector<std::string> names;
        for (const fasm::Annotation &ann : annotations) {
            names.emplace_back(ann.name);
        }
        CHECK((names == std::vector<std::string>{"module", "file", "line_number"}),
              "annotation range-for order");
    }

    // 36: annotation only.
    {
        fasm::Line line = file[36];
        CHECK(!line.set_feature().has_value() && line.annotations().size() == 1 &&
                  !line.comment().has_value(),
              "line 36 annotation only");
    }

    // 38: feature + annotation + comment.
    {
        fasm::Line line = file[38];
        CHECK(line.set_feature().has_value() && line.annotations().size() == 1,
              "line 38 feature and annotation");
        auto comment = line.comment();
        CHECK(comment.has_value() && *comment == " This is a comment", "line 38 comment");
        CHECK(line.to_string(false) ==
                  "INT_L_X10Y146.SW6BEG0.WW2END0 { .top_module = \"/a/b/c/d.txt\" } "
                  "# This is a comment",
              "line 38 to string: " << line.to_string(false));
        CHECK(line.to_string(true) == "INT_L_X10Y146.SW6BEG0.WW2END0", "line 38 canonical");
        CHECK(file[0].to_string(true).empty(), "canonical comment line is empty");
        CHECK(file[3].to_string(false) == "#", "bare #");
    }

    // set_feature_to_string (and its canonical check).
    {
        auto sf = file[29].set_feature();
        CHECK(sf->to_string(false) == "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[63:32] = 32'o1234567",
              "set feature to string");
        try {
            sf->to_string(true);
            CHECK(false, "expected an Error for a non canonical feature");
        } catch (const fasm::Error &e) {
            CHECK(e.status() == fasm::Status::Output, "not canonical is an output error");
        }
        auto sf11 = file[11].set_feature();
        CHECK(sf11->to_string(true) == "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT[17]",
              "canonical feature");
    }
}

void test_parse_file_and_output() {
    fasm::File file = fasm::File::parse_file(repo_path("examples/many.fasm"));
    check_many_lines(file);

    std::string out = file.to_string(false);
    std::string expected = read_file(repo_path("tests/corpus/oracle/many.fasm.out.txt"));
    CHECK(out == expected, "output matches many.fasm.out.txt");

    // Round trip: parsing the output and printing it again is stable.
    fasm::File again = fasm::File::parse(out);
    CHECK(again.size() == file.size(), "reparse line count");
    CHECK(again.to_string(false) == out, "round trip is identical");

    std::string canonical = file.to_string(true);
    std::string expected_canonical = read_file(repo_path("tests/corpus/oracle/many.fasm.canonical.txt"));
    CHECK(canonical == expected_canonical, "canonical output matches many.fasm.canonical.txt");
}

void test_parse_string_many() {
    // The same file through File::parse (a std::string_view, not
    // necessarily NUL terminated: build one from a buffer with trailing
    // garbage past the given length).
    std::string text = read_file(repo_path("examples/many.fasm"));
    std::vector<char> buf(text.begin(), text.end());
    buf.push_back('X'); // garbage after the given length must be ignored
    fasm::File file = fasm::File::parse(std::string_view(buf.data(), text.size()));
    check_many_lines(file);
}

void test_wide_value() {
    // A 256 bit value: bit 255 and bit 0 set, plus 0xAB in byte 16.
    const char *text = "BRAM.INIT_00[255:0] = "
                       "256'h800000000000000000000000000000AB00000000000000000000000000000001\n";
    fasm::File file = fasm::File::parse(text);
    auto sf = file[0].set_feature();
    REQUIRE(sf.has_value(), "wide value feature");
    auto v = sf->value();

    CHECK(sf->width() == 256, "width 256");
    CHECK(v.bit_length() == 256, "256 bit value");
    CHECK(!v.u64().has_value(), "does not fit in 64 bits");

    std::vector<std::uint8_t> bytes = v.bytes_le();
    REQUIRE(bytes.size() == 32, "needs 32 bytes, got " << bytes.size());
    std::vector<std::uint8_t> expected(32, 0);
    expected[0] = 0x01;
    expected[16] = 0xAB;
    expected[31] = 0x80;
    CHECK(bytes == expected, "wide value bytes");

    CHECK(v.bit(255) && v.bit(0) && !v.bit(1) && v.bit(133), "wide bits");
    CHECK(sf->to_string(false) == "BRAM.INIT_00[255:0] = "
                                  "256'h800000000000000000000000000000AB00000000000000000000000000000001",
          "wide value printed");
}

void test_errors() {
    // Syntax / value error with a position.
    {
        const std::string bad = "A.B\nA.B[3:0] = 5'h1F\n";
        try {
            fasm::File::parse(bad);
            CHECK(false, "expected a parse error");
        } catch (const fasm::Error &e) {
            CHECK(e.status() == fasm::Status::ParseError, "parse error status");
            CHECK(e.has_position(), "has a position");
            CHECK(e.line() == 2, "error line " << e.line());
            CHECK(e.column() == 11, "error column " << e.column());
            std::string_view what = e.what();
            CHECK(what.substr(0, 22) == "Parse error at 2:11 - ", "error message: " << what);
        }
    }

    // Unterminated address.
    try {
        fasm::File::parse("A.B[");
        CHECK(false, "expected a parse error");
    } catch (const fasm::Error &e) {
        CHECK(e.status() == fasm::Status::ParseError && e.line() == 1 && e.column() == 4,
              "unterminated address at 1:" << e.column());
    }

    // Invalid UTF-8.
    try {
        fasm::File::parse("A.B # \xff\xfe\n");
        CHECK(false, "expected a UTF-8 error");
    } catch (const fasm::Error &e) {
        CHECK(e.status() == fasm::Status::Utf8 && e.line() == 1, "invalid UTF-8 error");
    }

    // Missing file.
    try {
        fasm::File::parse_file(repo_path("does/not/exist.fasm"));
        CHECK(false, "expected an IO error");
    } catch (const fasm::Error &e) {
        CHECK(e.status() == fasm::Status::Io, "missing file status");
        CHECK(!e.has_position(), "io error has no position");
        std::string_view what = e.what();
        CHECK(what.find("does/not/exist.fasm") != std::string_view::npos,
              "io message: " << what);
    }

    // An empty string is a valid, empty file.
    {
        fasm::File file = fasm::File::parse(std::string_view());
        CHECK(file.size() == 0, "empty text is an empty file");
        CHECK(file.to_string(false) == "\n", "empty file prints \\n");
    }
}

// Streaming callback state, mirroring test_capi.c's stream_state.
struct StreamState {
    std::size_t calls = 0;
    std::size_t stop_after = static_cast<std::size_t>(-1);
    std::size_t features = 0;
    std::size_t last_line_number = 0;
    std::string first_name;
};

bool on_line(StreamState &state, const fasm::Line &line, std::size_t line_number) {
    state.calls++;
    state.last_line_number = line_number;
    if (auto sf = line.set_feature()) {
        if (state.features == 0) {
            state.first_name = sf->name();
        }
        state.features++;
    }
    return state.calls < state.stop_after;
}

void test_streaming() {
    const std::string text = "A.B\n\n# comment\nC.D[1:0] = 2'b10\nE.F\n";
    const std::string bad = "A.B\nC.D[\n";

    {
        StreamState state;
        fasm::File::parse_each(text, [&](const fasm::Line &line, std::size_t n) {
            return on_line(state, line, n);
        });
        CHECK(state.calls == 4 && state.features == 3,
              "4 lines, 3 features (" << state.calls << ", " << state.features << ")");
        CHECK(state.last_line_number == 5, "last line number " << state.last_line_number);
        CHECK(state.first_name == "A.B", "first name " << state.first_name);
    }

    {
        StreamState state;
        state.stop_after = 2;
        fasm::File::parse_each(text, [&](const fasm::Line &line, std::size_t n) {
            return on_line(state, line, n);
        });
        CHECK(state.calls == 2 && state.last_line_number == 3, "early stop after 2 lines");
    }

    // Stopping before a parse error hides the error.
    {
        StreamState state;
        state.stop_after = 1;
        fasm::File::parse_each(bad, [&](const fasm::Line &line, std::size_t n) {
            return on_line(state, line, n);
        });
        CHECK(state.calls == 1, "stop before the error");
    }

    // Lines before the error are delivered, then the error propagates as a
    // fasm::Error thrown out of parse_each.
    {
        StreamState state;
        try {
            fasm::File::parse_each(bad, [&](const fasm::Line &line, std::size_t n) {
                return on_line(state, line, n);
            });
            CHECK(false, "expected a parse error");
        } catch (const fasm::Error &e) {
            CHECK(e.status() == fasm::Status::ParseError && state.calls == 1 && e.line() == 2,
                  "lines before the error are delivered");
        }
    }

    // void returning callback (always continues).
    {
        std::size_t calls = 0;
        fasm::File::parse_each(text, [&](const fasm::Line &, std::size_t) { calls++; });
        CHECK(calls == 4, "void callback visits every line");
    }

    // parse_each_file over examples/many.fasm.
    {
        StreamState state;
        fasm::File::parse_each_file(repo_path("examples/many.fasm"),
                                    [&](const fasm::Line &line, std::size_t n) {
                                        return on_line(state, line, n);
                                    });
        CHECK(state.calls == 40 && state.features == 19,
              "stream many.fasm (" << state.calls << " lines, " << state.features
                                    << " features)");
        CHECK(state.first_name == "INT_L_X10Y146.SW6BEG0.WW2END0", "many first name");
        CHECK(state.last_line_number == 47, "many last line " << state.last_line_number);
    }

    // Missing file through the streaming API.
    try {
        fasm::File::parse_each_file(repo_path("nope.fasm"),
                                    [](const fasm::Line &, std::size_t) {});
        CHECK(false, "expected an IO error");
    } catch (const fasm::Error &e) {
        CHECK(e.status() == fasm::Status::Io, "stream missing file");
    }

    // An exception thrown from the callback propagates out of parse_each as
    // the very same exception type (not wrapped in a fasm::Error): the
    // trampoline documented at the top of fasm.hpp catches it with
    // `catch (...)`, stores it, stops parsing, and rethrows it once the C
    // call has returned.
    struct MyException : std::runtime_error {
        MyException() : std::runtime_error("callback boom") {}
    };
    {
        std::size_t seen = 0;
        bool caught_right_type = false;
        try {
            fasm::File::parse_each(text, [&](const fasm::Line &, std::size_t) -> bool {
                seen++;
                if (seen == 2) {
                    throw MyException();
                }
                return true;
            });
            CHECK(false, "expected MyException");
        } catch (const MyException &) {
            caught_right_type = true;
        } catch (...) {
            CHECK(false, "wrong exception type propagated");
        }
        CHECK(caught_right_type && seen == 2, "exception from the callback propagates as-is");
    }
}

bool zero_if_zero(std::string_view feature, int &calls) {
    calls++;
    return feature.find("ZERO") != std::string_view::npos;
}

std::int64_t reverse_key(std::string_view group_id) {
    return !group_id.empty() ? -static_cast<std::int64_t>(static_cast<unsigned char>(group_id[0]))
                             : 0;
}

void test_merge_and_sort() {
    fasm::File file = fasm::File::parse("B.X[1]\n# about A\nA.Y\nB.X[0]\nC.ZERO\n");

    {
        fasm::File merged = file.merge_and_sort();
        CHECK(merged.size() == 6, "merged lines " << merged.size());
        CHECK(merged.to_string(false) == "# about A\nA.Y\n\nB.X[1:0] = 2'b11\n\nC.ZERO\n",
              "merged output");
    }

    {
        int zero_calls = 0;
        fasm::File merged = file.merge_and_sort(
            fasm::File::ZeroFn{[&](std::string_view f) { return zero_if_zero(f, zero_calls); }},
            fasm::File::SortKeyFn{reverse_key});
        CHECK(zero_calls > 0, "zero function called");
        CHECK(merged.to_string(false) == "B.X[1:0] = 2'b11\n\n# about A\nA.Y\n",
              "merged output with callbacks");
    }

    {
        fasm::File merged = file.merge_and_sort(fasm::File::ZeroFn{}, fasm::File::SortKeyFn{});
        CHECK(merged.to_string(true) == "A.Y\nB.X\nB.X[1]\nC.ZERO\n", "ex without callbacks");
    }
}

// Calls of counter_key: the group ids (one letter each) in call order.
void test_merge_and_sort_counter_key() {
    // 26 groups A.F .. Z.F: the sort compares each several times.
    std::string text;
    for (char c = 'A'; c <= 'Z'; ++c) {
        text += c;
        text += ".F\n";
    }

    fasm::File file = fasm::File::parse(text);

    int count = 0;
    std::string order;
    fasm::File merged = file.merge_and_sort(
        fasm::File::ZeroFn{}, fasm::File::SortKeyFn{[&](std::string_view group_id) -> std::int64_t {
            if (group_id.size() == 1) {
                order.push_back(group_id[0]);
            }
            count++;
            return -static_cast<std::int64_t>(count);
        }});
    REQUIRE(count == 26, "sort key called once per group (" << count << " calls)");

    // The group seen last got the smallest key, so it is printed first.
    std::string expected;
    for (int i = 25; i >= 0; --i) {
        expected.push_back(order[static_cast<std::size_t>(i)]);
        expected += ".F\n";
        if (i > 0) {
            expected += "\n";
        }
    }
    CHECK(merged.to_string(false) == expected, "counter key order");
}

void test_build() {
    fasm::File file;
    CHECK(file.size() == 0, "new file is empty");

    // X.Y.INIT[15:0] = 16'h12A { a = "b c", d = "" } # built from C++
    fasm::SetFeatureSpec init_spec;
    init_spec.name = "X.Y.INIT";
    init_spec.start = 0;
    init_spec.end = 15;
    init_spec.value_le = {0x2A, 0x01};
    init_spec.format = fasm::ValueFormat::VerilogHex;
    std::vector<fasm::Annotation> init_annotations = {{"a", "b c"}, {"d", ""}};
    file.push_line(init_spec, init_annotations, " built from C++");

    // X.Y.EN (implicit 1).
    fasm::SetFeatureSpec en_spec;
    en_spec.name = "X.Y.EN";
    en_spec.value_le = {1};
    file.push_line(en_spec);

    // A 256 bit value, decimal.
    std::vector<std::uint8_t> wide(32, 0);
    wide[31] = 0x80;
    fasm::SetFeatureSpec wide_spec;
    wide_spec.name = "BRAM.INIT_01";
    wide_spec.start = 0;
    wide_spec.end = 255;
    wide_spec.value_le = wide;
    wide_spec.format = fasm::ValueFormat::VerilogDecimal;
    file.push_line(wide_spec);

    // Bare '#', then a blank line, then an annotation only line.
    file.push_comment("");
    file.push_blank_line();
    std::vector<fasm::Annotation> one_annotation = {{"a", "b c"}};
    file.push_annotations(one_annotation);
    CHECK(file.size() == 6, "6 lines built");

    std::string out = file.to_string(false);
    CHECK(out == "X.Y.INIT[15:0] = 16'h12A { a = \"b c\", d = \"\" } # built from C++\n"
                 "X.Y.EN\n"
                 "BRAM.INIT_01[255:0] = 256'd578960446186580977117854925043439539266349923328202"
                 "82019728792003956564819968\n"
                 "#\n"
                 "\n"
                 "{ a = \"b c\" }\n",
          "built file text:\n" << out);

    // The printed text parses back to the same model.
    fasm::File reparsed = fasm::File::parse(out);
    CHECK(reparsed.size() == 5, "blank line dropped when parsing (" << reparsed.size() << ")");
    auto reparsed_sf = reparsed[2].set_feature();
    CHECK(reparsed_sf.has_value() && reparsed_sf->value().bit_length() == 256,
          "wide value reparsed");

    // Validation errors append nothing.
    {
        fasm::SetFeatureSpec bad;
        bad.name = "X.Y.Z";
        bad.end = 3; // end without start
        try {
            file.push_line(bad);
            CHECK(false, "expected an error for end without start");
        } catch (const fasm::Error &e) {
            CHECK(e.status() == fasm::Status::InvalidArg, "end without start");
            std::string_view what = e.what();
            CHECK(what.find("without a start") != std::string_view::npos,
                  "message: " << what);
        }
    }
    {
        fasm::SetFeatureSpec bad;
        bad.name = "X.Y.Z";
        bad.start = 4;
        bad.end = 3; // end before start
        try {
            file.push_line(bad);
            CHECK(false, "expected an error for end before start");
        } catch (const fasm::Error &e) {
            CHECK(e.status() == fasm::Status::InvalidArg, "end before start");
        }
    }
    {
        fasm::SetFeatureSpec bad;
        bad.name = "X.Y.Z";
        bad.value_le = {2}; // value too wide (1 bit address, value needs 2)
        try {
            file.push_line(bad);
            CHECK(false, "expected an error for a too wide value");
        } catch (const fasm::Error &e) {
            CHECK(e.status() == fasm::Status::InvalidArg, "value too wide");
        }
    }
    {
        fasm::SetFeatureSpec bad;
        bad.name = std::string_view("X.\xff", 3);
        try {
            file.push_line(bad);
            CHECK(false, "expected a UTF-8 error");
        } catch (const fasm::Error &e) {
            CHECK(e.status() == fasm::Status::Utf8, "bad UTF-8 name");
        }
    }
    {
        try {
            file.push_comment(std::string_view("\xc3", 1));
            CHECK(false, "expected a UTF-8 error");
        } catch (const fasm::Error &e) {
            CHECK(e.status() == fasm::Status::Utf8, "bad UTF-8 comment");
        }
    }

    CHECK(file.size() == 6, "failed pushes append nothing");

    // SetFeatureSpec::with_u64 convenience constructor.
    fasm::SetFeatureSpec by_u64 =
        fasm::SetFeatureSpec::with_u64("X.Y.CNT", 258, fasm::ValueFormat::VerilogDecimal);
    by_u64.start = 0;
    by_u64.end = 15; // wide enough for 258 (9 bits)
    file.push_line(by_u64);
    auto last = file[file.size() - 1].set_feature();
    REQUIRE(last.has_value(), "with_u64 pushed");
    auto v = last->value().u64();
    CHECK(v.has_value() && *v == 258, "with_u64 value");
}

void test_move_semantics() {
    fasm::File file = fasm::File::parse("A.B\nC.D\n");
    CHECK(file.size() == 2, "before move");

    fasm::File moved = std::move(file);
    CHECK(moved.size() == 2, "moved-to file keeps the lines");

    fasm::File moved2;
    moved2 = std::move(moved);
    CHECK(moved2.size() == 2, "move assignment keeps the lines");

    // A file returned by value (parse/merge_and_sort) relies on move
    // semantics (or guaranteed copy elision) rather than a copy
    // constructor: File has none (copying is intentionally not implicit).
    static_assert(!std::is_copy_constructible_v<fasm::File>, "File must not be copyable");
    static_assert(std::is_move_constructible_v<fasm::File>, "File must be movable");
    static_assert(!std::is_copy_constructible_v<fasm::String>, "String must not be copyable");
    static_assert(std::is_move_constructible_v<fasm::String>, "String must be movable");
}

void test_iterators_and_algorithms() {
    fasm::File file = fasm::File::parse_file(repo_path("examples/many.fasm"));

    // Random access iterator category and distance.
    static_assert(std::is_same_v<std::iterator_traits<fasm::File::iterator>::iterator_category,
                                 std::random_access_iterator_tag>,
                  "File::iterator must be a random access iterator");
    CHECK(std::distance(file.begin(), file.end()) ==
              static_cast<std::ptrdiff_t>(file.size()),
          "iterator distance matches size");
    CHECK((file.begin() + 3) - file.begin() == 3, "iterator arithmetic");
    CHECK(file.begin()[3].raw() == file[3].raw(), "iterator operator[]");

    // std::count_if over lines with a feature.
    auto feature_lines = std::count_if(
        file.begin(), file.end(), [](const fasm::Line &l) { return l.set_feature().has_value(); });
    CHECK(feature_lines == 19, "count_if features: " << feature_lines);

    // range-for.
    std::size_t comment_lines = 0;
    for (const fasm::Line &line : file) {
        if (line.comment().has_value()) {
            comment_lines++;
        }
    }
    CHECK(comment_lines > 0, "range-for comment count: " << comment_lines);

    // std::any_of / std::find_if over a line's annotations.
    fasm::Line line34 = file[34];
    bool has_module = std::any_of(line34.annotations().begin(), line34.annotations().end(),
                                  [](const fasm::Annotation &a) { return a.name == "module"; });
    CHECK(has_module, "any_of over annotations");
}

void test_value_to_string_radix() {
    fasm::File file = fasm::File::parse("A.B[7:0] = 8'd200\n");
    auto sf = file[0].set_feature();
    REQUIRE(sf.has_value(), "value radix feature");
    auto v = sf->value();
    CHECK(v.to_string(10, false) == "200", "default-ish decimal");
    CHECK(v.to_string(16, false) == "c8", "hex lower");
    CHECK(v.to_string(16, true) == "C8", "hex upper");
    CHECK(v.to_string(2, false) == "11001000", "binary");
    CHECK(v.to_string(8, false) == "310", "octal");
    try {
        v.to_string(7, false);
        CHECK(false, "expected an error for radix 7");
    } catch (const fasm::Error &e) {
        CHECK(e.status() == fasm::Status::InvalidArg, "bad radix");
    }
}

} // namespace

int main(int argc, char **argv) {
    if (argc != 2) {
        std::fprintf(stderr, "usage: %s REPO_ROOT\n", argv[0]);
        return 2;
    }
    repo_root = argv[1];
    std::printf("libfasm %s (C++ wrapper)\n", std::string(fasm::version()).c_str());

    test_version_and_status();
    test_parse_file_and_output();
    test_parse_string_many();
    test_wide_value();
    test_errors();
    test_streaming();
    test_merge_and_sort();
    test_merge_and_sort_counter_key();
    test_build();
    test_move_semantics();
    test_iterators_and_algorithms();
    test_value_to_string_radix();

    std::printf("%d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}

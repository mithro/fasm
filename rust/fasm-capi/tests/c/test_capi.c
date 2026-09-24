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
 * Test program of the fasm C API (include/fasm/fasm.h), plain C99.
 *
 * Usage: test_capi REPO_ROOT
 *
 * Reads REPO_ROOT/examples/many.fasm and the Python oracle outputs in
 * REPO_ROOT/tests/corpus/oracle/. Exits with 0 when every check passes;
 * every failed check is reported on stderr. Every object is freed, so the
 * program is also a leak test when run under valgrind (see
 * CMakeLists.txt).
 */

#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "fasm/fasm.h"

static int checks = 0;
static int failures = 0;

#if defined(__GNUC__)
#define PRINTF_LIKE(fmt, args) __attribute__((format(printf, fmt, args)))
#else
#define PRINTF_LIKE(fmt, args)
#endif

/* Records the outcome of a check, printing a message if it failed. */
PRINTF_LIKE(5, 6)
static int check_impl(int ok, const char *file, int line, const char *expr, const char *fmt,
                      ...) {
    checks++;
    if (!ok) {
        va_list args;
        failures++;
        fprintf(stderr, "%s:%d: CHECK(%s) failed: ", file, line, expr);
        va_start(args, fmt);
        vfprintf(stderr, fmt, args);
        va_end(args);
        fputc('\n', stderr);
    }
    return ok;
}

/* CHECK(condition, printf style message, ...): records a failure. The
 * condition is evaluated once, the message arguments always. */
#define CHECK(cond, ...) check_impl((cond) ? 1 : 0, __FILE__, __LINE__, #cond, __VA_ARGS__)

/* REQUIRE: like CHECK, but stops the program (for pointers used next). */
#define REQUIRE(cond, ...)                                          \
    do {                                                            \
        if (!CHECK(cond, __VA_ARGS__)) {                            \
            fprintf(stderr, "stopping after a failed REQUIRE\n");   \
            exit(1);                                                \
        }                                                           \
    } while (0)

static const char *repo_root = ".";

/* Joins repo_root and rel into a static buffer. */
static const char *repo_path(const char *rel) {
    static char buf[4096];
    snprintf(buf, sizeof(buf), "%s/%s", repo_root, rel);
    return buf;
}

/* Reads a whole file into a malloc'ed NUL terminated buffer. */
static char *read_file(const char *path, size_t *len_out) {
    FILE *f = fopen(path, "rb");
    char *data;
    long len;
    REQUIRE(f != NULL, "cannot open %s", path);
    fseek(f, 0, SEEK_END);
    len = ftell(f);
    fseek(f, 0, SEEK_SET);
    data = malloc((size_t)len + 1);
    REQUIRE(data != NULL, "out of memory");
    REQUIRE(fread(data, 1, (size_t)len, f) == (size_t)len, "cannot read %s", path);
    data[len] = '\0';
    fclose(f);
    if (len_out) {
        *len_out = (size_t)len;
    }
    return data;
}

/* True if the fasm_str view equals the C string s. */
static int str_eq(fasm_str view, const char *s) {
    size_t n = strlen(s);
    return view.len == n && (n == 0 || memcmp(view.ptr, s, n) == 0);
}

/* True if the owned string equals s; frees it. */
static int take_eq(fasm_string *str, const char *s) {
    int eq = str != NULL && fasm_string_len(str) == strlen(s) &&
             strcmp(fasm_string_data(str), s) == 0;
    if (!eq && str != NULL) {
        fprintf(stderr, "  got: \"%s\"\n  expected: \"%s\"\n", fasm_string_data(str), s);
    }
    fasm_string_free(str);
    return eq;
}

/* The feature name of sf, via the buffer API (static buffer). */
static const char *feature_name(const fasm_set_feature *sf) {
    static char buf[256];
    size_t len = fasm_set_feature_name(sf, buf, sizeof(buf));
    CHECK(len < sizeof(buf), "name too long (%zu)", len);
    return buf;
}

/* Parses text (NUL terminated), requiring success. */
static fasm_file *parse(const char *text) {
    fasm_file *file = NULL;
    fasm_error *err = NULL;
    fasm_status status = fasm_parse_string(text, strlen(text), &file, &err);
    REQUIRE(status == FASM_OK, "parse failed: %s", fasm_error_message(err));
    CHECK(err == NULL, "err is reset to NULL on success");
    return file;
}

/* ------------------------------------------------------------------ */

static void test_version_and_status(void) {
    CHECK(fasm_version() != NULL && strlen(fasm_version()) > 0, "version string");
    CHECK(strcmp(fasm_status_string(FASM_OK), "ok") == 0, "FASM_OK string");
    CHECK(strcmp(fasm_status_string(FASM_ERR_PARSE), "parse error") == 0, "parse string");
    CHECK(strcmp(fasm_status_string(FASM_ERR_IO), "I/O error") == 0, "io string");
    CHECK(strcmp(fasm_status_string(FASM_ERR_INVALID_ARG), "invalid argument") == 0,
          "invalid arg string");
    CHECK(strcmp(fasm_status_string(FASM_ERR_UTF8), "invalid UTF-8") == 0, "utf8 string");
    CHECK(strcmp(fasm_status_string(FASM_ERR_PANIC), "internal error (panic)") == 0,
          "panic string");
    CHECK(strcmp(fasm_status_string(FASM_ERR_OUTPUT), "output error") == 0, "output string");
    CHECK(strcmp(fasm_status_string(1234), "unknown status") == 0, "unknown status string");
}

/* Checks lines of examples/many.fasm against tests/corpus/oracle/many.json. */
static void check_many_lines(const fasm_file *file) {
    const fasm_line *line;
    const fasm_set_feature *sf;
    fasm_str comment;
    fasm_annotation annotation;
    uint64_t value = 0;

    CHECK(fasm_file_line_count(file) == 40, "many.fasm has 40 lines, got %zu",
          fasm_file_line_count(file));
    CHECK(fasm_file_line(file, 40) == NULL, "out of range line is NULL");

    /* 0: comment only. */
    line = fasm_file_line(file, 0);
    REQUIRE(line != NULL, "line 0");
    CHECK(!fasm_line_has_set_feature(line), "line 0 has no feature");
    CHECK(fasm_line_set_feature(line) == NULL, "line 0 feature is NULL");
    CHECK(fasm_line_annotation_count(line) == 0, "line 0 has no annotations");
    CHECK(fasm_line_has_comment(line), "line 0 has a comment");
    CHECK(fasm_line_comment(line, &comment), "line 0 comment");
    CHECK(str_eq(comment,
                 " This file should have examples of all FASM lines that should parse."),
          "line 0 comment text");
    CHECK(fasm_line_comment(line, NULL), "comment test without out");

    /* 3: bare '#', an empty comment. 7: whitespace comment. */
    line = fasm_file_line(file, 3);
    CHECK(fasm_line_comment(line, &comment) && comment.len == 0, "line 3 empty comment");
    CHECK(fasm_line_comment(fasm_file_line(file, 7), &comment) && str_eq(comment, "    "),
          "line 7 whitespace comment");

    /* 10: INT_L_X10Y146.SW6BEG0.WW2END0 (implicit 1). */
    line = fasm_file_line(file, 10);
    CHECK(!fasm_line_has_comment(line), "line 10 has no comment");
    CHECK(!fasm_line_comment(line, &comment) && comment.ptr == NULL && comment.len == 0,
          "absent comment gives an empty view");
    CHECK(fasm_line_has_set_feature(line), "line 10 has a feature");
    sf = fasm_line_set_feature(line);
    REQUIRE(sf != NULL, "line 10 feature");
    CHECK(strcmp(feature_name(sf), "INT_L_X10Y146.SW6BEG0.WW2END0") == 0, "line 10 name");
    CHECK(fasm_set_feature_name_len(sf) == strlen("INT_L_X10Y146.SW6BEG0.WW2END0"),
          "line 10 name length");
    CHECK(take_eq(fasm_set_feature_name_string(sf), "INT_L_X10Y146.SW6BEG0.WW2END0"),
          "line 10 name string");
    CHECK(!fasm_set_feature_has_start(sf) && fasm_set_feature_start(sf) == 0, "no start");
    CHECK(!fasm_set_feature_has_end(sf) && fasm_set_feature_end(sf) == 0, "no end");
    CHECK(fasm_set_feature_value_format(sf) == FASM_VALUE_FORMAT_NONE, "implicit value");
    CHECK(fasm_set_feature_width(sf) == 1, "width 1");
    CHECK(fasm_set_feature_value_u64(sf, &value) && value == 1, "value 1");
    CHECK(fasm_set_feature_value_bits(sf) == 1, "1 bit");

    /* 11: CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT[17]. */
    sf = fasm_line_set_feature(fasm_file_line(file, 11));
    CHECK(strcmp(feature_name(sf), "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT") == 0, "line 11 name");
    CHECK(fasm_set_feature_has_start(sf) && fasm_set_feature_start(sf) == 17, "start 17");
    CHECK(!fasm_set_feature_has_end(sf), "line 11 no end");
    CHECK(fasm_set_feature_width(sf) == 1, "line 11 width");

    /* 13: ... = 1 (PLAIN). 16: [0:0] = 1'b1. */
    sf = fasm_line_set_feature(fasm_file_line(file, 13));
    CHECK(fasm_set_feature_value_format(sf) == FASM_VALUE_FORMAT_PLAIN, "line 13 plain");
    sf = fasm_line_set_feature(fasm_file_line(file, 16));
    CHECK(fasm_set_feature_has_start(sf) && fasm_set_feature_start(sf) == 0 &&
              fasm_set_feature_has_end(sf) && fasm_set_feature_end(sf) == 0,
          "line 16 [0:0]");
    CHECK(fasm_set_feature_value_format(sf) == FASM_VALUE_FORMAT_VERILOG_BINARY,
          "line 16 binary");

    /* 20: ... = 0. */
    sf = fasm_line_set_feature(fasm_file_line(file, 20));
    CHECK(fasm_set_feature_value_u64(sf, &value) && value == 0, "line 20 value 0");
    CHECK(fasm_set_feature_value_bits(sf) == 0, "0 needs 0 bits");
    CHECK(fasm_set_feature_value_bytes_le(sf, NULL, 0) == 0, "0 needs 0 bytes");

    /* 26: [63:32] = 32'b11110000_... (4042322160). */
    sf = fasm_line_set_feature(fasm_file_line(file, 26));
    CHECK(fasm_set_feature_start(sf) == 32 && fasm_set_feature_end(sf) == 63, "[63:32]");
    CHECK(fasm_set_feature_width(sf) == 32, "width 32");
    CHECK(fasm_set_feature_value_u64(sf, &value) && value == 4042322160u, "line 26 value");
    CHECK(fasm_set_feature_value_bit(sf, 31) && !fasm_set_feature_value_bit(sf, 0) &&
              !fasm_set_feature_value_bit(sf, 5000),
          "line 26 bits");
    CHECK(take_eq(fasm_set_feature_value_to_string(sf, 16, true, NULL), "F0F0F0F0"), "hex");
    CHECK(take_eq(fasm_set_feature_value_to_string(sf, 10, false, NULL), "4042322160"), "dec");
    CHECK(take_eq(fasm_set_feature_value_to_string(sf, 8, false, NULL), "36074170360"), "oct");
    CHECK(take_eq(fasm_set_feature_value_to_string(sf, 2, false, NULL),
                  "11110000111100001111000011110000"),
          "bin");

    /* 28: = 5'h1F (31, VERILOG_HEX). 29: = 32'o1234567 (342391). */
    sf = fasm_line_set_feature(fasm_file_line(file, 28));
    CHECK(fasm_set_feature_value_format(sf) == FASM_VALUE_FORMAT_VERILOG_HEX, "line 28 hex");
    CHECK(fasm_set_feature_value_u64(sf, &value) && value == 31, "line 28 value");
    sf = fasm_line_set_feature(fasm_file_line(file, 29));
    CHECK(fasm_set_feature_value_format(sf) == FASM_VALUE_FORMAT_VERILOG_OCTAL, "line 29 octal");
    CHECK(fasm_set_feature_value_u64(sf, &value) && value == 342391, "line 29 value");

    /* 31: { .attr = "" }. */
    line = fasm_file_line(file, 31);
    CHECK(fasm_line_annotation_count(line) == 1, "line 31 one annotation");
    CHECK(fasm_line_annotation(line, 0, &annotation) && str_eq(annotation.name, ".attr") &&
              str_eq(annotation.value, ""),
          "line 31 annotation");
    CHECK(!fasm_line_annotation(line, 1, &annotation), "annotation out of range");
    CHECK(!fasm_line_annotation(line, 0, NULL), "annotation NULL out");

    /* 34: three annotations. */
    line = fasm_file_line(file, 34);
    CHECK(fasm_line_annotation_count(line) == 3, "line 34 three annotations");
    CHECK(fasm_line_annotation(line, 0, &annotation) && str_eq(annotation.name, "module") &&
              str_eq(annotation.value, "top"),
          "line 34 annotation 0");
    CHECK(fasm_line_annotation(line, 1, &annotation) && str_eq(annotation.name, "file") &&
              str_eq(annotation.value, "/a/b/d.txt"),
          "line 34 annotation 1");
    CHECK(fasm_line_annotation(line, 2, &annotation) && str_eq(annotation.name, "line_number") &&
              str_eq(annotation.value, "123"),
          "line 34 annotation 2");

    /* 36: annotation only. */
    line = fasm_file_line(file, 36);
    CHECK(!fasm_line_has_set_feature(line) && fasm_line_annotation_count(line) == 1 &&
              !fasm_line_has_comment(line),
          "line 36 annotation only");

    /* 38: feature + annotation + comment. */
    line = fasm_file_line(file, 38);
    CHECK(fasm_line_has_set_feature(line) && fasm_line_annotation_count(line) == 1,
          "line 38 feature and annotation");
    CHECK(fasm_line_comment(line, &comment) && str_eq(comment, " This is a comment"),
          "line 38 comment");
    CHECK(take_eq(fasm_line_to_string(line, false, NULL),
                  "INT_L_X10Y146.SW6BEG0.WW2END0 { .top_module = \"/a/b/c/d.txt\" } "
                  "# This is a comment"),
          "line 38 to string");
    CHECK(take_eq(fasm_line_to_string(line, true, NULL), "INT_L_X10Y146.SW6BEG0.WW2END0"),
          "line 38 canonical");
    CHECK(take_eq(fasm_line_to_string(fasm_file_line(file, 0), true, NULL), ""),
          "canonical comment line is empty");
    CHECK(take_eq(fasm_line_to_string(fasm_file_line(file, 3), false, NULL), "#"), "bare #");

    /* set_feature_to_string (and its canonical check). */
    sf = fasm_line_set_feature(fasm_file_line(file, 29));
    CHECK(take_eq(fasm_set_feature_to_string(sf, false, NULL),
                  "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT[63:32] = 32'o1234567"),
          "set feature to string");
    {
        fasm_error *err = NULL;
        CHECK(fasm_set_feature_to_string(sf, true, &err) == NULL, "not canonical");
        CHECK(fasm_error_status(err) == FASM_ERR_OUTPUT, "not canonical is an output error");
        fasm_error_free(err);
    }
    sf = fasm_line_set_feature(fasm_file_line(file, 11));
    CHECK(take_eq(fasm_set_feature_to_string(sf, true, NULL),
                  "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT[17]"),
          "canonical feature");
}

static void test_parse_file_and_output(void) {
    fasm_file *file = NULL;
    fasm_file *again = NULL;
    fasm_error *err = NULL;
    fasm_string *out;
    fasm_string *out2;
    char *expected;
    fasm_status status;

    status = fasm_parse_file(repo_path("examples/many.fasm"), &file, &err);
    REQUIRE(status == FASM_OK && file != NULL, "parse many.fasm: %s", fasm_error_message(err));
    CHECK(err == NULL, "no error");
    check_many_lines(file);

    /* Output identical to the Python oracle. */
    out = fasm_file_to_string(file, false, &err);
    REQUIRE(out != NULL, "to string: %s", fasm_error_message(err));
    expected = read_file(repo_path("tests/corpus/oracle/many.fasm.out.txt"), NULL);
    CHECK(strcmp(fasm_string_data(out), expected) == 0, "output matches many.fasm.out.txt");
    CHECK(fasm_string_len(out) == strlen(expected), "output length");
    free(expected);

    /* Round trip: parsing the output and printing it again is stable. */
    status = fasm_parse_string(fasm_string_data(out), fasm_string_len(out), &again, &err);
    REQUIRE(status == FASM_OK, "reparse: %s", fasm_error_message(err));
    CHECK(fasm_file_line_count(again) == fasm_file_line_count(file), "reparse line count");
    out2 = fasm_file_to_string(again, false, NULL);
    CHECK(out2 != NULL && strcmp(fasm_string_data(out2), fasm_string_data(out)) == 0,
          "round trip is identical");
    fasm_string_free(out2);
    fasm_file_free(again);
    fasm_string_free(out);

    out = fasm_file_to_string(file, true, NULL);
    expected = read_file(repo_path("tests/corpus/oracle/many.fasm.canonical.txt"), NULL);
    CHECK(out != NULL && strcmp(fasm_string_data(out), expected) == 0,
          "canonical output matches many.fasm.canonical.txt");
    free(expected);
    fasm_string_free(out);

    fasm_file_free(file);
}

static void test_parse_string_many(void) {
    /* The same file through fasm_parse_string (not NUL terminated). */
    size_t len;
    char *text = read_file(repo_path("examples/many.fasm"), &len);
    fasm_file *file = NULL;
    char *copy = malloc(len + 1);
    REQUIRE(copy != NULL, "out of memory");
    memcpy(copy, text, len);
    copy[len] = 'X'; /* garbage after the given length must be ignored */
    CHECK(fasm_parse_string(copy, len, &file, NULL) == FASM_OK, "parse string");
    check_many_lines(file);
    fasm_file_free(file);
    free(copy);
    free(text);
}

static void test_wide_value(void) {
    /* A 256 bit value: bit 255 and bit 0 set, plus 0xAB in byte 16. */
    const char *text =
        "BRAM.INIT_00[255:0] = "
        "256'h800000000000000000000000000000AB00000000000000000000000000000001\n";
    fasm_file *file = parse(text);
    const fasm_set_feature *sf = fasm_line_set_feature(fasm_file_line(file, 0));
    uint8_t buf[40];
    uint8_t expected[40];
    uint64_t value = 0;
    size_t i;

    CHECK(fasm_set_feature_width(sf) == 256, "width 256");
    CHECK(fasm_set_feature_value_bits(sf) == 256, "256 bit value");
    CHECK(!fasm_set_feature_value_u64(sf, &value), "does not fit in 64 bits");
    CHECK(fasm_set_feature_value_bytes_le(sf, NULL, 0) == 32, "needs 32 bytes");
    memset(buf, 0x55, sizeof(buf));
    CHECK(fasm_set_feature_value_bytes_le(sf, buf, 31) == 32, "short buffer returns size");
    for (i = 0; i < sizeof(buf); i++) {
        CHECK(buf[i] == 0x55, "short buffer untouched at %zu", i);
    }
    CHECK(fasm_set_feature_value_bytes_le(sf, buf, sizeof(buf)) == 32, "fills buffer");
    memset(expected, 0, sizeof(expected));
    expected[0] = 0x01;
    expected[16] = 0xAB;
    expected[31] = 0x80;
    for (i = 0; i < sizeof(buf); i++) {
        CHECK(buf[i] == expected[i], "byte %zu: %02x != %02x", i, buf[i], expected[i]);
    }
    CHECK(fasm_set_feature_value_bit(sf, 255) && fasm_set_feature_value_bit(sf, 0) &&
              !fasm_set_feature_value_bit(sf, 1) && fasm_set_feature_value_bit(sf, 133),
          "wide bits");
    CHECK(take_eq(fasm_set_feature_to_string(sf, false, NULL),
                  "BRAM.INIT_00[255:0] = "
                  "256'h800000000000000000000000000000AB00000000000000000000000000000001"),
          "wide value printed");
    fasm_file_free(file);
}

static void test_errors(void) {
    fasm_file *file = (fasm_file *)1; /* must be overwritten with NULL */
    fasm_error *err = NULL;
    fasm_status status;
    const char *bad = "A.B\nA.B[3:0] = 5'h1F\n";
    const char *bad_utf8 = "A.B # \xff\xfe\n";

    /* Syntax / value error with a position. */
    status = fasm_parse_string(bad, strlen(bad), &file, &err);
    CHECK(status == FASM_ERR_PARSE, "parse error status %d", (int)status);
    CHECK(file == NULL, "out is NULL on error");
    REQUIRE(err != NULL, "error object");
    CHECK(fasm_error_status(err) == FASM_ERR_PARSE, "error status");
    CHECK(fasm_error_line(err) == 2, "error line %zu", fasm_error_line(err));
    CHECK(fasm_error_column(err) == 11, "error column %zu", fasm_error_column(err));
    CHECK(strncmp(fasm_error_message(err), "Parse error at 2:11 - ", 22) == 0,
          "error message: %s", fasm_error_message(err));
    fasm_error_free(err);
    err = NULL;

    status = fasm_parse_string("A.B[", 4, &file, &err);
    CHECK(status == FASM_ERR_PARSE && fasm_error_line(err) == 1 && fasm_error_column(err) == 4,
          "unterminated address at 1:%zu", fasm_error_column(err));
    fasm_error_free(err);

    /* Errors without an error object. */
    CHECK(fasm_parse_string(bad, strlen(bad), &file, NULL) == FASM_ERR_PARSE, "err NULL");

    /* Invalid UTF-8. */
    status = fasm_parse_string(bad_utf8, strlen(bad_utf8), &file, &err);
    CHECK(status == FASM_ERR_UTF8, "invalid UTF-8 status %d", (int)status);
    CHECK(fasm_error_status(err) == FASM_ERR_UTF8 && fasm_error_line(err) == 1,
          "invalid UTF-8 error");
    fasm_error_free(err);

    /* Missing file. */
    status = fasm_parse_file(repo_path("does/not/exist.fasm"), &file, &err);
    CHECK(status == FASM_ERR_IO, "missing file status %d", (int)status);
    CHECK(file == NULL, "no file");
    CHECK(fasm_error_status(err) == FASM_ERR_IO && fasm_error_line(err) == 0 &&
              fasm_error_column(err) == 0,
          "io error has no position");
    CHECK(strstr(fasm_error_message(err), "does/not/exist.fasm") != NULL, "io message: %s",
          fasm_error_message(err));
    fasm_error_free(err);

    /* Invalid arguments. */
    status = fasm_parse_string(NULL, 5, &file, &err);
    CHECK(status == FASM_ERR_INVALID_ARG && fasm_error_status(err) == FASM_ERR_INVALID_ARG,
          "NULL text with a length");
    fasm_error_free(err);
    CHECK(fasm_parse_string("A", 1, NULL, NULL) == FASM_ERR_INVALID_ARG, "NULL out");
    CHECK(fasm_parse_file(NULL, &file, NULL) == FASM_ERR_INVALID_ARG, "NULL path");
    CHECK(fasm_parse_file(repo_path("examples/many.fasm"), NULL, NULL) == FASM_ERR_INVALID_ARG,
          "parse file NULL out");
    CHECK(fasm_parse_string(NULL, 0, &file, NULL) == FASM_OK && fasm_file_line_count(file) == 0,
          "NULL text with length 0 is an empty file");
    CHECK(take_eq(fasm_file_to_string(file, false, NULL), "\n"), "empty file prints \\n");
    fasm_file_free(file);
}

/* Streaming callback state. */
struct stream_state {
    size_t calls;
    size_t stop_after;
    size_t features;
    size_t last_line_number;
    char first_name[64];
};

static bool on_line(const fasm_line *line, size_t line_number, void *user) {
    struct stream_state *state = user;
    const fasm_set_feature *sf = fasm_line_set_feature(line);
    state->calls++;
    state->last_line_number = line_number;
    if (sf != NULL) {
        if (state->features == 0) {
            fasm_set_feature_name(sf, state->first_name, sizeof(state->first_name));
        }
        state->features++;
    }
    return state->calls < state->stop_after;
}

static void test_streaming(void) {
    struct stream_state state;
    fasm_error *err = NULL;
    fasm_status status;
    const char *text = "A.B\n\n# comment\nC.D[1:0] = 2'b10\nE.F\n";
    const char *bad = "A.B\nC.D[\n";

    memset(&state, 0, sizeof(state));
    state.stop_after = (size_t)-1;
    status = fasm_parse_string_cb(text, strlen(text), on_line, &state, &err);
    CHECK(status == FASM_OK && err == NULL, "stream all");
    CHECK(state.calls == 4 && state.features == 3, "4 lines, 3 features (%zu, %zu)",
          state.calls, state.features);
    CHECK(state.last_line_number == 5, "last line number %zu", state.last_line_number);
    CHECK(strcmp(state.first_name, "A.B") == 0, "first name %s", state.first_name);

    memset(&state, 0, sizeof(state));
    state.stop_after = 2;
    status = fasm_parse_string_cb(text, strlen(text), on_line, &state, &err);
    CHECK(status == FASM_OK && state.calls == 2 && state.last_line_number == 3,
          "early stop after 2 lines");

    /* Stopping before a parse error hides the error. */
    memset(&state, 0, sizeof(state));
    state.stop_after = 1;
    CHECK(fasm_parse_string_cb(bad, strlen(bad), on_line, &state, NULL) == FASM_OK,
          "stop before the error");

    memset(&state, 0, sizeof(state));
    state.stop_after = (size_t)-1;
    status = fasm_parse_string_cb(bad, strlen(bad), on_line, &state, &err);
    CHECK(status == FASM_ERR_PARSE && state.calls == 1 && fasm_error_line(err) == 2,
          "lines before the error are delivered");
    fasm_error_free(err);

    memset(&state, 0, sizeof(state));
    state.stop_after = (size_t)-1;
    status = fasm_parse_file_cb(repo_path("examples/many.fasm"), on_line, &state, &err);
    CHECK(status == FASM_OK && state.calls == 40 && state.features == 19,
          "stream many.fasm (%zu lines, %zu features)", state.calls, state.features);
    CHECK(strcmp(state.first_name, "INT_L_X10Y146.SW6BEG0.WW2END0") == 0, "many first name");
    CHECK(state.last_line_number == 47, "many last line %zu", state.last_line_number);

    CHECK(fasm_parse_file_cb(repo_path("nope.fasm"), on_line, &state, NULL) == FASM_ERR_IO,
          "stream missing file");
    CHECK(fasm_parse_string_cb(text, strlen(text), NULL, NULL, &err) == FASM_ERR_INVALID_ARG,
          "NULL callback");
    fasm_error_free(err);
    CHECK(fasm_parse_file_cb(NULL, on_line, &state, NULL) == FASM_ERR_INVALID_ARG,
          "stream NULL path");
}

static bool zero_if_zero(const char *feature, size_t len, void *user) {
    int *calls = user;
    (*calls)++;
    return strlen(feature) == len && strstr(feature, "ZERO") != NULL;
}

static int64_t reverse_key(const char *group_id, size_t len, void *user) {
    (void)user;
    return len > 0 ? -(int64_t)(unsigned char)group_id[0] : 0;
}

/* Calls of counter_key: the group ids (one letter each) in call order. */
struct key_calls {
    int count;
    char order[64];
};

/* A non-deterministic key: minus the number of calls so far. */
static int64_t counter_key(const char *group_id, size_t len, void *user) {
    struct key_calls *calls = user;
    if (len == 1 && calls->count < (int)sizeof(calls->order)) {
        calls->order[calls->count] = group_id[0];
    }
    calls->count++;
    return -(int64_t)calls->count;
}

static void test_merge_and_sort_counter_key(void) {
    /* 26 groups A.F .. Z.F: the sort compares each several times. */
    char text[26 * 4 + 1];
    char expected[26 * 5 + 1];
    char *p = text;
    char *q = expected;
    struct key_calls calls;
    int i;
    fasm_file *file;
    fasm_file *merged;
    fasm_error *err = NULL;

    for (i = 0; i < 26; i++) {
        *p++ = (char)('A' + i);
        memcpy(p, ".F\n", 3);
        p += 3;
    }
    *p = '\0';
    memset(&calls, 0, sizeof(calls));
    file = parse(text);
    merged = fasm_file_merge_and_sort_ex(file, NULL, counter_key, &calls, &err);
    REQUIRE(merged != NULL, "a non-deterministic key must not fail: %s", fasm_error_message(err));
    REQUIRE(calls.count == 26, "sort key called once per group (%d calls)", calls.count);
    /* The group seen last got the smallest key, so it is printed first. */
    for (i = 25; i >= 0; i--) {
        *q++ = calls.order[i];
        memcpy(q, i > 0 ? ".F\n\n" : ".F\n", i > 0 ? 4 : 3);
        q += i > 0 ? 4 : 3;
    }
    *q = '\0';
    CHECK(take_eq(fasm_file_to_string(merged, false, NULL), expected), "counter key order");
    fasm_file_free(merged);
    fasm_file_free(file);
}

static void test_merge_and_sort(void) {
    fasm_file *file = parse("B.X[1]\n# about A\nA.Y\nB.X[0]\nC.ZERO\n");
    fasm_file *merged;
    fasm_error *err = NULL;
    int zero_calls = 0;

    merged = fasm_file_merge_and_sort(file, &err);
    REQUIRE(merged != NULL && err == NULL, "merge_and_sort");
    CHECK(fasm_file_line_count(merged) == 6, "merged lines %zu", fasm_file_line_count(merged));
    CHECK(take_eq(fasm_file_to_string(merged, false, NULL),
                  "# about A\nA.Y\n\nB.X[1:0] = 2'b11\n\nC.ZERO\n"),
          "merged output");
    fasm_file_free(merged);

    merged = fasm_file_merge_and_sort_ex(file, zero_if_zero, reverse_key, &zero_calls, &err);
    REQUIRE(merged != NULL, "merge_and_sort_ex");
    CHECK(zero_calls > 0, "zero function called");
    CHECK(take_eq(fasm_file_to_string(merged, false, NULL),
                  "B.X[1:0] = 2'b11\n\n# about A\nA.Y\n"),
          "merged output with callbacks");
    fasm_file_free(merged);

    merged = fasm_file_merge_and_sort_ex(file, NULL, NULL, NULL, NULL);
    CHECK(take_eq(fasm_file_to_string(merged, true, NULL), "A.Y\nB.X\nB.X[1]\nC.ZERO\n"),
          "ex without callbacks");
    fasm_file_free(merged);

    CHECK(fasm_file_merge_and_sort(NULL, &err) == NULL &&
              fasm_error_status(err) == FASM_ERR_INVALID_ARG,
          "merge NULL file");
    fasm_error_free(err);
    fasm_file_free(file);
}

/* A fasm_str view of a C string. */
static fasm_str S(const char *s) {
    fasm_str view;
    view.ptr = s;
    view.len = strlen(s);
    return view;
}

static fasm_set_feature_spec spec(const char *name) {
    fasm_set_feature_spec sf;
    memset(&sf, 0, sizeof(sf));
    sf.feature = S(name);
    sf.value_format = FASM_VALUE_FORMAT_NONE;
    return sf;
}

static void test_build(void) {
    fasm_file *file = fasm_file_new();
    fasm_error *err = NULL;
    fasm_set_feature_spec sf;
    fasm_annotation annotations[2];
    fasm_str comment = S(" built from C");
    fasm_str empty = S("");
    fasm_str bad_text;
    const uint8_t init[] = {0x2A, 0x01};
    const uint8_t one = 1;
    const uint8_t two = 2;
    uint8_t wide[32];
    fasm_status status;
    fasm_file *reparsed = NULL;
    fasm_string *out;

    REQUIRE(file != NULL, "fasm_file_new");
    CHECK(fasm_file_line_count(file) == 0, "new file is empty");

    /* X.Y.INIT[15:0] = 16'h12A { a = "b c", d = "" } # built from C */
    sf = spec("X.Y.INIT");
    sf.has_start = true;
    sf.start = 0;
    sf.has_end = true;
    sf.end = 15;
    sf.value_le = init;
    sf.value_len = sizeof(init);
    sf.value_format = FASM_VALUE_FORMAT_VERILOG_HEX;
    annotations[0].name = S("a");
    annotations[0].value = S("b c");
    annotations[1].name = S("d");
    annotations[1].value = S("");
    status = fasm_file_push_line(file, &sf, annotations, 2, &comment, &err);
    CHECK(status == FASM_OK && err == NULL, "push INIT: %s", fasm_error_message(err));

    /* X.Y.EN (implicit 1). */
    sf = spec("X.Y.EN");
    sf.value_le = &one;
    sf.value_len = 1;
    CHECK(fasm_file_push_line(file, &sf, NULL, 0, NULL, NULL) == FASM_OK, "push EN");

    /* A 256 bit value, decimal. */
    memset(wide, 0, sizeof(wide));
    wide[31] = 0x80;
    sf = spec("BRAM.INIT_01");
    sf.has_start = true;
    sf.start = 0;
    sf.has_end = true;
    sf.end = 255;
    sf.value_le = wide;
    sf.value_len = sizeof(wide);
    sf.value_format = FASM_VALUE_FORMAT_VERILOG_DECIMAL;
    CHECK(fasm_file_push_line(file, &sf, NULL, 0, NULL, NULL) == FASM_OK, "push wide");

    /* Bare '#', then a blank line, then an annotation only line. */
    CHECK(fasm_file_push_line(file, NULL, NULL, 0, &empty, NULL) == FASM_OK, "push #");
    CHECK(fasm_file_push_line(file, NULL, NULL, 0, NULL, NULL) == FASM_OK, "push blank");
    CHECK(fasm_file_push_line(file, NULL, annotations, 1, NULL, NULL) == FASM_OK,
          "push annotation");
    CHECK(fasm_file_line_count(file) == 6, "6 lines built");

    out = fasm_file_to_string(file, false, &err);
    REQUIRE(out != NULL, "print built file: %s", fasm_error_message(err));
    CHECK(strcmp(fasm_string_data(out),
                 "X.Y.INIT[15:0] = 16'h12A { a = \"b c\", d = \"\" } # built from C\n"
                 "X.Y.EN\n"
                 "BRAM.INIT_01[255:0] = 256'd578960446186580977117854925043439539266349923328202"
                 "82019728792003956564819968\n"
                 "#\n"
                 "\n"
                 "{ a = \"b c\" }\n") == 0,
          "built file text:\n%s", fasm_string_data(out));

    /* The printed text parses back to the same model. */
    CHECK(fasm_parse_string(fasm_string_data(out), fasm_string_len(out), &reparsed, NULL) ==
              FASM_OK,
          "reparse built file");
    CHECK(fasm_file_line_count(reparsed) == 5, "blank line dropped when parsing (%zu)",
          fasm_file_line_count(reparsed));
    CHECK(fasm_set_feature_value_bits(fasm_line_set_feature(fasm_file_line(reparsed, 2))) == 256,
          "wide value reparsed");
    fasm_file_free(reparsed);
    fasm_string_free(out);

    /* Validation errors append nothing. */
    sf = spec("X.Y.Z");
    sf.has_end = true;
    sf.end = 3;
    status = fasm_file_push_line(file, &sf, NULL, 0, NULL, &err);
    CHECK(status == FASM_ERR_INVALID_ARG, "end without start");
    CHECK(strstr(fasm_error_message(err), "without a start") != NULL, "message: %s",
          fasm_error_message(err));
    fasm_error_free(err);

    sf = spec("X.Y.Z");
    sf.has_start = true;
    sf.start = 4;
    sf.has_end = true;
    sf.end = 3;
    CHECK(fasm_file_push_line(file, &sf, NULL, 0, NULL, NULL) == FASM_ERR_INVALID_ARG,
          "end before start");

    sf = spec("X.Y.Z");
    sf.value_le = &two;
    sf.value_len = 1;
    CHECK(fasm_file_push_line(file, &sf, NULL, 0, NULL, NULL) == FASM_ERR_INVALID_ARG,
          "value too wide");

    sf = spec("X.Y.Z");
    sf.value_format = 7;
    CHECK(fasm_file_push_line(file, &sf, NULL, 0, NULL, NULL) == FASM_ERR_INVALID_ARG,
          "bad value format");

    sf = spec("X.Y.Z");
    sf.value_le = NULL;
    sf.value_len = 3;
    CHECK(fasm_file_push_line(file, &sf, NULL, 0, NULL, NULL) == FASM_ERR_INVALID_ARG,
          "NULL value with a length");

    sf = spec("X.\xff");
    CHECK(fasm_file_push_line(file, &sf, NULL, 0, NULL, NULL) == FASM_ERR_UTF8, "bad UTF-8 name");

    bad_text.ptr = "\xc3";
    bad_text.len = 1;
    CHECK(fasm_file_push_line(file, NULL, NULL, 0, &bad_text, NULL) == FASM_ERR_UTF8,
          "bad UTF-8 comment");
    annotations[1].value = bad_text;
    CHECK(fasm_file_push_line(file, NULL, annotations, 2, NULL, NULL) == FASM_ERR_UTF8,
          "bad UTF-8 annotation");
    CHECK(fasm_file_push_line(file, NULL, NULL, 1, NULL, NULL) == FASM_ERR_INVALID_ARG,
          "NULL annotations with a count");
    CHECK(fasm_file_push_line(NULL, NULL, NULL, 0, NULL, &err) == FASM_ERR_INVALID_ARG,
          "NULL file");
    fasm_error_free(err);

    CHECK(fasm_file_line_count(file) == 6, "failed pushes append nothing");
    fasm_file_free(file);
}

/* The struct layout seen by C matches the Rust side (struct_layout in
 * rust/fasm-capi/src/tests.rs asserts the same formulas). */
static void test_layout(void) {
    const size_t P = sizeof(void *);
    const size_t S = 2 * P;
    size_t spec_size = S + 20 + 2 * P;
    spec_size = (spec_size + P - 1) / P * P;
    CHECK(sizeof(fasm_str) == S && offsetof(fasm_str, len) == P, "fasm_str layout");
    CHECK(sizeof(fasm_annotation) == 2 * S && offsetof(fasm_annotation, value) == S,
          "fasm_annotation layout");
    CHECK(offsetof(fasm_set_feature_spec, feature) == 0, "spec.feature");
    CHECK(offsetof(fasm_set_feature_spec, has_start) == S, "spec.has_start");
    CHECK(offsetof(fasm_set_feature_spec, start) == S + 4, "spec.start");
    CHECK(offsetof(fasm_set_feature_spec, has_end) == S + 8, "spec.has_end");
    CHECK(offsetof(fasm_set_feature_spec, end) == S + 12, "spec.end");
    CHECK(offsetof(fasm_set_feature_spec, value_le) == S + 16, "spec.value_le");
    CHECK(offsetof(fasm_set_feature_spec, value_len) == S + 16 + P, "spec.value_len");
    CHECK(offsetof(fasm_set_feature_spec, value_format) == S + 16 + 2 * P,
          "spec.value_format");
    CHECK(sizeof(fasm_set_feature_spec) == spec_size, "sizeof(fasm_set_feature_spec) %zu",
          sizeof(fasm_set_feature_spec));
    CHECK(sizeof(fasm_status) == 4 && sizeof(fasm_value_format) == 4, "enum sizes");
}

static void test_null_handling(void) {
    fasm_str view;
    fasm_annotation annotation;
    uint64_t value;
    char buf[4] = {'x', 'x', 'x', 'x'};

    CHECK(fasm_file_line_count(NULL) == 0, "line count NULL");
    CHECK(fasm_file_line(NULL, 0) == NULL, "line NULL");
    fasm_file_free(NULL);

    CHECK(!fasm_line_has_set_feature(NULL), "has set feature NULL");
    CHECK(fasm_line_set_feature(NULL) == NULL, "set feature NULL");
    CHECK(fasm_line_annotation_count(NULL) == 0, "annotation count NULL");
    CHECK(!fasm_line_annotation(NULL, 0, &annotation), "annotation NULL");
    CHECK(!fasm_line_has_comment(NULL), "has comment NULL");
    CHECK(!fasm_line_comment(NULL, &view) && view.len == 0, "comment NULL");

    CHECK(fasm_set_feature_name_len(NULL) == 0, "name len NULL");
    CHECK(fasm_set_feature_name(NULL, buf, sizeof(buf)) == 0 && buf[0] == '\0', "name NULL");
    CHECK(fasm_set_feature_name(NULL, NULL, 0) == 0, "name NULL buffer");
    CHECK(fasm_set_feature_name_string(NULL) == NULL, "name string NULL");
    CHECK(!fasm_set_feature_has_start(NULL) && fasm_set_feature_start(NULL) == 0, "start NULL");
    CHECK(!fasm_set_feature_has_end(NULL) && fasm_set_feature_end(NULL) == 0, "end NULL");
    CHECK(fasm_set_feature_value_format(NULL) == FASM_VALUE_FORMAT_NONE, "format NULL");
    CHECK(fasm_set_feature_width(NULL) == 0, "width NULL");
    CHECK(fasm_set_feature_value_bits(NULL) == 0, "bits NULL");
    CHECK(!fasm_set_feature_value_u64(NULL, &value), "u64 NULL");
    CHECK(!fasm_set_feature_value_bit(NULL, 0), "bit NULL");
    CHECK(fasm_set_feature_value_bytes_le(NULL, NULL, 0) == 0, "bytes NULL");
    CHECK(fasm_set_feature_value_to_string(NULL, 10, false, NULL) == NULL, "value str NULL");
    CHECK(fasm_set_feature_to_string(NULL, false, NULL) == NULL, "sf to string NULL");
    CHECK(fasm_line_to_string(NULL, false, NULL) == NULL, "line to string NULL");
    CHECK(fasm_file_to_string(NULL, false, NULL) == NULL, "file to string NULL");
    CHECK(fasm_file_merge_and_sort(NULL, NULL) == NULL, "merge NULL");

    CHECK(fasm_string_len(NULL) == 0, "string len NULL");
    CHECK(fasm_string_data(NULL) != NULL && fasm_string_data(NULL)[0] == '\0', "data NULL");
    fasm_string_free(NULL);

    CHECK(fasm_error_status(NULL) == FASM_ERR_INVALID_ARG, "error status NULL");
    CHECK(fasm_error_message(NULL) != NULL && fasm_error_message(NULL)[0] == '\0',
          "error message NULL");
    CHECK(fasm_error_line(NULL) == 0 && fasm_error_column(NULL) == 0, "error pos NULL");
    fasm_error_free(NULL);

    /* NULL out-parameters of value accessors on a real feature. */
    {
        fasm_file *file = parse("A.B = 1\n");
        const fasm_set_feature *sf = fasm_line_set_feature(fasm_file_line(file, 0));
        fasm_error *err = NULL;
        CHECK(!fasm_set_feature_value_u64(sf, NULL), "u64 NULL out");
        CHECK(fasm_set_feature_value_to_string(sf, 7, false, &err) == NULL &&
                  fasm_error_status(err) == FASM_ERR_INVALID_ARG,
              "bad radix");
        fasm_error_free(err);
        fasm_file_free(file);
    }
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s REPO_ROOT\n", argv[0]);
        return 2;
    }
    repo_root = argv[1];
    printf("libfasm %s\n", fasm_version());

    test_version_and_status();
    test_parse_file_and_output();
    test_parse_string_many();
    test_wide_value();
    test_errors();
    test_streaming();
    test_merge_and_sort();
    test_merge_and_sort_counter_key();
    test_build();
    test_null_handling();
    test_layout();

    printf("%d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}

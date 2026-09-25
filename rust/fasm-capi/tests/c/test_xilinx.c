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
 * Test program of the fasm_xilinx_* functions of the C API
 * (include/fasm/fasm.h), plain C99 (plus POSIX system() for the command
 * line tools).
 *
 * Usage: test_xilinx REPO_ROOT CLI_DIR WORK_DIR
 *
 * Assembles the FASM files of tests/corpus/f4pga-xc-fasm on the mini
 * database and small designs on the synthetic Series7 and UltraScale+
 * databases of rust/fasm-xilinx/testdata, writes and reads back
 * bitstreams, and compares every .frm and .bit byte for byte with what the
 * Rust command line tools in CLI_DIR (fasm2frames, xc7frames2bit,
 * xcframes2bit, bitread, uray-bitread, xcfasm; skipped when they are not
 * there) write into WORK_DIR, and the error messages with theirs. With
 * FASM_DB_CACHE set to a directory holding prjxray-db/artix7, also the
 * counter_test design of tests/corpus/xilinx on xc7a35tcsg324-1. Every
 * object is freed (a leak test under valgrind).
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

#define CHECK(cond, ...) check_impl((cond) ? 1 : 0, __FILE__, __LINE__, #cond, __VA_ARGS__)

#define REQUIRE(cond, ...)                                          \
    do {                                                            \
        if (!CHECK(cond, __VA_ARGS__)) {                            \
            fprintf(stderr, "stopping after a failed REQUIRE\n");   \
            exit(1);                                                \
        }                                                           \
    } while (0)

/* Checks that a call succeeded, printing (and freeing) its error. */
#define CHECK_OK(status, err)                                                        \
    do {                                                                             \
        fasm_status check_ok_status = (status);                                      \
        CHECK(check_ok_status == FASM_OK, "status %d: %s", (int)check_ok_status,     \
              (err) != NULL ? fasm_error_message(err) : "(no error)");               \
        fasm_error_free(err);                                                        \
        (err) = NULL;                                                                \
    } while (0)

#define SOURCE_DATE_EPOCH 1700000000

static const char *repo_root = ".";
static const char *cli_dir = NULL;
static const char *work_dir = ".";
static int have_cli = 0;

/* A path under the repository root. */
static const char *repo(const char *rel) {
    static char bufs[8][4096];
    static int next = 0;
    char *buf = bufs[next++ % 8];
    snprintf(buf, sizeof bufs[0], "%s/%s", repo_root, rel);
    return buf;
}

/* A path under the work directory. */
static const char *work(const char *name) {
    static char bufs[8][4096];
    static int next = 0;
    char *buf = bufs[next++ % 8];
    snprintf(buf, sizeof bufs[0], "%s/%s", work_dir, name);
    return buf;
}

typedef struct {
    char *data;
    size_t len;
} buffer;

/* Reads a whole file (data is NULL if it cannot be read). */
static buffer read_file(const char *path) {
    buffer b = {NULL, 0};
    FILE *f = fopen(path, "rb");
    size_t cap = 0;
    if (f == NULL) {
        return b;
    }
    for (;;) {
        size_t n;
        if (b.len == cap) {
            char *grown;
            cap = cap == 0 ? 65536 : cap * 2;
            grown = (char *)realloc(b.data, cap + 1);
            REQUIRE(grown != NULL, "out of memory");
            b.data = grown;
        }
        n = fread(b.data + b.len, 1, cap - b.len, f);
        b.len += n;
        if (n == 0) {
            break;
        }
    }
    fclose(f);
    if (b.data == NULL) {
        b.data = (char *)malloc(1);
    }
    b.data[b.len] = '\0';
    return b;
}

static void write_file(const char *path, const char *text) {
    FILE *f = fopen(path, "wb");
    REQUIRE(f != NULL, "cannot write %s", path);
    fputs(text, f);
    fclose(f);
}

static int same_bytes(const void *a, size_t alen, const void *b, size_t blen) {
    return alen == blen && (alen == 0 || memcmp(a, b, alen) == 0);
}

/* Runs a command line tool of CLI_DIR with the shell (the arguments are
 * quoted by the caller), with SOURCE_DATE_EPOCH set and the database
 * cache off; its stderr goes to WORK_DIR/cli.err. Returns the exit
 * status. */
PRINTF_LIKE(2, 3)
static int run_cli(const char *tool, const char *fmt, ...) {
    char args[8192];
    char command[12288];
    va_list ap;
    int status;
    va_start(ap, fmt);
    vsnprintf(args, sizeof args, fmt, ap);
    va_end(ap);
    snprintf(command, sizeof command,
             "SOURCE_DATE_EPOCH=%d FASM_XDB_CACHE=0 '%s/%s' %s 2>'%s'", SOURCE_DATE_EPOCH,
             cli_dir, tool, args, work("cli.err"));
    status = system(command);
    return status;
}

/* The `.frm` text of `frames`. */
static char *frm_text(const fasm_xilinx_frames *frames, size_t *len) {
    fasm_string *s = NULL;
    fasm_error *err = NULL;
    char *copy;
    CHECK_OK(fasm_xilinx_frames_to_frm(frames, &s, &err), err);
    *len = fasm_string_len(s);
    copy = (char *)malloc(*len + 1);
    REQUIRE(copy != NULL, "out of memory");
    memcpy(copy, fasm_string_data(s), *len + 1);
    fasm_string_free(s);
    return copy;
}

/* Checks that `frames` as .frm text is byte for byte the file `path`. */
static void check_frm_file(const fasm_xilinx_frames *frames, const char *path,
                           const char *what) {
    size_t len;
    char *text = frm_text(frames, &len);
    buffer expected = read_file(path);
    CHECK(expected.data != NULL, "%s: cannot read %s", what, path);
    CHECK(same_bytes(text, len, expected.data, expected.len),
          "%s: .frm differs from %s (%zu vs %zu bytes)", what, path, len, expected.len);
    free(text);
    free(expected.data);
}

static fasm_xilinx_database *open_db(const char *rel_root, const char *part) {
    fasm_xilinx_database *db = NULL;
    fasm_error *err = NULL;
    fasm_status status = fasm_xilinx_database_open(repo(rel_root), part, &db, &err);
    CHECK_OK(status, err);
    REQUIRE(db != NULL, "cannot open %s", rel_root);
    return db;
}

static const char *const MINI_FIXTURES[] = {
    "lut.fasm",     "ff_int.fasm",    "ff_int_0s.fasm",         "ff_int_op1.fasm",
    "lut_int.fasm", "iob/liob_stepdown.fasm", "iob/riob_stepdown.fasm",
};

#define MINI_DB "rust/fasm-xilinx/testdata/mini-db"
#define SYNTHETIC_DB "rust/fasm-xilinx/testdata/synthetic-db"
#define USP_DB "rust/fasm-xilinx/testdata/synthetic-usp-db"

static const char *const SYNTHETIC_DESIGN =
    "INT_L_X6Y0.WW2BEG0.LOGIC_OUTS_L12\n"
    "BRAM_L_X6Y0.RAMB18_Y0.INIT_00[4:0] = 5'b10011\n"
    "BRAM_L_X6Y0.RAMB18_Y0.IN_USE\n"
    "LIOB33_X0Y1.IOB_Y0.PULL\n";

static const char *const USP_DESIGN =
    "CLEM_X1Y0.ALUT.INIT[15:0] = 16'hA5C3\n"
    "CLEM_X1Y1.ABCDFF.CEUSED.V1\n"
    "BRAM_X2Y0.RAMB18E2_L.INIT_00[7:0] = 8'hFF\n"
    "RCLK_INT_L_X2Y29.BUFCE_LEAF_X0Y0.BUFCE_LEAF.DELAY_TAP.V0\n"
    "EDGE_X0Y0.OK\n";

static void test_database(void) {
    fasm_xilinx_database *db = open_db(MINI_DB, "xc7");
    fasm_xilinx_database *no_part = NULL;
    fasm_xilinx_database *bad = NULL;
    fasm_xilinx_assembler *assembler = NULL;
    fasm_error *err = NULL;
    fasm_status status;

    CHECK(fasm_xilinx_database_architecture(db) == FASM_XILINX_SERIES7, "architecture");
    CHECK(fasm_xilinx_database_words_per_frame(db) == 101, "words per frame");
    CHECK(strcmp(fasm_xilinx_database_part(db), "xc7") == 0, "part");

    status = fasm_xilinx_database_open(repo(MINI_DB), "nope", &bad, &err);
    CHECK(status == FASM_ERR_DB && bad == NULL, "unknown part: %d", (int)status);
    CHECK(strcmp(fasm_error_kind(err), "fasm_xilinx.DbError") == 0, "kind %s",
          fasm_error_kind(err));
    CHECK(strstr(fasm_error_message(err), "part \"nope\" not found") != NULL, "message %s",
          fasm_error_message(err));
    fasm_error_free(err);
    err = NULL;

    CHECK_OK(fasm_xilinx_database_open(repo(MINI_DB), NULL, &no_part, &err), err);
    CHECK(fasm_xilinx_database_part(no_part) == NULL, "no part");
    status = fasm_xilinx_assembler_new(no_part, &assembler, &err);
    CHECK(status == FASM_ERR_DB && assembler == NULL, "assembler without part");
    fasm_error_free(err);
    err = NULL;
    fasm_xilinx_database_free(no_part);

    /* The cache gives the same database (built, then loaded). */
    {
        int i;
        fasm_xilinx_frames *reference = NULL;
        CHECK_OK(fasm_xilinx_fasm2frames_file(
                     db, repo("tests/corpus/f4pga-xc-fasm/lut_int.fasm"), NULL, &reference, &err),
                 err);
        for (i = 0; i < 2; i++) {
            fasm_xilinx_database *cached = NULL;
            fasm_xilinx_frames *frames = NULL;
            CHECK_OK(fasm_xilinx_database_open_cached(repo(MINI_DB), "xc7", work("cache"),
                                                      &cached, &err),
                     err);
            CHECK_OK(fasm_xilinx_fasm2frames_file(
                         cached, repo("tests/corpus/f4pga-xc-fasm/lut_int.fasm"), NULL,
                         &frames, &err),
                     err);
            CHECK(fasm_xilinx_frames_equal(frames, reference), "cached database, pass %d", i);
            fasm_xilinx_frames_free(frames);
            fasm_xilinx_database_free(cached);
        }
        fasm_xilinx_frames_free(reference);
    }
    fasm_xilinx_database_free(db);
}

static void test_lookup(void) {
    fasm_xilinx_database *db = open_db(SYNTHETIC_DB, "xc7test-1");
    fasm_xilinx_feature_info info;
    fasm_xilinx_bit bits[8];
    fasm_error *err = NULL;
    fasm_status status;
    const char *imux = "INT_L_X6Y0.IMUX_L1.EE2END0";
    const char *ppip = "INT_L_X6Y0.BYP_ALT0.VCC_WIRE";
    const char *nope = "NOPE_X0Y0.A";
    const char *missing = "INT_L_X6Y0.NOPE";
    size_t i, set = 0;

    CHECK_OK(fasm_xilinx_database_lookup(db, imux, strlen(imux), 0, &info, NULL, 0, &err), err);
    CHECK(info.bit_count == 5 && !info.pseudo_pip && info.block_type == 0, "imux info");
    CHECK_OK(fasm_xilinx_database_lookup(db, imux, strlen(imux), 0, &info, bits, 8, &err), err);
    for (i = 0; i < info.bit_count; i++) {
        set += bits[i].value ? 1 : 0;
        CHECK(bits[i].word < 101 && bits[i].bit < 32, "bit position");
    }
    CHECK(set == 2, "2 set and 3 cleared bits: %zu", set);

    CHECK_OK(fasm_xilinx_database_lookup(db, ppip, strlen(ppip), 0, &info, NULL, 0, &err), err);
    CHECK(info.pseudo_pip && info.bit_count == 0 && info.block_type == -1, "pseudo pip");

    status = fasm_xilinx_database_lookup(db, nope, strlen(nope), 0, &info, NULL, 0, &err);
    CHECK(status == FASM_ERR_ASSEMBLER, "unknown tile: %d", (int)status);
    CHECK(strcmp(fasm_error_kind(err), "KeyError") == 0, "kind %s", fasm_error_kind(err));
    CHECK(strcmp(fasm_error_message(err), "'NOPE_X0Y0'") == 0, "message %s",
          fasm_error_message(err));
    fasm_error_free(err);
    err = NULL;

    status = fasm_xilinx_database_lookup(db, missing, strlen(missing), 3, &info, NULL, 0, &err);
    CHECK(status == FASM_ERR_LOOKUP, "unknown feature: %d", (int)status);
    CHECK(strcmp(fasm_error_message(err), "Segment DB INT_L, key INT_L.NOPE[3] not found") == 0,
          "message %s", fasm_error_message(err));
    fasm_error_free(err);
    err = NULL;

    status = fasm_xilinx_database_lookup(db, imux, strlen(imux), 0, NULL, NULL, 0, &err);
    CHECK(status == FASM_ERR_INVALID_ARG, "NULL info");
    fasm_error_free(err);
    fasm_xilinx_database_free(db);
}

/* The mini database: fasm2frames vs the assembler step by step, the .frm
 * round trips, and the fasm2frames tool. */
static void test_mini_db(void) {
    fasm_xilinx_database *db = open_db(MINI_DB, "xc7");
    size_t i;
    int sparse;
    fasm_error *err = NULL;

    for (i = 0; i < sizeof MINI_FIXTURES / sizeof MINI_FIXTURES[0]; i++) {
        char rel[256];
        const char *path;
        snprintf(rel, sizeof rel, "tests/corpus/f4pga-xc-fasm/%s", MINI_FIXTURES[i]);
        path = repo(rel);
        for (sparse = 0; sparse <= 1; sparse++) {
            fasm_xilinx_fasm2frames_options options;
            fasm_xilinx_frames *frames = NULL, *step = NULL, *from_file = NULL, *back = NULL;
            fasm_xilinx_assembler *assembler = NULL;
            fasm_file *file = NULL;
            size_t len;
            char *text;

            memset(&options, 0, sizeof options);
            options.sparse = sparse != 0;
            CHECK_OK(fasm_xilinx_fasm2frames_file(db, path, &options, &frames, &err), err);
            REQUIRE(frames != NULL, "%s", path);

            /* The assembler step by step, from the file and from a model. */
            CHECK_OK(fasm_xilinx_assembler_new(db, &assembler, &err), err);
            CHECK_OK(fasm_xilinx_assembler_parse_file(assembler, path, &err), err);
            CHECK_OK(fasm_xilinx_assembler_add_required_features(assembler, &err), err);
            CHECK_OK(fasm_xilinx_assembler_propagate_stepdown(assembler, &err), err);
            CHECK_OK(fasm_xilinx_assembler_get_frames(assembler, sparse != 0, &step, &err), err);
            CHECK(fasm_xilinx_frames_equal(frames, step), "%s: assembler", path);
            CHECK(fasm_xilinx_assembler_warning_count(assembler) == 0, "no warnings");
            fasm_xilinx_assembler_free(assembler);
            fasm_xilinx_frames_free(step);
            step = NULL;

            CHECK_OK(fasm_parse_file(path, &file, &err), err);
            CHECK_OK(fasm_xilinx_assembler_new(db, &assembler, &err), err);
            CHECK_OK(fasm_xilinx_assembler_add_file(assembler, file, &err), err);
            CHECK_OK(fasm_xilinx_assembler_propagate_stepdown(assembler, &err), err);
            CHECK_OK(fasm_xilinx_assembler_get_frames(assembler, sparse != 0, &step, &err), err);
            CHECK(fasm_xilinx_frames_equal(frames, step), "%s: add_file", path);
            fasm_xilinx_assembler_free(assembler);
            fasm_xilinx_frames_free(step);
            fasm_file_free(file);

            /* .frm write/read/parse round trips. */
            CHECK_OK(fasm_xilinx_frames_write_frm(frames, work("c.frm"), &err), err);
            check_frm_file(frames, work("c.frm"), path);
            CHECK_OK(fasm_xilinx_frames_read_frm(work("c.frm"), 101, NULL, NULL, &from_file, &err),
                     err);
            CHECK(fasm_xilinx_frames_equal(frames, from_file), "%s: read_frm", path);
            text = frm_text(frames, &len);
            CHECK_OK(fasm_xilinx_frames_parse_frm(text, len, 101, NULL, NULL, &back, &err), err);
            CHECK(fasm_xilinx_frames_equal(frames, back), "%s: parse_frm", path);
            free(text);
            fasm_xilinx_frames_free(from_file);
            fasm_xilinx_frames_free(back);

            if (have_cli) {
                int status = run_cli("fasm2frames", "--db-root '%s' --part xc7 %s '%s' '%s'",
                                     repo(MINI_DB), sparse ? "--sparse" : "", path,
                                     work("cli.frm"));
                CHECK(status == 0, "fasm2frames %s: %d", path, status);
                check_frm_file(frames, work("cli.frm"), path);
            }
            fasm_xilinx_frames_free(frames);
        }
    }
    fasm_xilinx_database_free(db);
}

/* Errors have the status, kind and message the tools print. */
static void test_errors(void) {
    static const struct {
        const char *text;
        fasm_status status;
        const char *kind;
    } cases[] = {
        {"CLBLM_L_X10Y102.SLICEM_X0.NOPE\nCLBLM_L_X10Y102.X[3:2] = 3\n", FASM_ERR_LOOKUP,
         "prjxray.fasm_assembler.FasmLookupError"},
        {"NOPE_X1Y1.A\n", FASM_ERR_ASSEMBLER, "KeyError"},
        {"CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI\nA B\n", FASM_ERR_PARSE, "Exception"},
    };
    fasm_xilinx_database *db = open_db(MINI_DB, "xc7");
    size_t i;
    for (i = 0; i < sizeof cases / sizeof cases[0]; i++) {
        fasm_xilinx_frames *frames = NULL;
        fasm_error *err = NULL;
        fasm_status status;
        write_file(work("error.fasm"), cases[i].text);
        status = fasm_xilinx_fasm2frames_file(db, work("error.fasm"), NULL, &frames, &err);
        CHECK(status == cases[i].status && frames == NULL, "case %zu: status %d", i,
              (int)status);
        CHECK(strcmp(fasm_error_kind(err), cases[i].kind) == 0, "case %zu: kind %s", i,
              fasm_error_kind(err));
        if (have_cli) {
            char expected[4096];
            buffer printed;
            int code = run_cli("fasm2frames", "--db-root '%s' --part xc7 '%s' '%s'",
                               repo(MINI_DB), work("error.fasm"), work("cli.frm"));
            CHECK(code != 0, "case %zu: the tool fails", i);
            snprintf(expected, sizeof expected, "%s: %s\n", fasm_error_kind(err),
                     fasm_error_message(err));
            printed = read_file(work("cli.err"));
            CHECK(printed.data != NULL && strcmp(printed.data, expected) == 0,
                  "case %zu: '%s' vs '%s'", i, printed.data, expected);
            free(printed.data);
        }
        fasm_error_free(err);
    }
    fasm_xilinx_database_free(db);
}

/* Writes the bitstream of `frames` for `part` with the options
 * xc7frames2bit would use for the .frm file `frm_path`. */
static fasm_bytes *write_bit(const fasm_xilinx_part *part, const fasm_xilinx_frames *frames,
                             int32_t format, const char *part_name, const char *frm_path) {
    fasm_xilinx_bitstream_options options;
    fasm_bytes *bit = NULL;
    fasm_error *err = NULL;
    memset(&options, 0, sizeof options);
    options.format = format;
    options.part_name = part_name;
    options.design_name = frm_path;
    options.has_source_date_epoch = true;
    options.source_date_epoch = SOURCE_DATE_EPOCH;
    CHECK_OK(fasm_xilinx_bitstream_write(part, frames, &options, &bit, &err), err);
    return bit;
}

static void check_bit_file(const fasm_bytes *bit, const char *path, const char *what) {
    buffer expected = read_file(path);
    CHECK(expected.data != NULL, "%s: cannot read %s", what, path);
    CHECK(same_bytes(fasm_bytes_data(bit), fasm_bytes_len(bit), expected.data, expected.len),
          "%s: .bit differs from %s (%zu vs %zu bytes)", what, path, fasm_bytes_len(bit),
          expected.len);
    free(expected.data);
}

/* A design on the synthetic Series7 or UltraScale+ database: .frm vs
 * fasm2frames, .bit vs xc7frames2bit / xcframes2bit, reading back vs
 * bitread / uray-bitread. */
static void test_bitstreams(const char *rel_db, const char *part_name, const char *design,
                            const char *frames2bit, const char *bitread,
                            const char *architecture) {
    fasm_xilinx_database *db = open_db(rel_db, part_name);
    fasm_xilinx_part *part = NULL, *yaml_part = NULL;
    fasm_error *err = NULL;
    char rel_yaml[512];
    const char *yaml;
    int sparse;
    int is_usp = fasm_xilinx_database_architecture(db) == FASM_XILINX_ULTRASCALE_PLUS;

    snprintf(rel_yaml, sizeof rel_yaml, "%s/%s/part.yaml", rel_db, part_name);
    yaml = repo(rel_yaml);
    CHECK_OK(fasm_xilinx_part_from_database(db, &part, &err), err);
    CHECK_OK(fasm_xilinx_part_read_yaml(yaml, FASM_XILINX_SERIES7, &yaml_part, &err), err);
    CHECK(fasm_xilinx_part_architecture(part) == fasm_xilinx_part_architecture(yaml_part),
          "part architecture");
    write_file(work("design.fasm"), design);

    for (sparse = 0; sparse <= 1; sparse++) {
        fasm_xilinx_fasm2frames_options options;
        fasm_xilinx_frames *frames = NULL, *from_text = NULL, *back = NULL;
        fasm_bytes *bit = NULL, *bit2 = NULL;
        memset(&options, 0, sizeof options);
        options.sparse = sparse != 0;
        CHECK_OK(fasm_xilinx_fasm2frames_file(db, work("design.fasm"), &options, &frames, &err),
                 err);
        REQUIRE(frames != NULL, "%s", rel_db);
        CHECK_OK(fasm_xilinx_fasm2frames_string(db, design, strlen(design), &options,
                                                &from_text, &err),
                 err);
        CHECK(fasm_xilinx_frames_equal(frames, from_text), "%s: string input", rel_db);
        CHECK(fasm_xilinx_frames_words_per_frame(frames) ==
                  fasm_xilinx_database_words_per_frame(db),
              "words per frame");
        CHECK_OK(fasm_xilinx_frames_write_frm(frames, work("design.frm"), &err), err);

        bit = write_bit(part, frames, FASM_XILINX_FORMAT_DEFAULT, part_name, work("design.frm"));
        bit2 = write_bit(yaml_part, frames,
                         is_usp ? FASM_XILINX_FORMAT_ULTRASCALE_PLUS : FASM_XILINX_FORMAT_SERIES7,
                         part_name, work("design.frm"));
        CHECK(same_bytes(fasm_bytes_data(bit), fasm_bytes_len(bit), fasm_bytes_data(bit2),
                         fasm_bytes_len(bit2)),
              "%s: the part from the database and part.yaml", rel_db);
        CHECK_OK(fasm_xilinx_bitstream_write_file(part, frames, NULL, work("c.bit"), &err), err);
        CHECK_OK(fasm_xilinx_bitstream_read(part, fasm_bytes_data(bit), fasm_bytes_len(bit),
                                            FASM_XILINX_FORMAT_DEFAULT, true, false, &back, &err),
                 err);
        CHECK(fasm_xilinx_frames_count(back) > 0, "frames read back");
        if (is_usp) {
            /* Every frame written comes back. */
            size_t i;
            for (i = 0; i < fasm_xilinx_frames_count(frames); i++) {
                const uint32_t *words = fasm_xilinx_frames_find(
                    back, fasm_xilinx_frames_address(frames, i));
                CHECK(words != NULL && memcmp(words, fasm_xilinx_frames_words(frames, i),
                                              93 * sizeof(uint32_t)) == 0,
                      "frame %zu read back", i);
            }
        }
        if (have_cli) {
            int code = run_cli("fasm2frames", "--db-root '%s' --part %s %s '%s' '%s'",
                               repo(rel_db), part_name, sparse ? "--sparse" : "",
                               work("design.fasm"), work("cli.frm"));
            CHECK(code == 0, "fasm2frames: %d", code);
            check_frm_file(frames, work("cli.frm"), rel_db);
            code = run_cli(frames2bit,
                           "--architecture=%s --part_file='%s' --part_name=%s --frm_file='%s' "
                           "--output_file='%s'",
                           architecture, yaml, part_name, work("design.frm"), work("cli.bit"));
            CHECK(code == 0, "%s: %d", frames2bit, code);
            check_bit_file(bit, work("cli.bit"), rel_db);
            code = run_cli(bitread, "--architecture=%s --part_file='%s' --frm_out='%s' '%s' >/dev/null",
                           architecture, yaml, work("cli-back.frm"), work("cli.bit"));
            CHECK(code == 0, "%s: %d", bitread, code);
            check_frm_file(back, work("cli-back.frm"), rel_db);
        }
        fasm_xilinx_frames_free(back);
        back = NULL;
        CHECK_OK(fasm_xilinx_bitstream_read_file(part, work("c.bit"), FASM_XILINX_FORMAT_DEFAULT,
                                                 true, false, &back, &err),
                 err);
        CHECK(fasm_xilinx_frames_count(back) > 0, "frames read back from the file");
        fasm_bytes_free(bit);
        fasm_bytes_free(bit2);
        fasm_xilinx_frames_free(frames);
        fasm_xilinx_frames_free(from_text);
        fasm_xilinx_frames_free(back);
    }

    /* A bitstream for another architecture's part fails. */
    {
        fasm_xilinx_frames *frames = fasm_xilinx_frames_new(is_usp ? 101 : 93);
        fasm_bytes *bit = NULL;
        fasm_xilinx_frames *back = NULL;
        fasm_status status = fasm_xilinx_bitstream_write(part, frames, NULL, &bit, &err);
        CHECK(status == FASM_ERR_BITSTREAM && bit == NULL, "wrong frame size: %d", (int)status);
        CHECK(strcmp(fasm_error_kind(err), "fasm_xilinx.BitstreamError") == 0, "kind");
        fasm_error_free(err);
        err = NULL;
        status = fasm_xilinx_bitstream_read(part, (const uint8_t *)"no sync", 7,
                                            FASM_XILINX_FORMAT_DEFAULT, true, false, &back, &err);
        CHECK(status == FASM_ERR_BITSTREAM && back == NULL, "not a bitstream: %d", (int)status);
        CHECK(strcmp(fasm_error_message(err), "Input doesn't look like a bitstream") == 0,
              "message %s", fasm_error_message(err));
        fasm_error_free(err);
        err = NULL;
        status = fasm_xilinx_bitstream_read(part, NULL, 0, 42, true, false, &back, &err);
        CHECK(status == FASM_ERR_INVALID_ARG, "bad format");
        fasm_error_free(err);
        err = NULL;
        fasm_xilinx_frames_free(frames);
    }
    fasm_xilinx_part_free(part);
    fasm_xilinx_part_free(yaml_part);
    fasm_xilinx_database_free(db);
}

static int warning_count = 0;

static void count_warning(const char *message, size_t len, void *user) {
    (void)user;
    CHECK(strlen(message) == len, "NUL terminated warning");
    warning_count++;
}

static void test_frames(void) {
    fasm_xilinx_frames *frames = fasm_xilinx_frames_new(4);
    fasm_xilinx_frames *parsed = NULL;
    fasm_error *err = NULL;
    uint32_t words[4] = {1, 2, 3, 0xDEADBEEF};
    const char *frm = "# comment\n0x00000010 0x1,0x2,0x3,0xDEADBEEF\n0x00000001 0x1,0x2\n";
    fasm_status status;
    size_t len;
    char *text;

    CHECK(fasm_xilinx_frames_new(0) == NULL, "0 words per frame");
    CHECK_OK(fasm_xilinx_frames_set(frames, 0x10, words, 4, &err), err);
    status = fasm_xilinx_frames_set(frames, 0x11, words, 3, &err);
    CHECK(status == FASM_ERR_INVALID_ARG, "wrong word count");
    fasm_error_free(err);
    err = NULL;
    CHECK(fasm_xilinx_frames_count(frames) == 1, "count");
    CHECK(fasm_xilinx_frames_address(frames, 0) == 0x10, "address");
    CHECK(fasm_xilinx_frames_words(frames, 0)[3] == 0xDEADBEEF, "words");
    CHECK(fasm_xilinx_frames_find(frames, 0x10) == fasm_xilinx_frames_words(frames, 0), "find");
    CHECK(fasm_xilinx_frames_find(frames, 0x11) == NULL, "find missing");
    CHECK(fasm_xilinx_frames_words(frames, 1) == NULL, "index out of range");
    text = frm_text(frames, &len);
    CHECK(strcmp(text, "0x00000010 0x00000001,0x00000002,0x00000003,0xDEADBEEF\n") == 0,
          "frm text %s", text);
    free(text);

    CHECK_OK(fasm_xilinx_frames_parse_frm(frm, strlen(frm), 4, count_warning, NULL, &parsed, &err),
             err);
    CHECK(warning_count == 1, "one short line warning: %d", warning_count);
    CHECK(fasm_xilinx_frames_equal(frames, parsed), "parsed");
    fasm_xilinx_frames_free(parsed);
    parsed = NULL;

    status = fasm_xilinx_frames_parse_frm("zz 0x1\n", 7, 4, NULL, NULL, &parsed, &err);
    CHECK(status == FASM_ERR_FRM && parsed == NULL, "bad number: %d", (int)status);
    CHECK(fasm_error_line(err) == 1, "line");
    fasm_error_free(err);
    err = NULL;
    status = fasm_xilinx_frames_read_frm(work("missing.frm"), 4, NULL, NULL, &parsed, &err);
    CHECK(status == FASM_ERR_IO, "missing file");
    fasm_error_free(err);
    fasm_xilinx_frames_free(frames);
}

static void test_null_handling(void) {
    fasm_error *err = NULL;
    fasm_status status;
    fasm_xilinx_frames *frames = NULL;
    fasm_xilinx_database *db = NULL;

    fasm_xilinx_database_free(NULL);
    fasm_xilinx_assembler_free(NULL);
    fasm_xilinx_frames_free(NULL);
    fasm_xilinx_part_free(NULL);
    fasm_bytes_free(NULL);
    CHECK(fasm_bytes_len(NULL) == 0 && fasm_bytes_data(NULL) == NULL, "NULL bytes");
    CHECK(fasm_xilinx_database_part(NULL) == NULL, "NULL db part");
    CHECK(fasm_xilinx_database_words_per_frame(NULL) == 0, "NULL db words");
    CHECK(fasm_xilinx_frames_count(NULL) == 0, "NULL frames count");
    CHECK(fasm_xilinx_frames_equal(NULL, NULL), "NULL frames equal");
    CHECK(fasm_xilinx_assembler_warning_count(NULL) == 0, "NULL warnings");
    CHECK(fasm_xilinx_assembler_warning(NULL, 0).len == 0, "NULL warning");
    fasm_xilinx_assembler_set_prjuray(NULL, true);
    status = fasm_xilinx_database_open(NULL, NULL, &db, &err);
    CHECK(status == FASM_ERR_INVALID_ARG && db == NULL, "NULL root");
    fasm_error_free(err);
    err = NULL;
    status = fasm_xilinx_fasm2frames_string(NULL, "", 0, NULL, &frames, &err);
    CHECK(status == FASM_ERR_INVALID_ARG && frames == NULL, "NULL db");
    fasm_error_free(err);
    err = NULL;
    status = fasm_xilinx_assembler_parse_string(NULL, "", 0, &err);
    CHECK(status == FASM_ERR_INVALID_ARG, "NULL assembler");
    fasm_error_free(err);
    err = NULL;
    status = fasm_xilinx_bitstream_write(NULL, NULL, NULL, NULL, &err);
    CHECK(status == FASM_ERR_INVALID_ARG, "NULL out");
    fasm_error_free(err);
    CHECK(strcmp(fasm_status_string(FASM_ERR_LOOKUP), "feature not in the database") == 0,
          "status string");
    CHECK(strcmp(fasm_error_kind(NULL), "") == 0, "NULL kind");
}

/* counter_test on xc7a35tcsg324-1 (FASM_DB_CACHE): fasm2frames and xcfasm. */
static void test_counter_test(void) {
    const char *cache = getenv("FASM_DB_CACHE");
    char root[2048], yaml[4096], fasm_path[4096];
    FILE *probe;
    fasm_xilinx_database *db = NULL;
    fasm_xilinx_part *part = NULL;
    fasm_xilinx_frames *frames = NULL;
    fasm_error *err = NULL;
    fasm_bytes *bit;
    int sparse;
    if (cache == NULL || cache[0] == '\0') {
        printf("counter_test: skipped (FASM_DB_CACHE is not set)\n");
        return;
    }
    snprintf(root, sizeof root, "%s/prjxray-db/artix7", cache);
    snprintf(yaml, sizeof yaml, "%s/xc7a35tcsg324-1/part.yaml", root);
    probe = fopen(yaml, "rb");
    if (probe == NULL) {
        printf("counter_test: skipped (no %s)\n", yaml);
        return;
    }
    fclose(probe);
    snprintf(fasm_path, sizeof fasm_path, "%s",
             repo("tests/corpus/xilinx/artix7/designs/f4pga-examples/counter_test/arty_35/"
                  "top.fasm"));
    CHECK_OK(fasm_xilinx_database_open(root, "xc7a35tcsg324-1", &db, &err), err);
    REQUIRE(db != NULL, "artix7");
    CHECK_OK(fasm_xilinx_part_from_database(db, &part, &err), err);
    for (sparse = 0; sparse <= 1; sparse++) {
        fasm_xilinx_fasm2frames_options options;
        memset(&options, 0, sizeof options);
        options.sparse = sparse != 0;
        CHECK_OK(fasm_xilinx_fasm2frames_file(db, fasm_path, &options, &frames, &err), err);
        REQUIRE(frames != NULL, "counter_test");
        bit = write_bit(part, frames, FASM_XILINX_FORMAT_DEFAULT, "xc7a35tcsg324-1",
                        work("counter.frm"));
        if (have_cli) {
            int code = run_cli("xcfasm",
                               "--db-root '%s' --part xc7a35tcsg324-1 --part_file '%s' %s "
                               "--fn_in '%s' --bit_out '%s' --frm_out '%s'",
                               root, yaml, sparse ? "--sparse" : "", fasm_path, work("cli.bit"),
                               work("counter.frm"));
            CHECK(code == 0, "xcfasm: %d", code);
            check_frm_file(frames, work("counter.frm"), "counter_test");
            check_bit_file(bit, work("cli.bit"), "counter_test");
        }
        fasm_bytes_free(bit);
        fasm_xilinx_frames_free(frames);
        frames = NULL;
    }
    fasm_xilinx_part_free(part);
    fasm_xilinx_database_free(db);
}

int main(int argc, char **argv) {
    char probe_path[4096];
    FILE *probe;
    if (argc != 4) {
        fprintf(stderr, "usage: %s REPO_ROOT CLI_DIR WORK_DIR\n", argv[0]);
        return 2;
    }
    repo_root = argv[1];
    cli_dir = argv[2];
    work_dir = argv[3];
    snprintf(probe_path, sizeof probe_path, "%s/xcfasm", cli_dir);
    probe = fopen(probe_path, "rb");
    have_cli = probe != NULL;
    if (probe != NULL) {
        fclose(probe);
    } else {
        printf("the command line tools are not in %s: comparisons with them skipped\n",
               cli_dir);
    }

    test_database();
    test_lookup();
    test_mini_db();
    test_errors();
    test_bitstreams(SYNTHETIC_DB, "xc7test-1", SYNTHETIC_DESIGN, "xc7frames2bit", "bitread",
                    "Series7");
    test_bitstreams(USP_DB, "xcusptest-1", USP_DESIGN, "xcframes2bit", "uray-bitread",
                    "UltraScalePlus");
    test_frames();
    test_null_handling();
    test_counter_test();

    printf("%d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}

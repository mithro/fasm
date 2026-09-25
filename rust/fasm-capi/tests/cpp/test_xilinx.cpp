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
 * Test program of the fasm::xilinx C++ wrapper (include/fasm/fasm.hpp),
 * the C++ counterpart of tests/c/test_xilinx.c.
 *
 * Usage: test_xilinx_cpp REPO_ROOT CLI_DIR WORK_DIR
 *
 * Assembles the f4pga-xc-fasm fixtures on the mini database and designs on
 * the synthetic Series7 and UltraScale+ databases, and compares the .frm
 * and .bit files byte for byte with the Rust command line tools of CLI_DIR
 * (skipped when they are not there), whose output goes to WORK_DIR.
 */

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <iterator>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

#include "fasm/fasm.hpp"

namespace fs = std::filesystem;
namespace fx = fasm::xilinx;

static int checks = 0;
static int failures = 0;

#define CHECK(cond)                                                                        \
    do {                                                                                   \
        ++checks;                                                                          \
        if (!(cond)) {                                                                     \
            ++failures;                                                                    \
            std::cerr << __FILE__ << ":" << __LINE__ << ": CHECK(" #cond ") failed\n";     \
        }                                                                                  \
    } while (0)

static fs::path repo_root;
static fs::path cli_dir;
static fs::path work_dir;
static bool have_cli = false;
static const std::int64_t kSourceDateEpoch = 1700000000;

static std::string read_file(const fs::path &path) {
    std::ifstream in(path, std::ios::binary);
    if (!in) {
        throw std::runtime_error("cannot read " + path.string());
    }
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

static void write_file(const fs::path &path, const std::string &text) {
    std::ofstream out(path, std::ios::binary);
    out << text;
}

static std::string quote(const fs::path &p) { return "'" + p.string() + "'"; }

/* Runs a command line tool (arguments already quoted); its stderr goes to
 * WORK_DIR/cli.err. */
static int run_cli(const std::string &tool, const std::string &args) {
    std::string command = "SOURCE_DATE_EPOCH=" + std::to_string(kSourceDateEpoch) +
                          " FASM_XDB_CACHE=0 " + quote(cli_dir / tool) + " " + args + " 2>" +
                          quote(work_dir / "cli.err");
    return std::system(command.c_str());
}

static const char *const kMiniFixtures[] = {
    "lut.fasm",     "ff_int.fasm",    "ff_int_0s.fasm",         "ff_int_op1.fasm",
    "lut_int.fasm", "iob/liob_stepdown.fasm", "iob/riob_stepdown.fasm",
};

static void test_mini_db() {
    fx::Database db = fx::Database::open(repo_root / "rust/fasm-xilinx/testdata/mini-db", "xc7");
    CHECK(db.architecture() == fx::Architecture::Series7);
    CHECK(db.words_per_frame() == 101);
    CHECK(db.part() == std::optional<std::string_view>("xc7"));

    fx::FeatureInfo info = db.lookup("CLBLM_L_X10Y102.SLICEM_X0.A5FF.ZINI");
    CHECK(!info.pseudo_pip && info.block_type == 0 && !info.bits.empty());

    for (const char *fixture : kMiniFixtures) {
        fs::path path = repo_root / "tests/corpus/f4pga-xc-fasm" / fixture;
        for (bool sparse : {false, true}) {
            fx::Fasm2FramesOptions options;
            options.sparse = sparse;
            fx::Frames frames = fx::fasm2frames(db, path, options);

            fx::Assembler assembler(db);
            assembler.add_file(fasm::File::parse_file(path));
            assembler.add_required_features();
            assembler.propagate_stepdown();
            CHECK(assembler.get_frames(sparse) == frames);
            CHECK(assembler.warnings().empty());

            std::string frm = frames.to_frm();
            CHECK(fx::Frames::parse_frm(frm, 101) == frames);
            frames.write_frm(work_dir / "cpp.frm");
            CHECK(read_file(work_dir / "cpp.frm") == frm);
            CHECK(fx::Frames::read_frm(work_dir / "cpp.frm", 101) == frames);
            if (have_cli) {
                int code = run_cli("fasm2frames",
                                   "--db-root " +
                                       quote(repo_root / "rust/fasm-xilinx/testdata/mini-db") +
                                       " --part xc7 " + (sparse ? "--sparse " : "") +
                                       quote(path) + " " + quote(work_dir / "cli.frm"));
                CHECK(code == 0);
                CHECK(read_file(work_dir / "cli.frm") == frm);
            }
        }
    }

    // Errors: the kind and message the tools print.
    try {
        fx::fasm2frames_string(db, "NOPE_X1Y1.A\n");
        CHECK(false);
    } catch (const fasm::Error &e) {
        CHECK(e.status() == fasm::Status::Assembler);
        CHECK(e.kind() == "KeyError");
        CHECK(std::string(e.what()) == "'NOPE_X1Y1'");
    }
    try {
        fx::Assembler assembler(db);
        assembler.parse_string("CLBLM_L_X10Y102.SLICEM_X0.NOPE\n");
        CHECK(false);
    } catch (const fasm::Error &e) {
        CHECK(e.status() == fasm::Status::Lookup);
        CHECK(e.kind() == "prjxray.fasm_assembler.FasmLookupError");
    }
    try {
        fx::Database::open(repo_root / "rust/fasm-xilinx/testdata/mini-db", "nope");
        CHECK(false);
    } catch (const fasm::Error &e) {
        CHECK(e.status() == fasm::Status::Db);
        CHECK(e.kind() == "fasm_xilinx.DbError");
    }
}

static void test_bitstreams(const std::string &rel_db, const std::string &part_name,
                            const std::string &design, const std::string &frames2bit,
                            const std::string &bitread, const std::string &architecture) {
    fs::path root = repo_root / rel_db;
    fs::path yaml = root / part_name / "part.yaml";
    fx::Database db = fx::Database::open_cached(root, part_name, work_dir / "cache");
    fx::Part part = fx::Part::from_database(db);
    fx::Part yaml_part = fx::Part::read_yaml(yaml);
    CHECK(part.architecture() == yaml_part.architecture());
    CHECK(part.architecture() == db.architecture());
    write_file(work_dir / "design.fasm", design);

    for (bool sparse : {false, true}) {
        fx::Fasm2FramesOptions options;
        options.sparse = sparse;
        fx::Frames frames = fx::fasm2frames(db, work_dir / "design.fasm", options);
        CHECK(fx::fasm2frames_string(db, design, options) == frames);
        CHECK(frames.size() > 0 && frames.words_per_frame() == db.words_per_frame());
        fx::FrameView first = frames[0];
        CHECK(first.words.size() == frames.words_per_frame());
        CHECK(frames.find(first.address).has_value());
        frames.write_frm(work_dir / "design.frm");

        fx::BitstreamOptions bit_options;
        bit_options.part_name = part_name;
        bit_options.design_name = (work_dir / "design.frm").string();
        bit_options.source_date_epoch = kSourceDateEpoch;
        std::vector<std::uint8_t> bit = fx::write_bitstream(part, frames, bit_options);
        CHECK(fx::write_bitstream(yaml_part, frames, bit_options) == bit);
        fx::write_bitstream_file(part, frames, work_dir / "cpp.bit", bit_options);
        std::string bit_file = read_file(work_dir / "cpp.bit");
        CHECK(std::vector<std::uint8_t>(bit_file.begin(), bit_file.end()) == bit);
        fx::Frames back = fx::read_bitstream(part, bit);
        CHECK(fx::read_bitstream_file(part, work_dir / "cpp.bit") == back);
        if (have_cli) {
            int code = run_cli("fasm2frames", "--db-root " + quote(root) + " --part " +
                                                  part_name + (sparse ? " --sparse " : " ") +
                                                  quote(work_dir / "design.fasm") + " " +
                                                  quote(work_dir / "cli.frm"));
            CHECK(code == 0);
            CHECK(read_file(work_dir / "cli.frm") == frames.to_frm());
            code = run_cli(frames2bit, "--architecture=" + architecture + " --part_file=" +
                                           quote(yaml) + " --part_name=" + part_name +
                                           " --frm_file=" + quote(work_dir / "design.frm") +
                                           " --output_file=" + quote(work_dir / "cli.bit"));
            CHECK(code == 0);
            CHECK(read_file(work_dir / "cli.bit") == bit_file);
            code = run_cli(bitread, "--architecture=" + architecture + " --part_file=" +
                                        quote(yaml) + " --frm_out=" +
                                        quote(work_dir / "cli-back.frm") + " " +
                                        quote(work_dir / "cli.bit") + " >/dev/null");
            CHECK(code == 0);
            CHECK(read_file(work_dir / "cli-back.frm") == back.to_frm());
        }
    }

    // Frames of the wrong size.
    try {
        fx::write_bitstream(part, fx::Frames(7));
        CHECK(false);
    } catch (const fasm::Error &e) {
        CHECK(e.status() == fasm::Status::Bitstream);
    }
}

static void test_frames_and_callbacks() {
    fx::Frames frames(4);
    frames.set(0x10, {1, 2, 3, 0xDEADBEEF});
    CHECK(frames.size() == 1 && frames[0].address == 0x10 && frames[0].words.data()[3] == 0xDEADBEEF);
    CHECK(!frames.find(0x11).has_value());
    try {
        frames.set(0x11, {1});
        CHECK(false);
    } catch (const fasm::Error &e) {
        CHECK(e.status() == fasm::Status::InvalidArg);
    }
    try {
        fx::Frames bad(0);
        CHECK(false);
    } catch (const std::invalid_argument &) {
        CHECK(true);
    }

    // A warning callback: called, and an exception it throws propagates.
    const std::string frm = "0x00000010 0x1,0x2,0x3,0xDEADBEEF\n0x00000001 0x1\n";
    int warnings = 0;
    fx::Frames parsed = fx::Frames::parse_frm(frm, 4, [&](std::string_view w) {
        ++warnings;
        CHECK(w.find("found 1 words instead of 4") != std::string_view::npos);
    });
    CHECK(warnings == 1 && parsed == frames);
    struct Custom {};
    try {
        fx::Frames::parse_frm(frm, 4, [](std::string_view) { throw Custom(); });
        CHECK(false);
    } catch (const Custom &) {
        CHECK(true);
    }
    try {
        fx::Frames::parse_frm("zz 1\n", 4);
        CHECK(false);
    } catch (const fasm::Error &e) {
        CHECK(e.status() == fasm::Status::Frm && e.line() == 1);
    }
}

int main(int argc, char **argv) {
    if (argc != 4) {
        std::cerr << "usage: " << argv[0] << " REPO_ROOT CLI_DIR WORK_DIR\n";
        return 2;
    }
    repo_root = argv[1];
    cli_dir = argv[2];
    work_dir = argv[3];
    have_cli = fs::exists(cli_dir / "xcfasm");
    if (!have_cli) {
        std::cout << "the command line tools are not in " << cli_dir
                  << ": comparisons with them skipped\n";
    }
    try {
        test_mini_db();
        test_bitstreams("rust/fasm-xilinx/testdata/synthetic-db", "xc7test-1",
                        "INT_L_X6Y0.WW2BEG0.LOGIC_OUTS_L12\n"
                        "BRAM_L_X6Y0.RAMB18_Y0.INIT_00[4:0] = 5'b10011\n"
                        "BRAM_L_X6Y0.RAMB18_Y0.IN_USE\n"
                        "LIOB33_X0Y1.IOB_Y0.PULL\n",
                        "xc7frames2bit", "bitread", "Series7");
        test_bitstreams("rust/fasm-xilinx/testdata/synthetic-usp-db", "xcusptest-1",
                        "CLEM_X1Y0.ALUT.INIT[15:0] = 16'hA5C3\n"
                        "CLEM_X1Y1.ABCDFF.CEUSED.V1\n"
                        "BRAM_X2Y0.RAMB18E2_L.INIT_00[7:0] = 8'hFF\n"
                        "RCLK_INT_L_X2Y29.BUFCE_LEAF_X0Y0.BUFCE_LEAF.DELAY_TAP.V0\n"
                        "EDGE_X0Y0.OK\n",
                        "xcframes2bit", "uray-bitread", "UltraScalePlus");
        test_frames_and_callbacks();
    } catch (const std::exception &e) {
        std::cerr << "unexpected exception: " << e.what() << "\n";
        return 1;
    }
    std::cout << checks << " checks, " << failures << " failures\n";
    return failures == 0 ? 0 : 1;
}

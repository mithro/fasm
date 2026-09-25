// Copyright 2017-2022 F4PGA Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! Throughput benchmark of `fasm::parser` (`cargo bench -p fasm --bench parser`).
//!
//! Generates several synthetic FASM files of feature lines (default 100 MB
//! each, set `FASM_PARSER_BENCH_MB` to change it) shaped like real Xilinx
//! 7 series output over a grid of tiles:
//!
//! * `mixed`: routing pips, single bit features and multi bit LUT/BRAM
//!   `INIT` values;
//! * `pips`: mostly routing pips (`INT_L_X..Y...<dst>.<src>`, short lines
//!   without values, the worst case per byte);
//! * `lut`: mostly wide `INIT[...]` value assignments (64 bit LUT and
//!   256 bit BRAM `INIT`s), the worst case for value parsing;
//! * `annotated`: every feature line followed by a `{ .. }` annotation and
//!   about a third of lines are `#` comments or blank, exercising the
//!   annotation/comment scanning path;
//! * `stress`: a synthetic "every feature" shape in the spirit of
//!   `tools/gen-corpus.py` (many distinct short feature name components
//!   combined exhaustively per tile, multi bit values, `!`-cleared bits
//!   and duplicate/overwritten features), self contained here (no
//!   database or Python needed) so it stays a deterministic `cargo bench`
//!   input.
//!
//! Alternatively set `FASM_PARSER_BENCH_FILE` to parse an existing file.
//!
//! Reports MB/s and ns/line for a first ("cold": every feature name is
//! new to the interner) and a second ("warm": all names already interned)
//! pass over the input with the streaming [`fasm::parse_lines`] API.

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

use fasm::parse_lines;

/// Builds about `target_bytes` of pip heavy FASM feature lines.
fn generate_pips(target_bytes: usize) -> String {
    const DST: [&str; 8] = [
        "SS2END4", "WW2BEG3", "IMUX_L14", "BYP_ALT2", "NN6BEG1", "EE2BEG0", "GFAN1", "FAN_ALT5",
    ];
    const SRC: [&str; 6] = [
        "EE2END6",
        "LOGIC_OUTS_L22",
        "GFAN0",
        "NR1END3",
        "SW6END1",
        "WL1END0",
    ];
    let mut out = String::with_capacity(target_bytes + 256);
    let mut i = 0usize;
    while out.len() < target_bytes {
        let (x, y) = (i % 101, (i / 101) % 151);
        if i % 10 == 9 {
            let _ = writeln!(out, "CLBLL_L_X{x}Y{y}.SLICEL_X0.CARRY4.ACY0");
        } else {
            let _ = writeln!(
                out,
                "INT_L_X{x}Y{y}.{}{}.{}",
                DST[i % DST.len()],
                i % 7,
                SRC[(i / 3) % SRC.len()]
            );
        }
        i += 1;
    }
    out
}

/// Builds about `target_bytes` of mixed FASM feature lines.
fn generate(target_bytes: usize) -> String {
    let mut out = String::with_capacity(target_bytes + 256);
    let mut x = 0u32;
    'outer: loop {
        for y in 0..200u32 {
            let _ = writeln!(
                out,
                "INT_L_X{x}Y{y}.WW2BEG0.LOGIC_OUTS_L4\n\
                 INT_L_X{x}Y{y}.IMUX_L{}.GFAN0\n\
                 CLBLM_R_X{x}Y{y}.SLICEM_X0.ALUT.INIT[63:0] = \
                 64'b1111000011110000111100001111000011110000111100001111000011110000\n\
                 CLBLM_R_X{x}Y{y}.SLICEM_X0.BLUT.INIT[31:0] = 32'hDEADBEEF\n\
                 CLBLM_R_X{x}Y{y}.SLICEM_X0.AFF.ZINI\n\
                 CLBLM_R_X{x}Y{y}.SLICEM_X0.CARRY4.ACY0 = 1\n\
                 BRAM_L_X{x}Y{y}.RAMB18_Y0.INIT_00[255:0] = 256'h\
                 0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF\n\
                 HCLK_L_X{x}Y{y}.ENABLE_BUFFER.HCLK_CK_BUFHCLK{}",
                y % 48,
                y % 12
            );
            if out.len() >= target_bytes {
                break 'outer;
            }
        }
        x += 1;
    }
    out
}

/// Builds about `target_bytes` of LUT/BRAM `INIT` heavy FASM: mostly wide
/// value assignments (64 bit LUTs, 256 bit BRAM contents), the worst case
/// for the value parser rather than the name interner.
fn generate_lut(target_bytes: usize) -> String {
    const LUT64: [&str; 4] = [
        "1111000011110000111100001111000011110000111100001111000011110000",
        "0000111100001111000011110000111100001111000011110000111100001111",
        "1010101010101010101010101010101010101010101010101010101010101010",
        "0101010101010101010101010101010101010101010101010101010101010101",
    ];
    let mut out = String::with_capacity(target_bytes + 256);
    let mut x = 0u32;
    'outer: loop {
        for y in 0..200u32 {
            let lut = &LUT64[(x as usize + y as usize) % LUT64.len()][..64];
            let _ = writeln!(
                out,
                "CLBLM_R_X{x}Y{y}.SLICEM_X0.ALUT.INIT[63:0] = 64'b{lut}\n\
                 CLBLM_R_X{x}Y{y}.SLICEM_X0.BLUT.INIT[63:0] = 64'b{lut}\n\
                 BRAM_L_X{x}Y{y}.RAMB18_Y0.INIT_00[255:0] = 256'h\
                 0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF\n\
                 BRAM_L_X{x}Y{y}.RAMB18_Y0.INITP_00[255:0] = 256'h\
                 FEDCBA9876543210FEDCBA9876543210FEDCBA9876543210FEDCBA9876543210"
            );
            if out.len() >= target_bytes {
                break 'outer;
            }
        }
        x += 1;
    }
    out
}

/// Builds about `target_bytes` of feature lines each followed by an
/// annotation, with roughly a third of the lines comments or blank, to
/// exercise the `{ .. }` / `#` scanning path rather than value parsing.
fn generate_annotated(target_bytes: usize) -> String {
    let mut out = String::with_capacity(target_bytes + 256);
    let mut x = 0u32;
    let mut i = 0u32;
    'outer: loop {
        for y in 0..200u32 {
            if i.is_multiple_of(3) {
                let _ = writeln!(out, "# generated line {i} for tile X{x}Y{y}");
            } else if i % 3 == 1 {
                out.push('\n');
            } else {
                let _ = writeln!(
                    out,
                    "CLBLL_L_X{x}Y{y}.SLICEL_X0.CARRY4.ACY0 {{ .id = \"{i}\", \
                     .file = \"synthetic.v\", .line = \"{i}\" }}"
                );
            }
            i += 1;
            if out.len() >= target_bytes {
                break 'outer;
            }
        }
        x += 1;
    }
    out
}

/// Builds about `target_bytes` of a synthetic "every feature" stress shape:
/// many short distinct component names combined per tile, multi bit
/// values, `!`-cleared bits and immediately overwritten (duplicate)
/// features, in the spirit of `tools/gen-corpus.py`'s output.
fn generate_stress(target_bytes: usize) -> String {
    const COMPONENTS: [&str; 12] = [
        "OPTA",
        "OPTB",
        "OPTC",
        "OPTD",
        "MODE",
        "ZINI",
        "ZRST",
        "FFSYNC",
        "PRECYINIT",
        "CYINIT",
        "DINMUX",
        "D6MUX",
    ];
    let mut out = String::with_capacity(target_bytes + 256);
    let mut x = 0u32;
    let mut i = 0usize;
    'outer: loop {
        for y in 0..200u32 {
            let comp = COMPONENTS[i % COMPONENTS.len()];
            let _ = writeln!(out, "TILE_X{x}Y{y}.SITE_X0.{comp}");
            if i.is_multiple_of(5) {
                // A multi-bit value with a mix of set and cleared bits.
                let _ = writeln!(
                    out,
                    "TILE_X{x}Y{y}.SITE_X0.{comp}.INIT[15:0] = 16'b1010010110100101"
                );
            }
            if i.is_multiple_of(7) {
                // Overwrite the same feature (duplicate, worst case for
                // the merge/de-dup path when printed with `--canonical`).
                let _ = writeln!(out, "TILE_X{x}Y{y}.SITE_X0.{comp}");
            }
            i += 1;
            if out.len() >= target_bytes {
                break 'outer;
            }
        }
        x += 1;
    }
    out
}

fn run(name: &str, data: &[u8]) {
    let start = Instant::now();
    let mut lines = 0usize;
    for line in parse_lines(data) {
        black_box(line.expect("benchmark input parses"));
        lines += 1;
    }
    let elapsed = start.elapsed().as_secs_f64();
    let mb = data.len() as f64 / 1e6;
    println!(
        "{name:>11}: {mb:8.1} MB, {lines:9} lines, {:7.3} s, {:7.1} MB/s, {:6.1} ns/line",
        elapsed,
        mb / elapsed,
        elapsed * 1e9 / lines.max(1) as f64
    );
}

fn main() {
    let inputs: Vec<(&str, Vec<u8>)> = if let Ok(path) = std::env::var("FASM_PARSER_BENCH_FILE") {
        vec![(
            "file",
            std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}")),
        )]
    } else {
        let mb: usize = std::env::var("FASM_PARSER_BENCH_MB")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);
        vec![
            ("mixed", generate(mb * 1_000_000).into_bytes()),
            ("pips", generate_pips(mb * 1_000_000).into_bytes()),
            ("lut", generate_lut(mb * 1_000_000).into_bytes()),
            ("annotated", generate_annotated(mb * 1_000_000).into_bytes()),
            ("stress", generate_stress(mb * 1_000_000).into_bytes()),
        ]
    };
    for (name, data) in &inputs {
        run(&format!("{name} cold"), data);
        run(&format!("{name} warm"), data);
        run(&format!("{name} warm"), data);
    }
}

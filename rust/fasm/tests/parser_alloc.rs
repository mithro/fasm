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

//! Checks the parser's allocation claim (`fasm::parser` module docs): once
//! the feature names are interned, parsing feature lines allocates nothing
//! unless a value is wider than 256 bits.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator, only counting calls.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Number of allocations made while parsing `input` with the streaming API.
fn allocations(input: &[u8]) -> usize {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut lines = 0;
    for line in fasm::parse_lines(input) {
        std::hint::black_box(line.unwrap());
        lines += 1;
    }
    assert!(lines > 0);
    ALLOCATIONS.load(Ordering::Relaxed) - before
}

#[test]
fn only_wide_values_comments_and_annotations_allocate() {
    let input = format!(
        "INT_L_X1Y2.SS2END4.EE2END6\n\
         \n\
         CLB.SLICE.ALUT.INIT[63:0] = 64'hFFFF_0000_FFFF_0000\r\n\
         CLB.SLICE.CARRY[2] = 1\n\
         CLB.SLICE.X[255:0] = 256'h{}\n\
         CLB.SLICE.X[255:0] = 'h{}F\n\
         CLB.SLICE.X[255:0] = 'b{}1\n\
         CLB.SLICE.Y[127:0] = 'd{}\n\
         CLB.SLICE.Y[255:0] = {}\n\
         CLB.SLICE.Y[3:0] = 'o0_17\n",
        "F".repeat(64),
        "0".repeat(300),
        "0".repeat(1000),
        "9".repeat(38),
        "1".repeat(77),
    );
    // Intern the names first.
    let _ = allocations(input.as_bytes());
    assert_eq!(allocations(input.as_bytes()), 0);

    // Wide values, comments and annotations do allocate. (One test only:
    // the counter is process wide and tests run in parallel.)
    let _ = allocations(b"A.B[511:0] = 'h1\nA.B\n");
    assert!(allocations(format!("A.B[511:0] = 'h1{}", "0".repeat(100)).as_bytes()) > 0);
    assert!(allocations(format!("A.B[511:0] = 1{}", "0".repeat(100)).as_bytes()) > 0);
    assert!(allocations(b"A.B # c") > 0);
    assert!(allocations(b"A.B { a = \"b\" }") > 0);
}

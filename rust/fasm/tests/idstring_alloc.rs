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

//! Checks the allocation claims of `fasm::idstring`: once names are
//! interned, interning them again, looking them up, resolving them with
//! `with_str`, formatting and comparing them allocate nothing. (A test
//! binary of its own because it replaces the global allocator.)

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Write as _;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

use fasm::idstring::{IdString, Interner};

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

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: Counting = Counting;

/// Number of allocations made by `f`.
fn allocations(f: impl FnOnce()) -> usize {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    f();
    ALLOCATIONS.load(Ordering::Relaxed) - before
}

// One test only: the counter is process wide and tests run in parallel.
#[test]
fn known_names_do_not_allocate() {
    let names: Vec<String> = (0..2000)
        .map(|i| match i % 4 {
            0 => format!("INT_L_X{i}Y{}.SS2END{}.EE2END{}", i % 7, i % 5, i % 3),
            1 => format!("CLBLL_L_X{i}Y1.SLICEL_X{}.BLUT.INIT", i % 2),
            2 => format!("TILE_{i}"),
            _ => format!("HCLK_X{i}.ENABLE_BUFFER"),
        })
        .collect();
    let ids: Vec<IdString> = names.iter().map(|s| IdString::new(s)).collect();
    // A private interner whose tiny tables send most names to the
    // overflow table.
    let private = Interner::with_level_limit(3);
    let private_ids: Vec<IdString> = names.iter().map(|s| private.intern(s)).collect();
    let mut out = String::with_capacity(1 << 20);
    let mut sorted = ids.clone();
    // Warm up the per thread `with_str` buffer and anything lazily
    // initialised.
    ids[0].with_str(str::len);
    write!(out, "{}", ids[0]).unwrap();
    out.clear();

    let count = allocations(|| {
        let mut total = 0;
        for ((name, &id), &private_id) in names.iter().zip(&ids).zip(&private_ids) {
            assert_eq!(IdString::new(name), id);
            assert_eq!(IdString::from_bytes(name.as_bytes()), Ok(id));
            assert_eq!(IdString::lookup(name), Some(id));
            assert_eq!(private.intern(name), private_id);
            assert_eq!(private.intern_bytes(name.as_bytes()), Ok(private_id));
            assert_eq!(private.lookup(name), Some(private_id));
            assert_eq!(IdString::lookup("never.interned.name"), None);
            total += id.with_str(str::len);
            total += private.with_str(private_id, str::len);
            total += id.len() + id.components().count() + id.first_component().len();
            total += usize::from(id.starts_with_component("INT_L_X4Y4"));
            write!(out, "{id} {id:>60} {id:.8} {id:?}").unwrap();
            write!(out, "{}", private.resolved(private_id)).unwrap();
            assert!(id == name.as_str() && name.as_str() == id);
            let _ = black_box(id.cmp(&ids[0]));
            let _ = black_box(id < ids[1]);
            let _ = black_box(private.cmp(private_id, private_ids[0]));
        }
        sorted.sort_unstable();
        black_box(total);
    });
    assert_eq!(count, 0, "known names allocated {count} times");
    assert!(sorted.windows(2).all(|w| w[0] <= w[1]));
}

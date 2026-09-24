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

//! Tests of the public `IdString` / `Interner` API.

use super::repr::{decode, Repr};
use super::*;

const FEATURES: &[&str] = &[
    "INT_L_X10Y146.SW6BEG0.WW2END0",
    "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT",
    "CLBLL_R_X13Y132.SLICEL_X0.ALUT.INIT",
    "CLBLL_L_X12Y124.SLICEL_X0.ALUT.INIT",
    "CLBLL_L_X12Y124.SLICEL_X1.BLUT.INIT",
    "CLBLL_L_X12Y124",
    "CLBLL_L_X12Y124.SLICEL_X0",
];

#[test]
fn global_round_trip_and_equality() {
    for &s in FEATURES {
        let id = IdString::new(s);
        assert_eq!(id.resolve(), s);
        assert_eq!(id.to_string(), s);
        assert_eq!(id, s);
        assert_eq!(s, id);
        assert_eq!(id, IdString::new(s));
        assert_eq!(IdString::lookup(s), Some(id));
        assert_eq!(IdString::from(s), id);
        assert_eq!(s.parse::<IdString>(), Ok(id));
        assert_eq!(IdString::from_bytes(s.as_bytes()), Ok(id));
        assert_eq!(id.len(), s.len());
        assert_eq!(format!("{id:?}"), format!("IdString({s:?})"));
    }
    let ids: Vec<IdString> = FEATURES.iter().map(|s| IdString::new(s)).collect();
    for (i, a) in ids.iter().enumerate() {
        for (j, b) in ids.iter().enumerate() {
            assert_eq!(a == b, i == j);
            assert_eq!(a.cmp(b), FEATURES[i].cmp(FEATURES[j]));
        }
    }
}

#[test]
fn from_bytes_rejects_invalid_utf8() {
    assert!(IdString::from_bytes(b"A.\xff").is_err());
}

#[test]
fn intern_bytes_validates_unknown_names_only() {
    for interner in [Interner::new(), Interner::with_level_limit(2)] {
        let known = ["A.B.C", "\u{e9}.\u{e9}", "X", "Q.R.S.T"];
        for s in known {
            let id = interner.intern(s);
            assert_eq!(interner.intern_bytes(s.as_bytes()), Ok(id), "{s:?}");
        }
        // Invalid UTF-8 is rejected whatever the tables hold, also when
        // some levels are known, or when a multi byte character is split by
        // a dot ("\u{e9}" is C3 A9).
        for bytes in [
            &b"A.\xff"[..],
            b"\xff.B.C",
            b"A.B.\xff",
            b"A.B.C\xff",
            b"\xc3.\xa9",
            b"\xc3\xa9.\xc3",
            b"Q.R.S.\xff",
            b"\xed\xa0\x80",
        ] {
            let expected = std::str::from_utf8(bytes).map(|_| ());
            assert!(expected.is_err());
            assert_eq!(
                interner.intern_bytes(bytes).map(|_| ()),
                expected,
                "{bytes:?}"
            );
        }
        // New valid names are interned.
        let id = interner.intern_bytes("A.\u{e9}".as_bytes());
        assert_eq!(id.map(|id| interner.resolve(id)), Ok("A.\u{e9}".to_owned()));
    }
}

#[test]
fn private_interner_is_independent() {
    let interner = Interner::new();
    assert_eq!(interner.lookup("A.B"), None);
    let id = interner.intern("A.B.C.D");
    assert_eq!(interner.lookup("A.B.C.D"), Some(id));
    assert_eq!(interner.lookup("A.B.C"), None);
    assert_eq!(interner.resolve(id), "A.B.C.D");
    assert_eq!(interner.with_str(id, str::len), 7);
    assert_eq!(interner.resolved(id), "A.B.C.D");
    assert!(format!("{interner:?}").starts_with("Interner"));
}

#[test]
fn splitting_levels() {
    let interner = Interner::new();
    let a = interner.intern("A");
    let a_dot = interner.intern("A.");
    let a_dot_dot = interner.intern("A..");
    let a_b_c = interner.intern("A.B.C");
    let a_b_c_d = interner.intern("A.B.C.D");
    match (
        decode(a.raw()),
        decode(a_dot.raw()),
        decode(a_dot_dot.raw()),
    ) {
        (Repr::Levels(x), Repr::Levels(y), Repr::Levels(z)) => {
            assert_eq!(x[0], y[0]);
            assert_eq!(x[1..], [0, 0]);
            assert_ne!(y[1], 0);
            assert_eq!(y[2], 0);
            assert_eq!(z[1], y[1], "both have an empty second component");
            assert_ne!(z[2], 0);
        }
        other => panic!("unexpected representation {other:?}"),
    }
    match (decode(a_b_c.raw()), decode(a_b_c_d.raw())) {
        (Repr::Levels(x), Repr::Levels(y)) => {
            assert_eq!(x[..2], y[..2]);
            assert_ne!(x[2], y[2], "the remainder `C.D` is one entry");
        }
        other => panic!("unexpected representation {other:?}"),
    }
}

fn is_overflow(id: IdString) -> bool {
    matches!(decode(id.raw()), Repr::Overflow(_))
}

#[test]
fn full_level_tables_fall_back_to_overflow() {
    let interner = Interner::with_level_limit(4);
    let names: Vec<String> = (0..20)
        .flat_map(|tile| (0..3).map(move |site| format!("TILE_X{tile}Y0.SITE{site}.BEL.INIT")))
        .collect();
    let ids: Vec<IdString> = names.iter().map(|s| interner.intern(s)).collect();
    // The first four tiles fit in level 0, the rest overflow.
    assert!(!is_overflow(ids[0]));
    assert!(is_overflow(ids[names.len() - 1]));
    for (i, (name, &id)) in names.iter().zip(&ids).enumerate() {
        assert_eq!(interner.resolve(id), *name);
        assert_eq!(interner.intern(name), id, "canonical handle");
        assert_eq!(interner.lookup(name), Some(id));
        let resolved = interner.resolved(id);
        assert_eq!(
            resolved.first_component(),
            name.split('.').next().unwrap_or("")
        );
        assert_eq!(
            resolved.components().collect::<Vec<_>>(),
            name.split('.').collect::<Vec<_>>()
        );
        assert!(resolved.starts_with_component(resolved.first_component()));
        for (j, (other, &other_id)) in names.iter().zip(&ids).enumerate() {
            assert_eq!(id == other_id, i == j);
            assert_eq!(interner.cmp(id, other_id), name.cmp(other));
        }
    }
    assert_eq!(interner.lookup("TILE_X99Y0.SITE0.BEL.INIT"), None);
    let stats = interner.stats();
    assert_eq!(stats.level_entries, [4, 3, 1]);
    assert_eq!(stats.overflow_entries, 16 * 3);
    assert!(stats.heap_bytes > 0);
    assert!(format!("{interner:?}").contains("overflow_entries"));
}

#[test]
fn overflow_at_each_level() {
    let interner = Interner::with_level_limit(2);
    let inputs = [
        "A.B.C",
        "A.B.D",
        "A.B.E", // level 2 full at E
        "A.X.C",
        "A.Y.C", // level 1 full at Y
        "P.B.C",
        "Q.B.C", // level 0 full at Q
        "",
        ".",
        "..",
        "Q",
        "Q.",
        "A.Y",
        "A.B.C.D.E.F",
    ];
    let ids: Vec<IdString> = inputs.iter().map(|s| interner.intern(s)).collect();
    for (s, &id) in inputs.iter().zip(&ids) {
        assert_eq!(interner.resolve(id), *s);
        assert_eq!(interner.lookup(s), Some(id));
        assert_eq!(interner.intern(s), id);
    }
    for (a, &x) in inputs.iter().zip(&ids) {
        for (b, &y) in inputs.iter().zip(&ids) {
            assert_eq!(x == y, a == b);
            assert_eq!(interner.cmp(x, y), a.cmp(b), "{a:?} vs {b:?}");
            assert_eq!(interner.resolved(x) == interner.resolved(y), a == b);
        }
    }
    assert!(is_overflow(ids[2]), "A.B.E");
    assert!(is_overflow(ids[4]), "A.Y.C");
    assert!(is_overflow(ids[6]), "Q.B.C");
    assert!(!is_overflow(ids[3]), "A.X.C");
}

#[test]
fn more_than_u16_components_in_one_level() {
    // The idstring default of 16 bit indexes would overflow here; level 0
    // has 24 bits.
    let interner = Interner::new();
    let names: Vec<String> = (0..70_000)
        .map(|i| format!("INT_L_X{i}Y0.EE2BEG0"))
        .collect();
    let ids: Vec<IdString> = names.iter().map(|s| interner.intern(s)).collect();
    for (name, &id) in names.iter().zip(&ids) {
        assert!(!is_overflow(id));
        assert_eq!(interner.resolve(id), *name);
        assert_eq!(interner.lookup(name), Some(id));
    }
    // The same with a 16 bit sized table: the excess goes to the overflow
    // table and still round trips.
    let small = Interner::with_level_limit(u32::from(u16::MAX));
    let ids: Vec<IdString> = names.iter().map(|s| small.intern(s)).collect();
    assert!(!is_overflow(ids[65_534]));
    assert!(is_overflow(ids[65_535]));
    for (name, &id) in names.iter().zip(&ids) {
        assert_eq!(small.resolve(id), *name);
        assert_eq!(small.lookup(name), Some(id));
    }
}

/// Checks everything observable about `s` and its handle in `interner`.
fn check_round_trip(interner: &Interner, s: &str) -> IdString {
    let id = interner.intern(s);
    assert_eq!(interner.resolve(id), s);
    assert_eq!(interner.with_str(id, str::to_owned), s);
    assert_eq!(interner.intern(s), id, "{s:?}: handle is canonical");
    assert_eq!(interner.lookup(s), Some(id), "{s:?}");
    let resolved = interner.resolved(id);
    assert_eq!(resolved, s);
    assert_eq!(resolved.to_string(), s);
    assert_eq!(resolved.len(), s.len());
    assert_eq!(resolved.is_empty(), s.is_empty());
    assert_eq!(
        resolved.components().collect::<Vec<_>>(),
        s.split('.').collect::<Vec<_>>()
    );
    assert_eq!(
        resolved.first_component(),
        s.split('.').next().unwrap_or("")
    );
    id
}

fn edge_cases() -> Vec<String> {
    let mut cases: Vec<String> = [
        "",
        ".",
        "..",
        "...",
        "A",
        "A.",
        ".A",
        ".A.",
        "A..B",
        "A.B",
        "A.B.C",
        "A.B.C.",
        "A.B.C.D",
        "A.B.C.D.E.F.G",
        "..A..B..",
        "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT",
        "INT_L_X10Y146.SW6BEG0.WW2END0",
        "ü",
        "ü.ß",
        "漢字.かな.カナ.한글",
        "🙂.🙃.🙂🙃",
        "\0.\u{7f}.\u{10ffff}",
        " A . B ",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    cases.push(vec!["C"; 1500].join("."));
    cases.push(vec![""; 1200].join("."));
    cases.push("X".repeat(100_000));
    cases.push(format!(
        "{}.{}.{}",
        "A".repeat(5000),
        "B".repeat(5000),
        "C".repeat(5000)
    ));
    cases
}

#[test]
fn edge_cases_round_trip() {
    for interner in [Interner::new(), Interner::with_level_limit(3)] {
        let cases = edge_cases();
        let ids: Vec<IdString> = cases
            .iter()
            .map(|s| check_round_trip(&interner, s))
            .collect();
        for (a, &x) in cases.iter().zip(&ids) {
            for (b, &y) in cases.iter().zip(&ids) {
                assert_eq!(x == y, a == b);
                assert_eq!(interner.cmp(x, y), a.cmp(b), "{a:?} vs {b:?}");
            }
        }
    }
}

#[test]
fn edge_cases_round_trip_global() {
    for s in edge_cases() {
        let id = IdString::new(&s);
        assert_eq!(id.resolve(), s);
        assert_eq!(id.to_string(), s);
        assert_eq!(id.len(), s.len());
        assert_eq!(id.is_empty(), s.is_empty());
        assert_eq!(id.components().count(), s.split('.').count());
        assert!(id.starts_with_component(id.first_component()));
        assert!(id.starts_with_component(&s));
        assert_eq!(id, s.as_str());
    }
}

#[test]
fn levels_per_component_count() {
    let interner = Interner::new();
    // 1, 2, exactly LEVELS (3) and more than LEVELS components.
    for (s, levels) in [("A", 1), ("A.B", 2), ("A.B.C", 3), ("A.B.C.D", 3)] {
        let id = check_round_trip(&interner, s);
        match decode(id.raw()) {
            Repr::Levels(fields) => {
                assert_eq!(fields.iter().filter(|&&f| f != 0).count(), levels, "{s}");
            }
            Repr::Overflow(_) => panic!("{s} overflowed"),
        }
    }
}

#[test]
fn lookup_does_not_intern() {
    let interner = Interner::new();
    assert_eq!(interner.lookup("NEVER.SEEN.BEFORE"), None);
    assert_eq!(interner.lookup("NEVER.SEEN.BEFORE"), None);
    let id = interner.intern("NEVER.SEEN.BEFORE");
    assert_eq!(interner.lookup("NEVER.SEEN.BEFORE"), Some(id));
    assert_eq!(interner.lookup("NEVER.SEEN.AGAIN"), None);
    assert_eq!(interner.lookup("NEVER.BEFORE"), None);
    // Every level of these is known, so they have a handle already.
    let never_seen = interner.lookup("NEVER.SEEN");
    assert!(never_seen.is_some());
    assert_eq!(never_seen, Some(interner.intern("NEVER.SEEN")));
    assert_eq!(interner.lookup("NEVER"), Some(interner.intern("NEVER")));
    assert_eq!(
        IdString::lookup("idstring test: never interned anywhere"),
        None
    );
}

#[test]
fn type_properties() {
    fn assert_traits<T: Copy + Clone + Eq + std::hash::Hash + Ord + Send + Sync + 'static>() {}
    assert_traits::<IdString>();
    assert_eq!(std::mem::size_of::<IdString>(), 8);
    assert_eq!(std::mem::size_of::<Option<IdString>>(), 8);
    let set: std::collections::HashSet<IdString> = ["A.B", "A.B", "A.C", "A.B"]
        .iter()
        .map(|s| IdString::new(s))
        .collect();
    assert_eq!(set.len(), 2);
    let mut sorted: Vec<IdString> = ["B", "A.C", "A.B.C", "A.B", "A", ""]
        .iter()
        .map(|s| IdString::new(s))
        .collect();
    sorted.sort();
    let sorted: Vec<String> = sorted.into_iter().map(IdString::resolve).collect();
    assert_eq!(sorted, ["", "A", "A.B", "A.B.C", "A.C", "B"]);
}

#[test]
fn display_honours_width_fill_alignment_and_precision() {
    let long = format!("{}.{}.{}", "A".repeat(700), "B".repeat(700), "C");
    let strings = [
        "",
        "A",
        "A.B",
        "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT",
        "ü.漢字.🙂.x",
        long.as_str(),
    ];
    // A private interner with tiny tables: some of these are overflowed.
    let interner = Interner::with_level_limit(1);
    for s in strings {
        let id = IdString::new(s);
        let resolved = interner.resolved(interner.intern(s));
        let cases = [
            (s.to_owned(), format!("{id}"), format!("{resolved}")),
            (
                format!("{s:40}"),
                format!("{id:40}"),
                format!("{resolved:40}"),
            ),
            (
                format!("{s:<40}"),
                format!("{id:<40}"),
                format!("{resolved:<40}"),
            ),
            (
                format!("{s:>40}"),
                format!("{id:>40}"),
                format!("{resolved:>40}"),
            ),
            (
                format!("{s:^41}"),
                format!("{id:^41}"),
                format!("{resolved:^41}"),
            ),
            (
                format!("{s:*^9}"),
                format!("{id:*^9}"),
                format!("{resolved:*^9}"),
            ),
            (
                format!("{s:.5}"),
                format!("{id:.5}"),
                format!("{resolved:.5}"),
            ),
            (
                format!("{s:-<12.3}"),
                format!("{id:-<12.3}"),
                format!("{resolved:-<12.3}"),
            ),
            (
                format!("{s:2000}|"),
                format!("{id:2000}|"),
                format!("{resolved:2000}|"),
            ),
            (
                format!("{s:?}"),
                format!("{resolved:?}"),
                format!("{resolved:?}"),
            ),
        ];
        for (expected, global, private) in cases {
            assert_eq!(global, expected, "{s:?}");
            assert_eq!(private, expected, "{s:?}");
        }
        assert_eq!(format!("{id:?}"), format!("IdString({s:?})"));
    }
}

#[test]
#[should_panic(expected = "was not created by this interner")]
fn foreign_handle_panics() {
    let big = Interner::new();
    let small = Interner::new();
    let _ = small.intern("A");
    let id = (0..100).map(|i| big.intern(&format!("T{i}"))).last();
    if let Some(id) = id {
        let _ = small.resolve(id);
    }
}

#[test]
fn concurrent_interning_gives_equal_handles() {
    const THREADS: usize = 8;
    let names: Vec<String> = (0..10_000)
        .map(|i| format!("TILE_X{}Y{}.SITE_{}.BEL{}.INIT", i % 97, i % 89, i % 7, i))
        .chain(edge_cases())
        .collect();
    // A generous and a tiny level limit (the latter races on the overflow
    // decision).
    for interner in [Interner::new(), Interner::with_level_limit(50)] {
        let results: Vec<Vec<IdString>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..THREADS)
                .map(|t| {
                    let names = &names;
                    let interner = &interner;
                    scope.spawn(move || {
                        // Each thread visits the names in a different order.
                        let n = names.len();
                        let mut ids = vec![None; n];
                        for k in 0..n {
                            let rotated = (k + t * 997) % n;
                            let i = if t % 2 == 0 { rotated } else { n - 1 - rotated };
                            let id = interner.intern(&names[i]);
                            if let Some(previous) = ids[i] {
                                assert_eq!(previous, id);
                            }
                            ids[i] = Some(id);
                        }
                        ids.into_iter()
                            .map(|id| id.expect("every name visited"))
                            .collect()
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("interning thread panicked"))
                .collect()
        });
        for ids in &results[1..] {
            assert_eq!(ids, &results[0]);
        }
        for (name, &id) in names.iter().zip(&results[0]) {
            assert_eq!(interner.resolve(id), *name);
        }
    }
}

#[test]
fn concurrent_global_interning() {
    let names: Vec<String> = (0..2000)
        .map(|i| format!("GLOBAL_THREAD_X{i}.S.B.INIT"))
        .collect();
    let results: Vec<Vec<IdString>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| names.iter().map(|s| IdString::new(s)).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("interning thread panicked"))
            .collect()
    });
    for ids in &results {
        assert_eq!(ids, &results[0]);
    }
}

mod properties {
    use super::*;
    use proptest::prelude::*;

    /// Strings made of few distinct characters so that dots, empty
    /// components and shared components are frequent.
    fn dotted() -> impl Strategy<Value = String> {
        "[ab.é]{0,12}"
    }

    proptest! {
        #[test]
        fn round_trip_any_string(s in any::<String>()) {
            let id = IdString::new(&s);
            prop_assert_eq!(id.resolve(), s.clone());
            prop_assert_eq!(IdString::lookup(&s), Some(id));
            prop_assert_eq!(id.len(), s.len());
        }

        #[test]
        fn intern_bytes_matches_from_utf8(
            known in proptest::collection::vec(dotted(), 0..8),
            bytes in proptest::collection::vec(
                prop_oneof![
                    Just(b'a'), Just(b'b'), Just(b'.'), Just(0xc3), Just(0xa9), Just(0xff)
                ],
                0..12,
            ),
            limit in prop_oneof![Just(u32::MAX), 1u32..4],
        ) {
            let interner = Interner::with_level_limit(limit);
            for s in &known {
                interner.intern(s);
            }
            // First call: the name may be new; second call: known.
            let first = interner.intern_bytes(&bytes);
            let expected = std::str::from_utf8(&bytes).map(|s| interner.intern(s));
            prop_assert_eq!(first, expected);
            prop_assert_eq!(interner.intern_bytes(&bytes), expected);
        }

        #[test]
        fn round_trip_dotted(s in dotted()) {
            check_round_trip(&GLOBAL, &s);
        }

        #[test]
        fn equality_and_order_match_str(
            strings in proptest::collection::vec(dotted(), 1..40),
            limit in prop_oneof![Just(u32::MAX), 1u32..6],
        ) {
            let interner = Interner::with_level_limit(limit);
            let ids: Vec<IdString> = strings.iter().map(|s| interner.intern(s)).collect();
            for (a, &x) in strings.iter().zip(&ids) {
                prop_assert_eq!(interner.resolve(x), a.clone());
                prop_assert_eq!(interner.lookup(a), Some(x));
                for (b, &y) in strings.iter().zip(&ids) {
                    prop_assert_eq!(x == y, a == b);
                    prop_assert_eq!(interner.cmp(x, y), a.cmp(b));
                }
            }
        }

        #[test]
        fn global_order_matches_str(a in dotted(), b in dotted()) {
            let (x, y) = (IdString::new(&a), IdString::new(&b));
            prop_assert_eq!(x.cmp(&y), a.cmp(&b));
            prop_assert_eq!(x == y, a == b);
            prop_assert_eq!(x == b.as_str(), a == b);
        }

        #[test]
        fn starts_with_component_matches_str(a in dotted(), p in dotted()) {
            let expected = a == p || a.starts_with(&format!("{p}."));
            prop_assert_eq!(IdString::new(&a).starts_with_component(&p), expected);
        }
    }
}

/// Publication: once a writer has interned a name and published that with
/// a `Release` store, a reader that `Acquire`s the store must find the
/// name with `lookup` (and get the writer's handle), even while the level
/// tables, their index generations and the overflow table keep growing.
#[test]
fn published_names_are_found_while_tables_grow() {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    const WRITERS: usize = 4;
    const READERS: usize = 4;
    // Every level grows (20,000 / 977 / 1,511 distinct texts).
    let names: Vec<String> = (0..20_000)
        .map(|i| format!("TILE_X{i}Y{}.SITE{}.BEL{}.INIT", i % 13, i % 977, i % 1511))
        .collect();
    // The second interner overflows after 3,000 tiles, so the overflow
    // table grows concurrently too.
    for interner in [Interner::new(), Interner::with_level_limit(3000)] {
        // Writer `t` interns names `t`, `t + WRITERS`, ... and publishes
        // how many it has done in `progress[t]`, after storing the handle.
        let progress: [AtomicUsize; WRITERS] = Default::default();
        let handles: Vec<AtomicU64> = names.iter().map(|_| AtomicU64::new(0)).collect();
        let own: [usize; WRITERS] =
            std::array::from_fn(|t| (t..names.len()).step_by(WRITERS).count());
        let checked = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for t in 0..WRITERS {
                let (names, interner, progress, handles) = (&names, &interner, &progress, &handles);
                scope.spawn(move || {
                    for (k, i) in (t..names.len()).step_by(WRITERS).enumerate() {
                        let id = interner.intern(&names[i]);
                        handles[i].store(id.raw().get(), Ordering::Relaxed);
                        progress[t].store(k + 1, Ordering::Release);
                    }
                });
            }
            for r in 0..READERS {
                let (names, interner, progress, handles, checked, own) =
                    (&names, &interner, &progress, &handles, &checked, &own);
                scope.spawn(move || {
                    let mut state = 0x9e37_79b9_7f4a_7c15_u64 ^ r as u64;
                    loop {
                        let mut finished = true;
                        for (t, published) in progress.iter().enumerate() {
                            let done = published.load(Ordering::Acquire);
                            finished &= done == own[t];
                            if done == 0 {
                                continue;
                            }
                            // The latest published name and a random earlier one.
                            state = state
                                .wrapping_mul(6_364_136_223_846_793_005)
                                .wrapping_add(1);
                            for k in [done - 1, (state >> 33) as usize % done] {
                                let i = t + k * WRITERS;
                                let id = interner.lookup(&names[i]);
                                let expected = handles[i].load(Ordering::Relaxed);
                                assert_eq!(
                                    id.map(|id| id.raw().get()),
                                    Some(expected),
                                    "published name {:?} not found",
                                    names[i]
                                );
                                if let Some(id) = id {
                                    assert_eq!(interner.resolve(id), names[i]);
                                }
                                checked.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        // Names not published yet: a handle found for one
                        // must still be the right one.
                        let i = (state >> 17) as usize % names.len();
                        if let Some(id) = interner.lookup(&names[i]) {
                            assert_eq!(interner.resolve(id), names[i]);
                        }
                        if finished {
                            break;
                        }
                    }
                });
            }
        });
        assert!(checked.load(Ordering::Relaxed) >= READERS * WRITERS);
        for (name, handle) in names.iter().zip(&handles) {
            let id = interner.lookup(name);
            assert_eq!(
                id.map(|id| id.raw().get()),
                Some(handle.load(Ordering::Relaxed))
            );
            assert_eq!(id.map(|id| interner.resolve(id)), Some(name.clone()));
        }
    }
}

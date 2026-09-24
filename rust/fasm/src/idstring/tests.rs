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
        assert_eq!(IdString::get(s), Some(id));
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
fn private_interner_is_independent() {
    let interner = Interner::new();
    assert_eq!(interner.get("A.B"), None);
    let id = interner.intern("A.B.C.D");
    assert_eq!(interner.get("A.B.C.D"), Some(id));
    assert_eq!(interner.get("A.B.C"), None);
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
        assert_eq!(interner.get(name), Some(id));
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
    assert_eq!(interner.get("TILE_X99Y0.SITE0.BEL.INIT"), None);
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
        assert_eq!(interner.get(s), Some(id));
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
        assert_eq!(interner.get(name), Some(id));
    }
    // The same with a 16 bit sized table: the excess goes to the overflow
    // table and still round trips.
    let small = Interner::with_level_limit(u32::from(u16::MAX));
    let ids: Vec<IdString> = names.iter().map(|s| small.intern(s)).collect();
    assert!(!is_overflow(ids[65_534]));
    assert!(is_overflow(ids[65_535]));
    for (name, &id) in names.iter().zip(&ids) {
        assert_eq!(small.resolve(id), *name);
        assert_eq!(small.get(name), Some(id));
    }
}

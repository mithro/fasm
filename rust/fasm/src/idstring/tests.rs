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

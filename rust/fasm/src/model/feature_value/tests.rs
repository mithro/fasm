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

use super::*;

#[test]
fn default_and_zero_are_zero() {
    assert!(FeatureValue::default().is_zero());
    assert!(FeatureValue::zero().is_zero());
    assert_eq!(FeatureValue::default(), FeatureValue::from_u64(0));
}

#[test]
fn from_u64_basic() {
    let v = FeatureValue::from_u64(42);
    assert_eq!(v.to_u64(), Some(42));
    assert!(!v.is_zero());
    assert!(!v.is_one());
}

#[test]
fn from_bool() {
    assert_eq!(FeatureValue::from_bool(false), FeatureValue::from_u64(0));
    assert_eq!(FeatureValue::from_bool(true), FeatureValue::from_u64(1));
}

#[test]
fn from_u128_basic() {
    let v = FeatureValue::from_u128(u128::MAX);
    assert_eq!(v.bit_len(), 128);
    assert_eq!(
        v.to_radix_string(16, false),
        "ffffffffffffffffffffffffffffffff"
    );
}

#[test]
fn is_one() {
    assert!(FeatureValue::from_u64(1).is_one());
    assert!(!FeatureValue::from_u64(0).is_one());
    assert!(!FeatureValue::from_u64(2).is_one());
}

// --- from_digits -----------------------------------------------------

#[test]
fn from_digits_decimal() {
    assert_eq!(
        FeatureValue::from_digits(b"1234567890", 10).unwrap(),
        FeatureValue::from_u64(1_234_567_890)
    );
}

#[test]
fn from_digits_ignores_underscores() {
    assert_eq!(
        FeatureValue::from_digits(b"1_234_567", 10).unwrap(),
        FeatureValue::from_u64(1_234_567)
    );
    assert_eq!(
        FeatureValue::from_digits(b"__1__", 10).unwrap(),
        FeatureValue::from_u64(1)
    );
}

#[test]
fn from_digits_hex_mixed_case() {
    assert_eq!(
        FeatureValue::from_digits(b"deadBEEF", 16).unwrap(),
        FeatureValue::from_u64(0xdead_beef)
    );
}

#[test]
fn from_digits_octal() {
    assert_eq!(
        FeatureValue::from_digits(b"17", 8).unwrap(),
        FeatureValue::from_u64(15)
    );
}

#[test]
fn from_digits_binary() {
    assert_eq!(
        FeatureValue::from_digits(b"1010", 2).unwrap(),
        FeatureValue::from_u64(10)
    );
}

#[test]
fn from_digits_empty_after_removing_underscores() {
    assert_eq!(
        FeatureValue::from_digits(b"", 10),
        Err(ValueParseError::EmptyDigits)
    );
    assert_eq!(
        FeatureValue::from_digits(b"___", 10),
        Err(ValueParseError::EmptyDigits)
    );
}

#[test]
fn from_digits_invalid_digit() {
    assert_eq!(
        FeatureValue::from_digits(b"12g", 16),
        Err(ValueParseError::InvalidDigit {
            digit: 'g',
            radix: 16
        })
    );
    assert_eq!(
        FeatureValue::from_digits(b"012", 2),
        Err(ValueParseError::InvalidDigit {
            digit: '2',
            radix: 2
        })
    );
    assert_eq!(
        FeatureValue::from_digits(b"8", 8),
        Err(ValueParseError::InvalidDigit {
            digit: '8',
            radix: 8
        })
    );
}

#[test]
fn from_digits_unsupported_radix() {
    assert_eq!(
        FeatureValue::from_digits(b"1", 3),
        Err(ValueParseError::UnsupportedRadix(3))
    );
}

#[test]
fn from_hex_str_and_from_bin_str() {
    assert_eq!(
        FeatureValue::from_hex_str("ff").unwrap(),
        FeatureValue::from_u64(255)
    );
    assert_eq!(
        FeatureValue::from_bin_str("11").unwrap(),
        FeatureValue::from_u64(3)
    );
}

// --- Python parity cases from examples/many.fasm ----------------------

#[test]
fn python_parity_binary_literal() {
    // 32'b11110000_11110000_11110000_11110000
    let v = FeatureValue::from_digits(b"11110000_11110000_11110000_11110000", 2).unwrap();
    assert_eq!(v, FeatureValue::from_u64(4_042_322_160));
    assert_eq!(
        v.to_radix_string(2, false),
        "11110000111100001111000011110000"
    );
}

#[test]
fn python_parity_hex_literal() {
    // 5'h1F
    let v = FeatureValue::from_digits(b"1F", 16).unwrap();
    assert_eq!(v, FeatureValue::from_u64(31));
}

#[test]
fn python_parity_octal_literal() {
    // 32'o1234567
    let v = FeatureValue::from_digits(b"1234567", 8).unwrap();
    assert_eq!(v, FeatureValue::from_u64(342_391));
}

// --- bit_len / bit / fits_in_bits / to_u64 -----------------------------

#[test]
fn bit_len_of_zero_is_zero() {
    assert_eq!(FeatureValue::from_u64(0).bit_len(), 0);
}

#[test]
fn bit_len_powers_of_two() {
    assert_eq!(FeatureValue::from_u64(1).bit_len(), 1);
    assert_eq!(FeatureValue::from_u64(2).bit_len(), 2);
    assert_eq!(FeatureValue::from_u64(3).bit_len(), 2);
    assert_eq!(FeatureValue::from_u64(0xFF).bit_len(), 8);
    assert_eq!(FeatureValue::from_u64(u64::MAX).bit_len(), 64);
}

#[test]
fn bit_len_across_limb_boundary() {
    let v = FeatureValue::from_u64(1).shl(64);
    assert_eq!(v.bit_len(), 65);
    let v = FeatureValue::from_u64(1).shl(255);
    assert_eq!(v.bit_len(), 256);
    let v = FeatureValue::from_u64(1).shl(256);
    assert_eq!(v.bit_len(), 257);
}

#[test]
fn bit_reads_expected_positions() {
    let v = FeatureValue::from_u64(0b1010);
    assert!(!v.bit(0));
    assert!(v.bit(1));
    assert!(!v.bit(2));
    assert!(v.bit(3));
    assert!(!v.bit(4));
    assert!(!v.bit(1000));
}

#[test]
fn fits_in_bits() {
    let v = FeatureValue::from_u64(15);
    assert!(v.fits_in_bits(4));
    assert!(!v.fits_in_bits(3));
    assert!(v.fits_in_bits(5));
    assert!(FeatureValue::from_u64(0).fits_in_bits(0));
}

#[test]
fn to_u64_boundary() {
    assert_eq!(FeatureValue::from_u64(u64::MAX).to_u64(), Some(u64::MAX));
    let over = FeatureValue::from_u64(1).shl(64);
    assert_eq!(over.to_u64(), None);
}

#[test]
fn iter_set_bits_matches_expected() {
    let v = FeatureValue::from_u64(0b1011);
    assert_eq!(v.iter_set_bits().collect::<Vec<_>>(), vec![0, 1, 3]);

    let v = FeatureValue::from_u64(1)
        .shl(200)
        .bitor(&FeatureValue::from_u64(1));
    assert_eq!(v.iter_set_bits().collect::<Vec<_>>(), vec![0, 200]);
}

// --- shr / shl / mask / set_bit / bitor --------------------------------

#[test]
fn shr_basic() {
    assert_eq!(
        FeatureValue::from_u64(0b1010).shr(1),
        FeatureValue::from_u64(0b101)
    );
    assert_eq!(FeatureValue::from_u64(1).shr(0), FeatureValue::from_u64(1));
    assert_eq!(FeatureValue::from_u64(1).shr(1), FeatureValue::from_u64(0));
}

#[test]
fn shr_across_limb_boundary() {
    let v = FeatureValue::from_u128(1u128 << 100);
    assert_eq!(v.shr(64), FeatureValue::from_u64(1 << 36));
    assert_eq!(v.shr(100), FeatureValue::from_u64(1));
    assert_eq!(v.shr(101), FeatureValue::from_u64(0));
}

#[test]
fn shr_by_more_than_value_is_zero() {
    let v = FeatureValue::from_u64(1).shl(300);
    assert!(v.shr(1000).is_zero());
}

#[test]
fn shl_basic() {
    assert_eq!(FeatureValue::from_u64(1).shl(4), FeatureValue::from_u64(16));
    assert_eq!(FeatureValue::from_u64(1).shl(0), FeatureValue::from_u64(1));
}

#[test]
fn shl_across_limb_boundary_stays_inline() {
    let v = FeatureValue::from_u64(1).shl(255);
    assert_eq!(v.bit_len(), 256);
    assert_eq!(v.to_radix_string(16, true).len(), 64);
}

#[test]
fn shl_grows_to_heap() {
    let v = FeatureValue::from_u64(1).shl(256);
    assert_eq!(v.bit_len(), 257);
    assert!(v.bit(256));
    assert!(!v.bit(255));
}

#[test]
fn mask_basic() {
    assert_eq!(
        FeatureValue::from_u64(0xFF).mask(4),
        FeatureValue::from_u64(0xF)
    );
    assert_eq!(
        FeatureValue::from_u64(0xFF).mask(100),
        FeatureValue::from_u64(0xFF)
    );
    assert!(FeatureValue::from_u64(0xFF).mask(0).is_zero());
}

#[test]
fn mask_across_limb_boundary() {
    let v = FeatureValue::from_u64(1)
        .shl(64)
        .bitor(&FeatureValue::from_u64(1));
    assert_eq!(v.mask(64), FeatureValue::from_u64(1));
    assert_eq!(v.mask(65), v);
}

#[test]
fn set_bit_within_inline() {
    let mut v = FeatureValue::default();
    v.set_bit(5);
    assert_eq!(v, FeatureValue::from_u64(32));
    v.set_bit(0);
    assert_eq!(v, FeatureValue::from_u64(33));
}

#[test]
fn set_bit_grows_to_heap() {
    let mut v = FeatureValue::default();
    v.set_bit(300);
    assert_eq!(v.bit_len(), 301);
    assert!(v.bit(300));
    assert_eq!(v, FeatureValue::from_u64(1).shl(300));
}

#[test]
fn bitor_basic() {
    assert_eq!(
        FeatureValue::from_u64(0b1010).bitor(&FeatureValue::from_u64(0b0101)),
        FeatureValue::from_u64(0b1111)
    );
    let a = FeatureValue::from_u64(1).shl(300);
    let b = FeatureValue::from_u64(1);
    let both = a.bitor(&b);
    assert!(both.bit(0));
    assert!(both.bit(300));
}

// --- to_radix_string / Display ------------------------------------------

#[test]
fn to_radix_string_zero_is_zero_for_every_radix() {
    let v = FeatureValue::default();
    assert_eq!(v.to_radix_string(2, false), "0");
    assert_eq!(v.to_radix_string(8, false), "0");
    assert_eq!(v.to_radix_string(10, false), "0");
    assert_eq!(v.to_radix_string(16, false), "0");
}

#[test]
fn to_radix_string_no_leading_zeros() {
    let v = FeatureValue::from_u64(1);
    assert_eq!(v.to_radix_string(2, false), "1");
    assert_eq!(v.to_radix_string(16, false), "1");
}

#[test]
fn to_radix_string_hex_case() {
    let v = FeatureValue::from_u64(0xdead_beef);
    assert_eq!(v.to_radix_string(16, true), "DEADBEEF");
    assert_eq!(v.to_radix_string(16, false), "deadbeef");
}

#[test]
fn to_radix_string_octal_and_binary() {
    let v = FeatureValue::from_u64(342_391);
    assert_eq!(v.to_radix_string(8, false), "1234567");

    let v = FeatureValue::from_u64(4_042_322_160);
    assert_eq!(
        v.to_radix_string(2, false),
        "11110000111100001111000011110000"
    );
}

#[test]
fn to_radix_string_decimal_beyond_u64() {
    let v = FeatureValue::from_u128(u128::MAX);
    assert_eq!(v.to_radix_string(10, false), u128::MAX.to_string());
}

#[test]
#[should_panic(expected = "unsupported radix")]
fn to_radix_string_panics_on_bad_radix() {
    let _ = FeatureValue::from_u64(1).to_radix_string(3, false);
}

#[test]
fn display_matches_decimal_to_radix_string() {
    let v = FeatureValue::from_u128(u128::MAX);
    assert_eq!(v.to_string(), v.to_radix_string(10, false));
    assert_eq!(FeatureValue::default().to_string(), "0");
}

#[test]
fn debug_is_hex() {
    let v = FeatureValue::from_u64(255);
    assert_eq!(format!("{v:?}"), "FeatureValue(0xFF)");
}

// --- traits --------------------------------------------------------------

#[test]
fn partial_eq_u64() {
    assert_eq!(FeatureValue::from_u64(42), 42u64);
    assert_eq!(42u64, FeatureValue::from_u64(42));
    assert_ne!(FeatureValue::from_u64(1).shl(65), 0u64);
}

#[test]
fn from_impls() {
    let a: FeatureValue = 42u64.into();
    assert_eq!(a, FeatureValue::from_u64(42));
    let b: FeatureValue = u128::MAX.into();
    assert_eq!(b, FeatureValue::from_u128(u128::MAX));
}

#[test]
fn ord_within_inline() {
    assert!(FeatureValue::from_u64(1) < FeatureValue::from_u64(2));
    assert!(FeatureValue::from_u64(0) < FeatureValue::from_u64(1));
    assert_eq!(FeatureValue::from_u64(5), FeatureValue::from_u64(5));
}

#[test]
fn ord_across_inline_heap_boundary() {
    let max_inline = FeatureValue::from_u64(1).shl(256).mask(256).bitor(
        &FeatureValue::from_u64(1).shl(256).shr(1), // largest representable 256-bit value pattern below
    );
    let smallest_heap = FeatureValue::from_u64(1).shl(256); // needs 257 bits
    assert!(max_inline < smallest_heap);
    assert!(FeatureValue::from_u64(u64::MAX) < smallest_heap);
}

// --- size ------------------------------------------------------------

#[test]
fn size_of_feature_value() {
    // Documented in docs/rewrite/DESIGN-model.md.
    assert_eq!(std::mem::size_of::<FeatureValue>(), 40);
}

// --- proptest cross-check against num-bigint -----------------------------

mod proptest_cross_check {
    use num_bigint::BigUint;
    use proptest::prelude::*;

    use super::FeatureValue;

    fn to_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        /// Cross checks construction, radix formatting, `bit_len`, `bit`
        /// and `iter_set_bits` against `num_bigint::BigUint` for random
        /// values up to 1024 bits.
        #[test]
        fn matches_num_bigint(bytes in prop::collection::vec(any::<u8>(), 1..128)) {
            let fv = FeatureValue::from_hex_str(&to_hex(&bytes)).unwrap();
            let big = BigUint::from_bytes_be(&bytes);

            prop_assert_eq!(fv.to_radix_string(10, false), big.to_str_radix(10));
            prop_assert_eq!(fv.to_radix_string(16, false), big.to_str_radix(16));
            prop_assert_eq!(fv.to_radix_string(8, false), big.to_str_radix(8));
            prop_assert_eq!(fv.to_radix_string(2, false), big.to_str_radix(2));
            prop_assert_eq!(u64::from(fv.bit_len()), big.bits());

            let bit_len = fv.bit_len();
            let mut want_bits = Vec::new();
            for i in 0..bit_len.saturating_add(8) {
                let got = fv.bit(i);
                let want = big.bit(u64::from(i));
                prop_assert_eq!(got, want, "bit {} differs", i);
                if want {
                    want_bits.push(i);
                }
            }
            prop_assert_eq!(fv.iter_set_bits().collect::<Vec<_>>(), want_bits);
        }

        #[test]
        fn shr_matches_num_bigint(
            bytes in prop::collection::vec(any::<u8>(), 1..128),
            n in 0u32..1100,
        ) {
            let fv = FeatureValue::from_hex_str(&to_hex(&bytes)).unwrap();
            let big = BigUint::from_bytes_be(&bytes);

            let got = fv.shr(n);
            let want = &big >> (n as usize);
            prop_assert_eq!(got.to_radix_string(16, false), want.to_str_radix(16));
        }

        #[test]
        fn shl_matches_num_bigint(
            bytes in prop::collection::vec(any::<u8>(), 1..128),
            n in 0u32..300,
        ) {
            let fv = FeatureValue::from_hex_str(&to_hex(&bytes)).unwrap();
            let big = BigUint::from_bytes_be(&bytes);

            let got = fv.shl(n);
            let want = &big << (n as usize);
            prop_assert_eq!(got.to_radix_string(16, false), want.to_str_radix(16));
        }

        #[test]
        fn mask_matches_num_bigint(
            bytes in prop::collection::vec(any::<u8>(), 1..128),
            width in 0u32..1100,
        ) {
            let fv = FeatureValue::from_hex_str(&to_hex(&bytes)).unwrap();
            let big = BigUint::from_bytes_be(&bytes);

            let got = fv.mask(width);
            let want = if width == 0 {
                BigUint::from(0u32)
            } else {
                let m = (BigUint::from(1u32) << (width as usize)) - BigUint::from(1u32);
                &big & &m
            };
            prop_assert_eq!(got.to_radix_string(16, false), want.to_str_radix(16));
        }
    }
}

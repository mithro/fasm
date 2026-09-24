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

//! Small serde helpers for reading the database JSON files without
//! building a JSON DOM: borrowed strings, order preserving maps and
//! prjxray style integers.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};

/// A JSON string, borrowed from the input when it has no escapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JStr<'a>(pub(crate) Cow<'a, str>);

impl JStr<'_> {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for JStr<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = JStr<'a>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string")
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
                Ok(JStr(Cow::Borrowed(v)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(JStr(Cow::Owned(v.to_owned())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(JStr(Cow::Owned(v)))
            }
        }
        deserializer.deserialize_str(V(PhantomData))
    }
}

/// A JSON object read as a list of pairs, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrderedMap<K, V>(pub(crate) Vec<(K, V)>);

impl<K, V> Default for OrderedMap<K, V> {
    fn default() -> Self {
        OrderedMap(Vec::new())
    }
}

impl<'de, K: Deserialize<'de>, V: Deserialize<'de>> Deserialize<'de> for OrderedMap<K, V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MapVisitor<K, V>(PhantomData<(K, V)>);
        impl<'de, K: Deserialize<'de>, V: Deserialize<'de>> Visitor<'de> for MapVisitor<K, V> {
            type Value = OrderedMap<K, V>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut pairs = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some(pair) = map.next_entry()? {
                    pairs.push(pair);
                }
                Ok(OrderedMap(pairs))
            }
        }
        deserializer.deserialize_map(MapVisitor(PhantomData))
    }
}

/// An unsigned 32-bit integer given either as a JSON number or as a
/// string in Python `int(s, 0)` style (`"0x00400100"`, `"42"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PyInt(pub(crate) u32);

/// Parses `s` like Python `int(s, 0)` restricted to what the databases
/// use: `0x`/`0o`/`0b` prefixed or decimal, optional `_` separators are
/// not accepted.
pub(crate) fn parse_int0(s: &str) -> Option<u32> {
    let s = s.trim();
    let (digits, radix) = match s.get(..2) {
        Some("0x" | "0X") => (&s[2..], 16),
        Some("0o" | "0O") => (&s[2..], 8),
        Some("0b" | "0B") => (&s[2..], 2),
        _ => (s, 10),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    // Python rejects decimal literals with leading zeros ("010").
    if radix == 10
        && digits.len() > 1
        && digits.starts_with('0')
        && digits.bytes().any(|b| b != b'0')
    {
        return None;
    }
    u32::from_str_radix(digits, radix).ok()
}

impl<'de> Deserialize<'de> for PyInt {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = PyInt;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an unsigned 32-bit integer or an integer string like \"0x00400100\"")
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<PyInt, E> {
                u32::try_from(v)
                    .map(PyInt)
                    .map_err(|_| E::custom(format!("{v} does not fit in 32 bits")))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<PyInt, E> {
                u32::try_from(v)
                    .map(PyInt)
                    .map_err(|_| E::custom(format!("{v} is not an unsigned 32-bit integer")))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<PyInt, E> {
                parse_int0(v)
                    .map(PyInt)
                    .ok_or_else(|| E::custom(format!("{v:?} is not an unsigned 32-bit integer")))
            }
        }
        deserializer.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int0() {
        assert_eq!(parse_int0("0x00400100"), Some(0x0040_0100));
        assert_eq!(parse_int0("0X1f"), Some(31));
        assert_eq!(parse_int0("42"), Some(42));
        assert_eq!(parse_int0("0"), Some(0));
        assert_eq!(parse_int0("000"), Some(0));
        assert_eq!(parse_int0("0o17"), Some(15));
        assert_eq!(parse_int0("0b101"), Some(5));
        for bad in [
            "",
            "0x",
            "x1",
            "010",
            "1a",
            "-1",
            "0x1_0",
            "0x100000000",
            "4294967296",
        ] {
            assert_eq!(parse_int0(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn borrowed_and_escaped_strings() {
        let v: OrderedMap<JStr<'_>, JStr<'_>> =
            serde_json::from_str(r#"{"b": "x", "a": "y\"z"}"#).unwrap();
        assert!(matches!(v.0[0].0 .0, Cow::Borrowed("b")));
        assert_eq!(v.0[1].1.as_str(), "y\"z");
        let n: Vec<PyInt> = serde_json::from_str(r#"[1, "0x10", "7"]"#).unwrap();
        assert_eq!(n, [PyInt(1), PyInt(16), PyInt(7)]);
        assert!(serde_json::from_str::<PyInt>("-1").is_err());
        assert!(serde_json::from_str::<PyInt>("1.5").is_err());
    }
}

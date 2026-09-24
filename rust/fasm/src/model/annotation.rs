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

//! [`Annotation`]: the Rust equivalent of Python's `Annotation` namedtuple.

/// A single `name = "value"` annotation inside a `{ ... }` block, e.g.
/// `{ .filename = "/a/b/c.txt" }`.
///
/// Mirrors Python's `fasm.model.Annotation` namedtuple: both `name` and
/// `value` are arbitrary text (never `None`); `value` may be empty. The
/// text held in `value` is the raw text between the quotes, kept verbatim
/// (including any backslashes), exactly as both the ANTLR and textX Python
/// parsers hand it to `Annotation` (neither parser interprets escape
/// sequences before constructing the tuple).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Annotation {
    /// Annotation name.
    pub name: Box<str>,
    /// Annotation value; may be empty, never absent.
    pub value: Box<str>,
}

impl Annotation {
    /// Builds an `Annotation` from `name` and `value`.
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, value: impl Into<Box<str>>) -> Self {
        Annotation {
            name: name.into(),
            value: value.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_and_equality() {
        let a = Annotation::new("name", "value");
        let b = Annotation::new("name".to_string(), "value".to_string());
        assert_eq!(a, b);
        assert_eq!(&*a.name, "name");
        assert_eq!(&*a.value, "value");
    }

    #[test]
    fn empty_value_is_allowed() {
        let a = Annotation::new("attr", "");
        assert_eq!(&*a.value, "");
    }

    #[test]
    fn value_kept_verbatim_including_backslashes() {
        // The raw text between the quotes, backslashes and all: neither
        // reference parser unescapes before constructing the Annotation.
        let a = Annotation::new("path", r"C:\a\b\c.txt");
        assert_eq!(&*a.value, r"C:\a\b\c.txt");
    }

    #[test]
    fn different_values_are_not_equal() {
        assert_ne!(Annotation::new("n", "a"), Annotation::new("n", "b"));
    }
}

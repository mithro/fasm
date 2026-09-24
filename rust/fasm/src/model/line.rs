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

//! [`FasmLine`]: the Rust equivalent of Python's `FasmLine` namedtuple.

use super::annotation::Annotation;
use super::set_feature::SetFasmFeature;

/// One line of a FASM file:
/// `feature[31:0] = 42 { name = "value" } # comment`.
///
/// Mirrors Python's `fasm.model.FasmLine` namedtuple field for field:
///
/// * `set_feature`: `None` when the line has no `SetFasmFeature`.
/// * `annotations`: `None` when the line has no `{ ... }` block at all. An
///   empty `{}` block is not valid FASM syntax (see
///   `docs/specification/syntax.rst`'s grammar, where `Annotations`
///   requires at least one `Annotation`), so a parser never produces
///   `Some(vec![])`; it is still representable here (e.g. as an
///   intermediate value while building a line programmatically).
/// * `comment`: `None` when the line has no `#`. A bare `#` (no text after
///   it) is `Some("")`; the comment text is everything after `#`, verbatim,
///   including a leading space (e.g. `# foo` has `comment == Some(" foo")`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct FasmLine {
    /// The `SetFasmFeature` on this line, or `None`.
    pub set_feature: Option<SetFasmFeature>,
    /// The `{ ... }` annotations on this line, or `None` if there is no
    /// `{ ... }` block (see the struct docs for why `Some(vec![])` does
    /// not occur from parsing).
    pub annotations: Option<Vec<Annotation>>,
    /// The `# ...` comment text (everything after `#`, verbatim), or
    /// `None` if there is no `#`.
    pub comment: Option<Box<str>>,
}

impl FasmLine {
    /// Python truthiness of `annotations`: `not line.annotations` is `True`
    /// for both `None` and `[]` (an empty list is falsy in Python).
    fn annotations_is_falsy(&self) -> bool {
        self.annotations.as_ref().is_none_or(Vec::is_empty)
    }

    /// Python truthiness of `comment`: `not line.comment` is `True` for
    /// both `None` and `""` (an empty string is falsy in Python).
    fn comment_is_falsy(&self) -> bool {
        self.comment.as_deref().is_none_or(str::is_empty)
    }

    /// `True` if the line has no `SetFasmFeature`, no (non-empty)
    /// annotations and no (non-empty) comment.
    ///
    /// Mirrors Python's `fasm.output.is_blank_line`.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.set_feature.is_none() && self.annotations_is_falsy() && self.comment_is_falsy()
    }

    /// `True` if the line has no `SetFasmFeature`, no (non-empty)
    /// annotations, and a comment (`Some` and non-empty, including
    /// `Some("")` from a bare `#`... note: like Python, a bare `#` with an
    /// empty comment text is *not* "only a comment" by this definition,
    /// since `Some("")` is falsy; it is a blank line).
    ///
    /// Mirrors Python's `fasm.output.is_only_comment`.
    #[must_use]
    pub fn is_only_comment(&self) -> bool {
        self.set_feature.is_none() && self.annotations_is_falsy() && !self.comment_is_falsy()
    }

    /// `True` if the line has no `SetFasmFeature`, has (non-empty)
    /// annotations, and no (non-empty) comment.
    ///
    /// Mirrors Python's `fasm.output.is_only_annotation`.
    #[must_use]
    pub fn is_only_annotation(&self) -> bool {
        self.set_feature.is_none() && !self.annotations_is_falsy() && self.comment_is_falsy()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idstring::IdString;
    use crate::model::feature_value::FeatureValue;

    fn feature() -> SetFasmFeature {
        SetFasmFeature::new_unchecked(
            IdString::new("line_tests.F"),
            None,
            None,
            FeatureValue::from_u64(1),
            None,
        )
    }

    #[test]
    fn default_is_blank() {
        let line = FasmLine::default();
        assert!(line.is_blank());
        assert!(!line.is_only_comment());
        assert!(!line.is_only_annotation());
    }

    #[test]
    fn none_comment_is_falsy_like_empty() {
        let none_comment = FasmLine {
            comment: None,
            ..FasmLine::default()
        };
        let empty_comment = FasmLine {
            comment: Some("".into()),
            ..FasmLine::default()
        };
        assert!(none_comment.is_blank());
        assert!(empty_comment.is_blank());
        assert!(!none_comment.is_only_comment());
        assert!(!empty_comment.is_only_comment());
    }

    #[test]
    fn non_empty_comment_is_only_comment() {
        let line = FasmLine {
            comment: Some(" hello".into()),
            ..FasmLine::default()
        };
        assert!(line.is_only_comment());
        assert!(!line.is_blank());
        assert!(!line.is_only_annotation());
    }

    #[test]
    fn none_annotations_is_falsy_like_empty_vec() {
        let none_annotations = FasmLine::default();
        let empty_annotations = FasmLine {
            annotations: Some(vec![]),
            ..FasmLine::default()
        };
        assert!(none_annotations.is_blank());
        assert!(empty_annotations.is_blank());
        assert!(!none_annotations.is_only_annotation());
        assert!(!empty_annotations.is_only_annotation());
    }

    #[test]
    fn non_empty_annotations_is_only_annotation() {
        let line = FasmLine {
            annotations: Some(vec![Annotation::new("n", "v")]),
            ..FasmLine::default()
        };
        assert!(line.is_only_annotation());
        assert!(!line.is_blank());
        assert!(!line.is_only_comment());
    }

    #[test]
    fn set_feature_makes_none_of_the_three_true() {
        let line = FasmLine {
            set_feature: Some(feature()),
            annotations: Some(vec![Annotation::new("n", "v")]),
            comment: Some(" c".into()),
        };
        assert!(!line.is_blank());
        assert!(!line.is_only_comment());
        assert!(!line.is_only_annotation());
    }

    #[test]
    fn set_feature_alone_is_none_of_the_three() {
        let line = FasmLine {
            set_feature: Some(feature()),
            ..FasmLine::default()
        };
        assert!(!line.is_blank());
        assert!(!line.is_only_comment());
        assert!(!line.is_only_annotation());
    }
}

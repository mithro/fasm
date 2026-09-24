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

//! [`fasm_line_to_string`] / [`fasm_tuple_to_string`]: the Rust equivalent
//! of the identically named functions in `fasm/__init__.py`.

use std::fmt::Write as _;

use super::super::model::FasmLine;
use super::canonical::canonical_features;
use super::error::OutputError;
use super::format::set_feature_to_str;

/// Renders one `FasmLine` to zero or more lines of FASM source text.
///
/// Mirrors Python's `fasm_line_to_string` generator in `fasm/__init__.py`:
///
/// * `canonical = true`: yields the canonical form of `line.set_feature`
///   (see [`canonical_features`]), zero, one or several strings, and
///   ignores annotations/comments entirely. A line with no `set_feature`
///   (or whose value is `0`) yields nothing.
/// * `canonical = false`: yields exactly one string, the space joined
///   concatenation of: the `SetFasmFeature` (if any), the `{ name = "value",
///   ... }` annotation block (if `annotations` is non-empty; Python's
///   truthiness, so `Some(vec![])` counts as absent, matching
///   [`FasmLine::is_blank`]/friends), and `#{comment}` (if `comment` is
///   `Some`, **even `Some("")`** — unlike the annotations check, this is
///   not a truthiness check; a bare `#` becomes `Some("")` while parsing
///   and still renders as the literal string `"#"`). A line with none of
///   the three yields one empty string (Python: `' '.join([])` is `''`,
///   and the early return only applies when `canonical` is `true`).
///
/// # Errors
///
/// `canonical = false` propagates any [`OutputError`] from
/// [`super::set_feature_to_str`] and never panics, even for a `set_feature`
/// built with [`super::super::model::SetFasmFeature::new_unchecked`] from
/// inconsistent inputs. `canonical = true` also propagates
/// [`OutputError`]s from `set_feature_to_str`, but can still panic via
/// [`canonical_features`]'s documented panics for that same kind of
/// inconsistent input (see that function's docs, and
/// [`try_canonical_features`] for the fallible equivalent it wraps).
///
/// [`try_canonical_features`]: super::try_canonical_features
pub fn fasm_line_to_string(line: &FasmLine, canonical: bool) -> Result<Vec<String>, OutputError> {
    if canonical {
        let Some(set_feature) = &line.set_feature else {
            return Ok(Vec::new());
        };

        let mut out = Vec::new();
        for feature in canonical_features(set_feature) {
            out.push(set_feature_to_str(&feature, true)?);
        }
        return Ok(out);
    }

    let mut parts: Vec<String> = Vec::new();

    if let Some(set_feature) = &line.set_feature {
        parts.push(set_feature_to_str(set_feature, false)?);
    }

    if let Some(annotations) = &line.annotations {
        if !annotations.is_empty() {
            let mut s = String::from("{ ");
            for (i, annotation) in annotations.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                // `write!` into a `String` is infallible.
                write!(s, "{} = \"{}\"", annotation.name, annotation.value)
                    .expect("String write is infallible");
            }
            s.push_str(" }");
            parts.push(s);
        }
    }

    // Python: `if fasm_line.comment is not None` — an `is not None` check,
    // not a truthiness check, so `Some("")` (a bare `#`) still renders.
    if let Some(comment) = &line.comment {
        parts.push(format!("#{comment}"));
    }

    Ok(vec![parts.join(" ")])
}

/// Renders `lines` to the complete text of a FASM file (always ending in a
/// single trailing `\n`, even for an empty `lines`, matching Python's
/// `'\n'.join(lines) + '\n'`).
///
/// Mirrors Python's `fasm_tuple_to_string` in `fasm/__init__.py`. If
/// `canonical` is `true`, the rendered lines are deduplicated and sorted
/// byte-wise (`String`'s `Ord`, which matches Python's `str` ordering for
/// the ASCII text canonical lines are always made of) before joining,
/// mirroring `sorted(set(lines))`.
///
/// Note that, like the Python docstring says: calling a parser and then
/// this function replaces all optional whitespace with a single space.
///
/// # Errors
///
/// See [`fasm_line_to_string`].
pub fn fasm_tuple_to_string<'a>(
    lines: impl IntoIterator<Item = &'a FasmLine>,
    canonical: bool,
) -> Result<String, OutputError> {
    let mut rendered: Vec<String> = Vec::new();
    for line in lines {
        rendered.extend(fasm_line_to_string(line, canonical)?);
    }

    if canonical {
        rendered.sort();
        rendered.dedup();
    }

    let mut out = rendered.join("\n");
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests;

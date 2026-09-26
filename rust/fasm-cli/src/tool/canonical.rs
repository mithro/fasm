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

//! `--canonical` output without materialising it: [`CanonicalLines`].
//!
//! The original tool prints `sorted(set(lines))` of the canonical lines
//! (`FEATURE` or `FEATURE[address]`, one per set bit). A wide `INIT` value
//! expands to hundreds of lines, so a large design has tens of millions of
//! them (27.2 million, 1.04 GB, for a 1 million line file with 256 bit
//! BRAM `INIT`s). Instead of formatting every line into memory and sorting
//! the strings, this keeps 8 bytes per line (an *entity* and an address),
//! sorts the distinct entities (a feature name alone, or a feature name
//! followed by `[`) by their text, orders the lines by entity with a
//! counting sort, sorts the addresses of each entity like the text
//! `digits]` that follows the `[`, and formats the lines only while
//! writing them.
//!
//! That is exactly the order of `sorted` as long as no feature name
//! contains `[`: a line is its entity's text followed by nothing (bare
//! feature) or by `digits]` (addressed feature). Two lines of different
//! entities therefore compare like the entities' texts, unless one entity
//! text is a proper prefix of the other. If the shorter one is a bare
//! feature, its line is a prefix of the other line and sorts first, like
//! its entity. If it ends with `[`, the longer text continues it, so the
//! longer entity's feature name contains a `[`. Feature names from the
//! parser never do; [`CanonicalLines::finish`] checks it anyway and
//! otherwise sorts the formatted lines (the previous method).

use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::{self, Write};

use fasm::idstring::IdString;
use fasm::SetFasmFeature;

/// The canonical lines of a file, collected with [`CanonicalLines::push`]
/// and ordered by [`CanonicalLines::finish`].
#[derive(Default)]
pub(super) struct CanonicalLines {
    /// Index of each distinct feature in `features`.
    index: HashMap<IdString, u32>,
    /// The distinct features, in order of appearance.
    features: Vec<IdString>,
    /// The feature pushed last and its index (the lines of one
    /// `SetFasmFeature` come one after the other).
    last: Option<(IdString, u32)>,
    /// Per line: its entity, `2 * feature index + 1` for
    /// `FEATURE[address]` and `2 * feature index` for a bare `FEATURE`.
    entities: Vec<u32>,
    /// Per line: its address (`0` for a bare feature).
    addresses: Vec<u32>,
}

/// The collected lines in output order, see [`Sorted::write_to`].
pub(super) struct Sorted {
    /// The text of every distinct feature, back to back.
    texts: String,
    /// End of each feature's text in `texts`.
    ends: Vec<usize>,
    lines: Lines,
}

enum Lines {
    /// The entities in text order; the addresses of entity `order[i]` are
    /// `addresses[starts[i]..starts[i + 1]]`, in text order (duplicates
    /// are next to each other and printed once).
    Grouped {
        order: Vec<u32>,
        starts: Vec<usize>,
        addresses: Vec<u32>,
    },
    /// The formatted lines, sorted and deduplicated (only when a feature
    /// name contains `[`).
    Formatted(Vec<Vec<u8>>),
}

impl CanonicalLines {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Adds the line of the canonical feature `feature`, as produced by
    /// `try_canonical_features` (value 1, no end, no value format, no
    /// `[0]` address).
    pub(super) fn push(&mut self, feature: &SetFasmFeature) {
        debug_assert!(feature.end.is_none() && feature.value_format.is_none());
        debug_assert_ne!(feature.start, Some(0));
        debug_assert!(feature.value.is_one());
        let index = match self.last {
            Some((id, index)) if id == feature.feature => index,
            _ => {
                let next = self.features.len();
                let index = *self.index.entry(feature.feature).or_insert_with(|| {
                    // `2 * index + 1` below must fit in a `u32`, so `index`
                    // (hence `next`) must stay below 2^31, not just below
                    // 2^32 (what `u32::try_from` alone would check).
                    assert!(next < 1 << 31, "fewer than 2^31 features");
                    next as u32
                });
                if index as usize == next {
                    self.features.push(feature.feature);
                }
                self.last = Some((feature.feature, index));
                index
            }
        };
        self.entities
            .push(2 * index + u32::from(feature.start.is_some()));
        self.addresses.push(feature.start.unwrap_or(0));
    }

    /// Orders the lines (sorted like their text, see the module
    /// documentation).
    pub(super) fn finish(self) -> Sorted {
        let mut texts = String::new();
        let mut ends = Vec::with_capacity(self.features.len());
        for feature in &self.features {
            feature.with_str(|s| texts.push_str(s));
            ends.push(texts.len());
        }
        let mut sorted = Sorted {
            texts,
            ends,
            lines: Lines::Formatted(Vec::new()),
        };
        if sorted.texts.contains('[') {
            let mut lines: Vec<Vec<u8>> = self
                .entities
                .iter()
                .zip(&self.addresses)
                .map(|(&entity, &address)| {
                    let mut line = Vec::new();
                    sorted.write_line(&mut line, entity, address);
                    line
                })
                .collect();
            lines.sort_unstable();
            lines.dedup();
            sorted.lines = Lines::Formatted(lines);
            return sorted;
        }

        // The entities that have lines, in text order.
        let entity_count = 2 * self.features.len();
        let mut rank = vec![u32::MAX; entity_count];
        for &entity in &self.entities {
            rank[entity as usize] = 0;
        }
        let mut order: Vec<u32> = (0..entity_count)
            .filter(|&entity| rank[entity] == 0)
            .map(|entity| u32::try_from(entity).expect("fewer than 2^32 entities"))
            .collect();
        order.sort_unstable_by(|&a, &b| sorted.cmp_entities(a, b));
        for (position, &entity) in order.iter().enumerate() {
            rank[entity as usize] = u32::try_from(position).expect("fewer than 2^32 entities");
        }

        // Counting sort of the lines by the rank of their entity.
        let mut starts = vec![0usize; order.len() + 1];
        for &entity in &self.entities {
            starts[rank[entity as usize] as usize + 1] += 1;
        }
        for i in 1..starts.len() {
            starts[i] += starts[i - 1];
        }
        let mut next = starts.clone();
        let mut addresses = vec![0u32; self.addresses.len()];
        for (&entity, &address) in self.entities.iter().zip(&self.addresses) {
            let slot = &mut next[rank[entity as usize] as usize];
            addresses[*slot] = address;
            *slot += 1;
        }
        drop((self.entities, self.addresses, rank, next, self.index));

        // The addresses of each entity in text order.
        let mut keyed: Vec<(u64, u32)> = Vec::new();
        for (entity, range) in order.iter().zip(starts.windows(2)) {
            let group = &mut addresses[range[0]..range[1]];
            if entity & 1 == 0 || group.len() < 2 {
                continue;
            }
            keyed.clear();
            keyed.extend(
                group
                    .iter()
                    .map(|&address| (text_order_key(address), address)),
            );
            keyed.sort_unstable();
            for (slot, (_, address)) in group.iter_mut().zip(&keyed) {
                *slot = *address;
            }
        }
        sorted.lines = Lines::Grouped {
            order,
            starts,
            addresses,
        };
        sorted
    }
}

/// A key that orders addresses like the text `digits]` that follows the
/// `[` of their lines: the decimal digits as base 11 digits, followed by
/// the digit 10 up to ten digits (`]` sorts after every decimal digit, and
/// what follows it does not matter).
fn text_order_key(address: u32) -> u64 {
    let digits = address.checked_ilog10().unwrap_or(0) + 1;
    let mut value = 0u64;
    let mut rest = address;
    let mut weight = 1u64;
    for _ in 0..digits {
        value += u64::from(rest % 10) * weight;
        rest /= 10;
        weight *= 11;
    }
    // Pad to ten digits with tens.
    let padding = 11u64.pow(10 - digits);
    value * padding + (padding - 1)
}

impl Sorted {
    /// The text of feature `index`.
    fn text(&self, index: u32) -> &[u8] {
        let index = index as usize;
        let start = index.checked_sub(1).map_or(0, |i| self.ends[i]);
        &self.texts.as_bytes()[start..self.ends[index]]
    }

    /// Compares the texts of two entities (a feature name, followed by
    /// `[` for an addressed entity).
    fn cmp_entities(&self, a: u32, b: u32) -> Ordering {
        let bracket = |entity: u32| -> &'static [u8] {
            if entity & 1 == 1 {
                b"["
            } else {
                b""
            }
        };
        let (text_a, text_b) = (self.text(a / 2), self.text(b / 2));
        let common = text_a.len().min(text_b.len());
        text_a[..common].cmp(&text_b[..common]).then_with(|| {
            text_a[common..]
                .iter()
                .chain(bracket(a))
                .cmp(text_b[common..].iter().chain(bracket(b)))
        })
    }

    /// Appends the line (without newline) of `entity` with `address`.
    fn write_line(&self, out: &mut Vec<u8>, entity: u32, address: u32) {
        out.extend_from_slice(self.text(entity / 2));
        if entity & 1 == 1 {
            let mut digits = [0u8; 10];
            let mut at = digits.len();
            let mut rest = address;
            loop {
                at -= 1;
                digits[at] = b'0' + (rest % 10) as u8;
                rest /= 10;
                if rest == 0 {
                    break;
                }
            }
            out.push(b'[');
            out.extend_from_slice(&digits[at..]);
            out.push(b']');
        }
    }

    /// Writes the lines like `print('\n'.join(lines))` (so a lone `\n` for
    /// no lines), followed by the extra newline of `print`, in blocks of
    /// about 64 KiB.
    pub(super) fn write_to(&self, out: &mut dyn Write) -> io::Result<()> {
        const BLOCK: usize = 64 * 1024;
        let mut buffer: Vec<u8> = Vec::with_capacity(BLOCK + 256);
        let mut any = false;
        let mut emit = |buffer: &mut Vec<u8>, out: &mut dyn Write| -> io::Result<()> {
            buffer.push(b'\n');
            any = true;
            if buffer.len() >= BLOCK {
                out.write_all(buffer)?;
                buffer.clear();
            }
            Ok(())
        };
        match &self.lines {
            Lines::Grouped {
                order,
                starts,
                addresses,
            } => {
                for (&entity, range) in order.iter().zip(starts.windows(2)) {
                    let mut previous = None;
                    for &address in &addresses[range[0]..range[1]] {
                        if previous == Some(address) {
                            continue;
                        }
                        previous = Some(address);
                        self.write_line(&mut buffer, entity, address);
                        emit(&mut buffer, out)?;
                    }
                }
            }
            Lines::Formatted(lines) => {
                for line in lines {
                    buffer.extend_from_slice(line);
                    emit(&mut buffer, out)?;
                }
            }
        }
        if !any {
            buffer.push(b'\n');
        }
        buffer.push(b'\n');
        out.write_all(&buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bare `push` of `feature` (no address), matching what
    /// `try_canonical_features` produces for a `FEATURE` line.
    fn bare(feature: &str) -> SetFasmFeature {
        SetFasmFeature::new(
            IdString::from(feature),
            None,
            None,
            fasm::model::FeatureValue::from_u64(1),
            None,
        )
        .expect("valid bare feature")
    }

    /// A `push` of `feature[address]`, matching what
    /// `try_canonical_features` produces for an addressed line.
    fn addressed(feature: &str, address: u32) -> SetFasmFeature {
        SetFasmFeature::new(
            IdString::from(feature),
            Some(address),
            None,
            fasm::model::FeatureValue::from_u64(1),
            None,
        )
        .expect("valid addressed feature")
    }

    /// Renders the lines of `sorted` (module documentation order) as
    /// `String`s, for comparing against `sorted(set(...))` of the input.
    fn rendered(sorted: &Sorted) -> Vec<String> {
        let mut out = Vec::new();
        sorted
            .write_to(&mut out)
            .expect("writing to a Vec never fails");
        let text = String::from_utf8(out).expect("ASCII/UTF-8 lines only");
        // `write_to` always appends a trailing blank line (like `print`);
        // drop the final empty entry from the trailing "\n\n".
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        if lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines
    }

    /// A feature name containing `[` takes the "sort the formatted lines"
    /// fallback path (module documentation); check it against the plain
    /// `sorted(set(...))` of the formatted lines it should produce.
    #[test]
    fn bracket_in_feature_name_falls_back_to_sorting_formatted_lines() {
        let mut lines = CanonicalLines::new();
        // `A[` and `A[1]x` are feature names containing `[` (never
        // produced by the parser, but not rejected by it either); push
        // some duplicates and some ordinary addressed/bare features too.
        lines.push(&bare("A["));
        lines.push(&addressed("A[1]x", 5));
        lines.push(&addressed("A[1]x", 5)); // duplicate, must be deduplicated
        lines.push(&bare("B"));
        lines.push(&addressed("B", 2));
        lines.push(&addressed("B", 10));
        lines.push(&bare("A["));

        let mut expected: Vec<String> = vec![
            "A[".to_string(),
            "A[1]x[5]".to_string(),
            "B".to_string(),
            "B[2]".to_string(),
            "B[10]".to_string(),
        ];
        expected.sort();
        expected.dedup();

        let sorted = lines.finish();
        assert!(
            matches!(sorted.lines, Lines::Formatted(_)),
            "a `[` in a feature name must take the formatted-lines fallback"
        );
        assert_eq!(rendered(&sorted), expected);
    }

    #[test]
    fn text_order_key_orders_like_the_text() {
        let mut addresses: Vec<u32> = (0..2000).collect();
        addresses.extend([
            9_999,
            10_000,
            99_999,
            100_000,
            u32::MAX,
            u32::MAX - 1,
            4_000_000_000,
            1_000_000_000,
            999_999_999,
        ]);
        let mut by_key = addresses.clone();
        by_key.sort_by_key(|&a| text_order_key(a));
        let mut by_text = addresses.clone();
        by_text.sort_by_key(|&a| format!("{a}]"));
        assert_eq!(by_key, by_text);
        // Distinct addresses have distinct keys.
        let mut keys: Vec<u64> = addresses.iter().map(|&a| text_order_key(a)).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), addresses.len());
    }
}

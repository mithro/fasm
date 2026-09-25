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

//! Interned, 8 byte handles for hierarchical dotted strings such as FASM
//! feature names.
//!
//! Following <https://github.com/mithro/idstring>, a name such as
//! `CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT` is split on `.` into three levels:
//! the first component (`CLBLL_L_X12Y124`), the second component
//! (`SLICEL_X0`) and the remainder (`BLUT.INIT`). Each level is interned in
//! its own table and an [`IdString`] is a single `u64` holding the three
//! table indexes. Tile, site and bel names are shared by very many features,
//! so the tables stay small while every feature costs exactly 8 bytes.
//!
//! * `Eq` and `Hash` are integer operations: every string has exactly one
//!   handle.
//! * `Ord` compares by string value (like `str`); it reads the tables, but
//!   skips leading levels whose indexes are equal and never allocates.
//! * Any string round trips exactly (empty string, leading, trailing or
//!   repeated dots, any number of components, any Unicode).
//! * Resolving a handle takes no lock and returns `&'static str` pieces
//!   (interned text is never freed).
//!
//! ```
//! use fasm::idstring::IdString;
//!
//! let feature = IdString::new("CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT");
//! assert_eq!(feature, IdString::new("CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT"));
//! assert_eq!(feature, "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT");
//! assert_eq!(feature.to_string(), "CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT");
//! assert_eq!(feature.first_component(), "CLBLL_L_X12Y124");
//! assert!(feature.starts_with_component("CLBLL_L_X12Y124.SLICEL_X0"));
//! assert_eq!(
//!     feature.components().collect::<Vec<_>>(),
//!     ["CLBLL_L_X12Y124", "SLICEL_X0", "BLUT", "INIT"],
//! );
//! assert!(IdString::new("A.B") < IdString::new("A.B.C"));
//! assert_eq!(std::mem::size_of::<Option<IdString>>(), 8);
//! ```
//!
//! See `docs/rewrite/DESIGN-idstring.md` for the layout, the measurements
//! behind it and the behaviour when a level table overflows.

use std::cmp::Ordering;
use std::convert::Infallible;
use std::fmt;
use std::num::NonZeroU64;
use std::str::{FromStr, Utf8Error};

mod interner;
mod repr;
mod resolved;
mod storage;

pub use interner::{Interner, InternerStats};
pub use resolved::Resolved;

/// The process wide interner used by all [`IdString`] methods.
pub static GLOBAL: Interner = Interner::new();

/// Sorts `items` by the string of `key(item)` (a [`GLOBAL`] handle), like
/// `items.sort_by_key(key)` (stable, and `IdString`'s `Ord` is string
/// order) but about twice as fast for many items: every key is resolved
/// once, into one buffer, and the sort compares those bytes instead of
/// reading the interner tables on every comparison.
///
/// ```
/// # use fasm::idstring::{sort_by_string, IdString};
/// let mut pairs = vec![("B.X", 1), ("A.Y", 2), ("A.X", 3), ("A.Y", 4)];
/// sort_by_string(&mut pairs, |&(name, _)| IdString::new(name));
/// assert_eq!(pairs, [("A.X", 3), ("A.Y", 2), ("A.Y", 4), ("B.X", 1)]);
/// ```
///
/// # Panics
///
/// Like `Ord`, panics for a handle not created by [`GLOBAL`].
pub fn sort_by_string<T>(items: &mut Vec<T>, key: impl Fn(&T) -> IdString) {
    let mut text = String::new();
    // (start, end, index) of every key's text; 32 bit offsets keep the
    // entries small (see the fallback below).
    let mut keyed: Vec<(u32, u32, u32)> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let start = text.len();
        key(item).with_str(|s| text.push_str(s));
        match (
            u32::try_from(start),
            u32::try_from(text.len()),
            u32::try_from(index),
        ) {
            (Ok(start), Ok(end), Ok(index)) => keyed.push((start, end, index)),
            // More than 4 GiB of text: sort with `Ord` (same order).
            _ => return items.sort_by_key(|item| key(item)),
        }
    }
    let bytes = text.as_bytes();
    let range = |&(start, end, _): &(u32, u32, u32)| &bytes[start as usize..end as usize];
    // The index breaks ties: stable.
    keyed.sort_unstable_by(|a, b| range(a).cmp(range(b)).then(a.2.cmp(&b.2)));
    let mut slots: Vec<Option<T>> = items.drain(..).map(Some).collect();
    items.extend(
        keyed
            .iter()
            .filter_map(|&(_, _, index)| slots.get_mut(index as usize).and_then(Option::take)),
    );
}

/// An interned string, stored as a single 8 byte integer.
///
/// Created with [`IdString::new`] (or `From<&str>` / `FromStr`) in the
/// process wide interner [`GLOBAL`]. All methods and trait implementations
/// of `IdString` use [`GLOBAL`]; handles created by a private
/// [`Interner`] must be resolved and compared through that interner's own
/// methods instead.
///
/// `Option<IdString>` is also 8 bytes.
///
/// # Handle values depend on interning order
///
/// `Eq` and `Hash` work on the 8 byte value, which is made of table entry
/// numbers handed out in first-come order. The same string therefore gets
/// a different value in another run (or with another thread schedule), so
/// the raw value, the `Hash` output and the iteration order of a
/// `HashMap<IdString, _>` / `HashSet<IdString>` are **not deterministic
/// across runs**. Anything that must be reproducible (sorted output, a
/// canonical file) has to be ordered with `Ord`, which compares the
/// strings (e.g. collect into a `BTreeMap` or sort a `Vec<IdString>`).
///
/// ```
/// # use fasm::idstring::IdString;
/// let mut ids: Vec<IdString> = ["B.X", "A.Y", "A.X"].map(IdString::new).into();
/// ids.sort(); // by string value, independent of interning order
/// assert_eq!(ids, ["A.X", "A.Y", "B.X"]);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct IdString(NonZeroU64);

const _: () = assert!(size_of::<IdString>() == 8);
const _: () = assert!(size_of::<Option<IdString>>() == 8);

impl IdString {
    pub(crate) const fn from_raw(raw: NonZeroU64) -> Self {
        IdString(raw)
    }

    pub(crate) const fn raw(self) -> NonZeroU64 {
        self.0
    }

    /// Interns `s` in [`GLOBAL`] and returns its handle (see
    /// [`Interner::intern`]).
    #[inline]
    pub fn new(s: &str) -> Self {
        GLOBAL.intern(s)
    }

    /// Interns UTF-8 `bytes` in [`GLOBAL`] (see [`Interner::intern_bytes`]).
    ///
    /// Validates `bytes` only if the string is not known yet, so this is
    /// the fastest way to intern names read from a byte buffer: known
    /// names cost the same as [`IdString::new`], without a separate
    /// `std::str::from_utf8` pass.
    ///
    /// # Errors
    ///
    /// Returns the UTF-8 error if `bytes` is not valid UTF-8.
    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Utf8Error> {
        GLOBAL.intern_bytes(bytes)
    }

    /// [`IdString::from_bytes`] for a name whose first two `.` are at
    /// `dots[0]` and `dots[1]` (`usize::MAX` where there is none); see
    /// [`Interner::intern_split`].
    #[inline]
    pub(crate) fn from_split(bytes: &[u8], dots: [usize; 2]) -> Result<Self, Utf8Error> {
        GLOBAL.intern_split(bytes, dots)
    }

    /// Returns the handle `s` would have in [`GLOBAL`], if it can be
    /// produced without interning anything (see [`Interner::lookup`]).
    ///
    /// **Not a membership test**: this is also `Some` for strings that were
    /// never interned whole but whose levels are all known (after `A.B.C`
    /// and `X.Y`, `IdString::lookup("A.Y")` is `Some`).
    #[inline]
    pub fn lookup(s: &str) -> Option<Self> {
        GLOBAL.lookup(s)
    }

    /// Returns the interned pieces of the string, on which several string
    /// operations can be done without resolving the handle again.
    ///
    /// # Panics
    ///
    /// Like every method that reads the string (`resolve`, `with_str`,
    /// `components`, `len`, ...), this resolves the handle in the
    /// **global** interner [`GLOBAL`] and panics for a handle created by a
    /// private [`Interner`] that has no entry there; use the interner's
    /// own methods for such handles.
    pub fn resolved(self) -> Resolved {
        GLOBAL.resolved(self)
    }

    /// Returns the string as a new `String`.
    pub fn resolve(self) -> String {
        self.resolved().into_string()
    }

    /// Calls `f` with the string, without heap allocation in steady state
    /// for strings up to 1024 bytes (see [`Resolved::with_str`]).
    pub fn with_str<R>(self, f: impl FnOnce(&str) -> R) -> R {
        self.resolved().with_str(f)
    }

    /// Iterates over the `.` separated components of the string.
    ///
    /// ```
    /// # use fasm::idstring::IdString;
    /// let id = IdString::new("INT_L_X10Y146.SW6BEG0.WW2END0");
    /// assert_eq!(id.components().collect::<Vec<_>>(), ["INT_L_X10Y146", "SW6BEG0", "WW2END0"]);
    /// assert_eq!(IdString::new("").components().collect::<Vec<_>>(), [""]);
    /// ```
    pub fn components(self) -> impl Iterator<Item = &'static str> {
        self.resolved().components()
    }

    /// The first `.` separated component (the whole string if it has no
    /// `.`).
    pub fn first_component(self) -> &'static str {
        self.resolved().first_component()
    }

    /// Returns `true` if the string equals `prefix` or starts with `prefix`
    /// followed by `.` (see [`Resolved::starts_with_component`]).
    ///
    /// ```
    /// # use fasm::idstring::IdString;
    /// let id = IdString::new("CLBLL_L_X12Y124.SLICEL_X0.BLUT.INIT");
    /// assert!(id.starts_with_component("CLBLL_L_X12Y124"));
    /// assert!(id.starts_with_component("CLBLL_L_X12Y124.SLICEL_X0.BLUT"));
    /// assert!(!id.starts_with_component("CLBLL_L_X12Y124.SLICEL"));
    /// ```
    pub fn starts_with_component(self, prefix: &str) -> bool {
        self.resolved().starts_with_component(prefix)
    }

    /// Length of the string in bytes.
    pub fn len(self) -> usize {
        self.resolved().len()
    }

    /// Returns `true` for the empty string.
    pub fn is_empty(self) -> bool {
        self.resolved().is_empty()
    }
}

/// Writes the string (honouring width, fill, alignment and precision like
/// `str`).
///
/// # Panics
///
/// Resolves the handle in the **global** interner [`GLOBAL`]: panics for a
/// handle created by a private [`Interner`] that has no entry in
/// [`GLOBAL`] (and prints an unrelated string if it has one). Format such
/// handles through [`Interner::resolved`] instead.
impl fmt::Display for IdString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.resolved(), f)
    }
}

/// Writes `IdString("...")`.
///
/// # Panics
///
/// Like `Display`, resolves the handle in the **global** interner
/// [`GLOBAL`] and panics for a handle of a private [`Interner`] that has no
/// entry there.
impl fmt::Debug for IdString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("IdString").field(&self.resolved()).finish()
    }
}

/// The order of `Ord`.
///
/// # Panics
///
/// Like `Ord`, reads the tables of the **global** interner [`GLOBAL`] and
/// panics for handles of a private [`Interner`] missing from it.
impl PartialOrd for IdString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Orders by string value, byte by byte like `str` (see
/// [`Interner::cmp`]). Unlike the handle value used by `Eq` and `Hash`,
/// this order does not depend on the interning order: use it for
/// reproducible output.
///
/// # Panics
///
/// Reads the tables of the **global** interner [`GLOBAL`]: comparing
/// handles created by a private [`Interner`] panics if an entry is missing
/// from [`GLOBAL`] (or silently compares unrelated strings). Compare such
/// handles with [`Interner::cmp`] instead.
impl Ord for IdString {
    fn cmp(&self, other: &Self) -> Ordering {
        GLOBAL.cmp(*self, *other)
    }
}

impl From<&str> for IdString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl FromStr for IdString {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

/// Compares the string with a `str` (without allocating).
///
/// # Panics
///
/// Resolves the handle in the **global** interner [`GLOBAL`]: panics for a
/// handle created by a private [`Interner`] that has no entry there (and
/// compares an unrelated string if it has one). Compare such handles with
/// `interner.resolved(id) == s` instead.
impl PartialEq<str> for IdString {
    fn eq(&self, other: &str) -> bool {
        self.resolved() == *other
    }
}

/// See `PartialEq<str> for IdString` (resolves through [`GLOBAL`], panics
/// for handles of a private [`Interner`]).
impl PartialEq<&str> for IdString {
    fn eq(&self, other: &&str) -> bool {
        self.resolved() == **other
    }
}

/// See `PartialEq<str> for IdString` (resolves through [`GLOBAL`], panics
/// for handles of a private [`Interner`]).
impl PartialEq<IdString> for str {
    fn eq(&self, other: &IdString) -> bool {
        other == self
    }
}

/// See `PartialEq<str> for IdString` (resolves through [`GLOBAL`], panics
/// for handles of a private [`Interner`]).
impl PartialEq<IdString> for &str {
    fn eq(&self, other: &IdString) -> bool {
        other == self
    }
}

#[cfg(test)]
mod tests;

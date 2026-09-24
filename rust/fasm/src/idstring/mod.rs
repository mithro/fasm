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
// The overflow encoding is used by the overflow table (follow up change).
#[allow(dead_code)]
mod repr;
mod resolved;
mod storage;

pub use interner::Interner;
pub use resolved::Resolved;

/// The process wide interner used by all [`IdString`] methods.
pub static GLOBAL: Interner = Interner::new();

/// An interned string, stored as a single 8 byte integer.
///
/// Created with [`IdString::new`] (or `From<&str>` / `FromStr`) in the
/// process wide interner [`GLOBAL`]. All methods and trait implementations
/// of `IdString` use [`GLOBAL`]; handles created by a private
/// [`Interner`] must be resolved and compared through that interner's own
/// methods instead.
///
/// `Option<IdString>` is also 8 bytes.
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

    /// Interns `s` in [`GLOBAL`] and returns its handle.
    pub fn new(s: &str) -> Self {
        GLOBAL.intern(s)
    }

    /// Interns UTF-8 `bytes` in [`GLOBAL`].
    ///
    /// # Errors
    ///
    /// Returns the UTF-8 error if `bytes` is not valid UTF-8.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Utf8Error> {
        std::str::from_utf8(bytes).map(Self::new)
    }

    /// Returns the handle of `s` if it has already been interned in
    /// [`GLOBAL`], without interning it.
    pub fn get(s: &str) -> Option<Self> {
        GLOBAL.get(s)
    }

    /// Returns the interned pieces of the string, on which several string
    /// operations can be done without resolving the handle again.
    pub fn resolved(self) -> Resolved {
        GLOBAL.resolved(self)
    }

    /// Returns the string as a new `String`.
    pub fn resolve(self) -> String {
        self.resolved().into_string()
    }

    /// Calls `f` with the string, without heap allocation when the string
    /// is at most 256 bytes long (see [`Resolved::with_str`]).
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

impl fmt::Display for IdString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.resolved(), f)
    }
}

impl fmt::Debug for IdString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("IdString").field(&self.resolved()).finish()
    }
}

impl PartialOrd for IdString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Orders by string value (see [`Interner::cmp`]); reads the tables of
/// [`GLOBAL`].
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

impl PartialEq<str> for IdString {
    fn eq(&self, other: &str) -> bool {
        self.resolved() == *other
    }
}

impl PartialEq<&str> for IdString {
    fn eq(&self, other: &&str) -> bool {
        self.resolved() == **other
    }
}

impl PartialEq<IdString> for str {
    fn eq(&self, other: &IdString) -> bool {
        other == self
    }
}

impl PartialEq<IdString> for &str {
    fn eq(&self, other: &IdString) -> bool {
        other == self
    }
}

#[cfg(test)]
mod tests;

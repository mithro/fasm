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
//! feature names (see `docs/rewrite/DESIGN-idstring.md`).

// The storage layer and the handle layout are used by the interner added
// in a follow up change.
#[allow(dead_code)]
mod repr;
#[allow(dead_code)]
mod resolved;
#[allow(dead_code)]
mod storage;

pub use resolved::Resolved;

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

//! Scoped threads that fall back to the calling thread.
//!
//! The cache only uses threads to be faster; `std::thread::Scope::spawn`
//! panics when the operating system refuses a new thread (a thread limit),
//! which must not make opening a database fail. [`spawn`] then runs the
//! work right away on the calling thread instead.

use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{Builder, Scope, ScopedJoinHandle};

/// Work started by [`spawn`].
pub(crate) enum Task<'scope, T> {
    /// Running on a scoped thread (it returns `None` only if it found no
    /// work, which cannot happen).
    Thread(ScopedJoinHandle<'scope, Option<T>>),
    /// Already done on the calling thread (`None`: cannot happen either).
    Done(Option<T>),
}

impl<T> Task<'_, T> {
    /// The result, `None` if the thread panicked.
    pub(crate) fn join(self) -> Option<T> {
        match self {
            Task::Thread(handle) => handle.join().ok().flatten(),
            Task::Done(value) => value,
        }
    }
}

/// Runs `f` on a new thread of `scope`, or on this thread if no thread
/// can be started.
pub(crate) fn spawn<'scope, 'env, T, F>(scope: &'scope Scope<'scope, 'env>, f: F) -> Task<'scope, T>
where
    T: Send + 'scope,
    F: FnOnce() -> T + Send + 'scope,
{
    // The closure is only lent to the thread, so that it can be run here
    // when the thread cannot be started (`spawn_scoped` drops it then).
    let slot = Arc::new(Mutex::new(Some(f)));
    let lent = Arc::clone(&slot);
    let started = Builder::new().spawn_scoped(scope, move || {
        let f = lent.lock().unwrap_or_else(PoisonError::into_inner).take();
        f.map(|f| f())
    });
    match started {
        Ok(handle) => Task::Thread(handle),
        Err(_) => {
            let f = slot.lock().unwrap_or_else(PoisonError::into_inner).take();
            Task::Done(f.map(|f| f()))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn spawn_and_join() {
        let data = [1, 2, 3];
        let (a, b) = std::thread::scope(|scope| {
            let a = super::spawn(scope, || data.iter().sum::<i32>());
            let b = super::spawn(scope, || data.len());
            (a.join(), b.join())
        });
        assert_eq!((a, b), (Some(6), Some(3)));
        let done = super::Task::Done(Some(1)).join();
        assert_eq!(done, Some(1));
    }
}

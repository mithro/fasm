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

//! The terminal width argparse formats its help and usage messages for:
//! Python's `shutil.get_terminal_size().columns`.

use crate::pystr::{py_int, PyStr};

/// `shutil.get_terminal_size()`'s fallback width.
const FALLBACK_COLUMNS: i64 = 80;

/// Python's `shutil.get_terminal_size().columns`, given the value of the
/// `COLUMNS` environment variable and a function returning the width of
/// the terminal on stdout (`None` when stdout is not a terminal; the
/// function is only called when `COLUMNS` does not give a positive width).
///
/// `COLUMNS` is used if `int()` accepts it and it is positive; otherwise
/// the width of the terminal on stdout, if it is a terminal of non zero
/// width; otherwise 80. (Python also reads `LINES`, which only changes
/// whether it queries the terminal, not the resulting width.)
pub fn columns_from(
    columns_env: Option<&PyStr>,
    stdout_width: impl FnOnce() -> Option<u16>,
) -> i64 {
    if let Some(columns) = columns_env.and_then(py_int) {
        if columns > 0 {
            return columns;
        }
    }
    match stdout_width() {
        Some(width) if width > 0 => i64::from(width),
        _ => FALLBACK_COLUMNS,
    }
}

/// [`columns_from`] for this process: reads `COLUMNS` and queries the
/// terminal on file descriptor 1.
pub fn columns() -> i64 {
    let env = std::env::var_os("COLUMNS").map(|v| PyStr::from_os_str(&v));
    columns_from(env.as_ref(), stdout_terminal_width)
}

/// The width of the terminal on stdout (`os.get_terminal_size(1)`), or
/// `None` if stdout is not a terminal.
#[cfg(unix)]
#[allow(unsafe_code)]
fn stdout_terminal_width() -> Option<u16> {
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: TIOCGWINSZ writes a `winsize` through the pointer, which
    // points to a live, properly aligned `winsize`; the call has no other
    // effect. An invalid or non terminal descriptor makes it fail with -1.
    let result = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &raw mut size) };
    (result == 0).then_some(size.ws_col)
}

/// The width of the terminal on stdout; not implemented on this platform
/// (Python's fallback width is used).
#[cfg(not(unix))]
fn stdout_terminal_width() -> Option<u16> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(s: &str) -> PyStr {
        PyStr::from_str(s)
    }

    #[test]
    fn columns_env_wins_when_positive() {
        assert_eq!(columns_from(Some(&env("120")), || Some(50)), 120);
        assert_eq!(columns_from(Some(&env(" 1_0 ")), || Some(50)), 10);
        assert_eq!(columns_from(Some(&env("1")), || None), 1);
    }

    #[test]
    fn falls_back_to_terminal_then_80() {
        assert_eq!(columns_from(None, || Some(50)), 50);
        assert_eq!(columns_from(Some(&env("0")), || Some(50)), 50);
        assert_eq!(columns_from(Some(&env("-3")), || Some(50)), 50);
        assert_eq!(columns_from(Some(&env("abc")), || Some(50)), 50);
        assert_eq!(columns_from(None, || Some(0)), 80);
        assert_eq!(columns_from(None, || None), 80);
    }
}

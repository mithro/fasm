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

//! Checks that the checked in `include/fasm/fasm.h` is what cbindgen
//! generates from the current sources.
//!
//! cbindgen is not a build dependency (the header is generated explicitly
//! with `make capi-header`), so this test is skipped with a message when
//! no `cbindgen` executable is found, unless `FASM_REQUIRE_CBINDGEN=1` is
//! set (as `make capi-header-check` does), in which case a missing cbindgen
//! fails the test. The executable is taken from `$CBINDGEN`, then `PATH`,
//! then `$CARGO_HOME/bin` (default `~/.cargo/bin`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository root.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Whether `cmd --version` runs.
fn runs(cmd: &Path) -> bool {
    Command::new(cmd)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Finds a working cbindgen executable.
fn find_cbindgen() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("CBINDGEN") {
        return Some(PathBuf::from(explicit));
    }
    let on_path = PathBuf::from("cbindgen");
    if runs(&on_path) {
        return Some(on_path);
    }
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))?;
    let installed = cargo_home
        .join("bin")
        .join(format!("cbindgen{}", std::env::consts::EXE_SUFFIX));
    runs(&installed).then_some(installed)
}

#[test]
fn header_is_up_to_date() {
    let required = std::env::var_os("FASM_REQUIRE_CBINDGEN").is_some_and(|v| v == "1");
    let Some(cbindgen) = find_cbindgen() else {
        assert!(
            !required,
            "FASM_REQUIRE_CBINDGEN=1 but no cbindgen found (install with \
             `cargo install cbindgen --locked`)"
        );
        eprintln!("SKIPPED: cbindgen not found; include/fasm/fasm.h not checked");
        return;
    };

    let root = repo_root();
    let generated = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fasm.h");
    let output = Command::new(&cbindgen)
        .current_dir(&root)
        .args(["--config", "rust/fasm-capi/cbindgen.toml"])
        .args(["--crate", "fasm-capi", "--quiet", "--output"])
        .arg(&generated)
        .output()
        .expect("running cbindgen");
    assert!(
        output.status.success(),
        "cbindgen failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let expected = std::fs::read_to_string(&generated).expect("reading the generated header");
    let actual = std::fs::read_to_string(root.join("include/fasm/fasm.h"))
        .expect("reading include/fasm/fasm.h");
    // Compare modulo line endings (a Windows checkout may use CRLF).
    assert!(
        actual.replace("\r\n", "\n") == expected.replace("\r\n", "\n"),
        "include/fasm/fasm.h is out of date: run `make capi-header` (freshly generated \
         header: {})",
        generated.display()
    );
}

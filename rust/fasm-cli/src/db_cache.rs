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

//! The `fasm-db-cache` tool: maintenance of the binary database cache of
//! [`fasm_xilinx::cache`] (a Rust only tool, with a plain argument
//! parser).
//!
//! ```text
//! usage: fasm-db-cache [--cache-dir DIR] COMMAND [ARGS]
//! ```
//!
//! See [`USAGE`] for the commands. Exit codes: 0 on success, 1 when a
//! command failed (a build, a verification, a file that cannot be read or
//! removed, or a disabled cache), 2 for a usage error.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use fasm_xilinx::cache::{self, CacheInfo, CacheOptions, SourceKind};

/// The help text.
pub const USAGE: &str = "\
usage: fasm-db-cache [--cache-dir DIR] COMMAND [ARGS]

Maintains the binary cache of prjxray-db / prjuray-db part databases that
fasm2frames and xcfasm load instead of the text files.

commands:
  build DB_ROOT PART...  load the parts from the text files and (re)write
                         their cache files
  build --all DB_ROOT    the same for every part of the family
                         (mapping/parts.yaml, or every PART/tilegrid.json)
  verify [FILE...]       fully check cache files (default: all in the cache
                         directory): file hashes, loader version and the
                         content hash of every source file
  info [FILE...]         show the headers of cache files (default: all)
  list                   list the cache files
  clear                  remove every cache file of the cache directory

options:
  --cache-dir DIR        the cache directory (default: $FASM_XDB_CACHE, else
                         $XDG_CACHE_HOME/fasm/db, else ~/.cache/fasm/db)
  -h, --help             show this help

exit codes: 0 success, 1 failure, 2 usage error
";

/// A usage error.
struct Usage(String);

/// The parsed command line.
struct Args {
    cache_dir: Option<PathBuf>,
    command: String,
    rest: Vec<OsString>,
}

fn parse(args: &[OsString]) -> Result<Option<Args>, Usage> {
    let mut cache_dir = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "-h" || arg == "--help" {
            return Ok(None);
        }
        if arg == "--cache-dir" {
            let dir = args
                .get(i + 1)
                .ok_or_else(|| Usage("--cache-dir needs a directory".to_owned()))?;
            cache_dir = Some(PathBuf::from(dir));
            i += 2;
        } else if let Some(dir) = arg.strip_prefix("--cache-dir=") {
            cache_dir = Some(PathBuf::from(dir));
            i += 1;
        } else if arg.starts_with('-') {
            return Err(Usage(format!("unknown option {arg}")));
        } else {
            break;
        }
    }
    let command = args
        .get(i)
        .ok_or_else(|| Usage("no command given".to_owned()))?
        .to_string_lossy()
        .into_owned();
    let rest = args[i + 1..].to_vec();
    if rest.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(None);
    }
    Ok(Some(Args {
        cache_dir,
        command,
        rest,
    }))
}

/// Runs the tool with the command line arguments `args` (without the
/// program name); `env` gives the cache directory when `--cache-dir` is
/// not used ([`CacheOptions::from_env`] for the process). Returns the
/// exit code.
pub fn run(
    args: &[OsString],
    env: &CacheOptions,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let usage_error = |stderr: &mut dyn Write, message: &str| {
        let _ = write!(stderr, "{USAGE}\nfasm-db-cache: error: {message}\n");
        2
    };
    let args = match parse(args) {
        Ok(Some(args)) => args,
        Ok(None) => {
            let _ = stdout.write_all(USAGE.as_bytes());
            return 0;
        }
        Err(Usage(message)) => return usage_error(stderr, &message),
    };
    // Argument counts are checked before the directory, so that a usage
    // error is always exit code 2.
    let rest = &args.rest;
    let all = rest.first().is_some_and(|a| a == "--all");
    let arity_ok = match args.command.as_str() {
        "build" if all => rest.len() == 2,
        "build" => rest.len() >= 2 && !rest.iter().any(|a| a.to_string_lossy().starts_with('-')),
        "verify" | "info" => true,
        "list" | "clear" => rest.is_empty(),
        other => return usage_error(stderr, &format!("unknown command {other:?}")),
    };
    if !arity_ok {
        return usage_error(
            stderr,
            &format!("wrong arguments for {:?}", args.command.as_str()),
        );
    }
    let Some(dir) = args.cache_dir.clone().or_else(|| env.directory.clone()) else {
        let _ = writeln!(
            stderr,
            "fasm-db-cache: the cache is disabled ({}=0) and no --cache-dir was given",
            cache::CACHE_DIR_ENV
        );
        return 1;
    };
    let result = match args.command.as_str() {
        "build" if all => build_all(&dir, Path::new(&rest[1]), stdout, stderr),
        "build" => {
            let parts: Vec<String> = rest[1..]
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            build(&dir, Path::new(&rest[0]), &parts, stdout, stderr)
        }
        "verify" => files(&dir, rest).and_then(|files| verify(&files, stdout)),
        "info" => files(&dir, rest).and_then(|files| info(&files, stdout)),
        "list" => files(&dir, rest).and_then(|files| list(&files, stdout)),
        "clear" => cache::clear(&dir)
            .map(|n| {
                let _ = writeln!(stdout, "removed {n} files from {}", dir.display());
                true
            })
            .map_err(|e| format!("{}: {e}", dir.display())),
        _ => unreachable!("checked above"),
    };
    let code = match result {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(message) => {
            let _ = writeln!(stderr, "fasm-db-cache: {message}");
            1
        }
    };
    let _ = stdout.flush();
    let _ = stderr.flush();
    code
}

/// The files given, or every cache file of `dir`.
fn files(dir: &Path, given: &[OsString]) -> Result<Vec<PathBuf>, String> {
    if given.is_empty() {
        cache::cache_files(dir).map_err(|e| format!("{}: {e}", dir.display()))
    } else {
        Ok(given.iter().map(PathBuf::from).collect())
    }
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn build(
    dir: &Path,
    root: &Path,
    parts: &[String],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<bool, String> {
    let mut ok = true;
    for part in parts {
        let start = Instant::now();
        match cache::build(dir, root, part) {
            Ok(info) => {
                let file = cache::cache_file(dir, root, part).unwrap_or_default();
                let _ = writeln!(
                    stdout,
                    "{}: {part} ({} sources, {:.1} MiB) in {:.0} ms",
                    file.display(),
                    info.sources.len(),
                    mib(info.file_len),
                    start.elapsed().as_secs_f64() * 1e3
                );
            }
            Err(e) => {
                ok = false;
                let _ = writeln!(stderr, "fasm-db-cache: {part}: {e}");
            }
        }
    }
    Ok(ok)
}

fn build_all(
    dir: &Path,
    root: &Path,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<bool, String> {
    let parts = cache::family_parts(root).map_err(|e| e.to_string())?;
    build(dir, root, &parts, stdout, stderr)
}

fn verify(files: &[PathBuf], stdout: &mut dyn Write) -> Result<bool, String> {
    let mut ok = true;
    for file in files {
        let report = cache::verify_file(file);
        match report.problem {
            None => {
                let _ = writeln!(stdout, "{}: ok", file.display());
            }
            Some(problem) => {
                ok = false;
                let _ = writeln!(stdout, "{}: {problem}", file.display());
            }
        }
    }
    Ok(ok)
}

fn list(files: &[PathBuf], stdout: &mut dyn Write) -> Result<bool, String> {
    let mut ok = true;
    for file in files {
        match cache::read_info(file) {
            Ok(i) => {
                let _ = writeln!(
                    stdout,
                    "{}\t{}\t{}\t{:.1} MiB\t{}",
                    file.display(),
                    i.part,
                    layout_name(&i),
                    mib(i.file_len),
                    i.db_root.display()
                );
            }
            Err(e) => {
                ok = false;
                let _ = writeln!(stdout, "{}\tunreadable: {e}", file.display());
            }
        }
    }
    Ok(ok)
}

fn layout_name(info: &CacheInfo) -> &'static str {
    match info.layout {
        fasm_xilinx::Layout::Prjxray => "prjxray",
        fasm_xilinx::Layout::Prjuray => "prjuray",
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn info(files: &[PathBuf], stdout: &mut dyn Write) -> Result<bool, String> {
    let mut ok = true;
    for file in files {
        let i = match cache::read_info(file) {
            Ok(i) => i,
            Err(e) => {
                ok = false;
                let _ = writeln!(stdout, "{}: unreadable: {e}\n", file.display());
                continue;
            }
        };
        let mut text = String::new();
        let _ = writeln!(text, "{}", file.display());
        let _ = writeln!(text, "  part:            {}", i.part);
        let _ = writeln!(text, "  database root:   {}", i.db_root.display());
        let _ = writeln!(text, "  layout:          {}", layout_name(&i));
        let _ = writeln!(text, "  architecture:    {}", i.architecture);
        let _ = writeln!(text, "  format version:  {}", i.format_version);
        let _ = writeln!(
            text,
            "  written by:      fasm-xilinx {} (loader {}{})",
            i.crate_version,
            i.loader_fingerprint,
            if i.loader_fingerprint == cache::LOADER_FINGERPRINT {
                ", this build"
            } else {
                ", another build: will be rebuilt"
            }
        );
        let _ = writeln!(text, "  created:         {} (Unix time)", i.created);
        let _ = writeln!(
            text,
            "  size:            {} bytes (payload {})",
            i.file_len, i.payload_len
        );
        let _ = writeln!(
            text,
            "  source hash:     {} ({} bytes)",
            hex(&i.source_hash()),
            i.source_bytes()
        );
        let _ = writeln!(text, "  sources:");
        for s in &i.sources {
            let what = match s.kind {
                SourceKind::Content => {
                    format!("{:>10}  {}", s.size, hex(&s.hash.unwrap_or_default()[..8]))
                }
                SourceKind::Absent => format!("{:>10}  {:16}", "absent", ""),
                SourceKind::Present => format!("{:>10}  {:16}", "present", ""),
            };
            let _ = writeln!(text, "    {what}  {}", s.path);
        }
        let _ = writeln!(stdout, "{text}");
    }
    Ok(ok)
}

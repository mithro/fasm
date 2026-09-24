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

//! An emulation of the gflags command line parsing used by prjxray's C++
//! tools (`xc7frames2bit`, `bitread`): `ParseCommandLineFlags(&argc,
//! &argv, true)` of the gflags version bundled with prjxray
//! (`third_party/gflags/src/gflags.cc`, `gflags_reporting.cc`).
//!
//! * `-flag`, `--flag`, `-flag=value`, `--flag value` (not for booleans),
//!   `-noflag` for booleans, boolean values `1/0/t/f/true/false/y/n/
//!   yes/no` (any case), int32 values in decimal or `0x` hex;
//! * non flag arguments (and `-`) are moved after the flags (GNU style
//!   permutation), `--` ends the flags;
//! * errors (unknown flags, missing or illegal values) are collected per
//!   flag name and printed together on stderr, sorted by name, exit code
//!   1, after the help flags have been handled;
//! * `--help`/`--helpful`, `--helpshort`, `--helpon=M`, `--helpmatch=S`,
//!   `--helppackage`, `--helpxml`, `--version` print to stdout like
//!   gflags (exit code 1, 0 for `--version`), with the flags of each
//!   source file listed under `Flags from <file>:`;
//! * `--undefok`, `--fromenv`, `--tryfromenv` are supported.
//!
//! Not supported (documented in `docs/rewrite/COMPAT.md`): `--flagfile`
//! (an error) and the bash completion of `--tab_completion_word`
//! (ignored). The file names in the help are relative to the prjxray
//! checkout (`tools/bitread.cc`) instead of the absolute build paths.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::PathBuf;

/// The bytes of an argument or environment value (on non Unix systems,
/// its lossy UTF-8).
pub fn os_bytes(s: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        s.as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        s.to_string_lossy().into_owned().into_bytes()
    }
}

/// A path from bytes (see [`os_bytes`]).
pub fn bytes_path(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// The type of a flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlagType {
    /// `DEFINE_bool`.
    Bool,
    /// `DEFINE_int32`.
    Int32,
    /// `DEFINE_string`.
    String,
}

impl FlagType {
    /// The gflags type name.
    pub fn name(self) -> &'static str {
        match self {
            FlagType::Bool => "bool",
            FlagType::Int32 => "int32",
            FlagType::String => "string",
        }
    }
}

/// One flag definition (`DEFINE_<type>(name, default, help)` in `file`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flag {
    /// The name.
    pub name: &'static str,
    /// The source file (as shown by the help).
    pub file: &'static str,
    /// The type.
    pub ty: FlagType,
    /// The default value as gflags prints it (`false`, `-1`, a string).
    pub default: &'static str,
    /// The help text.
    pub help: &'static str,
}

const GFLAGS_CC: &str = "third_party/gflags/src/gflags.cc";
const COMPLETIONS_CC: &str = "third_party/gflags/src/gflags_completions.cc";
const REPORTING_CC: &str = "third_party/gflags/src/gflags_reporting.cc";

/// The flags gflags itself defines.
pub fn builtin_flags() -> Vec<Flag> {
    let f = |name, file, ty, default, help| Flag {
        name,
        file,
        ty,
        default,
        help,
    };
    use FlagType::{Bool, Int32, String};
    vec![
        f("flagfile", GFLAGS_CC, String, "", "load flags from file"),
        f(
            "fromenv",
            GFLAGS_CC,
            String,
            "",
            "set flags from the environment [use 'export FLAGS_flag1=value']",
        ),
        f(
            "tryfromenv",
            GFLAGS_CC,
            String,
            "",
            "set flags from the environment if present",
        ),
        f(
            "undefok",
            GFLAGS_CC,
            String,
            "",
            "comma-separated list of flag names that it is okay to specify on the command line even if the program does not define a flag with that name.  IMPORTANT: flags in this list that have arguments MUST use the flag=value format",
        ),
        f(
            "tab_completion_columns",
            COMPLETIONS_CC,
            Int32,
            "80",
            "Number of columns to use in output for tab completion",
        ),
        f(
            "tab_completion_word",
            COMPLETIONS_CC,
            String,
            "",
            "If non-empty, HandleCommandLineCompletions() will hijack the process and attempt to do bash-style command line flag completion on this value.",
        ),
        f(
            "help",
            REPORTING_CC,
            Bool,
            "false",
            "show help on all flags [tip: all flags can have two dashes]",
        ),
        f(
            "helpful",
            REPORTING_CC,
            Bool,
            "false",
            "show help on all flags -- same as -help",
        ),
        f(
            "helpmatch",
            REPORTING_CC,
            String,
            "",
            "show help on modules whose name contains the specified substr",
        ),
        f(
            "helpon",
            REPORTING_CC,
            String,
            "",
            "show help on the modules named by this flag value",
        ),
        f(
            "helppackage",
            REPORTING_CC,
            Bool,
            "false",
            "show help on all modules in the main package",
        ),
        f(
            "helpshort",
            REPORTING_CC,
            Bool,
            "false",
            "show help on only the main module for this program",
        ),
        f(
            "helpxml",
            REPORTING_CC,
            Bool,
            "false",
            "produce an xml version of help",
        ),
        f(
            "version",
            REPORTING_CC,
            Bool,
            "false",
            "show version and build info and exit",
        ),
    ]
}

/// A program: its flags and usage message.
#[derive(Clone, Debug)]
pub struct Program {
    /// `argv[0]`.
    pub argv0: Vec<u8>,
    /// `gflags::SetUsageMessage(...)`.
    pub usage: Vec<u8>,
    /// The program's own flags (the gflags ones are added).
    pub flags: Vec<Flag>,
}

/// The parsed flags and the remaining (positional) arguments.
#[derive(Clone, Debug)]
pub struct Parsed {
    flags: Vec<Flag>,
    current: Vec<Vec<u8>>,
    modified: Vec<bool>,
    /// The arguments that are not flags, in gflags' order.
    pub args: Vec<Vec<u8>>,
}

impl Parsed {
    fn index(&self, name: &str) -> usize {
        self.flags
            .iter()
            .position(|f| f.name == name)
            .unwrap_or_else(|| panic!("no flag {name}"))
    }

    /// The value of a string flag.
    ///
    /// # Panics
    ///
    /// If there is no flag `name` (a programming error).
    pub fn string(&self, name: &str) -> &[u8] {
        &self.current[self.index(name)]
    }

    /// The value of a bool flag.
    ///
    /// # Panics
    ///
    /// If there is no flag `name` (a programming error).
    pub fn bool(&self, name: &str) -> bool {
        self.current[self.index(name)] == b"true"
    }

    /// The value of an int32 flag.
    ///
    /// # Panics
    ///
    /// If there is no flag `name` (a programming error).
    pub fn int32(&self, name: &str) -> i32 {
        std::str::from_utf8(&self.current[self.index(name)])
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default()
    }

    /// `true` if the flag was set.
    ///
    /// # Panics
    ///
    /// If there is no flag `name` (a programming error).
    pub fn is_set(&self, name: &str) -> bool {
        self.modified[self.index(name)]
    }
}

/// The result of [`parse`].
#[derive(Clone, Debug)]
pub enum Outcome {
    /// Run the program.
    Run(Parsed),
    /// Exit with this code after writing the output (help, errors).
    Exit {
        /// Exit code.
        code: u8,
        /// Text for stdout.
        stdout: Vec<u8>,
        /// Text for stderr.
        stderr: Vec<u8>,
    },
}

const ERROR: &str = "ERROR: ";

/// C `isspace` in the "C" locale.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `FlagValue::ParseFrom`: the canonical text of the value, or `None`.
fn parse_value(ty: FlagType, value: &[u8]) -> Option<Vec<u8>> {
    match ty {
        FlagType::Bool => {
            let lower = value.to_ascii_lowercase();
            for (t, f) in [
                (&b"1"[..], &b"0"[..]),
                (b"t", b"f"),
                (b"true", b"false"),
                (b"y", b"n"),
                (b"yes", b"no"),
            ] {
                if lower == t {
                    return Some(b"true".to_vec());
                } else if lower == f {
                    return Some(b"false".to_vec());
                }
            }
            None
        }
        FlagType::String => Some(value.to_vec()),
        FlagType::Int32 => {
            if value.is_empty() {
                return None;
            }
            let base = if value.len() >= 2 && value[0] == b'0' && (value[1] | 0x20) == b'x' {
                16
            } else {
                10
            };
            let r = strtoll_full(value, base)?;
            i32::try_from(r).ok().map(|v| v.to_string().into_bytes())
        }
    }
}

/// `strtoll(value, &end, base)` that must consume the whole text and not
/// overflow (`errno`).
fn strtoll_full(value: &[u8], base: u32) -> Option<i64> {
    let mut rest = value;
    while let Some((&c, tail)) = rest.split_first() {
        if is_space(c) {
            rest = tail;
        } else {
            break;
        }
    }
    let mut negative = false;
    if let Some((&c, tail)) = rest.split_first() {
        if c == b'+' || c == b'-' {
            negative = c == b'-';
            rest = tail;
        }
    }
    if base == 16
        && rest.len() >= 3
        && rest[0] == b'0'
        && (rest[1] | 0x20) == b'x'
        && rest[2].is_ascii_hexdigit()
    {
        rest = &rest[2..];
    }
    if rest.is_empty() {
        return None;
    }
    let mut value: i128 = 0;
    for &c in rest {
        let d = (c as char).to_digit(base)?;
        value = value * i128::from(base) + i128::from(d);
        if value > i128::from(i64::MAX) + 1 {
            return None;
        }
    }
    let value = if negative { -value } else { value };
    i64::try_from(value).ok()
}

/// `ParseFlagList`: a comma separated list; an empty entry or one starting
/// with `-` is a fatal error.
fn parse_flag_list(value: &[u8]) -> Result<Vec<Vec<u8>>, Vec<u8>> {
    let mut out = Vec::new();
    if value.is_empty() {
        return Ok(out);
    }
    for entry in value.split(|&c| c == b',') {
        if entry.is_empty() {
            return Err(format!("{ERROR}empty flaglist entry\n").into_bytes());
        }
        if entry[0] == b'-' {
            let mut m = format!("{ERROR}flag \"").into_bytes();
            m.extend_from_slice(entry);
            m.extend_from_slice(b"\" begins with '-'\n");
            return Err(m);
        }
        out.push(entry.to_vec());
    }
    Ok(out)
}

struct State<'a> {
    flags: &'a [Flag],
    current: Vec<Vec<u8>>,
    modified: Vec<bool>,
    errors: BTreeMap<Vec<u8>, Vec<u8>>,
    undefined: BTreeSet<Vec<u8>>,
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

impl State<'_> {
    fn find(&self, name: &[u8]) -> Option<usize> {
        self.flags.iter().position(|f| f.name.as_bytes() == name)
    }

    /// `ProcessSingleOptionLocked`. `Err` is a fatal error (exit now).
    fn set(
        &mut self,
        index: usize,
        value: &[u8],
        getenv: &dyn Fn(&str) -> Option<Vec<u8>>,
        depth: usize,
    ) -> Result<(), Vec<u8>> {
        let flag = &self.flags[index];
        match parse_value(flag.ty, value) {
            Some(v) => {
                self.current[index] = v;
                self.modified[index] = true;
            }
            None => {
                let msg = cat(&[
                    ERROR.as_bytes(),
                    b"illegal value '",
                    value,
                    format!("' specified for {} flag '{}'\n", flag.ty.name(), flag.name).as_bytes(),
                ]);
                self.errors.insert(flag.name.as_bytes().to_vec(), msg);
                return Ok(());
            }
        }
        match flag.name {
            "flagfile" if !value.is_empty() => {
                let msg = cat(&[
                    ERROR.as_bytes(),
                    b"--flagfile is not supported by this implementation of the prjxray tools\n",
                ]);
                return Err(msg);
            }
            "fromenv" | "tryfromenv" if depth < 8 => {
                let fatal = flag.name == "fromenv";
                self.process_env(value, fatal, getenv, depth + 1)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// `ProcessFromenvLocked`.
    fn process_env(
        &mut self,
        value: &[u8],
        fatal: bool,
        getenv: &dyn Fn(&str) -> Option<Vec<u8>>,
        depth: usize,
    ) -> Result<(), Vec<u8>> {
        for name in parse_flag_list(value)? {
            let Some(index) = self.find(&name) else {
                let msg = cat(&[
                    ERROR.as_bytes(),
                    b"unknown command line flag '",
                    &name,
                    b"' (via --fromenv or --tryfromenv)\n",
                ]);
                self.errors.insert(name.clone(), msg);
                self.undefined.insert(name);
                continue;
            };
            let env_name = format!("FLAGS_{}", String::from_utf8_lossy(&name));
            let Some(env_value) = getenv(&env_name) else {
                if fatal {
                    let msg = format!("{ERROR}{env_name} not found in environment\n").into_bytes();
                    self.errors.insert(name, msg);
                }
                continue;
            };
            if env_value == b"fromenv" || env_value == b"tryfromenv" {
                let msg = cat(&[
                    ERROR.as_bytes(),
                    b"infinite recursion on environment flag '",
                    &env_value,
                    b"'\n",
                ]);
                self.errors.insert(name, msg);
                continue;
            }
            self.set(index, &env_value, getenv, depth)?;
        }
        Ok(())
    }
}

/// `ParseCommandLineFlags(&argc, &argv, true)`: parses `args` (without
/// `argv[0]`), handles the help flags and reports errors.
pub fn parse(
    program: &Program,
    args: &[Vec<u8>],
    getenv: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Outcome {
    let mut flags = builtin_flags();
    flags.extend(program.flags.iter().cloned());
    // GetAllFlags order: file name, then flag name.
    flags.sort_by(|a, b| (a.file, a.name).cmp(&(b.file, b.name)));
    let mut state = State {
        flags: &flags,
        current: flags
            .iter()
            .map(|f| f.default.as_bytes().to_vec())
            .collect(),
        modified: vec![false; flags.len()],
        errors: BTreeMap::new(),
        undefined: BTreeSet::new(),
    };
    let fatal = |stderr: Vec<u8>| Outcome::Exit {
        code: 1,
        stdout: Vec::new(),
        stderr,
    };

    // ParseNewCommandLineFlags.
    let mut argv: Vec<Vec<u8>> = args.to_vec();
    let mut first_nonopt = argv.len();
    let mut i = 0;
    while i < first_nonopt {
        let arg = argv[i].clone();
        if arg.first() != Some(&b'-') || arg.len() == 1 {
            let a = argv.remove(i);
            argv.push(a);
            first_nonopt -= 1;
            continue;
        }
        let mut s = &arg[1..];
        if s.first() == Some(&b'-') {
            s = &s[1..];
        }
        if s.is_empty() {
            first_nonopt = i + 1;
            break;
        }
        // SplitArgumentLocked.
        let (mut key, mut value): (Vec<u8>, Option<Vec<u8>>) =
            match s.iter().position(|&c| c == b'=') {
                Some(at) => (s[..at].to_vec(), Some(s[at + 1..].to_vec())),
                None => (s.to_vec(), None),
            };
        let index = match state.find(&key) {
            Some(index) => index,
            None => {
                let unknown = cat(&[
                    ERROR.as_bytes(),
                    b"unknown command line flag '",
                    &key,
                    b"'\n",
                ]);
                let Some(stripped) = key.strip_prefix(b"no") else {
                    state.undefined.insert(key.clone());
                    state.errors.insert(key, unknown);
                    i += 1;
                    continue;
                };
                match state.find(stripped) {
                    None => {
                        state.undefined.insert(key.clone());
                        state.errors.insert(key, unknown);
                        i += 1;
                        continue;
                    }
                    Some(index) if flags[index].ty != FlagType::Bool => {
                        let msg = cat(&[
                            ERROR.as_bytes(),
                            b"boolean value (",
                            &key,
                            format!(
                                ") specified for {} command line flag\n",
                                flags[index].ty.name()
                            )
                            .as_bytes(),
                        ]);
                        state.undefined.insert(key.clone());
                        state.errors.insert(key, msg);
                        i += 1;
                        continue;
                    }
                    Some(index) => {
                        key = stripped.to_vec();
                        value = Some(b"0".to_vec());
                        index
                    }
                }
            }
        };
        if value.is_none() && flags[index].ty == FlagType::Bool {
            value = Some(b"1".to_vec());
        }
        let value = match value {
            Some(value) => value,
            None => {
                if i + 1 >= first_nonopt {
                    let mut msg = cat(&[
                        ERROR.as_bytes(),
                        b"flag '",
                        &argv[i],
                        b"' is missing its argument",
                    ]);
                    if flags[index].help.as_bytes().first().is_some_and(|&c| c > 1) {
                        msg.extend_from_slice(b"; flag description: ");
                        msg.extend_from_slice(flags[index].help.as_bytes());
                    }
                    msg.push(b'\n');
                    state.errors.insert(key, msg);
                    break;
                }
                i += 1;
                argv[i].clone()
            }
        };
        if let Err(message) = state.set(index, &value, getenv, 0) {
            return fatal(message);
        }
        i += 1;
    }
    let positional = argv[first_nonopt.min(argv.len())..].to_vec();

    // HandleCommandLineHelpFlags.
    let get = |state: &State<'_>, name: &str| -> Vec<u8> {
        state.current[state.find(name.as_bytes()).expect("builtin flag")].clone()
    };
    let is_true = |state: &State<'_>, name: &str| get(state, name) == b"true";
    let short_name = program
        .argv0
        .rsplit(|&c| c == b'/')
        .next()
        .unwrap_or_default()
        .to_vec();
    let help = |stdout: Vec<u8>| Outcome::Exit {
        code: 1,
        stdout,
        stderr: Vec::new(),
    };
    let view = HelpView {
        flags: &flags,
        current: &state.current,
        modified: &state.modified,
        short_name: &short_name,
        usage: &program.usage,
    };
    let progname_substrings = || -> Vec<Vec<u8>> {
        [&b"."[..], b"-main.", b"_main."]
            .iter()
            .map(|suffix| cat(&[b"/", &short_name, suffix]))
            .collect()
    };
    if is_true(&state, "helpshort") {
        return help(view.usage_matching(&progname_substrings()));
    }
    if is_true(&state, "help") || is_true(&state, "helpful") {
        return help(view.usage_matching(&[]));
    }
    let helpon = get(&state, "helpon");
    if !helpon.is_empty() {
        return help(view.usage_matching(&[cat(&[b"/", &helpon, b"."])]));
    }
    let helpmatch = get(&state, "helpmatch");
    if !helpmatch.is_empty() {
        return help(view.usage_matching(&[helpmatch]));
    }
    if is_true(&state, "helppackage") {
        let substrings = progname_substrings();
        let mut out = Vec::new();
        let mut stderr = Vec::new();
        let mut last_package: Vec<u8> = Vec::new();
        for flag in &flags {
            if !file_matches(flag.file.as_bytes(), &substrings) {
                continue;
            }
            let package = cat(&[dirname(flag.file.as_bytes()), b"/"]);
            if package != last_package {
                out.extend(view.usage_matching(std::slice::from_ref(&package)));
                if !last_package.is_empty() {
                    stderr.extend_from_slice(b"WARNING: Multiple packages contain a file=");
                    stderr.extend_from_slice(&short_name);
                    stderr.push(b'\n');
                }
                last_package = package;
            }
        }
        if last_package.is_empty() {
            stderr.extend_from_slice(b"WARNING: Unable to find a package for file=");
            stderr.extend_from_slice(&short_name);
            stderr.push(b'\n');
        }
        return Outcome::Exit {
            code: 1,
            stdout: out,
            stderr,
        };
    }
    if is_true(&state, "helpxml") {
        return help(view.xml());
    }
    if is_true(&state, "version") {
        return Outcome::Exit {
            code: 0,
            stdout: cat(&[&short_name, b"\n"]),
            stderr: Vec::new(),
        };
    }

    // ReportErrors.
    let undefok = get(&state, "undefok");
    match parse_flag_list(&undefok) {
        Err(message) => return fatal(message),
        Ok(list) => {
            for name in list {
                let no_version = cat(&[b"no", &name]);
                if state.undefined.contains(&name) {
                    state.errors.insert(name, Vec::new());
                } else if state.undefined.contains(&no_version) {
                    state.errors.insert(no_version, Vec::new());
                }
            }
        }
    }
    let errors: Vec<u8> = state.errors.values().flatten().copied().collect();
    if !errors.is_empty() {
        return fatal(errors);
    }
    Outcome::Run(Parsed {
        current: state.current,
        modified: state.modified,
        flags,
        args: positional,
    })
}

fn dirname(file: &[u8]) -> &[u8] {
    match file.iter().rposition(|&c| c == b'/') {
        Some(at) => &file[..at],
        None => &[],
    }
}

/// `FileMatchesSubstring`.
fn file_matches(file: &[u8], substrings: &[Vec<u8>]) -> bool {
    substrings.iter().any(|target| {
        file.windows(target.len().max(1)).any(|w| w == &target[..])
            || (target.first() == Some(&b'/') && file.starts_with(&target[1..]))
    })
}

struct HelpView<'a> {
    flags: &'a [Flag],
    current: &'a [Vec<u8>],
    modified: &'a [bool],
    short_name: &'a [u8],
    usage: &'a [u8],
}

const LINE_LENGTH: usize = 80;

impl HelpView<'_> {
    /// `ShowUsageWithFlagsMatching`.
    fn usage_matching(&self, substrings: &[Vec<u8>]) -> Vec<u8> {
        let mut out = cat(&[self.short_name, b": ", self.usage, b"\n"]);
        let mut last_file: Option<&str> = None;
        let mut first_directory = true;
        let mut found = false;
        for (i, flag) in self.flags.iter().enumerate() {
            if !substrings.is_empty() && !file_matches(flag.file.as_bytes(), substrings) {
                continue;
            }
            found = true;
            if last_file != Some(flag.file) {
                let last_dir = last_file.map_or(&b""[..], |f| dirname(f.as_bytes()));
                if dirname(flag.file.as_bytes()) != last_dir {
                    if !first_directory {
                        out.extend_from_slice(b"\n\n");
                    }
                    first_directory = false;
                }
                out.extend_from_slice(format!("\n  Flags from {}:\n", flag.file).as_bytes());
                last_file = Some(flag.file);
            }
            out.extend(self.describe(i));
        }
        if !found && !substrings.is_empty() {
            out.extend_from_slice(b"\n  No modules matched: use -help\n");
        }
        out
    }

    fn value_text(&self, i: usize, text: &str, value: &[u8]) -> Vec<u8> {
        if self.flags[i].ty == FlagType::String {
            cat(&[text.as_bytes(), b": \"", value, b"\""])
        } else {
            cat(&[text.as_bytes(), b": ", value])
        }
    }

    /// `DescribeOneFlag`.
    fn describe(&self, i: usize) -> Vec<u8> {
        let flag = &self.flags[i];
        let main_part = format!("    -{} ({})", flag.name, flag.help).into_bytes();
        let mut c: &[u8] = &main_part;
        let mut out: Vec<u8> = Vec::new();
        let mut chars_in_line = 0usize;
        loop {
            let newline = c.iter().position(|&b| b == b'\n');
            if newline.is_none() && chars_in_line + c.len() < LINE_LENGTH {
                out.extend_from_slice(c);
                chars_in_line += c.len();
                break;
            }
            if let Some(n) = newline.filter(|&n| n < LINE_LENGTH.saturating_sub(chars_in_line)) {
                out.extend_from_slice(&c[..n]);
                c = &c[n + 1..];
            } else {
                let mut whitespace = LINE_LENGTH as isize - chars_in_line as isize - 1;
                while whitespace > 0 && !c.get(whitespace as usize).copied().is_some_and(is_space) {
                    whitespace -= 1;
                }
                if whitespace <= 0 {
                    out.extend_from_slice(c);
                    chars_in_line = LINE_LENGTH;
                    break;
                }
                let mut w = whitespace as usize;
                out.extend_from_slice(&c[..w]);
                chars_in_line += w;
                while c.get(w).copied().is_some_and(is_space) {
                    w += 1;
                }
                c = &c[w..];
            }
            if c.is_empty() {
                break;
            }
            out.extend_from_slice(b"\n      ");
            chars_in_line = 6;
        }
        let mut add = |s: &[u8]| {
            if chars_in_line + 1 + s.len() >= LINE_LENGTH {
                out.extend_from_slice(b"\n      ");
                chars_in_line = 6;
            } else {
                out.push(b' ');
                chars_in_line += 1;
            }
            out.extend_from_slice(s);
            chars_in_line += s.len();
        };
        add(format!("type: {}", flag.ty.name()).as_bytes());
        add(&self.value_text(i, "default", flag.default.as_bytes()));
        if self.modified[i] || self.current[i] != flag.default.as_bytes() {
            add(&self.value_text(i, "currently", &self.current[i]));
        }
        out.push(b'\n');
        out
    }

    /// `ShowXMLOfFlags`.
    fn xml(&self) -> Vec<u8> {
        fn text(s: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            for &c in s {
                match c {
                    b'&' => out.extend_from_slice(b"&amp;"),
                    b'<' => out.extend_from_slice(b"&lt;"),
                    _ => out.push(c),
                }
            }
            out
        }
        let mut out = b"<?xml version=\"1.0\"?>\n<AllFlags>\n".to_vec();
        out.extend(cat(&[
            b"<program>",
            &text(self.short_name),
            b"</program>\n",
        ]));
        out.extend(cat(&[b"<usage>", &text(self.usage), b"</usage>\n"]));
        for (i, flag) in self.flags.iter().enumerate() {
            let tag = |name: &str, value: &[u8]| {
                cat(&[
                    format!("<{name}>").as_bytes(),
                    &text(value),
                    format!("</{name}>").as_bytes(),
                ])
            };
            out.extend_from_slice(b"<flag>");
            out.extend(tag("file", flag.file.as_bytes()));
            out.extend(tag("name", flag.name.as_bytes()));
            out.extend(tag("meaning", flag.help.as_bytes()));
            out.extend(tag("default", flag.default.as_bytes()));
            out.extend(tag("current", &self.current[i]));
            out.extend(tag("type", flag.ty.name().as_bytes()));
            out.extend_from_slice(b"</flag>\n");
        }
        out.extend_from_slice(b"</AllFlags>\n");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program() -> Program {
        Program {
            argv0: b"/bin/tool".to_vec(),
            usage: b"/bin/tool".to_vec(),
            flags: vec![
                Flag {
                    name: "name",
                    file: "tools/tool.cc",
                    ty: FlagType::String,
                    default: "",
                    help: "A name",
                },
                Flag {
                    name: "b",
                    file: "tools/tool.cc",
                    ty: FlagType::Bool,
                    default: "false",
                    help: "a bool",
                },
                Flag {
                    name: "n",
                    file: "tools/tool.cc",
                    ty: FlagType::Int32,
                    default: "-1",
                    help: "a number",
                },
            ],
        }
    }

    fn run(args: &[&str]) -> Outcome {
        let args: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
        parse(&program(), &args, &|name| {
            (name == "FLAGS_name").then(|| b"from env".to_vec())
        })
    }

    fn ok(args: &[&str]) -> Parsed {
        match run(args) {
            Outcome::Run(p) => p,
            Outcome::Exit { stderr, stdout, .. } => panic!(
                "{args:?}: {}{}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            ),
        }
    }

    fn err(args: &[&str]) -> String {
        match run(args) {
            Outcome::Exit {
                code: 1, stderr, ..
            } => String::from_utf8(stderr).unwrap(),
            other => panic!("{args:?}: {other:?}"),
        }
    }

    #[test]
    fn values_and_positionals() {
        let p = ok(&["a", "--name", "x", "-b", "c", "-n=0x10", "--", "-d", "e"]);
        assert_eq!(p.string("name"), b"x");
        assert!(p.bool("b"));
        assert_eq!(p.int32("n"), 16);
        let args: Vec<&[u8]> = p.args.iter().map(Vec::as_slice).collect();
        assert_eq!(args, [&b"-d"[..], b"e", b"a", b"c"]);
        let p = ok(&["-nob", "-n", " -5", "-", "--b=No"]);
        assert!(!p.bool("b"));
        assert_eq!(p.int32("n"), -5);
        assert_eq!(p.args, [b"-".to_vec()]);
        assert!(ok(&["--b=YES"]).bool("b"));
        assert_eq!(ok(&["--fromenv=name"]).string("name"), b"from env");
        assert_eq!(ok(&["--bogus", "--undefok=bogus"]).args.len(), 0);
        assert_eq!(ok(&["-name="]).string("name"), b"");
    }

    #[test]
    fn errors() {
        assert_eq!(
            err(&["--bogus"]),
            "ERROR: unknown command line flag 'bogus'\n"
        );
        assert_eq!(
            err(&["--noname"]),
            "ERROR: boolean value (noname) specified for string command line flag\n"
        );
        assert_eq!(
            err(&["--name"]),
            "ERROR: flag '--name' is missing its argument; flag description: A name\n"
        );
        assert_eq!(
            err(&["a", "--name"]),
            "ERROR: flag '--name' is missing its argument; flag description: A name\n"
        );
        assert_eq!(
            err(&["--b=2", "-n=x", "--zz", "--aa"]),
            "ERROR: unknown command line flag 'aa'\n\
             ERROR: illegal value '2' specified for bool flag 'b'\n\
             ERROR: illegal value 'x' specified for int32 flag 'n'\n\
             ERROR: unknown command line flag 'zz'\n"
        );
        assert_eq!(
            err(&["-n=2147483648"]),
            "ERROR: illegal value '2147483648' specified for int32 flag 'n'\n"
        );
        assert_eq!(
            err(&["-n=1 "]),
            "ERROR: illegal value '1 ' specified for int32 flag 'n'\n"
        );
        assert_eq!(
            err(&["--fromenv=b"]),
            "ERROR: FLAGS_b not found in environment\n"
        );
        assert_eq!(err(&["--undefok=,"]), "ERROR: empty flaglist entry\n");
    }

    #[test]
    fn help() {
        let Outcome::Exit { code, stdout, .. } = run(&["--helpshort", "--name", "abc"]) else {
            panic!()
        };
        assert_eq!(code, 1);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "tool: /bin/tool\n\n  Flags from tools/tool.cc:\n    -b (a bool) type: bool default: false\n    -n (a number) type: int32 default: -1\n    -name (A name) type: string default: \"\" currently: \"abc\"\n"
        );
        let Outcome::Exit { code, stdout, .. } = run(&["--version"]) else {
            panic!()
        };
        assert_eq!((code, stdout), (0, b"tool\n".to_vec()));
        let Outcome::Exit { stdout, .. } = run(&["--helpmatch=zzz"]) else {
            panic!()
        };
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "tool: /bin/tool\n\n  No modules matched: use -help\n"
        );
        let Outcome::Exit { stdout, .. } = run(&["--help"]) else {
            panic!()
        };
        let text = String::from_utf8(stdout).unwrap();
        assert!(text.contains(
            "    -undefok (comma-separated list of flag names that it is okay to specify on\n      the command line even if the program does not define a flag with that\n      name.  IMPORTANT: flags in this list that have arguments MUST use the\n      flag=value format) type: string default: \"\"\n"
        ));
        assert!(text.contains("      type: bool default: false currently: true\n"));
        assert!(text.contains("\n\n\n\n  Flags from tools/tool.cc:\n"));
    }
}

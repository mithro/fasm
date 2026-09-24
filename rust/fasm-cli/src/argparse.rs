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

//! An emulation of Python 3.11's `argparse` for the one parser the original
//! `fasm/tool.py` defines:
//!
//! ```python
//! parser = argparse.ArgumentParser('FASM tool')
//! parser.add_argument('file', help='Filename to process')
//! parser.add_argument('--canonical', action='store_true', help=...)
//! parser.add_argument('--parser', type=nullable_string, help=...)
//! ```
//!
//! It follows `argparse.py` of Python 3.11.15 step by step (the
//! `_parse_known_args` pattern matching of `O`/`A`/`-` argument classes,
//! `_parse_optional`, `_get_option_tuples`, `consume_optional`), so it
//! reproduces all of its behaviour for this parser: unambiguous prefixes
//! (`--canon`, `--pars X`), `--opt=value`, `--` handling, `-h` anywhere,
//! repeated options (the last one wins), negative numbers as values, and
//! every error message. [`format_help`] and [`format_usage`] reproduce
//! `HelpFormatter`'s output, line wrapping at the terminal width included.

use crate::pystr::{decimal_value, PyStr};

/// The program name the original tool gives argparse.
pub const PROG: &str = "FASM tool";

/// The parsed command line (argparse's `Namespace`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Namespace {
    /// The `file` positional argument.
    pub file: PyStr,
    /// `--canonical`.
    pub canonical: bool,
    /// `--parser`, after `nullable_string` (an empty value is `None`).
    pub parser: Option<PyStr>,
}

/// The outcome of [`parse_args`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Parsed {
    /// Arguments parsed; run the tool.
    Run(Namespace),
    /// `-h`/`--help`: print [`format_help`] to stdout and exit with 0.
    Help,
    /// A usage error: print [`format_error`] of the message to stderr and
    /// exit with 2.
    Error(PyStr),
}

/// The parser's actions (`parser._actions`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Help,
    Canonical,
    Parser,
    File,
}

impl Action {
    /// `_get_action_name()`.
    fn name(self) -> &'static str {
        match self {
            Action::Help => "-h/--help",
            Action::Canonical => "--canonical",
            Action::Parser => "--parser",
            Action::File => "file",
        }
    }
}

/// `parser._option_string_actions`, in insertion order.
const OPTION_STRINGS: [(&str, Action); 4] = [
    ("-h", Action::Help),
    ("--help", Action::Help),
    ("--canonical", Action::Canonical),
    ("--parser", Action::Parser),
];

fn lookup_option(s: &PyStr) -> Option<Action> {
    OPTION_STRINGS
        .iter()
        .find(|(name, _)| s.eq_str(name))
        .map(|&(_, action)| action)
}

const DASH: u32 = '-' as u32;

/// Why parsing stopped early.
enum Stop {
    /// The help action ran.
    Help,
    /// `parser.error(message)`.
    Error(PyStr),
}

/// `ArgumentError(action, message)` turned into `parser.error(str(err))`.
fn argument_error(action: Action, message: &PyStr) -> Stop {
    let mut text = PyStr::from_str(&format!("argument {}: ", action.name()));
    text.0.extend_from_slice(&message.0);
    Stop::Error(text)
}

fn ignored_explicit_argument(action: Action, explicit_arg: &PyStr) -> Stop {
    let mut message = PyStr::from_str("ignored explicit argument ");
    message.0.extend_from_slice(&explicit_arg.repr().0);
    argument_error(action, &message)
}

/// An option tuple: `(action, option_string, sep, explicit_arg)`; only the
/// truthiness of `sep` matters, so it is a `bool`.
#[derive(Clone, Debug)]
struct OptionTuple {
    action: Option<Action>,
    option_string: PyStr,
    sep: bool,
    explicit_arg: Option<PyStr>,
}

/// `_negative_number_matcher.match(s)`: `^-\d+$|^-\d*\.\d+$` (`$` also
/// matches before a trailing newline, `\d` is any Unicode decimal digit).
fn looks_like_negative_number(s: &PyStr) -> bool {
    let mut chars: &[u32] = &s.0;
    if let Some((&last, rest)) = chars.split_last() {
        if last == u32::from('\n') {
            chars = rest;
        }
    }
    let Some((&first, rest)) = chars.split_first() else {
        return false;
    };
    if first != DASH {
        return false;
    }
    let is_digit = |c: &u32| decimal_value(*c).is_some();
    if !rest.is_empty() && rest.iter().all(is_digit) {
        return true;
    }
    match rest.iter().position(|&c| c == u32::from('.')) {
        Some(dot) => {
            let (int_part, frac_part) = (&rest[..dot], &rest[dot + 1..]);
            int_part.iter().all(is_digit) && !frac_part.is_empty() && frac_part.iter().all(is_digit)
        }
        None => false,
    }
}

/// `_get_option_tuples()`; `arg` starts with `-` and has at least two
/// characters.
fn get_option_tuples(arg: &PyStr) -> Vec<OptionTuple> {
    let mut result = Vec::new();
    if arg.at(1) == Some(DASH) {
        let (prefix, explicit_arg) = arg.partition_eq();
        for &(name, action) in &OPTION_STRINGS {
            if prefix.is_prefix_of(name) {
                result.push(OptionTuple {
                    action: Some(action),
                    option_string: PyStr::from_str(name),
                    sep: explicit_arg.is_some(),
                    explicit_arg: explicit_arg.clone(),
                });
            }
        }
    } else {
        let short_prefix = PyStr(arg.0[..2].to_vec());
        let short_explicit_arg = arg.slice_from(2);
        for &(name, action) in &OPTION_STRINGS {
            if short_prefix.eq_str(name) {
                result.push(OptionTuple {
                    action: Some(action),
                    option_string: PyStr::from_str(name),
                    sep: false,
                    explicit_arg: Some(short_explicit_arg.clone()),
                });
            } else if arg.is_prefix_of(name) {
                result.push(OptionTuple {
                    action: Some(action),
                    option_string: PyStr::from_str(name),
                    sep: false,
                    explicit_arg: None,
                });
            }
        }
    }
    result
}

/// `_parse_optional()`: `None` for a positional argument.
fn parse_optional(arg: &PyStr) -> Result<Option<OptionTuple>, Stop> {
    if arg.at(0) != Some(DASH) {
        return Ok(None);
    }
    if let Some(action) = lookup_option(arg) {
        return Ok(Some(OptionTuple {
            action: Some(action),
            option_string: arg.clone(),
            sep: false,
            explicit_arg: None,
        }));
    }
    if arg.len() == 1 {
        return Ok(None);
    }
    if let (option_string, Some(explicit_arg)) = arg.partition_eq() {
        if let Some(action) = lookup_option(&option_string) {
            return Ok(Some(OptionTuple {
                action: Some(action),
                option_string,
                sep: true,
                explicit_arg: Some(explicit_arg),
            }));
        }
    }
    let mut tuples = get_option_tuples(arg);
    if tuples.len() > 1 {
        let mut message = PyStr::from_str("ambiguous option: ");
        message.0.extend_from_slice(&arg.0);
        let matches: Vec<String> = tuples
            .iter()
            .map(|t| py_to_string(&t.option_string))
            .collect();
        message.0.extend(
            format!(" could match {}", matches.join(", "))
                .chars()
                .map(u32::from),
        );
        return Err(Stop::Error(message));
    }
    if let Some(tuple) = tuples.pop() {
        return Ok(Some(tuple));
    }
    if looks_like_negative_number(arg) || arg.contains(' ') {
        return Ok(None);
    }
    Ok(Some(OptionTuple {
        action: None,
        option_string: arg.clone(),
        sep: false,
        explicit_arg: None,
    }))
}

/// An option string (always one of [`OPTION_STRINGS`], so valid Unicode)
/// as a `String`.
fn py_to_string(s: &PyStr) -> String {
    s.0.iter().filter_map(|&c| char::from_u32(c)).collect()
}

/// `_match_argument()` for an optional action: the number of argument
/// strings it consumes from the argument class pattern `pattern`.
fn match_argument(action: Action, pattern: &[u8]) -> Result<usize, Stop> {
    match action {
        // nargs=0: pattern '()'.
        Action::Help | Action::Canonical => Ok(0),
        // nargs=None: pattern '(A)' for an optional.
        Action::Parser | Action::File => {
            if pattern.first() == Some(&b'A') {
                Ok(1)
            } else {
                Err(argument_error(
                    action,
                    &PyStr::from_str("expected one argument"),
                ))
            }
        }
    }
}

/// The state of `_parse_known_args()`.
struct State<'a> {
    args: &'a [PyStr],
    /// One of `O`, `A`, `-` per argument string.
    pattern: Vec<u8>,
    /// The option tuples of the `O` arguments, by index.
    options: Vec<Option<OptionTuple>>,
    extras: Vec<PyStr>,
    file_pending: bool,
    file: Option<PyStr>,
    canonical: bool,
    parser: Option<PyStr>,
}

impl State<'_> {
    /// `take_action()`.
    fn take_action(
        &mut self,
        action: Action,
        mut argument_strings: Vec<PyStr>,
    ) -> Result<(), Stop> {
        match action {
            Action::Help => return Err(Stop::Help),
            Action::Canonical => self.canonical = true,
            Action::Parser => {
                // `nullable_string`: an empty value means the default.
                let value = argument_strings.pop().unwrap_or_default();
                self.parser = (!value.is_empty()).then_some(value);
            }
            Action::File => {
                // `_get_values()` strips out the first '--'.
                if let Some(i) = argument_strings.iter().position(|a| a.eq_str("--")) {
                    argument_strings.remove(i);
                }
                self.file = argument_strings.pop();
            }
        }
        Ok(())
    }

    /// `consume_optional()`.
    fn consume_optional(&mut self, start_index: usize) -> Result<usize, Stop> {
        let OptionTuple {
            mut action,
            mut option_string,
            mut sep,
            mut explicit_arg,
        } = self.options[start_index]
            .clone()
            .expect("consume_optional is only called at an option");
        let mut action_tuples: Vec<(Action, Vec<PyStr>)> = Vec::new();
        let stop;
        loop {
            let Some(act) = action else {
                self.extras.push(self.args[start_index].clone());
                return Ok(start_index + 1);
            };
            if let Some(explicit) = explicit_arg.clone() {
                let arg_count = match_argument(act, b"A")?;
                if arg_count == 0 && option_string.at(1) != Some(DASH) && !explicit.is_empty() {
                    if sep || explicit.at(0) == Some(DASH) {
                        return Err(ignored_explicit_argument(act, &explicit));
                    }
                    action_tuples.push((act, Vec::new()));
                    let first = explicit.0[0];
                    let prefix_char = option_string.0[0];
                    option_string = PyStr(vec![prefix_char, first]);
                    if let Some(next) = lookup_option(&option_string) {
                        action = Some(next);
                        let rest = explicit.slice_from(1);
                        if rest.is_empty() {
                            sep = false;
                            explicit_arg = None;
                        } else if rest.at(0) == Some(u32::from('=')) {
                            sep = true;
                            explicit_arg = Some(rest.slice_from(1));
                        } else {
                            sep = false;
                            explicit_arg = Some(rest);
                        }
                    } else {
                        let mut extra = PyStr(vec![prefix_char]);
                        extra.0.extend_from_slice(&explicit.0);
                        self.extras.push(extra);
                        stop = start_index + 1;
                        break;
                    }
                } else if arg_count == 1 {
                    stop = start_index + 1;
                    action_tuples.push((act, vec![explicit]));
                    break;
                } else {
                    return Err(ignored_explicit_argument(act, &explicit));
                }
            } else {
                let start = start_index + 1;
                let arg_count = match_argument(act, &self.pattern[start..])?;
                stop = start + arg_count;
                action_tuples.push((act, self.args[start..stop].to_vec()));
                break;
            }
        }
        for (act, argument_strings) in action_tuples {
            self.take_action(act, argument_strings)?;
        }
        Ok(stop)
    }

    /// `consume_positionals()`: the `file` positional's pattern is
    /// `(-*A-*)`.
    fn consume_positionals(&mut self, start_index: usize) -> Result<usize, Stop> {
        if !self.file_pending {
            return Ok(start_index);
        }
        let selected = &self.pattern[start_index..];
        let mut n = selected.iter().take_while(|&&c| c == b'-').count();
        if selected.get(n) != Some(&b'A') {
            return Ok(start_index);
        }
        n += 1;
        n += selected[n..].iter().take_while(|&&c| c == b'-').count();
        let argument_strings = self.args[start_index..start_index + n].to_vec();
        self.take_action(Action::File, argument_strings)?;
        self.file_pending = false;
        Ok(start_index + n)
    }

    /// The smallest option index `>= start`.
    fn next_option_index(&self, start: usize) -> Option<usize> {
        (start..self.options.len()).find(|&i| self.options[i].is_some())
    }
}

/// `parse_known_args()`: the namespace and the unrecognized arguments.
fn parse_known_args(args: &[PyStr]) -> Result<(Namespace, Vec<PyStr>), Stop> {
    let mut pattern = Vec::with_capacity(args.len());
    let mut options = Vec::with_capacity(args.len());
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg.eq_str("--") {
            pattern.push(b'-');
            options.push(None);
            for _ in iter.by_ref() {
                pattern.push(b'A');
                options.push(None);
            }
            break;
        }
        match parse_optional(arg)? {
            None => {
                pattern.push(b'A');
                options.push(None);
            }
            Some(tuple) => {
                pattern.push(b'O');
                options.push(Some(tuple));
            }
        }
    }

    let mut state = State {
        args,
        pattern,
        options,
        extras: Vec::new(),
        file_pending: true,
        file: None,
        canonical: false,
        parser: None,
    };

    let max_option_index = state.options.iter().rposition(Option::is_some);
    let mut start_index = 0;
    while max_option_index.is_some_and(|max| start_index <= max) {
        let next_option_index = state
            .next_option_index(start_index)
            .expect("an option index >= start_index exists");
        if start_index != next_option_index {
            let positionals_end_index = state.consume_positionals(start_index)?;
            if positionals_end_index > start_index {
                start_index = positionals_end_index;
                continue;
            }
            start_index = positionals_end_index;
        }
        if state.options[start_index].is_none() {
            state
                .extras
                .extend_from_slice(&args[start_index..next_option_index]);
            start_index = next_option_index;
        }
        start_index = state.consume_optional(start_index)?;
    }
    let stop_index = state.consume_positionals(start_index)?;
    state.extras.extend_from_slice(&args[stop_index..]);

    let Some(file) = state.file else {
        return Err(Stop::Error(PyStr::from_str(
            "the following arguments are required: file",
        )));
    };
    Ok((
        Namespace {
            file,
            canonical: state.canonical,
            parser: state.parser,
        },
        state.extras,
    ))
}

/// `parser.parse_args(args)` for the command line arguments (without the
/// program name).
#[must_use]
pub fn parse_args(args: &[PyStr]) -> Parsed {
    match parse_known_args(args) {
        Err(Stop::Help) => Parsed::Help,
        Err(Stop::Error(message)) => Parsed::Error(message),
        Ok((namespace, extras)) => {
            if extras.is_empty() {
                Parsed::Run(namespace)
            } else {
                let mut message = PyStr::from_str("unrecognized arguments:");
                for extra in &extras {
                    message.0.push(u32::from(' '));
                    message.0.extend_from_slice(&extra.0);
                }
                Parsed::Error(message)
            }
        }
    }
}

// Help formatting (`HelpFormatter`).

/// The option usage parts (`[-h] [--canonical] [--parser PARSER]` split
/// by the `part_regexp`).
const OPTION_USAGE_PARTS: [&str; 3] = ["[-h]", "[--canonical]", "[--parser PARSER]"];
/// The positional usage parts.
const POSITIONAL_USAGE_PARTS: [&str; 1] = ["file"];
const USAGE_PREFIX: &str = "usage: ";

/// The help sections: heading and `(invocation, help)` per action.
const SECTIONS: [(&str, &[(&str, &str)]); 2] = [
    ("positional arguments", &[("file", "Filename to process")]),
    (
        "options",
        &[
            ("-h, --help", "show this help message and exit"),
            ("--canonical", "Return canonical form of FASM."),
            (
                "--parser PARSER",
                "Select FASM parser to use. Default is to choose the best \
                 implementation available.",
            ),
        ],
    ),
];

/// `get_lines()` inside `_format_usage()`.
fn usage_lines(
    parts: &[&str],
    indent: usize,
    prefix: Option<&str>,
    text_width: i64,
) -> Vec<String> {
    let indent_str = " ".repeat(indent);
    let mut lines = Vec::new();
    let mut line: Vec<&str> = Vec::new();
    let mut line_len = to_i64(prefix.map_or(indent, str::len)) - 1;
    for &part in parts {
        if line_len + 1 + to_i64(part.len()) > text_width && !line.is_empty() {
            lines.push(format!("{indent_str}{}", line.join(" ")));
            line.clear();
            line_len = to_i64(indent) - 1;
        }
        line.push(part);
        line_len += to_i64(part.len()) + 1;
    }
    if !line.is_empty() {
        lines.push(format!("{indent_str}{}", line.join(" ")));
    }
    if prefix.is_some() {
        if let Some(first) = lines.first_mut() {
            *first = first[indent..].to_string();
        }
    }
    lines
}

fn to_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// `_format_usage()`: `usage: ...` without the trailing blank line.
fn usage_text(width: i64) -> String {
    let prog = PROG;
    let full = format!(
        "{prog} {} {}",
        OPTION_USAGE_PARTS.join(" "),
        POSITIONAL_USAGE_PARTS.join(" ")
    );
    let text_width = width;
    if to_i64(USAGE_PREFIX.len() + full.len()) <= text_width {
        return format!("{USAGE_PREFIX}{full}");
    }
    let lines = if 4 * to_i64(USAGE_PREFIX.len() + prog.len()) <= 3 * text_width {
        // `len(prefix) + len(prog) <= 0.75 * text_width`.
        let indent = USAGE_PREFIX.len() + prog.len() + 1;
        let mut parts = vec![prog];
        parts.extend_from_slice(&OPTION_USAGE_PARTS);
        let mut lines = usage_lines(&parts, indent, Some(USAGE_PREFIX), text_width);
        lines.extend(usage_lines(
            &POSITIONAL_USAGE_PARTS,
            indent,
            None,
            text_width,
        ));
        lines
    } else {
        let indent = USAGE_PREFIX.len();
        let mut parts: Vec<&str> = OPTION_USAGE_PARTS.to_vec();
        parts.extend_from_slice(&POSITIONAL_USAGE_PARTS);
        let mut lines = usage_lines(&parts, indent, None, text_width);
        if lines.len() > 1 {
            lines = usage_lines(&OPTION_USAGE_PARTS, indent, None, text_width);
            lines.extend(usage_lines(
                &POSITIONAL_USAGE_PARTS,
                indent,
                None,
                text_width,
            ));
        }
        let mut with_prog = vec![prog.to_string()];
        with_prog.extend(lines);
        with_prog
    };
    format!("{USAGE_PREFIX}{}", lines.join("\n"))
}

/// `textwrap.wrap(text, width)` for a text of words separated by single
/// spaces, without hyphens or tabs (true of every help text here), and a
/// `width` of at least 1.
fn wrap(text: &str, width: usize) -> Vec<String> {
    debug_assert!(width >= 1 && !text.contains(['-', '\t', '\n']) && !text.contains("  "));
    // Chunks (words and single spaces), reversed like `_wrap_chunks`.
    let mut chunks: Vec<String> = Vec::new();
    for (i, word) in text.split(' ').enumerate() {
        if i > 0 {
            chunks.push(" ".to_string());
        }
        chunks.push(word.to_string());
    }
    chunks.reverse();
    let char_len = |s: &str| s.chars().count();
    let mut lines: Vec<String> = Vec::new();
    while !chunks.is_empty() {
        let mut cur_line: Vec<String> = Vec::new();
        let mut cur_len = 0;
        if !lines.is_empty() && chunks.last().is_some_and(|c| c.trim().is_empty()) {
            chunks.pop();
        }
        while let Some(chunk) = chunks.last() {
            let l = char_len(chunk);
            if cur_len + l <= width {
                cur_len += l;
                cur_line.push(chunks.pop().expect("chunk exists"));
            } else {
                break;
            }
        }
        if let Some(chunk) = chunks.last_mut() {
            if char_len(chunk) > width {
                // `_handle_long_word`, break_long_words=True.
                let space_left = width - cur_len;
                let head: String = chunk.chars().take(space_left).collect();
                let tail: String = chunk.chars().skip(space_left).collect();
                cur_line.push(head);
                *chunk = tail;
            }
        }
        if cur_line.last().is_some_and(|c| c.trim().is_empty()) {
            cur_line.pop();
        }
        if !cur_line.is_empty() {
            lines.push(cur_line.concat());
        }
    }
    lines
}

/// The `HelpFormatter` width for a terminal of `columns` columns.
fn formatter_width(columns: i64) -> i64 {
    columns.saturating_sub(2)
}

/// `parser.format_usage()` for a terminal of `columns` columns (see
/// [`crate::terminal::columns`]).
#[must_use]
pub fn format_usage(columns: i64) -> String {
    format!("{}\n", usage_text(formatter_width(columns)))
}

/// `parser.format_help()` for a terminal of `columns` columns.
#[must_use]
pub fn format_help(columns: i64) -> String {
    let width = formatter_width(columns);
    let indent: i64 = 2;
    let max_help_position = 24.min((width - 20).max(4));
    let action_max_length = SECTIONS
        .iter()
        .flat_map(|(_, actions)| actions.iter())
        .map(|(invocation, _)| to_i64(invocation.len()) + indent)
        .max()
        .unwrap_or(0);
    let help_position = (action_max_length + 2).min(max_help_position);
    let help_width = usize::try_from((width - help_position).max(11)).unwrap_or(usize::MAX);
    let action_width = help_position - indent - 2;
    let help_indent = " ".repeat(usize::try_from(help_position).unwrap_or(0));

    let mut out = usage_text(width);
    out.push('\n');
    for (heading, actions) in SECTIONS {
        out.push('\n');
        out.push_str(heading);
        out.push_str(":\n");
        for &(invocation, help) in actions {
            let lines = wrap(help, help_width);
            if to_i64(invocation.len()) <= action_width {
                let pad = usize::try_from(action_width).unwrap_or(0);
                out.push_str(&format!("  {invocation:<pad$}  "));
            } else {
                out.push_str(&format!("  {invocation}\n"));
                out.push_str(&help_indent);
            }
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    out.push_str(&help_indent);
                }
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// What `parser.error(message)` writes to stderr: the usage and
/// `FASM tool: error: {message}`, encoded like Python's stderr
/// (`backslashreplace`).
#[must_use]
pub fn format_error(message: &PyStr, columns: i64) -> String {
    format!(
        "{}{PROG}: error: {}\n",
        format_usage(columns),
        message.encode_backslashreplace()
    )
}

#[cfg(test)]
mod tests;

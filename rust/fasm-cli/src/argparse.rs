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

//! An emulation of Python 3.11's `argparse` for the small parsers of the
//! original tools, described declaratively by an [`ArgumentParser`] of
//! [`Argument`]s: `-h/--help`, `store_true` flags, `store` options (with
//! an optional `nullable_string` type), positionals with `nargs=None` or
//! `nargs='?'`, required options and defaults.
//!
//! It follows `argparse.py` of Python 3.11.15 step by step (the
//! `_parse_known_args` pattern matching of `O`/`A`/`-` argument classes,
//! `_parse_optional`, `_get_option_tuples`, `consume_optional`,
//! `consume_positionals` with `_match_arguments_partial`'s regular
//! expressions), so it reproduces all of its behaviour for these parsers:
//! unambiguous prefixes (`--canon`, `--pars X`), `--opt=value`, `--`
//! handling, `-h` anywhere, repeated options (the last one wins),
//! negative numbers as values, positionals consumed early by a `?`
//! positional, and every error message. [`ArgumentParser::format_help`]
//! and [`ArgumentParser::format_usage`] reproduce `HelpFormatter`'s
//! output, line wrapping at the terminal width included.
//!
//! [`parse_args`], [`format_help`], [`format_usage`] and [`format_error`]
//! are the parser of the original `fasm/tool.py`:
//!
//! ```python
//! parser = argparse.ArgumentParser('FASM tool')
//! parser.add_argument('file', help='Filename to process')
//! parser.add_argument('--canonical', action='store_true', help=...)
//! parser.add_argument('--parser', type=nullable_string, help=...)
//! ```

use crate::pystr::{decimal_value, PyStr};

/// How many argument strings an argument consumes (`nargs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nargs {
    /// `nargs=0` (`store_true`, help).
    Zero,
    /// `nargs=None`: exactly one.
    One,
    /// `nargs='?'`: zero or one.
    Optional,
}

/// What an argument does (`action`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// `-h/--help`: print the help and exit.
    Help,
    /// `action='store_true'`.
    StoreTrue,
    /// `action='store'` (no type).
    Store,
    /// `action='store'` with `type=nullable_string` (an empty value is
    /// `None`).
    StoreNullable,
}

/// One argument (`parser.add_argument(...)`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Argument {
    /// The option strings, empty for a positional argument.
    pub option_strings: Vec<&'static str>,
    /// The attribute name in the namespace.
    pub dest: &'static str,
    /// The action.
    pub kind: Kind,
    /// The number of values.
    pub nargs: Nargs,
    /// `required=True` (always for a positional with `nargs=None`).
    pub required: bool,
    /// The default value (`None` if absent).
    pub default: Option<PyStr>,
    /// The help text.
    pub help: &'static str,
}

impl Argument {
    /// `-h, --help` (added by `ArgumentParser` itself).
    pub fn help() -> Self {
        Argument {
            option_strings: vec!["-h", "--help"],
            dest: "help",
            kind: Kind::Help,
            nargs: Nargs::Zero,
            required: false,
            default: None,
            help: "show this help message and exit",
        }
    }

    /// `add_argument(option, action='store_true', help=help)`.
    pub fn flag(option: &'static str, dest: &'static str, help: &'static str) -> Self {
        Argument {
            option_strings: vec![option],
            dest,
            kind: Kind::StoreTrue,
            nargs: Nargs::Zero,
            required: false,
            default: None,
            help,
        }
    }

    /// `add_argument(option, help=help)`.
    pub fn option(option: &'static str, dest: &'static str, help: &'static str) -> Self {
        Argument {
            option_strings: vec![option],
            dest,
            kind: Kind::Store,
            nargs: Nargs::One,
            required: false,
            default: None,
            help,
        }
    }

    /// `add_argument(dest, help=help)`.
    pub fn positional(dest: &'static str, help: &'static str) -> Self {
        Argument {
            option_strings: Vec::new(),
            dest,
            kind: Kind::Store,
            nargs: Nargs::One,
            required: true,
            default: None,
            help,
        }
    }

    /// `required=...`.
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// `default=...`.
    #[must_use]
    pub fn default(mut self, default: PyStr) -> Self {
        self.default = Some(default);
        self
    }

    /// `nargs='?'` (a positional is then not required).
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.nargs = Nargs::Optional;
        if self.option_strings.is_empty() {
            self.required = false;
        }
        self
    }

    /// `type=nullable_string`.
    #[must_use]
    pub fn nullable(mut self) -> Self {
        self.kind = Kind::StoreNullable;
        self
    }

    fn is_positional(&self) -> bool {
        self.option_strings.is_empty()
    }

    /// The default metavar: `dest` for a positional, `dest.upper()` for
    /// an option.
    fn metavar(&self) -> String {
        if self.is_positional() {
            self.dest.to_string()
        } else {
            self.dest.to_uppercase()
        }
    }

    /// `_format_args()`.
    fn format_args(&self) -> String {
        match self.nargs {
            Nargs::Zero => String::new(),
            Nargs::One => self.metavar(),
            Nargs::Optional => format!("[{}]", self.metavar()),
        }
    }

    /// `_get_action_name()`.
    fn name(&self) -> String {
        if self.is_positional() {
            self.metavar()
        } else {
            self.option_strings.join("/")
        }
    }

    /// The part of the usage message (`_format_actions_usage`).
    fn usage_part(&self) -> String {
        let part = if self.is_positional() {
            return self.format_args();
        } else if self.nargs == Nargs::Zero {
            self.option_strings[0].to_string()
        } else {
            format!("{} {}", self.option_strings[0], self.format_args())
        };
        if self.required {
            part
        } else {
            format!("[{part}]")
        }
    }

    /// `_format_action_invocation()`.
    fn invocation(&self) -> String {
        if self.is_positional() {
            self.metavar()
        } else if self.nargs == Nargs::Zero {
            self.option_strings.join(", ")
        } else {
            let args = self.format_args();
            self.option_strings
                .iter()
                .map(|o| format!("{o} {args}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
}

/// A parsed value.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Value {
    /// `None`.
    None,
    /// A `store_true` flag.
    Bool(bool),
    /// A string.
    Str(PyStr),
}

/// The parsed command line (argparse's `Namespace`): the value of every
/// argument but help, by `dest`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Values(Vec<(&'static str, Value)>);

impl Values {
    /// The value of `dest` (`Value::None` if unknown).
    pub fn get(&self, dest: &str) -> &Value {
        self.0
            .iter()
            .find(|(d, _)| *d == dest)
            .map_or(&Value::None, |(_, v)| v)
    }

    /// The string value of `dest`, if it is one.
    pub fn str(&self, dest: &str) -> Option<&PyStr> {
        match self.get(dest) {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The flag value of `dest` (`false` if it is not a flag).
    pub fn flag(&self, dest: &str) -> bool {
        matches!(self.get(dest), Value::Bool(true))
    }

    fn set(&mut self, dest: &'static str, value: Value) {
        match self.0.iter_mut().find(|(d, _)| *d == dest) {
            Some(entry) => entry.1 = value,
            None => self.0.push((dest, value)),
        }
    }
}

/// The outcome of [`ArgumentParser::parse_args`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Arguments parsed; run the tool.
    Run(Values),
    /// `-h`/`--help`: print [`ArgumentParser::format_help`] to stdout and
    /// exit with 0.
    Help,
    /// A usage error: print [`ArgumentParser::format_error`] of the
    /// message to stderr and exit with 2.
    Error(PyStr),
}

/// An `argparse.ArgumentParser`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArgumentParser {
    /// The program name (`prog`).
    pub prog: String,
    /// The description shown between the usage and the arguments.
    pub description: Option<&'static str>,
    /// The arguments in `add_argument` order, the help argument first
    /// (`parser._actions`).
    pub arguments: Vec<Argument>,
}

const DASH: u32 = '-' as u32;

/// Why parsing stopped early.
enum Stop {
    /// The help action ran.
    Help,
    /// `parser.error(message)`.
    Error(PyStr),
}

/// An option tuple: `(action, option_string, sep, explicit_arg)`; only the
/// truthiness of `sep` matters, so it is a `bool`. `action` is an index
/// into [`ArgumentParser::arguments`].
#[derive(Clone, Debug)]
struct OptionTuple {
    action: Option<usize>,
    option_string: PyStr,
    sep: bool,
    explicit_arg: Option<PyStr>,
}

/// An option string (always one of the parser's, so valid Unicode) as a
/// `String`.
fn py_to_string(s: &PyStr) -> String {
    s.0.iter().filter_map(|&c| char::from_u32(c)).collect()
}

impl ArgumentParser {
    /// `parser._option_string_actions`, in insertion order.
    fn option_strings(&self) -> impl Iterator<Item = (&'static str, usize)> + '_ {
        self.arguments
            .iter()
            .enumerate()
            .flat_map(|(i, a)| a.option_strings.iter().map(move |&o| (o, i)))
    }

    fn lookup_option(&self, s: &PyStr) -> Option<usize> {
        self.option_strings()
            .find(|(name, _)| s.eq_str(name))
            .map(|(_, action)| action)
    }

    /// `ArgumentError(action, message)` turned into `parser.error(str(err))`.
    fn argument_error(&self, action: usize, message: &PyStr) -> Stop {
        let name = self.arguments[action].name();
        let mut text = PyStr::from_str(&format!("argument {name}: "));
        text.0.extend_from_slice(&message.0);
        Stop::Error(text)
    }

    fn ignored_explicit_argument(&self, action: usize, explicit_arg: &PyStr) -> Stop {
        let mut message = PyStr::from_str("ignored explicit argument ");
        message.0.extend_from_slice(&explicit_arg.repr().0);
        self.argument_error(action, &message)
    }

    /// `_get_option_tuples()`; `arg` starts with `-` and has at least two
    /// characters.
    fn get_option_tuples(&self, arg: &PyStr) -> Vec<OptionTuple> {
        let mut result = Vec::new();
        if arg.at(1) == Some(DASH) {
            let (prefix, explicit_arg) = arg.partition_eq();
            for (name, action) in self.option_strings() {
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
            for (name, action) in self.option_strings() {
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
    fn parse_optional(&self, arg: &PyStr) -> Result<Option<OptionTuple>, Stop> {
        if arg.at(0) != Some(DASH) {
            return Ok(None);
        }
        if let Some(action) = self.lookup_option(arg) {
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
            if let Some(action) = self.lookup_option(&option_string) {
                return Ok(Some(OptionTuple {
                    action: Some(action),
                    option_string,
                    sep: true,
                    explicit_arg: Some(explicit_arg),
                }));
            }
        }
        let mut tuples = self.get_option_tuples(arg);
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

    /// `_match_argument()` for an optional action: the number of argument
    /// strings it consumes from the argument class pattern `pattern`
    /// (`()`, `(A)` or `(A?)`: an option's pattern has no `-*`).
    fn match_argument(&self, action: usize, pattern: &[u8]) -> Result<usize, Stop> {
        let first_is_a = pattern.first() == Some(&b'A');
        match self.arguments[action].nargs {
            Nargs::Zero => Ok(0),
            Nargs::Optional => Ok(usize::from(first_is_a)),
            Nargs::One if first_is_a => Ok(1),
            Nargs::One => {
                Err(self.argument_error(action, &PyStr::from_str("expected one argument")))
            }
        }
    }

    /// `_match_arguments_partial()`: the argument counts of as many of
    /// the `positionals` as match the start of `pattern`.
    fn match_arguments_partial(&self, positionals: &[usize], pattern: &[u8]) -> Vec<usize> {
        for i in (1..=positionals.len()).rev() {
            let groups: Vec<Nargs> = positionals[..i]
                .iter()
                .map(|&p| self.arguments[p].nargs)
                .collect();
            let mut counts = Vec::new();
            if match_groups(&groups, pattern, 0, &mut counts) {
                return counts;
            }
        }
        Vec::new()
    }
}

/// `re.match` of the concatenated positional patterns (`(-*A-*)` for
/// `nargs=None`, `(-*A?-*)` for `nargs='?'`) at `pos` of `pattern`, with
/// the backtracking order of Python's regular expressions (greedy `-*`
/// and `A?` first); pushes the length of every group.
fn match_groups(groups: &[Nargs], pattern: &[u8], pos: usize, counts: &mut Vec<usize>) -> bool {
    let Some((&group, rest)) = groups.split_first() else {
        return true;
    };
    let dashes = |from: usize| {
        pattern
            .get(from..)
            .map_or(0, |p| p.iter().take_while(|&&c| c == b'-').count())
    };
    let a_at = |at: usize| pattern.get(at) == Some(&b'A');
    for lead in (0..=dashes(pos)).rev() {
        let middle_start = pos + lead;
        let middles: &[usize] = match group {
            Nargs::One if a_at(middle_start) => &[1],
            Nargs::One => &[],
            Nargs::Optional if a_at(middle_start) => &[1, 0],
            Nargs::Optional | Nargs::Zero => &[0],
        };
        for &middle in middles {
            let trail_start = middle_start + middle;
            for trail in (0..=dashes(trail_start)).rev() {
                let end = trail_start + trail;
                counts.push(end - pos);
                if match_groups(rest, pattern, end, counts) {
                    return true;
                }
                counts.pop();
            }
        }
    }
    false
}

/// The state of `_parse_known_args()`.
struct State<'a> {
    parser: &'a ArgumentParser,
    args: &'a [PyStr],
    /// One of `O`, `A`, `-` per argument string.
    pattern: Vec<u8>,
    /// The option tuples of the `O` arguments, by index.
    options: Vec<Option<OptionTuple>>,
    extras: Vec<PyStr>,
    /// The positionals left to be parsed.
    positionals: Vec<usize>,
    seen: Vec<bool>,
    values: Values,
}

impl State<'_> {
    /// `take_action()` with `_get_values()`.
    fn take_action(&mut self, action: usize, mut argument_strings: Vec<PyStr>) -> Result<(), Stop> {
        self.seen[action] = true;
        let argument = &self.parser.arguments[action];
        // `_get_values()` strips out the first '--' of a positional.
        if argument.is_positional() {
            if let Some(i) = argument_strings.iter().position(|a| a.eq_str("--")) {
                argument_strings.remove(i);
            }
        }
        let value = match argument.kind {
            Kind::Help => return Err(Stop::Help),
            Kind::StoreTrue => Value::Bool(true),
            Kind::Store | Kind::StoreNullable => match argument_strings.pop() {
                None => argument.default.clone().map_or(Value::None, Value::Str),
                Some(value) if argument.kind == Kind::StoreNullable && value.is_empty() => {
                    Value::None
                }
                Some(value) => Value::Str(value),
            },
        };
        self.values.set(argument.dest, value);
        Ok(())
    }

    /// `consume_optional()`.
    fn consume_optional(&mut self, start_index: usize) -> Result<usize, Stop> {
        let parser = self.parser;
        let OptionTuple {
            mut action,
            mut option_string,
            mut sep,
            mut explicit_arg,
        } = self.options[start_index]
            .clone()
            .expect("consume_optional is only called at an option");
        let mut action_tuples: Vec<(usize, Vec<PyStr>)> = Vec::new();
        let stop;
        loop {
            let Some(act) = action else {
                self.extras.push(self.args[start_index].clone());
                return Ok(start_index + 1);
            };
            if let Some(explicit) = explicit_arg.clone() {
                let arg_count = parser.match_argument(act, b"A")?;
                if arg_count == 0 && option_string.at(1) != Some(DASH) && !explicit.is_empty() {
                    if sep || explicit.at(0) == Some(DASH) {
                        return Err(parser.ignored_explicit_argument(act, &explicit));
                    }
                    action_tuples.push((act, Vec::new()));
                    let first = explicit.0[0];
                    let prefix_char = option_string.0[0];
                    option_string = PyStr(vec![prefix_char, first]);
                    if let Some(next) = parser.lookup_option(&option_string) {
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
                    return Err(parser.ignored_explicit_argument(act, &explicit));
                }
            } else {
                let start = start_index + 1;
                let arg_count = parser.match_argument(act, &self.pattern[start..])?;
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

    /// `consume_positionals()`.
    fn consume_positionals(&mut self, mut start_index: usize) -> Result<usize, Stop> {
        let counts = self
            .parser
            .match_arguments_partial(&self.positionals, &self.pattern[start_index..]);
        let matched: Vec<usize> = self.positionals.drain(..counts.len()).collect();
        for (action, count) in matched.into_iter().zip(counts) {
            let argument_strings = self.args[start_index..start_index + count].to_vec();
            start_index += count;
            self.take_action(action, argument_strings)?;
        }
        Ok(start_index)
    }

    /// The smallest option index `>= start`.
    fn next_option_index(&self, start: usize) -> Option<usize> {
        (start..self.options.len()).find(|&i| self.options[i].is_some())
    }
}

impl ArgumentParser {
    /// The initial namespace: every default (`False` for flags).
    fn defaults(&self) -> Values {
        let mut values = Values::default();
        for argument in &self.arguments {
            match argument.kind {
                Kind::Help => {}
                Kind::StoreTrue => values.set(argument.dest, Value::Bool(false)),
                Kind::Store | Kind::StoreNullable => values.set(
                    argument.dest,
                    argument.default.clone().map_or(Value::None, Value::Str),
                ),
            }
        }
        values
    }

    /// `parse_known_args()`: the namespace and the unrecognized arguments.
    fn parse_known_args(&self, args: &[PyStr]) -> Result<(Values, Vec<PyStr>), Stop> {
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
            match self.parse_optional(arg)? {
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
            parser: self,
            args,
            pattern,
            options,
            extras: Vec::new(),
            positionals: (0..self.arguments.len())
                .filter(|&i| self.arguments[i].is_positional())
                .collect(),
            seen: vec![false; self.arguments.len()],
            values: self.defaults(),
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

        let required: Vec<String> = self
            .arguments
            .iter()
            .zip(&state.seen)
            .filter(|(argument, &seen)| argument.required && !seen)
            .map(|(argument, _)| argument.name())
            .collect();
        if !required.is_empty() {
            return Err(Stop::Error(PyStr::from_str(&format!(
                "the following arguments are required: {}",
                required.join(", ")
            ))));
        }
        Ok((state.values, state.extras))
    }

    /// `parser.parse_args(args)` for the command line arguments (without
    /// the program name).
    #[must_use]
    pub fn parse_args(&self, args: &[PyStr]) -> Outcome {
        match self.parse_known_args(args) {
            Err(Stop::Help) => Outcome::Help,
            Err(Stop::Error(message)) => Outcome::Error(message),
            Ok((values, extras)) => {
                if extras.is_empty() {
                    Outcome::Run(values)
                } else {
                    let mut message = PyStr::from_str("unrecognized arguments:");
                    for extra in &extras {
                        message.0.push(u32::from(' '));
                        message.0.extend_from_slice(&extra.0);
                    }
                    Outcome::Error(message)
                }
            }
        }
    }

    /// The wrappable parts of the usage of the options or of the
    /// positionals: `part_regexp` applied to their usage text (so a
    /// required `--opt METAVAR` is two parts, `[--opt METAVAR]` one).
    fn usage_parts(&self, positional: bool) -> Vec<String> {
        let usage = self
            .arguments
            .iter()
            .filter(|a| a.is_positional() == positional)
            .map(Argument::usage_part)
            .collect::<Vec<_>>()
            .join(" ");
        split_usage(&usage)
    }

    /// `_format_usage()`: `usage: ...` without the trailing blank line.
    fn usage_text(&self, width: i64) -> String {
        let prog = self.prog.as_str();
        let option_parts = self.usage_parts(false);
        let positional_parts = self.usage_parts(true);
        let option_parts: Vec<&str> = option_parts.iter().map(String::as_str).collect();
        let positional_parts: Vec<&str> = positional_parts.iter().map(String::as_str).collect();
        let action_usage = option_parts
            .iter()
            .chain(&positional_parts)
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        let full = [prog, action_usage.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let text_width = width;
        if to_i64(USAGE_PREFIX.chars().count() + full.chars().count()) <= text_width {
            return format!("{USAGE_PREFIX}{full}");
        }
        let prog_len = prog.chars().count();
        let lines = if 4 * to_i64(USAGE_PREFIX.len() + prog_len) <= 3 * text_width {
            // `len(prefix) + len(prog) <= 0.75 * text_width`.
            let indent = USAGE_PREFIX.len() + prog_len + 1;
            let mut lines = if option_parts.is_empty() {
                let mut parts = vec![prog];
                parts.extend_from_slice(&positional_parts);
                usage_lines(&parts, indent, Some(USAGE_PREFIX), text_width)
            } else {
                let mut parts = vec![prog];
                parts.extend_from_slice(&option_parts);
                usage_lines(&parts, indent, Some(USAGE_PREFIX), text_width)
            };
            if !option_parts.is_empty() && !positional_parts.is_empty() {
                lines.extend(usage_lines(&positional_parts, indent, None, text_width));
            }
            lines
        } else {
            let indent = USAGE_PREFIX.len();
            let mut parts: Vec<&str> = option_parts.clone();
            parts.extend_from_slice(&positional_parts);
            let mut lines = usage_lines(&parts, indent, None, text_width);
            if lines.len() > 1 {
                lines = usage_lines(&option_parts, indent, None, text_width);
                lines.extend(usage_lines(&positional_parts, indent, None, text_width));
            }
            let mut with_prog = vec![prog.to_string()];
            with_prog.extend(lines);
            with_prog
        };
        format!("{USAGE_PREFIX}{}", lines.join("\n"))
    }

    /// `parser.format_usage()` for a terminal of `columns` columns (see
    /// [`crate::terminal::columns`]).
    #[must_use]
    pub fn format_usage(&self, columns: i64) -> String {
        format!("{}\n", self.usage_text(formatter_width(columns)))
    }

    /// `parser.format_help()` for a terminal of `columns` columns.
    #[must_use]
    pub fn format_help(&self, columns: i64) -> String {
        let width = formatter_width(columns);
        let indent: i64 = 2;
        let max_help_position = 24.min((width - 20).max(4));
        let action_max_length = self
            .arguments
            .iter()
            .map(|a| to_i64(a.invocation().chars().count()) + indent)
            .max()
            .unwrap_or(0);
        let help_position = (action_max_length + 2).min(max_help_position);
        let help_width = usize::try_from((width - help_position).max(11)).unwrap_or(usize::MAX);
        let action_width = help_position - indent - 2;
        let help_indent = " ".repeat(usize::try_from(help_position).unwrap_or(0));

        let mut out = self.usage_text(width);
        out.push('\n');
        if let Some(description) = self.description {
            let text_width = usize::try_from(width.max(11)).unwrap_or(usize::MAX);
            out.push('\n');
            for line in wrap(description, text_width) {
                out.push_str(&line);
                out.push('\n');
            }
        }
        for (heading, positional) in [("positional arguments", true), ("options", false)] {
            let arguments: Vec<&Argument> = self
                .arguments
                .iter()
                .filter(|a| a.is_positional() == positional)
                .collect();
            if arguments.is_empty() {
                continue;
            }
            out.push('\n');
            out.push_str(heading);
            out.push_str(":\n");
            for argument in arguments {
                let invocation = argument.invocation();
                let lines = wrap(argument.help, help_width);
                if to_i64(invocation.chars().count()) <= action_width {
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
    /// `{prog}: error: {message}`, encoded like Python's stderr
    /// (`backslashreplace`).
    #[must_use]
    pub fn format_error(&self, message: &PyStr, columns: i64) -> String {
        format!(
            "{}{}: error: {}\n",
            self.format_usage(columns),
            self.prog,
            message.encode_backslashreplace()
        )
    }
}

const USAGE_PREFIX: &str = "usage: ";

/// `re.findall(r'\(.*?\)+(?=\s|$)|\[.*?\]+(?=\s|$)|\S+', text)`
/// (`part_regexp` of `HelpFormatter._format_usage`).
fn split_usage(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let at_break = |i: usize| chars.get(i).is_none_or(|c| c.is_whitespace());
    let mut parts = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        // `\(.*?\)+(?=\s|$)` / `\[.*?\]+(?=\s|$)`: the shortest run up
        // to closing brackets followed by white space or the end (`.`
        // does not match a newline).
        let group_end = match chars[i] {
            '(' | '[' => {
                let close = if chars[i] == '(' { ')' } else { ']' };
                let mut end = None;
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '\n' {
                    if chars[j] == close {
                        let mut k = j;
                        while k < chars.len() && chars[k] == close {
                            k += 1;
                        }
                        if at_break(k) {
                            end = Some(k);
                            break;
                        }
                    }
                    j += 1;
                }
                end
            }
            _ => None,
        };
        let end = group_end.unwrap_or_else(|| {
            let mut k = i;
            while k < chars.len() && !chars[k].is_whitespace() {
                k += 1;
            }
            k
        });
        parts.push(chars[i..end].iter().collect());
        i = end;
    }
    parts
}

/// The `HelpFormatter` width for a terminal of `columns` columns.
fn formatter_width(columns: i64) -> i64 {
    columns.saturating_sub(2)
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

// The parser of the original `fasm/tool.py`.

/// The program name the original `fasm` tool gives argparse.
pub const PROG: &str = "FASM tool";

/// The argument parser of `fasm/tool.py`.
#[must_use]
pub fn fasm_tool_parser() -> ArgumentParser {
    ArgumentParser {
        prog: PROG.to_string(),
        description: None,
        arguments: vec![
            Argument::help(),
            Argument::positional("file", "Filename to process"),
            Argument::flag("--canonical", "canonical", "Return canonical form of FASM."),
            Argument::option(
                "--parser",
                "parser",
                "Select FASM parser to use. Default is to choose the best \
                 implementation available.",
            )
            .nullable(),
        ],
    }
}

/// The parsed command line of the `fasm` tool (argparse's `Namespace`).
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

/// `parser.parse_args(args)` of the `fasm` tool for the command line
/// arguments (without the program name).
#[must_use]
pub fn parse_args(args: &[PyStr]) -> Parsed {
    match fasm_tool_parser().parse_args(args) {
        Outcome::Run(values) => Parsed::Run(Namespace {
            file: values.str("file").cloned().unwrap_or_default(),
            canonical: values.flag("canonical"),
            parser: values.str("parser").cloned(),
        }),
        Outcome::Help => Parsed::Help,
        Outcome::Error(message) => Parsed::Error(message),
    }
}

/// `parser.format_usage()` of the `fasm` tool for a terminal of `columns`
/// columns (see [`crate::terminal::columns`]).
#[must_use]
pub fn format_usage(columns: i64) -> String {
    fasm_tool_parser().format_usage(columns)
}

/// `parser.format_help()` of the `fasm` tool for a terminal of `columns`
/// columns.
#[must_use]
pub fn format_help(columns: i64) -> String {
    fasm_tool_parser().format_help(columns)
}

/// What `parser.error(message)` of the `fasm` tool writes to stderr: the
/// usage and `FASM tool: error: {message}`, encoded like Python's stderr
/// (`backslashreplace`).
#[must_use]
pub fn format_error(message: &PyStr, columns: i64) -> String {
    fasm_tool_parser().format_error(message, columns)
}

#[cfg(test)]
mod tests;

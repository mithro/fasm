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

//! A small parser for the YAML subset used by the databases:
//! `part.yaml` (nested block mappings with `!<...>` tags) and
//! `mapping/{parts,devices}.yaml` (two level mappings of scalars).
//!
//! Supported: block mappings (indentation by spaces), block sequences
//! indented under their key or at its indentation (`- item`, `- !<tag>` followed by a nested
//! mapping, `- key: value` compact mappings; used by the
//! `configuration_ranges` form of `part.yaml`), plain, single and double
//! quoted scalars, `!<verbatim>` and `!shorthand` tags, one line flow
//! mappings (`{frame_count: 30}`), `#` comments, a leading `---`.
//! Anything else (anchors, block scalars, flow sequences, multi line flow
//! collections, tabs in indentation) is reported as an error with its
//! line instead of being guessed at.
//!
//! A full YAML library is not used: `serde_yaml` is deprecated and
//! unmaintained, and the structure of these files is fixed and small
//! (see `docs/rewrite/DESIGN-xilinx-db.md` §3.5, §3.6, §8 "Implementation
//! notes (T5.2)").

use std::fmt;

/// A parsed node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    /// The tag without `!<`/`>` (or the `!shorthand` without `!`).
    pub(crate) tag: Option<String>,
    /// 1-based line of the node.
    pub(crate) line: usize,
    pub(crate) value: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    /// An empty value (`key:` with nothing after it).
    Null,
    /// A scalar, with quotes removed.
    Scalar(String),
    /// A mapping, in file order.
    Map(Vec<(String, Node)>),
    /// A sequence.
    Seq(Vec<Node>),
}

/// A YAML error: 1-based line and message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct YamlError {
    pub(crate) line: usize,
    pub(crate) message: String,
}

impl fmt::Display for YamlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

fn error<T>(line: usize, message: impl Into<String>) -> Result<T, YamlError> {
    Err(YamlError {
        line,
        message: message.into(),
    })
}

impl Node {
    /// The entries of a mapping node.
    pub(crate) fn as_map(&self) -> Result<&[(String, Node)], YamlError> {
        match &self.value {
            Value::Map(entries) => Ok(entries),
            _ => error(self.line, "expected a mapping"),
        }
    }

    /// The items of a sequence node.
    pub(crate) fn as_seq(&self) -> Result<&[Node], YamlError> {
        match &self.value {
            Value::Seq(items) => Ok(items),
            _ => error(self.line, "expected a sequence"),
        }
    }

    /// The text of a scalar node.
    pub(crate) fn as_str(&self) -> Result<&str, YamlError> {
        match &self.value {
            Value::Scalar(s) => Ok(s),
            _ => error(self.line, "expected a scalar"),
        }
    }

    /// A scalar as an unsigned 32-bit integer, like yaml-cpp's
    /// `as<uint32_t>()` (decimal, `0x` hex or `0o` octal).
    pub(crate) fn as_u32(&self) -> Result<u32, YamlError> {
        let s = self.as_str()?;
        parse_u32(s).ok_or_else(|| YamlError {
            line: self.line,
            message: format!("{s:?} is not an unsigned 32-bit integer"),
        })
    }

    /// The value of `key` in a mapping node.
    pub(crate) fn get(&self, key: &str) -> Result<Option<&Node>, YamlError> {
        Ok(self
            .as_map()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v))
    }

    /// The value of `key` in a mapping node, which must exist.
    pub(crate) fn require(&self, key: &str) -> Result<&Node, YamlError> {
        self.get(key)?.ok_or_else(|| YamlError {
            line: self.line,
            message: format!("missing key {key:?}"),
        })
    }
}

/// Parses an unsigned integer scalar (decimal, `0x`, `0o`).
pub(crate) fn parse_u32(s: &str) -> Option<u32> {
    let s = s.strip_prefix('+').unwrap_or(s);
    let (digits, radix) = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        (hex, 16)
    } else if let Some(oct) = s.strip_prefix("0o") {
        (oct, 8)
    } else {
        (s, 10)
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, radix).ok()
}

#[derive(Clone, Copy)]
struct Line<'a> {
    number: usize,
    indent: usize,
    text: &'a str,
}

/// Removes a `#` comment (a `#` at the start or after whitespace, outside
/// quotes) and trailing whitespace.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if (b == b'\\' && q == b'"')
                    || (b == b'\'' && q == b'\'' && bytes.get(i + 1) == Some(&b'\''))
                {
                    i += 1;
                } else if b == q {
                    quote = None;
                }
            }
            None => {
                if b == b'#' && (i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
                    return line[..i].trim_end();
                }
                if (b == b'"' || b == b'\'')
                    && (i == 0 || matches!(bytes[i - 1], b' ' | b'{' | b',' | b':'))
                {
                    quote = Some(b);
                }
            }
        }
        i += 1;
    }
    line.trim_end()
}

/// Parses a whole document.
pub(crate) fn parse(text: &str) -> Result<Node, YamlError> {
    let mut lines = Vec::new();
    let mut started = false;
    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let content = strip_comment(raw);
        if content.is_empty() {
            continue;
        }
        let indent = content.len() - content.trim_start_matches(' ').len();
        let text = &content[indent..];
        if text.starts_with('\t') {
            return error(number, "tabs are not allowed in indentation");
        }
        if !started && indent == 0 && text.starts_with('%') {
            continue; // %YAML / %TAG directive
        }
        if indent == 0 && (text == "---" || text.starts_with("--- ")) {
            if started {
                return error(number, "only one YAML document is supported");
            }
            started = true;
            let rest = text[3..].trim_start();
            if !rest.is_empty() {
                lines.push(Line {
                    number,
                    indent: 0,
                    text: rest,
                });
            }
            continue;
        }
        if indent == 0 && text == "..." {
            break;
        }
        started = true;
        lines.push(Line {
            number,
            indent,
            text,
        });
    }
    let mut parser = Parser { lines, pos: 0 };
    let Some(first) = parser.lines.first() else {
        return Ok(Node {
            tag: None,
            line: 1,
            value: Value::Null,
        });
    };
    let (line, indent) = (first.number, first.indent);
    // A document may start with a tag on its own line.
    let (tag, rest) = split_tag(first.text, line)?;
    let node = if tag.is_some() && rest.is_empty() {
        parser.pos = 1;
        match parser.lines.get(1) {
            None => Node {
                tag,
                line,
                value: Value::Null,
            },
            Some(next) => {
                let mut node = parser.block(next.indent)?;
                node.tag = tag;
                node
            }
        }
    } else {
        parser.block(indent)?
    };
    if let Some(extra) = parser.lines.get(parser.pos) {
        return error(extra.number, "unexpected content (bad indentation?)");
    }
    Ok(node)
}

/// Splits a leading tag off `text`.
fn split_tag(text: &str, line: usize) -> Result<(Option<String>, &str), YamlError> {
    if let Some(rest) = text.strip_prefix("!<") {
        let Some(end) = rest.find('>') else {
            return error(line, "unterminated !<...> tag");
        };
        return Ok((Some(rest[..end].to_owned()), rest[end + 1..].trim_start()));
    }
    if let Some(rest) = text.strip_prefix('!') {
        let end = rest.find([' ', '\t']).unwrap_or(rest.len());
        return Ok((Some(rest[..end].to_owned()), rest[end..].trim_start()));
    }
    Ok((None, text))
}

struct Parser<'a> {
    lines: Vec<Line<'a>>,
    pos: usize,
}

impl Parser<'_> {
    /// Parses the block mapping (or sequence) whose entries are at
    /// `indent`.
    fn block(&mut self, indent: usize) -> Result<Node, YamlError> {
        let first_line = self.lines[self.pos].number;
        if is_item(self.lines[self.pos].text) {
            return self.sequence(indent);
        }
        let mut entries: Vec<(String, Node)> = Vec::new();
        while let Some(line) = self.lines.get(self.pos) {
            if line.indent < indent {
                break;
            }
            let number = line.number;
            if line.indent > indent {
                return error(number, "unexpected indentation");
            }
            let text = line.text;
            if is_item(text) {
                return error(number, "a sequence item where a mapping key was expected");
            }
            let (key, rest) = split_key(text, number)?;
            if entries.iter().any(|(k, _)| *k == key) {
                return error(number, format!("duplicate key {key:?}"));
            }
            self.pos += 1;
            let (tag, rest) = split_tag(rest, number)?;
            let mut node = if rest.is_empty() {
                match self.lines.get(self.pos) {
                    Some(next) if next.indent > indent => self.block(next.indent)?,
                    // `key:` followed by `- item`s at the key's own
                    // indentation (valid YAML, what many emitters write).
                    Some(next) if next.indent == indent && is_item(next.text) => {
                        self.sequence(indent)?
                    }
                    _ => Node {
                        tag: None,
                        line: number,
                        value: Value::Null,
                    },
                }
            } else {
                inline_value(rest, number)?
            };
            if tag.is_some() {
                node.tag = tag;
            }
            entries.push((key, node));
        }
        Ok(Node {
            tag: None,
            line: first_line,
            value: Value::Map(entries),
        })
    }
}

fn is_item(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

/// `true` if `text` starts a (compact) block mapping: `key: value` or
/// `key:`, not a quoted scalar or flow collection.
fn is_mapping_start(text: &str) -> bool {
    !text.starts_with(['"', '\'', '{', '[']) && (text.contains(": ") || text.ends_with(':'))
}

impl Parser<'_> {
    /// Parses the block sequence whose `- ` items are at `indent`.
    fn sequence(&mut self, indent: usize) -> Result<Node, YamlError> {
        let first_line = self.lines[self.pos].number;
        let mut items = Vec::new();
        while let Some(&line) = self.lines.get(self.pos) {
            if line.indent < indent {
                break;
            }
            let number = line.number;
            if line.indent > indent {
                return error(number, "unexpected indentation");
            }
            if !is_item(line.text) {
                if items.is_empty() {
                    return error(number, "expected a `- ` sequence item");
                }
                // The next key of a mapping whose sequence value is at
                // the key's indentation.
                break;
            }
            let (tag, rest) = split_tag(line.text[1..].trim_start(), number)?;
            let mut node = if rest.is_empty() {
                self.pos += 1;
                match self.lines.get(self.pos) {
                    Some(next) if next.indent > indent => self.block(next.indent)?,
                    _ => Node {
                        tag: None,
                        line: number,
                        value: Value::Null,
                    },
                }
            } else if is_mapping_start(rest) || is_item(rest) {
                // `- key: value` (or `- - item`): the mapping (sequence)
                // continues at the column of `key`.
                let column = indent + (line.text.len() - rest.len());
                self.lines[self.pos] = Line {
                    number,
                    indent: column,
                    text: rest,
                };
                self.block(column)?
            } else {
                self.pos += 1;
                inline_value(rest, number)?
            };
            if tag.is_some() {
                node.tag = tag;
            }
            items.push(node);
        }
        Ok(Node {
            tag: None,
            line: first_line,
            value: Value::Seq(items),
        })
    }
}

/// Splits `key: rest` (or `key:`).
fn split_key(text: &str, line: usize) -> Result<(String, &str), YamlError> {
    if text.starts_with('"') || text.starts_with('\'') {
        let (key, used) = quoted(text, line)?;
        let rest = &text[used..];
        let Some(rest) = rest.strip_prefix(':') else {
            return error(line, "expected `:` after a quoted key");
        };
        if !(rest.is_empty() || rest.starts_with(' ')) {
            return error(line, "expected a space after `:`");
        }
        return Ok((key, rest.trim_start()));
    }
    let colon = text
        .find(": ")
        .or_else(|| text.ends_with(':').then(|| text.len() - 1));
    let Some(colon) = colon else {
        return error(line, format!("expected `key: value`, got {text:?}"));
    };
    let key = text[..colon].trim_end();
    if key.is_empty() || key.starts_with(['{', '[', '&', '*', '!', '?', '|', '>']) {
        return error(line, format!("unsupported mapping key {key:?}"));
    }
    Ok((key.to_owned(), text[colon + 1..].trim_start()))
}

/// Parses a scalar or flow mapping that fills the rest of a line.
fn inline_value(text: &str, line: usize) -> Result<Node, YamlError> {
    if text.starts_with('{') {
        let mut flow = Flow { text, pos: 0, line };
        let node = flow.value()?;
        flow.skip_ws();
        if flow.pos != text.len() {
            return error(line, "unexpected text after a flow mapping");
        }
        return Ok(node);
    }
    if text.starts_with('"') || text.starts_with('\'') {
        let (value, used) = quoted(text, line)?;
        if !text[used..].trim().is_empty() {
            return error(line, "unexpected text after a quoted scalar");
        }
        return Ok(scalar(value, line));
    }
    if text.starts_with(['[', '&', '*', '|', '>', '@', '`']) {
        return error(line, format!("unsupported YAML value {text:?}"));
    }
    if text.contains(": ") {
        return error(line, "mapping values are not allowed here");
    }
    Ok(scalar(text.to_owned(), line))
}

fn scalar(value: String, line: usize) -> Node {
    Node {
        tag: None,
        line,
        value: Value::Scalar(value),
    }
}

/// Parses a quoted scalar at the start of `text`; returns the value and
/// the number of bytes used.
fn quoted(text: &str, line: usize) -> Result<(String, usize), YamlError> {
    let quote = text.as_bytes()[0];
    let mut out = String::new();
    let mut chars = text.char_indices().skip(1);
    while let Some((i, c)) = chars.next() {
        if quote == b'\'' {
            if c == '\'' {
                if text[i + 1..].starts_with('\'') {
                    out.push('\'');
                    chars.next();
                    continue;
                }
                return Ok((out, i + 1));
            }
            out.push(c);
        } else {
            match c {
                '"' => return Ok((out, i + 1)),
                '\\' => {
                    let Some((_, e)) = chars.next() else {
                        break;
                    };
                    let simple = match e {
                        '\\' => '\\',
                        '"' => '"',
                        '/' => '/',
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        ' ' => ' ',
                        _ => {
                            return error(
                                line,
                                format!("unsupported escape \\{e} in a quoted scalar"),
                            )
                        }
                    };
                    out.push(simple);
                }
                _ => out.push(c),
            }
        }
    }
    error(line, "unterminated quoted scalar")
}

/// A one line flow mapping.
struct Flow<'a> {
    text: &'a str,
    pos: usize,
    line: usize,
}

impl Flow<'_> {
    fn skip_ws(&mut self) {
        while self.text[self.pos..].starts_with([' ', '\t']) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, c: char) -> bool {
        self.skip_ws();
        if self.text[self.pos..].starts_with(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn scalar(&mut self) -> Result<String, YamlError> {
        self.skip_ws();
        let rest = &self.text[self.pos..];
        if rest.starts_with('"') || rest.starts_with('\'') {
            let (value, used) = quoted(rest, self.line)?;
            self.pos += used;
            return Ok(value);
        }
        let end = rest
            .find([',', '}', ':', '{', '[', ']'])
            .unwrap_or(rest.len());
        let value = rest[..end].trim_end();
        if value.is_empty() {
            return error(self.line, "empty scalar in a flow mapping");
        }
        self.pos += end;
        Ok(value.to_owned())
    }

    fn value(&mut self) -> Result<Node, YamlError> {
        self.skip_ws();
        let (tag, _) = split_tag(&self.text[self.pos..], self.line)?;
        if let Some(t) = &tag {
            // Skip the tag text itself.
            let rest = &self.text[self.pos..];
            let used = if rest.starts_with("!<") {
                t.len() + 3
            } else {
                t.len() + 1
            };
            self.pos += used;
            self.skip_ws();
        }
        let mut node = if self.eat('{') {
            let mut entries: Vec<(String, Node)> = Vec::new();
            if !self.eat('}') {
                loop {
                    let key = self.scalar()?;
                    if !self.eat(':') {
                        return error(self.line, "expected `:` in a flow mapping");
                    }
                    let value = self.value()?;
                    if entries.iter().any(|(k, _)| *k == key) {
                        return error(self.line, format!("duplicate key {key:?}"));
                    }
                    entries.push((key, value));
                    if self.eat('}') {
                        break;
                    }
                    if !self.eat(',') {
                        return error(self.line, "expected `,` or `}` in a flow mapping");
                    }
                }
            }
            Node {
                tag: None,
                line: self.line,
                value: Value::Map(entries),
            }
        } else {
            scalar(self.scalar()?, self.line)
        };
        node.tag = tag;
        Ok(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences() {
        let text = "\
ranges:
  - !<range>
    begin: !<addr>
      row: 1
    end: {row: 2}
  - a: 1
    b: 'x'
  - plain
  -
  - - nested
";
        let doc = parse(text).unwrap();
        let items = doc.require("ranges").unwrap().as_seq().unwrap();
        assert_eq!(items.len(), 5);
        assert_eq!(items[0].tag.as_deref(), Some("range"));
        let begin = items[0].require("begin").unwrap();
        assert_eq!(begin.tag.as_deref(), Some("addr"));
        assert_eq!(begin.require("row").unwrap().as_u32().unwrap(), 1);
        assert_eq!(
            items[0]
                .require("end")
                .unwrap()
                .require("row")
                .unwrap()
                .as_u32()
                .unwrap(),
            2
        );
        assert_eq!(items[1].require("b").unwrap().as_str().unwrap(), "x");
        assert_eq!(items[2].as_str().unwrap(), "plain");
        assert_eq!(items[3].value, Value::Null);
        assert_eq!(items[4].as_seq().unwrap()[0].as_str().unwrap(), "nested");
        assert!(doc.as_seq().is_err());
        assert_eq!(parse("- 1\n- 2\n").unwrap().as_seq().unwrap().len(), 2);
        // A sequence at its key's indentation, then the next key.
        let doc = parse("a:\n- 1\n- b: 2\n  c: 3\nd: 4\n").unwrap();
        let a = doc.require("a").unwrap().as_seq().unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a[1].require("c").unwrap().as_u32().unwrap(), 3);
        assert_eq!(doc.require("d").unwrap().as_u32().unwrap(), 4);
    }

    #[test]
    fn part_yaml_shape() {
        let text = "!<xilinx/xc7series/part>\nidcode: 0x362d093\nglobal_clock_regions:\n  top: !<xilinx/xc7series/global_clock_region>\n    rows:\n      0: !<xilinx/xc7series/row>\n        configuration_buses:\n          CLB_IO_CLK: !<xilinx/xc7series/configuration_bus>\n            configuration_columns:\n              0: !<xilinx/xc7series/configuration_column>\n                frame_count: 42\n              1: {frame_count: 30}  # comment\n  bottom: !<xilinx/xc7series/global_clock_region>\n    rows: {}\n";
        let doc = parse(text).unwrap();
        assert_eq!(doc.tag.as_deref(), Some("xilinx/xc7series/part"));
        assert_eq!(doc.require("idcode").unwrap().as_u32().unwrap(), 0x362d093);
        let regions = doc.require("global_clock_regions").unwrap();
        let top = regions.require("top").unwrap();
        assert_eq!(
            top.tag.as_deref(),
            Some("xilinx/xc7series/global_clock_region")
        );
        let row = &top.require("rows").unwrap().as_map().unwrap()[0];
        assert_eq!(row.0, "0");
        assert_eq!(row.1.tag.as_deref(), Some("xilinx/xc7series/row"));
        let columns = row
            .1
            .require("configuration_buses")
            .unwrap()
            .require("CLB_IO_CLK")
            .unwrap()
            .require("configuration_columns")
            .unwrap()
            .as_map()
            .unwrap();
        assert_eq!(columns.len(), 2);
        assert_eq!(
            columns[1]
                .1
                .require("frame_count")
                .unwrap()
                .as_u32()
                .unwrap(),
            30
        );
        assert_eq!(columns[1].1.line, 12);
        let bottom_rows = regions.require("bottom").unwrap().require("rows").unwrap();
        assert!(bottom_rows.as_map().unwrap().is_empty());
    }

    #[test]
    fn mapping_files() {
        let text = "# header\n\n\"xc7a35t\":\n  fabric: \"xc7a50t\"\nxc7a35tcsg324-1:\n    device: xc7a35t\n    package: ''\n    speedgrade: '1'\n    other: 2L\n    q: 'it''s # not a comment'\n    e: \"a\\\"b\"\n    n:\n";
        let doc = parse(text).unwrap();
        let entries = doc.as_map().unwrap();
        assert_eq!(entries[0].0, "xc7a35t");
        assert_eq!(
            entries[0].1.require("fabric").unwrap().as_str().unwrap(),
            "xc7a50t"
        );
        let part = &entries[1].1;
        assert_eq!(part.require("package").unwrap().as_str().unwrap(), "");
        assert_eq!(part.require("speedgrade").unwrap().as_str().unwrap(), "1");
        assert_eq!(part.require("other").unwrap().as_str().unwrap(), "2L");
        assert_eq!(
            part.require("q").unwrap().as_str().unwrap(),
            "it's # not a comment"
        );
        assert_eq!(part.require("e").unwrap().as_str().unwrap(), "a\"b");
        assert_eq!(part.require("n").unwrap().value, Value::Null);
        assert!(part.get("missing").unwrap().is_none());
        assert!(part.require("missing").is_err());
    }

    #[test]
    fn document_markers() {
        let doc = parse("%YAML 1.2\n--- !<t>\na: 1\n...\nignored: [\n").unwrap();
        assert_eq!(doc.tag.as_deref(), Some("t"));
        assert_eq!(doc.require("a").unwrap().as_u32().unwrap(), 1);
        assert_eq!(parse("").unwrap().value, Value::Null);
        assert_eq!(parse("# only a comment\n").unwrap().value, Value::Null);
    }

    #[test]
    fn unsupported_is_an_error_with_a_line() {
        let cases = [
            ("- 1\na: 2\n", 2, "unexpected content"),
            ("a:\n  - 1\n  b: 2\n", 3, "indentation"),
            ("a: 1\n  b: 2\n", 2, "indentation"),
            ("a: 1\na: 2\n", 2, "duplicate"),
            ("a: [1, 2]\n", 1, "unsupported"),
            ("a: &x 1\n", 1, "unsupported"),
            ("a: |\n  text\n", 1, "unsupported"),
            ("a: 'x\n", 1, "unterminated"),
            ("a: {b: 1\n", 1, "flow"),
            ("a: {b: 1} x\n", 1, "after a flow"),
            ("just text\n", 1, "expected `key: value`"),
            ("a: b: c\n", 1, "not allowed"),
            ("a:\n\tb: 1\n", 2, "tabs"),
            ("--- a: 1\n---\n", 2, "one YAML document"),
            ("a: \"\\q\"\n", 1, "escape"),
            ("!<t\n", 1, "unterminated"),
            ("\"a\" 1\n", 1, "expected `:`"),
        ];
        for (text, line, message) in cases {
            let err = parse(text).unwrap_err();
            assert_eq!(err.line, line, "{text:?}: {err}");
            assert!(err.message.contains(message), "{text:?}: {err}");
        }
        let doc = parse("a: x\nb: {c: 1}\n").unwrap();
        assert!(doc.require("a").unwrap().as_map().is_err());
        assert!(doc.require("b").unwrap().as_str().is_err());
        assert!(doc.require("a").unwrap().as_u32().is_err());
    }

    #[test]
    fn integers() {
        assert_eq!(parse_u32("0x4a42093"), Some(0x4a42093));
        assert_eq!(parse_u32("042"), Some(42));
        assert_eq!(parse_u32("+7"), Some(7));
        assert_eq!(parse_u32("0o17"), Some(15));
        for bad in ["", "-1", "0x", "1.0", "4294967296", "12a"] {
            assert_eq!(parse_u32(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn fuzz_does_not_panic() {
        let alphabet = b"ab: {}!<>\n  -'\"#,\\x0\t%.";
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        for _ in 0..20_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let len = (state % 30) as usize;
            let mut s = state;
            let text: String = (0..len)
                .map(|_| {
                    s = s
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    alphabet[((s >> 33) % alphabet.len() as u64) as usize] as char
                })
                .collect();
            let _ = parse(&text);
        }
        // Multi byte characters around the special characters.
        for text in ["é: 'ü'", "a: \"é\\", "a: {é: ü}", "'é", "!<é"] {
            let _ = parse(text);
        }
    }
}

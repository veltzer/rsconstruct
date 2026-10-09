//! A positioned YAML node tree, shaped like the one go-yaml hands actionlint.
//!
//! actionlint's parser works on `yaml.Node`s: kind, resolved tag (`!!str`,
//! `!!int`, ...), raw value, quoting style and the 1-based line and column of
//! the node's first character. This module builds the same tree from
//! libyaml-safer's events, resolving plain scalar tags exactly as go-yaml's
//! `resolve()` does, since actionlint's checks ("expected bool value but found
//! scalar node with \"!!str\" tag") key off those tags.

use std::fmt::Write as _;

use libyaml_safer::{ErrorKind, EventData, Parser, ScalarStyle};
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Document,
    Sequence,
    Mapping,
    Scalar,
    /// An alias that could not be resolved because it refers to the node
    /// being built (a recursive alias). go-yaml leaves it as an alias node.
    Alias,
}

impl Kind {
    /// go-yaml's kind names, as actionlint prints them.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Sequence => "sequence",
            Self::Mapping => "mapping",
            Self::Scalar => "scalar",
            Self::Alias => "alias",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub kind: Kind,
    /// The short tag: `!!str`, `!!int`, `!!float`, `!!bool`, `!!null`,
    /// `!!map`, `!!seq`, `!!merge`, `!!timestamp`, or an explicit tag.
    pub tag: String,
    /// The scalar value as written (after YAML escape processing).
    pub value: String,
    /// Whether the scalar was single- or double-quoted.
    pub quoted: bool,
    /// 1-based.
    pub line: u32,
    /// 1-based.
    pub column: u32,
    /// Mapping: key, value, key, value, ...; sequence: the items; document:
    /// the root node.
    pub content: Vec<Self>,
    pub anchor: Option<String>,
}

impl Node {
    /// The `(key, value)` pairs of a mapping node.
    pub fn pairs(&self) -> impl Iterator<Item = (&Self, &Self)> {
        self.content
            .as_chunks::<2>()
            .0
            .iter()
            .map(|kv| (&kv[0], &kv[1]))
    }
}

/// A problem go-yaml would raise while reading the document (actionlint
/// reports it as "could not parse as YAML: ..."), at go-yaml's position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlError {
    pub message: String,
    pub line: u32,
    pub column: u32,
}

/// Alias problems actionlint's parser reports itself (as syntax-check
/// errors, after the tree is built): recursive aliases and unused anchors.
#[derive(Debug, Default)]
pub struct AnchorReport {
    /// `(message, line, column)`.
    pub errors: Vec<(String, u32, u32)>,
}

/// One anchor definition. An alias refers to the latest definition of its
/// name, so a name defined twice is two entries, each with its own "used".
struct AnchorDef {
    name: String,
    line: u32,
    column: u32,
    /// `None` while the anchored node is still being built (an alias to it
    /// is recursive).
    node: Option<Node>,
    used: bool,
}

struct Composer<'r, 'b> {
    parser: Parser<&'r mut &'b [u8]>,
    anchors: Vec<AnchorDef>,
    anchor_errors: Vec<(String, u32, u32)>,
}

/// Parses the first document of `source` into a tree, as go-yaml's
/// `Unmarshal` into a `yaml.Node` does. An empty stream gives a document
/// node with no content at 0:0. Aliases are replaced by their anchored nodes
/// the way actionlint's `resolveAliases` does, and the report carries the
/// problems that step finds.
pub fn parse(source: &[u8]) -> (Result<Node, YamlError>, AnchorReport) {
    let mut input: &[u8] = source;
    let mut parser = Parser::new();
    parser.set_input_string(&mut input);
    let mut composer = Composer {
        parser,
        anchors: Vec::new(),
        anchor_errors: Vec::new(),
    };
    let result = composer.compose_document();
    let mut report = AnchorReport {
        errors: composer.anchor_errors,
    };
    if result.is_ok() {
        // actionlint: `anchor %q is defined but not used`, at the anchored node.
        for def in &composer.anchors {
            if !def.used {
                report.errors.push((
                    format!("anchor {} is defined but not used", go_quote(&def.name)),
                    def.line,
                    def.column,
                ));
            }
        }
    }
    (result, report)
}

impl Composer<'_, '_> {
    /// Records the finished node of the latest definition of an anchor.
    fn complete_anchor(&mut self, name: &str, node: &Node) {
        if let Some(def) = self.anchors.iter_mut().rev().find(|d| d.name == name) {
            def.node = Some(node.clone());
        }
    }

    fn next(&mut self) -> Result<libyaml_safer::Event, YamlError> {
        self.parser.parse().map_err(|e| {
            // go-yaml's parser.fail(): the context mark when it has a line,
            // else the problem mark; scanner errors get +1 on the line.
            let scanner = e.kind() == ErrorKind::Scanner;
            let context = e.context_mark();
            let problem = e.problem_mark();
            let mut line = match (context, problem) {
                (Some(c), _) if c.line != 0 => c.line,
                (_, Some(p)) if p.line != 0 => p.line,
                _ => 0,
            };
            if scanner && line != 0 {
                line += 1;
            }
            let column = match (context, problem) {
                (Some(c), _) if c.column != 0 => c.column,
                (_, Some(p)) if p.column != 0 => p.column,
                _ => 0,
            };
            let mut message = e.problem().to_string();
            if message.is_empty() {
                message = "unknown problem parsing YAML content".to_string();
            }
            YamlError {
                message,
                line: u32::try_from(line).unwrap_or(u32::MAX),
                column: u32::try_from(column).unwrap_or(u32::MAX),
            }
        })
    }

    fn compose_document(&mut self) -> Result<Node, YamlError> {
        let first = self.next()?;
        if !matches!(first.data, EventData::StreamStart { .. }) {
            return Err(YamlError {
                message: "expected stream start".to_string(),
                line: 0,
                column: 0,
            });
        }
        let event = self.next()?;
        match event.data {
            EventData::StreamEnd => Ok(Node {
                kind: Kind::Document,
                tag: String::new(),
                value: String::new(),
                quoted: false,
                line: 0,
                column: 0,
                content: Vec::new(),
                anchor: None,
            }),
            EventData::DocumentStart { .. } => {
                let mut doc = Node {
                    kind: Kind::Document,
                    tag: String::new(),
                    value: String::new(),
                    quoted: false,
                    line: mark_line(event.start_mark),
                    column: mark_column(event.start_mark),
                    content: Vec::new(),
                    anchor: None,
                };
                let child_event = self.next()?;
                let child = self.compose(child_event)?;
                doc.content.push(child);
                let end = self.next()?;
                if !matches!(end.data, EventData::DocumentEnd { .. }) {
                    return Err(YamlError {
                        message: "expected document end".to_string(),
                        line: mark_line(end.start_mark),
                        column: mark_column(end.start_mark),
                    });
                }
                // Further documents are ignored, as go-yaml's Unmarshal reads
                // only the first.
                Ok(doc)
            }
            _ => Err(YamlError {
                message: "expected document start".to_string(),
                line: mark_line(event.start_mark),
                column: mark_column(event.start_mark),
            }),
        }
    }

    fn compose(&mut self, event: libyaml_safer::Event) -> Result<Node, YamlError> {
        let line = mark_line(event.start_mark);
        let column = mark_column(event.start_mark);
        match event.data {
            EventData::Scalar {
                anchor,
                tag,
                value,
                style,
                ..
            } => {
                let quoted = matches!(style, ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted);
                let plain = matches!(style, ScalarStyle::Plain | ScalarStyle::Any);
                let resolved = match tag {
                    Some(t) if t != "!" => short_tag(&t),
                    _ => {
                        if plain {
                            if value == "<<" {
                                "!!merge".to_string()
                            } else {
                                resolve(&value).to_string()
                            }
                        } else {
                            "!!str".to_string()
                        }
                    }
                };
                let node = Node {
                    kind: Kind::Scalar,
                    tag: resolved,
                    value,
                    quoted,
                    line,
                    column,
                    content: Vec::new(),
                    anchor: anchor.clone(),
                };
                if let Some(name) = anchor {
                    self.anchors.push(AnchorDef {
                        name,
                        line,
                        column,
                        node: Some(node.clone()),
                        used: false,
                    });
                }
                Ok(node)
            }
            EventData::SequenceStart { anchor, tag, .. } => {
                let resolved = match tag {
                    Some(t) if t != "!" => short_tag(&t),
                    _ => "!!seq".to_string(),
                };
                if let Some(name) = &anchor {
                    self.anchors.push(AnchorDef {
                        name: name.clone(),
                        line,
                        column,
                        node: None,
                        used: false,
                    });
                }
                let mut node = Node {
                    kind: Kind::Sequence,
                    tag: resolved,
                    value: String::new(),
                    quoted: false,
                    line,
                    column,
                    content: Vec::new(),
                    anchor: anchor.clone(),
                };
                loop {
                    let item = self.next()?;
                    if matches!(item.data, EventData::SequenceEnd) {
                        break;
                    }
                    let child = self.compose(item)?;
                    node.content.push(child);
                }
                if let Some(name) = anchor {
                    self.complete_anchor(&name, &node);
                }
                Ok(node)
            }
            EventData::MappingStart { anchor, tag, .. } => {
                let resolved = match tag {
                    Some(t) if t != "!" => short_tag(&t),
                    _ => "!!map".to_string(),
                };
                if let Some(name) = &anchor {
                    self.anchors.push(AnchorDef {
                        name: name.clone(),
                        line,
                        column,
                        node: None,
                        used: false,
                    });
                }
                let mut node = Node {
                    kind: Kind::Mapping,
                    tag: resolved,
                    value: String::new(),
                    quoted: false,
                    line,
                    column,
                    content: Vec::new(),
                    anchor: anchor.clone(),
                };
                loop {
                    let key_event = self.next()?;
                    if matches!(key_event.data, EventData::MappingEnd) {
                        break;
                    }
                    let key = self.compose(key_event)?;
                    let value_event = self.next()?;
                    let value = self.compose(value_event)?;
                    node.content.push(key);
                    node.content.push(value);
                }
                if let Some(name) = anchor {
                    self.complete_anchor(&name, &node);
                }
                Ok(node)
            }
            EventData::Alias { anchor } => {
                // The latest definition of the name, as go-yaml resolves it.
                let Some(def) = self.anchors.iter_mut().rev().find(|d| d.name == anchor) else {
                    return Err(YamlError {
                        message: format!("yaml: unknown anchor '{anchor}' referenced"),
                        line: 0,
                        column: 0,
                    });
                };
                def.used = true;
                if let Some(target) = &def.node {
                    // actionlint replaces the alias by the anchored node
                    // itself, positions included.
                    return Ok(target.clone());
                }
                // Recursive: the anchored node is still being built.
                let (decl_line, decl_column) = (def.line, def.column);
                self.anchor_errors.push((
                    format!(
                        "recursive alias {} is found. anchor was declared at line:{decl_line}, column:{decl_column}",
                        go_quote(&anchor)
                    ),
                    line,
                    column,
                ));
                Ok(Node {
                    kind: Kind::Alias,
                    tag: String::new(),
                    value: anchor,
                    quoted: false,
                    line,
                    column,
                    content: Vec::new(),
                    anchor: None,
                })
            }
            other => Err(YamlError {
                message: format!("unexpected YAML event {other:?}"),
                line,
                column,
            }),
        }
    }
}

fn mark_line(mark: libyaml_safer::Mark) -> u32 {
    u32::try_from(mark.line + 1).unwrap_or(u32::MAX)
}

fn mark_column(mark: libyaml_safer::Mark) -> u32 {
    u32::try_from(mark.column + 1).unwrap_or(u32::MAX)
}

/// go-yaml's `shortTag`: `tag:yaml.org,2002:str` becomes `!!str`.
fn short_tag(tag: &str) -> String {
    match tag.strip_prefix("tag:yaml.org,2002:") {
        Some(rest) => format!("!!{rest}"),
        None => tag.to_string(),
    }
}

static YAML_STYLE_FLOAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[-+]?(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?$").expect("valid regex")
});

/// go-yaml's `resolve("", value)` for a plain scalar: the implicit tag.
pub fn resolve(value: &str) -> &'static str {
    let Some(first) = value.bytes().next() else {
        return "!!null";
    };
    let hint = match first {
        b'+' | b'-' => b'S',
        b'0'..=b'9' => b'D',
        b'y' | b'Y' | b'n' | b'N' | b't' | b'T' | b'f' | b'F' | b'o' | b'O' | b'~' => b'M',
        b'.' => b'.',
        _ => 0,
    };
    if hint == 0 {
        return "!!str";
    }
    match value {
        "true" | "True" | "TRUE" | "false" | "False" | "FALSE" => return "!!bool",
        "~" | "null" | "Null" | "NULL" => return "!!null",
        ".nan" | ".NaN" | ".NAN" | ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF"
        | "-.inf" | "-.Inf" | "-.INF" | "-0" | "-0.0" => return "!!float",
        _ => {}
    }
    match hint {
        b'M' => "!!str",
        b'.' => {
            if go_parse_float(value).is_some() {
                "!!float"
            } else {
                "!!str"
            }
        }
        _ => {
            if is_go_yaml_timestamp(value) {
                return "!!timestamp";
            }
            let plain: String = value.chars().filter(|c| *c != '_').collect();
            if go_parse_int_base0(&plain).is_some() || go_parse_uint_base0(&plain) {
                return "!!int";
            }
            if YAML_STYLE_FLOAT.is_match(&plain) && go_parse_float(&plain).is_some() {
                return "!!float";
            }
            "!!str"
        }
    }
}

/// Go's `strconv.ParseInt(s, 0, 64)`: optional sign, then a base prefix
/// (`0x`, `0o`, `0b`, or a leading `0` for octal) or decimal.
pub fn go_parse_int_base0(s: &str) -> Option<i64> {
    let (negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if digits.is_empty() {
        return None;
    }
    let (radix, body) = base_prefix(digits);
    if body.is_empty() || body.starts_with('_') || body.ends_with('_') || body.contains("__") {
        return None;
    }
    let body: String = body.chars().filter(|c| *c != '_').collect();
    let magnitude = u64::from_str_radix(&body, radix).ok()?;
    if negative {
        if magnitude > (i64::MAX as u64) + 1 {
            return None;
        }
        Some((magnitude as i64).wrapping_neg())
    } else {
        i64::try_from(magnitude).ok()
    }
}

fn go_parse_uint_base0(s: &str) -> bool {
    if s.starts_with('-') || s.starts_with('+') || s.is_empty() {
        return false;
    }
    let (radix, body) = base_prefix(s);
    if body.is_empty() || body.starts_with('_') || body.ends_with('_') || body.contains("__") {
        return false;
    }
    let body: String = body.chars().filter(|c| *c != '_').collect();
    u64::from_str_radix(&body, radix).is_ok()
}

fn base_prefix(digits: &str) -> (u32, &str) {
    let lower = digits.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("0x") {
        return (16, &digits[digits.len() - rest.len()..]);
    }
    if let Some(rest) = lower.strip_prefix("0o") {
        return (8, &digits[digits.len() - rest.len()..]);
    }
    if let Some(rest) = lower.strip_prefix("0b") {
        return (2, &digits[digits.len() - rest.len()..]);
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return (8, &digits[1..]);
    }
    (10, digits)
}

/// Go's `strconv.ParseFloat(s, 64)`: decimal and hex floats, `inf`,
/// `infinity` and `nan` in any case, with an optional sign.
pub fn go_parse_float(s: &str) -> Option<f64> {
    let lower = s.to_ascii_lowercase();
    let unsigned = lower.trim_start_matches(['+', '-']);
    if lower.len() - unsigned.len() > 1 {
        return None;
    }
    match unsigned {
        "inf" | "infinity" => {
            return Some(if lower.starts_with('-') {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            });
        }
        "nan" => return Some(f64::NAN),
        _ => {}
    }
    if unsigned.is_empty() {
        return None;
    }
    if let Some(body) = unsigned.strip_prefix("0x") {
        // Hex floats are rare in workflows; accept Go's shape loosely.
        let (mantissa, exponent) = body.split_once('p').unwrap_or((body, "0"));
        let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        if int_part.is_empty() && frac_part.is_empty() {
            return None;
        }
        let int_value = if int_part.is_empty() {
            0
        } else {
            u64::from_str_radix(int_part, 16).ok()?
        };
        let mut frac_value = 0f64;
        let mut scale = 1f64 / 16.0;
        for c in frac_part.chars() {
            frac_value = f64::mul_add(f64::from(c.to_digit(16)?), scale, frac_value);
            scale /= 16.0;
        }
        let exponent: i32 = exponent.parse().ok()?;
        let value = (int_value as f64 + frac_value) * 2f64.powi(exponent);
        return Some(if lower.starts_with('-') {
            -value
        } else {
            value
        });
    }
    // Decimal: digits with optional fraction and exponent; underscores
    // between digits are allowed by Go's syntax.
    let cleaned: String = s.chars().filter(|c| *c != '_').collect();
    if s.contains("__") || s.starts_with('_') || s.ends_with('_') {
        return None;
    }
    let body = cleaned.trim_start_matches(['+', '-']);
    let valid_shape = {
        let (mantissa, exponent) = match body.find(['e', 'E']) {
            Some(i) => (&body[..i], Some(&body[i + 1..])),
            None => (body, None),
        };
        let (int_part, frac_part) = match mantissa.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (mantissa, None),
        };
        let digits_ok = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
        let mantissa_ok = match frac_part {
            Some(f) => {
                (digits_ok(int_part) && (f.is_empty() || digits_ok(f)))
                    || (int_part.is_empty() && digits_ok(f))
            }
            None => digits_ok(int_part),
        };
        let exponent_ok = match exponent {
            Some(e) => {
                let e = e.strip_prefix(['+', '-']).unwrap_or(e);
                digits_ok(e)
            }
            None => true,
        };
        mantissa_ok && exponent_ok
    };
    if !valid_shape {
        return None;
    }
    cleaned.parse::<f64>().ok()
}

/// go-yaml's `parseTimestamp`: the few layouts it accepts.
fn is_go_yaml_timestamp(s: &str) -> bool {
    static DATE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(\d{4})-(\d{1,2})-(\d{1,2})$").expect("valid regex"));
    static DATE_TIME_TZ: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\d{4})-(\d{1,2})-(\d{1,2})[Tt](\d{1,2}):(\d{1,2}):(\d{1,2})(\.\d+)?(Z|[+-]\d{2}:\d{2})$")
            .expect("valid regex")
    });
    static DATE_TIME_SPACE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\d{4})-(\d{1,2})-(\d{1,2}) (\d{1,2}):(\d{1,2}):(\d{1,2})(\.\d+)?$")
            .expect("valid regex")
    });
    let bytes = s.as_bytes();
    if bytes.len() < 5 || !bytes[..4].iter().all(u8::is_ascii_digit) || bytes[4] != b'-' {
        return false;
    }
    let captures = DATE
        .captures(s)
        .or_else(|| DATE_TIME_TZ.captures(s))
        .or_else(|| DATE_TIME_SPACE.captures(s));
    let Some(c) = captures else {
        return false;
    };
    let num = |i: usize| {
        c.get(i)
            .map_or(0, |m| m.as_str().parse::<u32>().unwrap_or(99))
    };
    let (year, month, day) = (num(1), num(2), num(3));
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return false;
    }
    if c.get(4).is_some() && (num(4) > 23 || num(5) > 59 || num(6) > 59) {
        return false;
    }
    true
}

const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) {
                29
            } else {
                28
            }
        }
    }
}

/// Go's `%q` for strings: double quotes with Go escapes.
pub fn go_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{b}' => out.push_str("\\v"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

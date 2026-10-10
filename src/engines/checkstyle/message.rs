//! Messages: the English texts of the checks' `messages.properties`,
//! formatted by a port of `java.text.MessageFormat` (in `Locale.ROOT`), and
//! ordered and printed as checkstyle's `LocalizedMessage` and
//! `AuditEventDefaultFormatter` do.

use std::cmp::Ordering;

/// A message argument: checkstyle passes strings and line numbers.
#[derive(Clone, Debug)]
pub enum Arg {
    Str(String),
    Int(i32),
}

/// `SeverityLevel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Ignore,
    Info,
    Warning,
    Error,
}

impl Severity {
    /// `SeverityLevel.getInstance`: the name, trimmed, in any case.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "IGNORE" => Some(Self::Ignore),
            "INFO" => Some(Self::Info),
            "WARNING" => Some(Self::Warning),
            "ERROR" => Some(Self::Error),
            _ => None,
        }
    }

    /// The tag `AuditEventDefaultFormatter` prints.
    const fn tag(self) -> &'static str {
        match self {
            Self::Ignore => "IGNORE",
            Self::Info => "INFO",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
        }
    }
}

/// The default (English) pattern of a message key.
pub fn default_pattern(key: &str) -> &'static str {
    match key {
        "empty.statement" => "Empty statement.",
        "upperEll" => "Should use uppercase ''L''.",
        "array.type.style" => "Array brackets at illegal position.",
        "string.literal.equality" => {
            "Literal Strings should be compared using equals(), not ''{0}''."
        }
        "simplify.expression" => "Expression can be simplified.",
        "simplify.boolReturn" => "Conditional logic can be removed.",
        "import.lang" => "Redundant import from the java.lang package - {0}.",
        "import.same" => "Redundant import from the same package - {0}.",
        "import.duplicate" => "Duplicate import to line {0,number,integer} - {1}.",
        "covariant.equals" => "covariant equals without overriding equals(java.lang.Object).",
        "default.comes.last" => "Default should be last label in the switch.",
        "default.comes.last.in.casegroup" => "Default should be last label in the case group.",
        "equals.noHashCode" => {
            "Definition of ''equals()'' without corresponding definition of ''hashCode()''."
        }
        "equals.noEquals" => {
            "Definition of ''hashCode()'' without corresponding definition of ''equals()''."
        }
        "fall.through" => "Fall through from previous branch of the switch statement.",
        "fall.through.last" => "Fall through from the last branch of the switch statement.",
        "modified.control.variable" => "Control variable ''{0}'' is modified.",
        "multiple.statements.line" => "Only one statement per line allowed.",
        "import.unused" => "Unused import - {0}.",
        "noNewlineAtEOF" => "File does not end with a newline.",
        "wrong.line.end" => {
            "Expected line ending for file is LF(\\n), but CRLF(\\r\\n) is detected."
        }
        _ => "",
    }
}

/// A piece of a parsed `MessageFormat` pattern.
#[derive(Clone, Debug)]
enum Piece {
    Text(String),
    /// `{n}`, or `{n,number}` / `{n,number,integer}` (`number` true; the
    /// two print an integer argument alike).
    Arg {
        index: usize,
        number: bool,
    },
}

/// `MessageFormat.applyPattern`, for the argument formats checkstyle's
/// messages use: plain, `number` and `number,integer`.
fn parse_pattern(pattern: &str) -> Result<Vec<Piece>, String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut quoted = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                text.push('\'');
                i += 2;
                continue;
            }
            quoted = !quoted;
            i += 1;
            continue;
        }
        if c == '{' && !quoted {
            let close = chars[i + 1..]
                .iter()
                .position(|&c| c == '}')
                .ok_or("Unmatched braces in the pattern.")?;
            let inside: String = chars[i + 1..i + 1 + close].iter().collect();
            let parts: Vec<&str> = inside.split(',').map(str::trim).collect();
            let index: usize = parts[0]
                .parse()
                .map_err(|_| format!("can't parse argument number: {}", parts[0]))?;
            let number = match parts.as_slice() {
                [_] => false,
                [_, "number"] | [_, "number", "integer"] => true,
                _ => return Err(format!("unsupported format element {{{inside}}}")),
            };
            if !text.is_empty() {
                pieces.push(Piece::Text(std::mem::take(&mut text)));
            }
            pieces.push(Piece::Arg { index, number });
            i += close + 2;
            continue;
        }
        text.push(c);
        i += 1;
    }
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
    Ok(pieces)
}

/// `NumberFormat` in `Locale.ROOT`: grouping by three with `,`.
fn group_digits(n: i32) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(d);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// Check that a custom message pattern is one this port can format.
pub fn validate_pattern(pattern: &str) -> Result<(), String> {
    parse_pattern(pattern).map(|_| ())
}

/// `new MessageFormat(pattern, Locale.ROOT).format(args)`.
pub fn format(pattern: &str, args: &[Arg]) -> Result<String, String> {
    let mut out = String::new();
    for piece in parse_pattern(pattern)? {
        match piece {
            Piece::Text(t) => out.push_str(&t),
            Piece::Arg { index, number } => match args.get(index) {
                None => {
                    out.push('{');
                    out.push_str(&index.to_string());
                    out.push('}');
                }
                Some(Arg::Int(n)) => out.push_str(&group_digits(*n)),
                Some(Arg::Str(s)) if number => {
                    return Err(format!(
                        "java.lang.IllegalArgumentException: Cannot format given Object as a Number ({s})"
                    ));
                }
                Some(Arg::Str(s)) => out.push_str(s),
            },
        }
    }
    Ok(out)
}

/// A message's text: the module's custom message for the key, else the
/// default one.
pub fn format_key(key: &str, custom: Option<&str>, args: &[Arg]) -> Result<String, String> {
    format(custom.unwrap_or_else(|| default_pattern(key)), args)
}

/// A formatted violation, as an `AuditEvent` carries it.
#[derive(Clone, Debug)]
pub struct Message {
    pub line: i32,
    /// 1-based, tabs expanded; 0 when the message has no column.
    pub col: i32,
    pub severity: Severity,
    pub module_id: Option<String>,
    /// The check's class name without `Check`.
    pub source: &'static str,
    pub text: String,
}

fn utf16_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

impl Message {
    /// `LocalizedMessage.compareTo`: line, column, module id (none
    /// first), text. Messages that compare equal are one message.
    pub fn compare(&self, other: &Self) -> Ordering {
        self.line
            .cmp(&other.line)
            .then(self.col.cmp(&other.col))
            .then_with(|| match (&self.module_id, &other.module_id) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                (Some(a), Some(b)) => utf16_cmp(a, b),
            })
            .then_with(|| utf16_cmp(&self.text, &other.text))
    }

    /// `AuditEventDefaultFormatter.format`.
    pub fn render(&self, file: &str) -> String {
        let col = if self.col > 0 {
            format!(":{}", self.col)
        } else {
            String::new()
        };
        let module = self.module_id.as_deref().unwrap_or(self.source);
        format!(
            "[{}] {file}:{}{col}: {} [{module}]",
            self.severity.tag(),
            self.line,
            self.text
        )
    }
}

/// The `TreeSet` the messages of a file end up in: sorted, and of messages
/// that compare equal only the first one kept.
pub fn sort_and_dedupe(mut messages: Vec<Message>) -> Vec<Message> {
    messages.sort_by(Message::compare);
    messages.dedup_by(|b, a| a.compare(b) == Ordering::Equal);
    messages
}

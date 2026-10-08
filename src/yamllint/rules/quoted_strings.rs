//! yamllint's `quoted-strings` rule, with `PyYAML`'s implicit tag resolution
//! (a plain `42` is an int, not a string) that decides which scalars count.

use std::collections::HashMap;
use std::sync::OnceLock;

use libyaml_safer::{ScalarStyle, Scanner, TokenData};
use regex::Regex;

use super::super::Problem;
use super::super::config::RuleConf;
use super::super::parser::{Kind, Style, Token};
use super::{Alt, Check, Ctx, Default, OptKind, OptSpec, RuleSpec, States};

/// `PyYAML`'s `Resolver` implicit resolvers, in registration order, plus the
/// int resolver yamllint adds for `0o` octals. Each is a tag, the first
/// characters it is tried for, and its pattern.
const RESOLVERS: &[(&str, &str, &str)] = &[
    (
        "bool",
        "yYnNtTfFoO",
        r"^(?:yes|Yes|YES|no|No|NO|true|True|TRUE|false|False|FALSE|on|On|ON|off|Off|OFF)$",
    ),
    (
        "float",
        "-+0123456789.",
        r"^(?:[-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+][0-9]+)?|\.[0-9][0-9_]*(?:[eE][-+][0-9]+)?|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$",
    ),
    (
        "int",
        "-+0123456789",
        r"^(?:[-+]?0b[0-1_]+|[-+]?0[0-7_]+|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+)$",
    ),
    ("merge", "<", r"^(?:<<)$"),
    ("null", "~nN", r"^(?:~|null|Null|NULL|)$"),
    (
        "timestamp",
        "0123456789",
        r"^(?:[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]|[0-9][0-9][0-9][0-9]-[0-9][0-9]?-[0-9][0-9]?(?:[Tt]|[ \t]+)[0-9][0-9]?:[0-9][0-9]:[0-9][0-9](?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?)$",
    ),
    ("value", "=", r"^(?:=)$"),
    ("yaml", "!&*", r"^(?:!|&|\*)$"),
    // yamllint's own addition (https://stackoverflow.com/a/36514274).
    (
        "int",
        "-+0123456789",
        r"^(?:[-+]?0b[0-1_]+|[-+]?0o?[0-7_]+|[-+]?0[0-7_]+|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+)$",
    ),
];

fn resolver_regexes() -> &'static [Regex] {
    static REGEXES: OnceLock<Vec<Regex>> = OnceLock::new();
    REGEXES.get_or_init(|| {
        RESOLVERS
            .iter()
            .map(|(_, _, pattern)| Regex::new(pattern).expect("`PyYAML` resolver pattern compiles"))
            .collect()
    })
}

/// The tag `PyYAML` resolves a plain scalar to: `str` unless an implicit
/// resolver claims it. The empty string resolves to `null`.
pub fn resolve_plain_tag(value: &str) -> &'static str {
    let first = value.chars().next();
    for (i, (tag, first_chars, _)) in RESOLVERS.iter().enumerate() {
        let tried = match first {
            Some(c) => first_chars.contains(c),
            None => *tag == "null",
        };
        if tried && resolver_regexes()[i].is_match(value) {
            return tag;
        }
    }
    "str"
}

#[derive(Default)]
pub struct State {
    flow_nest_count: i64,
    /// The first quote style seen, for `quote-type: consistent`.
    consistent_style: Option<Style>,
    regexes: HashMap<String, Regex>,
}

impl State {
    fn regex(&mut self, pattern: &str) -> &Regex {
        self.regexes
            .entry(pattern.to_owned())
            .or_insert_with(|| Regex::new(pattern).expect("validated at config time"))
    }

    fn any_matches(&mut self, patterns: &[&str], value: &str) -> bool {
        patterns.iter().any(|p| self.regex(p).is_match(value))
    }

    /// Mirrors `_quote_match`, for a styled (quoted) scalar.
    fn quote_match(&mut self, quote_type: &str, style: Style) -> bool {
        if quote_type == "consistent" {
            let canonical = *self.consistent_style.get_or_insert(style);
            return canonical == style;
        }
        quote_type == "any"
            || (quote_type == "single" && style == Style::SingleQuoted)
            || (quote_type == "double" && style == Style::DoubleQuoted)
    }
}

/// `PyYAML`'s `Reader.check_printable`: every character is one YAML allows.
fn is_printable(text: &str) -> bool {
    text.chars().all(|c| {
        matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{7E}' | '\u{85}' | '\u{A0}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
    })
}

/// Mirrors `_has_backslash_on_at_least_one_line_ending`.
fn has_backslash_on_a_line_ending(token: &Token, buffer: &str) -> bool {
    if token.start.line == token.end.line {
        return false;
    }
    let from = token.start.index + 1;
    let to = token.end.index.saturating_sub(1);
    if from >= to || !buffer.is_char_boundary(from) || !buffer.is_char_boundary(to) {
        return false;
    }
    let inner = &buffer[from..to];
    inner.contains("\\\n") || inner.contains("\\\r\n")
}

/// Mirrors `_quotes_are_needed`: would the value survive unquoted?
fn quotes_are_needed(token: &Token, value: &str, buffer: &str, inside_flow: bool) -> bool {
    // Quotes are needed on strings containing flow tokens.
    if inside_flow
        && value
            .chars()
            .any(|c| matches!(c, ',' | '[' | ']' | '{' | '}'))
    {
        return true;
    }
    if token.scalar_style() == Some(Style::DoubleQuoted) {
        // Special characters in a double-quoted string are assumed to have
        // been backslash-escaped.
        if !is_printable(&format!("key: {value}")) {
            return true;
        }
        if has_backslash_on_a_line_ending(token, buffer) {
            return true;
        }
    }

    // Scan `key: <value>` and skip the five tokens of `key: ` (StreamStart,
    // BlockMappingStart, Key, Scalar(key), Value): the value survives if it
    // comes back as one plain scalar followed by the mapping's end.
    let text = format!("key: {value}");
    let mut scanner: Scanner<&mut &[u8]> = Scanner::new();
    let mut input = text.as_bytes();
    scanner.set_input_string(&mut input);
    let mut tokens = Vec::with_capacity(7);
    while tokens.len() < 7 {
        match Scanner::scan(&mut scanner) {
            Ok(token) => {
                let end = matches!(token.data, TokenData::StreamEnd);
                tokens.push(token.data);
                if end {
                    break;
                }
            }
            Err(_) => return true,
        }
    }
    match (tokens.get(5), tokens.get(6)) {
        (
            Some(TokenData::Scalar {
                value: unquoted,
                style: ScalarStyle::Plain,
            }),
            Some(TokenData::BlockEnd),
        ) => unquoted != value,
        _ => true,
    }
}

/// Mirrors `_has_quoted_quotes`.
fn has_quoted_quotes(style: Style, value: &str) -> bool {
    (style == Style::SingleQuoted && value.contains('"'))
        || (style == Style::DoubleQuoted && value.contains('\''))
}

fn check_quoted_strings(
    conf: &RuleConf,
    ctx: &Ctx<'_, '_>,
    states: &mut States,
    out: &mut Vec<Problem>,
) {
    let st = &mut states.quoted_strings;
    let token = ctx.token();
    match token.kind {
        Kind::FlowMappingStart | Kind::FlowSequenceStart => st.flow_nest_count += 1,
        Kind::FlowMappingEnd | Kind::FlowSequenceEnd => st.flow_nest_count -= 1,
        _ => {}
    }

    let (Some(value), Some(style)) = (token.scalar_value(), token.scalar_style()) else {
        return;
    };
    let Some(prev) = ctx.prev() else {
        return;
    };
    if !matches!(
        prev.kind,
        Kind::BlockEntry
            | Kind::FlowEntry
            | Kind::FlowSequenceStart
            | Kind::Tag { .. }
            | Kind::Value
            | Kind::Key
    ) {
        return;
    }

    let node = if prev.kind == Kind::Key {
        "key"
    } else {
        "value"
    };
    if node == "key" && !conf.bool_opt("check-keys") {
        return;
    }

    // Ignore explicit types, e.g. !!str testtest or !!int 42
    if let Kind::Tag { handle, .. } = &prev.kind
        && handle == "!!"
    {
        return;
    }

    // Ignore numbers, booleans, etc.
    let tag = resolve_plain_tag(value);
    if style == Style::Plain && tag != "str" {
        return;
    }

    // Ignore multi-line strings
    if style.is_block() {
        return;
    }

    let quote_type = conf.str_opt("quote-type");
    let required = conf.opt("required").clone();
    let allow_quoted_quotes = conf.bool_opt("allow-quoted-quotes");
    let extra_required = conf.str_list_opt("extra-required");
    let extra_allowed = conf.str_list_opt("extra-allowed");
    let quoted = style != Style::Plain;

    let not_quoted_with = format!("string {node} is not quoted with {quote_type} quotes");
    let mut msg: Option<String> = None;

    if required.as_bool() == Some(true) {
        // Quotes are mandatory and need to match config
        let ok = quoted
            && (st.quote_match(quote_type, style)
                || (allow_quoted_quotes && has_quoted_quotes(style, value)));
        if !ok {
            msg = Some(not_quoted_with);
        }
    } else if required.as_bool() == Some(false) {
        // Quotes are not mandatory but when used need to match config
        if quoted {
            let matches_config = st.quote_match(quote_type, style)
                || (allow_quoted_quotes && has_quoted_quotes(style, value));
            if !matches_config {
                msg = Some(not_quoted_with);
            }
        } else if st.any_matches(&extra_required, value) {
            msg = Some(format!("string {node} is not quoted"));
        }
    } else {
        // only-when-needed: quotes are not strictly needed here
        let inside_flow = st.flow_nest_count > 0;
        if quoted
            && tag == "str"
            && !value.is_empty()
            && !quotes_are_needed(token, value, ctx.buffer(), inside_flow)
        {
            let is_extra_required = st.any_matches(&extra_required, value);
            let is_extra_allowed = st.any_matches(&extra_allowed, value);
            if !(is_extra_required || is_extra_allowed) {
                msg = Some(format!(
                    "string {node} is redundantly quoted with {quote_type} quotes"
                ));
            }
        } else if quoted {
            // But when used need to match config
            let matches_config = st.quote_match(quote_type, style)
                || (allow_quoted_quotes && has_quoted_quotes(style, value));
            if !matches_config {
                msg = Some(not_quoted_with);
            }
        } else if !extra_required.is_empty() && st.any_matches(&extra_required, value) {
            msg = Some(format!("string {node} is not quoted"));
        }
    }

    if let Some(msg) = msg {
        out.push(Problem::new(
            token.start.line + 1,
            token.start.column + 1,
            msg,
        ));
    }
}

fn validate_quoted_strings(conf: &RuleConf) -> Option<String> {
    let required = conf.opt("required").as_bool();
    let extra_allowed = !conf.str_list_opt("extra-allowed").is_empty();
    let extra_required = !conf.str_list_opt("extra-required").is_empty();
    if required == Some(true) && extra_allowed {
        return Some("cannot use both \"required: true\" and \"extra-allowed\"".to_owned());
    }
    if required == Some(true) && extra_required {
        return Some("cannot use both \"required: true\" and \"extra-required\"".to_owned());
    }
    if required == Some(false) && extra_allowed {
        return Some("cannot use both \"required: false\" and \"extra-allowed\"".to_owned());
    }
    super::documents::validate_regex_list(conf, "extra-required")
        .or_else(|| super::documents::validate_regex_list(conf, "extra-allowed"))
}

const QUOTE_TYPES: &[Alt] = &[
    Alt::Str("any"),
    Alt::Str("single"),
    Alt::Str("double"),
    Alt::Str("consistent"),
];
const REQUIRED_ALTS: &[Alt] = &[
    Alt::Bool(true),
    Alt::Bool(false),
    Alt::Str("only-when-needed"),
];
const STR_LIST: &[Alt] = &[Alt::AnyStr];

pub const QUOTED_STRINGS: RuleSpec = RuleSpec {
    id: "quoted-strings",
    options: &[
        OptSpec {
            name: "quote-type",
            kind: OptKind::OneOf(QUOTE_TYPES),
            default: Default::Str("any"),
        },
        OptSpec {
            name: "required",
            kind: OptKind::OneOf(REQUIRED_ALTS),
            default: Default::Bool(true),
        },
        OptSpec {
            name: "extra-required",
            kind: OptKind::ListOf(STR_LIST),
            default: Default::EmptyList,
        },
        OptSpec {
            name: "extra-allowed",
            kind: OptKind::ListOf(STR_LIST),
            default: Default::EmptyList,
        },
        OptSpec {
            name: "allow-quoted-quotes",
            kind: OptKind::Bool,
            default: Default::Bool(false),
        },
        OptSpec {
            name: "check-keys",
            kind: OptKind::Bool,
            default: Default::Bool(false),
        },
    ],
    validate: Some(validate_quoted_strings),
    check: Check::Token(check_quoted_strings),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_scalars_resolve_like_pyyaml() {
        assert_eq!(resolve_plain_tag("42"), "int");
        assert_eq!(resolve_plain_tag("0o10"), "int");
        assert_eq!(resolve_plain_tag("3.14"), "float");
        assert_eq!(resolve_plain_tag(".inf"), "float");
        assert_eq!(resolve_plain_tag("yes"), "bool");
        assert_eq!(resolve_plain_tag("Off"), "bool");
        assert_eq!(resolve_plain_tag("null"), "null");
        assert_eq!(resolve_plain_tag(""), "null");
        assert_eq!(resolve_plain_tag("2001-12-14"), "timestamp");
        assert_eq!(resolve_plain_tag("<<"), "merge");
        assert_eq!(resolve_plain_tag("hello"), "str");
        assert_eq!(resolve_plain_tag("yesterday"), "str");
        assert_eq!(resolve_plain_tag("1.2.3"), "str");
    }
}

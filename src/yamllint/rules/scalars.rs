//! Scalar value rules: yamllint's `float-values`, `octal-values` and `truthy`.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use regex::Regex;

use super::super::Problem;
use super::super::config::RuleConf;
use super::super::parser::Kind;
use super::{Alt, Check, Ctx, Default, OptKind, OptSpec, RuleSpec, States};

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("yamllint scalar pattern compiles"))
}

// ─── float-values ────────────────────────────────────────────────────────────

fn check_float_values(conf: &RuleConf, ctx: &Ctx<'_, '_>, _: &mut States, out: &mut Vec<Problem>) {
    if ctx.prev().is_some_and(super::super::parser::Token::is_tag) {
        return;
    }
    let token = ctx.token();
    let Some(value) = token.scalar_value() else {
        return;
    };
    if token.is_styled_scalar() {
        return;
    }
    static NAN: OnceLock<Regex> = OnceLock::new();
    static INF: OnceLock<Regex> = OnceLock::new();
    static SCIENTIFIC: OnceLock<Regex> = OnceLock::new();
    static NO_NUMERAL: OnceLock<Regex> = OnceLock::new();
    let at = |desc: String| Problem::new(token.start.line + 1, token.start.column + 1, desc);

    if conf.bool_opt("forbid-nan") && regex(&NAN, r"^(\.nan|\.NaN|\.NAN)$").is_match(value) {
        out.push(at(format!("forbidden not a number value \"{value}\"")));
    }
    if conf.bool_opt("forbid-inf") && regex(&INF, r"^[-+]?(\.inf|\.Inf|\.INF)$").is_match(value) {
        out.push(at(format!("forbidden infinite value \"{value}\"")));
    }
    if conf.bool_opt("forbid-scientific-notation")
        && regex(
            &SCIENTIFIC,
            r"^[-+]?(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)$",
        )
        .is_match(value)
    {
        out.push(at(format!("forbidden scientific notation \"{value}\"")));
    }
    if conf.bool_opt("require-numeral-before-decimal")
        && regex(&NO_NUMERAL, r"^[-+]?(\.[0-9]+)([eE][-+]?[0-9]+)?$").is_match(value)
    {
        out.push(at(format!(
            "forbidden decimal missing 0 prefix \"{value}\""
        )));
    }
}

pub const FLOAT_VALUES: RuleSpec = RuleSpec {
    id: "float-values",
    options: &[
        OptSpec {
            name: "require-numeral-before-decimal",
            kind: OptKind::Bool,
            default: Default::Bool(false),
        },
        OptSpec {
            name: "forbid-scientific-notation",
            kind: OptKind::Bool,
            default: Default::Bool(false),
        },
        OptSpec {
            name: "forbid-nan",
            kind: OptKind::Bool,
            default: Default::Bool(false),
        },
        OptSpec {
            name: "forbid-inf",
            kind: OptKind::Bool,
            default: Default::Bool(false),
        },
    ],
    validate: None,
    check: Check::Token(check_float_values),
};

// ─── octal-values ────────────────────────────────────────────────────────────

fn is_octal_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| (b'0'..=b'7').contains(&b))
}

fn check_octal_values(conf: &RuleConf, ctx: &Ctx<'_, '_>, _: &mut States, out: &mut Vec<Problem>) {
    if ctx.prev().is_some_and(super::super::parser::Token::is_tag) {
        return;
    }
    let token = ctx.token();
    let Some(value) = token.scalar_value() else {
        return;
    };
    if token.is_styled_scalar() {
        return;
    }
    let at = |desc: String| Problem::new(token.start.line + 1, token.end.column + 1, desc);
    if conf.bool_opt("forbid-implicit-octal")
        && value.len() > 1
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.starts_with('0')
        && is_octal_digits(&value[1..])
    {
        out.push(at(format!("forbidden implicit octal value \"{value}\"")));
    }
    if conf.bool_opt("forbid-explicit-octal")
        && value.len() > 2
        && value.starts_with("0o")
        && is_octal_digits(&value[2..])
    {
        out.push(at(format!("forbidden explicit octal value \"{value}\"")));
    }
}

pub const OCTAL_VALUES: RuleSpec = RuleSpec {
    id: "octal-values",
    options: &[
        OptSpec {
            name: "forbid-implicit-octal",
            kind: OptKind::Bool,
            default: Default::Bool(true),
        },
        OptSpec {
            name: "forbid-explicit-octal",
            kind: OptKind::Bool,
            default: Default::Bool(true),
        },
    ],
    validate: None,
    check: Check::Token(check_octal_values),
};

// ─── truthy ──────────────────────────────────────────────────────────────────

pub const TRUTHY_1_1: &[&str] = &[
    "YES", "Yes", "yes", "NO", "No", "no", "TRUE", "True", "true", "FALSE", "False", "false", "ON",
    "On", "on", "OFF", "Off", "off",
];

pub const TRUTHY_1_2: &[&str] = &["TRUE", "True", "true", "FALSE", "False", "false"];

const TRUTHY_ALTS: &[Alt] = &[
    Alt::Str("YES"),
    Alt::Str("Yes"),
    Alt::Str("yes"),
    Alt::Str("NO"),
    Alt::Str("No"),
    Alt::Str("no"),
    Alt::Str("TRUE"),
    Alt::Str("True"),
    Alt::Str("true"),
    Alt::Str("FALSE"),
    Alt::Str("False"),
    Alt::Str("false"),
    Alt::Str("ON"),
    Alt::Str("On"),
    Alt::Str("on"),
    Alt::Str("OFF"),
    Alt::Str("Off"),
    Alt::Str("off"),
];

#[derive(Default)]
pub struct TruthyState {
    yaml_spec_version: Option<(i32, i32)>,
    bad_truthy_values: Option<BTreeSet<String>>,
}

fn check_truthy(conf: &RuleConf, ctx: &Ctx<'_, '_>, states: &mut States, out: &mut Vec<Problem>) {
    let state = &mut states.truthy;
    let token = ctx.token();
    match &token.kind {
        Kind::Directive {
            name,
            yaml_version: Some(version),
        } if name == "YAML" => state.yaml_spec_version = Some(*version),
        Kind::DocumentEnd => {
            state.yaml_spec_version = None;
            state.bad_truthy_values = None;
        }
        _ => {}
    }

    let prev = ctx.prev();
    if prev.is_some_and(super::super::parser::Token::is_tag) {
        return;
    }
    if !conf.bool_opt("check-keys")
        && prev.is_some_and(|p| p.kind == Kind::Key)
        && token.is_scalar()
    {
        return;
    }
    if !token.is_plain_scalar() {
        return;
    }
    let Some(value) = token.scalar_value() else {
        return;
    };
    let allowed = conf.str_list_opt("allowed-values");
    let bad = state.bad_truthy_values.get_or_insert_with(|| {
        let base = if state.yaml_spec_version == Some((1, 2)) {
            TRUTHY_1_2
        } else {
            TRUTHY_1_1
        };
        base.iter()
            .filter(|v| !allowed.contains(v))
            .map(|v| (*v).to_owned())
            .collect()
    });
    if bad.contains(value) {
        let mut sorted = allowed.clone();
        sorted.sort_unstable();
        out.push(Problem::new(
            token.start.line + 1,
            token.start.column + 1,
            format!("truthy value should be one of [{}]", sorted.join(", ")),
        ));
    }
}

pub const TRUTHY: RuleSpec = RuleSpec {
    id: "truthy",
    options: &[
        OptSpec {
            name: "allowed-values",
            kind: OptKind::ListOf(TRUTHY_ALTS),
            default: Default::StrList(&["true", "false"]),
        },
        OptSpec {
            name: "check-keys",
            kind: OptKind::Bool,
            default: Default::Bool(true),
        },
    ],
    validate: None,
    check: Check::Token(check_truthy),
};

//! Spacing around punctuation: yamllint's `braces`, `brackets`, `colons`,
//! `commas` and `hyphens` rules.

use super::super::Problem;
use super::super::config::RuleConf;
use super::super::parser::Kind;
use super::common::{spaces_after, spaces_before};
use super::{Alt, Check, Ctx, Default, OptKind, OptSpec, RuleSpec, States};

const FORBID_ALTS: &[Alt] = &[Alt::AnyBool, Alt::Str("non-empty")];

const FLOW_OPTIONS: &[OptSpec] = &[
    OptSpec {
        name: "forbid",
        kind: OptKind::OneOf(FORBID_ALTS),
        default: Default::Bool(false),
    },
    OptSpec {
        name: "min-spaces-inside",
        kind: OptKind::Int,
        default: Default::Int(0),
    },
    OptSpec {
        name: "max-spaces-inside",
        kind: OptKind::Int,
        default: Default::Int(0),
    },
    OptSpec {
        name: "min-spaces-inside-empty",
        kind: OptKind::Int,
        default: Default::Int(-1),
    },
    OptSpec {
        name: "max-spaces-inside-empty",
        kind: OptKind::Int,
        default: Default::Int(-1),
    },
];

/// The words of the `braces`/`brackets` messages for one bracket pair.
struct FlowWords {
    start: Kind,
    end: Kind,
    forbidden: &'static str,
    empty_few: &'static str,
    empty_many: &'static str,
    few: &'static str,
    many: &'static str,
}

const BRACES_WORDS: FlowWords = FlowWords {
    start: Kind::FlowMappingStart,
    end: Kind::FlowMappingEnd,
    forbidden: "forbidden flow mapping",
    empty_few: "too few spaces inside empty braces",
    empty_many: "too many spaces inside empty braces",
    few: "too few spaces inside braces",
    many: "too many spaces inside braces",
};

const BRACKETS_WORDS: FlowWords = FlowWords {
    start: Kind::FlowSequenceStart,
    end: Kind::FlowSequenceEnd,
    forbidden: "forbidden flow sequence",
    empty_few: "too few spaces inside empty brackets",
    empty_many: "too many spaces inside empty brackets",
    few: "too few spaces inside brackets",
    many: "too many spaces inside brackets",
};

/// The shared body of `braces.py` and `brackets.py`.
fn check_flow(conf: &RuleConf, ctx: &Ctx<'_, '_>, words: &FlowWords, out: &mut Vec<Problem>) {
    let token = ctx.token();
    let prev = ctx.prev();
    let next = ctx.next();
    let forbid = conf.opt("forbid");
    let forbid_all = forbid.as_bool() == Some(true);
    let forbid_non_empty = forbid.as_str() == Some("non-empty");
    let is_start = token.kind == words.start;
    let next_is_end = next.is_some_and(|n| n.kind == words.end);

    if is_start && (forbid_all || (forbid_non_empty && !next_is_end)) {
        out.push(Problem::new(
            token.start.line + 1,
            token.end.column + 1,
            words.forbidden,
        ));
    } else if is_start && next_is_end {
        let min_empty = conf.int_opt("min-spaces-inside-empty");
        let max_empty = conf.int_opt("max-spaces-inside-empty");
        let min = if min_empty == -1 {
            conf.int_opt("min-spaces-inside")
        } else {
            min_empty
        };
        let max = if max_empty == -1 {
            conf.int_opt("max-spaces-inside")
        } else {
            max_empty
        };
        out.extend(spaces_after(
            token,
            next,
            min,
            max,
            words.empty_few,
            words.empty_many,
        ));
    } else if is_start {
        out.extend(spaces_after(
            token,
            next,
            conf.int_opt("min-spaces-inside"),
            conf.int_opt("max-spaces-inside"),
            words.few,
            words.many,
        ));
    } else if token.kind == words.end && !prev.is_some_and(|p| p.kind == words.start) {
        out.extend(spaces_before(
            ctx.stream,
            token,
            prev,
            conf.int_opt("min-spaces-inside"),
            conf.int_opt("max-spaces-inside"),
            words.few,
            words.many,
        ));
    }
}

fn check_braces(conf: &RuleConf, ctx: &Ctx<'_, '_>, _: &mut States, out: &mut Vec<Problem>) {
    check_flow(conf, ctx, &BRACES_WORDS, out);
}

fn check_brackets(conf: &RuleConf, ctx: &Ctx<'_, '_>, _: &mut States, out: &mut Vec<Problem>) {
    check_flow(conf, ctx, &BRACKETS_WORDS, out);
}

pub const BRACES: RuleSpec = RuleSpec {
    id: "braces",
    options: FLOW_OPTIONS,
    validate: None,
    check: Check::Token(check_braces),
};

pub const BRACKETS: RuleSpec = RuleSpec {
    id: "brackets",
    options: FLOW_OPTIONS,
    validate: None,
    check: Check::Token(check_brackets),
};

fn check_colons(conf: &RuleConf, ctx: &Ctx<'_, '_>, _: &mut States, out: &mut Vec<Problem>) {
    let token = ctx.token();
    let prev = ctx.prev();
    let next = ctx.next();
    if token.kind == Kind::Value {
        // `*alias :` — the colon right after an alias is not a spacing matter.
        let after_alias = prev.is_some_and(|p| {
            matches!(p.kind, Kind::Alias(_)) && token.start.index - p.end.index == 1
        });
        if !after_alias {
            out.extend(spaces_before(
                ctx.stream,
                token,
                prev,
                -1,
                conf.int_opt("max-spaces-before"),
                "",
                "too many spaces before colon",
            ));
            out.extend(spaces_after(
                token,
                next,
                -1,
                conf.int_opt("max-spaces-after"),
                "",
                "too many spaces after colon",
            ));
        }
    }
    if token.kind == Kind::Key && token.is_explicit_key(ctx.buffer()) {
        out.extend(spaces_after(
            token,
            next,
            -1,
            conf.int_opt("max-spaces-after"),
            "",
            "too many spaces after question mark",
        ));
    }
}

pub const COLONS: RuleSpec = RuleSpec {
    id: "colons",
    options: &[
        OptSpec {
            name: "max-spaces-before",
            kind: OptKind::Int,
            default: Default::Int(0),
        },
        OptSpec {
            name: "max-spaces-after",
            kind: OptKind::Int,
            default: Default::Int(1),
        },
    ],
    validate: None,
    check: Check::Token(check_colons),
};

fn check_commas(conf: &RuleConf, ctx: &Ctx<'_, '_>, _: &mut States, out: &mut Vec<Problem>) {
    let token = ctx.token();
    if token.kind != Kind::FlowEntry {
        return;
    }
    let prev = ctx.prev();
    let next = ctx.next();
    let max_before = conf.int_opt("max-spaces-before");
    if let Some(p) = prev
        && max_before != -1
        && p.end.line < token.start.line
    {
        out.push(Problem::new(
            token.start.line + 1,
            token.start.column.max(1),
            "too many spaces before comma",
        ));
    } else {
        out.extend(spaces_before(
            ctx.stream,
            token,
            prev,
            -1,
            max_before,
            "",
            "too many spaces before comma",
        ));
    }
    out.extend(spaces_after(
        token,
        next,
        conf.int_opt("min-spaces-after"),
        conf.int_opt("max-spaces-after"),
        "too few spaces after comma",
        "too many spaces after comma",
    ));
}

pub const COMMAS: RuleSpec = RuleSpec {
    id: "commas",
    options: &[
        OptSpec {
            name: "max-spaces-before",
            kind: OptKind::Int,
            default: Default::Int(0),
        },
        OptSpec {
            name: "min-spaces-after",
            kind: OptKind::Int,
            default: Default::Int(1),
        },
        OptSpec {
            name: "max-spaces-after",
            kind: OptKind::Int,
            default: Default::Int(1),
        },
    ],
    validate: None,
    check: Check::Token(check_commas),
};

fn check_hyphens(conf: &RuleConf, ctx: &Ctx<'_, '_>, _: &mut States, out: &mut Vec<Problem>) {
    let token = ctx.token();
    if token.kind == Kind::BlockEntry {
        out.extend(spaces_after(
            token,
            ctx.next(),
            -1,
            conf.int_opt("max-spaces-after"),
            "",
            "too many spaces after hyphen",
        ));
    }
}

pub const HYPHENS: RuleSpec = RuleSpec {
    id: "hyphens",
    options: &[OptSpec {
        name: "max-spaces-after",
        kind: OptKind::Int,
        default: Default::Int(1),
    }],
    validate: None,
    check: Check::Token(check_hyphens),
};

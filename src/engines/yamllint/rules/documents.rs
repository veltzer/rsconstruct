//! Document structure rules: yamllint's `anchors`, `document-start`,
//! `document-end`, `empty-values`, `key-duplicates` and `key-ordering`.

use std::collections::BTreeMap;

use super::super::Problem;
use super::super::config::RuleConf;
use super::super::parser::{Kind, Token};
use super::{Alt, Check, Ctx, Default, OptKind, OptSpec, RuleSpec, States};

// ─── anchors ─────────────────────────────────────────────────────────────────

struct AnchorInfo {
    line: usize,
    column: usize,
    used: bool,
}

#[derive(Default)]
pub struct AnchorsState {
    anchors: BTreeMap<String, AnchorInfo>,
}

fn check_anchors(conf: &RuleConf, ctx: &Ctx<'_, '_>, states: &mut States, out: &mut Vec<Problem>) {
    let forbid_undeclared = conf.bool_opt("forbid-undeclared-aliases");
    let forbid_duplicated = conf.bool_opt("forbid-duplicated-anchors");
    let forbid_unused = conf.bool_opt("forbid-unused-anchors");
    let any = forbid_undeclared || forbid_duplicated || forbid_unused;
    let token = ctx.token();
    let state = &mut states.anchors;

    if any
        && matches!(
            token.kind,
            Kind::StreamStart | Kind::DocumentStart | Kind::DocumentEnd
        )
    {
        state.anchors.clear();
    }

    if forbid_undeclared
        && let Kind::Alias(name) = &token.kind
        && !state.anchors.contains_key(name)
    {
        out.push(Problem::new(
            token.start.line + 1,
            token.start.column + 1,
            format!("found undeclared alias \"{name}\""),
        ));
    }

    if forbid_duplicated
        && let Kind::Anchor(name) = &token.kind
        && state.anchors.contains_key(name)
    {
        out.push(Problem::new(
            token.start.line + 1,
            token.start.column + 1,
            format!("found duplicated anchor \"{name}\""),
        ));
    }

    if forbid_unused {
        // Unused anchors can only be detected at the end of a document.
        let next_ends_document = ctx.next().is_some_and(|n| {
            matches!(
                n.kind,
                Kind::StreamEnd | Kind::DocumentStart | Kind::DocumentEnd
            )
        });
        if next_ends_document {
            for (anchor, info) in &state.anchors {
                if !info.used {
                    out.push(Problem::new(
                        info.line + 1,
                        info.column + 1,
                        format!("found unused anchor \"{anchor}\""),
                    ));
                }
            }
        } else if let Kind::Alias(name) = &token.kind
            && let Some(info) = state.anchors.get_mut(name)
        {
            info.used = true;
        }
    }

    if any && let Kind::Anchor(name) = &token.kind {
        state.anchors.insert(
            name.clone(),
            AnchorInfo {
                line: token.start.line,
                column: token.start.column,
                used: false,
            },
        );
    }
}

pub const ANCHORS: RuleSpec = RuleSpec {
    id: "anchors",
    options: &[
        OptSpec {
            name: "forbid-undeclared-aliases",
            kind: OptKind::Bool,
            default: Default::Bool(true),
        },
        OptSpec {
            name: "forbid-duplicated-anchors",
            kind: OptKind::Bool,
            default: Default::Bool(false),
        },
        OptSpec {
            name: "forbid-unused-anchors",
            kind: OptKind::Bool,
            default: Default::Bool(false),
        },
    ],
    validate: None,
    check: Check::Token(check_anchors),
};

// ─── document-end ────────────────────────────────────────────────────────────

fn check_document_end(conf: &RuleConf, ctx: &Ctx<'_, '_>, _: &mut States, out: &mut Vec<Problem>) {
    let token = ctx.token();
    let prev = ctx.prev();
    if conf.bool_opt("present") {
        let is_stream_end = token.kind == Kind::StreamEnd;
        let is_start = token.kind == Kind::DocumentStart;
        let prev_is_end_or_stream_start =
            prev.is_some_and(|p| matches!(p.kind, Kind::DocumentEnd | Kind::StreamStart));
        let prev_is_directive = prev.is_some_and(Token::is_directive);

        if is_stream_end && !prev_is_end_or_stream_start {
            out.push(Problem::new(
                token.start.line,
                1,
                "missing document end \"...\"",
            ));
        } else if is_start && !(prev_is_end_or_stream_start || prev_is_directive) {
            out.push(Problem::new(
                token.start.line + 1,
                1,
                "missing document end \"...\"",
            ));
        }
    } else if token.kind == Kind::DocumentEnd {
        out.push(Problem::new(
            token.start.line + 1,
            token.start.column + 1,
            "found forbidden document end \"...\"",
        ));
    }
}

pub const DOCUMENT_END: RuleSpec = RuleSpec {
    id: "document-end",
    options: &[OptSpec {
        name: "present",
        kind: OptKind::Bool,
        default: Default::Bool(true),
    }],
    validate: None,
    check: Check::Token(check_document_end),
};

// ─── document-start ──────────────────────────────────────────────────────────

fn check_document_start(
    conf: &RuleConf,
    ctx: &Ctx<'_, '_>,
    _: &mut States,
    out: &mut Vec<Problem>,
) {
    let token = ctx.token();
    let prev = ctx.prev();
    if conf.bool_opt("present") {
        let prev_opens = prev.is_some_and(|p| {
            matches!(p.kind, Kind::StreamStart | Kind::DocumentEnd) || p.is_directive()
        });
        let token_is_start =
            matches!(token.kind, Kind::DocumentStart | Kind::StreamEnd) || token.is_directive();
        if prev_opens && !token_is_start {
            out.push(Problem::new(
                token.start.line + 1,
                1,
                "missing document start \"---\"",
            ));
        }
    } else if token.kind == Kind::DocumentStart {
        out.push(Problem::new(
            token.start.line + 1,
            token.start.column + 1,
            "found forbidden document start \"---\"",
        ));
    }
}

pub const DOCUMENT_START: RuleSpec = RuleSpec {
    id: "document-start",
    options: &[OptSpec {
        name: "present",
        kind: OptKind::Bool,
        default: Default::Bool(true),
    }],
    validate: None,
    check: Check::Token(check_document_start),
};

// ─── empty-values ────────────────────────────────────────────────────────────

fn check_empty_values(conf: &RuleConf, ctx: &Ctx<'_, '_>, _: &mut States, out: &mut Vec<Problem>) {
    let token = ctx.token();
    let Some(next) = ctx.next() else {
        return;
    };
    if conf.bool_opt("forbid-in-block-mappings")
        && token.kind == Kind::Value
        && matches!(next.kind, Kind::Key | Kind::BlockEnd)
    {
        out.push(Problem::new(
            token.start.line + 1,
            token.end.column + 1,
            "empty value in block mapping",
        ));
    }
    if conf.bool_opt("forbid-in-flow-mappings")
        && token.kind == Kind::Value
        && matches!(next.kind, Kind::FlowEntry | Kind::FlowMappingEnd)
    {
        out.push(Problem::new(
            token.start.line + 1,
            token.end.column + 1,
            "empty value in flow mapping",
        ));
    }
    if conf.bool_opt("forbid-in-block-sequences")
        && token.kind == Kind::BlockEntry
        && matches!(next.kind, Kind::Key | Kind::BlockEnd | Kind::BlockEntry)
    {
        out.push(Problem::new(
            token.start.line + 1,
            token.end.column + 1,
            "empty value in block sequence",
        ));
    }
}

pub const EMPTY_VALUES: RuleSpec = RuleSpec {
    id: "empty-values",
    options: &[
        OptSpec {
            name: "forbid-in-block-mappings",
            kind: OptKind::Bool,
            default: Default::Bool(true),
        },
        OptSpec {
            name: "forbid-in-flow-mappings",
            kind: OptKind::Bool,
            default: Default::Bool(true),
        },
        OptSpec {
            name: "forbid-in-block-sequences",
            kind: OptKind::Bool,
            default: Default::Bool(true),
        },
    ],
    validate: None,
    check: Check::Token(check_empty_values),
};

// ─── key-duplicates / key-ordering ───────────────────────────────────────────

/// A mapping or sequence being walked, with the keys seen so far.
struct KeysParent {
    is_map: bool,
    keys: Vec<String>,
}

/// The collection stack both key rules keep.
#[derive(Default)]
pub struct KeysState {
    stack: Vec<KeysParent>,
}

impl KeysState {
    /// The stack bookkeeping shared by `key-duplicates` and `key-ordering`.
    /// Returns the key scalar's value when the token is a key of the
    /// innermost mapping, which is what either rule then judges.
    fn track<'s>(&mut self, ctx: &Ctx<'s, '_>) -> Option<&'s str> {
        let token = ctx.token();
        match token.kind {
            Kind::BlockMappingStart | Kind::FlowMappingStart => self.stack.push(KeysParent {
                is_map: true,
                keys: Vec::new(),
            }),
            Kind::BlockSequenceStart | Kind::FlowSequenceStart => self.stack.push(KeysParent {
                is_map: false,
                keys: Vec::new(),
            }),
            Kind::BlockEnd | Kind::FlowMappingEnd | Kind::FlowSequenceEnd => {
                self.stack.pop();
            }
            Kind::Key => {
                // KeyTokens can be found inside flow sequences: strange, but
                // allowed, hence the check that the parent is a mapping.
                let value = ctx.next().and_then(|n| n.scalar_value())?;
                if self.stack.last().is_some_and(|p| p.is_map) {
                    return Some(value);
                }
            }
            _ => {}
        }
        None
    }
}

fn check_key_duplicates(
    conf: &RuleConf,
    ctx: &Ctx<'_, '_>,
    states: &mut States,
    out: &mut Vec<Problem>,
) {
    let state = &mut states.key_duplicates;
    let Some(value) = state.track(ctx) else {
        return;
    };
    let Some(parent) = state.stack.last_mut() else {
        return;
    };
    // `<<` is the merge key (http://yaml.org/type/merge.html): repeating it
    // is fine unless the config forbids that too.
    if parent.keys.iter().any(|k| k == value)
        && (value != "<<" || conf.bool_opt("forbid-duplicated-merge-keys"))
    {
        let next = ctx.next().expect("a key scalar follows the Key token");
        out.push(Problem::new(
            next.start.line + 1,
            next.start.column + 1,
            format!("duplication of key \"{value}\" in mapping"),
        ));
    } else {
        parent.keys.push(value.to_owned());
    }
}

pub const KEY_DUPLICATES: RuleSpec = RuleSpec {
    id: "key-duplicates",
    options: &[OptSpec {
        name: "forbid-duplicated-merge-keys",
        kind: OptKind::Bool,
        default: Default::Bool(false),
    }],
    validate: None,
    check: Check::Token(check_key_duplicates),
};

fn check_key_ordering(
    conf: &RuleConf,
    ctx: &Ctx<'_, '_>,
    states: &mut States,
    out: &mut Vec<Problem>,
) {
    let state = &mut states.key_ordering;
    let Some(value) = state.track(ctx) else {
        return;
    };
    let ignored = conf
        .str_list_opt("ignored-keys")
        .iter()
        .any(|pattern| regex::Regex::new(pattern).is_ok_and(|re| re.is_match(value)));
    if ignored {
        return;
    }
    let Some(parent) = state.stack.last_mut() else {
        return;
    };
    // yamllint compares with `strcoll`; with no `locale` configured (the
    // only mode this port accepts) that is code-point order.
    if parent.keys.iter().any(|key| value < key.as_str()) {
        let next = ctx.next().expect("a key scalar follows the Key token");
        out.push(Problem::new(
            next.start.line + 1,
            next.start.column + 1,
            format!("wrong ordering of key \"{value}\" in mapping"),
        ));
    } else {
        parent.keys.push(value.to_owned());
    }
}

/// Every pattern must compile: the check skips a pattern that does not, so
/// rejecting it here keeps a typo from silently ignoring nothing.
pub(super) fn validate_regex_list(conf: &RuleConf, name: &str) -> Option<String> {
    conf.str_list_opt(name).iter().find_map(|pattern| {
        regex::Regex::new(pattern)
            .err()
            .map(|e| format!("invalid regex {pattern:?} in {name}: {e}"))
    })
}

fn validate_key_ordering(conf: &RuleConf) -> Option<String> {
    validate_regex_list(conf, "ignored-keys")
}

const STR_LIST: &[Alt] = &[Alt::AnyStr];

pub const KEY_ORDERING: RuleSpec = RuleSpec {
    id: "key-ordering",
    options: &[OptSpec {
        name: "ignored-keys",
        kind: OptKind::ListOf(STR_LIST),
        default: Default::EmptyList,
    }],
    validate: Some(validate_key_ordering),
    check: Check::Token(check_key_ordering),
};

//! The yamllint rules, ported one for one from `yamllint/rules/*.py`.
//!
//! A rule is a `RuleSpec`: its id, its option schema (what `config.rs`
//! validates a `.yamllint.yaml` against), an optional cross-option check,
//! and its `check` function. Token rules see one token at a time with its
//! neighbours and keep state in `States`; comment and line rules see one
//! comment or line.

use super::Problem;
use super::config::RuleConf;
use super::parser::{Stream, Token};

pub mod comments;
pub mod common;
pub mod documents;
pub mod indentation;
pub mod lines;
pub mod quoted_strings;
pub mod scalars;
pub mod spacing;

/// One alternative of an option that accepts several shapes: yamllint's
/// `CONF = {option: (bool, 'non-empty')}` becomes `[AnyBool, Str("non-empty")]`.
#[derive(Debug, Clone, Copy)]
pub enum Alt {
    AnyBool,
    AnyInt,
    AnyStr,
    Bool(bool),
    Str(&'static str),
}

/// An option's accepted shape.
#[derive(Debug, Clone, Copy)]
pub enum OptKind {
    Bool,
    Int,
    /// Exactly one of the alternatives (a Python tuple in yamllint's `CONF`).
    OneOf(&'static [Alt]),
    /// A list whose every item is one of the alternatives (a Python list).
    ListOf(&'static [Alt]),
}

/// An option's default (yamllint's `DEFAULT`).
#[derive(Debug, Clone, Copy)]
pub enum Default {
    Bool(bool),
    Int(i64),
    Str(&'static str),
    EmptyList,
    StrList(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy)]
pub struct OptSpec {
    pub name: &'static str,
    pub kind: OptKind,
    pub default: Default,
}

/// The current token with its neighbours, as yamllint passes them to a
/// token rule: `curr`, `prev`, `next`, `nextnext`.
pub struct Ctx<'s, 'a> {
    pub stream: &'s Stream<'a>,
    pub idx: usize,
}

impl<'s, 'a> Ctx<'s, 'a> {
    pub fn token(&self) -> &'s Token {
        &self.stream.tokens[self.idx]
    }

    pub fn prev(&self) -> Option<&'s Token> {
        self.idx.checked_sub(1).map(|i| &self.stream.tokens[i])
    }

    pub fn next(&self) -> Option<&'s Token> {
        self.stream.tokens.get(self.idx + 1)
    }

    pub fn nextnext(&self) -> Option<&'s Token> {
        self.stream.tokens.get(self.idx + 2)
    }

    pub const fn buffer(&self) -> &'a str {
        self.stream.buffer
    }
}

/// Per-rule state across the tokens of one file (yamllint's `context`).
#[derive(Default)]
pub struct States {
    pub anchors: documents::AnchorsState,
    pub indentation: indentation::State,
    pub key_duplicates: documents::KeysState,
    pub key_ordering: documents::KeysState,
    pub quoted_strings: quoted_strings::State,
    pub truthy: scalars::TruthyState,
}

pub type TokenCheck = fn(&RuleConf, &Ctx<'_, '_>, &mut States, &mut Vec<Problem>);
pub type CommentCheck = fn(&RuleConf, &Stream<'_>, usize, &mut Vec<Problem>);
pub type LineCheck = fn(&RuleConf, &Stream<'_>, usize, &mut Vec<Problem>);

#[derive(Clone, Copy)]
pub enum Check {
    Token(TokenCheck),
    Comment(CommentCheck),
    Line(LineCheck),
}

pub struct RuleSpec {
    pub id: &'static str,
    pub options: &'static [OptSpec],
    /// yamllint's `VALIDATE`: a cross-option check run after the options are
    /// validated and defaulted; `Some(message)` rejects the config.
    pub validate: Option<fn(&RuleConf) -> Option<String>>,
    pub check: Check,
}

/// Every rule, in yamllint's alphabetical order.
pub static RULES: &[RuleSpec] = &[
    documents::ANCHORS,
    spacing::BRACES,
    spacing::BRACKETS,
    spacing::COLONS,
    spacing::COMMAS,
    comments::COMMENTS,
    comments::COMMENTS_INDENTATION,
    documents::DOCUMENT_END,
    documents::DOCUMENT_START,
    lines::EMPTY_LINES,
    documents::EMPTY_VALUES,
    scalars::FLOAT_VALUES,
    spacing::HYPHENS,
    indentation::INDENTATION,
    documents::KEY_DUPLICATES,
    documents::KEY_ORDERING,
    lines::LINE_LENGTH,
    lines::NEW_LINE_AT_END_OF_FILE,
    lines::NEW_LINES,
    scalars::OCTAL_VALUES,
    quoted_strings::QUOTED_STRINGS,
    lines::TRAILING_SPACES,
    scalars::TRUTHY,
];

/// The rule with this id.
pub fn get(id: &str) -> Option<&'static RuleSpec> {
    RULES.iter().find(|spec| spec.id == id)
}

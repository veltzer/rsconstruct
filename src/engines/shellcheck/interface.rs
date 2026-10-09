//! `ShellCheck`'s interface types (`ShellCheck.Interface`).

use super::ast::Id;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Ksh,
    Sh,
    Bash,
    Dash,
    BusyboxSh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    ErrorC,
    WarningC,
    InfoC,
    StyleC,
}

impl Severity {
    /// The name in gcc-style output.
    pub const fn gcc_name(self) -> &'static str {
        match self {
            Self::ErrorC => "error",
            Self::WarningC => "warning",
            Self::InfoC => "note",
            Self::StyleC => "note",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub file: String,
    pub line: i64,
    pub column: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub severity: Severity,
    pub code: i64,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertionPoint {
    InsertBefore,
    InsertAfter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replacement {
    pub start: Position,
    pub end: Position,
    pub string: String,
    pub precedence: i64,
    pub insertion_point: InsertionPoint,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Fix {
    pub replacements: Vec<Replacement>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PositionedComment {
    pub start: Position,
    pub end: Position,
    pub comment: Comment,
    pub fix: Option<Fix>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenComment {
    pub id: Id,
    pub comment: Comment,
    pub fix: Option<Fix>,
}

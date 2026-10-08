//! Helpers shared by several rules: yamllint's `rules/common.py`.

use super::super::Problem;
use super::super::parser::{Stream, Token};

/// Mirrors `spaces_after`: the number of spaces between `token` and `next`
/// on the same line, checked against `min`/`max` (`-1` disables a bound).
pub fn spaces_after(
    token: &Token,
    next: Option<&Token>,
    min: i64,
    max: i64,
    min_desc: &str,
    max_desc: &str,
) -> Option<Problem> {
    let next = next?;
    if token.end.line != next.start.line {
        return None;
    }
    let spaces = next.start.index as i64 - token.end.index as i64;
    if max != -1 && spaces > max {
        Some(Problem::new(
            token.start.line + 1,
            next.start.column,
            max_desc,
        ))
    } else if min != -1 && spaces < min {
        Some(Problem::new(
            token.start.line + 1,
            next.start.column + 1,
            min_desc,
        ))
    } else {
        None
    }
}

/// Mirrors `spaces_before`: the number of spaces between `prev` and `token`
/// on the same line. A `prev` that ends at the start of the next line (as
/// scalars often do) is discarded.
pub fn spaces_before(
    stream: &Stream<'_>,
    token: &Token,
    prev: Option<&Token>,
    min: i64,
    max: i64,
    min_desc: &str,
    max_desc: &str,
) -> Option<Problem> {
    let prev = prev?;
    if prev.end.line != token.start.line {
        return None;
    }
    if prev.end.index != 0 && stream.bytes()[prev.end.index - 1] == b'\n' {
        return None;
    }
    let spaces = token.start.index as i64 - prev.end.index as i64;
    if max != -1 && spaces > max {
        Some(Problem::new(
            token.start.line + 1,
            token.start.column,
            max_desc,
        ))
    } else if min != -1 && spaces < min {
        Some(Problem::new(
            token.start.line + 1,
            token.start.column + 1,
            min_desc,
        ))
    } else {
        None
    }
}

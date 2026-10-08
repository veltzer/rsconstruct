//! Line rules: yamllint's `empty-lines`, `line-length`,
//! `new-line-at-end-of-file`, `new-lines` and `trailing-spaces`.

use libyaml_safer::{Scanner, TokenData};

use super::super::Problem;
use super::super::config::RuleConf;
use super::super::parser::{Stream, is_python_whitespace};
use super::{Alt, Check, Default, OptKind, OptSpec, RuleSpec};

// ─── empty-lines ─────────────────────────────────────────────────────────────

fn check_empty_lines(conf: &RuleConf, stream: &Stream<'_>, idx: usize, out: &mut Vec<Problem>) {
    let line = stream.lines[idx];
    let buf = stream.bytes();
    if !(line.start == line.end && line.end < buf.len()) {
        return;
    }
    // Only alert on the last blank line of a series.
    if buf[line.end..].starts_with(b"\n\n") || buf[line.end..].starts_with(b"\r\n\r\n") {
        return;
    }

    let mut blank_lines = 0;
    let mut start = line.start;
    while start >= 2 && &buf[start - 2..start] == b"\r\n" {
        blank_lines += 1;
        start -= 2;
    }
    while start >= 1 && buf[start - 1] == b'\n' {
        blank_lines += 1;
        start -= 1;
    }

    // Special case: start of document.
    let at_start = start == 0;
    if at_start {
        blank_lines += 1; // the first line has no preceding \n
    }

    // Special case: end of document. The last line of a file is supposed to
    // end with a new line (POSIX's definition of a line).
    let at_end = (line.end + 1 == buf.len() && buf[line.end] == b'\n')
        || (line.end + 2 == buf.len() && &buf[line.end..] == b"\r\n");
    // Allow the exception of the one-byte file containing '\n'.
    if at_end && line.end == 0 {
        return;
    }

    let max = if at_end {
        conf.int_opt("max-end")
    } else if at_start {
        conf.int_opt("max-start")
    } else {
        conf.int_opt("max")
    };

    if i64::from(blank_lines) > max {
        out.push(Problem::new(
            line.number,
            1,
            format!("too many blank lines ({blank_lines} > {max})"),
        ));
    }
}

pub const EMPTY_LINES: RuleSpec = RuleSpec {
    id: "empty-lines",
    options: &[
        OptSpec {
            name: "max",
            kind: OptKind::Int,
            default: Default::Int(2),
        },
        OptSpec {
            name: "max-start",
            kind: OptKind::Int,
            default: Default::Int(0),
        },
        OptSpec {
            name: "max-end",
            kind: OptKind::Int,
            default: Default::Int(0),
        },
    ],
    validate: None,
    check: Check::Line(check_empty_lines),
};

// ─── line-length ─────────────────────────────────────────────────────────────

/// Mirrors `check_inline_mapping`: the line is `key: value` whose value has
/// no space in it, so it cannot be broken.
fn check_inline_mapping(content: &str) -> bool {
    let mut scanner: Scanner<&mut &[u8]> = Scanner::new();
    let mut input = content.as_bytes();
    scanner.set_input_string(&mut input);
    let mut in_mapping = false;
    loop {
        let Ok(token) = Scanner::scan(&mut scanner) else {
            return false;
        };
        match token.data {
            TokenData::StreamEnd => return false,
            TokenData::BlockMappingStart => in_mapping = true,
            TokenData::Value if in_mapping => {
                let Ok(next) = Scanner::scan(&mut scanner) else {
                    return false;
                };
                match next.data {
                    TokenData::Scalar { .. } => {
                        let column = next.start_mark.column as usize;
                        return !content.chars().skip(column).any(|c| c == ' ');
                    }
                    TokenData::StreamEnd => return false,
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn check_line_length(conf: &RuleConf, stream: &Stream<'_>, idx: usize, out: &mut Vec<Problem>) {
    let line = stream.lines[idx];
    let buf = stream.bytes();
    let max_length = conf.int_opt("max");
    // yamllint measures the line in characters.
    let length = line.content(stream.buffer).chars().count() as i64;
    if length <= max_length {
        return;
    }
    let allow_inline = conf.bool_opt("allow-non-breakable-inline-mappings");
    let allow_words = conf.bool_opt("allow-non-breakable-words") || allow_inline;
    if allow_words {
        let mut start = line.start;
        while start < line.end && buf[start] == b' ' {
            start += 1;
        }
        if start != line.end {
            if buf[start] == b'#' {
                while start < line.end && buf[start] == b'#' {
                    start += 1;
                }
                start += 1;
            } else if buf[start] == b'-' {
                start += 2;
            }
            let start = start.min(line.end);
            if !buf[start..line.end].contains(&b' ') {
                return;
            }
            if allow_inline && check_inline_mapping(line.content(stream.buffer)) {
                return;
            }
        }
    }
    out.push(Problem::new(
        line.number,
        (max_length + 1) as usize,
        format!("line too long ({length} > {max_length} characters)"),
    ));
}

pub const LINE_LENGTH: RuleSpec = RuleSpec {
    id: "line-length",
    options: &[
        OptSpec {
            name: "max",
            kind: OptKind::Int,
            default: Default::Int(80),
        },
        OptSpec {
            name: "allow-non-breakable-words",
            kind: OptKind::Bool,
            default: Default::Bool(true),
        },
        OptSpec {
            name: "allow-non-breakable-inline-mappings",
            kind: OptKind::Bool,
            default: Default::Bool(false),
        },
    ],
    validate: None,
    check: Check::Line(check_line_length),
};

// ─── new-line-at-end-of-file ─────────────────────────────────────────────────

fn check_new_line_at_end_of_file(
    _: &RuleConf,
    stream: &Stream<'_>,
    idx: usize,
    out: &mut Vec<Problem>,
) {
    let line = stream.lines[idx];
    if line.end == stream.buffer.len() && line.end > line.start {
        out.push(Problem::new(
            line.number,
            line.content(stream.buffer).chars().count() + 1,
            "no new line character at the end of file",
        ));
    }
}

pub const NEW_LINE_AT_END_OF_FILE: RuleSpec = RuleSpec {
    id: "new-line-at-end-of-file",
    options: &[],
    validate: None,
    check: Check::Line(check_new_line_at_end_of_file),
};

// ─── new-lines ───────────────────────────────────────────────────────────────

const NEW_LINE_TYPES: &[Alt] = &[Alt::Str("unix"), Alt::Str("dos"), Alt::Str("platform")];

fn check_new_lines(conf: &RuleConf, stream: &Stream<'_>, idx: usize, out: &mut Vec<Problem>) {
    let line = stream.lines[idx];
    let buf = stream.bytes();
    // `platform` is the running system's line separator: `\n` on the unix
    // systems rsconstruct supports.
    let newline: &[u8] = match conf.str_opt("type") {
        "dos" => b"\r\n",
        _ => b"\n",
    };
    if line.start == 0 && buf.len() > line.end && !buf[line.end..].starts_with(newline) {
        let shown = if newline == b"\n" { "\\n" } else { "\\r\\n" };
        out.push(Problem::new(
            1,
            line.content(stream.buffer).chars().count() + 1,
            format!("wrong new line character: expected {shown}"),
        ));
    }
}

pub const NEW_LINES: RuleSpec = RuleSpec {
    id: "new-lines",
    options: &[OptSpec {
        name: "type",
        kind: OptKind::OneOf(NEW_LINE_TYPES),
        default: Default::Str("unix"),
    }],
    validate: None,
    check: Check::Line(check_new_lines),
};

// ─── trailing-spaces ─────────────────────────────────────────────────────────

fn check_trailing_spaces(_: &RuleConf, stream: &Stream<'_>, idx: usize, out: &mut Vec<Problem>) {
    let line = stream.lines[idx];
    if line.end == 0 {
        return;
    }
    let buf = stream.bytes();
    // YAML recognizes two white space characters: space and tab.
    let mut pos = line.end;
    while pos > line.start && is_python_whitespace(buf[pos - 1]) {
        pos -= 1;
    }
    if pos != line.end && matches!(buf[pos], b' ' | b'\t') {
        let column = stream.buffer[line.start..pos].chars().count() + 1;
        out.push(Problem::new(line.number, column, "trailing spaces"));
    }
}

pub const TRAILING_SPACES: RuleSpec = RuleSpec {
    id: "trailing-spaces",
    options: &[],
    validate: None,
    check: Check::Line(check_trailing_spaces),
};

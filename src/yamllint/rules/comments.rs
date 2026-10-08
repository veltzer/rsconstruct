//! Comment rules: yamllint's `comments` and `comments-indentation`.

use super::super::Problem;
use super::super::config::RuleConf;
use super::super::parser::{Kind, Stream};
use super::{Check, Default, OptKind, OptSpec, RuleSpec};

fn check_comments(conf: &RuleConf, stream: &Stream<'_>, idx: usize, out: &mut Vec<Problem>) {
    let comment = stream.comments[idx];
    let buf = stream.bytes();
    let min_spaces = conf.int_opt("min-spaces-from-content");
    if min_spaces != -1 && stream.comment_is_inline(&comment) {
        let before = &stream.tokens[comment.token_before];
        if (comment.pointer as i64 - before.end.index as i64) < min_spaces {
            out.push(Problem::new(
                comment.line_no,
                comment.column_no,
                format!("too few spaces before comment: expected {min_spaces}"),
            ));
        }
    }

    if conf.bool_opt("require-starting-space") {
        let mut text_start = comment.pointer + 1;
        while text_start < buf.len() && buf[text_start] == b'#' {
            text_start += 1;
        }
        if text_start < buf.len() {
            if conf.bool_opt("ignore-shebangs")
                && comment.line_no == 1
                && comment.column_no == 1
                && buf[text_start] == b'!'
            {
                return;
            }
            // \r and \r\n are both covered by checking the first character;
            // \r itself is a valid newline on some older systems.
            if !matches!(buf[text_start], b' ' | b'\n' | b'\r' | 0) {
                let column = comment.column_no + text_start - comment.pointer;
                out.push(Problem::new(
                    comment.line_no,
                    column,
                    "missing starting space in comment",
                ));
            }
        }
    }
}

pub const COMMENTS: RuleSpec = RuleSpec {
    id: "comments",
    options: &[
        OptSpec {
            name: "require-starting-space",
            kind: OptKind::Bool,
            default: Default::Bool(true),
        },
        OptSpec {
            name: "ignore-shebangs",
            kind: OptKind::Bool,
            default: Default::Bool(true),
        },
        OptSpec {
            name: "min-spaces-from-content",
            kind: OptKind::Int,
            default: Default::Int(2),
        },
    ],
    validate: None,
    check: Check::Comment(check_comments),
};

// Case A:
//
//     prev: line:
//       # commented line
//       current: line
//
// Case B:
//
//       prev: line
//       # commented line 1
//     # commented line 2
//     current: line
fn check_comments_indentation(
    _: &RuleConf,
    stream: &Stream<'_>,
    idx: usize,
    out: &mut Vec<Problem>,
) {
    let comment = stream.comments[idx];
    let before = &stream.tokens[comment.token_before];
    // Only check block comments.
    if before.kind != Kind::StreamStart && before.end.line + 1 == comment.line_no {
        return;
    }

    let next_line_indent = comment.token_after.map_or(0, |i| {
        let after = &stream.tokens[i];
        if after.kind == Kind::StreamEnd {
            0
        } else {
            after.start.column
        }
    });

    let mut prev_line_indent = if before.kind == Kind::StreamStart {
        0
    } else {
        before.line_indent(stream.buffer)
    };

    // In the following case only the next line indent is valid:
    //     list:
    //         # comment
    //         - 1
    //         - 2
    prev_line_indent = prev_line_indent.max(next_line_indent);

    // If two indents are valid but a previous comment went back to normal
    // indent, the next ones must do the same. In other words, avoid this:
    //     list:
    //         - 1
    //     # comment on valid indent (0)
    //         # comment on valid indent (4)
    //     other-list:
    //         - 2
    if let Some(i) = comment.previous
        && !stream.comment_is_inline(&stream.comments[i])
    {
        prev_line_indent = stream.comments[i].column_no - 1;
    }

    if comment.column_no - 1 != prev_line_indent && comment.column_no - 1 != next_line_indent {
        out.push(Problem::new(
            comment.line_no,
            comment.column_no,
            "comment not indented like content",
        ));
    }
}

pub const COMMENTS_INDENTATION: RuleSpec = RuleSpec {
    id: "comments-indentation",
    options: &[],
    validate: None,
    check: Check::Comment(check_comments_indentation),
};

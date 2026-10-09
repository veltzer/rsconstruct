//! cpplint's line views and its brace/template matchers.
//!
//! `CleansedLines` holds the four copies of a file cpplint checks against:
//! the raw lines, the lines with C++11 raw strings blanked, the lines with
//! comments removed, and the "elided" lines with strings collapsed to `""`
//! as well. The expression matchers (`close_expression` and its reverse)
//! walk those lines across line boundaries to pair parentheses, braces,
//! brackets and template angle brackets.
//!
//! Positions are byte offsets into the line, always on a character boundary;
//! where cpplint indexes a line character by character the walk here goes
//! over `char_indices`, so the offsets agree with cpplint's on ASCII and
//! stay on boundaries elsewhere.

use super::Lint;
use super::regex::{Captures, find_all, g, is_match, is_search, pmatch, rx, sub};
use super::tables::alt_token_replacement;

/// `s[start:end]` with Python's tolerance: bounds are clamped to the string
/// and moved down to character boundaries, `start > end` gives `""`.
pub fn sl(s: &str, start: usize, end: usize) -> &str {
    let mut end = end.min(s.len());
    let mut start = start.min(end);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    while !s.is_char_boundary(start) {
        start -= 1;
    }
    &s[start..end]
}

/// `s[start:]`.
pub fn from(s: &str, start: usize) -> &str {
    sl(s, start, s.len())
}

/// `s[:end]`.
pub fn upto(s: &str, end: usize) -> &str {
    sl(s, 0, end)
}

/// The character starting at byte `pos`, if `pos` is a character boundary
/// inside `s`.
pub fn char_at(s: &str, pos: usize) -> Option<char> {
    s.get(pos..)?.chars().next()
}

/// The byte offset of the last character of `s`.
pub fn last_char_start(s: &str) -> Option<usize> {
    s.char_indices().last().map(|(i, _)| i)
}

/// The byte offset of the character after the one starting at `pos`
/// (cpplint's `pos + 1`).
pub fn advance_char(s: &str, pos: usize) -> usize {
    char_at(s, pos).map_or(pos + 1, |c| pos + c.len_utf8())
}

/// `s.count(needle)`.
pub fn count(s: &str, needle: &str) -> usize {
    s.matches(needle).count()
}

/// `s.count(needle)` as a signed number, for cpplint's count arithmetic.
pub fn icount(s: &str, needle: &str) -> i64 {
    i64::try_from(count(s, needle)).unwrap_or(i64::MAX)
}

/// cpplint's `IsCppString`: does `line` end inside a string constant?
pub fn is_cpp_string(line: &str) -> bool {
    let line = line.replace("\\\\", "XX");
    let n = icount(&line, "\"") - icount(&line, "\\\"") - icount(&line, "'\"'");
    (n & 1) == 1
}

/// cpplint's `CleanseRawStrings`: C++11 raw strings replaced by `""`, with
/// the inner lines of a multi-line raw string replaced whole.
pub fn cleanse_raw_strings(raw_lines: &[String]) -> Vec<String> {
    let mut delimiter: Option<String> = None;
    let mut out = Vec::with_capacity(raw_lines.len());
    for raw in raw_lines {
        let mut line = raw.clone();
        if let Some(delim) = &delimiter {
            if let Some(end) = line.find(delim.as_str()) {
                let leading = pmatch(r"(\s*)\S", &line)
                    .map_or("", |m| g(&m, 1))
                    .to_string();
                let rest = from(&line, end + delim.len()).to_string();
                line = format!("{leading}\"\"{rest}");
                delimiter = None;
            } else {
                line = "\"\"".to_string();
            }
        }
        while delimiter.is_none() {
            let Some(m) = pmatch(r#"(.*?)\b(?:R|u8R|uR|UR|LR)"([^\s\\()]*)\((.*)$"#, &line) else {
                break;
            };
            if is_match(r#"([^'"]|'(\\.|[^'])*'|"(\\.|[^"])*")*//"#, g(&m, 1)) {
                break;
            }
            let delim = format!("){}\"", g(&m, 2));
            let head = g(&m, 1).to_string();
            let body = g(&m, 3);
            if let Some(end) = body.find(delim.as_str()) {
                let rest = from(body, end + delim.len()).to_string();
                line = format!("{head}\"\"{rest}");
            } else {
                line = format!("{head}\"\"");
                delimiter = Some(delim);
            }
        }
        out.push(line);
    }
    out
}

fn find_next_multi_line_comment_start(lines: &[String], mut lineix: usize) -> usize {
    while lineix < lines.len() {
        let stripped = lines[lineix].trim();
        if stripped.starts_with("/*") && !stripped[2..].contains("*/") {
            return lineix;
        }
        lineix += 1;
    }
    lines.len()
}

fn find_next_multi_line_comment_end(lines: &[String], mut lineix: usize) -> usize {
    while lineix < lines.len() {
        if lines[lineix].trim().ends_with("*/") {
            return lineix;
        }
        lineix += 1;
    }
    lines.len()
}

/// cpplint's `RemoveMultiLineComments`: every line of a comment that spans
/// lines becomes `/**/`.
pub fn remove_multi_line_comments(lint: &mut Lint<'_>, lines: &mut [String]) {
    let mut lineix = 0;
    while lineix < lines.len() {
        let begin = find_next_multi_line_comment_start(lines, lineix);
        if begin >= lines.len() {
            return;
        }
        let end = find_next_multi_line_comment_end(lines, begin);
        if end >= lines.len() {
            lint.error(
                begin + 1,
                "readability/multiline_comment",
                5,
                "Could not find end of multi-line comment".to_string(),
            );
            return;
        }
        for line in &mut lines[begin..=end] {
            *line = "/**/".to_string();
        }
        lineix = end + 1;
    }
}

const C_COMMENTS_CLEANSE: &str = r"(\s*/\*(?:[^*]|\*(?!/))*\*/\s*$|/\*(?:[^*]|\*(?!/))*\*/\s+|\s+/\*(?:[^*]|\*(?!/))*\*/(?=\W)|/\*(?:[^*]|\*(?!/))*\*/)";

/// cpplint's `CleanseComments`: `//` comments and single-line `/* */`
/// comments removed.
pub fn cleanse_comments(line: &str) -> String {
    let mut line = line;
    if let Some(commentpos) = line.find("//")
        && !is_cpp_string(&line[..commentpos])
    {
        line = line[..commentpos].trim_end();
    }
    sub(C_COMMENTS_CLEANSE, "", line)
}

const ALT_TOKEN_PATTERN: &str =
    r"([ =()])(and|bitor|or|xor|compl|bitand|and_eq|or_eq|xor_eq|not|not_eq)([ (]|$)";

/// cpplint's `ReplaceAlternateTokens`.
pub fn replace_alternate_tokens(line: &str) -> String {
    let mut out = line.to_string();
    for m in find_all(ALT_TOKEN_PATTERN, line) {
        let word = g(&m, 2);
        let token = alt_token_replacement(word);
        let drop_tail = (word == "not" || word == "compl") && g(&m, 3) == " ";
        out = rx(ALT_TOKEN_PATTERN)
            .replacen(&out, 1, |caps: &Captures<'_>| {
                let tail = if drop_tail { "" } else { g(caps, 3) };
                format!("{}{token}{tail}", g(caps, 1))
            })
            .into_owned();
    }
    out
}

/// cpplint's `_RE_PATTERN_INCLUDE`.
pub const INCLUDE_PATTERN: &str = r#"^\s*#\s*include\s*([<"])([^>"]*)[>"].*$"#;

/// cpplint's `CleansedLines._CollapseStrings`: strings and character
/// literals collapsed to `""` and `''`, digit separators dropped.
pub fn collapse_strings(elided: &str) -> String {
    if is_match(INCLUDE_PATTERN, elided) {
        return elided.to_string();
    }
    let mut elided = sub(r#"\\([abfnrtv?"\\']|\d+|x[0-9a-fA-F]+)"#, "", elided);
    let mut collapsed = String::new();
    loop {
        let Some(m) = pmatch(r#"([^'"]*)(['"])(.*)$"#, &elided) else {
            collapsed.push_str(&elided);
            break;
        };
        let head = g(&m, 1).to_string();
        let quote = g(&m, 2).to_string();
        let tail = g(&m, 3).to_string();
        if quote == "\"" {
            if let Some(second) = tail.find('"') {
                collapsed.push_str(&head);
                collapsed.push_str("\"\"");
                elided = tail[second + 1..].to_string();
            } else {
                collapsed.push_str(&elided);
                break;
            }
        } else if is_search(r"\b(?:0[bBxX]?|[1-9])[0-9a-fA-F]*$", &head) {
            let literal = format!("'{tail}");
            let lit = pmatch(r"((?:'?[0-9a-zA-Z_])*)(.*)$", &literal);
            let (digits, rest) = lit.map_or(("", ""), |l| (g(&l, 1), g(&l, 2)));
            collapsed.push_str(&head);
            collapsed.push_str(&digits.replace('\'', ""));
            elided = rest.to_string();
        } else if let Some(second) = tail.find('\'') {
            collapsed.push_str(&head);
            collapsed.push_str("''");
            elided = tail[second + 1..].to_string();
        } else {
            collapsed.push_str(&elided);
            break;
        }
    }
    collapsed
}

/// The four views of a file's lines.
pub struct CleansedLines {
    /// Lines without strings and comments.
    pub elided: Vec<String>,
    /// Lines without comments.
    pub lines: Vec<String>,
    /// The lines as read (multi-line comments already blanked, alternate
    /// tokens replaced when that check is off).
    pub raw_lines: Vec<String>,
    /// `raw_lines` with C++11 raw strings removed.
    pub lines_without_raw_strings: Vec<String>,
    pub num_lines: usize,
}

impl CleansedLines {
    pub fn new(mut lines: Vec<String>, replace_alt_tokens: bool) -> Self {
        if replace_alt_tokens {
            for line in &mut lines {
                *line = replace_alternate_tokens(line);
            }
        }
        let lines_without_raw_strings = cleanse_raw_strings(&lines);
        let mut cleansed = Vec::with_capacity(lines.len());
        let mut elided = Vec::with_capacity(lines.len());
        for line in &lines_without_raw_strings {
            cleansed.push(cleanse_comments(line));
            elided.push(cleanse_comments(&collapse_strings(line)));
        }
        Self {
            elided,
            lines: cleansed,
            num_lines: lines.len(),
            raw_lines: lines,
            lines_without_raw_strings,
        }
    }
}

/// The outcome of scanning one line for the end (or start) of an expression.
pub enum Scan {
    /// The matching end was found at this offset (just past it for a forward
    /// scan, at it for a backward scan).
    Found(usize),
    /// The expression cannot close (mismatched or never opened).
    Fail,
    /// The line ended inside the expression; the nesting stack to continue
    /// with on the next line.
    Continue(Vec<char>),
}

const fn is_closing_pair(open: char, close: char) -> bool {
    matches!((open, close), ('(', ')') | ('[', ']') | ('{', '}'))
}

/// cpplint's `FindEndOfExpressionInLine`.
pub fn find_end_of_expression_in_line(line: &str, startpos: usize, mut stack: Vec<char>) -> Scan {
    let mut prev: Option<(usize, char)> = None;
    for (i, c) in line.char_indices() {
        if i < startpos {
            prev = Some((i, c));
            continue;
        }
        if "([{".contains(c) {
            stack.push(c);
        } else if c == '<' {
            if prev.is_some_and(|(_, p)| p == '<') {
                if stack.last() == Some(&'<') {
                    stack.pop();
                    if stack.is_empty() {
                        return Scan::Fail;
                    }
                }
            } else if prev.is_some() && is_search(r"\boperator\s*$", &line[..i]) {
                // operator<, not a template argument list
            } else {
                stack.push('<');
            }
        } else if ")]}".contains(c) {
            while stack.last() == Some(&'<') {
                stack.pop();
            }
            let Some(&top) = stack.last() else {
                return Scan::Fail;
            };
            if is_closing_pair(top, c) {
                stack.pop();
                if stack.is_empty() {
                    return Scan::Found(i + c.len_utf8());
                }
            } else {
                return Scan::Fail;
            }
        } else if c == '>' {
            if let Some((pi, pc)) = prev
                && (pc == '-' || is_search(r"\boperator\s*$", &line[..pi]))
            {
                prev = Some((i, c));
                continue;
            }
            if stack.last() == Some(&'<') {
                stack.pop();
                if stack.is_empty() {
                    return Scan::Found(i + c.len_utf8());
                }
            }
        } else if c == ';' {
            while stack.last() == Some(&'<') {
                stack.pop();
            }
            if stack.is_empty() {
                return Scan::Fail;
            }
        }
        prev = Some((i, c));
    }
    Scan::Continue(stack)
}

/// cpplint's `CloseExpression`: for an opener at `(linenum, pos)`, the
/// cleansed line, line number and offset just past the matching closer;
/// `(line, num_lines, None)` when there is none.
pub fn close_expression(
    cl: &CleansedLines,
    mut linenum: usize,
    pos: usize,
) -> (&str, usize, Option<usize>) {
    let mut line = cl.elided[linenum].as_str();
    let Some(c) = char_at(line, pos) else {
        return (line, cl.num_lines, None);
    };
    if !"({[<".contains(c) || is_match(r"<[<=]", &line[pos..]) {
        return (line, cl.num_lines, None);
    }
    let mut stack = match find_end_of_expression_in_line(line, pos, Vec::new()) {
        Scan::Found(end) => return (line, linenum, Some(end)),
        Scan::Fail => return (line, cl.num_lines, None),
        Scan::Continue(stack) => stack,
    };
    while !stack.is_empty() && linenum < cl.num_lines - 1 {
        linenum += 1;
        line = cl.elided[linenum].as_str();
        stack = match find_end_of_expression_in_line(line, 0, stack) {
            Scan::Found(end) => return (line, linenum, Some(end)),
            Scan::Fail => return (line, cl.num_lines, None),
            Scan::Continue(stack) => stack,
        };
    }
    (line, cl.num_lines, None)
}

/// cpplint's `FindStartOfExpressionInLine`; `endpos` is the offset of the
/// character to start from, `None` when the line is empty.
pub fn find_start_of_expression_in_line(
    line: &str,
    endpos: Option<usize>,
    mut stack: Vec<char>,
) -> Scan {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let Some(endpos) = endpos else {
        return Scan::Continue(stack);
    };
    let Some(start_k) = chars.iter().position(|(i, _)| *i == endpos) else {
        return Scan::Continue(stack);
    };
    let mut k: isize = isize::try_from(start_k).unwrap_or(isize::MAX);
    while k >= 0 {
        let ku = usize::try_from(k).unwrap_or(0);
        let (i, c) = chars[ku];
        let prev = if ku > 0 { Some(chars[ku - 1]) } else { None };
        if ")]}".contains(c) {
            stack.push(c);
        } else if c == '>' {
            if prev.is_some_and(|(pi, pc)| {
                pc == '-'
                    || is_match(r"\s>=\s", &line[pi..])
                    || is_search(r"\boperator\s*$", &line[..i])
            }) {
                k -= 1;
            } else {
                stack.push('>');
            }
        } else if c == '<' {
            if prev.is_some_and(|(_, pc)| pc == '<') {
                k -= 1;
            } else if stack.last() == Some(&'>') {
                stack.pop();
                if stack.is_empty() {
                    return Scan::Found(i);
                }
            }
        } else if "([{".contains(c) {
            while stack.last() == Some(&'>') {
                stack.pop();
            }
            let Some(&top) = stack.last() else {
                return Scan::Fail;
            };
            if is_closing_pair(c, top) {
                stack.pop();
                if stack.is_empty() {
                    return Scan::Found(i);
                }
            } else {
                return Scan::Fail;
            }
        } else if c == ';' {
            while stack.last() == Some(&'>') {
                stack.pop();
            }
            if stack.is_empty() {
                return Scan::Fail;
            }
        }
        k -= 1;
    }
    Scan::Continue(stack)
}

/// cpplint's `ReverseCloseExpression`: for a closer at `(linenum, pos)`,
/// the cleansed line, line number and offset of the matching opener;
/// `(line, 0, None)` when there is none.
pub fn reverse_close_expression(
    cl: &CleansedLines,
    mut linenum: usize,
    pos: usize,
) -> (&str, usize, Option<usize>) {
    let mut line = cl.elided[linenum].as_str();
    if !char_at(line, pos).is_some_and(|c| ")}]>".contains(c)) {
        return (line, 0, None);
    }
    let mut stack = match find_start_of_expression_in_line(line, Some(pos), Vec::new()) {
        Scan::Found(start) => return (line, linenum, Some(start)),
        Scan::Fail => return (line, 0, None),
        Scan::Continue(stack) => stack,
    };
    while !stack.is_empty() && linenum > 0 {
        linenum -= 1;
        line = cl.elided[linenum].as_str();
        stack = match find_start_of_expression_in_line(line, last_char_start(line), stack) {
            Scan::Found(start) => return (line, linenum, Some(start)),
            Scan::Fail => return (line, 0, None),
            Scan::Continue(stack) => stack,
        };
    }
    (line, 0, None)
}

/// cpplint's `GetLineWidth`: columns, with East Asian wide characters
/// counting two and combining characters none.
pub fn get_line_width(line: &str) -> usize {
    use unicode_normalization::UnicodeNormalization;
    use unicode_normalization::char::canonical_combining_class;
    use unicode_width::UnicodeWidthChar;
    let mut width = 0;
    for c in line.nfc() {
        if canonical_combining_class(c) != 0 {
            continue;
        }
        width += if c.width() == Some(2) { 2 } else { 1 };
    }
    width
}

/// cpplint's `GetIndentLevel`: the number of leading spaces before the
/// first non-blank character (0 for a blank line).
pub fn get_indent_level(line: &str) -> usize {
    pmatch(r"( *)\S", line).map_or(0, |m| g(&m, 1).len())
}

/// cpplint's `IsBlankLine`.
pub fn is_blank_line(line: &str) -> bool {
    line.trim().is_empty()
}

/// cpplint's `GetPreviousNonBlankLine`: the nearest earlier non-blank
/// elided line and its number, `("", None)` when there is none.
pub fn get_previous_non_blank_line(cl: &CleansedLines, linenum: usize) -> (&str, Option<usize>) {
    let mut prev = linenum;
    while prev > 0 {
        prev -= 1;
        let line = cl.elided[prev].as_str();
        if !is_blank_line(line) {
            return (line, Some(prev));
        }
    }
    ("", None)
}

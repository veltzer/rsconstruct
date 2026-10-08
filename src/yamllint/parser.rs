//! Token, comment and line streams over a YAML buffer, mirroring yamllint's
//! `parser.py`.
//!
//! Tokens come from libyaml (through `libyaml_safer`), the scanner design
//! `PyYAML` implements too, so start and end marks fall where the rules ported
//! from yamllint expect them. One difference: libyaml marks index the buffer
//! by byte where `PyYAML` indexes by character. The rules only measure spans
//! of whitespace and single ASCII characters between marks, where the two
//! agree; columns are characters in both.

use libyaml_safer::{ScalarStyle, Scanner, TokenData};

/// A position in the buffer: `PyYAML`'s `Mark`, 0-based like it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mark {
    /// Byte offset into the buffer.
    pub index: usize,
    /// 0-based line.
    pub line: usize,
    /// 0-based column, in characters.
    pub column: usize,
}

impl Mark {
    const fn from_lib(mark: libyaml_safer::Mark) -> Self {
        Self {
            index: mark.index as usize,
            line: mark.line as usize,
            column: mark.column as usize,
        }
    }
}

/// A scalar's style. `Plain` is `PyYAML`'s `style is None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Plain,
    SingleQuoted,
    DoubleQuoted,
    Literal,
    Folded,
}

impl Style {
    /// `|` or `>`.
    pub const fn is_block(self) -> bool {
        matches!(self, Self::Literal | Self::Folded)
    }
}

/// The token types `PyYAML`'s scanner emits, with the data the rules read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    StreamStart,
    StreamEnd,
    /// `%YAML 1.2` carries its version; `%TAG` and reserved directives do not.
    Directive {
        name: String,
        yaml_version: Option<(i32, i32)>,
    },
    DocumentStart,
    DocumentEnd,
    BlockSequenceStart,
    BlockMappingStart,
    BlockEnd,
    FlowSequenceStart,
    FlowSequenceEnd,
    FlowMappingStart,
    FlowMappingEnd,
    BlockEntry,
    FlowEntry,
    Key,
    Value,
    Alias(String),
    Anchor(String),
    Tag {
        handle: String,
        suffix: String,
    },
    Scalar {
        value: String,
        style: Style,
    },
}

/// A scanner token with `PyYAML`'s two marks.
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: Kind,
    pub start: Mark,
    pub end: Mark,
}

impl Token {
    pub const fn is_scalar(&self) -> bool {
        matches!(self.kind, Kind::Scalar { .. })
    }

    /// The scalar's value, or `None` for any other token.
    pub fn scalar_value(&self) -> Option<&str> {
        match &self.kind {
            Kind::Scalar { value, .. } => Some(value),
            _ => None,
        }
    }

    /// The scalar's style, or `None` for any other token.
    pub const fn scalar_style(&self) -> Option<Style> {
        match &self.kind {
            Kind::Scalar { style, .. } => Some(*style),
            _ => None,
        }
    }

    /// `PyYAML`'s `token.plain`: a scalar in the plain style.
    pub fn is_plain_scalar(&self) -> bool {
        self.scalar_style() == Some(Style::Plain)
    }

    /// `PyYAML`'s `token.style` being truthy: a scalar that is not plain.
    pub fn is_styled_scalar(&self) -> bool {
        matches!(self.scalar_style(), Some(style) if style != Style::Plain)
    }

    pub const fn is_tag(&self) -> bool {
        matches!(self.kind, Kind::Tag { .. })
    }

    pub const fn is_directive(&self) -> bool {
        matches!(self.kind, Kind::Directive { .. })
    }

    /// Mirrors `is_explicit_key` in yamllint's `rules/common.py`.
    pub const fn is_explicit_key(&self, buffer: &str) -> bool {
        self.start.index < self.end.index && buffer.as_bytes()[self.start.index] == b'?'
    }

    /// Mirrors `get_real_end_line`: the line a token really ends on. `PyYAML`
    /// scalar tokens often end on the next line.
    pub const fn real_end_line(&self, buffer: &str) -> usize {
        let mut end_line = self.end.line + 1;
        if !self.is_scalar() {
            return end_line;
        }
        let bytes = buffer.as_bytes();
        let mut pos = self.end.index as isize - 1;
        while pos >= self.start.index as isize - 1
            && pos >= 0
            && is_python_whitespace(bytes[pos as usize])
        {
            if bytes[pos as usize] == b'\n' {
                end_line -= 1;
            }
            pos -= 1;
        }
        end_line
    }

    /// Mirrors `get_line_indent`: the indent of the line the token starts in.
    pub fn line_indent(&self, buffer: &str) -> usize {
        let bytes = buffer.as_bytes();
        let start = bytes[..self.start.index]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |pos| pos + 1);
        let mut content = start;
        while content < bytes.len() && bytes[content] == b' ' {
            content += 1;
        }
        content - start
    }
}

/// Python's `string.whitespace`.
pub const fn is_python_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn convert(token: libyaml_safer::Token) -> Token {
    let kind = match token.data {
        TokenData::StreamStart { .. } => Kind::StreamStart,
        TokenData::StreamEnd => Kind::StreamEnd,
        TokenData::VersionDirective { major, minor } => Kind::Directive {
            name: "YAML".to_owned(),
            yaml_version: Some((major, minor)),
        },
        TokenData::TagDirective { .. } => Kind::Directive {
            name: "TAG".to_owned(),
            yaml_version: None,
        },
        TokenData::DocumentStart => Kind::DocumentStart,
        TokenData::DocumentEnd => Kind::DocumentEnd,
        TokenData::BlockSequenceStart => Kind::BlockSequenceStart,
        TokenData::BlockMappingStart => Kind::BlockMappingStart,
        TokenData::BlockEnd => Kind::BlockEnd,
        TokenData::FlowSequenceStart => Kind::FlowSequenceStart,
        TokenData::FlowSequenceEnd => Kind::FlowSequenceEnd,
        TokenData::FlowMappingStart => Kind::FlowMappingStart,
        TokenData::FlowMappingEnd => Kind::FlowMappingEnd,
        TokenData::BlockEntry => Kind::BlockEntry,
        TokenData::FlowEntry => Kind::FlowEntry,
        TokenData::Key => Kind::Key,
        TokenData::Value => Kind::Value,
        TokenData::Alias { value } => Kind::Alias(value),
        TokenData::Anchor { value } => Kind::Anchor(value),
        TokenData::Tag { handle, suffix } => Kind::Tag { handle, suffix },
        TokenData::Scalar { value, style } => Kind::Scalar {
            value,
            style: match style {
                ScalarStyle::SingleQuoted => Style::SingleQuoted,
                ScalarStyle::DoubleQuoted => Style::DoubleQuoted,
                ScalarStyle::Literal => Style::Literal,
                ScalarStyle::Folded => Style::Folded,
                // `Plain`, `Any` (never emitted by the scanner), and the
                // enum's non-exhaustive remainder.
                _ => Style::Plain,
            },
        },
    };
    Token {
        kind,
        start: Mark::from_lib(token.start_mark),
        end: Mark::from_lib(token.end_mark),
    }
}

/// Scan `buffer` into tokens, from `StreamStart` to `StreamEnd`. A scanner
/// error ends the stream where yamllint's generator stops too: `PyYAML` looks
/// two tokens ahead (`next`, `nextnext`) before yielding one, so the two
/// tokens scanned last before the error are never yielded.
pub fn scan_tokens(buffer: &str) -> Vec<Token> {
    let mut scanner: Scanner<&mut &[u8]> = Scanner::new();
    let mut input = buffer.as_bytes();
    scanner.set_input_string(&mut input);
    let mut tokens = Vec::new();
    loop {
        // `Scanner::scan`, named in full: the plain method call resolves to
        // `Iterator::scan`, which the type also implements.
        let Ok(token) = Scanner::scan(&mut scanner) else {
            tokens.truncate(tokens.len().saturating_sub(2));
            return tokens;
        };
        let end = matches!(token.data, TokenData::StreamEnd);
        tokens.push(convert(token));
        if end {
            return tokens;
        }
    }
}

/// A physical line: `content` is `buffer[start..end]`, without its newline.
#[derive(Debug, Clone, Copy)]
pub struct Line {
    /// 1-based (yamllint's `line_no`).
    pub number: usize,
    pub start: usize,
    pub end: usize,
}

impl Line {
    pub fn content<'a>(&self, buffer: &'a str) -> &'a str {
        &buffer[self.start..self.end]
    }
}

/// Mirrors `line_generator`: `\r\n` and `\n` both end a line; the last line
/// runs to the end of the buffer even when it is empty.
pub fn lines(buffer: &str) -> Vec<Line> {
    let bytes = buffer.as_bytes();
    let mut out = Vec::new();
    let mut cur = 0;
    while let Some(offset) = bytes[cur..].iter().position(|&b| b == b'\n') {
        let next = cur + offset;
        let end = if next > 0 && bytes[next - 1] == b'\r' {
            next - 1
        } else {
            next
        };
        out.push(Line {
            number: out.len() + 1,
            start: cur,
            end,
        });
        cur = next + 1;
    }
    out.push(Line {
        number: out.len() + 1,
        start: cur,
        end: bytes.len(),
    });
    out
}

/// A `#` comment, found between two tokens.
#[derive(Debug, Clone, Copy)]
pub struct Comment {
    /// 1-based.
    pub line_no: usize,
    /// 1-based, in characters.
    pub column_no: usize,
    /// Byte offset of the `#`.
    pub pointer: usize,
    /// Index into the token list.
    pub token_before: usize,
    /// Index into the token list; `None` after the last token.
    pub token_after: Option<usize>,
    /// Index into the comment list: the previous comment in the same gap
    /// between tokens (yamllint's `comment_before`).
    pub previous: Option<usize>,
}

/// One element of the merged stream, in the order yamllint's
/// `token_or_comment_or_line_generator` yields them.
#[derive(Debug, Clone, Copy)]
pub enum Elem {
    Token(usize),
    Comment(usize),
    Line(usize),
}

/// The buffer with its tokens, comments and lines, and the merged order.
pub struct Stream<'a> {
    pub buffer: &'a str,
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
    pub lines: Vec<Line>,
    pub elems: Vec<Elem>,
}

impl<'a> Stream<'a> {
    pub fn new(buffer: &'a str) -> Self {
        let tokens = scan_tokens(buffer);
        let lines = lines(buffer);
        let mut comments = Vec::new();
        // Tokens and comments in generation order, each with its line number.
        let mut tok_or_com: Vec<(usize, Elem)> = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            tok_or_com.push((token.start.line + 1, Elem::Token(i)));
            let first = comments.len();
            comments_between_tokens(buffer, &tokens, i, &mut comments);
            for (c, comment) in comments.iter().enumerate().skip(first) {
                tok_or_com.push((comment.line_no, Elem::Comment(c)));
            }
        }

        // Merge by line number; on a tie the token or comment comes first,
        // so a Line is always the last element of its line.
        let mut elems = Vec::with_capacity(tok_or_com.len() + lines.len());
        let mut tc = tok_or_com.into_iter().peekable();
        let mut ln = lines.iter().enumerate().peekable();
        loop {
            let take_line = match (tc.peek(), ln.peek()) {
                (None, None) => break,
                (Some(_), None) => false,
                (None, Some(_)) => true,
                (Some((tc_line, _)), Some((_, line))) => *tc_line > line.number,
            };
            if take_line {
                if let Some((i, _)) = ln.next() {
                    elems.push(Elem::Line(i));
                }
            } else if let Some((_, e)) = tc.next() {
                elems.push(e);
            }
        }

        Self {
            buffer,
            tokens,
            comments,
            lines,
            elems,
        }
    }

    pub const fn bytes(&self) -> &'a [u8] {
        self.buffer.as_bytes()
    }

    /// The comment's text, from its `#` to the end of its line.
    pub fn comment_text(&self, comment: &Comment) -> &'a str {
        let rest = &self.buffer[comment.pointer..];
        match rest.find('\n') {
            Some(end) => &rest[..end],
            None => rest,
        }
    }

    /// Mirrors `Comment.is_inline`: the comment follows content on its line.
    pub fn comment_is_inline(&self, comment: &Comment) -> bool {
        let before = &self.tokens[comment.token_before];
        if matches!(before.kind, Kind::StreamStart) {
            return false;
        }
        if comment.line_no != before.end.line + 1 {
            return false;
        }
        // Sometimes token end marks are on the next line. Python indexes
        // `buffer[pointer - 1]`, which for pointer 0 is the buffer's last
        // byte; mirror that so the two agree on every input.
        let bytes = self.bytes();
        let at = if before.end.index == 0 {
            bytes.len()
        } else {
            before.end.index
        };
        at > 0 && bytes[at - 1] != b'\n'
    }
}

/// Mirrors `comments_between_tokens`: every `#` comment between token `i` and
/// the token after it (or the end of the buffer).
fn comments_between_tokens(buffer: &str, tokens: &[Token], i: usize, out: &mut Vec<Comment>) {
    let before = &tokens[i];
    let after = tokens.get(i + 1);
    let (from, to) = match after {
        None => (before.end.index, buffer.len()),
        Some(after) => {
            if before.end.line == after.start.line
                && !matches!(before.kind, Kind::StreamStart)
                && !matches!(after.kind, Kind::StreamEnd)
            {
                return;
            }
            (before.end.index, after.start.index)
        }
    };
    if from > to {
        return;
    }
    let buf = &buffer[from..to];

    let first_line_no = before.end.line + 1;
    let mut column_no = before.end.column + 1;
    let mut pointer = before.end.index;
    let mut previous = None;
    for (offset, line) in buf.split('\n').enumerate() {
        if let Some(pos) = line.find('#') {
            out.push(Comment {
                line_no: first_line_no + offset,
                column_no: column_no + pos,
                pointer: pointer + pos,
                token_before: i,
                token_after: after.map(|_| i + 1),
                previous,
            });
            previous = Some(out.len() - 1);
        }
        pointer += line.len() + 1;
        column_no = 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_split_on_newline_and_crlf() {
        let got = lines("a\nbb\r\n\nc");
        let spans: Vec<(usize, usize, usize)> =
            got.iter().map(|l| (l.number, l.start, l.end)).collect();
        assert_eq!(spans, vec![(1, 0, 1), (2, 2, 4), (3, 6, 6), (4, 7, 8)]);
    }

    #[test]
    fn tokens_carry_both_marks() {
        let tokens = scan_tokens("key: value\n");
        let kinds: Vec<&Kind> = tokens.iter().map(|t| &t.kind).collect();
        assert!(matches!(kinds[0], Kind::StreamStart));
        assert!(matches!(kinds[1], Kind::BlockMappingStart));
        assert!(matches!(kinds[2], Kind::Key));
        assert_eq!(tokens[3].scalar_value(), Some("key"));
        assert_eq!((tokens[3].start.index, tokens[3].end.index), (0, 3));
        assert!(matches!(kinds[4], Kind::Value));
        assert_eq!(tokens[4].start.column, 3);
        assert_eq!(tokens[5].scalar_value(), Some("value"));
        assert!(matches!(kinds.last(), Some(Kind::StreamEnd)));
    }

    #[test]
    fn comments_are_found_between_tokens() {
        let stream = Stream::new("# top\nkey: value  # inline\n  # block\nother: 1\n");
        let texts: Vec<&str> = stream
            .comments
            .iter()
            .map(|c| stream.comment_text(c))
            .collect();
        assert_eq!(texts, vec!["# top", "# inline", "# block"]);
        assert_eq!(stream.comments[0].line_no, 1);
        assert_eq!(stream.comments[0].column_no, 1);
        assert!(!stream.comment_is_inline(&stream.comments[0]));
        assert!(stream.comment_is_inline(&stream.comments[1]));
        assert_eq!(stream.comments[1].column_no, 13);
        assert!(!stream.comment_is_inline(&stream.comments[2]));
        assert_eq!(stream.comments[2].column_no, 3);
    }

    #[test]
    fn elements_interleave_lines_after_their_tokens() {
        let stream = Stream::new("a: 1\nb: 2\n");
        // Every Line comes after the tokens and comments of that line.
        let mut seen_line = 0;
        for elem in &stream.elems {
            match elem {
                Elem::Line(i) => seen_line = stream.lines[*i].number,
                Elem::Token(i) => assert!(
                    stream.tokens[*i].start.line + 1 > seen_line
                        || stream.tokens[*i].kind == Kind::StreamEnd
                ),
                Elem::Comment(i) => assert!(stream.comments[*i].line_no > seen_line),
            }
        }
    }

    #[test]
    fn scanner_error_truncates_the_stream() {
        // An unterminated quoted scalar is a scanner error (an unclosed `[`
        // is only a parser error: the scanner tokenizes it fine).
        let tokens = scan_tokens("key: \"unterminated\n");
        assert!(!tokens.iter().any(|t| matches!(t.kind, Kind::StreamEnd)));
    }
}

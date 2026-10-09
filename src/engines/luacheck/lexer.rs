//! luacheck's lexer: Lua 5.1-5.4 and `LuaJIT` tokens over decoded characters.
//!
//! Offsets are 1-based character indexes. `line_offsets[line]` is the offset
//! of a line's first character and `line_lengths[line]` its length in
//! characters, filled in as the lexer crosses line ends (both are indexed by
//! the 1-based line number; index 0 is unused).

use super::decoder::Chars;

/// A token type. `Sym` covers keywords and operators, and the single
/// characters the lexer passes through unrecognized.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Name,
    String,
    Number,
    Eof,
    ShortComment,
    LongComment,
    Sym(Sym),
}

/// Keyword or operator text, at most 8 bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sym {
    bytes: [u8; 8],
    len: u8,
}

impl Sym {
    pub const fn new(text: &[u8]) -> Self {
        let mut bytes = [0u8; 8];
        let mut i = 0;
        while i < text.len() {
            bytes[i] = text[i];
            i += 1;
        }
        Self {
            bytes,
            len: text.len() as u8,
        }
    }

    pub fn text(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// The token for keyword or operator text.
pub const fn sym(text: &str) -> Tok {
    Tok::Sym(Sym::new(text.as_bytes()))
}

impl Tok {
    /// The token as the parser's error messages name it.
    pub fn name(self) -> Vec<u8> {
        match self {
            Self::Name => b"identifier".to_vec(),
            Self::Eof => b"<eof>".to_vec(),
            other => {
                let mut out = vec![b'\''];
                out.extend_from_slice(&other.text());
                out.push(b'\'');
                out
            }
        }
    }

    /// The token's text as luacheck's token strings spell it.
    pub fn text(self) -> Vec<u8> {
        match self {
            Self::Name => b"name".to_vec(),
            Self::String => b"string".to_vec(),
            Self::Number => b"number".to_vec(),
            Self::Eof => b"eof".to_vec(),
            Self::ShortComment => b"short_comment".to_vec(),
            Self::LongComment => b"long_comment".to_vec(),
            Self::Sym(s) => s.text().to_vec(),
        }
    }
}

const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

pub struct Lexer<'a> {
    pub src: &'a Chars,
    pub line: usize,
    pub line_offsets: Vec<Option<usize>>,
    pub line_lengths: Vec<Option<usize>>,
    pub offset: usize,
}

/// A lexed token: type, value, start line and offset.
pub struct Token {
    pub tok: Tok,
    pub value: Option<Vec<u8>>,
    pub line: usize,
    pub offset: usize,
}

/// A lexing error: message, line, offset and end offset.
pub struct LexError {
    pub msg: Vec<u8>,
    pub line: usize,
    pub offset: usize,
    pub end_offset: usize,
}

const fn to_hex(b: u32) -> Option<u32> {
    match b {
        0x30..=0x39 => Some(b - 0x30),
        0x61..=0x66 => Some(10 + b - 0x61),
        0x41..=0x46 => Some(10 + b - 0x41),
        _ => None,
    }
}

const fn to_dec(b: u32) -> Option<u32> {
    match b {
        0x30..=0x39 => Some(b - 0x30),
        _ => None,
    }
}

fn to_utf(codepoint: u32) -> Vec<u8> {
    if codepoint < 0x80 {
        return vec![codepoint as u8];
    }
    let mut buf = Vec::new();
    let mut codepoint = codepoint;
    let mut mfb = 0x3Fu32;
    loop {
        buf.push((codepoint % 0x40 + 0x80) as u8);
        codepoint /= 0x40;
        mfb /= 2;
        if codepoint <= mfb {
            break;
        }
    }
    buf.push((0xFE - mfb * 2 + codepoint) as u8);
    buf.reverse();
    buf
}

const fn is_alpha(b: u32) -> bool {
    (b >= 0x61 && b <= 0x7A) || (b >= 0x41 && b <= 0x5A) || b == 0x5F
}

const fn is_newline(b: Option<u32>) -> bool {
    matches!(b, Some(0x0A | 0x0D))
}

const fn is_space(b: Option<u32>) -> bool {
    matches!(b, Some(0x20 | 0x0C | 0x09 | 0x0B))
}

const fn simple_escape(b: u32) -> Option<u8> {
    Some(match b {
        0x61 => 0x07,
        0x62 => 0x08,
        0x66 => 0x0C,
        0x6E => b'\n',
        0x72 => b'\r',
        0x74 => b'\t',
        0x76 => 0x0B,
        0x5C => b'\\',
        0x27 => b'\'',
        0x22 => b'"',
        _ => return None,
    })
}

/// What a token handler returns: a token and its value, or an error message
/// with an optional start offset relative to the current offset.
enum Lexed {
    Token(Tok, Option<Vec<u8>>),
    Error(&'static str, Option<i64>),
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a Chars) -> Self {
        let mut lexer = Self {
            src,
            line: 1,
            line_offsets: vec![None, Some(1)],
            line_lengths: vec![None, None],
            offset: 1,
        };
        if src.get_length() >= 2 && src.get_substring(1, 2) == b"#!" {
            lexer.offset = 2;
            let b = lexer.next_byte();
            lexer.skip_to_newline(b);
        }
        lexer
    }

    fn cp(&self, offset: usize) -> Option<u32> {
        self.src.get_codepoint(offset)
    }

    fn next_byte(&mut self) -> Option<u32> {
        self.offset += 1;
        self.cp(self.offset)
    }

    fn set_line_offset(&mut self, line: usize, offset: usize) {
        if self.line_offsets.len() <= line {
            self.line_offsets.resize(line + 1, None);
        }
        self.line_offsets[line] = Some(offset);
    }

    pub fn set_line_length(&mut self, line: usize, length: usize) {
        if self.line_lengths.len() <= line {
            self.line_lengths.resize(line + 1, None);
        }
        self.line_lengths[line] = Some(length);
    }

    pub fn line_offset(&self, line: usize) -> usize {
        self.line_offsets[line].expect("line offset known")
    }

    pub fn line_length(&self, line: usize) -> Option<usize> {
        self.line_lengths.get(line).copied().flatten()
    }

    fn skip_newline(&mut self, newline: Option<u32>) -> Option<u32> {
        let first_newline_offset = self.offset;
        let mut b = self.next_byte();
        if b != newline && is_newline(b) {
            b = self.next_byte();
        }
        let line = self.line;
        let length = first_newline_offset - self.line_offset(line);
        self.set_line_length(line, length);
        self.line = line + 1;
        self.set_line_offset(self.line, self.offset);
        b
    }

    fn skip_to_newline(&mut self, mut b: Option<u32>) -> Option<u32> {
        while !is_newline(b) && b.is_some() {
            b = self.next_byte();
        }
        b
    }

    fn skip_space(&mut self, mut b: Option<u32>) -> Option<u32> {
        while is_space(b) || is_newline(b) {
            if is_newline(b) {
                b = self.skip_newline(b);
            } else {
                b = self.next_byte();
            }
        }
        b
    }

    /// Skips `[=*` or `]=*`; returns the next character and the number of `=`.
    fn skip_long_bracket(&mut self) -> (Option<u32>, usize) {
        let start = self.offset;
        let mut b = self.next_byte();
        while b == Some(0x3D) {
            b = self.next_byte();
        }
        (b, self.offset - start - 1)
    }

    fn lex_long_string(&mut self, opening_long_bracket: usize, is_string: bool) -> Lexed {
        let mut b = self.next_byte();
        if is_newline(b) {
            b = self.skip_newline(b);
        }
        let mut lines: Vec<Vec<u8>> = Vec::new();
        let mut line_start = self.offset;
        loop {
            if is_newline(b) {
                lines.push(
                    self.src
                        .get_substring(line_start as i64, self.offset as i64 - 1),
                );
                b = self.skip_newline(b);
                line_start = self.offset;
            } else if b == Some(0x5D) {
                let (next, long_bracket) = self.skip_long_bracket();
                b = next;
                if b == Some(0x5D) && long_bracket == opening_long_bracket {
                    break;
                }
            } else if b.is_none() {
                return Lexed::Error(
                    if is_string {
                        "unfinished long string"
                    } else {
                        "unfinished long comment"
                    },
                    None,
                );
            } else {
                b = self.next_byte();
            }
        }
        lines.push(self.src.get_substring(
            line_start as i64,
            self.offset as i64 - opening_long_bracket as i64 - 2,
        ));
        self.offset += 1;
        Lexed::Token(
            if is_string {
                Tok::String
            } else {
                Tok::LongComment
            },
            Some(lines.join(&b'\n')),
        )
    }

    fn lex_short_string(&mut self, quote: u32) -> Lexed {
        let mut b = self.next_byte();
        let mut chunks: Option<Vec<Vec<u8>>> = None;
        let mut chunk_start = self.offset;

        while b != Some(quote) {
            if b == Some(0x5C) {
                let chunks_vec = chunks.get_or_insert_with(Vec::new);
                if chunk_start != self.offset {
                    chunks_vec.push(
                        self.src
                            .get_substring(chunk_start as i64, self.offset as i64 - 1),
                    );
                }
                b = self.next_byte();
                let mut s: Option<Vec<u8>> = None;
                let escape_byte = b.and_then(simple_escape);
                if let Some(escape_byte) = escape_byte {
                    b = self.next_byte();
                    s = Some(vec![escape_byte]);
                } else if is_newline(b) {
                    b = self.skip_newline(b);
                    s = Some(b"\n".to_vec());
                } else if b == Some(0x78) {
                    b = self.next_byte();
                    let Some(c1) = b.and_then(to_hex) else {
                        return Lexed::Error("invalid hexadecimal escape sequence", Some(-2));
                    };
                    b = self.next_byte();
                    let Some(c2) = b.and_then(to_hex) else {
                        return Lexed::Error("invalid hexadecimal escape sequence", Some(-3));
                    };
                    b = self.next_byte();
                    s = Some(vec![(c1 * 16 + c2) as u8]);
                } else if b == Some(0x75) {
                    b = self.next_byte();
                    if b != Some(0x7B) {
                        return Lexed::Error("invalid UTF-8 escape sequence", Some(-2));
                    }
                    b = self.next_byte();
                    let Some(mut codepoint) = b.and_then(to_hex) else {
                        return Lexed::Error("invalid UTF-8 escape sequence", Some(-3));
                    };
                    let mut hexdigits: i64 = 0;
                    loop {
                        b = self.next_byte();
                        match b.and_then(to_hex) {
                            Some(hex) => {
                                hexdigits += 1;
                                let next = u64::from(codepoint) * 16 + u64::from(hex);
                                if next > 0x7FFF_FFFF {
                                    return Lexed::Error(
                                        "invalid UTF-8 escape sequence",
                                        Some(-hexdigits - 3),
                                    );
                                }
                                codepoint = next as u32;
                            }
                            None => break,
                        }
                    }
                    if b != Some(0x7D) {
                        return Lexed::Error("invalid UTF-8 escape sequence", Some(-hexdigits - 4));
                    }
                    b = self.next_byte();
                    s = Some(to_utf(codepoint));
                } else if b == Some(0x7A) {
                    let next = self.next_byte();
                    b = self.skip_space(next);
                } else {
                    let Some(mut cb) = b.and_then(to_dec) else {
                        return Lexed::Error("invalid escape sequence", Some(-1));
                    };
                    b = self.next_byte();
                    if let Some(c2) = b.and_then(to_dec) {
                        cb = 10 * cb + c2;
                        b = self.next_byte();
                        if let Some(c3) = b.and_then(to_dec) {
                            cb = 10 * cb + c3;
                            if cb > 255 {
                                return Lexed::Error("invalid decimal escape sequence", Some(-3));
                            }
                            b = self.next_byte();
                        }
                    }
                    s = Some(vec![cb as u8]);
                }
                if let Some(s) = s {
                    chunks.get_or_insert_with(Vec::new).push(s);
                }
                chunk_start = self.offset;
            } else if b.is_none() || is_newline(b) {
                return Lexed::Error("unfinished string", None);
            } else {
                b = self.next_byte();
            }
        }

        let string_value = match chunks {
            Some(mut chunks) => {
                if chunk_start != self.offset {
                    chunks.push(
                        self.src
                            .get_substring(chunk_start as i64, self.offset as i64 - 1),
                    );
                }
                chunks.concat()
            }
            None => self
                .src
                .get_substring(chunk_start as i64, self.offset as i64 - 1),
        };
        self.offset += 1;
        Lexed::Token(Tok::String, Some(string_value))
    }

    fn lex_number(&mut self, mut b: Option<u32>) -> Lexed {
        let start = self.offset;
        let (mut exp_lower, mut exp_upper) = (0x65, 0x45);
        let mut is_digit: fn(u32) -> Option<u32> = to_dec;
        let mut has_digits = false;
        let mut is_float = false;

        if b == Some(0x30) {
            b = self.next_byte();
            if b == Some(0x78) || b == Some(0x58) {
                exp_lower = 0x70;
                exp_upper = 0x50;
                is_digit = to_hex;
                b = self.next_byte();
            } else {
                has_digits = true;
            }
        }
        while b.is_some_and(|c| is_digit(c).is_some()) {
            b = self.next_byte();
            has_digits = true;
        }
        if b == Some(0x2E) {
            is_float = true;
            b = self.next_byte();
            while b.is_some_and(|c| is_digit(c).is_some()) {
                b = self.next_byte();
                has_digits = true;
            }
        }
        if b == Some(exp_lower) || b == Some(exp_upper) {
            is_float = true;
            b = self.next_byte();
            if b == Some(0x2B) || b == Some(0x2D) {
                b = self.next_byte();
            }
            if b.is_none_or(|c| to_dec(c).is_none()) {
                return Lexed::Error("malformed number", None);
            }
            loop {
                b = self.next_byte();
                if b.is_none_or(|c| to_dec(c).is_none()) {
                    break;
                }
            }
        }
        if !has_digits {
            return Lexed::Error("malformed number", None);
        }
        let is_l = |c: Option<u32>| c == Some(0x6C) || c == Some(0x4C);
        let is_u = |c: Option<u32>| c == Some(0x75) || c == Some(0x55);
        if b == Some(0x69) || b == Some(0x49) {
            self.offset += 1;
        } else if !is_float {
            if is_u(b) {
                let b1 = self.cp(self.offset + 1);
                if is_l(b1) {
                    let b2 = self.cp(self.offset + 2);
                    if is_l(b2) {
                        self.offset += 3;
                    }
                }
            } else if is_l(b) {
                let b1 = self.cp(self.offset + 1);
                if is_l(b1) {
                    let b2 = self.cp(self.offset + 2);
                    if is_u(b2) {
                        self.offset += 3;
                    } else {
                        self.offset += 2;
                    }
                }
            }
        }
        Lexed::Token(
            Tok::Number,
            Some(self.src.get_substring(start as i64, self.offset as i64 - 1)),
        )
    }

    fn lex_ident(&mut self) -> Lexed {
        let start = self.offset;
        let mut b = self.next_byte();
        while b.is_some_and(|c| is_alpha(c) || to_dec(c).is_some()) {
            b = self.next_byte();
        }
        let ident = self.src.get_substring(start as i64, self.offset as i64 - 1);
        match KEYWORDS.iter().find(|k| k.as_bytes() == ident.as_slice()) {
            Some(k) => Lexed::Token(sym(k), None),
            None => Lexed::Token(Tok::Name, Some(ident)),
        }
    }

    fn lex_dash(&mut self) -> Lexed {
        let mut b = self.next_byte();
        if b != Some(0x2D) {
            return Lexed::Token(sym("-"), None);
        }
        b = self.next_byte();
        let start = self.offset;
        if b == Some(0x5B) {
            let (next, long_bracket) = self.skip_long_bracket();
            b = next;
            if b == Some(0x5B) {
                return self.lex_long_string(long_bracket, false);
            }
        }
        self.skip_to_newline(b);
        let comment_value = self.src.get_substring(start as i64, self.offset as i64 - 1);
        Lexed::Token(Tok::ShortComment, Some(comment_value))
    }

    fn lex_bracket(&mut self) -> Lexed {
        let (b, long_bracket) = self.skip_long_bracket();
        if b == Some(0x5B) {
            self.lex_long_string(long_bracket, true)
        } else if long_bracket == 0 {
            Lexed::Token(sym("["), None)
        } else {
            Lexed::Error("invalid long string delimiter", None)
        }
    }

    /// A one-character token, or a two-character one when `second` follows.
    fn lex_pair(&mut self, second: &[(u32, &'static str)], single: &'static str) -> Lexed {
        let b = self.next_byte();
        for &(c, token) in second {
            if b == Some(c) {
                self.offset += 1;
                return Lexed::Token(sym(token), None);
            }
        }
        Lexed::Token(sym(single), None)
    }

    fn lex_dot(&mut self) -> Lexed {
        let b = self.next_byte();
        if b == Some(0x2E) {
            let b = self.next_byte();
            if b == Some(0x2E) {
                self.offset += 1;
                Lexed::Token(sym("..."), Some(b"...".to_vec()))
            } else {
                Lexed::Token(sym(".."), None)
            }
        } else if b.is_some_and(|c| to_dec(c).is_some()) {
            self.offset -= 2;
            let b = self.next_byte();
            self.lex_number(b)
        } else {
            Lexed::Token(sym("."), None)
        }
    }

    fn lex_any(&mut self, b: u32) -> Lexed {
        self.offset += 1;
        let b = b.min(255) as u8;
        Lexed::Token(Tok::Sym(Sym::new(&[b])), None)
    }

    fn dispatch(&mut self, b: u32) -> Lexed {
        match b {
            0x2E => self.lex_dot(),
            0x3A => self.lex_pair(&[(0x3A, "::")], ":"),
            0x5B => self.lex_bracket(),
            0x27 | 0x22 => self.lex_short_string(b),
            0x2D => self.lex_dash(),
            0x2F => self.lex_pair(&[(0x2F, "//")], "/"),
            0x3D => self.lex_pair(&[(0x3D, "==")], "="),
            0x7E => self.lex_pair(&[(0x3D, "~=")], "~"),
            0x3C => self.lex_pair(&[(0x3D, "<="), (0x3C, "<<")], "<"),
            0x3E => self.lex_pair(&[(0x3D, ">="), (0x3E, ">>")], ">"),
            0x30..=0x39 => self.lex_number(Some(b)),
            _ if is_alpha(b) => self.lex_ident(),
            _ => self.lex_any(b),
        }
    }

    /// `lexer.get_quoted_substring_or_line`.
    pub fn quoted_substring_or_line(
        &self,
        line: usize,
        offset: usize,
        end_offset: usize,
    ) -> Vec<u8> {
        let mut end_offset = end_offset as i64;
        if let Some(line_length) = self.line_length(line) {
            let line_end_offset = (self.line_offset(line) + line_length) as i64 - 1;
            if line_end_offset < end_offset {
                end_offset = line_end_offset;
            }
        }
        let mut out = vec![b'\''];
        out.extend(self.src.get_printable_substring(offset as i64, end_offset));
        out.push(b'\'');
        out
    }

    /// The next token, filling line offsets and lengths on the way.
    pub fn next_token(&mut self) -> Result<Token, LexError> {
        let first = self.cp(self.offset);
        let b = self.skip_space(first);
        let token_line = self.line;
        let line_offset = self.line_offset(token_line);
        let token_offset = self.offset;

        let Some(b) = b else {
            self.offset += 1;
            self.set_line_length(token_line, token_offset - line_offset);
            return Ok(Token {
                tok: Tok::Eof,
                value: None,
                line: token_line,
                offset: token_offset,
            });
        };

        match self.dispatch(b) {
            Lexed::Token(tok, value) => Ok(Token {
                tok,
                value,
                line: token_line,
                offset: token_offset,
            }),
            Lexed::Error(msg, Some(relative)) => {
                let error_offset = (self.offset as i64 + relative) as usize;
                let error_end_offset = self.offset.min(self.src.get_length());
                let mut message = msg.as_bytes().to_vec();
                message.push(b' ');
                message.extend(self.quoted_substring_or_line(
                    self.line,
                    error_offset,
                    error_end_offset,
                ));
                Err(LexError {
                    msg: message,
                    line: self.line,
                    offset: error_offset,
                    end_offset: error_end_offset,
                })
            }
            Lexed::Error(msg, None) => Err(LexError {
                msg: msg.as_bytes().to_vec(),
                line: token_line,
                offset: token_offset,
                end_offset: token_offset,
            }),
        }
    }
}

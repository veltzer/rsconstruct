//! The expression lexer, a port of actionlint's `expr_lexer.go`. Positions
//! follow Go's `text/scanner`: 1-based line and column (in characters) of the
//! next unread character, and its byte offset.

use super::ExprError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    End,
    Ident,
    String,
    Int,
    Float,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Dot,
    Not,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    Eq,
    NotEq,
    And,
    Or,
    Star,
    Comma,
}

impl TokenKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::End => "END",
            Self::Ident => "IDENT",
            Self::String => "STRING",
            Self::Int => "INTEGER",
            Self::Float => "FLOAT",
            Self::LeftParen => "(",
            Self::RightParen => ")",
            Self::LeftBracket => "[",
            Self::RightBracket => "]",
            Self::Dot => ".",
            Self::Not => "!",
            Self::Less => "<",
            Self::LessEq => "<=",
            Self::Greater => ">",
            Self::GreaterEq => ">=",
            Self::Eq => "==",
            Self::NotEq => "!=",
            Self::And => "&&",
            Self::Or => "||",
            Self::Star => "*",
            Self::Comma => ",",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub value: String,
    pub offset: usize,
    pub line: u32,
    pub column: u32,
}

const EXPECTED_PUNCT_CHARS: &str =
    "''', '}', '(', ')', '[', ']', '.', '!', '<', '>', '=', '&', '|', '*', ',', ' '";
const EXPECTED_DIGIT_CHARS: &str = "'0'..'9'";
const EXPECTED_ALPHA_CHARS: &str = "'a'..'z', 'A'..'Z', '_'";

fn expected_all_chars() -> String {
    format!("{EXPECTED_ALPHA_CHARS}, {EXPECTED_DIGIT_CHARS}, {EXPECTED_PUNCT_CHARS}")
}

const fn is_whitespace(r: char) -> bool {
    matches!(r, ' ' | '\n' | '\r' | '\t')
}

const fn is_alpha(r: char) -> bool {
    r.is_ascii_alphabetic()
}

const fn is_num(r: char) -> bool {
    r.is_ascii_digit()
}

const fn is_hex_num(r: char) -> bool {
    r.is_ascii_hexdigit()
}

const fn is_alnum(r: char) -> bool {
    is_alpha(r) || is_num(r)
}

#[derive(Debug, Clone, Copy)]
struct Position {
    offset: usize,
    line: u32,
    column: u32,
}

pub struct Lexer {
    chars: Vec<(usize, char)>,
    src_len: usize,
    /// Index of the next unread character.
    idx: usize,
    line: u32,
    column: u32,
    /// Characters on the previous line, newline included (text/scanner's
    /// `lastLineLen`), for the position quirk at end of input after a
    /// newline.
    last_line_len: u32,
    last_was_newline: bool,
    err: Option<ExprError>,
    start: Position,
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Self {
            chars: src.char_indices().collect(),
            src_len: src.len(),
            idx: 0,
            line: 1,
            column: 1,
            last_line_len: 0,
            last_was_newline: false,
            err: None,
            start: Position {
                offset: 0,
                line: 1,
                column: 1,
            },
        }
    }

    /// text/scanner's `Pos()`: the position of the next unread character.
    fn pos(&self) -> Position {
        let offset = self.chars.get(self.idx).map_or(self.src_len, |c| c.0);
        if self.idx >= self.chars.len() && self.last_was_newline {
            return Position {
                offset,
                line: self.line - 1,
                column: self.last_line_len,
            };
        }
        Position {
            offset,
            line: self.line,
            column: self.column,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.idx).map(|c| c.1)
    }

    /// Consumes the next character.
    fn advance(&mut self) {
        if let Some(&(_, c)) = self.chars.get(self.idx) {
            self.idx += 1;
            if c == '\n' {
                self.last_line_len = self.column;
                self.line += 1;
                self.column = 1;
                self.last_was_newline = true;
            } else {
                self.column += 1;
                self.last_was_newline = false;
            }
        }
    }

    /// `eat`: consume the current character and return the one after it.
    fn eat(&mut self) -> Option<char> {
        self.advance();
        self.peek()
    }

    fn error(&mut self, message: String) {
        if self.err.is_none() {
            let p = self.pos();
            self.err = Some(ExprError {
                message,
                offset: p.offset,
                line: p.line,
                column: p.column,
            });
        }
    }

    fn token(&mut self, kind: TokenKind) -> Token {
        let p = self.pos();
        let s = self.start;
        let value: String = self.chars[..]
            .iter()
            .filter(|(o, _)| *o >= s.offset && *o < p.offset)
            .map(|c| c.1)
            .collect();
        let t = Token {
            kind,
            value,
            offset: s.offset,
            line: s.line,
            column: s.column,
        };
        self.start = p;
        t
    }

    const fn eof(&self) -> Token {
        Token {
            kind: TokenKind::End,
            value: String::new(),
            offset: self.start.offset,
            line: self.start.line,
            column: self.start.column,
        }
    }

    fn skip_white(&mut self) {
        while let Some(r) = self.peek() {
            if !is_whitespace(r) {
                return;
            }
            self.advance();
            self.start = self.pos();
        }
    }

    fn unexpected(&mut self, r: Option<char>, where_: &str, expected: &str) -> Token {
        let what = match r {
            None => "EOF".to_string(),
            Some(c) => format!("character {}", go_quote_rune(c)),
        };
        let note = if r == Some('"') {
            ". do you mean string literals? only single quotes are available for string delimiter"
        } else {
            ""
        };
        let msg =
            format!("got unexpected {what} while lexing {where_}, expecting {expected}{note}");
        self.error(msg);
        self.eof()
    }

    fn unexpected_eof(&mut self) -> Token {
        self.error("unexpected EOF while lexing expression".to_string());
        self.eof()
    }

    fn lex_ident(&mut self) -> Token {
        loop {
            match self.eat() {
                Some(r) if is_alnum(r) || r == '_' || r == '-' => {}
                _ => return self.token(TokenKind::Ident),
            }
        }
    }

    fn lex_num(&mut self) -> Token {
        let mut r = self.peek();
        if r == Some('-') {
            r = self.eat();
        }
        if r == Some('0') {
            r = self.eat();
            if r == Some('x') {
                self.advance();
                return self.lex_hex_int();
            }
        } else {
            if !r.is_some_and(is_num) {
                return self.unexpected(r, "integer part of number", EXPECTED_DIGIT_CHARS);
            }
            loop {
                r = self.eat();
                if !r.is_some_and(is_num) {
                    break;
                }
            }
        }

        let has_fraction = if r == Some('.') {
            r = self.eat();
            if !r.is_some_and(is_num) {
                return self.unexpected(r, "fraction part of float number", EXPECTED_DIGIT_CHARS);
            }
            loop {
                r = self.eat();
                if !r.is_some_and(is_num) {
                    break;
                }
            }
            true
        } else {
            false
        };

        let has_exponent = if r == Some('e') || r == Some('E') {
            r = self.eat();
            if r == Some('-') {
                r = self.eat();
            }
            if r == Some('0') {
                r = self.eat();
            } else {
                if !r.is_some_and(is_num) {
                    return self.unexpected(
                        r,
                        "exponent part of float number",
                        EXPECTED_DIGIT_CHARS,
                    );
                }
                loop {
                    r = self.eat();
                    if !r.is_some_and(is_num) {
                        break;
                    }
                }
            }
            true
        } else {
            false
        };

        let k = if has_fraction || has_exponent {
            TokenKind::Float
        } else {
            TokenKind::Int
        };

        if r.is_some_and(is_alnum) {
            let s = self.current_text();
            return self.unexpected(
                r,
                &format!("character following number {s}"),
                EXPECTED_PUNCT_CHARS,
            );
        }

        self.token(k)
    }

    /// The text from the token start to the current position.
    fn current_text(&self) -> String {
        let p = self.pos();
        self.chars
            .iter()
            .filter(|(o, _)| *o >= self.start.offset && *o < p.offset)
            .map(|c| c.1)
            .collect()
    }

    fn lex_hex_int(&mut self) -> Token {
        let mut r = self.peek();
        if r == Some('0') {
            r = self.eat();
        } else {
            if !r.is_some_and(is_hex_num) {
                let e = format!("{EXPECTED_DIGIT_CHARS}, 'a'..'f', 'A'..'F'");
                return self.unexpected(r, "hex integer", &e);
            }
            loop {
                r = self.eat();
                if !r.is_some_and(is_hex_num) {
                    break;
                }
            }
        }
        if r.is_some_and(is_alnum) {
            let s = self.current_text();
            return self.unexpected(
                r,
                &format!("character following hex integer {s}"),
                EXPECTED_PUNCT_CHARS,
            );
        }
        self.token(TokenKind::Int)
    }

    fn lex_string(&mut self) -> Token {
        loop {
            match self.eat() {
                Some('\'') if self.eat() != Some('\'') => {
                    return self.token(TokenKind::String);
                }
                None => return self.unexpected(None, "end of string literal", "'''"),
                _ => {}
            }
        }
    }

    fn lex_end(&mut self) -> Token {
        let r = self.eat();
        if r != Some('}') {
            return self.unexpected(r, "end marker }}", "'}'");
        }
        self.advance();
        self.token(TokenKind::End)
    }

    fn lex_less(&mut self) -> Token {
        let mut k = TokenKind::Less;
        if self.eat() == Some('=') {
            k = TokenKind::LessEq;
            self.advance();
        }
        self.token(k)
    }

    fn lex_greater(&mut self) -> Token {
        let mut k = TokenKind::Greater;
        if self.eat() == Some('=') {
            k = TokenKind::GreaterEq;
            self.advance();
        }
        self.token(k)
    }

    fn lex_eq(&mut self) -> Token {
        let r = self.eat();
        if r != Some('=') {
            return self.unexpected(r, "== operator", "'='");
        }
        self.advance();
        self.token(TokenKind::Eq)
    }

    fn lex_bang(&mut self) -> Token {
        let k = if self.eat() == Some('=') {
            self.advance();
            TokenKind::NotEq
        } else {
            TokenKind::Not
        };
        self.token(k)
    }

    fn lex_and(&mut self) -> Token {
        let r = self.eat();
        if r != Some('&') {
            return self.unexpected(r, "&& operator", "'&'");
        }
        self.advance();
        self.token(TokenKind::And)
    }

    fn lex_or(&mut self) -> Token {
        let r = self.eat();
        if r != Some('|') {
            return self.unexpected(r, "|| operator", "'|'");
        }
        self.advance();
        self.token(TokenKind::Or)
    }

    fn lex_char(&mut self, k: TokenKind) -> Token {
        self.advance();
        self.token(k)
    }

    /// Lexes the next token. After an error every call returns an `End`
    /// token; `err()` has the first error.
    pub fn next_token(&mut self) -> Token {
        self.skip_white();
        let Some(r) = self.peek() else {
            return self.unexpected_eof();
        };
        if is_alpha(r) || r == '_' {
            return self.lex_ident();
        }
        if is_num(r) || r == '-' {
            return self.lex_num();
        }
        match r {
            '\'' => self.lex_string(),
            '}' => self.lex_end(),
            '!' => self.lex_bang(),
            '<' => self.lex_less(),
            '>' => self.lex_greater(),
            '=' => self.lex_eq(),
            '&' => self.lex_and(),
            '|' => self.lex_or(),
            '(' => self.lex_char(TokenKind::LeftParen),
            ')' => self.lex_char(TokenKind::RightParen),
            '[' => self.lex_char(TokenKind::LeftBracket),
            ']' => self.lex_char(TokenKind::RightBracket),
            '.' => self.lex_char(TokenKind::Dot),
            '*' => self.lex_char(TokenKind::Star),
            ',' => self.lex_char(TokenKind::Comma),
            _ => {
                let expected = expected_all_chars();
                self.unexpected(Some(r), "expression", &expected)
            }
        }
    }

    /// The byte offset of the next unread character.
    pub fn offset(&self) -> usize {
        self.pos().offset
    }

    pub const fn err(&self) -> Option<&ExprError> {
        self.err.as_ref()
    }
}

/// Go's `strconv.QuoteRune`.
pub fn go_quote_rune(c: char) -> String {
    match c {
        '\'' => "'\\''".to_string(),
        '\\' => "'\\\\'".to_string(),
        '\n' => "'\\n'".to_string(),
        '\r' => "'\\r'".to_string(),
        '\t' => "'\\t'".to_string(),
        '\u{7}' => "'\\a'".to_string(),
        '\u{8}' => "'\\b'".to_string(),
        '\u{c}' => "'\\f'".to_string(),
        '\u{b}' => "'\\v'".to_string(),
        c if (c as u32) < 0x20 || c == '\u{7f}' => format!("'\\x{:02x}'", c as u32),
        c => format!("'{c}'"),
    }
}

//! checkstyle's Java lexer: the ANTLR 2.7.7 lexers generated from `java.g`
//! (`GeneratedJavaLexer`) and `java14textblocks.g` (`GeneratedTextBlockLexer`),
//! and the token plumbing `JavaParser.parse` puts between them and the
//! parser: a `TokenStreamHiddenTokenFilter` over the Java lexer that hides
//! comments, and a `TokenStreamSelector` that switches to the text block
//! lexer while a `"""` literal is open.
//!
//! The filter reads one token ahead of what it hands out, and the Java lexer
//! switches the selector the moment it lexes `"""`; so a text block reaches
//! the parser as its content, its closing `"""`, and only then its opening
//! `"""` (the grammar's `textBlock` rule matches them in that order). The two
//! lexers share one input position, line and column; the text block lexer
//! does not override ANTLR's tab handling, so inside a text block a tab
//! moves the column to the next multiple of eight, and the Java lexer
//! carries on from there.
//!
//! `tokenize` runs the selector until it returns end of file or fails, and
//! keeps what it returned: the parser asks for tokens in that same order,
//! and lexing does not depend on the parser, so a failure is raised when the
//! parser reaches it, as ANTLR's token buffer would.

use std::rc::Rc;

use super::contents::{Comments, FileText};
use super::tables::{
    ASSERT, ASSIGN, AT, BAND, BAND_ASSIGN, BLOCK_COMMENT_BEGIN, BNOT, BOR, BOR_ASSIGN, BSR,
    BSR_ASSIGN, BXOR, BXOR_ASSIGN, CHAR_LITERAL, CHARS_0, CHARS_1, CHARS_2, CHARS_3, CHARS_5,
    CHARS_6, CHARS_7, CHARS_8, CHARS_9, CHARS_10, CHARS_11, CHARS_12, CHARS_13, CHARS_14, CHARS_15,
    CHARS_16, COLON, COMMA, DEC, DIV, DIV_ASSIGN, DOT, DOUBLE_COLON, ELLIPSIS, ENUM, EQUAL, GE, GT,
    IDENT, INC, LAMBDA, LAND, LBRACK, LCURLY, LE, LITERALS, LNOT, LOR, LPAREN, LT, MINUS,
    MINUS_ASSIGN, MOD, MOD_ASSIGN, NOT_EQUAL, NUM_DOUBLE, NUM_FLOAT, NUM_INT, NUM_LONG, PLUS,
    PLUS_ASSIGN, QUESTION, RBRACK, RCURLY, RPAREN, SEMI, SINGLE_LINE_COMMENT, SL, SL_ASSIGN, SR,
    SR_ASSIGN, STAR, STAR_ASSIGN, STRING_LITERAL, TEXT_BLOCK_CONTENT, TEXT_BLOCK_LITERAL_BEGIN,
    TEXT_BLOCK_LITERAL_END,
};

/// ANTLR's end-of-input character: what the input buffer returns past the
/// end, `(char) -1`. A U+FFFF in the file reads the same.
pub const EOF_CHAR: u16 = 0xFFFF;

/// ANTLR's `Token.EOF_TYPE`.
pub const EOF_TYPE: u16 = 1;

/// A set of UTF-16 code units, as ranges (ANTLR's lexer `BitSet`s).
pub struct CharSet(pub &'static [(u16, u16)]);

impl CharSet {
    pub fn contains(&self, c: u16) -> bool {
        self.0.iter().any(|&(lo, hi)| lo <= c && c <= hi)
    }
}

/// A token as the parser sees it. `text` is `None` for end of file, whose
/// token ANTLR makes without text (its messages print it as `null`).
/// `col` is 1-based, as ANTLR counts.
#[derive(Clone, Debug)]
pub struct Token {
    pub ty: u16,
    pub text: Option<Rc<str>>,
    pub line: i32,
    pub col: i32,
}

/// What stops the token stream: a recognition error of either lexer (ANTLR
/// formats it as `file:line:col: message`), or a Java exception thrown by
/// the comment listener (`FileContents`), which aborts the file the same way.
#[derive(Clone, Debug)]
pub enum StreamError {
    Recognition {
        line: i32,
        col: i32,
        message: String,
    },
    Crash(String),
}

impl StreamError {
    const fn at(line: i32, col: i32, message: String) -> Self {
        Self::Recognition { line, col, message }
    }
}

type LexResult<T> = Result<T, StreamError>;

/// ANTLR's `appendCharName`: how a mismatch message names a character.
fn char_name(c: u16) -> String {
    match c {
        EOF_CHAR => "'<EOF>'".to_string(),
        0x0A => "'\\n'".to_string(),
        0x0D => "'\\r'".to_string(),
        0x09 => "'\\t'".to_string(),
        _ => format!("'{}'", String::from_utf16_lossy(&[c])),
    }
}

/// `NoViableAltForCharException.getMessage()`.
fn unexpected_char(c: u16) -> String {
    if (0x20..=0x7E).contains(&c) {
        format!("unexpected char: '{}'", char::from(c as u8))
    } else {
        format!("unexpected char: 0x{c:X}")
    }
}

const fn is_digit(c: u16) -> bool {
    c >= b'0' as u16 && c <= b'9' as u16
}

const fn is_oct(c: u16) -> bool {
    c >= b'0' as u16 && c <= b'7' as u16
}

const fn is_bin(c: u16) -> bool {
    c == b'0' as u16 || c == b'1' as u16
}

fn is_hex(c: u16) -> bool {
    CHARS_5.contains(c)
}

const fn is(c: u16, ascii: u8) -> bool {
    c == ascii as u16
}

fn one_of(c: u16, ascii: &[u8]) -> bool {
    ascii.iter().any(|&a| c == u16::from(a))
}

/// Java's `Character.isJavaIdentifierStart(char)`.
fn is_java_identifier_start(c: u16) -> bool {
    use unicode_general_category::GeneralCategory as G;
    let Some(ch) = char::from_u32(u32::from(c)) else {
        return false;
    };
    matches!(
        unicode_general_category::get_general_category(ch),
        G::UppercaseLetter
            | G::LowercaseLetter
            | G::TitlecaseLetter
            | G::ModifierLetter
            | G::OtherLetter
            | G::LetterNumber
            | G::CurrencySymbol
            | G::ConnectorPunctuation
    )
}

/// Java's `Character.isJavaIdentifierPart(char)`: a start character, a
/// digit, a combining mark, or an ignorable control or format character.
fn is_java_identifier_part(c: u16) -> bool {
    use unicode_general_category::GeneralCategory as G;
    if c <= 0x08 || (0x0E..=0x1B).contains(&c) || (0x7F..=0x9F).contains(&c) {
        return true;
    }
    let Some(ch) = char::from_u32(u32::from(c)) else {
        return false;
    };
    is_java_identifier_start(c)
        || matches!(
            unicode_general_category::get_general_category(ch),
            G::DecimalNumber | G::NonspacingMark | G::SpacingMark | G::Format
        )
}

/// A read-only cursor for the number literal rules. `NUM_INT` only ever
/// runs them inside a syntactic predicate first, where a failure just means
/// "not this alternative", so each is a recognizer: does it match here, and
/// where does it end.
struct Cur<'a> {
    input: &'a [u16],
    pos: usize,
}

impl Cur<'_> {
    fn la(&self, i: usize) -> u16 {
        self.input
            .get(self.pos + i - 1)
            .copied()
            .unwrap_or(EOF_CHAR)
    }

    const fn bump(&mut self) {
        self.pos += 1;
    }

    /// Consume one character satisfying `ok`, or fail.
    fn expect(&mut self, ok: impl Fn(u16) -> bool) -> bool {
        if ok(self.la(1)) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// The `((D|'_')* D)` tail of an underscored digit run: ANTLR's loop
    /// takes a digit only when a digit or `_` follows it, then one digit
    /// must close the run.
    fn tail(&mut self, digit: impl Fn(u16) -> bool) -> bool {
        loop {
            let (c1, c2) = (self.la(1), self.la(2));
            if digit(c1) && (digit(c2) || is(c2, b'_')) || is(c1, b'_') {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(digit)
    }

    /// After a run's first digit: its tail when a digit or `_` follows,
    /// nothing when `follow` does, failure otherwise.
    fn tail_or(&mut self, digit: impl Fn(u16) -> bool, follow: &[u8]) -> bool {
        let c = self.la(1);
        if digit(c) || is(c, b'_') {
            self.tail(digit)
        } else {
            one_of(c, follow)
        }
    }

    /// After a run's first digit: its tail when a digit or `_` follows.
    fn tail_opt(&mut self, digit: impl Fn(u16) -> bool) -> bool {
        let c = self.la(1);
        if digit(c) || is(c, b'_') {
            self.tail(digit)
        } else {
            true
        }
    }

    /// `SIGNED_INTEGER`.
    fn signed_integer(&mut self) -> bool {
        let c = self.la(1);
        if one_of(c, b"+-") {
            self.bump();
        } else if !is_digit(c) {
            return false;
        }
        self.expect(is_digit) && self.tail_opt(is_digit)
    }

    /// `EXPONENT` (`e`) or `BINARY_EXPONENT` (`p`).
    fn exponent(&mut self, letters: &[u8]) -> bool {
        self.expect(|c| one_of(c, letters)) && self.signed_integer()
    }

    fn optional(&mut self, letters: &[u8]) {
        if one_of(self.la(1), letters) {
            self.bump();
        }
    }

    /// The synpred `((D)((D|'_')*(D))?)? '.'` shared by the decimal
    /// floating literals.
    fn decimal_point_ahead(&self) -> bool {
        let mut c = Cur {
            input: self.input,
            pos: self.pos,
        };
        let first = c.la(1);
        if is_digit(first) {
            c.bump();
            if !c.tail_or(is_digit, b".") {
                return false;
            }
        } else if !is(first, b'.') {
            return false;
        }
        c.expect(|x| is(x, b'.'))
    }

    /// The same for hex digits.
    fn hex_point_ahead(&self) -> bool {
        let mut c = Cur {
            input: self.input,
            pos: self.pos,
        };
        let first = c.la(1);
        if is_hex(first) {
            c.bump();
            if !c.tail_or(is_hex, b".") {
                return false;
            }
        } else if !is(first, b'.') {
            return false;
        }
        c.expect(|x| is(x, b'.'))
    }

    /// The mantissa of `DOUBLE_LITERAL` and `FLOAT_LITERAL` when the
    /// decimal point synpred held; `exp` are the letters that may follow
    /// the digits after the point.
    fn decimal_with_point(&mut self, exp: &[u8]) -> bool {
        let first = self.la(1);
        if is_digit(first) {
            self.bump();
            if !self.tail_or(is_digit, b".") || !self.expect(|c| is(c, b'.')) {
                return false;
            }
            true
        } else if is(first, b'.') {
            self.bump();
            self.expect(is_digit) && self.tail_or(is_digit, exp)
        } else {
            false
        }
    }

    /// `DOUBLE_LITERAL`.
    fn double_literal(&mut self) -> bool {
        let (c1, c2, c3) = (self.la(1), self.la(2), self.la(3));
        if (is(c1, b'.') || is_digit(c1))
            && (one_of(c2, b"._") || is_digit(c2))
            && (one_of(c3, b"._DEde") || is_digit(c3))
            && self.decimal_point_ahead()
        {
            if is_digit(c1) {
                if !self.decimal_with_point(b"DEde") {
                    return false;
                }
                let c = self.la(1);
                if is_digit(c) {
                    self.bump();
                    if !self.tail_or(is_digit, b"DEde") {
                        return false;
                    }
                } else if !one_of(c, b"DEde") {
                    return false;
                }
            } else if !self.decimal_with_point(b"DEde") {
                return false;
            }
        } else if is_digit(c1) && (is_digit(c2) || one_of(c2, b"DE_de")) {
            self.bump();
            if !self.tail_or(is_digit, b"DEde") {
                return false;
            }
        } else {
            return false;
        }
        let c = self.la(1);
        if one_of(c, b"Ee") {
            if !self.exponent(b"Ee") {
                return false;
            }
        } else if !one_of(c, b"Dd") {
            return false;
        }
        self.expect(|c| one_of(c, b"dD"))
    }

    /// `FLOAT_LITERAL`.
    fn float_literal(&mut self) -> bool {
        let (c1, c2) = (self.la(1), self.la(2));
        if (is(c1, b'.') || is_digit(c1))
            && (one_of(c2, b"._") || is_digit(c2))
            && self.decimal_point_ahead()
        {
            if is_digit(c1) {
                if !self.decimal_with_point(b"") {
                    return false;
                }
                if is_digit(self.la(1)) {
                    self.bump();
                    if !self.tail_opt(is_digit) {
                        return false;
                    }
                }
            } else {
                self.bump();
                if !self.expect(is_digit) || !self.tail_opt(is_digit) {
                    return false;
                }
            }
            if one_of(self.la(1), b"Ee") && !self.exponent(b"Ee") {
                return false;
            }
            self.optional(b"fF");
            true
        } else if is_digit(c1) && (is_digit(c2) || one_of(c2, b"EF_ef")) {
            self.bump();
            if !self.tail_or(is_digit, b"EFef") {
                return false;
            }
            let c = self.la(1);
            if one_of(c, b"Ee") {
                if !self.exponent(b"Ee") {
                    return false;
                }
                self.optional(b"fF");
                true
            } else {
                self.expect(|c| one_of(c, b"fF"))
            }
        } else {
            false
        }
    }

    /// `HEX_DOUBLE_LITERAL` (`suffix` `dD`, required) and
    /// `HEX_FLOAT_LITERAL` (`suffix` `fF`, optional; its no-point branch
    /// looks at three characters, not four).
    fn hex_floating(&mut self, double: bool) -> bool {
        if !self.expect(|c| is(c, b'0')) || !self.expect(|c| one_of(c, b"xX")) {
            return false;
        }
        let (c1, c2, c3, c4) = (self.la(1), self.la(2), self.la(3), self.la(4));
        if CHARS_10.contains(c1)
            && CHARS_12.contains(c2)
            && CHARS_11.contains(c3)
            && CHARS_13.contains(c4)
            && self.hex_point_ahead()
        {
            // The first character is a hex digit or the point.
            self.bump();
            if is_hex(c1) {
                if !self.tail_or(is_hex, b".") || !self.expect(|c| is(c, b'.')) {
                    return false;
                }
                let c = self.la(1);
                if is_hex(c) {
                    self.bump();
                    if !self.tail_or(is_hex, b"Pp") {
                        return false;
                    }
                } else if !one_of(c, b"Pp") {
                    return false;
                }
            } else if !self.expect(is_hex) || !self.tail_or(is_hex, b"Pp") {
                return false;
            }
        } else if is_hex(c1)
            && CHARS_15.contains(c2)
            && CHARS_16.contains(c3)
            && (!double || CHARS_16.contains(c4))
        {
            self.bump();
            if !self.tail_or(is_hex, b"Pp") {
                return false;
            }
        } else {
            return false;
        }
        if !self.exponent(b"Pp") {
            return false;
        }
        if double {
            self.expect(|c| one_of(c, b"dD"))
        } else {
            self.optional(b"fF");
            true
        }
    }

    /// `LONG_LITERAL`.
    fn long_literal(&mut self) -> bool {
        let c1 = self.la(1);
        if is(c1, b'0') {
            self.bump();
            let c = self.la(1);
            if one_of(c, b"xX") {
                self.bump();
                if !self.expect(is_hex) || !self.tail_or(is_hex, b"Ll") {
                    return false;
                }
            } else if one_of(c, b"bB") {
                self.bump();
                if !self.expect(is_bin) || !self.tail_or(is_bin, b"Ll") {
                    return false;
                }
            } else if is_oct(c) || one_of(c, b"_") {
                if !self.tail(is_oct) {
                    return false;
                }
            } else if !one_of(c, b"Ll") {
                return false;
            }
        } else if is_digit(c1) {
            self.bump();
            if !self.tail_or(is_digit, b"Ll") {
                return false;
            }
        } else {
            return false;
        }
        self.expect(|c| one_of(c, b"lL"))
    }

    /// `INT_LITERAL`.
    fn int_literal(&mut self) -> bool {
        let c1 = self.la(1);
        if is(c1, b'0') {
            self.bump();
            let c = self.la(1);
            if one_of(c, b"xX") {
                self.bump();
                self.expect(is_hex) && (!CHARS_14.contains(self.la(1)) || self.tail(is_hex))
            } else if one_of(c, b"bB") {
                self.bump();
                self.expect(is_bin) && self.tail_opt(is_bin)
            } else {
                !(is_oct(c) || is(c, b'_')) || self.tail(is_oct)
            }
        } else if is_digit(c1) {
            self.bump();
            self.tail_opt(is_digit)
        } else {
            false
        }
    }
}

/// Which token source the selector reads from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    /// The hidden-token filter over the Java lexer.
    Filter,
    /// The text block lexer.
    TextBlock,
}

/// The shared lexer input state and the stream plumbing.
struct Lexer<'a> {
    input: &'a [u16],
    pos: usize,
    line: i32,
    col: i32,
    tok_line: i32,
    tok_col: i32,
    tok_start: usize,
    selected: Source,
    stack: Vec<Source>,
    /// The filter's lookahead token (`nextMonitoredToken`).
    filter_la: Option<Token>,
    /// The comment listener (`FileContents`) and the text it reads.
    text: &'a FileText,
    comments: &'a mut Comments,
}

impl Lexer<'_> {
    fn la(&self, i: usize) -> u16 {
        self.input
            .get(self.pos + i - 1)
            .copied()
            .unwrap_or(EOF_CHAR)
    }

    /// `CharScanner.consume()` of the Java lexer, whose `tab()` advances
    /// the column by one like any character.
    const fn consume(&mut self) {
        self.pos += 1;
        self.col += 1;
    }

    /// `consume()` of the text block lexer, which keeps ANTLR's default
    /// `tab()`: the next tab stop of eight.
    fn consume_text_block(&mut self) {
        if self.la(1) == 0x09 {
            self.col = ((self.col - 1) / 8 + 1) * 8 + 1;
        } else {
            self.col += 1;
        }
        self.pos += 1;
    }

    const fn newline(&mut self) {
        self.line += 1;
        self.col = 1;
    }

    const fn reset_text(&mut self) {
        self.tok_start = self.pos;
        self.tok_line = self.line;
        self.tok_col = self.col;
    }

    const fn error(&self, message: String) -> StreamError {
        StreamError::at(self.line, self.col, message)
    }

    fn no_viable(&self) -> StreamError {
        self.error(unexpected_char(self.la(1)))
    }

    fn match_char(&mut self, c: u8) -> LexResult<()> {
        if self.la(1) == u16::from(c) {
            self.consume();
            Ok(())
        } else {
            Err(self.error(format!(
                "expecting {}, found {}",
                char_name(u16::from(c)),
                char_name(self.la(1))
            )))
        }
    }

    fn match_str(&mut self, s: &[u8]) -> LexResult<()> {
        for &c in s {
            self.match_char(c)?;
        }
        Ok(())
    }

    fn make_token(&self, ty: u16) -> Token {
        let text = String::from_utf16_lossy(&self.input[self.tok_start..self.pos]);
        Token {
            ty,
            text: Some(text.into()),
            line: self.tok_line,
            col: self.tok_col,
        }
    }

    const fn eof_token(&self) -> Token {
        Token {
            ty: EOF_TYPE,
            text: None,
            line: self.tok_line,
            col: self.tok_col,
        }
    }

    /// `testLiteralsTable`: an identifier that is a keyword.
    fn literal_type(text: &str, ty: u16) -> u16 {
        match LITERALS.binary_search_by(|(word, _)| (*word).cmp(text)) {
            Ok(i) => LITERALS[i].1,
            Err(_) => ty,
        }
    }

    // ---- the token stream -------------------------------------------------

    /// `TokenStreamSelector.nextToken()`.
    fn selector_next(&mut self) -> LexResult<Token> {
        match self.selected {
            Source::Filter => self.filter_next(),
            Source::TextBlock => self.text_block_next(),
        }
    }

    fn push_text_block(&mut self) {
        self.stack.push(self.selected);
        self.selected = Source::TextBlock;
    }

    fn pop(&mut self) {
        self.selected = self.stack.pop().unwrap_or(Source::Filter);
    }

    const fn hidden(ty: u16) -> bool {
        ty == SINGLE_LINE_COMMENT || ty == BLOCK_COMMENT_BEGIN
    }

    fn filter_consume(&mut self) -> LexResult<()> {
        let token = self.java_next()?;
        self.filter_la = Some(token);
        Ok(())
    }

    fn filter_la_hidden(&self) -> bool {
        self.filter_la.as_ref().is_some_and(|t| Self::hidden(t.ty))
    }

    /// `TokenStreamHiddenTokenFilter.nextToken()`.
    fn filter_next(&mut self) -> LexResult<Token> {
        if self.filter_la.is_none() {
            self.filter_consume()?;
            while self.filter_la_hidden() {
                self.filter_consume()?;
            }
        }
        let monitored = self.filter_la.take().unwrap_or_else(|| self.eof_token());
        self.filter_consume()?;
        while self.filter_la_hidden() {
            self.filter_consume()?;
        }
        Ok(monitored)
    }

    // ---- GeneratedJavaLexer --------------------------------------------------

    /// `GeneratedJavaLexer.nextToken()`.
    fn java_next(&mut self) -> LexResult<Token> {
        loop {
            self.reset_text();
            let (c1, c2, c3, c4) = (self.la(1), self.la(2), self.la(3), self.la(4));
            let single = match u8::try_from(c1).unwrap_or(0) {
                b'?' => Some(QUESTION),
                b'(' => Some(LPAREN),
                b')' => Some(RPAREN),
                b'[' => Some(LBRACK),
                b']' => Some(RBRACK),
                b'{' => Some(LCURLY),
                b'}' => Some(RCURLY),
                b',' => Some(COMMA),
                b'~' => Some(BNOT),
                b';' => Some(SEMI),
                b'@' => Some(AT),
                _ => None,
            };
            if let Some(ty) = single {
                self.consume();
                return Ok(self.make_token(ty));
            }
            if one_of(c1, b"\t\n\x0c\r ") {
                self.whitespace();
                continue;
            }
            if is(c1, b'\'') {
                return self.char_literal();
            }
            if is(c1, b'.') || is_digit(c1) {
                return self.num_int();
            }
            let pair = |a: u8, b: u8| is(c1, a) && is(c2, b);
            let fixed: Option<(&[u8], u16)> = if pair(b'>', b'>') && is(c3, b'>') && is(c4, b'=') {
                Some((b">>>=", BSR_ASSIGN))
            } else if pair(b'>', b'>') && is(c3, b'=') {
                Some((b">>=", SR_ASSIGN))
            } else if pair(b'>', b'>') && is(c3, b'>') {
                Some((b">>>", BSR))
            } else if pair(b'<', b'<') && is(c3, b'=') {
                Some((b"<<=", SL_ASSIGN))
            } else if pair(b'"', b'"') && is(c3, b'"') {
                self.match_str(b"\"\"\"")?;
                self.push_text_block();
                return Ok(self.make_token(TEXT_BLOCK_LITERAL_BEGIN));
            } else if pair(b':', b':') {
                Some((b"::", DOUBLE_COLON))
            } else if pair(b'=', b'=') {
                Some((b"==", EQUAL))
            } else if pair(b'!', b'=') {
                Some((b"!=", NOT_EQUAL))
            } else if pair(b'/', b'=') {
                Some((b"/=", DIV_ASSIGN))
            } else if pair(b'+', b'=') {
                Some((b"+=", PLUS_ASSIGN))
            } else if pair(b'+', b'+') {
                Some((b"++", INC))
            } else if pair(b'-', b'=') {
                Some((b"-=", MINUS_ASSIGN))
            } else if pair(b'-', b'-') {
                Some((b"--", DEC))
            } else if pair(b'*', b'=') {
                Some((b"*=", STAR_ASSIGN))
            } else if pair(b'%', b'=') {
                Some((b"%=", MOD_ASSIGN))
            } else if pair(b'>', b'>') {
                Some((b">>", SR))
            } else if pair(b'>', b'=') {
                Some((b">=", GE))
            } else if pair(b'<', b'<') {
                Some((b"<<", SL))
            } else if pair(b'<', b'=') {
                Some((b"<=", LE))
            } else if pair(b'-', b'>') {
                Some((b"->", LAMBDA))
            } else if pair(b'^', b'=') {
                Some((b"^=", BXOR_ASSIGN))
            } else if pair(b'|', b'=') {
                Some((b"|=", BOR_ASSIGN))
            } else if pair(b'|', b'|') {
                Some((b"||", LOR))
            } else if pair(b'&', b'=') {
                Some((b"&=", BAND_ASSIGN))
            } else if pair(b'&', b'&') {
                Some((b"&&", LAND))
            } else if pair(b'/', b'/') {
                return self.single_line_comment();
            } else if pair(b'/', b'*') {
                return self.block_comment();
            } else if is(c1, b'"') && c2 != EOF_CHAR {
                return self.string_literal();
            } else {
                match u8::try_from(c1).unwrap_or(0) {
                    b':' => Some((b":", COLON)),
                    b'=' => Some((b"=", ASSIGN)),
                    b'!' => Some((b"!", LNOT)),
                    b'/' => Some((b"/", DIV)),
                    b'+' => Some((b"+", PLUS)),
                    b'-' => Some((b"-", MINUS)),
                    b'*' => Some((b"*", STAR)),
                    b'%' => Some((b"%", MOD)),
                    b'>' => Some((b">", GT)),
                    b'<' => Some((b"<", LT)),
                    b'^' => Some((b"^", BXOR)),
                    b'|' => Some((b"|", BOR)),
                    b'&' => Some((b"&", BAND)),
                    _ => None,
                }
            };
            if let Some((text, ty)) = fixed {
                self.match_str(text)?;
                return Ok(self.make_token(ty));
            }
            if CHARS_0.contains(c1) {
                return self.ident();
            }
            if c1 == EOF_CHAR {
                return Ok(self.eof_token());
            }
            return Err(self.no_viable());
        }
    }

    /// `WS`: one or more blanks and line ends, skipped.
    fn whitespace(&mut self) {
        loop {
            let c = self.la(1);
            if one_of(c, b" \t\x0c") {
                self.consume();
            } else if is(c, b'\r') || is(c, b'\n') {
                if is(c, b'\r') && is(self.la(2), b'\n') {
                    self.consume();
                }
                self.consume();
                self.newline();
            } else {
                break;
            }
        }
    }

    /// `SINGLE_LINE_COMMENT`: `//` to the end of the line, the line end
    /// included; `newline()` runs even at the end of the file.
    fn single_line_comment(&mut self) -> LexResult<Token> {
        self.match_str(b"//")?;
        self.comments
            .report_single_line_comment(self.text, self.line, self.col - 3)
            .map_err(StreamError::Crash)?;
        while CHARS_1.contains(self.la(1)) {
            self.consume();
        }
        if is(self.la(1), b'\n') {
            self.consume();
        } else if is(self.la(1), b'\r') {
            self.consume();
            if is(self.la(1), b'\n') {
                self.consume();
            }
        }
        self.newline();
        Ok(self.make_token(SINGLE_LINE_COMMENT))
    }

    /// `BLOCK_COMMENT_BEGIN`: `/*` to `*/`, with `BLOCK_COMMENT_CONTENT`'s
    /// four-character lookahead near the end of the input.
    fn block_comment(&mut self) -> LexResult<Token> {
        self.match_str(b"/*")?;
        let (start_line, start_col) = (self.line, self.col - 3);
        loop {
            let (c1, c2, c3, c4) = (self.la(1), self.la(2), self.la(3), self.la(4));
            let line_end = (is(c1, b'\r') && is(c2, b'\n') && c3 != EOF_CHAR && c4 != EOF_CHAR)
                .then_some(2)
                .or_else(|| {
                    (is(c1, b'\r') && c2 != EOF_CHAR && c3 != EOF_CHAR || is(c1, b'\n'))
                        .then_some(1)
                });
            if is(c1, b'*') && c2 != EOF_CHAR && c3 != EOF_CHAR && !is(c2, b'/') {
                self.consume();
            } else if let Some(length) = line_end {
                for _ in 0..length {
                    self.consume();
                }
                self.newline();
            } else if CHARS_2.contains(c1) {
                self.consume();
            } else {
                break;
            }
        }
        self.match_str(b"*/")?;
        self.comments
            .report_block_comment(self.text, start_line, start_col, self.line, self.col - 2)
            .map_err(StreamError::Crash)?;
        Ok(self.make_token(BLOCK_COMMENT_BEGIN))
    }

    /// `CHAR_LITERAL`.
    fn char_literal(&mut self) -> LexResult<Token> {
        self.match_char(b'\'')?;
        if is(self.la(1), b'\\') {
            self.escape()?;
        } else if CHARS_3.contains(self.la(1)) {
            self.consume();
        } else {
            return Err(self.no_viable());
        }
        self.match_char(b'\'')?;
        Ok(self.make_token(CHAR_LITERAL))
    }

    /// `STRING_LITERAL`. A line end inside is just a character: the lexer's
    /// line count does not move.
    fn string_literal(&mut self) -> LexResult<Token> {
        self.match_char(b'"')?;
        loop {
            if is(self.la(1), b'\\') {
                self.escape()?;
            } else if CHARS_6.contains(self.la(1)) {
                self.consume();
            } else {
                break;
            }
        }
        self.match_char(b'"')?;
        Ok(self.make_token(STRING_LITERAL))
    }

    /// `HEX_DIGIT`.
    fn hex_digit(&mut self) -> LexResult<()> {
        if is_hex(self.la(1)) {
            self.consume();
            Ok(())
        } else {
            Err(self.no_viable())
        }
    }

    fn some_u(&mut self) -> LexResult<()> {
        if !is(self.la(1), b'u') {
            return Err(self.no_viable());
        }
        while is(self.la(1), b'u') {
            self.consume();
        }
        Ok(())
    }

    /// `ESC`.
    fn escape(&mut self) -> LexResult<()> {
        self.match_char(b'\\')?;
        let c = self.la(1);
        if is(c, b'u') {
            self.some_u()?;
            let (c1, c2, c3, c4) = (self.la(1), self.la(2), self.la(3), self.la(4));
            if is(c1, b'0') && is(c2, b'0') && is(c3, b'5') && one_of(c4, b"Cc") {
                self.consume();
                self.consume();
                self.consume();
                self.consume();
                let (d1, d2, d3, d4) = (self.la(1), self.la(2), self.la(3), self.la(4));
                let hex_or_u = |c: u16| is_hex(c) || is(c, b'u');
                if is(d1, b'\\') && is(d2, b'u') && hex_or_u(d3) && hex_or_u(d4) {
                    self.consume();
                    self.some_u()?;
                    for _ in 0..4 {
                        self.hex_digit()?;
                    }
                } else if one_of(d1, b"\"'01234567\\bfnrt") && d2 != EOF_CHAR {
                    self.std_escape()?;
                } else {
                    return Err(self.no_viable());
                }
            } else if is_hex(c1) && is_hex(c2) && is_hex(c3) && is_hex(c4) {
                for _ in 0..4 {
                    self.hex_digit()?;
                }
            } else {
                return Err(self.no_viable());
            }
            Ok(())
        } else if one_of(c, b"\"'01234567\\bfnrt") {
            self.std_escape()
        } else {
            Err(self.no_viable())
        }
    }

    /// `STD_ESC`, octal escapes included (a `4`-`7` takes any one digit
    /// after it, as the grammar says).
    fn std_escape(&mut self) -> LexResult<()> {
        let c = self.la(1);
        if one_of(c, b"nrtbf\"'\\") {
            self.consume();
            return Ok(());
        }
        if (u16::from(b'0')..=u16::from(b'3')).contains(&c) {
            self.consume();
            if is_oct(self.la(1)) && self.la(2) != EOF_CHAR {
                self.consume();
                if is_oct(self.la(1)) && self.la(2) != EOF_CHAR {
                    self.consume();
                } else if self.la(1) == EOF_CHAR {
                    return Err(self.no_viable());
                }
            } else if self.la(1) == EOF_CHAR {
                return Err(self.no_viable());
            }
            return Ok(());
        }
        if (u16::from(b'4')..=u16::from(b'7')).contains(&c) {
            self.consume();
            if is_digit(self.la(1)) && self.la(2) != EOF_CHAR {
                self.consume();
            } else if self.la(1) == EOF_CHAR {
                return Err(self.no_viable());
            }
            return Ok(());
        }
        Err(self.no_viable())
    }

    /// `IDENT` (`ID_START (ID_PART)*`), then the `assert`/`enum` and
    /// keyword tests.
    fn ident(&mut self) -> LexResult<Token> {
        self.id_char(true)?;
        while CHARS_9.contains(self.la(1)) {
            self.id_char(false)?;
        }
        let text = String::from_utf16_lossy(&self.input[self.tok_start..self.pos]);
        let ty = match text.as_str() {
            "assert" => ASSERT,
            "enum" => ENUM,
            _ => IDENT,
        };
        let ty = Self::literal_type(&text, ty);
        Ok(Token {
            ty,
            text: Some(text.into()),
            line: self.tok_line,
            col: self.tok_col,
        })
    }

    /// `ID_START` or `ID_PART`, whose semantic predicate failure becomes
    /// "Unexpected character 0x.. in identifier".
    fn id_char(&mut self, start: bool) -> LexResult<()> {
        let c = self.la(1);
        if is(c, b'_') || is(c, b'$') {
            self.consume();
            return Ok(());
        }
        let (set, ok) = if start {
            (&CHARS_7, is_java_identifier_start(c))
        } else {
            (&CHARS_8, is_java_identifier_part(c))
        };
        if !set.contains(c) {
            return Err(self.no_viable());
        }
        if !ok {
            return Err(self.error(format!("Unexpected character 0x{c:x} in identifier")));
        }
        self.consume();
        Ok(())
    }

    /// `NUM_INT`: the first literal rule whose lookahead and syntactic
    /// predicate hold, in the grammar's order.
    fn num_int(&mut self) -> LexResult<Token> {
        let (c1, c2, c3, c4) = (self.la(1), self.la(2), self.la(3), self.la(4));
        /// One alternative: its lookahead test, then its rule as a guess.
        fn attempt<'s>(
            input: &'s [u16],
            pos: usize,
            lookahead: bool,
            rule: fn(&mut Cur<'s>) -> bool,
        ) -> Option<usize> {
            if !lookahead {
                return None;
            }
            let mut cur = Cur { input, pos };
            rule(&mut cur).then_some(cur.pos)
        }
        let (input, pos) = (self.input, self.pos);
        let hex_pre =
            is(c1, b'0') && one_of(c2, b"Xx") && CHARS_10.contains(c3) && CHARS_11.contains(c4);
        let dot_or_digit = is(c1, b'.') || is_digit(c1);
        let found = attempt(input, pos, hex_pre, |c| c.hex_floating(true))
            .map(|end| (end, NUM_DOUBLE))
            .or_else(|| {
                attempt(input, pos, hex_pre, |c| c.hex_floating(false)).map(|end| (end, NUM_FLOAT))
            })
            .or_else(|| {
                (is(c1, b'.') && is(c2, b'.') && is(c3, b'.')).then_some((pos + 3, ELLIPSIS))
            })
            .or_else(|| {
                let lookahead = dot_or_digit && (is_digit(c2) || one_of(c2, b".DE_de"));
                attempt(input, pos, lookahead, Cur::double_literal).map(|end| (end, NUM_DOUBLE))
            })
            .or_else(|| {
                let lookahead = dot_or_digit && (is_digit(c2) || one_of(c2, b".EF_ef"));
                attempt(input, pos, lookahead, Cur::float_literal).map(|end| (end, NUM_FLOAT))
            })
            .or_else(|| {
                let lookahead = is_digit(c1) && (is_digit(c2) || one_of(c2, b"BLX_blx"));
                attempt(input, pos, lookahead, Cur::long_literal).map(|end| (end, NUM_LONG))
            })
            .or_else(|| is(c1, b'.').then_some((pos + 1, DOT)))
            .or_else(|| {
                attempt(input, pos, is_digit(c1), Cur::int_literal).map(|end| (end, NUM_INT))
            });
        let Some((end, ty)) = found else {
            return Err(self.no_viable());
        };
        while self.pos < end {
            self.consume();
        }
        Ok(self.make_token(ty))
    }

    // ---- GeneratedTextBlockLexer ---------------------------------------------

    /// `GeneratedTextBlockLexer.nextToken()`, a filter lexer: a rule that
    /// fails skips one more character and lexing starts over.
    fn text_block_next(&mut self) -> LexResult<Token> {
        loop {
            self.reset_text();
            let (c1, c2) = (self.la(1), self.la(2));
            let token = if is(c1, b'"') && is(c2, b'"') {
                self.text_block_end()
            } else if c1 != EOF_CHAR {
                self.text_block_content()
            } else {
                return Ok(self.eof_token());
            };
            if let Some(token) = token {
                let text = token.text.as_deref().unwrap_or("");
                let ty = Self::literal_type(text, token.ty);
                return Ok(Token { ty, ..token });
            }
            self.consume_text_block();
        }
    }

    /// `TEXT_BLOCK_LITERAL_END`: `"""`, and back to the Java lexer. `None`
    /// when the third quote is missing (what it matched stays consumed).
    fn text_block_end(&mut self) -> Option<Token> {
        for _ in 0..3 {
            if !is(self.la(1), b'"') {
                return None;
            }
            self.consume_text_block();
        }
        self.pop();
        Some(self.make_token(TEXT_BLOCK_LITERAL_END))
    }

    fn text_block_newline(&mut self) {
        if is(self.la(1), b'\r') && is(self.la(2), b'\n') {
            self.consume_text_block();
        }
        self.consume_text_block();
        self.newline();
    }

    /// `TEXT_BLOCK_CONTENT`: everything up to a `""` that does not continue
    /// as `""` plus a line end or another character. `None` when not even
    /// one piece matched.
    fn text_block_content(&mut self) -> Option<Token> {
        let not_quote_or_eol = |c: u16| c != EOF_CHAR && !one_of(c, b"\"\n\r");
        let mut count = 0;
        loop {
            let (c1, c2, c3) = (self.la(1), self.la(2), self.la(3));
            if is(c1, b'"') && is(c2, b'"') && (one_of(c3, b"\n\r") || not_quote_or_eol(c3)) {
                self.consume_text_block();
                self.consume_text_block();
                if one_of(c3, b"\n\r") {
                    self.text_block_newline();
                } else {
                    self.consume_text_block();
                }
            } else if is(c1, b'"') && c2 != EOF_CHAR && !is(c2, b'"') {
                self.consume_text_block();
                if one_of(c2, b"\n\r") {
                    self.text_block_newline();
                } else {
                    self.consume_text_block();
                }
            } else if is(c1, b'\\') && one_of(c2, b"nrstbf\"'\\") {
                self.consume_text_block();
                self.consume_text_block();
            } else if one_of(c1, b"\n\r") {
                self.text_block_newline();
            } else if not_quote_or_eol(c1) {
                self.consume_text_block();
            } else if count >= 1 {
                break;
            } else {
                return None;
            }
            count += 1;
        }
        Some(self.make_token(TEXT_BLOCK_CONTENT))
    }
}

/// The tokens the parser will read, in order, up to and including the
/// first end of file; or up to the point where the stream failed, with
/// the failure.
pub struct TokenStream {
    pub tokens: Vec<Token>,
    pub error: Option<StreamError>,
}

pub fn tokenize(text: &FileText, comments: &mut Comments) -> TokenStream {
    let mut lexer = Lexer {
        input: &text.full,
        pos: 0,
        line: 1,
        col: 1,
        tok_line: 1,
        tok_col: 1,
        tok_start: 0,
        selected: Source::Filter,
        stack: Vec::new(),
        filter_la: None,
        text,
        comments,
    };
    let mut tokens = Vec::new();
    loop {
        match lexer.selector_next() {
            Ok(token) => {
                let eof = token.ty == EOF_TYPE;
                tokens.push(token);
                if eof {
                    return TokenStream {
                        tokens,
                        error: None,
                    };
                }
            }
            Err(error) => {
                return TokenStream {
                    tokens,
                    error: Some(error),
                };
            }
        }
    }
}

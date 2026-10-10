//! `PPI::Tokenizer` and the token classes' `__TOKENIZER__*` methods.
//!
//! PPI tokenizes a line at a time, dispatching each character to the class
//! of the token being built (`class`), which returns what to do with it: add
//! it (`One`), nothing more (`Zero`: the class did the work, possibly
//! consuming further characters or lines), start a token of another class
//! (`Class`), or fail (`Undef`). The methods are ported one for one, in the
//! order PPI's files have them; the Perl regular expressions they test are
//! written out by hand on bytes.

use std::collections::VecDeque;

use super::{Class, DOCUMENT, Doc, HereDocData, Id, QuoteData, Section, is_space, is_word};

/// What a `__TOKENIZER__*` method returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Rv {
    /// undef: an error.
    Undef,
    /// 0: done, nothing to add.
    Zero,
    /// 1: the character belongs to the current token.
    One,
    /// A class name: start (or continue) a token of that class.
    Class(Class),
}

/// An exception's message (`PPI::Exception`, or a Perl `die`).
pub type Error = String;

type R = Result<Rv, Error>;

/// The result of a scan through possibly several lines.
enum Scan {
    Undef,
    /// Ran out of source: what was gathered (Perl returns a reference).
    Eof(Vec<u8>),
    Found(Vec<u8>),
}

/// The tokenizer and the lexer's state, around the document they build.
pub(super) struct Parser {
    pub doc: Doc,
    // PPI::Tokenizer.
    /// `source`: the lines not read yet; `None` once a read hit the end.
    source: Option<VecDeque<Vec<u8>>>,
    /// `line`: absent before the first line and after the last.
    line: Option<Vec<u8>>,
    cursor: isize,
    line_length: usize,
    line_count: usize,
    token: Option<Id>,
    class: Class,
    zone: Class,
    pub tokens: Vec<Id>,
    token_cursor: usize,
    token_eof: bool,
    pub perl6: Vec<Vec<u8>>,
    source_eof_chop: bool,
    // PPI::Lexer.
    pub buffer: VecDeque<Id>,
    pub delayed: Vec<Id>,
    /// `Word::on_char`'s re-entry for a forced keyword, which PPI repeats
    /// until Perl's stack gives out.
    word_recursion: usize,
}

const OPERATORS: &[&[u8]] = &[
    b"!", b"!=", b"!~", b"%", b"%=", b"&", b"&&", b"&&=", b"&=", b"*", b"**", b"**=", b"*=", b"+",
    b"++", b"+=", b",", b"-", b"--", b"-=", b"->", b".", b"..", b"...", b".=", b"/", b"//", b"//=",
    b"/=", b":", b"<", b"<<", b"<<=", b"<<>>", b"<=", b"<=>", b"<>", b"=", b"==", b"=>", b"=~",
    b">", b">=", b">>", b">>=", b"?", b"^", b"^=", b"and", b"cmp", b"eq", b"ge", b"gt", b"le",
    b"lt", b"ne", b"not", b"or", b"x", b"x=", b"xor", b"|", b"|=", b"||", b"||=", b"~", b"~~",
];

const MAGIC: &[&[u8]] = &[
    b"$!", b"$\"", b"$#", b"$#+", b"$#-", b"$$", b"$%", b"$&", b"$'", b"$(", b"$)", b"$*", b"$+",
    b"$,", b"$-", b"$.", b"$/", b"$0", b"$1", b"$2", b"$3", b"$4", b"$5", b"$6", b"$7", b"$8",
    b"$9", b"$::|", b"$:", b"$;", b"$<", b"$=", b"$>", b"$?", b"$@", b"$[", b"$\\", b"$]", b"$^",
    b"$^A", b"$^C", b"$^D", b"$^E", b"$^F", b"$^H", b"$^I", b"$^L", b"$^M", b"$^N", b"$^O", b"$^P",
    b"$^R", b"$^S", b"$^T", b"$^V", b"$^W", b"$^X", b"$_", b"$`", b"$|", b"$}", b"$~", b"%!",
    b"%+", b"%-", b"%^H", b"@*", b"@+", b"@-", b"@_",
];

const KEYWORDS: &[&[u8]] = &[
    b"abs",
    b"accept",
    b"alarm",
    b"and",
    b"atan2",
    b"bind",
    b"binmode",
    b"bless",
    b"break",
    b"caller",
    b"chdir",
    b"chmod",
    b"chomp",
    b"chop",
    b"chown",
    b"chr",
    b"chroot",
    b"close",
    b"closedir",
    b"cmp",
    b"connect",
    b"continue",
    b"cos",
    b"crypt",
    b"dbmclose",
    b"dbmopen",
    b"default",
    b"defined",
    b"delete",
    b"die",
    b"do",
    b"dump",
    b"each",
    b"else",
    b"elsif",
    b"endgrent",
    b"endhostent",
    b"endnetent",
    b"endprotoent",
    b"endpwent",
    b"endservent",
    b"eof",
    b"eq",
    b"eval",
    b"evalbytes",
    b"exec",
    b"exists",
    b"exit",
    b"exp",
    b"fc",
    b"fcntl",
    b"fileno",
    b"flock",
    b"for",
    b"foreach",
    b"fork",
    b"format",
    b"formline",
    b"ge",
    b"getc",
    b"getgrent",
    b"getgrgid",
    b"getgrnam",
    b"gethostbyaddr",
    b"gethostbyname",
    b"gethostent",
    b"getlogin",
    b"getnetbyaddr",
    b"getnetbyname",
    b"getnetent",
    b"getpeername",
    b"getpgrp",
    b"getppid",
    b"getpriority",
    b"getprotobyname",
    b"getprotobynumber",
    b"getprotoent",
    b"getpwent",
    b"getpwnam",
    b"getpwuid",
    b"getservbyname",
    b"getservbyport",
    b"getservent",
    b"getsockname",
    b"getsockopt",
    b"given",
    b"glob",
    b"gmtime",
    b"goto",
    b"grep",
    b"gt",
    b"hex",
    b"if",
    b"index",
    b"int",
    b"ioctl",
    b"join",
    b"keys",
    b"kill",
    b"last",
    b"lc",
    b"lcfirst",
    b"le",
    b"length",
    b"link",
    b"listen",
    b"local",
    b"localtime",
    b"lock",
    b"log",
    b"lstat",
    b"lt",
    b"m",
    b"map",
    b"mkdir",
    b"msgctl",
    b"msgget",
    b"msgrcv",
    b"msgsnd",
    b"my",
    b"ne",
    b"next",
    b"no",
    b"not",
    b"oct",
    b"open",
    b"opendir",
    b"or",
    b"ord",
    b"our",
    b"pack",
    b"package",
    b"pipe",
    b"pop",
    b"pos",
    b"print",
    b"printf",
    b"prototype",
    b"push",
    b"q",
    b"qq",
    b"qr",
    b"quotemeta",
    b"qw",
    b"qx",
    b"rand",
    b"read",
    b"readdir",
    b"readline",
    b"readlink",
    b"readpipe",
    b"recv",
    b"redo",
    b"ref",
    b"rename",
    b"require",
    b"reset",
    b"return",
    b"reverse",
    b"rewinddir",
    b"rindex",
    b"rmdir",
    b"s",
    b"say",
    b"scalar",
    b"seek",
    b"seekdir",
    b"select",
    b"semctl",
    b"semget",
    b"semop",
    b"send",
    b"setgrent",
    b"sethostent",
    b"setnetent",
    b"setpgrp",
    b"setpriority",
    b"setprotoent",
    b"setpwent",
    b"setservent",
    b"setsockopt",
    b"shift",
    b"shmctl",
    b"shmget",
    b"shmread",
    b"shmwrite",
    b"shutdown",
    b"sin",
    b"sleep",
    b"socket",
    b"socketpair",
    b"sort",
    b"splice",
    b"split",
    b"sprintf",
    b"sqrt",
    b"srand",
    b"stat",
    b"state",
    b"study",
    b"sub",
    b"substr",
    b"symlink",
    b"syscall",
    b"sysopen",
    b"sysread",
    b"sysseek",
    b"system",
    b"syswrite",
    b"tell",
    b"telldir",
    b"tie",
    b"tied",
    b"time",
    b"times",
    b"tr",
    b"truncate",
    b"uc",
    b"ucfirst",
    b"umask",
    b"undef",
    b"unless",
    b"unlink",
    b"unpack",
    b"unshift",
    b"untie",
    b"until",
    b"use",
    b"utime",
    b"values",
    b"vec",
    b"wait",
    b"waitpid",
    b"wantarray",
    b"warn",
    b"when",
    b"while",
    b"write",
    b"x",
    b"xor",
    b"y",
];

pub(super) fn is_operator(s: &[u8]) -> bool {
    OPERATORS.contains(&s)
}

fn is_magic(s: &[u8]) -> bool {
    MAGIC.contains(&s)
}

fn is_keyword(s: &[u8]) -> bool {
    KEYWORDS.binary_search(&s).is_ok()
}

/// `%QUOTELIKE`.
const fn quotelike(s: &[u8]) -> Option<Class> {
    Some(match s {
        b"q" => Class::QuoteLiteral,
        b"qq" => Class::QuoteInterpolate,
        b"qx" => Class::QuoteLikeCommand,
        b"qw" => Class::QuoteLikeWords,
        b"qr" => Class::QuoteLikeRegexp,
        b"m" => Class::RegexpMatch,
        b"s" => Class::RegexpSubstitute,
        b"tr" | b"y" => Class::RegexpTransliterate,
        _ => return None,
    })
}

/// The classes `Token::set_class` rebuilds a token for: the names matching
/// `\b(?:Quote|Regex)\b`, which are only `PPI::Token::Quote::*`.
const fn rebuilt_on_set_class(class: Class) -> bool {
    matches!(
        class,
        Class::QuoteSingle | Class::QuoteDouble | Class::QuoteLiteral | Class::QuoteInterpolate
    )
}

/// The classes built on `_QuoteEngine::Simple`.
const fn simple_quote(class: Class) -> bool {
    matches!(
        class,
        Class::QuoteSingle | Class::QuoteDouble | Class::QuoteLikeBacktick
    )
}

/// `_QuoteEngine::Full->new($init)`'s fields, from `%QUOTES`.
fn full_quote_data(init: &[u8]) -> Option<QuoteData> {
    let (operator, braced, separator, n_sections, modifiers): (
        Option<&[u8]>,
        Option<bool>,
        Option<u8>,
        usize,
        bool,
    ) = match init {
        b"q" | b"qq" | b"qx" | b"qw" => (Some(init), None, None, 1, false),
        b"qr" | b"m" => (Some(init), None, None, 1, true),
        b"s" | b"tr" | b"y" => (Some(init), None, None, 2, true),
        b"/" => (None, Some(false), Some(b'/'), 1, true),
        b"<" => (None, Some(true), None, 1, false),
        b"?" => (None, Some(false), Some(b'?'), 1, true),
        b"(" => (None, Some(true), None, 1, false),
        _ => return None,
    };
    let mut data = QuoteData {
        operator: operator.map(<[u8]>::to_vec),
        braced,
        separator,
        n_sections,
        modifiers: modifiers.then(Vec::new),
        sections: Vec::new(),
    };
    if init == b"<" {
        data.sections.push(braced_section(b'<'));
    }
    if init == b"(" {
        data.sections.push(braced_section(b'('));
    }
    Some(data)
}

/// A `%SECTIONS` entry: `type` and `_close` of an opening brace.
fn braced_section(open: u8) -> Section {
    let close = closing_brace(open);
    Section {
        position: 0,
        size: 0,
        kind: Some(vec![open, close]),
        close: Some(close),
    }
}

const fn closing_brace(open: u8) -> u8 {
    match open {
        b'(' => b')',
        b'<' => b'>',
        b'[' => b']',
        _ => b'}',
    }
}

const fn is_section_open(c: u8) -> bool {
    matches!(c, b'(' | b'<' | b'[' | b'{')
}

/// The first position at or after `start` matching `.*?(?<!\\)(?:\\\\)*X`
/// where X is a byte `target` accepts: the index of X.
fn find_unescaped(line: &[u8], start: usize, target: impl Fn(u8) -> bool) -> Option<usize> {
    let mut p = start;
    while p < line.len() {
        if p > start && line[p - 1] == b'\n' {
            return None;
        }
        if !(p > 0 && line[p - 1] == b'\\') {
            let run = line[p..].iter().take_while(|&&c| c == b'\\').count();
            for k in (0..=run / 2).rev() {
                let q = p + 2 * k;
                if q < line.len() && target(line[q]) {
                    return Some(q);
                }
            }
        }
        p += 1;
    }
    None
}

/// `\w+(?:(?:'|::)\w+)*(?:::)?` at `pos` (with `(?!\d)` first when
/// `no_digit`): the match's length.
fn match_bareword(line: &[u8], pos: usize, no_digit: bool) -> Option<usize> {
    let word = |i: usize| {
        line[i.min(line.len())..]
            .iter()
            .take_while(|&&c| is_word(c))
            .count()
    };
    if no_digit && line.get(pos).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let first = word(pos);
    if first == 0 {
        return None;
    }
    let mut end = pos + first;
    loop {
        let sep = if line[end..].starts_with(b"'") {
            1
        } else if line[end..].starts_with(b"::") {
            2
        } else {
            0
        };
        if sep == 0 {
            break;
        }
        let w = word(end + sep);
        if w == 0 {
            break;
        }
        end += sep + w;
    }
    if line[end..].starts_with(b"::") {
        end += 2;
    }
    Some(end - pos)
}

/// The length of a bareword to keep: `eq'foo` is the keyword `eq` and a
/// quote (`$word =~ /^(\w+)'/ && $KEYWORDS{$1}`), anything else all of it.
fn keyword_before_quote(word: &[u8]) -> usize {
    match word.iter().position(|&c| c == b'\'') {
        Some(quote)
            if quote > 0
                && word[..quote].iter().all(|&c| is_word(c))
                && is_keyword(&word[..quote]) =>
        {
            quote
        }
        _ => word.len(),
    }
}

/// `$CURLY_SYMBOL` (`\G\^[[:upper:]_]\w+\}`) at `pos`: its length.
fn curly_symbol(line: &[u8], pos: usize) -> Option<usize> {
    if line.get(pos) != Some(&b'^') {
        return None;
    }
    let c = *line.get(pos + 1)?;
    if !(c.is_ascii_uppercase() || c == b'_') {
        return None;
    }
    let w = line[pos + 2..].iter().take_while(|&&c| is_word(c)).count();
    if w == 0 || line.get(pos + 2 + w) != Some(&b'}') {
        return None;
    }
    Some(w + 3)
}

/// Whether `pos` starts `\s*` followed by `what`.
fn spaces_then(line: &[u8], pos: usize, what: &[u8]) -> bool {
    let rest = &line[pos.min(line.len())..];
    let skip = rest.iter().take_while(|&&c| is_space(c)).count();
    rest[skip..].starts_with(what)
}

/// The `\G\s*` gap match of `_scan_quote_like_operator_gap`: `\s*` then an
/// optional `#` comment to the end of the line, newline excluded.
fn gap_length(line: &[u8], pos: usize) -> usize {
    let rest = &line[pos.min(line.len())..];
    let mut n = rest.iter().take_while(|&&c| is_space(c)).count();
    if rest.get(n) == Some(&b'#') {
        n += rest[n..].iter().take_while(|&&c| c != b'\n').count();
    }
    n
}

impl Parser {
    pub(super) fn new(source: &[u8], filename: &[u8]) -> Self {
        let mut lines: VecDeque<Vec<u8>> = VecDeque::new();
        if !source.is_empty() {
            // s/(?:\015{1,2}\012|\015|\012)/\n/g, then split after each \n.
            let mut normalized = Vec::with_capacity(source.len());
            let mut i = 0;
            while i < source.len() {
                match source[i] {
                    b'\r' => {
                        if source.get(i + 1) == Some(&b'\n') {
                            i += 2;
                        } else if source.get(i + 1) == Some(&b'\r')
                            && source.get(i + 2) == Some(&b'\n')
                        {
                            i += 3;
                        } else {
                            i += 1;
                        }
                        normalized.push(b'\n');
                    }
                    c => {
                        normalized.push(c);
                        i += 1;
                    }
                }
            }
            let mut start = 0;
            for (i, &c) in normalized.iter().enumerate() {
                if c == b'\n' {
                    lines.push_back(normalized[start..=i].to_vec());
                    start = i + 1;
                }
            }
            if start < normalized.len() {
                lines.push_back(normalized[start..].to_vec());
            }
        }
        let has_end = lines.iter().any(|l| is_data_or_end_line(l));
        let mut source_eof_chop = false;
        if !has_end
            && let Some(last) = lines.back_mut()
            && !last.last().is_some_and(|&c| is_space(c))
        {
            source_eof_chop = true;
            last.push(b' ');
        }
        Self {
            doc: Doc::new(filename),
            source: Some(lines),
            line: None,
            cursor: 0,
            line_length: 0,
            line_count: 0,
            token: None,
            class: Class::Bom,
            zone: Class::Whitespace,
            tokens: Vec::new(),
            token_cursor: 0,
            token_eof: false,
            perl6: Vec::new(),
            source_eof_chop,
            buffer: VecDeque::new(),
            delayed: Vec::new(),
            word_recursion: 0,
        }
    }

    // ---- Main public methods.

    /// `get_token`: the next token, `None` at the end.
    pub(super) fn get_token(&mut self) -> Result<Option<Id>, Error> {
        if self.token_eof && self.token_cursor > self.tokens.len() {
            return Ok(None);
        }
        if let Some(&token) = self.tokens.get(self.token_cursor) {
            self.token_cursor += 1;
            return Ok(Some(token));
        }
        while self.process_next_line()? {
            if let Some(&token) = self.tokens.get(self.token_cursor) {
                self.token_cursor += 1;
                return Ok(Some(token));
            }
        }
        if let Some(&token) = self.tokens.get(self.token_cursor) {
            self.token_cursor += 1;
            return Ok(Some(token));
        }
        self.token_eof = true;
        Ok(None)
    }

    // ---- Working with source.

    fn get_line(&mut self) -> Option<Vec<u8>> {
        let source = self.source.as_mut()?;
        let line = source.pop_front();
        if line.is_none() {
            self.source = None;
        }
        line
    }

    /// `_fill_line`: true when a line was read.
    fn fill_line(&mut self, inscan: bool) -> bool {
        let Some(line) = self.get_line() else {
            if inscan {
                self.cursor = self.line_length as isize;
            } else {
                self.line = None;
            }
            return false;
        };
        self.line_length = line.len();
        self.line = Some(line);
        self.cursor = -1;
        self.line_count += 1;
        true
    }

    fn line(&self) -> &[u8] {
        self.line.as_deref().unwrap_or(&[])
    }

    /// `substr($line, $i, 1)`: empty past either end.
    fn at(&self, i: isize) -> Option<u8> {
        if i < 0 {
            return None;
        }
        self.line().get(i as usize).copied()
    }

    /// The character at the cursor, as a string (empty past the end).
    fn char_str(&self) -> Vec<u8> {
        self.at(self.cursor).map(|c| vec![c]).unwrap_or_default()
    }

    fn pos(&self) -> usize {
        self.cursor.max(0) as usize
    }

    fn sources_left(&self) -> bool {
        self.source.as_ref().is_some_and(|s| !s.is_empty())
    }

    // ---- Per line processing.

    /// `_process_next_line`: false at the end of the source.
    fn process_next_line(&mut self) -> Result<bool, Error> {
        if !self.fill_line(false) {
            self.finalize();
            return Ok(false);
        }
        let rv = self.on_line_start(self.class)?;
        if rv != Rv::One {
            if self.source.as_ref().is_some_and(VecDeque::is_empty) {
                self.clean_eof();
            }
            if rv == Rv::Zero {
                return Ok(true);
            }
            return Err(format!("Error at line {}", self.line_count));
        }
        loop {
            match self.process_next_char()? {
                Rv::One => {}
                Rv::Zero => break,
                _ => {
                    return Err(format!(
                        "Error at line {}, character {}",
                        self.line_count, self.cursor
                    ));
                }
            }
        }
        self.on_line_end(self.class);
        if !self.sources_left() {
            self.clean_eof();
        }
        Ok(true)
    }

    // ---- Per-character processing.

    fn process_next_char(&mut self) -> R {
        if self.line.is_none() {
            return Ok(Rv::Undef);
        }
        self.cursor += 1;
        if self.cursor >= self.line_length as isize {
            return Ok(Rv::Zero);
        }
        let result = self.on_char(self.class)?;
        match result {
            Rv::Undef => return Ok(Rv::Undef),
            Rv::Zero => return Ok(Rv::One),
            _ => {}
        }
        let ch = self.char_str();
        match result {
            Rv::One => {
                if let Some(token) = self.token {
                    self.doc.elems[token].content.extend_from_slice(&ch);
                } else {
                    let token = self.create(self.class, &ch)?;
                    self.token = Some(token);
                }
            }
            Rv::Class(class) => {
                if self.class != class {
                    self.new_token(class, &ch)?;
                } else if let Some(token) = self.token {
                    self.doc.elems[token].content.extend_from_slice(&ch);
                } else {
                    let token = self.create(self.class, &ch)?;
                    self.token = Some(token);
                }
            }
            Rv::Undef | Rv::Zero => {}
        }
        Ok(Rv::One)
    }

    // ---- Altering tokens.

    /// `_finalize_token`: the class to carry on with.
    fn finalize(&mut self) -> Class {
        let Some(token) = self.token.take() else {
            return self.class;
        };
        self.tokens.push(token);
        self.class = self.zone;
        self.class
    }

    /// `$class->new($content)`.
    fn create(&mut self, class: Class, content: &[u8]) -> Result<Id, Error> {
        let quote = if simple_quote(class) {
            // `my $separator = shift or return undef`.
            if content.is_empty() || content == b"0" {
                return Err("Unknown Exception".to_string());
            }
            Some(QuoteData {
                separator: content.first().copied(),
                ..QuoteData::default()
            })
        } else if class.is_quote_like_class() {
            Some(full_quote_data(content).ok_or_else(|| "Unknown Exception".to_string())?)
        } else {
            None
        };
        let id = self.doc.alloc(class, content);
        self.doc.elems[id].quote = quote.map(Box::new);
        Ok(id)
    }

    /// `_new_token`: returns 1, which callers pass on.
    fn new_token(&mut self, class: Class, content: &[u8]) -> R {
        if self.token.is_some() {
            self.finalize();
        }
        let token = self.create(class, content)?;
        self.token = Some(token);
        self.class = class;
        Ok(Rv::One)
    }

    /// `$t->{class} = $t->{token}->set_class($class)`.
    fn set_class(&mut self, class: Class) {
        let Some(token) = self.token else {
            self.class = class;
            return;
        };
        let elem = &mut self.doc.elems[token];
        let rebuild = rebuilt_on_set_class(elem.class) != rebuilt_on_set_class(class);
        elem.class = class;
        if rebuild {
            // `%$self = %{ $class->new($content) }`.
            elem.quote = if class.is_quote_like_class() {
                full_quote_data(&elem.content).map(Box::new)
            } else {
                None
            };
            elem.heredoc = None;
            elem.attribute_hint = false;
        }
        self.class = class;
    }

    fn token_content(&self) -> &[u8] {
        self.token.map_or(&[][..], |t| &self.doc.elems[t].content)
    }

    fn append(&mut self, bytes: &[u8]) {
        if let Some(token) = self.token {
            self.doc.elems[token].content.extend_from_slice(bytes);
        }
    }

    /// `_clean_eof`.
    fn clean_eof(&mut self) {
        if self.token.is_some() {
            self.finalize();
        }
        if self
            .tokens
            .last()
            .is_some_and(|&last| self.doc.elems[last].content.is_empty())
        {
            self.tokens.pop();
        }
        if self.source_eof_chop {
            // `s/ $//`: the space added at the end, or before a final
            // newline.
            if let Some(&last) = self.tokens.last() {
                let content = &mut self.doc.elems[last].content;
                if content.ends_with(b" ") {
                    content.pop();
                } else if content.ends_with(b" \n") {
                    let n = content.len();
                    content.remove(n - 2);
                }
                if content.is_empty() {
                    self.tokens.pop();
                }
            }
            self.source_eof_chop = false;
        }
    }

    // ---- Context.

    fn last_significant_token(&self) -> Option<Id> {
        self.previous_significant_tokens(1).first().copied()
    }

    fn previous_significant_tokens(&self, count: usize) -> Vec<Id> {
        let mut out = Vec::new();
        for &token in self.tokens.iter().rev() {
            if !self.doc.significant(token) {
                continue;
            }
            out.push(token);
            if out.len() >= count {
                break;
            }
        }
        out
    }

    fn content_of(&self, id: Id) -> &[u8] {
        &self.doc.elems[id].content
    }

    fn token_isa(&self, id: Option<Id>, class: Class) -> bool {
        id.is_some_and(|t| self.doc.isa(t, class))
    }

    fn token_is(&self, id: Option<Id>, class: Class, content: &[u8]) -> bool {
        id.is_some_and(|t| self.doc.isa(t, class) && self.content_of(t) == content)
    }

    /// `_opcontext`: "operator", "operand" or "".
    fn opcontext(&self) -> &'static str {
        let Some(p0) = self.last_significant_token() else {
            return "";
        };
        let class = self.doc.class(p0);
        if matches!(
            class,
            Class::Symbol
                | Class::Magic
                | Class::Number
                | Class::ArrayIndex
                | Class::QuoteDouble
                | Class::QuoteInterpolate
                | Class::QuoteLiteral
                | Class::QuoteSingle
                | Class::QuoteLikeBacktick
                | Class::QuoteLikeCommand
                | Class::QuoteLikeReadline
                | Class::QuoteLikeRegexp
                | Class::QuoteLikeWords
        ) {
            return "operator";
        }
        match self.content_of(p0) {
            b"(" | b"{" | b"[" | b";" => return "operand",
            b"}" => return "operator",
            _ => {}
        }
        if class.isa(Class::Operator) || self.content_of(p0).is_empty() {
            return "operand";
        }
        ""
    }

    /// `_current_x_is_operator`.
    fn current_x_is_operator(&self) -> bool {
        if self.tokens.is_empty() {
            return false;
        }
        let Some(&prev) = self.previous_significant_tokens(2).first() else {
            return false;
        };
        if self.doc.isa(prev, Class::Word) {
            return !self.current_token_is_forced_word(None);
        }
        let content = self.content_of(prev);
        (!self.doc.isa(prev, Class::Operator) || matches!(content, b"--" | b"++"))
            && (!self.doc.isa(prev, Class::StructureToken) || matches!(content, b"}" | b"]" | b")"))
            && !self.doc.isa(prev, Class::Label)
    }

    /// `__current_token_is_forced_word`.
    fn current_token_is_forced_word(&self, word: Option<&[u8]>) -> bool {
        let tokens = self.previous_significant_tokens(2);
        let line = self.line();
        let pos = self.pos();
        if let Some(&prev) = tokens.first() {
            let content = self.content_of(prev);
            if content == b"->" {
                return true;
            }
            if content == b"{" && spaces_then(line, pos, b"}") {
                return true;
            }
            if matches!(content, b"sub" | b"package" | b"use" | b"no") {
                if let Some(w) = word {
                    let vnum = w.len() > 1 && w[0] == b'v' && w[1..].iter().all(u8::is_ascii_digit);
                    if vnum && matches!(content, b"use" | b"no") {
                        return false;
                    }
                }
                let Some(&prevprev) = tokens.get(1) else {
                    return true;
                };
                let pp = self.content_of(prevprev);
                return !matches!(pp, b"sub" | b"package" | b"use" | b"no") && pp != b"->";
            }
        }
        spaces_then(line, pos, b"=>")
    }

    /// `_current_token_has_signatures_active`: whether signatures are on,
    /// and the previous significant tokens it looked at.
    fn current_token_has_signatures_active(&self) -> Result<(bool, Vec<Id>), Error> {
        let mut tokens_to_get = 3;
        let mut tokens: Vec<Id> = Vec::new();
        let mut first = true;
        while first || tokens.last().is_some_and(|&t| self.doc.parent(t).is_none()) {
            first = false;
            tokens = self.previous_significant_tokens(tokens_to_get);
            if tokens.len() < tokens_to_get {
                break;
            }
            tokens_to_get += 1;
        }
        let closest = tokens
            .iter()
            .copied()
            .find(|&t| self.doc.parent(t).is_some())
            .unwrap_or(DOCUMENT);
        Ok((self.doc.presumed_features(closest)?.signatures, tokens))
    }

    // ---- Dispatch.

    fn on_line_start(&mut self, class: Class) -> R {
        match class {
            Class::Whitespace => self.whitespace_on_line_start(),
            Class::Bom => self.bom_on_line_start(),
            Class::Pod => Ok(self.pod_on_line_start()),
            Class::End => self.end_on_line_start(),
            Class::Data => self.data_on_line_start(),
            _ => Ok(Rv::One),
        }
    }

    fn on_line_end(&mut self, class: Class) {
        if matches!(class, Class::Whitespace | Class::Comment) && self.token.is_some() {
            self.finalize();
        }
    }

    fn on_char(&mut self, class: Class) -> R {
        match class {
            Class::Whitespace => self.whitespace_on_char(),
            Class::Comment => self.comment_on_char(),
            Class::End => Ok(Rv::One),
            Class::Number => self.number_on_char(),
            Class::NumberBinary => self.binary_on_char(),
            Class::NumberOctal => self.octal_on_char(),
            Class::NumberHex => self.hex_on_char(),
            Class::NumberFloat => self.float_on_char(),
            Class::NumberExp => self.exp_on_char(),
            Class::NumberVersion => self.version_on_char(),
            Class::Word | Class::Separator => self.word_on_char(),
            Class::DashedWord => self.dashed_word_on_char(),
            Class::Symbol => self.symbol_on_char(),
            Class::Magic => self.magic_on_char(),
            Class::ArrayIndex => self.array_index_on_char(),
            Class::Operator => self.operator_on_char(),
            Class::Cast => self.cast_on_char(),
            Class::StructureToken => self.finalize_and_continue(),
            Class::HereDoc => self.heredoc_on_char(),
            Class::Attribute => self.attribute_on_char(),
            Class::Unknown => self.unknown_on_char(),
            c if c.is_quote_like_class() => Ok(self.quote_engine_on_char()),
            _ => Ok(Rv::Class(Class::Unknown)),
        }
    }

    /// `$t->_finalize_token->__TOKENIZER__on_char($t)`.
    fn finalize_and_continue(&mut self) -> R {
        let class = self.finalize();
        self.on_char(class)
    }

    // ---- PPI::Token::Whitespace.

    fn whitespace_on_line_start(&mut self) -> R {
        let line = self.line().to_vec();
        if line.iter().all(|&c| is_space(c)) {
            self.new_token(Class::Whitespace, &line)?;
            return Ok(Rv::Zero);
        }
        let indent = line.iter().take_while(|&&c| is_space(c)).count();
        if line.get(indent) == Some(&b'#') {
            self.new_token(Class::Comment, &line)?;
            self.finalize();
            return Ok(Rv::Zero);
        }
        if line.first() == Some(&b'=') && line.get(1).is_some_and(|&c| is_word(c)) {
            self.new_token(Class::Pod, &line)?;
            if !pod_command_is(&line, b"cut") {
                self.class = Class::Pod;
            }
            return Ok(Rv::Zero);
        }
        if line.starts_with(b"use v6-alpha;") {
            let mut perl6 = Vec::new();
            while let Some(l) = self.get_line() {
                perl6.extend(l);
            }
            self.perl6.push(perl6);
            return Ok(Rv::One);
        }
        Ok(Rv::One)
    }

    fn whitespace_on_char(&mut self) -> R {
        let c = self.at(self.cursor);
        let ch = c.unwrap_or(0);
        match ch {
            b'a'..=b'u' | b'A'..=b'Z' | b'w' | b'y' | b'z' | b'_' => return self.word_commit(),
            b';' | b'[' | b']' | b'{' | b'}' | b')' => {
                self.new_token(Class::StructureToken, &[ch])?;
                self.finalize();
                return Ok(Rv::Zero);
            }
            b'#' => return self.comment_commit(),
            b'v' => return self.version_commit(),
            b'0'..=b'9' => return Ok(Rv::Class(Class::Number)),
            b'=' | b'?' | b'|' | b'+' | b'>' | b'.' | b'!' | b'~' | b'^' | b',' => {
                return Ok(Rv::Class(Class::Operator));
            }
            b'*' | b'$' | b'@' | b'&' | b':' | b'%' => return Ok(Rv::Class(Class::Unknown)),
            b'\'' => return Ok(Rv::Class(Class::QuoteSingle)),
            b'"' => return Ok(Rv::Class(Class::QuoteDouble)),
            b'`' => return Ok(Rv::Class(Class::QuoteLikeBacktick)),
            b'\\' => return Ok(Rv::Class(Class::Cast)),
            9 | 10 | 12 | 13 | 32 => return Ok(Rv::Class(Class::Whitespace)),
            _ => {}
        }
        match ch {
            b'(' => {
                if self.token.is_some() {
                    self.finalize();
                }
                let (has_sig, tokens) = self.current_token_has_signatures_active()?;
                if has_sig {
                    return Ok(Rv::Class(Class::StructureToken));
                }
                let p0 = tokens.first().copied();
                let p1 = tokens.get(1).copied();
                let p2 = tokens.get(2).copied();
                if self.token_isa(p0, Class::Word)
                    && self.token_is(p1, Class::Word, b"sub")
                    && (p2.is_none()
                        || self.token_isa(p2, Class::StructureToken)
                        || self.token_is(p2, Class::Whitespace, b"")
                        || (self.token_isa(p2, Class::Word)
                            && p2.is_some_and(|t| {
                                matches!(self.content_of(t), b"my" | b"our" | b"state")
                            })))
                {
                    return Ok(Rv::Class(Class::Prototype));
                }
                if self.token_is(p0, Class::Word, b"sub")
                    && !self.token_is(p1, Class::Operator, b"->")
                {
                    return Ok(Rv::Class(Class::Prototype));
                }
                Ok(Rv::Class(Class::StructureToken))
            }
            b'<' => {
                if self.token.is_some() {
                    self.finalize();
                }
                let prev = self.last_significant_token();
                for class in [
                    Class::Symbol,
                    Class::Magic,
                    Class::Number,
                    Class::ArrayIndex,
                ] {
                    if self.token_isa(prev, class) {
                        return Ok(Rv::Class(Class::Operator));
                    }
                }
                let next = (self.at(self.cursor + 1), self.at(self.cursor + 2));
                if next.0 == Some(b'<') && next.1.is_some_and(|c| c != b'>') {
                    return Ok(Rv::Class(Class::Operator));
                }
                let Some(prev) = prev else {
                    return Ok(Rv::Class(Class::Operator));
                };
                let before = self.content_of(prev);
                let structure = self.doc.isa(prev, Class::StructureToken);
                let operator = self.doc.isa(prev, Class::Operator);
                if (structure && (before == b"(" || before == b";"))
                    || (self.doc.isa(prev, Class::Word) && before == b"while")
                    || (operator && (before == b"=" || before == b","))
                {
                    return Ok(Rv::Class(Class::QuoteLikeReadline));
                }
                // After a `}`: `\G<(?!\d)\w+>` is almost surely a readline.
                if structure && before == b"}" {
                    let line = self.line();
                    let pos = self.pos();
                    if line.get(pos) == Some(&b'<')
                        && line
                            .get(pos + 1)
                            .is_some_and(|&c| is_word(c) && !c.is_ascii_digit())
                        && line.get(
                            pos + 1 + line[pos + 1..].iter().take_while(|&&c| is_word(c)).count(),
                        ) == Some(&b'>')
                    {
                        return Ok(Rv::Class(Class::QuoteLikeReadline));
                    }
                }
                Ok(Rv::Class(Class::Operator))
            }
            b'/' => {
                if self.token.is_some() {
                    self.finalize();
                }
                let Some(prev) = self.last_significant_token() else {
                    return Ok(Rv::Class(Class::RegexpMatch));
                };
                let before = self.content_of(prev).to_vec();
                if self.doc.isa(prev, Class::Operator) {
                    return Ok(Rv::Class(Class::RegexpMatch));
                }
                if self.doc.isa(prev, Class::Symbol) {
                    return Ok(Rv::Class(Class::Operator));
                }
                if before == b"]" && self.doc.isa(prev, Class::StructureToken) {
                    return Ok(Rv::Class(Class::Operator));
                }
                if self.doc.isa(prev, Class::Number) {
                    return Ok(Rv::Class(Class::Operator));
                }
                if self.doc.isa(prev, Class::StructureToken)
                    && matches!(before.as_slice(), b"(" | b"{" | b";")
                {
                    return Ok(Rv::Class(Class::RegexpMatch));
                }
                if matches!(
                    before.as_slice(),
                    b"return" | b"split" | b"if" | b"unless" | b"grep" | b"map"
                ) && self.doc.isa(prev, Class::Word)
                {
                    return Ok(Rv::Class(Class::RegexpMatch));
                }
                if matches!(self.at(self.cursor + 1), Some(b'^' | b'[' | b'\\')) {
                    return Ok(Rv::Class(Class::RegexpMatch));
                }
                Ok(Rv::Class(Class::Operator))
            }
            b'x' => {
                if self.current_x_is_operator() {
                    let next = self.at(self.cursor + 1);
                    let next2 = self.at(self.cursor + 2);
                    let digit = next.is_some_and(|c| c.is_ascii_digit());
                    let blocked = (next == Some(b'=') && next2 == Some(b'>'))
                        || next.is_some_and(|c| is_word(c) || is_space(c));
                    if digit || !blocked {
                        return Ok(Rv::Class(Class::Operator));
                    }
                }
                self.word_commit()
            }
            b'-' => {
                if self.opcontext() == "operator" {
                    Ok(Rv::Class(Class::Operator))
                } else {
                    Ok(Rv::Class(Class::Unknown))
                }
            }
            _ => Err(format!("Encountered unexpected character '{ch}'")),
        }
    }

    // ---- PPI::Token::BOM.

    fn bom_on_line_start(&mut self) -> R {
        let line = self.line().to_vec();
        let boms: [&[u8]; 5] = [
            b"\x00\x00\xfe\xff",
            b"\xff\xfe\x00\x00",
            b"\xfe\xff",
            b"\xff\xfe",
            b"\xef\xbb\xbf",
        ];
        if let Some(bom) = boms.iter().find(|b| line.starts_with(b)) {
            if *bom != b"\xef\xbb\xbf" {
                return Err(
                    "Can't locate object method \"_error\" via package \"PPI::Tokenizer\""
                        .to_string(),
                );
            }
            self.new_token(Class::Bom, bom)?;
            self.cursor += bom.len() as isize;
        }
        self.class = Class::Whitespace;
        self.whitespace_on_line_start()
    }

    // ---- PPI::Token::Comment.

    fn comment_on_char(&mut self) -> R {
        if self.at(self.cursor) == Some(b'\n') {
            return self.finalize_and_continue();
        }
        Ok(Rv::One)
    }

    fn comment_commit(&mut self) -> R {
        let rest = self.line()[self.pos().min(self.line_length)..].to_vec();
        if let Some(body) = rest.strip_suffix(b"\n") {
            self.new_token(Class::Comment, body)?;
            self.new_token(Class::Whitespace, b"\n")?;
        } else {
            self.new_token(Class::Comment, &rest)?;
        }
        self.cursor = self.line_length as isize - 1;
        Ok(Rv::Zero)
    }

    // ---- PPI::Token::Pod, End, Data.

    fn pod_on_line_start(&mut self) -> Rv {
        let line = self.line().to_vec();
        self.append(&line);
        if pod_command_is(&line, b"cut") {
            self.finalize();
        }
        Rv::Zero
    }

    fn end_on_line_start(&mut self) -> R {
        let line = self.line().to_vec();
        if line.first() == Some(&b'=') && line.get(1).is_some_and(|&c| is_word(c)) {
            self.new_token(Class::Pod, &line)?;
            if !pod_command_is(&line, b"cut") {
                self.class = Class::Pod;
            }
        } else if self.token.is_some() {
            self.append(&line);
        } else {
            self.new_token(Class::End, &line)?;
        }
        Ok(Rv::Zero)
    }

    fn data_on_line_start(&mut self) -> R {
        let line = self.line().to_vec();
        if self.token.is_some() {
            self.append(&line);
        } else {
            let token = self.create(self.class, &line)?;
            self.token = Some(token);
        }
        Ok(Rv::Zero)
    }

    // ---- PPI::Token::Number and subclasses.

    fn number_on_char(&mut self) -> R {
        let c = self.at(self.cursor);
        if c == Some(b'_') {
            return Ok(Rv::One);
        }
        let content = self.token_content();
        let body = content.strip_prefix(b"-").unwrap_or(content);
        if body.first() == Some(&b'0') && body[1..].iter().all(|&c| c == b'_') {
            match c {
                Some(b'x' | b'X') => {
                    self.set_class(Class::NumberHex);
                    return Ok(Rv::One);
                }
                Some(b'b' | b'B') => {
                    self.set_class(Class::NumberBinary);
                    return Ok(Rv::One);
                }
                Some(b'o' | b'O') => {
                    self.set_class(Class::NumberOctal);
                    return Ok(Rv::One);
                }
                Some(d) if d.is_ascii_digit() => {
                    self.set_class(Class::NumberOctal);
                    return Ok(Rv::One);
                }
                _ => {}
            }
        }
        match c {
            Some(d) if d.is_ascii_digit() => Ok(Rv::One),
            Some(b'.') => {
                self.set_class(Class::NumberFloat);
                Ok(Rv::One)
            }
            Some(b'e' | b'E') => {
                self.set_class(Class::NumberExp);
                Ok(Rv::One)
            }
            _ => self.finalize_and_continue(),
        }
    }

    fn binary_on_char(&mut self) -> R {
        match self.at(self.cursor) {
            Some(c) if is_word(c) => Ok(Rv::One),
            _ => self.finalize_and_continue(),
        }
    }

    fn octal_on_char(&mut self) -> R {
        match self.at(self.cursor) {
            Some(b'_') => Ok(Rv::One),
            Some(c) if c.is_ascii_digit() => Ok(Rv::One),
            _ => self.finalize_and_continue(),
        }
    }

    fn hex_on_char(&mut self) -> R {
        match self.at(self.cursor) {
            Some(b'_') => Ok(Rv::One),
            Some(c) if c.is_ascii_hexdigit() => Ok(Rv::One),
            _ => self.finalize_and_continue(),
        }
    }

    fn float_on_char(&mut self) -> R {
        let c = self.at(self.cursor);
        match c {
            Some(b'_') => return Ok(Rv::One),
            Some(d) if d.is_ascii_digit() => return Ok(Rv::One),
            Some(b'.') => {
                let content = self.token_content().to_vec();
                if content.ends_with(b".") {
                    self.truncate_token(content.len() - 1);
                    self.set_class(Class::Number);
                    self.new_token(Class::Operator, b"..")?;
                    return Ok(Rv::Zero);
                }
                if contains_seq(&content, b"._") {
                    let mut parts: Vec<&[u8]> = content.split(|&c| c == b'.').collect();
                    while parts.last().is_some_and(|p| p.is_empty()) {
                        parts.pop();
                    }
                    let number = parts.first().copied().unwrap_or(b"").to_vec();
                    let bareword = parts.get(1).copied().unwrap_or(b"").to_vec();
                    if let Some(token) = self.token {
                        self.doc.elems[token].content = number;
                    }
                    self.set_class(Class::Number);
                    self.new_token(Class::Operator, b".")?;
                    self.new_token(Class::Word, &bareword)?;
                    self.new_token(Class::Operator, b".")?;
                    return Ok(Rv::Zero);
                }
                self.set_class(Class::NumberVersion);
                return Ok(Rv::One);
            }
            _ => {}
        }
        let c2 = self.at(self.cursor + 1);
        if matches!(c, Some(b'e' | b'E')) && matches!(c2, Some(b'0'..=b'9' | b'+' | b'-')) {
            self.set_class(Class::NumberExp);
            return Ok(Rv::One);
        }
        self.finalize_and_continue()
    }

    fn exp_on_char(&mut self) -> R {
        let c = self.at(self.cursor);
        match c {
            Some(b'_') => return Ok(Rv::One),
            Some(d) if d.is_ascii_digit() => return Ok(Rv::One),
            _ => {}
        }
        let content = self.token_content().to_vec();
        if content.last().is_some_and(|&e| e == b'e' || e == b'E') {
            if matches!(c, Some(b'-' | b'+')) {
                return Ok(Rv::One);
            }
            let n = content.len();
            if n >= 2 && content[n - 2] == b'.' {
                let word = [content[n - 1]];
                self.truncate_token(n - 2);
                self.set_class(Class::Number);
                self.new_token(Class::Operator, b".")?;
                self.new_token(Class::Word, &word)?;
                return self.on_char(self.class);
            }
        }
        self.finalize_and_continue()
    }

    fn version_on_char(&mut self) -> R {
        let c = self.at(self.cursor);
        if c.is_some_and(|d| d.is_ascii_digit()) {
            return Ok(Rv::One);
        }
        if c == Some(b'_') {
            let content = self.token_content().to_vec();
            if !content.ends_with(b".") {
                return Ok(Rv::One);
            }
            self.truncate_token(content.len() - 1);
            if !has_two_dots_apart(self.token_content()) {
                self.set_class(Class::NumberFloat);
            }
            self.new_token(Class::Operator, b".")?;
            self.new_token(Class::Word, b"_")?;
            return Ok(Rv::Zero);
        }
        if c == Some(b'.') {
            let content = self.token_content().to_vec();
            if content.ends_with(b".") {
                self.truncate_token(content.len() - 1);
                if !has_two_dots_apart(self.token_content()) {
                    self.set_class(Class::NumberFloat);
                }
                self.new_token(Class::Operator, b"..")?;
                return Ok(Rv::Zero);
            }
            return Ok(Rv::One);
        }
        self.finalize_and_continue()
    }

    fn version_commit(&mut self) -> R {
        let line = self.line().to_vec();
        let pos = self.pos();
        let Some(len) = vstring_length(&line, pos) else {
            return self.word_commit();
        };
        let content = line[pos..pos + len].to_vec();
        if !content.contains(&b'.') && self.current_token_is_forced_word(Some(&content)) {
            return self.word_commit();
        }
        self.cursor += len as isize;
        self.new_token(Class::NumberVersion, &content)?;
        self.finalize_and_continue()
    }

    fn truncate_token(&mut self, len: usize) {
        if let Some(token) = self.token {
            self.doc.elems[token].content.truncate(len);
        }
    }

    // ---- PPI::Token::Word.

    fn word_on_char(&mut self) -> R {
        self.word_recursion += 1;
        if self.word_recursion > 64 {
            return Err(
                "Deep recursion on subroutine \"PPI::Token::Word::__TOKENIZER__on_char\""
                    .to_string(),
            );
        }
        let result = self.word_on_char_inner();
        self.word_recursion -= 1;
        result
    }

    fn word_on_char_inner(&mut self) -> R {
        let line = self.line().to_vec();
        let pos = self.pos();
        if let Some(len) = match_bareword(&line, pos, false) {
            let word = line[pos..pos + keyword_before_quote(&line[pos..pos + len])].to_vec();
            self.append(&word);
            self.cursor += word.len() as isize;
        }
        if self.current_token_is_attribute() {
            self.set_class(Class::Attribute);
            return Err("Can't locate object method \"__TOKENIZER__commit\" via package \"PPI::Token::Attribute\"".to_string());
        }
        let word = self.token_content().to_vec();
        if is_keyword(&word) {
            if self.current_token_is_forced_word(None) {
                self.set_class(Class::Word);
                return self.word_on_char();
            }
            if let Some(class) = quotelike(&word) {
                self.set_class(class);
                return self.on_char(class);
            }
            if is_operator(&word) {
                self.set_class(Class::Operator);
                return self.finalize_and_continue();
            }
        }
        if word.contains(&b':') {
            return self.finalize_and_continue();
        }
        if self.at(self.cursor) == Some(b':') {
            self.append(b":");
            self.cursor += 1;
            self.set_class(Class::Label);
        } else if word == b"_" {
            self.set_class(Class::Magic);
        }
        self.finalize_and_continue()
    }

    fn word_commit(&mut self) -> R {
        let line = self.line().to_vec();
        let pos = self.pos();
        let Some(len) = match_bareword(&line, pos, true) else {
            return Err("Fatal error... regex failed to match when expected".to_string());
        };
        let mut word = line[pos..pos + keyword_before_quote(&line[pos..pos + len])].to_vec();
        self.cursor += word.len() as isize;
        if self.current_token_is_attribute() {
            self.new_token(Class::Attribute, &word)?;
            return if self.cursor >= self.line_length as isize {
                Ok(Rv::Zero)
            } else {
                self.on_char(self.class)
            };
        }
        if word == b"__END__" || word == b"__DATA__" {
            self.new_token(Class::Separator, &word)?;
            self.finalize();
            self.zone = if word == b"__END__" {
                Class::End
            } else {
                Class::Data
            };
            let rest = line[self.pos().min(line.len())..].to_vec();
            self.cursor = line.len() as isize;
            if let Some(body) = rest.strip_suffix(b"\n") {
                if !body.is_empty() {
                    self.new_token(Class::Comment, body)?;
                }
                self.new_token(Class::Whitespace, b"\n")?;
            } else if !rest.is_empty() {
                self.new_token(Class::Comment, &rest)?;
            }
            self.finalize();
            return Ok(Rv::Zero);
        }
        let token_class;
        if word.contains(&b':') || (is_keyword(&word) && self.current_token_is_forced_word(None)) {
            token_class = Class::Word;
        } else if let Some(class) = quotelike(&word) {
            self.new_token(class, &word)?;
            return if self.cursor >= self.line_length as isize {
                Ok(Rv::Zero)
            } else {
                self.on_char(self.class)
            };
        } else if is_operator(&word) && (word != b"x" || self.current_x_is_operator()) {
            token_class = Class::Operator;
        } else {
            let prev = self.previous_significant_tokens(1).first().copied();
            let rest = &line[self.pos().min(line.len())..];
            let spaces = rest.iter().take_while(|&&c| is_space(c)).count();
            if rest.get(spaces) == Some(&b':') && rest.get(spaces + 1) != Some(&b':') {
                if prev.is_some_and(|p| self.content_of(p) == b"sub") {
                    token_class = Class::Word;
                } else if !self.token_isa(prev, Class::Operator) {
                    word.extend_from_slice(&rest[..=spaces]);
                    self.cursor += spaces as isize + 1;
                    token_class = Class::Label;
                } else {
                    token_class = Class::Word;
                }
            } else if word == b"_" {
                token_class = Class::Magic;
            } else {
                token_class = Class::Word;
            }
        }
        self.new_token(token_class, &word)?;
        if self.cursor >= self.line_length as isize {
            self.finalize();
            return Ok(Rv::Zero);
        }
        self.finalize_and_continue()
    }

    /// `__current_token_is_attribute`.
    fn current_token_is_attribute(&self) -> bool {
        self.previous_significant_tokens(1)
            .first()
            .is_some_and(|&t| self.doc.elems[t].attribute_hint || self.doc.isa(t, Class::Attribute))
    }

    // ---- PPI::Token::DashedWord, ArrayIndex, Cast, Symbol, Magic.

    fn dashed_word_on_char(&mut self) -> R {
        let line = self.line().to_vec();
        let pos = self.pos();
        let w = line[pos.min(line.len())..]
            .iter()
            .take_while(|&&c| is_word(c))
            .count();
        if w > 0 {
            self.append(&line[pos..pos + w]);
            self.cursor += w as isize;
        }
        let content = self.token_content();
        let file_test = content.len() == 2
            && content[0] == b'-'
            && b"rwxoRWXOezsfdlpSbctugkTBMAC".contains(&content[1]);
        self.set_class(if file_test {
            Class::Operator
        } else {
            Class::Word
        });
        self.finalize_and_continue()
    }

    fn array_index_on_char(&mut self) -> R {
        let line = self.line().to_vec();
        let pos = self.pos();
        let w = line[pos.min(line.len())..]
            .iter()
            .take_while(|&&c| is_word(c) || c == b':' || c == b'\'')
            .count();
        if w > 0 {
            self.append(&line[pos..pos + w]);
            self.cursor += w as isize;
        }
        self.finalize_and_continue()
    }

    fn cast_on_char(&mut self) -> R {
        let mut candidate = self.token_content().to_vec();
        candidate.extend(self.char_str());
        if matches!(candidate.as_slice(), b"%*" | b"@*" | b"$*" | b"$#*") {
            return Ok(Rv::One);
        }
        self.finalize_and_continue()
    }

    fn symbol_on_char(&mut self) -> R {
        let line = self.line().to_vec();
        let pos = self.pos();
        let w = line[pos.min(line.len())..]
            .iter()
            .take_while(|&&c| is_word(c) || c == b':' || c == b'\'')
            .count();
        if w > 0 {
            self.append(&line[pos..pos + w]);
            self.cursor += w as isize;
        }
        let content = self.token_content().to_vec();
        if content == b"@_" || content == b"$_" {
            self.set_class(Class::Magic);
            return self.finalize_and_continue();
        }
        if content == b"$::" {
            if self.at(self.cursor) == Some(b'|') {
                self.append(b"|");
                self.cursor += 1;
                self.set_class(Class::Magic);
            }
            return self.finalize_and_continue();
        }
        if content.len() >= 3
            && b"$%*@&".contains(&content[0])
            && &content[1..3] == b"::"
            && content.get(3).is_none_or(|&c| !is_word(c))
        {
            let rest = content.len() - 3;
            self.truncate_token(3);
            self.cursor -= rest as isize;
            return self.finalize_and_continue();
        }
        if (content[0] == b'$' || content[0] == b'@')
            && content.get(1).is_some_and(u8::is_ascii_digit)
        {
            self.set_class(Class::Magic);
            return self.finalize_and_continue();
        }
        let Some(len) = symbol_prefix(&content) else {
            return Ok(Rv::Undef);
        };
        if len != content.len() {
            self.cursor += len as isize - content.len() as isize;
            self.truncate_token(len);
        }
        self.finalize_and_continue()
    }

    fn magic_on_char(&mut self) -> R {
        let mut c = self.token_content().to_vec();
        c.extend(self.char_str());
        let last_ok = magic_quick_test(&c);
        if c.first() == Some(&b'$') && last_ok {
            let dollar_underscore = c.len() >= 3 && c[1] == b'_' && (is_word(c[2]) || c[2] == b':');
            let dollar_colons = c.starts_with(b"$::");
            let dollar_quote = c.len() >= 3 && c[1] == b'\'' && is_word(c[2]);
            if dollar_underscore || dollar_colons || dollar_quote {
                if c.len() == 3 && c[1] == b'\'' && c[2].is_ascii_digit() {
                    return self.finalize_and_continue();
                }
                self.set_class(Class::Symbol);
                return self.symbol_on_char();
            }
            if c.len() >= 3 && c[1] == b'$' && is_word(c[2]) {
                self.replace_token(Class::Cast, b"$");
                self.finalize();
                return self.new_token(Class::Symbol, b"$");
            }
            if c == b"$${" && curly_symbol(self.line(), self.pos() + 1).is_some() {
                self.replace_token(Class::Cast, b"$");
                self.finalize();
                return self.new_token(Class::Magic, b"$");
            }
            if c == b"$#$" || c == b"$#{" {
                self.set_class(Class::Cast);
                return self.finalize_and_continue();
            }
            if c.len() >= 3 && c[1] == b'#' && is_word(c[2]) {
                self.replace_token(Class::ArrayIndex, b"$#");
                return self.array_index_on_char();
            }
            // /^\$\^\w+$/, whose `$` also matches before a final newline.
            let body = c.strip_suffix(b"\n").unwrap_or(&c);
            if body.len() >= 3 && body[1] == b'^' && body[2..].iter().all(|&x| is_word(x)) {
                let next = self.at(self.cursor + 1);
                if is_magic(&c)
                    && (next.is_none() || next == Some(b'0') || !next.is_some_and(is_word))
                {
                    if let Some(token) = self.token {
                        self.doc.elems[token].content.clone_from(&c);
                    }
                    self.cursor += 1;
                } else {
                    return Ok(Rv::One);
                }
            }
            if c.starts_with(b"$#{") {
                self.replace_token(Class::Cast, b"$#");
                self.finalize();
                return self.new_token(Class::StructureToken, b"{");
            }
        } else if c.starts_with(b"%^") {
            if c == b"%^" {
                return Ok(Rv::One);
            }
            if is_magic(&c) {
                if let Some(token) = self.token {
                    self.doc.elems[token].content.clone_from(&c);
                }
                self.cursor += 1;
            } else {
                if let Some(token) = self.token {
                    self.doc.elems[token].content.pop();
                    self.doc.elems[token].class = Class::Operator;
                }
                self.class = Class::Operator;
                self.cursor -= 1;
            }
        }
        if is_magic(&c) {
            let current = self.token_content().len();
            self.cursor += c.len() as isize - current as isize;
            if let Some(token) = self.token {
                self.doc.elems[token].content = c;
            }
        } else {
            let line = self.line().to_vec();
            let pos = self.pos();
            if let Some(len) = curly_symbol(&line, pos) {
                self.append(&line[pos..pos + len]);
                self.cursor += len as isize;
            } else if magic_capture_variable(&c) {
                let digits = line[pos.min(line.len())..]
                    .iter()
                    .take_while(|d| d.is_ascii_digit())
                    .count();
                if digits > 0 {
                    self.append(&line[pos..pos + digits]);
                    self.cursor += digits as isize;
                }
            }
        }
        self.finalize_and_continue()
    }

    /// `$t->{token} = PPI::Token::X->new(...)`: a fresh token in place of
    /// the current one, the dispatch class unchanged.
    fn replace_token(&mut self, class: Class, content: &[u8]) {
        let token = self.doc.alloc(class, content);
        self.token = Some(token);
    }

    // ---- PPI::Token::Operator.

    fn operator_on_char(&mut self) -> R {
        let content = self.token_content().to_vec();
        let line = self.line().to_vec();
        let pos = self.pos();
        if content.len() < 4 {
            let mut candidate = content.clone();
            let take = 4 - content.len();
            candidate.extend_from_slice(&line[pos.min(line.len())..(pos + take).min(line.len())]);
            if candidate == b"<<>>" {
                return Ok(Rv::One);
            }
        }
        let mut with_char = content.clone();
        with_char.extend(self.char_str());
        if is_operator(&with_char) {
            return Ok(Rv::One);
        }
        let c = self.at(self.cursor);
        if content == b"." && c.is_some_and(|d| d.is_ascii_digit()) {
            self.set_class(Class::NumberFloat);
            return self.float_on_char();
        }
        if content == b"<<" {
            let rest = &line[pos.min(line.len())..];
            let rest = rest.strip_prefix(b"~").unwrap_or(rest);
            let spaces = rest.iter().take_while(|&&c| is_space(c)).count();
            let heredoc = rest
                .first()
                .is_some_and(|&c| is_word(c) && !c.is_ascii_digit())
                || matches!(rest.get(spaces), Some(b'\'' | b'"' | b'`'))
                || (rest.first() == Some(&b'\\') && rest.get(1).is_some_and(|&c| is_word(c)));
            if heredoc {
                self.set_class(Class::HereDoc);
                return self.heredoc_on_char();
            }
        }
        if content == b"<>" || content == b"<<>>" {
            self.set_class(Class::QuoteLikeReadline);
        }
        self.finalize_and_continue()
    }

    // ---- PPI::Token::Unknown.

    fn unknown_on_char(&mut self) -> R {
        let c = self.token_content().to_vec();
        let ch = self.at(self.cursor);
        let after_arrow = |p: &Self| {
            p.previous_significant_tokens(1)
                .first()
                .is_some_and(|&t| p.doc.isa(t, Class::Operator) && p.content_of(t) == b"->")
        };
        let after_number = |p: &Self| {
            p.previous_significant_tokens(1)
                .first()
                .is_some_and(|&t| p.doc.isa(t, Class::Number))
        };
        let digit = ch.is_some_and(|d| d.is_ascii_digit());
        let word_or_colon = ch.is_some_and(|d| is_word(d) || d == b':');
        let with = |extra: Option<u8>| {
            let mut s = c.clone();
            s.extend(extra);
            s
        };
        match c.as_slice() {
            b"*" => {
                if digit {
                    self.set_class(Class::Operator);
                    return self.finalize_and_continue();
                }
                if word_or_colon && !after_number(self) {
                    self.set_class(Class::Symbol);
                    return Ok(Rv::One);
                }
                if ch == Some(b'{') && curly_symbol(self.line(), self.pos() + 1).is_some() {
                    self.set_class(Class::Magic);
                    return Ok(Rv::One);
                }
                if ch == Some(b'*') && after_arrow(self) {
                    self.set_class(Class::Cast);
                    return Ok(Rv::One);
                }
                if ch == Some(b'*') || ch == Some(b'=') {
                    self.set_class(Class::Operator);
                    return Ok(Rv::One);
                }
                if is_cast_or_op(ch) {
                    return self.as_cast_or_op();
                }
                self.set_class(Class::Operator);
                self.finalize_and_continue()
            }
            b"$" => {
                if matches!(ch, Some(b'*' | b'#')) && after_arrow(self) {
                    self.set_class(Class::Cast);
                    return Ok(Rv::One);
                }
                if ch.is_some_and(|d| d.is_ascii_alphabetic() || d == b'_') {
                    self.set_class(Class::Symbol);
                    return Ok(Rv::One);
                }
                if matches!(ch, Some(b')' | b'=' | b','))
                    && self.current_token_has_signatures_active()?.0
                {
                    self.set_class(Class::Symbol);
                    return self.finalize_and_continue();
                }
                if is_magic(&with(ch)) {
                    self.set_class(Class::Magic);
                    return Ok(Rv::One);
                }
                if ch == Some(b'{') && curly_symbol(self.line(), self.pos() + 1).is_some() {
                    self.set_class(Class::Magic);
                    return Ok(Rv::One);
                }
                self.set_class(Class::Cast);
                self.finalize_and_continue()
            }
            b"@" => {
                if ch == Some(b'*') && after_arrow(self) {
                    self.set_class(Class::Cast);
                    return Ok(Rv::One);
                }
                if word_or_colon {
                    self.set_class(Class::Symbol);
                    return Ok(Rv::One);
                }
                if ch == Some(b')') && self.current_token_has_signatures_active()?.0 {
                    self.set_class(Class::Symbol);
                    return self.finalize_and_continue();
                }
                if is_magic(&with(ch)) {
                    self.set_class(Class::Magic);
                    return Ok(Rv::One);
                }
                if ch == Some(b'{') && curly_symbol(self.line(), self.pos() + 1).is_some() {
                    self.set_class(Class::Magic);
                    return Ok(Rv::One);
                }
                self.set_class(Class::Cast);
                self.finalize_and_continue()
            }
            b"%" => {
                if matches!(ch, Some(b'*' | b'[')) && after_arrow(self) {
                    self.set_class(Class::Cast);
                    if ch == Some(b'*') {
                        return Ok(Rv::One);
                    }
                    return self.finalize_and_continue();
                }
                if digit {
                    self.set_class(Class::Operator);
                    return self.finalize_and_continue();
                }
                if ch == Some(b')') && self.current_token_has_signatures_active()?.0 {
                    self.set_class(Class::Symbol);
                    return self.finalize_and_continue();
                }
                if ch == Some(b'^') || is_magic(&with(ch)) {
                    self.set_class(Class::Magic);
                    return Ok(Rv::One);
                }
                if word_or_colon && !after_number(self) {
                    self.set_class(Class::Symbol);
                    return Ok(Rv::One);
                }
                if ch == Some(b'{') && curly_symbol(self.line(), self.pos() + 1).is_some() {
                    self.set_class(Class::Magic);
                    return Ok(Rv::One);
                }
                if is_cast_or_op(ch) {
                    return self.as_cast_or_op();
                }
                self.set_class(Class::Operator);
                self.operator_on_char()
            }
            b"&" => {
                if ch == Some(b'*') && after_arrow(self) {
                    self.set_class(Class::Cast);
                    return Ok(Rv::One);
                }
                if digit {
                    self.set_class(Class::Operator);
                    return self.finalize_and_continue();
                }
                if word_or_colon && !after_number(self) {
                    self.set_class(Class::Symbol);
                    return Ok(Rv::One);
                }
                if is_cast_or_op(ch) {
                    return self.as_cast_or_op();
                }
                self.set_class(Class::Operator);
                self.operator_on_char()
            }
            b"-" => {
                if digit {
                    self.set_class(Class::Number);
                    return Ok(Rv::One);
                }
                if ch == Some(b'.') {
                    self.set_class(Class::NumberFloat);
                    return Ok(Rv::One);
                }
                if ch.is_some_and(|d| d.is_ascii_alphabetic()) {
                    self.set_class(Class::DashedWord);
                    return Ok(Rv::One);
                }
                self.set_class(Class::Operator);
                self.operator_on_char()
            }
            b":" => {
                if ch == Some(b':') {
                    self.set_class(Class::Word);
                    return Ok(Rv::One);
                }
                if self.is_an_attribute() {
                    self.set_class(Class::Operator);
                    if let Some(token) = self.token {
                        self.doc.elems[token].attribute_hint = true;
                    }
                    return self.finalize_and_continue();
                }
                self.set_class(Class::Operator);
                self.operator_on_char()
            }
            _ => Err("Unknown value in PPI::Token::Unknown token".to_string()),
        }
    }

    fn as_cast_or_op(&mut self) -> R {
        let class = self.cast_or_op();
        self.set_class(class);
        self.finalize_and_continue()
    }

    /// `_cast_or_op`.
    fn cast_or_op(&self) -> Class {
        let tokens = &self.tokens;
        let sig = |cursor: &mut isize, extra: &mut dyn FnMut(Id) -> bool| -> Option<Id> {
            while *cursor >= 0 {
                let token = tokens[*cursor as usize];
                *cursor -= 1;
                if !self.doc.significant(token) {
                    continue;
                }
                if !extra(token) {
                    continue;
                }
                return Some(token);
            }
            None
        };
        let mut cursor = tokens.len() as isize - 1;
        let Some(token) = sig(&mut cursor, &mut |_| true) else {
            return Class::Cast;
        };
        if self.doc.isa(token, Class::StructureToken) && self.content_of(token) == b"}" {
            let mut depth = 1;
            let found = sig(&mut cursor, &mut |t| {
                if !self.doc.isa(t, Class::StructureToken) {
                    return false;
                }
                match self.content_of(t) {
                    b"}" => {
                        depth += 1;
                        false
                    }
                    b"{" => {
                        depth -= 1;
                        depth == 0
                    }
                    _ => true,
                }
            });
            if found.is_none() {
                return Class::Operator;
            }
            let Some(token) = sig(&mut cursor, &mut |_| true) else {
                return Class::Operator;
            };
            if self.doc.isa(token, Class::Symbol) {
                return Class::Operator;
            }
            let content = self.content_of(token);
            if matches!(content, b"->" | b"}" | b"]") {
                return Class::Operator;
            }
            let produces =
                self.doc.isa(token, Class::Word) && (content == b"do" || content == b"eval");
            return if produces {
                Class::Operator
            } else {
                Class::Cast
            };
        }
        let content = self.content_of(token);
        if (self.doc.isa(token, Class::StructureToken)
            && matches!(content, b";" | b"(" | b"{" | b"["))
            || self.doc.isa(token, Class::Cast)
            || self.doc.isa(token, Class::Operator)
            || self.doc.isa(token, Class::Label)
        {
            return Class::Cast;
        }
        if !self.doc.isa(token, Class::Word) {
            return Class::Operator;
        }
        match sig(&mut cursor, &mut |_| true) {
            Some(t) if self.content_of(t) == b"->" => Class::Operator,
            _ => Class::Cast,
        }
    }

    /// `__TOKENIZER__is_an_attribute`.
    fn is_an_attribute(&self) -> bool {
        let tokens = self.previous_significant_tokens(3);
        let Some(&p0) = tokens.first() else {
            return false;
        };
        if self.doc.isa(p0, Class::Attribute) || self.doc.isa(p0, Class::Prototype) {
            return true;
        }
        if !self.doc.isa(p0, Class::Word) {
            return false;
        }
        if self.content_of(p0) == b"sub" {
            return true;
        }
        let p1 = tokens.get(1).copied();
        let p2 = tokens.get(2).copied();
        self.token_is(p1, Class::Word, b"sub")
            && (p2.is_none()
                || self.token_isa(p2, Class::StructureToken)
                || self.token_is(p2, Class::Whitespace, b""))
    }

    // ---- PPI::Token::Attribute.

    fn attribute_on_char(&mut self) -> R {
        if self.at(self.cursor) != Some(b'(') {
            return self.finalize_and_continue();
        }
        match self.attribute_scan_for_end() {
            Scan::Eof(s) => {
                self.append(&s);
                self.finalize();
                Ok(Rv::Zero)
            }
            Scan::Found(s) => {
                self.append(&s);
                self.finalize_and_continue()
            }
            Scan::Undef => Ok(Rv::Undef),
        }
    }

    fn attribute_scan_for_end(&mut self) -> Scan {
        let mut string = Vec::new();
        let mut depth: i64 = 0;
        while self.line.is_some() {
            let line = self.line().to_vec();
            let pos = self.pos().min(line.len());
            let Some(end) = find_attribute_paren(&line, pos) else {
                string.extend_from_slice(&line[pos..]);
                if !self.fill_line(true) {
                    return Scan::Eof(string);
                }
                self.cursor = 0;
                continue;
            };
            let m = &line[pos..=end];
            string.extend_from_slice(m);
            self.cursor += m.len() as isize;
            depth += if line[end] == b'(' { 1 } else { -1 };
            if depth != 0 {
                continue;
            }
            return Scan::Found(string);
        }
        Scan::Eof(string)
    }

    // ---- PPI::Token::HereDoc.

    fn heredoc_on_char(&mut self) -> R {
        let line = self.line().to_vec();
        let pos = self.pos().min(line.len());
        let Some(len) = heredoc_introducer(&line, pos) else {
            self.set_class(Class::Operator);
            return self.finalize_and_continue();
        };
        self.append(&line[pos..pos + len]);
        self.cursor += len as isize;
        let content = self.token_content().to_vec();
        let Some((indented, terminator)) = heredoc_terminator(&content) else {
            return Ok(Rv::Undef);
        };
        let mut heredoc: Vec<Vec<u8>> = Vec::new();
        let mut with_newline = terminator.clone();
        with_newline.push(b'\n');
        let mut damaged = true;
        while let Some(l) = self.get_line() {
            if is_terminator(&with_newline, &l, indented) {
                if indented {
                    let indent = leading_space(&l);
                    if !heredoc.iter().all(|h| h.starts_with(&indent) || h == b"\n") {
                        heredoc.push(l);
                        break;
                    }
                    for h in &mut heredoc {
                        if h.starts_with(&indent) {
                            h.drain(..indent.len());
                        }
                    }
                }
                damaged = false;
                break;
            }
            heredoc.push(l);
        }
        if damaged {
            let mut terminator_line = None;
            if !heredoc.is_empty() {
                if self.source_eof_chop {
                    heredoc.last_mut().map(Vec::pop);
                    self.source_eof_chop = false;
                }
                if heredoc
                    .last()
                    .is_some_and(|l| is_terminator(&terminator, l, indented))
                {
                    terminator_line = heredoc.pop();
                }
            }
            if indented && let Some(t) = terminator_line {
                let indent = leading_space(&t);
                if heredoc.iter().all(|h| h.starts_with(&indent) || h == b"\n") {
                    for h in &mut heredoc {
                        if h.starts_with(&indent) {
                            h.drain(..indent.len());
                        }
                    }
                }
            }
        }
        if let Some(token) = self.token {
            self.doc.elems[token].heredoc = Some(Box::new(HereDocData { heredoc }));
        }
        self.finalize_and_continue()
    }

    // ---- PPI::Token::_QuoteEngine.

    fn quote_engine_on_char(&mut self) -> Rv {
        let Some(token) = self.token else {
            return Rv::Undef;
        };
        let class = self.doc.class(token);
        let ok = if simple_quote(class) && self.doc.elems[token].quote.is_some() {
            self.simple_fill(token)
        } else {
            self.full_fill(token)
        };
        if !ok {
            return Rv::Undef;
        }
        self.finalize();
        Rv::Zero
    }

    fn quote_mut(&mut self, token: Id) -> &mut QuoteData {
        self.doc.elems[token].quote.get_or_insert_with(Box::default)
    }

    /// `_QuoteEngine::Simple::_fill`: false for undef.
    fn simple_fill(&mut self, token: Id) -> bool {
        let separator = self.doc.elems[token]
            .quote
            .as_ref()
            .and_then(|q| q.separator);
        match self.scan_for_unescaped_character(separator) {
            Scan::Undef => false,
            Scan::Eof(s) | Scan::Found(s) => {
                self.doc.elems[token].content.extend(s);
                true
            }
        }
    }

    /// `_QuoteEngine::Full::_fill`: false for undef.
    fn full_fill(&mut self, token: Id) -> bool {
        let has_operator = self.doc.elems[token]
            .quote
            .as_ref()
            .is_some_and(|q| q.operator.is_some());
        if has_operator {
            if self.at(self.cursor).is_some_and(is_space) {
                match self.scan_quote_like_operator_gap() {
                    Scan::Undef => return false,
                    Scan::Eof(gap) => {
                        self.doc.elems[token].content.extend(gap);
                        return true;
                    }
                    Scan::Found(gap) => self.doc.elems[token].content.extend(gap),
                }
            }
            let sep = self.at(self.cursor);
            self.cursor += 1;
            self.doc.elems[token].content.extend(sep);
            let quote = self.quote_mut(token);
            match sep {
                Some(open) if is_section_open(open) => {
                    quote.braced = Some(true);
                    if quote.sections.is_empty() {
                        quote.sections.push(braced_section(open));
                    } else {
                        quote.sections[0] = braced_section(open);
                    }
                }
                _ => {
                    quote.braced = Some(false);
                    quote.separator = sep;
                }
            }
        }
        let braced = self.doc.elems[token]
            .quote
            .as_ref()
            .and_then(|q| q.braced)
            .unwrap_or(false);
        let rv = if braced {
            self.fill_braced(token)
        } else {
            self.fill_normal(token)
        };
        match rv {
            None => return false,
            Some(false) => return true,
            Some(true) => {}
        }
        if self.doc.elems[token]
            .quote
            .as_ref()
            .is_some_and(|q| q.modifiers.is_some())
        {
            while let Some(c) = self.at(self.cursor + 1).filter(u8::is_ascii_alphabetic) {
                self.doc.elems[token].content.push(c);
                let modifiers = self.quote_mut(token).modifiers.get_or_insert_with(Vec::new);
                let lower = c.to_ascii_lowercase();
                if !modifiers.contains(&lower) {
                    modifiers.push(lower);
                }
                self.cursor += 1;
            }
        }
        true
    }

    fn content_len(&self, token: Id) -> usize {
        self.doc.elems[token].content.len()
    }

    fn set_section(&mut self, token: Id, index: usize, section: Section) {
        let quote = self.quote_mut(token);
        while quote.sections.len() <= index {
            quote.sections.push(Section::default());
        }
        quote.sections[index] = section;
    }

    /// `_fill_normal`: `None` for undef, else whether it completed.
    fn fill_normal(&mut self, token: Id) -> Option<bool> {
        let separator = self.doc.elems[token]
            .quote
            .as_ref()
            .and_then(|q| q.separator);
        let kind = separator.map(|s| vec![s, s]);
        match self.scan_for_unescaped_character(separator) {
            Scan::Undef => return None,
            Scan::Eof(s) => {
                if s.len() > 1 {
                    let section = Section {
                        position: self.content_len(token),
                        size: s.len() - 1,
                        kind,
                        close: None,
                    };
                    self.set_section(token, 0, section);
                    self.quote_mut(token).n_sections = 1;
                } else {
                    let quote = self.quote_mut(token);
                    quote.sections.clear();
                    quote.n_sections = 0;
                }
                self.doc.elems[token].content.extend(s);
                return Some(false);
            }
            Scan::Found(s) => {
                let section = Section {
                    position: self.content_len(token),
                    size: s.len() - 1,
                    kind: kind.clone(),
                    close: None,
                };
                self.set_section(token, 0, section);
                self.doc.elems[token].content.extend(s);
            }
        }
        if self.quote_mut(token).n_sections == 1 {
            return Some(true);
        }
        self.cursor += 1;
        match self.scan_for_unescaped_character(separator) {
            Scan::Undef => None,
            Scan::Eof(s) => {
                if s.len() > 1 {
                    let section = Section {
                        position: self.content_len(token),
                        size: s.len() - 1,
                        kind,
                        close: None,
                    };
                    self.set_section(token, 1, section);
                } else {
                    self.quote_mut(token).n_sections = 1;
                }
                self.doc.elems[token].content.extend(s);
                Some(false)
            }
            Scan::Found(s) => {
                let section = Section {
                    position: self.content_len(token),
                    size: s.len() - 1,
                    kind: None,
                    close: None,
                };
                self.set_section(token, 1, section);
                self.doc.elems[token].content.extend(s);
                Some(true)
            }
        }
    }

    /// `_fill_braced`: `None` for undef, else whether it completed.
    fn fill_braced(&mut self, token: Id) -> Option<bool> {
        let (close, kind) = {
            let quote = self.quote_mut(token);
            let section = quote.sections.first()?;
            (section.close?, section.kind.clone())
        };
        match self.scan_for_brace_character(close) {
            Scan::Undef => return None,
            Scan::Eof(s) => {
                if s.len() > 1 {
                    let section = Section {
                        position: self.content_len(token),
                        size: s.len() - 1,
                        kind,
                        close: None,
                    };
                    self.set_section(token, 0, section);
                    self.quote_mut(token).n_sections = 1;
                } else {
                    let quote = self.quote_mut(token);
                    quote.sections.clear();
                    quote.n_sections = 0;
                }
                self.doc.elems[token].content.extend(s);
                return Some(false);
            }
            Scan::Found(s) => {
                let position = self.content_len(token);
                let quote = self.quote_mut(token);
                quote.sections[0].position = position;
                quote.sections[0].size = s.len() - 1;
                quote.sections[0].close = None;
                self.doc.elems[token].content.extend(s);
            }
        }
        if self.quote_mut(token).n_sections == 1 {
            return Some(true);
        }
        self.cursor += 1;
        let mut ch = self.at(self.cursor);
        if ch.is_some_and(is_space) {
            match self.scan_quote_like_operator_gap() {
                Scan::Undef => return None,
                Scan::Eof(gap) => {
                    self.doc.elems[token].content.extend(gap);
                    return Some(false);
                }
                Scan::Found(gap) => self.doc.elems[token].content.extend(gap),
            }
            ch = self.at(self.cursor);
        }
        match ch {
            Some(open) if is_section_open(open) => {
                self.doc.elems[token].content.push(open);
                self.cursor += 1;
                let kind = Some(vec![open, closing_brace(open)]);
                match self.scan_for_brace_character(closing_brace(open)) {
                    Scan::Undef => return None,
                    Scan::Eof(s) => {
                        if s.len() > 1 {
                            let section = Section {
                                position: self.content_len(token),
                                size: s.len() - 1,
                                kind,
                                close: None,
                            };
                            self.set_section(token, 1, section);
                            self.quote_mut(token).n_sections = 2;
                        } else {
                            self.quote_mut(token).n_sections = 1;
                        }
                        self.doc.elems[token].content.extend(s);
                        return Some(false);
                    }
                    Scan::Found(s) => {
                        let section = Section {
                            position: self.content_len(token),
                            size: s.len() - 1,
                            kind,
                            close: None,
                        };
                        self.set_section(token, 1, section);
                        self.doc.elems[token].content.extend(s);
                    }
                }
            }
            Some(sep) if !is_word(sep) && !is_space(sep) => {
                self.doc.elems[token].content.push(sep);
                self.cursor += 1;
                let kind = Some(vec![sep, sep]);
                match self.scan_for_unescaped_character(Some(sep)) {
                    Scan::Undef => return None,
                    Scan::Eof(s) => {
                        if s.len() > 1 {
                            let section = Section {
                                position: self.content_len(token),
                                size: s.len() - 1,
                                kind,
                                close: None,
                            };
                            self.set_section(token, 1, section);
                        } else {
                            self.quote_mut(token).n_sections = 1;
                        }
                        self.doc.elems[token].content.extend(s);
                        return Some(false);
                    }
                    Scan::Found(s) => {
                        let section = Section {
                            position: self.content_len(token),
                            size: s.len() - 1,
                            kind,
                            close: None,
                        };
                        self.set_section(token, 1, section);
                        self.doc.elems[token].content.extend(s);
                    }
                }
            }
            _ => {
                let section = Section {
                    position: self.content_len(token),
                    size: 0,
                    kind: Some(Vec::new()),
                    close: None,
                };
                self.set_section(token, 1, section);
                self.cursor -= 1;
                return Some(false);
            }
        }
        Some(true)
    }

    /// `_scan_for_unescaped_character`.
    fn scan_for_unescaped_character(&mut self, target: Option<u8>) -> Scan {
        let Some(target) = target else {
            return Scan::Undef;
        };
        let mut string = Vec::new();
        while self.line.is_some() {
            let line = self.line().to_vec();
            let pos = self.pos().min(line.len());
            if let Some(end) = find_unescaped(&line, pos, |c| c == target) {
                let m = &line[pos..=end];
                self.cursor += m.len() as isize - 1;
                string.extend_from_slice(m);
                return Scan::Found(string);
            }
            string.extend_from_slice(&line[pos..]);
            if !self.fill_line(true) {
                return Scan::Eof(string);
            }
            self.cursor = 0;
        }
        Scan::Undef
    }

    /// `_scan_for_brace_character`.
    fn scan_for_brace_character(&mut self, close: u8) -> Scan {
        let open = match close {
            b'>' => b'<',
            b')' => b'(',
            b'}' => b'{',
            _ => b'[',
        };
        let mut string = Vec::new();
        let mut depth: i64 = 1;
        while self.line.is_some() {
            let line = self.line().to_vec();
            let pos = self.pos().min(line.len());
            let Some(end) = find_unescaped(&line, pos, |c| c == open || c == close) else {
                string.extend_from_slice(&line[pos..]);
                if self.fill_line(true) {
                    self.cursor = 0;
                    continue;
                }
                return Scan::Eof(string);
            };
            let m = &line[pos..=end];
            string.extend_from_slice(m);
            self.cursor += m.len() as isize;
            depth += if line[end] == open { 1 } else { -1 };
            if depth != 0 {
                continue;
            }
            self.cursor -= 1;
            return Scan::Found(string);
        }
        Scan::Eof(string)
    }

    /// `_scan_quote_like_operator_gap`.
    fn scan_quote_like_operator_gap(&mut self) -> Scan {
        let mut string = Vec::new();
        while self.line.is_some() {
            let line = self.line().to_vec();
            let pos = self.pos().min(line.len());
            let len = gap_length(&line, pos);
            string.extend_from_slice(&line[pos..pos + len]);
            if pos + len != line.len() {
                self.cursor += len as isize;
                return Scan::Found(string);
            }
            if !self.fill_line(true) {
                return Scan::Eof(string);
            }
            self.cursor = 0;
        }
        Scan::Undef
    }
}

/// `^__(?:DATA|END)__\s*$`.
fn is_data_or_end_line(line: &[u8]) -> bool {
    let rest = line
        .strip_prefix(b"__DATA__")
        .or_else(|| line.strip_prefix(b"__END__"));
    rest.is_some_and(|r| r.iter().all(|&c| is_space(c)))
}

/// Whether a line is the POD command `=<name>` (`/^=(\w+)/`, `$1 eq name`).
fn pod_command_is(line: &[u8], name: &[u8]) -> bool {
    let w = line[1.min(line.len())..]
        .iter()
        .take_while(|&&c| is_word(c))
        .count();
    line.first() == Some(&b'=') && &line[1..=w] == name
}

fn contains_seq(s: &[u8], what: &[u8]) -> bool {
    s.windows(what.len()).any(|w| w == what)
}

/// `/\..+\./`: two dots with something between.
fn has_two_dots_apart(s: &[u8]) -> bool {
    let Some(first) = s.iter().position(|&c| c == b'.') else {
        return false;
    };
    s[first + 1..]
        .iter()
        .enumerate()
        .any(|(i, &c)| c == b'.' && i > 0 && !s[first + 1..first + 1 + i].contains(&b'\n'))
}

/// The v-string of `Number::Version::__TOKENIZER__commit`:
/// `v\d[_\d]*(?:\.\d[_\d]*)+|v\d[_\d]*\b`.
fn vstring_length(line: &[u8], pos: usize) -> Option<usize> {
    let digits = |i: usize| {
        line[i.min(line.len())..]
            .iter()
            .take_while(|&&c| c == b'_' || c.is_ascii_digit())
            .count()
    };
    if line.get(pos) != Some(&b'v') || !line.get(pos + 1).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let first_end = pos + 1 + digits(pos + 1);
    let mut end = first_end;
    let mut parts = 0;
    while line.get(end) == Some(&b'.') && line.get(end + 1).is_some_and(u8::is_ascii_digit) {
        end += 1 + digits(end + 1);
        parts += 1;
    }
    if parts > 0 {
        return Some(end - pos);
    }
    if line.get(first_end).is_none_or(|&c| !is_word(c)) {
        return Some(first_end - pos);
    }
    None
}

/// The length of the symbol PPI keeps: `^([\$@%&*](?::(?!:)|(?:::|'(?!\d))?\w+(?:(?:::|'(?!\d))\w+)*(?:::)?))`.
fn symbol_prefix(content: &[u8]) -> Option<usize> {
    if content.is_empty() || !b"$@%&*".contains(&content[0]) {
        return None;
    }
    if content.get(1) == Some(&b':') && content.get(2) != Some(&b':') {
        return Some(2);
    }
    let word = |i: usize| {
        content[i.min(content.len())..]
            .iter()
            .take_while(|&&c| is_word(c))
            .count()
    };
    let sep = |i: usize| -> Option<usize> {
        if content[i.min(content.len())..].starts_with(b"::") {
            Some(i + 2)
        } else if content.get(i) == Some(&b'\'')
            && !content.get(i + 1).is_some_and(u8::is_ascii_digit)
        {
            Some(i + 1)
        } else {
            None
        }
    };
    let rest = |start: usize| -> usize {
        let mut k = start + word(start);
        while let Some(s) = sep(k) {
            let w = word(s);
            if w == 0 {
                break;
            }
            k = s + w;
        }
        if content[k..].starts_with(b"::") {
            k += 2;
        }
        k
    };
    if let Some(s) = sep(1)
        && word(s) > 0
    {
        return Some(rest(s));
    }
    (word(1) > 0).then(|| rest(1))
}

/// Magic's quick test: `/^\$.*[\w:\$\{]$/`.
fn magic_quick_test(c: &[u8]) -> bool {
    if c.first() != Some(&b'$') {
        return false;
    }
    let body = c.strip_suffix(b"\n").unwrap_or(c);
    if body.len() < 2 || body[1..body.len() - 1].contains(&b'\n') {
        return false;
    }
    let last = body[body.len() - 1];
    is_word(last) || last == b':' || last == b'$' || last == b'{'
}

const fn is_cast_or_op(c: Option<u8>) -> bool {
    matches!(c, Some(b'$' | b'@' | b'%' | b'*' | b'{'))
}

/// `\G((?:\\.|[^()])*?[()])` from `pos`: the paren's index. The lazy loop
/// honours `\\.` escapes only while an unescaped paren still follows.
fn find_attribute_paren(line: &[u8], pos: usize) -> Option<usize> {
    let mut p = pos;
    while p < line.len() {
        let c = line[p];
        if c == b'(' || c == b')' {
            return Some(p);
        }
        if c == b'\\'
            && p + 1 < line.len()
            && line[p + 1] != b'\n'
            && line[p + 2..].iter().any(|&x| x == b'(' || x == b')')
        {
            p += 2;
            continue;
        }
        p += 1;
    }
    None
}

/// `HereDoc`'s introducer: `~?`, spaces, then a double-, single- or
/// back-quoted terminator, or `\\?\w+`.
fn heredoc_introducer(line: &[u8], pos: usize) -> Option<usize> {
    let rest = &line[pos..];
    let mut i = usize::from(rest.first() == Some(&b'~'));
    i += rest[i..].iter().take_while(|&&c| is_space(c)).count();
    match rest.get(i) {
        Some(&q @ (b'"' | b'\'' | b'`')) => {
            let close = rest[i + 1..].iter().position(|&c| c == q)?;
            Some(i + close + 2)
        }
        Some(b'\\') => {
            let w = rest[i + 1..].iter().take_while(|&&c| is_word(c)).count();
            if w > 0 { Some(i + 1 + w) } else { None }
        }
        _ => {
            let w = rest[i..].iter().take_while(|&&c| is_word(c)).count();
            if w > 0 { Some(i + w) } else { None }
        }
    }
}

/// The mode regexes of `HereDoc::__TOKENIZER__on_char`: whether the
/// heredoc is indented, and its terminator.
fn heredoc_terminator(content: &[u8]) -> Option<(bool, Vec<u8>)> {
    let rest = content.strip_prefix(b"<<")?;
    let (indented, rest) = match rest.strip_prefix(b"~") {
        Some(r) => (true, r),
        None => (false, rest),
    };
    if !rest.is_empty() && rest.iter().all(|&c| is_word(c)) {
        return Some((indented, rest.to_vec()));
    }
    let trimmed = &rest[rest.iter().take_while(|&&c| is_space(c)).count()..];
    for (q, escaped) in [
        (b'\'', &b"\\'"[..]),
        (b'"', &b"\\\""[..]),
        (b'`', &b"\\`"[..]),
    ] {
        if trimmed.len() >= 2 && trimmed[0] == q && trimmed[trimmed.len() - 1] == q {
            let inner = &trimmed[1..trimmed.len() - 1];
            if inner.contains(&b'\n') {
                continue;
            }
            let mut out = Vec::new();
            let mut i = 0;
            while i < inner.len() {
                if inner[i..].starts_with(escaped) {
                    out.push(q);
                    i += 2;
                } else {
                    out.push(inner[i]);
                    i += 1;
                }
            }
            return Some((indented, out));
        }
    }
    rest.strip_prefix(b"\\")
        .filter(|word| !word.is_empty() && word.iter().all(|&c| is_word(c)))
        .map(|word| (indented, word.to_vec()))
}

/// `_is_terminator`.
fn is_terminator(terminator: &[u8], line: &[u8], indented: bool) -> bool {
    if !indented {
        return line == terminator;
    }
    // /^\s*\Q$terminator\E$/: `\s*` takes any prefix of the leading
    // whitespace, and `$` also matches before a final newline.
    let lead = line.iter().take_while(|&&c| is_space(c)).count();
    (0..=lead).any(|k| {
        let rest = &line[k..];
        rest == terminator || rest.strip_suffix(b"\n") == Some(terminator)
    })
}

/// `/^\$\d+$/` (with `$` before a final newline).
fn magic_capture_variable(c: &[u8]) -> bool {
    let body = c.strip_suffix(b"\n").unwrap_or(c);
    body.len() >= 2 && body[0] == b'$' && body[1..].iter().all(u8::is_ascii_digit)
}

fn leading_space(line: &[u8]) -> Vec<u8> {
    line.iter().take_while(|&&c| is_space(c)).copied().collect()
}

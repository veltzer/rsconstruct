//! The Dockerfile parser: a port of language-docker 16.0.0 (the Haskell
//! library hadolint parses with). Instructions, their flags and arguments
//! come out the way that library's megaparsec grammar reads them, escaped
//! line breaks and heredocs included, so that the rules see the same tree.
//!
//! The grammar is reproduced combinator by combinator; where megaparsec
//! would fail with an "unexpected ... expecting ..." message, this parser
//! fails with a short message of its own at the same place. hadolint stops
//! at the first parse error, and so does this.

use super::shell::{self, ParsedShell};

pub type Pairs = Vec<(String, String)>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Image {
    pub registry: Option<String>,
    pub name: String,
}

impl Image {
    /// language-docker's `IsString Image`: a registry when the first path
    /// segment contains a dot.
    pub fn from_text(s: &str) -> Self {
        if s.contains('/') {
            let parts: Vec<&str> = s.split('/').collect();
            if parts[0].contains('.') {
                return Self {
                    registry: Some(parts[0].to_string()),
                    name: parts[1..].join("/"),
                };
            }
        }
        Self {
            registry: None,
            name: s.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaseImage {
    pub image: Image,
    pub tag: Option<String>,
    pub digest: Option<String>,
    pub alias: Option<String>,
    pub platform: Option<String>,
}

impl BaseImage {
    pub fn scratch() -> Self {
        Self {
            image: Image {
                registry: None,
                name: "scratch".to_string(),
            },
            tag: None,
            digest: None,
            alias: None,
            platform: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Port {
    Num(i64, Protocol),
    Var(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PortSpec {
    Port(Port),
    Range(Port, Port),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgsKind {
    /// Shell form (or a heredoc).
    Text,
    /// Exec form: a JSON array, joined with spaces.
    List,
}

/// The arguments of RUN, CMD, ENTRYPOINT, SHELL and HEALTHCHECK CMD, with
/// the shell parse hadolint runs over them (`shell.original` is the text).
#[derive(Debug, Clone)]
pub struct Args {
    pub kind: ArgsKind,
    pub shell: ParsedShell,
}

impl Args {
    fn new(kind: ArgsKind, text: String) -> Self {
        let shell = shell::parse_shell(&text);
        Self { kind, shell }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Relabel {
    Shared,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheSharing {
    Shared,
    Private,
    Locked,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunMount {
    Bind {
        target: String,
        source: Option<String>,
        from: Option<String>,
        read_only: Option<bool>,
        relabel: Option<Relabel>,
    },
    Cache {
        target: String,
        sharing: Option<CacheSharing>,
        id: Option<String>,
        read_only: Option<bool>,
        from: Option<String>,
        source: Option<String>,
        mode: Option<String>,
        uid: Option<String>,
        gid: Option<String>,
    },
    Tmpfs {
        target: String,
        size: Option<String>,
    },
    Secret(SecretOpts),
    Ssh(SecretOpts),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct SecretOpts {
    pub target: Option<String>,
    pub id: Option<String>,
    pub required: Option<bool>,
    pub source: Option<String>,
    pub env: Option<String>,
    pub mode: Option<String>,
    pub uid: Option<String>,
    pub gid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Insecure,
    Sandbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    None,
    Host,
    Default,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunFlags {
    /// A set in the original: kept sorted and without duplicates.
    pub mounts: Vec<RunMount>,
    pub security: Option<Security>,
    pub network: Option<Network>,
}

impl RunFlags {
    /// `hasCacheOrTmpfsMountWith`: a cache or tmpfs mount whose target
    /// contains `frag`.
    pub fn has_cache_or_tmpfs_mount_with(&self, frag: &str) -> bool {
        self.mounts.iter().any(|m| match m {
            RunMount::Cache { target, .. } | RunMount::Tmpfs { target, .. } => {
                target.contains(frag)
            }
            _ => false,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyFlags {
    pub chown: Option<String>,
    pub chmod: Option<String>,
    pub link: Option<bool>,
    pub parents: Option<bool>,
    pub from: Option<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddFlags {
    pub checksum: Option<String>,
    pub chown: Option<String>,
    pub chmod: Option<String>,
    pub link: Option<bool>,
    pub keep_git_dir: Option<bool>,
    pub unpack: Option<bool>,
    pub exclude: Vec<String>,
}

/// The full tree is kept as language-docker defines it; the rules read
/// only some of the fields, the rest document what the parser accepts.
#[allow(
    dead_code,
    reason = "a faithful AST: fields no rule reads are still parsed"
)]
#[derive(Debug, Clone)]
pub struct CheckArgs {
    pub command: Args,
    pub interval: Option<f64>,
    pub timeout: Option<f64>,
    pub start_period: Option<f64>,
    pub start_interval: Option<f64>,
    pub retries: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pragma {
    Escape(char),
    Syntax(String),
}

#[allow(
    dead_code,
    reason = "a faithful AST: fields no rule reads are still parsed"
)]
#[derive(Debug, Clone)]
pub enum Instruction {
    From(BaseImage),
    Add {
        sources: Vec<String>,
        target: String,
        flags: AddFlags,
    },
    User(String),
    Label(Pairs),
    Stopsignal(String),
    Copy {
        sources: Vec<String>,
        target: String,
        flags: CopyFlags,
    },
    Run {
        args: Args,
        flags: RunFlags,
    },
    Cmd(Args),
    Shell(Args),
    Workdir(String),
    Expose(Vec<PortSpec>),
    Volume(String),
    Entrypoint(Args),
    Maintainer(String),
    Env(Pairs),
    Arg {
        name: String,
        default: Option<String>,
    },
    /// `None` is `HEALTHCHECK NONE`.
    Healthcheck(Option<CheckArgs>),
    Pragma(Pragma),
    Comment(String),
    OnBuild(Box<Self>),
}

#[derive(Debug, Clone)]
pub struct InstructionPos {
    pub instruction: Instruction,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

/// Parse a whole Dockerfile (already decoded; CRLF becomes LF here).
pub fn parse(source: &str) -> Result<Vec<InstructionPos>, ParseError> {
    let text = source.replace("\r\n", "\n");
    let esc = find_escape_pragma(&text);
    let mut p = Parser {
        chars: text.chars().collect(),
        pos: 0,
        esc,
    };
    p.dockerfile()
}

/// `findEscapePragma`: the escape pragma counts only in the pragma lines at
/// the very top of the file.
fn find_escape_pragma(text: &str) -> char {
    for line in text.split('\n') {
        let mut p = Parser {
            chars: line.chars().collect(),
            pos: 0,
            esc: DEFAULT_ESC,
        };
        p.only_whitespaces();
        let Ok(instr) = p.comment_instruction() else {
            return DEFAULT_ESC;
        };
        if !p.at_end() {
            return DEFAULT_ESC;
        }
        match instr {
            Instruction::Pragma(Pragma::Escape(c)) => return c,
            Instruction::Pragma(_) => {}
            _ => return DEFAULT_ESC,
        }
    }
    DEFAULT_ESC
}

pub const DEFAULT_ESC: char = '\\';

type PResult<T> = Result<T, Fail>;

/// A parse failure at a position. `custom` failures are language-docker's
/// own error kinds (duplicate flag and the like), which stop the parse
/// even where plain failures would let another alternative try.
#[derive(Debug, Clone)]
struct Fail {
    pos: usize,
    message: String,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    esc: char,
}

const fn is_space(c: char) -> bool {
    c == ' ' || c == '\t'
}

const fn is_nl(c: char) -> bool {
    c == '\n'
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    const fn at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn fail<T>(&self, message: &str) -> PResult<T> {
        Err(Fail {
            pos: self.pos,
            message: message.to_string(),
        })
    }

    fn line_col(&self, pos: usize) -> (usize, usize) {
        let before = &self.chars[..pos.min(self.chars.len())];
        let line = before.iter().filter(|&&c| c == '\n').count() + 1;
        let col = before
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(pos, |p| pos - p - 1)
            + 1;
        (line, col)
    }

    /// Literal text, case-sensitively.
    fn string(&mut self, s: &str) -> PResult<()> {
        let n = s.chars().count();
        let matches = s
            .chars()
            .enumerate()
            .all(|(i, c)| self.peek_at(i) == Some(c));
        if matches {
            self.pos += n;
            Ok(())
        } else {
            self.fail(&format!("expecting {s:?}"))
        }
    }

    /// Literal text, case-insensitively (`string'`).
    fn string_ci(&mut self, s: &str) -> PResult<()> {
        let n = s.chars().count();
        let matches = s.chars().enumerate().all(|(i, c)| {
            self.peek_at(i)
                .is_some_and(|d| d.to_lowercase().eq(c.to_lowercase()))
        });
        if matches {
            self.pos += n;
            Ok(())
        } else {
            self.fail(&format!("expecting {s:?}"))
        }
    }

    fn char(&mut self, c: char) -> PResult<()> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            self.fail(&format!("expecting {c:?}"))
        }
    }

    fn take_while(&mut self, pred: impl Fn(char) -> bool) -> String {
        let start = self.pos;
        while self.peek().is_some_and(&pred) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn take_while1(&mut self, name: &str, pred: impl Fn(char) -> bool) -> PResult<String> {
        let s = self.take_while(pred);
        if s.is_empty() {
            return self.fail(&format!("expecting {name}"));
        }
        Ok(s)
    }

    /// Run `f`; on failure restore the position (megaparsec's `try`).
    fn attempt<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        let start = self.pos;
        let r = f(self);
        if r.is_err() {
            self.pos = start;
        }
        r
    }

    fn optional<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> Option<T> {
        self.attempt(f).ok()
    }

    fn not_followed_by(
        &mut self,
        f: impl FnOnce(&mut Self) -> PResult<()>,
        what: &str,
    ) -> PResult<()> {
        let start = self.pos;
        let r = f(self);
        self.pos = start;
        match r {
            Ok(()) => self.fail(what),
            Err(_) => Ok(()),
        }
    }

    // ---- whitespace -----------------------------------------------------------

    fn only_spaces(&mut self) -> String {
        self.take_while(is_space)
    }

    fn only_whitespaces(&mut self) {
        self.take_while(|c| c == ' ' || c == '\t' || c == '\n' || c == '\r');
    }

    fn comment_text(&mut self) -> PResult<String> {
        self.char('#')?;
        Ok(self.take_while(|c| !is_nl(c)))
    }

    /// `escapedLineBreaks`: one or more escaped line breaks (comment lines
    /// between them skipped). `Some(found)` tells whether whitespace
    /// followed the last one; `None` when there was no escaped break.
    fn escaped_line_breaks(&mut self) -> Option<bool> {
        let mut any = false;
        let mut found = false;
        loop {
            let esc = self.esc;
            let ok = self
                .attempt(|p| {
                    p.char(esc)?;
                    p.only_spaces();
                    p.take_while1("newline", is_nl)?;
                    Ok(())
                })
                .is_ok();
            if !ok {
                break;
            }
            any = true;
            // skipMany (onlySpaces *> comment *> newlines)
            while self
                .attempt(|p| {
                    p.only_spaces();
                    p.comment_text()?;
                    p.take_while1("newline", is_nl)?;
                    Ok(())
                })
                .is_ok()
            {}
            found = !self.only_spaces().is_empty();
        }
        if any { Some(found) } else { None }
    }

    /// `foundWhitespace`/`whitespace`: spaces and escaped line breaks;
    /// returns whether any significant whitespace was found.
    fn whitespace(&mut self) -> bool {
        let mut found = false;
        loop {
            if !self.only_spaces().is_empty() {
                found = true;
                continue;
            }
            match self.escaped_line_breaks() {
                Some(f) => found |= f,
                None => break,
            }
        }
        found
    }

    fn required_whitespace(&mut self) -> PResult<()> {
        if self.whitespace() {
            Ok(())
        } else {
            self.fail("missing whitespace")
        }
    }

    /// `eol`: at least one run of spaces, newlines or escaped line breaks.
    fn eol(&mut self) -> PResult<()> {
        let mut any = false;
        loop {
            if !self.only_spaces().is_empty() {
                any = true;
                continue;
            }
            if !self.take_while(is_nl).is_empty() {
                any = true;
                continue;
            }
            if self.escaped_line_breaks().is_some() {
                any = true;
                continue;
            }
            break;
        }
        if any {
            Ok(())
        } else {
            self.fail("end of line")
        }
    }

    fn reserved(&mut self, name: &str) -> PResult<()> {
        self.string_ci(name)?;
        self.required_whitespace()
    }

    /// `symbol`: the text then optional whitespace.
    fn symbol(&mut self, s: &str) -> PResult<()> {
        self.string(s)?;
        self.whitespace();
        Ok(())
    }

    /// `lexeme'`: the parser then plain spaces.
    fn lexeme_spaces(&mut self, f: impl FnOnce(&mut Self) -> PResult<()>) -> PResult<()> {
        f(self)?;
        self.only_spaces();
        Ok(())
    }

    // ---- text runs --------------------------------------------------------------

    /// `untilEol`: the rest of the line, escaped line breaks collapsed.
    fn until_eol(&mut self, name: &str) -> PResult<String> {
        let mut out = String::new();
        let esc = self.esc;
        loop {
            if let Some(found) = self.escaped_line_breaks() {
                if found {
                    out.push(' ');
                }
                continue;
            }
            let run = self.take_while(|c| c != '\n' && c != esc);
            if !run.is_empty() {
                out.push_str(&run);
                continue;
            }
            // A run of escape characters not followed by a newline stays.
            let ok = self.attempt(|p| {
                let r = p.take_while1("escape", |c| c == esc)?;
                p.not_followed_by(|p| p.char('\n'), "newline")?;
                Ok(r)
            });
            match ok {
                Ok(r) => out.push_str(&r),
                Err(_) => break,
            }
        }
        if out.is_empty() {
            return self.fail(&format!("expecting {name}"));
        }
        Ok(out)
    }

    /// `someUnless`: text up to whitespace, the escape character or a
    /// character `pred` accepts; escaped line breaks inside it collapse to
    /// a space unless a space would end the token.
    fn some_unless(&mut self, name: &str, pred: &dyn Fn(char) -> bool) -> PResult<String> {
        let mut out: Vec<String> = Vec::new();
        let esc = self.esc;
        loop {
            // try insignificantLineBreak
            let brk = self.attempt(|p| match p.escaped_line_breaks() {
                None => p.fail("no line break"),
                Some(found) => {
                    if found && pred(' ') {
                        p.fail("escaped line break separating two tokens")
                    } else {
                        Ok(found)
                    }
                }
            });
            if let Ok(found) = brk {
                out.push(if found {
                    " ".to_string()
                } else {
                    String::new()
                });
                continue;
            }
            let run = self.take_while(|c| !(self_is_space_nl(c, esc) || pred(c)));
            if !run.is_empty() {
                out.push(run);
                continue;
            }
            let escs = self.attempt(|p| {
                let r = p.take_while1("escape", |c| c == esc && !pred(c))?;
                p.not_followed_by(|p| p.char('\n'), "newline")?;
                Ok(r)
            });
            match escs {
                Ok(r) => out.push(r),
                Err(_) => break,
            }
        }
        if out.is_empty() {
            return self.fail(&format!("expecting {name}"));
        }
        Ok(out.concat())
    }

    fn any_unless(&mut self, pred: &dyn Fn(char) -> bool) -> String {
        self.some_unless("", pred).unwrap_or_default()
    }

    /// `stringWithEscaped`: text with the quote characters excluded unless
    /// escaped, escaped line breaks collapsed.
    fn string_with_escaped(
        &mut self,
        quotes: &[char],
        accept: Option<&dyn Fn(char) -> bool>,
    ) -> String {
        let mut out = String::new();
        let esc = self.esc;
        loop {
            // inner: some (escapedLineBreaks | takeWhile1 ...)
            let mut inner = String::new();
            let mut any_inner = false;
            loop {
                if let Some(found) = self.escaped_line_breaks() {
                    any_inner = true;
                    if found {
                        inner.push(' ');
                    }
                    continue;
                }
                let run = self.take_while(|c| {
                    c != esc && c != '\n' && !quotes.contains(&c) && accept.is_none_or(|a| a(c))
                });
                if !run.is_empty() {
                    any_inner = true;
                    inner.push_str(&run);
                    continue;
                }
                break;
            }
            if any_inner {
                out.push_str(&inner);
                continue;
            }
            let escs = self.attempt(|p| {
                let r = p.take_while1("escape", |c| c == esc)?;
                p.not_followed_by(
                    |p| {
                        if p.peek().is_some_and(|c| quotes.contains(&c)) {
                            p.pos += 1;
                            Ok(())
                        } else {
                            p.fail("quote")
                        }
                    },
                    "quote",
                )?;
                Ok(r)
            });
            if let Ok(r) = escs {
                out.push_str(&r);
                continue;
            }
            let escaped_quote = self.attempt(|p| {
                p.char(esc)?;
                match p.peek() {
                    Some(c) if quotes.contains(&c) => {
                        p.pos += 1;
                        Ok(c)
                    }
                    _ => p.fail("quote"),
                }
            });
            match escaped_quote {
                Ok(c) => out.push(c),
                Err(_) => break,
            }
        }
        out
    }

    /// `quotedString`: a quoted string with Haskell character-literal
    /// escapes (`\"`, `\\`, `\n`, `\t`, ...).
    fn quoted_string(&mut self, q: char) -> PResult<String> {
        self.char(q)?;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return self.fail("closing quote"),
                Some(c) if c == q => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some('\\') => {
                    self.pos += 1;
                    let Some(e) = self.peek() else {
                        return self.fail("literal character");
                    };
                    self.pos += 1;
                    out.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        'a' => '\x07',
                        'b' => '\x08',
                        'f' => '\x0c',
                        'v' => '\x0b',
                        '\\' | '"' | '\'' => e,
                        _ => return self.fail("literal character"),
                    });
                }
                Some(c) => {
                    self.pos += 1;
                    out.push(c);
                }
            }
        }
    }

    fn quoted_string_escaped(&mut self, q: char) -> PResult<String> {
        self.char(q)?;
        let s = self.string_with_escaped(&[q], None);
        self.char(q)?;
        Ok(s)
    }

    // ---- heredocs -----------------------------------------------------------------

    fn heredoc_marker(&mut self) -> PResult<String> {
        self.string("<<")?;
        self.take_while(|c| c == '-');
        let m = match self.attempt(|p| p.quoted_string('"')) {
            Ok(m) => m,
            Err(_) => {
                if let Ok(m) = self.attempt(|p| p.quoted_string('\'')) {
                    m
                } else {
                    // untilWS: anything up to and including a whitespace char
                    let mut m = String::new();
                    loop {
                        match self.peek() {
                            None => return self.fail("whitespace after the heredoc marker"),
                            Some(c) if c.is_whitespace() => {
                                self.pos += 1;
                                break;
                            }
                            Some(c) => {
                                self.pos += 1;
                                m.push(c);
                            }
                        }
                    }
                    m
                }
            }
        };
        // optional heredocRedirect
        self.optional(|p| {
            if p.string("|").is_err() && p.string(">").is_err() {
                return p.fail("redirect");
            }
            p.only_spaces();
            p.until_eol("heredoc path")?;
            Ok(())
        });
        Ok(m)
    }

    fn heredoc_content(&mut self, marker: &str) -> PResult<String> {
        // delimiter: the marker right here, followed by a newline or the end
        let empty = self.attempt(|p| {
            p.string(marker)?;
            if p.peek() == Some('\n') || p.at_end() {
                Ok(())
            } else {
                p.fail("delimiter")
            }
        });
        if empty.is_ok() {
            return Ok(String::new());
        }
        let start = self.pos;
        let term = format!("\n{marker}");
        loop {
            if self.at_end() {
                return self.fail("heredoc terminator");
            }
            let hit = self.attempt(|p| {
                p.string(&term)?;
                if p.peek() == Some('\n') || p.at_end() {
                    Ok(())
                } else {
                    p.fail("terminator")
                }
            });
            if hit.is_ok() {
                let end = self.pos - term.chars().count();
                let doc: String = self.chars[start..end].iter().collect();
                return Ok(doc.trim().to_string());
            }
            self.pos += 1;
        }
    }

    fn heredoc(&mut self) -> PResult<String> {
        let m = self.heredoc_marker()?;
        self.heredoc_content(&m)
    }

    /// `untilHeredoc`: text before a heredoc on the same (escaped) line,
    /// consuming the heredoc.
    fn until_heredoc(&mut self) -> PResult<String> {
        let mut out = String::new();
        loop {
            if self.attempt(Self::heredoc).is_ok() {
                return Ok(out.trim().to_string());
            }
            if let Some(found) = self.escaped_line_breaks() {
                if found {
                    out.push(' ');
                }
                continue;
            }
            match self.peek() {
                Some('\n') | None => return self.fail("heredoc"),
                Some(c) => {
                    self.pos += 1;
                    out.push(c);
                }
            }
        }
    }

    // ---- arguments --------------------------------------------------------------

    fn brackets<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        self.symbol("[")?;
        self.whitespace();
        let r = f(self)?;
        self.whitespace();
        self.symbol("]")?;
        Ok(r)
    }

    fn comma_sep(&mut self, f: impl Fn(&mut Self) -> PResult<String>) -> Vec<String> {
        let mut out = Vec::new();
        let Ok(first) = self.attempt(|p| {
            let s = f(p)?;
            p.whitespace();
            Ok(s)
        }) else {
            return out;
        };
        out.push(first);
        loop {
            let next = self.attempt(|p| {
                p.symbol(",")?;
                let s = f(p)?;
                p.whitespace();
                Ok(s)
            });
            match next {
                Ok(s) => out.push(s),
                Err(_) => break,
            }
        }
        out
    }

    fn arguments(&mut self) -> PResult<Args> {
        if let Ok(h) = self.attempt(Self::heredoc) {
            return Ok(Args::new(ArgsKind::Text, h));
        }
        if let Ok(list) =
            self.attempt(|p| p.brackets(|p| Ok(p.comma_sep(|p| p.quoted_string_escaped('"')))))
        {
            return Ok(Args::new(ArgsKind::List, list.join(" ")));
        }
        if let Ok(t) = self.attempt(Self::until_heredoc) {
            return Ok(Args::new(ArgsKind::Text, t));
        }
        let t = self.until_eol("the shell arguments")?;
        Ok(Args::new(ArgsKind::Text, t))
    }

    // ---- RUN -------------------------------------------------------------------------

    fn run(&mut self) -> PResult<Instruction> {
        self.reserved("RUN")?;
        let start = self.pos;
        let flags = match self.run_flags() {
            Ok(f) => {
                if self.required_whitespace().is_ok() {
                    f
                } else if self.pos == start {
                    RunFlags::default()
                } else {
                    return self.fail("whitespace after the RUN flags");
                }
            }
            Err(e) => {
                if self.pos == start {
                    RunFlags::default()
                } else {
                    return Err(e);
                }
            }
        };
        let args = self.arguments()?;
        Ok(Instruction::Run { args, flags })
    }

    fn run_flags(&mut self) -> PResult<RunFlags> {
        let mut flags = RunFlags::default();
        let mut mounts = Vec::new();
        let mut first = true;
        loop {
            if !first {
                let sep = self.attempt(|p| {
                    p.required_whitespace()?;
                    if p.peek() == Some('-') && p.peek_at(1) == Some('-') {
                        Ok(())
                    } else {
                        p.fail("flag")
                    }
                });
                if sep.is_err() {
                    break;
                }
            }
            let before = self.pos;
            match self.run_flag() {
                Ok(RunFlag::Mount(m)) => mounts.push(m),
                Ok(RunFlag::Security(s)) => flags.security = Some(s),
                Ok(RunFlag::Network(n)) => flags.network = Some(n),
                Err(e) => {
                    if self.pos == before {
                        if first {
                            // sepBy with no items
                            self.pos = before;
                            break;
                        }
                        return Err(e);
                    }
                    return Err(e);
                }
            }
            first = false;
        }
        mounts.sort();
        mounts.dedup();
        flags.mounts = mounts;
        Ok(flags)
    }

    fn run_flag(&mut self) -> PResult<RunFlag> {
        if self.attempt(|p| p.string("--mount=")).is_ok() {
            return self.run_flag_mount().map(RunFlag::Mount);
        }
        if self.attempt(|p| p.string("--security=")).is_ok() {
            if self.attempt(|p| p.string("insecure")).is_ok() {
                return Ok(RunFlag::Security(Security::Insecure));
            }
            if self.attempt(|p| p.string("sandbox")).is_ok() {
                return Ok(RunFlag::Security(Security::Sandbox));
            }
            return self.fail("insecure or sandbox");
        }
        if self.attempt(|p| p.string("--network=")).is_ok() {
            if self.attempt(|p| p.string("none")).is_ok() {
                return Ok(RunFlag::Network(Network::None));
            }
            if self.attempt(|p| p.string("host")).is_ok() {
                return Ok(RunFlag::Network(Network::Host));
            }
            if self.attempt(|p| p.string("default")).is_ok() {
                return Ok(RunFlag::Network(Network::Default));
            }
            return self.fail("none, host or default");
        }
        self.fail("a RUN flag")
    }

    fn mount_string_arg(&mut self) -> PResult<String> {
        if let Ok(s) = self.attempt(|p| p.quoted_string('"')) {
            return Ok(s);
        }
        self.some_unless("a string", &|c| c == ',')
    }

    fn run_flag_mount(&mut self) -> PResult<RunMount> {
        let mut args = vec![self.mount_arg()?];
        while self.attempt(|p| p.string(",")).is_ok() {
            args.push(self.mount_arg()?);
        }
        let types: Vec<&MountArg> = args
            .iter()
            .filter(|a| matches!(a, MountArg::Type(_)))
            .collect();
        let mount_type = match types.as_slice() {
            [] => MountType::Bind,
            [MountArg::Type(t)] => *t,
            _ => return self.fail("--mount with multiple `type` arguments"),
        };
        let rest: Vec<MountArg> = args
            .into_iter()
            .filter(|a| !matches!(a, MountArg::Type(_)))
            .collect();
        let (type_name, allowed, required): (&str, &[&str], &[&str]) = match mount_type {
            MountType::Bind => (
                "bind",
                &["target", "source", "from", "ro", "relabel"],
                &["target"],
            ),
            MountType::Cache => (
                "cache",
                &[
                    "target", "sharing", "id", "ro", "from", "source", "mode", "uid", "gid",
                ],
                &["target"],
            ),
            MountType::Tmpfs => ("tmpfs", &["target", "size"], &["target"]),
            MountType::Secret | MountType::Ssh => (
                "secret",
                &[
                    "target", "id", "required", "source", "mode", "uid", "gid", "env",
                ],
                &[],
            ),
        };
        // validArgs: foldr, so the last argument is checked first.
        let mut seen: Vec<&str> = Vec::new();
        for a in rest.iter().rev() {
            let name = a.name();
            if !allowed.contains(&name) {
                return self.fail(&format!(
                    "unexpected argument '{name}' for mount of type '{type_name}'"
                ));
            }
            if seen.contains(&name) {
                return self.fail(&format!("duplicate argument for mount flag: {name}"));
            }
            seen.push(name);
        }
        let missing: Vec<&str> = required
            .iter()
            .copied()
            .filter(|r| !seen.contains(r))
            .collect();
        if !missing.is_empty() {
            return self.fail(&format!(
                "missing required argument(s) for mount flag: {missing:?}"
            ));
        }
        // foldr over the args: the first occurrence wins (there are no duplicates anyway)
        Ok(match mount_type {
            MountType::Bind => {
                let mut target = String::new();
                let (mut source, mut from, mut read_only, mut relabel) = (None, None, None, None);
                for a in rest {
                    match a {
                        MountArg::Target(t) => target = t,
                        MountArg::Source(s) => source = Some(s),
                        MountArg::From(f) => from = Some(f),
                        MountArg::ReadOnly(r) => read_only = Some(r),
                        MountArg::Relabel(r) => relabel = Some(r),
                        _ => {}
                    }
                }
                RunMount::Bind {
                    target,
                    source,
                    from,
                    read_only,
                    relabel,
                }
            }
            MountType::Cache => {
                let mut target = String::new();
                let (
                    mut sharing,
                    mut id,
                    mut read_only,
                    mut from,
                    mut source,
                    mut mode,
                    mut uid,
                    mut gid,
                ) = (None, None, None, None, None, None, None, None);
                for a in rest {
                    match a {
                        MountArg::Target(t) => target = t,
                        MountArg::Sharing(s) => sharing = Some(s),
                        MountArg::Id(i) => id = Some(i),
                        MountArg::ReadOnly(r) => read_only = Some(r),
                        MountArg::From(f) => from = Some(f),
                        MountArg::Source(s) => source = Some(s),
                        MountArg::Mode(m) => mode = Some(m),
                        MountArg::Uid(u) => uid = Some(u),
                        MountArg::Gid(g) => gid = Some(g),
                        _ => {}
                    }
                }
                RunMount::Cache {
                    target,
                    sharing,
                    id,
                    read_only,
                    from,
                    source,
                    mode,
                    uid,
                    gid,
                }
            }
            MountType::Tmpfs => {
                let mut target = String::new();
                let mut size = None;
                for a in rest {
                    match a {
                        MountArg::Target(t) => target = t,
                        MountArg::Size(s) => size = Some(s),
                        _ => {}
                    }
                }
                RunMount::Tmpfs { target, size }
            }
            MountType::Secret | MountType::Ssh => {
                let mut o = SecretOpts::default();
                for a in rest {
                    match a {
                        MountArg::Target(t) => o.target = Some(t),
                        MountArg::Id(i) => o.id = Some(i),
                        MountArg::Required(r) => o.required = Some(r),
                        MountArg::Source(s) => o.source = Some(s),
                        MountArg::Env(e) => o.env = Some(e),
                        MountArg::Mode(m) => o.mode = Some(m),
                        MountArg::Uid(u) => o.uid = Some(u),
                        MountArg::Gid(g) => o.gid = Some(g),
                        _ => {}
                    }
                }
                if mount_type == MountType::Secret {
                    RunMount::Secret(o)
                } else {
                    RunMount::Ssh(o)
                }
            }
        })
    }

    fn key_value(&mut self, key: &str) -> PResult<String> {
        self.string(&format!("{key}="))?;
        self.mount_string_arg()
    }

    fn mount_arg(&mut self) -> PResult<MountArg> {
        if let Ok(v) = self.attempt(|p| p.key_value("env")) {
            return Ok(MountArg::Env(v));
        }
        if let Ok(v) = self.attempt(|p| p.key_value("from")) {
            return Ok(MountArg::From(v));
        }
        if let Ok(v) = self.attempt(|p| p.key_value("gid")) {
            return Ok(MountArg::Gid(v));
        }
        if let Ok(v) = self.attempt(|p| p.key_value("id")) {
            return Ok(MountArg::Id(v));
        }
        if let Ok(v) = self.attempt(|p| p.key_value("mode")) {
            return Ok(MountArg::Mode(v));
        }
        // read-only variants
        for (k, v, val) in [
            ("ro", "true", true),
            ("rw", "false", true),
            ("readonly", "true", true),
            ("readwrite", "false", true),
            ("rw", "true", false),
            ("ro", "false", false),
            ("readwrite", "true", false),
            ("readonly", "false", false),
        ] {
            if self
                .attempt(|p| {
                    p.string_ci(&format!("{k}="))?;
                    p.string_ci(v)
                })
                .is_ok()
            {
                return Ok(MountArg::ReadOnly(val));
            }
        }
        if self.attempt(|p| p.string_ci("ro")).is_ok()
            || self.attempt(|p| p.string_ci("readonly")).is_ok()
        {
            return Ok(MountArg::ReadOnly(true));
        }
        if self.attempt(|p| p.string_ci("rw")).is_ok()
            || self.attempt(|p| p.string_ci("readwrite")).is_ok()
        {
            return Ok(MountArg::ReadOnly(false));
        }
        if self.attempt(|p| p.string("relabel=")).is_ok() {
            if self.attempt(|p| p.string("shared")).is_ok() {
                return Ok(MountArg::Relabel(Relabel::Shared));
            }
            if self.attempt(|p| p.string("private")).is_ok() {
                return Ok(MountArg::Relabel(Relabel::Private));
            }
            return self.fail("shared or private");
        }
        if self.attempt(|p| p.string("z")).is_ok() {
            return Ok(MountArg::Relabel(Relabel::Shared));
        }
        if self.attempt(|p| p.string("Z")).is_ok() {
            return Ok(MountArg::Relabel(Relabel::Private));
        }
        if self.attempt(|p| p.string("required=true")).is_ok()
            || self.attempt(|p| p.string("required=True")).is_ok()
        {
            return Ok(MountArg::Required(true));
        }
        if self.attempt(|p| p.string("required=false")).is_ok()
            || self.attempt(|p| p.string("required=False")).is_ok()
        {
            return Ok(MountArg::Required(false));
        }
        if self.attempt(|p| p.string("required")).is_ok() {
            return Ok(MountArg::Required(true));
        }
        if self.attempt(|p| p.string("sharing=")).is_ok() {
            if self.attempt(|p| p.string("private")).is_ok() {
                return Ok(MountArg::Sharing(CacheSharing::Private));
            }
            if self.attempt(|p| p.string("shared")).is_ok() {
                return Ok(MountArg::Sharing(CacheSharing::Shared));
            }
            if self.attempt(|p| p.string("locked")).is_ok() {
                return Ok(MountArg::Sharing(CacheSharing::Locked));
            }
            return self.fail("private, shared or locked");
        }
        if let Ok(v) = self.attempt(|p| p.key_value("size")) {
            return Ok(MountArg::Size(v));
        }
        if self.attempt(|p| p.string("source=")).is_ok()
            || self.attempt(|p| p.string("src=")).is_ok()
        {
            return Ok(MountArg::Source(self.mount_string_arg()?));
        }
        if self.attempt(|p| p.string("target=")).is_ok()
            || self.attempt(|p| p.string("dst=")).is_ok()
            || self.attempt(|p| p.string("destination=")).is_ok()
        {
            return Ok(MountArg::Target(self.mount_string_arg()?));
        }
        if self.attempt(|p| p.string("type=")).is_ok() {
            for (name, t) in [
                ("bind", MountType::Bind),
                ("cache", MountType::Cache),
                ("tmpfs", MountType::Tmpfs),
                ("secret", MountType::Secret),
                ("ssh", MountType::Ssh),
            ] {
                if self.attempt(|p| p.string(name)).is_ok() {
                    return Ok(MountArg::Type(t));
                }
            }
            return self.fail("a mount type");
        }
        if let Ok(v) = self.attempt(|p| p.key_value("uid")) {
            return Ok(MountArg::Uid(v));
        }
        self.fail("a mount argument")
    }

    // ---- COPY / ADD ----------------------------------------------------------------

    fn copy(&mut self) -> PResult<Instruction> {
        self.reserved("COPY")?;
        let flags = self.flags_sep_end_by(Self::copy_flag);
        let mut cf = CopyFlags::default();
        let (mut chown, mut chmod, mut link, mut parents, mut from) = (0, 0, 0, 0, 0);
        for f in &flags {
            match f {
                Flag::Invalid(k, v) => return self.unexpected_flag(k, v),
                Flag::Chown(c) => {
                    chown += 1;
                    cf.chown.get_or_insert_with(|| c.clone());
                }
                Flag::Chmod(c) => {
                    chmod += 1;
                    cf.chmod.get_or_insert_with(|| c.clone());
                }
                Flag::Link(l) => {
                    link += 1;
                    cf.link.get_or_insert(*l);
                }
                Flag::Parents(p) => {
                    parents += 1;
                    cf.parents.get_or_insert(*p);
                }
                Flag::Source(s) => {
                    from += 1;
                    cf.from.get_or_insert_with(|| s.clone());
                }
                Flag::Exclude(e) => cf.exclude.push(e.clone()),
                _ => {}
            }
        }
        for (count, name) in [
            (chown, "--chown"),
            (chmod, "--chmod"),
            (link, "--link"),
            (parents, "--parents"),
            (from, "--from"),
        ] {
            if count > 1 {
                return self.fail(&format!("duplicate flag: {name}"));
            }
        }
        if let Ok((sources, target)) = self.attempt(Self::heredoc_list) {
            return Ok(Instruction::Copy {
                sources,
                target,
                flags: cf,
            });
        }
        let (sources, target) = self.file_list("COPY")?;
        Ok(Instruction::Copy {
            sources,
            target,
            flags: cf,
        })
    }

    fn add(&mut self) -> PResult<Instruction> {
        self.reserved("ADD")?;
        let flags = self.flags_sep_end_by(Self::add_flag);
        self.not_followed_by(
            |p| p.string("--"),
            "only the --checksum, --chown, --chmod, --link, --exclude, --keep-git-dir, --unpack flags or the src and dest paths",
        )?;
        let mut af = AddFlags::default();
        let (mut checksum, mut chown, mut chmod, mut link, mut kgd, mut unpack) =
            (0, 0, 0, 0, 0, 0);
        for f in &flags {
            match f {
                Flag::Invalid(k, v) => return self.unexpected_flag(k, v),
                Flag::Checksum(c) => {
                    checksum += 1;
                    af.checksum.get_or_insert_with(|| c.clone());
                }
                Flag::Chown(c) => {
                    chown += 1;
                    af.chown.get_or_insert_with(|| c.clone());
                }
                Flag::Chmod(c) => {
                    chmod += 1;
                    af.chmod.get_or_insert_with(|| c.clone());
                }
                Flag::Link(l) => {
                    link += 1;
                    af.link.get_or_insert(*l);
                }
                Flag::KeepGitDir(k) => {
                    kgd += 1;
                    af.keep_git_dir.get_or_insert(*k);
                }
                Flag::Unpack(u) => {
                    unpack += 1;
                    af.unpack.get_or_insert(*u);
                }
                Flag::Exclude(e) => af.exclude.push(e.clone()),
                _ => {}
            }
        }
        for (count, name) in [
            (checksum, "--checksum"),
            (chown, "--chown"),
            (chmod, "--chmod"),
            (link, "--link"),
            (kgd, "--keep-git-dir"),
            (unpack, "--unpack"),
        ] {
            if count > 1 {
                return self.fail(&format!("duplicate flag: {name}"));
            }
        }
        let (sources, target) = self.file_list("ADD")?;
        Ok(Instruction::Add {
            sources,
            target,
            flags: af,
        })
    }

    fn unexpected_flag<T>(&self, name: &str, value: &str) -> PResult<T> {
        if value.is_empty() {
            self.fail(&format!("unexpected flag {name} with no value"))
        } else {
            self.fail(&format!("invalid flag: {name}"))
        }
    }

    /// `flag sepEndBy requiredWhitespace`.
    fn flags_sep_end_by(&mut self, f: impl Fn(&mut Self) -> PResult<Flag>) -> Vec<Flag> {
        let mut out = Vec::new();
        while let Ok(flag) = self.attempt(&f) {
            out.push(flag);
            if self.attempt(Self::required_whitespace).is_err() {
                break;
            }
        }
        out
    }

    fn copy_flag(&mut self) -> PResult<Flag> {
        if self.attempt(|p| p.string("--from=")).is_ok() {
            return Ok(Flag::Source(
                self.some_unless("the copy source path", &is_nl)?,
            ));
        }
        if let Ok(f) = self.attempt(Self::common_flag) {
            return Ok(f);
        }
        if self.attempt(|p| p.string("--parents=")).is_ok() {
            return Ok(Flag::Parents(self.true_false()?));
        }
        if self.attempt(|p| p.string("--parents")).is_ok() {
            return Ok(Flag::Parents(true));
        }
        if self.attempt(|p| p.string("--exclude=")).is_ok() {
            return Ok(Flag::Exclude(
                self.some_unless("the exclude pattern", &|c| c == ' ')?,
            ));
        }
        self.any_flag()
    }

    fn add_flag(&mut self) -> PResult<Flag> {
        if self.attempt(|p| p.string("--checksum=")).is_ok() {
            return Ok(Flag::Checksum(
                self.some_unless("the remote file checksum", &|c| c == ' ')?,
            ));
        }
        if let Ok(f) = self.attempt(Self::common_flag) {
            return Ok(f);
        }
        if self.attempt(|p| p.string("--keep-git-dir=")).is_ok() {
            return Ok(Flag::KeepGitDir(self.true_false()?));
        }
        if self.attempt(|p| p.string("--keep-git-dir")).is_ok() {
            return Ok(Flag::KeepGitDir(true));
        }
        if self.attempt(|p| p.string("--unpack=")).is_ok() {
            return Ok(Flag::Unpack(self.true_false()?));
        }
        if self.attempt(|p| p.string("--unpack")).is_ok() {
            return Ok(Flag::Unpack(true));
        }
        if self.attempt(|p| p.string("--exclude=")).is_ok() {
            return Ok(Flag::Exclude(
                self.some_unless("the exclude pattern", &|c| c == ' ')?,
            ));
        }
        self.any_flag()
    }

    /// `--chown=`, `--chmod=`, `--link[=bool]`: shared by COPY and ADD.
    fn common_flag(&mut self) -> PResult<Flag> {
        if self.attempt(|p| p.string("--chown=")).is_ok() {
            return Ok(Flag::Chown(
                self.some_unless("the user and group for chown", &|c| c == ' ')?,
            ));
        }
        if self.attempt(|p| p.string("--chmod=")).is_ok() {
            return Ok(Flag::Chmod(
                self.some_unless("the mode for chmod", &|c| c == ' ')?,
            ));
        }
        if self.attempt(|p| p.string("--link=")).is_ok() {
            return Ok(Flag::Link(self.true_false()?));
        }
        if self.attempt(|p| p.string("--link")).is_ok() {
            return Ok(Flag::Link(true));
        }
        self.fail("a common flag")
    }

    fn true_false(&mut self) -> PResult<bool> {
        if self.attempt(|p| p.string("true")).is_ok() {
            return Ok(true);
        }
        if self.attempt(|p| p.string("false")).is_ok() {
            return Ok(false);
        }
        self.fail("true or false")
    }

    fn any_flag(&mut self) -> PResult<Flag> {
        self.string("--")?;
        let name = self.some_unless("the flag value", &|c| c == '=')?;
        self.char('=')?;
        let value = self.any_unless(&|c| c == ' ');
        Ok(Flag::Invalid(format!("--{name}"), value))
    }

    fn heredoc_list(&mut self) -> PResult<(Vec<String>, String)> {
        // spaceSep1 heredocMarker
        let mut markers = Vec::new();
        while let Ok(m) = self.attempt(Self::heredoc_marker) {
            markers.push(m);
            self.only_spaces();
        }
        if markers.is_empty() {
            return self.fail("a list of heredoc markers");
        }
        let target = self.until_eol("target path")?;
        let last = markers.last().cloned().unwrap_or_default();
        self.heredoc_content(&last)?;
        Ok((markers, target))
    }

    fn file_list(&mut self, name: &str) -> PResult<(Vec<String>, String)> {
        let paths = if let Ok(list) =
            self.attempt(|p| p.brackets(|p| Ok(p.comma_sep(|p| p.quoted_string('"')))))
        {
            list
        } else {
            let mut paths = Vec::new();
            while let Ok(path) = self.attempt(|p| p.some_unless("a file", &|c| c == ' ')) {
                paths.push(path);
                if self.attempt(Self::required_whitespace).is_err() {
                    break;
                }
            }
            if paths.is_empty() {
                return self.fail("a space separated list of file paths");
            }
            paths
        };
        if paths.len() < 2 {
            return self.fail(&format!(
                "unexpected end of line. At least two arguments are required for {name}"
            ));
        }
        let target = paths[paths.len() - 1].clone();
        let sources = paths[..paths.len() - 1].to_vec();
        Ok((sources, target))
    }

    // ---- FROM ------------------------------------------------------------------------

    fn from(&mut self) -> PResult<Instruction> {
        self.reserved("FROM")?;
        if let Ok(b) = self.attempt(|p| p.base_image(true)) {
            return Ok(Instruction::From(b));
        }
        Ok(Instruction::From(self.base_image(false)?))
    }

    fn base_image(&mut self, tagged: bool) -> PResult<BaseImage> {
        let platform = self.optional(|p| {
            p.string("--platform=")?;
            let s = p.some_unless("the platform for the FROM image", &|c| c == ' ')?;
            p.required_whitespace()?;
            Ok(s)
        });
        self.not_followed_by(|p| p.string("--"), "an image name, not a flag")?;
        let registry = self.optional(|p| {
            let domain = p.some_unless_expanded("a domain name", &|c| c == '.')?;
            p.char('.')?;
            let tld = p.some_unless_expanded("a TLD", &|c| c == '/')?;
            p.char('/')?;
            Ok(format!("{domain}.{tld}"))
        });
        let name =
            self.some_unless_expanded("the image name with a tag", &|c| c == '@' || c == ':')?;
        let tag = if tagged {
            self.optional(|p| {
                p.char(':')?;
                p.some_unless_expanded("the image tag", &|c| c == '@' || c == ':')
            })
        } else {
            self.not_followed_by(
                |p| p.string(":"),
                &format!("no ':' or a valid image tag string (example: {name}:valid-tag)"),
            )?;
            None
        };
        let digest = self.optional(|p| {
            p.char('@')?;
            p.some_unless("the image digest", &|c| c == '@')
        });
        let alias = self.optional(|p| {
            p.required_whitespace()?;
            p.reserved("AS")?;
            p.some_unless("the image alias", &is_nl)
        });
        Ok(BaseImage {
            image: Image { registry, name },
            tag,
            digest,
            alias,
            platform,
        })
    }

    /// `someUnlessExpanded`: like `some_unless`, with `${...}` opaque.
    fn some_unless_expanded(&mut self, name: &str, pred: &dyn Fn(char) -> bool) -> PResult<String> {
        let mut out = String::new();
        let mut any = false;
        loop {
            if self.peek() == Some('$') {
                self.pos += 1;
                any = true;
                out.push('$');
                if self.peek() == Some('{') {
                    self.pos += 1;
                    out.push('{');
                    let mut depth = 1;
                    loop {
                        let piece =
                            self.take_while(|c| !matches!(c, '{' | '}' | ' ' | '\t' | '\n'));
                        out.push_str(&piece);
                        match self.peek() {
                            Some('{') => {
                                self.pos += 1;
                                out.push('{');
                                depth += 1;
                            }
                            Some('}') => {
                                self.pos += 1;
                                out.push('}');
                                if depth <= 1 {
                                    break;
                                }
                                depth -= 1;
                            }
                            _ => break,
                        }
                    }
                }
                continue;
            }
            match self.attempt(|p| p.some_unless(name, &|c| pred(c) || c == '$')) {
                Ok(s) => {
                    any = true;
                    out.push_str(&s);
                }
                Err(_) => break,
            }
        }
        if !any {
            return self.fail(&format!("expecting {name}"));
        }
        Ok(out)
    }

    // ---- EXPOSE ------------------------------------------------------------------------

    fn natural(&mut self) -> PResult<i64> {
        let digits = self.take_while1("positive number", |c| c.is_ascii_digit())?;
        // Integer in the original, then fromIntegral to Int: wrap on overflow.
        let mut n: i64 = 0;
        for d in digits.chars() {
            n = n
                .wrapping_mul(10)
                .wrapping_add(i64::from(d as u32 - '0' as u32));
        }
        Ok(n)
    }

    fn protocol(&mut self) -> PResult<Protocol> {
        self.char('/')?;
        if self.attempt(|p| p.string_ci("tcp")).is_ok() {
            return Ok(Protocol::Tcp);
        }
        if self.attempt(|p| p.string_ci("udp")).is_ok() {
            return Ok(Protocol::Udp);
        }
        self.fail("invalid protocol")
    }

    fn expose(&mut self) -> PResult<Instruction> {
        self.reserved("EXPOSE")?;
        let mut ports = Vec::new();
        while let Ok(spec) = self.attempt(Self::portspec) {
            ports.push(spec);
            if self.attempt(Self::required_whitespace).is_err() {
                break;
            }
        }
        Ok(Instruction::Expose(ports))
    }

    fn portspec(&mut self) -> PResult<PortSpec> {
        if let Ok(r) = self.attempt(Self::port_range) {
            return Ok(PortSpec::Range(r.0, r.1));
        }
        Ok(PortSpec::Port(self.port()?))
    }

    fn port(&mut self) -> PResult<Port> {
        if let Ok(v) = self.attempt(|p| {
            p.char('$')?;
            p.some_unless("the variable name", &|c| c == '$')
        }) {
            return Ok(Port::Var(format!("${v}")));
        }
        if let Ok(p) = self.attempt(|p| {
            let n = p.natural()?;
            let proto = p.protocol()?;
            Ok(Port::Num(n, proto))
        }) {
            return Ok(p);
        }
        let n = self.natural()?;
        self.not_followed_by(
            |p| {
                if p.string("/").is_ok() || p.string("-").is_ok() {
                    Ok(())
                } else {
                    p.fail("")
                }
            },
            "a valid port number",
        )?;
        Ok(Port::Num(n, Protocol::Tcp))
    }

    fn port_range_limit(&mut self) -> PResult<Port> {
        if let Ok(n) = self.attempt(Self::natural) {
            return Ok(Port::Num(n, Protocol::Tcp));
        }
        self.char('$')?;
        let v = self.some_unless("the variable name", &|c| c == '-' || c == '/')?;
        Ok(Port::Var(format!("${v}")))
    }

    fn port_range(&mut self) -> PResult<(Port, Port)> {
        let start = self.port_range_limit()?;
        self.char('-')?;
        let finish = self.attempt(Self::port_range_limit)?;
        let proto = self.attempt(Self::protocol).unwrap_or(Protocol::Tcp);
        let set = |p: Port| match p {
            Port::Num(n, _) => Port::Num(n, proto),
            v @ Port::Var(_) => v,
        };
        Ok((set(start), set(finish)))
    }

    // ---- ENV / LABEL ----------------------------------------------------------------------

    /// `unquotedString`: `Ok(None)` when there is nothing to read (the value
    /// ends), `Err` for a stray quote (a hard error in the original too).
    fn unquoted_string(&mut self, accept: &dyn Fn(char) -> bool) -> PResult<Option<String>> {
        let start = self.pos;
        let s =
            self.string_with_escaped(&[' ', '\t'], Some(&|c| accept(c) && c != '"' && c != '\''));
        if s.is_empty() {
            self.pos = start;
            return Ok(None);
        }
        if s.starts_with('\'') {
            return self.fail(&format!(
                "unexpected end of single quoted string {s} (unmatched quote)"
            ));
        }
        if s.starts_with('"') {
            return self.fail(&format!(
                "unexpected end of double quoted string {s} (unmatched quote)"
            ));
        }
        Ok(Some(s))
    }

    fn single_value(&mut self, accept: &dyn Fn(char) -> bool) -> PResult<String> {
        let mut out = String::new();
        loop {
            if let Ok(s) = self.attempt(|p| p.quoted_string_escaped('"')) {
                out.push_str(&s);
                continue;
            }
            if let Ok(s) = self.attempt(|p| p.quoted_string_escaped('\'')) {
                out.push_str(&s);
                continue;
            }
            match self.unquoted_string(accept)? {
                Some(s) => out.push_str(&s),
                None => break,
            }
        }
        Ok(out)
    }

    fn pair(&mut self) -> PResult<(String, String)> {
        let key = self.single_value(&|c| c != '=')?;
        if self.attempt(|p| p.char('=')).is_ok() {
            let value = self.single_value(&|c| c != ' ' && c != '\t')?;
            return Ok((key, value));
        }
        self.required_whitespace()?;
        let value = self.until_eol("value")?;
        Ok((key, value))
    }

    fn pairs(&mut self) -> PResult<Pairs> {
        let mut out = vec![self.pair()?];
        loop {
            let next = self.attempt(|p| {
                p.required_whitespace()?;
                p.pair()
            });
            match next {
                Ok(pair) => out.push(pair),
                Err(_) => break,
            }
        }
        // sepEndBy: a trailing separator is consumed
        self.attempt(Self::required_whitespace).ok();
        Ok(out)
    }

    // ---- the rest ---------------------------------------------------------------------------

    fn arg(&mut self) -> PResult<Instruction> {
        self.reserved("ARG")?;
        if let Ok(i) = self.attempt(|p| {
            let name = p.some_unless("the argument name", &|c| c == '=')?;
            p.char('=')?;
            let df = p.until_eol("the argument value")?;
            Ok(Instruction::Arg {
                name,
                default: Some(df),
            })
        }) {
            return Ok(i);
        }
        if let Ok(i) = self.attempt(|p| {
            let name = p.some_unless("the argument name", &|c| c == '=')?;
            p.until_eol("the rest")?;
            Ok(Instruction::Arg {
                name,
                default: None,
            })
        }) {
            return Ok(i);
        }
        let name = self.until_eol("the argument name")?;
        Ok(Instruction::Arg {
            name,
            default: None,
        })
    }

    fn healthcheck(&mut self) -> PResult<Instruction> {
        self.reserved("HEALTHCHECK")?;
        if let Ok(check) = self.attempt(|p| {
            // fullCheck
            let flags = p
                .attempt(|p| {
                    let mut fs = vec![p.check_flag()?];
                    loop {
                        let cont = p
                            .attempt(|p| {
                                p.required_whitespace()?;
                                if p.peek() == Some('-') && p.peek_at(1) == Some('-') {
                                    Ok(())
                                } else {
                                    p.fail("flag")
                                }
                            })
                            .is_ok();
                        if !cont {
                            break;
                        }
                        fs.push(p.check_flag()?);
                    }
                    p.required_whitespace()?;
                    Ok(fs)
                })
                .unwrap_or_default();
            let (mut interval, mut timeout, mut start_period, mut start_interval, mut retries) =
                (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for f in flags {
                match f {
                    CheckFlag::Invalid(k, v) => return p.unexpected_flag(&k, &v),
                    CheckFlag::Interval(d) => interval.push(d),
                    CheckFlag::Timeout(d) => timeout.push(d),
                    CheckFlag::StartPeriod(d) => start_period.push(d),
                    CheckFlag::StartInterval(d) => start_interval.push(d),
                    CheckFlag::Retries(n) => retries.push(n),
                }
            }
            for (v, name) in [
                (interval.len(), "--interval"),
                (timeout.len(), "--timeout"),
                (start_period.len(), "--start-period"),
                (start_interval.len(), "--start-interval"),
                (retries.len(), "--retries"),
            ] {
                if v > 1 {
                    return p.fail(&format!("duplicate flag: {name}"));
                }
            }
            p.reserved("CMD")?;
            let command = p.arguments()?;
            Ok(CheckArgs {
                command,
                interval: interval.first().copied(),
                timeout: timeout.first().copied(),
                start_period: start_period.first().copied(),
                start_interval: start_interval.first().copied(),
                retries: retries.first().copied(),
            })
        }) {
            return Ok(Instruction::Healthcheck(Some(check)));
        }
        self.string("NONE")?;
        Ok(Instruction::Healthcheck(None))
    }

    fn check_flag(&mut self) -> PResult<CheckFlag> {
        for (name, make) in [
            ("--interval=", CheckFlag::Interval as fn(f64) -> CheckFlag),
            ("--timeout=", CheckFlag::Timeout),
            ("--start-period=", CheckFlag::StartPeriod),
            ("--start-interval=", CheckFlag::StartInterval),
        ] {
            if self.attempt(|p| p.string(name)).is_ok() {
                return Ok(make(self.duration()?));
            }
        }
        if self.attempt(|p| p.string("--retries=")).is_ok() {
            let n = self.natural()?;
            return Ok(CheckFlag::Retries(n));
        }
        match self.any_flag()? {
            Flag::Invalid(k, v) => Ok(CheckFlag::Invalid(k, v)),
            _ => self.fail("no flags"),
        }
    }

    fn duration(&mut self) -> PResult<f64> {
        // try fractional (digits '.' digits) else natural
        let value = match self.attempt(|p| {
            let a = p.take_while1("number", |c| c.is_ascii_digit())?;
            p.char('.')?;
            let b = p.take_while1("number", |c| c.is_ascii_digit())?;
            format!("{a}.{b}").parse::<f64>().map_err(|_| Fail {
                pos: p.pos,
                message: "fractional number".to_string(),
            })
        }) {
            Ok(v) => v,
            Err(_) => self.natural()? as f64,
        };
        match self.peek() {
            Some('s') => {
                self.pos += 1;
                Ok(value)
            }
            Some('m') => {
                self.pos += 1;
                Ok(value * 60.0)
            }
            Some('h') => {
                self.pos += 1;
                Ok(value * 3600.0)
            }
            _ => self.fail("either 's', 'm' or 'h' as the unit"),
        }
    }

    fn pragma(&mut self) -> PResult<Instruction> {
        self.lexeme_spaces(|p| p.char('#'))?;
        if self
            .attempt(|p| {
                p.lexeme_spaces(|p| p.string("escape"))?;
                p.lexeme_spaces(|p| p.string("="))
            })
            .is_ok()
        {
            // charLiteral: one character, with Haskell escapes
            let c = match self.peek() {
                None => return self.fail("an escape character"),
                Some('\\') => {
                    self.pos += 1;
                    match self.peek() {
                        Some('\\') => {
                            self.pos += 1;
                            '\\'
                        }
                        Some('n') => {
                            self.pos += 1;
                            '\n'
                        }
                        Some('t') => {
                            self.pos += 1;
                            '\t'
                        }
                        Some(c @ ('"' | '\'')) => {
                            self.pos += 1;
                            c
                        }
                        _ => return self.fail("a literal character"),
                    }
                }
                Some(c) => {
                    self.pos += 1;
                    c
                }
            };
            return Ok(Instruction::Pragma(Pragma::Escape(c)));
        }
        if self
            .attempt(|p| {
                p.lexeme_spaces(|p| p.string("syntax"))?;
                p.lexeme_spaces(|p| p.string("="))
            })
            .is_ok()
        {
            let img = self.until_eol("the syntax")?;
            return Ok(Instruction::Pragma(Pragma::Syntax(img)));
        }
        self.fail("an escape or a syntax pragma")
    }

    fn comment_instruction(&mut self) -> PResult<Instruction> {
        if let Ok(p) = self.attempt(Self::pragma) {
            return Ok(p);
        }
        Ok(Instruction::Comment(self.comment_text()?))
    }

    fn instruction(&mut self) -> PResult<Instruction> {
        if self.attempt(|p| p.reserved("ONBUILD")).is_ok() {
            let inner = self.instruction()?;
            return Ok(Instruction::OnBuild(Box::new(inner)));
        }
        let keyword: String = self.chars[self.pos..]
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_uppercase();
        match keyword.as_str() {
            "FROM" => self.from(),
            "COPY" => self.copy(),
            "RUN" => self.run(),
            "WORKDIR" => {
                self.reserved("WORKDIR")?;
                Ok(Instruction::Workdir(self.until_eol("the workdir path")?))
            }
            "ENTRYPOINT" => {
                self.reserved("ENTRYPOINT")?;
                Ok(Instruction::Entrypoint(self.arguments()?))
            }
            "VOLUME" => {
                self.reserved("VOLUME")?;
                Ok(Instruction::Volume(self.until_eol("the volume path")?))
            }
            "EXPOSE" => self.expose(),
            "ENV" => {
                self.reserved("ENV")?;
                Ok(Instruction::Env(self.pairs()?))
            }
            "ARG" => self.arg(),
            "USER" => {
                self.reserved("USER")?;
                Ok(Instruction::User(self.until_eol("the user")?))
            }
            "LABEL" => {
                self.reserved("LABEL")?;
                Ok(Instruction::Label(self.pairs()?))
            }
            "STOPSIGNAL" => {
                self.reserved("STOPSIGNAL")?;
                Ok(Instruction::Stopsignal(self.until_eol("the stop signal")?))
            }
            "CMD" => {
                self.reserved("CMD")?;
                Ok(Instruction::Cmd(self.arguments()?))
            }
            "SHELL" => {
                self.reserved("SHELL")?;
                Ok(Instruction::Shell(self.arguments()?))
            }
            "MAINTAINER" => {
                self.reserved("MAINTAINER")?;
                Ok(Instruction::Maintainer(
                    self.until_eol("the maintainer name")?,
                ))
            }
            "ADD" => self.add(),
            "HEALTHCHECK" => self.healthcheck(),
            _ => {
                if self.peek() == Some('#') {
                    return self.comment_instruction();
                }
                self.fail("an instruction")
            }
        }
    }

    fn dockerfile(&mut self) -> Result<Vec<InstructionPos>, ParseError> {
        self.only_whitespaces();
        let mut out = Vec::new();
        while !self.at_end() {
            let start = self.pos;
            let (line, _) = self.line_col(start);
            let instruction = match self.instruction() {
                Ok(i) => i,
                Err(e) => return Err(self.error(e)),
            };
            if !self.at_end() && self.eol().is_err() {
                return Err(self.error(Fail {
                    pos: self.pos,
                    message: "a new line followed by the next instruction".to_string(),
                }));
            }
            out.push(InstructionPos { instruction, line });
        }
        Ok(out)
    }

    fn error(&self, f: Fail) -> ParseError {
        let (line, column) = self.line_col(f.pos);
        let found = self
            .chars
            .get(f.pos)
            .map_or_else(|| "end of input".to_string(), |c| format!("{c:?}"));
        ParseError {
            line,
            column,
            message: format!("unexpected {found}, expecting {}", f.message),
        }
    }
}

const fn self_is_space_nl(c: char, esc: char) -> bool {
    c == ' ' || c == '\t' || c == '\n' || c == esc
}

enum RunFlag {
    Mount(RunMount),
    Security(Security),
    Network(Network),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MountType {
    Bind,
    Cache,
    Tmpfs,
    Secret,
    Ssh,
}

enum MountArg {
    Env(String),
    From(String),
    Id(String),
    Mode(String),
    ReadOnly(bool),
    Required(bool),
    Sharing(CacheSharing),
    Size(String),
    Source(String),
    Target(String),
    Type(MountType),
    Uid(String),
    Gid(String),
    Relabel(Relabel),
}

impl MountArg {
    const fn name(&self) -> &'static str {
        match self {
            Self::Env(_) => "env",
            Self::From(_) => "from",
            Self::Gid(_) => "gid",
            Self::Id(_) => "id",
            Self::Mode(_) => "mode",
            Self::ReadOnly(_) => "ro",
            Self::Required(_) => "required",
            Self::Sharing(_) => "sharing",
            Self::Size(_) => "size",
            Self::Source(_) => "source",
            Self::Target(_) => "target",
            Self::Type(_) => "type",
            Self::Uid(_) => "uid",
            Self::Relabel(_) => "relabel",
        }
    }
}

enum Flag {
    Checksum(String),
    Chown(String),
    Chmod(String),
    Link(bool),
    KeepGitDir(bool),
    Parents(bool),
    Unpack(bool),
    Source(String),
    Exclude(String),
    Invalid(String, String),
}

enum CheckFlag {
    Interval(f64),
    Timeout(f64),
    StartPeriod(f64),
    StartInterval(f64),
    Retries(i64),
    Invalid(String, String),
}

//! A Parsec engine for `ShellCheck`'s grammar.
//!
//! `ShellCheck`'s parser is written against Parsec, and its behavior (which
//! alternative runs, which warnings survive backtracking, which message a
//! syntax error ends with) follows from Parsec's semantics, so this engine
//! reproduces them:
//!
//! - A parser either consumed input or not, on success and on failure.
//!   `a <|> b` runs `b` only when `a` failed without consuming; `try`
//!   turns a consuming failure into a non-consuming one. Consumption is a
//!   token counter, restored by backtracking.
//! - Every result carries a parse error. The error of a non-consuming step
//!   is merged into the next step's error when that step does not consume
//!   either (Parsec's `mergeError`: the furthest position wins, equal
//!   positions concatenate messages, empty errors yield). Here the carried
//!   error lives in `err`; a non-consuming failure returns it already merged.
//! - The user state (token ids, spans, parse notes, here documents) is
//!   rolled back on backtracking. The system state (parse problems, the
//!   context stack) is not: a warning raised in an abandoned branch stays.

use std::rc::Rc;

use super::ast::{Annotation, Dashed, Id, Quoted, SourcePos, Token};
use super::hchar::is_alpha;
pub use super::interface::Severity;
use super::interface::Shell;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    SysUnExpect,
    UnExpect,
    Expect,
    Message(String),
}

impl Msg {
    const fn kind(&self) -> u8 {
        match self {
            Self::SysUnExpect => 0,
            Self::UnExpect => 1,
            Self::Expect => 2,
            Self::Message(_) => 3,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ParseError {
    pub pos: SourcePos,
    pub msgs: Vec<Msg>,
}

impl ParseError {
    pub const fn unknown(pos: SourcePos) -> Self {
        Self {
            pos,
            msgs: Vec::new(),
        }
    }

    /// Parsec's `mergeError`.
    pub fn merge(e1: &Self, e2: &Self) -> Self {
        if e2.msgs.is_empty() && !e1.msgs.is_empty() {
            return e1.clone();
        }
        if e1.msgs.is_empty() && !e2.msgs.is_empty() {
            return e2.clone();
        }
        match e1.pos.cmp(&e2.pos) {
            std::cmp::Ordering::Equal => {
                let mut msgs = e1.msgs.clone();
                msgs.extend(e2.msgs.iter().cloned());
                Self { pos: e1.pos, msgs }
            }
            std::cmp::Ordering::Greater => e1.clone(),
            std::cmp::Ordering::Less => e2.clone(),
        }
    }

    /// Parsec's `errorMessages`: sorted by kind, stably.
    pub fn sorted_messages(&self) -> Vec<Msg> {
        let mut msgs = self.msgs.clone();
        msgs.sort_by_key(Msg::kind);
        msgs
    }
}

/// A failed parse: whether it consumed input, and the error.
#[derive(Clone, Debug)]
pub struct Fail {
    pub consumed: bool,
    pub err: ParseError,
}

pub type R<T> = Result<T, Fail>;

/// One alternative of a `choice`.
pub type Alternative<'a, T> = dyn FnMut(&mut P) -> R<T> + 'a;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseNote {
    pub start: SourcePos,
    pub end: SourcePos,
    pub severity: Severity,
    pub code: i64,
    pub msg: String,
}

#[derive(Clone, Debug)]
pub enum Context {
    Name(SourcePos, String),
    Annotation(Vec<Annotation>),
    Source(String),
}

#[derive(Clone, Debug)]
pub struct HereDocPending {
    pub id: Id,
    pub dashed: Dashed,
    pub quoted: Quoted,
    pub end_token: String,
    pub contexts: Vec<Context>,
}

/// What a check needs from the outside world.
pub trait SystemInterface {
    /// Reads a sourced file (`siReadFile`).
    fn read_file(&self, external_sources: Option<bool>, path: &str) -> Result<String, String>;
    /// Resolves a sourced file name (`siFindSource`).
    fn find_source(
        &self,
        current_script: &str,
        external_sources: Option<bool>,
        paths: &[String],
        name: &str,
    ) -> String;
    /// The `.shellcheckrc` for a file: its path and contents (`siGetConfig`).
    fn get_config(&self, filename: &str) -> Option<(String, String)>;
}

pub struct Environment {
    pub system: Rc<dyn SystemInterface>,
    pub check_sourced: bool,
    pub ignore_rc: bool,
    pub current_filename: String,
    pub shell_type_override: Option<Shell>,
}

/// Parsec's user state, kept as logs that truncate on backtracking.
#[derive(Default)]
pub struct UserState {
    /// The last id handed out; `None` before the first.
    pub last_id: Option<usize>,
    /// Spans by id (entries past `last_id` are stale).
    pub positions: Vec<(SourcePos, SourcePos)>,
    pub notes: Vec<ParseNote>,
    /// Here document bodies, appended; later entries for an id win.
    pub here_docs: Vec<(Id, Vec<Token>)>,
    pub pending_here_docs: Vec<HereDocPending>,
}

pub struct UserMark {
    last_id: Option<usize>,
    notes: usize,
    here_docs: usize,
    pending: Vec<HereDocPending>,
}

#[derive(Default, Clone)]
pub struct SystemState {
    pub contexts: Vec<Context>,
    pub problems: Vec<ParseNote>,
}

/// A backtracking point.
pub struct Mark {
    input: Rc<Vec<char>>,
    idx: usize,
    pos: SourcePos,
    pub counter: u64,
    err: ParseError,
    user: UserMark,
}

pub struct P {
    pub input: Rc<Vec<char>>,
    pub idx: usize,
    pub pos: SourcePos,
    pub counter: u64,
    /// The error carried by the last success.
    pub err: ParseError,
    pub user: UserState,
    pub sys: SystemState,
    pub env: Environment,
    /// File names by `SourcePos::file`.
    pub files: Vec<String>,
}

/// Parsec's `updatePosChar`.
pub const fn update_pos_char(mut pos: SourcePos, c: char) -> SourcePos {
    match c {
        '\n' => {
            pos.line += 1;
            pos.column = 1;
        }
        '\t' => pos.column = pos.column + 8 - ((pos.column - 1) % 8),
        _ => pos.column += 1,
    }
    pos
}

impl P {
    pub fn new(env: Environment, file: &str, contents: &str) -> Self {
        let pos = SourcePos {
            file: 0,
            line: 1,
            column: 1,
        };
        Self {
            input: Rc::new(contents.chars().collect()),
            idx: 0,
            pos,
            counter: 0,
            err: ParseError::unknown(pos),
            user: UserState::default(),
            sys: SystemState::default(),
            env,
            files: vec![file.to_string()],
        }
    }

    pub fn file_index(&mut self, name: &str) -> u32 {
        if let Some(i) = self.files.iter().position(|f| f == name) {
            return i as u32;
        }
        self.files.push(name.to_string());
        (self.files.len() - 1) as u32
    }

    pub fn mark(&self) -> Mark {
        Mark {
            input: self.input.clone(),
            idx: self.idx,
            pos: self.pos,
            counter: self.counter,
            err: self.err.clone(),
            user: UserMark {
                last_id: self.user.last_id,
                notes: self.user.notes.len(),
                here_docs: self.user.here_docs.len(),
                pending: self.user.pending_here_docs.clone(),
            },
        }
    }

    pub fn restore(&mut self, m: &Mark) {
        self.input = m.input.clone();
        self.idx = m.idx;
        self.pos = m.pos;
        self.counter = m.counter;
        self.err = m.err.clone();
        self.user.last_id = m.user.last_id;
        self.user.notes.truncate(m.user.notes);
        self.user.here_docs.truncate(m.user.here_docs);
        self.user.pending_here_docs.clone_from(&m.user.pending);
    }

    pub const fn consumed_since(&self, f: &Fail, m: &Mark) -> bool {
        f.consumed || self.counter > m.counter
    }

    // ----- failures

    /// A non-consuming failure with `err`, merged with the carried error.
    pub fn empty_fail<T>(&self, err: &ParseError) -> R<T> {
        Err(Fail {
            consumed: false,
            err: ParseError::merge(&self.err, err),
        })
    }

    /// `fail msg`.
    pub fn fail<T>(&self, msg: &str) -> R<T> {
        self.empty_fail(&ParseError {
            pos: self.pos,
            msgs: vec![Msg::Message(msg.to_string())],
        })
    }

    /// `mzero` / a failed `guard`.
    pub fn zero<T>(&self) -> R<T> {
        self.empty_fail(&ParseError::unknown(self.pos))
    }

    pub fn guard(&self, cond: bool) -> R<()> {
        if cond { self.ok(()) } else { self.zero() }
    }

    /// `return x`: a non-consuming success.
    #[allow(
        clippy::unnecessary_wraps,
        reason = "Parsec's `return`, a parser that always succeeds: callers use it where an `R` is expected"
    )]
    pub const fn ok<T>(&self, x: T) -> R<T> {
        Ok(x)
    }

    // ----- primitives

    pub fn peek(&self) -> Option<char> {
        self.input.get(self.idx).copied()
    }

    pub fn at_eof(&self) -> bool {
        self.idx >= self.input.len()
    }

    fn advance(&mut self, c: char) {
        self.idx += 1;
        self.pos = update_pos_char(self.pos, c);
        self.counter += 1;
        self.err = ParseError::unknown(self.pos);
    }

    /// `satisfy`.
    pub fn satisfy(&mut self, f: impl Fn(char) -> bool) -> R<char> {
        match self.peek() {
            Some(c) if f(c) => {
                self.advance(c);
                Ok(c)
            }
            _ => self.empty_fail(&ParseError {
                pos: self.pos,
                msgs: vec![Msg::SysUnExpect, Msg::Expect],
            }),
        }
    }

    pub fn char_(&mut self, c: char) -> R<char> {
        self.satisfy(|x| x == c)
    }

    pub fn one_of(&mut self, cs: &str) -> R<char> {
        self.satisfy(|x| cs.contains(x))
    }

    pub fn none_of(&mut self, cs: &str) -> R<char> {
        self.satisfy(|x| !cs.contains(x))
    }

    pub fn any_char(&mut self) -> R<char> {
        self.satisfy(|_| true)
    }

    pub fn digit(&mut self) -> R<char> {
        self.satisfy(|c| c.is_ascii_digit())
    }

    pub fn letter(&mut self) -> R<char> {
        self.satisfy(is_alpha)
    }

    /// `string s`: fails without consuming when the first character does not
    /// match, consuming when a later one does not.
    pub fn string(&mut self, s: &str) -> R<String> {
        let chars: Vec<char> = s.chars().collect();
        if chars.is_empty() {
            return Ok(String::new());
        }
        let start_pos = self.pos;
        let err = ParseError {
            pos: start_pos,
            msgs: vec![Msg::SysUnExpect, Msg::Expect],
        };
        match self.peek() {
            Some(c) if c == chars[0] => {}
            _ => return self.empty_fail(&err),
        }
        for (i, &want) in chars.iter().enumerate() {
            match self.input.get(self.idx + i) {
                Some(&got) if got == want => {}
                _ => {
                    // Consumed the matching prefix: a consuming failure.
                    self.counter += 1;
                    return Err(Fail {
                        consumed: true,
                        err,
                    });
                }
            }
        }
        for &c in &chars {
            self.idx += 1;
            self.pos = update_pos_char(self.pos, c);
            self.counter += 1;
        }
        self.err = ParseError::unknown(self.pos);
        Ok(s.to_string())
    }

    /// `eof` (`notFollowedBy anyToken`; `anyToken` leaves the position as is).
    pub fn eof(&mut self) -> R<()> {
        if self.at_eof() {
            self.err = ParseError::merge(&self.err, &ParseError::unknown(self.pos));
            Ok(())
        } else {
            self.empty_fail(&ParseError {
                pos: self.pos,
                msgs: vec![Msg::Expect, Msg::UnExpect],
            })
        }
    }

    // ----- combinators

    /// `a <|> b`.
    pub fn alt<T>(
        &mut self,
        a: impl FnOnce(&mut Self) -> R<T>,
        b: impl FnOnce(&mut Self) -> R<T>,
    ) -> R<T> {
        let m = self.mark();
        match a(self) {
            Ok(v) => Ok(v),
            Err(f) if self.consumed_since(&f, &m) => Err(f),
            Err(f) => {
                self.restore(&m);
                self.err = f.err;
                b(self)
            }
        }
    }

    /// `choice`: alternatives in order, then `mzero`.
    pub fn choice<T>(&mut self, ps: &mut [&mut Alternative<'_, T>]) -> R<T> {
        let m = self.mark();
        for p in ps.iter_mut() {
            match p(self) {
                Ok(v) => return Ok(v),
                Err(f) if self.consumed_since(&f, &m) => return Err(f),
                Err(f) => {
                    self.restore(&m);
                    self.err = f.err;
                }
            }
        }
        self.zero()
    }

    /// `try p`.
    pub fn try_<T>(&mut self, p: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        let m = self.mark();
        match p(self) {
            Ok(v) => Ok(v),
            Err(f) => {
                let consumed = self.consumed_since(&f, &m);
                let err = if consumed {
                    ParseError::merge(&m.err, &f.err)
                } else {
                    f.err
                };
                self.restore(&m);
                Err(Fail {
                    consumed: false,
                    err,
                })
            }
        }
    }

    /// `lookAhead p`.
    pub fn look_ahead<T>(&mut self, p: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        let m = self.mark();
        let v = p(self)?;
        let pos = m.pos;
        self.restore(&m);
        self.err = ParseError::merge(&m.err, &ParseError::unknown(pos));
        Ok(v)
    }

    /// `many p`.
    pub fn many<T>(&mut self, mut p: impl FnMut(&mut Self) -> R<T>) -> R<Vec<T>> {
        let mut out = Vec::new();
        loop {
            let m = self.mark();
            if !out.is_empty() {
                self.err = ParseError::unknown(self.pos);
            }
            match p(self) {
                Ok(v) => {
                    assert!(
                        self.counter > m.counter,
                        "ShellCheck parser: 'many' applied to a parser that accepts an empty string"
                    );
                    out.push(v);
                }
                Err(f) if self.consumed_since(&f, &m) => return Err(f),
                Err(f) => {
                    self.restore(&m);
                    self.err = f.err;
                    return Ok(out);
                }
            }
        }
    }

    /// `many1 p`.
    pub fn many1<T>(&mut self, mut p: impl FnMut(&mut Self) -> R<T>) -> R<Vec<T>> {
        let first = p(self)?;
        let mut rest = self.many(p)?;
        rest.insert(0, first);
        Ok(rest)
    }

    /// `option x p`.
    pub fn option<T>(&mut self, x: T, p: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        self.alt(p, |s| s.ok(x))
    }

    /// `optionMaybe p`.
    pub fn option_maybe<T>(&mut self, p: impl FnOnce(&mut Self) -> R<T>) -> R<Option<T>> {
        self.alt(|s| p(s).map(Some), |s| s.ok(None))
    }

    /// `optional p`.
    pub fn optional<T>(&mut self, p: impl FnOnce(&mut Self) -> R<T>) -> R<()> {
        self.alt(|s| p(s).map(|_| ()), |s| s.ok(()))
    }

    /// `sepBy1 p sep`.
    pub fn sep_by1<T, S>(
        &mut self,
        mut p: impl FnMut(&mut Self) -> R<T>,
        mut sep: impl FnMut(&mut Self) -> R<S>,
    ) -> R<Vec<T>> {
        let first = p(self)?;
        let mut rest = self.many(|s| {
            sep(s)?;
            p(s)
        })?;
        rest.insert(0, first);
        Ok(rest)
    }

    /// `sepBy p sep`.
    pub fn sep_by<T, S>(
        &mut self,
        p: impl FnMut(&mut Self) -> R<T>,
        sep: impl FnMut(&mut Self) -> R<S>,
    ) -> R<Vec<T>> {
        self.alt(|s| s.sep_by1(p, sep), |s| s.ok(Vec::new()))
    }

    /// `chainl1 p op`.
    pub fn chainl1<T, F: FnOnce(T, T) -> T>(
        &mut self,
        mut p: impl FnMut(&mut Self) -> R<T>,
        mut op: impl FnMut(&mut Self) -> R<F>,
    ) -> R<T> {
        let mut x = p(self)?;
        loop {
            let m = self.mark();
            let step = (|| -> R<(F, T)> {
                let combine = op(self)?;
                let y = p(self)?;
                Ok((combine, y))
            })();
            match step {
                Ok((combine, y)) => x = combine(x, y),
                Err(fl) if self.consumed_since(&fl, &m) => return Err(fl),
                Err(fl) => {
                    self.restore(&m);
                    self.err = fl.err;
                    return Ok(x);
                }
            }
        }
    }

    /// `chainr1 p op`.
    pub fn chainr1<T, F: FnOnce(T, T) -> T>(
        &mut self,
        p: &mut dyn FnMut(&mut Self) -> R<T>,
        op: &mut dyn FnMut(&mut Self) -> R<F>,
    ) -> R<T> {
        let x = p(self)?;
        let m = self.mark();
        let step = (|| -> R<(F, T)> {
            let combine = op(self)?;
            let y = self.chainr1(p, op)?;
            Ok((combine, y))
        })();
        match step {
            Ok((combine, y)) => Ok(combine(x, y)),
            Err(fl) if self.consumed_since(&fl, &m) => Err(fl),
            Err(fl) => {
                self.restore(&m);
                self.err = fl.err;
                Ok(x)
            }
        }
    }

    // ----- ShellCheck's user and system state

    /// `getNextIdBetween`.
    pub fn next_id_between(&mut self, start: SourcePos, end: SourcePos) -> Id {
        let n = self.user.last_id.map_or(0, |n| n + 1);
        self.user.last_id = Some(n);
        if self.user.positions.len() <= n {
            self.user.positions.resize(n + 1, (start, end));
        }
        self.user.positions[n] = (start, end);
        Id(n)
    }

    pub fn span_for_id(&self, id: Id) -> (SourcePos, SourcePos) {
        self.user.positions[id.0]
    }

    /// `getNewIdFor`.
    pub fn new_id_for(&mut self, id: Id) -> Id {
        let (s, e) = self.span_for_id(id);
        self.next_id_between(s, e)
    }

    /// `startSpan` / `endSpan`.
    pub const fn start_span(&self) -> SourcePos {
        self.pos
    }

    pub fn end_span(&mut self, start: SourcePos) -> Id {
        let end = self.pos;
        self.next_id_between(start, end)
    }

    /// `getNextIdSpanningTokens`.
    pub fn id_spanning(&mut self, first: Id, last: Id) -> Id {
        let (s, _) = self.span_for_id(first);
        let (_, e) = self.span_for_id(last);
        self.next_id_between(s, e)
    }

    /// `getNextIdSpanningTokenList`.
    pub fn id_spanning_list(&mut self, tokens: &[&Token]) -> Id {
        if let (Some(first), Some(last)) = (tokens.first(), tokens.last()) {
            self.id_spanning(first.id, last.id)
        } else {
            let pos = self.pos;
            self.next_id_between(pos, pos)
        }
    }

    fn should_ignore_code(&self, code: i64) -> bool {
        let check_sourced = self.env.check_sourced;
        self.sys
            .contexts
            .iter()
            .any(|c| context_disables_code(check_sourced, code, c))
    }

    /// `addParseNote` (user state: rolled back with backtracking).
    pub fn add_parse_note(&mut self, note: ParseNote) {
        if !self.should_ignore_code(note.code) {
            self.user.notes.push(note);
        }
    }

    pub fn parse_note(&mut self, severity: Severity, code: i64, msg: &str) {
        let pos = self.pos;
        self.parse_note_at(pos, severity, code, msg);
    }

    pub fn parse_note_at(&mut self, pos: SourcePos, severity: Severity, code: i64, msg: &str) {
        self.add_parse_note(ParseNote {
            start: pos,
            end: pos,
            severity,
            code,
            msg: msg.to_string(),
        });
    }

    pub fn parse_note_at_id(&mut self, id: Id, severity: Severity, code: i64, msg: &str) {
        let (start, end) = self.span_for_id(id);
        self.add_parse_note(ParseNote {
            start,
            end,
            severity,
            code,
            msg: msg.to_string(),
        });
    }

    /// `parseProblemAtWithEnd` (system state: survives backtracking).
    pub fn parse_problem_at_with_end(
        &mut self,
        start: SourcePos,
        end: SourcePos,
        severity: Severity,
        code: i64,
        msg: &str,
    ) {
        if !self.should_ignore_code(code) {
            self.sys.problems.push(ParseNote {
                start,
                end,
                severity,
                code,
                msg: msg.to_string(),
            });
        }
    }

    pub fn parse_problem_at(&mut self, pos: SourcePos, severity: Severity, code: i64, msg: &str) {
        self.parse_problem_at_with_end(pos, pos, severity, code, msg);
    }

    pub fn parse_problem(&mut self, severity: Severity, code: i64, msg: &str) {
        let pos = self.pos;
        self.parse_problem_at(pos, severity, code, msg);
    }

    pub fn parse_problem_at_id(&mut self, id: Id, severity: Severity, code: i64, msg: &str) {
        let (start, end) = self.span_for_id(id);
        self.parse_problem_at_with_end(start, end, severity, code, msg);
    }

    /// `parsecBracket (pushContext entry) popContext p`: the context is popped
    /// on success and on a non-consuming failure, and stays on the stack
    /// after a consuming failure.
    pub fn with_context<T>(&mut self, entry: Context, p: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        self.sys.contexts.push(entry);
        let m = self.mark();
        match p(self) {
            Ok(v) => {
                self.sys.contexts.pop();
                Ok(v)
            }
            Err(f) if self.consumed_since(&f, &m) => Err(f),
            Err(f) => {
                self.restore(&m);
                self.err = f.err;
                self.sys.contexts.pop();
                self.fail("")
            }
        }
    }

    /// `called s p`.
    pub fn called<T>(&mut self, name: &str, p: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        let pos = self.pos;
        self.with_context(Context::Name(pos, name.to_string()), p)
    }

    /// `withAnnotations`.
    pub fn with_annotations<T>(
        &mut self,
        anns: Vec<Annotation>,
        p: impl FnOnce(&mut Self) -> R<T>,
    ) -> R<T> {
        if anns.is_empty() {
            p(self)
        } else {
            self.with_context(Context::Annotation(anns), p)
        }
    }

    /// `swapContext contexts p`.
    pub fn swap_context<T>(
        &mut self,
        contexts: Vec<Context>,
        p: impl FnOnce(&mut Self) -> R<T>,
    ) -> R<T> {
        let old = std::mem::replace(&mut self.sys.contexts, contexts);
        let m = self.mark();
        match p(self) {
            Ok(v) => {
                self.sys.contexts = old;
                Ok(v)
            }
            Err(f) if self.consumed_since(&f, &m) => Err(f),
            Err(f) => {
                self.restore(&m);
                self.err = f.err;
                self.sys.contexts = old;
                self.fail("")
            }
        }
    }

    /// `subParse pos parser input`: runs a parser on other input.
    pub fn sub_parse<T>(
        &mut self,
        pos: SourcePos,
        input: &str,
        p: impl FnOnce(&mut Self) -> R<T>,
    ) -> R<T> {
        let last_pos = self.pos;
        let last_input = self.input.clone();
        let last_idx = self.idx;
        self.pos = pos;
        self.input = Rc::new(input.chars().collect());
        self.idx = 0;
        let v = p(self)?;
        self.input = last_input;
        self.idx = last_idx;
        self.pos = last_pos;
        Ok(v)
    }
}

/// `contextItemDisablesCode`.
pub fn context_disables_code(check_sourced: bool, code: i64, item: &Context) -> bool {
    match item {
        Context::Annotation(list) => list.iter().any(
            |a| matches!(a, Annotation::DisableComment(from, to) if code >= *from && code < *to),
        ),
        Context::Source(_) => !check_sourced,
        Context::Name(..) => false,
    }
}

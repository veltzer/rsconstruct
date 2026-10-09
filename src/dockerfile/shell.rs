//! The shell side of hadolint's rules: which commands a RUN script calls,
//! with what arguments and flags, and whether it pipes.
//!
//! hadolint parses the script with `ShellCheck`'s parser and walks the tree
//! (`Hadolint.Shell`): every simple command whose name is a plain literal
//! becomes a `Command`, including the ones inside `$(...)`, `if`, loops
//! and subshells, in source order, and each one twice (the walk yields it
//! for its redirection wrapper and again for itself); each argument word
//! is flattened the way
//! `simplify` does it (quotes dropped, `$VAR` and `${VAR...}` kept as
//! `${...}`, command substitutions as `${...}` of their words run together,
//! backticks and arithmetic as `${VAR}`, brace expansions dropped). This
//! module is a shell reader written for that purpose: it understands the
//! quoting, expansion and compound-command syntax those queries depend on,
//! not the whole of bash.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdPart {
    pub arg: String,
    /// Position of the word in its command (`ShellCheck`'s token id stands
    /// in for this: only the order matters).
    pub id: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub name: String,
    pub arguments: Vec<CmdPart>,
    pub flags: Vec<CmdPart>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedShell {
    pub original: String,
    pub commands: Vec<Command>,
    pub has_pipes: bool,
}

impl Command {
    pub fn get_args(&self) -> Vec<&str> {
        self.arguments.iter().map(|a| a.arg.as_str()).collect()
    }

    /// `getArgsNoFlags`: the arguments that are not flags.
    pub fn args_no_flags(&self) -> Vec<&str> {
        let flag_ids: Vec<usize> = self.flags.iter().map(|f| f.id).collect();
        self.arguments
            .iter()
            .filter(|a| !flag_ids.contains(&a.id))
            .map(|a| a.arg.as_str())
            .collect()
    }

    pub fn has_flag(&self, flag: &str) -> bool {
        self.count_flag(flag) > 0
    }

    pub fn count_flag(&self, flag: &str) -> usize {
        self.flags.iter().filter(|f| f.arg == flag).count()
    }

    pub fn has_any_flag(&self, flags: &[&str]) -> bool {
        self.flags.iter().any(|f| flags.contains(&f.arg.as_str()))
    }

    pub fn has_arg(&self, arg: &str) -> bool {
        self.arguments.iter().any(|a| a.arg == arg)
    }

    /// `cmdHasArgs`: this command is `name` and has any of `args`.
    pub fn is_with_args(&self, name: &str, args: &[&str]) -> bool {
        self.name == name
            && self
                .arguments
                .iter()
                .any(|a| args.contains(&a.arg.as_str()))
    }

    /// `cmdsHaveArgs`.
    pub fn is_any_with_args(&self, names: &[&str], args: &[&str]) -> bool {
        names.contains(&self.name.as_str())
            && self
                .arguments
                .iter()
                .any(|a| args.contains(&a.arg.as_str()))
    }

    /// `cmdHasPrefixArg`.
    pub fn has_prefix_arg(&self, name: &str, prefix: &str) -> bool {
        self.name == name && self.arguments.iter().any(|a| a.arg.starts_with(prefix))
    }

    /// `dropFlagArg`: remove the value argument that follows each of the
    /// given flags (unless the flag carried its value inline with `=`).
    pub fn drop_flag_arg(&self, flags: &[&str]) -> Self {
        let mut drop_ids = Vec::new();
        for f in &self.flags {
            if !flags.contains(&f.arg.as_str()) {
                continue;
            }
            let inline = self
                .arguments
                .iter()
                .any(|a| a.id == f.id && a.arg.contains('='));
            if inline {
                continue;
            }
            if let Some(v) = self.value_id(f.id) {
                drop_ids.push(v);
            }
        }
        Self {
            name: self.name.clone(),
            arguments: self
                .arguments
                .iter()
                .filter(|a| !drop_ids.contains(&a.id))
                .cloned()
                .collect(),
            flags: self.flags.clone(),
        }
    }

    /// `getFlagArg`: the values following each occurrence of `flag`.
    pub fn flag_args(&self, flag: &str) -> Vec<&str> {
        let ids: Vec<usize> = self
            .flags
            .iter()
            .filter(|f| f.arg == flag)
            .filter_map(|f| self.value_id(f.id))
            .collect();
        self.arguments
            .iter()
            .filter(|a| ids.contains(&a.id))
            .map(|a| a.arg.as_str())
            .collect()
    }

    /// `getValueId`: the smallest argument id greater than `id`.
    fn value_id(&self, id: usize) -> Option<usize> {
        self.arguments
            .iter()
            .map(|a| a.id)
            .filter(|&i| i > id)
            .min()
    }

    /// `isPipInstall`.
    pub fn is_pip_install(&self) -> bool {
        let args = self.get_args();
        let std = self.name.starts_with("pip")
            && !self.name.starts_with("pipenv")
            && args.contains(&"install");
        let python =
            self.name.starts_with("python") && contains_seq(&args, &["-m", "pip", "install"]);
        std || python
    }
}

/// `isInfixOf` for argument lists.
pub fn contains_seq(args: &[&str], seq: &[&str]) -> bool {
    if seq.is_empty() {
        return true;
    }
    args.windows(seq.len()).any(|w| w == seq)
}

/// `getAllFlags`: `--name[=value]` gives `name`, `-abc` gives `a`, `b`,
/// `c`; `-` and `--` are not flags.
fn flags_of(args: &[CmdPart]) -> Vec<CmdPart> {
    let mut out = Vec::new();
    for a in args {
        let s = a.arg.as_str();
        if s == "--" || s == "-" {
            continue;
        }
        if let Some(rest) = s.strip_prefix("--") {
            let name = rest.split('=').next().unwrap_or("");
            out.push(CmdPart {
                arg: name.to_string(),
                id: a.id,
            });
        } else if let Some(rest) = s.strip_prefix('-') {
            for c in rest.chars() {
                out.push(CmdPart {
                    arg: c.to_string(),
                    id: a.id,
                });
            }
        }
    }
    out
}

pub fn parse_shell(text: &str) -> ParsedShell {
    let mut reader = Reader::new(text);
    reader.script();
    // ShellCheck gives up on a script it cannot parse (an unterminated
    // quote, a redirection without a target): hadolint then sees no
    // commands and no pipes at all.
    if reader.parse_failed {
        return ParsedShell {
            original: text.to_string(),
            commands: Vec::new(),
            has_pipes: false,
        };
    }
    ParsedShell {
        original: text.to_string(),
        commands: reader.commands,
        has_pipes: reader.pipes > 0,
    }
}

// ---- words ----------------------------------------------------------------------

/// One part of a word, as `ShellCheck`'s AST distinguishes them for `simplify`.
#[derive(Debug, Clone)]
enum Part {
    Literal(String),
    SingleQuoted(String),
    DoubleQuoted(Vec<Self>),
    /// `$NAME` or `${...}`: the text inside.
    Var(String),
    /// `$(...)`: the inner script's words, run together.
    CmdSubst(String),
    /// Backticks and `$((...))`.
    Opaque,
    Glob(String),
    /// `{a,b}`: dropped by `simplify`.
    Brace,
}

#[derive(Debug, Clone, Default)]
struct Word {
    parts: Vec<Part>,
    /// Commands found inside `$(...)` of this word, in order.
    inner: Vec<Command>,
    inner_pipes: usize,
}

impl Word {
    fn simplify(&self) -> String {
        self.parts.iter().map(simplify).collect()
    }

    /// `getLiteralString`: the word when it is literal text only.
    fn literal(&self) -> Option<String> {
        let mut out = String::new();
        for p in &self.parts {
            match p {
                Part::Literal(s) | Part::SingleQuoted(s) => out.push_str(s),
                Part::DoubleQuoted(inner) => {
                    for q in inner {
                        match q {
                            Part::Literal(s) => out.push_str(s),
                            _ => return None,
                        }
                    }
                }
                _ => return None,
            }
        }
        Some(out)
    }

    fn is_assignment(&self) -> bool {
        // NAME= or NAME+= or NAME[idx]= at the start, with a literal name
        let Some(Part::Literal(first)) = self.parts.first() else {
            return false;
        };
        let bytes = first.as_bytes();
        if bytes.is_empty() || !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_') {
            return false;
        }
        let mut i = 1;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'[' {
            while i < bytes.len() && bytes[i] != b']' {
                i += 1;
            }
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'+' {
            i += 1;
        }
        i < bytes.len() && bytes[i] == b'='
    }
}

fn simplify(p: &Part) -> String {
    match p {
        Part::Literal(s) | Part::SingleQuoted(s) | Part::Glob(s) => s.clone(),
        Part::DoubleQuoted(inner) => inner.iter().map(simplify).collect(),
        Part::Var(v) => format!("${{{v}}}"),
        Part::CmdSubst(s) => format!("${{{s}}}"),
        Part::Opaque => "${VAR}".to_string(),
        Part::Brace => String::new(),
    }
}

// ---- reader -----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(Word),
    /// `;`, `&&`, `||`, `&`, `|`, `|&`, `;;`, `;&`, `;;&`, `(`, `)`, newline
    Op(String),
    End,
}

impl PartialEq for Word {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}
impl Eq for Word {}

struct Reader {
    chars: Vec<char>,
    pos: usize,
    commands: Vec<Command>,
    pipes: usize,
    next_id: usize,
    /// Heredoc delimiters whose bodies start after the current line.
    pending_heredocs: Vec<String>,
    /// Something `ShellCheck`'s parser would reject outright.
    parse_failed: bool,
}

impl Reader {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            pos: 0,
            commands: Vec::new(),
            pipes: 0,
            next_id: 0,
            pending_heredocs: Vec::new(),
            parse_failed: false,
        }
    }
}

const RESERVED: &[&str] = &[
    "if", "then", "else", "elif", "fi", "do", "done", "while", "until", "!", "{", "}", "time",
    "coproc",
];

impl Reader {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn skip_blanks(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r') => self.pos += 1,
                Some('\\') if self.peek_at(1) == Some('\n') => self.pos += 2,
                Some('#') => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    /// After a newline: skip the bodies of heredocs opened on the line.
    fn skip_heredoc_bodies(&mut self) {
        let delims = std::mem::take(&mut self.pending_heredocs);
        for d in delims {
            loop {
                if self.pos >= self.chars.len() {
                    return;
                }
                let end = self.chars[self.pos..]
                    .iter()
                    .position(|&c| c == '\n')
                    .map_or(self.chars.len(), |p| self.pos + p);
                let line: String = self.chars[self.pos..end].iter().collect();
                self.pos = (end + 1).min(self.chars.len());
                if line.trim_start_matches('\t') == d {
                    break;
                }
            }
        }
    }

    fn next_token(&mut self) -> Token {
        self.skip_blanks();
        let Some(c) = self.peek() else {
            return Token::End;
        };
        let two: String = self.chars[self.pos..(self.pos + 3).min(self.chars.len())]
            .iter()
            .collect();
        for op in [
            ";;&", "|&", "&&", "||", ";;", ";&", "<<<", ">>", "&>", "<&", ">&", "<>", ">|",
        ] {
            if two.starts_with(op) {
                self.pos += op.chars().count();
                return self.op_token(op);
            }
        }
        if two.starts_with("<<") {
            self.pos += 2;
            if self.peek() == Some('-') {
                self.pos += 1;
            }
            // the delimiter word
            self.skip_blanks();
            let w = self.word();
            let d = w.simplify();
            self.pending_heredocs
                .push(d.trim_matches(['"', '\'']).to_string());
            return self.next_token();
        }
        match c {
            '\n' => {
                self.pos += 1;
                self.skip_heredoc_bodies();
                Token::Op("\n".to_string())
            }
            ';' | '&' | '|' | '(' | ')' => {
                self.pos += 1;
                self.op_token(&c.to_string())
            }
            '<' | '>' => {
                self.pos += 1;
                self.op_token(&c.to_string())
            }
            _ => {
                // A redirection with a file descriptor: digits then < or >
                let mut i = self.pos;
                while self.chars.get(i).is_some_and(char::is_ascii_digit) {
                    i += 1;
                }
                if i > self.pos && self.chars.get(i).is_some_and(|&c| c == '<' || c == '>') {
                    self.pos = i;
                    return self.next_token();
                }
                Token::Word(self.word())
            }
        }
    }

    /// Redirection operators swallow the word that follows; `|` counts.
    fn op_token(&mut self, op: &str) -> Token {
        match op {
            "<" | ">" | ">>" | "&>" | "<&" | ">&" | "<>" | ">|" | "<<<" => {
                self.skip_blanks();
                if self
                    .peek()
                    .is_some_and(|c| !matches!(c, '\n' | ';' | '&' | '|' | '(' | ')'))
                {
                    let _ = self.word();
                } else {
                    // A redirection needs a target.
                    self.parse_failed = true;
                }
                self.next_token()
            }
            "|" | "|&" => {
                self.pipes += 1;
                Token::Op(op.to_string())
            }
            _ => Token::Op(op.to_string()),
        }
    }

    fn word(&mut self) -> Word {
        let mut word = Word::default();
        let mut lit = String::new();
        let flush = |lit: &mut String, word: &mut Word| {
            if !lit.is_empty() {
                word.parts.push(Part::Literal(std::mem::take(lit)));
            }
        };
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\n' | '\r' | ';' | '&' | '|' | '(' | ')' | '<' | '>' => break,
                '\\' => {
                    self.pos += 1;
                    match self.peek() {
                        None => {}
                        Some('\n') => self.pos += 1,
                        Some(e) => {
                            self.pos += 1;
                            lit.push(e);
                        }
                    }
                }
                '\'' => {
                    flush(&mut lit, &mut word);
                    self.pos += 1;
                    let s = self.take_until(|c| c == '\'');
                    word.parts.push(Part::SingleQuoted(s));
                }
                '"' => {
                    flush(&mut lit, &mut word);
                    self.pos += 1;
                    let parts = self.double_quoted(&mut word);
                    word.parts.push(Part::DoubleQuoted(parts));
                }
                '$' => {
                    flush(&mut lit, &mut word);
                    if let Some(p) = self.dollar(&mut word) {
                        word.parts.push(p);
                    } else {
                        lit.push('$');
                    }
                }
                '`' => {
                    flush(&mut lit, &mut word);
                    self.pos += 1;
                    let inner = self.take_until(|c| c == '`');
                    self.absorb_inner(&inner, &mut word);
                    word.parts.push(Part::Opaque);
                }
                '*' | '?' => {
                    flush(&mut lit, &mut word);
                    self.pos += 1;
                    word.parts.push(Part::Glob(c.to_string()));
                }
                '[' => {
                    // a glob class when it closes before the word ends
                    let close = self.chars[self.pos + 1..]
                        .iter()
                        .position(|&c| c == ']' || c == ' ' || c == '\n' || c == '\t');
                    match close {
                        Some(n) if self.chars.get(self.pos + 1 + n) == Some(&']') && n > 0 => {
                            flush(&mut lit, &mut word);
                            let s: String =
                                self.chars[self.pos..=self.pos + 1 + n].iter().collect();
                            self.pos += n + 2;
                            word.parts.push(Part::Glob(s));
                        }
                        _ => {
                            self.pos += 1;
                            lit.push('[');
                        }
                    }
                }
                '{' => {
                    // brace expansion: {a,b} or {1..3} with no spaces
                    let close = self.chars[self.pos + 1..]
                        .iter()
                        .position(|&c| c == '}' || c == ' ' || c == '\n' || c == '\t');
                    let text: Option<String> = close.and_then(|n| {
                        if self.chars.get(self.pos + 1 + n) == Some(&'}') {
                            Some(self.chars[self.pos + 1..self.pos + 1 + n].iter().collect())
                        } else {
                            None
                        }
                    });
                    match text {
                        Some(t) if t.contains(',') || t.contains("..") => {
                            flush(&mut lit, &mut word);
                            self.pos += t.chars().count() + 2;
                            word.parts.push(Part::Brace);
                        }
                        _ => {
                            self.pos += 1;
                            lit.push('{');
                        }
                    }
                }
                _ => {
                    self.pos += 1;
                    lit.push(c);
                }
            }
        }
        flush(&mut lit, &mut word);
        word
    }

    /// Text up to the closing character; reaching the end instead is an
    /// unterminated quote, which `ShellCheck` rejects.
    fn take_until(&mut self, stop: impl Fn(char) -> bool) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            if stop(c) {
                return s;
            }
            s.push(c);
        }
        self.parse_failed = true;
        s
    }

    fn double_quoted(&mut self, word: &mut Word) -> Vec<Part> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        loop {
            let Some(c) = self.peek() else {
                self.parse_failed = true;
                break;
            };
            match c {
                '"' => {
                    self.pos += 1;
                    break;
                }
                '\\' => {
                    self.pos += 1;
                    match self.peek() {
                        Some(e @ ('"' | '\\' | '$' | '`')) => {
                            self.pos += 1;
                            lit.push(e);
                        }
                        Some('\n') => self.pos += 1,
                        Some(_) => lit.push('\\'),
                        None => {}
                    }
                }
                '$' => {
                    if !lit.is_empty() {
                        parts.push(Part::Literal(std::mem::take(&mut lit)));
                    }
                    if let Some(p) = self.dollar(word) {
                        parts.push(p);
                    } else {
                        lit.push('$');
                    }
                }
                '`' => {
                    if !lit.is_empty() {
                        parts.push(Part::Literal(std::mem::take(&mut lit)));
                    }
                    self.pos += 1;
                    let inner = self.take_until(|c| c == '`');
                    self.absorb_inner(&inner, word);
                    parts.push(Part::Opaque);
                }
                _ => {
                    self.pos += 1;
                    lit.push(c);
                }
            }
        }
        if !lit.is_empty() {
            parts.push(Part::Literal(lit));
        }
        parts
    }

    /// At a `$`: an expansion part, or `None` for a lone dollar.
    fn dollar(&mut self, word: &mut Word) -> Option<Part> {
        match self.peek_at(1) {
            Some('(') => {
                if self.peek_at(2) == Some('(') {
                    // arithmetic
                    self.pos += 3;
                    let mut depth = 2;
                    while let Some(c) = self.peek() {
                        self.pos += 1;
                        match c {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    return Some(Part::Opaque);
                }
                self.pos += 2;
                let inner = self.balanced(')');
                let text = self.absorb_inner(&inner, word);
                Some(Part::CmdSubst(text))
            }
            Some('{') => {
                self.pos += 2;
                let inner = self.balanced('}');
                Some(Part::Var(inner))
            }
            Some('\'') => {
                self.pos += 2;
                let s = self.take_until(|c| c == '\'');
                Some(Part::SingleQuoted(s))
            }
            Some('"') => {
                self.pos += 2;
                let parts = self.double_quoted(word);
                Some(Part::DoubleQuoted(parts))
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                self.pos += 1;
                let name = self.take_while_chars(|c| c.is_ascii_alphanumeric() || c == '_');
                Some(Part::Var(name))
            }
            Some(c)
                if c.is_ascii_digit() || matches!(c, '@' | '*' | '#' | '?' | '-' | '$' | '!') =>
            {
                self.pos += 2;
                Some(Part::Var(c.to_string()))
            }
            _ => {
                self.pos += 1;
                None
            }
        }
    }

    fn take_while_chars(&mut self, pred: impl Fn(char) -> bool) -> String {
        let start = self.pos;
        while self.peek().is_some_and(&pred) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    /// Text up to the matching `close`, honouring nesting and quotes.
    fn balanced(&mut self, close: char) -> String {
        let open = if close == ')' { '(' } else { '{' };
        let mut depth = 1;
        let mut out = String::new();
        let mut quote: Option<char> = None;
        while let Some(c) = self.peek() {
            self.pos += 1;
            if let Some(q) = quote {
                out.push(c);
                if c == '\\' && q == '"' {
                    if let Some(n) = self.peek() {
                        self.pos += 1;
                        out.push(n);
                    }
                } else if c == q {
                    quote = None;
                }
                continue;
            }
            match c {
                '\'' | '"' => {
                    quote = Some(c);
                    out.push(c);
                }
                '\\' => {
                    out.push(c);
                    if let Some(n) = self.peek() {
                        self.pos += 1;
                        out.push(n);
                    }
                }
                _ if c == open => {
                    depth += 1;
                    out.push(c);
                }
                _ if c == close => {
                    depth -= 1;
                    if depth == 0 {
                        return out;
                    }
                    out.push(c);
                }
                _ => out.push(c),
            }
        }
        self.parse_failed = true;
        out
    }

    /// Parse the text of a command substitution: its commands join the
    /// word's inner commands, and its words make the `${...}` text.
    fn absorb_inner(&mut self, inner: &str, word: &mut Word) -> String {
        let mut sub = Self::new(inner);
        let words_text = sub.script_words();
        if sub.parse_failed {
            self.parse_failed = true;
        }
        word.inner.extend(sub.commands);
        word.inner_pipes += sub.pipes;
        words_text
    }

    // ---- commands -----------------------------------------------------------------

    fn script(&mut self) {
        self.script_words();
    }

    /// Read the whole script; returns the simplified words of all its
    /// simple commands run together (what `simplify` makes of `$(...)`).
    fn script_words(&mut self) -> String {
        let mut all_words = String::new();
        let mut words: Vec<Word> = Vec::new();
        let mut at_start = true;
        let mut in_case_pattern = false;
        let mut expect_for_list = false;
        let mut skip_cond = false;
        loop {
            let tok = self.next_token();
            match tok {
                Token::End => {
                    self.flush_command(&mut words, &mut all_words);
                    break;
                }
                Token::Op(op) => {
                    match op.as_str() {
                        ")" if in_case_pattern => {
                            in_case_pattern = false;
                            words.clear();
                            at_start = true;
                            continue;
                        }
                        ";;" | ";&" | ";;&" => {
                            self.flush_command(&mut words, &mut all_words);
                            in_case_pattern = true;
                            at_start = true;
                            continue;
                        }
                        _ => {}
                    }
                    if expect_for_list && matches!(op.as_str(), ";" | "\n") {
                        expect_for_list = false;
                        words.clear();
                        at_start = true;
                        continue;
                    }
                    self.flush_command(&mut words, &mut all_words);
                    at_start = true;
                    skip_cond = false;
                }
                Token::Word(w) => {
                    if in_case_pattern && w.literal().as_deref() == Some("esac") {
                        in_case_pattern = false;
                        at_start = true;
                        continue;
                    }
                    if in_case_pattern || expect_for_list {
                        continue;
                    }
                    if skip_cond {
                        if w.literal().as_deref() == Some("]]") {
                            skip_cond = false;
                        }
                        continue;
                    }
                    if at_start {
                        let lit = w.literal();
                        match lit.as_deref() {
                            Some("[[") => {
                                skip_cond = true;
                                continue;
                            }
                            Some("case") => {
                                // case WORD in
                                let _subject = self.next_token();
                                let _in = self.next_token();
                                in_case_pattern = true;
                                continue;
                            }
                            Some("esac") => {
                                at_start = true;
                                continue;
                            }
                            Some("for" | "select") => {
                                let _name = self.next_token();
                                // optional `in list...` up to ; or newline
                                expect_for_list = true;
                                continue;
                            }
                            Some("function") => {
                                let _name = self.next_token();
                                continue;
                            }
                            Some("in") => {
                                // `for x` followed by `in` on the next line
                                expect_for_list = true;
                                continue;
                            }
                            Some(r) if RESERVED.contains(&r) => continue,
                            _ => {}
                        }
                        if w.is_assignment() {
                            // prefix assignment: its inner commands still count
                            self.commands.extend(w.inner.iter().cloned());
                            self.pipes += w.inner_pipes;
                            all_words.push_str(&w.simplify());
                            continue;
                        }
                        // function definition: name ( )
                        let save = self.pos;
                        let save_pipes = self.pipes;
                        if let Token::Op(o) = self.next_token()
                            && o == "("
                            && let Token::Op(c) = self.next_token()
                            && c == ")"
                        {
                            continue;
                        }
                        self.pos = save;
                        self.pipes = save_pipes;
                        at_start = false;
                    }
                    words.push(w);
                }
            }
        }
        all_words
    }

    fn flush_command(&mut self, words: &mut Vec<Word>, all_words: &mut String) {
        if words.is_empty() {
            return;
        }
        let taken = std::mem::take(words);
        for w in &taken {
            all_words.push_str(&w.simplify());
        }
        let name = taken[0].literal();
        let name = match name.as_deref() {
            Some(n) if (n.ends_with("busybox") || n == "builtin") && taken.len() > 1 => {
                taken[1].literal()
            }
            other => other.map(str::to_string),
        };
        let mut inner = Vec::new();
        let mut inner_pipes = 0;
        for w in &taken {
            inner.extend(w.inner.iter().cloned());
            inner_pipes += w.inner_pipes;
        }
        if let Some(name) = name {
            let arguments: Vec<CmdPart> = taken[1..]
                .iter()
                .map(|w| {
                    self.next_id += 1;
                    CmdPart {
                        arg: w.simplify(),
                        id: self.next_id,
                    }
                })
                .collect();
            let flags = flags_of(&arguments);
            let command = Command {
                name,
                arguments,
                flags,
            };
            // ShellCheck wraps every simple command in a redirection node,
            // and hadolint's tree walk yields the command once for the
            // wrapper and once for the command itself; the rules see each
            // command twice, and DL3059's "more than 2" relies on that.
            self.commands.push(command.clone());
            self.commands.push(command);
        }
        self.commands.extend(inner);
        self.pipes += inner_pipes;
    }
}

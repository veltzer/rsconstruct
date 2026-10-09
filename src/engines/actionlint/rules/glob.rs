//! The `glob` rule and the filter-pattern validator behind it, a port of
//! actionlint's `glob.go` and `rule_glob.go`.

use super::{Rule, RuleBase};
use crate::engines::actionlint::ast::{Event, Job, Pos, Str, WebhookEventFilter, Workflow};
use crate::engines::actionlint::expr::lexer::go_quote_rune;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidGlobPattern {
    pub message: String,
    /// 1-based column of the next unread character minus one; 0 when past
    /// the first line or unknown.
    pub column: u32,
}

struct GlobValidator {
    is_ref: bool,
    prec: bool,
    errs: Vec<InvalidGlobPattern>,
    chars: Vec<char>,
    idx: usize,
    line: u32,
}

impl GlobValidator {
    fn new(pat: &str, is_ref: bool) -> Self {
        Self {
            is_ref,
            prec: false,
            errs: Vec::new(),
            chars: pat.chars().collect(),
            idx: 0,
            line: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.idx).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek();
        if let Some(c) = c {
            self.idx += 1;
            if c == '\n' {
                self.line += 1;
            }
        }
        c
    }

    fn error(&mut self, message: String) {
        // text/scanner's Pos().Column is the 1-based column of the next
        // character; the Go code stores it minus one, and 0 past line 1.
        let column = if self.line > 1 {
            0
        } else {
            u32::try_from(self.idx).unwrap_or(u32::MAX)
        };
        self.errs.push(InvalidGlobPattern { message, column });
    }

    fn unexpected(&mut self, c: Option<char>, what: &str, why: &str) {
        let unexpected = match c {
            None => "unexpected EOF".to_string(),
            Some(c) => format!("unexpected character {}", go_quote_rune(c)),
        };
        let while_ = if what.is_empty() {
            String::new()
        } else {
            format!(" while checking {what}")
        };
        self.error(format!("invalid glob pattern. {unexpected}{while_}. {why}"));
    }

    fn invalid_ref_char(&mut self, c: Option<char>, why: &str) {
        let shown = match c {
            Some(c) if !c.is_control() => format!("'{c}'"),
            Some(c) => go_quote_rune(c),
            None => "'\\uFFFFFFFFFFFFFFFF'".to_string(),
        };
        self.error(format!(
            "character {shown} is invalid for branch and tag names. {why}. see `man git-check-ref-format` for more details. note that regular expression is unavailable"
        ));
    }

    fn validate_next(&mut self) -> bool {
        let mut c = self.next();
        let mut prec = true;
        match c {
            Some('\\') => match self.peek() {
                Some('[' | '?' | '*') => {
                    c = self.next();
                    if self.is_ref {
                        let p = self.peek();
                        self.invalid_ref_char(
                            p,
                            "ref name cannot contain spaces, ~, ^, :, [, ?, *",
                        );
                    }
                }
                Some('+' | '\\' | '!') => {
                    c = self.next();
                }
                _ => {
                    if self.is_ref {
                        self.invalid_ref_char(
                            Some('\\'),
                            "only special characters [, ?, +, *, \\, ! can be escaped with \\",
                        );
                        c = self.next();
                    }
                }
            },
            Some('?') => {
                if !self.prec {
                    self.unexpected(
                        Some('?'),
                        "special character ? (zero or one)",
                        "the preceding character must not be special character",
                    );
                }
                prec = false;
            }
            Some('+') => {
                if !self.prec {
                    self.unexpected(
                        Some('+'),
                        "special character + (one or more)",
                        "the preceding character must not be special character",
                    );
                }
                prec = false;
            }
            Some('*') => prec = false,
            Some('[') => {
                if self.peek() == Some(']') {
                    c = self.next();
                    self.unexpected(
                        Some(']'),
                        "content of character match []",
                        "character match must not be empty",
                    );
                } else {
                    let mut chars = 0;
                    loop {
                        c = self.next();
                        match c {
                            Some(']') => break,
                            None => {
                                self.unexpected(None, "end of character match []", "missing ]");
                                return false;
                            }
                            Some(s) => {
                                if self.peek() != Some('-') {
                                    chars += 1;
                                    continue;
                                }
                                chars += 2;
                                self.next(); // the '-'
                                match self.peek() {
                                    Some(']') => {
                                        c = self.next();
                                        self.unexpected(
                                            c,
                                            "character range in []",
                                            "end of range is missing",
                                        );
                                        break;
                                    }
                                    None => {}
                                    Some(_) => {
                                        c = self.next();
                                        if let Some(e) = c
                                            && s > e
                                        {
                                            let why = format!(
                                                "start of range {} ({}) is larger than end of range {} ({})",
                                                go_quote_rune(s),
                                                s as u32,
                                                go_quote_rune(e),
                                                e as u32
                                            );
                                            self.unexpected(c, "character range in []", &why);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if chars == 1 {
                        self.unexpected(
                            c,
                            "character match []",
                            "character match with single character is useless. simply use x instead of [x]",
                        );
                    }
                }
            }
            Some('\r') => {
                if self.peek() == Some('\n') {
                    c = self.next();
                }
                self.unexpected(c, "", "newline cannot be contained");
            }
            Some('\n') => self.unexpected(Some('\n'), "", "newline cannot be contained"),
            Some(' ' | '\t' | '~' | '^' | ':') if self.is_ref => {
                self.invalid_ref_char(c, "ref name cannot contain spaces, ~, ^, :, [, ?, *");
            }
            _ => {}
        }
        self.prec = prec;
        if self.peek().is_none() {
            if self.is_ref && matches!(c, Some('/' | '.')) {
                self.invalid_ref_char(c, "ref name must not end with / and .");
            }
            return false;
        }
        true
    }

    fn validate(&mut self, pat: &str) {
        if pat.is_empty() {
            self.error("glob pattern cannot be empty".to_string());
            return;
        }
        match self.peek() {
            Some('/') => {
                if self.is_ref {
                    self.next();
                    self.invalid_ref_char(Some('/'), "ref name must not start with /");
                    self.prec = true;
                }
            }
            Some('!') => {
                self.next();
                if self.peek().is_none() {
                    self.unexpected(
                        Some('!'),
                        "! at first character (negate pattern)",
                        "at least one character must follow !",
                    );
                    return;
                }
                self.prec = false;
            }
            _ => {}
        }
        while self.validate_next() {}
    }
}

fn validate_glob(pat: &str, is_ref: bool) -> Vec<InvalidGlobPattern> {
    let mut v = GlobValidator::new(pat, is_ref);
    v.validate(pat);
    v.errs
}

/// Validates a branch or tag filter pattern.
pub fn validate_ref_glob(pat: &str) -> Vec<InvalidGlobPattern> {
    validate_glob(pat, true)
}

/// Validates a path filter pattern.
pub fn validate_path_glob(pat: &str) -> Vec<InvalidGlobPattern> {
    let p = pat.trim();
    let mut errs = Vec::new();
    if pat != p {
        errs.push(InvalidGlobPattern {
            message: "leading and trailing spaces are not allowed in glob path".to_string(),
            column: 0,
        });
    }
    let p = p.strip_prefix('!').unwrap_or(p);
    if p == "." || p == ".." || p.starts_with("./") || p.starts_with("../") {
        errs.push(InvalidGlobPattern {
            message: "'.' and '..' are not allowed in glob path".to_string(),
            column: 0,
        });
    }
    if !errs.is_empty() {
        return errs;
    }
    validate_glob(pat, false)
}

pub struct RuleGlob {
    base: RuleBase,
}

impl RuleGlob {
    pub const fn new() -> Self {
        Self {
            base: RuleBase::new("glob"),
        }
    }

    fn check_ref_glob(&mut self, s: Option<&Str>) {
        let Some(s) = s else {
            return;
        };
        if s.value.is_empty() {
            return;
        }
        let errs = validate_ref_glob(&s.value);
        self.glob_errors(&errs, s.pos, s.quoted);
    }

    fn check_git_ref_globs(&mut self, filter: Option<&WebhookEventFilter>) {
        let Some(filter) = filter else {
            return;
        };
        for v in &filter.values {
            self.check_ref_glob(Some(v));
        }
    }

    fn check_file_path_globs(&mut self, filter: Option<&WebhookEventFilter>) {
        let Some(filter) = filter else {
            return;
        };
        for v in &filter.values {
            if !v.value.is_empty() {
                let errs = validate_path_glob(&v.value);
                self.glob_errors(&errs, v.pos, v.quoted);
            }
        }
    }

    fn glob_errors(&mut self, errs: &[InvalidGlobPattern], pos: Pos, quoted: bool) {
        for err in errs {
            let mut p = pos;
            if quoted {
                p.col += 1;
            }
            if err.column != 0 {
                p.col += err.column - 1;
            }
            self.base.error(
                p,
                format!(
                    "{}. note: filter pattern syntax is explained at https://docs.github.com/en/actions/using-workflows/workflow-syntax-for-github-actions#filter-pattern-cheat-sheet",
                    err.message
                ),
            );
        }
    }
}

impl Rule for RuleGlob {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_workflow_pre(&mut self, workflow: &Workflow) {
        for e in workflow.on.iter().flatten() {
            match e {
                Event::Webhook(e) => {
                    self.check_git_ref_globs(e.branches.as_ref());
                    self.check_git_ref_globs(e.branches_ignore.as_ref());
                    self.check_git_ref_globs(e.tags.as_ref());
                    self.check_git_ref_globs(e.tags_ignore.as_ref());
                    self.check_file_path_globs(e.paths.as_ref());
                    self.check_file_path_globs(e.paths_ignore.as_ref());
                }
                Event::ImageVersion(e) => {
                    for v in &e.versions {
                        self.check_ref_glob(Some(v));
                    }
                }
                _ => {}
            }
        }
    }

    fn visit_job_pre(&mut self, job: &Job) {
        if let Some(s) = &job.snapshot {
            self.check_ref_glob(s.version.as_ref());
        }
    }
}

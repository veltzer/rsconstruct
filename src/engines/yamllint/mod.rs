//! An in-process port of [yamllint](https://github.com/adrienverge/yamllint),
//! the Python YAML linter: its configuration format (`.yamllint.yaml` with
//! `extends`, rule levels, options and `ignore`), its 23 rules, and its
//! `# yamllint disable` directives, producing the same problems at the same
//! positions.
//!
//! The port follows yamllint's own structure so the two can be compared
//! file by file: `parser` is `parser.py`, `config` is `config.py`, `rules/`
//! is `rules/`, and `lint` here is `linter.py`. The iyamllint processor is
//! the only user; see `docs/src/processor/checker/iyamllint.md` for what it
//! covers and the two config keys it treats differently.

pub mod config;
pub mod parser;
pub mod rules;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;

use libyaml_safer::{EventData, Parser};
use regex::Regex;

pub use config::Config;
use parser::{Elem, Stream};
use rules::{Check, Ctx, States};

/// A problem's severity. yamllint fails a run on errors; warnings fail it
/// only under `--strict`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Warning,
    Error,
}

impl Level {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// One finding: yamllint's `LintProblem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// 1-based.
    pub line: usize,
    /// 1-based.
    pub column: usize,
    pub desc: String,
    /// The rule that found it; `None` for a syntax error.
    pub rule: Option<&'static str>,
    pub level: Level,
}

impl Problem {
    pub fn new(line: usize, column: usize, desc: impl Into<String>) -> Self {
        Self {
            line,
            column,
            desc: desc.into(),
            rule: None,
            level: Level::Error,
        }
    }

    /// yamllint's `message`: the description, with the rule in parentheses.
    pub fn message(&self) -> String {
        match self.rule {
            Some(rule) => format!("{} ({rule})", self.desc),
            None => self.desc.clone(),
        }
    }
}

impl std::fmt::Display for Problem {
    /// yamllint's `parsable` format, minus the leading file name:
    /// `line:column: [level] message (rule)`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: [{}] {}",
            self.line,
            self.column,
            self.level.as_str(),
            self.message()
        )
    }
}

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("yamllint directive pattern compiles"))
}

/// The rules a `# yamllint disable` / `enable` directive has switched off.
struct DisableDirective {
    rules: BTreeSet<&'static str>,
    all_rules: BTreeSet<&'static str>,
}

/// The rule ids named in a directive after its keyword: `# yamllint disable
/// rule:a rule:b` lists `a` and `b`; a bare directive lists none.
fn directive_rules(comment: &str, keyword_len: usize) -> impl Iterator<Item = &str> {
    comment[keyword_len..]
        .trim_end()
        .split(' ')
        .skip(1)
        .map(|item| item.get(5..).unwrap_or(""))
}

impl DisableDirective {
    const fn new(all_rules: BTreeSet<&'static str>) -> Self {
        Self {
            rules: BTreeSet::new(),
            all_rules,
        }
    }

    fn disable(&mut self, comment: &str, keyword_len: usize) {
        let named: Vec<&str> = directive_rules(comment, keyword_len).collect();
        if named.is_empty() {
            self.rules.clone_from(&self.all_rules);
        } else {
            for id in named {
                if let Some(known) = self.all_rules.get(id) {
                    self.rules.insert(known);
                }
            }
        }
    }

    fn process_comment(&mut self, comment: &str) {
        static DISABLE: OnceLock<Regex> = OnceLock::new();
        static ENABLE: OnceLock<Regex> = OnceLock::new();
        if regex(&DISABLE, r"^# yamllint disable( rule:\S+)*\s*$").is_match(comment) {
            self.disable(comment, "# yamllint disable".len());
        } else if regex(&ENABLE, r"^# yamllint enable( rule:\S+)*\s*$").is_match(comment) {
            let named: Vec<&str> = directive_rules(comment, "# yamllint enable".len()).collect();
            if named.is_empty() {
                self.rules.clear();
            } else {
                for id in named {
                    self.rules.remove(id);
                }
            }
        }
    }

    /// `# yamllint disable-line`, which applies to one line only.
    fn process_line_comment(&mut self, comment: &str) {
        static DISABLE_LINE: OnceLock<Regex> = OnceLock::new();
        if regex(&DISABLE_LINE, r"^# yamllint disable-line( rule:\S+)*\s*$").is_match(comment) {
            self.disable(comment, "# yamllint disable-line".len());
        }
    }

    fn is_disabled(&self, problem: &Problem) -> bool {
        problem.rule.is_some_and(|rule| self.rules.contains(rule))
    }
}

/// Mirrors `get_syntax_error`: parse the whole stream and report where the
/// parser gives up. libyaml words its problems differently from `PyYAML`, so
/// the text differs from yamllint's; the position is the parser's.
fn syntax_error(buffer: &str) -> Option<Problem> {
    let mut parser: Parser<&mut &[u8]> = Parser::new();
    let mut input = buffer.as_bytes();
    parser.set_input_string(&mut input);
    loop {
        match parser.parse() {
            Ok(event) => {
                if matches!(event.data, EventData::StreamEnd) {
                    return None;
                }
            }
            Err(e) => {
                let mark = e.problem_mark().unwrap_or_default();
                return Some(Problem {
                    line: mark.line as usize + 1,
                    column: mark.column as usize + 1,
                    desc: format!("syntax error: {} (syntax)", e.problem()),
                    rule: None,
                    level: Level::Error,
                });
            }
        }
    }
}

/// Mirrors `get_cosmetic_problems`: every rule over the token, comment and
/// line stream, with the disable directives applied line by line.
fn cosmetic_problems(buffer: &str, conf: &Config, filepath: Option<&Path>) -> Vec<Problem> {
    let stream = Stream::new(buffer);
    let enabled = conf.enabled_rules(filepath);
    let all_rules: BTreeSet<&'static str> = enabled.iter().map(|(spec, _)| spec.id).collect();
    let mut states = States::default();

    // Problems are cached and flushed at each end of line, so a directive
    // on the line can still remove them.
    let mut cache: Vec<Problem> = Vec::new();
    let mut out = Vec::new();
    let mut disabled = DisableDirective::new(all_rules.clone());
    let mut disabled_for_line = DisableDirective::new(all_rules.clone());
    let mut disabled_for_next_line = DisableDirective::new(all_rules.clone());

    for elem in &stream.elems {
        match *elem {
            Elem::Token(idx) => {
                let ctx = Ctx {
                    stream: &stream,
                    idx,
                };
                for (spec, rule_conf) in &enabled {
                    if let Check::Token(check) = spec.check {
                        let before = cache.len();
                        check(rule_conf, &ctx, &mut states, &mut cache);
                        for problem in &mut cache[before..] {
                            problem.rule = Some(spec.id);
                            problem.level = rule_conf.level();
                        }
                    }
                }
            }
            Elem::Comment(idx) => {
                for (spec, rule_conf) in &enabled {
                    if let Check::Comment(check) = spec.check {
                        let before = cache.len();
                        check(rule_conf, &stream, idx, &mut cache);
                        for problem in &mut cache[before..] {
                            problem.rule = Some(spec.id);
                            problem.level = rule_conf.level();
                        }
                    }
                }
                let comment = stream.comments[idx];
                let text = stream.comment_text(&comment);
                disabled.process_comment(text);
                if stream.comment_is_inline(&comment) {
                    disabled_for_line.process_line_comment(text);
                } else {
                    disabled_for_next_line.process_line_comment(text);
                }
            }
            Elem::Line(idx) => {
                for (spec, rule_conf) in &enabled {
                    if let Check::Line(check) = spec.check {
                        let before = cache.len();
                        check(rule_conf, &stream, idx, &mut cache);
                        for problem in &mut cache[before..] {
                            problem.rule = Some(spec.id);
                            problem.level = rule_conf.level();
                        }
                    }
                }
                // This is the last token/comment/line of this line: flush the
                // problems found, filtered by the directives.
                for problem in std::mem::take(&mut cache) {
                    if !(disabled_for_line.is_disabled(&problem) || disabled.is_disabled(&problem))
                    {
                        out.push(problem);
                    }
                }
                disabled_for_line = disabled_for_next_line;
                disabled_for_next_line = DisableDirective::new(all_rules.clone());
            }
        }
    }
    out
}

/// Mirrors `linter._run`: lint one buffer. `filepath` is the file's path
/// relative to the project, matched against the config's `ignore` patterns.
pub fn lint(buffer: &str, conf: &Config, filepath: Option<&Path>) -> Vec<Problem> {
    if let Some(path) = filepath
        && conf.is_file_ignored(path)
    {
        return Vec::new();
    }

    static DISABLE_FILE: OnceLock<Regex> = OnceLock::new();
    let first_line = buffer
        .split('\n')
        .next()
        .unwrap_or("")
        .trim_end_matches('\r');
    if regex(&DISABLE_FILE, r"^#\s*yamllint disable-file\s*$").is_match(first_line) {
        return Vec::new();
    }

    // A syntax error is reported at its place among the cosmetic problems,
    // and the cosmetic problem at that place is dropped as redundant.
    let mut syntax_error = syntax_error(buffer);
    let mut out = Vec::new();
    for problem in cosmetic_problems(buffer, conf, filepath) {
        if let Some(err) = &syntax_error
            && err.line <= problem.line
            && err.column <= problem.column
        {
            out.push(syntax_error.take().expect("checked above"));
            continue;
        }
        out.push(problem);
    }
    if let Some(err) = syntax_error {
        out.push(err);
    }
    out
}

#[cfg(test)]
mod tests;

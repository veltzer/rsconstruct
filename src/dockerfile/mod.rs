//! hadolint, in-process: a port of hadolint 2.15.1's Dockerfile parser,
//! pragma handling and DL rules.
//!
//! What is not here is `ShellCheck`: hadolint runs `ShellCheck` over every RUN
//! script and reports its SC codes; that is a separate program and stays
//! one. Everything hadolint decides itself, from the Dockerfile parse to
//! the last `DL` rule, the `# hadolint ignore=` pragmas, the severity
//! overrides and the exit status, is reproduced.
//!
//! * [`parser`] is language-docker 16.0.0's grammar.
//! * [`shell`] is the command view of RUN scripts the rules query.
//! * [`rules`] holds the DL rules.
//! * [`config`] reads the hadolint configuration file.

pub mod config;
pub mod parser;
pub mod rules;
pub mod shell;

use config::{Config, Severity};
use parser::{Instruction, InstructionPos};

/// One finding, as hadolint prints it: `file:LINE CODE severity: message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub line: usize,
}

/// The result for one file: the findings that survive the pragmas,
/// overrides and ignore list, in line order, and whether hadolint would
/// exit non-zero for them.
#[derive(Debug, Clone)]
pub struct Report {
    pub failures: Vec<Failure>,
    pub fails: bool,
}

/// Decode a Dockerfile's bytes the way hadolint does: by byte order mark,
/// UTF-8 otherwise, lenient.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE, 0, 0]) {
        return decode_utf32(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0, 0, 0xFE, 0xFF]) {
        return decode_utf32(rest, false);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(rest, false);
    }
    let rest = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(rest).into_owned()
}

fn decode_utf16(bytes: &[u8], little: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|c| {
            let (a, b) = (c[0], *c.get(1).unwrap_or(&0));
            if little {
                u16::from_le_bytes([a, b])
            } else {
                u16::from_be_bytes([a, b])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

fn decode_utf32(bytes: &[u8], little: bool) -> String {
    bytes
        .chunks(4)
        .map(|c| {
            let mut buf = [0u8; 4];
            buf[..c.len()].copy_from_slice(c);
            let n = if little {
                u32::from_le_bytes(buf)
            } else {
                u32::from_be_bytes(buf)
            };
            char::from_u32(n).unwrap_or('\u{FFFD}')
        })
        .collect()
}

/// Lint one Dockerfile. `Err` is a parse error (hadolint reports it and
/// runs no rules).
pub fn lint(source: &str, config: &Config) -> Result<Report, parser::ParseError> {
    let doc = parser::parse(source)?;
    let mut failures = rules::run_all(&doc, config);
    // Pragmas.
    if !config.disable_ignore_pragma {
        let pragmas = Pragmas::collect(&doc);
        failures.retain(|f| !pragmas.ignores(f.line, &f.code));
    }
    // fixSeverity: overrides, then the ignore list and the ignore severity.
    for f in &mut failures {
        for (rules, sev) in [
            (&config.style_rules, Severity::Style),
            (&config.info_rules, Severity::Info),
            (&config.warning_rules, Severity::Warning),
            (&config.error_rules, Severity::Error),
        ] {
            if rules.contains(&f.code) {
                f.severity = sev;
            }
        }
    }
    failures.retain(|f| !config.ignore_rules.contains(&f.code) && f.severity != Severity::Ignore);
    failures.sort_by(|a, b| a.line.cmp(&b.line).then_with(|| a.code.cmp(&b.code)));
    let fails = !config.no_fail
        && failures
            .iter()
            .any(|f| f.severity <= config.failure_threshold);
    Ok(Report { failures, fails })
}

/// The `# hadolint ignore=...`, `# hadolint stage ignore=...` and
/// `# hadolint global ignore=...` pragmas of a file.
struct Pragmas {
    /// Line -> codes ignored on that line (the line after the pragma).
    per_line: Vec<(usize, Vec<String>)>,
    /// Line -> codes ignored from there on in the stage; FROM lines reset.
    per_stage: Vec<(usize, Vec<String>)>,
    global: Vec<String>,
}

impl Pragmas {
    fn collect(doc: &[InstructionPos]) -> Self {
        let mut per_line = Vec::new();
        let mut per_stage: Vec<(usize, Vec<String>)> = Vec::new();
        let mut global = Vec::new();
        for ip in doc {
            match &ip.instruction {
                Instruction::Comment(c) => {
                    if let Some(codes) = parse_ignore_pragma(c, &[])
                        && !codes.is_empty()
                    {
                        per_line.push((ip.line + 1, codes));
                    }
                    if let Some(codes) = parse_ignore_pragma(c, &["stage"])
                        && !codes.is_empty()
                    {
                        insert_stage(&mut per_stage, ip.line + 1, codes);
                    }
                    if let Some(codes) = parse_ignore_pragma(c, &["global"]) {
                        global.extend(codes);
                    }
                }
                Instruction::From(_) if !per_stage.iter().any(|(l, _)| *l == ip.line) => {
                    per_stage.push((ip.line, Vec::new()));
                }
                _ => {}
            }
        }
        Self {
            per_line,
            per_stage,
            global,
        }
    }

    fn ignores(&self, line: usize, code: &str) -> bool {
        if self.global.iter().any(|c| c == code) {
            return true;
        }
        if self
            .per_line
            .iter()
            .any(|(l, codes)| *l == line && codes.iter().any(|c| c == code))
        {
            return true;
        }
        // The stage entry with the largest key below the line (0 if none).
        let key = self
            .per_stage
            .iter()
            .map(|(l, _)| *l)
            .filter(|l| *l < line)
            .max()
            .unwrap_or(0);
        self.per_stage
            .iter()
            .any(|(l, codes)| *l == key && codes.iter().any(|c| c == code))
    }
}

fn insert_stage(per_stage: &mut Vec<(usize, Vec<String>)>, line: usize, codes: Vec<String>) {
    match per_stage.iter_mut().find(|(l, _)| *l == line) {
        Some(entry) => entry.1 = codes,
        None => per_stage.push((line, codes)),
    }
}

/// `parseIgnorePragma` and friends: `hadolint [word...] ignore = RULE, RULE`
/// with an optional trailing `# comment`; `None` when the comment is not
/// such a pragma.
pub fn parse_ignore_pragma(comment: &str, words: &[&str]) -> Option<Vec<String>> {
    let mut rest = comment.trim_start_matches([' ', '\t']);
    rest = rest.strip_prefix("hadolint")?;
    rest = strip_spaces1(rest)?;
    for w in words {
        rest = rest.strip_prefix(w)?;
        rest = strip_spaces1(rest)?;
    }
    rest = rest.strip_prefix("ignore")?;
    rest = rest.trim_start_matches([' ', '\t']);
    rest = rest.strip_prefix('=')?;
    rest = rest.trim_start_matches([' ', '\t']);
    let mut codes = Vec::new();
    loop {
        let name: String = rest
            .chars()
            .take_while(|c| "DLSC0123456789".contains(*c))
            .collect();
        if name.is_empty() {
            return None;
        }
        rest = &rest[name.len()..];
        codes.push(name);
        // inlineComment: spaces, optional "# ..." (which then ends the list)
        let after = rest.trim_start_matches([' ', '\t']);
        if let Some(c) = after.strip_prefix('#') {
            rest = &c[c.find('\n').unwrap_or(c.len())..];
            break;
        }
        rest = after;
        if let Some(r) = rest.strip_prefix(',') {
            rest = r.trim_start_matches([' ', '\t']);
            continue;
        }
        break;
    }
    if rest.is_empty() { Some(codes) } else { None }
}

fn strip_spaces1(s: &str) -> Option<&str> {
    let t = s.trim_start_matches([' ', '\t']);
    if t.len() == s.len() { None } else { Some(t) }
}

#[cfg(test)]
mod tests;

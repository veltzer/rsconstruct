//! `Perl::Critic::Document` and `Perl::Critic::Annotation`: the parsed
//! file with the class index policies search, its `## no critic`
//! annotations, and the facts some policies ask of it.

use std::collections::{HashMap, HashSet};

use super::ppi::version::Version;
use super::ppi::{Class, DOCUMENT, Doc, Id, is_space};
use super::utils::shebang_line;

/// Which lines have policies disabled: by full policy name, or all.
#[derive(Default, Debug)]
struct Disabled {
    all: bool,
    policies: HashSet<String>,
}

pub struct CriticDoc {
    pub doc: Doc,
    pub filename: Vec<u8>,
    /// `_elements_of`: every element under the document, by each class it
    /// isa, in `find` order.
    elements_of: HashMap<Class, Vec<Id>>,
    disabled: HashMap<usize, Disabled>,
    is_module: bool,
    /// `_highest_explicit_perl_version`, computed once.
    highest_version: std::sync::OnceLock<Result<Option<Version>, String>>,
}

impl CriticDoc {
    pub fn new(doc: Doc, filename: &[u8], program_extensions: &[Vec<u8>]) -> Self {
        let mut elements_of: HashMap<Class, Vec<Id>> = HashMap::new();
        for id in doc.find(DOCUMENT, |_, _| Some(true)) {
            let mut class = Some(doc.class(id));
            while let Some(c) = class {
                elements_of.entry(c).or_default().push(id);
                class = c.superclass();
            }
        }
        let mut critic = Self {
            doc,
            filename: filename.to_vec(),
            elements_of,
            disabled: HashMap::new(),
            is_module: true,
            highest_version: std::sync::OnceLock::new(),
        };
        critic.disable_shebang_fix();
        critic.is_module = critic.determine_is_module(program_extensions);
        critic
    }

    /// `find($class)`.
    pub fn find(&self, class: Class) -> &[Id] {
        self.elements_of.get(&class).map_or(&[], Vec::as_slice)
    }

    pub const fn is_module(&self) -> bool {
        self.is_module
    }

    /// `_disable_shebang_fix`: a `perl -x` "eval 'exec ...'" preamble.
    fn disable_shebang_fix(&mut self) {
        let Some(first) = self.doc.schild(DOCUMENT, 0) else {
            return;
        };
        let content = self.doc.content(first);
        if !is_fixin_preamble(&content) {
            return;
        }
        let Some(location) = self.doc.location(first) else {
            return;
        };
        for line in [location.line, location.line + 1] {
            self.disabled.entry(line).or_default().all = true;
        }
    }

    fn determine_is_module(&self, program_extensions: &[Vec<u8>]) -> bool {
        if program_extensions
            .iter()
            .any(|ext| self.filename.ends_with(ext))
        {
            return false;
        }
        if shebang_line(&self.doc).is_some() {
            return false;
        }
        !self.filename.ends_with(b".PL")
    }

    /// `highest_explicit_perl_version`: `Err` where version.pm dies.
    pub fn highest_explicit_perl_version(&self) -> Result<Option<Version>, String> {
        self.highest_version
            .get_or_init(|| self.compute_highest_explicit_perl_version())
            .clone()
    }

    fn compute_highest_explicit_perl_version(&self) -> Result<Option<Version>, String> {
        let mut highest: Option<Version> = None;
        for include in self
            .doc
            .find(DOCUMENT, |d, e| Some(d.isa(e, Class::StatementInclude)))
        {
            let version = self.doc.include_version(include);
            if version.is_empty() || version == b"0" {
                continue;
            }
            let parsed = Version::parse_checked(&version).map_err(str::to_string)?;
            if highest.as_ref().is_none_or(|h| parsed > *h) {
                highest = Some(parsed);
            }
        }
        Ok(highest)
    }

    /// `process_annotations`: the `## no critic` comments, each disabling
    /// the enabled policies it names (or all) over its range of lines.
    pub fn process_annotations(&mut self, policy_names: &[String]) {
        let comments: Vec<Id> = self.find(Class::Comment).to_vec();
        for comment in comments {
            let text = self.doc.text(comment).to_vec();
            if !is_annotation(&text) {
                continue;
            }
            let disabled = parse_annotation(&text, policy_names);
            let (start, end) = self.effective_range(comment);
            for line in start..=end {
                let entry = self.disabled.entry(line).or_default();
                match &disabled {
                    None => entry.all = true,
                    Some(names) => entry.policies.extend(names.iter().cloned()),
                }
            }
        }
    }

    /// `line_is_disabled_for_policy`.
    pub fn line_is_disabled(&self, line: usize, policy: &str) -> bool {
        self.disabled
            .get(&line)
            .is_some_and(|d| d.all || d.policies.contains(policy))
    }

    fn logical_line(&self, id: Id) -> usize {
        self.doc.location(id).map_or(0, |l| l.logical_line)
    }

    /// `Annotation::_init`'s effective range.
    fn effective_range(&self, comment: Id) -> (usize, usize) {
        let doc = &self.doc;
        let line = self.logical_line(comment);
        if doc.text(comment).starts_with(b"#!") {
            return (line, line);
        }
        if self.is_single_line_annotation_on_simple_statement(comment) {
            return (line, line);
        }
        if let Some(parent) = doc.parent(comment)
            && doc.class(parent) == Class::StructureBlock
            && let Some(grandparent) = doc.parent(parent)
            && matches!(
                doc.class(grandparent),
                Class::StatementCompound | Class::StatementSub
            )
            && self.logical_line(parent) == line
        {
            let gline = self.logical_line(grandparent);
            return (gline, gline);
        }
        let mut end = comment;
        while let Some(sib) = doc.next_sibling(end) {
            end = sib;
            if doc.isa(sib, Class::Comment) && is_use_critic(&doc.content(sib)) {
                break;
            }
        }
        if doc.isa(end, Class::StatementEnd)
            && let Some(&kid) = doc.children(end).first()
        {
            end = kid;
            while let Some(sib) = doc.next_sibling(end) {
                end = sib;
                if doc.isa(sib, Class::Comment) && is_use_critic(&doc.content(sib)) {
                    break;
                }
            }
        }
        (line, self.logical_line(end))
    }

    fn is_single_line_annotation_on_simple_statement(&self, comment: Id) -> bool {
        let doc = &self.doc;
        let line = self.logical_line(comment);
        let Some(sib) = doc.sprevious_sibling(comment) else {
            return false;
        };
        if self.logical_line(sib) == line {
            return true;
        }
        if doc.class(sib).is_node() && self.logical_line(sib) < line {
            let mut neighbor = sib;
            while doc.class(neighbor).is_node() {
                match doc.schild(neighbor, -1) {
                    Some(kid) => neighbor = kid,
                    None => break,
                }
            }
            if self.logical_line(neighbor) == line {
                return true;
            }
        }
        false
    }
}

/// `\A (?: [#]! .*? )? \s* [#][#] \s* no \s+ critic` (`/xms`).
fn is_annotation(text: &[u8]) -> bool {
    let after_ws = |s: &[u8]| -> bool {
        let s = &s[s.iter().take_while(|&&c| is_space(c)).count()..];
        let Some(s) = s.strip_prefix(b"##") else {
            return false;
        };
        let s = &s[s.iter().take_while(|&&c| is_space(c)).count()..];
        let Some(s) = s.strip_prefix(b"no") else {
            return false;
        };
        let n = s.iter().take_while(|&&c| is_space(c)).count();
        n > 0 && s[n..].starts_with(b"critic")
    };
    if after_ws(text) {
        return true;
    }
    // `#!` then any text (lazily), then the annotation.
    if text.starts_with(b"#!") {
        return (2..=text.len()).any(|i| after_ws(&text[i..]));
    }
    false
}

/// `\A \s* [#][#] \s* use \s+ critic`.
fn is_use_critic(text: &[u8]) -> bool {
    let s = &text[text.iter().take_while(|&&c| is_space(c)).count()..];
    let Some(s) = s.strip_prefix(b"##") else {
        return false;
    };
    let s = &s[s.iter().take_while(|&&c| is_space(c)).count()..];
    let Some(s) = s.strip_prefix(b"use") else {
        return false;
    };
    let n = s.iter().take_while(|&&c| is_space(c)).count();
    n > 0 && s[n..].starts_with(b"critic")
}

/// `_parse_annotation`: the enabled policies an annotation names (`None`:
/// all policies). Its patterns are matched against every installed
/// policy; only the enabled ones matter here, and a pattern that matches
/// no installed policy names nothing that can be enabled.
fn parse_annotation(text: &[u8], policy_names: &[String]) -> Option<Vec<String>> {
    // `\#\# \s* no \s+ critic \s* (?:qw)? [(["'] ([\s\w:,]+)`, anywhere.
    let mut found = None;
    for start in 0..text.len() {
        let s = &text[start..];
        let Some(s) = s.strip_prefix(b"##") else {
            continue;
        };
        let s = &s[s.iter().take_while(|&&c| is_space(c)).count()..];
        let Some(s) = s.strip_prefix(b"no") else {
            continue;
        };
        let n = s.iter().take_while(|&&c| is_space(c)).count();
        if n == 0 {
            continue;
        }
        let Some(s) = s[n..].strip_prefix(b"critic") else {
            continue;
        };
        let s = &s[s.iter().take_while(|&&c| is_space(c)).count()..];
        let s = s
            .strip_prefix(b"qw")
            .filter(|r| matches!(r.first(), Some(b'(' | b'[' | b'"' | b'\'')))
            .unwrap_or(s);
        let Some((&open, rest)) = s.split_first() else {
            continue;
        };
        if !matches!(open, b'(' | b'[' | b'"' | b'\'') {
            continue;
        }
        let len = rest
            .iter()
            .take_while(|&&c| is_space(c) || super::ppi::is_word(c) || c == b':' || c == b',')
            .count();
        if len == 0 {
            continue;
        }
        found = Some(rest[..len].to_vec());
        break;
    }
    let patterns_string = found?;
    let patterns: Vec<Vec<u8>> = patterns_string
        .split(|&c| is_space(c) || c == b',')
        .filter(|p| !p.is_empty())
        .map(<[u8]>::to_ascii_lowercase)
        .collect();
    if patterns.is_empty() {
        return None;
    }
    Some(
        policy_names
            .iter()
            .filter(|name| {
                let lower = name.to_ascii_lowercase().into_bytes();
                patterns
                    .iter()
                    .any(|p| lower.windows(p.len()).any(|w| w == p.as_slice()))
            })
            .cloned()
            .collect(),
    )
}

/// `^eval 'exec .* \$0 \$[{]1[+]"\$@"}'\s*[\r\n]\s*if.+;` (`/ms`): at
/// the start of any line of the statement.
fn is_fixin_preamble(content: &[u8]) -> bool {
    let starts = std::iter::once(0).chain(
        content
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c == b'\n')
            .map(|(i, _)| i + 1),
    );
    for start in starts {
        let s = &content[start..];
        let Some(rest) = s.strip_prefix(b"eval 'exec ") else {
            continue;
        };
        let needle = b" $0 ${1+\"$@\"}'";
        let mut from = 0;
        while let Some(pos) = find(&rest[from..], needle) {
            let after = &rest[from + pos + needle.len()..];
            let ws = after.iter().take_while(|&&c| is_space(c)).count();
            // `\s*[\r\n]`: some whitespace including a line break, then
            // `\s*if.+;`.
            if (0..=ws).rev().any(|k| {
                k > 0 && matches!(after[k - 1], b'\r' | b'\n') && {
                    let tail = &after[k..];
                    let tail = &tail[tail.iter().take_while(|&&c| is_space(c)).count()..];
                    tail.starts_with(b"if") && tail.len() > 3 && tail[3..].contains(&b';')
                }
            }) {
                return true;
            }
            from += pos + 1;
        }
    }
    false
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

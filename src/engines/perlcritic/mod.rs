//! `Perl::Critic` 1.156 on PPI 1.284, in Rust.
//!
//! `ppi` ports the parser `Perl::Critic` works on (its tokenizer, lexer and
//! the element methods the policies call); `document` the critic's view of
//! a parsed file (the class index, `## no critic` annotations); `profile`
//! the `.perlcriticrc`; `policies` the policies of severity 5, the core
//! set a default profile enables. `check_file` returns what `perlcritic`
//! prints for one file: its violations in the profile's format, or the
//! reason it could not critique the file.
//!
//! Not here: policies of lower severities (a profile enabling one is an
//! error naming it), themes, `%d` diagnostics in the format, and the
//! statistics and colour options.

pub mod document;
pub mod policies;
pub mod ppi;
pub mod profile;
mod site;
pub mod utils;

#[cfg(test)]
mod tests;

pub use profile::Profile;

use document::CriticDoc;
use policies::{Found, Policy, elements, prepare_to_scan_document, violates};
use ppi::{Doc, Location};

/// What `perlcritic` makes of one file.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The rendered violations (empty: "source OK"), as the bytes
    /// perlcritic prints (source text is quoted as it is in the file).
    Violations(Vec<Vec<u8>>),
    /// PPI could not parse the file: `Problem while critiquing ...`.
    ParseError(String),
    /// A policy died on the file: `Fatal error while critiquing ...`.
    Fatal(String),
}

/// A violation, placed.
struct Violation {
    policy: Policy,
    severity: u8,
    description: Vec<u8>,
    location: Location,
    element: usize,
}

/// Critique one file.
pub fn check_file(name: &str, source: &[u8], profile: &Profile) -> Outcome {
    let doc = match ppi::parse(source, name.as_bytes()) {
        Ok(doc) => doc,
        Err(e) => {
            return Outcome::ParseError(format!(
                "Problem while critiquing \"{name}\": Can't parse code: {e}"
            ));
        }
    };
    let mut critic = CriticDoc::new(doc, name.as_bytes(), &profile.program_extensions);
    if !profile.force {
        let names: Vec<String> = profile
            .policies
            .iter()
            .map(|p| p.policy.long_name())
            .collect();
        critic.process_annotations(&names);
    }
    let mut violations = Vec::new();
    for config in &profile.policies {
        match critique_policy(config, &critic) {
            Ok(found) => violations.extend(found),
            Err(e) => {
                return Outcome::Fatal(format!("Fatal error while critiquing \"{name}\": {e}"));
            }
        }
    }
    if let Some(top) = profile.top
        && !violations.is_empty()
    {
        let limit = violations.len().min(top);
        violations.sort_by_key(|v| v.severity);
        violations.reverse();
        violations.truncate(limit);
    }
    violations.sort_by_key(|v| (v.location.line, v.location.character));
    let rendered = violations
        .iter()
        .map(|v| render(&profile.format, v, &critic.doc, name))
        .collect();
    Outcome::Violations(rendered)
}

/// `Perl::Critic::_critique` for one policy.
fn critique_policy(
    config: &profile::PolicyConfig,
    critic: &CriticDoc,
) -> Result<Vec<Violation>, String> {
    let policy = config.policy;
    if !prepare_to_scan_document(policy, critic)? {
        return Ok(Vec::new());
    }
    if config.maximum_violations == Some(0) {
        return Ok(Vec::new());
    }
    let long_name = policy.long_name();
    let mut out = Vec::new();
    for element in elements(policy, critic) {
        for Found {
            description,
            element,
        } in violates(policy, &config.settings, critic, element)?
        {
            let location = critic.doc.location(element).unwrap_or(Location {
                line: 0,
                character: 0,
                column: 0,
                logical_line: 0,
                logical_file: 0,
            });
            if critic.line_is_disabled(location.line, &long_name) {
                continue;
            }
            out.push(Violation {
                policy,
                severity: config.severity,
                description: chomp_periods(&description),
                location,
                element,
            });
            if config
                .maximum_violations
                .is_some_and(|max| out.len() >= max)
            {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

/// `_chomp_periods`: `s{ [.]+ \z }{}xms`.
fn chomp_periods(s: &[u8]) -> Vec<u8> {
    let n = s.iter().rev().take_while(|&&c| c == b'.').count();
    s[..s.len() - n].to_vec()
}

fn basename(path: &[u8]) -> Vec<u8> {
    let trimmed = {
        let n = path.iter().rev().take_while(|&&c| c == b'/').count();
        if n == path.len() {
            path
        } else {
            &path[..path.len() - n]
        }
    };
    match trimmed.iter().rposition(|&c| c == b'/') {
        Some(slash) => trimmed[slash + 1..].to_vec(),
        None => trimmed.to_vec(),
    }
}

/// `_line_containing_violation` (`%r`).
fn source_line(doc: &Doc, element: usize) -> Vec<u8> {
    let statement = doc.statement(element).unwrap_or(element);
    let code = doc.content(statement);
    let lines: Vec<&[u8]> = code.split(|&c| c == b'\n').collect();
    let line = |e: usize| doc.location(e).map_or(0, |l| l.line) as isize;
    let index = line(element) - line(statement);
    if index < 0 || index as usize >= lines.len() {
        return Vec::new();
    }
    lines[index as usize].to_vec()
}

/// `Violation::to_string`: `String::Format`'s `stringf` over the format.
fn render(format: &str, v: &Violation, doc: &Doc, filename: &str) -> Vec<u8> {
    let logical_file = || doc.files[v.location.logical_file].clone();
    let value = |spec: char| -> Option<Vec<u8>> {
        Some(match spec {
            'f' => logical_file(),
            'F' => basename(&logical_file()),
            'g' => filename.as_bytes().to_vec(),
            'G' => basename(filename.as_bytes()),
            'l' => v.location.logical_line.to_string().into_bytes(),
            'L' => v.location.line.to_string().into_bytes(),
            'c' => v.location.column.to_string().into_bytes(),
            'C' => doc.class(v.element).name().as_bytes().to_vec(),
            'm' => v.description.clone(),
            'e' => {
                let rendered = v.policy.explanation().render();
                chomp_periods(rendered.as_bytes())
            }
            's' => v.severity.to_string().into_bytes(),
            'r' => source_line(doc, v.element),
            'P' => v.policy.long_name().into_bytes(),
            'p' => v.policy.short_name().as_bytes().to_vec(),
            'n' => b"\n".to_vec(),
            't' => b"\t".to_vec(),
            '%' => b"%".to_vec(),
            _ => return None,
        })
    };
    let chars: Vec<char> = format.chars().collect();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '%' {
            let mut buf = [0; 4];
            out.extend_from_slice(chars[i].encode_utf8(&mut buf).as_bytes());
            i += 1;
            continue;
        }
        // %(-)?(\d*)?(?:\.(\d*))?(\{.*?\})?(\S)
        let start = i;
        let mut j = i + 1;
        let left = chars.get(j) == Some(&'-');
        if left {
            j += 1;
        }
        let min_start = j;
        while chars.get(j).is_some_and(char::is_ascii_digit) {
            j += 1;
        }
        let min: String = chars[min_start..j].iter().collect();
        let mut max = String::new();
        if chars.get(j) == Some(&'.') {
            let k0 = j + 1;
            let mut k = k0;
            while chars.get(k).is_some_and(char::is_ascii_digit) {
                k += 1;
            }
            max = chars[k0..k].iter().collect();
            j = k;
        }
        if chars.get(j) == Some(&'{')
            && let Some(close) = chars[j..].iter().position(|&c| c == '}')
        {
            j += close + 1;
        }
        let Some(&spec) = chars.get(j).filter(|c| !c.is_whitespace()) else {
            out.push(b'%');
            i = start + 1;
            continue;
        };
        match value(spec) {
            None => {
                let orig: String = chars[start..=j].iter().collect();
                out.extend_from_slice(orig.as_bytes());
            }
            Some(replacement) => {
                let len = replacement.len();
                let min_width = min.parse::<usize>().ok().filter(|&n| n > 0).unwrap_or(len);
                let max_width = max.parse::<usize>().ok().filter(|&n| n > 0).unwrap_or(len);
                if len > min_width && len < max_width {
                    out.extend_from_slice(&replacement);
                } else if len > max_width {
                    out.extend_from_slice(&replacement[..max_width]);
                } else if left {
                    // `" " x $n` is empty for a negative count.
                    out.extend_from_slice(&replacement);
                    out.extend(std::iter::repeat_n(b' ', min_width.saturating_sub(len)));
                } else {
                    out.extend(std::iter::repeat_n(b' ', min_width.saturating_sub(len)));
                    out.extend_from_slice(&replacement);
                }
            }
        }
        i = j + 1;
    }
    out
}

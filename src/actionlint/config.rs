//! actionlint's configuration file, `.github/actionlint.yaml`: labels of
//! self-hosted runners, the allowed configuration variables, and per-path
//! ignore patterns. A port of `config.go`.

use std::collections::BTreeMap;

use regex::Regex;
use serde::Deserialize;

use crate::actionlint::LintError;

#[derive(Debug, Clone)]
pub struct PathConfig {
    /// A doublestar glob matched against the workflow path.
    pub pattern: String,
    /// Error messages matching one of these are dropped.
    pub ignore: Vec<Regex>,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub self_hosted_runner_labels: Vec<String>,
    /// The `config-variables` list. `None` disables the check (the file says
    /// `null` or nothing); an empty list allows no variable.
    pub variables: Option<Vec<String>>,
    pub paths: Vec<PathConfig>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawRunner {
    #[serde(default)]
    labels: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
struct RawPath {
    #[serde(default)]
    ignore: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
struct Raw {
    #[serde(rename = "self-hosted-runner", default)]
    self_hosted_runner: Option<RawRunner>,
    #[serde(rename = "config-variables", default)]
    config_variables: Option<Vec<String>>,
    #[serde(default)]
    paths: Option<BTreeMap<String, RawPath>>,
}

impl Config {
    /// Parses the file's text. The error text is for a build failure.
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: Raw =
            serde_yaml_ng::from_str(text).map_err(|e| e.to_string().replace('\n', " "))?;
        let mut paths = Vec::new();
        for (pattern, cfg) in raw.paths.unwrap_or_default() {
            let mut ignore = Vec::new();
            for p in cfg.ignore.unwrap_or_default() {
                let re = Regex::new(&p)
                    .map_err(|e| format!("invalid regular expression {p:?} in \"ignore\": {e}"))?;
                ignore.push(re);
            }
            paths.push(PathConfig { pattern, ignore });
        }
        Ok(Self {
            self_hosted_runner_labels: raw
                .self_hosted_runner
                .and_then(|r| r.labels)
                .unwrap_or_default(),
            variables: raw.config_variables,
            paths,
        })
    }

    /// The path configurations whose pattern matches the workflow path.
    pub fn path_configs(&self, path: &str) -> Vec<&PathConfig> {
        self.paths
            .iter()
            .filter(|p| doublestar_match(&p.pattern, path))
            .collect()
    }

    /// Drops the errors a matching path configuration ignores.
    pub fn filter_errors(&self, path: &str, errors: Vec<LintError>) -> Vec<LintError> {
        let cfgs = self.path_configs(path);
        if cfgs.is_empty() {
            return errors;
        }
        errors
            .into_iter()
            .filter(|e| {
                !cfgs
                    .iter()
                    .any(|c| c.ignore.iter().any(|re| re.is_match(&e.message)))
            })
            .collect()
    }
}

/// A `doublestar`-style match: `**` spans directories, `*` and `?` stay
/// within one, `[...]` is a character class, `{a,b}` alternatives.
pub fn doublestar_match(pattern: &str, path: &str) -> bool {
    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    let path_segments: Vec<&str> = path.split('/').collect();
    match_segments(&pattern_segments, &path_segments)
}

fn match_segments(pattern_segments: &[&str], path_segments: &[&str]) -> bool {
    match pattern_segments.split_first() {
        None => path_segments.is_empty(),
        Some((&"**", rest)) => {
            (0..=path_segments.len()).any(|skip| match_segments(rest, &path_segments[skip..]))
        }
        Some((pat, rest)) => match path_segments.split_first() {
            Some((segment, remaining)) => {
                match_segment(pat, segment) && match_segments(rest, remaining)
            }
            None => false,
        },
    }
}

fn match_segment(pat: &str, s: &str) -> bool {
    // Expand `{a,b}` alternatives first.
    if let Some(open) = pat.find('{')
        && let Some(close) = pat[open..].find('}')
    {
        let close = open + close;
        let (head, tail) = (&pat[..open], &pat[close + 1..]);
        return pat[open + 1..close]
            .split(',')
            .any(|alt| match_segment(&format!("{head}{alt}{tail}"), s));
    }
    glob_match(
        &pat.chars().collect::<Vec<_>>(),
        &s.chars().collect::<Vec<_>>(),
    )
}

/// Matches one path segment: `*`, `?`, `[...]` (with `!`/`^` negation and
/// ranges) and `\` escapes.
pub fn glob_match(pat: &[char], s: &[char]) -> bool {
    match pat.first() {
        None => s.is_empty(),
        Some('*') => (0..=s.len()).any(|i| glob_match(&pat[1..], &s[i..])),
        Some('?') => !s.is_empty() && glob_match(&pat[1..], &s[1..]),
        Some('[') => {
            let Some(end) = pat.iter().skip(1).position(|c| *c == ']').map(|i| i + 1) else {
                return false;
            };
            let Some(&c) = s.first() else {
                return false;
            };
            let class = &pat[1..end];
            let (negate, class) = match class.first() {
                Some('!' | '^') => (true, &class[1..]),
                _ => (false, class),
            };
            let mut matched = false;
            let mut i = 0;
            while i < class.len() {
                if i + 2 < class.len() && class[i + 1] == '-' {
                    if class[i] <= c && c <= class[i + 2] {
                        matched = true;
                    }
                    i += 3;
                } else {
                    if class[i] == c {
                        matched = true;
                    }
                    i += 1;
                }
            }
            matched != negate && glob_match(&pat[end + 1..], &s[1..])
        }
        Some('\\') if pat.len() > 1 => s.first() == Some(&pat[1]) && glob_match(&pat[2..], &s[1..]),
        Some(&p) => s.first() == Some(&p) && glob_match(&pat[1..], &s[1..]),
    }
}

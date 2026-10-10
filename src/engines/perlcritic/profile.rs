//! The profile (`.perlcriticrc`): `Config::Tiny`'s format, the global options
//! of `Perl::Critic::OptionsProcessor`, and which policies `Config`'s
//! `_load_policies` enables, with their parameters.
//!
//! A profile that enables a policy this port does not implement is an
//! error naming it, as are options that change what is reported in ways
//! not ported (`theme`, `single-policy`).

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail};
use regex::bytes::Regex;

use super::policies::{Policy, Settings};
use super::site::SITE_POLICIES;

/// An enabled policy and its configuration.
#[derive(Debug)]
pub struct PolicyConfig {
    pub policy: Policy,
    pub severity: u8,
    pub maximum_violations: Option<usize>,
    pub settings: Settings,
}

/// What a run uses from the profile.
#[derive(Debug)]
pub struct Profile {
    /// The output format (after `interpolate`).
    pub format: String,
    pub force: bool,
    pub top: Option<usize>,
    pub program_extensions: Vec<Vec<u8>>,
    /// The enabled policies, in `Perl::Critic`'s order.
    pub policies: Vec<PolicyConfig>,
}

const GLOBAL_OPTIONS: &[&str] = &[
    "allow-unsafe",
    "color",
    "color-severity-1",
    "color-severity-2",
    "color-severity-3",
    "color-severity-4",
    "color-severity-5",
    "color-severity-high",
    "color-severity-highest",
    "color-severity-low",
    "color-severity-lowest",
    "color-severity-medium",
    "colour",
    "colour-severity-1",
    "colour-severity-2",
    "colour-severity-3",
    "colour-severity-4",
    "colour-severity-5",
    "colour-severity-high",
    "colour-severity-highest",
    "colour-severity-low",
    "colour-severity-lowest",
    "colour-severity-medium",
    "criticism-fatal",
    "exclude",
    "force",
    "include",
    "only",
    "pager",
    "profile-strictness",
    "program-extensions",
    "severity",
    "single-policy",
    "theme",
    "top",
    "verbose",
];

/// `%FORMAT_OF`.
const fn numeric_format(verbosity: u32) -> Option<&'static str> {
    Some(match verbosity {
        1 => "%f:%l:%c:%m\n",
        2 => "%f: (%l:%c) %m\n",
        3 => "%m at %f line %l\n",
        4 => "%m at line %l, column %c.  %e.  (Severity: %s)\n",
        5 => "%f: %m at line %l, column %c.  %e.  (Severity: %s)\n",
        6 => "%m at line %l, near '%r'.  (Severity: %s)\n",
        7 => "%f: %m at line %l near '%r'.  (Severity: %s)\n",
        8 => "[%p] %m at line %l, column %c.  (Severity: %s)\n",
        9 => "[%p] %m at line %l, near '%r'.  (Severity: %s)\n",
        10 => "%m at line %l, column %c.\n  %p (Severity: %s)\n%d\n",
        11 => "%m at line %l, near '%r'.\n  %p (Severity: %s)\n%d\n",
        _ => return None,
    })
}

/// A parsed `Config::Tiny` file: sections of key/value pairs, the root one
/// named `_`.
type Ini = Vec<(String, Vec<(String, String)>)>;

/// `Config::Tiny::read_string`.
fn parse_ini(text: &str) -> Result<Ini> {
    let mut ini: Ini = vec![("_".to_string(), Vec::new())];
    let mut current = 0;
    for (i, raw) in text.split(['\n', '\r']).enumerate() {
        let trimmed = raw.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        // s/\s\;\s.+$//g
        let line = match find_inline_comment(raw) {
            Some(at) => &raw[..at],
            None => raw,
        };
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') && t.len() > 2 {
            let name = t[1..t.len() - 1].trim().to_string();
            current = if let Some(p) = ini.iter().position(|(n, _)| *n == name) {
                p
            } else {
                ini.push((name, Vec::new()));
                ini.len() - 1
            };
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim();
            if !key.is_empty() {
                let value = line[eq + 1..].trim().to_string();
                let section = &mut ini[current].1;
                match section.iter_mut().find(|(k, _)| k == key) {
                    Some(entry) => entry.1 = value,
                    None => section.push((key.to_string(), value)),
                }
                continue;
            }
        }
        bail!("Syntax error at line {}: '{}'", line_number(text, i), raw);
    }
    Ok(ini)
}

/// `Config::Tiny` splits on CR LF, CR CR LF, CR or LF; `split` here splits
/// on each CR and LF, so the line number counts only real line breaks.
fn line_number(text: &str, index: usize) -> usize {
    let mut n = 0;
    let mut pieces = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && pieces < index {
        if bytes[i] == b'\r' || bytes[i] == b'\n' {
            pieces += 1;
            if !(bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n')) {
                n += 1;
            }
        }
        i += 1;
    }
    n + 1
}

/// The start of an inline comment: whitespace, `;`, whitespace, something.
fn find_inline_comment(line: &str) -> Option<usize> {
    let b = line.as_bytes();
    (0..b.len()).find(|&i| {
        b[i].is_ascii_whitespace()
            && b.get(i + 1) == Some(&b';')
            && b.get(i + 2).is_some_and(u8::is_ascii_whitespace)
            && b.len() > i + 3
    })
}

/// Perl truthiness of a string.
fn truthy(s: &str) -> bool {
    !s.is_empty() && s != "0"
}

/// `severity_to_number`.
fn severity_to_number(s: &str) -> Result<u8> {
    if let Some(n) = parse_integer(s) {
        if n == 0 {
            return Ok(5);
        }
        return Ok(n.clamp(1, 5) as u8);
    }
    Ok(match s.to_ascii_lowercase().as_str() {
        "gentle" => 5,
        "stern" => 4,
        "harsh" => 3,
        "cruel" => 2,
        "brutal" => 1,
        _ => bail!("Invalid severity: \"{s}\""),
    })
}

/// `is_integer`: `\A [+-]? \d+ \z`.
fn parse_integer(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse::<i64>().ok().or_else(|| {
        Some(if s.starts_with('-') {
            i64::MIN
        } else {
            i64::MAX
        })
    })
}

/// `interpolate`: Perl's double-quoted string escapes.
fn interpolate(literal: &str) -> Result<String> {
    let mut out = String::new();
    let mut chars = literal.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('f') => out.push('\x0c'),
                Some('e') => out.push('\x1b'),
                Some('a') => out.push('\x07'),
                Some('0') => out.push('\0'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            '$' | '@'
                if chars.peek().is_some_and(|n| {
                    n.is_ascii_alphanumeric() || *n == '_' || *n == '{' || *n == ':'
                }) =>
            {
                bail!(
                    "the verbose format interpolates a Perl variable, which iperlcritic does not support: {literal}"
                );
            }
            _ => out.push(c),
        }
    }
    Ok(out)
}

fn regex_ci(pattern: &str) -> Result<Regex> {
    regex::bytes::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .multi_line(true)
        .dot_matches_new_line(true)
        .ignore_whitespace(true)
        .build()
        .with_context(|| format!("invalid regular expression \"{pattern}\""))
}

fn default_list_funcs() -> String {
    let mut funcs = vec![
        "map".to_string(),
        "grep".to_string(),
        "List::Util::first".to_string(),
    ];
    for f in [
        "any",
        "all",
        "none",
        "notall",
        "true",
        "false",
        "firstidx",
        "first_index",
        "lastidx",
        "last_index",
        "insert_after",
        "insert_after_string",
    ] {
        funcs.push(format!("List::MoreUtils::{f}"));
        funcs.push(format!("List::SomeUtils::{f}"));
    }
    funcs.join(" ")
}

const STRICT_EQUIVALENT_MODULES: &[&str] = &[
    "Mo",
    "Moo",
    "Moo::Role",
    "Moose",
    "Moose::Role",
    "Moose::Exporter",
    "Moose::Util::TypeConstraints",
    "Test::Class::Moose",
    "MooseX::NonMoose",
    "MooseX::Singleton",
    "MooseX::Role::Parameterized",
    "MooseX::MethodAttributes::Role",
    "Mouse",
    "Mouse::Role",
    "Mouse::Exporter",
    "Mouse::Util",
    "Mouse::Util::TypeConstraints",
    "Moos",
    "Mousse",
    "Any::Moose",
    "Modern::Perl",
    "Dancer",
    "Dancer2",
    "Mojolicious::Lite",
    "Mojo::Base",
    "Object::Simple",
    "Role::Tiny",
    "Test::Spec",
    "sane",
    "strictures",
];

const DEFAULT_EVIL_MODULES: &str = "Class::ISA {Found use of Class::ISA. This module is deprecated by the Perl 5 Porters.} \
Pod::Plainer {Found use of Pod::Plainer. This module is deprecated by the Perl 5 Porters.} \
Shell {Found use of Shell. This module is deprecated by the Perl 5 Porters.} \
Switch {Found use of Switch. This module is deprecated by the Perl 5 Porters.}";

fn words(s: &str) -> Vec<Vec<u8>> {
    s.split_ascii_whitespace()
        .map(|w| w.as_bytes().to_vec())
        .collect()
}

/// `ProhibitEvilModules`' module specification regexes.
struct EvilSpec {
    modules: Regex,
    file_line: Regex,
}

impl EvilSpec {
    fn new() -> Self {
        let name = r"\b[[:alpha:]_](?:(?:\w|::)*\w)?\b";
        Self {
            modules: Regex::new(&format!(
                r"(?-u)\A\s*(?:({name})|/([^/]+)/)(?:\s*\{{([^}}]+)\}})?\s*"
            ))
            .expect("valid regex"),
            file_line: Regex::new(&format!(
                r"(?-u)(?s)\A\s*(?:({name})|/([^/]+)/)\s*(\S(?:.*\S)?)?\s*\z"
            ))
            .expect("valid regex"),
        }
    }
}

fn handle_evil_spec(
    settings: &mut Settings,
    module: Option<&[u8]>,
    regex: Option<&[u8]>,
    description: Option<&[u8]>,
) -> Result<()> {
    let description = description.unwrap_or_default().to_vec();
    if let Some(re) = regex.filter(|r| !r.is_empty() && *r != b"0") {
        let pattern = String::from_utf8_lossy(re).into_owned();
        let compiled = Regex::new(&pattern)
            .with_context(|| format!("contains an invalid regular expression: \"{pattern}\""))?;
        settings.evil_module_regexes.push((compiled, description));
    } else if let Some(m) = module {
        settings.evil_modules.insert(m.to_vec(), description);
    }
    Ok(())
}

fn parse_evil_modules(settings: &mut Settings, value: Option<&str>) -> Result<()> {
    let spec = EvilSpec::new();
    let mut rest = value.unwrap_or(DEFAULT_EVIL_MODULES).as_bytes().to_vec();
    if !truthy(std::str::from_utf8(&rest).unwrap_or("x"))
        || rest.iter().all(u8::is_ascii_whitespace)
    {
        return Ok(());
    }
    while let Some(caps) = spec.modules.captures(&rest) {
        let module = caps.get(1).map(|m| m.as_bytes().to_vec());
        let regex = caps.get(2).map(|m| m.as_bytes().to_vec());
        let description = caps.get(3).map(|m| m.as_bytes().to_vec());
        let end = caps.get(0).map_or(0, |m| m.end());
        handle_evil_spec(
            settings,
            module.as_deref(),
            regex.as_deref(),
            description.as_deref(),
        )
        .context("Modules::ProhibitEvilModules: modules")?;
        rest.drain(..end);
        if end == 0 {
            break;
        }
    }
    let leftover = String::from_utf8_lossy(&rest).into_owned();
    if truthy(&leftover) {
        bail!(
            "The value for the Modules::ProhibitEvilModules \"modules\" option (\"{}\") contains unparseable data: \"{leftover}\"",
            value.unwrap_or("")
        );
    }
    Ok(())
}

fn parse_evil_modules_file(settings: &mut Settings, value: &str) -> Result<()> {
    if !truthy(value) || value.trim().is_empty() {
        return Ok(());
    }
    let spec = EvilSpec::new();
    let text = std::fs::read(value)
        .with_context(|| format!("The value for the Modules::ProhibitEvilModules \"modules_file\" option (\"{value}\") refers to a file that could not be opened"))?;
    for raw in text.split_inclusive(|&c| c == b'\n') {
        let mut line = raw.to_vec();
        if let Some(hash) = line.iter().position(|&c| c == b'#') {
            line.truncate(hash);
        }
        let line = line.trim_ascii();
        if line.is_empty() || line == b"0" {
            continue;
        }
        let Some(caps) = spec.file_line.captures(line) else {
            bail!(
                "The value for the Modules::ProhibitEvilModules \"modules_file\" option (\"{value}\") contains unparseable data: \"{}\"",
                String::from_utf8_lossy(line)
            );
        };
        handle_evil_spec(
            settings,
            caps.get(1).map(|m| m.as_bytes()),
            caps.get(2).map(|m| m.as_bytes()),
            caps.get(3).map(|m| m.as_bytes()),
        )?;
    }
    Ok(())
}

/// The policy's settings from its section's parameters; `None` when the
/// policy declines to be enabled (`initialize_if_enabled`).
fn policy_settings(policy: Policy, params: &HashMap<String, String>) -> Result<Option<Settings>> {
    let mut settings = Settings::default();
    let get = |name: &str| params.get(name).map(String::as_str);
    match policy {
        Policy::ProhibitStringyEval => {
            settings.allow_includes = truthy(get("allow_includes").unwrap_or("0"));
        }
        Policy::ProhibitMutatingListFunctions => {
            let base = get("list_funcs").map_or_else(default_list_funcs, str::to_string);
            settings.list_funcs = words(&base)
                .into_iter()
                .chain(words(get("add_list_funcs").unwrap_or("")))
                .collect();
        }
        Policy::ProhibitEvilModules => {
            parse_evil_modules(&mut settings, get("modules"))?;
            parse_evil_modules_file(&mut settings, get("modules_file").unwrap_or(""))?;
            if settings.evil_modules.is_empty() && settings.evil_module_regexes.is_empty() {
                return Ok(None);
            }
        }
        Policy::ProhibitNoStrict => {
            let allow = get("allow").unwrap_or("").to_ascii_lowercase();
            settings.no_strict_allow = allow
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter(|w| !w.is_empty())
                .map(|w| w.as_bytes().to_vec())
                .collect();
        }
        Policy::RequireUseStrict => {
            let mut modules: HashSet<Vec<u8>> = words(get("equivalent_modules").unwrap_or(""))
                .into_iter()
                .collect();
            modules.insert(b"strict".to_vec());
            modules.extend(
                STRICT_EQUIVALENT_MODULES
                    .iter()
                    .map(|m| m.as_bytes().to_vec()),
            );
            settings.equivalent_modules = modules;
        }
        Policy::ProhibitLeadingZeros => {
            settings.leading_zeros_strict = truthy(get("strict").unwrap_or("0"));
        }
        _ => {}
    }
    Ok(Some(settings))
}

/// `policy_long_name` / `policy_short_name`.
fn short_name(name: &str) -> &str {
    name.strip_prefix("Perl::Critic::Policy::").unwrap_or(name)
}

/// Parse a profile and select the policies it enables.
pub fn load(text: &str) -> Result<Profile> {
    let ini = parse_ini(text)?;
    let mut globals: HashMap<String, String> = HashMap::new();
    // Per policy: named `[-Policy]`, named `[Policy]`, and its parameters.
    let mut sections: HashMap<String, (bool, bool, HashMap<String, String>)> = HashMap::new();
    for (name, entries) in ini {
        if name == "_" {
            for (key, value) in entries {
                if !GLOBAL_OPTIONS.contains(&key.as_str()) {
                    bail!("\"{key}\" is not a supported option");
                }
                globals.insert(key, value);
            }
            continue;
        }
        let (disabled, policy_name) = match name.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, name.as_str()),
        };
        let short = short_name(policy_name).to_string();
        if !SITE_POLICIES.iter().any(|(n, _)| *n == short) {
            bail!("Policy \"{short}\" is not installed.");
        }
        let entry = sections
            .entry(short)
            .or_insert_with(|| (false, false, HashMap::new()));
        if disabled {
            entry.0 = true;
        } else {
            entry.1 = true;
            entry.2.extend(entries);
        }
    }
    let get = |k: &str| globals.get(k).map(String::as_str);
    if get("theme").is_some_and(|t| !t.trim().is_empty()) {
        bail!("iperlcritic does not support the \"theme\" option");
    }
    if get("single-policy").is_some_and(|t| !t.trim().is_empty()) {
        bail!("iperlcritic does not support the \"single-policy\" option");
    }
    let min_severity = severity_to_number(get("severity").unwrap_or("5"))?;
    let only = truthy(get("only").unwrap_or("0"));
    let force = truthy(get("force").unwrap_or("0"));
    let top = match get("top") {
        None => None,
        Some(t) if !truthy(t) => None,
        Some(t) => Some(parse_integer(t).filter(|n| *n > 0).with_context(|| {
            format!(
                "The value for the global \"top\" option (\"{t}\") is not one of the valid values"
            )
        })? as usize),
    };
    let include: Vec<Regex> = get("include")
        .unwrap_or("")
        .split_ascii_whitespace()
        .map(regex_ci)
        .collect::<Result<_>>()?;
    let exclude: Vec<Regex> = get("exclude")
        .unwrap_or("")
        .split_ascii_whitespace()
        .map(regex_ci)
        .collect::<Result<_>>()?;
    let program_extensions = words(get("program-extensions").unwrap_or(""));
    let verbose = get("verbose").unwrap_or("4");
    let format = match parse_integer(verbose) {
        Some(n) => {
            // perlcritic names the file once it is given several, which
            // a build always does.
            let n = if verbose == "4" {
                5
            } else {
                n.unsigned_abs() as u32
            };
            numeric_format(n)
                .unwrap_or_else(|| numeric_format(4).expect("format 4"))
                .to_string()
        }
        None => interpolate(verbose)?,
    };
    if format.contains("%d") {
        bail!(
            "iperlcritic does not support \"%d\" (the policy's diagnostics) in the verbose format"
        );
    }

    let mut policies = Vec::new();
    for &(name, default_severity) in SITE_POLICIES {
        let long = format!("Perl::Critic::Policy::{name}");
        let section = sections.get(name);
        let params = section.map(|s| s.2.clone()).unwrap_or_default();
        let severity = match params.get("severity") {
            Some(s) => severity_to_number(s).with_context(|| format!("Policy {name}: severity"))?,
            None => default_severity,
        };
        // `_load_policies`, in its order: `only`, `[-Policy]`, `[Policy]`,
        // the severity, then `include` and `exclude`.
        let disabled = section.is_some_and(|s| s.0);
        let named = section.is_some_and(|s| s.1);
        let included = include.iter().any(|re| re.is_match(long.as_bytes()));
        let excluded = exclude.iter().any(|re| re.is_match(long.as_bytes()));
        let load_me =
            ((named || (!only && !disabled)) && severity >= min_severity || included) && !excluded;
        if !load_me {
            continue;
        }
        let Some(policy) = Policy::from_short_name(name) else {
            // ProhibitEvilVariables enables itself only once given
            // variables to prohibit.
            if name == "Variables::ProhibitEvilVariables"
                && params.get("variables").is_none_or(|v| v.trim().is_empty())
                && params
                    .get("variables_file")
                    .is_none_or(|v| v.trim().is_empty())
            {
                continue;
            }
            bail!("iperlcritic does not implement policy \"{name}\", which the profile enables");
        };
        for key in params.keys() {
            let general = matches!(
                key.as_str(),
                "severity" | "maximum_violations_per_document" | "set_themes" | "add_themes"
            );
            if !general && !policy.parameters().contains(&key.as_str()) {
                bail!("The \"{key}\" parameter is not supported by policy {name}");
            }
        }
        let maximum_violations = match params.get("maximum_violations_per_document") {
            Some(m) if !m.is_empty() && !m.eq_ignore_ascii_case("no_limit") => {
                let n = parse_integer(m)
                    .filter(|n| *n >= 0)
                    .with_context(|| format!("The value for the {name} \"maximum_violations_per_document\" option (\"{m}\") is not an integer greater than or equal to zero"))?;
                Some(n as usize)
            }
            _ => policy.default_maximum_violations(),
        };
        let Some(settings) = policy_settings(policy, &params)? else {
            continue;
        };
        policies.push(PolicyConfig {
            policy,
            severity,
            maximum_violations,
            settings,
        });
    }
    if policies.is_empty() {
        bail!("There are no enabled policies.");
    }
    Ok(Profile {
        format,
        force,
        top,
        program_extensions,
        policies,
    })
}

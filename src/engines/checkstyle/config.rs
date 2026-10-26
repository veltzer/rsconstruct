//! A checkstyle configuration (`checkstyle.xml`), read as checkstyle's
//! `ConfigurationLoader` reads it and set up as `Checker`, `TreeWalker` and
//! the modules' `AutomaticBean` setters do: `severity` and `tabWidth` pass
//! from parent to child unless a module sets its own, properties are
//! validated against what each module has, and `<message>` overrides
//! replace a key's text.
//!
//! Only the modules this port implements are accepted; any other module,
//! and any property whose effect is not reproduced, is an error naming it.

use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use regex::Regex;
use xmlparser::{ElementEnd, Token, Tokenizer};

use super::message::{Severity, validate_pattern};

/// The checks implemented here.
#[derive(Debug)]
pub enum CheckKind {
    ArrayTypeStyle {
        java_style: bool,
    },
    CovariantEquals,
    DefaultComesLast {
        skip_if_last_and_shared_with_case: bool,
    },
    EmptyStatement,
    EqualsHashCode,
    FallThrough {
        check_last_case_group: bool,
        relief_pattern: Regex,
    },
    ModifiedControlVariable {
        skip_enhanced_for_loop_variable: bool,
    },
    OneStatementPerLine {
        treat_try_resources_as_statement: bool,
    },
    RedundantImport,
    SimplifyBooleanExpression,
    SimplifyBooleanReturn,
    StringLiteralEquality,
    UnusedImports {
        process_javadoc: bool,
    },
    UpperEll,
}

/// The `TreeWalker` checks by name, with their package.
const CHECKS: &[(&str, &str)] = &[
    ("ArrayTypeStyle", "checks"),
    ("CovariantEquals", "checks.coding"),
    ("DefaultComesLast", "checks.coding"),
    ("EmptyStatement", "checks.coding"),
    ("EqualsHashCode", "checks.coding"),
    ("FallThrough", "checks.coding"),
    ("ModifiedControlVariable", "checks.coding"),
    ("OneStatementPerLine", "checks.coding"),
    ("RedundantImport", "checks.imports"),
    ("SimplifyBooleanExpression", "checks.coding"),
    ("SimplifyBooleanReturn", "checks.coding"),
    ("StringLiteralEquality", "checks.coding"),
    ("UnusedImports", "checks.imports"),
    ("UpperEll", "checks"),
];

impl CheckKind {
    /// The class name without `Check`, as messages are tagged.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::ArrayTypeStyle { .. } => "ArrayTypeStyle",
            Self::CovariantEquals => "CovariantEquals",
            Self::DefaultComesLast { .. } => "DefaultComesLast",
            Self::EmptyStatement => "EmptyStatement",
            Self::EqualsHashCode => "EqualsHashCode",
            Self::FallThrough { .. } => "FallThrough",
            Self::ModifiedControlVariable { .. } => "ModifiedControlVariable",
            Self::OneStatementPerLine { .. } => "OneStatementPerLine",
            Self::RedundantImport => "RedundantImport",
            Self::SimplifyBooleanExpression => "SimplifyBooleanExpression",
            Self::SimplifyBooleanReturn => "SimplifyBooleanReturn",
            Self::StringLiteralEquality => "StringLiteralEquality",
            Self::UnusedImports { .. } => "UnusedImports",
            Self::UpperEll => "UpperEll",
        }
    }
}

/// What every module has: `severity`, `id`, `tabWidth`, `<message>`s.
#[derive(Debug, Clone)]
pub struct Common {
    pub severity: Severity,
    pub id: Option<String>,
    pub tab_width: i32,
    pub messages: HashMap<String, String>,
}

#[derive(Debug)]
pub struct CheckModule {
    pub common: Common,
    pub kind: CheckKind,
}

/// `NewlineAtEndOfFile`'s `lineSeparator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineSeparator {
    Crlf,
    Cr,
    Lf,
    LfCrCrlf,
}

#[derive(Debug)]
pub enum FileSetModule {
    TreeWalker {
        file_extensions: Vec<String>,
        checks: Vec<CheckModule>,
    },
    NewlineAtEndOfFile {
        common: Common,
        file_extensions: Vec<String>,
        line_separator: LineSeparator,
    },
}

/// The `Checker` and what it runs.
#[derive(Debug)]
pub struct Config {
    pub file_extensions: Vec<String>,
    pub modules: Vec<FileSetModule>,
}

/// An element of the configuration file.
#[derive(Debug, Default)]
struct Element {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Self>,
}

impl Element {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Attribute value normalisation (line ends and tabs become spaces) and
/// the predefined and character references.
fn attribute_value(raw: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(i) = rest.find(['&', '\t', '\n', '\r']) {
        out.push_str(&rest[..i]);
        let c = rest.as_bytes()[i];
        if c != b'&' {
            out.push(' ');
            rest = &rest[i + 1..];
            continue;
        }
        let end = rest[i..]
            .find(';')
            .ok_or_else(|| anyhow!("unterminated reference in {raw:?}"))?;
        let name = &rest[i + 1..i + end];
        let expanded = match name {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let code = if let Some(hex) = name.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(dec) = name.strip_prefix('#') {
                    dec.parse().ok()
                } else {
                    None
                };
                code.and_then(char::from_u32)
                    .ok_or_else(|| anyhow!("unknown reference &{name}; in {raw:?}"))?
            }
        };
        out.push(expanded);
        rest = &rest[i + end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn parse_xml(text: &str) -> Result<Element> {
    let mut stack: Vec<Element> = vec![Element::default()];
    let mut pending: Option<Element> = None;
    for token in Tokenizer::from(text) {
        match token.map_err(|e| anyhow!("{e}"))? {
            Token::ElementStart { local, .. } => {
                pending = Some(Element {
                    name: local.as_str().to_string(),
                    ..Element::default()
                });
            }
            Token::Attribute { local, value, .. } => {
                if let Some(p) = pending.as_mut() {
                    p.attributes
                        .push((local.as_str().to_string(), attribute_value(value.as_str())?));
                }
            }
            Token::ElementEnd { end, .. } => match end {
                ElementEnd::Open => {
                    if let Some(p) = pending.take() {
                        stack.push(p);
                    }
                }
                ElementEnd::Empty => {
                    if let (Some(p), Some(parent)) = (pending.take(), stack.last_mut()) {
                        parent.children.push(p);
                    }
                }
                ElementEnd::Close(_, local) => {
                    let element = stack
                        .pop()
                        .ok_or_else(|| anyhow!("unbalanced </{}>", local.as_str()))?;
                    if element.name != local.as_str() {
                        bail!("</{}> closes <{}>", local.as_str(), element.name);
                    }
                    stack
                        .last_mut()
                        .ok_or_else(|| anyhow!("unbalanced </{}>", local.as_str()))?
                        .children
                        .push(element);
                }
            },
            _ => {}
        }
    }
    let mut document = stack.pop().ok_or_else(|| anyhow!("no document"))?;
    if !stack.is_empty() {
        bail!("unclosed <{}>", document.name);
    }
    let root = document
        .children
        .pop()
        .ok_or_else(|| anyhow!("no <module> element"))?;
    Ok(root)
}

/// A module's `<property>` values (with `${...}` resolved the only way the
/// command line can without system properties: through `default`) and its
/// `<message>` overrides.
struct Properties {
    values: Vec<(String, String)>,
    messages: HashMap<String, String>,
    children: Vec<Element>,
}

fn resolve_property(name: &str, value: &str, default: Option<&str>) -> Result<String> {
    let mut out = String::new();
    let mut rest = value;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        if let Some(stripped) = after.strip_prefix('$') {
            out.push('$');
            rest = stripped;
        } else if let Some(inner) = after.strip_prefix('{') {
            let end = inner
                .find('}')
                .ok_or_else(|| anyhow!("Syntax error in property: {value}"))?;
            match default {
                Some(d) => return Ok(d.to_string()),
                None => bail!(
                    "Property ${{{}}} has not been set (property {name:?})",
                    &inner[..end]
                ),
            }
        } else {
            out.push('$');
            rest = after;
        }
    }
    out.push_str(rest);
    Ok(out)
}

fn split_module(element: Element) -> Result<Properties> {
    let mut values = Vec::new();
    let mut messages = HashMap::new();
    let mut children = Vec::new();
    for child in element.children {
        match child.name.as_str() {
            "property" => {
                let name = child
                    .attribute("name")
                    .ok_or_else(|| anyhow!("<property> without a name"))?;
                let value = child
                    .attribute("value")
                    .ok_or_else(|| anyhow!("<property name={name:?}> without a value"))?;
                let value = resolve_property(name, value, child.attribute("default"))?;
                values.push((name.to_string(), value));
            }
            "message" => {
                let key = child
                    .attribute("key")
                    .ok_or_else(|| anyhow!("<message> without a key"))?;
                let value = child
                    .attribute("value")
                    .ok_or_else(|| anyhow!("<message key={key:?}> without a value"))?;
                validate_pattern(value).map_err(|e| anyhow!("message {key:?}: {e}"))?;
                messages.insert(key.to_string(), value.to_string());
            }
            "module" => children.push(child),
            "metadata" => {}
            other => bail!("unexpected element <{other}>"),
        }
    }
    Ok(Properties {
        values,
        messages,
        children,
    })
}

/// commons-beanutils' `BooleanConverter`.
fn parse_bool(module: &str, name: &str, value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "y" | "on" | "1" => Ok(true),
        "false" | "no" | "n" | "off" | "0" => Ok(false),
        _ => bail!("cannot initialize module {module} - Cannot set property '{name}' to '{value}'"),
    }
}

fn parse_int(module: &str, name: &str, value: &str) -> Result<i32> {
    value.trim().parse().map_err(|_| {
        anyhow!("cannot initialize module {module} - Cannot set property '{name}' to '{value}'")
    })
}

/// `RelaxedStringArrayConverter`, with `setFileExtensions`' leading dot.
fn parse_extensions(value: &str) -> Vec<String> {
    value
        .trim()
        .split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| {
            if e.starts_with('.') {
                e.to_string()
            } else {
                format!(".{e}")
            }
        })
        .collect()
}

fn parse_severity(module: &str, value: &str) -> Result<Severity> {
    Severity::parse(value).ok_or_else(|| {
        anyhow!("cannot initialize module {module} - Cannot set property 'severity' to '{value}'")
    })
}

/// The module class a name refers to: `Name`, `NameCheck`, or the fully
/// qualified class.
fn resolve_name<'a>(name: &str, known: &[(&'a str, &str)]) -> Option<&'a str> {
    known.iter().find_map(|&(short, package)| {
        let full = format!("com.puppycrawl.tools.checkstyle.{package}.{short}");
        let matches = name == short
            || name == format!("{short}Check")
            || name == full
            || name == format!("{full}Check");
        matches.then_some(short)
    })
}

fn unknown_property(module: &str, name: &str) -> anyhow::Error {
    anyhow!(
        "cannot initialize module {module} - Property '{name}' does not exist, please check the documentation"
    )
}

/// `severity`, `id` and `tabWidth`, inherited unless set.
fn common(
    module: &str,
    props: &mut Vec<(String, String)>,
    inherited: &Common,
    messages: HashMap<String, String>,
) -> Result<Common> {
    let mut common = Common {
        severity: inherited.severity,
        id: None,
        tab_width: inherited.tab_width,
        messages,
    };
    // Checkstyle's `setTabWidth` takes any int and `lengthExpandedTabs`
    // divides by it: a width of 0 is an `ArithmeticException` on the first
    // tab, a negative one produces nonsense columns. Neither is a
    // configuration anyone means, so both are refused here.
    let mut non_positive_tab_width = None;
    props.retain(|(name, value)| match name.as_str() {
        "severity" => {
            common.severity = match parse_severity(module, value) {
                Ok(s) => s,
                Err(_) => return true,
            };
            false
        }
        "id" => {
            common.id = Some(value.clone());
            false
        }
        "tabWidth" => match parse_int(module, name, value) {
            Ok(w) if w > 0 => {
                common.tab_width = w;
                false
            }
            Ok(_) => {
                non_positive_tab_width = Some(value.clone());
                true
            }
            Err(_) => true,
        },
        _ => true,
    });
    if let Some(value) = non_positive_tab_width {
        bail!(
            "cannot initialize module {module} - Cannot set property 'tabWidth' to '{value}': the tab width must be a positive number"
        );
    }
    if let Some((name, value)) = props
        .iter()
        .find(|(n, _)| n == "severity" || n == "tabWidth")
    {
        bail!("cannot initialize module {module} - Cannot set property '{name}' to '{value}'");
    }
    Ok(common)
}

fn check_module(element: Element, inherited: &Common) -> Result<CheckModule> {
    let raw_name = element.attribute("name").unwrap_or_default().to_string();
    let name = resolve_name(&raw_name, CHECKS).ok_or_else(|| {
        anyhow!(
            "icheckstyle does not implement module '{raw_name}' (under TreeWalker it implements: {})",
            CHECKS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
        )
    })?;
    let Properties {
        values: mut props,
        messages,
        children,
    } = split_module(element)?;
    if let Some(child) = children.first() {
        bail!(
            "cannot initialize module {name} - {} is not allowed as a child in {name}",
            child.attribute("name").unwrap_or_default()
        );
    }
    let common = common(name, &mut props, inherited, messages)?;
    let mut get_bool = |prop: &str, default: bool| -> Result<bool> {
        match props.iter().position(|(n, _)| n == prop) {
            Some(i) => {
                let (n, v) = props.remove(i);
                parse_bool(name, &n, &v)
            }
            None => Ok(default),
        }
    };
    let kind = match name {
        "ArrayTypeStyle" => CheckKind::ArrayTypeStyle {
            java_style: get_bool("javaStyle", true)?,
        },
        "CovariantEquals" => CheckKind::CovariantEquals,
        "DefaultComesLast" => CheckKind::DefaultComesLast {
            skip_if_last_and_shared_with_case: get_bool("skipIfLastAndSharedWithCase", false)?,
        },
        "EmptyStatement" => CheckKind::EmptyStatement,
        "EqualsHashCode" => CheckKind::EqualsHashCode,
        "FallThrough" => {
            let check_last_case_group = get_bool("checkLastCaseGroup", false)?;
            let pattern = match props.iter().position(|(n, _)| n == "reliefPattern") {
                Some(i) => props.remove(i).1,
                None => "falls?[ -]?thr(u|ough)".to_string(),
            };
            let relief_pattern = Regex::new(&pattern).map_err(|e| {
                anyhow!("cannot initialize module FallThrough - reliefPattern {pattern:?}: {e}")
            })?;
            CheckKind::FallThrough {
                check_last_case_group,
                relief_pattern,
            }
        }
        "ModifiedControlVariable" => CheckKind::ModifiedControlVariable {
            skip_enhanced_for_loop_variable: get_bool("skipEnhancedForLoopVariable", false)?,
        },
        "OneStatementPerLine" => CheckKind::OneStatementPerLine {
            treat_try_resources_as_statement: get_bool("treatTryResourcesAsStatement", false)?,
        },
        "RedundantImport" => CheckKind::RedundantImport,
        "SimplifyBooleanExpression" => CheckKind::SimplifyBooleanExpression,
        "SimplifyBooleanReturn" => CheckKind::SimplifyBooleanReturn,
        "StringLiteralEquality" => CheckKind::StringLiteralEquality,
        "UnusedImports" => CheckKind::UnusedImports {
            process_javadoc: get_bool("processJavadoc", true)?,
        },
        _ => CheckKind::UpperEll,
    };
    // `tokens`: these checks accept exactly their default tokens, so the
    // property can only restate them.
    if let Some(i) = props.iter().position(|(n, _)| n == "tokens") {
        let (_, value) = props.remove(i);
        let accepted = super::checks::tokens(&kind);
        for token in value.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            let ty = super::tables::TOKEN_NAMES.iter().position(|n| *n == token);
            let known = ty.is_some_and(|t| accepted.contains(&(t as u16)))
                || token_type_name_matches(token, accepted);
            if !known {
                bail!(
                    "cannot initialize module {name} - Token \"{token}\" was not found in Acceptable tokens list in check {name}"
                );
            }
        }
    }
    if let Some((prop, _)) = props.first() {
        return Err(unknown_property(name, prop));
    }
    Ok(CheckModule { common, kind })
}

/// `TokenTypes` names that differ from the grammar's (`LITERAL_TRUE` for
/// `"true"`, ...).
fn token_type_name_matches(token: &str, accepted: &[u16]) -> bool {
    accepted.iter().any(|&ty| {
        let grammar = super::tables::TOKEN_NAMES
            .get(usize::from(ty))
            .copied()
            .unwrap_or("");
        let literal = grammar.trim_matches('"');
        grammar.starts_with('"') && token == format!("LITERAL_{}", literal.to_ascii_uppercase())
    })
}

fn tree_walker(element: Element, inherited: &Common) -> Result<FileSetModule> {
    let Properties {
        values: mut props,
        messages,
        children,
    } = split_module(element)?;
    let common = common("TreeWalker", &mut props, inherited, messages)?;
    let mut file_extensions = vec![".java".to_string()];
    for (name, value) in props {
        match name.as_str() {
            "fileExtensions" => file_extensions = parse_extensions(&value),
            _ => return Err(unknown_property("TreeWalker", &name)),
        }
    }
    let child_context = Common {
        severity: common.severity,
        id: None,
        tab_width: common.tab_width,
        messages: HashMap::new(),
    };
    let checks = children
        .into_iter()
        .map(|c| check_module(c, &child_context))
        .collect::<Result<Vec<_>>>()?;
    Ok(FileSetModule::TreeWalker {
        file_extensions,
        checks,
    })
}

fn newline_at_end_of_file(element: Element, inherited: &Common) -> Result<FileSetModule> {
    let Properties {
        values: mut props,
        messages,
        children,
    } = split_module(element)?;
    if !children.is_empty() {
        bail!("cannot initialize module NewlineAtEndOfFile - it has no child modules");
    }
    let common = common("NewlineAtEndOfFile", &mut props, inherited, messages)?;
    let mut file_extensions = Vec::new();
    let mut line_separator = LineSeparator::LfCrCrlf;
    for (name, value) in props {
        match name.as_str() {
            "fileExtensions" => file_extensions = parse_extensions(&value),
            "lineSeparator" => {
                line_separator = match value.trim().to_ascii_uppercase().as_str() {
                    "CRLF" => LineSeparator::Crlf,
                    "CR" => LineSeparator::Cr,
                    "LF" | "SYSTEM" => LineSeparator::Lf,
                    "LF_CR_CRLF" => LineSeparator::LfCrCrlf,
                    _ => bail!(
                        "cannot initialize module NewlineAtEndOfFile - Cannot set property 'lineSeparator' to '{value}'"
                    ),
                }
            }
            _ => return Err(unknown_property("NewlineAtEndOfFile", &name)),
        }
    }
    Ok(FileSetModule::NewlineAtEndOfFile {
        common,
        file_extensions,
        line_separator,
    })
}

const FILE_SET_MODULES: &[(&str, &str)] = &[("TreeWalker", ""), ("NewlineAtEndOfFile", "checks")];

/// Read a configuration file's text.
pub fn parse(text: &str) -> Result<Config> {
    let root = parse_xml(text)?;
    let root_name = root.attribute("name").unwrap_or_default().to_string();
    if root.name != "module"
        || !(root_name == "Checker" || root_name == "com.puppycrawl.tools.checkstyle.Checker")
    {
        bail!("the configuration's root must be <module name=\"Checker\">, not {root_name:?}");
    }
    let Properties {
        values: mut props,
        messages,
        children,
    } = split_module(root)?;
    if !messages.is_empty() {
        bail!("Checker has no messages to override");
    }
    let base = Common {
        severity: Severity::Error,
        id: None,
        tab_width: 8,
        messages: HashMap::new(),
    };
    let checker = common("Checker", &mut props, &base, HashMap::new())?;
    if checker.id.is_some() {
        return Err(unknown_property("Checker", "id"));
    }
    let mut file_extensions = Vec::new();
    for (name, value) in props {
        match name.as_str() {
            "fileExtensions" => file_extensions = parse_extensions(&value),
            "charset" => {
                if !value.trim().eq_ignore_ascii_case("utf-8")
                    && !value.trim().eq_ignore_ascii_case("utf8")
                {
                    bail!("icheckstyle reads UTF-8 only (charset {value:?})");
                }
            }
            "haltOnException" => {
                if !parse_bool("Checker", &name, &value)? {
                    bail!("icheckstyle does not implement haltOnException = false");
                }
            }
            "localeLanguage" => {
                if !value.is_empty() && value != "en" {
                    bail!("icheckstyle has English messages only (localeLanguage {value:?})");
                }
            }
            "localeCountry" | "cacheFile" | "basedir" => {}
            _ => return Err(unknown_property("Checker", &name)),
        }
    }
    let mut modules = Vec::new();
    for child in children {
        let raw = child.attribute("name").unwrap_or_default().to_string();
        let name = resolve_name(&raw, FILE_SET_MODULES).or_else(|| {
            (raw == "com.puppycrawl.tools.checkstyle.TreeWalker").then_some("TreeWalker")
        });
        modules.push(match name {
            Some("TreeWalker") => tree_walker(child, &checker)?,
            Some(_) => newline_at_end_of_file(child, &checker)?,
            None if resolve_name(&raw, CHECKS).is_some() => {
                bail!("cannot initialize module {raw} - it must be a child of TreeWalker, not of Checker")
            }
            None => bail!(
                "icheckstyle does not implement module '{raw}' (under Checker it implements: TreeWalker, NewlineAtEndOfFile)"
            ),
        });
    }
    Ok(Config {
        file_extensions,
        modules,
    })
}

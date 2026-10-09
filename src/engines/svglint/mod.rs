//! svglint, in-process: a port of svglint 4.2.1's built-in rules.
//!
//! svglint parses each SVG leniently (htmlparser2 in XML mode), then runs
//! every configured rule over the result and reports each rule's findings,
//! rules in alphabetical order. The built-in rules are `valid` (the file is
//! well-formed XML, judged by fast-xml-parser's validator), `elm` (which
//! elements may appear, by CSS selector) and `attr` (which attributes an
//! element may carry, and with what values). svglint's `custom` rules are
//! JavaScript functions and have no counterpart here.
//!
//! * [`dom`]: the lenient parser.
//! * [`selector`]: CSS selectors with css-select's semantics.
//! * [`validator`]: the `valid` rule's XML check.
//! * [`rules`]: `elm` and `attr`.

pub mod dom;
pub mod rules;
pub mod selector;
pub mod validator;

use rules::{AttrRule, ElmRule};

/// The configured rules.
pub struct Rules {
    pub valid: bool,
    pub elm: Option<ElmRule>,
    pub attr: Vec<AttrRule>,
}

impl Rules {
    /// Build the rules from the processor's TOML fields. An empty `elm`
    /// table means no `elm` rule, as svglint has none without config.
    pub fn from_config(
        valid: bool,
        elm: &toml::Table,
        attr: &[toml::Table],
    ) -> Result<Self, String> {
        let elm = if elm.is_empty() {
            None
        } else {
            Some(ElmRule::from_toml(elm)?)
        };
        let attr = attr
            .iter()
            .enumerate()
            .map(|(i, t)| AttrRule::from_toml(t).map_err(|e| format!("{e} (attr entry {})", i + 1)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { valid, elm, attr })
    }
}

/// Where a finding points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct At {
    /// 1-based line of the element's start tag.
    pub line: usize,
    /// 1-based column, in UTF-16 code units, as svglint counts.
    pub column: usize,
    pub name: String,
}

/// One finding, as svglint would print it: the rule's display name
/// (`attr-2`, `elm`, `valid`), the message, and the element if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub rule: String,
    pub text: String,
    pub at: Option<At>,
}

/// Lint one document. `Err` is svglint's own failure to parse (a document
/// that yields no nodes at all); `Ok(reports)` the findings, in the order
/// svglint lists them.
pub fn lint(source: &str, rules: &Rules) -> Result<Vec<Report>, String> {
    let dom = dom::parse(source);
    if dom.roots.is_empty() && !source.trim().is_empty() {
        return Err("Unable to parse SVG".to_string());
    }
    let mut out = Vec::new();
    let locate = |dom: &dom::Dom, node: Option<dom::NodeId>| {
        node.map(|id| {
            let (line, column) = dom.position(dom.node(id).start_index);
            At {
                line,
                column,
                name: dom.tag_name(id).unwrap_or("").to_string(),
            }
        })
    };
    for (i, rule) in rules.attr.iter().enumerate() {
        let name = format!("attr-{}", i + 1);
        for m in rule.run(&dom) {
            out.push(Report {
                rule: name.clone(),
                text: m.text,
                at: locate(&dom, m.node),
            });
        }
    }
    if let Some(elm) = &rules.elm {
        for m in elm.run(&dom) {
            out.push(Report {
                rule: "elm".to_string(),
                text: m.text,
                at: locate(&dom, m.node),
            });
        }
    }
    if rules.valid
        && !source.is_empty()
        && let Some(text) = validator::validate(source)
    {
        out.push(Report {
            rule: "valid".to_string(),
            text,
            at: None,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests;

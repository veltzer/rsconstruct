//! svglint's built-in rules, `elm` and `attr`, with their configuration
//! read from TOML instead of a JavaScript object. The messages are
//! svglint's, word for word.
//!
//! `elm`: a table of CSS selector to expectation, where `true` means at
//! least one match, `false` none, a number an exact count and a pair
//! `[min, max]` a range. An element some rule forbids is still fine when
//! another rule allows it (`"title" = false, "svg > title" = true`).
//!
//! `attr`: an array of tables, each applying to the elements of
//! `"rule::selector"` (default `*`). A key names an attribute: `false`
//! forbids it, `true` requires it, a string requires that exact value, an
//! array of strings one of them, and `{ regex = "...", flags = "i" }` a
//! JavaScript-style regular expression match. A trailing `?` on the key
//! makes the attribute optional (checked when present, allowed under
//! `"rule::whitelist"`). `"rule::whitelist" = true` forbids attributes the
//! table does not name; `"rule::order"` is `true` for alphabetical order
//! or a list giving the required order.

use regex::Regex;

use super::dom::{Dom, NodeId};
use super::selector::SelectorList;

/// A problem a rule found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    /// The element the problem is about, when there is one.
    pub node: Option<NodeId>,
}

#[derive(Debug, Clone)]
enum ElmExpect {
    Required,
    Forbidden,
    Exactly(i64),
    Between(i64, i64),
}

#[derive(Debug, Clone)]
pub struct ElmRule {
    entries: Vec<(String, SelectorList, ElmExpect)>,
}

#[derive(Debug, Clone)]
enum AttrExpect {
    /// `true`: must exist, any value.
    Allowed,
    /// `false`: must not exist.
    Forbidden,
    Exact(String),
    OneOf(Vec<String>),
    Pattern {
        regex: Regex,
        /// `/source/flags`, for the message.
        display: String,
    },
}

impl AttrExpect {
    /// JavaScript truthiness of the configured value, which is what makes
    /// an attribute required: `false` and `""` are falsy, all else truthy.
    const fn is_truthy(&self) -> bool {
        match self {
            Self::Forbidden => false,
            Self::Exact(s) => !s.is_empty(),
            Self::Allowed | Self::OneOf(_) | Self::Pattern { .. } => true,
        }
    }
}

#[derive(Debug, Clone)]
enum Order {
    Alphabetical,
    Given(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct AttrRule {
    selector: SelectorList,
    whitelist: bool,
    order: Option<Order>,
    /// Attribute keys as configured (a trailing `?` marks an optional one).
    attrs: Vec<(String, AttrExpect)>,
}

const SPECIAL_KEYS: [&str; 3] = ["rule::selector", "rule::whitelist", "rule::order"];
const OPTIONAL_SUFFIX: char = '?';

const fn type_name(v: &toml::Value) -> &'static str {
    match v {
        toml::Value::String(_) => "a string",
        toml::Value::Integer(_) => "an integer",
        toml::Value::Float(_) => "a float",
        toml::Value::Boolean(_) => "a boolean",
        toml::Value::Datetime(_) => "a datetime",
        toml::Value::Array(_) => "an array",
        toml::Value::Table(_) => "a table",
    }
}

impl ElmRule {
    pub fn from_toml(table: &toml::Table) -> Result<Self, String> {
        let mut entries = Vec::new();
        for (selector_text, value) in table {
            let selector = SelectorList::parse(selector_text).map_err(|e| format!("elm: {e}"))?;
            let expect = match value {
                toml::Value::Boolean(true) => ElmExpect::Required,
                toml::Value::Boolean(false) => ElmExpect::Forbidden,
                toml::Value::Integer(n) => ElmExpect::Exactly(*n),
                toml::Value::Array(items) => match items.as_slice() {
                    [toml::Value::Integer(lo), toml::Value::Integer(hi)] => {
                        ElmExpect::Between(*lo, *hi)
                    }
                    _ => {
                        return Err(format!(
                            "elm {selector_text:?}: a range is two integers [min, max], got {value}"
                        ));
                    }
                },
                other => {
                    return Err(format!(
                        "elm {selector_text:?}: expected true, false, a count or [min, max], got {}",
                        type_name(other)
                    ));
                }
            };
            entries.push((selector_text.clone(), selector, expect));
        }
        Ok(Self { entries })
    }

    pub fn run(&self, dom: &Dom) -> Vec<Message> {
        // Each selector's verdict: the elements it allows, and the
        // problems it raises (with the element, or without one).
        let mut allowed: Vec<NodeId> = Vec::new();
        let mut disallowed: Vec<Vec<Message>> = Vec::new();
        for (text, selector, expect) in &self.entries {
            let matches = selector.select_all(dom);
            let mut problems = Vec::new();
            match expect {
                ElmExpect::Required => {
                    if matches.is_empty() {
                        problems.push(Message {
                            text: format!("Expected '{text}', none found"),
                            node: None,
                        });
                    }
                    allowed.extend(&matches);
                }
                ElmExpect::Forbidden => {
                    problems.extend(matches.iter().map(|&m| Message {
                        text: "Element disallowed".to_string(),
                        node: Some(m),
                    }));
                }
                ElmExpect::Exactly(n) => {
                    if i64::try_from(matches.len()).unwrap_or(i64::MAX) == *n {
                        allowed.extend(&matches);
                    } else {
                        let text = format!(
                            "Found {} elements for '{text}', expected {n}",
                            matches.len()
                        );
                        if matches.is_empty() {
                            problems.push(Message { text, node: None });
                        } else {
                            problems.extend(matches.iter().map(|&m| Message {
                                text: text.clone(),
                                node: Some(m),
                            }));
                        }
                    }
                }
                ElmExpect::Between(lo, hi) => {
                    let count = i64::try_from(matches.len()).unwrap_or(i64::MAX);
                    if count >= *lo && count <= *hi {
                        allowed.extend(&matches);
                    } else {
                        problems.push(Message {
                            text: format!(
                                "Found {} elements for '{text}', expected between {lo} and {hi}",
                                matches.len()
                            ),
                            node: None,
                        });
                    }
                }
            }
            disallowed.push(problems);
        }
        disallowed
            .into_iter()
            .flatten()
            .filter(|m| m.node.is_none_or(|n| !allowed.contains(&n)))
            .collect()
    }
}

/// Translate a JavaScript regular expression (source and flags) to the
/// `regex` crate.
fn compile_js_regex(source: &str, flags: &str) -> Result<Regex, String> {
    let mut inline = String::new();
    for f in flags.chars() {
        match f {
            'i' => inline.push('i'),
            'm' => inline.push('m'),
            's' => inline.push('s'),
            'u' => {}
            other => return Err(format!("unsupported regex flag {other:?}")),
        }
    }
    let pattern = if inline.is_empty() {
        source.to_string()
    } else {
        format!("(?{inline}){source}")
    };
    Regex::new(&pattern).map_err(|e| format!("bad regex /{source}/{flags}: {e}"))
}

impl AttrRule {
    pub fn from_toml(table: &toml::Table) -> Result<Self, String> {
        let selector_text = match table.get("rule::selector") {
            None => "*",
            Some(toml::Value::String(s)) => s.as_str(),
            Some(other) => {
                return Err(format!(
                    "attr: \"rule::selector\" must be a string, got {}",
                    type_name(other)
                ));
            }
        };
        let selector = SelectorList::parse(selector_text).map_err(|e| format!("attr: {e}"))?;
        let whitelist = match table.get("rule::whitelist") {
            None => false,
            Some(toml::Value::Boolean(b)) => *b,
            Some(other) => {
                return Err(format!(
                    "attr: \"rule::whitelist\" must be a boolean, got {}",
                    type_name(other)
                ));
            }
        };
        let order = match table.get("rule::order") {
            None | Some(toml::Value::Boolean(false)) => None,
            Some(toml::Value::Boolean(true)) => Some(Order::Alphabetical),
            Some(toml::Value::Array(items)) => {
                let mut names = Vec::new();
                for item in items {
                    match item {
                        toml::Value::String(s) => names.push(s.clone()),
                        other => {
                            return Err(format!(
                                "attr: \"rule::order\" entries must be strings, got {}",
                                type_name(other)
                            ));
                        }
                    }
                }
                Some(Order::Given(names))
            }
            Some(other) => {
                return Err(format!(
                    "attr: \"rule::order\" must be true or an array of strings, got {}",
                    type_name(other)
                ));
            }
        };
        let mut attrs = Vec::new();
        for (key, value) in table {
            if SPECIAL_KEYS.contains(&key.as_str()) {
                continue;
            }
            let expect = match value {
                toml::Value::Boolean(true) => AttrExpect::Allowed,
                toml::Value::Boolean(false) => AttrExpect::Forbidden,
                toml::Value::String(s) => AttrExpect::Exact(s.clone()),
                toml::Value::Array(items) => {
                    let mut values = Vec::new();
                    for item in items {
                        match item {
                            toml::Value::String(s) => values.push(s.clone()),
                            other => {
                                return Err(format!(
                                    "attr {key:?}: allowed values must be strings, got {}",
                                    type_name(other)
                                ));
                            }
                        }
                    }
                    AttrExpect::OneOf(values)
                }
                toml::Value::Table(t) => {
                    let Some(toml::Value::String(source)) = t.get("regex") else {
                        return Err(format!(
                            "attr {key:?}: a table value must be {{ regex = \"...\" }}"
                        ));
                    };
                    let flags = match t.get("flags") {
                        None => "",
                        Some(toml::Value::String(s)) => s.as_str(),
                        Some(other) => {
                            return Err(format!(
                                "attr {key:?}: flags must be a string, got {}",
                                type_name(other)
                            ));
                        }
                    };
                    if let Some(extra) = t.keys().find(|k| *k != "regex" && *k != "flags") {
                        return Err(format!("attr {key:?}: unknown regex key {extra:?}"));
                    }
                    AttrExpect::Pattern {
                        regex: compile_js_regex(source, flags)
                            .map_err(|e| format!("attr {key:?}: {e}"))?,
                        display: format!("/{source}/{flags}"),
                    }
                }
                other => {
                    return Err(format!(
                        "attr {key:?}: expected true, false, a string, an array of strings or {{ regex = \"...\" }}, got {}",
                        type_name(other)
                    ));
                }
            };
            attrs.push((key.clone(), expect));
        }
        Ok(Self {
            selector,
            whitelist,
            order,
            attrs,
        })
    }

    pub fn run(&self, dom: &Dom) -> Vec<Message> {
        let mut out = Vec::new();
        for element in self.selector.select_all(dom) {
            self.run_on_element(dom, element, &mut out);
        }
        out
    }

    fn expectation(&self, attribute: &str) -> Option<&AttrExpect> {
        self.attrs
            .iter()
            .find(|(k, _)| k == attribute)
            .or_else(|| {
                let optional = format!("{attribute}{OPTIONAL_SUFFIX}");
                self.attrs.iter().find(|(k, _)| *k == optional)
            })
            .map(|(_, e)| e)
    }

    fn run_on_element(&self, dom: &Dom, element: NodeId, out: &mut Vec<Message>) {
        let report = |out: &mut Vec<Message>, text: String| {
            out.push(Message {
                text,
                node: Some(element),
            });
        };
        let mut remaining: Vec<(String, String)> = dom.attrs(element).to_vec();

        // Required attributes.
        for (key, expect) in &self.attrs {
            if key.ends_with(OPTIONAL_SUFFIX) {
                continue;
            }
            if expect.is_truthy() && !remaining.iter().any(|(n, _)| n == key) {
                report(out, format!("Expected attribute '{key}', didn't find it"));
            }
        }

        // Ordering.
        if let Some(order) = &self.order {
            let names: Vec<&str> = remaining.iter().map(|(n, _)| n.as_str()).collect();
            if !names.is_empty() {
                let order: Vec<String> = match order {
                    Order::Alphabetical => {
                        let mut sorted: Vec<String> =
                            names.iter().map(|n| (*n).to_string()).collect();
                        sorted.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
                        sorted
                    }
                    Order::Given(list) => list.clone(),
                };
                let index_of =
                    |name: &str| -> Option<usize> { order.iter().position(|o| o == name) };
                let mut previous = index_of(names[0]);
                for name in &names[1..] {
                    let Some(index) = index_of(name) else {
                        continue;
                    };
                    if let Some(prev) = previous
                        && index < prev
                    {
                        report(
                            out,
                            format!(
                                "Wrong ordering of attributes, found \"{}\", expected \"{}\"",
                                names.join(", "),
                                order.join(", ")
                            ),
                        );
                        break;
                    }
                    previous = Some(index);
                }
            }
        }

        // Values.
        remaining.retain(|(name, value)| {
            let Some(expect) = self.expectation(name) else {
                return true;
            };
            match expect {
                AttrExpect::Allowed => {}
                AttrExpect::Forbidden => report(out, format!("Attribute '{name}' is disallowed")),
                AttrExpect::Exact(expected) => {
                    if value != expected {
                        report(
                            out,
                            format!(
                                "Expected attribute '{name}' to be \"{expected}\", was \"{value}\""
                            ),
                        );
                    }
                }
                AttrExpect::OneOf(values) => {
                    if !values.contains(value) {
                        let list = serde_json::to_string(values).unwrap_or_default();
                        report(
                            out,
                            format!(
                                "Expected attribute '{name}' to be one of {list}, was \"{value}\""
                            ),
                        );
                    }
                }
                AttrExpect::Pattern { regex, display } => {
                    if !regex.is_match(value) {
                        report(
                            out,
                            format!(
                                "Expected attribute '{name}' to match {display}, was \"{value}\""
                            ),
                        );
                    }
                }
            }
            false
        });

        if self.whitelist {
            let extra: Vec<&str> = remaining
                .iter()
                .map(|(n, _)| n.as_str())
                .filter(|n| !n.ends_with(OPTIONAL_SUFFIX))
                .collect();
            if !extra.is_empty() {
                let list = serde_json::to_string(&extra).unwrap_or_default();
                report(
                    out,
                    format!("Found extra attributes {list} with whitelisting enabled"),
                );
            }
        }
    }
}

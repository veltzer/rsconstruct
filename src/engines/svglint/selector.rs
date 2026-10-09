//! CSS selectors with css-select 5.2 semantics in XML mode, as cheerio
//! runs them for svglint's `elm` and `attr` rules: case-sensitive tag and
//! attribute names, the seven attribute operators (`!=` included), the
//! four combinators, selector lists, and the pseudo-classes css-select
//! implements without a browser: `:not`, `:is`/`:matches`/`:where`,
//! `:has` (with a leading combinator), the `*-child` and `*-of-type`
//! families with `An+B`, `:empty`, `:contains` and `:icontains`.
//!
//! cheerio appends the parsed document to a `<root>` wrapper element and
//! searches below it, so a top-level `<svg>` has a parent element as far
//! as combinators are concerned (`* > svg` matches it); the wrapper itself
//! is never a result. The matcher models that wrapper.
//!
//! Anything else (pseudo-elements, namespaces, `:root`, `:scope`,
//! `:nth-child(... of S)`, dynamic states) is refused with an error naming
//! it rather than matched approximately.

use super::dom::{Dom, NodeId};

#[derive(Debug, Clone)]
pub struct SelectorList {
    complexes: Vec<Complex>,
}

#[derive(Debug, Clone)]
struct Complex {
    /// Compounds left to right; each carries the combinator to its left.
    /// The first compound's combinator is the one that relates it to the
    /// anchor in `:has(...)`, and `Descendant` otherwise.
    parts: Vec<(Combinator, Compound)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

#[derive(Debug, Clone, Default)]
struct Compound {
    /// `None` for `*` or no type selector.
    tag: Option<String>,
    simples: Vec<Simple>,
}

#[derive(Debug, Clone)]
enum Simple {
    Id(String),
    Class(String),
    Attr {
        name: String,
        op: AttrOp,
        value: String,
        ignore_case: bool,
    },
    Not(SelectorList),
    Is(SelectorList),
    Has(SelectorList),
    Nth {
        kind: NthKind,
        a: i64,
        b: i64,
    },
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    Empty,
    Contains(String),
    IContains(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrOp {
    Exists,
    Equals,
    Element,
    Hyphen,
    Start,
    End,
    Any,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NthKind {
    Child,
    LastChild,
    OfType,
    LastOfType,
}

// ---- parsing ----------------------------------------------------------------

struct SelParser<'a> {
    chars: Vec<char>,
    pos: usize,
    text: &'a str,
}

const fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

/// Characters that end an identifier (a superset of what css-what treats
/// as structural); anything else, or a backslash escape, is part of a name.
const fn is_ident_char(c: char) -> bool {
    !(is_ws(c)
        || matches!(
            c,
            '#' | '.'
                | ':'
                | '['
                | ']'
                | ','
                | '>'
                | '+'
                | '~'
                | '('
                | ')'
                | '*'
                | '='
                | '|'
                | '^'
                | '$'
                | '!'
                | '"'
                | '\''
        ))
}

impl SelParser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn skip_ws(&mut self) -> bool {
        let start = self.pos;
        while self.peek().is_some_and(is_ws) {
            self.pos += 1;
        }
        self.pos > start
    }

    fn err(&self, what: &str) -> String {
        format!("selector {:?}: {what}", self.text)
    }

    /// A CSS escape after the backslash: `\31 ` style hex or a literal char.
    fn escape(&mut self) -> Result<char, String> {
        let mut hex = String::new();
        while hex.len() < 6 && self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
            hex.push(self.bump().unwrap_or('0'));
        }
        if !hex.is_empty() {
            if self.peek().is_some_and(is_ws) {
                self.pos += 1;
            }
            let code =
                u32::from_str_radix(&hex, 16).map_err(|e| self.err(&format!("bad escape: {e}")))?;
            return Ok(char::from_u32(code).unwrap_or('\u{FFFD}'));
        }
        self.bump().ok_or_else(|| self.err("dangling backslash"))
    }

    fn ident(&mut self) -> Result<String, String> {
        let mut out = String::new();
        loop {
            match self.peek() {
                Some('\\') => {
                    self.pos += 1;
                    out.push(self.escape()?);
                }
                Some(c) if is_ident_char(c) => {
                    out.push(c);
                    self.pos += 1;
                }
                _ => break,
            }
        }
        if out.is_empty() {
            return Err(self.err("expected a name"));
        }
        Ok(out)
    }

    fn quoted(&mut self, quote: char) -> Result<String, String> {
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(self.err("unterminated string")),
                Some(c) if c == quote => return Ok(out),
                Some('\\') => out.push(self.escape()?),
                Some(c) => out.push(c),
            }
        }
    }

    fn list(&mut self, relative: bool) -> Result<SelectorList, String> {
        let mut complexes = vec![self.complex(relative)?];
        loop {
            self.skip_ws();
            if self.peek() == Some(',') {
                self.pos += 1;
                self.skip_ws();
                complexes.push(self.complex(relative)?);
            } else {
                break;
            }
        }
        Ok(SelectorList { complexes })
    }

    fn combinator_char(&mut self) -> Option<Combinator> {
        let comb = match self.peek()? {
            '>' => Combinator::Child,
            '+' => Combinator::NextSibling,
            '~' => Combinator::SubsequentSibling,
            _ => return None,
        };
        self.pos += 1;
        Some(comb)
    }

    fn complex(&mut self, relative: bool) -> Result<Complex, String> {
        self.skip_ws();
        let leading = match self.combinator_char() {
            Some(c) if relative => c,
            Some(_) => return Err(self.err("a selector cannot start with a combinator")),
            None => Combinator::Descendant,
        };
        self.skip_ws();
        let mut parts = vec![(leading, self.compound()?)];
        loop {
            let had_ws = self.skip_ws();
            match self.peek() {
                None | Some(',' | ')') => break,
                _ => {}
            }
            let comb = match self.combinator_char() {
                Some(c) => {
                    self.skip_ws();
                    c
                }
                None if had_ws => Combinator::Descendant,
                None => {
                    return Err(self.err(&format!("unexpected {:?}", self.peek().unwrap_or(' '))));
                }
            };
            parts.push((comb, self.compound()?));
        }
        Ok(Complex { parts })
    }

    fn compound(&mut self) -> Result<Compound, String> {
        let mut compound = Compound::default();
        let mut any = false;
        if self.peek() == Some('*') {
            self.pos += 1;
            any = true;
        } else if self.peek().is_some_and(|c| is_ident_char(c) || c == '\\') {
            compound.tag = Some(self.ident()?);
            any = true;
        }
        loop {
            match self.peek() {
                Some('#') => {
                    self.pos += 1;
                    compound.simples.push(Simple::Id(self.ident()?));
                }
                Some('.') => {
                    self.pos += 1;
                    compound.simples.push(Simple::Class(self.ident()?));
                }
                Some('[') => {
                    self.pos += 1;
                    compound.simples.push(self.attribute()?);
                }
                Some(':') => {
                    self.pos += 1;
                    if self.peek() == Some(':') {
                        return Err(self.err("pseudo-elements are not supported"));
                    }
                    compound.simples.push(self.pseudo()?);
                }
                Some('|') => return Err(self.err("namespaces are not supported")),
                _ => break,
            }
            any = true;
        }
        if !any {
            return Err(self.err("expected a selector"));
        }
        Ok(compound)
    }

    fn attribute(&mut self) -> Result<Simple, String> {
        self.skip_ws();
        let name = self.ident()?;
        self.skip_ws();
        let op = match (self.peek(), self.peek_at(1)) {
            (Some(']'), _) => {
                self.pos += 1;
                return Ok(Simple::Attr {
                    name,
                    op: AttrOp::Exists,
                    value: String::new(),
                    ignore_case: false,
                });
            }
            (Some('='), _) => {
                self.pos += 1;
                AttrOp::Equals
            }
            (Some('~'), Some('=')) => {
                self.pos += 2;
                AttrOp::Element
            }
            (Some('|'), Some('=')) => {
                self.pos += 2;
                AttrOp::Hyphen
            }
            (Some('^'), Some('=')) => {
                self.pos += 2;
                AttrOp::Start
            }
            (Some('$'), Some('=')) => {
                self.pos += 2;
                AttrOp::End
            }
            (Some('*'), Some('=')) => {
                self.pos += 2;
                AttrOp::Any
            }
            (Some('!'), Some('=')) => {
                self.pos += 2;
                AttrOp::Not
            }
            _ => return Err(self.err("bad attribute selector")),
        };
        self.skip_ws();
        let value = if let Some(q @ ('"' | '\'')) = self.peek() {
            self.pos += 1;
            self.quoted(q)?
        } else {
            let mut out = String::new();
            loop {
                match self.peek() {
                    Some('\\') => {
                        self.pos += 1;
                        out.push(self.escape()?);
                    }
                    Some(c) if !is_ws(c) && c != ']' => {
                        out.push(c);
                        self.pos += 1;
                    }
                    _ => break,
                }
            }
            out
        };
        self.skip_ws();
        let mut ignore_case = false;
        if let Some(flag) = self.peek()
            && matches!(flag.to_ascii_lowercase(), 'i' | 's')
            && self.chars[self.pos + 1..]
                .iter()
                .copied()
                .find(|c| !is_ws(*c))
                == Some(']')
        {
            ignore_case = flag.eq_ignore_ascii_case(&'i');
            self.pos += 1;
            self.skip_ws();
        }
        if self.bump() != Some(']') {
            return Err(self.err("expected ']'"));
        }
        Ok(Simple::Attr {
            name,
            op,
            value,
            ignore_case,
        })
    }

    /// The raw text up to the `)` that closes the argument list, honouring
    /// nested parentheses and quotes.
    fn raw_argument(&mut self) -> Result<String, String> {
        let mut depth = 0;
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(self.err("expected ')'")),
                Some(')') if depth == 0 => break,
                Some(c @ ('"' | '\'')) => {
                    out.push(c);
                    out.push_str(&self.quoted(c)?);
                    out.push(c);
                }
                Some('(') => {
                    depth += 1;
                    out.push('(');
                }
                Some(')') => {
                    depth -= 1;
                    out.push(')');
                }
                Some(c) => out.push(c),
            }
        }
        Ok(out)
    }

    fn pseudo(&mut self) -> Result<Simple, String> {
        let name = self.ident()?.to_ascii_lowercase();
        let has_args = self.peek() == Some('(');
        if has_args {
            self.pos += 1;
        }
        let selector_args = |this: &mut Self, relative: bool| -> Result<SelectorList, String> {
            if !has_args {
                return Err(this.err(&format!(":{name} requires an argument")));
            }
            let list = this.list(relative)?;
            this.skip_ws();
            if this.bump() != Some(')') {
                return Err(this.err("expected ')'"));
            }
            Ok(list)
        };
        let simple = match name.as_str() {
            "not" => Simple::Not(selector_args(self, false)?),
            "is" | "matches" | "where" => Simple::Is(selector_args(self, false)?),
            "has" => Simple::Has(selector_args(self, true)?),
            "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" => {
                if !has_args {
                    return Err(self.err(&format!(":{name} requires an argument")));
                }
                let raw = self.raw_argument()?;
                let (a, b) = parse_nth(&raw)
                    .ok_or_else(|| self.err(&format!("bad :{name} argument {raw:?}")))?;
                let kind = match name.as_str() {
                    "nth-child" => NthKind::Child,
                    "nth-last-child" => NthKind::LastChild,
                    "nth-of-type" => NthKind::OfType,
                    _ => NthKind::LastOfType,
                };
                Simple::Nth { kind, a, b }
            }
            "contains" | "icontains" => {
                if !has_args {
                    return Err(self.err(&format!(":{name} requires an argument")));
                }
                let raw = self.raw_argument()?;
                let text = strip_quotes(raw.trim());
                if name == "contains" {
                    Simple::Contains(text)
                } else {
                    Simple::IContains(text)
                }
            }
            "first-child" | "last-child" | "only-child" | "first-of-type" | "last-of-type"
            | "only-of-type" | "empty" => {
                if has_args {
                    return Err(self.err(&format!(":{name} takes no argument")));
                }
                match name.as_str() {
                    "first-child" => Simple::FirstChild,
                    "last-child" => Simple::LastChild,
                    "only-child" => Simple::OnlyChild,
                    "first-of-type" => Simple::FirstOfType,
                    "last-of-type" => Simple::LastOfType,
                    "only-of-type" => Simple::OnlyOfType,
                    _ => Simple::Empty,
                }
            }
            _ => return Err(self.err(&format!("unsupported pseudo-class :{name}"))),
        };
        Ok(simple)
    }
}

/// css-what strips the quotes of a quoted pseudo-class argument.
fn strip_quotes(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// nth-check's grammar: `odd`, `even`, `An+B`, `An`, `B`, `n`, `-n+B`.
fn parse_nth(raw: &str) -> Option<(i64, i64)> {
    let s: String = raw
        .chars()
        .filter(|c| !is_ws(*c))
        .collect::<String>()
        .to_ascii_lowercase();
    match s.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    if let Some(n_pos) = s.find('n') {
        let a_text = &s[..n_pos];
        let a = match a_text {
            "" | "+" => 1,
            "-" => -1,
            _ => a_text.parse().ok()?,
        };
        let b_text = &s[n_pos + 1..];
        let b = if b_text.is_empty() {
            0
        } else {
            if !b_text.starts_with('+') && !b_text.starts_with('-') {
                return None;
            }
            b_text.parse().ok()?
        };
        Some((a, b))
    } else {
        Some((0, s.parse().ok()?))
    }
}

impl SelectorList {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut p = SelParser {
            chars: text.chars().collect(),
            pos: 0,
            text,
        };
        p.skip_ws();
        if p.peek().is_none() {
            return Err(p.err("empty selector"));
        }
        let list = p.list(false)?;
        p.skip_ws();
        if let Some(c) = p.peek() {
            return Err(p.err(&format!("unexpected {c:?}")));
        }
        Ok(list)
    }

    /// cheerio's `$root.find(selector)`: every element of the document that
    /// matches, in document order, each once.
    pub fn select_all(&self, dom: &Dom) -> Vec<NodeId> {
        dom.elements()
            .into_iter()
            .filter(|&id| self.matches(dom, El::Node(id)))
            .collect()
    }

    fn matches(&self, dom: &Dom, el: El) -> bool {
        self.complexes
            .iter()
            .any(|c| matches_from(dom, el, &c.parts, None))
    }
}

// ---- matching ---------------------------------------------------------------

/// An element as the matcher sees it: a document node, or cheerio's
/// `<root>` wrapper around the document.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum El {
    Node(NodeId),
    Wrapper,
}

fn parent_element(dom: &Dom, el: El) -> Option<El> {
    match el {
        El::Wrapper => None,
        El::Node(id) => match dom.node(id).parent {
            Some(p) if dom.is_tag(p) => Some(El::Node(p)),
            Some(_) => None,
            None => Some(El::Wrapper),
        },
    }
}

/// The element siblings list `el` belongs to, in order.
fn element_siblings(dom: &Dom, el: El) -> Vec<El> {
    match el {
        El::Wrapper => vec![El::Wrapper],
        El::Node(id) => dom
            .siblings(id)
            .iter()
            .filter(|&&s| dom.is_tag(s))
            .map(|&s| El::Node(s))
            .collect(),
    }
}

fn name_of(dom: &Dom, el: El) -> &str {
    match el {
        El::Wrapper => "root",
        El::Node(id) => dom.tag_name(id).unwrap_or(""),
    }
}

fn descendant_elements(dom: &Dom, el: El) -> Vec<El> {
    match el {
        El::Wrapper => dom.elements().into_iter().map(El::Node).collect(),
        El::Node(id) => dom.descendants(id).into_iter().map(El::Node).collect(),
    }
}

fn text_of(dom: &Dom, el: El) -> String {
    match el {
        El::Wrapper => dom.roots.iter().map(|&r| dom.text(r)).collect(),
        El::Node(id) => dom.text(id),
    }
}

/// Does `el` match the compound at the end of `parts`, with the compounds
/// to its left satisfied through their combinators? With an `anchor`, the
/// leftmost compound's match must relate to the anchor element through
/// the leading combinator (the `:has(...)` case).
fn matches_from(dom: &Dom, el: El, parts: &[(Combinator, Compound)], anchor: Option<El>) -> bool {
    let Some(((comb, compound), rest)) = parts.split_last() else {
        return true;
    };
    if !matches_compound(dom, el, compound) {
        return false;
    }
    if rest.is_empty() {
        return match anchor {
            None => true,
            Some(a) => related(dom, a, *comb, el),
        };
    }
    match comb {
        Combinator::Child => {
            parent_element(dom, el).is_some_and(|p| matches_from(dom, p, rest, anchor))
        }
        Combinator::Descendant => {
            let mut p = parent_element(dom, el);
            while let Some(parent) = p {
                if matches_from(dom, parent, rest, anchor) {
                    return true;
                }
                p = parent_element(dom, parent);
            }
            false
        }
        Combinator::NextSibling => {
            prev_element_sibling(dom, el).is_some_and(|s| matches_from(dom, s, rest, anchor))
        }
        Combinator::SubsequentSibling => {
            let siblings = element_siblings(dom, el);
            let me = siblings.iter().position(|&s| s == el).unwrap_or(0);
            siblings[..me]
                .iter()
                .any(|&s| matches_from(dom, s, rest, anchor))
        }
    }
}

fn prev_element_sibling(dom: &Dom, el: El) -> Option<El> {
    let siblings = element_siblings(dom, el);
    let me = siblings.iter().position(|&s| s == el)?;
    me.checked_sub(1).map(|i| siblings[i])
}

/// Is `el` in the relation `comb` to `anchor` (its child, descendant, next
/// sibling or later sibling)?
fn related(dom: &Dom, anchor: El, comb: Combinator, el: El) -> bool {
    match comb {
        Combinator::Child => parent_element(dom, el) == Some(anchor),
        Combinator::Descendant => {
            let mut p = parent_element(dom, el);
            while let Some(parent) = p {
                if parent == anchor {
                    return true;
                }
                p = parent_element(dom, parent);
            }
            false
        }
        Combinator::NextSibling => prev_element_sibling(dom, el) == Some(anchor),
        Combinator::SubsequentSibling => {
            let siblings = element_siblings(dom, el);
            let me = siblings.iter().position(|&s| s == el).unwrap_or(0);
            siblings[..me].contains(&anchor)
        }
    }
}

fn matches_compound(dom: &Dom, el: El, compound: &Compound) -> bool {
    if let Some(tag) = &compound.tag
        && name_of(dom, el) != tag
    {
        return false;
    }
    compound.simples.iter().all(|s| matches_simple(dom, el, s))
}

fn attr_of<'a>(dom: &'a Dom, el: El, name: &str) -> Option<&'a str> {
    match el {
        El::Wrapper => None,
        El::Node(id) => dom.attr(id, name),
    }
}

fn matches_simple(dom: &Dom, el: El, simple: &Simple) -> bool {
    match simple {
        Simple::Id(id) => attr_of(dom, el, "id") == Some(id.as_str()),
        Simple::Class(class) => attr_of(dom, el, "class")
            .is_some_and(|v| v.split([' ', '\t', '\n', '\r', '\x0c']).any(|c| c == class)),
        Simple::Attr {
            name,
            op,
            value,
            ignore_case,
        } => matches_attr(attr_of(dom, el, name), *op, value, *ignore_case),
        Simple::Not(list) => !list
            .complexes
            .iter()
            .any(|c| matches_from(dom, el, &c.parts, None)),
        Simple::Is(list) => list
            .complexes
            .iter()
            .any(|c| matches_from(dom, el, &c.parts, None)),
        Simple::Has(list) => list.complexes.iter().any(|c| {
            let leading = c.parts.first().map_or(Combinator::Descendant, |p| p.0);
            let candidates = match leading {
                Combinator::Child | Combinator::Descendant => descendant_elements(dom, el),
                Combinator::NextSibling | Combinator::SubsequentSibling => {
                    let siblings = element_siblings(dom, el);
                    let me = siblings.iter().position(|&s| s == el).unwrap_or(0);
                    let mut out = Vec::new();
                    for &s in &siblings[me + 1..] {
                        out.push(s);
                        out.extend(descendant_elements(dom, s));
                    }
                    out
                }
            };
            candidates
                .into_iter()
                .any(|cand| matches_from(dom, cand, &c.parts, Some(el)))
        }),
        Simple::Nth { kind, a, b } => {
            let siblings = element_siblings(dom, el);
            let same_type = matches!(kind, NthKind::OfType | NthKind::LastOfType);
            let name = name_of(dom, el);
            let considered: Vec<El> = siblings
                .into_iter()
                .filter(|&s| !same_type || name_of(dom, s) == name)
                .collect();
            let Some(index) = considered.iter().position(|&s| s == el) else {
                return false;
            };
            let pos = if matches!(kind, NthKind::LastChild | NthKind::LastOfType) {
                considered.len() - index
            } else {
                index + 1
            };
            nth_matches(*a, *b, i64::try_from(pos).unwrap_or(i64::MAX))
        }
        Simple::FirstChild => prev_element_sibling(dom, el).is_none(),
        Simple::LastChild => element_siblings(dom, el).last() == Some(&el),
        Simple::OnlyChild => element_siblings(dom, el).len() == 1,
        Simple::FirstOfType => {
            let name = name_of(dom, el);
            element_siblings(dom, el)
                .into_iter()
                .find(|&s| name_of(dom, s) == name)
                == Some(el)
        }
        Simple::LastOfType => {
            let name = name_of(dom, el);
            element_siblings(dom, el)
                .into_iter()
                .rev()
                .find(|&s| name_of(dom, s) == name)
                == Some(el)
        }
        Simple::OnlyOfType => {
            let name = name_of(dom, el);
            element_siblings(dom, el)
                .into_iter()
                .filter(|&s| name_of(dom, s) == name)
                .count()
                == 1
        }
        Simple::Empty => {
            let children: &[NodeId] = match el {
                El::Wrapper => &dom.roots,
                El::Node(id) => &dom.node(id).children,
            };
            !children
                .iter()
                .any(|&c| dom.is_tag(c) || !dom.text(c).is_empty())
        }
        Simple::Contains(text) => text_of(dom, el).contains(text.as_str()),
        Simple::IContains(text) => text_of(dom, el)
            .to_lowercase()
            .contains(&text.to_lowercase()),
    }
}

/// nth-check: does 1-based position `pos` satisfy `An+B`?
const fn nth_matches(a: i64, b: i64, pos: i64) -> bool {
    if a == 0 {
        return pos == b;
    }
    let diff = pos - b;
    if a > 0 {
        diff >= 0 && diff % a == 0
    } else {
        diff <= 0 && diff % a == 0
    }
}

fn matches_attr(actual: Option<&str>, op: AttrOp, value: &str, ignore_case: bool) -> bool {
    let fold = |s: &str| {
        if ignore_case {
            s.to_lowercase()
        } else {
            s.to_string()
        }
    };
    let value = fold(value);
    match op {
        AttrOp::Exists => actual.is_some(),
        AttrOp::Not => match actual {
            None => !value.is_empty(),
            Some(a) => {
                if value.is_empty() {
                    !a.is_empty()
                } else {
                    fold(a) != value
                }
            }
        },
        _ => {
            let Some(actual) = actual else {
                return false;
            };
            let actual = fold(actual);
            match op {
                AttrOp::Equals => actual == value,
                AttrOp::Element => {
                    !value.is_empty()
                        && !value.chars().any(is_ws)
                        && actual.split(is_ws).any(|part| part == value)
                }
                AttrOp::Hyphen => actual == value || actual.starts_with(&format!("{value}-")),
                AttrOp::Start => !value.is_empty() && actual.starts_with(&value),
                AttrOp::End => !value.is_empty() && actual.ends_with(&value),
                AttrOp::Any => !value.is_empty() && actual.contains(&value),
                AttrOp::Exists | AttrOp::Not => unreachable!("handled above"),
            }
        }
    }
}

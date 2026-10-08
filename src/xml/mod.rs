//! An XML well-formedness checker and XSD validator, in-process.
//!
//! This is the engine behind `processor.checker.ixmllint`, the Rust
//! alternative to `xmllint --noout [--schema x.xsd]`. It reports what xmllint
//! reports, at the line xmllint reports it, with xmllint's verdict: a
//! well-formedness error fails the file, a namespace error or a parser
//! warning is printed and the file passes (`xmllint` exits 0 on those), a
//! schema violation fails the file.
//!
//! `parser` turns bytes into a [`Document`] tree plus the problems found on
//! the way; `xsd` reads a schema from such a tree and validates another one.

mod parser;
#[cfg(test)]
mod tests;
mod xsd;

use std::fmt;

pub use xsd::Schema;

/// How xmllint classifies a problem; the words are xmllint's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A well-formedness error: the file is not XML. Fatal.
    ParserError,
    /// Something xmllint tolerates with a message, like an entity that an
    /// external DTD might define. Not fatal.
    ParserWarning,
    /// A namespace well-formedness problem, like an undeclared prefix.
    /// xmllint prints it and still exits 0.
    NamespaceError,
    /// A namespace nit xmllint only warns about: a relative namespace URI.
    NamespaceWarning,
    /// A violation of the XSD schema. Fatal.
    SchemaError,
}

impl Kind {
    /// Whether xmllint's exit status would be non-zero for this problem.
    pub const fn is_fatal(self) -> bool {
        matches!(self, Self::ParserError | Self::SchemaError)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ParserError => "parser error",
            Self::ParserWarning => "parser warning",
            Self::NamespaceError => "namespace error",
            Self::NamespaceWarning => "namespace warning",
            Self::SchemaError => "Schemas validity error",
        })
    }
}

/// One problem in one file, in xmllint's `line: kind : message` shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// 1-based line, as xmllint counts it.
    pub line: u32,
    pub kind: Kind,
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} : {}", self.line, self.kind, self.message)
    }
}

/// A parsed element: names resolved to namespaces, attributes with their
/// values normalized, the namespace declarations in scope, and children in
/// document order. Namespace declarations are not attributes here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub prefix: String,
    pub local: String,
    /// The namespace the element is in, `None` for no namespace (also when
    /// the prefix is undeclared, which is reported as a namespace error).
    pub namespace: Option<String>,
    pub attributes: Vec<Attribute>,
    /// Every `(prefix, uri)` declared on this element or an ancestor,
    /// innermost last; `""` is the default namespace's prefix. The implicit
    /// `xml` binding is included.
    pub scope: Vec<(String, String)>,
    pub children: Vec<Child>,
    /// 1-based line the start tag begins on (what xmllint quotes in
    /// "Opening and ending tag mismatch: b line 2").
    pub line: u32,
    /// 1-based line the start tag ends on: libxml2 stamps the node with
    /// this one, so schema problems are reported here.
    pub end_line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub prefix: String,
    pub local: String,
    pub namespace: Option<String>,
    /// The value with references expanded and whitespace normalized as the
    /// XML spec does for every attribute (tab, newline, return to space).
    pub value: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Child {
    Element(Element),
    /// Character data (text, CDATA, expanded entities), as written.
    Text(String),
}

impl Element {
    /// The namespace URI the given prefix resolves to in this element's
    /// scope. `""` asks for the default namespace.
    pub fn lookup_prefix(&self, prefix: &str) -> Option<&str> {
        self.scope
            .iter()
            .rev()
            .find(|(p, _)| p == prefix)
            .map(|(_, uri)| uri.as_str())
    }

    /// The element's text content: its direct text children concatenated.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for child in &self.children {
            if let Child::Text(t) = child {
                out.push_str(t);
            }
        }
        out
    }

    pub fn child_elements(&self) -> impl Iterator<Item = &Self> {
        self.children.iter().filter_map(|c| match c {
            Child::Element(e) => Some(e),
            Child::Text(_) => None,
        })
    }

    pub fn attribute(&self, namespace: Option<&str>, local: &str) -> Option<&Attribute> {
        self.attributes
            .iter()
            .find(|a| a.local == local && a.namespace.as_deref() == namespace)
    }

    /// `{namespace}local`, the notation xmllint uses in schema messages.
    pub fn expanded_name(&self) -> String {
        expanded_name(self.namespace.as_deref(), &self.local)
    }
}

/// `{namespace}local`, or just `local` when there is no namespace.
pub fn expanded_name(namespace: Option<&str>, local: &str) -> String {
    match namespace {
        Some(ns) => format!("{{{ns}}}{local}"),
        None => local.to_string(),
    }
}

/// A parsed file: the problems found, and the tree when there was a root
/// element to build it from.
#[derive(Debug, Default)]
pub struct Document {
    pub problems: Vec<Problem>,
    pub root: Option<Element>,
}

impl Document {
    /// Whether xmllint would exit non-zero for this file.
    pub fn is_well_formed(&self) -> bool {
        !self.problems.iter().any(|p| p.kind.is_fatal())
    }
}

/// Parses a file's bytes: decodes them as the XML declaration says (UTF-8 by
/// default, Latin-1 and UTF-16 also understood), then checks well-formedness
/// and namespace well-formedness while building the tree.
pub fn parse(bytes: &[u8]) -> Document {
    match parser::decode(bytes) {
        Ok(text) => parser::parse(&text),
        Err(problem) => Document {
            problems: vec![problem],
            root: None,
        },
    }
}

/// Validates a well-formed document against a schema, as `xmllint --schema`
/// does. The problems are all `Kind::SchemaError`; `Err` is a defect in the
/// schema itself (a reference to an undefined type, a circular derivation).
pub fn validate(document: &Element, schema: &Schema) -> anyhow::Result<Vec<Problem>> {
    xsd::validate(document, schema)
}

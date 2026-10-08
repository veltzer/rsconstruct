//! Bytes to an [`Element`] tree, reporting what `xmllint --noout` reports.
//!
//! The tokenizer is `xmlparser` (the one under roxmltree). It handles the
//! lexical level: names, quotes, comments, CDATA, the XML declaration, the
//! characters XML allows. Everything xmllint checks above that level is here:
//! tag matching, duplicate attributes, entity and character references, the
//! internal DTD subset's entities, namespace declarations and prefixes, and
//! the document's shape (one root, nothing after it).

use std::collections::HashMap;

use xmlparser::{
    ElementEnd, EntityDefinition, Error as TokenError, StreamError, Token, Tokenizer, XmlCharExt,
};

use super::{Attribute, Child, Document, Element, Kind, Problem};

const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Nested entity expansions beyond this depth are a loop (libxml2 uses a
/// similar bound rather than detecting cycles exactly).
const MAX_ENTITY_DEPTH: u8 = 40;

/// Decodes a file's bytes to text as xmllint would: a byte-order mark, else
/// the XML declaration's `encoding`, else UTF-8. UTF-8, UTF-16, Latin-1 and
/// US-ASCII are understood; another declared encoding is an error, where
/// xmllint would consult iconv.
pub(super) fn decode(bytes: &[u8]) -> Result<String, Problem> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return decode_utf8(rest);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_utf16(&bytes[2..], u16::from_be_bytes);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16(&bytes[2..], u16::from_le_bytes);
    }
    let declared = declared_encoding(bytes).map(str::to_ascii_lowercase);
    match declared.as_deref() {
        None | Some("utf-8" | "utf8") => decode_utf8(bytes),
        Some("iso-8859-1" | "iso_8859-1" | "latin1" | "latin-1" | "l1" | "cp819" | "iso8859-1") => {
            Ok(bytes.iter().map(|&b| char::from(b)).collect())
        }
        Some("us-ascii" | "ascii") => match bytes.iter().position(|&b| b >= 0x80) {
            None => decode_utf8(bytes),
            Some(offset) => Err(Problem {
                line: line_of_byte(bytes, offset),
                kind: Kind::ParserError,
                message: "Invalid bytes in character encoding".to_string(),
            }),
        },
        Some(other) => Err(Problem {
            line: 1,
            kind: Kind::ParserError,
            message: format!("Unsupported encoding {other}"),
        }),
    }
}

fn decode_utf8(bytes: &[u8]) -> Result<String, Problem> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(text.to_string()),
        Err(e) => Err(Problem {
            line: line_of_byte(bytes, e.valid_up_to()),
            kind: Kind::ParserError,
            message: "Invalid bytes in character encoding".to_string(),
        }),
    }
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> Result<String, Problem> {
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|c| unit([c[0], *c.get(1).unwrap_or(&0)]))
        .collect();
    String::from_utf16(&units).map_err(|_| Problem {
        line: 1,
        kind: Kind::ParserError,
        message: "Invalid bytes in character encoding".to_string(),
    })
}

/// The `encoding` pseudo-attribute of an XML declaration at the very start
/// of the bytes, if any. Read on the raw bytes, before decoding, because the
/// declaration is ASCII by definition.
fn declared_encoding(bytes: &[u8]) -> Option<&str> {
    let head = bytes.strip_prefix(b"<?xml")?;
    let end = head.windows(2).position(|w| w == b"?>")?;
    let decl = std::str::from_utf8(&head[..end]).ok()?;
    let after = decl.split("encoding").nth(1)?;
    let after = after.trim_start().strip_prefix('=')?.trim_start();
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    after[1..].split(quote).next()
}

fn line_of_byte(bytes: &[u8], offset: usize) -> u32 {
    // Segments between newlines: one more than the newlines, so the 1-based line.
    u32::try_from(bytes[..offset].split(|&b| b == b'\n').count()).unwrap_or(u32::MAX)
}

/// Byte offsets where each line starts, for offset-to-line lookups.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Self { starts }
    }

    fn line_of(&self, offset: usize) -> u32 {
        u32::try_from(self.starts.partition_point(|&s| s <= offset)).unwrap_or(u32::MAX)
    }

    fn last_line(&self) -> u32 {
        u32::try_from(self.starts.len()).unwrap_or(u32::MAX)
    }

    /// The byte offset of a tokenizer position (1-based row, 1-based column
    /// in characters).
    fn offset_of(&self, text: &str, pos: xmlparser::TextPos) -> usize {
        let row = usize::try_from(pos.row).unwrap_or(1).max(1);
        let start = self.starts.get(row - 1).copied().unwrap_or(text.len());
        let col = usize::try_from(pos.col).unwrap_or(1).max(1) - 1;
        text[start..]
            .char_indices()
            .nth(col)
            .map_or(text.len(), |(i, _)| start + i)
    }
}

enum Entity {
    /// Replacement text, as written in the declaration (references in it
    /// are expanded when the entity is used).
    Internal(String),
    /// A SYSTEM or PUBLIC entity. Without `--noent`/`--loaddtd`, xmllint
    /// leaves a reference to one unexpanded and says nothing.
    External,
}

/// An element whose start tag is being read: attributes arrive one token at
/// a time after `ElementStart`, until `ElementEnd`.
struct Pending<'a> {
    prefix: &'a str,
    local: &'a str,
    line: u32,
    /// The line the start tag ends on. libxml2 checks duplicate attributes
    /// and resolves prefixes once the whole tag is read, so it reports those
    /// problems here, not on the attribute's or the tag's first line.
    end_line: u32,
    attributes: Vec<RawAttribute<'a>>,
}

struct RawAttribute<'a> {
    prefix: &'a str,
    local: &'a str,
    value: &'a str,
    line: u32,
}

struct Parser<'a> {
    text: &'a str,
    lines: LineIndex,
    problems: Vec<Problem>,
    entities: HashMap<String, Entity>,
    /// The DOCTYPE names an external subset, so an undeclared entity might
    /// be declared there: xmllint then warns instead of failing.
    external_subset: bool,
    /// Open elements, innermost last, each holding the children read so far.
    stack: Vec<Element>,
    root: Option<Element>,
    root_seen: bool,
}

pub(super) fn parse(text: &str) -> Document {
    let mut parser = Parser {
        text,
        lines: LineIndex::new(text),
        problems: Vec::new(),
        entities: HashMap::new(),
        external_subset: false,
        stack: Vec::new(),
        root: None,
        root_seen: false,
    };
    parser.run();
    Document {
        problems: parser.problems,
        root: parser.root,
    }
}

impl<'a> Parser<'a> {
    fn report(&mut self, line: u32, kind: Kind, message: impl Into<String>) {
        self.problems.push(Problem {
            line,
            kind,
            message: message.into(),
        });
    }

    fn error(&mut self, line: u32, message: impl Into<String>) {
        self.report(line, Kind::ParserError, message);
    }

    fn has_fatal(&self) -> bool {
        self.problems.iter().any(|p| p.kind.is_fatal())
    }

    fn run(&mut self) {
        let mut pending: Option<Pending<'a>> = None;
        let mut stopped = false;
        for item in Tokenizer::from(self.text) {
            let token = match item {
                Ok(token) => token,
                Err(e) => {
                    self.tokenizer_error(&e);
                    stopped = true;
                    break;
                }
            };
            match token {
                Token::Declaration { version, .. } => {
                    if version.as_str() != "1.0" {
                        self.report(
                            1,
                            Kind::ParserWarning,
                            format!("Unsupported version '{}'", version.as_str()),
                        );
                    }
                }
                Token::ProcessingInstruction { .. }
                | Token::Comment { .. }
                | Token::DtdEnd { .. } => {}
                Token::DtdStart { external_id, .. } | Token::EmptyDtd { external_id, .. } => {
                    self.external_subset = external_id.is_some();
                }
                Token::EntityDeclaration {
                    name, definition, ..
                } => {
                    // The first declaration of an entity is binding.
                    self.entities
                        .entry(name.as_str().to_string())
                        .or_insert_with(|| match definition {
                            EntityDefinition::EntityValue(value) => {
                                Entity::Internal(value.as_str().to_string())
                            }
                            EntityDefinition::ExternalId(_) => Entity::External,
                        });
                }
                Token::ElementStart {
                    prefix,
                    local,
                    span,
                } => {
                    let line = self.lines.line_of(span.start());
                    pending = Some(Pending {
                        prefix: prefix.as_str(),
                        local: local.as_str(),
                        line,
                        end_line: line,
                        attributes: Vec::new(),
                    });
                }
                Token::Attribute {
                    prefix,
                    local,
                    value,
                    span,
                } => {
                    if let Some(p) = pending.as_mut() {
                        p.attributes.push(RawAttribute {
                            prefix: prefix.as_str(),
                            local: local.as_str(),
                            value: value.as_str(),
                            line: self.lines.line_of(span.start()),
                        });
                    }
                }
                Token::ElementEnd { end, span } => match end {
                    ElementEnd::Open | ElementEnd::Empty => {
                        if let Some(mut p) = pending.take() {
                            p.end_line = self.lines.line_of(span.start());
                            let element = self.open_element(&p);
                            if end == ElementEnd::Open {
                                self.stack.push(element);
                            } else {
                                self.attach(element);
                            }
                        }
                    }
                    ElementEnd::Close(prefix, local) => {
                        self.close_element(
                            prefix.as_str(),
                            local.as_str(),
                            self.lines.line_of(span.start()),
                        );
                    }
                },
                Token::Text { text } => {
                    let line = self.lines.line_of(text.start());
                    let expanded = self.expand_references(text.as_str(), line, 0);
                    self.add_child(Child::Text(expanded));
                }
                Token::Cdata { text, .. } => {
                    self.add_child(Child::Text(text.as_str().to_string()));
                }
            }
        }
        if stopped {
            return;
        }
        if let Some(open) = self.stack.last() {
            // libxml2 reports an unfinished document only when nothing else
            // went wrong: after a tag mismatch, what is still open is moot.
            if !self.has_fatal() {
                let message = format!(
                    "Premature end of data in tag {} line {}",
                    qualified(&open.prefix, &open.local),
                    open.line
                );
                self.error(self.lines.last_line(), message);
            }
            return;
        }
        if !self.root_seen {
            if self.text.trim().is_empty() {
                self.error(1, "Document is empty");
            } else {
                self.error(1, "Start tag expected, '<' not found");
            }
        }
    }

    /// Builds the element from its start tag: namespace declarations,
    /// duplicate attributes, prefix resolution, attribute values.
    fn open_element(&mut self, pending: &Pending<'a>) -> Element {
        let mut scope: Vec<(String, String)> = match self.stack.last() {
            Some(parent) => parent.scope.clone(),
            None => vec![("xml".to_string(), XML_NS.to_string())],
        };

        // Duplicate attributes, by the name as written.
        for (i, attr) in pending.attributes.iter().enumerate() {
            let dup = pending.attributes[..i]
                .iter()
                .any(|a| a.prefix == attr.prefix && a.local == attr.local);
            if dup {
                let message = format!("Attribute {} redefined", qualified(attr.prefix, attr.local));
                self.error(pending.end_line, message);
            }
        }

        for attr in &pending.attributes {
            let decl_prefix = if attr.prefix == "xmlns" {
                attr.local
            } else if attr.prefix.is_empty() && attr.local == "xmlns" {
                ""
            } else {
                continue;
            };
            let uri = self.expand_references(attr.value, attr.line, 0);
            let uri = normalize_attribute_value(&uri);
            if decl_prefix == "xml" {
                if uri != XML_NS {
                    self.report(
                        attr.line,
                        Kind::NamespaceError,
                        "xml namespace prefix mapped to wrong URI",
                    );
                }
                continue;
            }
            if uri == XML_NS {
                self.report(
                    attr.line,
                    Kind::NamespaceError,
                    "xml namespace URI mapped to wrong prefix",
                );
                continue;
            }
            if decl_prefix == "xmlns" {
                self.report(
                    attr.line,
                    Kind::NamespaceError,
                    "xmlns: redefining the xmlns prefix is forbidden",
                );
                continue;
            }
            if uri.is_empty() {
                if !decl_prefix.is_empty() {
                    self.report(
                        attr.line,
                        Kind::NamespaceError,
                        format!("xmlns:{decl_prefix}: Empty XML namespace is not allowed"),
                    );
                    continue;
                }
            } else if decl_prefix.is_empty() && !is_absolute_uri(&uri) {
                // libxml2 warns about a relative URI on the default namespace
                // only; for a prefixed one it needs --pedantic.
                self.report(
                    attr.line,
                    Kind::NamespaceWarning,
                    format!("xmlns: URI {uri} is not absolute"),
                );
            }
            scope.push((decl_prefix.to_string(), uri));
        }

        let namespace = resolve_prefix(&scope, pending.prefix);
        if namespace.is_none() && !pending.prefix.is_empty() {
            self.report(
                pending.end_line,
                Kind::NamespaceError,
                format!(
                    "Namespace prefix {} on {} is not defined",
                    pending.prefix, pending.local
                ),
            );
        }

        let mut attributes: Vec<Attribute> = Vec::new();
        for attr in &pending.attributes {
            if attr.prefix == "xmlns" || (attr.prefix.is_empty() && attr.local == "xmlns") {
                continue;
            }
            let attr_ns = if attr.prefix.is_empty() {
                None
            } else {
                let ns = resolve_prefix(&scope, attr.prefix);
                if ns.is_none() {
                    self.report(
                        pending.end_line,
                        Kind::NamespaceError,
                        format!(
                            "Namespace prefix {} for {} on {} is not defined",
                            attr.prefix, attr.local, pending.local
                        ),
                    );
                }
                ns
            };
            if let Some(ns) = &attr_ns {
                let same_expanded = attributes.iter().any(|a| {
                    a.local == attr.local
                        && a.namespace.as_deref() == Some(ns)
                        && a.prefix != attr.prefix
                });
                if same_expanded {
                    let message =
                        format!("Namespaced Attribute {} in '{ns}' redefined", attr.local);
                    self.error(pending.end_line, message);
                }
            }
            let value = self.expand_references(attr.value, attr.line, 0);
            attributes.push(Attribute {
                prefix: attr.prefix.to_string(),
                local: attr.local.to_string(),
                namespace: attr_ns,
                value: normalize_attribute_value(&value),
                line: attr.line,
            });
        }

        self.root_seen = true;
        Element {
            prefix: pending.prefix.to_string(),
            local: pending.local.to_string(),
            namespace,
            attributes,
            scope,
            children: Vec::new(),
            line: pending.line,
            end_line: pending.end_line,
        }
    }

    fn close_element(&mut self, prefix: &str, local: &str, line: u32) {
        let Some(open) = self.stack.pop() else {
            if !self.has_fatal() {
                self.error(line, "Extra content at the end of the document");
            }
            return;
        };
        if open.prefix != prefix || open.local != local {
            let message = format!(
                "Opening and ending tag mismatch: {} line {} and {}",
                qualified(&open.prefix, &open.local),
                open.line,
                qualified(prefix, local)
            );
            self.error(line, message);
        }
        self.attach(open);
    }

    /// Puts a finished element into its parent, or makes it the root.
    fn attach(&mut self, element: Element) {
        match self.stack.last_mut() {
            Some(parent) => parent.children.push(Child::Element(element)),
            None => {
                if self.root.is_none() {
                    self.root = Some(element);
                }
            }
        }
    }

    fn add_child(&mut self, child: Child) {
        if let Some(parent) = self.stack.last_mut() {
            parent.children.push(child);
        }
    }

    /// Expands character and entity references in text or an attribute
    /// value, reporting the ones xmllint rejects.
    fn expand_references(&mut self, raw: &str, line: u32, depth: u8) -> String {
        if !raw.contains('&') {
            return raw.to_string();
        }
        let mut out = String::with_capacity(raw.len());
        let mut rest = raw;
        while let Some(amp) = rest.find('&') {
            out.push_str(&rest[..amp]);
            rest = &rest[amp + 1..];
            let Some(semi) = rest.find(';') else {
                self.error(line, "EntityRef: expecting ';'");
                return out;
            };
            let body = &rest[..semi];
            rest = &rest[semi + 1..];
            if let Some(number) = body.strip_prefix('#') {
                self.expand_char_reference(number, line, &mut out);
            } else {
                self.expand_entity_reference(body, line, depth, &mut out);
            }
        }
        out.push_str(rest);
        out
    }

    fn expand_char_reference(&mut self, number: &str, line: u32, out: &mut String) {
        let value = match number.strip_prefix('x') {
            Some(hex) if !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) => {
                u32::from_str_radix(hex, 16).ok()
            }
            Some(_) => {
                self.error(line, "xmlParseCharRef: invalid hexadecimal value");
                return;
            }
            None if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) => {
                number.parse::<u32>().ok()
            }
            None => {
                self.error(line, "xmlParseCharRef: invalid decimal value");
                return;
            }
        };
        match value.and_then(char::from_u32) {
            Some(c) if c.is_xml_char() => out.push(c),
            _ => {
                let shown = value.map_or_else(|| number.to_string(), |v| v.to_string());
                self.error(
                    line,
                    format!("xmlParseCharRef: invalid xmlChar value {shown}"),
                );
            }
        }
    }

    fn expand_entity_reference(&mut self, name: &str, line: u32, depth: u8, out: &mut String) {
        if !is_xml_name(name) {
            self.error(line, "xmlParseEntityRef: no name");
            return;
        }
        match name {
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "amp" => out.push('&'),
            "apos" => out.push('\''),
            "quot" => out.push('"'),
            _ => match self.entities.get(name) {
                Some(Entity::Internal(replacement)) => {
                    if depth >= MAX_ENTITY_DEPTH {
                        self.error(line, "Detected an entity reference loop");
                        return;
                    }
                    let replacement = replacement.clone();
                    let expanded = self.expand_references(&replacement, line, depth + 1);
                    out.push_str(&expanded);
                }
                Some(Entity::External) => {}
                None => {
                    let kind = if self.external_subset {
                        Kind::ParserWarning
                    } else {
                        Kind::ParserError
                    };
                    self.report(line, kind, format!("Entity '{name}' not defined"));
                }
            },
        }
    }

    /// Translates a tokenizer error into xmllint's words at xmllint's line.
    fn tokenizer_error(&mut self, error: &TokenError) {
        let line = error.pos().row;
        let message = match error {
            TokenError::InvalidCharData(StreamError::InvalidCharacterData, _) => {
                "Sequence ']]>' not allowed in content".to_string()
            }
            TokenError::InvalidCharData(StreamError::NonXmlChar(c, _), _) => {
                format!("PCDATA invalid Char value {}", u32::from(*c))
            }
            TokenError::InvalidAttribute(StreamError::InvalidChar(b'<', _, _), _) => {
                "Unescaped '<' not allowed in attributes values".to_string()
            }
            TokenError::InvalidAttribute(StreamError::NonXmlChar(c, _), _) => {
                format!(
                    "invalid character in attribute value: Char value {}",
                    u32::from(*c)
                )
            }
            TokenError::InvalidAttribute(StreamError::InvalidQuote(..), _) => {
                "AttValue: \" or ' expected".to_string()
            }
            TokenError::InvalidAttribute(StreamError::InvalidChar(_, b'=', _), _) => {
                "Specification mandates value for attribute".to_string()
            }
            TokenError::InvalidAttribute(StreamError::InvalidSpace(..), _) => {
                "attributes construct error".to_string()
            }
            TokenError::InvalidComment(
                StreamError::InvalidCommentData | StreamError::InvalidCommentEnd,
                pos,
            ) => {
                // libxml2 quotes the comment up to the offending `--`, at
                // most 50 characters of it.
                let start = self.lines.offset_of(self.text, *pos);
                let body = self.text[start..].strip_prefix("<!--").unwrap_or("");
                let body = body.split_once("--").map_or(body, |(head, _)| head);
                let excerpt: String = body.chars().take(50).collect();
                format!("Double hyphen within comment: <!--{excerpt}")
            }
            TokenError::InvalidComment(StreamError::UnexpectedEndOfStream, _) => {
                "Comment not terminated".to_string()
            }
            TokenError::InvalidCdata(StreamError::UnexpectedEndOfStream, _) => {
                "CData section not finished".to_string()
            }
            TokenError::InvalidElement(StreamError::InvalidName, _) => {
                "StartTag: invalid element name".to_string()
            }
            TokenError::InvalidDeclaration(cause, _) => {
                format!("parsing XML declaration: {cause}")
            }
            TokenError::InvalidPI(cause, _) => format!("ParsePI: {cause}"),
            TokenError::InvalidDoctype(cause, _) | TokenError::InvalidEntity(cause, _) => {
                format!("DOCTYPE: {cause}")
            }
            TokenError::UnknownToken(pos) => {
                let offset = self.lines.offset_of(self.text, *pos);
                let here = &self.text[offset..];
                if here.starts_with("<?xml") {
                    "XML declaration allowed only at the start of the document".to_string()
                } else if self.stack.is_empty() && self.root_seen {
                    if self.has_fatal() {
                        return;
                    }
                    "Extra content at the end of the document".to_string()
                } else if !self.root_seen {
                    "Start tag expected, '<' not found".to_string()
                } else {
                    "StartTag: invalid element name".to_string()
                }
            }
            TokenError::InvalidElement(StreamError::UnexpectedEndOfStream, _)
            | TokenError::InvalidAttribute(StreamError::UnexpectedEndOfStream, _)
            | TokenError::InvalidCharData(StreamError::UnexpectedEndOfStream, _) => {
                if self.has_fatal() {
                    return;
                }
                match self.stack.last() {
                    Some(open) => format!(
                        "Premature end of data in tag {} line {}",
                        qualified(&open.prefix, &open.local),
                        open.line
                    ),
                    None => "Couldn't find end of Start Tag".to_string(),
                }
            }
            other => {
                // xmlparser's own description, with its position stripped:
                // the line is already in front of the message.
                let text = other.to_string();
                match text.split_once(" at ") {
                    Some((head, tail)) => match tail.split_once(" cause ") {
                        Some((_, cause)) => format!("{head}: {cause}"),
                        None => head.to_string(),
                    },
                    None => text,
                }
            }
        };
        self.error(line, message);
    }
}

fn qualified(prefix: &str, local: &str) -> String {
    if prefix.is_empty() {
        local.to_string()
    } else {
        format!("{prefix}:{local}")
    }
}

fn resolve_prefix(scope: &[(String, String)], prefix: &str) -> Option<String> {
    let uri = scope
        .iter()
        .rev()
        .find(|(p, _)| p == prefix)
        .map(|(_, uri)| uri.as_str())?;
    // `xmlns=""` undeclares the default namespace.
    if uri.is_empty() {
        None
    } else {
        Some(uri.to_string())
    }
}

/// Attribute-value normalization: each tab, newline and return becomes a
/// space (the XML spec's step for every attribute, before any type-specific
/// collapsing a schema may add).
fn normalize_attribute_value(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if matches!(c, '\t' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// Whether a namespace URI has a scheme, which is all xmllint checks before
/// warning "URI is not absolute".
fn is_absolute_uri(uri: &str) -> bool {
    let Some((scheme, _)) = uri.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn is_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_xml_name_start()) && chars.all(|c| c.is_xml_name())
}

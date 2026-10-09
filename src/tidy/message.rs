//! tidy's reports (message.c, messageobj.c, `language_en.h)`: the codes, their
//! levels and English texts, how each is formatted from the element and node
//! it concerns, how it counts towards the warning and error totals, and
//! which lines reach the output under the display options.

use super::node::{AttVal, NodeId, NodeType};
use super::tables::{self, VERS_HTML5};
use super::{CustomTags, Doc, Options};

/// tidy's `TidyReportLevel`, the report levels itidy emits, in tidy's
/// order. tidy's dialogue levels (the totals and footnotes printed after
/// the reports) are not reproduced: they carry nothing a build needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Info,
    Warning,
    Config,
    Error,
}

impl Level {
    const fn prefix(self) -> &'static str {
        match self {
            Self::Info => "Info: ",
            Self::Warning => "Warning: ",
            Self::Config => "Config: ",
            Self::Error => "Error: ",
        }
    }
}

/// The report codes itidy can emit (the non-accessibility rows of tidy's
/// dispatch table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    AddedMissingCharset,
    AnchorNotUnique,
    AnchorDuplicated,
    AposUndefined,
    AttrValueNotLcase,
    AttributeValueReplaced,
    AttributeIsNotAllowed,
    BackslashInUri,
    BadAttributeValueReplaced,
    BadAttributeValue,
    BadCdataContent,
    BadSummaryHtml5,
    BadSurrogateLead,
    BadSurrogatePair,
    BadSurrogateTail,
    CantBeNested,
    CoerceToEndtag,
    ContentAfterBody,
    CustomTagDetected,
    DiscardingUnexpected,
    DoctypeAfterTags,
    DuplicateFrameset,
    ElementVersMismatchError,
    ElementVersMismatchWarn,
    EscapedIllegalUri,
    FixedBackslash,
    FoundStyleInBody,
    IdNameMismatch,
    IllegalNesting,
    IllegalUriCodepoint,
    IllegalUriReference,
    InsertingAutoAttribute,
    InsertingTag,
    InvalidAttribute,
    InvalidNcr,
    InvalidUtf8,
    InvalidXmlId,
    JoiningAttribute,
    MalformedComment,
    MalformedCommentEos,
    MalformedCommentDropping,
    MalformedCommentWarn,
    MalformedDoctype,
    MismatchedAttributeError,
    MismatchedAttributeWarn,
    MissingAttrValue,
    MissingAttribute,
    MissingDoctype,
    MissingEndtagBefore,
    MissingEndtagFor,
    MissingEndtagOptional,
    MissingImagemap,
    MissingQuotemark,
    MissingQuotemarkOpen,
    MissingSemicolonNcr,
    MissingSemicolon,
    MissingStarttag,
    MissingTitleElement,
    MovedStyleToHead,
    NestedEmphasis,
    NestedQuotation,
    NewlineInUri,
    NoframesContent,
    NonMatchingEndtag,
    ObsoleteElement,
    PreviousLocation,
    ProprietaryAttrValue,
    ProprietaryAttribute,
    ProprietaryElement,
    RemovedHtml5,
    RepeatedAttribute,
    ReplacingElement,
    ReplacingUnexElement,
    SpacePrecedingXmldecl,
    StringContentLooks,
    StringDoctypeGiven,
    StringNoSysid,
    SuspectedMissingQuote,
    TagNotAllowedIn,
    TooManyElementsIn,
    TrimEmptyElement,
    UnescapedAmpersand,
    UnexpectedEndOfFileAttr,
    UnexpectedEndOfFile,
    UnexpectedEqualsign,
    UnexpectedGt,
    UnexpectedQuotemark,
    UnknownElementLooksCustom,
    UnknownElement,
    UnknownEntity,
    UsingBrInplaceOf,
    WhiteInUri,
    XmlDeclarationDetected,
    XmlIdSyntax,
    BlankTitleElement,
}

impl Code {
    /// The level of the dispatch table. `DiscardingUnexpected` is decided
    /// at report time.
    const fn level(self) -> Level {
        use Code::{
            AddedMissingCharset, AttributeValueReplaced, CustomTagDetected, DuplicateFrameset,
            ElementVersMismatchError, MalformedComment, MalformedCommentEos,
            MismatchedAttributeError, MissingEndtagOptional, MissingQuotemarkOpen,
            PreviousLocation, StringContentLooks, StringDoctypeGiven, StringNoSysid,
            UnknownElement, UnknownElementLooksCustom,
        };
        match self {
            AddedMissingCharset
            | AttributeValueReplaced
            | CustomTagDetected
            | MalformedComment
            | MissingEndtagOptional
            | MissingQuotemarkOpen
            | PreviousLocation
            | StringContentLooks
            | StringDoctypeGiven
            | StringNoSysid => Level::Info,
            DuplicateFrameset
            | ElementVersMismatchError
            | MalformedCommentEos
            | MismatchedAttributeError
            | UnknownElementLooksCustom
            | UnknownElement => Level::Error,
            _ => Level::Warning,
        }
    }

    /// The English format string (`language_en.h`).
    const fn text(self) -> &'static str {
        use Code::{
            AddedMissingCharset, AnchorDuplicated, AnchorNotUnique, AposUndefined,
            AttrValueNotLcase, AttributeIsNotAllowed, AttributeValueReplaced, BackslashInUri,
            BadAttributeValue, BadAttributeValueReplaced, BadCdataContent, BadSummaryHtml5,
            BadSurrogateLead, BadSurrogatePair, BadSurrogateTail, BlankTitleElement, CantBeNested,
            CoerceToEndtag, ContentAfterBody, CustomTagDetected, DiscardingUnexpected,
            DoctypeAfterTags, DuplicateFrameset, ElementVersMismatchError, ElementVersMismatchWarn,
            EscapedIllegalUri, FixedBackslash, FoundStyleInBody, IdNameMismatch, IllegalNesting,
            IllegalUriCodepoint, IllegalUriReference, InsertingAutoAttribute, InsertingTag,
            InvalidAttribute, InvalidNcr, InvalidUtf8, InvalidXmlId, JoiningAttribute,
            MalformedComment, MalformedCommentDropping, MalformedCommentEos, MalformedCommentWarn,
            MalformedDoctype, MismatchedAttributeError, MismatchedAttributeWarn, MissingAttrValue,
            MissingAttribute, MissingDoctype, MissingEndtagBefore, MissingEndtagFor,
            MissingEndtagOptional, MissingImagemap, MissingQuotemark, MissingQuotemarkOpen,
            MissingSemicolon, MissingSemicolonNcr, MissingStarttag, MissingTitleElement,
            MovedStyleToHead, NestedEmphasis, NestedQuotation, NewlineInUri, NoframesContent,
            NonMatchingEndtag, ObsoleteElement, PreviousLocation, ProprietaryAttrValue,
            ProprietaryAttribute, ProprietaryElement, RemovedHtml5, RepeatedAttribute,
            ReplacingElement, ReplacingUnexElement, SpacePrecedingXmldecl, StringContentLooks,
            StringDoctypeGiven, StringNoSysid, SuspectedMissingQuote, TagNotAllowedIn,
            TooManyElementsIn, TrimEmptyElement, UnescapedAmpersand, UnexpectedEndOfFile,
            UnexpectedEndOfFileAttr, UnexpectedEqualsign, UnexpectedGt, UnexpectedQuotemark,
            UnknownElement, UnknownElementLooksCustom, UnknownEntity, UsingBrInplaceOf, WhiteInUri,
            XmlDeclarationDetected, XmlIdSyntax,
        };
        match self {
            AddedMissingCharset => "Added appropriate missing <meta charset=...> to %s",
            AnchorNotUnique => "%s anchor \"%s\" already defined",
            AnchorDuplicated => "Implicit %s anchor \"%s\" duplicated by Tidy.",
            AposUndefined => "named entity &apos; only defined in XML/XHTML",
            AttrValueNotLcase => "%s attribute value \"%s\" must be lower case for XHTML",
            AttributeIsNotAllowed => "%s attribute \"is\" not allowed for autonomous custom tags.",
            AttributeValueReplaced => "%s attribute \"%s\", incorrect value \"%s\" replaced",
            BackslashInUri => "%s URI reference contains backslash. Typo?",
            BadAttributeValueReplaced => {
                "%s attribute \"%s\" had invalid value \"%s\" and has been replaced"
            }
            BadAttributeValue => "%s attribute \"%s\" has invalid value \"%s\"",
            BadCdataContent => "'<' + '/' + letter not allowed here",
            BadSummaryHtml5 => "The summary attribute on the %s element is obsolete in HTML5",
            BadSurrogateLead => {
                "Trailing (Low) surrogate pair U+%04X, with no leading (High) entity, replaced with U+FFFD."
            }
            BadSurrogatePair => {
                "Have out-of-range surrogate pair U+%04X:U+%04X, replaced with U+FFFD value."
            }
            BadSurrogateTail => {
                "Leading (High) surrogate pair U+%04X, with no trailing (Low) entity, replaced with U+FFFD."
            }
            CantBeNested => "%s can't be nested",
            CoerceToEndtag => "<%s> is probably intended as </%s>",
            ContentAfterBody => "content occurs after end of body",
            CustomTagDetected => "detected autonomous custom tag %s; will treat as %s",
            DiscardingUnexpected => "discarding unexpected %s",
            DoctypeAfterTags => "<!DOCTYPE> isn't allowed after elements",
            DuplicateFrameset => "repeated FRAMESET element",
            ElementVersMismatchError | ElementVersMismatchWarn => "%s element not available in %s",
            EscapedIllegalUri => "%s escaping malformed URI reference",
            FixedBackslash => "%s converting backslash in URI to slash",
            FoundStyleInBody => "found <style> tag in <body>! fix-style-tags: yes to move.",
            IdNameMismatch => "%s id and name attribute value mismatch",
            IllegalNesting => "%s shouldn't be nested",
            IllegalUriCodepoint => "%s illegal characters found in URI",
            IllegalUriReference => "%s improperly escaped URI reference",
            InsertingAutoAttribute => "%s inserting \"%s\" attribute using value \"%s\"",
            InsertingTag => "inserting implicit <%s>",
            InvalidAttribute => "%s attribute name \"%s\" (value=\"%s\") is invalid",
            InvalidNcr => "%s invalid numeric character reference %s",
            InvalidUtf8 => "%s invalid UTF-8 bytes (char. code %s)",
            InvalidXmlId => "%s cannot copy name attribute to id",
            JoiningAttribute => "%s joining values of repeated attribute \"%s\"",
            MalformedComment => "tidy replaced adjacent \"-\" with \"=\"",
            MalformedCommentDropping => "dropping a possible comment due to a missing hyphen",
            MalformedCommentEos => {
                "the end of the document was reached before the end of the comment"
            }
            MalformedCommentWarn => {
                "detected adjacent hyphens within the comment; consider fix-bad-comments"
            }
            MalformedDoctype => "discarding malformed <!DOCTYPE>",
            MismatchedAttributeError | MismatchedAttributeWarn => {
                "%s attribute \"%s\" not allowed for %s"
            }
            MissingAttrValue => "%s attribute \"%s\" lacks value",
            MissingAttribute => "%s lacks \"%s\" attribute",
            MissingDoctype => "missing <!DOCTYPE> declaration",
            MissingEndtagBefore => "missing </%s> before %s",
            MissingEndtagFor => "missing </%s>",
            MissingEndtagOptional => "missing optional end tag </%s>",
            MissingImagemap => "%s should use client-side image map",
            MissingQuotemark => "%s attribute with missing trailing quote mark",
            MissingQuotemarkOpen => "value for attribute \"%s\" missing quote marks",
            MissingSemicolonNcr => "numeric character reference \"%s\" doesn't end in ';'",
            MissingSemicolon => "entity \"%s\" doesn't end in ';'",
            MissingStarttag => "missing <%s>",
            MissingTitleElement => "inserting missing 'title' element",
            MovedStyleToHead => "moved <style> tag to <head>! fix-style-tags: no to avoid.",
            NestedEmphasis => "nested emphasis %s",
            NestedQuotation => "nested q elements, possible typo.",
            NewlineInUri => "%s discarding newline in URI reference",
            NoframesContent => "%s not inside 'noframes' element",
            NonMatchingEndtag => "replacing unexpected %s with </%s>",
            ObsoleteElement => "replacing obsolete element %s with %s",
            PreviousLocation => "<%s> previously mentioned",
            ProprietaryAttrValue => "%s proprietary attribute value \"%s\"",
            ProprietaryAttribute => "%s proprietary attribute \"%s\"",
            ProprietaryElement => "%s is not approved by W3C",
            RemovedHtml5 => "%s element removed from HTML5",
            RepeatedAttribute => "%s dropping value \"%s\" for repeated attribute \"%s\"",
            ReplacingElement => "replacing %s with %s",
            ReplacingUnexElement => "replacing unexpected %s with %s",
            SpacePrecedingXmldecl => "removing whitespace preceding XML Declaration",
            StringContentLooks => "Document content looks like %s",
            StringDoctypeGiven => "Doctype given is \"%s\"",
            StringNoSysid => "No system identifier in emitted doctype",
            SuspectedMissingQuote => "suspected missing quote mark for attribute value",
            TagNotAllowedIn => "%s isn't allowed in <%s> elements",
            TooManyElementsIn => "too many %s elements in <%s>",
            TrimEmptyElement => "trimming empty %s",
            UnescapedAmpersand => "unescaped & which should be written as &amp;",
            UnexpectedEndOfFileAttr => "%s end of file while parsing attributes",
            UnexpectedEndOfFile => "unexpected end of file %s",
            UnexpectedEqualsign => "%s unexpected '=', expected attribute name",
            UnexpectedGt => "%s missing '>' for end of tag",
            UnexpectedQuotemark => "%s unexpected or duplicate quote mark",
            UnknownElementLooksCustom => {
                "%s is not recognized! Did you mean to enable the custom-tags option?"
            }
            UnknownElement => "%s is not recognized!",
            UnknownEntity => "unescaped & or unknown entity \"%s\"",
            UsingBrInplaceOf => "using <br> in place of %s",
            WhiteInUri => "%s discarding whitespace in URI reference",
            XmlDeclarationDetected => {
                "An XML declaration was detected. Did you mean to use input-xml?"
            }
            XmlIdSyntax => "%s ID \"%s\" uses XML ID syntax",
            BlankTitleElement => "blank 'title' element",
        }
    }
}

/// Substitute the `%s`/`%04X` conversions of a tidy format string.
fn format(text: &str, args: &[String]) -> String {
    let mut out = String::new();
    let mut args = args.iter();
    let mut rest = text;
    while let Some(pos) = rest.find('%') {
        out.push_str(&rest[..pos]);
        let spec = &rest[pos..];
        if let Some(tail) = spec.strip_prefix("%s") {
            out.push_str(args.next().map_or("", String::as_str));
            rest = tail;
        } else if let Some(tail) = spec.strip_prefix("%04X") {
            out.push_str(args.next().map_or("", String::as_str));
            rest = tail;
        } else {
            out.push('%');
            rest = &spec[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Where a report is positioned: at a node, at the lexer's current
/// position, or nowhere.
#[derive(Clone, Copy)]
enum At {
    Node(NodeId),
    Lexer,
    Nowhere,
}

impl Doc {
    /// `TagToString`: how a node is named in a message.
    pub fn tag_to_string(&self, node: Option<NodeId>) -> String {
        let Some(node) = node else {
            return String::new();
        };
        let n = self.tree.node(node);
        match n.ntype {
            NodeType::StartTag | NodeType::StartEndTag => {
                format!("<{}>", n.element.as_deref().unwrap_or(""))
            }
            NodeType::EndTag => format!("</{}>", n.element.as_deref().unwrap_or("")),
            NodeType::DocType => "<!DOCTYPE>".to_string(),
            NodeType::Text => "plain text".to_string(),
            NodeType::XmlDecl => "XML declaration".to_string(),
            _ => n.element.clone().unwrap_or_default(),
        }
    }

    fn element_name(&self, node: Option<NodeId>) -> String {
        node.and_then(|n| self.tree.node(n).element.clone())
            .unwrap_or_default()
    }

    /// message.c `HTMLVersion`: the name of the version the document is
    /// held to, for the "not allowed for X" messages.
    pub fn html_version_name(&self) -> String {
        let version = if self.lexer.version_emitted == 0 {
            self.lexer.doctype
        } else {
            self.lexer.version_emitted
        };
        tables::name_from_vers(version)
            .unwrap_or("HTML Proprietary")
            .to_string()
    }

    fn position(&self, at: At) -> (i32, i32) {
        match at {
            At::Node(n) => {
                let node = self.tree.node(n);
                (node.line as i32, node.column as i32)
            }
            At::Lexer => (self.lexer.lines as i32, self.lexer.columns as i32),
            At::Nowhere => (0, 0),
        }
    }

    /// `messageOut`: count the message and print it if the display
    /// options allow.
    fn emit(&mut self, level: Level, code: Code, at: At, message: String) {
        match level {
            Level::Info => self.counts.infos += 1,
            Level::Warning => self.counts.warnings += 1,
            Level::Config => self.counts.option_errors += 1,
            Level::Error => self.counts.errors += 1,
        }
        let opts: &Options = &self.opts;
        // tidy stops printing reports once `show-errors` errors were
        // counted (the count above happens first, so the sixth error is
        // already silent).
        let mut go = self.counts.errors < opts.show_errors;
        if opts.quiet {
            go &= !matches!(
                code,
                Code::StringDoctypeGiven | Code::StringContentLooks | Code::StringNoSysid
            );
            go &= level != Level::Config;
            go &= level != Level::Info;
        }
        if !opts.show_info {
            go &= level != Level::Info;
        }
        if !opts.show_warnings {
            go &= level != Level::Warning;
        }
        if !go {
            return;
        }
        let (line, column) = self.position(at);
        let text = if line > 0 && column > 0 {
            format!("line {line} column {column} - {}{message}", level.prefix())
        } else {
            format!("{}{message}", level.prefix())
        };
        self.out.push(text);
    }

    /// `TY_(Report)` for the codes formatted by `formatStandard` and
    /// `formatStandardDynamic`: `element` is the containing element,
    /// `node` the offending node.
    pub fn report(&mut self, element: Option<NodeId>, node: Option<NodeId>, code: Code) {
        use Code::{
            AddedMissingCharset, BadCdataContent, BadSummaryHtml5, BlankTitleElement, CantBeNested,
            CoerceToEndtag, ContentAfterBody, CustomTagDetected, DiscardingUnexpected,
            DoctypeAfterTags, DuplicateFrameset, ElementVersMismatchError, ElementVersMismatchWarn,
            FoundStyleInBody, IllegalNesting, InsertingTag, MalformedComment,
            MalformedCommentDropping, MalformedCommentEos, MalformedCommentWarn, MalformedDoctype,
            MissingDoctype, MissingEndtagBefore, MissingEndtagFor, MissingEndtagOptional,
            MissingStarttag, MissingTitleElement, MovedStyleToHead, NestedEmphasis,
            NestedQuotation, NoframesContent, NonMatchingEndtag, ObsoleteElement, PreviousLocation,
            ProprietaryElement, RemovedHtml5, ReplacingElement, ReplacingUnexElement,
            SpacePrecedingXmldecl, StringNoSysid, SuspectedMissingQuote, TagNotAllowedIn,
            TooManyElementsIn, TrimEmptyElement, UnexpectedEndOfFile, UnknownElement,
            UnknownElementLooksCustom, UsingBrInplaceOf, XmlDeclarationDetected,
        };
        let nodedesc = self.tag_to_string(node);
        let elemdesc = self.tag_to_string(element);
        let rpt = element.or(node);
        let rpt_at = rpt.map_or(At::Lexer, At::Node);
        let node_at = node.map_or(At::Lexer, At::Node);
        let mut level = code.level();
        let (at, args): (At, Vec<String>) = match code {
            CustomTagDetected => {
                let tagtype = match self.opts.custom_tags {
                    CustomTags::Blocklevel => "block level",
                    CustomTags::Empty => "empty",
                    CustomTags::Inline => "inline",
                    CustomTags::No | CustomTags::Pre => "pre",
                };
                (
                    element.map_or(At::Lexer, At::Node),
                    vec![elemdesc, tagtype.to_string()],
                )
            }
            StringNoSysid => (At::Nowhere, vec![]),
            SpacePrecedingXmldecl => (node_at, vec![]),
            CantBeNested | NoframesContent | UsingBrInplaceOf => (node_at, vec![nodedesc]),
            ElementVersMismatchError | ElementVersMismatchWarn => {
                (node_at, vec![nodedesc, self.html_version_name()])
            }
            TagNotAllowedIn => (node_at, vec![nodedesc, self.element_name(element)]),
            InsertingTag | MissingStarttag => (node_at, vec![self.element_name(node)]),
            BadCdataContent
            | ContentAfterBody
            | DoctypeAfterTags
            | DuplicateFrameset
            | MalformedComment
            | MalformedCommentDropping
            | MalformedCommentEos
            | MalformedCommentWarn
            | MalformedDoctype
            | MissingDoctype
            | MissingTitleElement
            | NestedQuotation
            | SuspectedMissingQuote
            | XmlDeclarationDetected
            | BlankTitleElement => (rpt_at, vec![]),
            FoundStyleInBody | IllegalNesting | MovedStyleToHead | TrimEmptyElement
            | UnexpectedEndOfFile => (rpt_at, vec![elemdesc]),
            ObsoleteElement | ReplacingElement | ReplacingUnexElement => {
                (rpt_at, vec![elemdesc, nodedesc])
            }
            AddedMissingCharset
            | BadSummaryHtml5
            | NestedEmphasis
            | ProprietaryElement
            | RemovedHtml5
            | UnknownElement
            | UnknownElementLooksCustom => (rpt_at, vec![nodedesc]),
            MissingEndtagFor | MissingEndtagOptional | PreviousLocation => {
                (rpt_at, vec![self.element_name(element)])
            }
            MissingEndtagBefore => (rpt_at, vec![self.element_name(element), nodedesc]),
            CoerceToEndtag | NonMatchingEndtag => {
                let name = self.element_name(node);
                (rpt_at, vec![name.clone(), name])
            }
            TooManyElementsIn => (
                rpt_at,
                vec![self.element_name(node), self.element_name(element)],
            ),
            DiscardingUnexpected => {
                level = if self.bad_form != 0 {
                    Level::Error
                } else {
                    Level::Warning
                };
                (node_at, vec![nodedesc])
            }
            other => {
                // Codes with their own reporting functions never reach here.
                panic!("report(): {other:?} needs its own report function");
            }
        };
        let message = format(code.text(), &args);
        self.emit(level, code, at, message);
        if code == TagNotAllowedIn || code == TooManyElementsIn {
            self.report(element, node, PreviousLocation);
        }
    }

    /// `TY_(ReportAttrError)`: the codes of `formatAttributeReport`. The
    /// node is absent while an XML declaration's pseudo-attributes are read.
    pub fn report_attr(&mut self, node: Option<NodeId>, av: Option<&AttVal>, code: Code) {
        use Code::{
            AnchorDuplicated, AnchorNotUnique, AttrValueNotLcase, AttributeIsNotAllowed,
            AttributeValueReplaced, BackslashInUri, BadAttributeValue, BadAttributeValueReplaced,
            EscapedIllegalUri, FixedBackslash, IdNameMismatch, IllegalUriCodepoint,
            IllegalUriReference, InsertingAutoAttribute, InvalidAttribute, InvalidXmlId,
            JoiningAttribute, MismatchedAttributeError, MismatchedAttributeWarn, MissingAttrValue,
            MissingImagemap, MissingQuotemark, MissingQuotemarkOpen, NewlineInUri,
            ProprietaryAttrValue, ProprietaryAttribute, RepeatedAttribute, UnexpectedEndOfFileAttr,
            UnexpectedEqualsign, UnexpectedGt, UnexpectedQuotemark, WhiteInUri, XmlIdSyntax,
        };
        let tagdesc = self.tag_to_string(node);
        let name = av
            .and_then(|a| a.attribute.clone())
            .unwrap_or_else(|| "NULL".to_string());
        let value = av
            .and_then(|a| a.value.clone())
            .unwrap_or_else(|| "NULL".to_string());
        let mut at = node.map_or(At::Lexer, At::Node);
        let args = match code {
            MissingQuotemarkOpen => vec![name],
            BackslashInUri | EscapedIllegalUri | FixedBackslash | IdNameMismatch
            | IllegalUriCodepoint | IllegalUriReference | InvalidXmlId | MissingImagemap
            | MissingQuotemark | NewlineInUri | UnexpectedEqualsign | UnexpectedGt
            | UnexpectedQuotemark | WhiteInUri => vec![tagdesc],
            AttributeIsNotAllowed | JoiningAttribute | MissingAttrValue | ProprietaryAttribute => {
                vec![tagdesc, name]
            }
            AttributeValueReplaced
            | BadAttributeValue
            | BadAttributeValueReplaced
            | InsertingAutoAttribute
            | InvalidAttribute => vec![tagdesc, name, value],
            MismatchedAttributeError | MismatchedAttributeWarn => {
                vec![tagdesc, name, self.html_version_name()]
            }
            AnchorNotUnique | AnchorDuplicated | AttrValueNotLcase | ProprietaryAttrValue
            | XmlIdSyntax => vec![tagdesc, value],
            RepeatedAttribute => vec![tagdesc, value, name],
            UnexpectedEndOfFileAttr => {
                // On end of file the position is the end of the input.
                self.lexer.lines = self.stream.curline as u32;
                self.lexer.columns = self.stream.curcol as u32;
                at = At::Lexer;
                vec![tagdesc]
            }
            other => panic!("report_attr(): {other:?} is not an attribute report"),
        };
        let message = format(code.text(), &args);
        self.emit(code.level(), code, at, message);
    }

    /// `TY_(ReportMissingAttr)`.
    pub fn report_missing_attr(&mut self, node: NodeId, name: &str) {
        let nodedesc = self.tag_to_string(Some(node));
        let message = format(Code::MissingAttribute.text(), &[nodedesc, name.to_string()]);
        self.emit(
            Level::Warning,
            Code::MissingAttribute,
            At::Node(node),
            message,
        );
    }

    /// `TY_(ReportEntityError)`: positioned at the lexer.
    pub fn report_entity(&mut self, code: Code, entity: &str) {
        let message = format(code.text(), &[entity.to_string()]);
        self.emit(code.level(), code, At::Lexer, message);
    }

    /// `TY_(ReportEncodingError)`: `formatEncodingReport`.
    pub fn report_encoding(&mut self, code: Code, c: u32, discarded: bool) {
        let action = if discarded { "discarding" } else { "replacing" };
        let shown = match code {
            Code::InvalidUtf8 => format!("U+{c:04X}"),
            _ => c.to_string(),
        };
        if code == Code::InvalidUtf8 {
            self.bad_chars |= BC_INVALID_UTF8;
        } else {
            self.bad_chars |= BC_OTHER;
        }
        let message = format(code.text(), &[action.to_string(), shown]);
        self.emit(code.level(), code, At::Lexer, message);
    }

    /// `TY_(ReportSurrogateError)`.
    pub fn report_surrogate(&mut self, code: Code, c1: u32, c2: u32) {
        let message = format(code.text(), &[format!("{c1:04X}"), format!("{c2:04X}")]);
        self.emit(code.level(), code, At::Lexer, message);
    }

    /// `ReportMarkupVersion`: the Info lines about the document's version
    /// that `tidy` prints after the reports (and `-q` hides).
    pub fn report_summary(&mut self) {
        if let Some(doctype) = self.given_doctype.clone() {
            let message = format(Code::StringDoctypeGiven.text(), &[doctype]);
            self.emit(Level::Info, Code::StringDoctypeGiven, At::Nowhere, message);
        }
        let apparent = self.apparent_version();
        let vers = tables::name_from_vers(apparent)
            .unwrap_or("HTML Proprietary")
            .to_string();
        let message = format(Code::StringContentLooks.text(), &[vers]);
        self.emit(Level::Info, Code::StringContentLooks, At::Nowhere, message);
        if self.warn_missing_si_in_emitted_doctype() {
            self.emit(
                Level::Info,
                Code::StringNoSysid,
                At::Nowhere,
                Code::StringNoSysid.text().to_string(),
            );
        }
    }

    /// `HTMLVersion` of lexer.c: the version the document conforms to,
    /// chosen among the W3C doctypes the content still allows.
    pub fn html_version(&self) -> u32 {
        let vers = self.lexer.versions;
        let dtver = self.lexer.doctype;
        let dtmode = self.opts.doctype_mode;
        let xhtml = (self.opts.output_xml || self.lexer.isvoyager) && !self.opts.output_html;
        let html4 = matches!(
            dtmode,
            super::DoctypeMode::Strict | super::DoctypeMode::Loose
        ) || (tables::VERS_HTML40PX & dtver) != 0;
        let html5 =
            !html4 && matches!(dtmode, super::DoctypeMode::Auto | super::DoctypeMode::Html5);
        if xhtml && dtver == tables::VERS_UNKNOWN {
            return tables::XH50;
        }
        if dtver == tables::VERS_UNKNOWN {
            return tables::HT50;
        }
        if !xhtml && dtver == VERS_HTML5 {
            return tables::HT50;
        }
        if xhtml && html5 && (vers & VERS_HTML5) == tables::XH50 {
            return tables::XH50;
        }
        let mut score = 0;
        let mut best = None;
        for d in tables::W3C_DOCTYPES {
            if (xhtml && (tables::VERS_XHTML & d.vers) == 0)
                || (html4 && (tables::VERS_HTML40PX & d.vers) == 0)
            {
                continue;
            }
            if (vers & d.vers) != 0 && (d.score < score || score == 0) {
                score = d.score;
                best = Some(d.vers);
            }
        }
        best.unwrap_or(tables::VERS_UNKNOWN)
    }

    /// `TY_(ApparentVersion)`.
    pub fn apparent_version(&self) -> u32 {
        let doctype = self.lexer.doctype;
        if (doctype == tables::XH11 || doctype == tables::XB10)
            && (self.lexer.versions & doctype) != 0
        {
            doctype
        } else {
            self.html_version()
        }
    }

    /// `TY_(WarnMissingSIInEmittedDocType)`.
    fn warn_missing_si_in_emitted_doctype(&self) -> bool {
        if self.lexer.isvoyager {
            return false;
        }
        if tables::name_from_vers(self.lexer.version_emitted).is_none() {
            return false;
        }
        if tables::si_from_vers(self.lexer.version_emitted).is_none() {
            return false;
        }
        match self.find_doctype() {
            Some(doctype) => self.tree.attr_by_name(doctype, "SYSTEM").is_none(),
            None => false,
        }
    }
}

/// `doc->badChars` bits that matter here: only whether anything was seen.
pub const BC_INVALID_UTF8: u32 = 4;
pub const BC_OTHER: u32 = 1;

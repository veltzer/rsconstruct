//! The clean-and-repair pass that follows parsing (tidylib.c
//! `tidyDocCleanAndRepair` with tidy's default options, and the clean.c
//! routines it runs): style elements moved to the head, nested emphasis
//! merged, indentation lists turned into divs, the meta charset checked,
//! the doctype fixed, anchors and language attributes reconciled, and
//! finally the HTML5 and proprietary tag and attribute reports.

use super::Doc;
use super::message::Code;
use super::node::{NodeId, NodeType};
use super::tables::{
    self, HT50, ParserKind, TagId, VERS_FRAMESET, VERS_HTML5, VERS_HTML40, VERS_HTML40_STRICT,
    VERS_LOOSE, VERS_PROPRIETARY, VERS_STRICT, VERS_UNKNOWN, VERS_XHTML, X10F, X10S, X10T, XB10,
    XH11, XH50,
};
use super::{DoctypeMode, TriState, USING_LAYER, USING_NOBR, USING_SPACER};

impl Doc {
    /// `tidyDocCleanAndRepair`.
    pub(crate) fn clean_and_repair(&mut self) {
        self.clean_style();
        if self.opts.merge_emphasis {
            self.nested_emphasis(Some(0));
        }
        self.list2bq(Some(0));
        self.bq2div(Some(0));
        self.tidy_meta_charset();

        if let Some(doctype) = self.find_doctype()
            && let Some(i) = self.tree.attr_by_name(doctype, "PUBLIC")
            && let Some(value) = self.tree.node(doctype).attributes[i].value.clone()
        {
            self.given_doctype = Some(value);
        }

        if self.tree.content(0).is_some() {
            if self.opts.output_html
                && self.lexer.isvoyager
                && let Some(doctype) = self.find_doctype()
            {
                self.tree.remove_node(doctype);
            }
            let want_name = self.opts.anchor_as_name;
            if self.opts.output_xhtml && !self.opts.output_html {
                self.set_xhtml_doctype();
                self.fix_anchors(Some(0), want_name, true);
                self.fix_xhtml_namespace(true);
                self.fix_language_information(Some(0), true, true);
            } else {
                self.fix_doctype();
                self.fix_anchors(Some(0), want_name, true);
                self.fix_xhtml_namespace(false);
                self.fix_language_information(Some(0), false, true);
            }
        }

        if (self.lexer.version_emitted & VERS_HTML5) != 0 {
            self.check_html5(Some(0));
        }
        self.check_html_tags_attribs_versions(Some(0));
        if !self.lexer.isvoyager && self.xml_detected {
            let decl = self.find_xml_decl();
            self.report(None, decl, Code::XmlDeclarationDetected);
        }
        self.clean_head();
    }

    // ---- style elements in the body -----------------------------------------

    fn style_to_head(&mut self, head: NodeId, mut node: Option<NodeId>, fix: bool) {
        while let Some(n) = node {
            let next = self.tree.next(n);
            if self.node_is(n, TagId::STYLE) {
                if fix {
                    self.tree.remove_node(n);
                    self.tree.insert_node_at_end(head, n);
                    self.report(Some(n), Some(head), Code::MovedStyleToHead);
                } else {
                    self.report(Some(n), Some(head), Code::FoundStyleInBody);
                }
            } else if let Some(content) = self.tree.content(n) {
                self.style_to_head(head, Some(content), fix);
            }
            node = next;
        }
    }

    /// `TY_(CleanStyle)`.
    fn clean_style(&mut self) {
        let fix = self.opts.fix_style_tags;
        if let (Some(head), Some(body)) = (self.find_head(), self.find_body()) {
            self.style_to_head(head, Some(body), fix);
        }
    }

    // ---- structural clean-ups -------------------------------------------------

    /// `DiscardContainer`: replace an element by its content; returns the
    /// node that now sits where the element was.
    fn discard_container(&mut self, element: NodeId) -> Option<NodeId> {
        let Some(first) = self.tree.content(element) else {
            return self.tree.discard_element(element);
        };
        let parent = self.tree.parent(element).expect("a container has a parent");
        let last = self
            .tree
            .last(element)
            .expect("content implies a last child");
        let next = self.tree.next(element);
        let prev = self.tree.prev(element);
        self.tree.node_mut(last).next = next;
        match next {
            Some(n) => self.tree.node_mut(n).prev = Some(last),
            None => self.tree.node_mut(parent).last = Some(last),
        }
        match prev {
            Some(p) => {
                self.tree.node_mut(first).prev = Some(p);
                self.tree.node_mut(p).next = Some(first);
            }
            None => self.tree.node_mut(parent).content = Some(first),
        }
        let mut child = Some(first);
        while let Some(c) = child {
            self.tree.node_mut(c).parent = Some(parent);
            child = self.tree.next(c);
        }
        let e = self.tree.node_mut(element);
        e.next = None;
        e.content = None;
        e.last = None;
        e.parent = None;
        e.prev = None;
        Some(first)
    }

    /// `StripOnlyChild`.
    fn strip_only_child(&mut self, node: NodeId) {
        let child = self
            .tree
            .content(node)
            .expect("strip_only_child needs a child");
        let content = self.tree.content(child);
        let last = self.tree.last(child);
        self.tree.node_mut(node).content = content;
        self.tree.node_mut(node).last = last;
        self.tree.node_mut(child).content = None;
        let mut c = content;
        while let Some(n) = c {
            self.tree.node_mut(n).parent = Some(node);
            c = self.tree.next(n);
        }
    }

    /// `RenameElem`.
    fn rename_elem(&mut self, node: NodeId, id: TagId) {
        let index = self.tag_index(id);
        let name = self.tags[index].name.clone();
        let n = self.tree.node_mut(node);
        n.element = Some(name);
        n.tag = Some(index);
    }

    fn has_one_child(&self, node: NodeId) -> bool {
        self.tree
            .content(node)
            .is_some_and(|c| self.tree.next(c).is_none())
    }

    /// `TY_(NestedEmphasis)`: `<b><b>` and `<i><i>` collapsed.
    fn nested_emphasis(&mut self, mut node: Option<NodeId>) {
        while let Some(n) = node {
            let mut next = self.tree.next(n);
            if (self.node_is(n, TagId::B) || self.node_is(n, TagId::I))
                && let Some(parent) = self.tree.parent(n)
                && self.tree.node(parent).tag.is_some()
                && self.tree.node(parent).tag == self.tree.node(n).tag
            {
                next = self.discard_container(n);
                node = next;
                continue;
            }
            if let Some(content) = self.tree.content(n) {
                self.nested_emphasis(Some(content));
            }
            node = next;
        }
    }

    /// `TY_(List2BQ)`.
    fn list2bq(&mut self, mut node: Option<NodeId>) {
        while let Some(n) = node {
            if let Some(content) = self.tree.content(n) {
                self.list2bq(Some(content));
            }
            if self.node_parser(n) == Some(ParserKind::ParseList)
                && self.has_one_child(n)
                && self
                    .tree
                    .content(n)
                    .is_some_and(|c| self.tree.node(c).implicit)
            {
                self.strip_only_child(n);
                self.rename_elem(n, TagId::BLOCKQUOTE);
                self.tree.node_mut(n).implicit = true;
            }
            node = self.tree.next(n);
        }
    }

    /// `TY_(BQ2Div)`.
    fn bq2div(&mut self, mut node: Option<NodeId>) {
        while let Some(n) = node {
            if self.node_is(n, TagId::BLOCKQUOTE) && self.tree.node(n).implicit {
                let mut indent = 1;
                while self.has_one_child(n)
                    && self
                        .tree
                        .content(n)
                        .is_some_and(|c| self.node_is(c, TagId::BLOCKQUOTE))
                    && self.tree.node(n).implicit
                {
                    indent += 1;
                    self.strip_only_child(n);
                }
                if let Some(content) = self.tree.content(n) {
                    self.bq2div(Some(content));
                }
                self.rename_elem(n, TagId::DIV);
                self.add_style_property(n, &format!("margin-left: {}em", 2 * indent));
            } else if let Some(content) = self.tree.content(n) {
                self.bq2div(Some(content));
            }
            node = self.tree.next(n);
        }
    }

    /// `TY_(AddStyleProperty)`: add a declaration to the node's `style`.
    pub(crate) fn add_style_property(&mut self, node: NodeId, property: &str) {
        let known = tables::tables().known;
        if let Some(index) = self.tree.attr_by_id(node, known.style) {
            let current = self.tree.node(node).attributes[index].value.clone();
            let merged = match current {
                Some(existing) => merge_properties(&existing, property),
                None => property.to_string(),
            };
            self.tree.node_mut(node).attributes[index].value = Some(merged);
        } else {
            let mut av = super::node::AttVal::new();
            av.delim = u32::from(b'"');
            av.attribute = Some("style".to_string());
            av.value = Some(property.to_string());
            av.dict = Some(known.style);
            self.tree.node_mut(node).attributes.insert(0, av);
        }
    }

    // ---- meta charset -----------------------------------------------------------

    /// `TY_(TidyMetaCharset)` for a UTF-8 output encoding.
    fn tidy_meta_charset(&mut self) {
        let Some(head) = self.find_head() else {
            return;
        };
        if self.opts.show_body_only == TriState::Yes {
            return;
        }
        let known = tables::tables().known;
        let enc = "utf-8";
        let charset_string = format!("charset={enc}");
        let mut charset_found = false;
        let mut current = self.tree.content(head);
        while let Some(node) = current {
            if !self.node_is(node, TagId::META) {
                current = self.tree.next(node);
                continue;
            }
            let charset_attr = self.tree.attr_by_id(node, known.charset);
            let http_equiv_attr = self.tree.attr_by_id(node, known.http_equiv);
            if charset_attr.is_none() && http_equiv_attr.is_none() {
                current = self.tree.next(node);
                continue;
            }
            if let (Some(ci), None) = (charset_attr, http_equiv_attr) {
                let value = self.tree.node(node).attributes[ci].value.clone();
                if charset_found || value.is_none() {
                    let prev = self.tree.prev(node);
                    self.report(Some(head), Some(node), Code::DiscardingUnexpected);
                    self.tree.discard_element(node);
                    current = match prev {
                        Some(p) => self.tree.next(p),
                        None => self.tree.content(head),
                    };
                    continue;
                }
                charset_found = true;
                if !value.as_deref().unwrap_or("").eq_ignore_ascii_case(enc) {
                    let av = self.tree.node(node).attributes[ci].clone();
                    self.report_attr(Some(node), Some(&av), Code::AttributeValueReplaced);
                    self.tree.node_mut(node).attributes[ci].value = Some(enc.to_string());
                }
                let second = self.tree.content(head).and_then(|c| self.tree.next(c));
                if Some(node) != second {
                    self.tree.remove_node(node);
                    self.tree.insert_node_at_start(head, node);
                }
                current = self.tree.next(node);
                continue;
            }
            if let (None, Some(hi)) = (charset_attr, http_equiv_attr) {
                let Some(ci) = self.tree.attr_by_id(node, known.content) else {
                    current = self.tree.next(node);
                    continue;
                };
                let http_value = self.tree.node(node).attributes[hi].value.clone();
                let Some(http_value) = http_value else {
                    let prev = self.tree.prev(node);
                    self.report(Some(head), Some(node), Code::DiscardingUnexpected);
                    self.tree.discard_element(node);
                    current = match prev {
                        Some(p) => self.tree.next(p),
                        None => self.tree.content(head),
                    };
                    continue;
                };
                if !http_value.eq_ignore_ascii_case("content-type") {
                    current = self.tree.next(node);
                    continue;
                }
                let Some(content_value) = self.tree.node(node).attributes[ci].value.clone() else {
                    current = self.tree.next(node);
                    continue;
                };
                if content_value.eq_ignore_ascii_case(&charset_string) {
                    if charset_found {
                        let prev = self.tree.prev(node);
                        self.report(Some(head), Some(node), Code::DiscardingUnexpected);
                        self.tree.discard_element(node);
                        current = match prev {
                            Some(p) => self.tree.next(p),
                            None => self.tree.content(head),
                        };
                        continue;
                    }
                    charset_found = true;
                } else if charset_found {
                    let prev = self.tree.prev(node);
                    self.report(Some(head), Some(node), Code::DiscardingUnexpected);
                    self.tree.discard_element(node);
                    current = match prev {
                        Some(p) => self.tree.next(p),
                        None => self.tree.content(head),
                    };
                    continue;
                } else {
                    if self.opts.show_meta_change {
                        let av = self.tree.node(node).attributes[ci].clone();
                        self.report_attr(Some(node), Some(&av), Code::AttributeValueReplaced);
                    }
                    self.tree.node_mut(node).attributes[ci].value =
                        Some(format!("text/html; charset={enc}"));
                    charset_found = true;
                }
                current = self.tree.next(node);
                continue;
            }
            // Both charset and http-equiv: discard with a warning.
            let prev = self.tree.prev(node);
            self.report(Some(head), Some(node), Code::DiscardingUnexpected);
            self.tree.discard_element(node);
            current = match prev {
                Some(p) => self.tree.next(p),
                None => self.tree.content(head),
            };
        }
        if self.opts.add_meta_charset && !charset_found {
            let meta = self.inferred_tag(TagId::META);
            match self.html_version() {
                HT50 | XH50 => {
                    self.add_attribute(meta, "charset", Some(enc));
                }
                _ => {
                    self.add_attribute(meta, "http-equiv", Some("Content-Type"));
                    self.add_attribute(
                        meta,
                        "content",
                        Some(&format!("text/html; {charset_string}")),
                    );
                }
            }
            self.tree.insert_node_at_start(head, meta);
            self.report(Some(meta), Some(head), Code::AddedMissingCharset);
        }
    }

    // ---- doctype ----------------------------------------------------------------

    /// `NewDocTypeNode`: a doctype before the html element.
    fn new_doctype_node(&mut self) -> Option<NodeId> {
        let html = self.find_html()?;
        let doctype = self.tree.alloc(super::node::Node::new(0, 0));
        self.tree.node_mut(doctype).ntype = NodeType::DocType;
        self.tree.insert_node_before_element(html, doctype);
        Some(doctype)
    }

    /// `TY_(SetXHTMLDocType)`.
    fn set_xhtml_doctype(&mut self) -> bool {
        let mut doctype = self.find_doctype();
        let dtmode = self.opts.doctype_mode;
        self.lexer.version_emitted = self.apparent_version();
        if dtmode == DoctypeMode::Omit {
            if let Some(d) = doctype {
                self.tree.discard_element(d);
            }
            return true;
        }
        if dtmode == DoctypeMode::User && self.opts.doctype_user.is_none() {
            return false;
        }
        match doctype {
            None => {
                doctype = self.new_doctype_node();
                if let Some(d) = doctype {
                    self.tree.node_mut(d).element = Some("html".to_string());
                }
            }
            Some(d) => {
                let lowered = self
                    .tree
                    .node(d)
                    .element
                    .as_deref()
                    .map(str::to_ascii_lowercase);
                self.tree.node_mut(d).element = lowered;
            }
        }
        let Some(doctype) = doctype else {
            return false;
        };
        match dtmode {
            DoctypeMode::Html5 => {
                self.repair_attr_value(doctype, "PUBLIC", None);
                self.repair_attr_value(doctype, "SYSTEM", None);
                self.lexer.version_emitted = XH50;
            }
            DoctypeMode::Strict => {
                self.repair_attr_value(doctype, "PUBLIC", tables::fpi_from_vers(X10S));
                self.repair_attr_value(doctype, "SYSTEM", tables::si_from_vers(X10S));
                self.lexer.version_emitted = X10S;
            }
            DoctypeMode::Loose => {
                self.repair_attr_value(doctype, "PUBLIC", tables::fpi_from_vers(X10T));
                self.repair_attr_value(doctype, "SYSTEM", tables::si_from_vers(X10T));
                self.lexer.version_emitted = X10T;
            }
            DoctypeMode::User => {
                let user = self.opts.doctype_user.clone();
                self.repair_attr_value(doctype, "PUBLIC", user.as_deref());
                self.repair_attr_value(doctype, "SYSTEM", Some(""));
            }
            DoctypeMode::Auto => {
                let versions = self.lexer.versions;
                let given = self.lexer.doctype;
                if given == VERS_UNKNOWN || given == VERS_HTML5 {
                    self.lexer.version_emitted = XH50;
                    return true;
                } else if (versions & XH11) != 0 && given == XH11 {
                    if self.tree.attr_by_name(doctype, "SYSTEM").is_none() {
                        self.repair_attr_value(doctype, "SYSTEM", tables::si_from_vers(XH11));
                    }
                    self.lexer.version_emitted = XH11;
                    return true;
                } else if (versions & XH11) != 0 && (versions & VERS_HTML40) == 0 {
                    self.repair_attr_value(doctype, "PUBLIC", tables::fpi_from_vers(XH11));
                    self.repair_attr_value(doctype, "SYSTEM", tables::si_from_vers(XH11));
                    self.lexer.version_emitted = XH11;
                } else if (versions & XB10) != 0 && given == XB10 {
                    if self.tree.attr_by_name(doctype, "SYSTEM").is_none() {
                        self.repair_attr_value(doctype, "SYSTEM", tables::si_from_vers(XB10));
                    }
                    self.lexer.version_emitted = XB10;
                    return true;
                } else if (versions & VERS_HTML40_STRICT) != 0 {
                    self.repair_attr_value(doctype, "PUBLIC", tables::fpi_from_vers(X10S));
                    self.repair_attr_value(doctype, "SYSTEM", tables::si_from_vers(X10S));
                    self.lexer.version_emitted = X10S;
                } else if (versions & VERS_FRAMESET) != 0 {
                    self.repair_attr_value(doctype, "PUBLIC", tables::fpi_from_vers(X10F));
                    self.repair_attr_value(doctype, "SYSTEM", tables::si_from_vers(X10F));
                    self.lexer.version_emitted = X10F;
                } else if (versions & VERS_LOOSE) != 0 {
                    self.repair_attr_value(doctype, "PUBLIC", tables::fpi_from_vers(X10T));
                    self.repair_attr_value(doctype, "SYSTEM", tables::si_from_vers(X10T));
                    self.lexer.version_emitted = X10T;
                } else if (versions & VERS_HTML5) != 0 {
                    // nothing to do
                } else {
                    self.tree.discard_element(doctype);
                    return false;
                }
            }
            DoctypeMode::Omit => {}
        }
        false
    }

    /// `TY_(FixDocType)`.
    fn fix_doctype(&mut self) -> bool {
        let mut doctype = self.find_doctype();
        let dtmode = self.opts.doctype_mode;
        if doctype.is_some() && dtmode == DoctypeMode::Auto && self.lexer.doctype == VERS_HTML5 {
            self.lexer.version_emitted = HT50;
            return true;
        }
        if dtmode == DoctypeMode::Auto
            && (self.lexer.versions & self.lexer.doctype) != 0
            && ((VERS_XHTML & self.lexer.doctype) == 0 || self.lexer.isvoyager)
            && doctype.is_some()
        {
            self.lexer.version_emitted = self.lexer.doctype;
            return true;
        }
        if dtmode == DoctypeMode::Omit {
            if let Some(d) = doctype {
                self.tree.discard_element(d);
            }
            self.lexer.version_emitted = self.apparent_version();
            return true;
        }
        if self.opts.output_xml {
            return true;
        }
        let had_si = doctype.is_some_and(|d| self.tree.attr_by_name(d, "SYSTEM").is_some());
        if matches!(dtmode, DoctypeMode::Strict | DoctypeMode::Loose)
            && let Some(d) = doctype
        {
            self.tree.discard_element(d);
            doctype = None;
        }
        let guessed = match dtmode {
            DoctypeMode::Html5 => HT50,
            DoctypeMode::Strict => tables::H41S,
            DoctypeMode::Loose => tables::H41T,
            DoctypeMode::Auto => self.html_version(),
            DoctypeMode::Omit | DoctypeMode::User => VERS_UNKNOWN,
        };
        self.lexer.version_emitted = guessed;
        if guessed == VERS_UNKNOWN {
            return false;
        }
        let doctype = if let Some(d) = doctype {
            let lowered = self
                .tree
                .node(d)
                .element
                .as_deref()
                .map(str::to_ascii_lowercase);
            self.tree.node_mut(d).element = lowered;
            d
        } else {
            let Some(d) = self.new_doctype_node() else {
                return false;
            };
            self.tree.node_mut(d).element = Some("html".to_string());
            d
        };
        self.repair_attr_value(doctype, "PUBLIC", tables::fpi_from_vers(guessed));
        if had_si {
            self.repair_attr_value(doctype, "SYSTEM", tables::si_from_vers(guessed));
        }
        true
    }

    // ---- anchors, namespace, language ------------------------------------------

    /// `TY_(FixAnchors)`.
    fn fix_anchors(&mut self, mut node: Option<NodeId>, want_name: bool, want_id: bool) {
        let known = tables::tables().known;
        while let Some(n) = node {
            let next = self.tree.next(n);
            if self.is_anchor_element(n) {
                let name_i = self.tree.attr_by_id(n, known.name);
                let id_i = self.tree.attr_by_id(n, known.id);
                let had_name = name_i.is_some();
                let had_id = id_i.is_some();
                let mut id_emitted = false;
                let mut name_emitted = false;
                match (name_i, id_i) {
                    (Some(ni), Some(ii)) => {
                        let name = self.tree.node(n).attributes[ni].clone();
                        let id = self.tree.node(n).attributes[ii].clone();
                        if name.has_value() != id.has_value()
                            || (name.has_value() && id.has_value() && name.value != id.value)
                        {
                            self.report_attr(Some(n), Some(&name), Code::IdNameMismatch);
                        }
                    }
                    (Some(ni), None) if want_id => {
                        if (self.node_attribute_versions(n, known.id) & self.lexer.version_emitted)
                            != 0
                        {
                            let name = self.tree.node(n).attributes[ni].clone();
                            let value = name.value.clone().unwrap_or_default();
                            if is_valid_html_id_str(&value) {
                                self.repair_attr_value(n, "id", Some(&value));
                                id_emitted = true;
                            } else {
                                self.report_attr(Some(n), Some(&name), Code::InvalidXmlId);
                            }
                        }
                    }
                    (None, Some(ii))
                        if want_name
                            && (self.node_attribute_versions(n, known.name)
                                & self.lexer.version_emitted)
                                != 0 =>
                    {
                        let value = self.tree.node(n).attributes[ii]
                            .value
                            .clone()
                            .unwrap_or_default();
                        self.repair_attr_value(n, "name", Some(&value));
                        name_emitted = true;
                    }
                    _ => {}
                }
                if let Some(ii) = self.tree.attr_by_id(n, known.id)
                    && !want_id
                    && (had_name || !want_name || name_emitted)
                {
                    self.remove_attribute(n, ii);
                }
                if let Some(ni) = self.tree.attr_by_id(n, known.name)
                    && !want_name
                    && (had_id || !want_id || id_emitted)
                {
                    self.remove_attribute(n, ni);
                }
            }
            if let Some(content) = self.tree.content(n) {
                self.fix_anchors(Some(content), want_name, want_id);
            }
            node = next;
        }
    }

    /// `TY_(FixXhtmlNamespace)`.
    fn fix_xhtml_namespace(&mut self, want_xmlns: bool) {
        let Some(html) = self.find_html() else {
            return;
        };
        let known = tables::tables().known;
        let xmlns = self.tree.attr_by_id(html, known.xmlns);
        if want_xmlns {
            let has = xmlns.is_some_and(|i| {
                self.tree.node(html).attributes[i].value_is("http://www.w3.org/1999/xhtml")
            });
            if !has {
                self.repair_attr_value(html, "xmlns", Some("http://www.w3.org/1999/xhtml"));
            }
        } else if let Some(i) = xmlns {
            self.remove_attribute(html, i);
        }
    }

    /// `TY_(FixLanguageInformation)`.
    fn fix_language_information(
        &mut self,
        mut node: Option<NodeId>,
        want_xml_lang: bool,
        want_lang: bool,
    ) {
        let known = tables::tables().known;
        while let Some(n) = node {
            let next = self.tree.next(n);
            if self.tree.node(n).is_element() {
                let lang = self.tree.attr_by_id(n, known.lang);
                let xml_lang = self.tree.attr_by_id(n, known.xml_lang);
                match (lang, xml_lang) {
                    (Some(_), Some(_)) => {}
                    (Some(li), None) if want_xml_lang => {
                        if (self.node_attribute_versions(n, known.xml_lang)
                            & self.lexer.version_emitted)
                            != 0
                        {
                            let value = self.tree.node(n).attributes[li].value.clone();
                            self.repair_attr_value(n, "xml:lang", value.as_deref());
                        }
                    }
                    (None, Some(xi))
                        if want_lang
                            && (self.node_attribute_versions(n, known.lang)
                                & self.lexer.version_emitted)
                                != 0 =>
                    {
                        let value = self.tree.node(n).attributes[xi].value.clone();
                        self.repair_attr_value(n, "lang", value.as_deref());
                    }
                    _ => {}
                }
                if !want_lang && let Some(i) = self.tree.attr_by_id(n, known.lang) {
                    self.remove_attribute(n, i);
                }
                if !want_xml_lang && let Some(i) = self.tree.attr_by_id(n, known.xml_lang) {
                    self.remove_attribute(n, i);
                }
            }
            if let Some(content) = self.tree.content(n) {
                self.fix_language_information(Some(content), want_xml_lang, want_lang);
            }
            node = next;
        }
    }

    // ---- HTML5 and version checks ------------------------------------------------

    /// `CheckHTML5`: elements removed from HTML5, with the attributes
    /// `--strict-tags-attributes` would otherwise report.
    fn check_html5(&mut self, mut node: Option<NodeId>) {
        let known = tables::tables().known;
        let already_strict = self.opts.strict_tags_attributes;
        let body = self.find_body();
        while let Some(n) = node {
            if let Some(ai) = self.tree.attr_by_id(n, known.align)
                && !already_strict
            {
                let av = self.tree.node(n).attributes[ai].clone();
                self.report_attr(Some(n), Some(&av), Code::MismatchedAttributeWarn);
            }
            if Some(n) == body {
                if !already_strict {
                    for attr in [
                        known.background,
                        known.bgcolor,
                        known.text,
                        known.link,
                        known.vlink,
                        known.alink,
                    ] {
                        if let Some(i) = self.tree.attr_by_id(n, attr) {
                            let av = self.tree.node(n).attributes[i].clone();
                            self.report_attr(Some(n), Some(&av), Code::MismatchedAttributeWarn);
                        }
                    }
                }
            } else {
                let id = self.tag_id(n);
                let removed = matches!(
                    id,
                    TagId::ACRONYM
                        | TagId::APPLET
                        | TagId::BASEFONT
                        | TagId::BIG
                        | TagId::CENTER
                        | TagId::DIR
                        | TagId::FONT
                        | TagId::FRAME
                        | TagId::FRAMESET
                        | TagId::NOFRAMES
                        | TagId::STRIKE
                        | TagId::TT
                );
                if removed {
                    if !already_strict {
                        self.report(Some(n), Some(n), Code::RemovedHtml5);
                    }
                } else if self.tree.node(n).is_element()
                    && let Some(tag) = self.tree.node(n).tag
                {
                    let versions = self.tags[tag].versions;
                    if (versions & VERS_HTML5) == 0
                        && (versions & VERS_PROPRIETARY) == 0
                        && !already_strict
                    {
                        self.report(Some(n), Some(n), Code::RemovedHtml5);
                    }
                }
            }
            if let Some(content) = self.tree.content(n) {
                self.check_html5(Some(content));
            }
            node = self.tree.next(n);
        }
    }

    /// `CheckHTMLTagsAttribsVersions`: proprietary elements and attributes,
    /// and version mismatches.
    fn check_html_tags_attribs_versions(&mut self, mut node: Option<NodeId>) {
        let version_emitted = self.lexer.version_emitted;
        let declared = self.lexer.doctype;
        let version = if version_emitted == 0 {
            declared
        } else {
            version_emitted
        };
        let tag_report = if (VERS_STRICT & version) != 0 {
            Code::ElementVersMismatchError
        } else {
            Code::ElementVersMismatchWarn
        };
        let attr_report = if (VERS_STRICT & version) != 0 {
            Code::MismatchedAttributeError
        } else {
            Code::MismatchedAttributeWarn
        };
        let check_versions = self.opts.strict_tags_attributes;
        let html_is5 = (self.lexer.doctype & VERS_HTML5) != 0;
        while let Some(n) = node {
            if self.tree.node(n).is_element()
                && let Some(tag) = self.tree.node(n).tag
            {
                let versions = self.tags[tag].versions;
                if check_versions && (versions & version) == 0 {
                    self.report(None, Some(n), tag_report);
                } else if (versions & VERS_PROPRIETARY) != 0
                    && (!self.opts.make_clean
                        || (!self.node_is(n, TagId::NOBR) && !self.node_is(n, TagId::WBR)))
                {
                    let looks_custom = self.node_is_autonomous_custom_format(n);
                    if !html_is5 || !looks_custom {
                        self.report(None, Some(n), Code::ProprietaryElement);
                    }
                    if self.node_is(n, TagId::LAYER) {
                        self.bad_layout |= USING_LAYER;
                    } else if self.node_is(n, TagId::SPACER) {
                        self.bad_layout |= USING_SPACER;
                    } else if self.node_is(n, TagId::NOBR) {
                        self.bad_layout |= USING_NOBR;
                    }
                }
            }
            if self.tree.node(n).is_element() {
                let mut i = 0;
                while i < self.tree.node(n).attributes.len() {
                    let av = self.tree.node(n).attributes[i].clone();
                    let proprietary = self.attribute_is_proprietary(n, &av);
                    let mismatched = if check_versions || html_is5 {
                        self.attribute_is_mismatched(n, &av)
                    } else {
                        false
                    };
                    if proprietary {
                        if self.opts.warn_proprietary_attributes {
                            self.report_attr(Some(n), Some(&av), Code::ProprietaryAttribute);
                        }
                    } else if mismatched {
                        if html_is5 {
                            let code = if check_versions {
                                Code::MismatchedAttributeError
                            } else {
                                Code::MismatchedAttributeWarn
                            };
                            self.report_attr(Some(n), Some(&av), code);
                        } else {
                            self.report_attr(Some(n), Some(&av), attr_report);
                        }
                    }
                    if (proprietary || mismatched) && self.opts.drop_proprietary_attributes {
                        self.remove_attribute(n, i);
                        continue;
                    }
                    i += 1;
                }
            }
            if let Some(content) = self.tree.content(n) {
                self.check_html_tags_attribs_versions(Some(content));
            }
            node = self.tree.next(n);
        }
    }

    /// `TY_(CleanHead)`: discard multiple title elements.
    fn clean_head(&mut self) {
        if self.opts.show_body_only == TriState::Yes {
            return;
        }
        let Some(head) = self.find_head() else {
            return;
        };
        let mut titles = 0;
        let mut node = self.tree.content(head);
        while let Some(n) = node {
            let next = self.tree.next(n);
            if self.node_is(n, TagId::TITLE) {
                titles += 1;
                if titles > 1 {
                    self.report(Some(head), Some(n), Code::DiscardingUnexpected);
                    self.tree.discard_element(n);
                }
            }
            node = next;
        }
    }
}

fn is_valid_html_id_str(id: &str) -> bool {
    !id.chars().any(|c| super::lexer::is_html_space(c as u32))
}

/// `MergeProperties`: the sorted union of two style declaration lists,
/// the first value winning for a repeated property.
fn merge_properties(s1: &str, s2: &str) -> String {
    let mut props: Vec<(String, String)> = Vec::new();
    for style in [s1, s2] {
        for decl in style.split(';') {
            let Some((name, value)) = decl.split_once(':') else {
                continue;
            };
            let name = name.trim_start_matches(' ');
            let value = value.trim_start_matches(' ');
            if props.iter().any(|(n, _)| n == name) {
                continue;
            }
            props.push((name.to_string(), value.to_string()));
        }
    }
    props.sort_by(|a, b| a.0.cmp(&b.0));
    props
        .iter()
        .map(|(n, v)| format!("{n}: {v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

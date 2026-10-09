//! The parser (tidy's parser.c): one routine per content model that
//! consumes tokens, builds the tree, infers the start and end tags the
//! markup left out, moves misplaced content where browsers render it, and
//! reports every repair.

use super::Doc;
use super::lexer::{TokenMode, is_white};
use super::message::Code;
use super::node::{NodeId, NodeType};
use super::tables::{
    self, CM_BLOCK, CM_DEFLIST, CM_EMPTY, CM_FIELD, CM_FRAMES, CM_HEAD, CM_HEADING, CM_HTML,
    CM_INLINE, CM_LIST, CM_MIXED, CM_NEW, CM_OBJECT, CM_OPT, CM_PARAM, CM_ROW, CM_ROWGRP, CM_TABLE,
    ParserKind, TagId, VERS_FRAMESET, VERS_HTML5, VERS_HTML20, VERS_HTML40_STRICT,
    VERS_PROPRIETARY,
};
use super::{
    BA_INVALID_LINK_NOFRAMES, BA_USING_FRAMES, BA_USING_NOFRAMES, DoctypeMode, FLG_BAD_FORM,
    FLG_BAD_MAIN, FN_TRIM_EMPTY_ELEMENT, TriState, USING_FONT, USING_NOBR,
};

const XHTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";

impl Doc {
    fn showing_body_only(&self) -> bool {
        self.opts.show_body_only == TriState::Yes
    }

    fn ntype(&self, node: NodeId) -> NodeType {
        self.tree.node(node).ntype
    }

    fn is_element(&self, node: NodeId) -> bool {
        self.tree.node(node).is_element()
    }

    fn is_text(&self, node: NodeId) -> bool {
        self.tree.node(node).is_text()
    }

    fn has_tag(&self, node: NodeId) -> bool {
        self.tree.node(node).tag.is_some()
    }

    fn same_tag(&self, a: NodeId, b: NodeId) -> bool {
        let ta = self.tree.node(a).tag;
        ta.is_some() && ta == self.tree.node(b).tag
    }

    /// `node->tag == element->tag` where either may be untagged (then
    /// equal only if both are).
    fn tags_equal(&self, a: NodeId, b: NodeId) -> bool {
        self.tree.node(a).tag == self.tree.node(b).tag
    }

    /// `TY_(CoerceNode)`: turn a node into another element, reporting it.
    pub(crate) fn coerce_node(
        &mut self,
        node: NodeId,
        id: TagId,
        obsolete: bool,
        unexpected: bool,
    ) {
        let tmp = self.inferred_tag(id);
        if obsolete {
            self.report(Some(node), Some(tmp), Code::ObsoleteElement);
        } else if unexpected {
            self.report(Some(node), Some(tmp), Code::ReplacingUnexElement);
        } else {
            self.report(Some(node), Some(tmp), Code::ReplacingElement);
        }
        let index = self.tag_index(id);
        let name = self.tags[index].name.clone();
        let n = self.tree.node_mut(node);
        n.was = n.tag;
        n.tag = Some(index);
        n.ntype = NodeType::StartTag;
        n.implicit = true;
        n.element = Some(name);
    }

    /// `CanPrune`.
    fn can_prune(&self, element: NodeId) -> bool {
        if !self.opts.drop_empty_elements {
            return false;
        }
        if self.is_text(element) {
            return true;
        }
        if self.tree.content(element).is_some() {
            return false;
        }
        let Some(tag) = self.tree.node(element).tag else {
            return false;
        };
        let model = self.tags[tag].model;
        let has_attrs = !self.tree.node(element).attributes.is_empty();
        if (model & CM_BLOCK) != 0 && has_attrs {
            return false;
        }
        if self.node_is(element, TagId::A) && has_attrs {
            return false;
        }
        if self.node_is(element, TagId::P) && !self.opts.drop_empty_paras {
            return false;
        }
        if (model & CM_ROW) != 0 || (model & CM_EMPTY) != 0 {
            return false;
        }
        let id = self.tags[tag].id;
        if matches!(id, TagId::APPLET | TagId::OBJECT) {
            return false;
        }
        let known = tables::tables().known;
        if id == TagId::SCRIPT && self.tree.attr_by_id(element, known.src).is_some() {
            return false;
        }
        if matches!(
            id,
            TagId::TITLE | TagId::IFRAME | TagId::TEXTAREA | TagId::CANVAS | TagId::PROGRESS
        ) {
            return false;
        }
        if self.tree.attr_by_id(element, known.id).is_some()
            || self.tree.attr_by_id(element, known.name).is_some()
        {
            return false;
        }
        if self.tree.attr_by_id(element, known.datafld).is_some() {
            return false;
        }
        if id == TagId::UNKNOWN {
            return false;
        }
        if matches!(id, TagId::BODY | TagId::COLGROUP) {
            return false;
        }
        if id == TagId::OPTION && has_attrs {
            return false;
        }
        if id == TagId::DD {
            return false;
        }
        true
    }

    /// `TY_(TrimEmptyElement)`: returns the next node.
    pub(crate) fn trim_empty_element(&mut self, element: NodeId) -> Option<NodeId> {
        if self.can_prune(element) {
            if !self.is_text(element) {
                self.footnotes |= FN_TRIM_EMPTY_ELEMENT;
                self.report(Some(element), None, Code::TrimEmptyElement);
            }
            return self.tree.discard_element(element);
        }
        self.tree.next(element)
    }

    /// `TY_(DropEmptyElements)`.
    fn drop_empty_elements(&mut self, mut node: Option<NodeId>) {
        while let Some(n) = node {
            let next = self.tree.next(n);
            if let Some(content) = self.tree.content(n) {
                self.drop_empty_elements(Some(content));
            }
            let nd = self.tree.node(n);
            let empty_text = nd.is_text() && nd.start >= nd.end;
            if !nd.is_element() && !empty_text {
                node = next;
                continue;
            }
            node = self.trim_empty_element(n);
        }
    }

    const fn bad_form(&mut self) {
        self.bad_form |= FLG_BAD_FORM;
    }

    /// `TrimTrailingSpace`.
    fn trim_trailing_space(&mut self, element: NodeId, last: NodeId) {
        if !self.is_text(last) {
            return;
        }
        let (start, end) = {
            let n = self.tree.node(last);
            (n.start, n.end)
        };
        if end > start && self.lexer.lexbuf[end - 1] == b' ' {
            self.tree.node_mut(last).end -= 1;
            let model = self.node_model(element);
            if (model & CM_INLINE) != 0 && (model & CM_FIELD) == 0 {
                self.lexer.insertspace = true;
            }
        }
    }

    /// `TrimInitialSpace`.
    fn trim_initial_space(&mut self, element: NodeId, text: NodeId) {
        let (tstart, tend) = {
            let n = self.tree.node(text);
            (n.start, n.end)
        };
        if !(self.is_text(text) && tstart < tend && self.lexer.lexbuf[tstart] == b' ') {
            return;
        }
        let model = self.node_model(element);
        if (model & CM_INLINE) != 0 && (model & CM_FIELD) == 0 {
            let prev = self.tree.prev(element);
            if let Some(p) = prev.filter(|&p| self.is_text(p)) {
                let pend = self.tree.node(p).end;
                if pend == 0 || self.lexer.lexbuf[pend - 1] != b' ' {
                    self.lexbuf_set(pend, b' ');
                    self.tree.node_mut(p).end += 1;
                }
                self.tree.node_mut(element).start += 1;
            } else {
                let node = self.new_node();
                let estart = self.tree.node(element).start;
                self.tree.node_mut(element).start += 1;
                {
                    let n = self.tree.node_mut(node);
                    n.start = estart;
                    n.end = estart + 1;
                }
                self.lexbuf_set(estart, b' ');
                self.tree.insert_node_before_element(element, node);
            }
        }
        self.tree.node_mut(text).start += 1;
    }

    /// Write one byte of the lexer buffer, growing it if at the end.
    fn lexbuf_set(&mut self, index: usize, value: u8) {
        if index < self.lexer.lexbuf.len() {
            self.lexer.lexbuf[index] = value;
        } else {
            while self.lexer.lexbuf.len() < index {
                self.lexer.lexbuf.push(b' ');
            }
            self.lexer.lexbuf.push(value);
        }
    }

    /// `IsPreDescendant`.
    fn is_pre_descendant(&self, node: NodeId) -> bool {
        let mut parent = self.tree.parent(node);
        while let Some(p) = parent {
            if self.node_parser(p) == Some(ParserKind::ParsePre) {
                return true;
            }
            parent = self.tree.parent(p);
        }
        false
    }

    /// `CleanTrailingWhitespace`.
    fn clean_trailing_whitespace(&self, node: NodeId) -> bool {
        if !self.is_text(node) {
            return false;
        }
        let parent = self.tree.parent(node).expect("text nodes have parents");
        if self.ntype(parent) == NodeType::DocType {
            return false;
        }
        if self.is_pre_descendant(node) {
            return false;
        }
        if self.node_parser(parent) == Some(ParserKind::ParseScript) {
            return false;
        }
        let next = self.tree.next(node);
        if next.is_none() && !self.node_has_cm(parent, CM_INLINE) {
            return true;
        }
        if next.is_none()
            && let Some(pn) = self.tree.next(parent)
            && !self.node_has_cm(pn, CM_INLINE)
        {
            return true;
        }
        let Some(next) = next else {
            return false;
        };
        if self.node_is(next, TagId::BR) {
            return true;
        }
        if self.node_has_cm(next, CM_INLINE) {
            return false;
        }
        if self.ntype(next) == NodeType::StartTag || self.ntype(next) == NodeType::StartEndTag {
            return true;
        }
        let nn = self.tree.node(next);
        if nn.is_text() && nn.start < nn.end && is_white(u32::from(self.lexer.lexbuf[nn.start])) {
            return true;
        }
        false
    }

    /// `CleanLeadingWhitespace`.
    fn clean_leading_whitespace(&self, node: NodeId) -> bool {
        if !self.is_text(node) {
            return false;
        }
        let parent = self.tree.parent(node).expect("text nodes have parents");
        if self.ntype(parent) == NodeType::DocType {
            return false;
        }
        if self.is_pre_descendant(node) {
            return false;
        }
        if self.node_parser(parent) == Some(ParserKind::ParseScript) {
            return false;
        }
        let prev = self.tree.prev(node);
        if self.opt_node_is(prev, TagId::BR) {
            return true;
        }
        if prev.is_none() && !self.node_has_cm(parent, CM_INLINE) {
            return true;
        }
        if let Some(p) = prev
            && !self.node_has_cm(p, CM_INLINE)
            && self.is_element(p)
        {
            return true;
        }
        if prev.is_none()
            && self.tree.prev(parent).is_none()
            && let Some(gp) = self.tree.parent(parent)
            && !self.node_has_cm(gp, CM_INLINE)
        {
            return true;
        }
        false
    }

    /// `CleanSpaces`.
    fn clean_spaces(&mut self, mut node: Option<NodeId>) {
        while let Some(n) = node {
            let next = self.tree.next(n);
            if self.is_text(n) && self.clean_leading_whitespace(n) {
                while {
                    let nd = self.tree.node(n);
                    nd.start < nd.end && is_white(u32::from(self.lexer.lexbuf[nd.start]))
                } {
                    self.tree.node_mut(n).start += 1;
                }
            }
            if self.is_text(n) && self.clean_trailing_whitespace(n) {
                while {
                    let nd = self.tree.node(n);
                    nd.end > nd.start && is_white(u32::from(self.lexer.lexbuf[nd.end - 1]))
                } {
                    self.tree.node_mut(n).end -= 1;
                }
            }
            {
                let nd = self.tree.node(n);
                if nd.is_text() && nd.start >= nd.end {
                    self.tree.remove_node(n);
                    node = next;
                    continue;
                }
            }
            if let Some(content) = self.tree.content(n) {
                self.clean_spaces(Some(content));
            }
            node = next;
        }
    }

    /// `TrimSpaces`.
    fn trim_spaces(&mut self, element: NodeId) {
        if self.node_is(element, TagId::PRE) || self.is_pre_descendant(element) {
            return;
        }
        if let Some(text) = self.tree.content(element)
            && self.is_text(text)
        {
            self.trim_initial_space(element, text);
        }
        if let Some(text) = self.tree.last(element)
            && self.is_text(text)
        {
            self.trim_trailing_space(element, text);
        }
    }

    fn descendant_of(&self, element: NodeId, id: TagId) -> bool {
        let mut parent = self.tree.parent(element);
        while let Some(p) = parent {
            if self.node_is(p, id) {
                return true;
            }
            parent = self.tree.parent(p);
        }
        false
    }

    /// `InsertMisc`: comments, processing instructions and the like go
    /// straight into the tree.
    fn insert_misc(&mut self, element: NodeId, node: NodeId) -> bool {
        match self.ntype(node) {
            NodeType::Comment
            | NodeType::ProcIns
            | NodeType::Cdata
            | NodeType::Section
            | NodeType::Asp
            | NodeType::Jste
            | NodeType::Php => {
                self.tree.insert_node_at_end(element, node);
                return true;
            }
            NodeType::XmlDecl => {
                let root = 0;
                let first = self.tree.content(root);
                if !first.is_some_and(|f| self.ntype(f) == NodeType::XmlDecl) {
                    self.tree.insert_node_at_start(root, node);
                    return true;
                }
            }
            _ => {}
        }
        if let Some(tag) = self.tree.node(node).tag
            && self.is_element(node)
            && (self.tags[tag].model & CM_EMPTY) != 0
            && self.tags[tag].id == TagId::UNKNOWN
            && (self.tags[tag].versions & VERS_PROPRIETARY) != 0
        {
            self.tree.insert_node_at_end(element, node);
            return true;
        }
        false
    }

    /// `ParseTag`: dispatch to the tag's parser.
    pub(crate) fn parse_tag(&mut self, node: NodeId, mode: TokenMode) {
        let Some(tag) = self.tree.node(node).tag else {
            return;
        };
        let model = self.tags[tag].model;
        let parser = self.tags[tag].parser;
        if (model & CM_EMPTY) != 0 {
            self.lexer.waswhite = false;
            if parser.is_none() {
                return;
            }
        } else if (model & CM_INLINE) == 0 {
            self.lexer.insertspace = false;
        }
        let Some(parser) = parser else {
            return;
        };
        if self.ntype(node) == NodeType::StartEndTag {
            return;
        }
        self.lexer.parent = Some(node);
        match parser {
            ParserKind::ParseBlock => self.parse_block(node, mode),
            ParserKind::ParseBody => self.parse_body(node, mode),
            ParserKind::ParseColGroup => self.parse_colgroup(node),
            ParserKind::ParseDatalist => self.parse_datalist(node),
            ParserKind::ParseDefList => self.parse_deflist(node, mode),
            ParserKind::ParseEmpty => self.parse_empty(node, mode),
            ParserKind::ParseFrameSet => self.parse_frameset(node),
            ParserKind::ParseHTML => self.parse_html(node, mode),
            ParserKind::ParseHead => self.parse_head(node),
            ParserKind::ParseInline => self.parse_inline(node, mode),
            ParserKind::ParseList => self.parse_list(node),
            ParserKind::ParseNamespace => self.parse_namespace(node),
            ParserKind::ParseNoFrames => self.parse_noframes(node),
            ParserKind::ParseOptGroup => self.parse_optgroup(node),
            ParserKind::ParsePre => self.parse_pre(node),
            ParserKind::ParseRow => self.parse_row(node),
            ParserKind::ParseRowGroup => self.parse_rowgroup(node),
            ParserKind::ParseScript => self.parse_script(node),
            ParserKind::ParseSelect => self.parse_select(node),
            ParserKind::ParseTableTag => self.parse_table(node),
            ParserKind::ParseText => self.parse_text(node, mode),
            ParserKind::ParseTitle => self.parse_title(node),
        }
    }

    /// `InsertDocType`: a doctype found after other tags.
    fn insert_doctype(&mut self, element: NodeId, doctype: NodeId) {
        if self.find_doctype().is_some() {
            self.report(Some(element), Some(doctype), Code::DiscardingUnexpected);
        } else {
            self.report(Some(element), Some(doctype), Code::DoctypeAfterTags);
            let mut e = element;
            while !self.node_is(e, TagId::HTML) {
                e = self.tree.parent(e).expect("an html ancestor exists");
            }
            self.tree.insert_node_before_element(e, doctype);
        }
    }

    /// `MoveToHead`.
    fn move_to_head(&mut self, element: NodeId, node: NodeId) {
        self.tree.remove_node(node);
        if self.is_element(node) {
            self.report(Some(element), Some(node), Code::TagNotAllowedIn);
            let head = self
                .find_head()
                .expect("a head exists when content is moved to it");
            self.tree.insert_node_at_end(head, node);
            if self.node_parser(node).is_some() {
                self.parse_tag(node, TokenMode::IgnoreWhitespace);
            }
        } else {
            self.report(Some(element), Some(node), Code::DiscardingUnexpected);
        }
    }

    /// `MoveNodeToBody`.
    fn move_node_to_body(&mut self, node: NodeId) {
        if let Some(body) = self.find_body() {
            self.tree.remove_node(node);
            self.tree.insert_node_at_end(body, node);
        }
    }

    /// `AddClassNoIndent`.
    fn add_class_no_indent(&mut self, node: NodeId) {
        if !self.opts.decorate_inferred_ul {
            return;
        }
        self.add_style_property(
            node,
            "padding-left: 2ex; margin-left: 0ex; margin-top: 0ex; margin-bottom: 0ex",
        );
    }

    // ---- ParseBlock -----------------------------------------------------------

    /// `TY_(ParseBlock)`.
    #[allow(
        clippy::too_many_lines,
        reason = "a one-to-one port of tidy's ParseBlock"
    )]
    fn parse_block(&mut self, element: NodeId, mut mode: TokenMode) {
        let emodel = self.node_model(element);
        if (emodel & CM_EMPTY) != 0 {
            return;
        }
        if self.node_is(element, TagId::FORM) && self.descendant_of(element, TagId::FORM) {
            self.report(Some(element), None, Code::IllegalNesting);
        }
        let mut istackbase = 0;
        if (emodel & CM_OBJECT) != 0 {
            istackbase = self.lexer.istackbase;
            self.lexer.istackbase = self.lexer.istack.len();
        }
        if (emodel & CM_MIXED) == 0 {
            self.inline_dup(None);
        }
        if (emodel & CM_INLINE) == 0 || (emodel & CM_FIELD) != 0 {
            mode = TokenMode::IgnoreWhitespace;
        } else if mode == TokenMode::IgnoreWhitespace {
            mode = TokenMode::MixedContent;
        }
        let mut checkstack = true;
        let mut node_opt;
        loop {
            node_opt = self.get_token(mode);
            let Some(mut node) = node_opt else {
                break;
            };
            // end tag for this element
            if self.ntype(node) == NodeType::EndTag
                && self.has_tag(node)
                && (self.same_tag(node, element)
                    || self.tree.node(element).was == self.tree.node(node).tag)
            {
                if (emodel & CM_OBJECT) != 0 {
                    while self.lexer.istack.len() > self.lexer.istackbase {
                        self.pop_inline(None);
                    }
                    self.lexer.istackbase = istackbase;
                }
                self.tree.node_mut(element).closed = true;
                self.trim_spaces(element);
                return;
            }

            if self.node_is(node, TagId::HTML)
                || self.node_is(node, TagId::HEAD)
                || self.node_is(node, TagId::BODY)
            {
                if self.is_element(node) {
                    self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                }
                continue;
            }

            if self.ntype(node) == NodeType::EndTag {
                if !self.has_tag(node) {
                    self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                    continue;
                } else if self.node_is(node, TagId::BR) {
                    self.tree.node_mut(node).ntype = NodeType::StartTag;
                } else if self.node_is(node, TagId::P) {
                    let n = self.tree.node_mut(node);
                    n.ntype = NodeType::StartEndTag;
                    n.implicit = true;
                } else if self
                    .tree
                    .node(node)
                    .tag
                    .is_some_and(|t| self.tree.descendant_of(element, t))
                {
                    self.unget_token();
                    break;
                } else if self.lexer.exiled
                    && (self.node_has_cm(node, CM_TABLE) || self.node_is(node, TagId::TABLE))
                {
                    self.unget_token();
                    self.trim_spaces(element);
                    return;
                }
            }

            // mixed content model permits text
            if self.is_text(node) {
                if checkstack {
                    checkstack = false;
                    if (emodel & CM_MIXED) == 0 && self.inline_dup(Some(node)) > 0 {
                        continue;
                    }
                }
                self.tree.insert_node_at_end(element, node);
                mode = TokenMode::MixedContent;
                if matches!(
                    self.tag_id(element),
                    TagId::BODY | TagId::MAP | TagId::BLOCKQUOTE | TagId::FORM | TagId::NOSCRIPT
                ) {
                    self.constrain_version(!VERS_HTML40_STRICT);
                }
                continue;
            }

            if self.insert_misc(element, node) {
                continue;
            }

            if self.node_is(node, TagId::PARAM) {
                if self.node_has_cm(element, CM_PARAM) && self.is_element(node) {
                    self.tree.insert_node_at_end(element, node);
                    continue;
                }
                self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                continue;
            }

            if self.node_is(node, TagId::AREA) {
                if self.node_is(element, TagId::MAP) && self.is_element(node) {
                    self.tree.insert_node_at_end(element, node);
                    continue;
                }
                self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                continue;
            }

            if !self.has_tag(node) {
                self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                continue;
            }

            if !self.node_has_cm(node, CM_INLINE) {
                if !self.is_element(node) {
                    if self.node_is(node, TagId::FORM) {
                        self.bad_form();
                    }
                    self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                    continue;
                }

                if self.node_is(element, TagId::LI)
                    && matches!(
                        self.tag_id(node),
                        TagId::FRAME | TagId::FRAMESET | TagId::OPTGROUP | TagId::OPTION
                    )
                {
                    self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                    continue;
                }

                if self.node_is(element, TagId::TD) || self.node_is(element, TagId::TH) {
                    if self.node_has_cm(node, CM_HEAD) {
                        self.move_to_head(element, node);
                        continue;
                    }
                    if self.node_has_cm(node, CM_LIST) {
                        self.unget_token();
                        node = self.inferred_tag(TagId::UL);
                        self.add_class_no_indent(node);
                        self.lexer.exclude_blocks = true;
                    } else if self.node_has_cm(node, CM_DEFLIST) {
                        self.unget_token();
                        node = self.inferred_tag(TagId::DL);
                        self.lexer.exclude_blocks = true;
                    }
                    if !self.node_has_cm(node, CM_BLOCK) {
                        self.unget_token();
                        self.trim_spaces(element);
                        return;
                    }
                } else if self.node_has_cm(node, CM_BLOCK) {
                    if self.lexer.exclude_blocks {
                        if !self.node_has_cm(element, CM_OPT) {
                            self.report(Some(element), Some(node), Code::MissingEndtagBefore);
                        }
                        self.unget_token();
                        if self.node_has_cm(element, CM_OBJECT) {
                            self.lexer.istackbase = istackbase;
                        }
                        self.trim_spaces(element);
                        return;
                    }
                } else {
                    if self.node_has_cm(node, CM_HEAD) {
                        self.move_to_head(element, node);
                        continue;
                    }

                    if self.node_is(element, TagId::FORM)
                        && let Some(parent) = self.tree.parent(element)
                        && self.node_is(parent, TagId::TD)
                        && self.tree.node(parent).implicit
                    {
                        if self.node_is(node, TagId::TD) {
                            self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                            continue;
                        }
                        if self.node_is(node, TagId::TH) {
                            self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                            let th = self.tag_index(TagId::TH);
                            let p = self.tree.node_mut(parent);
                            p.element = Some("th".to_string());
                            p.tag = Some(th);
                            continue;
                        }
                    }

                    if !self.node_has_cm(element, CM_OPT) && !self.tree.node(element).implicit {
                        self.report(Some(element), Some(node), Code::MissingEndtagBefore);
                    }
                    if !self.opts.omit_optional_tags && self.node_has_cm(element, CM_OPT) {
                        self.report(Some(element), Some(node), Code::MissingEndtagOptional);
                    }

                    self.unget_token();

                    if self.node_has_cm(node, CM_LIST) {
                        if let Some(parent) = self.tree.parent(element)
                            && self.node_parser(parent) == Some(ParserKind::ParseList)
                        {
                            self.trim_spaces(element);
                            return;
                        }
                        node = self.inferred_tag(TagId::UL);
                        self.add_class_no_indent(node);
                    } else if self.node_has_cm(node, CM_DEFLIST) {
                        if self
                            .tree
                            .parent(element)
                            .is_some_and(|p| self.node_is(p, TagId::DL))
                        {
                            self.trim_spaces(element);
                            return;
                        }
                        node = self.inferred_tag(TagId::DL);
                    } else if self.node_has_cm(node, CM_TABLE) || self.node_has_cm(node, CM_ROW) {
                        if self.lexer.exiled {
                            return;
                        }
                        node = self.inferred_tag(TagId::TABLE);
                    } else if self.node_has_cm(element, CM_OBJECT) {
                        while self.lexer.istack.len() > self.lexer.istackbase {
                            self.pop_inline(None);
                        }
                        self.lexer.istackbase = istackbase;
                        self.trim_spaces(element);
                        return;
                    } else {
                        self.trim_spaces(element);
                        return;
                    }
                }
            }

            // an <a> ends any open <a>
            if self.node_is(node, TagId::A)
                && !self.tree.node(node).implicit
                && (self.node_is(element, TagId::A) || self.descendant_of(element, TagId::A))
            {
                if self.ntype(node) != NodeType::EndTag
                    && self.tree.node(node).attributes.is_empty()
                    && self.opts.coerce_endtags
                {
                    self.tree.node_mut(node).ntype = NodeType::EndTag;
                    self.report(Some(element), Some(node), Code::CoerceToEndtag);
                    self.unget_token();
                    continue;
                }
                if self.node_is(element, TagId::A) {
                    self.report(Some(element), Some(node), Code::MissingEndtagBefore);
                    self.unget_token();
                } else {
                    self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                }
                if !mode.is_preformatted_bit() {
                    self.trim_spaces(element);
                }
                return;
            }

            // parse known element
            if self.is_element(node) {
                if self.node_has_cm(node, CM_INLINE) {
                    if checkstack && !self.tree.node(node).implicit {
                        checkstack = false;
                        if (emodel & CM_MIXED) == 0 && self.inline_dup(Some(node)) > 0 {
                            continue;
                        }
                    }
                    mode = TokenMode::MixedContent;
                } else {
                    checkstack = true;
                    mode = TokenMode::IgnoreWhitespace;
                }
                if self.node_is(node, TagId::BR) {
                    self.trim_spaces(element);
                }
                self.tree.insert_node_at_end(element, node);
                if self.tree.node(node).implicit {
                    self.report(Some(element), Some(node), Code::InsertingTag);
                }
                self.parse_tag(node, TokenMode::IgnoreWhitespace);
                continue;
            }

            // discard unexpected tags
            if self.ntype(node) == NodeType::EndTag {
                self.pop_inline(Some(node));
            }
            self.report(Some(element), Some(node), Code::DiscardingUnexpected);
        }

        if (emodel & CM_OPT) == 0 {
            self.report(Some(element), node_opt, Code::MissingEndtagFor);
        }
        if (emodel & CM_OBJECT) != 0 {
            while self.lexer.istack.len() > self.lexer.istackbase {
                self.pop_inline(None);
            }
            self.lexer.istackbase = istackbase;
        }
        self.trim_spaces(element);
    }

    // ---- ParseNamespace (svg, math) ---------------------------------------------

    /// `FindMatchingDescendant`: walk up from `parent` looking for a node
    /// with the tag of `node`; also tells whether the search passed the
    /// base node.
    fn find_matching_descendant(
        &self,
        parent: NodeId,
        node: NodeId,
        marker: NodeId,
    ) -> (Option<NodeId>, bool) {
        let want_id = self.tag_id(node);
        let want_name = self.tree.node(node).element.clone();
        let mut passed = false;
        let mut cur = Some(parent);
        while let Some(n) = cur {
            if self.tag_id(n) == want_id
                && (want_id != TagId::UNKNOWN
                    || (self.tree.node(n).element.is_some()
                        && self.tree.node(n).element == want_name))
            {
                return (Some(n), passed);
            }
            if n == marker {
                passed = true;
            }
            cur = self.tree.parent(n);
        }
        (None, passed)
    }

    /// `TY_(ParseNamespace)`.
    fn parse_namespace(&mut self, basenode: NodeId) {
        let mut parent = basenode;
        self.defer_dup();
        let istackbase = self.lexer.istackbase;
        self.lexer.istackbase = self.lexer.istack.len();
        let mode = TokenMode::OtherNamespace;
        while let Some(mut node) = self.get_token(mode) {
            if self.ntype(node) == NodeType::EndTag {
                let (mp, outside) = self.find_matching_descendant(parent, node, basenode);
                if let Some(mp) = mp {
                    let base_parent = self.tree.parent(basenode);
                    let mut n = Some(parent);
                    while let Some(cur) = n {
                        if Some(cur) == base_parent || cur == mp {
                            break;
                        }
                        self.tree.node_mut(cur).closed = true;
                        let p = self.tree.parent(cur);
                        self.report(p, Some(cur), Code::MissingEndtagBefore);
                        n = p;
                    }
                    let n = n.expect("the loop stops at a node");
                    if outside {
                        self.unget_token();
                        node = basenode;
                        parent = self.tree.parent(node).expect("the base node has a parent");
                    } else {
                        self.tree.node_mut(n).closed = true;
                        node = n;
                        parent = self.tree.parent(node).expect("a matched node has a parent");
                    }
                    if node == basenode {
                        self.lexer.istackbase = istackbase;
                        return;
                    }
                } else {
                    self.report(Some(parent), Some(node), Code::DiscardingUnexpected);
                }
            } else if self.ntype(node) == NodeType::StartTag {
                for av in &mut self.tree.node_mut(node).attributes {
                    av.dict = None;
                }
                self.tree.insert_node_at_end(parent, node);
                parent = node;
            } else {
                for av in &mut self.tree.node_mut(node).attributes {
                    av.dict = None;
                }
                self.tree.insert_node_at_end(parent, node);
            }
        }
        let bp = self.tree.parent(basenode);
        self.report(bp, Some(basenode), Code::MissingEndtagFor);
    }

    // ---- ParseInline ------------------------------------------------------------

    /// `TY_(ParseInline)`.
    #[allow(
        clippy::too_many_lines,
        reason = "a one-to-one port of tidy's ParseInline"
    )]
    fn parse_inline(&mut self, mut element: NodeId, mut mode: TokenMode) {
        let emodel = self.node_model(element);
        if (emodel & CM_EMPTY) != 0 {
            return;
        }
        if (self.node_has_cm(element, CM_BLOCK) || self.node_is(element, TagId::DT))
            && !self.node_has_cm(element, CM_MIXED)
        {
            self.inline_dup(None);
        } else if self.node_has_cm(element, CM_INLINE) {
            self.push_inline(element);
        }
        if self.node_is(element, TagId::NOBR) {
            self.bad_layout |= USING_NOBR;
        } else if self.node_is(element, TagId::FONT) {
            self.bad_layout |= USING_FONT;
        }
        if mode != TokenMode::Preformatted {
            mode = TokenMode::MixedContent;
        }
        let mut node_opt;
        loop {
            node_opt = self.get_token(mode);
            let Some(node) = node_opt else {
                break;
            };
            // end tag for current element
            if self.tags_equal(node, element) && self.ntype(node) == NodeType::EndTag {
                if self.node_has_cm(element, CM_INLINE) {
                    self.pop_inline(Some(node));
                }
                if !mode.is_preformatted_bit() {
                    self.trim_spaces(element);
                }
                if self.node_is(element, TagId::FONT)
                    && let Some(child) = self.tree.content(element)
                    && Some(child) == self.tree.last(element)
                    && self.node_is(child, TagId::A)
                {
                    let (eparent, enext, eprev) = {
                        let e = self.tree.node(element);
                        (e.parent, e.next, e.prev)
                    };
                    let (ccontent, clast) = {
                        let c = self.tree.node(child);
                        (c.content, c.last)
                    };
                    {
                        let c = self.tree.node_mut(child);
                        c.parent = eparent;
                        c.next = enext;
                        c.prev = eprev;
                        c.content = Some(element);
                    }
                    {
                        let e = self.tree.node_mut(element);
                        e.next = None;
                        e.prev = None;
                        e.parent = Some(child);
                        e.content = ccontent;
                        e.last = clast;
                    }
                    self.tree.fix_node_links(child);
                    self.tree.fix_node_links(element);
                }
                self.tree.node_mut(element).closed = true;
                self.trim_spaces(element);
                return;
            }

            // <u>...<u> maps the second <u> to </u> if the first is explicit
            if self.ntype(node) == NodeType::StartTag
                && self.same_tag(node, element)
                && self.is_pushed(node)
                && !self.tree.node(node).implicit
                && !self.tree.node(element).implicit
                && self.node_has_cm(node, CM_INLINE)
                && !matches!(
                    self.tag_id(node),
                    TagId::A
                        | TagId::FONT
                        | TagId::BIG
                        | TagId::SMALL
                        | TagId::SUB
                        | TagId::SUP
                        | TagId::Q
                        | TagId::SPAN
                )
                && self.opts.coerce_endtags
            {
                let last = self.tree.last(element);
                if self.tree.content(element).is_some()
                    && self.tree.node(node).attributes.is_empty()
                    && last.is_some_and(|l| self.is_text(l))
                    && !last.is_some_and(|l| self.text_node_ends_with_space(l))
                {
                    self.report(Some(element), Some(node), Code::CoerceToEndtag);
                    self.tree.node_mut(node).ntype = NodeType::EndTag;
                    self.unget_token();
                    continue;
                }
                if self.tree.node(node).attributes.is_empty()
                    || self.tree.node(element).attributes.is_empty()
                {
                    self.report(Some(element), Some(node), Code::NestedEmphasis);
                }
            } else if self.is_pushed(node)
                && self.ntype(node) == NodeType::StartTag
                && self.node_is(node, TagId::Q)
                && self.html_version() != tables::HT50
            {
                self.report(Some(element), Some(node), Code::NestedQuotation);
            }

            if self.is_text(node) {
                if self.tree.content(element).is_none() && !mode.is_preformatted_bit() {
                    self.trim_spaces(element);
                }
                let nd = self.tree.node(node);
                if nd.start >= nd.end {
                    continue;
                }
                self.tree.insert_node_at_end(element, node);
                continue;
            }

            if self.insert_misc(element, node) {
                continue;
            }

            if self.node_is(node, TagId::HTML) {
                if self.is_element(node) {
                    self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                self.unget_token();
                if !mode.is_preformatted_bit() {
                    self.trim_spaces(element);
                }
                return;
            }

            // within <dt> or <pre> map <p> to <br>
            if self.node_is(node, TagId::P)
                && self.ntype(node) == NodeType::StartTag
                && (mode.is_preformatted_bit()
                    || self.node_is(element, TagId::DT)
                    || self.descendant_of(element, TagId::DT))
            {
                let br = self.tag_index(TagId::BR);
                let n = self.tree.node_mut(node);
                n.tag = Some(br);
                n.element = Some("br".to_string());
                self.trim_spaces(element);
                self.tree.insert_node_at_end(element, node);
                continue;
            }

            // <p> allowed within <address> in HTML 4.01 Transitional
            if self.node_is(node, TagId::P)
                && self.ntype(node) == NodeType::StartTag
                && self.node_is(element, TagId::ADDRESS)
            {
                self.constrain_version(!VERS_HTML40_STRICT);
                self.tree.insert_node_at_end(element, node);
                self.lexer.parent = Some(node);
                let parser = self.node_parser(node).expect("p has a parser");
                self.run_parser(parser, node, mode);
                continue;
            }

            if !self.has_tag(node) || self.node_is(node, TagId::PARAM) {
                self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                continue;
            }

            if self.node_is(node, TagId::BR) && self.ntype(node) == NodeType::EndTag {
                self.tree.node_mut(node).ntype = NodeType::StartTag;
            }

            if self.ntype(node) == NodeType::EndTag {
                if self.node_is(node, TagId::BR) {
                    self.tree.node_mut(node).ntype = NodeType::StartTag;
                } else if self.node_is(node, TagId::P) {
                    if !self.descendant_of(element, TagId::P) {
                        self.coerce_node(node, TagId::BR, false, false);
                        self.trim_spaces(element);
                        self.tree.insert_node_at_end(element, node);
                        let br = self.inferred_tag(TagId::BR);
                        self.tree.insert_node_at_end(element, br);
                        continue;
                    }
                } else if self.node_has_cm(node, CM_INLINE)
                    && !self.node_is(node, TagId::A)
                    && !self.node_has_cm(node, CM_OBJECT)
                    && self.node_has_cm(element, CM_INLINE)
                {
                    if !self.node_is(element, TagId::A)
                        && !self.same_tag(node, element)
                        && self.is_pushed(node)
                        && self.is_pushed(element)
                        && self.switch_inline(element, node)
                    {
                        self.report(Some(element), Some(node), Code::NonMatchingEndtag);
                        self.unget_token();
                        self.inline_dup1(None, Some(element));
                        if !mode.is_preformatted_bit() {
                            self.trim_spaces(element);
                        }
                        return;
                    }
                    self.pop_inline(Some(element));
                    if !self.node_is(element, TagId::A) {
                        if self.node_is(node, TagId::A) && !self.same_tag(node, element) {
                            self.report(Some(element), Some(node), Code::MissingEndtagBefore);
                            self.unget_token();
                        } else {
                            self.report(Some(element), Some(node), Code::NonMatchingEndtag);
                        }
                        if !mode.is_preformatted_bit() {
                            self.trim_spaces(element);
                        }
                        return;
                    }
                    self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                    continue;
                } else if self.lexer.exiled
                    && (self.node_has_cm(node, CM_TABLE) || self.node_is(node, TagId::TABLE))
                {
                    self.unget_token();
                    self.trim_spaces(element);
                    return;
                }
            }

            // allow any header tag to end current header
            if self.node_has_cm(node, CM_HEADING) && self.node_has_cm(element, CM_HEADING) {
                if self.same_tag(node, element) {
                    self.report(Some(element), Some(node), Code::NonMatchingEndtag);
                } else {
                    self.report(Some(element), Some(node), Code::MissingEndtagBefore);
                    self.unget_token();
                }
                if !mode.is_preformatted_bit() {
                    self.trim_spaces(element);
                }
                return;
            }

            // an <a> ends any open <a>
            if self.node_is(node, TagId::A)
                && !self.tree.node(node).implicit
                && (self.node_is(element, TagId::A) || self.descendant_of(element, TagId::A))
            {
                if self.ntype(node) != NodeType::EndTag
                    && self.tree.node(node).attributes.is_empty()
                    && self.opts.coerce_endtags
                {
                    self.tree.node_mut(node).ntype = NodeType::EndTag;
                    self.report(Some(element), Some(node), Code::CoerceToEndtag);
                    self.unget_token();
                    continue;
                }
                self.unget_token();
                self.report(Some(element), Some(node), Code::MissingEndtagBefore);
                if !mode.is_preformatted_bit() {
                    self.trim_spaces(element);
                }
                return;
            }

            if self.node_has_cm(element, CM_HEADING) {
                if self.node_is(node, TagId::CENTER) || self.node_is(node, TagId::DIV) {
                    if !self.is_element(node) {
                        self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                        continue;
                    }
                    self.report(Some(element), Some(node), Code::TagNotAllowedIn);
                    if self.tree.content(element).is_none() {
                        self.tree.insert_node_as_parent(element, node);
                        continue;
                    }
                    self.tree.insert_node_after_element(element, node);
                    if !mode.is_preformatted_bit() {
                        self.trim_spaces(element);
                    }
                    element = self.clone_node(Some(element));
                    self.tree.insert_node_at_end(node, element);
                    continue;
                }
                if self.node_is(node, TagId::HR) {
                    if !self.is_element(node) {
                        self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                        continue;
                    }
                    self.report(Some(element), Some(node), Code::TagNotAllowedIn);
                    if self.tree.content(element).is_none() {
                        self.tree.insert_node_before_element(element, node);
                        continue;
                    }
                    self.tree.insert_node_after_element(element, node);
                    if !mode.is_preformatted_bit() {
                        self.trim_spaces(element);
                    }
                    element = self.clone_node(Some(element));
                    self.tree.insert_node_after_element(node, element);
                    continue;
                }
            }

            if self.node_is(element, TagId::DT) && self.node_is(node, TagId::HR) {
                if !self.is_element(node) {
                    self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                self.report(Some(element), Some(node), Code::TagNotAllowedIn);
                let dd = self.inferred_tag(TagId::DD);
                if self.tree.content(element).is_none() {
                    self.tree.insert_node_before_element(element, dd);
                    self.tree.insert_node_at_end(dd, node);
                    continue;
                }
                self.tree.insert_node_after_element(element, dd);
                self.tree.insert_node_at_end(dd, node);
                if !mode.is_preformatted_bit() {
                    self.trim_spaces(element);
                }
                element = self.clone_node(Some(element));
                self.tree.insert_node_after_element(dd, element);
                continue;
            }

            // end tag for an ancestor element: infer the end of this one
            if self.ntype(node) == NodeType::EndTag {
                let mut parent = self.tree.parent(element);
                let mut matched = false;
                while let Some(p) = parent {
                    if self.tags_equal(node, p) {
                        matched = true;
                        break;
                    }
                    parent = self.tree.parent(p);
                }
                if matched {
                    if !self.node_has_cm(element, CM_OPT) && !self.tree.node(element).implicit {
                        self.report(Some(element), Some(node), Code::MissingEndtagBefore);
                    }
                    if self.is_pushed_last(Some(element), node) {
                        self.pop_inline(Some(element));
                    }
                    self.unget_token();
                    if !mode.is_preformatted_bit() {
                        self.trim_spaces(element);
                    }
                    return;
                }
            }

            // block level tags end this element
            let stays_inline = self.node_has_cm(node, CM_INLINE)
                || self.node_has_cm(element, CM_MIXED)
                || (self.node_is(element, TagId::SPAN) && self.node_is(node, TagId::META));
            if !stays_inline {
                if !self.is_element(node) {
                    self.report(Some(element), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                if self.node_is(element, TagId::DATALIST) {
                    self.constrain_version(!VERS_HTML5);
                } else if !self.node_has_cm(element, CM_OPT) {
                    self.report(Some(element), Some(node), Code::MissingEndtagBefore);
                }
                if self.node_has_cm(node, CM_HEAD) && !self.node_has_cm(node, CM_BLOCK) {
                    self.move_to_head(element, node);
                    continue;
                }
                if self.node_is(element, TagId::A) {
                    if self.has_tag(node) && !self.node_has_cm(node, CM_HEADING) {
                        self.pop_inline(Some(element));
                    } else if self.tree.content(element).is_none() {
                        self.tree.discard_element(element);
                        self.unget_token();
                        return;
                    }
                }
                self.unget_token();
                if !mode.is_preformatted_bit() {
                    self.trim_spaces(element);
                }
                return;
            }

            // parse inline element
            if self.is_element(node) {
                if self.tree.node(node).implicit {
                    self.report(Some(element), Some(node), Code::InsertingTag);
                }
                if self.node_is(node, TagId::BR) {
                    self.trim_spaces(element);
                }
                self.tree.insert_node_at_end(element, node);
                self.parse_tag(node, mode);
                continue;
            }

            self.report(Some(element), Some(node), Code::DiscardingUnexpected);
        }

        if !self.node_has_cm(element, CM_OPT) {
            self.report(Some(element), node_opt, Code::MissingEndtagFor);
        }
    }

    /// Call a tag's parser directly (`(*node->tag->parser)(doc, node, mode)`).
    fn run_parser(&mut self, parser: ParserKind, node: NodeId, mode: TokenMode) {
        match parser {
            ParserKind::ParseBlock => self.parse_block(node, mode),
            ParserKind::ParseInline => self.parse_inline(node, mode),
            other => {
                self.lexer.parent = Some(node);
                let _ = other;
                self.parse_tag(node, mode);
            }
        }
    }

    // ---- simple content models -------------------------------------------------

    /// `TY_(ParseEmpty)`.
    fn parse_empty(&mut self, element: NodeId, mode: TokenMode) {
        if self.lexer.isvoyager
            && let Some(node) = self.get_token(mode)
            && !(self.ntype(node) == NodeType::EndTag && self.tags_equal(node, element))
        {
            self.unget_token();
        }
    }

    /// `TY_(ParseDefList)`.
    fn parse_deflist(&mut self, mut list: NodeId, mode: TokenMode) {
        if (self.node_model(list) & CM_EMPTY) != 0 {
            return;
        }
        self.lexer.insert = None;
        let mut node_opt;
        loop {
            node_opt = self.get_token(TokenMode::IgnoreWhitespace);
            let Some(mut node) = node_opt else {
                break;
            };
            if self.tags_equal(node, list) && self.ntype(node) == NodeType::EndTag {
                self.tree.node_mut(list).closed = true;
                return;
            }
            if self.insert_misc(list, node) {
                continue;
            }
            if self.is_text(node) {
                self.unget_token();
                node = self.inferred_tag(TagId::DT);
                self.report(Some(list), Some(node), Code::MissingStarttag);
            }
            if !self.has_tag(node) {
                self.report(Some(list), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.ntype(node) == NodeType::EndTag {
                if self.node_is(node, TagId::FORM) {
                    self.bad_form();
                    self.report(Some(list), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                let mut discard_it = false;
                let mut parent = self.tree.parent(list);
                let mut matched = false;
                while let Some(p) = parent {
                    if self.node_is(p, TagId::BODY) {
                        discard_it = true;
                        break;
                    }
                    if self.tags_equal(node, p) {
                        matched = true;
                        break;
                    }
                    parent = self.tree.parent(p);
                }
                if matched {
                    self.report(Some(list), Some(node), Code::MissingEndtagBefore);
                    self.unget_token();
                    return;
                }
                if discard_it {
                    self.report(Some(list), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
            }
            if self.node_is(node, TagId::CENTER) {
                if self.tree.content(list).is_some() {
                    self.tree.insert_node_after_element(list, node);
                } else {
                    self.tree.insert_node_before_element(list, node);
                }
                let parent = self.tree.parent(node).expect("the center has a parent");
                self.lexer.exclude_blocks = false;
                self.parse_tag(node, mode);
                self.lexer.exclude_blocks = true;
                if self.tree.last(parent) == Some(node) {
                    list = self.inferred_tag(TagId::DL);
                    self.tree.insert_node_after_element(node, list);
                }
                continue;
            }
            if !(self.node_is(node, TagId::DT) || self.node_is(node, TagId::DD)) {
                self.unget_token();
                if (self.node_model(node) & (CM_BLOCK | CM_INLINE)) == 0 {
                    self.report(Some(list), Some(node), Code::TagNotAllowedIn);
                    return;
                }
                if (self.node_model(node) & CM_INLINE) == 0 && self.lexer.exclude_blocks {
                    return;
                }
                node = self.inferred_tag(TagId::DD);
                self.report(Some(list), Some(node), Code::MissingStarttag);
            }
            if self.ntype(node) == NodeType::EndTag {
                self.report(Some(list), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            self.tree.insert_node_at_end(list, node);
            self.parse_tag(node, TokenMode::IgnoreWhitespace);
        }
        self.report(Some(list), node_opt, Code::MissingEndtagFor);
    }

    /// `FindLastLI`.
    fn find_last_li(&self, list: NodeId) -> Option<NodeId> {
        let mut found = None;
        let mut node = self.tree.content(list);
        while let Some(n) = node {
            if self.node_is(n, TagId::LI) && self.ntype(n) == NodeType::StartTag {
                found = Some(n);
            }
            node = self.tree.next(n);
        }
        found
    }

    /// `TY_(ParseList)`.
    fn parse_list(&mut self, list: NodeId) {
        if (self.node_model(list) & CM_EMPTY) != 0 {
            return;
        }
        self.lexer.insert = None;
        let node_is_ol = self.node_is(list, TagId::OL);
        let mut node_opt;
        loop {
            node_opt = self.get_token(TokenMode::IgnoreWhitespace);
            let Some(mut node) = node_opt else {
                break;
            };
            let mut found_li = false;
            if self.tags_equal(node, list) && self.ntype(node) == NodeType::EndTag {
                self.tree.node_mut(list).closed = true;
                return;
            }
            if self.insert_misc(list, node) {
                continue;
            }
            if !self.is_text(node) && !self.has_tag(node) {
                self.report(Some(list), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.is_text(node) {
                let all_white = self
                    .node_text(node)
                    .iter()
                    .all(|&b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'));
                if all_white {
                    continue;
                }
            }
            if self.ntype(node) == NodeType::EndTag {
                if self.node_is(node, TagId::FORM) {
                    self.bad_form();
                    self.report(Some(list), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                if self.node_has_cm(node, CM_INLINE) {
                    self.report(Some(list), Some(node), Code::DiscardingUnexpected);
                    self.pop_inline(Some(node));
                    continue;
                }
                let mut parent = self.tree.parent(list);
                while let Some(p) = parent {
                    if self.node_is(p, TagId::BODY) {
                        break;
                    }
                    if self.tags_equal(node, p) {
                        self.report(Some(list), Some(node), Code::MissingEndtagBefore);
                        self.unget_token();
                        return;
                    }
                    parent = self.tree.parent(p);
                }
                self.report(Some(list), Some(node), Code::DiscardingUnexpected);
                continue;
            }

            if !self.node_is(node, TagId::LI) && node_is_ol {
                found_li = self.find_last_li(list).is_some();
            }

            if self.node_is(node, TagId::LI) || (self.html5_mode && !found_li) {
                self.tree.insert_node_at_end(list, node);
            } else {
                self.unget_token();
                if self.node_has_cm(node, CM_BLOCK) && self.lexer.exclude_blocks {
                    self.report(Some(list), Some(node), Code::MissingEndtagBefore);
                    return;
                } else if self.lexer.exiled
                    && (self.node_has_cm(node, CM_TABLE | CM_ROWGRP | CM_ROW)
                        || self.node_is(node, TagId::TABLE))
                {
                    return;
                }
                if self.node_is(list, TagId::OL)
                    && let Some(lastli) = self.find_last_li(list)
                {
                    let li = self.inferred_tag(TagId::LI);
                    self.report(Some(list), Some(li), Code::MissingStarttag);
                    node = lastli;
                } else {
                    let wasblock = self.node_has_cm(node, CM_BLOCK);
                    node = self.inferred_tag(TagId::LI);
                    self.add_style_property(
                        node,
                        if wasblock {
                            "list-style: none; display: inline"
                        } else {
                            "list-style: none"
                        },
                    );
                    self.report(Some(list), Some(node), Code::MissingStarttag);
                    self.tree.insert_node_at_end(list, node);
                }
            }
            self.parse_tag(node, TokenMode::IgnoreWhitespace);
        }
        self.report(Some(list), node_opt, Code::MissingEndtagFor);
    }

    // ---- tables -------------------------------------------------------------------

    /// `MoveBeforeTable`.
    fn move_before_table(&mut self, row: NodeId, node: NodeId) {
        let mut table = self.tree.parent(row);
        while let Some(t) = table {
            if self.node_is(t, TagId::TABLE) {
                self.tree.insert_node_before_element(t, node);
                return;
            }
            table = self.tree.parent(t);
        }
        let parent = self.tree.parent(row).expect("a row has a parent");
        self.tree.insert_node_before_element(parent, node);
    }

    /// `FixEmptyRow`.
    fn fix_empty_row(&mut self, row: NodeId) {
        if self.tree.content(row).is_none() {
            let cell = self.inferred_tag(TagId::TD);
            self.tree.insert_node_at_end(row, cell);
            self.report(Some(row), Some(cell), Code::MissingStarttag);
        }
    }

    /// `TY_(ParseRow)`.
    fn parse_row(&mut self, row: NodeId) {
        if (self.node_model(row) & CM_EMPTY) != 0 {
            return;
        }
        while let Some(mut node) = self.get_token(TokenMode::IgnoreWhitespace) {
            if self.tags_equal(node, row) && self.has_tag(node) {
                if self.ntype(node) == NodeType::EndTag {
                    self.tree.node_mut(row).closed = true;
                    self.fix_empty_row(row);
                    return;
                }
                self.unget_token();
                self.fix_empty_row(row);
                return;
            }
            if self.ntype(node) == NodeType::EndTag {
                if (self.node_has_cm(node, CM_HTML | CM_TABLE) || self.node_is(node, TagId::TABLE))
                    && self
                        .tree
                        .node(node)
                        .tag
                        .is_some_and(|t| self.tree.descendant_of(row, t))
                {
                    self.unget_token();
                    return;
                }
                if self.node_is(node, TagId::FORM) || self.node_has_cm(node, CM_BLOCK | CM_INLINE) {
                    if self.node_is(node, TagId::FORM) {
                        self.bad_form();
                    }
                    self.report(Some(row), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                if self.node_is(node, TagId::TD) || self.node_is(node, TagId::TH) {
                    self.report(Some(row), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
            }
            if self.insert_misc(row, node) {
                continue;
            }
            if !self.has_tag(node) && !self.is_text(node) {
                self.report(Some(row), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.node_is(node, TagId::TABLE) {
                self.report(Some(row), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.node_has_cm(node, CM_ROWGRP) {
                self.unget_token();
                return;
            }
            if self.ntype(node) == NodeType::EndTag {
                self.report(Some(row), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.node_is(node, TagId::FORM) {
                self.unget_token();
                node = self.inferred_tag(TagId::TD);
                self.report(Some(row), Some(node), Code::MissingStarttag);
            } else if self.is_text(node) || self.node_has_cm(node, CM_BLOCK | CM_INLINE) {
                self.move_before_table(row, node);
                self.report(Some(row), Some(node), Code::TagNotAllowedIn);
                self.lexer.exiled = true;
                let exclude_state = self.lexer.exclude_blocks;
                self.lexer.exclude_blocks = false;
                if !self.is_text(node) {
                    self.parse_tag(node, TokenMode::IgnoreWhitespace);
                }
                self.lexer.exiled = false;
                self.lexer.exclude_blocks = exclude_state;
                continue;
            } else if (self.node_model(node) & CM_HEAD) != 0 {
                self.report(Some(row), Some(node), Code::TagNotAllowedIn);
                self.move_to_head(row, node);
                continue;
            }
            if !(self.node_is(node, TagId::TD) || self.node_is(node, TagId::TH)) {
                self.report(Some(row), Some(node), Code::TagNotAllowedIn);
                continue;
            }
            self.tree.insert_node_at_end(row, node);
            let exclude_state = self.lexer.exclude_blocks;
            self.lexer.exclude_blocks = false;
            self.parse_tag(node, TokenMode::IgnoreWhitespace);
            self.lexer.exclude_blocks = exclude_state;
            while self.lexer.istack.len() > self.lexer.istackbase {
                self.pop_inline(None);
            }
        }
    }

    /// `TY_(ParseRowGroup)`.
    fn parse_rowgroup(&mut self, rowgroup: NodeId) {
        if (self.node_model(rowgroup) & CM_EMPTY) != 0 {
            return;
        }
        while let Some(mut node) = self.get_token(TokenMode::IgnoreWhitespace) {
            if self.tags_equal(node, rowgroup) && self.has_tag(node) {
                if self.ntype(node) == NodeType::EndTag {
                    self.tree.node_mut(rowgroup).closed = true;
                    return;
                }
                self.unget_token();
                return;
            }
            if self.node_is(node, TagId::TABLE) && self.ntype(node) == NodeType::EndTag {
                self.unget_token();
                return;
            }
            if self.insert_misc(rowgroup, node) {
                continue;
            }
            if !self.has_tag(node) && !self.is_text(node) {
                self.report(Some(rowgroup), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.ntype(node) != NodeType::EndTag {
                if self.node_is(node, TagId::TD) || self.node_is(node, TagId::TH) {
                    self.unget_token();
                    node = self.inferred_tag(TagId::TR);
                    self.report(Some(rowgroup), Some(node), Code::MissingStarttag);
                } else if self.is_text(node) || self.node_has_cm(node, CM_BLOCK | CM_INLINE) {
                    self.move_before_table(rowgroup, node);
                    self.report(Some(rowgroup), Some(node), Code::TagNotAllowedIn);
                    self.lexer.exiled = true;
                    if !self.is_text(node) {
                        self.parse_tag(node, TokenMode::IgnoreWhitespace);
                    }
                    self.lexer.exiled = false;
                    continue;
                } else if (self.node_model(node) & CM_HEAD) != 0 {
                    self.report(Some(rowgroup), Some(node), Code::TagNotAllowedIn);
                    self.move_to_head(rowgroup, node);
                    continue;
                }
            }
            if self.ntype(node) == NodeType::EndTag {
                if self.node_is(node, TagId::FORM) || self.node_has_cm(node, CM_BLOCK | CM_INLINE) {
                    if self.node_is(node, TagId::FORM) {
                        self.bad_form();
                    }
                    self.report(Some(rowgroup), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                if self.node_is(node, TagId::TR)
                    || self.node_is(node, TagId::TD)
                    || self.node_is(node, TagId::TH)
                {
                    self.report(Some(rowgroup), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                let mut parent = self.tree.parent(rowgroup);
                while let Some(p) = parent {
                    if self.tags_equal(node, p) {
                        self.unget_token();
                        return;
                    }
                    parent = self.tree.parent(p);
                }
            }
            if (self.node_model(node) & CM_ROWGRP) != 0 && self.ntype(node) != NodeType::EndTag {
                self.unget_token();
                return;
            }
            if self.ntype(node) == NodeType::EndTag {
                self.report(Some(rowgroup), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if !self.node_is(node, TagId::TR) {
                node = self.inferred_tag(TagId::TR);
                self.report(Some(rowgroup), Some(node), Code::MissingStarttag);
                self.unget_token();
            }
            self.tree.insert_node_at_end(rowgroup, node);
            self.parse_tag(node, TokenMode::IgnoreWhitespace);
        }
    }

    /// `TY_(ParseColGroup)`.
    fn parse_colgroup(&mut self, colgroup: NodeId) {
        if (self.node_model(colgroup) & CM_EMPTY) != 0 {
            return;
        }
        while let Some(node) = self.get_token(TokenMode::IgnoreWhitespace) {
            if self.tags_equal(node, colgroup) && self.ntype(node) == NodeType::EndTag {
                self.tree.node_mut(colgroup).closed = true;
                return;
            }
            if self.ntype(node) == NodeType::EndTag {
                if self.node_is(node, TagId::FORM) {
                    self.bad_form();
                    self.report(Some(colgroup), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                let mut parent = self.tree.parent(colgroup);
                while let Some(p) = parent {
                    if self.tags_equal(node, p) {
                        self.unget_token();
                        return;
                    }
                    parent = self.tree.parent(p);
                }
            }
            if self.is_text(node) {
                self.unget_token();
                return;
            }
            if self.insert_misc(colgroup, node) {
                continue;
            }
            if !self.has_tag(node) {
                self.report(Some(colgroup), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if !self.node_is(node, TagId::COL) {
                self.unget_token();
                return;
            }
            if self.ntype(node) == NodeType::EndTag {
                self.report(Some(colgroup), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            self.tree.insert_node_at_end(colgroup, node);
            self.parse_tag(node, TokenMode::IgnoreWhitespace);
        }
    }

    /// `TY_(ParseTableTag)`.
    fn parse_table(&mut self, table: NodeId) {
        self.defer_dup();
        let istackbase = self.lexer.istackbase;
        self.lexer.istackbase = self.lexer.istack.len();
        let mut node_opt;
        loop {
            node_opt = self.get_token(TokenMode::IgnoreWhitespace);
            let Some(mut node) = node_opt else {
                break;
            };
            if self.tags_equal(node, table) && self.has_tag(node) {
                if self.ntype(node) != NodeType::EndTag {
                    self.unget_token();
                    self.report(Some(table), Some(node), Code::TagNotAllowedIn);
                }
                self.lexer.istackbase = istackbase;
                self.tree.node_mut(table).closed = true;
                return;
            }
            if self.insert_misc(table, node) {
                continue;
            }
            if !self.has_tag(node) && !self.is_text(node) {
                self.report(Some(table), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.ntype(node) != NodeType::EndTag {
                if self.node_is(node, TagId::TD)
                    || self.node_is(node, TagId::TH)
                    || self.node_is(node, TagId::TABLE)
                {
                    self.unget_token();
                    node = self.inferred_tag(TagId::TR);
                    self.report(Some(table), Some(node), Code::MissingStarttag);
                } else if self.is_text(node) || self.node_has_cm(node, CM_BLOCK | CM_INLINE) {
                    self.tree.insert_node_before_element(table, node);
                    self.report(Some(table), Some(node), Code::TagNotAllowedIn);
                    self.lexer.exiled = true;
                    if !self.is_text(node) {
                        self.parse_tag(node, TokenMode::IgnoreWhitespace);
                    }
                    self.lexer.exiled = false;
                    continue;
                } else if (self.node_model(node) & CM_HEAD) != 0 {
                    self.move_to_head(table, node);
                    continue;
                }
            }
            if self.ntype(node) == NodeType::EndTag {
                if self.node_is(node, TagId::FORM) {
                    self.bad_form();
                    self.report(Some(table), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                if self.node_has_cm(node, CM_TABLE | CM_ROW)
                    || self.node_has_cm(node, CM_BLOCK | CM_INLINE)
                {
                    self.report(Some(table), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                let mut parent = self.tree.parent(table);
                while let Some(p) = parent {
                    if self.tags_equal(node, p) {
                        self.report(Some(table), Some(node), Code::MissingEndtagBefore);
                        self.unget_token();
                        self.lexer.istackbase = istackbase;
                        return;
                    }
                    parent = self.tree.parent(p);
                }
            }
            if (self.node_model(node) & CM_TABLE) == 0 {
                self.unget_token();
                self.report(Some(table), Some(node), Code::TagNotAllowedIn);
                self.lexer.istackbase = istackbase;
                return;
            }
            if self.is_element(node) {
                self.tree.insert_node_at_end(table, node);
                self.parse_tag(node, TokenMode::IgnoreWhitespace);
                continue;
            }
            self.report(Some(table), Some(node), Code::DiscardingUnexpected);
        }
        self.report(Some(table), node_opt, Code::MissingEndtagFor);
        self.lexer.istackbase = istackbase;
    }

    // ---- pre, forms, text ---------------------------------------------------------

    /// `PreContent`.
    fn pre_content(&self, node: NodeId) -> bool {
        if self.node_is(node, TagId::P) || self.is_text(node) {
            return true;
        }
        if !self.has_tag(node)
            || self.node_is(node, TagId::PARAM)
            || !self.node_has_cm(node, CM_INLINE | CM_NEW)
        {
            return false;
        }
        true
    }

    /// `TY_(ParsePre)`.
    fn parse_pre(&mut self, mut pre: NodeId) {
        if (self.node_model(pre) & CM_EMPTY) != 0 {
            return;
        }
        self.inline_dup(None);
        let mut node_opt;
        loop {
            node_opt = self.get_token(TokenMode::Preformatted);
            let Some(node) = node_opt else {
                break;
            };
            if self.ntype(node) == NodeType::EndTag
                && (self.tags_equal(node, pre)
                    || self
                        .tree
                        .node(node)
                        .tag
                        .is_some_and(|t| self.tree.descendant_of(pre, t)))
            {
                if self.node_is(node, TagId::BODY) || self.node_is(node, TagId::HTML) {
                    self.report(Some(pre), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                if !self.tags_equal(node, pre) {
                    self.report(Some(pre), Some(node), Code::MissingEndtagBefore);
                    self.unget_token();
                }
                self.tree.node_mut(pre).closed = true;
                self.trim_spaces(pre);
                return;
            }
            if self.is_text(node) {
                self.tree.insert_node_at_end(pre, node);
                continue;
            }
            if self.insert_misc(pre, node) {
                continue;
            }
            if !self.has_tag(node) {
                self.report(Some(pre), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if !self.pre_content(node) {
                if self.ntype(node) == NodeType::EndTag {
                    if self.lexer.exiled
                        && (self.node_has_cm(node, CM_TABLE) || self.node_is(node, TagId::TABLE))
                    {
                        self.unget_token();
                        self.trim_spaces(pre);
                        return;
                    }
                    self.report(Some(pre), Some(node), Code::DiscardingUnexpected);
                    continue;
                } else if self.node_has_cm(node, CM_TABLE | CM_ROW)
                    || self.node_is(node, TagId::TABLE)
                {
                    if !self.lexer.exiled {
                        self.report(Some(pre), Some(node), Code::MissingEndtagBefore);
                    }
                    self.unget_token();
                    return;
                }
                self.tree.insert_node_after_element(pre, node);
                self.report(Some(pre), Some(node), Code::MissingEndtagBefore);
                self.parse_tag(node, TokenMode::IgnoreWhitespace);
                let newnode = self.inferred_tag(TagId::PRE);
                self.report(Some(pre), Some(newnode), Code::InsertingTag);
                pre = newnode;
                self.tree.insert_node_after_element(node, pre);
                continue;
            }
            if self.node_is(node, TagId::P) {
                if self.ntype(node) == NodeType::StartTag {
                    self.report(Some(pre), Some(node), Code::UsingBrInplaceOf);
                    self.trim_spaces(pre);
                    self.coerce_node(node, TagId::BR, false, false);
                    self.free_attrs(node);
                    self.tree.insert_node_at_end(pre, node);
                } else {
                    self.report(Some(pre), Some(node), Code::DiscardingUnexpected);
                }
                continue;
            }
            if self.is_element(node) {
                if self.node_is(node, TagId::BR) {
                    self.trim_spaces(pre);
                }
                self.tree.insert_node_at_end(pre, node);
                self.parse_tag(node, TokenMode::Preformatted);
                continue;
            }
            self.report(Some(pre), Some(node), Code::DiscardingUnexpected);
        }
        self.report(Some(pre), node_opt, Code::MissingEndtagFor);
    }

    /// `TY_(ParseOptGroup)`.
    fn parse_optgroup(&mut self, field: NodeId) {
        self.lexer.insert = None;
        while let Some(node) = self.get_token(TokenMode::IgnoreWhitespace) {
            if self.tags_equal(node, field) && self.ntype(node) == NodeType::EndTag {
                self.tree.node_mut(field).closed = true;
                self.trim_spaces(field);
                return;
            }
            if self.insert_misc(field, node) {
                continue;
            }
            if self.ntype(node) == NodeType::StartTag
                && (self.node_is(node, TagId::OPTION) || self.node_is(node, TagId::OPTGROUP))
            {
                if self.node_is(node, TagId::OPTGROUP) {
                    self.report(Some(field), Some(node), Code::CantBeNested);
                }
                self.tree.insert_node_at_end(field, node);
                self.parse_tag(node, TokenMode::MixedContent);
                continue;
            }
            self.report(Some(field), Some(node), Code::DiscardingUnexpected);
        }
    }

    /// `TY_(ParseSelect)` and `TY_(ParseDatalist)`, which are the same.
    fn parse_select(&mut self, field: NodeId) {
        self.lexer.insert = None;
        let mut node_opt;
        loop {
            node_opt = self.get_token(TokenMode::IgnoreWhitespace);
            let Some(node) = node_opt else {
                break;
            };
            if self.tags_equal(node, field) && self.ntype(node) == NodeType::EndTag {
                self.tree.node_mut(field).closed = true;
                self.trim_spaces(field);
                return;
            }
            if self.insert_misc(field, node) {
                continue;
            }
            if self.ntype(node) == NodeType::StartTag
                && matches!(
                    self.tag_id(node),
                    TagId::OPTION | TagId::OPTGROUP | TagId::DATALIST | TagId::SCRIPT
                )
            {
                self.tree.insert_node_at_end(field, node);
                self.parse_tag(node, TokenMode::IgnoreWhitespace);
                continue;
            }
            self.report(Some(field), Some(node), Code::DiscardingUnexpected);
        }
        self.report(Some(field), node_opt, Code::MissingEndtagFor);
    }

    fn parse_datalist(&mut self, field: NodeId) {
        self.parse_select(field);
    }

    /// `TY_(ParseText)`: option and textarea content.
    fn parse_text(&mut self, field: NodeId, _mode: TokenMode) {
        self.lexer.insert = None;
        let mode = if self.node_is(field, TagId::TEXTAREA) {
            TokenMode::Preformatted
        } else {
            TokenMode::MixedContent
        };
        let mut node_opt;
        loop {
            node_opt = self.get_token(mode);
            let Some(node) = node_opt else {
                break;
            };
            if self.tags_equal(node, field) && self.ntype(node) == NodeType::EndTag {
                self.tree.node_mut(field).closed = true;
                self.trim_spaces(field);
                return;
            }
            if self.insert_misc(field, node) {
                continue;
            }
            if self.is_text(node) {
                if self.tree.content(field).is_none() && !mode.is_preformatted_bit() {
                    self.trim_spaces(field);
                }
                let nd = self.tree.node(node);
                if nd.start >= nd.end {
                    continue;
                }
                self.tree.insert_node_at_end(field, node);
                continue;
            }
            if self.has_tag(node)
                && self.node_has_cm(node, CM_INLINE)
                && !self.node_has_cm(node, CM_FIELD)
            {
                self.report(Some(field), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if !self.node_has_cm(field, CM_OPT) {
                self.report(Some(field), Some(node), Code::MissingEndtagBefore);
            }
            self.unget_token();
            self.trim_spaces(field);
            return;
        }
        if !self.node_has_cm(field, CM_OPT) {
            self.report(Some(field), node_opt, Code::MissingEndtagFor);
        }
    }

    /// `TY_(ParseTitle)`.
    fn parse_title(&mut self, title: NodeId) {
        let mut node_opt;
        loop {
            node_opt = self.get_token(TokenMode::MixedContent);
            let Some(node) = node_opt else {
                break;
            };
            if self.tags_equal(node, title)
                && self.ntype(node) == NodeType::StartTag
                && self.opts.coerce_endtags
            {
                self.report(Some(title), Some(node), Code::CoerceToEndtag);
                self.tree.node_mut(node).ntype = NodeType::EndTag;
                self.unget_token();
                continue;
            } else if self.tags_equal(node, title) && self.ntype(node) == NodeType::EndTag {
                self.tree.node_mut(title).closed = true;
                self.trim_spaces(title);
                return;
            }
            if self.is_text(node) {
                if self.tree.content(title).is_none() {
                    self.trim_initial_space(title, node);
                }
                let nd = self.tree.node(node);
                if nd.start >= nd.end {
                    continue;
                }
                self.tree.insert_node_at_end(title, node);
                continue;
            }
            if self.insert_misc(title, node) {
                continue;
            }
            if !self.has_tag(node) {
                self.report(Some(title), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            self.report(Some(title), Some(node), Code::MissingEndtagBefore);
            self.unget_token();
            self.trim_spaces(title);
            return;
        }
        self.report(Some(title), node_opt, Code::MissingEndtagFor);
    }

    /// `TY_(ParseScript)`.
    fn parse_script(&mut self, script: NodeId) {
        self.lexer.parent = Some(script);
        let node = self.get_token(TokenMode::CdataContent);
        self.lexer.parent = None;
        if let Some(n) = node {
            self.tree.insert_node_at_end(script, n);
        } else {
            self.report(Some(script), None, Code::MissingEndtagFor);
            return;
        }
        let node = self.get_token(TokenMode::IgnoreWhitespace);
        let is_end = node.is_some_and(|n| {
            self.ntype(n) == NodeType::EndTag
                && self.has_tag(n)
                && self.tag_id(n) == self.tag_id(script)
        });
        if !is_end {
            self.report(Some(script), node, Code::MissingEndtagFor);
            if node.is_some() {
                self.unget_token();
            }
        }
    }

    // ---- head, body, frames, html -------------------------------------------------

    /// `TY_(ParseHead)`.
    fn parse_head(&mut self, head: NodeId) {
        let mut has_title = 0;
        let mut has_base = 0;
        while let Some(node) = self.get_token(TokenMode::IgnoreWhitespace) {
            if self.tags_equal(node, head) && self.ntype(node) == NodeType::EndTag {
                self.tree.node_mut(head).closed = true;
                break;
            }
            if (self.tags_equal(node, head) || self.node_is(node, TagId::HTML))
                && self.ntype(node) == NodeType::StartTag
            {
                self.report(Some(head), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.is_text(node) {
                if !self.opts.omit_optional_tags && !self.showing_body_only() {
                    self.report(Some(head), Some(node), Code::TagNotAllowedIn);
                }
                self.unget_token();
                break;
            }
            if self.ntype(node) == NodeType::ProcIns
                && self.tree.node(node).element.as_deref() == Some("xml-stylesheet")
            {
                self.report(Some(head), Some(node), Code::TagNotAllowedIn);
                let html = self.find_html().expect("html exists while parsing head");
                self.tree.insert_node_before_element(html, node);
                continue;
            }
            if self.insert_misc(head, node) {
                continue;
            }
            if self.ntype(node) == NodeType::DocType {
                self.insert_doctype(head, node);
                continue;
            }
            if !self.has_tag(node) {
                self.report(Some(head), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if (self.node_model(node) & CM_HEAD) == 0 {
                if self.lexer.isvoyager {
                    self.report(Some(head), Some(node), Code::TagNotAllowedIn);
                }
                self.unget_token();
                break;
            }
            if self.is_element(node) {
                if self.node_is(node, TagId::TITLE) {
                    has_title += 1;
                    if has_title > 1 {
                        self.report(Some(head), Some(node), Code::TooManyElementsIn);
                    }
                } else if self.node_is(node, TagId::BASE) {
                    has_base += 1;
                    if has_base > 1 {
                        self.report(Some(head), Some(node), Code::TooManyElementsIn);
                    }
                }
                self.tree.insert_node_at_end(head, node);
                self.parse_tag(node, TokenMode::IgnoreWhitespace);
                continue;
            }
            self.report(Some(head), Some(node), Code::DiscardingUnexpected);
        }
    }

    /// `FindNodeById` over the whole document.
    fn find_node_by_id(&self, id: TagId) -> bool {
        fn walk(doc: &Doc, mut node: Option<NodeId>, id: TagId) -> bool {
            while let Some(n) = node {
                if doc.node_is(n, id) {
                    return true;
                }
                if let Some(content) = doc.tree.content(n)
                    && walk(doc, Some(content), id)
                {
                    return true;
                }
                node = doc.tree.next(n);
            }
            false
        }
        walk(self, self.tree.content(0), id)
    }

    /// `TY_(BumpObject)`: object elements with real content move from the
    /// head to the body.
    fn bump_object(&mut self, html: NodeId) {
        let mut head = None;
        let mut body = None;
        let mut node = self.tree.content(html);
        while let Some(n) = node {
            if self.node_is(n, TagId::HEAD) {
                head = Some(n);
            }
            if self.node_is(n, TagId::BODY) {
                body = Some(n);
            }
            node = self.tree.next(n);
        }
        let (Some(head), Some(body)) = (head, body) else {
            return;
        };
        let mut node = self.tree.content(head);
        while let Some(n) = node {
            let next = self.tree.next(n);
            if self.node_is(n, TagId::OBJECT) {
                let mut bump = false;
                let mut child = self.tree.content(n);
                while let Some(c) = child {
                    if (self.is_text(c) && !self.is_blank(n)) || !self.node_is(c, TagId::PARAM) {
                        bump = true;
                        break;
                    }
                    child = self.tree.next(c);
                }
                if bump {
                    self.tree.remove_node(n);
                    self.tree.insert_node_at_start(body, n);
                }
            }
            node = next;
        }
    }

    /// `TY_(ParseBody)`.
    #[allow(
        clippy::too_many_lines,
        reason = "a one-to-one port of tidy's ParseBody"
    )]
    fn parse_body(&mut self, body: NodeId, _mode: TokenMode) {
        let mut mode = TokenMode::IgnoreWhitespace;
        let mut checkstack = true;
        if let Some(parent) = self.tree.parent(body) {
            self.bump_object(parent);
        }
        while let Some(mut node) = self.get_token(mode) {
            if self.tags_equal(node, body) && self.ntype(node) == NodeType::StartTag {
                self.report(Some(body), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.node_is(node, TagId::HTML) {
                if self.is_element(node) || self.lexer.seen_end_html {
                    self.report(Some(body), Some(node), Code::DiscardingUnexpected);
                } else {
                    self.lexer.seen_end_html = true;
                }
                continue;
            }
            if self.lexer.seen_end_body
                && matches!(
                    self.ntype(node),
                    NodeType::StartTag | NodeType::EndTag | NodeType::StartEndTag
                )
            {
                self.report(Some(body), Some(node), Code::ContentAfterBody);
            }
            if self.tags_equal(node, body) && self.ntype(node) == NodeType::EndTag {
                self.tree.node_mut(body).closed = true;
                self.trim_spaces(body);
                self.lexer.seen_end_body = true;
                mode = TokenMode::IgnoreWhitespace;
                if self
                    .tree
                    .parent(body)
                    .is_some_and(|p| self.node_is(p, TagId::NOFRAMES))
                {
                    break;
                }
                continue;
            }
            if self.node_is(node, TagId::NOFRAMES) {
                if self.ntype(node) == NodeType::StartTag {
                    self.tree.insert_node_at_end(body, node);
                    self.parse_block(node, mode);
                    continue;
                }
                if self.ntype(node) == NodeType::EndTag
                    && self
                        .tree
                        .parent(body)
                        .is_some_and(|p| self.node_is(p, TagId::NOFRAMES))
                {
                    self.trim_spaces(body);
                    self.unget_token();
                    break;
                }
            }
            if (self.node_is(node, TagId::FRAME) || self.node_is(node, TagId::FRAMESET))
                && self
                    .tree
                    .parent(body)
                    .is_some_and(|p| self.node_is(p, TagId::NOFRAMES))
            {
                self.trim_spaces(body);
                self.unget_token();
                break;
            }
            // tidy reads `lexbuf[node->start]` even for an empty text node,
            // which is the stale byte left there (the inserted space, say).
            let iswhitenode = {
                let nd = self.tree.node(node);
                nd.is_text()
                    && nd.end <= nd.start + 1
                    && self.lexer.lexbuf.get(nd.start) == Some(&b' ')
            };
            if self.insert_misc(body, node) {
                continue;
            }
            if self.is_text(node) {
                if iswhitenode && mode == TokenMode::IgnoreWhitespace {
                    continue;
                }
                self.constrain_version(!(VERS_HTML40_STRICT | VERS_HTML20));
                if checkstack {
                    checkstack = false;
                    if self.inline_dup(Some(node)) > 0 {
                        continue;
                    }
                }
                self.tree.insert_node_at_end(body, node);
                mode = TokenMode::MixedContent;
                continue;
            }
            if self.ntype(node) == NodeType::DocType {
                self.insert_doctype(body, node);
                continue;
            }
            if !self.has_tag(node) || self.node_is(node, TagId::PARAM) {
                self.report(Some(body), Some(node), Code::DiscardingUnexpected);
                continue;
            }

            self.lexer.exclude_blocks = false;

            if (self.node_is(node, TagId::INPUT)
                || (!self.node_has_cm(node, CM_BLOCK) && !self.node_has_cm(node, CM_INLINE)))
                && !self.html5_mode
            {
                if (self.node_model(node) & CM_HEAD) == 0 {
                    self.report(Some(body), Some(node), Code::TagNotAllowedIn);
                }
                let model = self.node_model(node);
                if (model & CM_HTML) != 0 {
                    if self.node_is(node, TagId::BODY)
                        && self.tree.node(body).implicit
                        && self.tree.node(body).attributes.is_empty()
                    {
                        let attrs = std::mem::take(&mut self.tree.node_mut(node).attributes);
                        self.tree.node_mut(body).attributes = attrs;
                    }
                    continue;
                }
                if (model & CM_HEAD) != 0 {
                    self.move_to_head(body, node);
                    continue;
                }
                if (model & CM_LIST) != 0 {
                    self.unget_token();
                    node = self.inferred_tag(TagId::UL);
                    self.add_class_no_indent(node);
                    self.lexer.exclude_blocks = true;
                } else if (model & CM_DEFLIST) != 0 {
                    self.unget_token();
                    node = self.inferred_tag(TagId::DL);
                    self.lexer.exclude_blocks = true;
                } else if (model & (CM_TABLE | CM_ROWGRP | CM_ROW)) != 0 {
                    if self.ntype(node) != NodeType::EndTag {
                        self.unget_token();
                        node = self.inferred_tag(TagId::TABLE);
                    }
                    self.lexer.exclude_blocks = true;
                } else if self.node_is(node, TagId::INPUT) {
                    self.unget_token();
                    node = self.inferred_tag(TagId::FORM);
                    self.lexer.exclude_blocks = true;
                } else {
                    if !self.node_has_cm(node, CM_ROW | CM_FIELD) {
                        self.unget_token();
                        return;
                    }
                    continue;
                }
            }

            if self.ntype(node) == NodeType::EndTag {
                if self.node_is(node, TagId::BR) {
                    self.tree.node_mut(node).ntype = NodeType::StartTag;
                } else if self.node_is(node, TagId::P) {
                    let n = self.tree.node_mut(node);
                    n.ntype = NodeType::StartEndTag;
                    n.implicit = true;
                } else if self.node_has_cm(node, CM_INLINE) {
                    self.pop_inline(Some(node));
                }
            }

            if self.is_element(node) {
                if self.node_is(node, TagId::MAIN) && self.find_node_by_id(TagId::MAIN) {
                    self.bad_form |= FLG_BAD_MAIN;
                    self.report(Some(body), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                if self.node_has_cm(node, CM_INLINE) {
                    if self.node_is(node, TagId::IMG) {
                        self.constrain_version(!VERS_HTML40_STRICT);
                    } else {
                        self.constrain_version(!(VERS_HTML40_STRICT | VERS_HTML20));
                    }
                    if checkstack && !self.tree.node(node).implicit {
                        checkstack = false;
                        if self.inline_dup(Some(node)) > 0 {
                            continue;
                        }
                    }
                    mode = TokenMode::MixedContent;
                } else {
                    checkstack = true;
                    mode = TokenMode::IgnoreWhitespace;
                }
                if self.tree.node(node).implicit {
                    self.report(Some(body), Some(node), Code::InsertingTag);
                }
                self.tree.insert_node_at_end(body, node);
                self.parse_tag(node, mode);
                continue;
            }
            self.report(Some(body), Some(node), Code::DiscardingUnexpected);
        }
    }

    /// `TY_(ParseNoFrames)`.
    fn parse_noframes(&mut self, noframes: NodeId) {
        self.bad_access |= BA_USING_NOFRAMES;
        let mode = TokenMode::IgnoreWhitespace;
        let mut node_opt;
        loop {
            node_opt = self.get_token(mode);
            let Some(mut node) = node_opt else {
                break;
            };
            if self.tags_equal(node, noframes) && self.ntype(node) == NodeType::EndTag {
                self.tree.node_mut(noframes).closed = true;
                self.trim_spaces(noframes);
                return;
            }
            if self.node_is(node, TagId::FRAME) || self.node_is(node, TagId::FRAMESET) {
                self.trim_spaces(noframes);
                if self.ntype(node) == NodeType::EndTag {
                    self.report(Some(noframes), Some(node), Code::DiscardingUnexpected);
                } else {
                    self.report(Some(noframes), Some(node), Code::MissingEndtagBefore);
                    self.unget_token();
                }
                return;
            }
            if self.node_is(node, TagId::HTML) {
                if self.is_element(node) {
                    self.report(Some(noframes), Some(node), Code::DiscardingUnexpected);
                }
                continue;
            }
            if self.insert_misc(noframes, node) {
                continue;
            }
            if self.node_is(node, TagId::BODY) && self.ntype(node) == NodeType::StartTag {
                let seen_body = self.lexer.seen_end_body;
                self.tree.insert_node_at_end(noframes, node);
                self.parse_tag(node, TokenMode::IgnoreWhitespace);
                if seen_body && self.find_body() != Some(node) {
                    self.coerce_node(node, TagId::DIV, false, false);
                    self.move_node_to_body(node);
                }
                continue;
            }
            if self.is_text(node) || (self.has_tag(node) && self.ntype(node) != NodeType::EndTag) {
                let body = self.find_body();
                if body.is_some() || self.lexer.seen_end_body {
                    let Some(body) = body else {
                        self.report(Some(noframes), Some(node), Code::DiscardingUnexpected);
                        continue;
                    };
                    if self.is_text(node) {
                        self.unget_token();
                        node = self.inferred_tag(TagId::P);
                        self.report(Some(noframes), Some(node), Code::ContentAfterBody);
                    }
                    self.tree.insert_node_at_end(body, node);
                } else {
                    self.unget_token();
                    node = self.inferred_tag(TagId::BODY);
                    if self.opts.output_xml {
                        self.report(Some(noframes), Some(node), Code::InsertingTag);
                    }
                    self.tree.insert_node_at_end(noframes, node);
                }
                self.parse_tag(node, TokenMode::IgnoreWhitespace);
                continue;
            }
            self.report(Some(noframes), Some(node), Code::DiscardingUnexpected);
        }
        self.report(Some(noframes), node_opt, Code::MissingEndtagFor);
    }

    /// `TY_(ParseFrameSet)`.
    fn parse_frameset(&mut self, frameset: NodeId) {
        self.bad_access |= BA_USING_FRAMES;
        let mut node_opt;
        loop {
            node_opt = self.get_token(TokenMode::IgnoreWhitespace);
            let Some(mut node) = node_opt else {
                break;
            };
            if self.tags_equal(node, frameset) && self.ntype(node) == NodeType::EndTag {
                self.tree.node_mut(frameset).closed = true;
                self.trim_spaces(frameset);
                return;
            }
            if self.insert_misc(frameset, node) {
                continue;
            }
            if !self.has_tag(node) {
                self.report(Some(frameset), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.is_element(node) && (self.node_model(node) & CM_HEAD) != 0 {
                self.move_to_head(frameset, node);
                continue;
            }
            if self.node_is(node, TagId::BODY) {
                self.unget_token();
                node = self.inferred_tag(TagId::NOFRAMES);
                self.report(Some(frameset), Some(node), Code::InsertingTag);
            }
            if self.ntype(node) == NodeType::StartTag && (self.node_model(node) & CM_FRAMES) != 0 {
                self.tree.insert_node_at_end(frameset, node);
                self.lexer.exclude_blocks = false;
                self.parse_tag(node, TokenMode::MixedContent);
                continue;
            } else if self.ntype(node) == NodeType::StartEndTag
                && (self.node_model(node) & CM_FRAMES) != 0
            {
                self.tree.insert_node_at_end(frameset, node);
                continue;
            }
            if self.node_is(node, TagId::A) {
                self.bad_access |= BA_INVALID_LINK_NOFRAMES;
            }
            self.report(Some(frameset), Some(node), Code::DiscardingUnexpected);
        }
        self.report(Some(frameset), node_opt, Code::MissingEndtagFor);
    }

    /// `TY_(ParseHTML)`.
    #[allow(
        clippy::too_many_lines,
        reason = "a one-to-one port of tidy's ParseHTML"
    )]
    fn parse_html(&mut self, html: NodeId, mode: TokenMode) {
        let mut frameset: Option<NodeId> = None;
        let mut noframes: Option<NodeId> = None;
        let head;
        loop {
            let node = self.get_token(TokenMode::IgnoreWhitespace);
            let Some(node) = node else {
                head = self.inferred_tag(TagId::HEAD);
                break;
            };
            if self.node_is(node, TagId::HEAD) {
                head = node;
                break;
            }
            if self.tags_equal(node, html) && self.ntype(node) == NodeType::EndTag {
                self.report(Some(html), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.tags_equal(node, html) && self.ntype(node) == NodeType::StartTag {
                self.report(Some(html), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.insert_misc(html, node) {
                continue;
            }
            self.unget_token();
            head = self.inferred_tag(TagId::HEAD);
            break;
        }
        self.tree.insert_node_at_end(html, head);
        self.parse_head(head);

        let body;
        loop {
            let node = self.get_token(TokenMode::IgnoreWhitespace);
            let Some(mut node) = node else {
                if frameset.is_none() {
                    let node = self.inferred_tag(TagId::BODY);
                    self.tree.insert_node_at_end(html, node);
                    self.parse_body(node, mode);
                }
                return;
            };
            if self.tags_equal(node, html) {
                if self.ntype(node) != NodeType::StartTag && frameset.is_none() {
                    self.report(Some(html), Some(node), Code::DiscardingUnexpected);
                }
                continue;
            }
            if self.insert_misc(html, node) {
                continue;
            }
            if self.node_is(node, TagId::BODY) {
                if self.ntype(node) != NodeType::StartTag {
                    self.report(Some(html), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                if let Some(fs) = frameset {
                    self.unget_token();
                    match noframes {
                        None => {
                            let nf = self.inferred_tag(TagId::NOFRAMES);
                            self.tree.insert_node_at_end(fs, nf);
                            self.report(Some(html), Some(nf), Code::InsertingTag);
                            noframes = Some(nf);
                        }
                        Some(nf) => {
                            if self.ntype(nf) == NodeType::StartEndTag {
                                self.tree.node_mut(nf).ntype = NodeType::StartTag;
                            }
                        }
                    }
                    let nf = noframes.expect("noframes was just set");
                    self.parse_tag(nf, mode);
                    continue;
                }
                self.constrain_version(!VERS_FRAMESET);
                body = node;
                break;
            }
            if self.node_is(node, TagId::FRAMESET) {
                if self.ntype(node) != NodeType::StartTag {
                    self.report(Some(html), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                if frameset.is_some() {
                    self.report(Some(html), Some(node), Code::DuplicateFrameset);
                } else {
                    frameset = Some(node);
                }
                self.tree.insert_node_at_end(html, node);
                self.parse_tag(node, mode);
                let fs = frameset.expect("frameset was just set");
                let mut child = self.tree.content(fs);
                while let Some(c) = child {
                    if self.node_is(c, TagId::NOFRAMES) {
                        noframes = Some(c);
                    }
                    child = self.tree.next(c);
                }
                continue;
            }
            if self.node_is(node, TagId::NOFRAMES) {
                if self.ntype(node) != NodeType::StartTag {
                    self.report(Some(html), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                let Some(fs) = frameset else {
                    self.report(Some(html), Some(node), Code::DiscardingUnexpected);
                    body = self.inferred_tag(TagId::BODY);
                    break;
                };
                if noframes.is_none() {
                    noframes = Some(node);
                    self.tree.insert_node_at_end(fs, node);
                }
                let nf = noframes.expect("noframes is set");
                self.parse_tag(nf, mode);
                continue;
            }
            if self.is_element(node) {
                if (self.node_model(node) & CM_HEAD) != 0 {
                    self.move_to_head(html, node);
                    continue;
                }
                if frameset.is_some() && self.node_is(node, TagId::FRAME) {
                    self.report(Some(html), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
            }
            self.unget_token();
            if let Some(fs) = frameset {
                match noframes {
                    None => {
                        let nf = self.inferred_tag(TagId::NOFRAMES);
                        self.tree.insert_node_at_end(fs, nf);
                        noframes = Some(nf);
                    }
                    Some(nf) => {
                        self.report(Some(html), Some(node), Code::NoframesContent);
                        if self.ntype(nf) == NodeType::StartEndTag {
                            self.tree.node_mut(nf).ntype = NodeType::StartTag;
                        }
                    }
                }
                self.constrain_version(VERS_FRAMESET);
                let nf = noframes.expect("noframes is set");
                self.parse_tag(nf, mode);
                continue;
            }
            node = self.inferred_tag(TagId::BODY);
            if !self.showing_body_only() {
                self.report(Some(html), Some(node), Code::InsertingTag);
            }
            self.constrain_version(!VERS_FRAMESET);
            body = node;
            break;
        }
        self.tree.insert_node_at_end(html, body);
        self.parse_tag(body, mode);
    }

    // ---- the document --------------------------------------------------------------

    fn node_cm_is_only_inline(&self, node: NodeId) -> bool {
        self.node_has_cm(node, CM_INLINE) && !self.node_has_cm(node, CM_BLOCK)
    }

    /// `EncloseBodyText`.
    fn enclose_body_text(&mut self) {
        let Some(body) = self.find_body() else {
            return;
        };
        let mut node = self.tree.content(body);
        while let Some(n) = node {
            if (self.is_text(n) && !self.is_blank(n))
                || (self.is_element(n) && self.node_cm_is_only_inline(n))
            {
                let p = self.inferred_tag(TagId::P);
                self.tree.insert_node_before_element(n, p);
                let mut cur = Some(n);
                while let Some(c) = cur
                    && (!self.is_element(c) || self.node_cm_is_only_inline(c))
                {
                    let next = self.tree.next(c);
                    self.tree.remove_node(c);
                    self.tree.insert_node_at_end(p, c);
                    cur = next;
                }
                self.trim_spaces(p);
                node = cur;
                continue;
            }
            node = self.tree.next(n);
        }
    }

    /// `EncloseBlockText`.
    fn enclose_block_text(&mut self, mut node: Option<NodeId>) {
        while let Some(n) = node {
            let next = self.tree.next(n);
            if let Some(content) = self.tree.content(n) {
                self.enclose_block_text(Some(content));
            }
            let is_target = matches!(
                self.tag_id(n),
                TagId::FORM | TagId::NOSCRIPT | TagId::BLOCKQUOTE
            );
            let Some(block) = self.tree.content(n).filter(|_| is_target) else {
                node = next;
                continue;
            };
            if (self.is_text(block) && !self.is_blank(block))
                || (self.is_element(block) && self.node_cm_is_only_inline(block))
            {
                let p = self.inferred_tag(TagId::P);
                self.tree.insert_node_before_element(block, p);
                let mut cur = Some(block);
                while let Some(c) = cur
                    && (!self.is_element(c) || self.node_cm_is_only_inline(c))
                {
                    let temp_next = self.tree.next(c);
                    self.tree.remove_node(c);
                    self.tree.insert_node_at_end(p, c);
                    cur = temp_next;
                }
                self.trim_spaces(p);
                continue;
            }
            node = next;
        }
    }

    /// `ReplaceObsoleteElements`.
    fn replace_obsolete_elements(&mut self, mut node: Option<NodeId>) {
        while let Some(n) = node {
            let next = self.tree.next(n);
            if self.node_is(n, TagId::DIR) {
                self.coerce_node(n, TagId::UL, true, true);
            }
            if matches!(
                self.tag_id(n),
                TagId::XMP | TagId::LISTING | TagId::PLAINTEXT
            ) {
                self.coerce_node(n, TagId::PRE, true, true);
            }
            if let Some(content) = self.tree.content(n) {
                self.replace_obsolete_elements(Some(content));
            }
            node = next;
        }
    }

    /// `TY_(ParseDocument)`.
    pub(crate) fn parse_document(&mut self) {
        let root: NodeId = 0;
        let mut doctype: Option<NodeId> = None;
        while let Some(node) = self.get_token(TokenMode::IgnoreWhitespace) {
            if self.ntype(node) == NodeType::XmlDecl {
                self.xml_detected = true;
                if self.find_xml_decl().is_some() && self.tree.content(root).is_some() {
                    self.report(Some(root), Some(node), Code::DiscardingUnexpected);
                    continue;
                }
                let nd = self.tree.node(node);
                if nd.line > 1 || nd.column != 1 {
                    self.report(Some(root), Some(node), Code::SpacePrecedingXmldecl);
                }
            }
            if self.insert_misc(root, node) {
                continue;
            }
            if self.ntype(node) == NodeType::DocType {
                if doctype.is_none() {
                    self.tree.insert_node_at_end(root, node);
                    doctype = Some(node);
                } else {
                    self.report(Some(root), Some(node), Code::DiscardingUnexpected);
                }
                continue;
            }
            if self.ntype(node) == NodeType::EndTag {
                self.report(Some(root), Some(node), Code::DiscardingUnexpected);
                continue;
            }
            if self.ntype(node) == NodeType::StartTag && self.node_is(node, TagId::HTML) {
                let known = tables::tables().known;
                if let Some(i) = self.tree.attr_by_id(node, known.xmlns)
                    && self.tree.node(node).attributes[i].value_is(XHTML_NAMESPACE)
                {
                    let html_out = self.opts.output_html;
                    self.lexer.isvoyager = true;
                    self.opts.output_xhtml = !html_out;
                    self.opts.output_xml = !html_out;
                }
            }
            let html = if self.ntype(node) != NodeType::StartTag || !self.node_is(node, TagId::HTML)
            {
                self.unget_token();
                self.inferred_tag(TagId::HTML)
            } else {
                node
            };
            if self.find_doctype().is_none() {
                let dtmode = self.opts.doctype_mode;
                if dtmode != DoctypeMode::Omit && !self.showing_body_only() {
                    self.report(None, None, Code::MissingDoctype);
                }
                if dtmode != DoctypeMode::Auto && dtmode != DoctypeMode::Html5 {
                    self.adjust_tags();
                }
            }
            self.tree.insert_node_at_end(root, html);
            self.parse_html(html, TokenMode::IgnoreWhitespace);
            break;
        }

        if self.find_html().is_none() {
            let html = self.inferred_tag(TagId::HTML);
            self.tree.insert_node_at_end(root, html);
            self.parse_html(html, TokenMode::IgnoreWhitespace);
        }

        match self.find_title() {
            None => {
                let head = self.find_head();
                if !self.showing_body_only() {
                    self.report(head, None, Code::MissingTitleElement);
                }
                let title = self.inferred_tag(TagId::TITLE);
                if let Some(head) = head {
                    self.tree.insert_node_at_end(head, title);
                }
            }
            Some(title) => {
                if self.tree.content(title).is_none()
                    && !self.showing_body_only()
                    && self.html5_mode
                {
                    self.report(Some(title), None, Code::BlankTitleElement);
                }
            }
        }

        self.attribute_checks(Some(root));
        self.replace_obsolete_elements(Some(root));
        self.drop_empty_elements(Some(root));
        self.clean_spaces(Some(root));

        if self.opts.enclose_text {
            self.enclose_body_text();
        }
        if self.opts.enclose_block_text {
            self.enclose_block_text(Some(root));
        }
    }
}

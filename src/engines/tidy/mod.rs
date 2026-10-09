//! A port of HTML Tidy 5.8.0's checks: the lexer, the parser with its
//! repairs, the attribute checks and the clean-up passes that produce
//! `tidy -errors` warnings and errors, without the pretty-printer. The
//! engine behind `processor.checker.itidy`.
//!
//! The port follows tidy's C sources file by file (lexer.c, parser.c,
//! attrs.c, tags.c, clean.c, tidylib.c, message.c) so that every message,
//! its position and the warning/error totals come out the same. Only
//! UTF-8 input is read; the options that rewrite the document for output
//! (`clean`, `word-2000`, ...) are refused rather than half-followed.

mod attrs;
mod clean;
mod config;
mod istack;
mod lexer;
mod message;
mod node;
mod parser;
mod stream;
mod tables;
#[cfg(test)]
mod tests;

pub use config::{CustomTags, DoctypeMode, Options, TriState, UppercaseAttrs};

use lexer::Lexer;
use node::{Node, NodeId, NodeType, Tree};
use stream::Stream;
use tables::{TagDef, TagId};

/// The warnings, errors and other counters tidy keeps on the document.
#[derive(Debug, Default, Clone, Copy)]
pub struct Counts {
    pub infos: u32,
    pub warnings: u32,
    pub errors: u32,
    pub option_errors: u32,
}

/// What a run reports: the report lines `tidy -errors` would print under
/// the options (not the dialogue tidy adds after them: the totals and the
/// "needs intervention" text), and the totals that decide the exit status.
#[derive(Debug, Clone)]
pub struct Report {
    pub lines: Vec<String>,
    pub warnings: u32,
    pub errors: u32,
}

/// An anchor (`id` or `name` on an anchor element) seen so far.
pub struct Anchor {
    name: String,
    node: NodeId,
}

/// tidy's `TidyDocImpl`: everything one document's run needs.
pub struct Doc {
    pub(crate) opts: Options,
    pub(crate) tree: Tree,
    pub(crate) lexer: Lexer,
    pub(crate) stream: Stream,
    /// The tag dictionary: tidy's built-ins (adjusted for HTML4 or HTML5
    /// mode) followed by the tags declared by options or found as
    /// autonomous custom tags.
    pub(crate) tags: Vec<TagDef>,
    pub(crate) counts: Counts,
    pub(crate) out: Vec<String>,
    pub(crate) bad_form: u32,
    pub(crate) bad_chars: u32,
    pub(crate) bad_layout: u32,
    pub(crate) bad_access: u32,
    pub(crate) footnotes: u32,
    pub(crate) html5_mode: bool,
    pub(crate) xml_detected: bool,
    pub(crate) given_doctype: Option<String>,
    pub(crate) anchors: Vec<Anchor>,
}

pub const FLG_BAD_FORM: u32 = 1;
pub const FLG_BAD_MAIN: u32 = 2;
pub const USING_SPACER: u32 = 1;
pub const USING_LAYER: u32 = 2;
pub const USING_NOBR: u32 = 4;
pub const USING_FONT: u32 = 8;
pub const BA_MISSING_IMAGE_ALT: u32 = 1;
pub const BA_MISSING_LINK_ALT: u32 = 2;
pub const BA_MISSING_SUMMARY: u32 = 4;
pub const BA_MISSING_IMAGE_MAP: u32 = 8;
pub const BA_USING_FRAMES: u32 = 16;
pub const BA_USING_NOFRAMES: u32 = 32;
pub const BA_INVALID_LINK_NOFRAMES: u32 = 64;
pub const FN_TRIM_EMPTY_ELEMENT: u32 = 1;

/// Run tidy's checks over one document.
pub fn check(input: &[u8], opts: &Options) -> Result<Report, String> {
    let mut doc = Doc::new(input.to_vec(), opts.clone());
    doc.stream.skip_bom()?;
    doc.declare_user_tags();
    doc.parse_document();
    doc.clean_and_repair();
    doc.report_summary();
    Ok(Report {
        lines: doc.out,
        warnings: doc.counts.warnings,
        errors: doc.counts.errors,
    })
}

impl Doc {
    fn new(input: Vec<u8>, opts: Options) -> Self {
        let tables = tables::tables();
        let mut tree = Tree::default();
        let mut root = Node::new(1, 1);
        root.ntype = NodeType::Root;
        tree.alloc(root);
        let stream = Stream::new(input, opts.tab_size, opts.keep_tabs);
        let mut doc = Self {
            opts,
            tree,
            lexer: Lexer::new(),
            stream,
            tags: tables.tags.clone(),
            counts: Counts::default(),
            out: Vec::new(),
            bad_form: 0,
            bad_chars: 0,
            bad_layout: 0,
            bad_access: 0,
            footnotes: 0,
            html5_mode: true,
            xml_detected: false,
            given_doctype: None,
            anchors: Vec::new(),
        };
        doc.reset_tags();
        doc
    }

    /// Read one character from the input, reporting any decoding error
    /// the stream met at the position it recorded (`ReadCharFromStream`).
    pub(crate) fn read_char(&mut self) -> u32 {
        let c = self.stream.read_char();
        if !self.stream.errors.is_empty() {
            let errors = std::mem::take(&mut self.stream.errors);
            for e in errors {
                self.lexer.lines = e.line as u32;
                self.lexer.columns = e.column as u32;
                self.report_encoding(message::Code::InvalidUtf8, e.code, false);
            }
        }
        c
    }

    pub(crate) fn unget_char(&mut self, c: u32) {
        self.stream.unget_char(c);
    }

    // ---- tag dictionary -------------------------------------------------

    /// The dictionary index of a built-in tag.
    pub(crate) fn tag_index(&self, id: TagId) -> usize {
        tables::tables().tag_index(id)
    }

    /// `TagId(node)`: the built-in identity of a node's tag, UNKNOWN for
    /// none or a declared tag.
    pub(crate) fn tag_id(&self, node: NodeId) -> TagId {
        self.tree
            .node(node)
            .tag
            .map_or(TagId::UNKNOWN, |t| self.tags[t].id)
    }

    /// `TagIsId(node, tid)`.
    pub(crate) fn node_is(&self, node: NodeId, id: TagId) -> bool {
        self.tree
            .node(node)
            .tag
            .is_some_and(|t| self.tags[t].id == id)
    }

    pub(crate) fn opt_node_is(&self, node: Option<NodeId>, id: TagId) -> bool {
        node.is_some_and(|n| self.node_is(n, id))
    }

    /// `TY_(nodeHasCM)`: does the node's tag have any of these content
    /// model bits?
    pub(crate) fn node_has_cm(&self, node: NodeId, cm: u32) -> bool {
        self.tree
            .node(node)
            .tag
            .is_some_and(|t| (self.tags[t].model & cm) != 0)
    }

    pub(crate) fn node_model(&self, node: NodeId) -> u32 {
        self.tree.node(node).tag.map_or(0, |t| self.tags[t].model)
    }

    pub(crate) fn node_parser(&self, node: NodeId) -> Option<tables::ParserKind> {
        self.tree.node(node).tag.and_then(|t| self.tags[t].parser)
    }

    /// `tagsLookup`: the dictionary index of a tag by name.
    pub(crate) fn lookup_tag(&self, name: &str) -> Option<usize> {
        self.tags.iter().position(|t| t.name == name)
    }

    /// `TY_(DefineTag)`: declare a user tag of the given kind.
    pub(crate) fn define_tag(&mut self, kind: UserTagType, name: &str) {
        let (model, parser) = match kind {
            UserTagType::Empty => (
                tables::CM_EMPTY | tables::CM_NO_INDENT | tables::CM_NEW,
                tables::ParserKind::ParseBlock,
            ),
            UserTagType::Inline => (
                tables::CM_INLINE | tables::CM_NO_INDENT | tables::CM_NEW,
                tables::ParserKind::ParseInline,
            ),
            UserTagType::Block => (
                tables::CM_BLOCK | tables::CM_NO_INDENT | tables::CM_NEW,
                tables::ParserKind::ParseBlock,
            ),
            UserTagType::Pre => (
                tables::CM_BLOCK | tables::CM_NO_INDENT | tables::CM_NEW,
                tables::ParserKind::ParsePre,
            ),
        };
        if let Some(existing) = self.lookup_tag(name) {
            // Never overwrite a predefined tag; a redeclared user tag gains
            // the new model bits (tidy ORs them in).
            if self.tags[existing].id == TagId::UNKNOWN {
                self.tags[existing].versions = tables::VERS_PROPRIETARY;
                self.tags[existing].model |= model;
                self.tags[existing].parser = Some(parser);
                self.tags[existing].check = None;
                self.tags[existing].attrvers = None;
            }
            return;
        }
        self.tags.push(TagDef {
            id: TagId::UNKNOWN,
            name: name.to_string(),
            versions: tables::VERS_PROPRIETARY,
            attrvers: None,
            model,
            parser: Some(parser),
            check: None,
        });
    }

    /// The tags declared by the `new-*-tags` options.
    fn declare_user_tags(&mut self) {
        let lists = [
            (UserTagType::Inline, self.opts.new_inline_tags.clone()),
            (UserTagType::Block, self.opts.new_blocklevel_tags.clone()),
            (UserTagType::Empty, self.opts.new_empty_tags.clone()),
            (UserTagType::Pre, self.opts.new_pre_tags.clone()),
        ];
        for (kind, names) in lists {
            for name in names {
                self.define_tag(kind, &name);
            }
        }
    }

    /// `TY_(DeclareUserTag)` for an autonomous custom tag: its kind comes
    /// from the `custom-tags` option.
    pub(crate) fn declare_custom_tag(&mut self, name: &str) {
        let kind = match self.opts.custom_tags {
            CustomTags::Blocklevel => UserTagType::Block,
            CustomTags::Empty => UserTagType::Empty,
            CustomTags::Inline => UserTagType::Inline,
            CustomTags::Pre => UserTagType::Pre,
            CustomTags::No => return,
        };
        self.define_tag(kind, name);
    }

    /// `TY_(ResetTags)`: the HTML5 shape of the tags that differ between
    /// HTML4 and HTML5.
    pub(crate) fn reset_tags(&mut self) {
        let a = self.tag_index(TagId::A);
        self.tags[a].parser = Some(tables::ParserKind::ParseBlock);
        self.tags[a].model = tables::CM_INLINE | tables::CM_BLOCK | tables::CM_MIXED;
        let caption = self.tag_index(TagId::CAPTION);
        self.tags[caption].parser = Some(tables::ParserKind::ParseBlock);
        let object = self.tag_index(TagId::OBJECT);
        self.tags[object].model =
            tables::CM_OBJECT | tables::CM_IMG | tables::CM_INLINE | tables::CM_PARAM;
        let button = self.tag_index(TagId::BUTTON);
        self.tags[button].parser = Some(tables::ParserKind::ParseInline);
        self.html5_mode = true;
    }

    /// `TY_(AdjustTags)`: back to the HTML4 shape.
    pub(crate) fn adjust_tags(&mut self) {
        let a = self.tag_index(TagId::A);
        self.tags[a].parser = Some(tables::ParserKind::ParseInline);
        self.tags[a].model = tables::CM_INLINE;
        let caption = self.tag_index(TagId::CAPTION);
        self.tags[caption].parser = Some(tables::ParserKind::ParseInline);
        let object = self.tag_index(TagId::OBJECT);
        self.tags[object].model |= tables::CM_HEAD;
        let button = self.tag_index(TagId::BUTTON);
        self.tags[button].parser = Some(tables::ParserKind::ParseBlock);
        self.html5_mode = false;
    }

    // ---- finding the document's landmarks --------------------------------

    pub(crate) fn find_doctype(&self) -> Option<NodeId> {
        let mut node = self.tree.content(0);
        while let Some(n) = node {
            if self.tree.node(n).ntype == NodeType::DocType {
                return Some(n);
            }
            node = self.tree.next(n);
        }
        None
    }

    pub(crate) fn find_html(&self) -> Option<NodeId> {
        let mut node = self.tree.content(0);
        while let Some(n) = node {
            if self.node_is(n, TagId::HTML) {
                return Some(n);
            }
            node = self.tree.next(n);
        }
        None
    }

    pub(crate) fn find_xml_decl(&self) -> Option<NodeId> {
        let mut node = self.tree.content(0);
        while let Some(n) = node {
            if self.tree.node(n).ntype == NodeType::XmlDecl {
                return Some(n);
            }
            node = self.tree.next(n);
        }
        None
    }

    pub(crate) fn find_head(&self) -> Option<NodeId> {
        let html = self.find_html()?;
        let mut node = self.tree.content(html);
        while let Some(n) = node {
            if self.node_is(n, TagId::HEAD) {
                return Some(n);
            }
            node = self.tree.next(n);
        }
        None
    }

    pub(crate) fn find_title(&self) -> Option<NodeId> {
        let head = self.find_head()?;
        let mut node = self.tree.content(head);
        while let Some(n) = node {
            if self.node_is(n, TagId::TITLE) {
                return Some(n);
            }
            node = self.tree.next(n);
        }
        None
    }

    pub(crate) fn find_body(&self) -> Option<NodeId> {
        let html = self.find_html()?;
        let mut node = self.tree.content(html);
        while let Some(n) = node {
            if self.node_is(n, TagId::BODY) || self.node_is(n, TagId::FRAMESET) {
                break;
            }
            node = self.tree.next(n);
        }
        let mut node = node?;
        if self.node_is(node, TagId::FRAMESET) {
            let mut child = self.tree.content(node);
            while let Some(c) = child {
                if self.node_is(c, TagId::NOFRAMES) {
                    break;
                }
                child = self.tree.next(c);
            }
            let noframes = child?;
            let mut inner = self.tree.content(noframes);
            while let Some(c) = inner {
                if self.node_is(c, TagId::BODY) {
                    break;
                }
                inner = self.tree.next(c);
            }
            node = inner?;
        }
        Some(node)
    }

    // ---- text helpers ------------------------------------------------------

    /// The text of a text node.
    pub(crate) fn node_text(&self, node: NodeId) -> &[u8] {
        let n = self.tree.node(node);
        let end = n.end.min(self.lexer.lexbuf.len());
        &self.lexer.lexbuf[n.start.min(end)..end]
    }

    /// `TY_(IsBlank)`: a text node that is empty or one blank.
    pub(crate) fn is_blank(&self, node: NodeId) -> bool {
        let n = self.tree.node(node);
        n.is_text()
            && (n.end == n.start || (n.end == n.start + 1 && self.lexer.lexbuf[n.start] == b' '))
    }

    /// `TY_(TextNodeEndWithSpace)`.
    pub(crate) fn text_node_ends_with_space(&self, node: NodeId) -> bool {
        let n = self.tree.node(node);
        n.is_text() && n.end > n.start && self.lexer.lexbuf[n.end - 1] == b' '
    }
}

/// tidy's `UserTagType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserTagType {
    Empty,
    Inline,
    Block,
    Pre,
}

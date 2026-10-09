//! The inline stack (tidy's istack.c): the inline elements currently open,
//! kept so that their effect can be re-created inside block elements and
//! after mismatched end tags, as Mosaic did.

use super::Doc;
use super::node::{AttVal, Node, NodeId, NodeType};
use super::tables::{CM_INLINE, CM_OBJECT, TagId};

/// One entry of the inline stack: the tag, its name and attributes.
#[derive(Clone, Debug)]
pub struct IStack {
    pub tag: usize,
    pub element: String,
    pub attributes: Vec<AttVal>,
}

impl Doc {
    /// `TY_(DupAttrs)`: a copy of an attribute list, server-side nodes
    /// cloned too.
    pub(crate) fn dup_attrs(&mut self, attrs: &[AttVal]) -> Vec<AttVal> {
        let mut out = Vec::with_capacity(attrs.len());
        for a in attrs {
            let mut copy = a.clone();
            copy.asp = a.asp.map(|n| self.clone_node(Some(n)));
            copy.php = a.php.map(|n| self.clone_node(Some(n)));
            copy.dict = copy
                .attribute
                .as_deref()
                .and_then(|name| super::tables::tables().attr_index(name));
            out.push(copy);
        }
        out
    }

    /// `IsNodePushable`.
    fn is_node_pushable(&self, node: NodeId) -> bool {
        let Some(tag) = self.tree.node(node).tag else {
            return false;
        };
        let model = self.tags[tag].model;
        if (model & CM_INLINE) == 0 {
            return false;
        }
        if (model & CM_OBJECT) != 0 {
            return false;
        }
        if self.node_is(node, TagId::INS) || self.node_is(node, TagId::DEL) {
            return false;
        }
        true
    }

    /// `TY_(PushInline)`.
    pub(crate) fn push_inline(&mut self, node: NodeId) {
        if self.tree.node(node).implicit {
            return;
        }
        if !self.is_node_pushable(node) {
            return;
        }
        if !self.node_is(node, TagId::FONT) && self.is_pushed(node) {
            return;
        }
        let n = self.tree.node(node);
        let tag = n.tag.expect("pushable nodes have a tag");
        let element = n.element.clone().unwrap_or_default();
        let attributes = n.attributes.clone();
        let attributes = self.dup_attrs(&attributes);
        self.lexer.istack.push(IStack {
            tag,
            element,
            attributes,
        });
    }

    fn pop_istack(&mut self) {
        self.lexer.istack.pop();
    }

    /// Pop entries until one of this tag has been popped (tidy inspects
    /// the entry it just removed).
    fn pop_istack_until(&mut self, id: TagId) {
        while let Some(popped) = self.lexer.istack.pop() {
            if self.tags[popped.tag].id == id {
                break;
            }
        }
    }

    /// `TY_(PopInline)`: `node` is the end tag being closed, or `None`
    /// to pop the top entry.
    pub(crate) fn pop_inline(&mut self, node: Option<NodeId>) {
        if let Some(node) = node {
            if !self.is_node_pushable(node) {
                return;
            }
            if self.node_is(node, TagId::A) {
                self.pop_istack_until(TagId::A);
                return;
            }
        }
        if !self.lexer.istack.is_empty() {
            self.pop_istack();
            if let Some(insert) = self.lexer.insert
                && insert >= self.lexer.istack.len()
            {
                self.lexer.insert = None;
            }
        }
    }

    /// `TY_(IsPushed)`.
    pub(crate) fn is_pushed(&self, node: NodeId) -> bool {
        let tag = self.tree.node(node).tag;
        self.lexer.istack.iter().rev().any(|e| Some(e.tag) == tag)
    }

    /// `TY_(IsPushedLast)`: is the top of the stack the same tag as `node`?
    pub(crate) fn is_pushed_last(&self, element: Option<NodeId>, node: NodeId) -> bool {
        if let Some(e) = element
            && !self.is_node_pushable(e)
        {
            return false;
        }
        match self.lexer.istack.last() {
            Some(top) => Some(top.tag) == self.tree.node(node).tag,
            None => false,
        }
    }

    /// `TY_(InlineDup)`: arrange for the open inlines above the stack base
    /// to be re-inserted as tokens, `node` first if given. Returns how
    /// many there are.
    pub(crate) const fn inline_dup(&mut self, node: Option<NodeId>) -> usize {
        let n = self
            .lexer
            .istack
            .len()
            .saturating_sub(self.lexer.istackbase);
        if n > 0 {
            self.lexer.insert = Some(self.lexer.istackbase);
            self.lexer.inode = node;
        }
        n
    }

    /// `TY_(DeferDup)`.
    pub(crate) const fn defer_dup(&mut self) {
        self.lexer.insert = None;
        self.lexer.inode = None;
    }

    /// `TY_(InsertedToken)`: the next re-inserted inline start tag, or the
    /// deferred node once the stack is exhausted.
    pub(crate) fn inserted_token(&mut self) -> Option<NodeId> {
        let Some(insert) = self.lexer.insert else {
            let node = self.lexer.inode;
            self.lexer.inode = None;
            return node;
        };
        if self.lexer.inode.is_none() {
            self.lexer.lines = self.stream.curline as u32;
            self.lexer.columns = self.stream.curcol as u32;
        }
        let mut node = Node::new(self.lexer.lines, self.lexer.columns);
        node.ntype = NodeType::StartTag;
        node.implicit = true;
        node.start = self.lexer.txtstart;
        node.end = self.lexer.txtend;
        let entry = self.lexer.istack[insert].clone();
        node.element = Some(entry.element);
        node.tag = Some(entry.tag);
        let id = self.tree.alloc(node);
        let attributes = self.dup_attrs(&entry.attributes);
        self.tree.node_mut(id).attributes = attributes;
        let next = insert + 1;
        self.lexer.insert = if next < self.lexer.istack.len() {
            Some(next)
        } else {
            None
        };
        Some(id)
    }

    /// `TY_(SwitchInline)`: swap the stack entries of `element` and `node`
    /// (an inline closed by the wrong end tag) when both are pushed.
    pub(crate) fn switch_inline(&mut self, element: NodeId, node: NodeId) -> bool {
        let (Some(etag), Some(ntag)) = (self.tree.node(element).tag, self.tree.node(node).tag)
        else {
            return false;
        };
        if !(self.is_pushed(element)
            && self.is_pushed(node)
            && self.lexer.istack.len() - self.lexer.istackbase >= 2)
        {
            return false;
        }
        // tidy indexes from the top of the frame down to entry 0, not to
        // the frame base.
        let mut i = (self.lexer.istack.len() - self.lexer.istackbase) as isize - 1;
        while i >= 0 {
            let iu = i as usize;
            if self.lexer.istack[iu].tag == etag {
                let mut j = i - 1;
                while j >= 0 {
                    let ju = j as usize;
                    if self.lexer.istack[ju].tag == ntag {
                        self.lexer.istack.swap(iu, ju);
                        return true;
                    }
                    j -= 1;
                }
            }
            i -= 1;
        }
        false
    }

    /// `TY_(InlineDup1)`: re-insert one specific pushed element.
    pub(crate) fn inline_dup1(&mut self, node: Option<NodeId>, element: Option<NodeId>) -> bool {
        let Some(element) = element else {
            return false;
        };
        let Some(etag) = self.tree.node(element).tag else {
            return false;
        };
        let n = self
            .lexer
            .istack
            .len()
            .saturating_sub(self.lexer.istackbase);
        if n == 0 {
            return false;
        }
        for i in (0..n).rev() {
            if self.lexer.istack[i].tag == etag {
                self.lexer.insert = Some(i);
                self.lexer.inode = node;
                return true;
            }
        }
        false
    }
}

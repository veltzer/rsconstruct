//! checkstyle's AST: `DetailAstImpl` over ANTLR 2's `BaseAST`, built by
//! ANTLR's `ASTFactory`, in an arena.
//!
//! The checks walk parent, sibling and child links, and those links are
//! whatever ANTLR's tree building left behind, so the operations here are
//! the Java ones step by step: `setFirstChild` and `setNextSibling` set
//! the parent of the whole sibling chain they attach, `setNextSibling`
//! records the previous sibling but `setFirstChild` does not (a node moved
//! to the front of a list keeps its old previous sibling), and `addChild`
//! links the new last child directly. A node made from a token type and a
//! name (an imaginary node such as `TYPE`) has no position of its own: its
//! line and column are its first child's, else its next sibling's, looked
//! up each time they are asked for.

use std::rc::Rc;

use super::lexer::Token;

pub type NodeId = u32;

/// `DetailAstImpl.NOT_INITIALIZED`.
pub const NOT_INITIALIZED: i32 = i32::MIN;

#[derive(Clone, Debug)]
pub struct Node {
    pub ty: u16,
    pub text: Rc<str>,
    line: i32,
    col: i32,
    first_child: Option<NodeId>,
    next_sibling: Option<NodeId>,
    parent: Option<NodeId>,
    prev_sibling: Option<NodeId>,
}

#[derive(Default)]
pub struct Ast {
    nodes: Vec<Node>,
}

/// ANTLR's `ASTPair`: the tree a rule is building and the child that new
/// siblings attach to.
#[derive(Clone, Copy, Default)]
pub struct Pair {
    pub root: Option<NodeId>,
    pub child: Option<NodeId>,
}

impl Ast {
    fn push(&mut self, ty: u16, text: Rc<str>, line: i32, col: i32) -> NodeId {
        let id = NodeId::try_from(self.nodes.len()).expect("AST arena overflow");
        self.nodes.push(Node {
            ty,
            text,
            line,
            col,
            first_child: None,
            next_sibling: None,
            parent: None,
            prev_sibling: None,
        });
        id
    }

    /// `astFactory.create(Token)`: `DetailAstImpl.initialize(Token)`, with
    /// ANTLR's 1-based column made 0-based.
    pub fn token_node(&mut self, token: &Token) -> NodeId {
        let text = token.text.clone().unwrap_or_else(|| Rc::from("null"));
        self.push(token.ty, text, token.line, token.col - 1)
    }

    /// `astFactory.create(type, text)`: an imaginary node.
    pub fn imaginary(&mut self, ty: u16, text: &str) -> NodeId {
        self.push(ty, Rc::from(text), NOT_INITIALIZED, NOT_INITIALIZED)
    }

    fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id as usize]
    }

    pub fn ty(&self, id: NodeId) -> u16 {
        self.node(id).ty
    }

    pub fn text(&self, id: NodeId) -> &str {
        &self.node(id).text
    }

    pub fn set_type(&mut self, id: NodeId, ty: u16) {
        self.node_mut(id).ty = ty;
    }

    pub fn set_text(&mut self, id: NodeId, text: &str) {
        self.node_mut(id).text = Rc::from(text);
    }

    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).first_child
    }

    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).next_sibling
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }

    pub fn previous_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).prev_sibling
    }

    /// `getLineNo()`.
    pub fn line(&self, id: NodeId) -> i32 {
        let node = self.node(id);
        if node.line == NOT_INITIALIZED {
            if let Some(child) = node.first_child {
                return self.line(child);
            }
            if let Some(next) = node.next_sibling {
                return self.line(next);
            }
        }
        node.line
    }

    /// `getColumnNo()`, 0-based.
    pub fn column(&self, id: NodeId) -> i32 {
        let node = self.node(id);
        if node.col == NOT_INITIALIZED {
            if let Some(child) = node.first_child {
                return self.column(child);
            }
            if let Some(next) = node.next_sibling {
                return self.column(next);
            }
        }
        node.col
    }

    /// `setParent`: the node and every next sibling of it.
    fn set_parent(&mut self, id: NodeId, parent: Option<NodeId>) {
        let mut current = Some(id);
        while let Some(node) = current {
            self.node_mut(node).parent = parent;
            current = self.next_sibling(node);
        }
    }

    /// `DetailAstImpl.setFirstChild`.
    pub fn set_first_child(&mut self, id: NodeId, child: Option<NodeId>) {
        self.node_mut(id).first_child = child;
        if let Some(child) = child {
            self.set_parent(child, Some(id));
        }
    }

    /// `DetailAstImpl.setNextSibling`.
    pub fn set_next_sibling(&mut self, id: NodeId, next: Option<NodeId>) {
        self.node_mut(id).next_sibling = next;
        if let Some(next) = next {
            if let Some(parent) = self.parent(id) {
                self.set_parent(next, Some(parent));
            }
            self.node_mut(next).prev_sibling = Some(id);
        }
    }

    /// `DetailAstImpl.addChild`: appended after the last child.
    pub fn add_child(&mut self, id: NodeId, child: Option<NodeId>) {
        let Some(child) = child else {
            return;
        };
        self.set_parent(child, Some(id));
        let last = self.last_child(id);
        self.node_mut(child).prev_sibling = last;
        match last {
            Some(last) => self.node_mut(last).next_sibling = Some(child),
            None => self.node_mut(id).first_child = Some(child),
        }
    }

    pub fn last_child(&self, id: NodeId) -> Option<NodeId> {
        let mut child = self.first_child(id)?;
        while let Some(next) = self.next_sibling(child) {
            child = next;
        }
        Some(child)
    }

    /// `findFirstToken(type)`: the first child of that type.
    pub fn find_first_token(&self, id: NodeId, ty: u16) -> Option<NodeId> {
        self.children(id).find(|&c| self.ty(c) == ty)
    }

    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.first_child(id), |&c| self.next_sibling(c))
    }

    pub fn child_count(&self, id: NodeId) -> usize {
        self.children(id).count()
    }

    /// `ASTFactory.dup`: `DetailAstImpl.initialize(AST)` copies the text,
    /// the type and the position as the getters give it now.
    fn dup(&mut self, id: NodeId) -> NodeId {
        let (ty, text) = (self.ty(id), self.node(id).text.clone());
        let (line, col) = (self.line(id), self.column(id));
        self.push(ty, text, line, col)
    }

    /// `ASTFactory.dupTree`.
    pub fn dup_tree(&mut self, id: Option<NodeId>) -> Option<NodeId> {
        let id = id?;
        let result = self.dup(id);
        let children = self.first_child(id);
        let copy = self.dup_list(children);
        self.set_first_child(result, copy);
        Some(result)
    }

    /// `ASTFactory.dupList`: the node and its next siblings.
    pub fn dup_list(&mut self, id: Option<NodeId>) -> Option<NodeId> {
        let result = self.dup_tree(id);
        let mut copy = result;
        let mut original = id;
        while let Some(node) = original {
            original = self.next_sibling(node);
            let next = self.dup_tree(original);
            if let Some(c) = copy {
                self.set_next_sibling(c, next);
            }
            copy = next;
        }
        result
    }

    /// `ASTFactory.make(nodes)`: the first node is the root of the rest.
    pub fn make(&mut self, nodes: &[Option<NodeId>]) -> Option<NodeId> {
        let mut root = *nodes.first()?;
        if let Some(r) = root {
            self.set_first_child(r, None);
        }
        let mut tail: Option<NodeId> = None;
        for &node in &nodes[1..] {
            let Some(node) = node else {
                continue;
            };
            let mut t = match (root, tail) {
                (None, _) => {
                    root = Some(node);
                    node
                }
                (Some(r), None) => {
                    self.set_first_child(r, Some(node));
                    node
                }
                (Some(_), Some(t)) => {
                    self.set_next_sibling(t, Some(node));
                    node
                }
            };
            while let Some(next) = self.next_sibling(t) {
                t = next;
            }
            tail = Some(t);
        }
        root
    }

    /// `ASTFactory.addASTChild`.
    pub fn add_ast_child(&mut self, pair: &mut Pair, child: Option<NodeId>) {
        let Some(child) = child else {
            return;
        };
        match (pair.root, pair.child) {
            (None, _) => pair.root = Some(child),
            (Some(root), None) => self.set_first_child(root, Some(child)),
            (Some(_), Some(c)) => self.set_next_sibling(c, Some(child)),
        }
        pair.child = Some(child);
        self.advance_child_to_end(pair);
    }

    /// `ASTFactory.makeASTRoot`.
    pub fn make_ast_root(&mut self, pair: &mut Pair, root: Option<NodeId>) {
        let Some(root) = root else {
            return;
        };
        self.add_child(root, pair.root);
        pair.child = pair.root;
        self.advance_child_to_end(pair);
        pair.root = Some(root);
    }

    pub fn advance_child_to_end(&self, pair: &mut Pair) {
        if let Some(mut child) = pair.child {
            while let Some(next) = self.next_sibling(child) {
                child = next;
            }
            pair.child = Some(child);
        }
    }

    /// The rule result pattern `#rule = #(root, #rule)` followed by
    /// `currentAST.root = #rule; currentAST.child = first child or root;
    /// advanceChildToEnd()`.
    pub fn set_rule_tree(&self, pair: &mut Pair, tree: Option<NodeId>) {
        pair.root = tree;
        pair.child = tree.map(|t| self.first_child(t).unwrap_or(t));
        self.advance_child_to_end(pair);
    }
}

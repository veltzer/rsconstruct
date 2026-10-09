//! The parse tree: nodes in an arena, linked by index the way tidy links
//! them by pointer (parent, prev, next, first and last child), and the
//! attribute list of a node.
//!
//! tidy frees nodes it discards; here a discarded node simply stays in the
//! arena, detached, which keeps every `Node*` of the C code a plain index.

/// Index of a node in [`Tree::nodes`]. Node 0 is the document root.
pub type NodeId = usize;

/// tidy's `NodeType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeType {
    Root,
    DocType,
    Comment,
    ProcIns,
    Text,
    StartTag,
    EndTag,
    StartEndTag,
    Cdata,
    Section,
    Asp,
    Jste,
    Php,
    XmlDecl,
}

/// One attribute of a start tag (tidy's `AttVal`). `dict` is the index of
/// the attribute in the attribute table when the name is a known one.
#[derive(Clone, Debug)]
pub struct AttVal {
    pub dict: Option<usize>,
    pub asp: Option<NodeId>,
    pub php: Option<NodeId>,
    /// The quote character the value had, `0` for none.
    pub delim: u32,
    pub attribute: Option<String>,
    pub value: Option<String>,
}

impl AttVal {
    pub const fn new() -> Self {
        Self {
            dict: None,
            asp: None,
            php: None,
            delim: 0,
            attribute: None,
            value: None,
        }
    }

    pub const fn has_value(&self) -> bool {
        self.value.is_some()
    }

    /// tidy's `AttrValueIs`: a case-insensitive comparison of the value.
    pub fn value_is(&self, val: &str) -> bool {
        self.value
            .as_deref()
            .is_some_and(|v| v.eq_ignore_ascii_case(val))
    }

    /// tidy's `AttrContains`: a substring test on the value.
    pub fn value_contains(&self, val: &str) -> bool {
        self.value.as_deref().is_some_and(|v| v.contains(val))
    }
}

impl Default for AttVal {
    fn default() -> Self {
        Self::new()
    }
}

/// tidy's `Node`. Text content is the span `start..end` of the lexer's
/// buffer; `tag` and `was` index the document's tag table.
#[derive(Clone, Debug)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub prev: Option<NodeId>,
    pub next: Option<NodeId>,
    pub content: Option<NodeId>,
    pub last: Option<NodeId>,
    pub attributes: Vec<AttVal>,
    pub was: Option<usize>,
    pub tag: Option<usize>,
    pub element: Option<String>,
    pub start: usize,
    pub end: usize,
    pub ntype: NodeType,
    pub line: u32,
    pub column: u32,
    pub closed: bool,
    pub implicit: bool,
    pub linebreak: bool,
}

impl Node {
    pub const fn new(line: u32, column: u32) -> Self {
        Self {
            parent: None,
            prev: None,
            next: None,
            content: None,
            last: None,
            attributes: Vec::new(),
            was: None,
            tag: None,
            element: None,
            start: 0,
            end: 0,
            ntype: NodeType::Text,
            line,
            column,
            closed: false,
            implicit: false,
            linebreak: false,
        }
    }

    /// tidy's `nodeIsElement`: a start tag or an empty element.
    pub const fn is_element(&self) -> bool {
        matches!(self.ntype, NodeType::StartTag | NodeType::StartEndTag)
    }

    pub fn is_text(&self) -> bool {
        self.ntype == NodeType::Text
    }
}

/// The node arena and the tree operations of parser.c that only touch
/// links.
#[derive(Debug, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
}

impl Tree {
    pub fn alloc(&mut self, node: Node) -> NodeId {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id]
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].parent
    }

    pub fn next(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].next
    }

    pub fn prev(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].prev
    }

    pub fn content(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].content
    }

    pub fn last(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].last
    }

    /// `TY_(RemoveNode)`: extract a node (with its children) from the tree.
    pub fn remove_node(&mut self, node: NodeId) {
        let (parent, prev, next) = {
            let n = &self.nodes[node];
            (n.parent, n.prev, n.next)
        };
        if let Some(p) = prev {
            self.nodes[p].next = next;
        }
        if let Some(nx) = next {
            self.nodes[nx].prev = prev;
        }
        if let Some(par) = parent {
            if self.nodes[par].content == Some(node) {
                self.nodes[par].content = next;
            }
            if self.nodes[par].last == Some(node) {
                self.nodes[par].last = prev;
            }
        }
        let n = &mut self.nodes[node];
        n.parent = None;
        n.prev = None;
        n.next = None;
    }

    /// `TY_(DiscardElement)`: remove a node and return what followed it.
    pub fn discard_element(&mut self, element: NodeId) -> Option<NodeId> {
        let next = self.nodes[element].next;
        self.remove_node(element);
        next
    }

    /// `TY_(InsertNodeAtStart)`.
    pub fn insert_node_at_start(&mut self, element: NodeId, node: NodeId) {
        let first = self.nodes[element].content;
        self.nodes[node].parent = Some(element);
        match first {
            None => self.nodes[element].last = Some(node),
            Some(f) => self.nodes[f].prev = Some(node),
        }
        self.nodes[node].next = first;
        self.nodes[node].prev = None;
        self.nodes[element].content = Some(node);
    }

    /// `TY_(InsertNodeAtEnd)`.
    pub fn insert_node_at_end(&mut self, element: NodeId, node: NodeId) {
        let last = self.nodes[element].last;
        self.nodes[node].parent = Some(element);
        self.nodes[node].prev = last;
        match last {
            Some(l) => self.nodes[l].next = Some(node),
            None => self.nodes[element].content = Some(node),
        }
        self.nodes[element].last = Some(node);
    }

    /// `InsertNodeAsParent`: put `node` where `element` was, with
    /// `element` as its only child.
    pub fn insert_node_as_parent(&mut self, element: NodeId, node: NodeId) {
        let (parent, prev, next) = {
            let e = &self.nodes[element];
            (e.parent, e.prev, e.next)
        };
        self.nodes[node].content = Some(element);
        self.nodes[node].last = Some(element);
        self.nodes[node].parent = parent;
        self.nodes[element].parent = Some(node);
        if let Some(p) = parent {
            if self.nodes[p].content == Some(element) {
                self.nodes[p].content = Some(node);
            }
            if self.nodes[p].last == Some(element) {
                self.nodes[p].last = Some(node);
            }
        }
        self.nodes[node].prev = prev;
        self.nodes[element].prev = None;
        if let Some(p) = prev {
            self.nodes[p].next = Some(node);
        }
        self.nodes[node].next = next;
        self.nodes[element].next = None;
        if let Some(n) = next {
            self.nodes[n].prev = Some(node);
        }
    }

    /// `TY_(InsertNodeBeforeElement)`.
    pub fn insert_node_before_element(&mut self, element: NodeId, node: NodeId) {
        let parent = self.nodes[element].parent;
        let prev = self.nodes[element].prev;
        self.nodes[node].parent = parent;
        self.nodes[node].next = Some(element);
        self.nodes[node].prev = prev;
        self.nodes[element].prev = Some(node);
        if let Some(p) = prev {
            self.nodes[p].next = Some(node);
        }
        if let Some(par) = parent
            && self.nodes[par].content == Some(element)
        {
            self.nodes[par].content = Some(node);
        }
    }

    /// `TY_(InsertNodeAfterElement)`.
    pub fn insert_node_after_element(&mut self, element: NodeId, node: NodeId) {
        let parent = self.nodes[element].parent;
        self.nodes[node].parent = parent;
        if let Some(par) = parent
            && self.nodes[par].last == Some(element)
        {
            self.nodes[par].last = Some(node);
        } else {
            let next = self.nodes[element].next;
            self.nodes[node].next = next;
            if let Some(n) = next {
                self.nodes[n].prev = Some(node);
            }
        }
        self.nodes[element].next = Some(node);
        self.nodes[node].prev = Some(element);
    }

    /// `TY_(FixNodeLinks)`: make the links around `node` agree with it.
    pub fn fix_node_links(&mut self, node: NodeId) {
        let (parent, prev, next) = {
            let n = &self.nodes[node];
            (n.parent, n.prev, n.next)
        };
        let parent = parent.expect("fix_node_links on a node without parent");
        match prev {
            Some(p) => self.nodes[p].next = Some(node),
            None => self.nodes[parent].content = Some(node),
        }
        match next {
            Some(n) => self.nodes[n].prev = Some(node),
            None => self.nodes[parent].last = Some(node),
        }
        let mut child = self.nodes[node].content;
        while let Some(c) = child {
            self.nodes[c].parent = Some(node);
            child = self.nodes[c].next;
        }
    }

    /// `DescendantOf` on tag indices: does an ancestor carry this tag?
    pub fn descendant_of(&self, element: NodeId, tag: usize) -> bool {
        let mut parent = self.nodes[element].parent;
        while let Some(p) = parent {
            if self.nodes[p].tag == Some(tag) {
                return true;
            }
            parent = self.nodes[p].parent;
        }
        false
    }

    /// `TY_(GetAttrByName)`: the attribute with exactly this name.
    pub fn attr_by_name(&self, node: NodeId, name: &str) -> Option<usize> {
        self.nodes[node]
            .attributes
            .iter()
            .position(|a| a.attribute.as_deref() == Some(name))
    }

    /// `TY_(AttrGetById)`: the attribute with this dictionary index.
    pub fn attr_by_id(&self, node: NodeId, id: usize) -> Option<usize> {
        self.nodes[node]
            .attributes
            .iter()
            .position(|a| a.dict == Some(id))
    }
}

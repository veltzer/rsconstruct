//! The AST, shaped like luacheck's (almost `MetaLua`) tables.
//!
//! Every Lua table of the original AST is a [`Node`] in one arena: tagged
//! nodes (`Id`, `Call`, `If`, ...) and the untagged lists that hold
//! statements, arguments or expression lists. A node's array part is
//! `items`, whose elements are child nodes or strings (an `Id`'s name, an
//! `Op`'s operator, a `String`'s value), exactly where the Lua tables keep
//! them, so the analysis stages read like their Lua counterparts.

/// A location: the line of the first character, and the first and last
/// character offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub line: usize,
    pub offset: usize,
    pub end_offset: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Id,
    String,
    Number,
    Nil,
    True,
    False,
    Dots,
    Table,
    Pair,
    Function,
    Index,
    Call,
    Invoke,
    Paren,
    Op,
    If,
    While,
    Do,
    Fornum,
    Forin,
    Repeat,
    Set,
    OpSet,
    Local,
    Localrec,
    Label,
    Return,
    Break,
    Goto,
}

pub type NodeId = usize;

#[derive(Clone, Debug)]
pub enum Item {
    Node(NodeId),
    Str(Vec<u8>),
}

/// How a global-related node resolves (`detect_globals`'s `node.resolution`).
#[derive(Clone, Debug)]
pub enum Resolution {
    Unknown,
    NotString,
    /// A `String` node.
    Str(NodeId),
    /// An indexing of a global: the global's `Id` node, then keys, each
    /// `Unknown`, `NotString` or a `String` node.
    Index {
        keys: Vec<Self>,
        global: NodeId,
        previous_indexing_len: Option<usize>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub tag: Option<Tag>,
    pub range: Option<Range>,
    pub items: Vec<Item>,
    /// The implicit `self` argument of a method.
    pub implicit: bool,
    /// A `Function`'s `end` token.
    pub end_range: Option<Range>,
    /// The local variable an `Id` (or `Dots`) refers to.
    pub var: Option<usize>,
    /// The value a `Function` node is assigned as.
    pub value: Option<usize>,
    /// A `Function`'s name (`name_functions`).
    pub name: Option<Vec<u8>>,
    pub resolution: Option<Resolution>,
    pub contains_call: Option<bool>,
}

#[derive(Default)]
pub struct Ast {
    pub nodes: Vec<Node>,
}

impl Ast {
    pub fn add(&mut self, node: Node) -> NodeId {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// A new untagged list without a range.
    pub fn list(&mut self, items: Vec<NodeId>) -> NodeId {
        self.add(Node {
            items: items.into_iter().map(Item::Node).collect(),
            ..Node::default()
        })
    }

    pub fn tag(&self, id: NodeId) -> Option<Tag> {
        self.nodes[id].tag
    }

    pub fn is(&self, id: NodeId, tag: Tag) -> bool {
        self.nodes[id].tag == Some(tag)
    }

    pub fn range(&self, id: NodeId) -> Range {
        self.nodes[id].range.expect("node has a range")
    }

    /// `#node`.
    pub fn len(&self, id: NodeId) -> usize {
        self.nodes[id].items.len()
    }

    /// `node[i]` (1-based) as a node, `None` when absent or a string.
    pub fn get(&self, id: NodeId, i: usize) -> Option<NodeId> {
        match self.nodes[id].items.get(i.checked_sub(1)?) {
            Some(Item::Node(n)) => Some(*n),
            _ => None,
        }
    }

    /// `node[i]` (1-based), which must be a node.
    pub fn kid(&self, id: NodeId, i: usize) -> NodeId {
        self.get(id, i).expect("child node")
    }

    /// `node[i]` (1-based) as a string.
    pub fn str(&self, id: NodeId, i: usize) -> Option<&[u8]> {
        match self.nodes[id].items.get(i.checked_sub(1)?) {
            Some(Item::Str(s)) => Some(s),
            _ => None,
        }
    }

    /// The node children of a node, in order (strings skipped).
    pub fn kids(&self, id: NodeId) -> Vec<NodeId> {
        self.nodes[id]
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Node(n) => Some(*n),
                Item::Str(_) => None,
            })
            .collect()
    }
}

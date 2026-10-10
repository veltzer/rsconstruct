//! The control flow graph (`ShellCheck.CFG`).
//!
//! `ShellCheck` builds its graph with fgl, and two fgl behaviours shape the
//! results, so they are reproduced here: `ufold` hands out each node's
//! context with the edges to already-decomposed (lower-numbered) nodes
//! removed, and `&` re-inserts a node with exactly the adjacency it is
//! given. `inlineSubshells` combines the two, which drops the edges into a
//! subshell node from lower-numbered nodes; the post-dominators are computed
//! on that graph.

use std::collections::{BTreeMap, BTreeSet};

use super::ast::{AssignmentMode, CaseType, ConditionType, Id, Inner, IntMap, Token};
use super::astlib::{
    PseudoGlob, get_braced_modifier, get_braced_reference, get_bsd_opts, get_generic_opts,
    get_gnu_opts, get_index_references, get_literal_string, get_literal_string_def,
    get_offset_references, get_unquoted_literal, is_closing_file_op,
    is_unmodified_parameter_expansion, is_variable_name, oversimplify, pseudo_glob_is_super_set_of,
    will_split, word_to_exact_pseudo_glob,
};
use super::data::{FLAGS_FOR_MAPFILE, FLAGS_FOR_READ};
use super::regex::{Regex, mk_regex};

use Inner::{
    T_AndIf, T_Annotation, T_Arithmetic, T_Array, T_Assignment, T_Backgrounded, T_Backticked,
    T_Banged, T_BatsTest, T_BraceExpansion, T_BraceGroup, T_CLOBBER, T_CaseExpression, T_CoProc,
    T_CoProcBody, T_Condition, T_DGREAT, T_DollarArithmetic, T_DollarBraceCommandExpansion,
    T_DollarBraced, T_DollarBracket, T_DollarDoubleQuoted, T_DollarExpansion, T_DollarSingleQuoted,
    T_DoubleQuoted, T_Extglob, T_FdRedirect, T_ForArithmetic, T_ForIn, T_Function, T_GREATAND,
    T_Glob, T_Greater, T_HereDoc, T_HereString, T_IfExpression, T_Include, T_IndexedElement,
    T_IoDuplicate, T_IoFile, T_LESSAND, T_LESSGREAT, T_Less, T_Literal, T_NormalWord, T_OrIf,
    T_ParamSubSpecialChar, T_Pipeline, T_ProcSub, T_Redirecting, T_Script, T_SelectIn,
    T_SimpleCommand, T_SingleQuoted, T_SourceCommand, T_Subshell, T_UntilExpression,
    T_WhileExpression, TA_Assignment, TA_Binary, TA_Expansion, TA_Parenthesis, TA_Sequence,
    TA_Trinary, TA_Unary, TA_Variable, TC_And, TC_Binary, TC_Empty, TC_Group, TC_Nullary, TC_Or,
    TC_Unary,
};

pub type Node = usize;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(
    clippy::enum_variant_names,
    reason = "the variant names are ShellCheck's CFNode constructors"
)]
pub enum CfNode {
    CFStructuralNode,
    CFEntryPoint(String),
    CFDropPrefixAssignments,
    CFApplyEffects(Vec<IdTagged>),
    CFExecuteCommand(Option<String>),
    CFExecuteSubshell(String, Node, Node),
    CFSetExitCode(Id),
    CFImpliedExit,
    CFResolvedExit,
    CFUnresolvedExit,
    CFUnreachable,
    CFSetBackgroundPid(Id),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(
    clippy::enum_variant_names,
    reason = "the variant names are ShellCheck's CFEdge constructors"
)]
pub enum CfEdge {
    CFEFlow,
    CFEFalseFlow,
    CFEExit,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(
    clippy::enum_variant_names,
    reason = "the variant names are ShellCheck's CFEffect constructors"
)]
pub enum CfEffect {
    CFSetProps(Option<Scope>, String, PropSet),
    CFUnsetProps(Option<Scope>, String, PropSet),
    CFReadVariable(String),
    CFWriteVariable(String, CfValue),
    CFWriteGlobal(String, CfValue),
    CFWriteLocal(String, CfValue),
    CFWritePrefix(String, CfValue),
    CFDefineFunction(String, Id, Node, Node),
    CFUndefine(String),
    CFUndefineVariable(String),
    CFUndefineFunction(String),
    CFUndefineNameref(String),
    CFHintArray(String),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct IdTagged(pub Id, pub CfEffect);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(
    clippy::enum_variant_names,
    reason = "the variant names are ShellCheck's CFValue constructors"
)]
pub enum CfValue {
    CFValueArray,
    CFValueString,
    CFValueInteger,
    CFValueComputed(Id, Vec<CfStringPart>),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(
    clippy::enum_variant_names,
    reason = "the variant names are ShellCheck's CFStringPart constructors"
)]
pub enum CfStringPart {
    CFStringLiteral(String),
    CFStringVariable(String),
    CFStringInteger,
    CFStringUnknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(
    clippy::enum_variant_names,
    reason = "the variant names are ShellCheck's CFVariableProp constructors"
)]
pub enum CfVariableProp {
    CFVPExport,
    CFVPArray,
    CFVPAssociative,
    CFVPInteger,
}

impl CfVariableProp {
    const ALL: [Self; 4] = [
        Self::CFVPExport,
        Self::CFVPArray,
        Self::CFVPAssociative,
        Self::CFVPInteger,
    ];

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// `Set CFVariableProp`, one bit per property. Ordered as the `Data.Set`
/// is, by its elements in order. The dataflow states hold sets of these
/// for every variable, and a `BTreeSet` stores even one property in a
/// node allocated for eleven.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PropSet(u8);

impl PropSet {
    /// The number of distinct sets.
    pub const COUNT: usize = 1 << CfVariableProp::ALL.len();

    pub const fn bits(self) -> u8 {
        self.0
    }

    /// The set whose `bits` are `bits`.
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn insert(&mut self, p: CfVariableProp) {
        self.0 |= p.bit();
    }

    pub const fn contains(self, p: CfVariableProp) -> bool {
        self.0 & p.bit() != 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// The properties in order.
    pub fn iter(self) -> impl Iterator<Item = CfVariableProp> {
        CfVariableProp::ALL
            .into_iter()
            .filter(move |p| self.contains(*p))
    }
}

impl Ord for PropSet {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.iter().cmp(other.iter())
    }
}

impl PartialOrd for PropSet {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(
    clippy::enum_variant_names,
    reason = "the variant names are ShellCheck's Scope constructors"
)]
pub enum Scope {
    GlobalScope,
    LocalScope,
    PrefixScope,
}

/// `CFGParameters`.
#[derive(Clone, Copy, Debug)]
pub struct CfgParameters {
    pub cf_lastpipe: bool,
}

/// A graph: labelled nodes and labelled edges (fgl's `Gr`). Adjacency is
/// kept per node, ordered by neighbour, as fgl's `PatriciaTree` keeps it.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: BTreeMap<Node, CfNode>,
    /// Successors: node -> neighbour -> edge labels (in insertion order).
    pub succ: BTreeMap<Node, BTreeMap<Node, Vec<CfEdge>>>,
    /// Predecessors: node -> neighbour -> edge labels.
    pub pred: BTreeMap<Node, BTreeMap<Node, Vec<CfEdge>>>,
}

pub type Adj = Vec<(CfEdge, Node)>;

/// An fgl context: (incoming, node, label, outgoing).
pub type Context = (Adj, Node, CfNode, Adj);

impl Graph {
    /// `mkGraph`.
    pub fn mk_graph(nodes: &[(Node, CfNode)], edges: &[(Node, Node, CfEdge)]) -> Self {
        let mut g = Self::default();
        for (n, l) in nodes {
            g.nodes.insert(*n, l.clone());
            g.succ.entry(*n).or_default();
            g.pred.entry(*n).or_default();
        }
        for (from, to, l) in edges {
            g.insert_edge(*from, *to, *l);
        }
        g
    }

    fn insert_edge(&mut self, from: Node, to: Node, l: CfEdge) {
        assert!(
            self.nodes.contains_key(&from) && self.nodes.contains_key(&to),
            "ShellCheck CFG: edge between missing nodes"
        );
        self.succ
            .entry(from)
            .or_default()
            .entry(to)
            .or_default()
            .push(l);
        self.pred
            .entry(to)
            .or_default()
            .entry(from)
            .or_default()
            .push(l);
    }

    fn adj(map: &BTreeMap<Node, Vec<CfEdge>>) -> Adj {
        let mut out = Vec::new();
        for (n, labels) in map {
            for l in labels {
                out.push((*l, *n));
            }
        }
        out
    }

    /// `context`: the full adjacency of a node. As fgl's `match` builds it,
    /// a self-loop is listed among the successors only.
    pub fn context(&self, n: Node) -> Context {
        let label = self
            .nodes
            .get(&n)
            .cloned()
            .expect("ShellCheck CFG: context of a missing node");
        let incoming: Adj = self
            .pred
            .get(&n)
            .map(Self::adj)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, p)| *p != n)
            .collect();
        let outgoing = self.succ.get(&n).map(Self::adj).unwrap_or_default();
        (incoming, n, label, outgoing)
    }

    /// `delNode`.
    pub fn del_node(&mut self, n: Node) {
        if let Some(succs) = self.succ.remove(&n) {
            for s in succs.keys() {
                if let Some(p) = self.pred.get_mut(s) {
                    p.remove(&n);
                }
            }
        }
        if let Some(preds) = self.pred.remove(&n) {
            for p in preds.keys() {
                if let Some(s) = self.succ.get_mut(p) {
                    s.remove(&n);
                }
            }
        }
        self.nodes.remove(&n);
    }

    /// `&`: insert a node with exactly this adjacency.
    pub fn merge_context(&mut self, ctx: Context) {
        let (incoming, n, label, outgoing) = ctx;
        self.nodes.insert(n, label);
        self.succ.entry(n).or_default();
        self.pred.entry(n).or_default();
        for (l, p) in incoming {
            self.insert_edge(p, n, l);
        }
        for (l, s) in outgoing {
            self.insert_edge(n, s, l);
        }
    }

    /// `safeUpdate`: `ctx & delNode node graph`.
    pub fn safe_update(&mut self, ctx: Context) {
        self.del_node(ctx.1);
        self.merge_context(ctx);
    }

    /// `ufold`: contexts as fgl decomposes the graph, lowest node first,
    /// each without the edges to nodes already decomposed. Returned in
    /// decomposition order.
    pub fn decomposition_contexts(&self) -> Vec<Context> {
        let mut out = Vec::new();
        for (&n, label) in &self.nodes {
            let incoming: Adj = self
                .pred
                .get(&n)
                .map(|m| {
                    let mut v = Vec::new();
                    for (p, labels) in m {
                        if *p > n {
                            for l in labels {
                                v.push((*l, *p));
                            }
                        }
                    }
                    v
                })
                .unwrap_or_default();
            let outgoing: Adj = self
                .succ
                .get(&n)
                .map(|m| {
                    let mut v = Vec::new();
                    for (s, labels) in m {
                        if *s >= n {
                            for l in labels {
                                v.push((*l, *s));
                            }
                        }
                    }
                    v
                })
                .unwrap_or_default();
            out.push((incoming, n, label.clone(), outgoing));
        }
        out
    }

    /// `grev`.
    pub fn reversed(&self) -> Self {
        Self {
            nodes: self.nodes.clone(),
            succ: self.pred.clone(),
            pred: self.succ.clone(),
        }
    }

    /// `nodeRange`.
    pub fn node_range(&self) -> (Node, Node) {
        let min = *self.nodes.keys().next().expect("nodeRange of empty graph");
        let max = *self
            .nodes
            .keys()
            .next_back()
            .expect("nodeRange of empty graph");
        (min, max)
    }

    pub fn successors(&self, n: Node) -> Vec<Node> {
        self.succ
            .get(&n)
            .map(|m| Self::adj(m).into_iter().map(|(_, s)| s).collect())
            .unwrap_or_default()
    }

    /// `dom`: the dominators of every node reachable from `root`.
    pub fn dom(&self, root: Node) -> BTreeMap<Node, BTreeSet<Node>> {
        // Reachable nodes, in reverse postorder.
        let mut visited: BTreeSet<Node> = BTreeSet::new();
        let mut order: Vec<Node> = Vec::new();
        let mut stack: Vec<(Node, Vec<Node>, usize)> = vec![(root, self.successors(root), 0)];
        visited.insert(root);
        while let Some(top) = stack.last_mut() {
            if top.2 < top.1.len() {
                let next = top.1[top.2];
                top.2 += 1;
                if visited.insert(next) {
                    let succs = self.successors(next);
                    stack.push((next, succs, 0));
                }
            } else {
                order.push(top.0);
                stack.pop();
            }
        }
        order.reverse();
        let index: IntMap<Node, usize> = order.iter().enumerate().map(|(i, n)| (*n, i)).collect();
        // Cooper, Harvey and Kennedy's iterative algorithm.
        let mut idom: Vec<Option<usize>> = vec![None; order.len()];
        idom[0] = Some(0);
        let preds: Vec<Vec<usize>> = order
            .iter()
            .map(|n| {
                self.pred
                    .get(n)
                    .map(|m| m.keys().filter_map(|p| index.get(p).copied()).collect())
                    .unwrap_or_default()
            })
            .collect();
        let intersect = |idom: &[Option<usize>], mut a: usize, mut b: usize| -> usize {
            while a != b {
                while a > b {
                    a = idom[a].expect("processed");
                }
                while b > a {
                    b = idom[b].expect("processed");
                }
            }
            a
        };
        let mut changed = true;
        while changed {
            changed = false;
            for i in 1..order.len() {
                let mut new_idom: Option<usize> = None;
                for &p in &preds[i] {
                    if idom[p].is_some() {
                        new_idom = Some(match new_idom {
                            None => p,
                            Some(cur) => intersect(&idom, p, cur),
                        });
                    }
                }
                if new_idom.is_some() && idom[i] != new_idom {
                    idom[i] = new_idom;
                    changed = true;
                }
            }
        }
        let mut out = BTreeMap::new();
        for (i, n) in order.iter().enumerate() {
            let mut set = BTreeSet::new();
            let mut cur = i;
            set.insert(order[cur]);
            while cur != 0 {
                cur = idom[cur].expect("reachable nodes have dominators");
                set.insert(order[cur]);
            }
            out.insert(*n, set);
        }
        out
    }
}

/// `CFGResult`.
pub struct CfgResult {
    pub graph: Graph,
    pub id_to_range: IntMap<Id, (Node, Node)>,
    pub id_to_nodes: IntMap<Id, BTreeSet<Node>>,
    /// Indexed by node: the nodes that post-dominate it.
    pub post_dominators: Vec<BTreeSet<Node>>,
}

type Cfw = (
    Vec<(Node, CfNode)>,
    Vec<(Node, Node, CfEdge)>,
    Vec<(Id, (Node, Node))>,
    Vec<(Id, Node)>,
);

/// `buildGraph`.
pub fn build_graph(params: CfgParameters, root: &Token) -> CfgResult {
    let mut b = Builder {
        counter: 0,
        nodes: Vec::new(),
        edges: Vec::new(),
        mapping: Vec::new(),
        association: Vec::new(),
    };
    let ctx = CfContext {
        is_condition: false,
        is_function: false,
        token_stack: Vec::new(),
        exit_target: None,
        return_target: None,
        params,
    };
    b.build_root(&ctx, root);
    let (nodes, edges, mapping, association) =
        remove_unnecessary_structural_nodes((b.nodes, b.edges, b.mapping, b.association));
    let mut id_to_range: IntMap<Id, (Node, Node)> = IntMap::default();
    for (id, r) in &mapping {
        id_to_range.insert(*id, *r);
    }
    let mut id_to_nodes: IntMap<Id, BTreeSet<Node>> = IntMap::default();
    for (id, n) in &association {
        id_to_nodes.entry(*id).or_default().insert(*n);
    }
    let only_real_edges: Vec<(Node, Node, CfEdge)> = edges
        .iter()
        .filter(|(_, _, e)| matches!(e, CfEdge::CFEFlow | CfEdge::CFEExit))
        .copied()
        .collect();
    let (_, main_exit) = *id_to_range
        .get(&root.id)
        .expect("ShellCheck CFG: root has a range");
    let graph = Graph::mk_graph(&nodes, &edges);
    let post_dominators =
        find_post_dominators(main_exit, &Graph::mk_graph(&nodes, &only_real_edges));
    CfgResult {
        graph,
        id_to_range,
        id_to_nodes,
        post_dominators,
    }
}

fn remap_helper(map: &BTreeMap<Node, Node>, n: Node) -> Node {
    map.get(&n).copied().unwrap_or(n)
}

fn remap_graph(remap: &BTreeMap<Node, Node>, g: Cfw) -> Cfw {
    let (nodes, edges, mapping, assoc) = g;
    (
        nodes
            .into_iter()
            .map(|(n, label)| {
                let new_label = match label {
                    CfNode::CFApplyEffects(effects) => CfNode::CFApplyEffects(
                        effects
                            .into_iter()
                            .map(|IdTagged(id, e)| match e {
                                CfEffect::CFDefineFunction(name, fid, start, end) => IdTagged(
                                    id,
                                    CfEffect::CFDefineFunction(
                                        name,
                                        fid,
                                        remap_helper(remap, start),
                                        remap_helper(remap, end),
                                    ),
                                ),
                                other => IdTagged(id, other),
                            })
                            .collect(),
                    ),
                    CfNode::CFExecuteSubshell(s, a, b) => {
                        CfNode::CFExecuteSubshell(s, remap_helper(remap, a), remap_helper(remap, b))
                    }
                    other => other,
                };
                (remap_helper(remap, n), new_label)
            })
            .collect(),
        edges
            .into_iter()
            .map(|(f, t, l)| (remap_helper(remap, f), remap_helper(remap, t), l))
            .collect(),
        mapping
            .into_iter()
            .map(|(id, (a, b))| (id, (remap_helper(remap, a), remap_helper(remap, b))))
            .collect(),
        assoc
            .into_iter()
            .map(|(id, n)| (id, remap_helper(remap, n)))
            .collect(),
    )
}

/// `removeUnnecessaryStructuralNodes`: collapse chains of structural nodes.
fn remove_unnecessary_structural_nodes(g: Cfw) -> Cfw {
    let (nodes, edges, mapping, association) = g;
    let regular_edges: Vec<&(Node, Node, CfEdge)> = edges
        .iter()
        .filter(|(_, _, l)| *l == CfEdge::CFEFlow)
        .collect();
    let mut in_degree: IntMap<Node, usize> = IntMap::default();
    let mut out_degree: IntMap<Node, usize> = IntMap::default();
    for (from, to, _) in &regular_edges {
        *in_degree.entry(*from).or_default() += 1;
        *out_degree.entry(*to).or_default() += 1;
    }
    let is_linear = |n: Node| {
        in_degree.get(&n).copied().unwrap_or(0) == 1
            && out_degree.get(&n).copied().unwrap_or(0) == 1
    };
    let candidate_nodes: BTreeSet<Node> = nodes
        .iter()
        .filter(|(n, l)| *l == CfNode::CFStructuralNode && is_linear(*n))
        .map(|(n, _)| *n)
        .collect();
    let edges_to_collapse: BTreeSet<(Node, Node, CfEdge)> = regular_edges
        .iter()
        .filter(|(a, b, _)| candidate_nodes.contains(a) && candidate_nodes.contains(b))
        .map(|e| **e)
        .collect();
    let mut remapping: BTreeMap<Node, Node> = BTreeMap::new();
    for (a, b, _) in &edges_to_collapse {
        let (k, v) = if a < b { (*b, *a) } else { (*a, *b) };
        remapping.insert(k, v);
    }
    let recursive_lookup = |mut n: Node| -> Node {
        while let Some(x) = remapping.get(&n) {
            n = *x;
        }
        n
    };
    let recursive_remapping: BTreeMap<Node, Node> = remapping
        .keys()
        .map(|k| (*k, recursive_lookup(*k)))
        .collect();
    remap_graph(
        &recursive_remapping,
        (
            nodes
                .into_iter()
                .filter(|(n, _)| !recursive_remapping.contains_key(n))
                .collect(),
            edges
                .into_iter()
                .filter(|e| !edges_to_collapse.contains(e))
                .collect(),
            mapping,
            association,
        ),
    )
}

/// `inlineSubshells`.
fn inline_subshells(graph: &Graph) -> Graph {
    let subshells: Vec<(Node, CfNode, Node, Node, Adj, Adj)> = graph
        .decomposition_contexts()
        .into_iter()
        .filter_map(|(incoming, node, label, outgoing)| match &label {
            CfNode::CFExecuteSubshell(_, start, end) => {
                Some((node, label.clone(), *start, *end, incoming, outgoing))
            }
            _ => None,
        })
        .collect();
    let mut g = graph.clone();
    for (node, label, start, end, incoming, outgoing) in subshells {
        let (end_incoming, end_node, end_label, _) = g.context(end);
        g.safe_update((end_incoming, end_node, end_label, outgoing));
        g.safe_update((incoming, node, label, vec![(CfEdge::CFEFlow, start)]));
    }
    g
}

/// `findTerminalNodes`.
fn find_terminal_nodes(graph: &Graph) -> Vec<Node> {
    let mut out = Vec::new();
    for (n, label) in &graph.nodes {
        match label {
            CfNode::CFUnresolvedExit => out.push(*n),
            CfNode::CFApplyEffects(effects) => {
                for IdTagged(_, e) in effects {
                    if let CfEffect::CFDefineFunction(_, _, _, end) = e {
                        out.push(*end);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// `findPostDominators`.
fn find_post_dominators(main_exit: Node, graph: &Graph) -> Vec<BTreeSet<Node>> {
    let mut inlined = inline_subshells(graph);
    let terminals = find_terminal_nodes(&inlined);
    let (mut incoming, _, label, outgoing) = graph.context(main_exit);
    incoming.extend(terminals.into_iter().map(|c| (CfEdge::CFEFlow, c)));
    inlined.safe_update((incoming, main_exit, label, outgoing));
    let reversed = inlined.reversed();
    let post_doms = reversed.dom(main_exit);
    let (_, max_node) = graph.node_range();
    let mut out = vec![BTreeSet::new(); max_node + 1];
    for (n, set) in post_doms {
        out[n] = set;
    }
    out
}

#[derive(Clone, Copy)]
struct Range(Node, Node);

#[derive(Clone)]
struct CfContext {
    is_condition: bool,
    is_function: bool,
    token_stack: Vec<Id>,
    exit_target: Option<Node>,
    return_target: Option<Node>,
    params: CfgParameters,
}

struct Builder {
    counter: Node,
    nodes: Vec<(Node, CfNode)>,
    edges: Vec<(Node, Node, CfEdge)>,
    mapping: Vec<(Id, (Node, Node))>,
    association: Vec<(Id, Node)>,
}

thread_local! {
    static VARIABLE_ASSIGN_RE: Regex = mk_regex("^([_a-zA-Z][_a-zA-Z0-9]*)=");
}

fn apply_single(e: IdTagged) -> CfNode {
    CfNode::CFApplyEffects(vec![e])
}

impl Builder {
    fn new_node(&mut self, ctx: &CfContext, label: CfNode) -> Node {
        let n = self.counter;
        self.counter += 1;
        self.nodes.push((n, label));
        for c in ctx.token_stack.iter().rev() {
            self.association.push((*c, n));
        }
        n
    }

    fn new_node_range(&mut self, ctx: &CfContext, label: CfNode) -> Range {
        let n = self.new_node(ctx, label);
        Range(n, n)
    }

    fn none(&mut self, ctx: &CfContext) -> Range {
        self.new_node_range(ctx, CfNode::CFStructuralNode)
    }

    fn link(&mut self, from: Node, to: Node, label: CfEdge) {
        self.edges.push((from, to, label));
    }

    fn register_node(&mut self, id: Id, r: Range) {
        self.mapping.push((id, (r.0, r.1)));
    }

    fn link_range(&mut self, a: Range, b: Range) -> Range {
        self.link_range_as(CfEdge::CFEFlow, a, b)
    }

    fn link_range_as(&mut self, label: CfEdge, a: Range, b: Range) -> Range {
        self.link(a.1, b.0, label);
        Range(a.0, b.1)
    }

    fn link_ranges(&mut self, list: &[Range]) -> Range {
        let (first, rest) = list.split_first().expect("Empty range");
        let mut acc = *first;
        for r in rest {
            acc = self.link_range(acc, *r);
        }
        acc
    }

    fn sequentially(&mut self, ctx: &CfContext, list: &[Token]) -> Range {
        let first = self.none(ctx);
        let mut ranges = vec![first];
        for t in list {
            ranges.push(self.build(ctx, t));
        }
        self.link_ranges(&ranges)
    }

    fn sequentially_refs(&mut self, ctx: &CfContext, list: &[&Token]) -> Range {
        let first = self.none(ctx);
        let mut ranges = vec![first];
        for t in list {
            ranges.push(self.build(ctx, t));
        }
        self.link_ranges(&ranges)
    }

    fn under(ctx: &CfContext, id: Id) -> CfContext {
        let mut c = ctx.clone();
        c.token_stack.push(id);
        c
    }

    fn subshell(
        &mut self,
        ctx: &CfContext,
        id: Id,
        reason: &str,
        p: &mut dyn FnMut(&mut Self, &CfContext) -> Range,
    ) -> Range {
        let start = self.new_node(
            ctx,
            CfNode::CFEntryPoint(format!("Subshell Id {}: {reason}", id.0)),
        );
        let end = self.new_node(ctx, CfNode::CFStructuralNode);
        let mut inner = ctx.clone();
        inner.exit_target = Some(end);
        inner.return_target = Some(end);
        let middle = p(self, &inner);
        self.link_ranges(&[Range(start, start), middle, Range(end, end)]);
        self.new_node_range(
            ctx,
            CfNode::CFExecuteSubshell(reason.to_string(), start, end),
        )
    }

    fn with_function_scope(
        &mut self,
        ctx: &CfContext,
        p: &mut dyn FnMut(&mut Self, &CfContext) -> Range,
    ) -> Range {
        let end = self.new_node(ctx, CfNode::CFStructuralNode);
        let mut inner = ctx.clone();
        inner.return_target = Some(end);
        inner.is_function = true;
        let body = p(self, &inner);
        self.link_ranges(&[body, Range(end, end)])
    }

    fn build_root(&mut self, ctx: &CfContext, t: &Token) -> Range {
        let ctx = Self::under(ctx, t.id);
        let entry = self.new_node_range(&ctx, CfNode::CFEntryPoint("MAIN".to_string()));
        let implied_exit = self.new_node(&ctx, CfNode::CFImpliedExit);
        let end = self.new_node(&ctx, CfNode::CFStructuralNode);
        let mut inner = ctx.clone();
        inner.exit_target = Some(end);
        inner.return_target = Some(implied_exit);
        let start = self.build(&inner, t);
        let range = self.link_ranges(&[
            entry,
            start,
            Range(implied_exit, implied_exit),
            Range(end, end),
        ]);
        self.register_node(t.id, range);
        range
    }

    fn build(&mut self, ctx: &CfContext, t: &Token) -> Range {
        let inner = Self::under(ctx, t.id);
        let range = self.build_inner(&inner, t);
        self.register_node(t.id, range);
        range
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one arm per token type, as in ShellCheck's build'"
    )]
    fn build_inner(&mut self, ctx: &CfContext, t: &Token) -> Range {
        let id = t.id;
        match &t.inner {
            T_Annotation(_, list) => self.build(ctx, list),
            T_Script(_, list) => self.sequentially(ctx, list),
            TA_Assignment(op, var, rhs) if matches!(var.inner, TA_Variable(..)) => {
                let TA_Variable(name, indices) = &var.inner else {
                    unreachable!("checked above")
                };
                let value = self.build(ctx, rhs);
                let subscript = self.sequentially(ctx, indices);
                let read = if op == "=" {
                    self.none(ctx)
                } else {
                    self.new_node_range(
                        ctx,
                        apply_single(IdTagged(id, CfEffect::CFReadVariable(name.clone()))),
                    )
                };
                let v = if indices.is_empty() {
                    CfValue::CFValueInteger
                } else {
                    CfValue::CFValueArray
                };
                let write = self.new_node_range(
                    ctx,
                    apply_single(IdTagged(id, CfEffect::CFWriteVariable(name.clone(), v))),
                );
                self.link_ranges(&[value, subscript, read, write])
            }
            TA_Assignment(_, lhs, rhs) => self.sequentially_refs(ctx, &[lhs, rhs]),
            TA_Binary(_, a, b) => self.sequentially_refs(ctx, &[a, b]),
            TA_Expansion(list) | TA_Sequence(list) => self.sequentially(ctx, list),
            TA_Parenthesis(t) => self.build(ctx, t),
            TA_Trinary(cond, a, b) => {
                let condition = self.build(ctx, cond);
                let ifthen = self.build(ctx, a);
                let elsethen = self.build(ctx, b);
                let end = self.none(ctx);
                self.link_ranges(&[condition, ifthen, end]);
                self.link_ranges(&[condition, elsethen, end])
            }
            TA_Variable(name, indices) => {
                let subscript = self.sequentially(ctx, indices);
                let hint = if indices.is_empty() {
                    self.none(ctx)
                } else {
                    self.new_node_range(
                        ctx,
                        apply_single(IdTagged(id, CfEffect::CFHintArray(name.clone()))),
                    )
                };
                let read = self.new_node_range(
                    ctx,
                    apply_single(IdTagged(id, CfEffect::CFReadVariable(name.clone()))),
                );
                self.link_ranges(&[subscript, hint, read])
            }
            TA_Unary(op, arg)
                if (op.contains("--") || op.contains("++"))
                    && matches!(arg.inner, TA_Variable(..)) =>
            {
                let TA_Variable(name, indices) = &arg.inner else {
                    unreachable!("checked above")
                };
                let subscript = self.sequentially(ctx, indices);
                let read = self.new_node_range(
                    ctx,
                    apply_single(IdTagged(id, CfEffect::CFReadVariable(name.clone()))),
                );
                let v = if indices.is_empty() {
                    CfValue::CFValueInteger
                } else {
                    CfValue::CFValueArray
                };
                let write = self.new_node_range(
                    ctx,
                    apply_single(IdTagged(id, CfEffect::CFWriteVariable(name.clone(), v))),
                );
                self.link_ranges(&[subscript, read, write])
            }
            TA_Unary(_, arg) => self.build(ctx, arg),
            TC_And(ConditionType::SingleBracket, _, lhs, rhs)
            | TC_Or(ConditionType::SingleBracket, _, lhs, rhs) => {
                self.sequentially_refs(ctx, &[lhs, rhs])
            }
            TC_And(ConditionType::DoubleBracket, _, lhs, rhs)
            | TC_Or(ConditionType::DoubleBracket, _, lhs, rhs) => {
                let left = self.build(ctx, lhs);
                let right = self.build(ctx, rhs);
                let end = self.none(ctx);
                self.link_ranges(&[left, right, end]);
                self.link_range(left, end)
            }
            TC_Binary(_, _, lhs, rhs) => {
                let left = self.build(ctx, lhs);
                let right = self.build(ctx, rhs);
                self.link_range(left, right)
            }
            TC_Empty(_) => self.none(ctx),
            TC_Group(_, t) | TC_Nullary(_, t) | TC_Unary(_, _, t) => self.build(ctx, t),
            T_Arithmetic(root) => {
                let exe = self.build(ctx, root);
                let status = self.new_node_range(ctx, CfNode::CFSetExitCode(id));
                self.link_range(exe, status)
            }
            T_AndIf(lhs, rhs) | T_OrIf(lhs, rhs) => {
                let left = self.build(ctx, lhs);
                let right = self.build(ctx, rhs);
                let end = self.none(ctx);
                self.link_range(left, right);
                self.link_range(right, end);
                self.link_range(left, end)
            }
            T_Array(list) | T_BraceExpansion(list) | T_BraceGroup(list) => {
                self.sequentially(ctx, list)
            }
            T_Assignment(..) => self.build_assignment(ctx, None, t),
            T_Backgrounded(body) => {
                let start = self.none(ctx);
                let fork =
                    self.subshell(ctx, id, "backgrounding '&'", &mut |s, c| s.build(c, body));
                let pid = self.new_node_range(ctx, CfNode::CFSetBackgroundPid(id));
                let status = self.new_node_range(ctx, CfNode::CFSetExitCode(id));
                self.link_range(start, fork);
                self.link_range_as(CfEdge::CFEFalseFlow, fork, pid);
                self.link_ranges(&[start, pid, status])
            }
            T_Backticked(body) => self.subshell(ctx, id, "`..` expansion", &mut |s, c| {
                s.sequentially(c, body)
            }),
            T_Banged(cmd) => {
                let main = self.build(ctx, cmd);
                let status = self.new_node_range(ctx, CfNode::CFSetExitCode(id));
                self.link_range(main, status)
            }
            T_BatsTest(_, body) => {
                let status = self.new_node_range(
                    ctx,
                    apply_single(IdTagged(
                        id,
                        CfEffect::CFWriteVariable("status".to_string(), CfValue::CFValueInteger),
                    )),
                );
                let output = self.new_node_range(
                    ctx,
                    apply_single(IdTagged(
                        id,
                        CfEffect::CFWriteVariable("output".to_string(), CfValue::CFValueString),
                    )),
                );
                let main = self.build(ctx, body);
                self.link_ranges(&[status, output, main])
            }
            T_CaseExpression(w, list) if list.is_empty() => self.build(ctx, w),
            T_CaseExpression(w, list) => self.build_case(ctx, w, list),
            T_Condition(_, op) => {
                let cond = self.build(ctx, op);
                let status = self.new_node_range(ctx, CfNode::CFSetExitCode(id));
                self.link_range(cond, status)
            }
            T_CoProc(name_token, body) => {
                let maybe_name = match name_token {
                    Some(x) => get_literal_string(x),
                    None => Some("COPROC".to_string()),
                };
                let parent_node = match maybe_name {
                    Some(s) => apply_single(IdTagged(
                        id,
                        CfEffect::CFWriteVariable(s, CfValue::CFValueArray),
                    )),
                    None => CfNode::CFStructuralNode,
                };
                let start = self.none(ctx);
                let parent = self.new_node_range(ctx, parent_node);
                let child = self.subshell(ctx, id, "coproc", &mut |s, c| s.build(c, body));
                let end = self.new_node_range(ctx, CfNode::CFSetExitCode(id));
                self.link_range(start, parent);
                self.link_range(start, child);
                self.link_range(parent, end);
                self.link_range_as(CfEdge::CFEFalseFlow, child, end);
                Range(start.0, end.1)
            }
            T_CoProcBody(t)
            | T_DollarArithmetic(t)
            | T_DollarBracket(t)
            | T_HereString(t)
            | T_Include(t) => self.build(ctx, t),
            T_DollarDoubleQuoted(list)
            | T_DoubleQuoted(list)
            | T_Extglob(_, list)
            | T_HereDoc(_, _, _, list)
            | T_NormalWord(list) => self.sequentially(ctx, list),
            T_DollarSingleQuoted(_) | T_Glob(_) | T_Literal(_) | T_SingleQuoted(_) => {
                self.none(ctx)
            }
            T_DollarBraced(_, w) => {
                let s: String = oversimplify(w).concat();
                let modifier = get_braced_modifier(&s);
                let reference = get_braced_reference(&s);
                let mut extra = get_index_references(&s);
                extra.extend(get_offset_references(&s));
                let vals = self.build(ctx, w);
                let mut ranges = vec![vals];
                for x in extra {
                    ranges.push(self.new_node_range(
                        ctx,
                        apply_single(IdTagged(id, CfEffect::CFReadVariable(x))),
                    ));
                }
                let deps = self.link_ranges(&ranges);
                let read = self.new_node_range(
                    ctx,
                    apply_single(IdTagged(id, CfEffect::CFReadVariable(reference.clone()))),
                );
                let total_read = self.link_range(deps, read);
                if modifier.starts_with('=') || modifier.starts_with(":=") {
                    let optional_assign = self.new_node_range(
                        ctx,
                        apply_single(IdTagged(
                            id,
                            CfEffect::CFWriteVariable(reference, CfValue::CFValueString),
                        )),
                    );
                    let result = self.none(ctx);
                    self.link_range(optional_assign, result);
                    self.link_range(total_read, result)
                } else {
                    total_read
                }
            }
            T_DollarBraceCommandExpansion(_, body) => self.sequentially(ctx, body),
            T_DollarExpansion(body) => self.subshell(ctx, id, "$(..) expansion", &mut |s, c| {
                s.sequentially(c, body)
            }),
            T_FdRedirect(name, op) if name.starts_with('{') => {
                let var: String = name[1..].chars().take_while(|&c| c != '}').collect();
                let expression = self.build(ctx, op);
                let effect = if is_closing_file_op(op) {
                    CfEffect::CFReadVariable(var)
                } else {
                    CfEffect::CFWriteVariable(var, CfValue::CFValueInteger)
                };
                let rw = self.new_node_range(ctx, apply_single(IdTagged(id, effect)));
                self.link_range(expression, rw)
            }
            T_FdRedirect(_, t) => self.build(ctx, t),
            T_ForArithmetic(init_t, cond_t, inc_t, body_t) => {
                let init = self.build(ctx, init_t);
                let cond = self.build(ctx, cond_t);
                let body = self.sequentially(ctx, body_t);
                let inc = self.build(ctx, inc_t);
                let end = self.none(ctx);
                self.link_ranges(&[init, cond, body, inc]);
                self.link_range(cond, end);
                self.link_range(inc, cond);
                Range(init.0, end.1)
            }
            T_ForIn(name, words, body) | T_SelectIn(name, words, body) => {
                self.for_in_helper(ctx, id, name, words, body)
            }
            T_Function(_, _, name, body) => {
                let mut fctx = ctx.clone();
                fctx.exit_target = None;
                let entry =
                    self.new_node_range(&fctx, CfNode::CFEntryPoint(format!("function {name}")));
                let f = self.with_function_scope(&fctx, &mut |s, c| s.build(c, body));
                let range = self.link_range(entry, f);
                let definition = self.new_node_range(
                    ctx,
                    apply_single(IdTagged(
                        id,
                        CfEffect::CFDefineFunction(name.clone(), id, range.0, range.1),
                    )),
                );
                let exe = self.new_node_range(ctx, CfNode::CFSetExitCode(id));
                self.link_range(definition, exe)
            }
            T_IfExpression(ifs, elses) => {
                let start = self.none(ctx);
                let mut branches: Vec<Range> = Vec::new();
                let mut prev = start;
                for (conds, thens) in ifs {
                    let mut cctx = ctx.clone();
                    cctx.is_condition = true;
                    let cond = self.sequentially(&cctx, conds);
                    let action = self.sequentially(ctx, thens);
                    self.link_range(prev, cond);
                    self.link_range(cond, action);
                    prev = cond;
                    branches.insert(0, action);
                }
                let rest = if elses.is_empty() {
                    self.new_node_range(ctx, CfNode::CFSetExitCode(id))
                } else {
                    self.sequentially(ctx, elses)
                };
                self.link_range(prev, rest);
                branches.insert(0, rest);
                let end = self.none(ctx);
                for br in branches {
                    self.link_range(br, end);
                }
                Range(start.0, end.1)
            }
            T_IndexedElement(indices_t, value_t) => {
                let indices = self.sequentially(ctx, indices_t);
                let value = self.build(ctx, value_t);
                self.link_range(indices, value)
            }
            T_IoDuplicate(op, _) => self.build(ctx, op),
            T_IoFile(op, t) => {
                let exp = self.build(ctx, t);
                let doesnt_do_much = self.build(ctx, op);
                self.link_range(exp, doesnt_do_much)
            }
            T_Pipeline(_, cmds) if cmds.len() == 1 => self.build(ctx, &cmds[0]),
            T_Pipeline(_, cmds) => {
                let start = self.none(ctx);
                let has_lastpipe = ctx.params.cf_lastpipe;
                let (leading, last) = self.build_pipe(ctx, id, has_lastpipe, cmds);
                let end = self.new_node_range(ctx, CfNode::CFSetExitCode(id));
                for l in &leading {
                    self.link_range(start, *l);
                }
                for l in &leading {
                    self.link_range_as(CfEdge::CFEFalseFlow, *l, end);
                }
                let mut ranges = vec![start];
                ranges.extend(last);
                ranges.push(end);
                self.link_ranges(&ranges)
            }
            T_ProcSub(op, cmds) => {
                let start = self.none(ctx);
                let reason = format!("{op}() process substitution");
                let body = self.subshell(ctx, id, &reason, &mut |s, c| s.sequentially(c, cmds));
                let end = self.none(ctx);
                self.link_range(start, body);
                self.link_range_as(CfEdge::CFEFalseFlow, body, end);
                self.link_range(start, end)
            }
            T_Redirecting(redirs, cmd) => {
                let redir = self.sequentially(ctx, redirs);
                let body = self.build(ctx, cmd);
                self.link_range(redir, body)
            }
            T_SimpleCommand(vars, words) if words.is_empty() => {
                let assignments = self.sequentially(ctx, vars);
                let status = self.new_node_range(ctx, CfNode::CFSetExitCode(id));
                self.link_range(assignments, status)
            }
            T_SimpleCommand(vars, words) => {
                let literal = get_unquoted_literal(&words[0]);
                let args: Vec<&Token> = words.iter().collect();
                self.handle_command(ctx, t, vars, &args, literal)
            }
            T_SourceCommand(original, inlined) => {
                let cmd = self.build(ctx, original);
                let end = self.none(ctx);
                let inline = self.build(ctx, inlined);
                self.link_range(cmd, inline);
                self.link_range(inline, end);
                Range(cmd.0, inline.1)
            }
            T_Subshell(body) => {
                let main = self.subshell(ctx, id, "explicit (..) subshell", &mut |s, c| {
                    s.sequentially(c, body)
                });
                let status = self.new_node_range(ctx, CfNode::CFSetExitCode(id));
                self.link_range(main, status)
            }
            T_UntilExpression(cond, body) | T_WhileExpression(cond, body) => {
                let mut cctx = ctx.clone();
                cctx.is_condition = true;
                let cond_range = self.sequentially(&cctx, cond);
                let body_range = self.sequentially(ctx, body);
                let end = self.new_node_range(ctx, CfNode::CFSetExitCode(id));
                self.link_range(cond_range, body_range);
                self.link_range(body_range, cond_range);
                self.link_range(cond_range, end)
            }
            T_CLOBBER
            | T_GREATAND
            | T_LESSAND
            | T_LESSGREAT
            | T_DGREAT
            | T_Greater
            | T_Less
            | T_ParamSubSpecialChar(_) => self.none(ctx),
            _ => panic!("ShellCheck CFG: Unimplemented: {t:?}"),
        }
    }

    fn build_case(
        &mut self,
        ctx: &CfContext,
        w: &Token,
        list: &[(CaseType, Vec<Token>, Vec<Token>)],
    ) -> Range {
        let start = self.none(ctx);
        let token = self.build(ctx, w);
        let mut branches: Vec<(CaseType, Range, Range)> = Vec::new();
        for (typ, cond, body) in list {
            // buildCond
            let cstart = self.none(ctx);
            let mut conds = Vec::new();
            for c in cond {
                conds.push(self.build(ctx, c));
            }
            let cend = self.none(ctx);
            let mut all = vec![cstart];
            all.extend(conds.iter().copied());
            self.link_ranges(&all);
            for c in &conds {
                self.link_range(*c, cend);
            }
            let c = Range(cstart.0, cend.1);
            let b = self.sequentially(ctx, body);
            self.link_range(c, b);
            branches.push((*typ, c, b));
        }
        let end = self.none(ctx);
        let first_cond = branches[0].1;
        let (_, _, last_body) = branches[branches.len() - 1];
        self.link_range(start, token);
        self.link_range(token, first_cond);
        for i in 0..branches.len() - 1 {
            let (typ, cond, body) = branches[i];
            let (_, next_cond, next_body) = branches[i + 1];
            self.link_range(cond, next_cond);
            match typ {
                CaseType::CaseBreak => {
                    self.link_range(body, end);
                }
                CaseType::CaseFallThrough => {
                    self.link_range(body, next_body);
                }
                CaseType::CaseContinue => {
                    self.link_range(body, next_cond);
                }
            }
        }
        self.link_range(last_body, end);
        let is_catch_all = |c: &Token| {
            word_to_exact_pseudo_glob(c)
                .is_some_and(|pg| pseudo_glob_is_super_set_of(&pg, &[PseudoGlob::PGMany]))
        };
        let has_catch_all = list
            .iter()
            .any(|(_, cond, _)| cond.iter().any(is_catch_all));
        if !has_catch_all {
            self.link_range(token, end);
        }
        Range(start.0, end.1)
    }

    fn build_pipe(
        &mut self,
        ctx: &CfContext,
        id: Id,
        lastpipe: bool,
        cmds: &[Token],
    ) -> (Vec<Range>, Vec<Range>) {
        match cmds {
            [x] if lastpipe => (Vec::new(), vec![self.build(ctx, x)]),
            [first, rest @ ..] => {
                let this = self.subshell(ctx, id, "pipeline", &mut |s, c| s.build(c, first));
                let (mut leading, last) = self.build_pipe(ctx, id, lastpipe, rest);
                leading.insert(0, this);
                (leading, last)
            }
            [] => (Vec::new(), Vec::new()),
        }
    }

    fn for_in_helper(
        &mut self,
        ctx: &CfContext,
        id: Id,
        name: &str,
        words: &[Token],
        body: &[Token],
    ) -> Range {
        let entry = self.none(ctx);
        let expansion = self.sequentially(ctx, words);
        let assignment_choice = self.none(ctx);
        let assignments: Vec<Range> = if words.is_empty() || words.iter().any(will_split) {
            vec![self.new_node_range(
                ctx,
                apply_single(IdTagged(
                    id,
                    CfEffect::CFWriteVariable(name.to_string(), CfValue::CFValueString),
                )),
            )]
        } else {
            words
                .iter()
                .map(|t| {
                    self.new_node_range(
                        ctx,
                        apply_single(IdTagged(
                            id,
                            CfEffect::CFWriteVariable(
                                name.to_string(),
                                CfValue::CFValueComputed(t.id, token_to_parts(t)),
                            ),
                        )),
                    )
                })
                .collect()
        };
        let body = self.sequentially(ctx, body);
        let exit = self.none(ctx);
        self.link_ranges(&[entry, expansion, assignment_choice]);
        for a in &assignments {
            self.link_ranges(&[assignment_choice, *a, body]);
        }
        self.link_range(body, exit);
        self.link_range(expansion, exit);
        self.link_range(body, assignment_choice);
        Range(entry.0, exit.1)
    }

    fn handle_command(
        &mut self,
        ctx: &CfContext,
        cmd: &Token,
        vars: &[Token],
        args: &[&Token],
        literal_cmd: Option<String>,
    ) -> Range {
        match literal_cmd.as_deref() {
            Some("exit") => self.regular_expansion(ctx, vars, args, &mut |s, c| s.handle_exit(c)),
            Some("return") => {
                self.regular_expansion(ctx, vars, args, &mut |s, c| s.handle_return(c))
            }
            Some("unset") => self.regular_expansion_with_status(ctx, vars, args, &mut |s, c| {
                s.handle_unset(c, args)
            }),
            Some("declare" | "local" | "typeset") => self.handle_declare(ctx, args),
            Some("printf") => self.regular_expansion_with_status(ctx, vars, args, &mut |s, c| {
                s.handle_printf(c, args)
            }),
            Some("wait") => self
                .regular_expansion_with_status(ctx, vars, args, &mut |s, c| s.handle_wait(c, args)),
            Some("mapfile" | "readarray") => {
                self.regular_expansion_with_status(ctx, vars, args, &mut |s, c| {
                    s.handle_mapfile(c, args)
                })
            }
            Some("read") => self
                .regular_expansion_with_status(ctx, vars, args, &mut |s, c| s.handle_read(c, args)),
            Some("DEFINE_boolean" | "DEFINE_float" | "DEFINE_integer" | "DEFINE_string") => self
                .regular_expansion_with_status(ctx, vars, args, &mut |s, c| {
                    s.handle_define(c, args)
                }),
            Some("builtin") => {
                if args.len() == 1 {
                    self.handle_others(ctx, cmd.id, vars, args, literal_cmd)
                } else {
                    let newcmd = args[1];
                    self.handle_command(ctx, newcmd, vars, &args[1..], get_literal_string(newcmd))
                }
            }
            Some("command") => {
                if args.len() == 1 {
                    self.handle_others(ctx, cmd.id, vars, args, literal_cmd)
                } else {
                    let newcmd = args[1];
                    self.handle_others(ctx, newcmd.id, vars, &args[1..], get_literal_string(newcmd))
                }
            }
            _ => self.handle_others(ctx, cmd.id, vars, args, literal_cmd),
        }
    }

    fn handle_exit(&mut self, ctx: &CfContext) -> Range {
        if let Some(target) = ctx.exit_target {
            let exit = self.new_node(ctx, CfNode::CFResolvedExit);
            self.link(exit, target, CfEdge::CFEExit);
            let unreachable = self.new_node(ctx, CfNode::CFUnreachable);
            Range(exit, unreachable)
        } else {
            let exit = self.new_node(ctx, CfNode::CFUnresolvedExit);
            let unreachable = self.new_node(ctx, CfNode::CFUnreachable);
            Range(exit, unreachable)
        }
    }

    fn handle_return(&mut self, ctx: &CfContext) -> Range {
        let target = ctx
            .return_target
            .expect("ShellCheck internal error, please report: missing return target");
        let ret = self.new_node(ctx, CfNode::CFStructuralNode);
        self.link(ret, target, CfEdge::CFEFlow);
        let unreachable = self.new_node(ctx, CfNode::CFUnreachable);
        Range(ret, unreachable)
    }

    fn handle_unset(&mut self, ctx: &CfContext, args: &[&Token]) -> Range {
        let rest = args_slice(&args[1..]);
        let pairs: Vec<(String, &Token)> = match get_gnu_opts("vfn", &rest) {
            Some(opts) => opts.into_iter().map(|(s, (flag, _))| (s, flag)).collect(),
            None => rest.iter().map(|c| (String::new(), c)).collect(),
        };
        let flag_names: Vec<&String> = pairs
            .iter()
            .filter(|(s, _)| !s.is_empty())
            .map(|(s, _)| s)
            .collect();
        let literal_names = pairs
            .iter()
            .filter(|(s, _)| s.is_empty())
            .filter_map(|(_, t)| get_literal_string(t).map(|n| (*t, n)));
        let has = |f: &str| flag_names.iter().any(|x| *x == f);
        let make = |name: String| -> CfEffect {
            if has("n") {
                CfEffect::CFUndefineNameref(name)
            } else if has("v") {
                CfEffect::CFUndefineVariable(name)
            } else if has("f") {
                CfEffect::CFUndefineFunction(name)
            } else {
                CfEffect::CFUndefine(name)
            }
        };
        let effects = literal_names
            .map(|(t, n)| IdTagged(t.id, make(n)))
            .collect();
        self.new_node_range(ctx, CfNode::CFApplyEffects(effects))
    }

    fn handle_declare(&mut self, ctx: &CfContext, args_with_cmd: &[&Token]) -> Range {
        let cmd = args_with_cmd[0];
        let args = &args_with_cmd[1..];
        let is_func = ctx.is_function;
        let args_owned = args_slice(args);
        let opts: Vec<String> = get_generic_opts(&args_owned)
            .into_iter()
            .map(|(f, _)| f)
            .collect();
        let has = |f: &str| opts.iter().any(|x| x == f);
        let associative = has("A");
        let array = has("a") || associative;
        let integer = has("i");
        let global = has("g");
        let export = has("x");
        let writer = |name: String, v: CfValue| -> CfEffect {
            if global {
                CfEffect::CFWriteGlobal(name, v)
            } else if is_func {
                CfEffect::CFWriteLocal(name, v)
            } else {
                CfEffect::CFWriteVariable(name, v)
            }
        };
        let scope = if global {
            Some(Scope::GlobalScope)
        } else if is_func {
            Some(Scope::LocalScope)
        } else {
            None
        };
        let mut added_props = PropSet::default();
        if array {
            added_props.insert(CfVariableProp::CFVPArray);
        }
        if integer {
            added_props.insert(CfVariableProp::CFVPInteger);
        }
        if export {
            added_props.insert(CfVariableProp::CFVPExport);
        }
        if associative {
            added_props.insert(CfVariableProp::CFVPAssociative);
        }
        let unset_options: String = args
            .iter()
            .filter_map(|t| get_literal_string(t))
            .filter(|s| s.starts_with('+'))
            .map(|s| s[1..].to_string())
            .collect();
        let mut removed_props = PropSet::default();
        if unset_options.contains('i') {
            removed_props.insert(CfVariableProp::CFVPInteger);
        }
        if unset_options.contains('e') {
            removed_props.insert(CfVariableProp::CFVPExport);
        }
        let mut evaluated: Vec<&Token> = Vec::new();
        let mut assignments: Vec<IdTagged> = Vec::new();
        let mut added: Vec<IdTagged> = Vec::new();
        let mut removed: Vec<IdTagged> = Vec::new();
        for t in args {
            if let T_Assignment(mode, var, idx, value) = &t.inner {
                evaluated.extend(idx.iter());
                evaluated.push(value);
                let mut parts = Vec::new();
                if *mode == AssignmentMode::Append {
                    parts.push(CfStringPart::CFStringVariable(var.clone()));
                }
                parts.extend(token_to_parts(value));
                assignments.push(IdTagged(
                    t.id,
                    writer(var.clone(), CfValue::CFValueComputed(value.id, parts)),
                ));
                if !added_props.is_empty() {
                    added.push(IdTagged(
                        t.id,
                        CfEffect::CFSetProps(scope, var.clone(), added_props),
                    ));
                }
                if !removed_props.is_empty() {
                    // ShellCheck unsets the added properties here.
                    removed.push(IdTagged(
                        t.id,
                        CfEffect::CFUnsetProps(scope, var.clone(), added_props),
                    ));
                }
            } else {
                evaluated.push(t);
                let literal = get_literal_string_def("\0", t);
                let is_known = !literal.contains('\0');
                let matched = VARIABLE_ASSIGN_RE
                    .with(|re| re.match_groups(&literal))
                    .map(|g| g[0].clone());
                let name = matched.clone().unwrap_or_else(|| literal.clone());
                if !is_variable_name(&name) {
                    continue;
                }
                let as_literal = IdTagged(
                    t.id,
                    writer(
                        name.clone(),
                        CfValue::CFValueComputed(
                            t.id,
                            vec![CfStringPart::CFStringLiteral(match literal.find('=') {
                                Some(i) => literal[i + 1..].to_string(),
                                None => String::new(),
                            })],
                        ),
                    ),
                );
                let as_unknown = IdTagged(t.id, writer(name.clone(), CfValue::CFValueString));
                if matched.is_some() && is_known {
                    assignments.push(as_literal);
                } else if matched.is_some() {
                    assignments.push(as_unknown);
                }
                added.push(IdTagged(
                    t.id,
                    CfEffect::CFSetProps(scope, name.clone(), added_props),
                ));
                removed.push(IdTagged(
                    t.id,
                    CfEffect::CFUnsetProps(scope, name, removed_props),
                ));
            }
        }
        let before = self.sequentially_refs(ctx, &evaluated);
        let assignments_r = self.new_node_range(ctx, CfNode::CFApplyEffects(assignments));
        let added_r = if added.is_empty() {
            self.none(ctx)
        } else {
            self.new_node_range(ctx, CfNode::CFApplyEffects(added))
        };
        let removed_r = if removed.is_empty() {
            self.none(ctx)
        } else {
            self.new_node_range(ctx, CfNode::CFApplyEffects(removed))
        };
        let result = self.new_node_range(ctx, CfNode::CFSetExitCode(cmd.id));
        self.link_ranges(&[before, assignments_r, added_r, removed_r, result])
    }

    fn handle_printf(&mut self, ctx: &CfContext, args_with_cmd: &[&Token]) -> Range {
        let args = args_slice(&args_with_cmd[1..]);
        let effect = (|| {
            let flags = get_bsd_opts("v:", &args)?;
            let (_, (_, arg)) = flags.iter().find(|(f, _)| f == "v")?;
            let name = get_literal_string(arg)?;
            Some(IdTagged(
                arg.id,
                CfEffect::CFWriteVariable(name, CfValue::CFValueString),
            ))
        })();
        self.new_node_range(ctx, CfNode::CFApplyEffects(effect.into_iter().collect()))
    }

    fn handle_wait(&mut self, ctx: &CfContext, args_with_cmd: &[&Token]) -> Range {
        let args = args_slice(&args_with_cmd[1..]);
        let effect = (|| {
            let flags = get_generic_opts(&args);
            let (_, (_, arg)) = flags.iter().find(|(f, _)| f == "p")?;
            let name = get_literal_string(arg)?;
            Some(IdTagged(
                arg.id,
                CfEffect::CFWriteVariable(name, CfValue::CFValueInteger),
            ))
        })();
        self.new_node_range(ctx, CfNode::CFApplyEffects(effect.into_iter().collect()))
    }

    fn handle_mapfile(&mut self, ctx: &CfContext, args_with_cmd: &[&Token]) -> Range {
        let cmd = args_with_cmd[0];
        let args = args_slice(&args_with_cmd[1..]);
        let from_arg = (|| {
            let flags = get_gnu_opts(FLAGS_FOR_MAPFILE, &args)?;
            let (_, (_, arg)) = flags.iter().find(|(f, _)| f.is_empty())?;
            let name = get_literal_string(arg)?;
            Some((arg.id, name))
        })();
        let from_fallback = || {
            args.iter().rev().find_map(|c| {
                let name = get_literal_string(c)?;
                if is_variable_name(&name) {
                    Some((c.id, name))
                } else {
                    None
                }
            })
        };
        let (id, name) = from_arg
            .or_else(from_fallback)
            .unwrap_or_else(|| (cmd.id, "MAPFILE".to_string()));
        self.new_node_range(
            ctx,
            CfNode::CFApplyEffects(vec![IdTagged(
                id,
                CfEffect::CFWriteVariable(name, CfValue::CFValueArray),
            )]),
        )
    }

    fn handle_read(&mut self, ctx: &CfContext, args_with_cmd: &[&Token]) -> Range {
        let cmd = args_with_cmd[0];
        let args = args_slice(&args_with_cmd[1..]);
        let fallback = || -> Vec<IdTagged> {
            let mut names: Vec<(Id, String)> = Vec::new();
            for c in args.iter().rev() {
                match get_literal_string(c) {
                    Some(n) => names.push((c.id, n)),
                    None => break,
                }
            }
            names.reverse();
            if names.is_empty() {
                names.push((cmd.id, "REPLY".to_string()));
            }
            let has_dash_a = get_generic_opts(&args).iter().any(|(f, _)| f == "a");
            let value = if has_dash_a {
                CfValue::CFValueArray
            } else {
                CfValue::CFValueString
            };
            names
                .into_iter()
                .map(|(id, name)| IdTagged(id, CfEffect::CFWriteVariable(name, value.clone())))
                .collect()
        };
        let main = match get_gnu_opts(FLAGS_FOR_READ, &args) {
            None => fallback(),
            Some(flags) => match flags.iter().find(|(f, _)| f == "a") {
                Some((_, (_, token))) => match get_literal_string(token) {
                    Some(name) => vec![IdTagged(
                        token.id,
                        CfEffect::CFWriteVariable(name, CfValue::CFValueArray),
                    )],
                    None => Vec::new(),
                },
                None => flags
                    .iter()
                    .filter(|(f, _)| f.is_empty())
                    .filter_map(|(_, (t, _))| {
                        let name = get_literal_string(t)?;
                        Some(IdTagged(
                            t.id,
                            CfEffect::CFWriteVariable(name, CfValue::CFValueString),
                        ))
                    })
                    .collect(),
            },
        };
        self.new_node_range(ctx, CfNode::CFApplyEffects(main))
    }

    fn handle_define(&mut self, ctx: &CfContext, args_with_cmd: &[&Token]) -> Range {
        let args = &args_with_cmd[1..];
        let effect = (|| {
            let name = args.get(1)?;
            let s = get_literal_string(name)?;
            if !is_variable_name(&s) {
                return None;
            }
            Some(IdTagged(
                name.id,
                CfEffect::CFWriteVariable(s, CfValue::CFValueString),
            ))
        })();
        self.new_node_range(ctx, CfNode::CFApplyEffects(effect.into_iter().collect()))
    }

    fn handle_others(
        &mut self,
        ctx: &CfContext,
        id: Id,
        vars: &[Token],
        args: &[&Token],
        cmd: Option<String>,
    ) -> Range {
        self.regular_expansion(ctx, vars, args, &mut |s, c| {
            let exe = s.new_node_range(c, CfNode::CFExecuteCommand(cmd.clone()));
            let status = s.new_node_range(c, CfNode::CFSetExitCode(id));
            s.link_range(exe, status)
        })
    }

    fn regular_expansion(
        &mut self,
        ctx: &CfContext,
        vars: &[Token],
        args: &[&Token],
        p: &mut dyn FnMut(&mut Self, &CfContext) -> Range,
    ) -> Range {
        let args_r = self.sequentially_refs(ctx, args);
        let mut assignments = Vec::new();
        for v in vars {
            assignments.push(self.build_assignment(ctx, Some(Scope::PrefixScope), v));
        }
        let exe = p(self, ctx);
        let mut ranges = vec![args_r];
        ranges.extend(assignments);
        ranges.push(exe);
        if !vars.is_empty() {
            ranges.push(self.new_node_range(ctx, CfNode::CFDropPrefixAssignments));
        }
        self.link_ranges(&ranges)
    }

    fn regular_expansion_with_status(
        &mut self,
        ctx: &CfContext,
        vars: &[Token],
        args: &[&Token],
        p: &mut dyn FnMut(&mut Self, &CfContext) -> Range,
    ) -> Range {
        let initial = self.regular_expansion(ctx, vars, args, p);
        let status = self.new_node_range(ctx, CfNode::CFSetExitCode(args[0].id));
        self.link_range(initial, status)
    }

    fn build_assignment(&mut self, ctx: &CfContext, scope: Option<Scope>, t: &Token) -> Range {
        let T_Assignment(mode, var, indices, value) = &t.inner else {
            panic!("ShellCheck CFG: buildAssignment on a non-assignment");
        };
        let id = t.id;
        let expand = self.build(ctx, value);
        let index = self.sequentially(ctx, indices);
        let read = match mode {
            AssignmentMode::Append => self.new_node_range(
                ctx,
                apply_single(IdTagged(id, CfEffect::CFReadVariable(var.clone()))),
            ),
            AssignmentMode::Assign => self.none(ctx),
        };
        let value_type = if indices.is_empty() {
            match &value.inner {
                T_NormalWord(_) | T_Literal(_) => {
                    let mut parts = Vec::new();
                    if *mode == AssignmentMode::Append {
                        parts.push(CfStringPart::CFStringVariable(var.clone()));
                    }
                    parts.extend(token_to_parts(value));
                    CfValue::CFValueComputed(id, parts)
                }
                T_Array(_) => CfValue::CFValueArray,
                _ => panic!("ShellCheck CFG: unexpected assignment value"),
            }
        } else {
            CfValue::CFValueArray
        };
        let effect = match scope {
            Some(Scope::PrefixScope) => CfEffect::CFWritePrefix(var.clone(), value_type),
            Some(Scope::LocalScope) => CfEffect::CFWriteLocal(var.clone(), value_type),
            Some(Scope::GlobalScope) => CfEffect::CFWriteGlobal(var.clone(), value_type),
            None => CfEffect::CFWriteVariable(var.clone(), value_type),
        };
        let write = self.new_node_range(ctx, apply_single(IdTagged(id, effect)));
        let op = self.link_ranges(&[expand, index, read, write]);
        self.register_node(id, op);
        op
    }
}

/// The arguments as an owned slice, for the option parsers.
fn args_slice(args: &[&Token]) -> Vec<Token> {
    args.iter().map(|t| (*t).clone()).collect()
}

/// `tokenToParts`.
pub fn token_to_parts(t: &Token) -> Vec<CfStringPart> {
    match &t.inner {
        T_NormalWord(list) | T_DoubleQuoted(list) => list.iter().flat_map(token_to_parts).collect(),
        T_SingleQuoted(s) | T_Literal(s) => vec![CfStringPart::CFStringLiteral(s.clone())],
        T_DollarArithmetic(_) | T_DollarBracket(_) => vec![CfStringPart::CFStringInteger],
        T_DollarBraced(_, list) if is_unmodified_parameter_expansion(t) => {
            vec![CfStringPart::CFStringVariable(get_braced_reference(
                &oversimplify(list).concat(),
            ))]
        }
        _ => vec![
            get_literal_string(t)
                .map_or(CfStringPart::CFStringUnknown, CfStringPart::CFStringLiteral),
        ],
    }
}

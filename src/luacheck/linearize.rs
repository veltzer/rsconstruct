//! luacheck's `linearize` stage: each function body becomes a line of
//! items (evaluations, assignments, jumps), local variables are resolved
//! to scopes, and redefined or shadowed locals and unused labels are
//! reported.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::ast::{Item, Node, NodeId, Range, Tag};
use super::check::{
    CheckState, ItemId, ItemTag, LItem, Line, LineId, Overwriting, Secondaries, Value, ValueId,
    Var, VarId, VarType, Warning,
};
use super::parser::{SyntaxError, syntax_error};

const PSEUDO_LABELS: &[&[u8]] = &[b"do", b"else", b"break", b"end", b"return"];

struct Label {
    range: Option<Range>,
    index: usize,
    used: bool,
}

struct Goto {
    name: Vec<u8>,
    jump: ItemId,
    range: Option<Range>,
}

struct Scope {
    vars: HashMap<Vec<u8>, VarId>,
    /// Labels in definition order, for deterministic warnings.
    labels: Vec<(Vec<u8>, Label)>,
    gotos: Vec<Goto>,
    line: Option<LineId>,
}

struct LinState<'a> {
    st: &'a mut CheckState,
    lines: Vec<LineId>,
    scopes: Vec<Scope>,
}

fn is_unpacking(st: &CheckState, node: NodeId) -> bool {
    matches!(st.ast.tag(node), Some(Tag::Dots | Tag::Call | Tag::Invoke))
}

impl LinState<'_> {
    fn top_line(&self) -> LineId {
        *self.lines.last().expect("current line")
    }

    fn items_size(&self) -> usize {
        self.st.lines[self.top_line()].items.len()
    }

    fn enter_scope(&mut self) {
        self.scopes.push(Scope {
            vars: HashMap::new(),
            labels: Vec::new(),
            gotos: Vec::new(),
            line: self.lines.last().copied(),
        });
    }

    fn leave_scope(&mut self) -> Result<(), SyntaxError> {
        let mut left_scope = self.scopes.pop().expect("scope");
        let top_line = self.lines.last().copied();
        for goto in std::mem::take(&mut left_scope.gotos) {
            if let Some((_, label)) = left_scope
                .labels
                .iter_mut()
                .find(|(name, _)| *name == goto.name)
            {
                self.st.items[goto.jump].to = label.index;
                label.used = true;
            } else {
                let prev_line = self.scopes.last().map(|s| s.line);
                if prev_line.is_none() || prev_line != Some(top_line) {
                    let range = goto.range.expect("goto range");
                    if goto.name == b"break" {
                        return syntax_error(b"'break' is not inside a loop".to_vec(), range, None);
                    }
                    let mut msg = b"no visible label '".to_vec();
                    msg.extend_from_slice(&goto.name);
                    msg.push(b'\'');
                    return syntax_error(msg, range, None);
                }
                self.scopes.last_mut().expect("scope").gotos.push(goto);
            }
        }
        for (name, label) in &left_scope.labels {
            if !label.used && !PSEUDO_LABELS.contains(&name.as_slice()) {
                let mut warning = Warning::new("521");
                warning.label = Some(name.clone());
                self.st
                    .warn_range(warning, label.range.expect("label range"));
            }
        }
        let size = self.items_size();
        for &var in left_scope.vars.values() {
            self.st.vars[var].scope_end = size;
        }
        Ok(())
    }

    fn resolve_var(&self, name: &[u8]) -> Option<VarId> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.vars.get(name).copied())
    }

    fn warn_redefined(&mut self, var: VarId, prev_var: VarId, is_same_scope: bool) {
        let (line, prev_line_id) = (self.st.vars[var].line, self.st.vars[prev_var].line);
        let middle = if is_same_scope {
            '1'
        } else if line == prev_line_id {
            '2'
        } else {
            '3'
        };
        let code = format!("4{middle}{}", self.st.vars[prev_var].ty.code());
        let prev_node = self.st.vars[prev_var].node;
        let prev_range = self.st.ast.range(prev_node);
        let mut warning = Warning::new(&code);
        warning.is_self = self.st.vars[var].is_self && self.st.vars[prev_var].is_self;
        warning.prev_line = Some(prev_range.line);
        warning.prev_column = Some(self.st.offset_to_column(prev_range.line, prev_range.offset));
        warning.prev_end_column = Some(
            self.st
                .offset_to_column(prev_range.line, prev_range.end_offset),
        );
        self.st.warn_var(warning, var);
    }

    fn register_var(&mut self, node: NodeId, ty: VarType) -> VarId {
        let name = self.st.ast.str(node, 1).unwrap_or_default().to_vec();
        let hint_unused = ty == VarType::Arg
            && name.len() >= 2
            && name[0] == b'_'
            && name[1].is_ascii_alphabetic();
        let var = Var {
            name: name.clone(),
            node,
            ty,
            is_self: self.st.ast.nodes[node].implicit,
            line: self.top_line(),
            hint_unused,
            scope_start: self.items_size() + 1,
            scope_end: 0,
            values: Vec::new(),
            accessed: false,
            mutated: false,
        };
        self.st.vars.push(var);
        let var = self.st.vars.len() - 1;
        if let Some(prev_var) = self.resolve_var(&name) {
            let is_same_scope = self.scopes.last().expect("scope").vars.contains_key(&name);
            if name != b"..." {
                self.warn_redefined(var, prev_var, is_same_scope);
            }
            if is_same_scope {
                self.st.vars[prev_var].scope_end = self.items_size();
            }
        }
        self.scopes
            .last_mut()
            .expect("scope")
            .vars
            .insert(name, var);
        self.st.ast.nodes[node].var = Some(var);
        var
    }

    fn register_vars(&mut self, list: NodeId, ty: VarType) {
        for node in self.st.ast.kids(list) {
            self.register_var(node, ty);
        }
    }

    fn check_var(&mut self, node: NodeId) -> Option<VarId> {
        if self.st.ast.nodes[node].var.is_none() {
            let name = self.st.ast.str(node, 1).unwrap_or_default().to_vec();
            self.st.ast.nodes[node].var = self.resolve_var(&name);
        }
        self.st.ast.nodes[node].var
    }

    fn register_label(&mut self, name: &[u8], range: Option<Range>) -> Result<(), SyntaxError> {
        if let Some((_, prev)) = self
            .scopes
            .last()
            .expect("scope")
            .labels
            .iter()
            .find(|(n, _)| n == name)
        {
            let prev_range = prev.range.expect("previous label range");
            let mut msg = b"label '".to_vec();
            msg.extend_from_slice(name);
            msg.extend_from_slice(
                format!("' already defined on line {}", prev_range.line).as_bytes(),
            );
            return syntax_error(msg, range.expect("label range"), Some(prev_range));
        }
        let index = self.items_size() + 1;
        self.scopes.last_mut().expect("scope").labels.push((
            name.to_vec(),
            Label {
                range,
                index,
                used: false,
            },
        ));
        Ok(())
    }

    fn emit(&mut self, item: LItem) -> ItemId {
        self.st.items.push(item);
        let id = self.st.items.len() - 1;
        let line = self.top_line();
        self.st.lines[line].items.push(id);
        id
    }

    fn emit_goto(&mut self, name: &[u8], is_conditional: bool, range: Option<Range>) {
        let jump = self.emit(LItem::new(if is_conditional {
            ItemTag::Cjump
        } else {
            ItemTag::Jump
        }));
        self.scopes.last_mut().expect("scope").gotos.push(Goto {
            name: name.to_vec(),
            jump,
            range,
        });
    }

    /// Emits a goto to `name` taken when `cond_node` is false.
    fn emit_cond_goto(&mut self, name: &[u8], cond_node: NodeId) {
        let cond_bool = match self.st.ast.tag(cond_node) {
            Some(Tag::Nil | Tag::False) => Some(false),
            Some(Tag::True | Tag::Number | Tag::String | Tag::Table | Tag::Function) => Some(true),
            _ => None,
        };
        if cond_bool != Some(true) {
            self.emit_goto(name, cond_bool != Some(false), None);
        }
    }

    fn emit_noop(&mut self, node: NodeId, loop_end: bool) {
        let mut item = LItem::new(ItemTag::Noop);
        item.node = Some(node);
        item.loop_end = loop_end;
        self.emit(item);
    }

    fn emit_stmts(&mut self, stmts: NodeId) -> Result<(), SyntaxError> {
        for stmt in self.st.ast.kids(stmts) {
            self.emit_stmt(stmt)?;
        }
        Ok(())
    }

    fn emit_block(&mut self, block: NodeId) -> Result<(), SyntaxError> {
        self.enter_scope();
        self.emit_stmts(block)?;
        self.leave_scope()
    }

    fn emit_stmt(&mut self, node: NodeId) -> Result<(), SyntaxError> {
        let ast = &self.st.ast;
        match ast.tag(node).expect("statement tag") {
            Tag::Do => {
                self.emit_noop(node, false);
                self.emit_block(node)
            }
            Tag::While => {
                let (cond, block) = (ast.kid(node, 1), ast.kid(node, 2));
                self.emit_noop(node, false);
                self.enter_scope();
                self.register_label(b"do", None)?;
                self.emit_expr(cond)?;
                self.emit_cond_goto(b"break", cond);
                self.emit_block(block)?;
                self.emit_noop(node, true);
                self.emit_goto(b"do", false, None);
                self.register_label(b"break", None)?;
                self.leave_scope()
            }
            Tag::Repeat => {
                let (block, cond) = (ast.kid(node, 1), ast.kid(node, 2));
                self.emit_noop(node, false);
                self.enter_scope();
                self.register_label(b"do", None)?;
                self.enter_scope();
                self.emit_stmts(block)?;
                self.emit_expr(cond)?;
                self.leave_scope()?;
                self.emit_cond_goto(b"do", cond);
                self.register_label(b"break", None)?;
                self.leave_scope()
            }
            Tag::Fornum => {
                let var = ast.kid(node, 1);
                let has_step = ast.get(node, 5).is_some();
                let block = if has_step {
                    ast.kid(node, 5)
                } else {
                    ast.kid(node, 4)
                };
                let (from, to) = (ast.kid(node, 2), ast.kid(node, 3));
                let step = if has_step {
                    Some(ast.kid(node, 4))
                } else {
                    None
                };
                self.emit_noop(node, false);
                self.emit_expr(from)?;
                self.emit_expr(to)?;
                if let Some(step) = step {
                    self.emit_expr(step)?;
                }
                self.enter_scope();
                self.register_label(b"do", None)?;
                self.emit_goto(b"break", true, None);
                self.enter_scope();
                let lhs = self.st.ast.list(vec![var]);
                self.emit_local_item(lhs, None);
                self.register_var(var, VarType::Loopi);
                self.emit_stmts(block)?;
                self.leave_scope()?;
                self.emit_noop(node, true);
                self.emit_goto(b"do", false, None);
                self.register_label(b"break", None)?;
                self.leave_scope()
            }
            Tag::Forin => {
                let (vars, exprs, block) = (ast.kid(node, 1), ast.kid(node, 2), ast.kid(node, 3));
                self.emit_noop(node, false);
                self.emit_exprs(exprs)?;
                self.enter_scope();
                self.register_label(b"do", None)?;
                self.emit_goto(b"break", true, None);
                self.enter_scope();
                self.emit_local_item(vars, None);
                self.register_vars(vars, VarType::Loop);
                self.emit_stmts(block)?;
                self.leave_scope()?;
                self.emit_noop(node, true);
                self.emit_goto(b"do", false, None);
                self.register_label(b"break", None)?;
                self.leave_scope()
            }
            Tag::If => {
                let len = ast.len(node);
                self.emit_noop(node, false);
                self.enter_scope();
                let mut i = 1;
                while i < len {
                    let (cond, block) = (self.st.ast.kid(node, i), self.st.ast.kid(node, i + 1));
                    self.enter_scope();
                    self.emit_expr(cond)?;
                    self.emit_cond_goto(b"else", cond);
                    self.emit_block(block)?;
                    self.emit_goto(b"end", false, None);
                    self.register_label(b"else", None)?;
                    self.leave_scope()?;
                    i += 2;
                }
                if len % 2 == 1 {
                    let block = self.st.ast.kid(node, len);
                    self.emit_block(block)?;
                }
                self.register_label(b"end", None)?;
                self.leave_scope()
            }
            Tag::Label => {
                let name = ast.str(node, 1).unwrap_or_default().to_vec();
                let range = ast.range(node);
                self.register_label(&name, Some(range))
            }
            Tag::Goto => {
                let name = ast.str(node, 1).unwrap_or_default().to_vec();
                let range = ast.range(node);
                self.emit_noop(node, false);
                self.emit_goto(&name, false, Some(range));
                Ok(())
            }
            Tag::Break => {
                let range = ast.range(node);
                self.emit_goto(b"break", false, Some(range));
                Ok(())
            }
            Tag::Return => {
                self.emit_noop(node, false);
                self.emit_exprs(node)?;
                self.emit_goto(b"return", false, None);
                Ok(())
            }
            Tag::Call | Tag::Invoke => self.emit_expr(node),
            Tag::Local => {
                let (lhs, rhs) = (ast.kid(node, 1), ast.get(node, 2));
                let item = self.emit_local_item(lhs, rhs);
                self.st.items[item].node = Some(node);
                if let Some(rhs) = rhs {
                    self.scan_exprs(item, rhs)?;
                }
                self.register_vars(lhs, VarType::Var);
                Ok(())
            }
            Tag::Localrec => {
                let (lhs, rhs) = (ast.kid(node, 1), ast.kid(node, 2));
                let var = ast.kid(lhs, 1);
                let func = ast.kid(rhs, 1);
                self.register_var(var, VarType::Var);
                let item = self.emit_local_item(lhs, Some(rhs));
                self.st.items[item].node = Some(node);
                self.scan_expr(item, func)
            }
            Tag::Set | Tag::OpSet => {
                let tag = if ast.is(node, Tag::Set) {
                    ItemTag::Set
                } else {
                    ItemTag::OpSet
                };
                let (lhs, rhs) = (ast.kid(node, 1), ast.kid(node, 2));
                let mut item = LItem::new(tag);
                item.node = Some(node);
                item.lhs = Some(lhs);
                item.rhs = Some(rhs);
                item.accesses = Some(BTreeMap::default());
                item.mutations = Some(BTreeMap::default());
                item.used_values = Some(BTreeMap::default());
                item.lines = Some(Vec::new());
                self.st.items.push(item);
                let item = self.st.items.len() - 1;
                self.scan_exprs(item, rhs)?;
                for expr in self.st.ast.kids(lhs) {
                    if self.st.ast.is(expr, Tag::Id) {
                        if let Some(var) = self.check_var(expr) {
                            self.register_upvalue_action(item, var, Upvalue::Set);
                        }
                    } else {
                        self.scan_lhs_index(item, expr)?;
                    }
                }
                let line = self.top_line();
                self.st.lines[line].items.push(item);
                Ok(())
            }
            tag => unreachable!("not a statement: {tag:?}"),
        }
    }

    /// `new_local_item`: an item for a local definition with the given lists.
    fn emit_local_item(&mut self, lhs: NodeId, rhs: Option<NodeId>) -> ItemId {
        let mut item = LItem::new(ItemTag::Local);
        // The item's node is the statement for a `local`, a synthetic list
        // otherwise (luacheck builds `{lhs}` tables for loops and arguments).
        let synthetic = self.st.ast.add(Node {
            items: std::iter::once(Item::Node(lhs))
                .chain(rhs.map(Item::Node))
                .collect(),
            ..Node::default()
        });
        item.node = Some(synthetic);
        item.lhs = Some(lhs);
        item.rhs = rhs;
        if rhs.is_some() {
            item.accesses = Some(BTreeMap::default());
            item.used_values = Some(BTreeMap::default());
            item.lines = Some(Vec::new());
        }
        self.emit(item)
    }

    fn emit_expr(&mut self, node: NodeId) -> Result<(), SyntaxError> {
        let mut item = LItem::new(ItemTag::Eval);
        item.node = Some(node);
        item.accesses = Some(BTreeMap::default());
        item.used_values = Some(BTreeMap::default());
        item.lines = Some(Vec::new());
        self.st.items.push(item);
        let item = self.st.items.len() - 1;
        self.scan_expr(item, node)?;
        let line = self.top_line();
        self.st.lines[line].items.push(item);
        Ok(())
    }

    fn emit_exprs(&mut self, exprs: NodeId) -> Result<(), SyntaxError> {
        for expr in self.st.ast.kids(exprs) {
            self.emit_expr(expr)?;
        }
        Ok(())
    }

    fn register_upvalue_action(&mut self, item: ItemId, var: VarId, kind: Upvalue) {
        let var_line = self.st.vars[var].line;
        for &line in self.lines.iter().rev() {
            if line == var_line {
                break;
            }
            let line = &mut self.st.lines[line];
            let map = match kind {
                Upvalue::Accessed => &mut line.accessed_upvalues,
                Upvalue::Mutated => &mut line.mutated_upvalues,
                Upvalue::Set => &mut line.set_upvalues,
            };
            map.entry(var).or_default().push(item);
        }
    }

    fn mark_access(&mut self, item: ItemId, node: NodeId) {
        let var = self.st.ast.nodes[node].var.expect("resolved variable");
        self.st.vars[var].accessed = true;
        self.st.items[item]
            .accesses
            .as_mut()
            .expect("item with accesses")
            .entry(var)
            .or_default()
            .push(node);
        self.register_upvalue_action(item, var, Upvalue::Accessed);
    }

    fn mark_mutation(&mut self, item: ItemId, node: NodeId) {
        let var = self.st.ast.nodes[node].var.expect("resolved variable");
        self.st.vars[var].mutated = true;
        self.st.items[item]
            .mutations
            .as_mut()
            .expect("item with mutations")
            .entry(var)
            .or_default()
            .push(node);
        self.register_upvalue_action(item, var, Upvalue::Mutated);
    }

    fn scan_expr(&mut self, item: ItemId, node: NodeId) -> Result<(), SyntaxError> {
        match self.st.ast.tag(node) {
            Some(Tag::Id) => {
                if self.check_var(node).is_some() {
                    self.mark_access(item, node);
                }
                Ok(())
            }
            Some(Tag::Dots) => {
                let dots = self.check_var(node);
                if dots.is_none() || self.st.vars[dots.expect("dots")].line != self.top_line() {
                    return syntax_error(
                        b"cannot use '...' outside a vararg function".to_vec(),
                        self.st.ast.range(node),
                        None,
                    );
                }
                self.mark_access(item, node);
                Ok(())
            }
            Some(Tag::Index | Tag::Call | Tag::Invoke | Tag::Paren | Tag::Table | Tag::Pair) => {
                self.scan_exprs(item, node)
            }
            Some(Tag::Op) => {
                self.scan_expr(item, self.st.ast.kid(node, 2))?;
                if let Some(rhs) = self.st.ast.get(node, 3) {
                    self.scan_expr(item, rhs)?;
                }
                Ok(())
            }
            Some(Tag::Function) => {
                let line = self.build_line(node)?;
                let nested = self.st.lines[line].lines.clone();
                let lines = self.st.items[item].lines.as_mut().expect("item with lines");
                lines.push(line);
                lines.extend(nested);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn scan_exprs(&mut self, item: ItemId, nodes: NodeId) -> Result<(), SyntaxError> {
        for node in self.st.ast.kids(nodes) {
            self.scan_expr(item, node)?;
        }
        Ok(())
    }

    fn scan_lhs_index(&mut self, item: ItemId, node: NodeId) -> Result<(), SyntaxError> {
        let base = self.st.ast.kid(node, 1);
        match self.st.ast.tag(base) {
            Some(Tag::Id) => {
                if self.check_var(base).is_some() {
                    self.mark_mutation(item, base);
                }
            }
            Some(Tag::Index) => self.scan_lhs_index(item, base)?,
            _ => self.scan_expr(item, base)?,
        }
        self.scan_expr(item, self.st.ast.kid(node, 2))
    }

    fn new_value(
        &mut self,
        var_node: NodeId,
        value_node: Option<NodeId>,
        item: ItemId,
        is_init: bool,
    ) -> ValueId {
        let var = self.st.ast.nodes[var_node].var.expect("resolved variable");
        let var_ty = self.st.vars[var].ty;
        let mut ty = if is_init { var_ty } else { VarType::Var };
        let value_id = self.st.values.len();
        if let Some(value_node) = value_node
            && self.st.ast.is(value_node, Tag::Function)
        {
            ty = VarType::Func;
            self.st.ast.nodes[value_node].value = Some(value_id);
        }
        self.st.values.push(Value {
            var,
            var_node,
            ty,
            node: value_node,
            using_lines: BTreeSet::default(),
            empty: is_init && value_node.is_none() && var_ty == VarType::Var,
            item,
            secondaries: None,
            used: false,
            mutated: false,
            overwriting_item: Overwriting::Nil,
        });
        value_id
    }

    fn register_set_variables(&mut self) {
        let line = self.top_line();
        for item in self.st.lines[line].items.clone() {
            let tag = self.st.items[item].tag;
            if !matches!(tag, ItemTag::Local | ItemTag::Set | ItemTag::OpSet) {
                continue;
            }
            self.st.items[item].set_variables = Some(BTreeMap::default());
            let is_init = tag == ItemTag::Local;
            let lhs = self.st.items[item].lhs.expect("lhs");
            let rhs = self.st.items[item].rhs;
            let rhs_nodes = rhs.map(|rhs| self.st.ast.kids(rhs)).unwrap_or_default();
            let lhs_nodes = self.st.ast.kids(lhs);
            let unpacking_item = rhs_nodes
                .last()
                .copied()
                .filter(|&last| is_unpacking(self.st, last));
            let secondaries = if unpacking_item.is_some() && lhs_nodes.len() > rhs_nodes.len() {
                self.st.secondaries.push(Secondaries::default());
                Some(self.st.secondaries.len() - 1)
            } else {
                None
            };
            for (i, &node) in lhs_nodes.iter().enumerate() {
                let i = i + 1;
                let value = if let Some(var) = self.st.ast.nodes[node].var {
                    if tag == ItemTag::OpSet {
                        self.mark_access(item, node);
                    }
                    let value_node = rhs_nodes.get(i - 1).copied().or(unpacking_item);
                    let v = self.new_value(node, value_node, item, is_init);
                    self.st.items[item]
                        .set_variables
                        .as_mut()
                        .expect("set variables")
                        .insert(var, v);
                    self.st.vars[var].values.push(v);
                    Some(v)
                } else {
                    None
                };
                if let Some(sec) = secondaries
                    && i >= rhs_nodes.len()
                {
                    match value {
                        Some(v) => {
                            self.st.values[v].secondaries = Some(sec);
                            self.st.secondaries[sec].values.push(v);
                        }
                        None => self.st.secondaries[sec].used = true,
                    }
                }
            }
        }
    }

    /// Builds the line of a function node (or the main chunk's synthetic
    /// node): `node[1]` is the argument list, `node[2]` the body.
    fn build_line(&mut self, node: NodeId) -> Result<LineId, SyntaxError> {
        let parent = self.lines.last().copied();
        self.st.lines.push(Line {
            accessed_upvalues: BTreeMap::default(),
            mutated_upvalues: BTreeMap::default(),
            set_upvalues: BTreeMap::default(),
            lines: Vec::new(),
            node,
            parent,
            items: Vec::new(),
        });
        let line = self.st.lines.len() - 1;
        self.lines.push(line);
        let (args, body) = (self.st.ast.kid(node, 1), self.st.ast.kid(node, 2));
        self.enter_scope();
        self.emit_local_item(args, None);
        self.enter_scope();
        self.register_vars(args, VarType::Arg);
        self.emit_stmts(body)?;
        self.leave_scope()?;
        self.register_label(b"return", None)?;
        self.leave_scope()?;
        self.register_set_variables();
        self.lines.pop();
        for &prev_line in &self.lines {
            self.st.lines[prev_line].lines.push(line);
        }
        Ok(line)
    }
}

#[derive(Clone, Copy)]
enum Upvalue {
    Accessed,
    Mutated,
    Set,
}

pub fn run(st: &mut CheckState) -> Result<(), SyntaxError> {
    let dots = st.ast.add(Node {
        tag: Some(Tag::Dots),
        items: vec![Item::Str(b"...".to_vec())],
        ..Node::default()
    });
    let args = st.ast.list(vec![dots]);
    let root = st.root;
    let top = st.ast.list(vec![args, root]);
    let mut lin = LinState {
        st,
        lines: Vec::new(),
        scopes: Vec::new(),
    };
    let top_line = lin.build_line(top)?;
    st.top_line = top_line;
    st.all_lines = std::iter::once(top_line)
        .chain(st.lines[top_line].lines.iter().copied())
        .collect();
    Ok(())
}

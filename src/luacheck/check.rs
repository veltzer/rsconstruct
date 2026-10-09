//! The check state shared by the stages (luacheck's `check_state`), the
//! warning record, and `check`: one source through all stages.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::ast::{Ast, NodeId, Range};
use super::decoder::Chars;
use super::inline_options::InlineOption;
use super::parser::{Comment, LineEnding, SyntaxError};

/// A key of a global's indexing chain, as `detect_globals` records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexKey {
    /// Indexing with something that may or may not be a string (`true`).
    Unknown,
    /// Indexing with something that is not a string (`false`).
    NotString,
    Str(Vec<u8>),
}

/// A warning or error. luacheck keeps these as tables with per-code fields;
/// every field any code uses is here, unset by default.
#[derive(Clone, Debug, Default)]
pub struct Warning {
    pub code: String,
    pub line: usize,
    pub column: usize,
    pub end_column: usize,
    pub name: Option<Vec<u8>>,
    pub msg: Option<Vec<u8>>,
    pub prev_line: Option<usize>,
    pub prev_column: Option<usize>,
    pub prev_end_column: Option<usize>,
    pub is_self: bool,
    pub func: bool,
    pub secondary: bool,
    pub useless: bool,
    pub recursive: bool,
    pub mutually_recursive: bool,
    pub overwritten_line: Option<usize>,
    pub overwritten_column: Option<usize>,
    pub overwritten_end_column: Option<usize>,
    pub indexing: Option<Vec<IndexKey>>,
    pub previous_indexing_len: Option<usize>,
    pub top: bool,
    pub indirect: bool,
    pub module: bool,
    pub field: Option<Vec<u8>>,
    pub index: bool,
    pub operator: Option<Vec<u8>>,
    pub replacement_operator: Option<Vec<u8>>,
    pub label: Option<Vec<u8>>,
    pub complexity: Option<usize>,
    pub function_type: Option<&'static str>,
    pub function_name: Option<Vec<u8>>,
    pub max_complexity: Option<f64>,
    pub max_length: Option<f64>,
    pub limit: Option<Vec<u8>>,
}

impl Warning {
    pub fn new(code: &str) -> Self {
        Self {
            code: code.to_string(),
            ..Self::default()
        }
    }
}

pub type VarId = usize;
pub type ValueId = usize;
pub type ItemId = usize;
pub type LineId = usize;
pub type VarMap<T> = BTreeMap<VarId, T>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarType {
    Var,
    Func,
    Arg,
    Loop,
    Loopi,
}

impl VarType {
    /// `type_codes`: the digit warning codes use for a variable type.
    pub const fn code(self) -> char {
        match self {
            Self::Var | Self::Func => '1',
            Self::Arg => '2',
            Self::Loop | Self::Loopi => '3',
        }
    }
}

pub struct Var {
    pub name: Vec<u8>,
    pub node: NodeId,
    pub ty: VarType,
    pub is_self: bool,
    pub line: LineId,
    pub hint_unused: bool,
    pub scope_start: usize,
    pub scope_end: usize,
    pub values: Vec<ValueId>,
    pub accessed: bool,
    pub mutated: bool,
}

/// `value.overwriting_item`: unset, `false`, or an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overwriting {
    Nil,
    False,
    Item(ItemId),
}

pub struct Value {
    pub var: VarId,
    pub var_node: NodeId,
    pub ty: VarType,
    pub node: Option<NodeId>,
    pub using_lines: BTreeSet<LineId>,
    pub empty: bool,
    pub item: ItemId,
    pub secondaries: Option<usize>,
    pub used: bool,
    pub mutated: bool,
    pub overwriting_item: Overwriting,
}

/// Values unpacked together from the rightmost expression of an assignment.
#[derive(Default)]
pub struct Secondaries {
    pub values: Vec<ValueId>,
    pub used: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemTag {
    Jump,
    Cjump,
    Eval,
    Noop,
    Local,
    Set,
    OpSet,
}

/// An item of a line (luacheck's linearized flow graph).
pub struct LItem {
    pub tag: ItemTag,
    /// A jump's target index (1-based).
    pub to: usize,
    pub node: Option<NodeId>,
    pub loop_end: bool,
    pub lhs: Option<NodeId>,
    pub rhs: Option<NodeId>,
    pub accesses: Option<VarMap<Vec<NodeId>>>,
    pub mutations: Option<VarMap<Vec<NodeId>>>,
    pub used_values: Option<VarMap<Vec<ValueId>>>,
    pub lines: Option<Vec<LineId>>,
    pub set_variables: Option<VarMap<ValueId>>,
}

impl LItem {
    pub const fn new(tag: ItemTag) -> Self {
        Self {
            tag,
            to: 0,
            node: None,
            loop_end: false,
            lhs: None,
            rhs: None,
            accesses: None,
            mutations: None,
            used_values: None,
            lines: None,
            set_variables: None,
        }
    }
}

/// A function body (or the main chunk) as a list of items.
pub struct Line {
    pub accessed_upvalues: VarMap<Vec<ItemId>>,
    pub mutated_upvalues: VarMap<Vec<ItemId>>,
    pub set_upvalues: VarMap<Vec<ItemId>>,
    pub lines: Vec<LineId>,
    pub node: NodeId,
    pub parent: Option<LineId>,
    pub items: Vec<ItemId>,
}

pub struct CheckState {
    pub source: Chars,
    pub line_offsets: Vec<Option<usize>>,
    pub line_lengths: Vec<Option<usize>>,
    pub warnings: Vec<Warning>,
    pub ast: Ast,
    pub root: NodeId,
    pub comments: Vec<Comment>,
    pub code_lines: HashMap<usize, bool>,
    pub line_endings: HashMap<usize, LineEnding>,
    pub useless_semicolons: Vec<Range>,
    pub vars: Vec<Var>,
    pub values: Vec<Value>,
    pub secondaries: Vec<Secondaries>,
    pub items: Vec<LItem>,
    pub lines: Vec<Line>,
    pub top_line: LineId,
    /// All lines: the main chunk first, then nested ones.
    pub all_lines: Vec<LineId>,
    pub inline_options: Vec<InlineOption>,
}

impl CheckState {
    pub fn line_length(&self, line: usize) -> Option<usize> {
        self.line_lengths.get(line).copied().flatten()
    }

    /// The column of a character given its offset, never past the line end.
    pub fn offset_to_column(&self, line: usize, offset: usize) -> usize {
        let line_offset = self.line_offsets[line].expect("line offset known");
        let column = offset as i64 - line_offset as i64 + 1;
        match self.line_length(line) {
            None => column.max(0) as usize,
            Some(line_length) => column.min(line_length as i64).max(1) as usize,
        }
    }

    pub fn warn(
        &mut self,
        mut warning: Warning,
        line: usize,
        offset: usize,
        end_offset: usize,
    ) -> usize {
        warning.line = line;
        warning.column = self.offset_to_column(line, offset);
        warning.end_column = self.offset_to_column(line, end_offset);
        self.warnings.push(warning);
        self.warnings.len() - 1
    }

    pub fn warn_range(&mut self, warning: Warning, range: Range) -> usize {
        self.warn(warning, range.line, range.offset, range.end_offset)
    }

    pub fn warn_node(&mut self, warning: Warning, node: NodeId) -> usize {
        let range = self.ast.range(node);
        self.warn_range(warning, range)
    }

    pub fn warn_var(&mut self, mut warning: Warning, var: VarId) -> usize {
        warning.name = Some(self.vars[var].name.clone());
        let node = self.vars[var].node;
        self.warn_node(warning, node)
    }

    pub fn warn_value(&mut self, mut warning: Warning, value: ValueId) -> usize {
        let var = self.values[value].var;
        warning.name = Some(self.vars[var].name.clone());
        let node = self.values[value].var_node;
        self.warn_node(warning, node)
    }

    /// `Line:walk`: visits items reachable from `index` in depth-first order
    /// (the target of a conditional jump before the next item); the callback
    /// returns `true` to stop walking from the current item.
    pub fn walk(
        &self,
        line: LineId,
        visited: &mut BTreeSet<usize>,
        index: usize,
        callback: &mut dyn FnMut(&Self, usize, Option<ItemId>) -> bool,
    ) {
        let items = &self.lines[line].items;
        let mut pending = vec![index];
        while let Some(mut index) = pending.pop() {
            loop {
                if !visited.insert(index) {
                    break;
                }
                let item = items.get(index - 1).copied();
                if callback(self, index, item) {
                    break;
                }
                let Some(item) = item else { break };
                match self.items[item].tag {
                    ItemTag::Jump => index = self.items[item].to,
                    ItemTag::Cjump => {
                        pending.push(index + 1);
                        index = self.items[item].to;
                    }
                    _ => index += 1,
                }
            }
        }
    }
}

/// The parse stage's result for a source that failed to parse.
pub fn syntax_error_warning(state: &CheckState, err: &SyntaxError) -> Warning {
    let mut warning = Warning::new("011");
    warning.line = err.range.line;
    warning.column = state.offset_to_column(err.range.line, err.range.offset);
    warning.end_column = state.offset_to_column(err.range.line, err.range.end_offset);
    warning.msg = Some(err.msg.clone());
    if let Some(prev) = err.prev {
        warning.prev_line = Some(prev.line);
        warning.prev_column = Some(state.offset_to_column(prev.line, prev.offset));
        warning.prev_end_column = Some(state.offset_to_column(prev.line, prev.end_offset));
    }
    warning
}

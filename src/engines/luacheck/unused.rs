//! luacheck's `detect_unused_locals` stage: unused locals, values and
//! arguments, variables never set or never accessed, and unused
//! (mutually) recursive functions.

use std::collections::{BTreeMap, BTreeSet};

use super::ast::{NodeId, Tag};
use super::check::{
    CheckState, ItemId, ItemTag, LineId, Overwriting, ValueId, VarId, VarType, Warning,
};

fn is_secondary(st: &CheckState, value: ValueId) -> bool {
    st.values[value]
        .secondaries
        .is_some_and(|sec| st.secondaries[sec].used)
}

fn warn_unused_var(st: &mut CheckState, value: ValueId, is_useless: bool) {
    let var = st.values[value].var;
    let code = format!("21{}", st.vars[var].ty.code());
    let mut warning = Warning::new(&code);
    warning.secondary = is_secondary(st, value);
    warning.func = st.values[value].ty == VarType::Func;
    warning.is_self = st.vars[var].is_self;
    warning.useless = st.vars[var].name == b"_" && is_useless;
    st.warn_value(warning, value);
}

fn warn_unaccessed_var(st: &mut CheckState, var: VarId, is_mutated: bool) {
    let secondary = st.vars[var]
        .values
        .iter()
        .all(|&value| st.values[value].empty || is_secondary(st, value));
    let code = format!(
        "2{}{}",
        if is_mutated { '4' } else { '3' },
        st.vars[var].ty.code()
    );
    let mut warning = Warning::new(&code);
    warning.secondary = secondary;
    st.warn_var(warning, var);
}

fn warn_unused_value(st: &mut CheckState, value: ValueId, overwriting_node: Option<NodeId>) {
    let code = format!(
        "3{}{}",
        if st.values[value].mutated { '3' } else { '1' },
        st.values[value].ty.code()
    );
    let mut warning = Warning::new(&code);
    warning.secondary = is_secondary(st, value);
    if let Some(node) = overwriting_node {
        let range = st.ast.range(node);
        warning.overwritten_line = Some(range.line);
        warning.overwritten_column = Some(st.offset_to_column(range.line, range.offset));
        warning.overwritten_end_column = Some(st.offset_to_column(range.line, range.end_offset));
    }
    st.warn_value(warning, value);
}

fn is_function_var(st: &CheckState, var: VarId) -> bool {
    let values = &st.vars[var].values;
    (values.len() == 1 && st.values[values[0]].ty == VarType::Func)
        || (values.len() == 2
            && st.values[values[0]].empty
            && st.values[values[1]].ty == VarType::Func)
}

fn is_externally_accessible(st: &CheckState, value: ValueId) -> bool {
    let v = &st.values[value];
    v.ty != VarType::Var
        || v.node.is_some_and(|node| {
            matches!(
                st.ast.tag(node),
                Some(
                    Tag::Id
                        | Tag::Index
                        | Tag::Call
                        | Tag::Invoke
                        | Tag::Op
                        | Tag::Paren
                        | Tag::Dots
                )
            )
        })
}

fn lhs_nodes(st: &CheckState, item: ItemId) -> Vec<NodeId> {
    st.items[item]
        .lhs
        .map(|lhs| st.ast.kids(lhs))
        .unwrap_or_default()
}

fn get_overwriting_lhs_node(st: &CheckState, item: ItemId, value: ValueId) -> Option<NodeId> {
    let var = st.values[value].var;
    lhs_nodes(st, item)
        .into_iter()
        .find(|&node| st.ast.nodes[node].var == Some(var))
}

fn get_second_overwriting_lhs_node(
    st: &CheckState,
    item: ItemId,
    value: ValueId,
) -> Option<NodeId> {
    let var = st.values[value].var;
    let mut after_value_node = false;
    for node in lhs_nodes(st, item) {
        if st.ast.nodes[node].var == Some(var) {
            if after_value_node {
                return Some(node);
            } else if node == st.values[value].var_node {
                after_value_node = true;
            }
        }
    }
    None
}

fn detect_unused_local(st: &mut CheckState, var: VarId) {
    let values = st.vars[var].values.clone();
    if is_function_var(st, var) {
        let value = *values.get(1).unwrap_or(&values[0]);
        if !st.values[value].used {
            warn_unused_var(st, value, false);
        }
    } else if values.len() == 1 {
        let value = values[0];
        let v = &st.values[value];
        if st.vars[var].hint_unused && st.vars[var].name != b"_ENV" {
            if v.used {
                st.warn_var(Warning::new("214"), var);
            }
        } else if !v.used {
            if v.mutated {
                if !is_externally_accessible(st, value) {
                    warn_unaccessed_var(st, var, true);
                }
            } else {
                let empty = v.empty;
                warn_unused_var(st, value, empty);
            }
        } else if v.empty {
            st.warn_var(Warning::new("221"), var);
        }
    } else if !st.vars[var].accessed && !st.vars[var].mutated {
        warn_unaccessed_var(st, var, false);
    } else {
        let no_values_externally_accessible =
            !values.iter().any(|&v| is_externally_accessible(st, v));
        if !st.vars[var].accessed && no_values_externally_accessible {
            warn_unaccessed_var(st, var, true);
        }
        for &value in &values {
            let v = &st.values[value];
            if v.empty || v.used {
                continue;
            }
            if !v.mutated {
                let overwriting_node = match v.overwriting_item {
                    Overwriting::Item(overwriting_item) => {
                        if overwriting_item == v.item {
                            None
                        } else {
                            get_overwriting_lhs_node(st, overwriting_item, value)
                        }
                    }
                    Overwriting::Nil | Overwriting::False => {
                        get_second_overwriting_lhs_node(st, v.item, value)
                    }
                };
                warn_unused_value(st, value, overwriting_node);
            } else if !is_externally_accessible(st, value)
                && (st.vars[var].accessed || !no_values_externally_accessible)
            {
                warn_unused_value(st, value, None);
            }
        }
    }
}

fn detect_unused_locals(st: &mut CheckState) {
    for line in st.all_lines.clone() {
        for item in st.lines[line].items.clone() {
            if st.items[item].tag != ItemTag::Local {
                continue;
            }
            let vars: Vec<VarId> = st.items[item]
                .set_variables
                .as_ref()
                .map(|sv| sv.keys().copied().collect())
                .unwrap_or_default();
            for var in vars {
                // The implicit top level vararg has no location.
                if st.ast.nodes[st.vars[var].node].range.is_some() {
                    detect_unused_local(st, var);
                }
            }
        }
    }
}

type Edges = BTreeMap<LineId, BTreeSet<LineId>>;

fn mark_reachable_lines(
    edges: &Edges,
    marked: &mut BTreeSet<LineId>,
    base: &BTreeSet<LineId>,
    line: LineId,
) {
    let mut stack = vec![line];
    while let Some(line) = stack.pop() {
        for &connected in edges.get(&line).into_iter().flatten() {
            if !marked.contains(&connected) && !base.contains(&connected) {
                marked.insert(connected);
                stack.push(connected);
            }
        }
    }
}

fn detect_unused_rec_funcs(st: &mut CheckState) {
    let line = st.top_line;
    let nested_lines = st.lines[line].lines.clone();
    let mut forward_edges: Edges = BTreeMap::new();
    let mut backward_edges: Edges = BTreeMap::new();
    forward_edges.insert(line, BTreeSet::new());
    backward_edges.insert(line, BTreeSet::new());
    for &nested in &nested_lines {
        forward_edges.insert(nested, BTreeSet::new());
        backward_edges.insert(nested, BTreeSet::new());
    }
    for &nested in &nested_lines {
        let node = st.lines[nested].node;
        if let Some(value) = st.ast.nodes[node].value {
            for &using_line in &st.values[value].using_lines {
                forward_edges.entry(using_line).or_default().insert(nested);
                backward_edges.entry(nested).or_default().insert(using_line);
            }
        } else if let Some(parent) = st.lines[nested].parent {
            forward_edges.entry(parent).or_default().insert(nested);
            backward_edges.entry(nested).or_default().insert(parent);
        }
    }

    let empty = BTreeSet::new();
    let mut marked: BTreeSet<LineId> = BTreeSet::new();
    marked.insert(line);
    let mut newly = BTreeSet::new();
    mark_reachable_lines(&forward_edges, &mut newly, &marked, line);
    marked.extend(newly);

    for &nested in &nested_lines {
        let node = st.lines[nested].node;
        if let Some(value) = st.ast.nodes[node].value
            && !st.values[value].used
        {
            marked.insert(nested);
            let mut newly = BTreeSet::new();
            mark_reachable_lines(&forward_edges, &mut newly, &marked, nested);
            marked.extend(newly);
        }
    }

    for &nested in &nested_lines {
        let node = st.lines[nested].node;
        let Some(value) = st.ast.nodes[node].value else {
            continue;
        };
        if !st.values[value].used || marked.contains(&nested) {
            continue;
        }
        let mut forward_marked = BTreeSet::new();
        let mut backward_marked = BTreeSet::new();
        mark_reachable_lines(&forward_edges, &mut forward_marked, &marked, nested);
        mark_reachable_lines(&backward_edges, &mut backward_marked, &marked, nested);
        for &mut_rec_line in &forward_marked {
            if !backward_marked.contains(&mut_rec_line) {
                continue;
            }
            marked.insert(mut_rec_line);
            let node = st.lines[mut_rec_line].node;
            let Some(value) = st.ast.nodes[node].value else {
                continue;
            };
            let is_self_recursive = forward_edges
                .get(&mut_rec_line)
                .unwrap_or(&empty)
                .contains(&mut_rec_line);
            let var = st.values[value].var;
            if is_function_var(st, var) {
                let mut warning = Warning::new("211");
                warning.func = true;
                warning.mutually_recursive = !is_self_recursive;
                warning.recursive = is_self_recursive;
                st.warn_value(warning, value);
            } else {
                st.warn_value(Warning::new("311"), value);
            }
        }
    }
}

pub fn run(st: &mut CheckState) {
    detect_unused_locals(st);
    detect_unused_rec_funcs(st);
}

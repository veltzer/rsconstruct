//! luacheck's `resolve_locals` stage: connects assignments of local
//! variables with the accesses they can reach, through the flow graph of
//! each line and through closures, and records which assignment overwrites
//! a value unavoidably.

use std::collections::BTreeSet;

use super::ast::{NodeId, Tag};
use super::check::{CheckState, ItemId, ItemTag, LineId, Overwriting, ValueId, VarId};

fn add_resolution(
    st: &mut CheckState,
    line: LineId,
    item: ItemId,
    var: VarId,
    value: ValueId,
    is_mutation: bool,
) {
    st.items[item]
        .used_values
        .as_mut()
        .expect("item with used values")
        .entry(var)
        .or_default()
        .push(value);
    let v = &mut st.values[value];
    if is_mutation {
        v.mutated = true;
    } else {
        v.used = true;
    }
    v.using_lines.insert(line);
    if let Some(sec) = v.secondaries {
        st.secondaries[sec].used = true;
    }
}

fn add_resolutions(
    st: &mut CheckState,
    line: LineId,
    items: Option<Vec<ItemId>>,
    var: VarId,
    value: ValueId,
    is_mutation: bool,
) {
    for item in items.unwrap_or_default() {
        add_resolution(st, line, item, var, value, is_mutation);
    }
}

fn setting_value(st: &CheckState, setting_item: ItemId, var: VarId) -> ValueId {
    st.items[setting_item]
        .set_variables
        .as_ref()
        .expect("set variables")[&var]
}

fn cross_resolve_closures(st: &mut CheckState, access_line: LineId, set_line: LineId) {
    let set_upvalues: Vec<(VarId, Vec<ItemId>)> = st.lines[set_line]
        .set_upvalues
        .iter()
        .map(|(v, items)| (*v, items.clone()))
        .collect();
    for (var, setting_items) in set_upvalues {
        for setting_item in setting_items {
            let value = setting_value(st, setting_item, var);
            let accessed = st.lines[access_line].accessed_upvalues.get(&var).cloned();
            add_resolutions(st, access_line, accessed, var, value, false);
            let mutated = st.lines[access_line].mutated_upvalues.get(&var).cloned();
            add_resolutions(st, access_line, mutated, var, value, true);
        }
    }
}

fn in_scope(st: &CheckState, var: VarId, index: usize) -> bool {
    st.vars[var].scope_start <= index && index <= st.vars[var].scope_end
}

fn contains_call(st: &mut CheckState, node: NodeId) -> bool {
    if let Some(cached) = st.ast.nodes[node].contains_call {
        return cached;
    }
    if matches!(st.ast.tag(node), Some(Tag::Call | Tag::Invoke)) {
        st.ast.nodes[node].contains_call = Some(true);
        return true;
    }
    if !st.ast.is(node, Tag::Function) {
        for sub in st.ast.kids(node) {
            if contains_call(st, sub) {
                st.ast.nodes[node].contains_call = Some(true);
                return true;
            }
        }
    }
    st.ast.nodes[node].contains_call = Some(false);
    false
}

fn is_circular_reference(st: &mut CheckState, item: ItemId, var: VarId) -> bool {
    let it = &st.items[item];
    if !matches!(it.tag, ItemTag::Set | ItemTag::Local) {
        return false;
    }
    let (Some(lhs), Some(rhs)) = (it.lhs, it.rhs) else {
        return false;
    };
    if st.ast.len(lhs) != 1 || st.ast.len(rhs) != 1 {
        return false;
    }
    let right = st.ast.kid(rhs, 1);
    if contains_call(st, right) {
        return false;
    }
    let left = st.ast.kid(lhs, 1);
    if contains_call(st, left) {
        return false;
    }
    // `left_assignment[1]`: an Index's base node; an Id's name (a string,
    // whose `.var` is nil).
    match st.ast.get(left, 1) {
        Some(base) => st.ast.nodes[base].var == Some(var),
        None => false,
    }
}

fn propagate_main_assignments(st: &mut CheckState, line: LineId) {
    let items = st.lines[line].items.clone();
    for (i, &item) in items.iter().enumerate() {
        let Some(set_variables) = st.items[item].set_variables.clone() else {
            continue;
        };
        for (var, value) in set_variables {
            if st.vars[var].line != line {
                continue;
            }
            let mut visited = BTreeSet::new();
            let mut actions: Vec<Action> = Vec::new();
            // Walk with a read-only callback that records the effects, then
            // apply them in order: the walk itself only reads the graph.
            let mut overwriting = st.values[value].overwriting_item;
            st.walk(line, &mut visited, i + 2, &mut |st, index, item| {
                if !in_scope(st, var, index) {
                    overwriting = Overwriting::False;
                    return true;
                }
                let item = item.expect("in-scope item");
                actions.push(Action::Reach(item));
                let it = &st.items[item];
                if it
                    .set_variables
                    .as_ref()
                    .is_some_and(|sv| sv.contains_key(&var))
                {
                    if overwriting != Overwriting::False {
                        overwriting = match overwriting {
                            Overwriting::Item(prev) if prev != item => Overwriting::False,
                            _ => Overwriting::Item(item),
                        };
                    }
                    return true;
                }
                false
            });
            st.values[value].overwriting_item = overwriting;
            for action in actions {
                let Action::Reach(item) = action;
                apply_main_assignment(st, line, item, var, value);
            }
        }
    }
}

enum Action {
    Reach(ItemId),
}

/// The effects of a main assignment reaching an item.
fn apply_main_assignment(
    st: &mut CheckState,
    line: LineId,
    item: ItemId,
    var: VarId,
    value: ValueId,
) {
    if st.items[item]
        .accesses
        .as_ref()
        .is_some_and(|a| a.contains_key(&var))
        && !is_circular_reference(st, item, var)
    {
        add_resolution(st, line, item, var, value, false);
    }
    if st.items[item]
        .mutations
        .as_ref()
        .is_some_and(|m| m.contains_key(&var))
    {
        add_resolution(st, line, item, var, value, true);
    }
    if let Some(created_lines) = st.items[item].lines.clone() {
        for created_line in created_lines {
            if !is_circular_reference(st, item, var) {
                let accessed = st.lines[created_line].accessed_upvalues.get(&var).cloned();
                add_resolutions(st, created_line, accessed, var, value, false);
            }
            let mutated = st.lines[created_line].mutated_upvalues.get(&var).cloned();
            add_resolutions(st, created_line, mutated, var, value, true);
        }
    }
}

/// The effects of a closure creation reaching an item.
fn apply_closure_creation(
    st: &mut CheckState,
    line: LineId,
    item: ItemId,
    propagated_line: LineId,
) {
    if let Some(set_variables) = st.items[item].set_variables.clone() {
        for (var, value) in set_variables {
            let accessed = st.lines[propagated_line]
                .accessed_upvalues
                .get(&var)
                .cloned();
            add_resolutions(st, propagated_line, accessed, var, value, false);
            let mutated = st.lines[propagated_line]
                .mutated_upvalues
                .get(&var)
                .cloned();
            add_resolutions(st, propagated_line, mutated, var, value, true);
        }
    }
    if let Some(created_lines) = st.items[item].lines.clone() {
        for created_line in created_lines {
            cross_resolve_closures(st, propagated_line, created_line);
            cross_resolve_closures(st, created_line, propagated_line);
        }
    }
    let set_upvalues: Vec<(VarId, Vec<ItemId>)> = st.lines[propagated_line]
        .set_upvalues
        .iter()
        .map(|(v, items)| (*v, items.clone()))
        .collect();
    for (var, setting_items) in set_upvalues {
        if st.items[item]
            .accesses
            .as_ref()
            .is_some_and(|a| a.contains_key(&var))
        {
            for &setting_item in &setting_items {
                let value = setting_value(st, setting_item, var);
                add_resolution(st, line, item, var, value, false);
            }
        }
        if st.items[item]
            .mutations
            .as_ref()
            .is_some_and(|m| m.contains_key(&var))
        {
            for &setting_item in &setting_items {
                let value = setting_value(st, setting_item, var);
                add_resolution(st, line, item, var, value, true);
            }
        }
    }
}

fn propagate_closure_creations(st: &mut CheckState, line: LineId) {
    let items = st.lines[line].items.clone();
    for (i, &item) in items.iter().enumerate() {
        let Some(created_lines) = st.items[item].lines.clone() else {
            continue;
        };
        for created_line in created_lines {
            let mut visited = BTreeSet::new();
            let mut reached = Vec::new();
            st.walk(line, &mut visited, i + 1, &mut |_, _, item| match item {
                None => true,
                Some(item) => {
                    reached.push(item);
                    false
                }
            });
            for item in reached {
                apply_closure_creation(st, line, item, created_line);
            }
        }
    }
}

pub fn run(st: &mut CheckState) {
    for line in st.all_lines.clone() {
        propagate_main_assignments(st, line);
        propagate_closure_creations(st, line);
    }
}

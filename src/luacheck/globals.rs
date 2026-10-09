//! luacheck's `detect_globals` stage: assignments, accesses and mutations
//! of globals and their fields, traced through localizing assignments such
//! as `local t = table`.

use super::ast::{NodeId, Resolution, Tag};
use super::check::{CheckState, IndexKey, ItemId, ItemTag, Warning};

const fn resolved_to_index(resolution: &Resolution) -> bool {
    matches!(resolution, Resolution::Index { .. })
}

const fn resolution_len(resolution: &Resolution) -> usize {
    match resolution {
        Resolution::Index { keys, .. } => 1 + keys.len(),
        _ => 0,
    }
}

fn resolve_node(st: &mut CheckState, node: NodeId, item: ItemId) -> Resolution {
    match st.ast.tag(node) {
        Some(Tag::Id | Tag::Index) => {
            deep_resolve(st, node, item);
            st.ast.nodes[node].resolution.clone().expect("resolved")
        }
        Some(Tag::String) => Resolution::Str(node),
        Some(Tag::Nil | Tag::True | Tag::False | Tag::Number | Tag::Table | Tag::Function) => {
            Resolution::NotString
        }
        _ => Resolution::Unknown,
    }
}

/// `deep_resolve`: sets `node.resolution` for an `Id`, `Index` or `Invoke`.
fn deep_resolve(st: &mut CheckState, node: NodeId, item: ItemId) {
    if st.ast.nodes[node].resolution.is_some() {
        return;
    }
    st.ast.nodes[node].resolution = Some(Resolution::Unknown);

    let mut base = node;
    let mut base_is_index = !st.ast.is(node, Tag::Id);
    let mut keys = Vec::new();
    while base_is_index {
        keys.insert(0, st.ast.kid(base, 2));
        base = st.ast.kid(base, 1);
        base_is_index = st.ast.is(base, Tag::Index);
    }
    if !st.ast.is(base, Tag::Id) {
        return;
    }

    let mut previous_indexing_len = None;
    let base_resolution = if let Some(var) = st.ast.nodes[base].var {
        let used = st.items[item]
            .used_values
            .as_ref()
            .and_then(|u| u.get(&var));
        let Some(used) = used else { return };
        if used.len() != 1 {
            return;
        }
        let value = used[0];
        let Some(value_node) = st.values[value].node else {
            return;
        };
        let value_item = st.values[value].item;
        let resolution = resolve_node(st, value_node, value_item);
        if resolved_to_index(&resolution) {
            previous_indexing_len = Some(resolution_len(&resolution));
        }
        resolution
    } else {
        Resolution::Index {
            keys: Vec::new(),
            global: base,
            previous_indexing_len: None,
        }
    };

    if keys.is_empty() {
        st.ast.nodes[node].resolution = Some(base_resolution);
    } else if let Resolution::Index {
        keys: base_keys,
        global,
        ..
    } = base_resolution
    {
        let mut resolution_keys = base_keys;
        for key in keys {
            let key_resolution = resolve_node(st, key, item);
            resolution_keys.push(if resolved_to_index(&key_resolution) {
                Resolution::Unknown
            } else {
                key_resolution
            });
        }
        st.ast.nodes[node].resolution = Some(Resolution::Index {
            keys: resolution_keys,
            global,
            previous_indexing_len,
        });
    } else {
        st.ast.nodes[node].resolution = Some(Resolution::Unknown);
    }
}

fn warn_global(
    st: &mut CheckState,
    node: NodeId,
    resolution: &Resolution,
    is_lhs: bool,
    is_top_line: bool,
) {
    let Resolution::Index {
        keys,
        global,
        previous_indexing_len,
    } = resolution
    else {
        return;
    };
    let action = if is_lhs {
        if keys.is_empty() { '1' } else { '2' }
    } else {
        '3'
    };
    let mut warning = Warning::new(&format!("11{action}"));
    if !keys.is_empty() {
        warning.indexing = Some(
            keys.iter()
                .map(|key| match key {
                    Resolution::Unknown => IndexKey::Unknown,
                    Resolution::NotString => IndexKey::NotString,
                    Resolution::Str(node) => {
                        IndexKey::Str(st.ast.str(*node, 1).unwrap_or_default().to_vec())
                    }
                    Resolution::Index { .. } => IndexKey::Unknown,
                })
                .collect(),
        );
    }
    warning.name = st.ast.str(*global, 1).map(<[u8]>::to_vec);
    warning.previous_indexing_len = *previous_indexing_len;
    warning.top = is_top_line && action == '1';
    warning.indirect = node != *global;
    st.warn_node(warning, node);
}

fn detect_in_node(
    st: &mut CheckState,
    item: ItemId,
    node: NodeId,
    is_top_line: bool,
    is_lhs: bool,
) {
    let tag = st.ast.tag(node);
    if matches!(tag, Some(Tag::Index | Tag::Invoke | Tag::Id)) {
        if tag == Some(Tag::Id) && st.ast.nodes[node].var.is_some() {
            return;
        }
        deep_resolve(st, node, item);
        let resolution = st.ast.nodes[node].resolution.clone().expect("resolved");
        let mut node = node;
        if tag == Some(Tag::Invoke) {
            for i in 3..=st.ast.len(node) {
                if let Some(arg) = st.ast.get(node, i) {
                    detect_in_node(st, item, arg, is_top_line, false);
                }
            }
        }
        if tag != Some(Tag::Id) {
            loop {
                let key = st.ast.kid(node, 2);
                detect_in_node(st, item, key, is_top_line, false);
                node = st.ast.kid(node, 1);
                if !st.ast.is(node, Tag::Index) {
                    break;
                }
            }
            if !st.ast.is(node, Tag::Id) {
                detect_in_node(st, item, node, is_top_line, false);
            }
        }
        if resolved_to_index(&resolution) {
            warn_global(st, node, &resolution, is_lhs, is_top_line);
        }
    } else if tag != Some(Tag::Function) {
        for nested in st.ast.kids(node) {
            detect_in_node(st, item, nested, is_top_line, false);
        }
    }
}

pub fn run(st: &mut CheckState) {
    for line in st.all_lines.clone() {
        let is_top_line = line == st.top_line;
        for item in st.lines[line].items.clone() {
            let it = &st.items[item];
            match it.tag {
                ItemTag::Eval => {
                    let node = it.node.expect("eval node");
                    detect_in_node(st, item, node, is_top_line, false);
                }
                ItemTag::Local => {
                    if let Some(rhs) = it.rhs {
                        for node in st.ast.kids(rhs) {
                            detect_in_node(st, item, node, is_top_line, false);
                        }
                    }
                }
                ItemTag::Set | ItemTag::OpSet => {
                    let (lhs, rhs) = (it.lhs.expect("lhs"), it.rhs.expect("rhs"));
                    for node in st.ast.kids(lhs) {
                        detect_in_node(st, item, node, is_top_line, true);
                    }
                    for node in st.ast.kids(rhs) {
                        detect_in_node(st, item, node, is_top_line, false);
                    }
                }
                _ => {}
            }
        }
    }
}

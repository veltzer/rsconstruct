//! The smaller luacheck stages, each a port of its `stages/*.lua` module.

use std::collections::BTreeSet;

use super::ast::{Item, NodeId, Tag};
use super::check::{CheckState, ItemTag, LineId, Warning};
use super::decoder;
use super::pattern::Capture;

// ---------------------------------------------------------------------------
// unwrap_parens

const fn relational(op: &[u8]) -> Option<(&'static str, &'static str)> {
    Some(match op {
        b"ne" => ("~=", "=="),
        b"eq" => ("==", "~="),
        b"gt" => (">", "<="),
        b"ge" => (">=", "<"),
        b"lt" => ("<", ">="),
        b"le" => ("<=", ">"),
        _ => return None,
    })
}

fn unwrap_nodes(st: &mut CheckState, nodes: NodeId, list_start: Option<usize>) {
    let num_nodes = st.ast.len(nodes);
    for index in 1..=num_nodes {
        let Some(node) = st.ast.get(nodes, index) else {
            continue;
        };
        let tag = st.ast.tag(node);
        match tag {
            Some(Tag::Table | Tag::Return) => unwrap_nodes(st, node, Some(1)),
            Some(Tag::Call) => unwrap_nodes(st, node, Some(2)),
            Some(Tag::Invoke) => unwrap_nodes(st, node, Some(3)),
            Some(Tag::Forin) => {
                let (exprs, block) = (st.ast.kid(node, 2), st.ast.kid(node, 3));
                unwrap_nodes(st, exprs, Some(1));
                unwrap_nodes(st, block, None);
            }
            Some(Tag::Local) => {
                if let Some(rhs) = st.ast.get(node, 2) {
                    unwrap_nodes(st, rhs, None);
                }
            }
            Some(Tag::Set | Tag::OpSet) => {
                let (lhs, rhs) = (st.ast.kid(node, 1), st.ast.kid(node, 2));
                unwrap_nodes(st, lhs, None);
                unwrap_nodes(st, rhs, Some(1));
            }
            _ => {
                if tag == Some(Tag::Op)
                    && st.ast.str(node, 1).and_then(relational).is_some()
                    && let Some(operand) = st.ast.get(node, 2)
                    && st.ast.is(operand, Tag::Op)
                    && st.ast.str(operand, 1) == Some(b"not")
                {
                    st.warn_node(Warning::new("582"), node);
                }
                unwrap_nodes(st, node, None);
                if tag == Some(Tag::Op)
                    && st.ast.str(node, 1) == Some(b"not")
                    && let Some(operand) = st.ast.get(node, 2)
                    && st.ast.is(operand, Tag::Op)
                    && let Some((operator, replacement)) =
                        st.ast.str(operand, 1).and_then(relational)
                {
                    let mut warning = Warning::new("581");
                    warning.operator = Some(operator.as_bytes().to_vec());
                    warning.replacement_operator = Some(replacement.as_bytes().to_vec());
                    st.warn_node(warning, node);
                }
                if tag == Some(Tag::Paren)
                    && (list_start.is_none()
                        || index < list_start.unwrap_or(0)
                        || index != num_nodes)
                {
                    let inner = st.ast.kid(node, 1);
                    if !matches!(st.ast.tag(inner), Some(Tag::Call | Tag::Invoke | Tag::Dots)) {
                        st.ast.nodes[nodes].items[index - 1] = Item::Node(inner);
                    }
                }
            }
        }
    }
}

pub fn unwrap_parens(st: &mut CheckState) {
    unwrap_nodes(st, st.root, None);
}

// ---------------------------------------------------------------------------
// name_functions

fn index_name(st: &CheckState, base_name: &[u8], key: NodeId) -> Option<Vec<u8>> {
    if st.ast.is(key, Tag::String) {
        let mut name = base_name.to_vec();
        name.push(b'.');
        name.extend_from_slice(st.ast.str(key, 1).unwrap_or_default());
        return Some(name);
    }
    None
}

fn full_field_name(st: &CheckState, node: NodeId) -> Option<Vec<u8>> {
    match st.ast.tag(node) {
        Some(Tag::Id) => st.ast.str(node, 1).map(<[u8]>::to_vec),
        Some(Tag::Index) => {
            let base = full_field_name(st, st.ast.kid(node, 1))?;
            index_name(st, &base, st.ast.kid(node, 2))
        }
        _ => None,
    }
}

fn name_nodes(st: &mut CheckState, nodes: NodeId) {
    for node in st.ast.kids(nodes) {
        name_node(st, node, None);
    }
}

fn name_node(st: &mut CheckState, node: NodeId, name: Option<Vec<u8>>) {
    match st.ast.tag(node) {
        Some(Tag::Function) => {
            st.ast.nodes[node].name = name;
            name_nodes(st, st.ast.kid(node, 2));
        }
        Some(Tag::Set | Tag::Local | Tag::Localrec) => {
            let lhs = st.ast.kid(node, 1);
            if let Some(rhs) = st.ast.get(node, 2) {
                name_nodes(st, lhs);
                for (index, rhs_node) in st.ast.kids(rhs).into_iter().enumerate() {
                    let field_name = st
                        .ast
                        .get(lhs, index + 1)
                        .and_then(|lhs_node| full_field_name(st, lhs_node));
                    name_node(st, rhs_node, field_name);
                }
            }
        }
        Some(Tag::Table) if name.is_some() => {
            let name = name.expect("name");
            for pair in st.ast.kids(node) {
                if st.ast.is(pair, Tag::Pair) {
                    let (key, value) = (st.ast.kid(pair, 1), st.ast.kid(pair, 2));
                    name_node(st, key, None);
                    let value_name = index_name(st, &name, key);
                    name_node(st, value, value_name);
                } else {
                    name_node(st, pair, None);
                }
            }
        }
        _ => name_nodes(st, node),
    }
}

pub fn name_functions(st: &mut CheckState) {
    name_nodes(st, st.root);
}

// ---------------------------------------------------------------------------
// core_utils

/// Lua 5.1 `tonumber` on a numeric literal's text (C `strtod`, which reads
/// hexadecimal too).
pub fn lua_tonumber(text: &[u8]) -> Option<f64> {
    lua_str_tonumber(text)
}

/// Lua 5.1 `tonumber(s)` for a string: C `strtod` (leading whitespace,
/// sign, decimal or hexadecimal, `inf`/`nan`), trailing whitespace allowed.
pub fn lua_str_tonumber(text: &[u8]) -> Option<f64> {
    let s = std::str::from_utf8(text).ok()?;
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r');
    let trimmed = s.trim_start_matches(is_space).trim_end_matches(is_space);
    let (negative, body) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let value = if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        parse_hex_float(hex)?
    } else {
        let lower = body.to_ascii_lowercase();
        if matches!(lower.as_str(), "inf" | "infinity") {
            f64::INFINITY
        } else if lower == "nan" {
            f64::NAN
        } else {
            if body.is_empty()
                || !body
                    .bytes()
                    .all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'))
            {
                return None;
            }
            body.parse::<f64>().ok()?
        }
    };
    Some(if negative { -value } else { value })
}

#[allow(clippy::suboptimal_flops)] // Unfused, digit by digit: fusing changes rounding.
fn parse_hex_float(s: &str) -> Option<f64> {
    let (mantissa, exponent) = match s.find(['p', 'P']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (int_part, frac_part) = match mantissa.find('.') {
        Some(i) => (&mantissa[..i], &mantissa[i + 1..]),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let mut value = 0f64;
    for c in int_part.chars() {
        value = value * 16.0 + f64::from(c.to_digit(16)?);
    }
    let mut scale = 1.0 / 16.0;
    for c in frac_part.chars() {
        value += f64::from(c.to_digit(16)?) * scale;
        scale /= 16.0;
    }
    if let Some(exponent) = exponent {
        let e: i32 = exponent.parse().ok()?;
        value *= 2f64.powi(e);
    }
    Some(value)
}

/// A constant key of a table constructor.
#[derive(Clone, Debug)]
pub enum ConstValue {
    Bool(bool),
    Num(f64),
    Str(Vec<u8>),
}

impl PartialEq for ConstValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Num(a), Self::Num(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            _ => false,
        }
    }
}

/// `core_utils.eval_const_node`: a node's constant value and its
/// representation, without resolving locals.
pub fn eval_const_node(st: &CheckState, node: NodeId) -> Option<(ConstValue, Vec<u8>)> {
    match st.ast.tag(node) {
        Some(Tag::True) => Some((ConstValue::Bool(true), b"true".to_vec())),
        Some(Tag::False) => Some((ConstValue::Bool(false), b"false".to_vec())),
        Some(Tag::String) => {
            let value = st.ast.str(node, 1).unwrap_or_default().to_vec();
            let repr = decoder::printable(&value);
            Some((ConstValue::Str(value), repr))
        }
        _ => {
            let mut node = node;
            let mut is_negative = false;
            if st.ast.is(node, Tag::Op) && st.ast.str(node, 1) == Some(b"unm") {
                is_negative = true;
                node = st.ast.kid(node, 2);
            }
            if !st.ast.is(node, Tag::Number) {
                return None;
            }
            let text = st.ast.str(node, 1).unwrap_or_default();
            if text.iter().any(|c| b"iIuUlL".contains(c)) {
                return None;
            }
            let mut number = lua_tonumber(text)?;
            if is_negative {
                number = -number;
            }
            if number.is_finite() {
                let mut repr = if is_negative {
                    b"-".to_vec()
                } else {
                    Vec::new()
                };
                repr.extend_from_slice(text);
                return Some((ConstValue::Num(number), repr));
            }
            None
        }
    }
}

const fn is_statement_container(tag: Option<Tag>) -> bool {
    matches!(
        tag,
        None | Some(Tag::Do | Tag::While | Tag::Repeat | Tag::Fornum | Tag::Forin | Tag::If)
    )
}

fn scan_for_statements(st: &CheckState, items: NodeId, tags: &[Tag], found: &mut Vec<NodeId>) {
    for item in st.ast.kids(items) {
        let tag = st.ast.tag(item);
        if tag.is_some_and(|t| tags.contains(&t)) {
            found.push(item);
        }
        if is_statement_container(tag) {
            scan_for_statements(st, item, tags, found);
        }
    }
}

/// `core_utils.each_statement`: statement nodes with the given tags, line by
/// line, in source order within a line.
pub fn each_statement(st: &CheckState, tags: &[Tag]) -> Vec<NodeId> {
    let mut found = Vec::new();
    for &line in &st.all_lines {
        let body = st.ast.kid(st.lines[line].node, 2);
        scan_for_statements(st, body, tags, &mut found);
    }
    found
}

// ---------------------------------------------------------------------------
// detect_bad_whitespace

pub fn detect_bad_whitespace(st: &mut CheckState) {
    let num_lines = st.line_offsets.len() - 1;
    for line_number in 1..=num_lines {
        let line_offset = st.line_offsets[line_number].expect("line offset");
        let line_length = st.line_length(line_number).expect("line length");
        if line_length == 0 {
            continue;
        }
        let pattern: &[u8] = if line_number == num_lines {
            b"^[^\r\n]-()[ \t\x0c\x0b]+()[\r\n]?$"
        } else {
            b"^[^\r\n]-()[ \t\x0c\x0b]+()[\r\n]"
        };
        let found = st.source.find(pattern, line_offset).expect("valid pattern");
        let mut trailing_ws_code = None;
        if let Some(m) = found {
            let positions: Vec<usize> = m.captures.iter().filter_map(Capture::position).collect();
            let (trailing_ws_start_byte, line_end_byte) = (positions[0], positions[1]);
            trailing_ws_code = Some(if trailing_ws_start_byte == m.start {
                "611"
            } else {
                match st.line_endings.get(&line_number) {
                    None => "612",
                    Some(super::parser::LineEnding::String) => "613",
                    Some(super::parser::LineEnding::Comment) => "614",
                }
            });
            let trailing_ws_end_byte = line_end_byte - 1;
            let trailing_ws_end_char = line_offset + line_length - 1;
            let trailing_ws_start_char =
                trailing_ws_end_char - (trailing_ws_end_byte - trailing_ws_start_byte);
            let code = trailing_ws_code.expect("code");
            st.warn(
                Warning::new(code),
                line_number,
                trailing_ws_start_char,
                trailing_ws_end_char,
            );
        }
        if trailing_ws_code != Some("611") {
            let found = st
                .source
                .find(b"^[ \t\x0c\x0b]- \t[ \t\x0c\x0b]*", line_offset)
                .expect("valid pattern");
            if let Some(m) = found {
                let leading_ws_end_char = line_offset + (m.end - m.start);
                st.warn(
                    Warning::new("621"),
                    line_number,
                    line_offset,
                    leading_ws_end_char,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// detect_compound_operators

pub fn detect_compound_operators(st: &mut CheckState) {
    for node in each_statement(st, &[Tag::OpSet]) {
        let operator: &[u8] = match st.ast.str(node, 3).unwrap_or_default() {
            b"add" => b"+=",
            b"sub" => b"-=",
            b"mul" => b"*=",
            b"mod" => b"%=",
            b"pow" => b"^=",
            b"div" => b"/=",
            b"idiv" => b"//=",
            b"band" => b"&=",
            b"bor" => b"|=",
            b"bxor" => b"~=",
            b"shl" => b"<<=",
            b"shr" => b">>=",
            _ => b"..=",
        };
        let mut warning = Warning::new("033");
        warning.operator = Some(operator.to_vec());
        st.warn_node(warning, node);
    }
}

// ---------------------------------------------------------------------------
// detect_cyclomatic_complexity

fn cc_expr(st: &CheckState, node: NodeId, count: &mut usize) {
    if st.ast.is(node, Tag::Op) && matches!(st.ast.str(node, 1), Some(b"and" | b"or")) {
        *count += 1;
    }
    if !st.ast.is(node, Tag::Function) {
        cc_exprs(st, node, count);
    }
}

fn cc_exprs(st: &CheckState, exprs: NodeId, count: &mut usize) {
    for expr in st.ast.kids(exprs) {
        cc_expr(st, expr, count);
    }
}

fn cc_stmts(st: &CheckState, stmts: NodeId, count: &mut usize) {
    for stmt in st.ast.kids(stmts) {
        match st.ast.tag(stmt) {
            Some(Tag::If) => {
                let len = st.ast.len(stmt);
                let mut i = 1;
                while i < len {
                    *count += 1;
                    cc_stmts(st, st.ast.kid(stmt, i + 1), count);
                    i += 2;
                }
                if len % 2 == 1 {
                    cc_stmts(st, st.ast.kid(stmt, len), count);
                }
            }
            Some(Tag::While) => {
                *count += 1;
                cc_stmts(st, st.ast.kid(stmt, 2), count);
            }
            Some(Tag::Repeat) => {
                *count += 1;
                cc_stmts(st, st.ast.kid(stmt, 1), count);
            }
            Some(Tag::Forin) => {
                *count += 1;
                cc_stmts(st, st.ast.kid(stmt, 3), count);
            }
            Some(Tag::Fornum) => {
                *count += 1;
                let block = st.ast.get(stmt, 5).unwrap_or_else(|| st.ast.kid(stmt, 4));
                cc_stmts(st, block, count);
            }
            _ => {}
        }
    }
}

pub fn detect_cyclomatic_complexity(st: &mut CheckState) {
    for line in st.all_lines.clone() {
        let mut count = 1usize;
        let node = st.lines[line].node;
        cc_stmts(st, st.ast.kid(node, 2), &mut count);
        for &item in &st.lines[line].items {
            let it = &st.items[item];
            match it.tag {
                ItemTag::Eval => cc_expr(st, it.node.expect("eval node"), &mut count),
                ItemTag::Local | ItemTag::Set => {
                    if let Some(rhs) = it.rhs {
                        cc_exprs(st, rhs, &mut count);
                    }
                }
                _ => {}
            }
        }
        let mut warning = Warning::new("561");
        warning.complexity = Some(count);
        if line == st.top_line {
            warning.function_type = Some("main_chunk");
            st.warn(warning, 1, 1, 1);
        } else {
            let args = st.ast.kid(node, 1);
            let is_method = st
                .ast
                .get(args, 1)
                .is_some_and(|first| st.ast.nodes[first].implicit);
            warning.function_type = Some(if is_method { "method" } else { "function" });
            warning.function_name.clone_from(&st.ast.nodes[node].name);
            st.warn_node(warning, node);
        }
    }
}

// ---------------------------------------------------------------------------
// detect_empty_blocks, detect_empty_statements

pub fn detect_empty_blocks(st: &mut CheckState) {
    for node in each_statement(st, &[Tag::Do, Tag::If]) {
        if st.ast.is(node, Tag::Do) {
            if st.ast.len(node) == 0 {
                st.warn_node(Warning::new("541"), node);
            }
            continue;
        }
        let len = st.ast.len(node);
        let mut blocks = Vec::new();
        let mut index = 2;
        while index <= len {
            blocks.push(st.ast.kid(node, index));
            index += 2;
        }
        if len % 2 == 1 {
            blocks.push(st.ast.kid(node, len));
        }
        for block in blocks {
            if st.ast.len(block) == 0 {
                st.warn_node(Warning::new("542"), block);
            }
        }
    }
}

pub fn detect_empty_statements(st: &mut CheckState) {
    for range in st.useless_semicolons.clone() {
        st.warn_range(Warning::new("551"), range);
    }
}

// ---------------------------------------------------------------------------
// detect_reversed_fornum_loops

pub fn detect_reversed_fornum_loops(st: &mut CheckState) {
    for node in each_statement(st, &[Tag::Fornum]) {
        let from = st.ast.kid(node, 2);
        if !(st.ast.is(from, Tag::Op) && st.ast.str(from, 1) == Some(b"len")) {
            continue;
        }
        let Some((ConstValue::Num(limit), limit_repr)) = eval_const_node(st, st.ast.kid(node, 3))
        else {
            // A non-numeric constant compares with an error in Lua; luacheck
            // only gets numbers here in practice, other values are skipped.
            continue;
        };
        if limit > 1.0 {
            continue;
        }
        let step = if st.ast.get(node, 5).is_some() {
            match eval_const_node(st, st.ast.kid(node, 4)) {
                Some((ConstValue::Num(step), _)) => Some(step),
                Some(_) | None => None,
            }
        } else {
            Some(1.0)
        };
        if step.is_some_and(|step| step >= 0.0) {
            let mut warning = Warning::new("571");
            warning.limit = Some(limit_repr);
            st.warn_node(warning, node);
        }
    }
}

// ---------------------------------------------------------------------------
// detect_unbalanced_assignments

pub fn detect_unbalanced_assignments(st: &mut CheckState) {
    for node in each_statement(st, &[Tag::Set, Tag::Local]) {
        let Some(rhs) = st.ast.get(node, 2) else {
            continue;
        };
        let lhs = st.ast.kid(node, 1);
        let (nr, nl) = (st.ast.len(rhs), st.ast.len(lhs));
        if nr > nl {
            st.warn_node(Warning::new("531"), node);
        } else if nr < nl && st.ast.is(node, Tag::Set) {
            let last = st.ast.kid(rhs, nr);
            if !matches!(st.ast.tag(last), Some(Tag::Dots | Tag::Call | Tag::Invoke)) {
                st.warn_node(Warning::new("532"), node);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// detect_unreachable_code

pub fn detect_unreachable_code(st: &mut CheckState) {
    for line in st.all_lines.clone() {
        let mut reachable = BTreeSet::new();
        st.walk(line, &mut reachable, 1, &mut |_, _, _| false);
        let items: Vec<_> = st.lines[line].items.clone();
        for (i, &item) in items.iter().enumerate() {
            let index = i + 1;
            if reachable.contains(&index) {
                continue;
            }
            let Some(node) = st.items[item].node else {
                continue;
            };
            if st.ast.nodes[node].range.is_none() {
                continue;
            }
            let code = if st.items[item].loop_end {
                "512"
            } else {
                "511"
            };
            st.warn_node(Warning::new(code), node);
            st.walk(line, &mut reachable, index, &mut |_, _, _| false);
        }
    }
}

// ---------------------------------------------------------------------------
// detect_unused_fields

/// Lua 5.1 `tostring` of a number (`%.14g`).
pub fn lua_number_to_string(n: f64) -> String {
    if n.is_nan() {
        return if n.is_sign_negative() {
            "-nan".into()
        } else {
            "nan".into()
        };
    }
    if n.is_infinite() {
        return if n > 0.0 { "inf".into() } else { "-inf".into() };
    }
    format_g(n, 14)
}

/// C's `%.{precision}g`.
pub fn format_g(n: f64, precision: usize) -> String {
    if n == 0.0 {
        return if n.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    let exponent = format!("{:.*e}", precision - 1, n);
    let (mantissa, exp) = exponent.split_once('e').expect("exponent form");
    let exp: i32 = exp.parse().expect("exponent");
    let strip = |s: String| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    };
    if exp < -4 || exp >= precision as i32 {
        let mantissa = strip(mantissa.to_string());
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{mantissa}e{sign}{:02}", exp.abs())
    } else {
        let decimals = (precision as i32 - 1 - exp).max(0) as usize;
        strip(format!("{n:.decimals$}"))
    }
}

fn check_table(st: &mut CheckState, node: NodeId) {
    let mut array_index = 1.0f64;
    // (key value, key node), in assignment order; later entries replace.
    let mut key_value_to_node: Vec<(ConstValue, NodeId)> = Vec::new();
    let mut key_node_to_repr: Vec<(NodeId, Vec<u8>)> = Vec::new();
    let mut index_key_nodes: Vec<NodeId> = Vec::new();
    for pair in st.ast.kids(node) {
        let key_node;
        let key: Option<(ConstValue, Vec<u8>)>;
        let is_pair = st.ast.is(pair, Tag::Pair);
        if is_pair {
            key_node = st.ast.kid(pair, 1);
            key = eval_const_node(st, key_node);
        } else {
            key_node = pair;
            key = Some((
                ConstValue::Num(array_index),
                lua_number_to_string(array_index.floor()).into_bytes(),
            ));
            array_index += 1.0;
        }
        let Some((key_value, key_repr)) = key else {
            continue;
        };
        if key_value == ConstValue::Bool(false) {
            continue;
        }
        let prev_key_node = key_value_to_node
            .iter()
            .find(|(v, _)| *v == key_value)
            .map(|(_, n)| *n);
        if let Some(prev_key_node) = prev_key_node {
            let prev_key_repr = key_node_to_repr
                .iter()
                .rev()
                .find(|(n, _)| *n == prev_key_node)
                .map(|(_, r)| r.clone())
                .unwrap_or_default();
            let prev_key_is_index = index_key_nodes.contains(&prev_key_node);
            let overwriting = st.ast.range(key_node);
            let mut warning = Warning::new("314");
            warning.field = Some(prev_key_repr);
            warning.index = prev_key_is_index;
            warning.overwritten_line = Some(overwriting.line);
            warning.overwritten_column =
                Some(st.offset_to_column(overwriting.line, overwriting.offset));
            warning.overwritten_end_column =
                Some(st.offset_to_column(overwriting.line, overwriting.end_offset));
            st.warn_node(warning, prev_key_node);
        }
        match key_value_to_node.iter_mut().find(|(v, _)| *v == key_value) {
            Some(entry) => entry.1 = key_node,
            None => key_value_to_node.push((key_value, key_node)),
        }
        key_node_to_repr.push((key_node, key_repr));
        if !is_pair {
            index_key_nodes.push(key_node);
        }
    }
}

fn check_nodes(st: &mut CheckState, nodes: NodeId) {
    for node in st.ast.kids(nodes) {
        if st.ast.is(node, Tag::Table) {
            check_table(st, node);
        }
        check_nodes(st, node);
    }
}

pub fn detect_unused_fields(st: &mut CheckState) {
    check_nodes(st, st.root);
}

// ---------------------------------------------------------------------------
// detect_uninit_accesses

pub fn detect_uninit_accesses(st: &mut CheckState) {
    let lines: Vec<LineId> = st.all_lines.clone();
    for line in lines {
        for item in st.lines[line].items.clone() {
            for (map, code) in [(false, "321"), (true, "341")] {
                let it = &st.items[item];
                let var_map = if map {
                    it.mutations.clone()
                } else {
                    it.accesses.clone()
                };
                let Some(var_map) = var_map else { continue };
                for (var, accessing_nodes) in var_map {
                    let Some(used) = st.items[item]
                        .used_values
                        .as_ref()
                        .and_then(|u| u.get(&var))
                        .cloned()
                    else {
                        continue;
                    };
                    let values = &st.vars[var].values;
                    if values.len() == 1 && st.values[values[0]].empty {
                        continue;
                    }
                    if used.iter().all(|&v| st.values[v].empty) {
                        for node in accessing_nodes {
                            let mut warning = Warning::new(code);
                            warning.name = st.ast.str(node, 1).map(<[u8]>::to_vec);
                            st.warn_node(warning, node);
                        }
                    }
                }
            }
        }
    }
}

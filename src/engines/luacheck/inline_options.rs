//! luacheck's `parse_inline_options` stage: `-- luacheck: ...` comments,
//! `push`/`pop` directives and the implicit option scope of each function,
//! turned into a line-ordered list of option pushes and pop counts.

use std::rc::Rc;

use super::check::{CheckState, Warning};
use super::options::{NULLARY_INLINE_OPTIONS, VARIADIC_INLINE_OPTIONS, split, strip};
use super::pattern;
use super::stages::lua_str_tonumber;
use super::value::{LKey, LTable, LVal};

const LIMIT_OPTS: &[&str] = &[
    "max_line_length",
    "max_code_line_length",
    "max_string_line_length",
    "max_comment_line_length",
    "max_cyclomatic_complexity",
];

#[derive(Clone, Debug)]
pub enum InlineOpts {
    Table(Rc<LTable>),
    Push,
    Pop,
}

/// An entry of `chstate.inline_options`: a pop count and/or an option table
/// pushed before the warnings of `line` are processed.
#[derive(Clone, Debug)]
pub struct InlineOption {
    pub line: usize,
    pub column: Option<usize>,
    pub end_column: Option<usize>,
    pub options: Option<InlineOpts>,
    pub pop_count: Option<usize>,
}

fn is_variadic(name: &str) -> bool {
    VARIADIC_INLINE_OPTIONS.iter().any(|(n, _)| *n == name)
}

fn is_valid_option_name(name: &str) -> bool {
    if name == "std" || is_variadic(name) {
        return true;
    }
    let name = name.strip_prefix("no_").unwrap_or(name);
    NULLARY_INLINE_OPTIONS.contains(&name) || LIMIT_OPTS.contains(&name)
}

/// The longest prefix of the tokens that names an option, and the rest.
fn split_invocation(tokens: &[Vec<u8>]) -> Option<(String, Vec<Vec<u8>>)> {
    let mut cur_name: Option<Vec<u8>> = None;
    let mut last_valid: Option<(String, usize)> = None;
    for (i, token) in tokens.iter().enumerate() {
        let name = match cur_name.take() {
            Some(mut n) => {
                n.push(b'_');
                n.extend_from_slice(token);
                n
            }
            None => token.clone(),
        };
        if let Ok(s) = std::str::from_utf8(&name)
            && is_valid_option_name(s)
        {
            last_valid = Some((s.to_string(), i + 1));
        }
        cur_name = Some(name);
    }
    let (name, end) = last_valid?;
    Some((name, tokens[end..].to_vec()))
}

fn unexpected_num_args(name: &str, args: &[Vec<u8>], expected: usize) -> String {
    format!(
        "inline option '{name}' expects {expected} argument{}, {} given",
        if expected == 1 { "" } else { "s" },
        args.len()
    )
}

fn set(opts: &mut Vec<(LKey, LVal)>, key: &str, value: LVal) {
    let key = LKey::Str(key.as_bytes().to_vec());
    match opts.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 = value,
        None => opts.push((key, value)),
    }
}

fn strings(args: &[Vec<u8>]) -> LVal {
    LVal::table(
        args.iter().map(|a| LVal::Str(a.clone())).collect(),
        Vec::new(),
    )
}

fn parse_options(body: &[u8]) -> Result<Rc<LTable>, String> {
    let mut opts: Vec<(LKey, LVal)> = Vec::new();
    let parts = split(body, Some(b','));
    for name_and_args in &parts {
        let tokens = split(name_and_args, None);
        let Some((name, args)) = split_invocation(&tokens) else {
            if tokens.is_empty() {
                return Err(if parts.len() == 1 {
                    "empty inline option".into()
                } else {
                    "empty inline option invocation".into()
                });
            }
            let joined: Vec<String> = tokens
                .iter()
                .map(|t| String::from_utf8_lossy(t).into_owned())
                .collect();
            return Err(format!("unknown inline option '{}'", joined.join(" ")));
        };
        if name == "std" {
            if args.len() != 1 {
                return Err(unexpected_num_args(&name, &args, 1));
            }
            set(&mut opts, "std", LVal::Str(args[0].clone()));
        } else if name == "ignore" && args.is_empty() {
            set(
                &mut opts,
                "ignore",
                LVal::table(vec![LVal::str(".*")], Vec::new()),
            );
        } else if is_variadic(&name) {
            set(&mut opts, &name, strings(&args));
        } else {
            let full_name = name.replace('_', " ");
            let (name, flag) = match name.strip_prefix("no_") {
                Some(rest) => (rest.to_string(), false),
                None => (name.clone(), true),
            };
            if NULLARY_INLINE_OPTIONS.contains(&name.as_str()) {
                if !args.is_empty() {
                    return Err(unexpected_num_args(&full_name, &args, 0));
                }
                set(&mut opts, &name, LVal::Bool(flag));
            } else if flag {
                if args.len() != 1 {
                    return Err(unexpected_num_args(&full_name, &args, 1));
                }
                let Some(value) = lua_str_tonumber(&args[0]) else {
                    return Err(format!("inline option '{name}' expects number as argument"));
                };
                set(&mut opts, &name, LVal::Num(value));
            } else {
                if !args.is_empty() {
                    return Err(unexpected_num_args(&full_name, &args, 0));
                }
                set(&mut opts, &name, LVal::Bool(false));
            }
        }
    }
    Ok(Rc::new(LTable::new(Vec::new(), opts)))
}

enum Parsed {
    NotInline,
    Options(InlineOpts, Option<InlineOpts>),
    Error(String),
}

fn parse_inline_comment(contents: &[u8]) -> Parsed {
    let stripped = strip(contents);
    let Some(m) = pattern::find(stripped, b"^luacheck:", 1).expect("valid pattern") else {
        return Parsed::NotInline;
    };
    let body = &stripped[m.end..];
    let (without_parens, _) =
        pattern::gsub(body, b"%b()", |_| Some(b" ".to_vec())).expect("valid pattern");
    let body = strip(&without_parens).to_vec();
    let mut opts2 = None;
    let mut body_ref: &[u8] = &body;
    let after_push = pattern::lua_match(&body, b"^push%s+(.*)", 1).expect("valid pattern");
    let after_push_bytes;
    if let Some(caps) = after_push {
        after_push_bytes = caps[0].bytes(&body).to_vec();
        opts2 = Some(InlineOpts::Push);
        body_ref = &after_push_bytes;
    } else if body == b"push" {
        return Parsed::Options(InlineOpts::Push, None);
    } else if body == b"pop" {
        return Parsed::Options(InlineOpts::Pop, None);
    }
    match parse_options(body_ref) {
        Ok(opts) => Parsed::Options(InlineOpts::Table(opts), opts2),
        Err(err) => Parsed::Error(err),
    }
}

const fn order(t: &InlineOption) -> u8 {
    match t.options {
        Some(InlineOpts::Push) => 1,
        Some(InlineOpts::Pop) => 3,
        _ => 2,
    }
}

fn warn_column_range(st: &mut CheckState, code: &str, item: &InlineOption) {
    let mut warning = Warning::new(code);
    warning.line = item.line;
    warning.column = item.column.unwrap_or(0);
    warning.end_column = item.end_column.unwrap_or(0);
    st.warnings.push(warning);
}

fn apply_boundaries(st: &mut CheckState, items: Vec<InlineOption>) -> Vec<InlineOption> {
    let mut res: Vec<InlineOption> = Vec::new();
    // Index into `res` of the last table, if any.
    let mut res_last: Option<usize> = None;
    let mut pushes: Vec<InlineOption> = Vec::new();
    let mut push_option_counts: Vec<usize> = Vec::new();
    let mut option_count = 0usize;

    for item in items {
        match item.options {
            Some(InlineOpts::Push) => {
                pushes.push(item);
                push_option_counts.push(option_count);
            }
            Some(InlineOpts::Pop) => {
                let top_is_function = pushes.last().is_some_and(|p| p.end_column.is_none());
                if pushes.is_empty() || (item.end_column.is_some() && top_is_function) {
                    warn_column_range(st, "023", &item);
                    continue;
                }
                if item.end_column.is_none() {
                    while pushes.last().is_some_and(|p| p.end_column.is_some()) {
                        let top = pushes.pop().expect("push");
                        warn_column_range(st, "022", &top);
                        push_option_counts.pop();
                    }
                }
                pushes.pop();
                let prev_option_count = push_option_counts.pop().expect("push count");
                let pop_count = option_count - prev_option_count;
                if pop_count > 0 {
                    let line = item.line + 1;
                    match res_last {
                        Some(i) if res[i].line == line => {
                            res[i].pop_count = Some(res[i].pop_count.unwrap_or(0) + pop_count);
                        }
                        _ => {
                            res.push(InlineOption {
                                line,
                                column: None,
                                end_column: None,
                                options: None,
                                pop_count: Some(pop_count),
                            });
                            res_last = Some(res.len() - 1);
                        }
                    }
                }
                option_count = prev_option_count;
            }
            _ => {
                let line = item.line;
                match res_last {
                    Some(i) if res[i].line == line => {
                        res[i].options.clone_from(&item.options);
                        res[i].column = item.column;
                        res[i].end_column = item.end_column;
                    }
                    _ => {
                        res.push(item);
                        res_last = Some(res.len() - 1);
                    }
                }
                if st.code_lines.contains_key(&line) {
                    res.push(InlineOption {
                        line: line + 1,
                        column: None,
                        end_column: None,
                        options: None,
                        pop_count: Some(1),
                    });
                    res_last = Some(res.len() - 1);
                } else {
                    option_count += 1;
                }
            }
        }
    }
    while let Some(top) = pushes.pop() {
        warn_column_range(st, "022", &top);
    }
    res
}

pub fn run(st: &mut CheckState) {
    let mut items: Vec<InlineOption> = Vec::new();
    let comments: Vec<_> = st
        .comments
        .iter()
        .map(|c| (c.contents.clone(), c.range))
        .collect();
    for (contents, range) in comments {
        let column = Some(st.offset_to_column(range.line, range.offset));
        let end_column = Some(st.offset_to_column(range.line, range.end_offset));
        match parse_inline_comment(&contents) {
            Parsed::NotInline => {}
            Parsed::Options(opts1, opts2) => {
                items.push(InlineOption {
                    line: range.line,
                    column,
                    end_column,
                    options: Some(opts1),
                    pop_count: None,
                });
                if let Some(opts2) = opts2 {
                    items.push(InlineOption {
                        line: range.line,
                        column,
                        end_column,
                        options: Some(opts2),
                        pop_count: None,
                    });
                }
            }
            Parsed::Error(msg) => {
                let mut warning = Warning::new("021");
                warning.msg = Some(msg.into_bytes());
                st.warn_range(warning, range);
            }
        }
    }
    for &line in &st.lines[st.top_line].lines.clone() {
        let fn_node = st.lines[line].node;
        let range = st.ast.range(fn_node);
        let end_range = st.ast.nodes[fn_node].end_range.expect("function end");
        items.push(InlineOption {
            line: range.line,
            column: Some(st.offset_to_column(range.line, range.offset)),
            end_column: None,
            options: Some(InlineOpts::Push),
            pop_count: None,
        });
        items.push(InlineOption {
            line: end_range.line,
            column: Some(st.offset_to_column(end_range.line, end_range.offset)),
            end_column: None,
            options: Some(InlineOpts::Pop),
            pop_count: None,
        });
    }
    items.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| order(a).cmp(&order(b)))
            .then_with(|| a.column.cmp(&b.column))
    });
    st.inline_options = apply_boundaries(st, items);
}

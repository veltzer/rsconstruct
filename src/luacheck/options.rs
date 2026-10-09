//! luacheck's `options` module: validating option tables and normalizing
//! an option stack (outermost first) into the final std, limits and rules.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::rc::Rc;

use super::pattern;
use super::standards::{self, Def};
use super::value::{LKey, LTable, LVal};

pub type Stds = BTreeMap<Vec<u8>, Rc<LTable>>;

/// How an option's value is validated.
#[derive(Clone, Copy, Debug)]
pub enum Validator {
    Boolean,
    NumberOrFalse,
    ArrayOfStrings,
    Std,
    FieldMap,
    Table,
    StringOrBoolean,
    StringOrFunction,
    Quiet,
    Jobs,
}

pub const NULLARY_INLINE_OPTIONS: &[&str] = &[
    "global",
    "unused",
    "redefined",
    "unused_args",
    "unused_secondaries",
    "self",
    "compat",
    "allow_defined",
    "allow_defined_top",
    "module",
];

pub const VARIADIC_INLINE_OPTIONS: &[(&str, Validator)] = &[
    ("globals", Validator::FieldMap),
    ("read_globals", Validator::FieldMap),
    ("new_globals", Validator::FieldMap),
    ("new_read_globals", Validator::FieldMap),
    ("not_globals", Validator::ArrayOfStrings),
    ("ignore", Validator::ArrayOfStrings),
    ("enable", Validator::ArrayOfStrings),
    ("only", Validator::ArrayOfStrings),
];

/// `options.all_options`.
pub fn all_options() -> Vec<(&'static str, Validator)> {
    let mut out = vec![
        ("std", Validator::Std),
        ("max_line_length", Validator::NumberOrFalse),
        ("max_code_line_length", Validator::NumberOrFalse),
        ("max_string_line_length", Validator::NumberOrFalse),
        ("max_comment_line_length", Validator::NumberOrFalse),
        ("max_cyclomatic_complexity", Validator::NumberOrFalse),
        ("operators", Validator::ArrayOfStrings),
    ];
    out.extend(
        NULLARY_INLINE_OPTIONS
            .iter()
            .map(|&n| (n, Validator::Boolean)),
    );
    out.extend(VARIADIC_INLINE_OPTIONS.iter().copied());
    out
}

/// The options a configuration may set at top level.
pub fn top_options() -> Vec<(&'static str, Validator)> {
    let mut out = vec![
        ("cache", Validator::StringOrBoolean),
        ("jobs", Validator::Jobs),
        ("files", Validator::Table),
        ("stds", Validator::Table),
        ("exclude_files", Validator::ArrayOfStrings),
        ("include_files", Validator::ArrayOfStrings),
        ("quiet", Validator::Quiet),
        ("color", Validator::Boolean),
        ("codes", Validator::Boolean),
        ("ranges", Validator::Boolean),
        ("formatter", Validator::StringOrFunction),
    ];
    out.extend(all_options());
    out
}

/// `utils.split(s, sep)` (or on whitespace when `sep` is `None`).
pub fn split(s: &[u8], sep: Option<u8>) -> Vec<Vec<u8>> {
    match sep {
        Some(sep) => s.split(|&c| c == sep).map(<[u8]>::to_vec).collect(),
        None => s
            .split(|c| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
            .filter(|p| !p.is_empty())
            .map(<[u8]>::to_vec)
            .collect(),
    }
}

/// `utils.strip`.
pub fn strip(s: &[u8]) -> &[u8] {
    let is_space = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let start = s.iter().position(|c| !is_space(c)).unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|c| !is_space(c))
        .map_or(start, |e| e + 1);
    &s[start..end.max(start)]
}

/// A validated std string: std names, and whether it adds to the current std.
pub struct StdParts {
    pub parts: Vec<Vec<u8>>,
    pub add: bool,
}

/// `split_std`.
pub fn split_std(std: &[u8], stds: &Stds) -> Result<StdParts, String> {
    let mut parts = split(std, Some(b'+'));
    let mut add = false;
    if parts.first().is_some_and(|p| strip(p).is_empty()) {
        add = true;
        parts.remove(0);
    }
    for part in &mut parts {
        *part = strip(part).to_vec();
        if !stds.contains_key(part.as_slice()) {
            return Err(format!("unknown std '{}'", String::from_utf8_lossy(part)));
        }
    }
    Ok(StdParts { parts, add })
}

fn array_of_strings(x: &LVal) -> Result<(), String> {
    let LVal::Table(t) = x else {
        return Err(format!("array of strings expected, got {}", x.type_name()));
    };
    for (index, item) in t.array.iter().enumerate() {
        if !matches!(item, LVal::Str(_)) {
            return Err(format!(
                "array of strings expected, got {} at index [{}]",
                item.type_name(),
                index + 1
            ));
        }
    }
    Ok(())
}

#[allow(clippy::float_cmp)] // Lua's `math.floor(x) == x` integer test, exact by design.
fn is_integer(n: f64) -> bool {
    n.floor() == n
}

pub fn validate_value(validator: Validator, x: &LVal, stds: &Stds) -> Result<(), String> {
    let t = x.type_name();
    match validator {
        Validator::Boolean => match x {
            LVal::Bool(_) => Ok(()),
            _ => Err(format!("boolean expected, got {t}")),
        },
        Validator::Table => match x {
            LVal::Table(_) => Ok(()),
            _ => Err(format!("table expected, got {t}")),
        },
        Validator::NumberOrFalse => match x {
            LVal::Num(_) | LVal::Bool(false) => Ok(()),
            LVal::Bool(true) => Err("number or false expected, got true".into()),
            _ => Err(format!("number or false expected, got {t}")),
        },
        Validator::StringOrBoolean => match x {
            LVal::Str(_) | LVal::Bool(_) => Ok(()),
            _ => Err(format!("string or boolean expected, got {t}")),
        },
        Validator::StringOrFunction => match x {
            LVal::Str(_) | LVal::Other("function") => Ok(()),
            _ => Err(format!("string or function expected, got {t}")),
        },
        Validator::ArrayOfStrings => array_of_strings(x),
        Validator::FieldMap => match x {
            LVal::Table(_) => standards::validate_globals_table(x),
            _ => Err(format!("table expected, got {t}")),
        },
        Validator::Std => match x {
            LVal::Str(s) => split_std(s, stds).map(|_| ()),
            LVal::Table(table) => standards::validate_std_table(table),
            _ => Err(format!("string or table expected, got {t}")),
        },
        Validator::Quiet => match x {
            LVal::Num(n) if is_integer(*n) && (0.0..=3.0).contains(n) => Ok(()),
            LVal::Num(n) => Err(format!(
                "integer in range 0..3 expected, got {}",
                super::value::format_20g(*n)
            )),
            _ => Err(format!("integer in range 0..3 expected, got {t}")),
        },
        Validator::Jobs => match x {
            LVal::Num(n) if is_integer(*n) && *n >= 1.0 => Ok(()),
            LVal::Num(n) => Err(format!(
                "positive integer expected, got {}",
                super::value::format_20g(*n)
            )),
            _ => Err(format!("positive integer expected, got {t}")),
        },
    }
}

/// `options.validate`: options are checked in sorted name order.
pub fn validate(
    option_set: &[(&'static str, Validator)],
    opts: &LVal,
    stds: &Stds,
) -> Result<(), String> {
    let LVal::Table(table) = opts else {
        return Err(format!("option table expected, got {}", opts.type_name()));
    };
    let mut sorted: Vec<&(&str, Validator)> = option_set.iter().collect();
    sorted.sort_by_key(|(name, _)| *name);
    for (option, validator) in sorted {
        if let Some(value) = table.get(option) {
            validate_value(*validator, value, stds)
                .map_err(|err| format!("invalid value of option '{option}': {err}"))?;
        }
    }
    Ok(())
}

/// An option stack: option tables, outermost first.
pub type OptionStack = Vec<Rc<LTable>>;

fn get_std_tables(opts_stack: &OptionStack, stds: &Stds) -> Vec<Rc<LTable>> {
    let mut base_std: Option<Rc<LTable>> = None;
    let mut add_stds = Vec::new();
    let mut no_compat = false;
    for opts in opts_stack.iter().rev() {
        let compat = opts.get("compat").and_then(LVal::as_bool);
        if compat == Some(true) && !no_compat {
            base_std = stds.get(b"max".as_slice()).cloned();
            break;
        } else if compat == Some(false) {
            no_compat = true;
        }
        match opts.get("std") {
            Some(LVal::Table(std)) => {
                base_std = Some(std.clone());
                break;
            }
            Some(LVal::Str(std)) => {
                let parts = split_std(std, stds).expect("validated std");
                for part in &parts.parts {
                    add_stds.push(stds[part.as_slice()].clone());
                }
                if !parts.add {
                    base_std = Some(Rc::new(LTable::default()));
                    break;
                }
            }
            _ => {}
        }
    }
    let base = base_std.unwrap_or_else(|| stds[b"max".as_slice()].clone());
    std::iter::once(base).chain(add_stds).collect()
}

fn index_of_last_option_usage(opts_stack: &OptionStack, option_name: &str) -> usize {
    for (index, opts) in opts_stack.iter().enumerate().rev() {
        if opts.get(option_name).is_some_and(LVal::truthy) {
            return index + 1;
        }
    }
    0
}

fn split_field(field_name: &[u8]) -> Vec<Vec<u8>> {
    split(field_name, Some(b'.'))
}

fn field_comparator(a: &(Vec<Vec<u8>>, bool), b: &(Vec<Vec<u8>>, bool)) -> Ordering {
    let (parts1, parts2) = (&a.0, &b.0);
    for i in 0..parts1.len().max(parts2.len()) {
        match (parts1.get(i), parts2.get(i)) {
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(p1), Some(p2)) if p1 != p2 => return p1.cmp(p2),
            _ => {}
        }
    }
    Ordering::Equal
}

fn get_final_std(opts_stack: &OptionStack, stds: &Stds) -> Def {
    let mut final_std = Def::default();
    for std_table in get_std_tables(opts_stack, stds) {
        standards::add_std_table(&mut final_std, &std_table, false, false);
    }
    let last_new_globals = index_of_last_option_usage(opts_stack, "new_globals");
    let last_new_read_globals = index_of_last_option_usage(opts_stack, "new_read_globals");
    for (index, opts) in opts_stack.iter().enumerate() {
        let index = index + 1;
        let pick = |new: &str, old: &str, last: usize| -> Option<LVal> {
            if index < last {
                return None;
            }
            opts.get(new)
                .filter(|v| v.truthy())
                .or_else(|| opts.get(old))
                .filter(|v| v.truthy())
                .cloned()
        };
        let globals = pick("new_globals", "globals", last_new_globals);
        let read_globals = pick("new_read_globals", "read_globals", last_new_read_globals);
        let mut new_fields: Vec<(Vec<Vec<u8>>, bool)> = Vec::new();
        if let Some(LVal::Table(globals)) = &globals {
            for global in globals.strings() {
                new_fields.push((split_field(&global), false));
            }
        }
        if let Some(LVal::Table(read_globals)) = &read_globals {
            for read_global in read_globals.strings() {
                new_fields.push((split_field(&read_global), true));
            }
        }
        if globals.is_some() && read_globals.is_some() {
            new_fields.sort_by(field_comparator);
        }
        for (field, read_only) in &new_fields {
            standards::overwrite_field(&mut final_std, field, *read_only);
        }
        let mut hash = Vec::new();
        if let Some(globals) = globals {
            hash.push((LKey::Str(b"globals".to_vec()), globals));
        }
        if let Some(read_globals) = read_globals {
            hash.push((LKey::Str(b"read_globals".to_vec()), read_globals));
        }
        standards::add_std_table(&mut final_std, &LTable::new(Vec::new(), hash), true, true);
        if let Some(LVal::Table(not_globals)) = opts.get("not_globals") {
            for not_global in not_globals.strings() {
                standards::remove_field(&mut final_std, &split_field(&not_global));
            }
        }
    }
    standards::finalize(&mut final_std);
    final_std
}

fn get_scalar_opt(opts_stack: &OptionStack, option: &str) -> Option<LVal> {
    opts_stack
        .iter()
        .rev()
        .find_map(|opts| opts.get(option).cloned())
}

/// A limit option: a number, or `false` for no limit.
fn limit(value: Option<LVal>) -> Option<f64> {
    match value {
        Some(LVal::Num(n)) => Some(n),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleType {
    Enable,
    Disable,
    Only,
}

/// A normalized pattern: code pattern and name pattern, either may be absent.
pub type NormalizedPattern = (Option<Vec<u8>>, Option<Vec<u8>>);

pub struct Rule {
    pub patterns: Vec<NormalizedPattern>,
    pub ty: RuleType,
}

const MACROS: &[(&str, &str)] = &[
    ("unused_args", "21[23]"),
    ("global", "1"),
    ("unused", "[23]"),
    ("redefined", "4"),
];

fn anchor_pattern(pattern: Option<&[u8]>, only_start: bool) -> Option<Vec<u8>> {
    let pattern = pattern?;
    if pattern.first() == Some(&b'^') || pattern.last() == Some(&b'$') {
        return Some(pattern.to_vec());
    }
    let mut out = vec![b'^'];
    out.extend_from_slice(pattern);
    if !only_start {
        out.push(b'$');
    }
    Some(out)
}

fn normalize_pattern(pattern: &[u8]) -> NormalizedPattern {
    let (code_pattern, name_pattern) =
        if let Some(slash_pos) = pattern.iter().position(|&c| c == b'/') {
            (Some(&pattern[..slash_pos]), Some(&pattern[slash_pos + 1..]))
        } else if pattern
            .iter()
            .any(|&c| c == b'_' || c.is_ascii_alphabetic())
        {
            (None, Some(pattern))
        } else {
            (Some(pattern), None)
        };
    (
        anchor_pattern(code_pattern, true),
        anchor_pattern(name_pattern, false),
    )
}

fn get_rules(opts_stack: &OptionStack) -> Vec<Rule> {
    let mut rules = Vec::new();
    let mut used_macros: Vec<&str> = Vec::new();
    for opts in opts_stack.iter().rev() {
        for &(option, macro_pattern) in MACROS {
            if used_macros.contains(&option) {
                continue;
            }
            if let Some(value) = opts.get(option) {
                rules.push(Rule {
                    patterns: vec![normalize_pattern(macro_pattern.as_bytes())],
                    ty: if value.truthy() {
                        RuleType::Enable
                    } else {
                        RuleType::Disable
                    },
                });
                used_macros.push(option);
            }
        }
        for (key, ty) in [
            ("ignore", RuleType::Disable),
            ("only", RuleType::Only),
            ("enable", RuleType::Enable),
        ] {
            if let Some(LVal::Table(patterns)) = opts.get(key) {
                rules.push(Rule {
                    patterns: patterns
                        .strings()
                        .iter()
                        .map(|p| normalize_pattern(p))
                        .collect(),
                    ty,
                });
            }
        }
    }
    rules
}

fn get_operators(opts_stack: &OptionStack) -> Option<Vec<Vec<u8>>> {
    let mut operators: Option<Vec<Vec<u8>>> = None;
    for opts in opts_stack {
        if let Some(LVal::Table(ops)) = opts.get("operators") {
            let list = operators.get_or_insert_with(Vec::new);
            for op in ops.strings() {
                if !list.contains(&op) {
                    list.push(op);
                }
            }
        }
    }
    operators
}

/// Normalized options for a line of a file.
pub struct Normalized {
    pub std: Def,
    pub operators: Option<Vec<Vec<u8>>>,
    pub unused_secondaries: bool,
    pub is_self: bool,
    pub module: bool,
    pub allow_defined: bool,
    pub allow_defined_top: bool,
    pub max_cyclomatic_complexity: Option<f64>,
    pub max_code_line_length: Option<f64>,
    pub max_string_line_length: Option<f64>,
    pub max_comment_line_length: Option<f64>,
    pub rules: Vec<Rule>,
}

fn scalar_bool(opts_stack: &OptionStack, option: &str, default: bool) -> bool {
    get_scalar_opt(opts_stack, option).map_or(default, |v| v.truthy())
}

/// `options.normalize`.
pub fn normalize(opts_stack: &OptionStack, stds: &Stds) -> Normalized {
    let mut subs = [Some(120.0); 3];
    let sub_names = [
        "max_code_line_length",
        "max_string_line_length",
        "max_comment_line_length",
    ];
    for opts in opts_stack {
        if let Some(value) = opts.get("max_line_length") {
            subs = [limit(Some(value.clone())); 3];
        }
        for (i, name) in sub_names.iter().enumerate() {
            if let Some(value) = opts.get(name) {
                subs[i] = limit(Some(value.clone()));
            }
        }
    }
    Normalized {
        std: get_final_std(opts_stack, stds),
        operators: get_operators(opts_stack),
        unused_secondaries: scalar_bool(opts_stack, "unused_secondaries", true),
        is_self: scalar_bool(opts_stack, "self", true),
        module: scalar_bool(opts_stack, "module", false),
        allow_defined: scalar_bool(opts_stack, "allow_defined", false),
        allow_defined_top: scalar_bool(opts_stack, "allow_defined_top", false),
        max_cyclomatic_complexity: limit(get_scalar_opt(opts_stack, "max_cyclomatic_complexity")),
        max_code_line_length: subs[0],
        max_string_line_length: subs[1],
        max_comment_line_length: subs[2],
        rules: get_rules(opts_stack),
    }
}

/// An invalid user pattern, as luacheck's `InvalidPatternError`.
#[derive(Debug, Clone)]
pub struct InvalidPattern {
    pub pattern: Vec<u8>,
}

fn pmatch(subject: &[u8], pattern: &[u8]) -> Result<bool, InvalidPattern> {
    pattern::pmatch(subject, pattern).map_err(|_| InvalidPattern {
        pattern: pattern.to_vec(),
    })
}

/// `filter.passes_rules_filter`, including its quirk: an enabled name sets
/// `matches_code`, not `matches_name`.
pub fn passes_rules_filter(
    rules: &[Rule],
    code: &str,
    name: Option<&[u8]>,
) -> Result<bool, InvalidPattern> {
    let mut enabled_code = false;
    let mut enabled_name = false;
    for rule in rules {
        let mut matches_one = false;
        for (code_pattern, name_pattern) in &rule.patterns {
            let mut matches_code = match code_pattern {
                Some(p) => Some(pmatch(code.as_bytes(), p)?),
                None => None,
            };
            let matches_name = match name_pattern {
                Some(p) => match name {
                    None => Some(false),
                    Some(name) => Some(pmatch(name, p)?),
                },
                None => None,
            };
            if enabled_code {
                matches_code = Some(rule.ty != RuleType::Disable);
            }
            if enabled_name {
                matches_code = Some(rule.ty != RuleType::Disable);
            }
            if (matches_code == Some(true) && matches_name != Some(false))
                || (matches_name == Some(true) && matches_code != Some(false))
            {
                matches_one = true;
            }
            match rule.ty {
                RuleType::Enable => {
                    if matches_code == Some(true) {
                        enabled_code = true;
                    }
                    if matches_name == Some(true) {
                        enabled_name = true;
                    }
                    if enabled_code && enabled_name {
                        return Ok(true);
                    }
                }
                RuleType::Disable => {
                    if matches_one {
                        return Ok(false);
                    }
                }
                RuleType::Only => {}
            }
        }
        if rule.ty == RuleType::Only && !matches_one {
            return Ok(false);
        }
    }
    Ok(true)
}

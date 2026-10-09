//! `ShellCheck`'s general checks (`ShellCheck.Analytics`), part one: the
//! check lists, the helpers they share, and the first per-node checks.
//!
//! Each check is named after its Haskell original (`checkEchoWc` is
//! `check_echo_wc`) and emits the same codes and messages under the same
//! conditions.

use std::collections::{BTreeMap, BTreeSet};

use super::analyzerlib::{
    DataType, Out, Parameters, StackData, err, get_closest_command, get_parents_of_id, get_path,
    info, is_command, is_command_match, is_confused_glob_regex, is_counting_reference,
    is_parent_of, is_quote_free, is_quoted_alternative_reference, is_strictly_quote_free,
    is_test_command, is_unqualified_command, make_comment, make_comment_with_fix, style,
    used_as_command_name, warn, warn_with_fix,
};
use super::ast::{AssignmentMode, ConditionType, Id, Inner, Token, do_analysis};
use super::astlib::{
    concat_oversimplify, e4m, get_all_flags, get_braced_modifier, get_braced_reference,
    get_command, get_command_basename, get_command_name_from_expansion, get_command_token_or_this,
    get_glob_or_literal_string, get_literal_string, get_literal_string_def,
    get_trailing_unquoted_literal, get_unquoted_literal, get_word_parts, is_array_expansion,
    is_assignment, is_constant, is_glob, is_literal, is_literal_number, is_quotes,
    is_unquoted_flag, is_variable_char, is_variable_name, may_become_multiple_args,
    only_literal_string, oversimplify, will_become_multiple_args, will_concat_in_assignment,
    will_split, words_can_be_equal,
};
use super::cfganalysis::{self, NumericalStatus};
use super::data::{ARITHMETIC_BINARY_TEST_OPS, BINARY_TEST_OPS, COMMON_COMMANDS, UNARY_TEST_OPS};
use super::interface::{Fix, InsertionPoint, Replacement, Severity, Shell};
use super::regex::{Regex, mk_regex};

use Inner::{
    T_AndIf, T_Annotation, T_Arithmetic, T_Array, T_Assignment, T_Backticked, T_BatsTest,
    T_BraceExpansion, T_BraceGroup, T_CaseExpression, T_Condition, T_DGREAT, T_DollarArithmetic,
    T_DollarBraceCommandExpansion, T_DollarBraced, T_DollarBracket, T_DollarExpansion,
    T_DoubleQuoted, T_Extglob, T_FdRedirect, T_ForArithmetic, T_ForIn, T_Function, T_GREATAND,
    T_Glob, T_Greater, T_HereDoc, T_IfExpression, T_IndexedElement, T_IoDuplicate, T_IoFile,
    T_Less, T_Literal, T_NormalWord, T_OrIf, T_ParamSubSpecialChar, T_Pipeline, T_ProcSub,
    T_Redirecting, T_Script, T_SimpleCommand, T_SingleQuoted, T_UntilExpression, T_WhileExpression,
    TA_Binary, TA_Expansion, TC_And, TC_Binary, TC_Nullary, TC_Or, TC_Unary,
};

pub type NodeCheck = fn(&Parameters<'_>, &Token, &mut Out);
pub type TreeCheck = fn(&Parameters<'_>, &Token) -> Out;

/// `nodeChecksToTreeCheck`.
pub fn node_checks_to_tree_check(list: &[NodeCheck], p: &Parameters<'_>, t: &Token) -> Out {
    let mut out = Vec::new();
    do_analysis(t, &mut |x| {
        for f in list {
            f(p, x, &mut out);
        }
    });
    out
}

/// `hasFloatingPoint`.
pub fn has_floating_point(params: &Parameters<'_>) -> bool {
    params.shell_type == Shell::Ksh
}

/// `isCondition`: is this token (the first of the path) part of a
/// condition?
pub fn is_condition(path: &[&Token]) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return false;
    };
    let mut child = *first;
    for parent in rest {
        if matches!(child.inner, T_BatsTest(..)) {
            return true;
        }
        let children: Vec<Id> = match &parent.inner {
            T_AndIf(left, _) | T_OrIf(left, _) => vec![left.id],
            T_IfExpression(conditions, _) => conditions
                .iter()
                .filter_map(|(c, _)| c.last().map(|t| t.id))
                .collect(),
            T_WhileExpression(c, _) | T_UntilExpression(c, _) => {
                c.last().map(|t| t.id).into_iter().collect()
            }
            _ => Vec::new(),
        };
        if children.contains(&child.id) {
            return true;
        }
        child = parent;
    }
    false
}

/// The number of tokens on the path from this id to the root.
fn depth(params: &Parameters<'_>, id: Id) -> i64 {
    1 + get_parents_of_id(&params.parent_map, id).len() as i64
}

/// `replaceStart`.
pub fn replace_start(id: Id, params: &Parameters<'_>, n: i64, r: &str) -> Replacement {
    let (start, _) = &params.token_positions[&id];
    let mut new_end = start.clone();
    new_end.column = start.column + n;
    Replacement {
        start: start.clone(),
        end: new_end,
        string: r.to_string(),
        precedence: depth(params, id),
        insertion_point: InsertionPoint::InsertAfter,
    }
}

/// `replaceEnd`.
pub fn replace_end(id: Id, params: &Parameters<'_>, n: i64, r: &str) -> Replacement {
    let (_, end) = &params.token_positions[&id];
    let mut new_start = end.clone();
    new_start.column = end.column - n;
    Replacement {
        start: new_start,
        end: end.clone(),
        string: r.to_string(),
        precedence: depth(params, id),
        insertion_point: InsertionPoint::InsertBefore,
    }
}

/// `replaceToken`.
pub fn replace_token(id: Id, params: &Parameters<'_>, r: &str) -> Replacement {
    let (start, end) = &params.token_positions[&id];
    Replacement {
        start: start.clone(),
        end: end.clone(),
        string: r.to_string(),
        precedence: depth(params, id),
        insertion_point: InsertionPoint::InsertBefore,
    }
}

/// `surroundWith`.
pub fn surround_with(id: Id, params: &Parameters<'_>, s: &str) -> Fix {
    fix_with(vec![
        replace_start(id, params, 0, s),
        replace_end(id, params, 0, s),
    ])
}

/// `fixWith`.
pub const fn fix_with(fixes: Vec<Replacement>) -> Fix {
    Fix {
        replacements: fixes,
    }
}

/// `functions`: function names to definition ids (the first definition
/// in tree order wins).
pub fn functions(t: &Token) -> BTreeMap<String, Id> {
    let mut list: Vec<(String, Id)> = Vec::new();
    do_analysis(t, &mut |x| {
        if let T_Function(_, _, name, _) = &x.inner {
            list.push((name.clone(), x.id));
        }
    });
    let mut map = BTreeMap::new();
    for (k, v) in list.into_iter().rev() {
        map.insert(k, v);
    }
    map
}

/// `aliases`: alias names to definition ids.
pub fn aliases(t: &Token) -> BTreeMap<String, Id> {
    let mut list: Vec<(String, Id)> = Vec::new();
    do_analysis(t, &mut |x| {
        if let T_SimpleCommand(_, words) = &x.inner
            && !words.is_empty()
            && is_unqualified_command(x, "alias")
        {
            for arg in &words[1..] {
                let string = only_literal_string(arg);
                if string.contains('=') {
                    list.push((string.chars().take_while(|&c| c != '=').collect(), arg.id));
                }
            }
        }
    });
    let mut map = BTreeMap::new();
    for (k, v) in list.into_iter().rev() {
        map.insert(k, v);
    }
    map
}

/// `checkCommand str f t`: run `f cmd rest` if `t` invokes `str`.
pub fn check_command<'t>(name: &str, t: &'t Token, f: &mut dyn FnMut(&'t Token, &'t [Token])) {
    if let T_SimpleCommand(_, words) = &t.inner
        && let Some((cmd, rest)) = words.split_first()
        && is_command(t, name)
    {
        f(cmd, rest);
    }
}

/// `checkUnqualifiedCommand`.
pub fn check_unqualified_command<'t>(
    name: &str,
    t: &'t Token,
    f: &mut dyn FnMut(&'t Token, &'t [Token]),
) {
    if let T_SimpleCommand(_, words) = &t.inner
        && let Some((cmd, rest)) = words.split_first()
        && is_unqualified_command(t, name)
    {
        f(cmd, rest);
    }
}

thread_local! {
    static WRONG_ARITH_RE: Regex = mk_regex("^([_a-zA-Z][_a-zA-Z0-9]*)([+*-]).+$");
    static ENV_SPLIT_RE: Regex = mk_regex("env +(-S|--split-string)");
    static SQ_VAR_RE: Regex = mk_regex("\\$[{(0-9a-zA-Z_]|`[^`]+`");
    static SED_CONTRA_RE: Regex = mk_regex("\\$[{dpsaic]($|[^a-zA-Z])");
    static FLOAT_RE: Regex = mk_regex("^[-+]?[0-9]+\\.[0-9]+$");
    static METACHARS_RE: Regex = mk_regex("[][*.+()|]");
    static OCTAL_RE: Regex = mk_regex("^0[0-7]*[8-9]");
    static ENCLOSED_RE: Regex = mk_regex("\\\\\\[.*\\\\\\]");
    static ESCAPE_RE: Regex = mk_regex("\\\\x1[Bb]|\\\\e|\u{1B}|\\\\033");
}

// ----- per-node checks, part one

pub fn check_echo_wc(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_Pipeline(_, cmds) = &t.inner
        && let [a, b] = cmds.as_slice()
    {
        let acmd = oversimplify(a);
        let bcmd = oversimplify(b);
        if acmd == ["echo", "${VAR}"] && (bcmd == ["wc", "-c"] || bcmd == ["wc", "-m"]) {
            style(out, t.id, 2000, "See if you can use ${#variable} instead.");
        }
    }
}

pub fn check_piped_assignment(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_Pipeline(_, cmds) = &t.inner
        && cmds.len() >= 2
        && let T_Redirecting(_, cmd) = &cmds[0].inner
        && let T_SimpleCommand(vars, words) = &cmd.inner
        && !vars.is_empty()
        && words.is_empty()
    {
        warn(
            out,
            cmd.id,
            2036,
            "If you wanted to assign the output of the pipeline, use a=$(b | c) .",
        );
    }
}

pub fn check_assign_ate_command(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_SimpleCommand(vars, list) = &t.inner
        && let [
            Token {
                inner: T_Assignment(_, _, _, assignment_term),
                ..
            },
        ] = vars.as_slice()
    {
        let first_word_is_arg = list
            .first()
            .is_some_and(|h| is_glob(h) || is_unquoted_flag(h));
        if first_word_is_arg {
            err(
                out,
                t.id,
                2037,
                "To assign the output of a command, use var=$(cmd) .",
            );
        } else if get_unquoted_literal(assignment_term)
            .is_some_and(|s| COMMON_COMMANDS.contains(&s.as_str()))
        {
            warn(
                out,
                t.id,
                2209,
                "Use var=$(command) to assign output (or quote to assign string).",
            );
        }
    }
}

pub fn check_arithmetic_op_command(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_SimpleCommand(vars, words) = &t.inner
        && let [
            Token {
                inner: T_Assignment(..),
                ..
            },
        ] = vars.as_slice()
        && let Some(first_word) = words.first()
        && let Some(op) = get_glob_or_literal_string(first_word)
        && ["+", "-", "*", "/"].contains(&op.as_str())
    {
        warn(
            out,
            first_word.id,
            2099,
            &format!("Use $((..)) for arithmetics, e.g. i=$((i {op} 2))"),
        );
    }
}

pub fn check_wrong_arithmetic_assignment(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_SimpleCommand(vars, words) = &t.inner else {
        return;
    };
    if !words.is_empty() {
        return;
    }
    let [
        Token {
            inner: T_Assignment(_, _, _, val),
            ..
        },
    ] = vars.as_slice()
    else {
        return;
    };
    let T_NormalWord(parts) = &val.inner else {
        return;
    };
    let mut s = String::new();
    for p in parts {
        match &p.inner {
            T_Literal(x) | T_Glob(x) => s.push_str(x),
            _ => return,
        }
    }
    let Some(groups) = WRONG_ARITH_RE.with(|re| re.match_groups(&s)) else {
        return;
    };
    let (var, op) = (&groups[0], &groups[1]);
    let referenced = params
        .variable_flow
        .iter()
        .any(|d| matches!(d, StackData::Assignment((_, _, name, _)) if name == var));
    if referenced {
        warn(
            out,
            val.id,
            2100,
            &format!("Use $((..)) for arithmetics, e.g. i=$((i {op} 2))"),
        );
    }
}

pub fn check_uuoc(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_Pipeline(_, cmds) = &t.inner
        && cmds.len() >= 2
        && let T_Redirecting(_, cmd) = &cmds[0].inner
    {
        check_command("cat", cmd, &mut |_, rest| {
            if let [word] = rest
                && !(may_become_multiple_args(word) || only_literal_string(word).starts_with('-'))
            {
                style(
                    out,
                    word.id,
                    2002,
                    "Useless cat. Consider 'cmd < file | ..' or 'cmd file | ..' instead.",
                );
            }
        });
    }
}

/// `indexOfSublists`.
fn index_of_sublists(sub: &[&str], list: &[String]) -> Vec<usize> {
    let matches = |a: &[String]| -> bool {
        a.len() >= sub.len() && sub.iter().zip(a.iter()).all(|(x, y)| *x == "?" || x == y)
    };
    (0..list.len()).filter(|&n| matches(&list[n..])).collect()
}

pub fn check_pipe_pitfalls(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_Pipeline(_, commands) = &t.inner else {
        return;
    };
    let heads: Vec<String> = commands
        .iter()
        .map(|c| oversimplify(c).into_iter().next().unwrap_or_default())
        .collect();
    let for_each = |l: &[&str], f: &mut dyn FnMut(&[Token])| -> bool {
        let indices = index_of_sublists(l, &heads);
        for n in &indices {
            let end = (n + l.len()).min(commands.len());
            f(&commands[*n..end]);
        }
        !indices.is_empty()
    };
    let has_short_parameter =
        |c: char, args: &[String]| args.iter().any(|x| x.starts_with('-') && x.contains(c));
    let has_parameter = |s: &str, args: &[String]| {
        args.iter()
            .any(|x| x.trim_start_matches('-').starts_with(s))
    };
    for_each(&["find", "xargs"], &mut |cmds| {
        let (find, xargs) = (&cmds[0], &cmds[1]);
        let mut args = oversimplify(xargs);
        args.extend(oversimplify(find));
        if !(has_short_parameter('0', &args)
            || has_parameter("null", &args)
            || has_parameter("print0", &args)
            || has_parameter("printf", &args))
        {
            warn(
                out,
                find.id,
                2038,
                "Use 'find .. -print0 | xargs -0 ..' or 'find .. -exec .. +' to allow non-alphanumeric filenames.",
            );
        }
    });
    for_each(&["ps", "grep"], &mut |cmds| {
        let ps = &cmds[0];
        let ps_flags: Vec<String> = get_command(ps)
            .map(|c| get_all_flags(c).into_iter().map(|(_, f)| f).collect())
            .unwrap_or_default();
        if !ps_flags
            .iter()
            .any(|f| ["p", "pid", "q", "quick-pid"].contains(&f.as_str()))
        {
            info(
                out,
                ps.id,
                2009,
                "Consider using pgrep instead of grepping ps output.",
            );
        }
    });
    for_each(&["grep", "wc"], &mut |cmds| {
        let (grep, wc) = (&cmds[0], &cmds[1]);
        let flags_grep: Vec<String> = get_command(grep)
            .map(|c| get_all_flags(c).into_iter().map(|(_, f)| f).collect())
            .unwrap_or_default();
        let flags_wc: Vec<String> = get_command(wc)
            .map(|c| get_all_flags(c).into_iter().map(|(_, f)| f).collect())
            .unwrap_or_default();
        let grep_ok = flags_grep.iter().any(|f| {
            [
                "l",
                "files-with-matches",
                "L",
                "files-without-matches",
                "o",
                "only-matching",
                "r",
                "R",
                "recursive",
                "A",
                "after-context",
                "B",
                "before-context",
            ]
            .contains(&f.as_str())
        });
        let wc_ok = flags_wc.iter().any(|f| {
            [
                "m",
                "chars",
                "w",
                "words",
                "c",
                "bytes",
                "L",
                "max-line-length",
            ]
            .contains(&f.as_str())
        });
        if !(grep_ok || wc_ok || flags_wc.is_empty()) {
            style(
                out,
                grep.id,
                2126,
                "Consider using 'grep -c' instead of 'grep|wc -l'.",
            );
        }
    });
    let first_id = |cmds: &[Token]| cmds.first().map(|x| get_command_token_or_this(x).id);
    let a = for_each(&["ls", "grep"], &mut |cmds| {
        if let Some(id) = first_id(cmds) {
            warn(
                out,
                id,
                2010,
                "Don't use ls | grep. Use a glob or a for loop with a condition to allow non-alphanumeric filenames.",
            );
        }
    });
    let b = for_each(&["ls", "xargs"], &mut |cmds| {
        if let Some(id) = first_id(cmds) {
            warn(
                out,
                id,
                2011,
                "Use 'find .. -print0 | xargs -0 ..' or 'find .. -exec .. +' to allow non-alphanumeric filenames.",
            );
        }
    });
    if !(a || b) {
        for_each(&["ls", "?"], &mut |cmds| {
            let ls = &cmds[0];
            if !has_short_parameter('N', &oversimplify(ls)) {
                info(
                    out,
                    ls.id,
                    2012,
                    "Use find instead of ls to better handle non-alphanumeric filenames.",
                );
            }
        });
    }
}

pub fn check_shebang_parameters(_: &Parameters<'_>, t: &Token) -> Out {
    shebang_parameters(t)
}

fn shebang_parameters(t: &Token) -> Out {
    match &t.inner {
        T_Annotation(_, t) => shebang_parameters(t),
        T_Script(sb, _) => {
            let T_Literal(s) = &sb.inner else {
                return Vec::new();
            };
            let is_multi_word = super::astlib::haskell_words(s).len() > 2
                && !ENV_SPLIT_RE.with(|re| re.is_match(s));
            if is_multi_word {
                vec![make_comment(
                    Severity::ErrorC,
                    sb.id,
                    2096,
                    "On most OS, shebangs can only specify a single parameter.",
                )]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

pub fn check_shebang(params: &Parameters<'_>, t: &Token) -> Out {
    match &t.inner {
        T_Annotation(list, inner) => {
            if list
                .iter()
                .any(|a| matches!(a, super::ast::Annotation::ShellOverride(_)))
            {
                Vec::new()
            } else {
                check_shebang(params, inner)
            }
        }
        T_Script(sb, _) => {
            let T_Literal(s) = &sb.inner else {
                return Vec::new();
            };
            let mut out = Vec::new();
            let id = sb.id;
            if !params.shell_type_specified {
                if s.is_empty() {
                    err(
                        &mut out,
                        id,
                        2148,
                        "Tips depend on target shell and yours is unknown. Add a shebang or a 'shell' directive.",
                    );
                }
                if super::astlib::executable_from_shebang(s) == "ash" {
                    warn(
                        &mut out,
                        id,
                        2187,
                        "Ash scripts will be checked as Dash. Add '# shellcheck shell=dash' to silence.",
                    );
                }
            }
            if !s.is_empty() {
                if !s.starts_with('/') {
                    err(
                        &mut out,
                        id,
                        2239,
                        "Ensure the shebang uses an absolute path to the interpreter.",
                    );
                }
                if super::astlib::haskell_words(s)
                    .first()
                    .is_some_and(|w| w.ends_with('/'))
                {
                    err(
                        &mut out,
                        id,
                        2246,
                        "This shebang specifies a directory. Ensure the interpreter is a file.",
                    );
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

pub fn check_for_in_quoted(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_ForIn(_, words, _) = &t.inner else {
        return;
    };
    if let [single] = words.as_slice() {
        if let T_NormalWord(parts) = &single.inner
            && let [word] = parts.as_slice()
        {
            if let T_DoubleQuoted(list) = &word.inner
                && ((list.iter().any(will_split) && !may_become_multiple_args(word))
                    || get_literal_string(word).is_some_and(|s| s.contains('*')))
            {
                err(
                    out,
                    word.id,
                    2066,
                    "Since you double quoted this, it will not word split, and the loop will only run once.",
                );
                return;
            }
            if let T_SingleQuoted(_) = &word.inner {
                warn(
                    out,
                    word.id,
                    2041,
                    "This is a literal string. To run as a command, use $(..) instead of '..' . ",
                );
                return;
            }
        }
        if get_unquoted_literal(single).is_some_and(|s| s.contains(',')) {
            warn(
                out,
                single.id,
                2042,
                "Use spaces, not commas, to separate loop elements.",
            );
            return;
        }
        if !(will_split(single) || may_become_multiple_args(single)) {
            warn(
                out,
                single.id,
                2043,
                "This loop will only ever run once. Bad quoting or missing glob/expansion?",
            );
            return;
        }
        // A single word that passes all the guards falls through to the
        // general case below, as the Haskell equations do.
    }
    for arg in words {
        if let Some(suffix) = get_trailing_unquoted_literal(arg)
            && let Some(string) = get_literal_string(suffix)
            && string.ends_with(',')
        {
            warn_with_fix(
                out,
                arg.id,
                2258,
                "The trailing comma is part of the value, not a separator. Delete or quote it.",
                fix_with(vec![replace_end(suffix.id, params, 1, "")]),
            );
        }
    }
}

pub fn check_for_in_cat(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_ForIn(_, words, _) = &t.inner else {
        return;
    };
    let [
        Token {
            inner: T_NormalWord(w),
            ..
        },
    ] = words.as_slice()
    else {
        return;
    };
    let is_line_based = |cmd: &Token| {
        ["grep", "fgrep", "egrep", "sed", "cat", "awk", "cut", "sort"]
            .iter()
            .any(|c| is_command(cmd, c))
    };
    for part in w {
        let (id, cmds) = match &part.inner {
            T_DollarExpansion(c) | T_Backticked(c) => (part.id, c),
            _ => continue,
        };
        if let [
            Token {
                inner: T_Pipeline(_, r),
                ..
            },
        ] = cmds.as_slice()
            && r.iter().all(is_line_based)
        {
            info(
                out,
                id,
                2013,
                "To read lines rather than words, pipe/redirect to a 'while read' loop.",
            );
        }
    }
}

pub fn check_for_in_ls(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_ForIn(_, words, _) = &t.inner else {
        return;
    };
    let [
        Token {
            inner: T_NormalWord(parts),
            ..
        },
    ] = words.as_slice()
    else {
        return;
    };
    let [part] = parts.as_slice() else {
        return;
    };
    let x = match &part.inner {
        T_DollarExpansion(c) | T_Backticked(c) if c.len() == 1 => &c[0],
        _ => return,
    };
    let o = oversimplify(x);
    match o.first().map(String::as_str) {
        Some("ls") => {
            let msg = "Iterating over ls output is fragile. Use globs.";
            if o[1..].iter().any(|s| s.starts_with('-')) {
                warn(out, part.id, 2045, msg);
            } else {
                err(out, part.id, 2045, msg);
            }
        }
        Some("find") => warn(
            out,
            part.id,
            2044,
            "For loops over find output are fragile. Use find -exec or a while read loop.",
        ),
        _ => {}
    }
}

pub fn check_find_exec(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_SimpleCommand(_, words) = &t.inner else {
        return;
    };
    if words.is_empty() || !is_command(t, "find") {
        return;
    }
    let mut v = false;
    for w in &words[1..] {
        if v && let T_NormalWord(l) = &w.inner {
            for x in l {
                if matches!(
                    x.inner,
                    T_DollarExpansion(_) | T_Backticked(_) | T_Glob(_) | T_Extglob(..)
                ) {
                    info(
                        out,
                        x.id,
                        2014,
                        "This will expand once before find runs, not per file found.",
                    );
                }
            }
        }
        match get_literal_string(w).as_deref() {
            Some("-exec" | "-execdir") => v = true,
            Some("+" | ";") => v = false,
            _ => {}
        }
    }
    if v {
        let word_id = words.last().map_or(t.id, |w| w.id);
        err(
            out,
            word_id,
            2067,
            "Missing ';' or + terminating -exec. You can't use |/||/&&, and ';' has to be a separate, quoted argument.",
        );
    }
}

pub fn check_unquoted_expansions(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let (T_DollarExpansion(contents)
    | T_Backticked(contents)
    | T_DollarBraceCommandExpansion(_, contents)) = &t.inner
    else {
        return;
    };
    let tree = &params.parent_map;
    let should_be_split = matches!(
        get_command_name_from_expansion(t).as_deref(),
        Some("seq" | "pgrep")
    );
    if !(contents.is_empty()
        || should_be_split
        || is_quote_free(params.shell_type, tree, t)
        || used_as_command_name(tree, t))
    {
        warn(out, t.id, 2046, "Quote this to prevent word splitting.");
    }
}

pub fn check_redirect_to_same(params: &Parameters<'_>, s: &Token, out: &mut Out) {
    let T_Pipeline(_, list) = &s.inner else {
        return;
    };
    let get_redirs = |t: &Token| -> Vec<Token> {
        match &t.inner {
            T_FdRedirect(_, x) => match &x.inner {
                T_IoFile(op, file) if matches!(op.inner, T_Greater | T_Less | T_DGREAT) => {
                    vec![(**file).clone()]
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    };
    let all_redirs: Vec<Token> = list
        .iter()
        .flat_map(|t| match &t.inner {
            T_Redirecting(ls, _) => ls.iter().flat_map(get_redirs).collect(),
            _ => Vec::new(),
        })
        .collect();
    let io_op = |t: &Token| -> Option<Inner> {
        match get_parents_of_id(&params.parent_map, t.id)
            .first()
            .map(|p| &p.inner)
        {
            Some(T_IoFile(op, _)) => Some(op.inner.clone()),
            _ => None,
        }
    };
    let is_input = |t: &Token| matches!(io_op(t), Some(T_Less));
    let is_output = |t: &Token| matches!(io_op(t), Some(T_Greater | T_DGREAT));
    let special = |x: &Token| concat_oversimplify(x).starts_with("/dev/");
    let is_harmless_command = |arg: &Token| -> bool {
        let Some(this) = params.id_map.get(&arg.id) else {
            return false;
        };
        get_closest_command(&params.parent_map, this)
            .and_then(get_command_basename)
            .is_some_and(|n| ["echo", "mapfile", "printf", "sponge"].contains(&n.as_str()))
    };
    let contains_assignment = |arg: &Token| -> bool {
        let Some(this) = params.id_map.get(&arg.id) else {
            return false;
        };
        get_closest_command(&params.parent_map, this).is_some_and(is_assignment)
    };
    for l in list {
        for x in &all_redirs {
            do_analysis(l, &mut |u| {
                if let (T_NormalWord(xs), T_NormalWord(ys)) = (&x.inner, &u.inner)
                    && x.id != u.id
                    && xs == ys
                    && !(is_input(x) && is_input(u))
                    && !(is_output(x) && is_output(u))
                    && !special(x)
                    && !(is_harmless_command(x) || is_harmless_command(u))
                    && !contains_assignment(u)
                {
                    let note =
                        "Make sure not to read and write the same file in the same pipeline.";
                    out.push(make_comment(Severity::InfoC, u.id, 2094, note));
                    out.push(make_comment(Severity::InfoC, x.id, 2094, note));
                }
            });
        }
    }
}

pub fn check_shorthand_if(params: &Parameters<'_>, x: &Token, out: &mut Out) {
    let T_OrIf(lhs, rhs) = &x.inner else {
        return;
    };
    let T_AndIf(_, b) = &lhs.inner else {
        return;
    };
    let T_Pipeline(_, t) = &rhs.inner else {
        return;
    };
    let is_ok = match t.as_slice() {
        [t] => {
            is_assignment(t)
                || get_command_basename(t).is_some_and(|n| {
                    ["echo", "exit", "return", "printf", "true", ":"].contains(&n.as_str())
                })
        }
        _ => false,
    };
    let Some(xr) = params.id_map.get(&x.id) else {
        return;
    };
    let in_condition = is_condition(&get_path(&params.parent_map, xr));
    if !is_ok && !in_condition && !is_test_command(b) {
        info(
            out,
            lhs.id,
            2015,
            "Note that A && B || C is not if-then-else. C may run when A is true.",
        );
    }
}

pub fn check_dollar_star(p: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_NormalWord(parts) = &t.inner
        && let [
            Token {
                id,
                inner: T_DollarBraced(_, l),
            },
        ] = parts.as_slice()
        && !is_strictly_quote_free(p.shell_type, &p.parent_map, t)
    {
        let s = concat_oversimplify(l);
        if s.starts_with('*') {
            warn(
                out,
                *id,
                2048,
                "Use \"$@\" (with quotes) to prevent whitespace problems.",
            );
        }
        if get_braced_modifier(&s).starts_with("[*]")
            && is_variable_char(s.chars().next().unwrap_or('!'))
        {
            warn(
                out,
                *id,
                2048,
                "Use \"${array[@]}\" (with quotes) to prevent whitespace problems.",
            );
        }
    }
}

pub fn check_unquoted_dollar_at(p: &Parameters<'_>, word: &Token, out: &mut Out) {
    if let T_NormalWord(parts) = &word.inner
        && !is_strictly_quote_free(p.shell_type, &p.parent_map, word)
        && let Some(x) = parts.iter().find(|x| is_array_expansion(x))
        && !is_quoted_alternative_reference(x)
    {
        err(
            out,
            x.id,
            2068,
            "Double quote array expansions to avoid re-splitting elements.",
        );
    }
}

pub fn check_concatenated_dollar_at(p: &Parameters<'_>, word: &Token, out: &mut Out) {
    if !matches!(word.inner, T_NormalWord(_)) {
        return;
    }
    let parts = get_word_parts(word);
    if !(is_quote_free(p.shell_type, &p.parent_map, word) || parts.len() <= 1)
        && let Some(t) = parts.iter().find(|x| is_array_expansion(x))
    {
        err(
            out,
            t.id,
            2145,
            "Argument mixes string and array. Use * or separate argument.",
        );
    }
}

pub fn check_array_as_string(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_Assignment(_, _, _, word) = &t.inner {
        if will_concat_in_assignment(word) {
            warn(
                out,
                word.id,
                2124,
                "Assigning an array to a string! Assign as array, or use * instead of @ to concatenate.",
            );
        } else if will_become_multiple_args(word) {
            warn(
                out,
                word.id,
                2125,
                "Brace expansions and globs are literal in assignments. Quote it or use an array.",
            );
        }
    }
}

/// `doVariableFlowAnalysis`: run a reader and a writer over the flow.
pub fn do_variable_flow_analysis<'a, S>(
    read_func: &mut dyn FnMut(&mut S, &Token, &Token, &str) -> Out,
    write_func: &mut dyn FnMut(&mut S, &Token, &Token, &str, &DataType<'a>) -> Out,
    state: &mut S,
    flow: &[StackData<'a>],
) -> Out {
    let mut out = Vec::new();
    for x in flow {
        let l = match x {
            StackData::Reference((base, token, name)) => read_func(state, base, token, name),
            StackData::Assignment((base, token, name, values)) => {
                write_func(state, base, token, name, values)
            }
            _ => Vec::new(),
        };
        let mut l = l;
        l.extend(out);
        out = l;
    }
    out
}

pub fn check_array_without_index(params: &Parameters<'_>, _: &Token) -> Out {
    let mut set: BTreeSet<String> = super::data::ARRAY_VARIABLES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    do_variable_flow_analysis(
        &mut |s: &mut BTreeSet<String>, _base, t, _name| match &t.inner {
            T_DollarBraced(_, token) => match get_literal_string(token) {
                Some(name) if s.contains(&name) => vec![make_comment(
                    Severity::WarningC,
                    t.id,
                    2128,
                    "Expanding an array without an index only gives the first element.",
                )],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        },
        &mut |s: &mut BTreeSet<String>, _base, t, name, data| {
            match (&t.inner, data) {
                (T_Assignment(mode, aname, indices, _), DataType::DataString(_))
                    if indices.is_empty() =>
                {
                    if !s.contains(aname) {
                        return Vec::new();
                    }
                    return match mode {
                        AssignmentMode::Assign => vec![make_comment(
                            Severity::WarningC,
                            t.id,
                            2178,
                            "Variable was used as an array but is now assigned a string.",
                        )],
                        AssignmentMode::Append => vec![make_comment(
                            Severity::WarningC,
                            t.id,
                            2179,
                            "Use array+=(\"item\") to append items to an array.",
                        )],
                    };
                }
                (_, DataType::DataArray(_)) => {
                    s.insert(name.to_string());
                }
                _ => {
                    let is_indexed =
                        matches!(&t.inner, T_Assignment(_, _, indices, _) if !indices.is_empty());
                    if is_indexed {
                        s.insert(name.to_string());
                    } else {
                        s.remove(name);
                    }
                }
            }
            Vec::new()
        },
        &mut set,
        &params.variable_flow,
    )
}

pub fn check_stderr_redirect(params: &Parameters<'_>, redir: &Token, out: &mut Out) {
    let T_Redirecting(redirs, _) = &redir.inner else {
        return;
    };
    let [first, second] = redirs.as_slice() else {
        return;
    };
    let T_FdRedirect(fd1, dup) = &first.inner else {
        return;
    };
    if fd1 != "2" {
        return;
    }
    let T_IoDuplicate(op1, target) = &dup.inner else {
        return;
    };
    if !matches!(op1.inner, T_GREATAND) || target != "1" {
        return;
    }
    let T_FdRedirect(_, file) = &second.inner else {
        return;
    };
    let T_IoFile(op, _) = &file.inner else {
        return;
    };
    if !matches!(op.inner, T_Greater | T_DGREAT) {
        return;
    }
    let Some(redir_ref) = params.id_map.get(&redir.id) else {
        return;
    };
    let path = get_path(&params.parent_map, redir_ref);
    let uses_output = |t: &Token| match &t.inner {
        T_Pipeline(_, list) => {
            list.len() > 1
                && !is_parent_of(
                    &params.parent_map,
                    list.last().expect("non-empty"),
                    redir_ref,
                )
        }
        T_ProcSub(..) | T_DollarExpansion(_) | T_Backticked(_) => true,
        _ => false,
    };
    let is_captured = path.iter().any(|t| uses_output(t));
    if !is_captured {
        warn(
            out,
            first.id,
            2069,
            "To redirect stdout+stderr, 2>&1 must be last (or use '{ cmd > file; } 2>&1' to clarify).",
        );
    }
}

pub fn check_single_quoted_variables(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_SingleQuoted(s) = &t.inner else {
        return;
    };
    if !SQ_VAR_RE.with(|re| re.is_match(s)) {
        return;
    }
    let parents = &params.parent_map;
    let Some(this) = params.id_map.get(&t.id) else {
        return;
    };
    let command_name: String = (|| {
        let cmd = get_closest_command(parents, this)?;
        let name = get_command_basename(cmd)?;
        Some(match name.as_str() {
            "find" => get_find_command(cmd),
            "git" => get_git_command(cmd),
            "mumps" => get_mumps_command(cmd),
            _ => name,
        })
    })()
    .unwrap_or_default();
    let show_message = |out: &mut Out| {
        info(
            out,
            t.id,
            2016,
            "Expressions don't expand in single quotes, use double quotes for that.",
        );
    };
    if command_name == "sed" {
        if !SED_CONTRA_RE.with(|re| re.is_match(s)) {
            show_message(out);
        }
        return;
    }
    let path = get_path(parents, this);
    let is_ok_assignment = |t: &&Token| match &t.inner {
        T_Assignment(_, name, _, _) => {
            ["PS1", "PS2", "PS3", "PS4", "PROMPT_COMMAND"].contains(&name.as_str())
        }
        TC_Unary(_, op, _) => op == "-v",
        _ => false,
    };
    let is_probably_ok = path.iter().take(3).any(is_ok_assignment)
        || [
            "trap",
            "sh",
            "bash",
            "ksh",
            "zsh",
            "ssh",
            "eval",
            "xprop",
            "alias",
            "sudo",
            "doas",
            "run0",
            "docker",
            "podman",
            "oc",
            "dpkg-query",
            "jq",
            "rename",
            "rg",
            "unset",
            "git filter-branch",
            "mumps -run %XCMD",
            "mumps -run LOOP%XCMD",
        ]
        .contains(&command_name.as_str())
        || command_name.ends_with("awk")
        || command_name.starts_with("perl");
    if !is_probably_ok {
        show_message(out);
    }
}

fn get_find_command(t: &Token) -> String {
    match &t.inner {
        T_SimpleCommand(_, words) => {
            let list: Vec<Option<String>> = words.iter().map(get_literal_string).collect();
            let pos = list
                .iter()
                .position(|x| x.as_deref() == Some("-exec") || x.as_deref() == Some("-execdir"));
            match pos {
                Some(i) if i + 1 < list.len() => {
                    list[i + 1].clone().unwrap_or_else(|| "find".to_string())
                }
                _ => "find".to_string(),
            }
        }
        T_Redirecting(_, cmd) => get_find_command(cmd),
        _ => "find".to_string(),
    }
}

fn get_git_command(t: &Token) -> String {
    match &t.inner {
        T_SimpleCommand(_, words) => {
            let list: Vec<Option<String>> = words.iter().map(get_literal_string).collect();
            if list.len() >= 2
                && list[0].as_deref() == Some("git")
                && list[1].as_deref() == Some("filter-branch")
            {
                "git filter-branch".to_string()
            } else {
                "git".to_string()
            }
        }
        T_Redirecting(_, cmd) => get_git_command(cmd),
        _ => "git".to_string(),
    }
}

fn get_mumps_command(t: &Token) -> String {
    match &t.inner {
        T_SimpleCommand(_, words) => {
            let list: Vec<Option<String>> = words.iter().map(get_literal_string).collect();
            if list.len() >= 3
                && list[0].as_deref() == Some("mumps")
                && list[1].as_deref() == Some("-run")
            {
                match list[2].as_deref() {
                    Some("%XCMD") => return "mumps -run %XCMD".to_string(),
                    Some("LOOP%XCMD") => return "mumps -run LOOP%XCMD".to_string(),
                    _ => {}
                }
            }
            "mumps".to_string()
        }
        T_Redirecting(_, cmd) => get_mumps_command(cmd),
        _ => "mumps".to_string(),
    }
}

pub fn check_unquoted_n(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let TC_Unary(ConditionType::SingleBracket, op, w) = &t.inner
        && op == "-n"
        && will_split(w)
        && !get_word_parts(w).iter().any(|x| is_array_expansion(x))
    {
        err(
            out,
            w.id,
            2070,
            "-n doesn't work with unquoted arguments. Quote or use [[ ]].",
        );
    }
}

pub fn check_number_comparisons(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let TC_Binary(typ, op, lhs, rhs) = &t.inner else {
        return;
    };
    let id = t.id;
    let has_string_comparison = params.shell_type != Shell::Sh;
    let is_lt_gt = |o: &str| ["<", "\\<", ">", "\\>"].contains(&o);
    let is_le_ge = |o: &str| ["<=", "\\<=", ">=", "\\>="].contains(&o);
    let is_fraction = |t: &Token| match oversimplify(t).as_slice() {
        [v] => FLOAT_RE.with(|re| re.match_groups(v)).is_some(),
        _ => false,
    };
    let decimal_error =
        "Decimals are not supported. Either use integers only, or use bc or awk to compare.";
    let check_decimals = |hs: &Token, out: &mut Out| {
        if is_fraction(hs) && !has_floating_point(params) {
            err(out, hs.id, 2072, decimal_error);
        }
    };
    let is_num = |t: &Token| -> bool {
        let parts = get_word_parts(t);
        match parts.as_slice() {
            [
                Token {
                    inner: T_DollarArithmetic(_),
                    ..
                },
            ] => true,
            [
                b @ Token {
                    inner: T_DollarBraced(_, c),
                    ..
                },
            ] => {
                let var = get_braced_reference(&concat_oversimplify(c));
                (|| {
                    let cfga = params.cfg_analysis.as_ref()?;
                    let state = cfganalysis::get_incoming_state(cfga, b.id)?;
                    let value = state.variable(&var)?;
                    Some(
                        value.variable_value.numerical_status
                            >= NumericalStatus::NumericalStatusMaybe,
                    )
                })()
                .unwrap_or(false)
            }
            _ => match oversimplify(t).as_slice() {
                [v] => v.chars().all(|c| c.is_ascii_digit()),
                _ => false,
            },
        }
    };
    let eqv = |o: &str| -> &'static str {
        match o.trim_start_matches('\\') {
            "<" => "-lt",
            ">" => "-gt",
            "<=" => "-le",
            ">=" => "-ge",
            _ => "the numerical equivalent",
        }
    };
    let esc = if *typ == ConditionType::SingleBracket {
        "\\"
    } else {
        ""
    };
    let seqv = |o: &str| -> String {
        match o {
            "-ge" => format!("! a {esc}< b"),
            "-gt" => format!("{esc}>"),
            "-le" => format!("! a {esc}> b"),
            "-lt" => format!("{esc}<"),
            "-eq" => "=".to_string(),
            "-ne" => "!=".to_string(),
            _ => "the string equivalent".to_string(),
        }
    };
    let invert = |o: &str| -> &'static str {
        match o.trim_start_matches('\\') {
            "<=" => ">",
            ">=" => "<",
            _ => panic!("invert: unexpected operator"),
        }
    };
    if is_num(lhs) || is_num(rhs) {
        if is_lt_gt(op) {
            err(
                out,
                id,
                2071,
                &format!("{op} is for string comparisons. Use {} instead.", eqv(op)),
            );
        }
        if is_le_ge(op) && has_string_comparison {
            err(
                out,
                id,
                2071,
                &format!("{op} is not a valid operator. Use {} .", eqv(op)),
            );
        }
    } else {
        if is_le_ge(op) || is_lt_gt(op) {
            check_decimals(lhs, out);
            check_decimals(rhs, out);
        }
        if is_le_ge(op) && has_string_comparison {
            err(
                out,
                id,
                2122,
                &format!(
                    "{op} is not a valid operator. Use '! a {esc}{} b' instead.",
                    invert(op)
                ),
            );
        }
        if *typ == ConditionType::SingleBracket && (op == "<" || op == ">") {
            match params.shell_type {
                Shell::Sh => {}
                Shell::Dash | Shell::BusyboxSh => {
                    err(
                        out,
                        id,
                        2073,
                        &format!("Escape \\{op} to prevent it redirecting."),
                    );
                }
                _ => err(
                    out,
                    id,
                    2073,
                    &format!("Escape \\{op} to prevent it redirecting (or switch to [[ .. ]])."),
                ),
            }
        }
    }
    if ARITHMETIC_BINARY_TEST_OPS.contains(&op.as_str()) {
        check_decimals(lhs, out);
        check_decimals(rhs, out);
        let assigned: Vec<&String> = params
            .variable_flow
            .iter()
            .filter_map(|d| match d {
                StackData::Assignment((_, _, name, _)) => Some(name),
                _ => None,
            })
            .collect();
        for t in [lhs, rhs] {
            let as_string = get_literal_string_def("\0", t);
            let is_var = is_variable_name(&as_string);
            let kind = if is_var {
                "a variable"
            } else {
                "an arithmetic expression"
            };
            let fix = if is_var { "$var" } else { "$((expr))" };
            let is_non_num = !only_literal_string(t)
                .chars()
                .all(|x| x.is_ascii_digit() || "+-. ".contains(x));
            if !is_non_num {
                continue;
            }
            if *typ == ConditionType::SingleBracket {
                err(
                    out,
                    t.id,
                    2170,
                    &format!(
                        "Invalid number for {op}. Use {} to compare as string (or use {fix} to expand as {kind}).",
                        seqv(op)
                    ),
                );
            } else if !is_var
                || get_word_parts(t).iter().any(|x| is_quotes(x))
                || !assigned.iter().any(|n| **n == as_string)
            {
                warn(
                    out,
                    t.id,
                    2309,
                    &format!(
                        "{op} treats this as {kind}. Use {} to compare as string (or expand explicitly with {fix}).",
                        seqv(op)
                    ),
                );
            }
        }
    }
}

pub fn check_single_bracket_operators(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let TC_Binary(ConditionType::SingleBracket, op, _, _) = &t.inner
        && op == "=~"
        && matches!(params.shell_type, Shell::Bash | Shell::Ksh)
    {
        err(out, t.id, 2074, "Can't use =~ in [ ]. Use [[..]] instead.");
    }
}

pub fn check_double_bracket_operators(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let TC_Binary(ConditionType::DoubleBracket, op, _, _) = &t.inner
        && (op == "\\<" || op == "\\>")
    {
        err(
            out,
            t.id,
            2075,
            &format!("Escaping {op} is required in [..], but invalid in [[..]]"),
        );
    }
}

pub fn check_conditional_and_ors(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    match &t.inner {
        TC_And(ConditionType::SingleBracket, op, ..) if op == "&&" => {
            err(
                out,
                t.id,
                2107,
                "Instead of [ a && b ], use [ a ] && [ b ].",
            );
        }
        TC_And(ConditionType::DoubleBracket, op, ..) if op == "-a" => {
            err(out, t.id, 2108, "In [[..]], use && instead of -a.");
        }
        TC_Or(ConditionType::SingleBracket, op, ..) if op == "||" => {
            err(
                out,
                t.id,
                2109,
                "Instead of [ a || b ], use [ a ] || [ b ].",
            );
        }
        TC_Or(ConditionType::DoubleBracket, op, ..) if op == "-o" => {
            err(out, t.id, 2110, "In [[..]], use || instead of -o.");
        }
        TC_And(ConditionType::SingleBracket, op, ..) if op == "-a" => {
            warn(
                out,
                t.id,
                2166,
                "Prefer [ p ] && [ q ] as [ p -a q ] is not well defined.",
            );
        }
        TC_Or(ConditionType::SingleBracket, op, ..) if op == "-o" => {
            warn(
                out,
                t.id,
                2166,
                "Prefer [ p ] || [ q ] as [ p -o q ] is not well defined.",
            );
        }
        _ => {}
    }
}

pub fn check_quoted_cond_regex(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let TC_Binary(_, op, _, rhs) = &t.inner
        && op == "=~"
        && let T_NormalWord(parts) = &rhs.inner
        && let [
            Token {
                inner: T_DoubleQuoted(_) | T_SingleQuoted(_),
                ..
            },
        ] = parts.as_slice()
    {
        let is_constant_non_re =
            get_literal_string(rhs).is_some_and(|s| !METACHARS_RE.with(|re| re.is_match(&s)));
        if !is_constant_non_re {
            warn(
                out,
                rhs.id,
                2076,
                "Remove quotes from right-hand side of =~ to match as a regex rather than literally.",
            );
        }
    }
}

pub fn check_globbed_regex(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let TC_Binary(ConditionType::DoubleBracket, op, _, rhs) = &t.inner
        && op == "=~"
        && is_confused_glob_regex(&concat_oversimplify(rhs))
    {
        warn(
            out,
            rhs.id,
            2049,
            "=~ is for regex, but this looks like a glob. Use = instead.",
        );
    }
}

pub fn check_constant_ifs(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let TC_Binary(typ, op, lhs, rhs) = &t.inner else {
        return;
    };
    let is_dynamic = (ARITHMETIC_BINARY_TEST_OPS.contains(&op.as_str())
        && *typ == ConditionType::DoubleBracket)
        || ["-nt", "-ot", "-ef"].contains(&op.as_str());
    if is_dynamic {
        return;
    }
    if is_constant(lhs) && is_constant(rhs) {
        warn(
            out,
            t.id,
            2050,
            "This expression is constant. Did you forget the $ on a variable?",
        );
    } else if ["=", "==", "!="].contains(&op.as_str()) && !words_can_be_equal(lhs, rhs) {
        warn(
            out,
            t.id,
            2193,
            "The arguments to this comparison can never be equal. Make sure your syntax is correct.",
        );
    }
}

pub fn check_literal_breaking_test(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let has_equals = |t: &Token| get_literal_string(t).is_some_and(|s| s.contains('='));
    let is_non_empty = |t: &Token| get_literal_string(t).is_some_and(|s| !s.is_empty());
    let tautology = |w: &Token, s: &str, out: &mut Out| -> bool {
        match get_word_parts(w).into_iter().find(|x| is_non_empty(x)) {
            Some(token) => {
                err(out, token.id, 2157, s);
                true
            }
            None => false,
        }
    };
    match &t.inner {
        TC_Nullary(_, w) => {
            if let T_NormalWord(l) = &w.inner {
                if is_constant(w) {
                    return;
                }
                if let Some(token) = l.iter().find(|x| has_equals(x)) {
                    err(
                        out,
                        token.id,
                        2077,
                        "You need spaces around the comparison operator.",
                    );
                } else {
                    tautology(
                        w,
                        "Argument to implicit -n is always true due to literal strings.",
                        out,
                    );
                }
            }
        }
        TC_Unary(_, op, w) if matches!(w.inner, T_NormalWord(_)) => match op.as_str() {
            "-n" => {
                tautology(
                    w,
                    "Argument to -n is always true due to literal strings.",
                    out,
                );
            }
            "-z" => {
                tautology(
                    w,
                    "Argument to -z is always false due to literal strings.",
                    out,
                );
            }
            _ => {}
        },
        _ => {}
    }
}

pub fn check_constant_nullary(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let TC_Nullary(_, w) = &t.inner
        && is_constant(w)
    {
        match only_literal_string(w).as_str() {
            "false" => err(out, w.id, 2158, "[ false ] is true. Remove the brackets."),
            "0" => err(out, w.id, 2159, "[ 0 ] is true. Use 'false' instead."),
            "true" => style(out, w.id, 2160, "Instead of '[ true ]', just use 'true'."),
            "1" => style(out, w.id, 2161, "Instead of '[ 1 ]', use 'true'."),
            _ => err(
                out,
                w.id,
                2078,
                "This expression is constant. Did you forget a $ somewhere?",
            ),
        }
    }
}

pub fn check_div_before_mult(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let TA_Binary(op, a, y) = &t.inner
        && op == "*"
        && let TA_Binary(op2, _, x) = &a.inner
        && op2 == "/"
        && !has_floating_point(params)
        && **x != **y
    {
        info(
            out,
            a.id,
            2017,
            "Increase precision by replacing a/b*c with a*c/b.",
        );
    }
}

pub fn check_arithmetic_deref(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let TA_Expansion(parts) = &t.inner else {
        return;
    };
    let [
        Token {
            id,
            inner: T_DollarBraced(_, l),
        },
    ] = parts.as_slice()
    else {
        return;
    };
    let s = concat_oversimplify(l);
    let is_exception = s.is_empty()
        || s.chars().any(|c| "/.:#%?*@$-!+=^,".contains(c))
        || s.chars().next().is_some_and(|c| c.is_ascii_digit());
    if is_exception {
        return;
    }
    let Some(this) = params.id_map.get(&t.id) else {
        return;
    };
    for p in get_path(&params.parent_map, this) {
        match p.inner {
            T_Arithmetic(_) | T_DollarArithmetic(_) | T_ForArithmetic(..) | T_Assignment(..) => {
                style(
                    out,
                    *id,
                    2004,
                    "$/${} is unnecessary on arithmetic variables.",
                );
                return;
            }
            T_SimpleCommand(..) => return,
            _ => {}
        }
    }
}

pub fn check_arithmetic_bad_octal(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let TA_Expansion(_) = &t.inner
        && let Some(s) = get_literal_string(t)
        && OCTAL_RE.with(|re| re.is_match(&s))
    {
        err(
            out,
            t.id,
            2080,
            "Numbers with leading 0 are considered octal.",
        );
    }
}

pub fn check_comparison_against_glob(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let TC_Binary(typ, op, _, word) = &t.inner else {
        return;
    };
    let is_eq = ["=", "==", "!="].contains(&op.as_str());
    if *typ == ConditionType::DoubleBracket
        && is_eq
        && let T_NormalWord(parts) = &word.inner
        && let [
            Token {
                inner: T_DollarBraced(..),
                ..
            },
        ] = parts.as_slice()
    {
        warn(
            out,
            word.id,
            2053,
            &format!("Quote the right-hand side of {op} in [[ ]] to prevent glob matching."),
        );
        return;
    }
    if *typ == ConditionType::SingleBracket && is_eq && is_glob(word) {
        let msg = if matches!(params.shell_type, Shell::Bash | Shell::Ksh) {
            "[ .. ] can't match globs. Use [[ .. ]] or case statement."
        } else {
            "[ .. ] can't match globs. Use a case statement."
        };
        err(out, word.id, 2081, msg);
        return;
    }
    if *typ == ConditionType::DoubleBracket
        && params.shell_type == Shell::BusyboxSh
        && is_eq
        && is_glob(word)
    {
        err(
            out,
            word.id,
            2330,
            "BusyBox [[ .. ]] does not support glob matching. Use a case statement.",
        );
    }
}

pub fn check_case_against_glob(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_CaseExpression(_, cases) = &t.inner {
        for (_, list, _) in cases {
            for expr in list {
                if let T_NormalWord(l) = &expr.inner
                    && !is_glob(expr)
                    && l.iter().any(super::astlib::is_quoteable_expansion)
                {
                    warn(
                        out,
                        expr.id,
                        2254,
                        "Quote expansions in case patterns to match literally rather than as a glob.",
                    );
                }
            }
        }
    }
}

pub fn check_commarrays(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    fn literal(t: &Token) -> String {
        match &t.inner {
            T_IndexedElement(_, l) => literal(l),
            T_NormalWord(l) => l.iter().map(literal).collect(),
            T_Literal(s) => s.clone(),
            _ => String::new(),
        }
    }
    if let T_Array(l) = &t.inner
        && l.iter().any(|x| literal(x).contains(','))
    {
        warn(
            out,
            t.id,
            2054,
            "Use spaces, not commas, to separate array elements.",
        );
    }
}

/// `getExpr` of `checkOrNeq` / `checkAndEq`.
fn get_cond_expr(x: &Token, and: bool) -> Option<(&Token, &str, &Token)> {
    match &x.inner {
        T_OrIf(lhs, _) if !and => get_cond_expr(lhs, and),
        T_AndIf(lhs, _) if and => get_cond_expr(lhs, and),
        T_Pipeline(_, cmds) if cmds.len() == 1 => get_cond_expr(&cmds[0], and),
        T_Redirecting(_, c) | T_Condition(_, c) => get_cond_expr(c, and),
        TC_Binary(_, op, lhs, rhs) => match (is_constant(lhs), is_constant(rhs)) {
            (true, false) => Some((rhs, op, lhs)),
            (false, true) => Some((lhs, op, rhs)),
            _ => None,
        },
        _ => None,
    }
}

pub fn check_or_neq(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    match &t.inner {
        TC_Or(typ, _, a, b) => {
            if let (TC_Binary(_, op1, lhs1, rhs1), TC_Binary(_, op2, lhs2, rhs2)) =
                (&a.inner, &b.inner)
                && op1 == op2
                && (op1 == "-ne" || op1 == "!=")
                && lhs1 == lhs2
                && rhs1 != rhs2
                && !(is_glob(rhs1) || is_glob(rhs2))
            {
                warn(
                    out,
                    t.id,
                    2055,
                    &format!(
                        "You probably wanted {} here, otherwise it's always true.",
                        if *typ == ConditionType::SingleBracket {
                            "-a"
                        } else {
                            "&&"
                        }
                    ),
                );
            }
        }
        TA_Binary(op, a, b) if op == "||" => {
            if let (TA_Binary(o1, w1, _), TA_Binary(o2, w2, _)) = (&a.inner, &b.inner)
                && o1 == "!="
                && o2 == "!="
                && w1 == w2
            {
                warn(
                    out,
                    t.id,
                    2056,
                    "You probably wanted && here, otherwise it's always true.",
                );
            }
        }
        T_OrIf(lhs, rhs) => {
            if let (Some((lhs1, op1, rhs1)), Some((lhs2, op2, rhs2))) =
                (get_cond_expr(lhs, false), get_cond_expr(rhs, false))
                && op1 == op2
                && (op1 == "-ne" || op1 == "!=")
                && lhs1 == lhs2
                && rhs1 != rhs2
                && !(is_glob(rhs1) || is_glob(rhs2))
            {
                warn(
                    out,
                    t.id,
                    2252,
                    "You probably wanted && here, otherwise it's always true.",
                );
            }
        }
        _ => {}
    }
}

fn check_and_eq_operands(op: &str, rhs1: &Token, rhs2: &Token) -> bool {
    match op {
        "-eq" => is_literal_number(rhs1) && is_literal_number(rhs2),
        "=" | "==" => is_literal(rhs1) && is_literal(rhs2),
        _ => false,
    }
}

pub fn check_and_eq(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    match &t.inner {
        TC_And(typ, _, a, b) => {
            if let (TC_Binary(_, op1, lhs1, rhs1), TC_Binary(_, op2, lhs2, rhs2)) =
                (&a.inner, &b.inner)
                && op1 == op2
                && lhs1 == lhs2
                && rhs1 != rhs2
                && check_and_eq_operands(op1, rhs1, rhs2)
            {
                warn(
                    out,
                    t.id,
                    2333,
                    &format!(
                        "You probably wanted {} here, otherwise it's always false.",
                        if *typ == ConditionType::SingleBracket {
                            "-o"
                        } else {
                            "||"
                        }
                    ),
                );
            }
        }
        TA_Binary(op, a, b) if op == "&&" => {
            if let (TA_Binary(o1, lhs1, rhs1), TA_Binary(o2, lhs2, rhs2)) = (&a.inner, &b.inner)
                && o1 == "=="
                && o2 == "=="
                && lhs1 == lhs2
                && is_literal_number(rhs1)
                && is_literal_number(rhs2)
            {
                warn(
                    out,
                    t.id,
                    2334,
                    "You probably wanted || here, otherwise it's always false.",
                );
            }
        }
        T_AndIf(lhs, rhs) => {
            if let (Some((lhs1, op1, rhs1)), Some((lhs2, op2, rhs2))) =
                (get_cond_expr(lhs, true), get_cond_expr(rhs, true))
                && op1 == op2
                && lhs1 == lhs2
                && rhs1 != rhs2
                && check_and_eq_operands(op1, rhs1, rhs2)
            {
                warn(
                    out,
                    t.id,
                    2333,
                    "You probably wanted || here, otherwise it's always false.",
                );
            }
        }
        _ => {}
    }
}

pub fn check_valid_cond_ops(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    match &t.inner {
        TC_Binary(_, s, _, _) if !BINARY_TEST_OPS.contains(&s.as_str()) => {
            warn(out, t.id, 2057, "Unknown binary operator.");
        }
        TC_Unary(_, s, _) if !UNARY_TEST_OPS.contains(&s.as_str()) => {
            warn(out, t.id, 2058, "Unknown unary operator.");
        }
        _ => {}
    }
}

pub fn check_uuoe_var(_: &Parameters<'_>, p: &Token, out: &mut Out) {
    fn could_be_optimized(f: &Token) -> bool {
        match &f.inner {
            T_Glob(_) | T_Extglob(..) | T_BraceExpansion(_) => false,
            T_NormalWord(l) | T_DoubleQuoted(l) => l.iter().all(could_be_optimized),
            _ => true,
        }
    }
    let cmd = match &p.inner {
        T_Backticked(c) | T_DollarExpansion(c) if c.len() == 1 => &c[0],
        _ => return,
    };
    let T_Pipeline(_, cmds) = &cmd.inner else {
        return;
    };
    let [
        Token {
            inner: T_Redirecting(_, c),
            ..
        },
    ] = cmds.as_slice()
    else {
        return;
    };
    check_unqualified_command("echo", c, &mut |_, vars| {
        if let Some((first, rest)) = vars.split_first() {
            let is_covered =
                rest.is_empty() && super::analyzerlib::token_is_just_command_output(first);
            if !(is_covered || only_literal_string(first).starts_with('-'))
                && vars.iter().all(could_be_optimized)
            {
                style(
                    out,
                    p.id,
                    2116,
                    "Useless echo? Instead of 'cmd $(echo foo)', just use 'cmd foo'.",
                );
            }
        }
    });
}

pub fn check_test_redirects(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_Redirecting(redirs, cmd) = &t.inner
        && is_command(cmd, "test")
    {
        for r in redirs {
            let suspicious = match &r.inner {
                T_FdRedirect(fd, x) => match &x.inner {
                    T_IoFile(op, _) => fd != "2" && matches!(op.inner, T_Greater | T_Less),
                    _ => false,
                },
                _ => false,
            };
            if suspicious {
                warn(
                    out,
                    r.id,
                    2065,
                    "This is interpreted as a shell file redirection, not a comparison.",
                );
            }
        }
    }
}

pub fn check_ps1_assignments(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_Assignment(_, name, _, word) = &t.inner
        && name == "PS1"
    {
        let contents = concat_oversimplify(word);
        let unenclosed = ENCLOSED_RE.with(|re| re.replace_all(&contents, ""));
        if ESCAPE_RE.with(|re| re.match_groups(&unenclosed)).is_some() {
            info(
                out,
                word.id,
                2025,
                "Make sure all escape sequences are enclosed in \\[..\\] to prevent line wrapping issues",
            );
        }
    }
}

pub fn check_backticks(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_Backticked(list) = &t.inner
        && !list.is_empty()
    {
        out.push(make_comment_with_fix(
            Severity::StyleC,
            t.id,
            2006,
            "Use $(...) notation instead of legacy backticks `...`.",
            fix_with(vec![
                replace_start(t.id, params, 1, "$("),
                replace_end(t.id, params, 1, ")"),
            ]),
        ));
    }
}

pub fn check_bad_parameter_substitution(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_DollarBraced(_, w) = &t.inner else {
        return;
    };
    let T_NormalWord(contents) = &w.inner else {
        return;
    };
    let Some(first) = contents.first() else {
        return;
    };
    let indirection_part = |t: &Token| -> Option<bool> {
        match &t.inner {
            T_DollarExpansion(_) | T_Backticked(_) | T_DollarBraced(..) | T_DollarArithmetic(_) => {
                Some(true)
            }
            T_Literal(s) => {
                if s.chars().all(is_variable_char) {
                    None
                } else {
                    Some(false)
                }
            }
            _ => Some(false),
        }
    };
    let list: Vec<bool> = contents.iter().filter_map(indirection_part).collect();
    if !list.is_empty() && list.iter().all(|b| *b) {
        err(
            out,
            t.id,
            2082,
            "To expand via indirection, use arrays, ${!name} or (for sh only) eval.",
        );
        return;
    }
    let is_variable = |s: &str| -> bool {
        let cs: Vec<char> = s.chars().collect();
        match cs.as_slice() {
            [c] => {
                super::astlib::is_variable_start_char(*c)
                    || super::astlib::is_special_variable_char(*c)
                    || c.is_ascii_digit()
            }
            _ => is_variable_name(s),
        }
    };
    match &first.inner {
        T_Literal(s) if !s.is_empty() => {
            let c = s.chars().next().expect("non-empty");
            if !(is_variable_char(c) || super::astlib::is_special_variable_char(c)) {
                err(
                    out,
                    first.id,
                    2296,
                    &format!(
                        "Parameter expansions can't start with {}. Double check syntax.",
                        e4m(&c.to_string())
                    ),
                );
            }
        }
        T_ParamSubSpecialChar(_) => {}
        T_DoubleQuoted(inner) if matches!(inner.as_slice(), [Token { inner: T_Literal(s), .. }] if is_variable(s)) =>
        {
            err(
                out,
                first.id,
                2297,
                "Double quotes must be outside ${}: ${\"invalid\"} vs \"${valid}\".",
            );
        }
        T_DollarBraced(braces, _) if super::astlib::is_unmodified_parameter_expansion(first) => {
            err(
                out,
                first.id,
                2298,
                &format!(
                    "{} is invalid. For expansion, use ${{x}}. For indirection, use arrays, ${{!x}} or (for sh) eval.",
                    if *braces { "${${x}}" } else { "${$x}" }
                ),
            );
        }
        T_DollarBraced(..) => {
            err(
                out,
                first.id,
                2299,
                "Parameter expansions can't be nested. Use temporary variables.",
            );
        }
        _ if super::astlib::is_command_substitution(first) => {
            err(
                out,
                first.id,
                2300,
                "Parameter expansion can't be applied to command substitutions. Use temporary variables.",
            );
        }
        _ => {
            let name = match first.inner {
                T_SingleQuoted(_) | T_DoubleQuoted(_) => "quotes",
                _ => "syntax",
            };
            err(
                out,
                first.id,
                2301,
                &format!("Parameter expansion starts with unexpected {name}. Double check syntax."),
            );
        }
    }
}

pub fn check_inexplicably_unquoted(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_NormalWord(tokens) = &t.inner else {
        return;
    };
    let quotes_single_thing = |x: &[Token]| {
        matches!(
            x,
            [Token {
                inner: T_DollarExpansion(_) | T_DollarBraced(..) | T_Backticked(_),
                ..
            }]
        )
    };
    let is_special = |path: &[&Token]| -> bool {
        let mut i = 0;
        loop {
            match path.get(i) {
                None => return false,
                Some(t) => match &t.inner {
                    T_Redirecting(..) => return false,
                    T_DollarBraced(..) => return true,
                    _ => {
                        if let Some(next) = path.get(i + 1)
                            && let TC_Binary(_, op, _, rhs) = &next.inner
                            && op == "=~"
                        {
                            return t.id == rhs.id;
                        }
                        i += 1;
                    }
                },
            }
        }
    };
    for k in 0..tokens.len() {
        let rest = &tokens[k..];
        match rest {
            [
                Token {
                    inner: T_SingleQuoted(_),
                    ..
                },
                Token {
                    id,
                    inner: T_Literal(s),
                },
                ..,
            ] if !s.is_empty() && s.chars().all(super::hchar::is_alpha_num) => {
                info(
                    out,
                    *id,
                    2026,
                    "This word is outside of quotes. Did you intend to 'nest '\"'single quotes'\"' instead'? ",
                );
            }
            [
                Token {
                    inner: T_DoubleQuoted(before),
                    ..
                },
                trapped,
                Token {
                    inner: T_DoubleQuoted(after),
                    ..
                },
                ..,
            ] => match &trapped.inner {
                T_DollarExpansion(_) | T_DollarBraced(..) => {
                    warn(
                        out,
                        trapped.id,
                        2027,
                        "The surrounding quotes actually unquote this. Remove or escape them.",
                    );
                }
                T_Literal(s) => {
                    let path: Vec<&Token> = match params.id_map.get(&trapped.id) {
                        Some(node) => get_path(&params.parent_map, node),
                        None => vec![trapped],
                    };
                    if !((quotes_single_thing(before) && quotes_single_thing(after))
                        || ["=", ":", "/"].contains(&s.as_str())
                        || is_special(&path))
                    {
                        warn(
                            out,
                            trapped.id,
                            2140,
                            "Word is of the form \"A\"B\"C\" (B indicated). Did you mean \"ABC\" or \"A\\\"B\\\"C\"?",
                        );
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

pub fn check_tilde_in_quotes(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_NormalWord(parts) = &t.inner else {
        return;
    };
    let verify = |id: Id, s: &str, out: &mut Out| {
        if s.starts_with("~/") {
            warn(out, id, 2088, "Tilde does not expand in quotes. Use $HOME.");
        }
    };
    match parts.first() {
        Some(Token {
            id,
            inner: T_SingleQuoted(s),
        }) => verify(*id, s, out),
        Some(Token {
            inner: T_DoubleQuoted(inner),
            ..
        }) => {
            if let Some(Token {
                id,
                inner: T_Literal(s),
            }) = inner.first()
            {
                verify(*id, s, out);
            }
        }
        _ => {}
    }
}

pub fn check_lonely_dot_dash(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if matches!(t.inner, T_Redirecting(..)) && is_unqualified_command(t, "./") {
        err(
            out,
            t.id,
            2083,
            "Don't add spaces after the slash in './file'.",
        );
    }
}

pub fn check_spurious_exec(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if params.has_execfail {
        return;
    }
    let cleanup = |c: &Token| match &c.inner {
        T_Pipeline(_, cmds) if cmds.len() == 1 => {
            is_command_match(&cmds[0], &|s| {
                [":", "echo", "exit", "printf", "return"].contains(&s)
            }) || is_assignment(&cmds[0])
        }
        _ => false,
    };
    let comment_if_exec = |c: &Token, out: &mut Out| {
        let c = match &c.inner {
            T_Pipeline(_, cmds) if cmds.len() == 1 => &cmds[0],
            _ => c,
        };
        if let T_Redirecting(_, inner) = &c.inner
            && let T_SimpleCommand(_, words) = &inner.inner
            && words.len() >= 2
            && get_literal_string(&words[0]).as_deref() == Some("exec")
        {
            warn(
                out,
                inner.id,
                2093,
                "Remove \"exec \" if script should continue after this command.",
            );
        }
    };
    let do_list = |cmds: &[Token], in_loop: bool, out: &mut Out| {
        let mut end = cmds.len();
        while end > 0 && cleanup(&cmds[end - 1]) {
            end -= 1;
        }
        let list = &cmds[..end];
        // doList' (current:t@(following:_)) False / doList' (current:tail) True.
        // Each step recurses through doList, which strips trailing cleanup again;
        // stripping is idempotent on a suffix of an already-stripped list.
        for (i, c) in list.iter().enumerate() {
            if in_loop || i + 1 < list.len() {
                comment_if_exec(c, out);
            }
        }
    };
    match &t.inner {
        T_Script(_, cmds) | T_BraceGroup(cmds) => do_list(cmds, false, out),
        T_WhileExpression(_, cmds)
        | T_UntilExpression(_, cmds)
        | T_ForIn(_, _, cmds)
        | T_ForArithmetic(_, _, _, cmds) => {
            do_list(cmds, true, out);
        }
        T_IfExpression(thens, elses) => {
            for (_, l) in thens {
                do_list(l, false, out);
            }
            do_list(elses, false, out);
        }
        _ => {}
    }
}

pub fn check_spurious_expansion(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_SimpleCommand(_, words) = &t.inner
        && let [
            Token {
                inner: T_NormalWord(parts),
                ..
            },
        ] = words.as_slice()
        && let [word] = parts.as_slice()
    {
        match &word.inner {
            T_DollarExpansion(_) => warn(
                out,
                word.id,
                2091,
                "Remove surrounding $() to avoid executing output (or use eval if intentional).",
            ),
            T_Backticked(_) => warn(
                out,
                word.id,
                2092,
                "Remove backticks to avoid executing output (or use eval if intentional).",
            ),
            T_DollarArithmetic(_) => err(
                out,
                word.id,
                2084,
                "Remove '$' or use '_=$((expr))' to avoid executing output.",
            ),
            _ => {}
        }
    }
}

pub fn check_dollar_brackets(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_DollarBracket(_) = &t.inner {
        style(out, t.id, 2007, "Use $((..)) instead of deprecated $[..]");
    }
}

pub fn check_ssh_here_doc(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_Redirecting(redirs, cmd) = &t.inner
        && is_command(cmd, "ssh")
    {
        for r in redirs {
            if let T_FdRedirect(_, h) = &r.inner
                && let T_HereDoc(_, super::ast::Quoted::Unquoted, token, tokens) = &h.inner
                && !tokens.iter().all(is_constant)
            {
                warn(
                    out,
                    h.id,
                    2087,
                    &format!(
                        "Quote '{}' to make here document expansions happen on the server side rather than on the client.",
                        e4m(token)
                    ),
                );
            }
        }
    }
}

/// `subshellAssignmentCheck`.
pub fn subshell_assignment_check(params: &Parameters<'_>, _: &Token) -> Out {
    #[derive(Clone)]
    enum VarState {
        Dead(Id, &'static str),
        Alive,
    }
    let mut out = Vec::new();
    let mut scopes: Vec<(&'static str, Vec<(String, Id)>)> = vec![("oops", Vec::new())];
    let mut dead_vars: BTreeMap<String, VarState> = BTreeMap::new();
    for x in &params.variable_flow {
        match x {
            StackData::Assignment((_, token, s, data)) => {
                if super::analyzerlib::is_true_assignment_source(data) {
                    scopes
                        .last_mut()
                        .expect("a scope")
                        .1
                        .push((s.clone(), token.id));
                    dead_vars.insert(s.clone(), VarState::Alive);
                }
            }
            StackData::Reference((_, read_token, s)) => {
                if !["@", "*", "IFS"].contains(&s.as_str())
                    && let Some(VarState::Dead(write_id, reason)) = dead_vars.get(s)
                {
                    info(
                        &mut out,
                        *write_id,
                        2030,
                        &format!("Modification of {s} is local (to subshell caused by {reason})."),
                    );
                    info(
                        &mut out,
                        read_token.id,
                        2031,
                        &format!("{s} was modified in a subshell. That change might be lost."),
                    );
                }
            }
            StackData::StackScope(super::analyzerlib::Scope::SubshellScope(reason)) => {
                scopes.push((reason, Vec::new()));
            }
            StackData::StackScope(super::analyzerlib::Scope::NoneScope) => {}
            StackData::StackScopeEnd => {
                // findSubshelled has no equation for an end without a scope.
                let (reason, scope) = scopes
                    .pop()
                    .expect("ShellCheck: unbalanced subshell scopes");
                // The Haskell scope list is newest first, folded from the left:
                // the earliest assignment of a variable is inserted last and wins.
                for (var, id) in scope.into_iter().rev() {
                    dead_vars.insert(var, VarState::Dead(id, reason));
                }
            }
        }
    }
    out
}

/// `quotesMayConflictWithSC2281`.
pub fn quotes_may_conflict_with_sc2281(params: &Parameters<'_>, t: &Token) -> bool {
    let parents = get_parents_of_id(&params.parent_map, t.id);
    if let [parent, grand, ..] = parents.as_slice()
        && let T_NormalWord(parts) = &parent.inner
        && let [
            me,
            Token {
                inner: T_Literal(s),
                ..
            },
            ..,
        ] = parts.as_slice()
        && s.starts_with('=')
        && let T_SimpleCommand(_, words) = &grand.inner
        && let Some(cmd) = words.first()
    {
        return t.id == me.id && parent.id == cmd.id;
    }
    false
}

pub fn check_spacefulness_cfg(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    check_spacefulness_cfg_ext(true, params, t, out);
}

pub fn check_verbose_spacefulness_cfg(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    check_spacefulness_cfg_ext(false, params, t, out);
}

fn check_spacefulness_cfg_ext(
    dirty_pass: bool,
    params: &Parameters<'_>,
    token: &Token,
    out: &mut Out,
) {
    let T_DollarBraced(_, list) = &token.inner else {
        return;
    };
    let id = token.id;
    let braced_string = concat_oversimplify(list);
    let name = get_braced_reference(&braced_string);
    let parents = &params.parent_map;
    let needs_quoting = !is_array_expansion(token)
        && !is_counting_reference(token)
        && !is_quote_free(params.shell_type, parents, token)
        && !is_quoted_alternative_reference(token)
        && !used_as_command_name(parents, token);
    let is_clean = (|| {
        let cfga = params.cfg_analysis.as_ref()?;
        let state = cfganalysis::get_incoming_state(cfga, id)?;
        let value = state.variable(&name)?;
        Some(
            value
                .variable_properties
                .iter()
                .all(|s| s.contains(&super::cfg::CfVariableProp::CFVPInteger))
                || value.variable_value.space_status == cfganalysis::SpaceStatus::SpaceStatusClean,
        )
    })()
    .unwrap_or(false);
    if !(needs_quoting && (dirty_pass != is_clean)) {
        return;
    }
    if super::data::SPECIAL_VARIABLES_WITHOUT_SPACES.contains(&name.as_str())
        || quotes_may_conflict_with_sc2281(params, token)
    {
        return;
    }
    if dirty_pass {
        let modifier = get_braced_modifier(&braced_string);
        let is_default_assignment = (modifier.starts_with('=') || modifier.starts_with(":="))
            && super::analyzerlib::is_param_to(parents, ":", token);
        if is_default_assignment {
            info(
                out,
                id,
                2223,
                "This default assignment may cause DoS due to globbing. Quote it.",
            );
        } else {
            super::analyzerlib::info_with_fix(
                out,
                id,
                2086,
                "Double quote to prevent globbing and word splitting.",
                surround_with(id, params, "\""),
            );
        }
    } else {
        super::analyzerlib::style_with_fix(
            out,
            id,
            2248,
            "Prefer double quoting even when variables don't contain special characters.",
            surround_with(id, params, "\""),
        );
    }
}

pub fn check_variable_braces(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_DollarBraced(false, l) = &t.inner {
        let name = get_braced_reference(&concat_oversimplify(l));
        if !super::data::UNBRACED_VARIABLES.contains(&name.as_str())
            && !quotes_may_conflict_with_sc2281(params, t)
        {
            super::analyzerlib::style_with_fix(
                out,
                t.id,
                2250,
                "Prefer putting braces around variable references even when not strictly required.",
                fix_with(vec![
                    replace_start(t.id, params, 1, "${"),
                    replace_end(t.id, params, 0, "}"),
                ]),
            );
        }
    }
}

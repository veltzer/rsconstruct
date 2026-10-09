//! `ShellCheck`'s general checks (`ShellCheck.Analytics`), part three: the
//! checks that run on the whole tree, the optional checks, and the lists.

use std::collections::{BTreeMap, BTreeSet};

use super::analytics::{
    NodeCheck, TreeCheck, aliases, check_and_eq, check_arithmetic_bad_octal,
    check_arithmetic_deref, check_arithmetic_op_command, check_array_as_string,
    check_array_without_index, check_assign_ate_command, check_backticks,
    check_bad_parameter_substitution, check_case_against_glob, check_commarrays,
    check_comparison_against_glob, check_concatenated_dollar_at, check_conditional_and_ors,
    check_constant_ifs, check_constant_nullary, check_div_before_mult, check_dollar_brackets,
    check_dollar_star, check_double_bracket_operators, check_echo_wc, check_find_exec,
    check_for_in_cat, check_for_in_ls, check_for_in_quoted, check_globbed_regex,
    check_inexplicably_unquoted, check_literal_breaking_test, check_lonely_dot_dash,
    check_number_comparisons, check_or_neq, check_pipe_pitfalls, check_piped_assignment,
    check_ps1_assignments, check_quoted_cond_regex, check_redirect_to_same, check_shebang,
    check_shebang_parameters, check_shorthand_if, check_single_bracket_operators,
    check_single_quoted_variables, check_spacefulness_cfg, check_spurious_exec,
    check_spurious_expansion, check_ssh_here_doc, check_stderr_redirect, check_test_redirects,
    check_tilde_in_quotes, check_unquoted_dollar_at, check_unquoted_expansions, check_unquoted_n,
    check_uuoc, check_uuoe_var, check_valid_cond_ops, check_variable_braces,
    check_verbose_spacefulness_cfg, check_wrong_arithmetic_assignment, do_variable_flow_analysis,
    fix_with, functions, is_condition, node_checks_to_tree_check, replace_end, replace_start,
    subshell_assignment_check, surround_with,
};
use super::analytics2::{
    check_assign_to_self, check_bad_test_and_or, check_bats_test_does_not_use_negation,
    check_blatant_recursion, check_cd_and_back, check_char_range_glob,
    check_command_is_unreachable, check_command_with_trailing_symbol,
    check_comparison_with_leading_x, check_default_case, check_dollar_quote_paren,
    check_empty_condition, check_equals_in_command, check_expansion_with_redirection,
    check_flag_as_command, check_for_loop_glob_variables, check_function_declarations,
    check_glob_as_command, check_globs_as_options, check_loop_keyword_scope,
    check_loop_variable_reassignment, check_modified_arithmetic_in_redirection,
    check_multiple_appends, check_nullary_expansion_test, check_overriding_path,
    check_overwritten_exit_code, check_pipe_to_nowhere, check_plus_equals_number,
    check_prefix_assignment_reference, check_read_without_r, check_redirected_nowhere,
    check_redirection_to_command, check_redirection_to_number, check_return_against_zero,
    check_second_arg_is_comparison, check_should_use_grep_q, check_splitting_in_arrays,
    check_stderr_pipe, check_subshell_as_test, check_subshelled_tests, check_suspicious_ifs,
    check_test_argument_splitting, check_tilde_in_path, check_trailing_bracket,
    check_translated_string_variable, check_unary_test_a, check_unmatchable_cases,
    check_unnecessarily_inverted_test, check_unnecessary_arithmetic_expansion_index,
    check_unnecessary_parens, check_unquoted_parameter_expansion_pattern, check_unsupported,
    check_useless_bang, check_while_read_pitfalls,
};
use super::analyzerlib::{
    DataSource, DataType, Out, Parameters, StackData, get_parents_of_id, get_path,
    get_variable_flow, info, is_param_to, is_quote_free, make_comment, should_ignore_code,
    style_with_fix, supports_arrays, warn, warn_with_fix,
};
use super::ast::{ConditionType, Id, Inner, Token, do_analysis, do_transform};
use super::astlib::{
    basename, concat_oversimplify, e4m, get_all_flags, get_associative_arrays, get_braced_modifier,
    get_braced_reference, get_command, get_command_basename, get_command_name,
    get_command_name_and_token, get_command_sequences, get_literal_string, get_literal_string_def,
    get_unquoted_literal, get_word_parts, is_variable_char, is_variable_name, only_literal_string,
    oversimplify,
};
use super::cfganalysis;
use super::data::{COMMON_COMMANDS, DECLARING_COMMANDS, INTERNAL_VARIABLES};
use super::hchar;
use super::interface::Shell;
use super::regex::{Regex, mk_regex};

use Inner::{
    T_AndIf, T_Arithmetic, T_Array, T_Assignment, T_Backticked, T_Banged, T_Condition,
    T_DollarBraced, T_DollarExpansion, T_DoubleQuoted, T_ForIn, T_Function, T_HereDoc,
    T_HereString, T_IfExpression, T_IndexedElement, T_Literal, T_NormalWord, T_OrIf, T_Pipeline,
    T_ProcSub, T_Redirecting, T_Script, T_SimpleCommand, T_SourceCommand, T_UntilExpression,
    T_WhileExpression, TA_Sequence, TA_Variable, TC_Binary, TC_Nullary, TC_Unary,
};

thread_local! {
    static QUOTE_RE: Regex = mk_regex("\"|([/= ]|^)'|'( |$)|\\\\ ");
    static GUARD_RE: Regex = mk_regex("^(\\[.*\\])?:?[-?]");
    static SAFE_DIR_RE: Regex = mk_regex("^/*((\\.|\\.\\.)/+)*(\\.|\\.\\.)?$");
}

pub fn check_quotes_in_literals(params: &Parameters<'_>, _: &Token) -> Out {
    let parents = &params.parent_map;
    let contains_quotes = |s: &str| QUOTE_RE.with(|re| re.is_match(s));
    fn for_token(
        map: &BTreeMap<String, Id>,
        t: &Token,
        contains_quotes: &dyn Fn(&str) -> bool,
    ) -> Option<Id> {
        match &t.inner {
            T_DollarBraced(_, l) => map.get(&concat_oversimplify(l)).copied(),
            T_DoubleQuoted(tokens) | T_NormalWord(tokens) => tokens
                .iter()
                .find_map(|x| for_token(map, x, contains_quotes)),
            _ => {
                if contains_quotes(&concat_oversimplify(t)) {
                    Some(t.id)
                } else {
                    None
                }
            }
        }
    }
    let squashes_quotes = |t: &Token| matches!(&t.inner, T_DollarBraced(_, l) if concat_oversimplify(l).starts_with('#'));
    let suggestion = if supports_arrays(params.shell_type) {
        "Use an array."
    } else {
        "Rewrite using set/\"$@\" or functions."
    };
    let mut state: BTreeMap<String, Id> = BTreeMap::new();
    do_variable_flow_analysis(
        &mut |m: &mut BTreeMap<String, Id>, _base, expr, name| {
            let Some(j) = m.get(name).copied() else {
                return Vec::new();
            };
            if !is_param_to(parents, "eval", expr)
                && !is_quote_free(params.shell_type, parents, expr)
                && !squashes_quotes(expr)
            {
                vec![
                    make_comment(
                        super::interface::Severity::WarningC,
                        j,
                        2089,
                        &format!("Quotes/backslashes will be treated literally. {suggestion}"),
                    ),
                    make_comment(
                        super::interface::Severity::WarningC,
                        expr.id,
                        2090,
                        "Quotes/backslashes in this variable will not be respected.",
                    ),
                ]
            } else {
                Vec::new()
            }
        },
        &mut |m: &mut BTreeMap<String, Id>, _base, _t, name, data| {
            if let DataType::DataString(DataSource::SourceFrom(values)) = data {
                let quoted = values
                    .iter()
                    .find_map(|v| for_token(m, v, &contains_quotes));
                match quoted {
                    None => {
                        m.remove(name);
                    }
                    Some(x) => {
                        m.insert(name.to_string(), x);
                    }
                }
            }
            Vec::new()
        },
        &mut state,
        &params.variable_flow,
    )
}

pub fn check_functions_used_externally(params: &Parameters<'_>, t: &Token) -> Out {
    let mut functions_and_aliases = functions(t);
    for (k, v) in aliases(t) {
        functions_and_aliases.entry(k).or_insert(v);
    }
    let pattern_context = |id: Id| -> String {
        match params.token_positions.get(&id) {
            Some((start, _)) => format!(" on line {}.", start.line),
            None => ".".to_string(),
        }
    };
    let mut out = Vec::new();
    do_analysis(t, &mut |x| {
        let T_SimpleCommand(_, argv) = &x.inner else {
            return;
        };
        let (Some(s), tok) = get_command_name_and_token(false, x) else {
            return;
        };
        let name = basename(&s).to_string();
        let rest: Vec<&Token> = match argv.iter().position(|c| c.id == tok.id) {
            Some(i) => argv[i + 1..].iter().collect(),
            None => Vec::new(),
        };
        let arg_strings: Vec<(String, &Token)> =
            rest.iter().map(|a| (only_literal_string(a), *a)).collect();
        fn drop_flags<'t>(l: &[(String, &'t Token)]) -> Vec<(String, &'t Token)> {
            l.iter()
                .skip_while(|x| x.0.starts_with('-'))
                .cloned()
                .collect()
        }
        let candidates: Vec<(String, &Token)> = match name.as_str() {
            "chroot" | "screen" | "sudo" | "doas" | "run0" | "xargs" | "tmux" => {
                drop_flags(&arg_strings).into_iter().take(1).collect()
            }
            "ssh" => drop_flags(&arg_strings)
                .into_iter()
                .skip(1)
                .take(1)
                .collect(),
            "find" => arg_strings
                .iter()
                .skip_while(|x| !["-exec", "-execdir", "-ok"].contains(&x.0.as_str()))
                .skip(1)
                .take(1)
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        for (_, arg) in candidates {
            let Some(literal_arg) = get_unquoted_literal(arg) else {
                continue;
            };
            let Some(definition_id) = functions_and_aliases.get(&literal_arg) else {
                continue;
            };
            warn(
                &mut out,
                arg.id,
                2033,
                "Shell functions can't be passed to external commands. Use separate script or sh -c.",
            );
            info(
                &mut out,
                *definition_id,
                2032,
                &format!(
                    "This function can't be invoked via {name}{}",
                    pattern_context(tok.id)
                ),
            );
        }
    });
    out
}

pub fn check_unused_assignments(params: &Parameters<'_>, _: &Token) -> Out {
    let flow = &params.variable_flow;
    let strip_suffix =
        |s: &str| -> String { s.chars().take_while(|&c| is_variable_char(c)).collect() };
    let mut references: BTreeSet<String> = BTreeSet::new();
    for d in flow {
        if let StackData::Reference((_, _, name)) = d {
            references.insert(strip_suffix(name));
        }
    }
    for v in INTERNAL_VARIABLES {
        references.insert((*v).to_string());
    }
    let mut assignments: BTreeMap<String, Id> = BTreeMap::new();
    for d in flow {
        if let StackData::Assignment((_, token, name, _)) = d
            && is_variable_name(name)
        {
            assignments.insert(name.clone(), token.id);
        }
    }
    let mut out = Vec::new();
    for (name, id) in assignments {
        if references.contains(&name) || name.starts_with('_') {
            continue;
        }
        warn(
            &mut out,
            id,
            2034,
            &format!("{name} appears unused. Verify use (or export if used externally)."),
        );
    }
    out
}

/// Levenshtein distance (`dist`).
fn dist(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = if a[i - 1] == b[j - 1] {
                prev[j - 1]
            } else {
                1 + prev[j - 1].min(prev[j]).min(cur[j - 1])
            };
        }
        prev = cur;
    }
    prev[b.len()]
}

pub fn check_unassigned_references(params: &Parameters<'_>, t: &Token) -> Out {
    check_unassigned_references_ext(false, params, t)
}

pub fn check_unassigned_references_globals(params: &Parameters<'_>, t: &Token) -> Out {
    check_unassigned_references_ext(true, params, t)
}

fn check_unassigned_references_ext(
    include_globals: bool,
    params: &Parameters<'_>,
    _: &Token,
) -> Out {
    let mut read_map: BTreeMap<String, Id> = BTreeMap::new();
    let mut write_map: BTreeSet<String> = BTreeSet::new();
    for d in &params.variable_flow {
        match d {
            StackData::Assignment((_, _, name, _)) => {
                write_map.insert(name.clone());
            }
            StackData::Reference((_, place, name)) => {
                read_map.entry(name.clone()).or_insert(place.id);
            }
            _ => {}
        }
    }
    let default_assigned: BTreeSet<&str> = INTERNAL_VARIABLES
        .iter()
        .copied()
        .filter(|s| !s.is_empty())
        .collect();
    let unassigned: Vec<(&String, &Id)> = read_map
        .iter()
        .filter(|(k, _)| !write_map.contains(*k) && !default_assigned.contains(k.as_str()))
        .collect();
    let written_vars: Vec<&String> = write_map.iter().filter(|v| is_variable_name(v)).collect();
    let match_score = |var: &str, candidate: &str| -> usize {
        if var != candidate && hchar::lower_string(var) == hchar::lower_string(candidate) {
            1
        } else {
            dist(var, candidate)
        }
    };
    let get_best_match = |var: &str| -> Option<String> {
        let mut matches: Vec<(&String, usize)> = written_vars
            .iter()
            .map(|x| (*x, match_score(var, x)))
            .collect();
        matches.sort_by_key(|(_, s)| *s);
        let (m, score) = matches.first()?;
        let l = m.chars().count();
        if (l > 3 && *score <= 1) || (l > 7 && *score <= 2) {
            Some((*m).clone())
        } else {
            None
        }
    };
    let is_exception = |var: &str, id: Id| -> bool {
        let Some(this) = params.id_map.get(&id) else {
            return false;
        };
        get_path(&params.parent_map, this)
            .iter()
            .any(|t| match &t.inner {
                T_DollarBraced(_, l) => {
                    let s = concat_oversimplify(l);
                    let r = get_braced_reference(&s);
                    let m = get_braced_modifier(&s);
                    r != var || m.starts_with('+') || m.starts_with(":+")
                }
                _ => false,
            })
    };
    let is_guarded = |id: Id| -> bool {
        let Some(t) = params.id_map.get(&id) else {
            return false;
        };
        match &t.inner {
            T_DollarBraced(_, v) => {
                let name = concat_oversimplify(v);
                let rest: String = name
                    .trim_start_matches(['#', '!'])
                    .chars()
                    .skip_while(|&c| is_variable_char(c))
                    .collect();
                GUARD_RE.with(|re| re.is_match(&rest))
            }
            _ => false,
        }
    };
    let mut out = Vec::new();
    for (var, place) in unassigned {
        if !is_variable_name(var) {
            continue;
        }
        if is_exception(var, *place) || is_guarded(*place) {
            continue;
        }
        let is_local = var.chars().any(hchar::is_lower);
        if include_globals || is_local {
            let optional_tip = if COMMON_COMMANDS.contains(&var.as_str()) {
                format!(" (for output from commands, use \"$({var} ...)\" )")
            } else {
                get_best_match(var)
                    .map(|m| format!(" (did you mean '{m}'?)"))
                    .unwrap_or_default()
            };
            warn(
                &mut out,
                *place,
                2154,
                &format!("{var} is referenced but not assigned{optional_tip}."),
            );
        } else if let Some(m) = get_best_match(var) {
            info(
                &mut out,
                *place,
                2153,
                &format!("Possible misspelling: {var} may not be assigned. Did you mean {m}?"),
            );
        }
    }
    out
}

pub fn check_unpassed_in_functions(params: &Parameters<'_>, root: &Token) -> Out {
    let is_positional = |s: &str| {
        s == "*"
            || s == "@"
            || s == "#"
            || (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) && s != "0")
    };
    let has_default_value = |t: &Token| match &t.inner {
        T_DollarBraced(true, l) => {
            let m: Vec<char> = get_braced_modifier(&concat_oversimplify(l))
                .chars()
                .collect();
            match m.as_slice() {
                [':', c, ..] | [c, ..] => "-+?".contains(*c),
                [] => false,
            }
        }
        _ => false,
    };
    let is_direct_child_of = |child: &Token, parent: &Token| -> bool {
        let Some(this) = params.id_map.get(&child.id) else {
            return false;
        };
        get_path(&params.parent_map, this)
            .iter()
            .find(|x| matches!(x.inner, T_Function(..) | T_Script(..)))
            .is_some_and(|f| f.id == parent.id)
    };
    let mut function_list: Vec<(String, &Token)> = Vec::new();
    do_analysis(root, &mut |t| {
        if let T_Function(_, _, name, body) = &t.inner {
            let flow =
                get_variable_flow(params, params.id_map.get(&body.id).copied().unwrap_or(body));
            let any_ref = flow.iter().any(|x| match x {
                StackData::Reference((_, tok, s)) => {
                    is_positional(s) && is_direct_child_of(tok, t) && !has_default_value(tok)
                }
                _ => false,
            });
            let any_assign = flow
                .iter()
                .any(|x| matches!(x, StackData::Assignment((_, _, s, _)) if is_positional(s)));
            if any_ref && !any_assign {
                function_list.push((name.clone(), params.id_map.get(&t.id).copied().unwrap_or(t)));
            }
        }
    });
    let mut function_map: BTreeMap<String, &Token> = BTreeMap::new();
    for (k, v) in function_list {
        function_map.insert(k, v);
    }
    let mut reference_list: Vec<(String, bool, Id)> = Vec::new();
    do_analysis(root, &mut |t| {
        if let T_SimpleCommand(_, words) = &t.inner
            && let Some((cmd, args)) = words.split_first()
            && let Some(s) = get_literal_string(cmd)
            && function_map.contains_key(&s)
        {
            reference_list.push((s, args.is_empty(), t.id));
        }
    });
    let mut groups: BTreeMap<String, Vec<(String, bool, Id)>> = BTreeMap::new();
    for r in reference_list {
        groups.entry(r.0.clone()).or_default().push(r);
    }
    let mut out = Vec::new();
    for group in groups.values() {
        let name = &group[0].0;
        let func = function_map[name];
        let ignoring = should_ignore_code(params, 2120, func);
        if group.iter().all(|(_, b, _)| *b) && !ignoring {
            for (n, _, id) in group {
                info(
                    &mut out,
                    *id,
                    2119,
                    &format!(
                        "Use {} \"$@\" if function's $1 should mean script's $1.",
                        e4m(n)
                    ),
                );
            }
            warn(
                &mut out,
                func.id,
                2120,
                &format!("{name} references arguments, but none are ever passed."),
            );
        }
    }
    out
}

pub fn check_unchecked_cd_pushd_popd(params: &Parameters<'_>, root: &Token) -> Out {
    if params.has_set_e {
        return Vec::new();
    }
    let mut out = Vec::new();
    do_analysis(root, &mut |t| {
        if !matches!(t.inner, T_SimpleCommand(..)) {
            return;
        }
        let name = get_command_name(t).unwrap_or_default();
        if !["cd", "pushd", "popd"].contains(&name.as_str()) {
            return;
        }
        let is_safe_dir = match oversimplify(t).as_slice() {
            [_, s] => SAFE_DIR_RE.with(|re| re.is_match(s)),
            _ => false,
        };
        if is_safe_dir {
            return;
        }
        if (name == "pushd" || name == "popd") && get_all_flags(t).iter().any(|(_, f)| f == "n") {
            return;
        }
        let path = params
            .id_map
            .get(&t.id)
            .map(|r| get_path(&params.parent_map, r))
            .unwrap_or_default();
        if is_condition(&path) {
            return;
        }
        warn_with_fix(
            &mut out,
            t.id,
            2164,
            &format!("Use '{name} ... || exit' or '{name} ... || return' in case {name} fails."),
            fix_with(vec![replace_end(t.id, params, 0, " || exit")]),
        );
    });
    out
}

pub fn check_array_assignment_indices(params: &Parameters<'_>, root: &Token) -> Out {
    let assocs = get_associative_arrays(root);
    let mut out = Vec::new();
    do_analysis(root, &mut |t| {
        let T_Assignment(_, name, indices, value) = &t.inner else {
            return;
        };
        if !indices.is_empty() {
            return;
        }
        let T_Array(list) = &value.inner else {
            return;
        };
        let is_assoc = assocs.contains(name);
        for el in list {
            match &el.inner {
                T_IndexedElement(_, v) => {
                    if let T_Literal(s) = &v.inner
                        && s.is_empty()
                    {
                        warn(
                            &mut out,
                            v.id,
                            2192,
                            "This array element has no value. Remove spaces after = or use \"\" for empty string.",
                        );
                    }
                }
                T_NormalWord(parts) => {
                    let literal_equals: Vec<Id> = parts
                        .iter()
                        .filter_map(|p| match &p.inner {
                            T_Literal(s) => {
                                let (before, after) = match s.find('=') {
                                    Some(i) => (&s[..i], &s[i..]),
                                    None => (s.as_str(), ""),
                                };
                                if before.chars().all(|c| c.is_ascii_digit()) && !after.is_empty() {
                                    Some(p.id)
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        })
                        .collect();
                    if literal_equals.is_empty() && is_assoc {
                        warn(
                            &mut out,
                            el.id,
                            2190,
                            "Elements in associative arrays need index, e.g. array=( [index]=value ) .",
                        );
                    } else {
                        for id in literal_equals {
                            warn_with_fix(
                                &mut out,
                                id,
                                2191,
                                "The = here is literal. To assign by index, use ( [index]=value ) with no spaces. To keep as literal, quote it.",
                                surround_with(id, params, "\""),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    });
    out
}

pub fn check_use_before_definition(params: &Parameters<'_>, t: &Token) -> Out {
    let Some(cfga) = params.cfg_analysis.as_ref() else {
        return Vec::new();
    };
    let mut funcs: BTreeMap<String, Vec<Id>> = BTreeMap::new();
    do_analysis(t, &mut |x| {
        if let T_Function(_, _, name, _) = &x.inner {
            funcs.entry(name.clone()).or_default().insert(0, x.id);
        }
    });
    if funcs.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    do_analysis(t, &mut |x| {
        if let T_SimpleCommand(_, words) = &x.inner
            && let Some(cmd) = words.first()
            && let Some(name) = get_literal_string(cmd)
            && let Some(invocations) = funcs.get(&name)
            && invocations
                .iter()
                .any(|c| cfganalysis::does_post_dominate(cfga, *c, x.id))
            && !invocations
                .iter()
                .any(|c| cfganalysis::does_post_dominate(cfga, x.id, *c))
        {
            super::analyzerlib::err(
                &mut out,
                x.id,
                2218,
                "This function is only defined later. Move the definition up.",
            );
        }
    });
    out
}

pub fn check_alias_used_in_same_parsing_unit(params: &Parameters<'_>, root: &Token) -> Out {
    let commands: Vec<&Token> = get_command_sequences(root)
        .into_iter()
        .flat_map(|s| s.iter())
        .collect();
    let line_span = |id: Id| -> Option<(i64, i64)> {
        let (start, end) = params.token_positions.get(&id)?;
        Some((start.line, end.line))
    };
    let follows_on_line = |a: &Token, b: &Token| -> bool {
        match (line_span(a.id), line_span(b.id)) {
            (Some((_, end)), Some((start, _))) => end == start,
            _ => false,
        }
    };
    // groupByLink
    let mut units: Vec<Vec<&Token>> = Vec::new();
    for c in commands {
        match units.last_mut() {
            Some(unit) if follows_on_line(unit.last().expect("non-empty"), c) => unit.push(c),
            _ => units.push(vec![c]),
        }
    }
    let is_sourced = |t: &Token| {
        params.id_map.get(&t.id).is_some_and(|r| {
            get_path(&params.parent_map, r)
                .iter()
                .any(|p| matches!(p.inner, T_SourceCommand(..)))
        })
    };
    let mut out = Vec::new();
    for unit in units {
        let mut aliases: BTreeMap<String, &Token> = BTreeMap::new();
        for u in unit {
            do_analysis(u, &mut |t| {
                let T_SimpleCommand(_, words) = &t.inner else {
                    return;
                };
                let Some((cmd, args)) = words.split_first() else {
                    return;
                };
                match get_unquoted_literal(cmd).as_deref() {
                    Some("alias") => {
                        for arg in args {
                            let s = get_literal_string_def("-", arg);
                            let (name, value) = match s.find('=') {
                                Some(i) => (&s[..i], &s[i..]),
                                None => (s.as_str(), ""),
                            };
                            if is_variable_name(name) && !value.is_empty() {
                                aliases.entry(name.to_string()).or_insert(arg);
                            }
                        }
                    }
                    Some(name) if !name.contains('/') => {
                        if let Some(alias) = aliases.get(name)
                            && !(is_sourced(t) || should_ignore_code(params, 2262, alias))
                        {
                            warn(
                                &mut out,
                                alias.id,
                                2262,
                                "This alias can't be defined and used in the same parsing unit. Use a function instead.",
                            );
                            info(
                                &mut out,
                                t.id,
                                2263,
                                "Since they're in the same parsing unit, this command will not refer to the previously mentioned alias.",
                            );
                        }
                    }
                    _ => {}
                }
            });
        }
    }
    out
}

pub fn check_array_value_used_as_index(params: &Parameters<'_>, _: &Token) -> Out {
    let parents = &params.parent_map;
    let get_array_name = |t: &Token| -> Option<String> {
        let parts = get_word_parts(t);
        let [
            Token {
                inner: T_DollarBraced(_, l),
                ..
            },
        ] = parts.as_slice()
        else {
            return None;
        };
        let s = concat_oversimplify(l);
        if get_braced_modifier(&s) == "[@]" && !s.starts_with('!') {
            Some(get_braced_reference(&s))
        } else {
            None
        }
    };
    let get_array_if_used_as_index = |name: &str, t: &Token| -> Option<(Id, String)> {
        match &t.inner {
            T_DollarBraced(_, list) => {
                if get_braced_reference(&concat_oversimplify(list)) != name {
                    return None;
                }
                let list_t = *parents.get(&t.id)?;
                if !matches!(list_t.inner, T_NormalWord(_)) {
                    return None;
                }
                let parent = *parents.get(&list_t.id)?;
                let T_DollarBraced(_, parent_list) = &parent.inner else {
                    return None;
                };
                let parts = get_word_parts(parent_list);
                let [
                    Token {
                        inner: T_Literal(_),
                        ..
                    },
                    index,
                    Token {
                        inner: T_Literal(_),
                        ..
                    },
                    ..,
                ] = parts.as_slice()
                else {
                    return None;
                };
                let s = concat_oversimplify(list_t);
                let modifier = get_braced_modifier(&s);
                if index.id != t.id || !modifier.starts_with("[${VAR}]") {
                    return None;
                }
                Some((t.id, get_braced_reference(&s)))
            }
            T_NormalWord(_) => {
                let parent = *parents.get(&t.id)?;
                if !matches!(parent.inner, T_DollarBraced(..)) {
                    return None;
                }
                let s = concat_oversimplify(t);
                let modifier = get_braced_modifier(&s);
                if !modifier.starts_with(&format!("[{name}]")) {
                    return None;
                }
                Some((parent.id, get_braced_reference(&s)))
            }
            TA_Variable(r, indices) if indices.is_empty() => {
                if r != name {
                    return None;
                }
                let seq = *parents.get(&t.id)?;
                let TA_Sequence(elems) = &seq.inner else {
                    return None;
                };
                let [element] = elems.as_slice() else {
                    return None;
                };
                if element.id != t.id {
                    return None;
                }
                let parent = *parents.get(&seq.id)?;
                let TA_Variable(array_name, pidx) = &parent.inner else {
                    return None;
                };
                let [pelement] = pidx.as_slice() else {
                    return None;
                };
                if pelement.id != seq.id {
                    return None;
                }
                Some((parent.id, array_name.clone()))
            }
            _ => None,
        }
    };
    type Loops = BTreeMap<String, (Id, Vec<(Id, String)>)>;
    let mut state: Loops = BTreeMap::new();
    do_variable_flow_analysis(
        &mut |m: &mut Loops, _base, t, name| {
            (|| {
                let (loop_id, arrays) = m.get(name)?;
                let (array_ref, array_name) = get_array_if_used_as_index(name, t)?;
                let (loop_word, _) = arrays.iter().find(|(_, n)| *n == array_name)?;
                let this = params.id_map.get(&t.id)?;
                if !get_path(parents, this).iter().any(|x| x.id == *loop_id) {
                    return None;
                }
                Some(vec![
                    make_comment(
                        super::interface::Severity::WarningC,
                        *loop_word,
                        2302,
                        "This loops over values. To loop over keys, use \"${!array[@]}\".",
                    ),
                    make_comment(
                        super::interface::Severity::WarningC,
                        array_ref,
                        2303,
                        &format!("{} is an array value, not a key. Use directly or loop over keys instead.", e4m(name)),
                    ),
                ])
            })()
            .unwrap_or_default()
        },
        &mut |m: &mut Loops, base, _t, name, data| {
            if let (T_ForIn(..), DataType::DataString(DataSource::SourceFrom(words))) =
                (&base.inner, data)
            {
                let arrays: Vec<(Id, String)> = words
                    .iter()
                    .filter_map(|x| get_array_name(x).map(|n| (x.id, n)))
                    .collect();
                m.insert(name.to_string(), (base.id, arrays));
            } else {
                m.remove(name);
            }
            Vec::new()
        },
        &mut state,
        &params.variable_flow,
    )
}

// ----- optional checks

pub fn check_require_double_bracket(params: &Parameters<'_>, t: &Token) -> Out {
    if !matches!(
        params.shell_type,
        Shell::Bash | Shell::Ksh | Shell::BusyboxSh
    ) {
        return Vec::new();
    }
    fn is_simple(t: &Token) -> bool {
        match &t.inner {
            T_Condition(_, s) => is_simple(s),
            TC_Binary(_, op, _, _) => !op.contains('<') && !op.contains('>'),
            TC_Unary(..) | TC_Nullary(..) => true,
            _ => false,
        }
    }
    let mut out = Vec::new();
    do_analysis(t, &mut |x| {
        if let T_Condition(ConditionType::SingleBracket, _) = &x.inner {
            let fix = if is_simple(x) {
                fix_with(vec![
                    replace_start(x.id, params, 0, "["),
                    replace_end(x.id, params, 0, "]"),
                ])
            } else {
                fix_with(Vec::new())
            };
            style_with_fix(
                &mut out,
                x.id,
                2292,
                "Prefer [[ ]] over [ ] for tests in Bash/Ksh/Busybox.",
                fix,
            );
        }
    });
    out
}

pub fn check_set_e_suppressed(params: &Parameters<'_>, t: &Token) -> Out {
    if !params.has_set_e {
        return Vec::new();
    }
    let functions_ = functions(t);
    let is_function_cmd =
        |cmd: &Token| get_unquoted_literal(cmd).is_some_and(|s| functions_.contains_key(&s));
    let mut out = Vec::new();
    do_analysis(t, &mut |x| {
        let T_SimpleCommand(_, words) = &x.inner else {
            return;
        };
        let Some(cmd) = words.first() else {
            return;
        };
        if !is_function_cmd(cmd) {
            return;
        }
        let Some(this) = params.id_map.get(&cmd.id) else {
            return;
        };
        let path = get_path(&params.parent_map, this);
        let inform_conditional = |cond_type: &str, out: &mut Out| {
            info(
                out,
                cmd.id,
                2310,
                &format!(
                    "This function is invoked in {cond_type} so set -e will be disabled. Invoke separately if failures should cause the script to exit."
                ),
            );
        };
        let errexit_enabled =
            |t: &Token| params.has_inherit_errexit || super::analyzerlib::contains_set_e(t);
        for w in path.windows(2) {
            let (child, parent) = (w[0], w[1]);
            let is_in = |cmds: &[&Token]| cmds.iter().any(|c| c.id == child.id);
            match &parent.inner {
                T_Banged(condition) if is_in(&[condition]) => {
                    inform_conditional("a ! condition", &mut out);
                }
                T_AndIf(condition, _) if is_in(&[condition]) => {
                    inform_conditional("an && condition", &mut out);
                }
                T_OrIf(condition, _) if is_in(&[condition]) => {
                    inform_conditional("an || condition", &mut out);
                }
                T_IfExpression(conditions, _)
                    if is_in(
                        &conditions
                            .iter()
                            .flat_map(|(c, _)| c.iter())
                            .collect::<Vec<_>>(),
                    ) =>
                {
                    inform_conditional("an 'if' condition", &mut out);
                }
                T_UntilExpression(condition, _) if is_in(&condition.iter().collect::<Vec<_>>()) => {
                    inform_conditional("an 'until' condition", &mut out);
                }
                T_WhileExpression(condition, _) if is_in(&condition.iter().collect::<Vec<_>>()) => {
                    inform_conditional("a 'while' condition", &mut out);
                }
                T_DollarExpansion(_) | T_Backticked(_) if !errexit_enabled(parent) => {
                    info(
                        &mut out,
                        cmd.id,
                        2311,
                        "Bash implicitly disabled set -e for this function invocation because it's inside a command substitution. Add set -e; before it or enable inherit_errexit.",
                    );
                }
                _ => {}
            }
        }
    });
    out
}

pub fn check_extra_masked_returns(params: &Parameters<'_>, t: &Token) -> Out {
    let is_transparent_command = |t: &Token| get_command_basename(t).as_deref() == Some("time");
    let mut transformed = t.clone();
    do_transform(&mut transformed, &mut |x| {
        if let T_SimpleCommand(_, words) = &x.inner
            && !words.is_empty()
            && is_transparent_command(x)
            && let T_SimpleCommand(_, words) = &mut x.inner
        {
            words.remove(0);
        }
    });
    fn contains_simple_command(t: &Token) -> bool {
        let mut found = false;
        do_analysis(t, &mut |x| {
            if matches!(x.inner, T_SimpleCommand(..)) {
                found = true;
            }
        });
        found
    }
    fn all_but_last_simple_commands(cmds: &[Token]) -> Vec<&Token> {
        let simple: Vec<&Token> = cmds.iter().filter(|c| contains_simple_command(c)).collect();
        if simple.is_empty() {
            simple
        } else {
            simple[..simple.len() - 1].to_vec()
        }
    }
    let ancestors = |t: &Token| get_parents_of_id(&params.parent_map, t.id);
    let is_mask_deliberate = |t: &Token| -> bool {
        // NE.init of the path: the token and its ancestors, without the root.
        let mut path: Vec<&Token> = vec![t];
        path.extend(ancestors(t));
        path.pop();
        path.iter().any(|x| match &x.inner {
            T_OrIf(_, rhs) => match &rhs.inner {
                T_Pipeline(_, cmds) => match cmds.as_slice() {
                    [
                        Token {
                            inner: T_Redirecting(_, cmd),
                            ..
                        },
                    ] => {
                        matches!(get_command_basename(cmd).as_deref(), Some("true" | ":"))
                    }
                    _ => false,
                },
                _ => false,
            },
            _ => false,
        })
    };
    let is_checked_elsewhere = |t: &Token| -> bool {
        ancestors(t).iter().any(|x| {
            let Some(cmd) = get_command(x) else {
                return false;
            };
            let Some(b) = get_command_basename(cmd) else {
                return false;
            };
            if b == "local" && get_all_flags(cmd).iter().any(|(_, f)| f == "r") {
                false
            } else {
                DECLARING_COMMANDS.contains(&b.as_str())
            }
        })
    };
    let is_harmless_command = |t: &Token| {
        get_command_basename(t).is_some_and(|b| {
            ["echo", "basename", "dirname", "printf", "set", "shopt"].contains(&b.as_str())
        })
    };
    let is_masked_node =
        |t: &Token| !(is_harmless_command(t) || is_checked_elsewhere(t) || is_mask_deliberate(t));
    let mut out = Vec::new();
    let find_masked_in_list = |list: &[&Token], out: &mut Out| {
        for l in list {
            do_analysis(l, &mut |x| {
                let candidate = match &x.inner {
                    T_SimpleCommand(_, words) => !words.is_empty(),
                    T_Condition(..) => true,
                    _ => false,
                };
                if candidate && is_masked_node(x) {
                    info(
                        out,
                        x.id,
                        2312,
                        "Consider invoking this command separately to avoid masking its return value (or use '|| true' to ignore).",
                    );
                }
            });
        }
    };
    do_analysis(&transformed, &mut |x| match &x.inner {
        T_Arithmetic(list) => find_masked_in_list(&[list], &mut out),
        T_Array(list) | T_DoubleQuoted(list) | T_NormalWord(list) => {
            find_masked_in_list(&all_but_last_simple_commands(list), &mut out);
        }
        T_Condition(_, condition) => find_masked_in_list(&[condition], &mut out),
        T_HereDoc(_, _, _, list) | T_ProcSub(_, list) => {
            find_masked_in_list(&list.iter().collect::<Vec<_>>(), &mut out);
        }
        T_HereString(word) => find_masked_in_list(&[word], &mut out),
        T_Pipeline(_, cmds) if !params.has_pipefail => {
            find_masked_in_list(&all_but_last_simple_commands(cmds), &mut out);
        }
        T_SimpleCommand(assigns, words) if !words.is_empty() => {
            let l: Vec<&Token> = assigns.iter().chain(words[1..].iter()).collect();
            find_masked_in_list(&l, &mut out);
        }
        T_SimpleCommand(assigns, _) => {
            find_masked_in_list(&all_but_last_simple_commands(assigns), &mut out);
        }
        _ => {}
    });
    out
}

// ----- the lists

/// `nodeChecks`.
pub const NODE_CHECKS: &[NodeCheck] = &[
    check_pipe_pitfalls,
    check_for_in_quoted,
    check_for_in_ls,
    check_shorthand_if,
    check_dollar_star,
    check_unquoted_dollar_at,
    check_stderr_redirect,
    check_unquoted_n,
    check_number_comparisons,
    check_single_bracket_operators,
    check_double_bracket_operators,
    check_literal_breaking_test,
    check_constant_nullary,
    check_div_before_mult,
    check_arithmetic_deref,
    check_arithmetic_bad_octal,
    check_comparison_against_glob,
    check_case_against_glob,
    check_commarrays,
    check_or_neq,
    check_and_eq,
    check_echo_wc,
    check_constant_ifs,
    check_piped_assignment,
    check_assign_ate_command,
    check_uuoe_var,
    check_quoted_cond_regex,
    check_for_in_cat,
    check_find_exec,
    check_valid_cond_ops,
    check_globbed_regex,
    check_test_redirects,
    check_bad_parameter_substitution,
    check_ps1_assignments,
    check_backticks,
    check_inexplicably_unquoted,
    check_tilde_in_quotes,
    check_lonely_dot_dash,
    check_spurious_exec,
    check_spurious_expansion,
    check_dollar_brackets,
    check_ssh_here_doc,
    check_globs_as_options,
    check_while_read_pitfalls,
    check_arithmetic_op_command,
    check_char_range_glob,
    check_unquoted_expansions,
    check_single_quoted_variables,
    check_redirect_to_same,
    check_prefix_assignment_reference,
    check_loop_keyword_scope,
    check_cd_and_back,
    check_wrong_arithmetic_assignment,
    check_conditional_and_ors,
    check_function_declarations,
    check_stderr_pipe,
    check_overriding_path,
    check_array_as_string,
    check_unsupported,
    check_multiple_appends,
    check_suspicious_ifs,
    check_should_use_grep_q,
    check_test_argument_splitting,
    check_concatenated_dollar_at,
    check_tilde_in_path,
    check_read_without_r,
    check_loop_variable_reassignment,
    check_trailing_bracket,
    check_return_against_zero,
    check_redirected_nowhere,
    check_unmatchable_cases,
    check_subshell_as_test,
    check_splitting_in_arrays,
    check_redirection_to_number,
    check_glob_as_command,
    check_flag_as_command,
    check_empty_condition,
    check_pipe_to_nowhere,
    check_for_loop_glob_variables,
    check_subshelled_tests,
    check_redirection_to_command,
    check_dollar_quote_paren,
    check_useless_bang,
    check_translated_string_variable,
    check_modified_arithmetic_in_redirection,
    check_blatant_recursion,
    check_bad_test_and_or,
    check_assign_to_self,
    check_equals_in_command,
    check_second_arg_is_comparison,
    check_comparison_with_leading_x,
    check_command_with_trailing_symbol,
    check_unquoted_parameter_expansion_pattern,
    check_bats_test_does_not_use_negation,
    check_command_is_unreachable,
    check_spacefulness_cfg,
    check_overwritten_exit_code,
    check_unnecessary_arithmetic_expansion_index,
    check_unnecessary_parens,
    check_plus_equals_number,
    check_expansion_with_redirection,
    check_unary_test_a,
];

fn node_checks(params: &Parameters<'_>, t: &Token) -> Out {
    node_checks_to_tree_check(NODE_CHECKS, params, t)
}

/// `treeChecks`.
pub const TREE_CHECKS: &[TreeCheck] = &[
    node_checks,
    subshell_assignment_check,
    check_quotes_in_literals,
    check_shebang_parameters,
    check_functions_used_externally,
    check_unused_assignments,
    check_unpassed_in_functions,
    check_array_without_index,
    check_shebang,
    check_unassigned_references,
    check_unchecked_cd_pushd_popd,
    check_array_assignment_indices,
    check_use_before_definition,
    check_alias_used_in_same_parsing_unit,
    check_array_value_used_as_index,
];

fn opt_verbose_spacefulness(p: &Parameters<'_>, t: &Token) -> Out {
    node_checks_to_tree_check(&[check_verbose_spacefulness_cfg], p, t)
}

fn opt_nullary_expansion_test(p: &Parameters<'_>, t: &Token) -> Out {
    node_checks_to_tree_check(&[check_nullary_expansion_test], p, t)
}

fn opt_unnecessarily_inverted_test(p: &Parameters<'_>, t: &Token) -> Out {
    node_checks_to_tree_check(&[check_unnecessarily_inverted_test], p, t)
}

fn opt_default_case(p: &Parameters<'_>, t: &Token) -> Out {
    node_checks_to_tree_check(&[check_default_case], p, t)
}

fn opt_variable_braces(p: &Parameters<'_>, t: &Token) -> Out {
    node_checks_to_tree_check(&[check_variable_braces], p, t)
}

fn opt_uuoc(p: &Parameters<'_>, t: &Token) -> Out {
    node_checks_to_tree_check(&[check_uuoc], p, t)
}

/// `optionalTreeChecks`: (name, check), in `ShellCheck`'s order.
pub const OPTIONAL_TREE_CHECKS: &[(&str, TreeCheck)] = &[
    ("quote-safe-variables", opt_verbose_spacefulness),
    ("avoid-nullary-conditions", opt_nullary_expansion_test),
    ("avoid-negated-conditions", opt_unnecessarily_inverted_test),
    ("add-default-case", opt_default_case),
    ("require-variable-braces", opt_variable_braces),
    (
        "check-unassigned-uppercase",
        check_unassigned_references_globals,
    ),
    ("require-double-brackets", check_require_double_bracket),
    ("check-set-e-suppressed", check_set_e_suppressed),
    ("check-extra-masked-returns", check_extra_masked_returns),
    ("useless-use-of-cat", opt_uuoc),
];

/// Analytics' `checker`: the tree checks plus the enabled optional checks.
pub fn run(params: &Parameters<'_>, root: &Token, optional_keys: &[String]) -> Out {
    let mut out = Vec::new();
    for f in TREE_CHECKS {
        out.extend(f(params, root));
    }
    if optional_keys.iter().any(|k| k == "all") {
        for (_, f) in OPTIONAL_TREE_CHECKS {
            out.extend(f(params, root));
        }
    } else {
        for k in optional_keys {
            if let Some((_, f)) = OPTIONAL_TREE_CHECKS.iter().find(|(n, _)| n == k) {
                out.extend(f(params, root));
            }
        }
    }
    out
}

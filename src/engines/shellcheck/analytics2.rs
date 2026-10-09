//! `ShellCheck`'s general checks (`ShellCheck.Analytics`), part two: the
//! remaining per-node checks.

use std::collections::BTreeMap;

use super::analytics::{
    fix_with, is_condition, replace_end, replace_start, replace_token, surround_with,
};
use super::analyzerlib::{
    Out, Parameters, Scope, StackData, err, err_with_fix, get_closest_command, get_parents_of_id,
    get_path, info, info_with_fix, is_command, is_command_match, is_counting_reference,
    is_dereferencing_binary_op, is_quoted_alternative_reference, is_test_command,
    is_unqualified_command, lead_type, style, style_with_fix, warn, warn_with_fix,
};
use super::ast::{AssignmentMode, CaseType, ConditionType, Id, Inner, Token, do_analysis};
use super::astlib::{
    PseudoGlob, arguments, concat_oversimplify, get_all_flags, get_braced_modifier,
    get_braced_reference, get_command, get_command_argv, get_command_basename, get_command_name,
    get_command_name_and_token, get_command_sequences, get_command_token_or_this, get_gnu_opts,
    get_leading_unquoted_string, get_literal_string, get_literal_string_def,
    get_literal_string_ext, get_unmodified_parameter_expansion, get_unquoted_literal,
    get_word_parts, has_flag, is_array_expansion, is_brace_expansion, is_command_substitution,
    is_constant, is_function, is_glob, is_loop, is_quoteable_expansion, is_quotes,
    is_unquoted_flag, is_variable_char, is_variable_name, oversimplify,
    pseudo_glob_is_super_set_of, pseudo_globs_can_overlap, will_become_multiple_args,
    word_to_exact_pseudo_glob, word_to_pseudo_glob,
};
use super::cfganalysis;
use super::data::{
    ARITHMETIC_BINARY_TEST_OPS, BINARY_TEST_OPS, COMMON_COMMANDS, FLAGS_FOR_READ,
    NON_READING_COMMANDS, UNARY_TEST_OPS, VARIABLES_WITHOUT_SPACES,
};
use super::hchar;
use super::interface::{Fix, Shell};
use super::regex::{Regex, mk_regex};

use Inner::{
    T_AndIf, T_Annotation, T_Arithmetic, T_Array, T_Assignment, T_Backgrounded, T_Backticked,
    T_Banged, T_BatsTest, T_BraceGroup, T_CLOBBER, T_CaseExpression, T_Condition, T_DGREAT,
    T_DollarArithmetic, T_DollarBraceCommandExpansion, T_DollarBraced, T_DollarDoubleQuoted,
    T_DollarExpansion, T_DoubleQuoted, T_FdRedirect, T_ForArithmetic, T_ForIn, T_Function,
    T_GREATAND, T_Glob, T_Greater, T_HereDoc, T_HereString, T_IfExpression, T_IoDuplicate,
    T_IoFile, T_LESSAND, T_Less, T_Literal, T_NormalWord, T_OrIf, T_Pipe, T_Pipeline, T_ProcSub,
    T_Redirecting, T_Script, T_SimpleCommand, T_SingleQuoted, T_SourceCommand, T_Subshell,
    T_UntilExpression, T_WhileExpression, TA_Assignment, TA_Binary, TA_Expansion, TA_Parenthesis,
    TA_Sequence, TA_Trinary, TA_Unary, TA_Variable, TC_Binary, TC_Empty, TC_Group, TC_Nullary,
    TC_Unary,
};

thread_local! {
    static SAFE_DIR_RE: Regex = mk_regex("^/*((\\.|\\.\\.)/+)*(\\.|\\.\\.)?$");
    static POSITIONAL_ASSIGNMENT_RE: Regex = mk_regex("^[0-9][0-9]?=");
}

fn path_of<'a>(params: &Parameters<'a>, t: &Token) -> Vec<&'a Token> {
    match params.id_map.get(&t.id) {
        Some(r) => get_path(&params.parent_map, r),
        None => Vec::new(),
    }
}

pub fn check_globs_as_options(_: &Parameters<'_>, cmd: &Token, out: &mut Out) {
    let T_SimpleCommand(_, args) = &cmd.inner else {
        return;
    };
    if get_command_basename(cmd).is_some_and(|b| b == "echo" || b == "printf") {
        return;
    }
    let is_end_of_args =
        |t: &Token| matches!(concat_oversimplify(t).as_str(), "--" | ":::" | "::::");
    for v in args.iter().skip(1).take_while(|t| !is_end_of_args(t)) {
        if let T_NormalWord(parts) = &v.inner
            && let Some(Token {
                id,
                inner: T_Glob(s),
            }) = parts.first()
            && (s == "*" || s == "?")
        {
            info(
                out,
                *id,
                2035,
                "Use ./*glob* or -- *glob* so names with dashes won't become options.",
            );
        }
    }
}

fn stdin_redirect(t: &Token) -> bool {
    match &t.inner {
        T_FdRedirect(fd, op) => {
            if fd == "0" {
                true
            } else if fd.is_empty() {
                match &op.inner {
                    T_IoFile(o, _) => matches!(o.inner, T_Less),
                    T_IoDuplicate(o, _) => matches!(o.inner, T_LESSAND),
                    T_HereString(_) | T_HereDoc(..) => true,
                    _ => false,
                }
            } else {
                false
            }
        }
        _ => false,
    }
}

pub fn check_while_read_pitfalls(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_WhileExpression(conds, contents) = &t.inner else {
        return;
    };
    let [command] = conds.as_slice() else {
        return;
    };
    let is_stdin_read_command = match &command.inner {
        T_Pipeline(_, cmds) => match cmds.as_slice() {
            [
                Token {
                    inner: T_Redirecting(redirs, cmd),
                    ..
                },
            ] => {
                let plaintext = oversimplify(cmd);
                plaintext.first().map_or("", String::as_str) == "read"
                    && !plaintext.iter().any(|s| s == "-u")
                    && !redirs.iter().any(stdin_redirect)
            }
            _ => false,
        },
        _ => false,
    };
    if !is_stdin_read_command {
        return;
    }
    for c in contents {
        check_muncher(params, t.id, c, out);
    }
}

fn check_muncher(params: &Parameters<'_>, while_id: Id, t: &Token, out: &mut Out) {
    match &t.inner {
        T_Backgrounded(inner) => check_muncher(params, while_id, inner, out),
        T_Pipeline(_, cmds) => {
            let Some(Token {
                inner: T_Redirecting(redirs, cmd),
                ..
            }) = cmds.first()
            else {
                return;
            };
            if let T_SimpleCommand(vars, args) = &cmd.inner {
                for w in vars.iter().chain(args.iter()) {
                    let words = match &w.inner {
                        T_Assignment(_, _, _, x) => get_word_parts(x),
                        _ => get_word_parts(w),
                    };
                    for part in words {
                        for seq in get_command_sequences(part) {
                            for c in seq {
                                check_muncher(params, while_id, c, out);
                            }
                        }
                    }
                }
            }
            if redirs.iter().any(stdin_redirect) {
                return;
            }
            for seq in get_command_sequences(cmd) {
                for c in seq {
                    check_muncher(params, while_id, c, out);
                }
            }
            let Some(name) = get_command_basename(cmd) else {
                return;
            };
            let has_flag_check = |flag: &str| -> bool {
                let f = flag.strip_prefix('-').unwrap_or(flag);
                get_all_flags(cmd).iter().any(|(_, x)| x == f)
            };
            let has_argument = |arg: &str| -> bool {
                get_command_argv(cmd)
                    .expect("a command")
                    .iter()
                    .filter_map(get_literal_string)
                    .any(|s| s == arg)
            };
            let add_flag = |s: &str| -> Fix {
                fix_with(vec![replace_end(
                    get_command_token_or_this(cmd).id,
                    params,
                    0,
                    &format!(" {s}"),
                )])
            };
            let add_redirect = |s: &str| -> Fix {
                fix_with(vec![replace_end(cmd.id, params, 0, &format!(" {s}"))])
            };
            let (excluded, fix, flag): (bool, Fix, &str) = match name.as_str() {
                "ssh" => (has_flag_check("-n"), add_flag("-n"), "-n"),
                "ffmpeg" => (has_argument("-nostdin"), add_flag("-nostdin"), "-nostdin"),
                "mplayer" => (
                    has_argument("-noconsolecontrols"),
                    add_flag("-noconsolecontrols"),
                    "-noconsolecontrols",
                ),
                "HandBrakeCLI" => (false, add_redirect("< /dev/null"), "< /dev/null"),
                _ => return,
            };
            if excluded {
                return;
            }
            info(
                out,
                while_id,
                2095,
                &format!("{name} may swallow stdin, preventing this loop from working properly."),
            );
            warn_with_fix(
                out,
                cmd.id,
                2095,
                &format!("Use {name} {flag} to prevent {name} from swallowing stdin."),
                fix,
            );
        }
        _ => {}
    }
}

pub fn check_prefix_assignment_reference(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_DollarBraced(_, value) = &t.inner else {
        return;
    };
    let name = get_braced_reference(&concat_oversimplify(value));
    let path = path_of(params, t);
    let id_path: Vec<Id> = path.iter().map(|x| x.id).collect();
    for p in &path {
        if let T_SimpleCommand(vars, words) = &p.inner
            && !words.is_empty()
        {
            for v in vars {
                if let T_Assignment(_, a_name, indices, _) = &v.inner
                    && indices.is_empty()
                    && *a_name == name
                    && !id_path.contains(&v.id)
                {
                    warn(
                        out,
                        v.id,
                        2097,
                        "This assignment is only seen by the forked process.",
                    );
                    warn(
                        out,
                        t.id,
                        2098,
                        "This expansion will not see the mentioned assignment.",
                    );
                }
            }
            return;
        }
    }
}

pub fn check_char_range_glob(p: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_Glob(s) = &t.inner else {
        return;
    };
    let is_char_class = s.starts_with('[') && s.ends_with(']');
    if !is_char_class {
        return;
    }
    let chars: Vec<char> = s.chars().collect();
    let inner: String = if chars.len() >= 2 {
        chars[1..chars.len() - 1].iter().collect()
    } else {
        String::new()
    };
    let contents: String = inner
        .strip_prefix('!')
        .or_else(|| inner.strip_prefix('^'))
        .unwrap_or(&inner)
        .to_string();
    let path = path_of(p, t);
    let is_ignored_command = path
        .first()
        .and_then(|this| get_closest_command(&p.parent_map, this))
        .is_some_and(|cmd| is_command_match(cmd, &|c| c == "tr" || c == "read"));
    let is_dereferenced = path
        .iter()
        .find_map(|x| match &x.inner {
            TC_Binary(ConditionType::DoubleBracket, op, _, _) => {
                Some(is_dereferencing_binary_op(op))
            }
            TC_Unary(_, op, _) => Some(op == "-v"),
            T_SimpleCommand(..) => Some(false),
            _ => None,
        })
        .unwrap_or(false);
    if is_ignored_command || is_dereferenced {
        return;
    }
    if contents.starts_with(':') && contents.ends_with(':') && contents != ":" {
        warn(
            out,
            t.id,
            2101,
            "Named class needs outer [], e.g. [[:digit:]].",
        );
    } else {
        let mut sorted: Vec<char> = contents.chars().filter(|&c| c != '-').collect();
        sorted.sort_unstable();
        let has_dupes = sorted.windows(2).any(|w| w[0] == w[1]);
        if !contents.contains('[') && has_dupes {
            info(
                out,
                t.id,
                2102,
                "Ranges can only match single chars (mentioned due to duplicates).",
            );
        }
    }
}

pub fn check_cd_and_back(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if params.has_set_e {
        return;
    }
    fn get_candidate(t: &Token) -> Option<&Token> {
        match &t.inner {
            T_Annotation(_, x) => get_candidate(x),
            T_Pipeline(_, cmds) if cmds.len() == 1 && is_command(&cmds[0], "cd") => Some(&cmds[0]),
            _ => None,
        }
    }
    let is_cd_revert = |t: &Token| match oversimplify(t).as_slice() {
        [_, p] => p == ".." || p == "-",
        _ => false,
    };
    for list in get_command_sequences(t) {
        let candidates: Vec<&Token> = list.iter().filter_map(get_candidate).collect();
        for w in candidates.windows(2) {
            if is_cd_revert(w[1]) && !is_cd_revert(w[0]) {
                info(
                    out,
                    w[1].id,
                    2103,
                    "Use a ( subshell ) to avoid having to cd back.",
                );
                break;
            }
        }
    }
}

pub fn check_loop_keyword_scope(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let Some(name) = get_command_name(t) else {
        return;
    };
    if name != "continue" && name != "break" {
        return;
    }
    let subshell_type = |x: &Token| match lead_type(params, x) {
        Scope::NoneScope => None,
        Scope::SubshellScope(s) => Some(s),
    };
    let relevant = |x: &Token| is_loop(x) || is_function(x) || subshell_type(x).is_some();
    let full = path_of(params, t);
    let path: Vec<&Token> = full.into_iter().filter(|x| relevant(x)).collect();
    if path.iter().any(|x| is_loop(x)) {
        let types: Vec<Option<&'static str>> = path
            .iter()
            .filter(|x| !is_function(x))
            .map(|x| subshell_type(x))
            .collect();
        if let Some(Some(s)) = types.first() {
            warn(
                out,
                t.id,
                2106,
                &format!("This only exits the subshell caused by the {s}."),
            );
        }
    } else {
        match path.first() {
            Some(h) if is_function(h) => {
                err(
                    out,
                    t.id,
                    2104,
                    &format!("In functions, use return instead of {name}."),
                );
            }
            _ => err(out, t.id, 2105, &format!("{name} is only valid in loops.")),
        }
    }
}

pub fn check_function_declarations(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_Function(has_keyword, has_parens, _, _) = &t.inner else {
        return;
    };
    let (k, p) = (*has_keyword, *has_parens);
    match params.shell_type {
        Shell::Bash => {}
        Shell::Ksh => {
            if k && p {
                err(
                    out,
                    t.id,
                    2111,
                    "ksh does not allow 'function' keyword and '()' at the same time.",
                );
            }
        }
        Shell::Dash | Shell::BusyboxSh | Shell::Sh => {
            if k && p {
                warn(
                    out,
                    t.id,
                    2112,
                    "'function' keyword is non-standard. Delete it.",
                );
            }
            if k && !p {
                warn(
                    out,
                    t.id,
                    2113,
                    "'function' keyword is non-standard. Use 'foo()' instead of 'function foo'.",
                );
            }
        }
    }
}

pub fn check_stderr_pipe(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if params.shell_type == Shell::Ksh
        && let T_Pipe(s) = &t.inner
        && s == "|&"
    {
        err(out, t.id, 2118, "Ksh does not support |&. Use 2>&1 |.");
    }
}

pub fn check_overriding_path(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_SimpleCommand(vars, words) = &t.inner else {
        return;
    };
    if !words.is_empty() {
        return;
    }
    for v in vars {
        if let T_Assignment(AssignmentMode::Assign, name, indices, word) = &v.inner
            && name == "PATH"
            && indices.is_empty()
        {
            let string = concat_oversimplify(word);
            if string.contains("/bin") || string.contains("/sbin") {
                continue;
            }
            if string.contains('/') && !string.contains(':') {
                warn(
                    out,
                    v.id,
                    2123,
                    "PATH is the shell search path. Use another name.",
                );
            }
            if super::astlib::is_literal(word) && !string.contains(':') && !string.contains('/') {
                warn(
                    out,
                    v.id,
                    2123,
                    "PATH is the shell search path. Use another name.",
                );
            }
        }
    }
}

pub fn check_tilde_in_path(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_SimpleCommand(vars, _) = &t.inner else {
        return;
    };
    for v in vars {
        if let T_Assignment(AssignmentMode::Assign, name, indices, word) = &v.inner
            && name == "PATH"
            && indices.is_empty()
            && let T_NormalWord(parts) = &word.inner
            && parts
                .iter()
                .any(|x| is_quotes(x) && super::astlib::only_literal_string(x).contains('~'))
        {
            warn(
                out,
                v.id,
                2147,
                "Literal tilde in PATH works poorly across programs.",
            );
        }
    }
}

const fn shell_name(s: Shell) -> &'static str {
    match s {
        Shell::Ksh => "ksh",
        Shell::Sh => "sh",
        Shell::Bash => "bash",
        Shell::Dash => "dash",
        Shell::BusyboxSh => "busyboxsh",
    }
}

pub fn check_unsupported(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let (name, support): (&str, Vec<Shell>) = match &t.inner {
        T_CaseExpression(_, list) => {
            let seps: Vec<CaseType> = list.iter().map(|(a, _, _)| *a).collect();
            if seps.contains(&CaseType::CaseContinue) {
                ("cases with ;;&", vec![Shell::Bash])
            } else if seps.contains(&CaseType::CaseFallThrough) {
                ("cases with ;&", vec![Shell::Bash, Shell::Ksh])
            } else {
                ("", Vec::new())
            }
        }
        T_DollarBraceCommandExpansion(..) => {
            ("${ ..; } command expansion", vec![Shell::Bash, Shell::Ksh])
        }
        _ => ("", Vec::new()),
    };
    if !(support.is_empty() || support.contains(&params.shell_type)) {
        let shells: Vec<&str> = support.iter().map(|s| shell_name(*s)).collect();
        err(
            out,
            t.id,
            2127,
            &format!(
                "To use {name}, specify #!/usr/bin/env {}",
                shells.join(" or ")
            ),
        );
    }
}

pub fn check_multiple_appends(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    fn get_target(t: &Token) -> Option<(&Token, Id)> {
        match &t.inner {
            T_Annotation(_, t) => get_target(t),
            T_Pipeline(_, args) if !args.is_empty() => get_target(args.last().expect("non-empty")),
            T_Redirecting(list, _) => {
                let file = list.iter().find_map(|r| match &r.inner {
                    T_FdRedirect(_, x) => match &x.inner {
                        T_IoFile(op, f) if matches!(op.inner, T_DGREAT) => Some(&**f),
                        _ => None,
                    },
                    _ => None,
                })?;
                Some((file, t.id))
            }
            _ => None,
        }
    }
    for list in get_command_sequences(t) {
        let targets: Vec<Option<(&Token, Id)>> = list.iter().map(get_target).collect();
        let mut i = 0;
        while i < targets.len() {
            let mut j = i + 1;
            while j < targets.len() && targets[j].map(|x| x.0) == targets[i].map(|x| x.0) {
                j += 1;
            }
            if j - i >= 3
                && let Some((_, id)) = targets[i]
            {
                style(
                    out,
                    id,
                    2129,
                    "Consider using { cmd1; cmd2; } >> file instead of individual redirects.",
                );
            }
            i = j;
        }
    }
}

pub fn check_suspicious_ifs(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_Assignment(_, name, indices, value) = &t.inner else {
        return;
    };
    if name != "IFS" || !indices.is_empty() {
        return;
    }
    let Some(v) = get_literal_string(value) else {
        return;
    };
    let has_dollar_single = matches!(params.shell_type, Shell::Bash | Shell::Ksh);
    let n = if has_dollar_single {
        "$'\\n'"
    } else {
        "'<literal linefeed here>'"
    };
    let tab = if has_dollar_single {
        "$'\\t'"
    } else {
        "\"$(printf '\\t')\""
    };
    let suggest = |r: &str, out: &mut Out| {
        warn(
            out,
            value.id,
            2141,
            &format!("This backslash is literal. Did you mean IFS={r} ?"),
        );
    };
    let suggest2 = |desc: &str, out: &mut Out| {
        warn(
            out,
            value.id,
            2141,
            &format!(
                "This IFS value contains {desc}. For tabs/linefeeds/escapes, use $'..', literal, or printf."
            ),
        );
    };
    match v.as_str() {
        "\\n" => suggest(n, out),
        "\\t" => suggest(tab, out),
        x if x.contains('\\') => suggest2("a literal backslash", out),
        x if x.contains('n') => suggest2("the literal letter 'n'", out),
        x if x.contains('t') => suggest2("the literal letter 't'", out),
        _ => {}
    }
}

pub fn check_should_use_grep_q(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let (bool_, token) = match &t.inner {
        TC_Nullary(_, token) => (true, token),
        TC_Unary(_, op, token) if op == "-n" => (true, token),
        TC_Unary(_, op, token) if op == "-z" => (false, token),
        _ => return,
    };
    fn get_pipeline(t: &Token) -> Option<&[Token]> {
        match &t.inner {
            T_NormalWord(l) | T_DoubleQuoted(l) | T_DollarExpansion(l) if l.len() == 1 => {
                get_pipeline(&l[0])
            }
            T_Pipeline(_, cmds) => Some(cmds),
            _ => None,
        }
    }
    let Some(cmds) = get_pipeline(token) else {
        return;
    };
    let Some(last) = cmds.last() else {
        return;
    };
    let Some(name) = get_command_basename(last) else {
        return;
    };
    if ![
        "grep", "egrep", "fgrep", "bz3grep", "bzgrep", "xzgrep", "zgrep", "zipgrep", "zstdgrep",
    ]
    .contains(&name.as_str())
    {
        return;
    }
    let op = if bool_ { "-n" } else { "-z" };
    let flip = if bool_ { "" } else { "! " };
    style(
        out,
        t.id,
        2143,
        &format!("Use {flip}{name} -q instead of comparing output with [ {op} .. ]."),
    );
}

pub fn check_test_argument_splitting(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let check_arrays = |typ: ConditionType, token: &Token, out: &mut Out| {
        if get_word_parts(token).iter().any(|x| is_array_expansion(x)) {
            if typ == ConditionType::SingleBracket {
                warn(
                    out,
                    token.id,
                    2198,
                    "Arrays don't work as operands in [ ]. Use a loop (or concatenate with * instead of @).",
                );
            } else {
                err(
                    out,
                    token.id,
                    2199,
                    "Arrays implicitly concatenate in [[ ]]. Use a loop (or explicit * instead of @).",
                );
            }
        }
    };
    let check_braces = |typ: ConditionType, token: &Token, out: &mut Out| {
        if get_word_parts(token).iter().any(|x| is_brace_expansion(x)) {
            if typ == ConditionType::SingleBracket {
                warn(
                    out,
                    token.id,
                    2200,
                    "Brace expansions don't work as operands in [ ]. Use a loop.",
                );
            } else {
                err(
                    out,
                    token.id,
                    2201,
                    "Brace expansion doesn't happen in [[ ]]. Use a loop.",
                );
            }
        }
    };
    let check_globs = |typ: ConditionType, token: &Token, out: &mut Out| {
        if is_glob(token) {
            if typ == ConditionType::SingleBracket {
                warn(
                    out,
                    token.id,
                    2202,
                    "Globs don't work as operands in [ ]. Use a loop.",
                );
            } else {
                err(
                    out,
                    token.id,
                    2203,
                    "Globs are ignored in [[ ]] except right of =/!=. Use a loop.",
                );
            }
        }
    };
    let check_all = |typ: ConditionType, token: &Token, out: &mut Out| {
        check_arrays(typ, token, out);
        check_braces(typ, token, out);
        check_globs(typ, token, out);
    };
    match &t.inner {
        TC_Unary(typ, op, token) if is_glob(token) => {
            if op == "-v" {
                if *typ == ConditionType::SingleBracket {
                    err(
                        out,
                        token.id,
                        2208,
                        "Use [[ ]] or quote arguments to -v to avoid glob expansion.",
                    );
                }
            } else if *typ == ConditionType::SingleBracket && params.shell_type == Shell::Ksh {
                if "bcdfgkprsuwxLhNOGRS"
                    .chars()
                    .any(|c| format!("-{c}") == *op)
                {
                    warn(
                        out,
                        token.id,
                        2245,
                        &format!(
                            "{op} only applies to the first expansion of this glob. Use a loop to check any/all."
                        ),
                    );
                }
            } else {
                err(
                    out,
                    token.id,
                    2144,
                    &format!("{op} doesn't work with globs. Use a for loop."),
                );
            }
        }
        TC_Nullary(typ, token) => {
            check_braces(*typ, token, out);
            check_globs(*typ, token, out);
            if *typ == ConditionType::DoubleBracket {
                check_arrays(*typ, token, out);
            }
        }
        TC_Unary(typ, _, token) => check_all(*typ, token, out),
        TC_Binary(typ, op, lhs, rhs) if ARITHMETIC_BINARY_TEST_OPS.contains(&op.as_str()) => {
            for c in [lhs, rhs] {
                if *typ != ConditionType::DoubleBracket
                    && params.shell_type != Shell::Ksh
                    && is_glob(c)
                {
                    err(
                        out,
                        c.id,
                        2255,
                        "[ ] does not apply arithmetic evaluation. Evaluate with $((..)) for numbers, or use string comparator for strings.",
                    );
                }
                check_arrays(*typ, c, out);
                check_braces(*typ, c, out);
            }
        }
        TC_Binary(typ, op, lhs, rhs) => {
            check_all(*typ, lhs, out);
            if ["=", "==", "!=", "=~"].contains(&op.as_str()) {
                check_arrays(*typ, rhs, out);
                check_braces(*typ, rhs, out);
            } else {
                check_all(*typ, rhs, out);
            }
        }
        _ => {}
    }
}

pub fn check_read_without_r(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if !matches!(t.inner, T_SimpleCommand(..)) || !is_unqualified_command(t, "read") {
        return;
    }
    let flags = get_all_flags(t);
    let has_r = flags.iter().any(|(_, f)| f == "r");
    let has_t0 = get_gnu_opts(FLAGS_FOR_READ, arguments(t))
        .and_then(|parsed| {
            parsed
                .iter()
                .find(|(f, _)| f == "t")
                .map(|(_, (_, x))| get_literal_string(x))
        })
        .flatten()
        .as_deref()
        == Some("0");
    if !has_r && !has_t0 {
        info(
            out,
            get_command_token_or_this(t).id,
            2162,
            "read without -r will mangle backslashes.",
        );
    }
}

pub fn check_loop_variable_reassignment(params: &Parameters<'_>, token: &Token, out: &mut Out) {
    if !matches!(token.inner, T_ForIn(..) | T_ForArithmetic(..)) {
        return;
    }
    fn loop_variable(t: &Token) -> Option<String> {
        match &t.inner {
            T_ForIn(s, _, _) => Some(s.clone()),
            T_ForArithmetic(init, _, _, _) => match &init.inner {
                TA_Sequence(list) => match list.as_slice() {
                    [
                        Token {
                            inner: TA_Assignment(op, lhs, _),
                            ..
                        },
                    ] if op == "=" => match &lhs.inner {
                        TA_Variable(var, _) => Some(var.clone()),
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    }
    let Some(s) = loop_variable(token) else {
        return;
    };
    if s == "_" {
        return;
    }
    let path = get_parents_of_id(&params.parent_map, token.id);
    if let Some(next) = path
        .iter()
        .find(|x| loop_variable(x).as_deref() == Some(s.as_str()))
    {
        warn(
            out,
            token.id,
            2165,
            "This nested loop overrides the index variable of its parent.",
        );
        warn(
            out,
            next.id,
            2167,
            "This parent loop has its index variable overridden.",
        );
    }
}

pub fn check_trailing_bracket(_: &Parameters<'_>, token: &Token, out: &mut Out) {
    let T_SimpleCommand(_, tokens) = &token.inner else {
        return;
    };
    let Some(last) = tokens.last() else {
        return;
    };
    if let T_NormalWord(parts) = &last.inner
        && let [
            Token {
                inner: T_Literal(s),
                ..
            },
        ] = parts.as_slice()
        && (s == "]]" || s == "]")
    {
        let opposite = if s == "]]" { "[[" } else { "[" };
        if !oversimplify(token).iter().any(|p| p == opposite) {
            warn(
                out,
                last.id,
                2171,
                &format!(
                    "Found trailing {s} outside test. Add missing {opposite} or quote if intentional."
                ),
            );
        }
    }
}

pub fn check_return_against_zero(params: &Parameters<'_>, token: &Token, out: &mut Out) {
    let is_exit_code = |t: &Token| match get_word_parts(t).as_slice() {
        [
            Token {
                inner: T_DollarBraced(_, l),
                ..
            },
        ] => concat_oversimplify(l) == "?",
        _ => false,
    };
    let is_zero = |t: &Token| get_literal_string(t).as_deref() == Some("0");
    let checks_success_lhs = |op: &str| !["-gt", "-ne", "!=", "!"].contains(&op);
    let checks_success_rhs = |op: &str| !["-ne", "!="].contains(&op);
    let message = |for_success: bool, id: Id, out: &mut Out| {
        if is_only_test_in_command(params, token) && !is_first_command_in_function(params, token) {
            style(
                out,
                id,
                2181,
                &format!(
                    "Check exit code directly with e.g. 'if {}mycmd;', not indirectly with $?.",
                    if for_success { "" } else { "! " }
                ),
            );
        }
    };
    let check = |op: &str, lhs: &Token, rhs: &Token, out: &mut Out| {
        if is_zero(rhs) && is_exit_code(lhs) {
            message(checks_success_lhs(op), lhs.id, out);
        } else if is_zero(lhs) && is_exit_code(rhs) {
            message(checks_success_rhs(op), rhs.id, out);
        }
    };
    match &token.inner {
        TC_Binary(_, op, lhs, rhs) => check(op, lhs, rhs, out),
        TA_Binary(op, lhs, rhs) if [">", "<", ">=", "<=", "==", "!="].contains(&op.as_str()) => {
            check(op, lhs, rhs, out);
        }
        TA_Unary(op, exp) if op == "!" && is_exit_code(exp) => {
            message(checks_success_lhs(op), exp.id, out);
        }
        TA_Sequence(list) if list.len() == 1 && is_exit_code(&list[0]) => {
            message(false, list[0].id, out);
        }
        _ => {}
    }
}

fn is_only_test_in_command(params: &Parameters<'_>, t: &Token) -> bool {
    let parents = get_parents_of_id(&params.parent_map, t.id);
    match parents.as_slice() {
        [
            Token {
                inner: T_Condition(..) | T_Arithmetic(_),
                ..
            },
            ..,
        ] => true,
        [
            Token {
                inner: TA_Sequence(l),
                ..
            },
            Token {
                inner: T_Arithmetic(_),
                ..
            },
            ..,
        ] if l.len() == 1 => true,
        [next, ..] => match &next.inner {
            TC_Unary(_, op, _) if op == "!" => is_only_test_in_command(params, next),
            TA_Unary(op, _) if op == "!" => is_only_test_in_command(params, next),
            TC_Group(..) | TA_Parenthesis(_) => is_only_test_in_command(params, next),
            TA_Sequence(l) if l.len() == 1 => is_only_test_in_command(params, next),
            _ => false,
        },
        _ => false,
    }
}

fn get_first_command_in_function(t: &Token) -> &Token {
    match &t.inner {
        T_Function(_, _, _, x) | T_Annotation(_, x) | T_AndIf(x, _) | T_OrIf(x, _) => {
            get_first_command_in_function(x)
        }
        T_BraceGroup(l) | T_Subshell(l) if !l.is_empty() => get_first_command_in_function(&l[0]),
        T_Pipeline(_, l) if !l.is_empty() => get_first_command_in_function(&l[0]),
        T_Redirecting(_, inner) => match &inner.inner {
            T_IfExpression(branches, _) if !branches.is_empty() && !branches[0].0.is_empty() => {
                get_first_command_in_function(&branches[0].0[0])
            }
            _ => t,
        },
        _ => t,
    }
}

fn is_first_command_in_function(params: &Parameters<'_>, token: &Token) -> bool {
    let path = path_of(params, token);
    let Some(func) = path.iter().find(|x| is_function(x)) else {
        return false;
    };
    let Some(this) = path.first() else {
        return false;
    };
    let Some(cmd) = get_closest_command(&params.parent_map, this) else {
        return false;
    };
    cmd.id == get_first_command_in_function(func).id
}

pub fn check_redirected_nowhere(params: &Parameters<'_>, token: &Token, out: &mut Out) {
    fn get_dangling_redirect(t: &Token) -> Option<&Token> {
        match &t.inner {
            T_Redirecting(redirs, cmd) if !redirs.is_empty() => match &cmd.inner {
                T_SimpleCommand(a, w) if a.is_empty() && w.is_empty() => Some(&redirs[0]),
                _ => None,
            },
            _ => None,
        }
    }
    fn is_in_expansion(params: &Parameters<'_>, t: &Token) -> bool {
        match get_parents_of_id(&params.parent_map, t.id).first() {
            Some(p) => match &p.inner {
                T_DollarExpansion(l) | T_Backticked(l) if l.len() == 1 => true,
                T_Annotation(..) => is_in_expansion(params, p),
                _ => false,
            },
            None => false,
        }
    }
    if let T_Pipeline(_, list) = &token.inner {
        if let [single] = list.as_slice() {
            if let Some(redir) = get_dangling_redirect(single)
                && !is_in_expansion(params, token)
            {
                warn(
                    out,
                    redir.id,
                    2188,
                    "This redirection doesn't have a command. Move to its command (or use 'true' as no-op).",
                );
            }
        } else {
            for x in list {
                if let Some(redir) = get_dangling_redirect(x) {
                    err(
                        out,
                        redir.id,
                        2189,
                        "You can't have | between this redirection and the command it should apply to.",
                    );
                }
            }
        }
    }
}

pub fn check_unmatchable_cases(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_CaseExpression(word, list) = &t.inner else {
        return;
    };
    let allpatterns: Vec<&Token> = list.iter().flat_map(|(_, p, _)| p.iter()).collect();
    let breakpatterns: Vec<&Token> = list
        .iter()
        .filter(|(typ, _, _)| *typ == CaseType::CaseBreak)
        .flat_map(|(_, p, _)| p.iter())
        .collect();
    if is_constant(word) {
        warn(
            out,
            word.id,
            2194,
            "This word is constant. Did you forget the $ on a variable?",
        );
    } else {
        let target = word_to_pseudo_glob(word);
        for candidate in &allpatterns {
            if !pseudo_globs_can_overlap(&target, &word_to_pseudo_glob(candidate)) {
                warn(
                    out,
                    candidate.id,
                    2195,
                    "This pattern will never match the case statement's word. Double check them.",
                );
            }
        }
    }
    let pattern_context = |id: Id| -> String {
        match params.token_positions.get(&id) {
            Some((start, _)) => format!(" on line {}.", start.line),
            None => ".".to_string(),
        }
    };
    let exact: Vec<(&Token, Option<Vec<PseudoGlob>>)> = breakpatterns
        .iter()
        .map(|x| (*x, word_to_exact_pseudo_glob(x)))
        .collect();
    let fuzzy: Vec<(&Token, Vec<PseudoGlob>)> = breakpatterns
        .iter()
        .map(|x| (*x, word_to_pseudo_glob(x)))
        .collect();
    for (i, (glob, x)) in exact.iter().enumerate() {
        let Some(x) = x else {
            continue;
        };
        let rest = &fuzzy[(i + 1).min(fuzzy.len())..];
        if let Some((first, _)) = rest.iter().find(|(_, p)| pseudo_glob_is_super_set_of(x, p)) {
            warn(
                out,
                glob.id,
                2221,
                &format!(
                    "This pattern always overrides a later one{}",
                    pattern_context(first.id)
                ),
            );
            warn(
                out,
                first.id,
                2222,
                &format!(
                    "This pattern never matches because of a previous pattern{}",
                    pattern_context(glob.id)
                ),
            );
        }
    }
}

pub fn check_subshell_as_test(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_Subshell(list) = &t.inner else {
        return;
    };
    let [w] = list.as_slice() else {
        return;
    };
    let id = t.id;
    let mut cur = w;
    loop {
        match &cur.inner {
            T_Banged(w) | T_AndIf(w, _) | T_OrIf(w, _) => cur = w,
            T_Pipeline(_, cmds) => {
                if let [
                    Token {
                        inner: T_Redirecting(_, c),
                        ..
                    },
                ] = cmds.as_slice()
                    && let T_SimpleCommand(vars, words) = &c.inner
                    && vars.is_empty()
                    && words.len() >= 2
                {
                    let (first, second) = (&words[0], &words[1]);
                    if get_literal_string(first)
                        .is_some_and(|s| UNARY_TEST_OPS.contains(&s.as_str()))
                    {
                        err(
                            out,
                            id,
                            2204,
                            "(..) is a subshell. Did you mean [ .. ], a test expression?",
                        );
                    }
                    if get_literal_string(second)
                        .is_some_and(|s| BINARY_TEST_OPS.contains(&s.as_str()))
                    {
                        warn(
                            out,
                            id,
                            2205,
                            "(..) is a subshell. Did you mean [ .. ], a test expression?",
                        );
                    }
                }
                return;
            }
            _ => return,
        }
    }
}

pub fn check_splitting_in_arrays(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_Array(elements) = &t.inner else {
        return;
    };
    let ksh = params.shell_type == Shell::Ksh;
    for word in elements {
        let T_NormalWord(parts) = &word.inner else {
            continue;
        };
        for part in parts {
            match &part.inner {
                T_DollarExpansion(_) | T_DollarBraceCommandExpansion(..) | T_Backticked(_) => warn(
                    out,
                    part.id,
                    2207,
                    if ksh {
                        "Prefer read -A or while read to split command output (or quote to avoid splitting)."
                    } else {
                        "Prefer mapfile or read -a to split command output (or quote to avoid splitting)."
                    },
                ),
                T_DollarBraced(_, s)
                    if !is_counting_reference(part)
                        && !is_quoted_alternative_reference(part)
                        && !VARIABLES_WITHOUT_SPACES
                            .contains(&get_braced_reference(&concat_oversimplify(s)).as_str()) =>
                {
                    warn(
                        out,
                        part.id,
                        2206,
                        if ksh {
                            "Quote to prevent word splitting/globbing, or split robustly with read -A or while read."
                        } else {
                            "Quote to prevent word splitting/globbing, or split robustly with mapfile or read -a."
                        },
                    );
                }
                _ => {}
            }
        }
    }
}

pub fn check_redirection_to_number(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_IoFile(_, word) = &t.inner
        && let Some(file) = get_unquoted_literal(word)
        && file.chars().all(|c| c.is_ascii_digit())
    {
        warn(
            out,
            t.id,
            2210,
            "This is a file redirection. Was it supposed to be a comparison or fd operation?",
        );
    }
}

pub fn check_glob_as_command(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_SimpleCommand(_, words) = &t.inner
        && let Some(first) = words.first()
        && is_glob(first)
    {
        warn(
            out,
            first.id,
            2211,
            "This is a glob used as a command name. Was it supposed to be in ${..}, array, or is it missing quoting?",
        );
    }
}

pub fn check_flag_as_command(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_SimpleCommand(vars, words) = &t.inner
        && vars.is_empty()
        && let Some(first) = words.first()
        && is_unquoted_flag(first)
    {
        warn(
            out,
            first.id,
            2215,
            "This flag is used as a command name. Bad line break or missing [ .. ]?",
        );
    }
}

pub fn check_empty_condition(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let TC_Empty(_) = &t.inner {
        style(
            out,
            t.id,
            2212,
            "Use 'false' instead of empty [/[[ conditionals.",
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::enum_variant_names,
    reason = "the constructors of ShellCheck's PipeType"
)]
enum PipeType {
    StdoutPipe,
    StdoutStderrPipe,
    NoPipe,
}

fn tree_contains(pred: &dyn Fn(&Token) -> bool, t: &Token) -> bool {
    let mut found = false;
    do_analysis(t, &mut |x| {
        if !found && pred(x) {
            found = true;
        }
    });
    found
}

fn get_default_fds(redir: &Token) -> Option<Vec<i64>> {
    match &redir.inner {
        T_HereDoc(..) | T_HereString(_) => Some(vec![0]),
        T_IoFile(op, _) => match &op.inner {
            T_Less => Some(vec![0]),
            T_Greater | T_DGREAT | T_CLOBBER => Some(vec![1]),
            T_GREATAND => Some(vec![1, 2]),
            T_IoDuplicate(op2, s) if s == "-" => get_default_fds(op2),
            _ => None,
        },
        _ => None,
    }
}

fn get_redirection_fds(t: &Token) -> Option<Vec<i64>> {
    match &t.inner {
        T_FdRedirect(fd, x) if fd.is_empty() => get_default_fds(x),
        T_FdRedirect(fd, _) if fd == "&" => Some(vec![1, 2]),
        T_FdRedirect(num, x) if num.chars().all(|c| c.is_ascii_digit()) => {
            get_default_fds(x)?;
            Some(vec![num.parse::<i64>().unwrap_or(i64::MAX)])
        }
        _ => None,
    }
}

fn get_op_id(t: &Token) -> Id {
    match &t.inner {
        T_FdRedirect(_, x) => get_op_id(x),
        T_IoFile(op, _) => op.id,
        _ => t.id,
    }
}

pub fn check_pipe_to_nowhere(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let may_consume = |t: &Token| match &t.inner {
        T_ProcSub(op, _) => op == "<",
        T_Backticked(_) | T_DollarExpansion(_) => true,
        _ => false,
    };
    let may_produce = |t: &Token| matches!(&t.inner, T_ProcSub(op, _) if op == ">");
    let has_additional_consumers = |t: &Token| tree_contains(&may_consume, t);
    let has_additional_producers = |t: &Token| tree_contains(&may_produce, t);
    let interactive_flag_cmds = ["cp", "mv", "rm"];
    let has_interactive_flag = |cmd: &Token| has_flag(cmd, "i") || has_flag(cmd, "interactive");
    let command_specific_exception = |name: &str, cmd: &Token| -> bool {
        if name == "du" {
            get_all_flags(cmd)
                .iter()
                .any(|(_, f)| f == "exclude-from" || f == "files0-from")
        } else if interactive_flag_cmds.contains(&name) {
            has_interactive_flag(cmd)
        } else {
            false
        }
    };
    let fd_name = |n: i64| -> String {
        match n {
            0 => "stdin".to_string(),
            1 => "stdout".to_string(),
            2 => "stderr".to_string(),
            _ => format!("FD {n}"),
        }
    };
    let pipe_type = |t: &Token| match &t.inner {
        T_Pipe(s) if s == "|" => PipeType::StdoutPipe,
        T_Pipe(s) if s == "|&" => PipeType::StdoutStderrPipe,
        _ => PipeType::NoPipe,
    };
    match &t.inner {
        T_Pipeline(pipes, cmds) => {
            let pipe_types: Vec<PipeType> = pipes.iter().map(pipe_type).collect();
            let mut inputs = vec![PipeType::NoPipe];
            inputs.extend(pipe_types.iter().copied());
            let mut outputs = pipe_types.clone();
            outputs.push(PipeType::NoPipe);
            for ((input, stage), output) in inputs.iter().zip(cmds.iter()).zip(outputs.iter()) {
                let has_consumers = has_additional_consumers(stage);
                let has_producers = has_additional_producers(stage);
                if let Some(cmd) = get_command(stage)
                    && let Some(name) = get_command_basename(cmd)
                    && NON_READING_COMMANDS.contains(&name.as_str())
                    && !has_consumers
                    && *input != PipeType::NoPipe
                    && !command_specific_exception(&name, cmd)
                {
                    let suggestion = if name == "echo" {
                        "Did you want 'cat' instead?"
                    } else {
                        "Wrong command or missing xargs?"
                    };
                    warn(
                        out,
                        cmd.id,
                        2216,
                        &format!(
                            "Piping to '{name}', a command that doesn't read stdin. {suggestion}"
                        ),
                    );
                }
                let T_Redirecting(redirs, _) = &stage.inner else {
                    continue;
                };
                let Some(fds): Option<Vec<Vec<i64>>> =
                    redirs.iter().map(get_redirection_fds).collect()
                else {
                    continue;
                };
                let mut fd_map: BTreeMap<i64, Vec<&Token>> = BTreeMap::new();
                for (list, redir) in fds.iter().zip(redirs.iter()) {
                    for n in list {
                        fd_map.entry(*n).or_default().insert(0, redir);
                    }
                }
                if *input != PipeType::NoPipe
                    && !has_consumers
                    && let Some(over) = fd_map.get(&0).and_then(|l| l.first())
                {
                    err(
                        out,
                        get_op_id(over),
                        2259,
                        "This redirection overrides piped input. To use both, merge or pass filenames.",
                    );
                }
                if *output == PipeType::StdoutPipe
                    && !has_producers
                    && let Some(over) = fd_map.get(&1).and_then(|l| l.first())
                {
                    err(
                        out,
                        get_op_id(over),
                        2260,
                        "This redirection overrides the output pipe. Use 'tee' to output to both.",
                    );
                }
                for (n, list) in &fd_map {
                    if list.len() >= 2 {
                        for c in list {
                            err(
                                out,
                                get_op_id(c),
                                2261,
                                &format!(
                                    "Multiple redirections compete for {}. Use cat, tee, or pass filenames instead.",
                                    fd_name(*n)
                                ),
                            );
                        }
                    }
                }
            }
        }
        T_Redirecting(redirects, cmd)
            if redirects
                .iter()
                .any(|r| get_redirection_fds(r).is_some_and(|f| f.contains(&0))) =>
        {
            if let Some(name) = get_command_basename(cmd)
                && NON_READING_COMMANDS.contains(&name.as_str())
                && !has_additional_consumers(cmd)
                && !(interactive_flag_cmds.contains(&name.as_str()) && has_interactive_flag(cmd))
            {
                let suggestion = if name == "echo" {
                    "Did you want 'cat' instead?"
                } else {
                    "Bad quoting, wrong command or missing xargs?"
                };
                warn(
                    out,
                    cmd.id,
                    2217,
                    &format!(
                        "Redirecting to '{name}', a command that doesn't read stdin. {suggestion}"
                    ),
                );
            }
        }
        _ => {}
    }
}

pub fn check_for_loop_glob_variables(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_ForIn(_, words, _) = &t.inner else {
        return;
    };
    for w in words {
        if let T_NormalWord(parts) = &w.inner
            && parts.iter().any(is_glob)
        {
            for p in parts.iter().filter(|x| is_quoteable_expansion(x)) {
                info(
                    out,
                    p.id,
                    2231,
                    "Quote expansions in this for loop glob to prevent wordsplitting, e.g. \"$dir\"/*.txt .",
                );
            }
        }
    }
}

pub fn check_subshelled_tests(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_Subshell(list) = &t.inner else {
        return;
    };
    fn local_is_test_command(t: &Token) -> bool {
        match &t.inner {
            T_Pipeline(pipes, cmds) if pipes.is_empty() => match cmds.as_slice() {
                [
                    Token {
                        inner: T_Redirecting(_, cmd),
                        ..
                    },
                ] => match cmd.inner {
                    T_Condition(..) => true,
                    _ => is_command(cmd, "test"),
                },
                _ => false,
            },
            _ => false,
        }
    }
    fn is_test_structure(t: &Token) -> bool {
        match &t.inner {
            T_Banged(t) => is_test_structure(t),
            T_AndIf(a, b) | T_OrIf(a, b) => is_test_structure(a) && is_test_structure(b),
            T_Pipeline(pipes, cmds) if pipes.is_empty() => match cmds.as_slice() {
                [
                    Token {
                        inner: T_Redirecting(_, cmd),
                        ..
                    },
                ] => match &cmd.inner {
                    T_BraceGroup(ts) | T_Subshell(ts) => ts.iter().all(is_test_structure),
                    _ => local_is_test_command(t),
                },
                _ => local_is_test_command(t),
            },
            _ => local_is_test_command(t),
        }
    }
    let has_assignment = tree_contains(
        &|x: &Token| match &x.inner {
            TA_Assignment(..) | T_DollarBraceCommandExpansion(..) => true,
            TA_Unary(s, _) => s.contains("++") || s.contains("--"),
            T_DollarBraced(_, l) => {
                let m = get_braced_modifier(&concat_oversimplify(l));
                m.starts_with('=') || m.starts_with(":=")
            }
            _ => false,
        },
        t,
    );
    if !list.iter().all(is_test_structure) || has_assignment {
        return;
    }
    let path = path_of(params, t);
    let is_compound_condition = {
        let rest: Vec<&&Token> = path
            .iter()
            .skip(1)
            .skip_while(|x| match &x.inner {
                T_Redirecting(r, _) => r.is_empty(),
                T_Pipeline(p, _) => p.is_empty(),
                T_Annotation(..) => true,
                _ => false,
            })
            .collect();
        matches!(
            rest.first().map(|x| &x.inner),
            Some(T_IfExpression(..) | T_WhileExpression(..) | T_UntilExpression(..))
        )
    };
    let is_single_test = matches!(list.as_slice(), [c] if is_test_command(c));
    let is_function_body = path.get(1).is_some_and(|f| is_function(f));
    if is_compound_condition {
        style(
            out,
            t.id,
            2233,
            "Remove superfluous (..) around condition to avoid subshell overhead.",
        );
    } else if is_single_test && !is_function_body {
        style(
            out,
            t.id,
            2234,
            "Remove superfluous (..) around test command to avoid subshell overhead.",
        );
    } else {
        style(
            out,
            t.id,
            2235,
            "Use { ..; } instead of (..) to avoid subshell overhead.",
        );
    }
}

pub fn check_unnecessarily_inverted_test(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let inversion = |op: &str| -> Option<&'static str> {
        Some(match op {
            "=" | "==" => "!=",
            "!=" => "=",
            "-eq" => "-ne",
            "-ne" => "-eq",
            "-le" => "-gt",
            "-gt" => "-le",
            "-ge" => "-lt",
            "-lt" => "-ge",
            _ => return None,
        })
    };
    let suggest = |bang_inside: bool, style_: ConditionType, op: &str, out: &mut Out| {
        let Some(new_op) = inversion(op) else {
            return;
        };
        let old_expr = format!("a {op} b");
        let new_expr = format!("a {new_op} b");
        let bracket = |s: &str| {
            if style_ == ConditionType::SingleBracket {
                format!("[ {s} ]")
            } else {
                format!("[[ {s} ]]")
            }
        };
        if bang_inside {
            style(
                out,
                t.id,
                2335,
                &format!("Use {new_expr} instead of ! {old_expr}."),
            );
        } else {
            style(
                out,
                t.id,
                2335,
                &format!(
                    "Use {} instead of ! {}.",
                    bracket(&new_expr),
                    bracket(&old_expr)
                ),
            );
        }
    };
    fn banged_condition(t: &Token) -> Option<&Token> {
        let T_Banged(p) = &t.inner else {
            return None;
        };
        let T_Pipeline(_, cmds) = &p.inner else {
            return None;
        };
        let [
            Token {
                inner: T_Redirecting(_, c),
                ..
            },
        ] = cmds.as_slice()
        else {
            return None;
        };
        match &c.inner {
            T_Condition(_, inner) => Some(inner),
            _ => None,
        }
    }
    match &t.inner {
        TC_Unary(_, bang, inner) if bang == "!" => match &inner.inner {
            TC_Unary(_, op, _) => match op.as_str() {
                "-n" => style(out, t.id, 2236, "Use -z instead of ! -n."),
                "-z" => style(out, t.id, 2236, "Use -n instead of ! -z."),
                _ => {}
            },
            TC_Binary(bs, op, _, _) => suggest(true, *bs, op, out),
            _ => {}
        },
        T_Banged(_) => {
            if let Some(cond) = banged_condition(t) {
                match &cond.inner {
                    TC_Unary(_, op, _) => match op.as_str() {
                        "-n" => style(out, t.id, 2237, "Use [ -z .. ] instead of ! [ -n .. ]."),
                        "-z" => style(out, t.id, 2237, "Use [ -n .. ] instead of ! [ -z .. ]."),
                        _ => {}
                    },
                    TC_Binary(bs, op, _, _) => suggest(false, *bs, op, out),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

pub fn check_redirection_to_command(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_IoFile(_, w) = &t.inner
        && let T_NormalWord(parts) = &w.inner
        && let [
            Token {
                inner: T_Literal(s),
                ..
            },
        ] = parts.as_slice()
        && COMMON_COMMANDS.contains(&s.as_str())
        && s != "file"
    {
        warn(
            out,
            w.id,
            2238,
            "Redirecting to/from command name instead of file. Did you want pipes/xargs (or quote to ignore)?",
        );
    }
}

pub fn check_nullary_expansion_test(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let TC_Nullary(_, word) = &t.inner else {
        return;
    };
    let id = word.id;
    let fix = || fix_with(vec![replace_start(id, params, 0, "-n ")]);
    let parts = get_word_parts(word);
    match parts.as_slice() {
        [x] if is_command_substitution(x) => style_with_fix(
            out,
            id,
            2243,
            "Prefer explicit -n to check for output (or run command without [/[[ to check for success).",
            fix(),
        ),
        x if x.iter().all(|p| !is_constant(p)) => style_with_fix(
            out,
            id,
            2244,
            "Prefer explicit -n to check non-empty string (or use =/-ne to check boolean/integer).",
            fix(),
        ),
        _ => {}
    }
}

pub fn check_dollar_quote_paren(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_DollarDoubleQuoted(parts) = &t.inner
        && let Some(Token {
            inner: T_Literal(s),
            ..
        }) = parts.first()
        && s.starts_with(['(', '{'])
    {
        warn_with_fix(
            out,
            t.id,
            2247,
            "Flip leading $ and \" if this should be a quoted substitution.",
            fix_with(vec![replace_start(t.id, params, 2, "\"$")]),
        );
    }
}

pub fn check_translated_string_variable(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_DollarDoubleQuoted(parts) = &t.inner
        && let [Token { inner: T_Literal(s), .. }] = parts.as_slice()
        && s.chars().all(is_variable_char)
        && params
            .variable_flow
            .iter()
            .any(|d| matches!(d, StackData::Assignment((_, _, name, _)) if name == s && is_variable_name(name)))
    {
        warn_with_fix(
            out,
            t.id,
            2256,
            "This translated string is the name of a variable. Flip leading $ and \" if this should be a quoted substitution.",
            fix_with(vec![replace_start(t.id, params, 2, "\"$")]),
        );
    }
}

pub fn check_default_case(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_CaseExpression(_, list) = &t.inner {
        let can_match_any = |pat: &Token| {
            word_to_exact_pseudo_glob(pat)
                .is_some_and(|pg| pseudo_glob_is_super_set_of(&pg, &[PseudoGlob::PGMany]))
        };
        if !list.iter().any(|(_, l, _)| l.iter().any(can_match_any)) {
            info(
                out,
                t.id,
                2249,
                "Consider adding a default *) case, even if it just exits with error.",
            );
        }
    }
}

pub fn check_useless_bang(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if !params.has_set_e {
        return;
    }
    fn drop_last(t: &[Token]) -> &[Token] {
        if t.is_empty() { t } else { &t[..t.len() - 1] }
    }
    let is_function_body = |t: &Token| {
        get_parents_of_id(&params.parent_map, t.id)
            .first()
            .is_some_and(|p| matches!(p.inner, T_Function(..)))
    };
    fn non_returning<'t>(
        t: &'t Token,
        is_function_body: &dyn Fn(&Token) -> bool,
    ) -> Vec<&'t Token> {
        match &t.inner {
            T_Script(_, list) | T_Subshell(list) => drop_last(list).iter().collect(),
            T_BraceGroup(list) => {
                if is_function_body(t) {
                    drop_last(list).iter().collect()
                } else {
                    list.iter().collect()
                }
            }
            T_WhileExpression(conds, cmds) | T_UntilExpression(conds, cmds) => {
                drop_last(conds).iter().chain(cmds.iter()).collect()
            }
            T_ForIn(_, _, list) | T_ForArithmetic(_, _, _, list) => list.iter().collect(),
            T_Annotation(_, t) => non_returning(t, is_function_body),
            T_IfExpression(conds, elses) => {
                let mut out: Vec<&Token> = Vec::new();
                for (c, _) in conds {
                    out.extend(drop_last(c).iter());
                }
                for (_, s) in conds {
                    out.extend(s.iter());
                }
                out.extend(elses.iter());
                out
            }
            _ => Vec::new(),
        }
    }
    fn check(params: &Parameters<'_>, t: &Token, out: &mut Out) {
        match &t.inner {
            T_Banged(cmd) => {
                let path = path_of(params, t);
                if !is_condition(&path) {
                    out.push(super::analyzerlib::make_comment_with_fix(
                        super::interface::Severity::InfoC,
                        t.id,
                        2251,
                        "This ! is not on a condition and skips errexit. Use `&& exit 1` instead, or make sure $? is checked.",
                        fix_with(vec![replace_start(t.id, params, 1, ""), replace_end(cmd.id, params, 0, " && exit 1")]),
                    ));
                }
            }
            T_Annotation(_, t) => check(params, t, out),
            _ => {}
        }
    }
    for c in non_returning(t, &is_function_body) {
        check(params, c, out);
    }
}

pub fn check_modified_arithmetic_in_redirection(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if matches!(params.shell_type, Shell::Dash | Shell::BusyboxSh) {
        return;
    }
    let T_Redirecting(redirs, cmd) = &t.inner else {
        return;
    };
    if !matches!(&cmd.inner, T_SimpleCommand(_, w) if !w.is_empty()) {
        return;
    }
    fn check_modifying(t: &Token, out: &mut Out) {
        match &t.inner {
            TA_Sequence(list) => list.iter().for_each(|x| check_modifying(x, out)),
            TA_Unary(s, _) if ["|++", "++|", "|--", "--|"].contains(&s.as_str()) => {
                warn_for(t.id, out);
            }
            TA_Assignment(..) => warn_for(t.id, out),
            TA_Binary(_, x, y) => {
                check_modifying(x, out);
                check_modifying(y, out);
            }
            TA_Trinary(x, y, z) => {
                check_modifying(x, out);
                check_modifying(y, out);
                check_modifying(z, out);
            }
            _ => {}
        }
    }
    fn warn_for(id: Id, out: &mut Out) {
        warn(
            out,
            id,
            2257,
            "Arithmetic modifications in command redirections may be discarded. Do them separately.",
        );
    }
    let check_arithmetic = |t: &Token, out: &mut Out| {
        if let T_DollarArithmetic(x) = &t.inner {
            check_modifying(x, out);
        }
    };
    for r in redirs {
        let T_FdRedirect(_, x) = &r.inner else {
            continue;
        };
        match &x.inner {
            T_IoFile(_, word) | T_HereString(word) => {
                for p in get_word_parts(word) {
                    check_arithmetic(p, out);
                }
            }
            T_HereDoc(_, _, _, list) => {
                for p in list {
                    check_arithmetic(p, out);
                }
            }
            _ => {}
        }
    }
}

pub fn check_blatant_recursion(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_Function(_, _, name, body) = &t.inner else {
        return;
    };
    let seqs = get_command_sequences(body);
    let [seq] = seqs.as_slice() else {
        return;
    };
    let Some(first) = seq.first() else {
        return;
    };
    fn check_list(params: &Parameters<'_>, name: &str, t: &Token, out: &mut Out) {
        match &t.inner {
            T_Backgrounded(t) | T_AndIf(t, _) | T_OrIf(t, _) => check_list(params, name, t, out),
            T_Pipeline(_, cmds) => {
                for cmd in cmds {
                    let (invoked, tok) = get_command_name_and_token(true, cmd);
                    if invoked.as_deref() == Some(name) {
                        err_with_fix(
                            out,
                            tok.id,
                            2264,
                            "This function unconditionally re-invokes itself. Missing 'command'?",
                            fix_with(vec![replace_start(tok.id, params, 0, "command ")]),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    check_list(params, name, first, out);
}

pub fn check_bad_test_and_or(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    match &t.inner {
        T_Pipeline(seps, cmds) if cmds.len() >= 2 => {
            let check_pipe = |p: Option<&Token>, out: &mut Out| {
                if let Some(pt) = p
                    && let T_Pipe(s) = &pt.inner
                    && s == "|"
                {
                    warn_with_fix(
                        out,
                        pt.id,
                        2266,
                        "Use || for logical OR. Single | will pipe.",
                        fix_with(vec![replace_end(pt.id, params, 0, "|")]),
                    );
                }
            };
            for (i, cmd) in cmds.iter().enumerate() {
                if is_test_command(cmd) {
                    let before = if i == 0 { None } else { seps.get(i - 1) };
                    let after = seps.get(i);
                    check_pipe(before, out);
                    check_pipe(after, out);
                }
            }
        }
        T_Backgrounded(cmd) => {
            let id = t.id;
            let mut cur: &Token = cmd;
            loop {
                match &cur.inner {
                    T_AndIf(_, rhs) | T_OrIf(_, rhs) => cur = rhs,
                    T_Pipeline(_, list) if !list.is_empty() => {
                        cur = list.last().expect("non-empty");
                    }
                    _ => {
                        if is_test_command(cur) {
                            err_with_fix(
                                out,
                                id,
                                2265,
                                "Use && for logical AND. Single & will background and return true.",
                                fix_with(vec![replace_end(id, params, 0, "&")]),
                            );
                        }
                        break;
                    }
                }
            }
        }
        _ => {}
    }
}

pub fn check_comparison_with_leading_x(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let fix_leading_x = |token: &Token| -> Option<super::interface::Replacement> {
        let parts = get_word_parts(token);
        match parts.first().map(|p| (p.id, &p.inner)) {
            Some((id, T_Literal(s)))
                if s.chars().next().is_some_and(|c| hchar::to_lower(c) == 'x') =>
            {
                if let T_NormalWord(w) = &token.inner
                    && let [
                        Token {
                            id: lid,
                            inner: T_Literal(one),
                        },
                    ] = w.as_slice()
                    && one.chars().count() == 1
                {
                    return Some(replace_start(*lid, params, 1, "\"\""));
                }
                Some(replace_start(id, params, 1, ""))
            }
            Some((id, T_SingleQuoted(s)))
                if s.chars().next().is_some_and(|c| hchar::to_lower(c) == 'x') =>
            {
                Some(replace_start(id, params, 2, "'"))
            }
            _ => None,
        }
    };
    let check = |lhs: &Token, rhs: &Token, out: &mut Out| {
        if let (Some(l), Some(r)) = (fix_leading_x(lhs), fix_leading_x(rhs)) {
            style_with_fix(
                out,
                lhs.id,
                2268,
                "Avoid x-prefix in comparisons as it no longer serves a purpose.",
                fix_with(vec![l, r]),
            );
        }
    };
    match &t.inner {
        TC_Binary(_, op, lhs, rhs) if ["=", "==", "!="].contains(&op.as_str()) => {
            check(lhs, rhs, out);
        }
        T_SimpleCommand(_, words) => {
            if let [cmd, lhs, op, rhs] = words.as_slice()
                && get_literal_string(cmd).as_deref() == Some("test")
                && matches!(get_literal_string(op).as_deref(), Some("=" | "==" | "!="))
            {
                check(lhs, rhs, out);
            }
        }
        _ => {}
    }
}

pub fn check_assign_to_self(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_SimpleCommand(vars, words) = &t.inner
        && words.is_empty()
    {
        for v in vars {
            if let T_Assignment(AssignmentMode::Assign, name, indices, value) = &v.inner
                && indices.is_empty()
                && let [
                    Token {
                        inner: T_DollarBraced(_, b),
                        ..
                    },
                ] = get_word_parts(value).as_slice()
                && get_literal_string(b).as_deref() == Some(name.as_str())
            {
                info(
                    out,
                    v.id,
                    2269,
                    "This variable is assigned to itself, so the assignment does nothing.",
                );
            }
        }
    }
}

pub fn check_equals_in_command(params: &Parameters<'_>, original: &Token, out: &mut Out) {
    let T_SimpleCommand(vars, words) = &original.inner else {
        return;
    };
    let Some(word) = words.first() else {
        return;
    };
    let T_NormalWord(list) = &word.inner else {
        return;
    };
    let has_equals = |t: &Token| matches!(&t.inner, T_Literal(s) if s.contains('='));
    let Some(pos) = list.iter().position(has_equals) else {
        return;
    };
    let mut leading: &[Token] = &list[..pos];
    if let Some(Token {
        inner: T_Literal(s),
        ..
    }) = leading.last()
        && s == "+"
    {
        leading = &leading[..leading.len() - 1];
    }
    let eq = &list[pos];
    let T_Literal(s) = &eq.inner else {
        return;
    };
    let lit_id = eq.id;
    let cmd = word;
    let assign0 = |id: Id, bashfix: Fix, out: &mut Out| match params.shell_type {
        Shell::Bash => err_with_fix(
            out,
            id,
            2277,
            "Use BASH_ARGV0 to assign to $0 in bash (or use [ ] to compare).",
            bashfix,
        ),
        Shell::Ksh => err(
            out,
            id,
            2278,
            "$0 can't be assigned in Ksh (but it does reflect the current function).",
        ),
        Shell::Dash => err(
            out,
            id,
            2279,
            "$0 can't be assigned in Dash. This becomes a command name.",
        ),
        Shell::BusyboxSh => err(
            out,
            id,
            2279,
            "$0 can't be assigned in Busybox Ash. This becomes a command name.",
        ),
        Shell::Sh => err(
            out,
            id,
            2280,
            "$0 can't be assigned this way, and there is no portable alternative.",
        ),
    };
    let positional = |id: Id, out: &mut Out| {
        err(
            out,
            id,
            2270,
            "To assign positional parameters, use 'set -- first second ..' (or use [ ] to compare).",
        );
    };
    let indirection = |id: Id, out: &mut Out| {
        err(
            out,
            id,
            2271,
            "For indirection, use arrays, declare \"var$n=value\", or (for sh) read/eval.",
        );
    };
    let generic = |id: Id, out: &mut Out| {
        err(
            out,
            id,
            2276,
            "This is interpreted as a command name containing '='. Bad assignment or comparison?",
        );
    };
    let is_conflict_marker = |cmd: &Token| {
        get_unquoted_literal(cmd).is_some_and(|s| {
            let n = s.chars().count();
            s.chars().all(|c| c == '=') && (4..=12).contains(&n)
        })
    };
    let may_be_variable_name = |l: &[Token]| -> bool {
        if l.iter().any(is_quotes) || l.iter().any(will_become_multiple_args) {
            return false;
        }
        let w = Token::new(Id(0), T_NormalWord(l.to_vec()));
        get_literal_string_ext(&w, &|_| Some("x".to_string())).is_some_and(|s| is_variable_name(&s))
    };
    let is_leading_number_var = |s: &str| {
        let lead: String = s.chars().take_while(|&c| c != '=').collect();
        match lead.chars().next() {
            Some(x) => {
                x.is_ascii_digit()
                    && lead.chars().all(is_variable_char)
                    && !lead.chars().all(|c| c.is_ascii_digit())
            }
            None => false,
        }
    };
    if leading.is_empty() && s.starts_with('-') {
        return;
    }
    if leading.is_empty() && s.starts_with('=') {
        if vars.is_empty() && words.len() == 1 && is_conflict_marker(word) {
            err(
                out,
                original.id,
                2273,
                "Sequence of ===s found. Merge conflict or intended as a commented border?",
            );
        } else if s.starts_with("===") {
            err(
                out,
                original.id,
                2274,
                "Command name starts with ===. Intended as a commented border?",
            );
        } else {
            err(
                out,
                cmd.id,
                2275,
                "Command name starts with =. Bad line break?",
            );
        }
        return;
    }
    if s.contains("==") {
        err(
            out,
            cmd.id,
            2272,
            "Command name contains ==. For comparison, use [ \"$var\" = value ].",
        );
        return;
    }
    if let [
        Token {
            id,
            inner: T_DollarBraced(braced, l),
        },
    ] = leading
        && s.starts_with('=')
    {
        let id = *id;
        let variable_str = concat_oversimplify(l);
        let variable_reference = get_braced_reference(&variable_str);
        let variable_modifier = get_braced_modifier(&variable_str);
        let is_plain = is_variable_name(&variable_str);
        let is_positional = variable_str.chars().all(|c| c.is_ascii_digit());
        let is_array = !variable_reference.is_empty()
            && variable_modifier.starts_with('[')
            && variable_modifier.ends_with(']');
        if variable_str.is_empty() || variable_str.starts_with('#') {
            generic(cmd.id, out);
        } else if variable_str == "0" {
            assign0(
                id,
                fix_with(vec![replace_token(id, params, "BASH_ARGV0")]),
                out,
            );
        } else if is_positional {
            positional(id, out);
        } else if is_array || is_plain {
            err_with_fix(
                out,
                id,
                2281,
                &format!(
                    "Don't use {} on the left side of assignments.",
                    if *braced { "${}" } else { "$" }
                ),
                fix_with(if *braced {
                    vec![
                        replace_start(id, params, 2, ""),
                        replace_end(id, params, 1, ""),
                    ]
                } else {
                    vec![replace_start(id, params, 1, "")]
                }),
            );
        } else {
            indirection(id, out);
        }
        return;
    }
    if leading.is_empty() && POSITIONAL_ASSIGNMENT_RE.with(|re| re.is_match(s)) {
        if s.starts_with("0=") {
            assign0(
                lit_id,
                fix_with(vec![replace_start(lit_id, params, 1, "BASH_ARGV0")]),
                out,
            );
        } else {
            positional(lit_id, out);
        }
        return;
    }
    if leading.is_empty() && is_leading_number_var(s) {
        err(
            out,
            cmd.id,
            2282,
            "Variable names can't start with numbers, so this is interpreted as a command.",
        );
        return;
    }
    if !leading.is_empty()
        && may_be_variable_name(leading)
        && s.chars().take_while(|&c| c != '=').all(is_variable_char)
    {
        indirection(cmd.id, out);
        return;
    }
    generic(cmd.id, out);
}

pub fn check_second_arg_is_comparison(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_SimpleCommand(_, words) = &t.inner else {
        return;
    };
    let [_, arg, ..] = words.as_slice() else {
        return;
    };
    let Some(arg_string) = get_leading_unquoted_string(arg) else {
        return;
    };
    let head_id = |t: &Token| match &t.inner {
        T_NormalWord(l) if !l.is_empty() => l[0].id,
        _ => t.id,
    };
    if arg_string.starts_with("====") {
        return;
    }
    if arg_string.starts_with("+=") {
        err(
            out,
            head_id(t),
            2285,
            "Remove spaces around += to assign (or quote '+=' if literal).",
        );
    } else if arg_string.starts_with("==") {
        err(
            out,
            t.id,
            2284,
            "Use [ x = y ] to compare values (or quote '==' if literal).",
        );
    } else if arg_string.starts_with('=') {
        err(
            out,
            head_id(arg),
            2283,
            "Remove spaces around = to assign (or use [ ] to compare, or quote '=' if literal).",
        );
    }
}

pub fn check_command_with_trailing_symbol(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_SimpleCommand(_, words) = &t.inner else {
        return;
    };
    let Some(cmd) = words.first() else {
        return;
    };
    let s = get_literal_string_def("x", cmd);
    let last = s.chars().last().unwrap_or('x');
    let format = |x: char| -> String {
        match x {
            ' ' => "space".to_string(),
            '\'' => "apostrophe".to_string(),
            '"' => "doublequote".to_string(),
            x => format!("'{x}'"),
        }
    };
    match s.as_str() {
        "." | ":" | " " | "//" => {}
        "" => err(
            out,
            cmd.id,
            2286,
            "This empty string is interpreted as a command name. Double check syntax (or use 'true' as a no-op).",
        ),
        _ if last == '/' => err(
            out,
            cmd.id,
            2287,
            "This is interpreted as a command name ending with '/'. Double check syntax.",
        ),
        _ if "\\.,([{<>}])#\"'% ".contains(last) => warn(
            out,
            cmd.id,
            2288,
            &format!(
                "This is interpreted as a command name ending with {}. Double check syntax.",
                format(last)
            ),
        ),
        _ if s.contains('\t') => err(
            out,
            cmd.id,
            2289,
            "This is interpreted as a command name containing a tab. Double check syntax.",
        ),
        _ if s.contains('\n') => err(
            out,
            cmd.id,
            2289,
            "This is interpreted as a command name containing a linefeed. Double check syntax.",
        ),
        _ => {}
    }
}

pub fn check_unquoted_parameter_expansion_pattern(
    params: &Parameters<'_>,
    x: &Token,
    out: &mut Out,
) {
    let T_DollarBraced(true, word) = &x.inner else {
        return;
    };
    let T_NormalWord(parts) = &word.inner else {
        return;
    };
    let [
        Token {
            inner: T_Literal(_),
            ..
        },
        rest @ ..,
    ] = parts.as_slice()
    else {
        return;
    };
    if rest.is_empty() {
        return;
    }
    let modifier = get_braced_modifier(&concat_oversimplify(word));
    if !(modifier.starts_with('%') || modifier.starts_with('#')) {
        return;
    }
    for t in rest {
        if matches!(
            t.inner,
            T_DollarBraced(..) | T_DollarExpansion(_) | T_Backticked(_)
        ) {
            info_with_fix(
                out,
                t.id,
                2295,
                "Expansions inside ${..} need to be quoted separately, otherwise they match as patterns.",
                surround_with(t.id, params, "\""),
            );
        }
    }
}

pub fn check_bats_test_does_not_use_negation(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_BatsTest(_, body) = &t.inner else {
        return;
    };
    let T_BraceGroup(commands) = &body.inner else {
        return;
    };
    let is_last_of = |t: &Token| commands.last().is_some_and(|x| x == t);
    for c in commands {
        let T_Banged(cmd) = &c.inner else {
            continue;
        };
        let is_condition = match &cmd.inner {
            T_Pipeline(_, cmds) => matches!(
                cmds.as_slice(),
                [Token { inner: T_Redirecting(_, inner), .. }] if matches!(inner.inner, T_Condition(..))
            ),
            _ => false,
        };
        if is_condition {
            if is_last_of(c) {
                style(
                    out,
                    c.id,
                    2315,
                    "In Bats, ! will not fail the test if it is not the last command anymore. Fold the `!` into the conditional!",
                );
            } else {
                err(
                    out,
                    c.id,
                    2315,
                    "In Bats, ! does not cause a test failure. Fold the `!` into the conditional!",
                );
            }
        } else if is_last_of(c) {
            style_with_fix(
                out,
                c.id,
                2314,
                "In Bats, ! will not fail the test if it is not the last command anymore. Use `run ! ` (on Bats >= 1.5.0) instead.",
                fix_with(vec![replace_start(c.id, params, 0, "run ")]),
            );
        } else {
            err_with_fix(
                out,
                c.id,
                2314,
                "In Bats, ! does not cause a test failure. Use 'run ! ' (on Bats >= 1.5.0) instead.",
                fix_with(vec![replace_start(c.id, params, 0, "run ")]),
            );
        }
    }
}

fn is_sourced(params: &Parameters<'_>, t: &Token) -> bool {
    matches!(t.inner, T_SourceCommand(..))
        || get_parents_of_id(&params.parent_map, t.id)
            .iter()
            .any(|p| matches!(p.inner, T_SourceCommand(..)))
}

pub fn check_command_is_unreachable(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let is_unreachable = |t: &Token| -> bool {
        params
            .cfg_analysis
            .as_ref()
            .and_then(|c| cfganalysis::get_incoming_state(c, t.id))
            .is_some_and(|s| !s.state_is_reachable())
    };
    let is_unreachable_function = |f: &Token| match &f.inner {
        T_Function(_, _, _, body) => is_unreachable(body),
        _ => false,
    };
    match &t.inner {
        T_Pipeline(..) => {
            let Some(cfga) = params.cfg_analysis.as_ref() else {
                return;
            };
            let Some(state) = cfganalysis::get_incoming_state(cfga, t.id) else {
                return;
            };
            if state.state_is_reachable() || is_sourced(params, t) {
                return;
            }
            if get_parents_of_id(&params.parent_map, t.id)
                .iter()
                .any(|p| is_unreachable(p) || is_unreachable_function(p))
            {
                return;
            }
            info(
                out,
                t.id,
                2317,
                "Command appears to be unreachable. Check usage (or ignore if invoked indirectly).",
            );
        }
        T_Function(..)
            if is_unreachable_function(t)
                && !get_parents_of_id(&params.parent_map, t.id)
                    .iter()
                    .any(|p| is_unreachable_function(p))
                && !is_sourced(params, t) =>
        {
            info(
                out,
                t.id,
                2329,
                "This function is never invoked. Check usage (or ignored if invoked indirectly).",
            );
        }
        _ => {}
    }
}

pub fn check_overwritten_exit_code(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_DollarBraced(_, val) = &t.inner else {
        return;
    };
    if get_literal_string(val).as_deref() != Some("?") {
        return;
    }
    let id = t.id;
    let Some(cfga) = params.cfg_analysis.as_ref() else {
        return;
    };
    let Some(state) = cfganalysis::get_incoming_state(cfga, id) else {
        return;
    };
    let exit_code_ids = &state.exit_codes();
    if exit_code_ids.is_empty() {
        return;
    }
    let Some(tokens): Option<Vec<&Token>> = exit_code_ids
        .iter()
        .map(|k| params.id_map.get(k).copied())
        .collect()
    else {
        return;
    };
    let is_cond = |t: &Token| match &t.inner {
        T_Condition(..) => true,
        T_SimpleCommand(..) => get_command_name(t).as_deref() == Some("test"),
        _ => false,
    };
    let used_unconditionally = exit_code_ids
        .iter()
        .all(|c| cfganalysis::does_post_dominate(cfga, t.id, *c));
    if tokens.iter().all(|x| is_cond(x)) && !used_unconditionally {
        warn(
            out,
            id,
            2319,
            "This $? refers to a condition, not a command. Assign to a variable to avoid it being overwritten.",
        );
    }
    let is_printing =
        |t: &Token| matches!(get_command_basename(t).as_deref(), Some("echo" | "printf"));
    if tokens.iter().all(|x| is_printing(x)) {
        warn(
            out,
            id,
            2320,
            "This $? refers to echo/printf, not a previous command. Assign to variable to avoid it being overwritten.",
        );
    }
}

pub fn check_unnecessary_arithmetic_expansion_index(
    params: &Parameters<'_>,
    t: &Token,
    out: &mut Out,
) {
    if let T_Assignment(_, _, indices, _) = &t.inner
        && let [
            Token {
                inner: TA_Sequence(seq),
                ..
            },
        ] = indices.as_slice()
        && let [
            Token {
                inner: TA_Expansion(exp),
                ..
            },
        ] = seq.as_slice()
        && let [
            e @ Token {
                inner: T_DollarArithmetic(_),
                ..
            },
        ] = exp.as_slice()
    {
        style_with_fix(
            out,
            e.id,
            2321,
            "Array indices are already arithmetic contexts. Prefer removing the $(( and )).",
            fix_with(vec![
                replace_start(e.id, params, 3, ""),
                replace_end(e.id, params, 2, ""),
            ]),
        );
    }
}

pub fn check_unnecessary_parens(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let fix = |id: Id| {
        fix_with(vec![
            replace_start(id, params, 1, ""),
            replace_end(id, params, 1, ""),
        ])
    };
    let check_leading = |s: &str, t: &Token, out: &mut Out| {
        if let TA_Sequence(l) = &t.inner
            && let [
                Token {
                    id,
                    inner: TA_Parenthesis(_),
                },
            ] = l.as_slice()
        {
            style_with_fix(
                out,
                *id,
                2323,
                &format!("{s}. Prefer not wrapping in additional parentheses."),
                fix(*id),
            );
        }
    };
    match &t.inner {
        T_DollarArithmetic(x) => check_leading("$(( (x) )) is the same as $(( x ))", x, out),
        T_ForArithmetic(x, y, z, _) => {
            for v in [x, y, z] {
                check_leading(
                    "for (((x); (y); (z))) is the same as for ((x; y; z))",
                    v,
                    out,
                );
            }
        }
        T_Assignment(_, _, indices, _) if indices.len() == 1 => {
            check_leading("a[(x)] is the same as a[x]", &indices[0], out);
        }
        T_Arithmetic(x) => check_leading("(( (x) )) is the same as (( x ))", x, out),
        TA_Parenthesis(inner) => {
            if let TA_Sequence(l) = &inner.inner
                && let [
                    Token {
                        id,
                        inner: TA_Parenthesis(_),
                    },
                ] = l.as_slice()
            {
                style_with_fix(
                    out,
                    *id,
                    2322,
                    "In arithmetic contexts, ((x)) is the same as (x). Prefer only one layer of parentheses.",
                    fix(*id),
                );
            }
        }
        _ => {}
    }
}

pub fn check_plus_equals_number(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_Assignment(AssignmentMode::Append, var, _, word) = &t.inner else {
        return;
    };
    let Some(cfga) = params.cfg_analysis.as_ref() else {
        return;
    };
    let Some(state) = cfganalysis::get_incoming_state(cfga, t.id) else {
        return;
    };
    let unquoted_literal = get_unquoted_literal(word);
    let is_empty = unquoted_literal.as_deref() == Some("");
    let is_unquoted_number = !is_empty
        && unquoted_literal
            .as_ref()
            .is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()));
    let is_numerical_variable_name = unquoted_literal
        .as_ref()
        .and_then(|s| cfganalysis::variable_may_be_assigned_integer(state, s))
        .unwrap_or(false);
    let is_numerical_variable_expansion = match &word.inner {
        T_NormalWord(parts) if parts.len() == 1 => get_unmodified_parameter_expansion(&parts[0])
            .and_then(|s| cfganalysis::variable_may_be_assigned_integer(state, &s))
            .unwrap_or(false),
        _ => false,
    };
    let is_number =
        is_unquoted_number || is_numerical_variable_name || is_numerical_variable_expansion;
    if !is_number {
        return;
    }
    if cfganalysis::variable_may_be_declared_integer(state, var).unwrap_or(false) {
        return;
    }
    warn(
        out,
        t.id,
        2324,
        "var+=1 will append, not increment. Use (( var += 1 )), typeset -i var, or quote number to silence.",
    );
}

pub fn check_expansion_with_redirection(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let (capture_id, cmd) = match &t.inner {
        T_DollarExpansion(c) | T_Backticked(c) | T_DollarBraceCommandExpansion(_, c)
            if c.len() == 1 =>
        {
            (t.id, &c[0])
        }
        _ => return,
    };
    let T_Pipeline(_, cmds) = &cmd.inner else {
        return;
    };
    let Some(last) = cmds.last() else {
        return;
    };
    let T_Redirecting(redirs, _) = &last.inner else {
        // checkCmd has no equation for anything else.
        panic!("ShellCheck: checkExpansionWithRedirection on a non-redirecting command");
    };
    let emit = |redirect_id: Id, suggest_tee: bool, out: &mut Out| {
        warn(
            out,
            capture_id,
            2327,
            "This command substitution will be empty because the command's output gets redirected away.",
        );
        err(
            out,
            redirect_id,
            2328,
            &format!(
                "This redirection takes output away from the command substitution{}",
                if suggest_tee {
                    " (use tee to duplicate)."
                } else {
                    "."
                }
            ),
        );
    };
    for r in redirs {
        let T_FdRedirect(fd, x) = &r.inner else {
            continue;
        };
        match &x.inner {
            T_IoDuplicate(_, target) if target == "1" => return,
            T_IoDuplicate(..) if fd == "1" => return,
            T_IoDuplicate(op, _) if fd.is_empty() && matches!(op.inner, T_GREATAND | T_Greater) => {
                emit(r.id, true, out);
                return;
            }
            T_IoFile(op, file)
                if (fd.is_empty() || fd == "1") && matches!(op.inner, T_DGREAT | T_Greater) =>
            {
                emit(
                    r.id,
                    get_literal_string(file).as_deref() != Some("/dev/null"),
                    out,
                );
                return;
            }
            _ => {}
        }
    }
}

pub fn check_unary_test_a(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let TC_Unary(_, op, _) = &t.inner
        && op == "-a"
    {
        style_with_fix(
            out,
            t.id,
            2331,
            "For file existence, prefer standard -e over legacy -a.",
            fix_with(vec![replace_start(t.id, params, 2, "-e")]),
        );
    }
}

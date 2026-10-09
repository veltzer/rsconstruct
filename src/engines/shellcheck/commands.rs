//! Checks that examine specific commands by name
//! (`ShellCheck.Checks.Commands`).

use std::collections::{BTreeMap, BTreeSet};

use super::analyzerlib::{
    Out, Parameters, err, find_first, get_parents_of_id, info, modifies_variable, style,
    token_is_just_command_output, warn, when_shell,
};
use super::ast::{Id, Inner, Token};
use super::astlib::{
    arguments, brace_expand, concat_oversimplify, e4m, get_all_flags, get_braced_modifier,
    get_braced_reference, get_bsd_opts, get_command_name, get_command_token_or_this, get_gnu_opts,
    get_leading_unquoted_string, get_literal_string, get_literal_string_def,
    get_literal_string_ext, get_trailing_unquoted_literal, get_unquoted_literal, get_word_parts,
    has_flag, is_array_expansion, is_constant, is_flag, is_function_like, is_glob, is_literal,
    is_variable_name, may_become_multiple_args, only_literal_string, will_split,
};
use super::cfg::{CfEffect, CfNode, IdTagged};
use super::data::{DECLARING_COMMANDS, FLAGS_FOR_READ, SAMPLE_WORDS};
use super::hchar;
use super::interface::Shell;
use super::regex::{Regex, mk_regex};

use Inner::{
    T_Annotation, T_Assignment, T_Backticked, T_BatsTest, T_BraceGroup, T_CaseExpression, T_DGREAT,
    T_DollarArithmetic, T_DollarBraceCommandExpansion, T_DollarBraced, T_DollarExpansion,
    T_DoubleQuoted, T_FdRedirect, T_Function, T_Glob, T_Greater, T_IoFile, T_Less, T_Literal,
    T_NormalWord, T_Pipeline, T_Redirecting, T_Script, T_SimpleCommand, T_WhileExpression,
};

type CheckFn = fn(&Parameters<'_>, &Token, &mut Out);

#[derive(Clone, Copy)]
enum CommandName {
    Exactly(&'static str),
    Basename(&'static str),
}

thread_local! {
    static SUSPICIOUS_RE: Regex = mk_regex("([A-Za-z1-9])\\*");
    static CONTRA_RE: Regex = mk_regex("[^a-zA-Z1-9]\\*|[][^$+\\\\]");
    static COMMAND_RE: Regex = mk_regex("[ |;]");
    static HAS_ESCAPES_RE: Regex = mk_regex("\\\\([rntabefv\\']|[0-7]{1,3}|x([0-9]|[A-F]|[a-f]){1,2})");
    static SUBDIR_RE: Regex = mk_regex("^(\\.\\.?\\/)+[^/]+$");
    static PRINTF_RE: Regex =
        mk_regex("^#?-?\\+? ?0?(\\*|\\d*)\\.?(\\d*|\\*)(hh|h|l|ll|q|L|j|z|Z|t)?([diouxXfFeEgGaAcsbqQSC])((\n|.)*)");
    static ALIAS_ARGS_RE: Regex = mk_regex("\\$\\{?[0-9*@]");
}

/// The token and its ancestors (`getPath`), for a token that may be a
/// rewritten copy of a tree token (as for `builtin`).
fn path_of<'x>(params: &'x Parameters<'_>, t: &'x Token) -> Vec<&'x Token> {
    let mut out = vec![t];
    for p in get_parents_of_id(&params.parent_map, t.id) {
        out.push(p);
    }
    out
}

fn closest_command<'x>(params: &'x Parameters<'_>, t: &'x Token) -> Option<&'x Token> {
    let path = path_of(params, t);
    find_first(
        |t: &&Token| match t.inner {
            T_Redirecting(..) => Some(true),
            T_Script(..) => Some(false),
            _ => None,
        },
        &path,
    )
    .copied()
}

fn check_tr(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    for w in arguments(t) {
        if is_glob(w) {
            warn(
                out,
                w.id,
                2060,
                "Quote parameters to tr to prevent glob expansion.",
            );
            continue;
        }
        match get_literal_string(w).as_deref() {
            Some("a-z") => info(
                out,
                w.id,
                2018,
                "Use '[:lower:]' to support accents and foreign alphabets.",
            ),
            Some("A-Z") => info(
                out,
                w.id,
                2019,
                "Use '[:upper:]' to support accents and foreign alphabets.",
            ),
            Some(s) => {
                let relevant: Vec<char> = s.chars().filter(|&c| hchar::is_alpha(c)).collect();
                let mut seen: Vec<char> = Vec::new();
                for c in &relevant {
                    if !seen.contains(c) {
                        seen.push(*c);
                    }
                }
                let duplicated = relevant != seen;
                if !(s.starts_with('-') || s.contains("[:")) && duplicated {
                    info(
                        out,
                        w.id,
                        2020,
                        "tr replaces sets of chars, not words (mentioned due to duplicates).",
                    );
                }
                if !(s.starts_with("[:") || s.starts_with("[="))
                    && s.starts_with('[')
                    && s.ends_with(']')
                    && s.chars().count() > 2
                    && !s.contains('*')
                {
                    info(
                        out,
                        w.id,
                        2021,
                        "Don't use [] around classes in tr, it replaces literal square brackets.",
                    );
                }
            }
            None => {}
        }
    }
}

fn check_find_name_glob(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let accepts_glob = |s: &str| {
        [
            "-ilname",
            "-iname",
            "-ipath",
            "-iregex",
            "-iwholename",
            "-lname",
            "-name",
            "-path",
            "-regex",
            "-wholename",
        ]
        .contains(&s)
    };
    for w in arguments(t).windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if let Some(s) = get_literal_string(a)
            && accepts_glob(&s)
            && is_glob(b)
        {
            warn(
                out,
                b.id,
                2061,
                &format!("Quote the parameter to {s} so the shell won't interpret it."),
            );
        }
    }
}

fn check_expr(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let args = arguments(t);
    let exceptions = [
        ":", "<", ">", "<=", ">=", "match", "length", "substr", "index",
    ];
    if args
        .iter()
        .filter_map(get_literal_string)
        .all(|w| !exceptions.contains(&w.as_str()))
    {
        style(
            out,
            get_command_token_or_this(t).id,
            2003,
            "expr is antiquated. Consider rewriting this using $((..)), ${} or [[ ]].",
        );
    }
    let check_op = |side: &Token, out: &mut Out| {
        let msg = match get_literal_string(side).as_deref() {
            Some("match") => "'expr match' has unspecified results. Prefer 'expr str : regex'.",
            Some("length") => "'expr length' has unspecified results. Prefer ${#var}.",
            Some("substr") => "'expr substr' has unspecified results. Prefer 'cut' or ${var#???}.",
            Some("index") => {
                "'expr index' has unspecified results. Prefer x=${var%%[chars]*}; $((${#x}+1))."
            }
            _ => return,
        };
        info(out, side.id, 2308, msg);
    };
    match args {
        [lhs, op, rhs] => {
            check_op(lhs, out);
            match get_word_parts(op).as_slice() {
                [
                    Token {
                        inner: T_Glob(g), ..
                    },
                ] if g == "*" => err(
                    out,
                    op.id,
                    2304,
                    "* must be escaped to multiply: \\*. Modern $((x * y)) avoids this issue.",
                ),
                [
                    Token {
                        inner: T_Literal(l),
                        ..
                    },
                ] if l == ":" && is_glob(rhs) => {
                    warn(
                        out,
                        rhs.id,
                        2305,
                        "Quote regex argument to expr to avoid it expanding as a glob.",
                    );
                }
                _ => {}
            }
        }
        [single] if !will_split(single) => warn(
            out,
            single.id,
            2307,
            "'expr' expects 3+ arguments but sees 1. Make sure each operator/operand is a separate argument, and escape <>&|.",
        ),
        [first, second]
            if only_literal_string(first) != "length"
                && !(will_split(first) || will_split(second)) =>
        {
            check_op(first, out);
            warn(
                out,
                t.id,
                2307,
                "'expr' expects 3+ arguments, but sees 2. Make sure each operator/operand is a separate argument, and escape <>&|.",
            );
        }
        [first, rest @ ..] => {
            check_op(first, out);
            for x in rest {
                if is_glob(x) {
                    warn(
                        out,
                        x.id,
                        2306,
                        "Escape glob characters in arguments to expr to avoid pathname expansion.",
                    );
                }
            }
        }
        [] => {}
    }
}

fn check_grep_re(_: &Parameters<'_>, cmd: &Token, out: &mut Out) {
    let args = arguments(cmd);
    let skippable = |s: &str| !s.starts_with("--regex=") && s.starts_with('-');
    let mut i = 0;
    let re_index = loop {
        let Some(x) = args.get(i) else {
            return;
        };
        let s = get_literal_string_def("_", x);
        if ["--", "-e", "--regex"].contains(&s.as_str()) {
            break i + 1;
        }
        if skippable(&s) {
            i += 1;
            continue;
        }
        break i;
    };
    let Some(re) = args.get(re_index) else {
        return;
    };
    if is_glob(re) {
        warn(
            out,
            re.id,
            2062,
            "Quote the grep pattern so the shell won't interpret it.",
        );
    }
    let flags: Vec<String> = get_all_flags(cmd).into_iter().map(|(_, f)| f).collect();
    let grep_glob_flags = [
        "fixed-strings",
        "F",
        "include",
        "exclude",
        "exclude-dir",
        "o",
        "only-matching",
    ];
    if flags.iter().any(|f| grep_glob_flags.contains(&f.as_str())) {
        return;
    }
    let string = concat_oversimplify(re);
    if super::analyzerlib::is_confused_glob_regex(&string) {
        warn(
            out,
            re.id,
            2063,
            "Grep uses regex, but this looks like a glob.",
        );
        return;
    }
    let suspicious = SUSPICIOUS_RE.with(|r| r.match_groups(&string));
    if let Some(groups) = suspicious
        && let [g] = groups.as_slice()
        && g.chars().count() == 1
        && !CONTRA_RE.with(|r| r.is_match(&string))
    {
        let c = g.chars().next().expect("one char");
        let candidates: Vec<String> = SAMPLE_WORDS
            .iter()
            .map(|s| (*s).to_string())
            .chain(SAMPLE_WORDS.iter().map(|s| {
                let mut cs = s.chars();
                let first = cs.next().map_or(' ', hchar::to_upper);
                format!("{first}{}", cs.as_str())
            }))
            .collect();
        let word = candidates
            .iter()
            .find(|w| w.starts_with(c))
            .cloned()
            .unwrap_or_else(|| format!("{c}test"));
        info(
            out,
            re.id,
            2022,
            &format!("Note that unlike globs, {c}* here matches '{c}{c}{c}' but not '{word}'."),
        );
    }
}

fn check_trap_quotes(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let Some(x) = arguments(t).first()
        && let T_NormalWord(parts) = &x.inner
        && let [
            Token {
                inner: T_DoubleQuoted(rs),
                ..
            },
        ] = parts.as_slice()
    {
        for r in rs {
            if matches!(
                r.inner,
                T_DollarExpansion(_) | T_Backticked(_) | T_DollarBraced(..) | T_DollarArithmetic(_)
            ) {
                warn(
                    out,
                    r.id,
                    2064,
                    "Use single quotes, otherwise this expands now rather than when signalled.",
                );
            }
        }
    }
}

fn return_or_exit(t: &Token, out: &mut Out, multi: (i64, &str), invalid: (i64, &str)) {
    let lit = |x: &Token| match x.inner {
        T_DollarBraced(..) | T_DollarArithmetic(_) | T_DollarExpansion(_) | T_Backticked(_) => {
            Some("0".to_string())
        }
        _ => Some("WTF".to_string()),
    };
    match arguments(t) {
        [first, _, ..] => err(out, first.id, multi.0, multi.1),
        [value] => {
            let s = get_literal_string_ext(value, &lit).unwrap_or_default();
            let is_invalid = s.is_empty()
                || s.chars().any(|c| !c.is_ascii_digit())
                || s.chars().count() > 5
                || s.parse::<u64>().map_or(true, |v| v > 255);
            if is_invalid {
                err(out, value.id, invalid.0, invalid.1);
            }
        }
        [] => {}
    }
}

fn check_return(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    return_or_exit(
        t,
        out,
        (
            2151,
            "Only one integer 0-255 can be returned. Use stdout for other data.",
        ),
        (
            2152,
            "Can only return 0-255. Other data should be written to stdout.",
        ),
    );
}

fn check_exit(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    return_or_exit(
        t,
        out,
        (
            2241,
            "The exit status can only be one integer 0-255. Use stdout for other data.",
        ),
        (
            2242,
            "Can only exit with status 0-255. Other data should be written to stdout/stderr.",
        ),
    );
}

fn check_find_exec_with_single_argument(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    for w in arguments(t).windows(3) {
        let (exec, arg, term) = (&w[0], &w[1], &w[2]);
        let (Some(exec_s), Some(term_s)) = (get_literal_string(exec), get_literal_string(term))
        else {
            continue;
        };
        let cmd_s = get_literal_string_def(" ", arg);
        if ["-exec", "-execdir"].contains(&exec_s.as_str())
            && [";", "+"].contains(&term_s.as_str())
            && COMMAND_RE.with(|re| re.is_match(&cmd_s))
        {
            warn(
                out,
                exec.id,
                2150,
                "-exec does not invoke a shell. Rewrite or use -exec sh -c .. .",
            );
        }
    }
}

fn check_unused_echo_escapes(params: &Parameters<'_>, cmd: &Token, out: &mut Out) {
    if !when_shell(params, &[Shell::Sh, Shell::Bash, Shell::Ksh]) || has_flag(cmd, "e") {
        return;
    }
    for token in arguments(cmd) {
        if HAS_ESCAPES_RE.with(|re| re.is_match(&only_literal_string(token))) {
            info(
                out,
                token.id,
                2028,
                "echo may not expand escape sequences. Use printf.",
            );
        }
    }
}

fn check_injectable_find_sh(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let id_strings: Vec<(Id, String)> = arguments(t)
        .iter()
        .map(|x| (x.id, only_literal_string(x)))
        .collect();
    let patterns: [&dyn Fn(&str) -> bool; 3] = [
        &|s| s == "-exec" || s == "-execdir",
        &|s| ["sh", "bash", "dash", "ksh"].contains(&s),
        &|s| s == "-c",
    ];
    fn walk(patterns: &[&dyn Fn(&str) -> bool], args: &[(Id, String)], out: &mut Out) {
        let Some((next, rest)) = args.split_first() else {
            return;
        };
        match patterns.split_first() {
            None => {
                let (id, arg) = next;
                if arg.contains("{}") {
                    warn(
                        out,
                        *id,
                        2156,
                        "Injecting filenames is fragile and insecure. Use parameters.",
                    );
                }
            }
            Some((p, tests)) => {
                if p(&next.1) {
                    walk(tests, rest, out);
                }
                walk(patterns, rest, out);
            }
        }
    }
    walk(&patterns, &id_strings, out);
}

fn check_find_action_precedence(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let is_param = |strs: &[&str], t: &Token| {
        get_literal_string(t).is_some_and(|p| strs.contains(&p.as_str()))
    };
    let is_match = |t: &Token| {
        is_param(
            &[
                "-name",
                "-regex",
                "-iname",
                "-iregex",
                "-wholename",
                "-iwholename",
            ],
            t,
        )
    };
    let is_action = |t: &Token| {
        is_param(
            &[
                "-exec", "-execdir", "-delete", "-print", "-print0", "-fls", "-fprint", "-fprint0",
                "-fprintf", "-ls", "-ok", "-okdir", "-printf",
            ],
            t,
        )
    };
    let args = arguments(t);
    let mut i = 0;
    while args.len() - i >= 6 {
        let l = &args[i..];
        if is_match(&l[0]) && is_param(&["-o", "-or"], &l[2]) && is_match(&l[3]) && is_action(&l[5])
        {
            warn(
                out,
                l[5].id,
                2146,
                "This action ignores everything before the -o. Use \\( \\) to group.",
            );
            return;
        }
        i += 1;
    }
}

fn check_mkdir_dash_pm(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let flags = get_all_flags(t);
    if !flags.iter().any(|(_, f)| f == "p" || f == "parents") {
        return;
    }
    let Some((dash_m, _)) = flags.iter().find(|(_, f)| f == "m" || f == "mode") else {
        return;
    };
    let could_have_subdirs = |t: &Token| match get_literal_string(t) {
        Some(name) => name.contains('/') && !SUBDIR_RE.with(|re| re.is_match(&name)),
        None => true,
    };
    if arguments(t).iter().skip(1).any(could_have_subdirs) {
        warn(
            out,
            dash_m.id,
            2174,
            "When used with -p, -m only applies to the deepest directory.",
        );
    }
}

fn check_nonportable_signals(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let args = arguments(t);
    let Some((first, rest)) = args.split_first() else {
        return;
    };
    if is_flag(first) {
        return;
    }
    for param in rest {
        let Some(s) = get_literal_string(param) else {
            continue;
        };
        if !s.is_empty()
            && s.chars().all(|c| c.is_ascii_digit())
            && s != "0"
            && !["1", "2", "3", "6", "9", "14", "15"].contains(&s.as_str())
        {
            warn(
                out,
                param.id,
                2172,
                "Trapping signals by number is not well defined. Prefer signal names.",
            );
        }
        if ["kill", "9", "sigkill", "stop", "sigstop"].contains(&hchar::lower_string(&s).as_str()) {
            err(out, param.id, 2173, "SIGKILL/SIGSTOP can not be trapped.");
        }
    }
}

fn check_interactive_su(params: &Parameters<'_>, cmd: &Token, out: &mut Out) {
    if arguments(cmd).len() > 1 {
        return;
    }
    let undirected = |t: &&Token| match &t.inner {
        T_Pipeline(_, cmds) => cmds.len() < 2,
        T_Redirecting(redirs, _) => redirs.is_empty(),
        _ => true,
    };
    if path_of(params, cmd).iter().all(undirected) {
        info(
            out,
            cmd.id,
            2117,
            "To run commands as another user, use su -c or sudo.",
        );
    }
}

fn check_ssh_command_string(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let is_option = |x: &Token| concat_oversimplify(x).starts_with('-');
    let (options, rest): (Vec<&Token>, Vec<&Token>) =
        arguments(t).iter().partition(|x| is_option(x));
    if !options.is_empty() || rest.len() < 2 {
        return;
    }
    let last = rest[rest.len() - 1];
    if let T_NormalWord(parts) = &last.inner
        && let [
            Token {
                inner: T_DoubleQuoted(inner),
                ..
            },
        ] = parts.as_slice()
        && let Some(x) = inner.iter().find(|x| !is_constant(x))
    {
        info(
            out,
            x.id,
            2029,
            "Note that, unescaped, this expands on the client side.",
        );
    }
}

/// `getPrintfFormats`.
pub fn get_printf_formats(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '%' && cs.get(i + 1) == Some(&'%') {
            i += 2;
            continue;
        }
        if cs[i] == '%' && cs.get(i + 1) == Some(&'(') {
            let rest = &cs[i + 2..];
            match rest.iter().position(|&c| c == ')') {
                Some(p) if p + 1 < rest.len() => {
                    out.push(rest[p + 1]);
                    i = i + 2 + p + 2;
                    continue;
                }
                _ => return out,
            }
        }
        if cs[i] == '%' {
            let rest: String = cs[i + 1..].iter().collect();
            match PRINTF_RE.with(|re| re.match_groups(&rest)) {
                Some(g) if g.len() == 6 => {
                    if g[0] == "*" {
                        out.push('*');
                    }
                    if g[1] == "*" {
                        out.push('*');
                    }
                    out.push_str(&g[3]);
                    out.push_str(&get_printf_formats(&g[4]));
                    return out;
                }
                _ => {
                    // `take 1 rest ++ getFormats rest`: the scan resumes at the
                    // character after the %, which is also emitted.
                    if let Some(c) = cs.get(i + 1) {
                        out.push(*c);
                    }
                    out.push_str(&get_printf_formats(&cs[i + 1..].iter().collect::<String>()));
                    return out;
                }
            }
        }
        i += 1;
    }
    out
}

fn check_printf_var(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let mut args = arguments(t);
    loop {
        match args {
            [d, rest @ ..] if get_literal_string(d).as_deref() == Some("--") => args = rest,
            [v, _, rest @ ..] if get_literal_string(v).as_deref() == Some("-v") => args = rest,
            _ => break,
        }
    }
    let Some((format, more)) = args.split_first() else {
        return;
    };
    if let Some(string) = get_literal_string(format) {
        let formats = get_printf_formats(&string);
        let format_count = formats.chars().count();
        let arg_count = more.len();
        let pluralise = |word: &str, n: usize| {
            if n > 1 {
                format!("{word}s")
            } else {
                word.to_string()
            }
        };
        let only_trailing_ts = formats.chars().skip(arg_count).all(|c| c == 'T');
        if format_count == 0 && arg_count > 0 {
            err(
                out,
                format.id,
                2182,
                "This printf format string has no variables. Other arguments are ignored.",
            );
        } else if !(format_count == 0
            || more.iter().any(may_become_multiple_args)
            || (arg_count < format_count && only_trailing_ts)
            || (arg_count > 0 && arg_count % format_count == 0))
        {
            warn(
                out,
                format.id,
                2183,
                &format!(
                    "This format string has {format_count} {}, but is passed {arg_count}{}.",
                    pluralise("variable", format_count),
                    pluralise(" argument", arg_count)
                ),
            );
        }
    }
    if !(concat_oversimplify(format).contains('%') || is_literal(format)) {
        info(
            out,
            format.id,
            2059,
            "Don't use variables in the printf format string. Use printf '..%s..' \"$foo\".",
        );
    }
}

fn check_uuoe_cmd(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let [token] = arguments(t)
        && token_is_just_command_output(token)
    {
        style(
            out,
            token.id,
            2005,
            "Useless echo? Instead of 'echo $(cmd)', just use 'cmd'.",
        );
    }
}

fn check_set_assignment(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    fn literal(t: &Token) -> String {
        match &t.inner {
            T_NormalWord(l) => l.iter().map(literal).collect(),
            T_Literal(s) => s.clone(),
            _ => "*".to_string(),
        }
    }
    if let [var, rest @ ..] = arguments(t) {
        let s = literal(var);
        if (!rest.is_empty() && is_variable_name(&s)) || s.contains('=') {
            warn(
                out,
                var.id,
                2121,
                "To assign a variable, use just 'var=value', no 'set ..'.",
            );
        }
    }
}

fn get_single_unmodified_braced_string(word: &Token) -> Option<String> {
    match get_word_parts(word).as_slice() {
        [
            Token {
                inner: T_DollarBraced(_, l),
                ..
            },
        ] => {
            let contents = concat_oversimplify(l);
            if contents == get_braced_reference(&contents) {
                Some(contents)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn check_exported_expansions(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    for a in arguments(t) {
        if let Some(name) = get_single_unmodified_braced_string(a) {
            warn(
                out,
                a.id,
                2163,
                &format!(
                    "This does not export '{name}'. Remove $/${{}} for that, or use ${{var?}} to quiet."
                ),
            );
        }
    }
}

fn check_read_expansions(_: &Parameters<'_>, cmd: &Token, out: &mut Out) {
    let args = arguments(cmd);
    let vars: Vec<&Token> = get_gnu_opts(FLAGS_FOR_READ, args)
        .map(|opts| {
            opts.iter()
                .filter(|(x, _)| x.is_empty() || x == "a")
                .map(|(_, (_, y))| *y)
                .collect()
        })
        .unwrap_or_default();
    for t in vars {
        if let Some(name) = get_single_unmodified_braced_string(t)
            && is_variable_name(&name)
        {
            warn(
                out,
                t.id,
                2229,
                &format!(
                    "This does not read '{name}'. Remove $/${{}} for that, or use ${{var?}} to quiet."
                ),
            );
        }
    }
    for word in args {
        if get_word_parts(word)
            .iter()
            .any(|t| matches!(&t.inner, T_Glob(s) if s.starts_with('[')))
        {
            warn(
                out,
                word.id,
                2313,
                "Quote array indices to avoid them expanding as globs.",
            );
        }
    }
}

fn check_aliases_uses_args(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    for arg in arguments(t) {
        let s = get_literal_string_def("_", arg);
        if s.contains('=') && ALIAS_ARGS_RE.with(|re| re.is_match(&s)) {
            err(
                out,
                arg.id,
                2142,
                "Aliases can't use positional parameters. Use a function.",
            );
        }
    }
}

fn check_aliases_expand_early(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    for arg in arguments(t) {
        if concat_oversimplify(arg).contains('=')
            && let Some(x) = get_word_parts(arg).into_iter().find(|x| !is_literal(x))
        {
            warn(
                out,
                x.id,
                2139,
                "This expands when defined, not when used. Consider escaping.",
            );
        }
    }
}

fn check_unset_globs(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    for arg in arguments(t) {
        if is_glob(arg) {
            warn(
                out,
                arg.id,
                2184,
                "Quote arguments to unset so they're not glob expanded.",
            );
        }
    }
}

fn check_find_without_path(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_SimpleCommand(_, words) = &t.inner else {
        return;
    };
    let Some((cmd, args)) = words.split_first() else {
        return;
    };
    let is_leading_flag = |flag: &str| {
        flag.chars().count() <= 2 || flag.chars().all(|c| "-EHLPXdfsxO0123456789".contains(c))
    };
    fn has_path(args: &[Token], is_leading_flag: &dyn Fn(&str) -> bool) -> bool {
        match args.split_first() {
            None => false,
            Some((first, rest)) => {
                let flag = get_literal_string_def("___", first);
                !flag.starts_with('-')
                    || (is_leading_flag(&flag) && has_path(rest, is_leading_flag))
            }
        }
    }
    if !(has_flag(t, "help") || has_path(args, &is_leading_flag)) {
        info(
            out,
            cmd.id,
            2185,
            "Some finds don't have a default path. Specify '.' explicitly.",
        );
    }
}

fn check_time_parameters(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_SimpleCommand(_, words) = &t.inner
        && let [cmd, args, ..] = words.as_slice()
        && when_shell(params, &[Shell::Bash, Shell::Sh])
    {
        let s = concat_oversimplify(args);
        if s.starts_with('-') && s != "-p" {
            info(
                out,
                cmd.id,
                2023,
                "The shell may override 'time' as seen in man time(1). Use 'command time ..' for that one.",
            );
        }
    }
}

fn check_timed_command(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_SimpleCommand(_, words) = &t.inner else {
        return;
    };
    let [c, args @ ..] = words.as_slice() else {
        return;
    };
    if args.is_empty() || !when_shell(params, &[Shell::Sh, Shell::Dash, Shell::BusyboxSh]) {
        return;
    }
    let cmd = &args[args.len() - 1];
    if matches!(&cmd.inner, T_Pipeline(_, l) if l.len() >= 2) {
        warn(
            out,
            c.id,
            2176,
            "'time' is undefined for pipelines. time single stage or bash -c instead.",
        );
    }
    let is_simple = match &cmd.inner {
        T_Pipeline(_, l) => match l.first().map(|x| &x.inner) {
            Some(T_Redirecting(_, a)) => Some(matches!(a.inner, T_SimpleCommand(..))),
            _ => None,
        },
        _ => None,
    };
    if is_simple == Some(false) {
        warn(
            out,
            cmd.id,
            2177,
            "'time' is undefined for compound commands, time sh -c instead.",
        );
    }
}

fn check_local_scope(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if !when_shell(params, &[Shell::Bash, Shell::Dash, Shell::BusyboxSh]) {
        return;
    }
    if !path_of(params, t).iter().any(|x| is_function_like(x)) {
        err(
            out,
            get_command_token_or_this(t).id,
            2168,
            "'local' is only valid in functions.",
        );
    }
}

fn check_multiple_declaring(cmd: &str, t: &Token, out: &mut Out) {
    for a in arguments(t) {
        if let Some(lit) = get_unquoted_literal(a)
            && DECLARING_COMMANDS.contains(&lit.as_str())
        {
            err(
                out,
                get_command_token_or_this(a).id,
                2316,
                &format!(
                    "This applies {cmd} to the variable named {lit}, which is probably not what you want. Use a separate command or the appropriate `declare` options instead."
                ),
            );
        }
    }
}

fn check_deprecated_tempfile(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    warn(
        out,
        get_command_token_or_this(t).id,
        2186,
        "tempfile is deprecated. Use mktemp instead.",
    );
}

fn check_deprecated_egrep(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    info(
        out,
        get_command_token_or_this(t).id,
        2196,
        "egrep is non-standard and deprecated. Use grep -E instead.",
    );
}

fn check_deprecated_fgrep(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    info(
        out,
        get_command_token_or_this(t).id,
        2197,
        "fgrep is non-standard and deprecated. Use grep -F instead.",
    );
}

fn check_while_getopts_case(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_SimpleCommand(_, words) = &t.inner else {
        return;
    };
    let [_, arg1, name, ..] = words.as_slice() else {
        return;
    };
    let path = path_of(params, t);
    let Some(options) = get_literal_string(arg1) else {
        return;
    };
    let Some(getopts_var) = get_literal_string(name) else {
        return;
    };
    let Some(w) = find_first(
        |x: &&Token| match x.inner {
            T_WhileExpression(..) => Some(true),
            T_Script(..) => Some(false),
            _ => None,
        },
        &path,
    ) else {
        return;
    };
    let T_WhileExpression(_, body) = &w.inner else {
        return;
    };
    fn find_case(t: &Token) -> Option<&Token> {
        match &t.inner {
            T_Annotation(_, x) => find_case(x),
            T_Pipeline(_, l) if l.len() == 1 => find_case(&l[0]),
            T_Redirecting(_, x) if matches!(x.inner, T_CaseExpression(..)) => Some(x),
            _ => None,
        }
    }
    let Some(case) = body.iter().find_map(find_case) else {
        return;
    };
    let T_CaseExpression(var, list) = &case.inner else {
        return;
    };
    let [
        Token {
            inner: T_DollarBraced(_, braced_word),
            ..
        },
    ] = get_word_parts(var).as_slice()
    else {
        return;
    };
    let [
        Token {
            inner: T_Literal(case_var),
            ..
        },
    ] = get_word_parts(braced_word).as_slice()
    else {
        return;
    };
    if *case_var != getopts_var {
        return;
    }
    let group = Token::new(Id(0), T_BraceGroup(body.clone()));
    if modifies_variable(params, &group, &getopts_var) {
        return;
    }
    let opts: Vec<String> = options
        .chars()
        .filter(|&c| c != ':')
        .map(|c| c.to_string())
        .collect();
    let from_glob = |t: &Token| -> Option<String> {
        match &t.inner {
            T_Glob(s) => {
                let cs: Vec<char> = s.chars().collect();
                match cs.as_slice() {
                    ['[', c, ']'] => Some(c.to_string()),
                    ['*'] => Some("*".to_string()),
                    ['?'] => Some("?".to_string()),
                    _ => None,
                }
            }
            _ => None,
        }
    };
    let literal = |t: &Token| -> Option<String> {
        match (get_literal_string(t), from_glob(t)) {
            (Some(a), Some(b)) => Some(format!("{a}{b}")),
            (Some(a), None) => Some(a),
            (None, b) => b,
        }
    };
    let mut handled_map: BTreeMap<Option<String>, &Token> = BTreeMap::new();
    for (_, globs, _) in list {
        for g in globs {
            handled_map.insert(literal(g), g);
        }
    }
    let requested: BTreeSet<Option<String>> = opts.iter().map(|x| Some(x.clone())).collect();
    let case_id = case.id;
    if !handled_map.contains_key(&None) {
        for s in requested
            .iter()
            .filter(|k| !handled_map.contains_key(*k))
            .flatten()
        {
            warn(
                out,
                case_id,
                2213,
                &format!(
                    "getopts specified -{}, but it's not handled by this 'case'.",
                    e4m(s)
                ),
            );
        }
        if !handled_map.contains_key(&Some("*".to_string()))
            && !handled_map.contains_key(&Some("?".to_string()))
        {
            warn(
                out,
                case_id,
                2220,
                "Invalid flags are not handled. Add a *) case.",
            );
        }
    }
    for (k, expr) in &handled_map {
        if requested.contains(k) {
            continue;
        }
        if let Some(s) = k
            && !["*", ":", "?"].contains(&s.as_str())
        {
            warn(out, expr.id, 2214, "This case is not specified by getopts.");
        }
    }
}

fn check_catastrophic_rm(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let is_recursive = get_all_flags(t)
        .iter()
        .any(|(_, f)| ["r", "R", "recursive"].contains(&f.as_str()));
    if !is_recursive {
        return;
    }
    let paths = [
        "",
        "/bin",
        "/etc",
        "/home",
        "/mnt",
        "/usr",
        "/usr/share",
        "/usr/local",
        "/var",
        "/lib",
        "/dev",
        "/media",
        "/boot",
        "/lib64",
        "/usr/bin",
    ];
    let mut important: Vec<String> = Vec::new();
    for x in ["", "/", "/*", "/*/*"] {
        for p in &paths {
            let s = format!("{p}{x}");
            if !s.is_empty() {
                important.push(s);
            }
        }
    }
    let skip_repeating = |c: char, s: &str| -> String {
        let mut out: Vec<char> = Vec::new();
        for a in s.chars().rev() {
            if a == c && out.last() == Some(&c) {
                out.pop();
            }
            out.push(a);
        }
        out.into_iter().rev().collect()
    };
    let fix_path = |filename: &str| -> String {
        let normalized = skip_repeating('/', &skip_repeating('*', filename));
        if normalized == "/" {
            normalized
        } else {
            normalized.trim_end_matches('/').to_string()
        }
    };
    for arg in arguments(t) {
        for token in brace_expand(arg) {
            if let Some(s) = get_literal_string(&token) {
                if important.contains(&fix_path(&s)) {
                    warn(out, token.id, 2114, "Warning: deletes a system directory.");
                }
            } else {
                let potential = get_literal_string_ext(&token, &|x| match &x.inner {
                    T_Glob(s) => Some(s.clone()),
                    T_DollarBraced(_, word) => {
                        let var = only_literal_string(word);
                        if [":?", ":-", ":="].iter().any(|m| var.contains(m)) {
                            None
                        } else {
                            Some(String::new())
                        }
                    }
                    _ => Some(String::new()),
                });
                if let Some(filename) = potential {
                    let path = fix_path(&filename);
                    if important.contains(&path) {
                        warn(
                            out,
                            token.id,
                            2115,
                            &format!("Use \"${{var:?}}\" to ensure this never expands to {path} ."),
                        );
                    }
                }
            }
        }
    }
}

fn check_let_usage(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if when_shell(params, &[Shell::Bash, Shell::Ksh]) {
        style(
            out,
            t.id,
            2219,
            "Instead of 'let expr', prefer (( expr )) .",
        );
    }
}

fn missing_destination(t: &Token) -> bool {
    let args = get_all_flags(t);
    let params: Vec<&Token> = args
        .iter()
        .filter(|(_, f)| f.is_empty())
        .map(|(x, _)| *x)
        .collect();
    let has_target = args
        .iter()
        .any(|(_, x)| !x.is_empty() && "target-directory".starts_with(x.as_str()));
    matches!(params.as_slice(), [single] if !(has_target || may_become_multiple_args(single)))
}

fn check_mv_arguments(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if missing_destination(t) {
        err(
            out,
            t.id,
            2224,
            "This mv has no destination. Check the arguments.",
        );
    }
}

fn check_cp_arguments(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if missing_destination(t) {
        err(
            out,
            t.id,
            2225,
            "This cp has no destination. Check the arguments.",
        );
    }
}

fn check_ln_arguments(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if missing_destination(t) {
        warn(
            out,
            t.id,
            2226,
            "This ln has no destination. Check the arguments, or specify '.' explicitly.",
        );
    }
}

fn check_find_redirections(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let Some(r) = closest_command(params, t) else {
        return;
    };
    if let T_Redirecting(redirs, cmd) = &r.inner
        && !redirs.is_empty()
        && let T_SimpleCommand(_, args) = &cmd.inner
        && args.len() >= 2
    {
        let min_redir = redirs.iter().map(|x| x.id).min().expect("non-empty");
        let max_arg = args.iter().map(|x| x.id).max().expect("non-empty");
        if min_redir < max_arg {
            warn(
                out,
                min_redir,
                2227,
                "Redirection applies to the find command itself. Rewrite to work per action (or move to end).",
            );
        }
    }
}

fn check_which(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    info(
        out,
        get_command_token_or_this(t).id,
        2230,
        "'which' is non-standard. Use builtin 'command -v' instead.",
    );
}

fn check_sudo_redirect(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let Some(r) = closest_command(params, t) else {
        panic!("ShellCheck: checkSudoRedirect without a command");
    };
    let T_Redirecting(redirs, _) = &r.inner else {
        return;
    };
    for redir in redirs {
        if let T_FdRedirect(s, x) = &redir.inner
            && let T_IoFile(op, file) = &x.inner
            && (s.is_empty() || s == "&")
            && concat_oversimplify(file) != "/dev/null"
        {
            match op.inner {
                T_Less => info(
                    out,
                    op.id,
                    2024,
                    "sudo doesn't affect redirects. Use sudo cat file | ..",
                ),
                T_Greater => warn(
                    out,
                    op.id,
                    2024,
                    "sudo doesn't affect redirects. Use ..| sudo tee file",
                ),
                T_DGREAT => warn(
                    out,
                    op.id,
                    2024,
                    "sudo doesn't affect redirects. Use .. | sudo tee -a file",
                ),
                _ => {}
            }
        }
    }
}

fn check_sudo_args(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let builtins = [
        "cd", "command", "declare", "eval", "exec", "exit", "export", "hash", "history", "local",
        "popd", "pushd", "read", "readonly", "return", "set", "source", "trap", "type", "typeset",
        "ulimit", "umask", "unset", "wait",
    ];
    let Some(opts) = get_bsd_opts("vAknSbEHPa:g:h:p:u:c:T:r:", arguments(t)) else {
        return;
    };
    let Some((_, (command_arg, _))) = opts.iter().find(|(f, _)| f.is_empty()) else {
        return;
    };
    let Some(command) = get_literal_string(command_arg) else {
        return;
    };
    if builtins.contains(&command.as_str()) {
        warn(
            out,
            t.id,
            2232,
            &format!(
                "Can't use sudo with builtins like {command}. Did you want sudo sh -c .. instead?"
            ),
        );
    }
}

fn check_source_args(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    if when_shell(params, &[Shell::Sh, Shell::Dash])
        && let [_, arg1, ..] = arguments(t)
    {
        warn(
            out,
            arg1.id,
            2240,
            "The dot command does not support arguments in sh/dash. Set them as variables.",
        );
    }
}

fn check_chmod_dashr(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    for a in arguments(t) {
        if get_literal_string(a).as_deref() == Some("-r") {
            warn(
                out,
                a.id,
                2253,
                "Use -R to recurse, or explicitly a-r to remove read permissions.",
            );
        }
    }
}

fn check_xargs_dashi(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let Some(opts) = get_bsd_opts("0oprtxadR:S:J:L:l:n:P:s:e:E:i:I:", arguments(t))
        && let Some((_, (option, _))) = opts.iter().find(|(f, _)| f == "i")
    {
        info(
            out,
            option.id,
            2267,
            "GNU xargs -i is deprecated in favor of -I{}",
        );
    }
}

fn check_arg_comparison(cmd: &str, t: &Token, out: &mut Out) {
    let head_id = |t: &Token| match &t.inner {
        T_NormalWord(l) if !l.is_empty() => l[0].id,
        _ => t.id,
    };
    for arg in arguments(t) {
        if let Some(s) = get_leading_unquoted_string(arg) {
            if s.starts_with('=') {
                err(out, head_id(arg), 2290, "Remove spaces around = to assign.");
            } else if s.starts_with("+=") {
                err(
                    out,
                    head_id(arg),
                    2290,
                    "Remove spaces around += to append.",
                );
            }
        }
        if cmd == "let"
            && let Some(token) = get_trailing_unquoted_literal(arg)
            && let Some(s) = get_literal_string(token)
            && s.ends_with('=')
        {
            err(out, token.id, 2290, "Remove spaces around = to assign.");
        }
    }
}

fn check_masked_returns(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let path = path_of(params, t);
    let Some(name) = get_command_name(t) else {
        return;
    };
    let flags: Vec<String> = get_all_flags(t).into_iter().map(|(_, f)| f).collect();
    let has_r = flags.iter().any(|f| f == "r");
    let has_g = flags.iter().any(|f| f == "g");
    let is_scoped_function = |x: &&Token| match &x.inner {
        T_BatsTest(..) => true,
        T_Function(has_function, _, _, _) => params.shell_type != Shell::Ksh || *has_function,
        _ => false,
    };
    let is_in_scoped_function = path.iter().any(is_scoped_function);
    let is_local =
        !has_g && ["local", "declare", "typeset"].contains(&name.as_str()) && is_in_scoped_function;
    let is_read_only = name == "readonly" || has_r;
    if is_local && is_read_only {
        return;
    }
    for a in arguments(t) {
        if let T_Assignment(_, _, _, word) = &a.inner
            && get_word_parts(word).iter().any(|x| {
                matches!(
                    x.inner,
                    T_Backticked(_) | T_DollarExpansion(_) | T_DollarBraceCommandExpansion(..)
                )
            })
        {
            warn(
                out,
                a.id,
                2155,
                "Declare and assign separately to avoid masking return values.",
            );
        }
    }
}

fn check_unquoted_echo_spaces(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let args = arguments(t);
    let m = params.token_positions;
    let Some(r) = closest_command(params, t) else {
        return;
    };
    let positions: Vec<&(super::interface::Position, super::interface::Position)> =
        args.iter().filter_map(|c| m.get(&c.id)).collect();
    let T_Redirecting(redir_tokens, _) = &r.inner else {
        return;
    };
    let redir_positions: Vec<&super::interface::Position> = redir_tokens
        .iter()
        .filter_map(|c| m.get(&c.id).map(|p| &p.0))
        .collect();
    let has_spaces_between = |(a, b): &(super::interface::Position, super::interface::Position),
                              (c, d): &(super::interface::Position, super::interface::Position)|
     -> bool {
        a.line == d.line
            && c.column - b.column >= 4
            && !redir_positions.iter().any(|x| b < *x && *x < c)
    };
    if positions.windows(2).any(|w| has_spaces_between(w[0], w[1])) {
        info(
            out,
            t.id,
            2291,
            "Quote repeated spaces to avoid them collapsing into one.",
        );
    }
}

fn check_eval_array(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    for a in arguments(t) {
        for p in get_word_parts(a) {
            if is_array_expansion(p) {
                let is_escaped = match &p.inner {
                    T_DollarBraced(_, l) => {
                        get_braced_modifier(&concat_oversimplify(l)).contains('Q')
                    }
                    _ => false,
                };
                if is_escaped {
                    style(
                        out,
                        p.id,
                        2293,
                        "When eval'ing @Q-quoted words, use * rather than @ as the index.",
                    );
                } else {
                    warn(
                        out,
                        p.id,
                        2294,
                        "eval negates the benefit of arrays. Drop eval to preserve whitespace/symbols (or eval as string).",
                    );
                }
            }
        }
    }
}

fn check_backreferencing_declaration(cmd: &str, params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let Some(cfga) = params.cfg_analysis.as_ref() else {
        return;
    };
    let find_references = |list: &[&Token]| -> BTreeSet<String> {
        let mut nodes: BTreeSet<usize> = BTreeSet::new();
        for x in list {
            if let Some(s) = cfga.token_to_nodes.get(&x.id) {
                nodes.extend(s.iter().copied());
            }
        }
        let mut refs = BTreeSet::new();
        for n in nodes {
            if let Some(CfNode::CFApplyEffects(effects)) = cfga.graph.nodes.get(&n) {
                for IdTagged(_, e) in effects {
                    if let CfEffect::CFReadVariable(name) = e {
                        refs.insert(name.clone());
                    }
                }
            }
        }
        refs
    };
    let mut left_args: BTreeMap<String, Id> = BTreeMap::new();
    for a in arguments(t) {
        let list: Vec<&Token> = match &a.inner {
            T_Assignment(_, _, idx, value) => {
                let mut l = vec![&**value];
                l.extend(idx.iter());
                l
            }
            _ => vec![a],
        };
        let references = find_references(&list);
        for (name, id) in &left_args {
            if references.contains(name) {
                warn(
                    out,
                    *id,
                    2318,
                    &format!(
                        "This assignment is used again in this '{cmd}', but won't have taken effect. Use two '{cmd}'s."
                    ),
                );
            }
        }
        if let T_Assignment(_, name, _, _) = &a.inner {
            left_args.insert(name.clone(), a.id);
        }
    }
}

/// The command checks: (name, check).
fn command_checks() -> Vec<(CommandName, CheckFn)> {
    use CommandName::{Basename, Exactly};
    let mut v: Vec<(CommandName, CheckFn)> = vec![
        (Basename("tr"), check_tr),
        (Basename("find"), check_find_name_glob),
        (Basename("expr"), check_expr),
        (Basename("grep"), check_grep_re),
        (Exactly("trap"), check_trap_quotes),
        (Exactly("return"), check_return),
        (Exactly("exit"), check_exit),
        (Basename("find"), check_find_exec_with_single_argument),
        (Basename("echo"), check_unused_echo_escapes),
        (Basename("find"), check_injectable_find_sh),
        (Basename("find"), check_find_action_precedence),
        (Basename("mkdir"), check_mkdir_dash_pm),
        (Exactly("trap"), check_nonportable_signals),
        (Basename("su"), check_interactive_su),
        (Basename("ssh"), check_ssh_command_string),
        (Exactly("printf"), check_printf_var),
        (Exactly("echo"), check_uuoe_cmd),
        (Exactly("set"), check_set_assignment),
        (Exactly("export"), check_exported_expansions),
        (Exactly("alias"), check_aliases_uses_args),
        (Exactly("alias"), check_aliases_expand_early),
        (Exactly("unset"), check_unset_globs),
        (Basename("find"), check_find_without_path),
        (Exactly("time"), check_time_parameters),
        (Exactly("time"), check_timed_command),
        (Exactly("local"), check_local_scope),
        (Basename("tempfile"), check_deprecated_tempfile),
        (Basename("egrep"), check_deprecated_egrep),
        (Basename("fgrep"), check_deprecated_fgrep),
        (Exactly("getopts"), check_while_getopts_case),
        (Basename("rm"), check_catastrophic_rm),
        (Exactly("let"), check_let_usage),
        (Basename("mv"), check_mv_arguments),
        (Basename("cp"), check_cp_arguments),
        (Basename("ln"), check_ln_arguments),
        (Basename("find"), check_find_redirections),
        (Exactly("read"), check_read_expansions),
        (Basename("sudo"), check_sudo_redirect),
        (Basename("sudo"), check_sudo_args),
        (Exactly("."), check_source_args),
        (Basename("chmod"), check_chmod_dashr),
        (Basename("xargs"), check_xargs_dashi),
        (Basename("echo"), check_unquoted_echo_spaces),
        (Exactly("eval"), check_eval_array),
    ];
    // map checkArgComparison ("alias" : declaringCommands)
    v.push((Exactly("alias"), |_, t, o| {
        check_arg_comparison("alias", t, o);
    }));
    v.push((Exactly("local"), |_, t, o| {
        check_arg_comparison("local", t, o);
    }));
    v.push((Exactly("declare"), |_, t, o| {
        check_arg_comparison("declare", t, o);
    }));
    v.push((Exactly("export"), |_, t, o| {
        check_arg_comparison("export", t, o);
    }));
    v.push((Exactly("readonly"), |_, t, o| {
        check_arg_comparison("readonly", t, o);
    }));
    v.push((Exactly("typeset"), |_, t, o| {
        check_arg_comparison("typeset", t, o);
    }));
    v.push((Exactly("let"), |_, t, o| check_arg_comparison("let", t, o)));
    for name in ["local", "declare", "export", "readonly", "typeset", "let"] {
        v.push((Exactly(name), check_masked_returns));
    }
    v.push((Exactly("local"), |_, t, o| {
        check_multiple_declaring("local", t, o);
    }));
    v.push((Exactly("declare"), |_, t, o| {
        check_multiple_declaring("declare", t, o);
    }));
    v.push((Exactly("export"), |_, t, o| {
        check_multiple_declaring("export", t, o);
    }));
    v.push((Exactly("readonly"), |_, t, o| {
        check_multiple_declaring("readonly", t, o);
    }));
    v.push((Exactly("typeset"), |_, t, o| {
        check_multiple_declaring("typeset", t, o);
    }));
    v.push((Exactly("let"), |_, t, o| {
        check_multiple_declaring("let", t, o);
    }));
    v.push((Exactly("local"), |p, t, o| {
        check_backreferencing_declaration("local", p, t, o);
    }));
    v.push((Exactly("declare"), |p, t, o| {
        check_backreferencing_declaration("declare", p, t, o);
    }));
    v.push((Exactly("export"), |p, t, o| {
        check_backreferencing_declaration("export", p, t, o);
    }));
    v.push((Exactly("readonly"), |p, t, o| {
        check_backreferencing_declaration("readonly", p, t, o);
    }));
    v.push((Exactly("typeset"), |p, t, o| {
        check_backreferencing_declaration("typeset", p, t, o);
    }));
    v.push((Exactly("let"), |p, t, o| {
        check_backreferencing_declaration("let", p, t, o);
    }));
    v
}

/// `optionalCommandChecks`.
const OPTIONAL_COMMAND_CHECKS: &[(&str, CommandName, CheckFn)] = &[(
    "deprecate-which",
    CommandName::Basename("which"),
    check_which,
)];

/// The command checks enabled for this analysis.
pub struct Commands {
    checks: Vec<(CommandName, CheckFn)>,
}

impl Commands {
    pub fn new(optional_keys: &[String]) -> Self {
        let mut checks = command_checks();
        if optional_keys.iter().any(|k| k == "all") {
            checks.extend(OPTIONAL_COMMAND_CHECKS.iter().map(|(_, n, f)| (*n, *f)));
        } else {
            for k in optional_keys {
                if let Some((_, n, f)) = OPTIONAL_COMMAND_CHECKS
                    .iter()
                    .find(|(name, _, _)| name == k)
                {
                    checks.push((*n, *f));
                }
            }
        }
        Self { checks }
    }

    fn run_named(
        &self,
        exact: bool,
        name: &str,
        params: &Parameters<'_>,
        t: &Token,
        out: &mut Out,
    ) {
        for (n, f) in &self.checks {
            let hit = match n {
                CommandName::Exactly(x) => exact && *x == name,
                CommandName::Basename(x) => !exact && *x == name,
            };
            if hit {
                f(params, t, out);
            }
        }
    }

    /// `checkCommand`: the per-token check.
    pub fn check(&self, params: &Parameters<'_>, t: &Token, out: &mut Out) {
        let T_SimpleCommand(cmd_prefix, words) = &t.inner else {
            return;
        };
        let Some((cmd, rest)) = words.split_first() else {
            return;
        };
        let Some(name) = get_literal_string(cmd) else {
            return;
        };
        if name.contains('/') {
            let base = name.rsplit('/').next().unwrap_or(&name).to_string();
            self.run_named(false, &base, params, t, out);
        } else if name == "builtin" && !rest.is_empty() {
            let rewritten = Token::new(t.id, T_SimpleCommand(cmd_prefix.clone(), rest.to_vec()));
            let selected = only_literal_string(&rest[0]);
            self.run_named(true, &selected, params, &rewritten, out);
        } else {
            self.run_named(true, &name, params, t, out);
            self.run_named(false, &name, params, t, out);
        }
    }
}

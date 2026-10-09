//! Checks for features a shell dialect lacks
//! (`ShellCheck.Checks.ShellSupport`).
//!
//! `bashism` is one Haskell function of many equations, tried in order;
//! the first equation whose pattern and guard match is the one that runs,
//! even when its body then does nothing. The Rust version keeps that order
//! and returns after the first matching equation.

use super::analyzerlib::{
    Out, Parameters, StackData, err, find_first, get_parents_of_id, info, is_command,
    is_unqualified_command, style, warn,
};
use super::ast::{AssignmentMode, ConditionType, Id, Inner, Token};
use super::astlib::{
    concat_oversimplify, get_braced_modifier, get_braced_reference, get_command_name,
    get_leading_flags, get_literal_string, get_literal_string_ext, is_flag, is_glob,
    is_only_redirection, is_variable_name, only_literal_string, oversimplify,
};
use super::hchar;
use super::interface::Shell;
use super::regex::{Regex, mk_regex};

use Inner::{
    T_Arithmetic, T_Array, T_Assignment, T_Backticked, T_Banged, T_BraceExpansion, T_CoProc,
    T_Condition, T_DollarArithmetic, T_DollarBraced, T_DollarBracket, T_DollarDoubleQuoted,
    T_DollarExpansion, T_DollarSingleQuoted, T_Extglob, T_FdRedirect, T_ForArithmetic, T_Function,
    T_GREATAND, T_Glob, T_Greater, T_HereString, T_IndexedElement, T_IoFile, T_Literal, T_Pipe,
    T_Pipeline, T_ProcSub, T_Redirecting, T_Script, T_SelectIn, T_SimpleCommand, T_SourceCommand,
    TA_Binary, TA_Expansion, TA_Unary, TA_Variable, TC_Binary, TC_Unary,
};

const VAR_CHARS: &str = "_0-9a-zA-Z";

thread_local! {
    static ADVANCED: Vec<(Regex, i64, &'static str)> = vec![
        (mk_regex(&format!("^![{VAR_CHARS}]")), 3053, "indirect expansion is"),
        (mk_regex(&format!("^[{VAR_CHARS}]+\\[.*\\]$")), 3054, "array references are"),
        (mk_regex(&format!("^![{VAR_CHARS}]+\\[[*@]]$")), 3055, "array key expansion is"),
        (mk_regex(&format!("^![{VAR_CHARS}]+[*@]$")), 3056, "name matching prefixes are"),
        (mk_regex(&format!("^[{VAR_CHARS}*@]+(\\[.*\\])?[,^]")), 3059, "case modification is"),
    ];
    static SIMPLE: Vec<(Regex, i64, &'static str)> = vec![
        (mk_regex(&format!("^[{VAR_CHARS}*@]+:[^-=?+]")), 3057, "string indexing is"),
        (mk_regex("^([*@][%#]|#[@*])"), 3058, "string operations on $@/$* are"),
        (mk_regex(&format!("^[{VAR_CHARS}*@]+(\\[.*\\])?/")), 3060, "string replacement is"),
    ];
    static ECHO_FLAG_RE: Regex = mk_regex("^-[eEsn]+$");
    static BUSYBOX_FLAG_RE: Regex = mk_regex("^-[en]+$");
    static STARTS_OPTION_RE: Regex = mk_regex("^(\\+|-[^-])");
    static O_FLAG_RE: Regex = mk_regex("^[-+][abCefhmnuvxo]*o$");
    static VALID_FLAGS_RE: Regex = mk_regex("^[-+]([abCefhmnuvxo]+o?|o)$");
    static DOUBLE_DASH_RE: Regex = mk_regex("^--.+$");
    static RADIX_RE: Regex = mk_regex("^[0-9]+#");
    static SED_RE: Regex = mk_regex("^s(.)([^\n]*)g?$");
    static MULTI_DIM_RE: Regex = mk_regex("^\\[.*\\]\\[.*\\]");
    static ENCLOSED_RE: Regex = mk_regex("\\\\\\[.*\\\\\\]");
    static ESCAPE_RE: Regex = mk_regex("\\\\x1[Bb]|\\\\e|\u{1B}|\\\\033");
}

const BASH_VARS: &[&str] = &[
    "OSTYPE",
    "MACHTYPE",
    "HOSTTYPE",
    "HOSTNAME",
    "DIRSTACK",
    "EUID",
    "UID",
    "SHLVL",
    "PIPESTATUS",
    "SHELLOPTS",
    "_",
    "BASHOPTS",
    "BASHPID",
    "BASH_ALIASES",
    "BASH_ARGC",
    "BASH_ARGV",
    "BASH_ARGV0",
    "BASH_CMDS",
    "BASH_COMMAND",
    "BASH_EXECUTION_STRING",
    "BASH_LINENO",
    "BASH_REMATCH",
    "BASH_SOURCE",
    "BASH_SUBSHELL",
    "BASH_VERSINFO",
    "EPOCHREALTIME",
    "EPOCHSECONDS",
    "FUNCNAME",
    "GROUPS",
    "MACHTYPE",
    "MAPFILE",
];

type Check = fn(&Parameters<'_>, &Token, &mut Out);

/// The checks with the shells they apply to (`checks`).
const CHECKS: &[(&[Shell], Check)] = &[
    (
        &[Shell::Sh, Shell::Dash, Shell::BusyboxSh, Shell::Bash],
        check_for_decimals,
    ),
    (&[Shell::Sh, Shell::Dash, Shell::BusyboxSh], check_bashisms),
    (&[Shell::Bash, Shell::Ksh], check_echo_sed),
    (&[Shell::Bash], check_brace_expansion_vars),
    (&[Shell::Bash], check_multi_dimensional_arrays),
    (&[Shell::Bash], check_ps1_assignments),
    (
        &[Shell::Dash, Shell::BusyboxSh, Shell::Sh],
        check_multiple_bangs,
    ),
    (
        &[Shell::Dash, Shell::BusyboxSh, Shell::Sh, Shell::Bash],
        check_bang_after_pipe,
    ),
    (&[Shell::Bash], check_negated_unary_ops),
];

/// The per-token check: every check that applies to the shell.
pub fn check(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    for (shells, f) in CHECKS {
        if shells.contains(&params.shell_type) {
            f(params, t, out);
        }
    }
}

fn check_for_decimals(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let TA_Expansion(_) = &t.inner
        && let Some(s) = get_literal_string(t)
        && let Some(first) = s.chars().next()
        && first.is_ascii_digit()
        && s[first.len_utf8()..].contains('.')
    {
        err(
            out,
            t.id,
            2079,
            "(( )) doesn't support decimals. Use bc or awk.",
        );
    }
}

struct Ctx<'p, 'a> {
    params: &'p Parameters<'a>,
    is_busybox: bool,
    is_dash: bool,
}

impl Ctx<'_, '_> {
    fn warn_msg(&self, out: &mut Out, id: Id, code: i64, s: &str) {
        if self.is_dash {
            err(out, id, code, &format!("In dash, {s} not supported."));
        } else {
            warn(out, id, code, &format!("In POSIX sh, {s} undefined."));
        }
    }

    fn is_assigned(&self, var: &str) -> bool {
        self.params
            .variable_flow
            .iter()
            .any(|x| matches!(x, StackData::Assignment((_, _, name, _)) if name == var))
    }

    fn is_bash_variable(&self, var: &str) -> bool {
        (var == "RANDOM"
            || var == "SECONDS"
            || (BASH_VARS.contains(&var) && !self.is_assigned(var)))
            && !(self.is_dash && var == "_")
    }

    fn check_test_op(&self, out: &mut Out, binary: bool, op: &str, id: Id) {
        let entry: Option<(i64, &[Shell], String)> = if binary {
            match op {
                "<" | ">" | "\\<" | "\\>" | "<=" | ">=" | "\\<=" | "\\>=" => Some((
                    3012,
                    &[Shell::Dash, Shell::BusyboxSh],
                    format!("lexicographical {op} is"),
                )),
                "==" => Some((3014, &[Shell::BusyboxSh], format!("{op} in place of = is"))),
                "=~" => Some((3015, &[], format!("{op} regex matching is"))),
                _ => None,
            }
        } else {
            match op {
                "-v" => Some((
                    3016,
                    &[],
                    format!("test {op} (in place of [ -n \"${{var+x}}\" ]) is"),
                )),
                "-a" => Some((3017, &[], format!("unary {op} in place of -e is"))),
                "-o" => Some((3062, &[], format!("test {op} to check options is"))),
                "-R" => Some((3063, &[], format!("test {op} and namerefs in general are"))),
                "-N" => Some((3064, &[], format!("test {op} is"))),
                "-k" => Some((
                    3065,
                    &[Shell::Dash, Shell::BusyboxSh],
                    format!("test {op} is"),
                )),
                "-G" => Some((
                    3066,
                    &[Shell::Dash, Shell::BusyboxSh],
                    format!("test {op} is"),
                )),
                "-O" => Some((
                    3067,
                    &[Shell::Dash, Shell::BusyboxSh],
                    format!("test {op} is"),
                )),
                _ => None,
            }
        };
        if let Some((code, shells, msg)) = entry
            && !shells.contains(&self.params.shell_type)
        {
            self.warn_msg(out, id, code, &msg);
        }
    }
}

fn check_bashisms(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let is_busybox = params.shell_type == Shell::BusyboxSh;
    let c = Ctx {
        params,
        is_busybox,
        is_dash: params.shell_type == Shell::Dash || is_busybox,
    };
    bashism(&c, t, out);
}

#[allow(
    clippy::too_many_lines,
    reason = "one arm per equation of ShellCheck's bashism"
)]
fn bashism(c: &Ctx<'_, '_>, t: &Token, out: &mut Out) {
    let id = t.id;
    match &t.inner {
        T_ProcSub(..) => return c.warn_msg(out, id, 3001, "process substitution is"),
        T_Extglob(..) => return c.warn_msg(out, id, 3002, "extglob is"),
        T_DollarSingleQuoted(_) => {
            if !c.is_busybox {
                c.warn_msg(out, id, 3003, "$'..' is");
            }
            return;
        }
        T_DollarDoubleQuoted(_) => return c.warn_msg(out, id, 3004, "$\"..\" is"),
        T_ForArithmetic(..) => return c.warn_msg(out, id, 3005, "arithmetic for loops are"),
        T_Arithmetic(_) => return c.warn_msg(out, id, 3006, "standalone ((..)) is"),
        T_DollarBracket(_) => return c.warn_msg(out, id, 3007, "$[..] in place of $((..)) is"),
        T_SelectIn(..) => return c.warn_msg(out, id, 3008, "select loops are"),
        T_BraceExpansion(_) => return c.warn_msg(out, id, 3009, "brace expansion is"),
        T_Condition(ConditionType::DoubleBracket, _) => {
            if !c.is_busybox {
                c.warn_msg(out, id, 3010, "[[ ]] is");
            }
            return;
        }
        T_HereString(_) => return c.warn_msg(out, id, 3011, "here-strings are"),
        TC_Binary(_, op, _, _) => return c.check_test_op(out, true, op, id),
        _ => {}
    }
    if let T_SimpleCommand(_, words) = &t.inner
        && let [w0, _, w2, _] = words.as_slice()
        && get_literal_string(w0).as_deref() == Some("test")
        && let Some(op) = get_literal_string(w2)
    {
        return c.check_test_op(out, true, &op, id);
    }
    if let TC_Unary(_, op, _) = &t.inner {
        return c.check_test_op(out, false, op, id);
    }
    if let T_SimpleCommand(_, words) = &t.inner
        && let [w0, w1, _] = words.as_slice()
        && get_literal_string(w0).as_deref() == Some("test")
        && let Some(op) = get_literal_string(w1)
    {
        return c.check_test_op(out, false, &op, id);
    }
    match &t.inner {
        TA_Unary(op, _) if ["|++", "|--", "++|", "--|"].contains(&op.as_str()) => {
            let o: String = op.chars().filter(|&ch| ch != '|').collect();
            return c.warn_msg(out, id, 3018, &format!("{o} is"));
        }
        TA_Binary(op, _, _) if op == "**" => return c.warn_msg(out, id, 3019, "exponentials are"),
        T_FdRedirect(fd, x)
            if fd == "&"
                && matches!(&x.inner, T_IoFile(op, _) if matches!(op.inner, T_Greater)) =>
        {
            if !c.is_busybox {
                c.warn_msg(out, id, 3020, "&> is");
            }
            return;
        }
        T_FdRedirect(fd, x)
            if fd.is_empty()
                && matches!(&x.inner, T_IoFile(op, _) if matches!(op.inner, T_GREATAND)) =>
        {
            let T_IoFile(_, file) = &x.inner else {
                unreachable!("matched above")
            };
            if !only_literal_string(file)
                .chars()
                .all(|ch| ch.is_ascii_digit())
            {
                c.warn_msg(out, id, 3021, ">& filename (as opposed to >& fd) is");
            }
            return;
        }
        T_FdRedirect(fd, _) if fd.starts_with('{') => {
            return c.warn_msg(out, id, 3022, "named file descriptors are");
        }
        T_FdRedirect(num, _)
            if num.chars().all(|ch| ch.is_ascii_digit()) && num.chars().count() > 1 =>
        {
            return c.warn_msg(out, id, 3023, "FDs outside 0-9 are");
        }
        T_Assignment(AssignmentMode::Append, ..) => return c.warn_msg(out, id, 3024, "+= is"),
        T_IoFile(_, word)
            if {
                let file = only_literal_string(word);
                file.starts_with("/dev/tcp") || file.starts_with("/dev/udp")
            } =>
        {
            return c.warn_msg(out, id, 3025, "/dev/{tcp,udp} is");
        }
        T_Glob(s) if s.contains("[^") => {
            return c.warn_msg(
                out,
                id,
                3026,
                "^ in place of ! in glob bracket expressions is",
            );
        }
        TA_Variable(s, _) if c.is_bash_variable(s) => {
            return c.warn_msg(out, id, 3028, &format!("{s} is"));
        }
        T_DollarBraced(_, token) => {
            let s = concat_oversimplify(token);
            let var = get_braced_reference(&s);
            if !c.is_busybox {
                SIMPLE.with(|list| {
                    for (re, code, feature) in list {
                        if re.match_groups(&s).is_some() {
                            c.warn_msg(out, id, *code, feature);
                        }
                    }
                });
            }
            ADVANCED.with(|list| {
                for (re, code, feature) in list {
                    if re.match_groups(&s).is_some() {
                        c.warn_msg(out, id, *code, feature);
                    }
                }
            });
            if c.is_bash_variable(&var) {
                c.warn_msg(out, id, 3028, &format!("{var} is"));
            }
            return;
        }
        T_Pipe(s) if s == "|&" => return c.warn_msg(out, id, 3029, "|& in place of 2>&1 | is"),
        T_Array(_) => return c.warn_msg(out, id, 3030, "arrays are"),
        T_IoFile(_, w) if is_glob(w) => {
            return c.warn_msg(out, id, 3031, "redirecting to/from globs is");
        }
        T_CoProc(..) => return c.warn_msg(out, id, 3032, "coproc is"),
        T_Function(_, _, s, _) if !is_variable_name(s) => {
            return c.warn_msg(
                out,
                id,
                3033,
                "naming functions outside [a-zA-Z_][a-zA-Z0-9_]* is",
            );
        }
        T_DollarExpansion(l) if l.len() == 1 && is_only_redirection(&l[0]) => {
            return c.warn_msg(out, id, 3034, "$(<file) to read files is");
        }
        T_Backticked(l) if l.len() == 1 && is_only_redirection(&l[0]) => {
            return c.warn_msg(out, id, 3035, "`<file` to read files is");
        }
        _ => {}
    }
    if let T_SimpleCommand(_, words) = &t.inner {
        if let [_, arg, ..] = words.as_slice() {
            let arg_string = concat_oversimplify(arg);
            if is_command(t, "echo") && ECHO_FLAG_RE.with(|re| re.is_match(&arg_string)) {
                if c.is_busybox {
                    if !BUSYBOX_FLAG_RE.with(|re| re.is_match(&arg_string)) {
                        c.warn_msg(out, arg.id, 3036, "echo flags besides -n and -e");
                    }
                } else if c.is_dash {
                    if arg_string != "-n" {
                        c.warn_msg(out, arg.id, 3036, "echo flags besides -n");
                    }
                } else {
                    c.warn_msg(out, arg.id, 3037, "echo flags are");
                }
                return;
            }
            if get_literal_string(&words[0]).as_deref() == Some("exec")
                && arg_string.starts_with('-')
            {
                return c.warn_msg(out, arg.id, 3038, "exec flags are");
            }
        }
        if is_command(t, "let") {
            return c.warn_msg(out, id, 3039, "'let' is");
        }
        if !words.is_empty() && is_command(t, "set") {
            if !c.is_dash {
                check_set_options(c, &words[1..], out);
            }
            return;
        }
        if let Some((cmd, rest)) = words.split_first() {
            general_command(c, t, cmd, rest, out);
            return;
        }
    }
    match &t.inner {
        T_SourceCommand(src, _) if get_command_name(src).as_deref() == Some("source") => {
            if !c.is_busybox {
                c.warn_msg(out, id, 3051, "'source' in place of '.' is");
            }
        }
        TA_Expansion(parts) => {
            if let Some(Token {
                id: lid,
                inner: T_Literal(s),
            }) = parts.first()
                && RADIX_RE.with(|re| re.is_match(s))
            {
                c.warn_msg(out, *lid, 3052, "arithmetic base conversion is");
            }
        }
        _ => {}
    }
}

fn check_set_options(c: &Ctx<'_, '_>, args: &[Token], out: &mut Out) {
    let mut lits: Vec<(Id, String)> = Vec::new();
    for a in args {
        match get_literal_string(a) {
            Some(s) => lits.push((a.id, s)),
            None => break,
        }
    }
    let long_options = [
        "allexport",
        "errexit",
        "ignoreeof",
        "monitor",
        "noclobber",
        "noexec",
        "noglob",
        "nolog",
        "notify",
        "nounset",
        "verbose",
        "vi",
        "xtrace",
    ];
    check_options(c, &lits, out, &long_options);
}

fn check_flags_then(c: &Ctx<'_, '_>, list: &[(Id, String)], out: &mut Out, long_options: &[&str]) {
    let Some((flag, rest)) = list.split_first() else {
        return;
    };
    let (fid, f) = flag;
    if STARTS_OPTION_RE.with(|re| re.is_match(f)) {
        if !VALID_FLAGS_RE.with(|re| re.is_match(f)) {
            for letter in f.chars().skip(1) {
                if !"abCefhmnuvxo".contains(letter) {
                    c.warn_msg(out, *fid, 3041, &format!("set flag -{letter} is"));
                }
            }
        }
        check_options(c, rest, out, long_options);
    } else if DOUBLE_DASH_RE.with(|re| re.is_match(f)) {
        c.warn_msg(out, *fid, 3042, &format!("set flag {f} is"));
        check_options(c, rest, out, long_options);
    }
}

fn check_options(c: &Ctx<'_, '_>, list: &[(Id, String)], out: &mut Out, long_options: &[&str]) {
    match list {
        [flag, opt, rest @ ..] if O_FLAG_RE.with(|re| re.is_match(&flag.1)) => {
            if !long_options.contains(&opt.1.as_str()) {
                c.warn_msg(out, opt.0, 3040, &format!("set option {} is", opt.1));
            }
            let mut v = vec![flag.clone()];
            v.extend(rest.iter().cloned());
            check_flags_then(c, &v, out, long_options);
        }
        [] => {}
        _ => check_flags_then(c, list, out, long_options),
    }
}

fn general_command(c: &Ctx<'_, '_>, t: &Token, cmd: &Token, rest: &[Token], out: &mut Out) {
    let id = t.id;
    let name = get_command_name(t).unwrap_or_default();
    let flags = get_leading_flags(t);
    if name == "local" && !c.is_dash {
        c.warn_msg(out, id, 3043, "'local' is");
    }
    let unsupported = [
        "let",
        "caller",
        "builtin",
        "complete",
        "compgen",
        "declare",
        "dirs",
        "disown",
        "enable",
        "mapfile",
        "readarray",
        "pushd",
        "popd",
        "shopt",
        "suspend",
        "typeset",
    ];
    if unsupported.contains(&name.as_str()) {
        c.warn_msg(out, id, 3044, &format!("'{name}' is"));
    }
    let allowed: Option<Option<Vec<&str>>> = match name.as_str() {
        "cd" => Some(Some(vec!["L", "P"])),
        "exec" | "printf" | "trap" | "wait" => Some(Some(vec![])),
        "export" | "readonly" => Some(Some(vec!["p"])),
        "hash" => Some(Some(if c.is_dash { vec!["r", "v"] } else { vec!["r"] })),
        "jobs" => Some(Some(vec!["l", "p"])),
        "read" => Some(Some(if c.is_dash || c.is_busybox {
            vec!["r", "p"]
        } else {
            vec!["r"]
        })),
        "type" => Some(Some(if c.is_busybox { vec!["p"] } else { vec![] })),
        "ulimit" => Some(if c.is_dash { None } else { Some(vec!["f"]) }),
        "umask" => Some(Some(vec!["S"])),
        "unset" => Some(Some(vec!["f", "v"])),
        _ => None,
    };
    if let Some(Some(allowed)) = allowed
        && let Some((word, flag)) = flags
            .iter()
            .find(|(_, f)| !f.is_empty() && !allowed.contains(&f.as_str()))
    {
        c.warn_msg(out, word.id, 3045, &format!("{name} -{flag} is"));
    }
    if name == "source" && !c.is_busybox {
        c.warn_msg(out, id, 3046, "'source' in place of '.' is");
    }
    if name == "trap" {
        for token in rest.iter().skip(1) {
            if let Some(s) = get_literal_string(token) {
                let upper: String = s.chars().map(hchar::to_upper).collect();
                if ["ERR", "DEBUG", "RETURN"].contains(&upper.as_str()) {
                    c.warn_msg(out, token.id, 3047, &format!("trapping {s} is"));
                }
                if !c.is_busybox && upper.starts_with("SIG") {
                    c.warn_msg(out, token.id, 3048, "prefixing signal names with 'SIG' is");
                }
                if !c.is_dash && upper != s {
                    c.warn_msg(
                        out,
                        token.id,
                        3049,
                        "using lower/mixed case for signal names is",
                    );
                }
            }
        }
    }
    if name == "printf"
        && let Some(format) = rest.first()
        && only_literal_string(format).contains("%q")
    {
        c.warn_msg(out, format.id, 3050, "printf %q is");
    }
    if name == "read" && rest.iter().all(is_flag) {
        c.warn_msg(out, cmd.id, 3061, "read without a variable is");
    }
}

fn check_echo_sed(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let is_simple_sed = |s: &str| -> bool {
        let Some(g) = SED_RE.with(|re| re.match_groups(s)) else {
            return false;
        };
        let [h, rest] = g.as_slice() else {
            return false;
        };
        let Some(h) = h.chars().next() else {
            return false;
        };
        rest.chars().filter(|&ch| ch == h).count() == 2
    };
    let check_sed = |id: Id, cmd: &[String], out: &mut Out| {
        let v = match cmd {
            [s, v] if s == "sed" => v,
            [s, e, v] if s == "sed" && e == "-e" => v,
            _ => return,
        };
        if is_simple_sed(v) {
            style(
                out,
                id,
                2001,
                "See if you can use ${variable//search/replace} instead.",
            );
        }
    };
    match &t.inner {
        T_Redirecting(lefts, r) => {
            if lefts.iter().any(
                |l| matches!(&l.inner, T_FdRedirect(_, x) if matches!(x.inner, T_HereString(_))),
            ) {
                check_sed(t.id, &oversimplify(r), out);
            }
        }
        T_Pipeline(_, cmds) => {
            if let [a, b] = cmds.as_slice()
                && oversimplify(a) == ["echo", "${VAR}"]
            {
                check_sed(t.id, &oversimplify(b), out);
            }
        }
        _ => {}
    }
}

fn check_brace_expansion_vars(params: &Parameters<'_>, t: &Token, out: &mut Out) {
    let T_BraceExpansion(list) = &t.inner else {
        return;
    };
    let to_string = |t: &Token| {
        get_literal_string_ext(t, &|x| match x.inner {
            T_DollarBraced(..) | T_DollarExpansion(_) | T_DollarArithmetic(_) => {
                Some("$".to_string())
            }
            _ => Some("-".to_string()),
        })
        .unwrap_or_default()
    };
    for element in list {
        let s = to_string(element);
        if s.contains("$..") || s.contains("..$") {
            let mut path: Vec<&Token> = vec![element];
            path.extend(get_parents_of_id(&params.parent_map, element.id));
            let cmd = find_first(
                |x: &&Token| match x.inner {
                    T_Redirecting(..) => Some(true),
                    T_Script(..) => Some(false),
                    _ => None,
                },
                &path,
            );
            let evaled = cmd.is_some_and(|c| is_unqualified_command(c, "eval"));
            if evaled {
                style(
                    out,
                    t.id,
                    2175,
                    "Quote this invalid brace expansion since it should be passed literally to eval.",
                );
            } else {
                warn(
                    out,
                    t.id,
                    2051,
                    "Bash doesn't support variables in brace range expansions.",
                );
            }
        }
    }
}

fn check_multi_dimensional_arrays(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    let about = |id: Id, out: &mut Out| {
        warn(
            out,
            id,
            2180,
            "Bash does not support multidimensional arrays. Use 1D or associative arrays.",
        );
    };
    match &t.inner {
        T_Assignment(_, _, indices, _) | T_IndexedElement(indices, _) if indices.len() >= 2 => {
            about(indices[1].id, out);
        }
        T_DollarBraced(_, l)
            if MULTI_DIM_RE
                .with(|re| re.is_match(&get_braced_modifier(&concat_oversimplify(l)))) =>
        {
            about(t.id, out);
        }
        _ => {}
    }
}

fn check_ps1_assignments(_: &Parameters<'_>, t: &Token, out: &mut Out) {
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

fn check_multiple_bangs(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_Banged(inner) = &t.inner
        && matches!(inner.inner, T_Banged(_))
    {
        err(
            out,
            t.id,
            2325,
            "Multiple ! in front of pipelines are a bash/ksh extension. Use only 0 or 1.",
        );
    }
}

fn check_bang_after_pipe(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let T_Pipeline(_, cmds) = &t.inner {
        for c in cmds {
            if let T_Banged(_) = &c.inner {
                err(
                    out,
                    c.id,
                    2326,
                    "! is not allowed in the middle of pipelines. Use command group as in cmd | { ! cmd; } if necessary.",
                );
            }
        }
    }
}

fn check_negated_unary_ops(_: &Parameters<'_>, t: &Token, out: &mut Out) {
    if let TC_Unary(ConditionType::SingleBracket, bang, inner) = &t.inner
        && bang == "!"
        && let TC_Unary(_, op, _) = &inner.inner
    {
        match op.as_str() {
            "-o" => err(
                out,
                t.id,
                2332,
                "[ ! -o opt ] is always true because -o becomes logical OR. Use [[ ]] or ! [ -o opt ].",
            ),
            "-a" => err(
                out,
                t.id,
                2332,
                "[ ! -a file ] is always true because -a becomes logical AND. Use -e instead.",
            ),
            _ => {}
        }
    }
}

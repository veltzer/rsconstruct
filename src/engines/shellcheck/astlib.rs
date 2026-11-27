//! Token helpers (`ShellCheck.ASTLib`).

use std::collections::HashMap;
use std::fmt::Write as _;

use super::ast::{Annotation, Id, Inner, Token, do_analysis};
use super::hchar;
use super::regex::{Regex, mk_regex};

use Inner::{
    TAnnotation, TAssignment, TBackticked, TBatsTest, TBraceExpansion, TBraceGroup,
    TDollarArithmetic, TDollarBraceCommandExpansion, TDollarBraced, TDollarDoubleQuoted,
    TDollarExpansion, TDollarSingleQuoted, TDoubleQuoted, TExtglob, TForArithmetic, TForIn,
    TFunction, TGlob, TIfExpression, TIoDuplicate, TLexGreatand, TLexLessand, TLiteral,
    TNormalWord, TParamSubSpecialChar, TPipeline, TRedirecting, TScript, TSelectIn, TSimpleCommand,
    TSingleQuoted, TSubshell, TUntilExpression, TWhileExpression, TaExpansion, TaSequence,
};

/// `arguments`: a simple command's arguments after the name.
pub fn arguments(t: &Token) -> &[Token] {
    match &t.inner {
        TSimpleCommand(_, words) if !words.is_empty() => &words[1..],
        _ => panic!("arguments on a non-command"),
    }
}

pub const fn is_loop(t: &Token) -> bool {
    matches!(
        t.inner,
        TWhileExpression(..)
            | TUntilExpression(..)
            | TForIn(..)
            | TForArithmetic(..)
            | TSelectIn(..)
    )
}

pub fn will_split(x: &Token) -> bool {
    match &x.inner {
        TDollarBraced(..) | TDollarExpansion(..) | TBackticked(..) | TBraceExpansion(..)
        | TGlob(..) | TExtglob(..) => true,
        TDoubleQuoted(l) => l.iter().any(will_become_multiple_args),
        TNormalWord(l) => l.iter().any(will_split),
        _ => false,
    }
}

pub fn is_glob(t: &Token) -> bool {
    match &t.inner {
        TExtglob(..) | TGlob(..) => true,
        TNormalWord(l) => l.iter().any(is_glob) || has_split_range(l),
        _ => false,
    }
}

fn has_split_range(l: &[Token]) -> bool {
    let after = l
        .iter()
        .skip_while(|t| !matches!(&t.inner, TLiteral(s) if s == "["));
    after
        .into_iter()
        .any(|t| matches!(&t.inner, TLiteral(s) if s.contains(']')))
}

pub fn is_constant(token: &Token) -> bool {
    match &token.inner {
        TNormalWord(l) => {
            if let Some(Token {
                inner: TLiteral(s), ..
            }) = l.first()
                && s.starts_with('~')
            {
                return false;
            }
            l.iter().all(is_constant)
        }
        TDoubleQuoted(l) => l.iter().all(is_constant),
        TSingleQuoted(_) | TLiteral(_) => true,
        _ => false,
    }
}

/// `oversimplify`.
pub fn oversimplify(token: &Token) -> Vec<String> {
    match &token.inner {
        TNormalWord(l) | TDoubleQuoted(l) => {
            vec![l.iter().flat_map(oversimplify).collect::<String>()]
        }
        TSingleQuoted(s)
        | TGlob(s)
        | TLiteral(s)
        | TParamSubSpecialChar(s)
        | TDollarSingleQuoted(s) => vec![s.clone()],
        TDollarBraced(..) | TDollarArithmetic(_) | TDollarExpansion(_) | TBackticked(_) => {
            vec!["${VAR}".to_string()]
        }
        TPipeline(_, cmds) if cmds.len() == 1 => oversimplify(&cmds[0]),
        TSimpleCommand(_, words) => words.iter().flat_map(oversimplify).collect(),
        TRedirecting(_, cmd) | TAnnotation(_, cmd) => oversimplify(cmd),
        TaSequence(l) if l.len() == 1 => match &l[0].inner {
            TaExpansion(v) => v.iter().flat_map(oversimplify).collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

pub fn concat_oversimplify(t: &Token) -> String {
    oversimplify(t).concat()
}

/// `getFlagsUntil`.
pub fn get_flags_until<'a>(
    stop: &dyn Fn(&str) -> bool,
    cmd: &'a Token,
) -> Vec<(&'a Token, String)> {
    let TSimpleCommand(_, words) = &cmd.inner else {
        panic!("getFlags on non-command");
    };
    assert!(!words.is_empty(), "getFlags on non-command");
    let args: Vec<(&Token, String)> = words[1..]
        .iter()
        .map(|x| (x, concat_oversimplify(x)))
        .collect();
    let split = args.iter().position(|(_, s)| stop(s)).unwrap_or(args.len());
    let mut out = Vec::new();
    for (x, s) in &args[..split] {
        if let Some(arg) = s.strip_prefix("--") {
            out.push((*x, arg.chars().take_while(|&c| c != '=').collect()));
        } else if let Some(flags) = s.strip_prefix('-') {
            for v in flags.chars() {
                out.push((*x, v.to_string()));
            }
        } else {
            out.push((*x, String::new()));
        }
    }
    for (x, _) in &args[split..] {
        out.push((*x, String::new()));
    }
    out
}

pub fn get_all_flags(cmd: &Token) -> Vec<(&Token, String)> {
    get_flags_until(&|s| s == "--", cmd)
}

pub fn get_leading_flags(cmd: &Token) -> Vec<(&Token, String)> {
    get_flags_until(&|s| s == "--" || !s.starts_with('-'), cmd)
}

pub fn has_flag(cmd: &Token, s: &str) -> bool {
    get_all_flags(cmd).iter().any(|(_, f)| f == s)
}

pub fn is_flag(token: &Token) -> bool {
    matches!(get_word_parts(token).first(), Some(Token { inner: TLiteral(s), .. }) if s.starts_with('-'))
}

pub fn is_unquoted_flag(token: &Token) -> bool {
    get_leading_unquoted_string(token).is_some_and(|s| s.starts_with('-'))
}

pub type Opts<'a> = Vec<(String, (&'a Token, &'a Token))>;

pub fn get_gnu_opts<'a>(s: &str, args: &'a [Token]) -> Option<Opts<'a>> {
    get_opts((true, false), s, &[], args)
}

pub fn get_bsd_opts<'a>(s: &str, args: &'a [Token]) -> Option<Opts<'a>> {
    get_opts((false, false), s, &[], args)
}

/// `getOpts`.
pub fn get_opts<'a>(
    config: (bool, bool),
    string: &str,
    longopts: &[(&str, bool)],
    args: &'a [Token],
) -> Option<Opts<'a>> {
    fn process<'a>(
        args: &'a [Token],
        gnu: bool,
        arbitrary: bool,
        flag_map: &HashMap<String, bool>,
        list_to_args: &dyn Fn(&'a [Token]) -> Opts<'a>,
    ) -> Option<Opts<'a>> {
        let Some((token, rest)) = args.split_first() else {
            return Some(Vec::new());
        };
        let s = get_literal_string_def("\0", token);
        if s == "--" {
            return Some(list_to_args(rest));
        }
        if let Some(word) = s.strip_prefix("--") {
            let (name, arg) = word
                .find('=')
                .map_or((word, ""), |p| (&word[..p], &word[p..]));
            let needs_arg = if arbitrary {
                flag_map.get(name).copied().unwrap_or(false)
            } else {
                *flag_map.get(name)?
            };
            if needs_arg && arg.is_empty() {
                let (value, rest2) = rest.split_first()?;
                let mut more = process(rest2, gnu, arbitrary, flag_map, list_to_args)?;
                more.insert(0, (name.to_string(), (token, value)));
                return Some(more);
            }
            let mut more = process(rest, gnu, arbitrary, flag_map, list_to_args)?;
            more.insert(0, (name.to_string(), (token, token)));
            return Some(more);
        }
        if let Some(opts) = s.strip_prefix('-') {
            let opts: Vec<char> = opts.chars().collect();
            return short_to_opts(&opts, token, rest, gnu, arbitrary, flag_map, list_to_args);
        }
        if gnu {
            let mut more = process(rest, gnu, arbitrary, flag_map, list_to_args)?;
            more.insert(0, (String::new(), (token, token)));
            Some(more)
        } else {
            Some(list_to_args(args))
        }
    }

    fn short_to_opts<'a>(
        opts: &[char],
        token: &'a Token,
        args: &'a [Token],
        gnu: bool,
        arbitrary: bool,
        flag_map: &HashMap<String, bool>,
        list_to_args: &dyn Fn(&'a [Token]) -> Opts<'a>,
    ) -> Option<Opts<'a>> {
        let Some((&c, rest)) = opts.split_first() else {
            return process(args, gnu, arbitrary, flag_map, list_to_args);
        };
        let needs_arg = *flag_map.get(&c.to_string())?;
        if needs_arg && rest.is_empty() {
            let (next, rest_args) = args.split_first()?;
            let mut more = process(rest_args, gnu, arbitrary, flag_map, list_to_args)?;
            more.insert(0, (c.to_string(), (token, next)));
            Some(more)
        } else if needs_arg {
            let mut more = process(args, gnu, arbitrary, flag_map, list_to_args)?;
            more.insert(0, (c.to_string(), (token, token)));
            Some(more)
        } else {
            let mut more =
                short_to_opts(rest, token, args, gnu, arbitrary, flag_map, list_to_args)?;
            more.insert(0, (c.to_string(), (token, token)));
            Some(more)
        }
    }

    let (gnu, arbitrary_long_opts) = config;
    let mut flag_map: HashMap<String, bool> = HashMap::new();
    // Map.fromList: later entries win.
    let mut entries: Vec<(String, bool)> = vec![(String::new(), false)];
    let chars: Vec<char> = string.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i + 1] == ':' {
            entries.push((chars[i].to_string(), true));
            i += 2;
        } else {
            entries.push((chars[i].to_string(), false));
            i += 1;
        }
    }
    for (name, takes) in longopts {
        entries.push(((*name).to_string(), *takes));
    }
    for (k, v) in entries {
        flag_map.insert(k, v);
    }
    let list_to_args =
        |rest: &'a [Token]| -> Opts<'a> { rest.iter().map(|x| (String::new(), (x, x))).collect() };

    process(args, gnu, arbitrary_long_opts, &flag_map, &list_to_args)
}

/// `getGenericOpts`.
pub fn get_generic_opts(args: &[Token]) -> Opts<'_> {
    let mut out = Vec::new();
    let mut rest = args;
    while let Some((token, tail)) = rest.split_first() {
        let s = get_literal_string_def("\0", token);
        if s == "--" {
            out.extend(tail.iter().map(|c| (String::new(), (c, c))));
            return out;
        }
        if let Some(word) = s.strip_prefix("--") {
            out.push((
                word.chars()
                    .take_while(|c| *c != '\0' && *c != '=')
                    .collect(),
                (token, token),
            ));
            rest = tail;
            continue;
        }
        if let Some(opt_string) = s.strip_prefix('-') {
            let opts: Vec<char> = opt_string.chars().take_while(|&c| c != '\0').collect();
            match tail.split_first() {
                Some((next, _)) if get_literal_string_def("\0", next).starts_with('-') => {
                    out.extend(opts.iter().map(|c| (c.to_string(), (token, token))));
                    rest = tail;
                }
                Some((next, remainder)) => {
                    if let Some((last, initial)) = opts.split_last() {
                        out.extend(initial.iter().map(|c| (c.to_string(), (token, token))));
                        out.push((last.to_string(), (token, next)));
                    }
                    rest = remainder;
                }
                None => {
                    out.extend(opts.iter().map(|c| (c.to_string(), (token, token))));
                    return out;
                }
            }
            continue;
        }
        out.push((String::new(), (token, token)));
        rest = tail;
    }
    out
}

pub fn is_array_expansion(t: &Token) -> bool {
    match &t.inner {
        TDollarBraced(_, l) => {
            let s = concat_oversimplify(l);
            s.starts_with('@') || (!s.starts_with('#') && s.contains("[@]"))
        }
        _ => false,
    }
}

pub fn may_become_multiple_args(t: &Token) -> bool {
    fn f(quoted: bool, t: &Token) -> bool {
        match &t.inner {
            TDollarBraced(_, l) => {
                let s = concat_oversimplify(l);
                !quoted || s.starts_with('!')
            }
            TDoubleQuoted(parts) => parts.iter().any(|p| f(true, p)),
            TNormalWord(parts) => parts.iter().any(|p| f(quoted, p)),
            _ => false,
        }
    }
    will_become_multiple_args(t) || f(false, t)
}

pub fn will_become_multiple_args(t: &Token) -> bool {
    fn f(t: &Token) -> bool {
        match &t.inner {
            TExtglob(..) | TGlob(_) | TBraceExpansion(_) => true,
            TNormalWord(parts) => parts.iter().any(f),
            _ => false,
        }
    }
    will_concat_in_assignment(t) || f(t)
}

pub fn will_concat_in_assignment(token: &Token) -> bool {
    match &token.inner {
        TDollarBraced(..) => is_array_expansion(token),
        TDoubleQuoted(parts) | TNormalWord(parts) => parts.iter().any(will_concat_in_assignment),
        _ => false,
    }
}

/// `getLiteralStringExt`.
pub fn get_literal_string_ext(
    t: &Token,
    more: &dyn Fn(&Token) -> Option<String>,
) -> Option<String> {
    match &t.inner {
        TDoubleQuoted(l) | TDollarDoubleQuoted(l) | TNormalWord(l) | TaExpansion(l) => {
            let mut out = String::new();
            for x in l {
                out.push_str(&get_literal_string_ext(x, more)?);
            }
            Some(out)
        }
        TSingleQuoted(s) | TLiteral(s) | TParamSubSpecialChar(s) => Some(s.clone()),
        TDollarSingleQuoted(s) => Some(decode_escapes(s)),
        _ => more(t),
    }
}

/// Bash style `$'..'` decoding.
fn decode_escapes(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '\\' && i + 1 < cs.len() {
            let c = cs[i + 1];
            let simple = match c {
                'a' => Some('\u{7}'),
                'b' => Some('\u{8}'),
                'e' => Some('\u{1B}'),
                'f' => Some('\u{C}'),
                'n' => Some('\n'),
                'r' => Some('\r'),
                't' => Some('\t'),
                'v' => Some('\u{B}'),
                '\'' => Some('\''),
                '"' => Some('"'),
                '\\' => Some('\\'),
                _ => None,
            };
            if let Some(decoded) = simple {
                out.push(decoded);
                i += 2;
                continue;
            }
            if c == 'x' {
                let hi = cs.get(i + 2).copied();
                let lo = cs.get(i + 3).copied();
                match (hi, lo) {
                    (Some(hi), Some(lo)) if hi.is_ascii_hexdigit() && lo.is_ascii_hexdigit() => {
                        let code = 16 * hi.to_digit(16).unwrap_or(0) + lo.to_digit(16).unwrap_or(0);
                        out.push(char::from_u32(code).unwrap_or('\0'));
                        i += 4;
                    }
                    (Some(hi), _) if hi.is_ascii_hexdigit() => {
                        out.push(char::from_u32(hi.to_digit(16).unwrap_or(0)).unwrap_or('\0'));
                        i += 3;
                    }
                    _ => {
                        out.push('\\');
                        out.push('x');
                        i += 2;
                    }
                }
                continue;
            }
            if hchar::is_oct_digit(c) {
                let mut j = i + 1;
                let mut num: u32 = 0;
                while j < cs.len() && j < i + 4 && hchar::is_oct_digit(cs[j]) {
                    num = num * 8 + cs[j].to_digit(8).unwrap_or(0);
                    j += 1;
                }
                out.push(char::from_u32(num % 256).unwrap_or('\0'));
                i = j;
                continue;
            }
            out.push('\\');
            out.push(c);
            i += 2;
            continue;
        }
        out.push(cs[i]);
        i += 1;
    }
    out
}

pub fn get_literal_string(t: &Token) -> Option<String> {
    get_literal_string_ext(t, &|_| None)
}

pub fn get_literal_string_def(def: &str, t: &Token) -> String {
    get_literal_string_ext(t, &|_| Some(def.to_string())).unwrap_or_default()
}

pub fn only_literal_string(t: &Token) -> String {
    get_literal_string_def("", t)
}

pub fn get_unquoted_literal(t: &Token) -> Option<String> {
    match &t.inner {
        TNormalWord(list) => {
            let mut out = String::new();
            for x in list {
                match &x.inner {
                    TLiteral(s) => out.push_str(s),
                    _ => return None,
                }
            }
            Some(out)
        }
        _ => None,
    }
}

pub const fn is_quotes(t: &Token) -> bool {
    matches!(t.inner, TDoubleQuoted(_) | TSingleQuoted(_))
}

pub fn get_trailing_unquoted_literal(t: &Token) -> Option<&Token> {
    match &t.inner {
        TNormalWord(parts) => match parts.last() {
            Some(
                last @ Token {
                    inner: TLiteral(_), ..
                },
            ) => Some(last),
            _ => None,
        },
        _ => None,
    }
}

pub fn get_leading_unquoted_string(t: &Token) -> Option<String> {
    match &t.inner {
        TNormalWord(list) => match list.first() {
            Some(Token {
                inner: TLiteral(s), ..
            }) => {
                let mut out = s.clone();
                for x in &list[1..] {
                    match &x.inner {
                        TLiteral(s) => out.push_str(s),
                        _ => break,
                    }
                }
                Some(out)
            }
            _ => None,
        },
        _ => None,
    }
}

pub fn get_glob_or_literal_string(t: &Token) -> Option<String> {
    get_literal_string_ext(t, &|x| match &x.inner {
        TGlob(s) => Some(s.clone()),
        _ => None,
    })
}

pub fn is_literal(t: &Token) -> bool {
    get_literal_string(t).is_some()
}

pub fn is_literal_number(t: &Token) -> bool {
    get_literal_string(t).is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()))
}

/// `escapeForMessage` (`e4m`).
pub fn e4m(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{1B}' => out.push_str("\\e"),
            _ => {
                let escape = !hchar::is_print(c) || (!c.is_ascii() && !hchar::is_alpha(c));
                if escape {
                    if (c as u32) < 256 {
                        let _ = write!(out, "\\x{:02X}", c as u32);
                    } else {
                        let _ = write!(out, "\\U{:04X}", c as u32);
                    }
                } else {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// `getWordParts`.
pub fn get_word_parts(t: &Token) -> Vec<&Token> {
    match &t.inner {
        TNormalWord(l) | TaExpansion(l) => l.iter().flat_map(get_word_parts).collect(),
        TDoubleQuoted(l) => l.iter().collect(),
        _ => vec![t],
    }
}

/// `braceExpand`: at most 1000 words.
pub fn brace_expand(t: &Token) -> Vec<Token> {
    let TNormalWord(list) = &t.inner else {
        panic!("braceExpand on a non-word");
    };
    let options: Vec<Vec<Token>> = list
        .iter()
        .map(|part| match &part.inner {
            TBraceExpansion(items) => items.iter().flat_map(brace_expand).collect(),
            _ => vec![part.clone()],
        })
        .collect();
    let mut out = Vec::new();
    let mut current = Vec::new();
    product(&options, 0, &mut current, &mut out, t.id);
    out
}

fn product(
    options: &[Vec<Token>],
    i: usize,
    current: &mut Vec<Token>,
    out: &mut Vec<Token>,
    id: Id,
) {
    if out.len() >= 1000 {
        return;
    }
    if i == options.len() {
        out.push(Token::new(id, TNormalWord(current.clone())));
        return;
    }
    for opt in &options[i] {
        current.push(opt.clone());
        product(options, i + 1, current, out, id);
        current.pop();
        if out.len() >= 1000 {
            return;
        }
    }
}

/// `getCommand`.
pub fn get_command(t: &Token) -> Option<&Token> {
    match &t.inner {
        TRedirecting(_, w) | TAnnotation(_, w) => get_command(w),
        TSimpleCommand(_, words) if !words.is_empty() => Some(t),
        _ => None,
    }
}

pub fn get_command_name(t: &Token) -> Option<String> {
    get_command_name_and_token(false, t).0
}

pub fn get_command_argv(t: &Token) -> Option<&[Token]> {
    match &get_command(t)?.inner {
        TSimpleCommand(_, args) if !args.is_empty() => Some(args),
        _ => None,
    }
}

pub fn get_command_token_or_this(t: &Token) -> &Token {
    get_command_name_and_token(false, t).1
}

/// `getCommandNameAndToken`.
pub fn get_command_name_and_token(direct: bool, t: &Token) -> (Option<String>, &Token) {
    let Some(cmd) = get_command(t) else {
        return (None, t);
    };
    let TSimpleCommand(_, words) = &cmd.inner else {
        return (None, t);
    };
    let (w, rest) = words
        .split_first()
        .expect("getCommand returns non-empty commands");
    let Some(s) = get_literal_string(w) else {
        return (None, t);
    };
    if !direct && let Some(actual) = get_effective_command_token(&s, rest) {
        return (get_literal_string(actual), actual);
    }
    (Some(s), w)
}

fn get_effective_command_token<'a>(s: &str, args: &'a [Token]) -> Option<&'a Token> {
    let first_arg = || {
        let arg = args.first()?;
        if is_flag(arg) { None } else { Some(arg) }
    };
    match s {
        "busybox" | "builtin" | "command" | "run" => first_arg(),
        "exec" => {
            let opts = get_bsd_opts("cla:", args)?;
            opts.iter()
                .find(|(f, _)| f.is_empty())
                .map(|(_, (t, _))| *t)
        }
        _ => None,
    }
}

pub fn get_command_name_from_expansion(t: &Token) -> Option<String> {
    let (TDollarExpansion(cmds) | TBackticked(cmds) | TDollarBraceCommandExpansion(_, cmds)) =
        &t.inner
    else {
        return None;
    };
    if cmds.len() != 1 {
        return None;
    }
    match &cmds[0].inner {
        TPipeline(_, cmd) if cmd.len() == 1 => get_command_name(&cmd[0]),
        _ => None,
    }
}

pub fn get_command_basename(t: &Token) -> Option<String> {
    get_command_name(t).map(|s| basename(&s).to_string())
}

pub fn basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

pub fn is_assignment(t: &Token) -> bool {
    match &t.inner {
        TRedirecting(_, w) | TAnnotation(_, w) => is_assignment(w),
        TSimpleCommand(assigns, words) => !assigns.is_empty() && words.is_empty(),
        TAssignment(..) => true,
        _ => false,
    }
}

pub fn is_only_redirection(t: &Token) -> bool {
    match &t.inner {
        TPipeline(_, cmds) if cmds.len() == 1 => is_only_redirection(&cmds[0]),
        TAnnotation(_, w) => is_only_redirection(w),
        TRedirecting(redirs, c) if !redirs.is_empty() => is_only_redirection(c),
        TSimpleCommand(a, w) => a.is_empty() && w.is_empty(),
        _ => false,
    }
}

pub const fn is_function(t: &Token) -> bool {
    matches!(t.inner, TFunction(..))
}

pub const fn is_function_like(t: &Token) -> bool {
    matches!(t.inner, TFunction(..) | TBatsTest(..))
}

pub const fn is_brace_expansion(t: &Token) -> bool {
    matches!(t.inner, TBraceExpansion(_))
}

/// `getCommandSequences`.
pub fn get_command_sequences(t: &Token) -> Vec<&[Token]> {
    match &t.inner {
        TScript(_, cmds)
        | TBraceGroup(cmds)
        | TSubshell(cmds)
        | TForIn(_, _, cmds)
        | TForArithmetic(_, _, _, cmds)
        | TDollarExpansion(cmds)
        | TDollarBraceCommandExpansion(_, cmds)
        | TBackticked(cmds) => vec![cmds.as_slice()],
        TWhileExpression(cond, cmds) | TUntilExpression(cond, cmds) => {
            vec![cond.as_slice(), cmds.as_slice()]
        }
        TIfExpression(thens, elses) => {
            let mut out: Vec<&[Token]> = Vec::new();
            for (a, b) in thens {
                out.push(a);
                out.push(b);
            }
            out.push(elses);
            out
        }
        TAnnotation(_, t) => get_command_sequences(t),
        _ => Vec::new(),
    }
}

/// `getAssociativeArrays`.
pub fn get_associative_arrays(t: &Token) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    do_analysis(t, &mut |t| {
        if !matches!(t.inner, TSimpleCommand(..)) {
            return;
        }
        let Some(name) = get_command_name(t) else {
            return;
        };
        if !["declare", "local", "typeset"].contains(&name.as_str()) {
            return;
        }
        let flags = get_all_flags(t);
        if !flags.iter().any(|(_, f)| f == "A") {
            return;
        }
        for (arg, f) in &flags {
            if !f.is_empty() {
                continue;
            }
            let found = get_literal_string_ext(arg, &|x| match &x.inner {
                TAssignment(_, name, _, _) => Some(name.clone()),
                _ => None,
            });
            if let Some(n) = found {
                names.push(n);
            }
        }
    });
    let mut out: Vec<String> = Vec::new();
    for n in names {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PseudoGlob {
    Any,
    Many,
    Char(char),
}

pub fn word_to_pseudo_glob(word: &Token) -> Vec<PseudoGlob> {
    word_to_pseudo_glob_ext(false, word).unwrap_or_else(|| vec![PseudoGlob::Many])
}

pub fn word_to_exact_pseudo_glob(word: &Token) -> Option<Vec<PseudoGlob>> {
    word_to_pseudo_glob_ext(true, word)
}

fn word_to_pseudo_glob_ext(exact: bool, word: &Token) -> Option<Vec<PseudoGlob>> {
    let f = |x: &Token| -> Option<Vec<PseudoGlob>> {
        match &x.inner {
            TLiteral(s) | TSingleQuoted(s) => Some(s.chars().map(PseudoGlob::Char).collect()),
            TGlob(s) if s == "?" => Some(vec![PseudoGlob::Any]),
            TGlob(s) if s == "*" => Some(vec![PseudoGlob::Many]),
            TGlob(s) if s.starts_with('[') && !exact => Some(vec![PseudoGlob::Any]),
            _ => {
                if exact {
                    None
                } else {
                    Some(vec![PseudoGlob::Many])
                }
            }
        }
    };
    let glob = match &word.inner {
        TNormalWord(parts) if matches!(parts.first(), Some(Token { inner: TLiteral(s), .. }) if s.starts_with('~')) =>
        {
            if exact {
                return None;
            }
            let Some(Token {
                inner: TLiteral(s), ..
            }) = parts.first()
            else {
                return None;
            };
            let str_ = &s[1..];
            let mut this = vec![PseudoGlob::Many];
            this.extend(str_.chars().skip_while(|&c| c != '/').map(PseudoGlob::Char));
            for p in parts[1..].iter().flat_map(get_word_parts) {
                this.extend(f(p)?);
            }
            this
        }
        _ => {
            let mut out = Vec::new();
            for p in get_word_parts(word) {
                out.extend(f(p)?);
            }
            out
        }
    };
    Some(simplify_pseudo_glob(&glob))
}

/// `simplifyPseudoGlob`.
pub fn simplify_pseudo_glob(list: &[PseudoGlob]) -> Vec<PseudoGlob> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < list.len() {
        if let PseudoGlob::Char(_) = list[i] {
            out.push(list[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < list.len() && matches!(list[i], PseudoGlob::Many | PseudoGlob::Any) {
            i += 1;
        }
        let run = &list[start..i];
        out.extend(run.iter().filter(|x| **x == PseudoGlob::Any));
        if run.contains(&PseudoGlob::Many) {
            out.push(PseudoGlob::Many);
        }
    }
    out
}

pub fn pseudo_globs_can_overlap(x: &[PseudoGlob], y: &[PseudoGlob]) -> bool {
    match (x.split_first(), y.split_first()) {
        (Some((xf, xs)), Some((yf, ys))) => match (xf, yf) {
            (PseudoGlob::Many, _) | (_, PseudoGlob::Many) => {
                pseudo_globs_can_overlap(x, ys) || pseudo_globs_can_overlap(xs, y)
            }
            (PseudoGlob::Any, _) | (_, PseudoGlob::Any) => pseudo_globs_can_overlap(xs, ys),
            _ => xf == yf && pseudo_globs_can_overlap(xs, ys),
        },
        (None, None) => true,
        (Some((PseudoGlob::Many, rest)), None) => pseudo_globs_can_overlap(rest, &[]),
        (Some(_), None) => false,
        (None, Some(_)) => pseudo_globs_can_overlap(y, &[]),
    }
}

pub fn pseudo_glob_is_super_set_of(x: &[PseudoGlob], y: &[PseudoGlob]) -> bool {
    match (x.split_first(), y.split_first()) {
        (Some((xf, xs)), Some((yf, ys))) => match (xf, yf) {
            (PseudoGlob::Many, PseudoGlob::Many) => pseudo_glob_is_super_set_of(x, ys),
            (PseudoGlob::Many, _) => {
                pseudo_glob_is_super_set_of(x, ys) || pseudo_glob_is_super_set_of(xs, y)
            }
            (_, PseudoGlob::Many) => false,
            (PseudoGlob::Any, _) => pseudo_glob_is_super_set_of(xs, ys),
            (_, PseudoGlob::Any) => false,
            _ => xf == yf && pseudo_glob_is_super_set_of(xs, ys),
        },
        (None, None) => true,
        (Some((PseudoGlob::Many, rest)), None) => pseudo_glob_is_super_set_of(rest, &[]),
        _ => false,
    }
}

pub fn words_can_be_equal(x: &Token, y: &Token) -> bool {
    pseudo_globs_can_overlap(&word_to_pseudo_glob(x), &word_to_pseudo_glob(y))
}

pub const fn is_quoteable_expansion(t: &Token) -> bool {
    matches!(t.inner, TDollarBraced(..)) || is_command_substitution(t)
}

pub const fn is_command_substitution(t: &Token) -> bool {
    matches!(
        t.inner,
        TDollarExpansion(_) | TDollarBraceCommandExpansion(..) | TBackticked(_)
    )
}

pub fn is_string_expansion(t: &Token) -> bool {
    is_command_substitution(t)
        || match &t.inner {
            TDollarArithmetic(_) => true,
            TDollarBraced(..) => !is_array_expansion(t),
            _ => false,
        }
}

pub fn is_annotation_ignoring_code(code: i64, t: &Token) -> bool {
    match &t.inner {
        TAnnotation(anns, _) => anns.iter().any(
            |a| matches!(a, Annotation::DisableComment(from, to) if code >= *from && code < *to),
        ),
        _ => false,
    }
}

thread_local! {
    static ENV_RE: Regex = mk_regex("/env +(-S|--split-string=?)? *(.*)");
    static VARIABLE_NAME_RE: Regex = mk_regex("[_a-zA-Z][_a-zA-Z0-9]*");
    static INDEX_RE: Regex = mk_regex("(\\[.*\\])");
    static OFFSET_RE: Regex = mk_regex("^(\\[.+\\])? *:([^-=?+].*)");
}

/// `executableFromShebang`.
pub fn executable_from_shebang(s: &str) -> String {
    fn basename_s(s: &str) -> String {
        basename(s).to_string()
    }
    fn from_env_args(args: &[&str]) -> String {
        args.iter()
            .skip_while(|a| a.starts_with('-'))
            .find(|a| !a.contains('='))
            .map(|s| (*s).to_string())
            .unwrap_or_default()
    }
    if ENV_RE.with(|re| re.is_match(s)) {
        return match ENV_RE.with(|re| re.match_groups(s)) {
            Some(groups) if groups.len() == 2 => from_env_args(&haskell_words(&groups[1])),
            _ => String::new(),
        };
    }
    let w = haskell_words(s);
    match w.as_slice() {
        [] => String::new(),
        [x] => basename_s(x),
        [first, second, ..] if basename(first) == "busybox" => match basename(second) {
            "sh" => "busybox sh".to_string(),
            "ash" => "busybox ash".to_string(),
            x => x.to_string(),
        },
        [first, args @ ..] if basename(first) == "env" => from_env_args(args),
        [first, ..] => basename_s(first),
    }
}

/// Haskell's `words`: split on `isSpace`.
pub fn haskell_words(s: &str) -> Vec<&str> {
    s.split(hchar::is_space).filter(|w| !w.is_empty()).collect()
}

pub const fn is_variable_start_char(x: char) -> bool {
    x == '_' || x.is_ascii_lowercase() || x.is_ascii_uppercase()
}

pub const fn is_variable_char(x: char) -> bool {
    is_variable_start_char(x) || x.is_ascii_digit()
}

pub fn is_special_variable_char(x: char) -> bool {
    "*@#?-$!".contains(x)
}

pub fn is_variable_name(s: &str) -> bool {
    let mut it = s.chars();
    it.next()
        .is_some_and(|x| is_variable_start_char(x) && it.all(is_variable_char))
}

/// `getBracedReference`.
pub fn get_braced_reference(s: &str) -> String {
    let no_prefix = match s.chars().next() {
        Some(c) if c == '!' || c == '#' => &s[1..],
        _ => s,
    };
    let take_name = |s: &str| -> Option<String> {
        let name: String = s.chars().take_while(|&c| is_variable_char(c)).collect();
        if name.is_empty() { None } else { Some(name) }
    };
    let get_special = |s: &str| -> Option<String> {
        match s.chars().next() {
            Some(c) if is_special_variable_char(c) => Some(c.to_string()),
            _ => None,
        }
    };
    let name_expansion = || -> Option<String> {
        let rest = s.strip_prefix('!')?;
        let mut it = rest.chars();
        let next = it.next()?;
        if !is_variable_char(next) {
            return None;
        }
        let first = it.find(|&c| !is_variable_char(c))?;
        if "*?@".contains(first) {
            Some(String::new())
        } else {
            None
        }
    };
    name_expansion()
        .or_else(|| take_name(no_prefix))
        .or_else(|| get_special(no_prefix))
        .or_else(|| get_special(s))
        .unwrap_or_else(|| s.to_string())
}

/// `getBracedModifier`.
pub fn get_braced_modifier(s: &str) -> String {
    let var = get_braced_reference(s);
    let candidates: Vec<&str> = match s.chars().next() {
        Some(c) if c == '#' || c == '!' => vec![&s[1..], s],
        _ => vec![s],
    };
    for a in candidates {
        if let Some(rest) = a.strip_prefix(var.as_str()) {
            return rest.to_string();
        }
    }
    String::new()
}

/// `getIndexReferences`.
pub fn get_index_references(s: &str) -> Vec<String> {
    let Some(groups) = INDEX_RE.with(|re| re.match_groups(s)) else {
        return Vec::new();
    };
    groups.first().map_or_else(Vec::new, |index| {
        VARIABLE_NAME_RE.with(|re| re.match_all_strings(index))
    })
}

/// `getOffsetReferences`.
pub fn get_offset_references(mods: &str) -> Vec<String> {
    let Some(groups) = OFFSET_RE.with(|re| re.match_groups(mods)) else {
        return Vec::new();
    };
    groups.get(1).map_or_else(Vec::new, |offsets| {
        VARIABLE_NAME_RE.with(|re| re.match_all_strings(offsets))
    })
}

pub fn is_unmodified_parameter_expansion(t: &Token) -> bool {
    match &t.inner {
        TDollarBraced(false, _) => true,
        TDollarBraced(_, list) => {
            let s = concat_oversimplify(list);
            get_braced_reference(&s) == s
        }
        _ => false,
    }
}

pub fn get_unmodified_parameter_expansion(t: &Token) -> Option<String> {
    match &t.inner {
        TDollarBraced(_, list) => {
            let s = concat_oversimplify(list);
            if get_braced_reference(&s) == s {
                Some(s)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn is_closing_file_op(op: &Token) -> bool {
    match &op.inner {
        TIoDuplicate(t, s) => s == "-" && matches!(t.inner, TLexGreatand | TLexLessand),
        _ => false,
    }
}

pub fn get_enable_directives(root: &Token) -> Vec<String> {
    match &root.inner {
        TAnnotation(list, _) => list
            .iter()
            .filter_map(|a| match a {
                Annotation::EnableComment(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub fn get_extended_analysis_directive(root: &Token) -> Option<bool> {
    match &root.inner {
        TAnnotation(list, _) => list.iter().find_map(|a| match a {
            Annotation::ExtendedAnalysis(b) => Some(*b),
            _ => None,
        }),
        _ => None,
    }
}

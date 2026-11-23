//! The analyzer's shared machinery (`ShellCheck.AnalyzerLib`): the
//! parameters every check sees (parent map, shell, options in effect, the
//! linear variable flow), comment construction, and the helpers checks
//! share.

use std::borrow::Cow;

use super::analyzer::AnalysisSpec;
use super::ast::{
    Annotation, AssignmentMode, ConditionType, Id, Inner, IntMap, Token, do_analysis,
    do_stack_analysis,
};
use super::astlib::{
    concat_oversimplify, executable_from_shebang, get_all_flags, get_braced_modifier,
    get_braced_reference, get_bsd_opts, get_command_name, get_extended_analysis_directive,
    get_generic_opts, get_gnu_opts, get_index_references, get_literal_string,
    get_literal_string_def, get_literal_string_ext, get_offset_references, get_word_parts,
    is_annotation_ignoring_code, is_closing_file_op, is_only_redirection, is_variable_char,
    is_variable_name, oversimplify,
};
use super::cfg::CfgParameters;
use super::cfganalysis::{self, CfgAnalysis};
use super::data::{FLAGS_FOR_READ, shell_for_executable};
use super::interface::{Comment, Fix, Position, Severity, Shell, TokenComment};
use super::regex::{Regex, mk_regex};

use Inner::{
    TAnnotation, TArithmetic, TArray, TAssignment, TBackgrounded, TBackticked, TBatsTest,
    TCaseExpression, TCoProc, TCoProcBody, TCondition, TDollarBraced, TDollarDoubleQuoted,
    TDollarExpansion, TDoubleQuoted, TFdRedirect, TForIn, TGlob, THereDoc, TInclude, TLiteral,
    TNormalWord, TPipeline, TRedirecting, TScript, TSelectIn, TSimpleCommand, TSingleQuoted,
    TSubshell, TaAssignment, TaSequence, TaUnary, TaVariable, TcBinary, TcNullary, TcUnary,
};

pub type Tree<'a> = IntMap<Id, &'a Token>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    SubshellScope(&'static str),
    NoneScope,
}

#[derive(Clone, Debug)]
pub enum DataSource<'a> {
    From(Vec<Cow<'a, Token>>),
    External,
    Declaration,
    Integer,
    Checked,
}

#[derive(Clone, Debug)]
pub enum DataType<'a> {
    DataString(DataSource<'a>),
    DataArray(DataSource<'a>),
}

/// (base expression, specific position, name, value).
pub type AssignmentData<'a> = (Cow<'a, Token>, Cow<'a, Token>, String, DataType<'a>);
/// (base expression, specific position, name).
pub type ReferenceData<'a> = (Cow<'a, Token>, Cow<'a, Token>, String);

#[derive(Clone, Debug)]
pub enum StackData<'a> {
    StackScope(Scope),
    StackScopeEnd,
    Assignment(AssignmentData<'a>),
    Reference(ReferenceData<'a>),
}

/// `Parameters`.
pub struct Parameters<'a> {
    pub has_lastpipe: bool,
    pub has_inherit_errexit: bool,
    pub has_set_e: bool,
    pub has_pipefail: bool,
    pub has_execfail: bool,
    pub variable_flow: Vec<StackData<'a>>,
    pub id_map: Tree<'a>,
    pub parent_map: Tree<'a>,
    pub shell_type: Shell,
    pub shell_type_specified: bool,
    pub root_node: &'a Token,
    pub token_positions: &'a IntMap<Id, (Position, Position)>,
    pub cfg_analysis: Option<CfgAnalysis>,
}

pub fn make_comment(severity: Severity, id: Id, code: i64, note: &str) -> TokenComment {
    TokenComment {
        id,
        comment: Comment {
            severity,
            code,
            message: note.to_string(),
        },
        fix: None,
    }
}

/// `makeCommentWithFix`: an empty fix is no fix.
pub fn make_comment_with_fix(
    severity: Severity,
    id: Id,
    code: i64,
    note: &str,
    fix: Fix,
) -> TokenComment {
    let mut c = make_comment(severity, id, code, note);
    c.fix = if fix.replacements.is_empty() {
        None
    } else {
        Some(fix)
    };
    c
}

pub type Out = Vec<TokenComment>;

pub fn warn(out: &mut Out, id: Id, code: i64, s: &str) {
    out.push(make_comment(Severity::WarningC, id, code, s));
}

pub fn err(out: &mut Out, id: Id, code: i64, s: &str) {
    out.push(make_comment(Severity::ErrorC, id, code, s));
}

pub fn info(out: &mut Out, id: Id, code: i64, s: &str) {
    out.push(make_comment(Severity::InfoC, id, code, s));
}

pub fn style(out: &mut Out, id: Id, code: i64, s: &str) {
    out.push(make_comment(Severity::StyleC, id, code, s));
}

pub fn warn_with_fix(out: &mut Out, id: Id, code: i64, s: &str, fix: Fix) {
    out.push(make_comment_with_fix(Severity::WarningC, id, code, s, fix));
}

pub fn err_with_fix(out: &mut Out, id: Id, code: i64, s: &str, fix: Fix) {
    out.push(make_comment_with_fix(Severity::ErrorC, id, code, s, fix));
}

pub fn info_with_fix(out: &mut Out, id: Id, code: i64, s: &str, fix: Fix) {
    out.push(make_comment_with_fix(Severity::InfoC, id, code, s, fix));
}

pub fn style_with_fix(out: &mut Out, id: Id, code: i64, s: &str, fix: Fix) {
    out.push(make_comment_with_fix(Severity::StyleC, id, code, s, fix));
}

/// `makeParameters`.
pub fn make_parameters<'a>(spec: &AnalysisSpec<'a>) -> Parameters<'a> {
    let root = spec.script;
    let extended_analysis = spec
        .extended_analysis
        .or_else(|| get_extended_analysis_directive(root))
        .unwrap_or(true);
    let shell_type = spec
        .shell_type
        .unwrap_or_else(|| determine_shell(spec.fallback_shell, root));
    let has_lastpipe = match shell_type {
        Shell::Bash => is_option_set("lastpipe", root),
        Shell::Dash | Shell::BusyboxSh | Shell::Sh => false,
        Shell::Ksh => true,
    };
    let has_inherit_errexit = match shell_type {
        Shell::Bash => is_option_set("inherit_errexit", root),
        Shell::Dash | Shell::BusyboxSh | Shell::Sh => true,
        Shell::Ksh => false,
    };
    let has_pipefail = match shell_type {
        Shell::Bash | Shell::BusyboxSh | Shell::Ksh => is_option_set("pipefail", root),
        Shell::Dash | Shell::Sh => true,
    };
    let has_execfail = shell_type == Shell::Bash && is_option_set("execfail", root);
    let mut params = Parameters {
        has_lastpipe,
        has_inherit_errexit,
        has_set_e: contains_set_e(root),
        has_pipefail,
        has_execfail,
        variable_flow: Vec::new(),
        id_map: get_token_map(root),
        parent_map: get_parent_tree(root),
        shell_type,
        shell_type_specified: spec.shell_type.is_some() || spec.fallback_shell.is_some(),
        root_node: root,
        token_positions: spec.token_positions,
        cfg_analysis: None,
    };
    params.variable_flow = get_variable_flow(&params, root);
    if extended_analysis {
        // ShellCheck also sets `cfPipefail`, which its CFG never reads.
        let cf = CfgParameters {
            cf_lastpipe: has_lastpipe,
        };
        params.cfg_analysis = Some(cfganalysis::analyze_control_flow(cf, root));
    }
    params
}

thread_local! {
    static SET_E_RE: Regex = mk_regex("[[:space:]]-[^-]*e");
    static VARIABLE_RE: Regex = mk_regex("\\$\\{?([A-Za-z0-9_]+)");
    static QUOTED_ALTERNATIVE_RE: Regex = mk_regex("(^|\\]):?\\+");
}

/// Does any token satisfy `p`? (`isNothing $ doAnalysis (guard . not . p)`).
fn any_token(root: &Token, p: &dyn Fn(&Token) -> bool) -> bool {
    let mut found = false;
    do_analysis(root, &mut |t| {
        if !found && p(t) {
            found = true;
        }
    });
    found
}

/// `containsSetE`.
pub fn contains_set_e(root: &Token) -> bool {
    any_token(root, &|t| match &t.inner {
        TScript(sb, _) => match &sb.inner {
            TLiteral(s) => SET_E_RE.with(|re| re.is_match(s)),
            _ => false,
        },
        TSimpleCommand(..) => {
            is_unqualified_command(t, "set")
                && (oversimplify(t).iter().any(|s| s == "errexit")
                    || get_all_flags(t).iter().any(|(_, f)| f == "e"))
        }
        _ => false,
    })
}

fn contains_set_option(opt: &str, root: &Token) -> bool {
    any_token(root, &|t| {
        matches!(t.inner, TSimpleCommand(..))
            && is_unqualified_command(t, "set")
            && (oversimplify(t).iter().any(|s| s == opt)
                || get_all_flags(t).iter().any(|(_, f)| f == "o"))
    })
}

fn contains_shopt(shopt: &str, root: &Token) -> bool {
    any_token(root, &|t| {
        matches!(t.inner, TSimpleCommand(..))
            && is_unqualified_command(t, "shopt")
            && oversimplify(t).iter().any(|s| s == shopt)
    })
}

/// `isOptionSet`: `shopt -s opt` or `set -o opt` anywhere.
pub fn is_option_set(opt: &str, root: &Token) -> bool {
    contains_shopt(opt, root) || contains_set_option(opt, root)
}

/// `determineShell`.
pub fn determine_shell(fallback_shell: Option<Shell>, t: &Token) -> Shell {
    let from_shebang = |s: &Token| -> String {
        match &s.inner {
            TScript(sb, _) => match &sb.inner {
                TLiteral(s) => executable_from_shebang(s),
                _ => String::new(),
            },
            _ => panic!("determineShell: not a script"),
        }
    };
    let shell_string = match &t.inner {
        TScript(..) => from_shebang(t),
        TAnnotation(annotations, s) => annotations
            .iter()
            .find_map(|a| match a {
                Annotation::ShellOverride(x) => Some(x.clone()),
                _ => None,
            })
            .unwrap_or_else(|| from_shebang(s)),
        _ => panic!("determineShell: not a script"),
    };
    shell_for_executable(&shell_string)
        .or(fallback_shell)
        .unwrap_or(Shell::Bash)
}

/// `getParentTree`.
pub fn get_parent_tree(t: &Token) -> Tree<'_> {
    fn walk<'a>(t: &'a Token, map: &mut Tree<'a>) {
        t.for_each_child(&mut |c| {
            map.insert(c.id, t);
            walk(c, map);
        });
    }
    let mut map: Tree<'_> = Tree::default();
    walk(t, &mut map);
    map
}

/// `getTokenMap`.
pub fn get_token_map(t: &Token) -> Tree<'_> {
    let mut map: Tree<'_> = Tree::default();
    do_analysis(t, &mut |x| {
        map.insert(x.id, x);
    });
    map
}

/// `getPath`: the token and its ancestors, innermost first.
pub fn get_path<'a>(tree: &Tree<'a>, t: &'a Token) -> Vec<&'a Token> {
    let mut out = vec![t];
    let mut cur = t;
    while let Some(p) = tree.get(&cur.id) {
        out.push(p);
        cur = p;
    }
    out
}

/// The ancestors of the token with this id (`NE.tail . getPath tree`).
pub fn get_parents_of_id<'a>(tree: &Tree<'a>, id: Id) -> Vec<&'a Token> {
    let mut out = Vec::new();
    let mut cur = id;
    while let Some(p) = tree.get(&cur) {
        out.push(*p);
        cur = p.id;
    }
    out
}

pub fn is_strictly_quote_free(shell: Shell, tree: &Tree<'_>, t: &Token) -> bool {
    is_quote_free_node(true, shell, tree, t)
}

pub fn is_quote_free(shell: Shell, tree: &Tree<'_>, t: &Token) -> bool {
    is_quote_free_node(false, shell, tree, t)
}

/// `isQuoteFreeNode`.
pub fn is_quote_free_node(strict: bool, shell: Shell, tree: &Tree<'_>, t: &Token) -> bool {
    let is_assignment_param_to_command = |id: Id| match tree.get(&id).map(|p| &p.inner) {
        Some(TSimpleCommand(_, words)) if !words.is_empty() => {
            words[1..].iter().any(|a| a.id == id)
        }
        _ => false,
    };
    let assignment_is_quoting = |id: Id| shell != Shell::Sh || !is_assignment_param_to_command(id);
    let element = match &t.inner {
        TAssignment(..) => assignment_is_quoting(t.id),
        TFdRedirect(..) => true,
        _ => false,
    };
    if element {
        return true;
    }
    for p in get_parents_of_id(tree, t.id) {
        let ctx = match &p.inner {
            TcNullary(ConditionType::DoubleBracket, _)
            | TcUnary(ConditionType::DoubleBracket, ..)
            | TcBinary(ConditionType::DoubleBracket, ..)
            | TaSequence(_)
            | TArithmetic(_)
            | TDoubleQuoted(_)
            | TDollarDoubleQuoted(_)
            | TCaseExpression(..)
            | THereDoc(..)
            | TDollarBraced(..) => Some(true),
            TAssignment(..) => Some(assignment_is_quoting(p.id)),
            TRedirecting(..) => Some(false),
            TForIn(..) | TSelectIn(..) => Some(!strict),
            _ => None,
        };
        if let Some(b) = ctx {
            return b;
        }
    }
    false
}

/// `isParamTo`.
pub fn is_param_to(tree: &Tree<'_>, cmd: &str, t: &Token) -> bool {
    let mut cur = t.id;
    loop {
        let Some(parent) = tree.get(&cur) else {
            return false;
        };
        match &parent.inner {
            TSingleQuoted(_) | TDoubleQuoted(_) | TNormalWord(_) => cur = parent.id,
            TSimpleCommand(..) | TRedirecting(..) => return is_command(parent, cmd),
            _ => return false,
        }
    }
}

/// `findFirst`: the first element where `p` is `Some(true)`, stopping at
/// `Some(false)`.
pub fn find_first<T>(p: impl Fn(&T) -> Option<bool>, list: &[T]) -> Option<&T> {
    for x in list {
        match p(x) {
            Some(true) => return Some(x),
            Some(false) => return None,
            None => {}
        }
    }
    None
}

/// `getClosestCommand`.
pub fn get_closest_command<'a>(tree: &Tree<'a>, t: &'a Token) -> Option<&'a Token> {
    let path = get_path(tree, t);
    find_first(
        |t: &&Token| match t.inner {
            TRedirecting(..) => Some(true),
            TScript(..) => Some(false),
            _ => None,
        },
        &path,
    )
    .copied()
}

/// `usedAsCommandName`.
pub fn used_as_command_name(tree: &Tree<'_>, token: &Token) -> bool {
    let mut current_id = token.id;
    for p in get_parents_of_id(tree, token.id) {
        match &p.inner {
            TNormalWord(parts) | TDoubleQuoted(parts)
                if parts.len() == 1 && parts[0].id == current_id =>
            {
                current_id = p.id;
            }
            TSimpleCommand(_, words) if !words.is_empty() => {
                return words[0].id == current_id
                    || super::astlib::get_command_token_or_this(p).id == current_id;
            }
            _ => return false,
        }
    }
    false
}

/// `isParentOf`.
pub fn is_parent_of<'a>(tree: &Tree<'a>, parent: &Token, child: &'a Token) -> bool {
    get_path(tree, child).iter().any(|t| t.id == parent.id)
}

/// `tokenIsJustCommandOutput`.
pub fn token_is_just_command_output(t: &Token) -> bool {
    let check = |cmds: &[Token]| cmds.len() == 1 && !is_only_redirection(&cmds[0]);
    match &t.inner {
        TNormalWord(parts) if parts.len() == 1 => match &parts[0].inner {
            TDollarExpansion(cmds) | TBackticked(cmds) => check(cmds),
            TDoubleQuoted(inner) if inner.len() == 1 => match &inner[0].inner {
                TDollarExpansion(cmds) | TBackticked(cmds) => check(cmds),
                _ => false,
            },
            _ => false,
        },
        _ => false,
    }
}

/// `getVariableFlow`: assignments and references in tree order, with
/// subshell scopes.
pub fn get_variable_flow<'a>(params: &Parameters<'a>, t: &'a Token) -> Vec<StackData<'a>> {
    let assign_first = |t: &Token| matches!(t.inner, TForIn(..) | TSelectIn(..) | TBatsTest(..));
    let out = std::cell::RefCell::new(Vec::new());
    do_stack_analysis(
        t,
        &mut |t| {
            let scope_type = lead_type(params, t);
            if scope_type != Scope::NoneScope {
                out.borrow_mut().push(StackData::StackScope(scope_type));
            }
            if assign_first(t) {
                for v in get_modified_variables(t) {
                    out.borrow_mut().push(StackData::Assignment(v));
                }
            }
        },
        &mut |t| {
            let scope_type = lead_type(params, t);
            for v in get_referenced_variables(&params.parent_map, t) {
                out.borrow_mut().push(StackData::Reference(v));
            }
            if !assign_first(t) {
                for v in get_modified_variables(t) {
                    out.borrow_mut().push(StackData::Assignment(v));
                }
            }
            if scope_type != Scope::NoneScope {
                out.borrow_mut().push(StackData::StackScopeEnd);
            }
        },
    );
    out.into_inner()
}

/// `leadType`.
pub fn lead_type(params: &Parameters<'_>, t: &Token) -> Scope {
    match &t.inner {
        TDollarExpansion(_) => Scope::SubshellScope("$(..) expansion"),
        TBackticked(_) => Scope::SubshellScope("`..` expansion"),
        TBackgrounded(_) => Scope::SubshellScope("backgrounding &"),
        TSubshell(_) => Scope::SubshellScope("(..) group"),
        TBatsTest(..) => Scope::SubshellScope("@bats test"),
        TCoProcBody(_) => Scope::SubshellScope("coproc"),
        TRedirecting(..) => {
            let causes_subshell = match params.parent_map.get(&t.id).map(|p| &p.inner) {
                Some(TPipeline(_, list)) => {
                    list.len() >= 2
                        && (!params.has_lastpipe || list.last().map(|l| l.id) != Some(t.id))
                }
                _ => false,
            };
            if causes_subshell {
                Scope::SubshellScope("pipeline")
            } else {
                Scope::NoneScope
            }
        }
        _ => Scope::NoneScope,
    }
}

const fn b(t: &Token) -> Cow<'_, Token> {
    Cow::Borrowed(t)
}

/// `getModifiedVariables`.
pub fn get_modified_variables(t: &Token) -> Vec<AssignmentData<'_>> {
    match &t.inner {
        TSimpleCommand(vars, words) if words.is_empty() => vars
            .iter()
            .filter_map(|x| match &x.inner {
                TAssignment(_, name, _, w) => {
                    Some((b(x), b(x), name.clone(), data_type_from(false, w)))
                }
                _ => None,
            })
            .collect(),
        TSimpleCommand(..) => get_modified_variable_command(t),
        TaUnary(op, v) if op.contains("--") || op.contains("++") => match &v.inner {
            TaVariable(name, _) => vec![(
                b(t),
                b(v),
                name.clone(),
                DataType::DataString(DataSource::Integer),
            )],
            _ => Vec::new(),
        },
        TaAssignment(op, lhs, _) => match &lhs.inner {
            TaVariable(name, _)
                if [
                    "=", "*=", "/=", "%=", "+=", "-=", "<<=", ">>=", "&=", "^=", "|=",
                ]
                .contains(&op.as_str()) =>
            {
                vec![(
                    b(t),
                    b(t),
                    name.clone(),
                    DataType::DataString(DataSource::Integer),
                )]
            }
            _ => Vec::new(),
        },
        TBatsTest(..) => vec![
            (
                b(t),
                b(t),
                "lines".to_string(),
                DataType::DataArray(DataSource::External),
            ),
            (
                b(t),
                b(t),
                "status".to_string(),
                DataType::DataString(DataSource::Integer),
            ),
            (
                b(t),
                b(t),
                "output".to_string(),
                DataType::DataString(DataSource::External),
            ),
            (
                b(t),
                b(t),
                "stderr".to_string(),
                DataType::DataString(DataSource::External),
            ),
            (
                b(t),
                b(t),
                "stderr_lines".to_string(),
                DataType::DataArray(DataSource::External),
            ),
        ],
        TcUnary(_, op, token) if op == "-v" => match get_variable_for_test_dash_v(token) {
            Some(s) => vec![(b(t), b(token), s, DataType::DataString(DataSource::Checked))],
            None => Vec::new(),
        },
        TcUnary(_, op, token) if op == "-n" || op == "-z" => mark_checked(t, token),
        TcNullary(_, token) => mark_checked(t, token),
        TDollarBraced(_, l) => {
            let string = concat_oversimplify(l);
            let modifier = get_braced_modifier(&string);
            if modifier.starts_with('=') || modifier.starts_with(":=") {
                vec![(
                    b(t),
                    b(t),
                    get_braced_reference(&string),
                    DataType::DataString(DataSource::From(vec![b(l)])),
                )]
            } else {
                Vec::new()
            }
        }
        TFdRedirect(var, op) if var.starts_with('{') => {
            if is_closing_file_op(op) {
                Vec::new()
            } else {
                vec![(
                    b(t),
                    b(t),
                    var[1..].chars().take_while(|&c| c != '}').collect(),
                    DataType::DataString(DataSource::Integer),
                )]
            }
        }
        TCoProc(None, _) => vec![(
            b(t),
            b(t),
            "COPROC".to_string(),
            DataType::DataArray(DataSource::Integer),
        )],
        TCoProc(Some(token), _) => match get_literal_string(token) {
            Some(name) => vec![(b(t), b(t), name, DataType::DataArray(DataSource::Integer))],
            None => Vec::new(),
        },
        TForIn(s, words, _) if words.is_empty() => {
            vec![(
                b(t),
                b(t),
                s.clone(),
                DataType::DataString(DataSource::External),
            )]
        }
        TForIn(s, words, _) | TSelectIn(s, words, _) => vec![(
            b(t),
            b(t),
            s.clone(),
            DataType::DataString(DataSource::From(words.iter().map(b).collect())),
        )],
        _ => Vec::new(),
    }
}

fn mark_checked<'a>(place: &'a Token, token: &'a Token) -> Vec<AssignmentData<'a>> {
    get_word_parts(token)
        .into_iter()
        .filter_map(|t| match &t.inner {
            TDollarBraced(_, l) => {
                let s = get_braced_reference(&concat_oversimplify(l));
                if is_variable_name(&s) {
                    Some((b(place), b(t), s, DataType::DataString(DataSource::Checked)))
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect()
}

/// `getReferencedVariableCommand`.
pub fn get_referenced_variable_command(base: &Token) -> Vec<ReferenceData<'_>> {
    let TSimpleCommand(_, words) = &base.inner else {
        return Vec::new();
    };
    let Some((first, rest)) = words.split_first() else {
        return Vec::new();
    };
    let TNormalWord(parts) = &first.inner else {
        return Vec::new();
    };
    let Some(Token {
        inner: TLiteral(x), ..
    }) = parts.first()
    else {
        return Vec::new();
    };
    let flags: Vec<String> = get_all_flags(base).into_iter().map(|(_, f)| f).collect();
    let has = |f: &str| flags.iter().any(|x| x == f);
    fn get_reference(t: &Token) -> Vec<ReferenceData<'_>> {
        match &t.inner {
            TAssignment(_, name, _, _) => vec![(b(t), b(t), name.clone())],
            TNormalWord(p) => match p.as_slice() {
                [
                    Token {
                        inner: TLiteral(name),
                        ..
                    },
                ] if !name.starts_with('-') => vec![(b(t), b(t), name.clone())],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }
    let rest_refs = || -> Vec<ReferenceData<'_>> { rest.iter().flat_map(get_reference).collect() };
    match x.as_str() {
        "declare" | "typeset" => {
            if (has("x") || has("p")) && !(has("f") || has("F")) {
                rest_refs()
            } else {
                Vec::new()
            }
        }
        "export" => {
            if has("f") {
                Vec::new()
            } else {
                rest_refs()
            }
        }
        "local" => {
            if has("x") {
                rest_refs()
            } else {
                Vec::new()
            }
        }
        "trap" => match rest.first() {
            Some(head) => get_variables_from_literal_token(head)
                .into_iter()
                .map(|x| (b(base), b(head), x))
                .collect(),
            None => Vec::new(),
        },
        "alias" => rest
            .iter()
            .flat_map(|token| {
                get_variables_from_literal_token(token)
                    .into_iter()
                    .map(move |name| (b(base), b(token), name))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `getModifiedVariableCommand`.
pub fn get_modified_variable_command(base: &Token) -> Vec<AssignmentData<'_>> {
    let TSimpleCommand(_, words) = &base.inner else {
        return Vec::new();
    };
    let Some(first) = words.first() else {
        return Vec::new();
    };
    let x = match &first.inner {
        TNormalWord(parts) => match parts.first() {
            Some(Token {
                inner: TLiteral(x), ..
            }) => x.clone(),
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    modified_variable_command_for(base, &x)
        .into_iter()
        .filter(|(_, _, s, _)| !s.starts_with('-'))
        .collect()
}

fn own_assignment<'a>(a: AssignmentData<'_>) -> AssignmentData<'a> {
    (
        Cow::Owned(a.0.into_owned()),
        Cow::Owned(a.1.into_owned()),
        a.2,
        own_data_type(a.3),
    )
}

/// The cases of `getModifiedVariableCommand` for a command named `x`.
fn modified_variable_command_for<'a>(base: &'a Token, x: &str) -> Vec<AssignmentData<'a>> {
    let TSimpleCommand(cmd_prefix, words) = &base.inner else {
        return Vec::new();
    };
    let rest: &'a [Token] = &words[1..];
    let flags: Vec<String> = get_all_flags(base).into_iter().map(|(_, f)| f).collect();
    let has = |f: &str| flags.iter().any(|x| x == f);
    let get_literal_of_data_type = |t: &'a Token, d: DataType<'a>| -> Option<AssignmentData<'a>> {
        let s = get_literal_string(t)?;
        if s.starts_with('-') {
            return None;
        }
        Some((b(base), b(t), s, d))
    };
    let get_literal =
        |t: &'a Token| get_literal_of_data_type(t, DataType::DataString(DataSource::External));
    let get_modifier_param = |array: bool, t: &'a Token| -> Vec<AssignmentData<'a>> {
        match &t.inner {
            TAssignment(_, name, _, value) => {
                vec![(b(base), b(t), name.clone(), data_type_from(array, value))]
            }
            TNormalWord(_) => match get_literal_string(t) {
                Some(name) if is_variable_name(&name) => {
                    let d = if array {
                        DataType::DataArray(DataSource::Declaration)
                    } else {
                        DataType::DataString(DataSource::Declaration)
                    };
                    vec![(b(base), b(t), name, d)]
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    };
    match x {
        "builtin" => {
            let new_base = Token::new(base.id, TSimpleCommand(cmd_prefix.clone(), rest.to_vec()));
            get_modified_variable_command(&new_base)
                .into_iter()
                .map(own_assignment)
                .collect()
        }
        "read" => {
            let mut fallback: Vec<AssignmentData<'a>> = Vec::new();
            for t in rest.iter().rev() {
                match get_literal(t) {
                    Some(x) => fallback.push(x),
                    None => break,
                }
            }
            match get_gnu_opts(FLAGS_FOR_READ, rest) {
                None => fallback,
                Some(parsed) => match parsed.iter().find(|(f, _)| f == "a") {
                    Some((_, (_, var))) => {
                        match get_literal_of_data_type(
                            var,
                            DataType::DataArray(DataSource::External),
                        ) {
                            Some(x) => vec![x],
                            None => fallback,
                        }
                    }
                    None => parsed
                        .iter()
                        .filter(|(f, _)| f.is_empty())
                        .filter_map(|(_, (_, v))| get_literal(v))
                        .collect(),
                },
            }
        }
        "getopts" => match rest {
            [_, var, ..] => get_literal(var).into_iter().collect(),
            _ => Vec::new(),
        },
        "let" => rest
            .iter()
            .flat_map(|token| {
                let var: String = concat_oversimplify(token)
                    .chars()
                    .skip_while(|c| *c == '+' || *c == '-')
                    .take_while(|&c| is_variable_char(c))
                    .collect();
                if var.is_empty() {
                    Vec::new()
                } else {
                    vec![(
                        b(base),
                        b(token),
                        var,
                        DataType::DataString(DataSource::From(vec![Cow::Owned(
                            strip_equals_from(token),
                        )])),
                    )]
                }
            })
            .collect(),
        "export" => {
            if has("f") {
                Vec::new()
            } else {
                rest.iter()
                    .flat_map(|t| get_modifier_param(false, t))
                    .collect()
            }
        }
        "declare" | "typeset" => {
            if has("F") || has("f") || has("p") {
                Vec::new()
            } else {
                let array = has("a") || has("A");
                rest.iter()
                    .flat_map(|t| get_modifier_param(array, t))
                    .collect()
            }
        }
        "local" => rest
            .iter()
            .flat_map(|t| get_modifier_param(false, t))
            .collect(),
        "readonly" => {
            if has("f") || has("p") {
                Vec::new()
            } else {
                rest.iter()
                    .flat_map(|t| get_modifier_param(false, t))
                    .collect()
            }
        }
        "set" => match get_set_params(rest) {
            Some(params) => vec![(
                b(base),
                b(base),
                "@".to_string(),
                DataType::DataString(DataSource::From(params.into_iter().map(b).collect())),
            )],
            None => Vec::new(),
        },
        "printf" => get_flag_assigned_variable(
            base,
            "v",
            DataSource::From(rest.iter().map(b).collect()),
            get_bsd_opts("v:", rest),
        )
        .into_iter()
        .collect(),
        "wait" => {
            get_flag_assigned_variable(base, "p", DataSource::Integer, Some(get_generic_opts(rest)))
                .into_iter()
                .collect()
        }
        "mapfile" | "readarray" => get_mapfile_array(base, rest).into_iter().collect(),
        "DEFINE_boolean" | "DEFINE_float" | "DEFINE_integer" | "DEFINE_string" => match rest {
            [n, _, ..] => match get_literal_string(n) {
                Some(name) => vec![(
                    b(base),
                    b(n),
                    format!("FLAGS_{name}"),
                    DataType::DataString(DataSource::External),
                )],
                None => Vec::new(),
            },
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn own_data_type<'a>(d: DataType<'_>) -> DataType<'a> {
    let own_source = |s: DataSource<'_>| -> DataSource<'a> {
        match s {
            DataSource::From(l) => {
                DataSource::From(l.into_iter().map(|t| Cow::Owned(t.into_owned())).collect())
            }
            DataSource::External => DataSource::External,
            DataSource::Declaration => DataSource::Declaration,
            DataSource::Integer => DataSource::Integer,
            DataSource::Checked => DataSource::Checked,
        }
    };
    match d {
        DataType::DataString(s) => DataType::DataString(own_source(s)),
        DataType::DataArray(s) => DataType::DataArray(own_source(s)),
    }
}

fn strip_equals(s: &str) -> String {
    match s.find('=') {
        Some(i) => s[i + 1..].to_string(),
        None => String::new(),
    }
}

/// `stripEqualsFrom`.
fn strip_equals_from(t: &Token) -> Token {
    match &t.inner {
        TNormalWord(parts) => match parts.as_slice() {
            [
                Token {
                    id: id2,
                    inner: TLiteral(s),
                },
                rest @ ..,
            ] => {
                let mut new_parts = vec![Token::new(*id2, TLiteral(strip_equals(s)))];
                new_parts.extend(rest.iter().cloned());
                Token::new(t.id, TNormalWord(new_parts))
            }
            [
                Token {
                    id: id2,
                    inner: TDoubleQuoted(inner),
                },
            ] => match inner.as_slice() {
                [
                    Token {
                        id: id3,
                        inner: TLiteral(s),
                    },
                ] => Token::new(
                    t.id,
                    TNormalWord(vec![Token::new(
                        *id2,
                        TDoubleQuoted(vec![Token::new(*id3, TLiteral(strip_equals(s)))]),
                    )]),
                ),
                _ => t.clone(),
            },
            _ => t.clone(),
        },
        _ => t.clone(),
    }
}

/// `getSetParams`.
fn get_set_params(args: &[Token]) -> Option<Vec<&Token>> {
    match args {
        [] => None,
        [t, _, rest @ ..] if get_literal_string(t).as_deref() == Some("-o") => get_set_params(rest),
        [t, rest @ ..] => match get_literal_string(t).as_deref() {
            Some("--") => Some(rest.iter().collect()),
            Some(s) if s.starts_with('-') => get_set_params(rest),
            _ => {
                let mut out = vec![t];
                out.extend(get_set_params(rest).unwrap_or_default());
                Some(out)
            }
        },
    }
}

fn get_flag_assigned_variable<'a>(
    base: &'a Token,
    flag: &str,
    data_source: DataSource<'a>,
    flags: Option<super::astlib::Opts<'a>>,
) -> Option<AssignmentData<'a>> {
    let flags = flags?;
    let (_, (_, value)) = flags.iter().find(|(f, _)| f == flag)?;
    let variable_name = get_literal_string_ext(value, &|_| Some("!".to_string()))?;
    let (base_name, index) = match variable_name.find('[') {
        Some(i) => (variable_name[..i].to_string(), &variable_name[i..]),
        None => (variable_name.clone(), ""),
    };
    let dt = if index.is_empty() {
        DataType::DataString(data_source)
    } else {
        DataType::DataArray(data_source)
    };
    Some((b(base), b(value), base_name, dt))
}

fn get_mapfile_array<'a>(base: &'a Token, rest: &'a [Token]) -> Option<AssignmentData<'a>> {
    let parse_args = || -> Option<AssignmentData<'a>> {
        let args = get_gnu_opts("d:n:O:s:u:C:c:t", rest)?;
        let plain: Vec<&Token> = args
            .iter()
            .filter(|(f, _)| f.is_empty())
            .map(|(_, (_, y))| *y)
            .collect();
        match plain.first() {
            None => Some((
                b(base),
                b(base),
                "MAPFILE".to_string(),
                DataType::DataArray(DataSource::External),
            )),
            Some(first) => {
                let name = get_literal_string(first)?;
                if !is_variable_name(&name) {
                    return None;
                }
                Some((
                    b(base),
                    b(first),
                    name,
                    DataType::DataArray(DataSource::External),
                ))
            }
        }
    };
    let fallback = || -> Option<AssignmentData<'a>> {
        rest.iter().rev().find_map(|arg| {
            let name = get_literal_string(arg)?;
            if !is_variable_name(&name) {
                return None;
            }
            Some((
                b(base),
                b(arg),
                name,
                DataType::DataArray(DataSource::External),
            ))
        })
    };
    parse_args().or_else(fallback)
}

/// `getVariableForTestDashV`.
pub fn get_variable_for_test_dash_v(t: &Token) -> Option<String> {
    let s = get_literal_string_ext(t, &|x| match &x.inner {
        TGlob(s) => Some(s.clone()),
        _ => Some("\0".to_string()),
    })?;
    let s: String = s.chars().take_while(|&c| c != '[').collect();
    if is_variable_name(&s) { Some(s) } else { None }
}

/// `getReferencedVariables`.
pub fn get_referenced_variables<'a>(parents: &Tree<'a>, t: &'a Token) -> Vec<ReferenceData<'a>> {
    match &t.inner {
        TDollarBraced(_, l) => {
            let s = concat_oversimplify(l);
            let mut out = vec![(b(t), b(t), get_braced_reference(&s))];
            for x in get_index_references(&s)
                .into_iter()
                .chain(get_offset_references(&get_braced_modifier(&s)))
            {
                out.push((b(l), b(l), x));
            }
            out
        }
        TaVariable(name, _) => {
            let is_arithmetic_assignment = match parents.get(&t.id).map(|p| &p.inner) {
                Some(TaAssignment(op, lhs, _)) if op == "=" => **lhs == *t,
                _ => false,
            };
            if is_arithmetic_assignment {
                Vec::new()
            } else {
                vec![(b(t), b(t), name.clone())]
            }
        }
        TAssignment(mode, s, _, word) => {
            let mut out = Vec::new();
            if *mode == AssignmentMode::Append {
                out.push((b(t), b(t), s.clone()));
            }
            if ["PS1", "PS2", "PS3", "PS4", "PROMPT_COMMAND"].contains(&s.as_str()) {
                for x in get_variables_from_literal_token(word) {
                    out.push((b(t), b(t), x));
                }
            }
            out
        }
        TcUnary(_, op, token) if op == "-v" || op == "-R" => get_if_reference(t, token),
        TcBinary(ConditionType::DoubleBracket, op, lhs, rhs) => {
            if is_dereferencing_binary_op(op) {
                let mut out = get_if_reference(t, lhs);
                out.extend(get_if_reference(t, rhs));
                out
            } else {
                Vec::new()
            }
        }
        TBatsTest(..) => vec![
            (b(t), b(t), "lines".to_string()),
            (b(t), b(t), "status".to_string()),
            (b(t), b(t), "output".to_string()),
        ],
        TFdRedirect(var, op) if var.starts_with('{') => {
            if is_closing_file_op(op) {
                vec![(
                    b(t),
                    b(t),
                    var[1..].chars().take_while(|&c| c != '}').collect(),
                )]
            } else {
                Vec::new()
            }
        }
        _ => get_referenced_variable_command(t),
    }
}

fn get_if_reference<'a>(context: &'a Token, token: &'a Token) -> Vec<ReferenceData<'a>> {
    match get_variable_for_test_dash_v(token) {
        Some(s) => vec![(b(context), b(token), get_braced_reference(&s))],
        None => Vec::new(),
    }
}

pub fn is_dereferencing_binary_op(op: &str) -> bool {
    ["-eq", "-ne", "-lt", "-le", "-gt", "-ge"].contains(&op)
}

/// `dataTypeFrom defaultType v`.
pub fn data_type_from(array_default: bool, v: &Token) -> DataType<'_> {
    let is_array = matches!(v.inner, TArray(_));
    let src = DataSource::From(vec![b(v)]);
    if is_array || array_default {
        DataType::DataArray(src)
    } else {
        DataType::DataString(src)
    }
}

/// `isCommand`: also matches `/usr/bin/sed` for `sed`.
pub fn is_command(token: &Token, s: &str) -> bool {
    let suffix = format!("/{s}");
    is_command_match(token, &|cmd| cmd == s || cmd.ends_with(&suffix))
}

/// `isUnqualifiedCommand`.
pub fn is_unqualified_command(token: &Token, s: &str) -> bool {
    is_command_match(token, &|cmd| cmd == s)
}

pub fn is_command_match(token: &Token, matcher: &dyn Fn(&str) -> bool) -> bool {
    get_command_name(token).is_some_and(|c| matcher(&c))
}

/// `isConfusedGlobRegex`.
pub fn is_confused_glob_regex(s: &str) -> bool {
    let c: Vec<char> = s.chars().collect();
    match c.as_slice() {
        ['*', ..] => true,
        [x, '*'] => *x != '\\' && *x != '.',
        _ => false,
    }
}

pub fn get_variables_from_literal_token(token: &Token) -> Vec<String> {
    get_variables_from_literal(&get_literal_string_def(" ", token))
}

/// `getVariablesFromLiteral`.
pub fn get_variables_from_literal(s: &str) -> Vec<String> {
    VARIABLE_RE.with(|re| {
        re.match_all_subgroups(s)
            .into_iter()
            .map(|g| g[0].clone())
            .collect()
    })
}

/// `filterByAnnotation`.
pub fn filter_by_annotation(
    spec: &AnalysisSpec<'_>,
    params: &Parameters<'_>,
    comments: Vec<TokenComment>,
) -> Vec<TokenComment> {
    comments
        .into_iter()
        .filter(|note| {
            let code = note.comment.code;
            !get_parents_of_id(&params.parent_map, note.id)
                .iter()
                .any(|t| match t.inner {
                    TInclude(_) => !spec.check_sourced,
                    _ => is_annotation_ignoring_code(code, t),
                })
        })
        .collect()
}

/// `shouldIgnoreCode`.
pub fn should_ignore_code(params: &Parameters<'_>, code: i64, t: &Token) -> bool {
    is_annotation_ignoring_code(code, t)
        || get_parents_of_id(&params.parent_map, t.id)
            .iter()
            .any(|p| is_annotation_ignoring_code(code, p))
}

/// `isCountingReference`.
pub fn is_counting_reference(t: &Token) -> bool {
    match &t.inner {
        TDollarBraced(_, token) => concat_oversimplify(token).starts_with('#'),
        _ => false,
    }
}

/// `isQuotedAlternativeReference`.
pub fn is_quoted_alternative_reference(t: &Token) -> bool {
    match &t.inner {
        TDollarBraced(_, l) => {
            let m = get_braced_modifier(&concat_oversimplify(l));
            QUOTED_ALTERNATIVE_RE.with(|re| re.is_match(&m))
        }
        _ => false,
    }
}

pub const fn supports_arrays(shell: Shell) -> bool {
    matches!(shell, Shell::Bash | Shell::Ksh)
}

/// `isTrueAssignmentSource`.
pub const fn is_true_assignment_source(c: &DataType<'_>) -> bool {
    !matches!(
        c,
        DataType::DataString(DataSource::Checked | DataSource::Declaration)
            | DataType::DataArray(DataSource::Checked | DataSource::Declaration)
    )
}

/// `modifiesVariable`.
pub fn modifies_variable<'a>(params: &Parameters<'a>, token: &'a Token, name: &str) -> bool {
    get_variable_flow(params, token).iter().any(|t| match t {
        StackData::Assignment((_, _, n, source)) => is_true_assignment_source(source) && n == name,
        _ => false,
    })
}

/// `isTestCommand`.
pub fn is_test_command(t: &Token) -> bool {
    match &t.inner {
        TCondition(..) => true,
        TSimpleCommand(..) => is_command(t, "test"),
        TRedirecting(_, t) | TAnnotation(_, t) => is_test_command(t),
        TPipeline(_, cmds) if cmds.len() == 1 => is_test_command(&cmds[0]),
        _ => false,
    }
}

/// `whenShell`.
pub fn when_shell(params: &Parameters<'_>, shells: &[Shell]) -> bool {
    shells.contains(&params.shell_type)
}

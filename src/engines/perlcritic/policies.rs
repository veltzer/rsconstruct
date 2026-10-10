//! The severity-5 policies of `Perl::Critic` 1.156 (the core policies a
//! default profile enables), each ported from its module.

use std::collections::{HashMap, HashSet};

use super::document::CriticDoc;
use super::ppi::version::Version;
use super::ppi::{Class, DOCUMENT, Doc, Id, is_word, parse};
use super::utils::{
    Arg, Fatal, first_arg, is_function_call, is_hash_key, is_method_call, parse_arg_list,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Policy {
    ProhibitSleepViaSelect,
    ProhibitStringyEval,
    RequireGlobFunction,
    ProhibitOneArgBless,
    ProhibitMutatingListFunctions,
    ProhibitBarewordDirHandles,
    ProhibitBarewordFileHandles,
    ProhibitInteractiveTest,
    ProhibitTwoArgOpen,
    RequireEncodingWithUtf8Layer,
    ProhibitEvilModules,
    RequireBarewordIncludes,
    RequireFilenameMatchesPackage,
    ProhibitExplicitReturnUndef,
    ProhibitNestedSubs,
    ProhibitReturnSort,
    ProhibitSubroutinePrototypes,
    ProhibitNoStrict,
    RequireUseStrict,
    ProhibitLeadingZeros,
    ProhibitConditionalDeclarations,
    RequireLexicalLoopIterators,
}

/// `$EXPL`: a text, or pages of Perl Best Practices.
#[derive(Clone, Copy, Debug)]
pub enum Explanation {
    Text(&'static str),
    Pages(&'static [u32]),
}

impl Explanation {
    /// `Violation::explanation`.
    pub fn render(self) -> String {
        match self {
            Self::Text(text) => text.to_string(),
            Self::Pages(pages) => {
                let word = if pages.len() > 1 { "pages" } else { "page" };
                let list: Vec<String> = pages.iter().map(u32::to_string).collect();
                format!("See {word} {} of PBP", list.join(","))
            }
        }
    }
}

impl Policy {
    pub const ALL: [Self; 22] = [
        Self::ProhibitSleepViaSelect,
        Self::ProhibitStringyEval,
        Self::RequireGlobFunction,
        Self::ProhibitOneArgBless,
        Self::ProhibitMutatingListFunctions,
        Self::ProhibitBarewordDirHandles,
        Self::ProhibitBarewordFileHandles,
        Self::ProhibitInteractiveTest,
        Self::ProhibitTwoArgOpen,
        Self::RequireEncodingWithUtf8Layer,
        Self::ProhibitEvilModules,
        Self::RequireBarewordIncludes,
        Self::RequireFilenameMatchesPackage,
        Self::ProhibitExplicitReturnUndef,
        Self::ProhibitNestedSubs,
        Self::ProhibitReturnSort,
        Self::ProhibitSubroutinePrototypes,
        Self::ProhibitNoStrict,
        Self::RequireUseStrict,
        Self::ProhibitLeadingZeros,
        Self::ProhibitConditionalDeclarations,
        Self::RequireLexicalLoopIterators,
    ];

    /// The name without `Perl::Critic::Policy::` (`%p`).
    pub const fn short_name(self) -> &'static str {
        match self {
            Self::ProhibitSleepViaSelect => "BuiltinFunctions::ProhibitSleepViaSelect",
            Self::ProhibitStringyEval => "BuiltinFunctions::ProhibitStringyEval",
            Self::RequireGlobFunction => "BuiltinFunctions::RequireGlobFunction",
            Self::ProhibitOneArgBless => "ClassHierarchies::ProhibitOneArgBless",
            Self::ProhibitMutatingListFunctions => {
                "ControlStructures::ProhibitMutatingListFunctions"
            }
            Self::ProhibitBarewordDirHandles => "InputOutput::ProhibitBarewordDirHandles",
            Self::ProhibitBarewordFileHandles => "InputOutput::ProhibitBarewordFileHandles",
            Self::ProhibitInteractiveTest => "InputOutput::ProhibitInteractiveTest",
            Self::ProhibitTwoArgOpen => "InputOutput::ProhibitTwoArgOpen",
            Self::RequireEncodingWithUtf8Layer => "InputOutput::RequireEncodingWithUTF8Layer",
            Self::ProhibitEvilModules => "Modules::ProhibitEvilModules",
            Self::RequireBarewordIncludes => "Modules::RequireBarewordIncludes",
            Self::RequireFilenameMatchesPackage => "Modules::RequireFilenameMatchesPackage",
            Self::ProhibitExplicitReturnUndef => "Subroutines::ProhibitExplicitReturnUndef",
            Self::ProhibitNestedSubs => "Subroutines::ProhibitNestedSubs",
            Self::ProhibitReturnSort => "Subroutines::ProhibitReturnSort",
            Self::ProhibitSubroutinePrototypes => "Subroutines::ProhibitSubroutinePrototypes",
            Self::ProhibitNoStrict => "TestingAndDebugging::ProhibitNoStrict",
            Self::RequireUseStrict => "TestingAndDebugging::RequireUseStrict",
            Self::ProhibitLeadingZeros => "ValuesAndExpressions::ProhibitLeadingZeros",
            Self::ProhibitConditionalDeclarations => "Variables::ProhibitConditionalDeclarations",
            Self::RequireLexicalLoopIterators => "Variables::RequireLexicalLoopIterators",
        }
    }

    pub fn long_name(self) -> String {
        format!("Perl::Critic::Policy::{}", self.short_name())
    }

    pub fn from_short_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.short_name() == name)
    }

    /// `applies_to`.
    const fn applies_to(self) -> Class {
        match self {
            Self::RequireGlobFunction => Class::QuoteLikeReadline,
            Self::ProhibitInteractiveTest => Class::Operator,
            Self::ProhibitEvilModules | Self::RequireBarewordIncludes | Self::ProhibitNoStrict => {
                Class::StatementInclude
            }
            Self::RequireFilenameMatchesPackage | Self::RequireUseStrict => Class::Document,
            Self::ProhibitNestedSubs | Self::ProhibitSubroutinePrototypes => Class::StatementSub,
            Self::ProhibitLeadingZeros => Class::NumberOctal,
            Self::ProhibitConditionalDeclarations => Class::StatementVariable,
            Self::RequireLexicalLoopIterators => Class::StatementCompound,
            _ => Class::Word,
        }
    }

    pub const fn explanation(self) -> Explanation {
        match self {
            Self::ProhibitSleepViaSelect => Explanation::Pages(&[168]),
            Self::ProhibitStringyEval => Explanation::Pages(&[161]),
            Self::RequireGlobFunction => Explanation::Pages(&[167]),
            Self::ProhibitOneArgBless => Explanation::Pages(&[365]),
            Self::ProhibitMutatingListFunctions => Explanation::Pages(&[114]),
            Self::ProhibitBarewordDirHandles | Self::ProhibitBarewordFileHandles => {
                Explanation::Pages(&[202, 204])
            }
            Self::ProhibitInteractiveTest => Explanation::Pages(&[218]),
            Self::ProhibitTwoArgOpen => Explanation::Pages(&[207]),
            Self::RequireEncodingWithUtf8Layer => {
                Explanation::Text("Use \":encoding(UTF-8)\" to get strict validation")
            }
            Self::ProhibitEvilModules => Explanation::Text("Find an alternative module"),
            Self::RequireBarewordIncludes => Explanation::Text("Use a bareword instead"),
            Self::RequireFilenameMatchesPackage => {
                Explanation::Text("Correct the filename or package statement")
            }
            Self::ProhibitExplicitReturnUndef => Explanation::Pages(&[199]),
            Self::ProhibitNestedSubs => Explanation::Text(
                "Declaring a named sub inside another named sub does not prevent the inner sub from being global",
            ),
            Self::ProhibitReturnSort => {
                Explanation::Text("Behavior is undefined if called in scalar context")
            }
            Self::ProhibitSubroutinePrototypes => Explanation::Pages(&[194]),
            Self::ProhibitNoStrict | Self::RequireUseStrict => Explanation::Pages(&[429]),
            Self::ProhibitLeadingZeros => Explanation::Pages(&[58]),
            Self::ProhibitConditionalDeclarations => {
                Explanation::Text("Declare variables outside of the condition")
            }
            Self::RequireLexicalLoopIterators => Explanation::Pages(&[108]),
        }
    }

    /// The policy's own parameters (`supported_parameters`).
    pub const fn parameters(self) -> &'static [&'static str] {
        match self {
            Self::ProhibitStringyEval => &["allow_includes"],
            Self::ProhibitMutatingListFunctions => &["list_funcs", "add_list_funcs"],
            Self::ProhibitEvilModules => &["modules", "modules_file"],
            Self::ProhibitNoStrict => &["allow"],
            Self::RequireUseStrict => &["equivalent_modules"],
            Self::ProhibitLeadingZeros => &["strict"],
            _ => &[],
        }
    }

    /// `default_maximum_violations_per_document`.
    pub const fn default_maximum_violations(self) -> Option<usize> {
        match self {
            Self::RequireUseStrict => Some(1),
            _ => None,
        }
    }
}

/// A policy's parameter values.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub allow_includes: bool,
    pub list_funcs: HashSet<Vec<u8>>,
    pub evil_modules: HashMap<Vec<u8>, Vec<u8>>,
    pub evil_module_regexes: Vec<(regex::bytes::Regex, Vec<u8>)>,
    pub no_strict_allow: HashSet<Vec<u8>>,
    pub equivalent_modules: HashSet<Vec<u8>>,
    pub leading_zeros_strict: bool,
}

/// A violation before it is placed: its description and element.
pub struct Found {
    pub description: Vec<u8>,
    pub element: Id,
}

fn found(description: impl Into<Vec<u8>>, element: Id) -> Vec<Found> {
    vec![Found {
        description: description.into(),
        element,
    }]
}

/// `prepare_to_scan_document`.
pub fn prepare_to_scan_document(policy: Policy, critic: &CriticDoc) -> Result<bool, Fatal> {
    match policy {
        Policy::RequireFilenameMatchesPackage => Ok(critic.is_module()),
        Policy::RequireLexicalLoopIterators => {
            let version = critic.highest_explicit_perl_version()?;
            Ok(version.is_none_or(|v| v >= Version::decimal(5, 4)))
        }
        _ => Ok(true),
    }
}

/// The elements a policy examines, in order.
pub fn elements(policy: Policy, critic: &CriticDoc) -> Vec<Id> {
    match policy.applies_to() {
        Class::Document => vec![DOCUMENT],
        class => critic.find(class).to_vec(),
    }
}

/// `violates`.
pub fn violates(
    policy: Policy,
    settings: &Settings,
    critic: &CriticDoc,
    elem: Id,
) -> Result<Vec<Found>, Fatal> {
    let doc = &critic.doc;
    let text = || doc.content(elem);
    Ok(match policy {
        Policy::ProhibitSleepViaSelect => {
            if text() != b"select" || !is_function_call(doc, elem) {
                return Ok(Vec::new());
            }
            let arguments = parse_arg_list(doc, elem);
            if arguments.len() != 4 {
                return Ok(Vec::new());
            }
            for argument in &arguments[..3] {
                if !first_is(doc, argument, b"undef")? {
                    return Ok(Vec::new());
                }
            }
            if first_is(doc, &arguments[3], b"undef")? {
                return Ok(Vec::new());
            }
            found("\"select\" used to emulate \"sleep\"", elem)
        }
        Policy::ProhibitStringyEval => {
            if text() != b"eval" || !is_function_call(doc, elem) {
                return Ok(Vec::new());
            }
            let Some(argument) = first_arg(doc, elem) else {
                return Ok(Vec::new());
            };
            if doc.isa(argument, Class::StructureBlock) {
                return Ok(Vec::new());
            }
            if settings.allow_includes && string_eval_is_an_include(doc, argument)? {
                return Ok(Vec::new());
            }
            found("Expression form of \"eval\"", elem)
        }
        Policy::RequireGlobFunction => {
            if text().iter().any(|&c| c == b'*' || c == b'?') {
                found("Glob written as <...>", elem)
            } else {
                Vec::new()
            }
        }
        Policy::ProhibitOneArgBless => {
            if text() != b"bless" || !is_function_call(doc, elem) {
                return Ok(Vec::new());
            }
            if parse_arg_list(doc, elem).len() == 1 {
                found("One-argument \"bless\" used", elem)
            } else {
                Vec::new()
            }
        }
        Policy::ProhibitMutatingListFunctions => {
            if !settings.list_funcs.contains(&text()) || !is_function_call(doc, elem) {
                return Ok(Vec::new());
            }
            let Some(first) = first_arg(doc, elem) else {
                return Ok(Vec::new());
            };
            if !doc.isa(first, Class::StructureBlock) || !has_topic_side_effect(doc, first)? {
                return Ok(Vec::new());
            }
            found("Don't modify $_ in list functions", elem)
        }
        Policy::ProhibitBarewordDirHandles => {
            if text() != b"opendir" || !is_function_call(doc, elem) {
                return Ok(Vec::new());
            }
            let args = parse_arg_list(doc, elem);
            let Some(first) = args.first().filter(|a| a.defined()) else {
                return Ok(Vec::new());
            };
            let Some(mut token) = first.first()?.cloned() else {
                return Ok(Vec::new());
            };
            if token.isa(doc, Class::Word) && token.content(doc) == b"local" {
                let Some(next) = first.second()?.cloned() else {
                    return Ok(Vec::new());
                };
                token = next;
            }
            if token.isa(doc, Class::Cast) && token.content(doc) == b"\\" {
                let Some(next) = first.second()?.cloned() else {
                    return Ok(Vec::new());
                };
                token = next;
            }
            let content = token.content(doc);
            if token.isa(doc, Class::Symbol) {
                if content.first() == Some(&b'*') {
                    return Ok(found("Bareword dir handle opened", elem));
                }
            } else if token.isa(doc, Class::Word) && content != b"my" && content != b"our" {
                return Ok(found("Bareword dir handle opened", elem));
            }
            Vec::new()
        }
        Policy::ProhibitBarewordFileHandles => {
            if !matches!(text().as_slice(), b"open" | b"sysopen") || !is_function_call(doc, elem) {
                return Ok(Vec::new());
            }
            let args = parse_arg_list(doc, elem);
            let first_token = match args.first() {
                None | Some(Arg::Nodes(None)) => return Ok(Vec::new()),
                Some(Arg::Elem(e)) => {
                    if doc.class(*e) != Class::StatementScheduled {
                        return Ok(Vec::new());
                    }
                    doc.schild(*e, 0).map(super::utils::Node::Elem)
                }
                Some(arg @ Arg::Nodes(Some(_))) => arg.first()?.cloned(),
            };
            let Some(first_token) = first_token else {
                return Ok(Vec::new());
            };
            if first_token.isa(doc, Class::Word) {
                let content = first_token.content(doc);
                if content != b"my"
                    && !matches!(content.as_slice(), b"STDIN" | b"STDOUT" | b"STDERR")
                {
                    return Ok(found("Bareword file handle opened", elem));
                }
            }
            Vec::new()
        }
        Policy::ProhibitInteractiveTest => {
            if text() == b"-t" {
                found("Use IO::Interactive::is_interactive() instead of -t", elem)
            } else {
                Vec::new()
            }
        }
        Policy::ProhibitTwoArgOpen => {
            if text() != b"open" || !is_function_call(doc, elem) {
                return Ok(Vec::new());
            }
            let version = critic.highest_explicit_perl_version()?;
            if version.is_some_and(|v| v < Version::decimal(5, 6)) {
                return Ok(Vec::new());
            }
            let args = parse_arg_list(doc, elem);
            if args.len() > 2 {
                return Ok(Vec::new());
            }
            if args.len() == 2 {
                let Some(second) = args[1].first()? else {
                    return Err("Can't call method \"isa\" on an undefined value".to_string());
                };
                if second.isa(doc, Class::Quote)
                    && matches!(second.string(doc).as_slice(), b"-|" | b"|-")
                {
                    return Ok(Vec::new());
                }
            }
            found("Two-argument \"open\" used", elem)
        }
        Policy::RequireEncodingWithUtf8Layer => {
            let args = match text().as_slice() {
                b"binmode" | b"open" => parse_arg_list(doc, elem),
                _ => return Ok(Vec::new()),
            };
            let encoding = if text() == b"binmode" {
                args.get(1).and_then(|a| argument_string(doc, a))
            } else {
                if args.len() < 3 {
                    return Ok(Vec::new());
                }
                argument_string(doc, &args[1]).map(|s| strip_open_mode(&s))
            };
            let Some(encoding) = encoding.filter(|e| !e.is_empty() && e != b"0") else {
                return Ok(Vec::new());
            };
            if mentions_utf8_layer(&encoding) {
                found("I/O layer \":utf8\" used", elem)
            } else {
                Vec::new()
            }
        }
        Policy::ProhibitEvilModules => {
            let module = doc.include_module(elem);
            if module.is_empty() || module == b"0" {
                return Ok(Vec::new());
            }
            let description = if let Some(d) = settings.evil_modules.get(&module) {
                Some(d.clone())
            } else {
                settings
                    .evil_module_regexes
                    .iter()
                    .find(|(re, _)| re.is_match(&module))
                    .map(|(_, d)| d.clone())
            };
            match description {
                Some(d) => {
                    let d = if d.is_empty() || d == b"0" {
                        let mut s = b"Prohibited module \"".to_vec();
                        s.extend(&module);
                        s.extend(b"\" used");
                        s
                    } else {
                        d
                    };
                    found(d, elem)
                }
                None => Vec::new(),
            }
        }
        Policy::RequireBarewordIncludes => {
            let Some(child) = doc.schild(elem, 1) else {
                return Ok(Vec::new());
            };
            if !doc.isa(child, Class::Quote) {
                return Ok(Vec::new());
            }
            let mut description = b"\"".to_vec();
            description.extend(doc.include_type(elem).unwrap_or_default());
            description.extend(b"\" statement with library name as string");
            found(description, elem)
        }
        Policy::RequireFilenameMatchesPackage => filename_matches_package(critic),
        Policy::ProhibitExplicitReturnUndef | Policy::ProhibitReturnSort => {
            if text() != b"return" || is_hash_key(doc, elem) {
                return Ok(Vec::new());
            }
            let Some(sib) = doc.snext_sibling(elem) else {
                return Ok(Vec::new());
            };
            let (word, description): (&[u8], &str) = if policy == Policy::ProhibitReturnSort {
                (b"sort", "\"return\" statement followed by \"sort\"")
            } else {
                (b"undef", "\"return\" statement with explicit \"undef\"")
            };
            if doc.isa(sib, Class::Word) && doc.text(sib) == word {
                found(description, elem)
            } else {
                Vec::new()
            }
        }
        Policy::ProhibitNestedSubs => {
            if doc.isa(elem, Class::StatementScheduled)
                || doc.declaration_type(elem, false).is_some()
            {
                return Ok(Vec::new());
            }
            let mut outer = doc.parent(elem);
            while let Some(o) = outer {
                if doc.isa(o, Class::StatementSub) && !doc.isa(o, Class::StatementScheduled) {
                    return Ok(found("Nested named subroutine", elem));
                }
                outer = doc.parent(o);
            }
            Vec::new()
        }
        Policy::ProhibitSubroutinePrototypes => {
            let prototype = doc
                .children(elem)
                .iter()
                .copied()
                .find(|&c| doc.isa(c, Class::Prototype))
                .map(|p| {
                    let content = doc.text(p);
                    let content = content.strip_prefix(b"(").unwrap_or(content);
                    let content = content.strip_suffix(b")").unwrap_or(content);
                    content
                        .iter()
                        .copied()
                        .filter(|&c| !super::ppi::is_space(c))
                        .collect::<Vec<u8>>()
                });
            match prototype {
                Some(p) if !p.is_empty() && p != b"0" => found("Subroutine prototypes used", elem),
                _ => Vec::new(),
            }
        }
        Policy::ProhibitNoStrict => {
            if doc.include_type(elem).as_deref() != Some(b"no")
                || doc.include_pragma(elem) != b"strict"
            {
                return Ok(Vec::new());
            }
            let Some(statement) = doc.statement(elem) else {
                return Ok(Vec::new());
            };
            let content = doc.content(statement);
            let words: Vec<Vec<u8>> = content
                .split(|c| !c.is_ascii_lowercase())
                .filter(|w| !w.is_empty() && !matches!(*w, b"qw" | b"no" | b"strict"))
                .map(<[u8]>::to_vec)
                .collect();
            if !words.is_empty() && words.iter().all(|w| settings.no_strict_allow.contains(w)) {
                return Ok(Vec::new());
            }
            found("Stricture disabled", elem)
        }
        Policy::RequireUseStrict => require_use_strict(settings, critic),
        Policy::ProhibitLeadingZeros => {
            let content = text();
            if !has_leading_zero(&content) {
                return Ok(Vec::new());
            }
            let mut description = b"Integer with leading zeros: \"".to_vec();
            description.extend(&content);
            description.push(b'"');
            if settings.leading_zeros_strict {
                return Ok(found(description, elem));
            }
            if is_permission_argument(doc, elem) {
                return Ok(Vec::new());
            }
            found(description, elem)
        }
        Policy::ProhibitConditionalDeclarations => {
            if doc.declaration_type(elem, true).as_deref() == Some(b"local") {
                return Ok(Vec::new());
            }
            let conditional = doc.find_first(elem, |d, e| {
                if !matches!(
                    d.content(e).as_slice(),
                    b"if" | b"while" | b"foreach" | b"for" | b"until" | b"unless"
                ) {
                    return None;
                }
                if !d.isa(e, Class::Word) || is_hash_key(d, e) || is_method_call(d, e) {
                    return None;
                }
                Some(true)
            });
            if conditional.is_some() {
                found("Variable declared in conditional statement", elem)
            } else {
                Vec::new()
            }
        }
        Policy::RequireLexicalLoopIterators => {
            if doc.compound_type(elem)? != Some("foreach") {
                return Ok(Vec::new());
            }
            let Some(first) = doc.schild(elem, 0) else {
                return Ok(Vec::new());
            };
            let start = isize::from(doc.class(first) == Class::Label);
            let Some(scope) = doc.schild(elem, start + 1) else {
                return Ok(Vec::new());
            };
            if doc.isa(scope, Class::StructureList) || doc.is(scope, "my") {
                return Ok(Vec::new());
            }
            found("Loop iterator is not lexical", elem)
        }
    })
}

/// `$arg->[0] eq $word` (Perl dies for an argument that is not a list).
fn first_is(doc: &Doc, arg: &Arg, word: &[u8]) -> Result<bool, Fatal> {
    Ok(arg.first()?.is_some_and(|n| n.content(doc) == word))
}

/// `_string_eval_is_an_include`. A string PPI cannot parse leaves Perl
/// calling a method on undef: the policy dies.
fn string_eval_is_an_include(doc: &Doc, argument: Id) -> Result<bool, Fatal> {
    if !doc.isa(argument, Class::Quote) {
        return Ok(false);
    }
    let string = doc.quote_string(argument);
    let sub = parse(&string, b"")
        .map_err(|_| "Can't call method \"schildren\" on an undefined value".to_string())?;
    Ok(string_eval_statements_are_an_include(doc, argument, &sub))
}

fn string_eval_statements_are_an_include(doc: &Doc, argument: Id, sub: &Doc) -> bool {
    let statements = sub.schildren(DOCUMENT);
    if statements.len() > 3 {
        return false;
    }
    // ^(?:PPI::Statement::Package,)?PPI::Statement::Include(?:,PPI::Statement)?$
    // over the classes joined with commas: unanchored inside the names.
    let structure: Vec<&str> = statements.iter().map(|&s| sub.class(s).name()).collect();
    let joined = structure.join(",");
    let re = regex::Regex::new(
        r"^(?:PPI::Statement::Package,)?PPI::Statement::Include(?:,PPI::Statement)?$",
    )
    .expect("valid regex");
    if !re.is_match(&joined) {
        return false;
    }
    let is_q = matches!(
        doc.class(argument),
        Class::QuoteSingle | Class::QuoteLiteral | Class::Prototype
    );
    for statement in statements {
        let class = sub.class(statement);
        if class.isa(Class::StatementPackage) {
            if sub.parent(statement) != Some(DOCUMENT) {
                return false;
            }
        } else if class.isa(Class::StatementInclude) {
            if sub.include_type(statement).as_deref() == Some(b"no") {
                return false;
            }
            if is_q
                && !sub
                    .find(statement, |d, e| Some(d.isa(e, Class::Symbol)))
                    .is_empty()
            {
                return false;
            }
        } else {
            let components = sub.schildren(statement);
            if components.len() > 2 || !sub.isa(components[0], Class::Number) {
                return false;
            }
            if components.len() == 2 && sub.content(components[1]) != b";" {
                return false;
            }
        }
    }
    true
}

/// `_has_topic_side_effect`.
fn has_topic_side_effect(doc: &Doc, node: Id) -> Result<bool, Fatal> {
    let tokens = doc.find(node, |d, e| Some(d.isa(e, Class::Token)));
    for elem in tokens {
        if !doc.significant(elem) {
            continue;
        }
        if is_assignment_to_topic(doc, elem)
            || is_topic_mutating_regex(doc, elem)?
            || is_topic_mutating_func(doc, elem)
            || is_topic_mutating_substr(doc, elem)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_topic(doc: &Doc, elem: Option<Id>) -> bool {
    elem.is_some_and(|e| doc.isa(e, Class::Magic) && doc.text(e) == b"$_")
}

fn is_assignment_to_topic(doc: &Doc, elem: Id) -> bool {
    if !is_topic(doc, Some(elem)) {
        return false;
    }
    if let Some(sib) = doc.snext_sibling(elem)
        && doc.isa(sib, Class::Operator)
        && matches!(
            doc.content(sib).as_slice(),
            b"=" | b"*="
                | b"/="
                | b"+="
                | b"-="
                | b"%="
                | b"**="
                | b"x="
                | b".="
                | b"&="
                | b"|="
                | b"^="
                | b"&&="
                | b"||="
                | b"<<="
                | b">>="
                | b"//="
                | b"++"
                | b"--"
        )
    {
        return true;
    }
    doc.sprevious_sibling(elem).is_some_and(|psib| {
        doc.isa(psib, Class::Operator) && matches!(doc.content(psib).as_slice(), b"++" | b"--")
    })
}

fn is_topic_mutating_regex(doc: &Doc, elem: Id) -> Result<bool, Fatal> {
    let substitute = doc.isa(elem, Class::RegexpSubstitute);
    let transliterate = doc.isa(elem, Class::RegexpTransliterate);
    if !substitute && !transliterate {
        return Ok(false);
    }
    let modifiers = doc.modifiers(elem);
    if transliterate {
        let subs = doc.section_content(elem, 1).unwrap_or_default();
        if modifiers.contains(&b'r') {
            return Ok(false);
        }
        if subs.is_empty() {
            if !modifiers.contains(&b'd') && !modifiers.contains(&b's') {
                return Ok(false);
            }
        } else if doc.section_content(elem, 0).unwrap_or_default() == subs
            && !modifiers.contains(&b'c')
            && !modifiers.contains(&b's')
        {
            return Ok(false);
        }
    }
    // PPIx::Regexp's `modifier_asserted('r')`: a one-letter modifier is
    // asserted when it occurs exactly once among the trailing modifiers.
    if substitute {
        let content = doc.text(elem);
        let trailing = content.iter().rev().take_while(|c| c.is_ascii_alphabetic());
        if trailing.filter(|&&c| c == b'r').count() == 1 {
            return Ok(false);
        }
    }
    let Some(prevsib) = doc.sprevious_sibling(elem) else {
        return Ok(true);
    };
    if !matches!(doc.content(prevsib).as_slice(), b"=~" | b"!~") {
        return Ok(true);
    }
    // `_is_topic` calls `isa` on what is before the binding operator,
    // which is '' when nothing is: Perl dies.
    let Some(bound_to) = doc.sprevious_sibling(prevsib) else {
        return Err("Can't call method \"isa\" without a package or object reference".to_string());
    };
    Ok(is_topic(doc, Some(bound_to)))
}

fn is_topic_mutating_func(doc: &Doc, elem: Id) -> bool {
    if !doc.isa(elem, Class::Word) {
        return false;
    }
    let content = doc.text(elem);
    if !matches!(content, b"chop" | b"chomp" | b"undef") || !is_function_call(doc, elem) {
        return false;
    }
    match first_arg(doc, elem) {
        None => content != b"undef",
        Some(first) => is_topic(doc, Some(first)),
    }
}

fn is_topic_mutating_substr(doc: &Doc, elem: Id) -> Result<bool, Fatal> {
    if doc.content(elem) != b"substr" || !is_function_call(doc, elem) {
        return Ok(false);
    }
    let args = parse_arg_list(doc, elem);
    if args.len() < 4 {
        return Ok(false);
    }
    Ok(is_topic(
        doc,
        args[0].first()?.and_then(super::utils::Node::elem),
    ))
}

/// `_get_argument_string`.
fn argument_string(doc: &Doc, arg: &Arg) -> Option<Vec<u8>> {
    let Arg::Nodes(Some(nodes)) = arg else {
        return None;
    };
    if nodes.len() != 1 {
        return None;
    }
    nodes[0]
        .isa(doc, Class::Quote)
        .then(|| nodes[0].string(doc))
}

/// `$string =~ s/ [+]? (?: < | >{1,2} ) //smx`: the first mode marker.
fn strip_open_mode(s: &[u8]) -> Vec<u8> {
    for i in 0..s.len() {
        let mut j = i;
        if s[j] == b'+' {
            j += 1;
        }
        let len = match s.get(j) {
            Some(b'<') => 1,
            Some(b'>') => {
                if s.get(j + 1) == Some(&b'>') {
                    2
                } else {
                    1
                }
            }
            _ => 0,
        };
        if len > 0 {
            let mut out = s[..i].to_vec();
            out.extend_from_slice(&s[j + len..]);
            return out;
        }
    }
    s.to_vec()
}

/// `m/ (?: \A | : ) utf8 \b /smxi`.
fn mentions_utf8_layer(s: &[u8]) -> bool {
    let lower = s.to_ascii_lowercase();
    (0..lower.len()).any(|i| {
        (i == 0 || lower[i - 1] == b':')
            && lower[i..].starts_with(b"utf8")
            && lower.get(i + 4).is_none_or(|&c| !is_word(c))
    })
}

/// `\A [+-]? (?: 0+ _* )+ [1-9]`.
fn has_leading_zero(s: &[u8]) -> bool {
    let s = s
        .strip_prefix(b"+")
        .or_else(|| s.strip_prefix(b"-"))
        .unwrap_or(s);
    if s.first() != Some(&b'0') {
        return false;
    }
    let rest = s.iter().skip_while(|&&c| c == b'0' || c == b'_');
    rest.clone()
        .next()
        .is_some_and(|&c| (b'1'..=b'9').contains(&c))
}

/// The exemptions of `ProhibitLeadingZeros`: a mode argument of chmod,
/// umask, mkdir, mkfifo, dbmopen or sysopen.
fn is_permission_argument(doc: &Doc, elem: Id) -> bool {
    let previous = |e: Id| -> Option<Id> {
        let mut token = doc.previous_token(e);
        while let Some(t) = token {
            if doc.significant(t) && !matches!(doc.text(t), b"(" | b")") {
                break;
            }
            token = doc.previous_token(t);
        }
        token
    };
    let content = |e: Id| doc.content(e);
    // chmod / umask.
    if previous(elem).is_some_and(|p| matches!(content(p).as_slice(), b"chmod" | b"umask")) {
        return true;
    }
    // mkdir / mkfifo: second argument.
    let second_of = |names: &[&[u8]]| -> bool {
        let Some(p) = previous(elem) else {
            return false;
        };
        if content(p) != b"," {
            return false;
        }
        let Some(p) = previous(p) else { return false };
        let Some(p) = previous(p) else { return false };
        names.contains(&content(p).as_slice())
    };
    if second_of(&[b"mkdir"]) || second_of(&[b"mkfifo", b"POSIX::mkfifo"]) {
        return true;
    }
    // dbmopen: third argument.
    let third_of_dbmopen = || -> bool {
        let Some(p) = previous(elem) else {
            return false;
        };
        if content(p) != b"," {
            return false;
        }
        let Some(p) = previous(p) else { return false };
        let Some(p) = previous(p) else { return false };
        if content(p) != b"," {
            return false;
        }
        let Some(p) = previous(p) else { return false };
        let Some(p) = previous(p) else { return false };
        content(p) == b"dbmopen"
    };
    if third_of_dbmopen() {
        return true;
    }
    // sysopen: fourth argument.
    let fourth_of_sysopen = || -> bool {
        let Some(p) = previous(elem) else {
            return false;
        };
        if content(p) != b"," {
            return false;
        }
        let mut p = previous(p);
        while let Some(t) = p {
            if content(t) == b"," {
                break;
            }
            p = previous(t);
        }
        let Some(p) = p else { return false };
        if content(p) != b"," {
            return false;
        }
        let Some(p) = previous(p) else { return false };
        let Some(p) = previous(p) else { return false };
        if content(p) != b"," {
            return false;
        }
        let Some(p) = previous(p) else { return false };
        let Some(mut p) = previous(p) else {
            return false;
        };
        if content(p) == b"my" {
            let Some(q) = previous(p) else { return false };
            p = q;
        }
        content(p) == b"sysopen"
    };
    fourth_of_sysopen()
}

/// `RequireFilenameMatchesPackage`'s `violates`.
fn filename_matches_package(critic: &CriticDoc) -> Vec<Found> {
    let doc = &critic.doc;
    let Some(&package) = critic.find(Class::StatementPackage).first() else {
        return Vec::new();
    };
    let namespace = doc.package_namespace(package);
    if namespace == b"main" {
        return Vec::new();
    }
    let mut pkg_parts = split_package(&namespace);
    let filename = doc
        .location(package)
        .map(|l| doc.files[l.logical_file].clone())
        .filter(|f| !f.is_empty() && f != b"0")
        .unwrap_or_else(|| critic.filename.clone());
    if filename.is_empty() || filename == b"0" {
        return Vec::new();
    }
    let (dirs, file) = match filename.iter().rposition(|&c| c == b'/') {
        Some(slash) => (&filename[..=slash], &filename[slash + 1..]),
        None => (&b""[..], &filename[..]),
    };
    let stem = strip_extension(file);
    let mut path_parts: Vec<Vec<u8>> = dirs
        .split(|&c| c == b'/')
        .map(<[u8]>::to_vec)
        .chain(std::iter::once(stem))
        .filter(|p| !p.is_empty())
        .collect();
    let mut matched_any = false;
    while let (Some(pkg_part), Some(path_part)) =
        (pkg_parts.last().cloned(), path_parts.last().cloned())
    {
        pkg_parts.pop();
        path_parts.pop();
        if pkg_part == path_part {
            matched_any = true;
            continue;
        }
        if path_part.iter().any(|&c| !is_word(c)) {
            break;
        }
        return found("Package declaration must match filename", package);
    }
    if matched_any {
        return Vec::new();
    }
    found("Package declaration must match filename", package)
}

/// `split m/(?:\'|::)/xms` (trailing empty fields dropped).
fn split_package(namespace: &[u8]) -> Vec<Vec<u8>> {
    let mut parts = Vec::new();
    let mut current = Vec::new();
    let mut i = 0;
    while i < namespace.len() {
        if namespace[i] == b'\'' {
            parts.push(std::mem::take(&mut current));
            i += 1;
        } else if namespace[i..].starts_with(b"::") {
            parts.push(std::mem::take(&mut current));
            i += 2;
        } else {
            current.push(namespace[i]);
            i += 1;
        }
    }
    parts.push(current);
    while parts.last().is_some_and(Vec::is_empty) {
        parts.pop();
    }
    parts
}

/// `s/ [.] \w+ \z //xms`.
fn strip_extension(file: &[u8]) -> Vec<u8> {
    let word = file.iter().rev().take_while(|&&c| is_word(c)).count();
    if word > 0 && file.len() > word && file[file.len() - word - 1] == b'.' {
        return file[..file.len() - word - 1].to_vec();
    }
    file.to_vec()
}

/// `RequireUseStrict`'s `violates`.
fn require_use_strict(settings: &Settings, critic: &CriticDoc) -> Vec<Found> {
    let doc = &critic.doc;
    // A version that version.pm dies on ends the search: PPI's
    // `find_first` returns undef from its `eval`.
    let mut failed = false;
    let strict = doc.find_first(DOCUMENT, |d, e| {
        if !d.isa(e, Class::StatementInclude) || d.include_type(e).as_deref() != Some(b"use") {
            return Some(false);
        }
        if d.parent(e) != Some(DOCUMENT) {
            return Some(false);
        }
        let pragma = d.include_pragma(e);
        if !pragma.is_empty() && pragma != b"0" {
            return Some(settings.equivalent_modules.contains(&pragma));
        }
        let module = d.include_module(e);
        if !module.is_empty() && module != b"0" {
            return Some(settings.equivalent_modules.contains(&module));
        }
        let mut version = d.include_version(e);
        if !version.is_empty() && version != b"0" {
            // `tr/././ > 1`: two or more dots.
            if version.first() != Some(&b'v') && version.split(|&c| c == b'.').count() > 2 {
                version.insert(0, b'v');
            }
            if let Some(v) = Version::parse(&version) {
                return Some(v >= Version::dotted(&[5, 11, 0]));
            }
            failed = true;
            return Some(true);
        }
        Some(false)
    });
    let strict = strict.filter(|_| !failed);
    let strict_line = strict.and_then(|s| doc.location(s)).map(|l| l.line);
    let mut out = Vec::new();
    for &statement in critic.find(Class::Statement) {
        let class = doc.class(statement);
        if class.isa(Class::StatementPackage) || class.isa(Class::StatementInclude) {
            continue;
        }
        if class.isa(Class::StatementEnd) || class.isa(Class::StatementData) {
            break;
        }
        let line = doc.location(statement).map_or(0, |l| l.line);
        if strict_line.is_none_or(|s| line < s) {
            out.push(Found {
                description: b"Code before strictures are enabled".to_vec(),
                element: statement,
            });
        }
    }
    out
}

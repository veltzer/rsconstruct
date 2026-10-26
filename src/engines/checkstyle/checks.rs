//! The checks: ports of checkstyle 8.36.1's `TreeWalker` checks the
//! fleet's configuration enables, the javadoc tag scanning
//! `UnusedImports` relies on, and the walk that drives them.
//!
//! Each check sees the tree as checkstyle builds it (`super::ast`) and logs
//! as `AbstractCheck.log` does: the column is the node's, with tabs
//! expanded to `tabWidth`, plus one. A Java exception a check would throw
//! (an index past a line, a substring out of range, a null dereference)
//! is an `Err` with that exception's text; it aborts the file, as
//! checkstyle's `haltOnException` does.

// Token types are matched by the grammar's names (`LITERAL_return`).
#![allow(non_upper_case_globals)]

use std::collections::{HashMap, HashSet};

use regex::Regex;

use super::ast::{Ast, NodeId};
use super::config::{CheckKind, CheckModule};
use super::contents::{Comment, FileContents, is_blank, is_regex_space, substring};
use super::message::{Arg, Message};
use super::tables::{
    ABSTRACT, ANNOTATION_DEF, ANNOTATION_FIELD_DEF, ARRAY_DECLARATOR, ASSIGN, BAND_ASSIGN,
    BOR_ASSIGN, BSR_ASSIGN, BXOR_ASSIGN, CASE_GROUP, CLASS_DEF, COMPACT_CTOR_DEF, CTOR_DEF, DEC,
    DIV_ASSIGN, DO_WHILE, DOT, ELIST, EMPTY_STAT, ENUM_CONSTANT_DEF, ENUM_DEF, EQUAL, EXPR,
    FOR_EACH_CLAUSE, FOR_INIT, FOR_ITERATOR, IDENT, IMPORT, INC, INTERFACE_DEF, LAMBDA, LAND,
    LITERAL_break, LITERAL_case, LITERAL_catch, LITERAL_continue, LITERAL_default, LITERAL_do,
    LITERAL_false, LITERAL_finally, LITERAL_for, LITERAL_if, LITERAL_native, LITERAL_new,
    LITERAL_return, LITERAL_static, LITERAL_switch, LITERAL_synchronized, LITERAL_throw,
    LITERAL_true, LITERAL_try, LITERAL_while, LNOT, LOR, METHOD_DEF, MINUS_ASSIGN, MOD_ASSIGN,
    MODIFIERS, NOT_EQUAL, NUM_LONG, OBJBLOCK, PACKAGE_DEF, PARAMETER_DEF, PARAMETERS, PLUS_ASSIGN,
    POST_DEC, POST_INC, RCURLY, RECORD_DEF, RESOURCE_SPECIFICATION, RESOURCES, RPAREN, SEMI,
    SL_ASSIGN, SLIST, SR_ASSIGN, STAR_ASSIGN, STATIC_IMPORT, STRING_LITERAL, SWITCH_RULE,
    TEXT_BLOCK_LITERAL_BEGIN, TYPE, VARIABLE_DEF,
};

/// A Java exception, as its stack trace's first line prints it.
pub type Crash = String;

const NPE: &str = "java.lang.NullPointerException";

/// The file and tree a check runs over.
pub struct Ctx<'a> {
    pub ast: &'a Ast,
    pub contents: &'a FileContents,
}

impl Ctx<'_> {
    fn some(node: Option<NodeId>) -> Result<NodeId, Crash> {
        node.ok_or_else(|| NPE.to_string())
    }

    fn parent(&self, node: NodeId) -> Result<NodeId, Crash> {
        Self::some(self.ast.parent(node))
    }

    fn first_child(&self, node: NodeId) -> Result<NodeId, Crash> {
        Self::some(self.ast.first_child(node))
    }

    fn next_sibling(&self, node: NodeId) -> Result<NodeId, Crash> {
        Self::some(self.ast.next_sibling(node))
    }

    fn previous_sibling(&self, node: NodeId) -> Result<NodeId, Crash> {
        Self::some(self.ast.previous_sibling(node))
    }

    fn find(&self, node: NodeId, ty: u16) -> Option<NodeId> {
        self.ast.find_first_token(node, ty)
    }

    fn ty(&self, node: NodeId) -> u16 {
        self.ast.ty(node)
    }

    /// `FullIdent.createFullIdent`: the dotted name and its first leaf.
    fn full_ident(&self, node: Option<NodeId>) -> Result<(String, Option<NodeId>), Crash> {
        let mut text = String::new();
        let mut detail = None;
        self.extract_full_ident(node, &mut text, &mut detail)?;
        Ok((text, detail))
    }

    fn extract_full_ident(
        &self,
        node: Option<NodeId>,
        text: &mut String,
        detail: &mut Option<NodeId>,
    ) -> Result<(), Crash> {
        let Some(node) = node else {
            return Ok(());
        };
        match self.ty(node) {
            DOT => {
                let first = self.ast.first_child(node);
                self.extract_full_ident(first, text, detail)?;
                text.push('.');
                let second = self.ast.next_sibling(Self::some(first)?);
                self.extract_full_ident(second, text, detail)?;
            }
            ARRAY_DECLARATOR => {
                self.extract_full_ident(self.ast.first_child(node), text, detail)?;
                text.push_str("[]");
            }
            _ => {
                text.push_str(self.ast.text(node));
                if detail.is_none() {
                    *detail = Some(node);
                }
            }
        }
        Ok(())
    }

    /// `CommonUtil.lengthExpandedTabs(line, toIdx, tabWidth)`.
    fn expanded_column(&self, line_no: i32, col: i32, tab_width: i32) -> Result<i32, Crash> {
        let line = self.contents.line(i64::from(line_no) - 1)?;
        let mut len = 0;
        for idx in 0..col.max(0) {
            let Some(&c) = line.get(idx as usize) else {
                return Err(format!(
                    "java.lang.StringIndexOutOfBoundsException: Index {idx} out of bounds for length {}",
                    line.len()
                ));
            };
            if c == 0x09 {
                // The configuration refuses a non-positive `tabWidth`, so
                // this is unreachable; it stays the exception Java throws
                // rather than a Rust panic.
                if tab_width == 0 {
                    return Err("java.lang.ArithmeticException: / by zero".to_string());
                }
                len = (len / tab_width + 1) * tab_width;
            } else {
                len += 1;
            }
        }
        Ok(len)
    }
}

/// A violation as `LocalizedMessage` holds it before formatting.
#[derive(Clone, Debug)]
pub struct Violation {
    pub line: i32,
    /// 1-based, tabs expanded; 0 for a line-only violation.
    pub col: i32,
    pub key: &'static str,
    pub args: Vec<Arg>,
}

/// `CheckUtil.isEqualsMethod`.
fn is_equals_method(ctx: &Ctx, ast: NodeId) -> Result<bool, Crash> {
    if ctx.ty(ast) != METHOD_DEF {
        return Ok(false);
    }
    let modifiers = Ctx::some(ctx.find(ast, MODIFIERS))?;
    if ctx.find(modifiers, LITERAL_static).is_some() || ctx.find(modifiers, ABSTRACT).is_some() {
        return Ok(false);
    }
    let name = Ctx::some(ctx.find(ast, IDENT))?;
    if ctx.ast.text(name) != "equals" {
        return Ok(false);
    }
    let params = Ctx::some(ctx.find(ast, PARAMETERS))?;
    Ok(ctx.ast.child_count(params) == 1)
}

/// The parameter's type is `Object` or `java.lang.Object`.
fn is_object_param(ctx: &Ctx, param: NodeId) -> Result<bool, Crash> {
    let ty = Ctx::some(ctx.find(param, TYPE))?;
    let (name, _) = ctx.full_ident(ctx.ast.first_child(ty))?;
    Ok(name == "Object" || name == "java.lang.Object")
}

/// `String.substring(0, s.lastIndexOf('.'))`, which throws without a dot.
fn package_part(name: &str) -> Result<&str, Crash> {
    match name.rfind('.') {
        Some(i) => Ok(&name[..i]),
        None => Err(format!(
            "java.lang.StringIndexOutOfBoundsException: Range [0, -1) out of bounds for length {}",
            name.encode_utf16().count()
        )),
    }
}

// ---- javadoc tags (UnusedImports) ------------------------------------------

fn is_ascii_alpha(c: u16) -> bool {
    u8::try_from(c).is_ok_and(|b| b.is_ascii_alphabetic())
}

/// A valid javadoc tag that can name an imported type.
fn tag_references_imports(name: &str) -> bool {
    matches!(
        name,
        "see" | "link" | "value" | "linkplain" | "throws" | "exception"
    )
}

fn is_valid_tag(name: &str) -> bool {
    matches!(
        name,
        "author"
            | "code"
            | "docRoot"
            | "deprecated"
            | "exception"
            | "inheritDoc"
            | "link"
            | "linkplain"
            | "literal"
            | "param"
            | "return"
            | "see"
            | "serial"
            | "serialData"
            | "serialField"
            | "since"
            | "throws"
            | "value"
            | "version"
    )
}

/// Java's `String.trim()`: code units up to `' '` at both ends.
fn java_trim(s: &[u16]) -> &[u16] {
    let start = s.iter().position(|&c| c > 0x20).unwrap_or(s.len());
    let end = s.iter().rposition(|&c| c > 0x20).map_or(start, |e| e + 1);
    &s[start..end.max(start)]
}

/// `BlockTagUtil.extractBlockTags`: `(name, value)` of `@tag value` lines.
fn block_tags(lines: &[Vec<u16>]) -> Vec<(String, Vec<u16>)> {
    let mut tags = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let found = if i == 0 {
            // /\*{2,}\s*@(\p{Alpha}+)\s  (find)
            (0..line.len()).find_map(|start| {
                let rest = &line[start..];
                if !rest.starts_with(&[0x2F, 0x2A, 0x2A]) {
                    return None;
                }
                let mut p = 3;
                while rest.get(p) == Some(&0x2A) {
                    p += 1;
                }
                while rest.get(p).copied().is_some_and(is_regex_space) {
                    p += 1;
                }
                tag_at(rest, p).map(|(name_start, name_end)| (start + name_start, start + name_end))
            })
        } else {
            // ^\s*\**\s*@(\p{Alpha}+)\s
            let mut p = 0;
            while line.get(p).copied().is_some_and(is_regex_space) {
                p += 1;
            }
            while line.get(p) == Some(&0x2A) {
                p += 1;
            }
            while line.get(p).copied().is_some_and(is_regex_space) {
                p += 1;
            }
            tag_at(line, p)
        };
        if let Some((name_start, name_end)) = found {
            let name = String::from_utf16_lossy(&line[name_start..name_end]);
            let mut value = java_trim(&line[name_end..]).to_vec();
            if value.ends_with(&[0x2A, 0x2F]) {
                let end = value.len() - 2;
                value = java_trim(&value[..end]).to_vec();
            }
            tags.push((name, value));
        }
    }
    tags
}

/// `@(\p{Alpha}+)\s` at `p`: the name's range. The name run is greedy but
/// may give back letters so that a blank follows; a blank can only follow
/// the whole run, so the run must end at a blank.
fn tag_at(s: &[u16], p: usize) -> Option<(usize, usize)> {
    if s.get(p) != Some(&0x40) {
        return None;
    }
    let start = p + 1;
    let mut end = start;
    while s.get(end).copied().is_some_and(is_ascii_alpha) {
        end += 1;
    }
    (end > start && s.get(end).copied().is_some_and(is_regex_space)).then_some((start, end))
}

/// `InlineTagUtil.extractInlineTags`: `(name, value)` of `{@tag value}`
/// across the comment's lines joined with `\n`.
fn inline_tags(lines: &[Vec<u16>]) -> Vec<(String, Vec<u16>)> {
    let mut text: Vec<u16> = Vec::new();
    for line in lines {
        text.extend_from_slice(line);
        text.push(0x0A);
    }
    let mut tags = Vec::new();
    let mut from = 0;
    // \{@(\p{Alpha}+)\b(.*?)}  with DOTALL, find() repeatedly.
    while from < text.len() {
        let Some(open) = (from..text.len().saturating_sub(1))
            .find(|&i| text[i] == 0x7B && text[i + 1] == 0x40 && inline_tag_at(&text, i).is_some())
        else {
            break;
        };
        let (name_end, close) = inline_tag_at(&text, open).unwrap_or((open, open));
        let name = String::from_utf16_lossy(&text[open + 2..name_end]);
        let value = collapse_whitespace(&remove_leading_javadoc(&text[name_end..close]));
        tags.push((name, value));
        from = close + 1;
    }
    tags
}

/// At a `{@`: the end of the tag name and the index of the closing `}`.
/// `\p{Alpha}+` backtracks until `\b` holds (an ASCII word boundary), then
/// the lazy `(.*?)` runs to the first `}`.
fn inline_tag_at(text: &[u16], open: usize) -> Option<(usize, usize)> {
    let start = open + 2;
    let mut end = start;
    while text.get(end).copied().is_some_and(is_ascii_alpha) {
        end += 1;
    }
    let word = |c: Option<&u16>| {
        c.copied()
            .is_some_and(|c| u8::try_from(c).is_ok_and(|b| b.is_ascii_alphanumeric() || b == b'_'))
    };
    let close = |from: usize| (from..text.len()).find(|&i| text[i] == 0x7D);
    let mut name_end = end;
    while name_end > start {
        // \b between the last name letter (a word character) and what follows.
        if !word(text.get(name_end)) {
            if let Some(c) = close(name_end) {
                return Some((name_end, c));
            }
            return None;
        }
        name_end -= 1;
    }
    None
}

/// `^\s*\*` removed at the start of every line (MULTILINE: after `\n`,
/// `\r\n`, `\r`, U+0085, U+2028 and U+2029).
fn remove_leading_javadoc(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    let mut line_start = true;
    while i < s.len() {
        if line_start {
            let mut p = i;
            while p < s.len() && is_regex_space(s[p]) {
                p += 1;
            }
            if s.get(p) == Some(&0x2A) {
                i = p + 1;
                line_start = false;
                continue;
            }
        }
        let c = s[i];
        out.push(c);
        line_start = matches!(c, 0x0A | 0x0D | 0x85 | 0x2028 | 0x2029)
            && !(c == 0x0D && s.get(i + 1) == Some(&0x0A));
        i += 1;
    }
    out
}

/// `\s+` replaced by one space, then `trim()`.
fn collapse_whitespace(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    let mut in_space = false;
    for &c in s {
        if is_regex_space(c) {
            if !in_space {
                out.push(0x20);
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    java_trim(&out).to_vec()
}

/// `((:?[\p{L}_$][\p{L}\p{N}_$]*\.)*[\p{L}_$][\p{L}\p{N}_$]*)`, the class
/// name pattern (the `(:?` is a typo for `(?:` that makes `:` optional).
fn class_name_regex() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^((:?[\p{L}_$][\p{L}\p{N}_$]*\.)*[\p{L}_$][\p{L}\p{N}_$]*)")
            .expect("class name pattern")
    })
}

fn argument_regex() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"[(,][\t\n\x0B\x0C\r ]*((:?[\p{L}_$][\p{L}\p{N}_$]*\.)*[\p{L}_$][\p{L}\p{N}_$]*)",
        )
        .expect("argument pattern")
    })
}

fn top_level_type(name: &str) -> String {
    name.split('.').next().unwrap_or(name).to_string()
}

/// The type names a javadoc comment's `@see`, `{@link}`, ... tags refer to.
fn javadoc_references(comment: &Comment) -> HashSet<String> {
    let mut references = HashSet::new();
    let tags = inline_tags(&comment.text)
        .into_iter()
        .chain(block_tags(&comment.text));
    for (name, value) in tags {
        if !is_valid_tag(&name) || !tag_references_imports(&name) {
            continue;
        }
        let identifier = String::from_utf16_lossy(java_trim(&value));
        if let Some(m) = class_name_regex()
            .captures(&identifier)
            .and_then(|c| c.get(1))
        {
            references.insert(top_level_type(m.as_str()));
        }
        for c in argument_regex().captures_iter(&identifier) {
            if let Some(m) = c.get(1) {
                references.insert(top_level_type(m.as_str()));
            }
        }
    }
    references
}

// ---- the checks -----------------------------------------------------------------

/// Per-file state of the checks that keep any.
enum State {
    Stateless,
    RedundantImport {
        pkg_name: Option<String>,
        imports: Vec<(String, i32)>,
        static_imports: Vec<(String, i32)>,
    },
    EqualsHashCode {
        with_equals: HashMap<NodeId, NodeId>,
        with_hash_code: HashMap<NodeId, NodeId>,
    },
    UnusedImports {
        collect: bool,
        imports: Vec<(String, NodeId)>,
        referenced: HashSet<String>,
    },
    ModifiedControlVariable {
        stack: Vec<Vec<String>>,
    },
    OneStatementPerLine {
        count_of_semi_in_lambda: Vec<i32>,
        last_statement_end: i32,
        for_statement_end: i32,
        in_for_header: bool,
        is_in_lambda: bool,
        lambda_statement_end: i32,
        last_variable_resource_statement_end: i32,
    },
}

/// One configured check, with its state and messages for the current file.
pub struct Check<'c> {
    pub module: &'c CheckModule,
    state: State,
    pub violations: Vec<Violation>,
}

/// The token types a check is registered for (its default tokens).
pub const fn tokens(kind: &CheckKind) -> &'static [u16] {
    match kind {
        CheckKind::ArrayTypeStyle { .. } => &[ARRAY_DECLARATOR],
        CheckKind::CovariantEquals => &[CLASS_DEF, LITERAL_new, ENUM_DEF, RECORD_DEF],
        CheckKind::DefaultComesLast { .. } => &[LITERAL_default],
        CheckKind::EmptyStatement => &[EMPTY_STAT],
        CheckKind::EqualsHashCode => &[METHOD_DEF],
        CheckKind::FallThrough { .. } => &[CASE_GROUP],
        CheckKind::ModifiedControlVariable { .. } => &[
            OBJBLOCK,
            LITERAL_for,
            FOR_ITERATOR,
            FOR_EACH_CLAUSE,
            ASSIGN,
            PLUS_ASSIGN,
            MINUS_ASSIGN,
            STAR_ASSIGN,
            DIV_ASSIGN,
            MOD_ASSIGN,
            SR_ASSIGN,
            BSR_ASSIGN,
            SL_ASSIGN,
            BAND_ASSIGN,
            BXOR_ASSIGN,
            BOR_ASSIGN,
            INC,
            POST_INC,
            DEC,
            POST_DEC,
        ],
        CheckKind::OneStatementPerLine { .. } => &[SEMI, FOR_INIT, FOR_ITERATOR, LAMBDA],
        CheckKind::RedundantImport => &[IMPORT, STATIC_IMPORT, PACKAGE_DEF],
        CheckKind::SimplifyBooleanExpression => &[LITERAL_true, LITERAL_false],
        CheckKind::SimplifyBooleanReturn => &[LITERAL_if],
        CheckKind::StringLiteralEquality => &[EQUAL, NOT_EQUAL],
        CheckKind::UnusedImports { .. } => &[
            IDENT,
            IMPORT,
            STATIC_IMPORT,
            PACKAGE_DEF,
            ANNOTATION_DEF,
            ANNOTATION_FIELD_DEF,
            ENUM_DEF,
            ENUM_CONSTANT_DEF,
            CLASS_DEF,
            INTERFACE_DEF,
            METHOD_DEF,
            CTOR_DEF,
            VARIABLE_DEF,
            RECORD_DEF,
            COMPACT_CTOR_DEF,
        ],
        CheckKind::UpperEll => &[NUM_LONG],
    }
}

const MUTATION_OPERATIONS: [u16; 5] = [POST_INC, POST_DEC, DEC, INC, ASSIGN];

impl<'c> Check<'c> {
    pub const fn new(module: &'c CheckModule) -> Self {
        Self {
            module,
            state: State::Stateless,
            violations: Vec::new(),
        }
    }

    fn log(
        &mut self,
        ctx: &Ctx,
        node: NodeId,
        key: &'static str,
        args: Vec<Arg>,
    ) -> Result<(), Crash> {
        let line = ctx.ast.line(node);
        let col =
            1 + ctx.expanded_column(line, ctx.ast.column(node), self.module.common.tab_width)?;
        self.violations.push(Violation {
            line,
            col,
            key,
            args,
        });
        Ok(())
    }

    /// `beginTree`.
    pub fn begin_tree(&mut self) {
        self.violations.clear();
        self.state = match &self.module.kind {
            CheckKind::RedundantImport => State::RedundantImport {
                pkg_name: None,
                imports: Vec::new(),
                static_imports: Vec::new(),
            },
            CheckKind::EqualsHashCode => State::EqualsHashCode {
                with_equals: HashMap::new(),
                with_hash_code: HashMap::new(),
            },
            CheckKind::UnusedImports { .. } => State::UnusedImports {
                collect: false,
                imports: Vec::new(),
                referenced: HashSet::new(),
            },
            CheckKind::ModifiedControlVariable { .. } => {
                State::ModifiedControlVariable { stack: Vec::new() }
            }
            CheckKind::OneStatementPerLine { .. } => State::OneStatementPerLine {
                count_of_semi_in_lambda: Vec::new(),
                last_statement_end: -1,
                for_statement_end: -1,
                in_for_header: false,
                is_in_lambda: false,
                lambda_statement_end: -1,
                last_variable_resource_statement_end: -1,
            },
            _ => State::Stateless,
        };
    }

    /// `visitToken`.
    pub fn visit(&mut self, ctx: &Ctx, ast: NodeId) -> Result<(), Crash> {
        match &self.module.kind {
            CheckKind::EmptyStatement => self.log(ctx, ast, "empty.statement", vec![]),
            CheckKind::UpperEll => {
                if ctx.ast.text(ast).ends_with('l') {
                    self.log(ctx, ast, "upperEll", vec![])?;
                }
                Ok(())
            }
            CheckKind::ArrayTypeStyle { java_style } => {
                self.array_type_style(ctx, ast, *java_style)
            }
            CheckKind::StringLiteralEquality => {
                if ctx.find(ast, STRING_LITERAL).is_some()
                    || ctx.find(ast, TEXT_BLOCK_LITERAL_BEGIN).is_some()
                {
                    let text = ctx.ast.text(ast).to_string();
                    self.log(ctx, ast, "string.literal.equality", vec![Arg::Str(text)])?;
                }
                Ok(())
            }
            CheckKind::SimplifyBooleanExpression => {
                let parent = ctx.parent(ast)?;
                if matches!(ctx.ty(parent), NOT_EQUAL | EQUAL | LNOT | LOR | LAND) {
                    self.log(ctx, parent, "simplify.expression", vec![])?;
                }
                Ok(())
            }
            CheckKind::SimplifyBooleanReturn => self.simplify_boolean_return(ctx, ast),
            CheckKind::RedundantImport => self.redundant_import(ctx, ast),
            CheckKind::CovariantEquals => self.covariant_equals(ctx, ast),
            CheckKind::DefaultComesLast {
                skip_if_last_and_shared_with_case,
            } => self.default_comes_last(ctx, ast, *skip_if_last_and_shared_with_case),
            CheckKind::EqualsHashCode => self.equals_hash_code_visit(ctx, ast),
            CheckKind::FallThrough {
                check_last_case_group,
                relief_pattern,
            } => self.fall_through(ctx, ast, *check_last_case_group, relief_pattern),
            CheckKind::ModifiedControlVariable { .. } => {
                self.modified_control_variable_visit(ctx, ast)
            }
            CheckKind::OneStatementPerLine {
                treat_try_resources_as_statement,
            } => self.one_statement_visit(ctx, ast, *treat_try_resources_as_statement),
            CheckKind::UnusedImports { process_javadoc } => {
                self.unused_imports_visit(ctx, ast, *process_javadoc)
            }
        }
    }

    /// `leaveToken`.
    pub fn leave(&mut self, ctx: &Ctx, ast: NodeId) -> Result<(), Crash> {
        match &self.module.kind {
            CheckKind::ModifiedControlVariable {
                skip_enhanced_for_loop_variable,
            } => self.modified_control_variable_leave(ctx, ast, *skip_enhanced_for_loop_variable),
            CheckKind::OneStatementPerLine { .. } => {
                self.one_statement_leave(ctx, ast);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// `finishTree`.
    pub fn finish_tree(&mut self, ctx: &Ctx) -> Result<(), Crash> {
        match std::mem::replace(&mut self.state, State::Stateless) {
            State::EqualsHashCode {
                with_equals,
                mut with_hash_code,
            } => {
                for (block, equals) in with_equals {
                    if with_hash_code.remove(&block).is_none() {
                        self.log(ctx, equals, "equals.noHashCode", vec![])?;
                    }
                }
                for hash_code in with_hash_code.into_values() {
                    self.log(ctx, hash_code, "equals.noEquals", vec![])?;
                }
            }
            State::UnusedImports {
                imports,
                referenced,
                ..
            } => {
                for (name, detail) in imports {
                    let base = name.rsplit('.').next().unwrap_or(&name);
                    if !referenced.contains(base) || is_java_lang_type(&name) {
                        self.log(ctx, detail, "import.unused", vec![Arg::Str(name.clone())])?;
                    }
                }
            }
            state @ State::OneStatementPerLine { .. } => self.state = state,
            _ => {}
        }
        Ok(())
    }

    fn array_type_style(&mut self, ctx: &Ctx, ast: NodeId, java_style: bool) -> Result<(), Crash> {
        let type_ast = ctx.parent(ast)?;
        if ctx.ty(type_ast) != TYPE {
            return Ok(());
        }
        let Some(variable) = ctx.ast.next_sibling(type_ast) else {
            return Ok(());
        };
        let is_method = ctx.ty(ctx.parent(type_ast)?) == METHOD_DEF;
        let is_java_style = ctx.ast.line(variable) > ctx.ast.line(ast)
            || ctx.ast.column(variable) - ctx.ast.column(ast) > -1;
        let method_violation = is_method && !is_java_style;
        let variable_violation = !is_method && is_java_style != java_style;
        if method_violation || variable_violation {
            self.log(ctx, ast, "array.type.style", vec![])?;
        }
        Ok(())
    }

    fn simplify_boolean_return(&mut self, ctx: &Ctx, ast: NodeId) -> Result<(), Crash> {
        let Some(else_literal) = ctx.find(ast, super::tables::LITERAL_else) else {
            return Ok(());
        };
        let else_statement = ctx.ast.first_child(else_literal);
        let condition = ctx.next_sibling(ctx.first_child(ast)?)?;
        let then_statement = ctx.ast.next_sibling(ctx.next_sibling(condition)?);
        if can_return_only_boolean_literal(ctx, then_statement)?
            && can_return_only_boolean_literal(ctx, else_statement)?
        {
            self.log(ctx, ast, "simplify.boolReturn", vec![])?;
        }
        Ok(())
    }

    fn redundant_import(&mut self, ctx: &Ctx, ast: NodeId) -> Result<(), Crash> {
        let ty = ctx.ty(ast);
        let full_last_but_one = |ctx: &Ctx| -> Result<(String, Option<NodeId>), Crash> {
            let last = Ctx::some(ctx.ast.last_child(ast))?;
            ctx.full_ident(ctx.ast.previous_sibling(last))
        };
        if ty == PACKAGE_DEF {
            let (name, _) = full_last_but_one(ctx)?;
            if let State::RedundantImport { pkg_name, .. } = &mut self.state {
                *pkg_name = Some(name);
            }
            return Ok(());
        }
        if ty == IMPORT {
            let (name, detail) = ctx.full_ident(ctx.ast.first_child(ast))?;
            let line = ctx.ast.line(Ctx::some(detail)?);
            let pkg = match &self.state {
                State::RedundantImport { pkg_name, .. } => pkg_name.clone(),
                _ => None,
            };
            if package_part(&name)? == "java.lang" {
                self.log(ctx, ast, "import.lang", vec![Arg::Str(name.clone())])?;
            } else if let Some(pkg) = pkg
                && package_part(&name)? == pkg
            {
                self.log(ctx, ast, "import.same", vec![Arg::Str(name.clone())])?;
            }
            let earlier: Vec<i32> = match &self.state {
                State::RedundantImport { imports, .. } => imports
                    .iter()
                    .filter(|(n, _)| *n == name)
                    .map(|(_, l)| *l)
                    .collect(),
                _ => Vec::new(),
            };
            for l in earlier {
                self.log(
                    ctx,
                    ast,
                    "import.duplicate",
                    vec![Arg::Int(l), Arg::Str(name.clone())],
                )?;
            }
            if let State::RedundantImport { imports, .. } = &mut self.state {
                imports.push((name, line));
            }
        } else {
            let (name, detail) = full_last_but_one(ctx)?;
            let line = ctx.ast.line(Ctx::some(detail)?);
            let earlier: Vec<i32> = match &self.state {
                State::RedundantImport { static_imports, .. } => static_imports
                    .iter()
                    .filter(|(n, _)| *n == name)
                    .map(|(_, l)| *l)
                    .collect(),
                _ => Vec::new(),
            };
            for l in earlier {
                self.log(
                    ctx,
                    ast,
                    "import.duplicate",
                    vec![Arg::Int(l), Arg::Str(name.clone())],
                )?;
            }
            if let State::RedundantImport { static_imports, .. } = &mut self.state {
                static_imports.push((name, line));
            }
        }
        Ok(())
    }

    fn covariant_equals(&mut self, ctx: &Ctx, ast: NodeId) -> Result<(), Crash> {
        let Some(obj_block) = ctx.find(ast, OBJBLOCK) else {
            return Ok(());
        };
        let mut equals_methods = Vec::new();
        let mut has_equals_object = false;
        for child in ctx.ast.children(obj_block) {
            if is_equals_method(ctx, child)? {
                let params = Ctx::some(ctx.find(child, PARAMETERS))?;
                let param = Ctx::some(ctx.find(params, PARAMETER_DEF))?;
                if is_object_param(ctx, param)? {
                    has_equals_object = true;
                } else {
                    equals_methods.push(child);
                }
            }
        }
        if !has_equals_object {
            for method in equals_methods {
                let name = Ctx::some(ctx.find(method, IDENT))?;
                self.log(ctx, name, "covariant.equals", vec![])?;
            }
        }
        Ok(())
    }

    fn default_comes_last(
        &mut self,
        ctx: &Ctx,
        ast: NodeId,
        skip_if_last_and_shared_with_case: bool,
    ) -> Result<(), Crash> {
        let group = ctx.parent(ast)?;
        let is_switch_rule = ctx.ty(group) == SWITCH_RULE;
        let next_of = |node: NodeId, ty: u16| {
            std::iter::successors(ctx.ast.next_sibling(node), |&n| ctx.ast.next_sibling(n))
                .find(|&n| ctx.ty(n) == ty)
        };
        if skip_if_last_and_shared_with_case && !is_switch_rule {
            if next_of(ast, LITERAL_case).is_some() {
                self.log(ctx, ast, "default.comes.last.in.casegroup", vec![])?;
            } else if ctx.ast.previous_sibling(ast).is_none()
                && next_of(group, CASE_GROUP).is_some()
            {
                self.log(ctx, ast, "default.comes.last", vec![])?;
            }
        } else if next_of(group, CASE_GROUP).is_some() || next_of(group, SWITCH_RULE).is_some() {
            self.log(ctx, ast, "default.comes.last", vec![])?;
        }
        Ok(())
    }

    fn equals_hash_code_visit(&mut self, ctx: &Ctx, ast: NodeId) -> Result<(), Crash> {
        let modifiers = ctx.first_child(ast)?;
        let has_body =
            ctx.find(ast, SLIST).is_some() || ctx.find(modifiers, LITERAL_native).is_some();
        let parameters = Ctx::some(ctx.find(ast, PARAMETERS))?;
        let is_equals = is_equals_method(ctx, ast)?
            && {
                let first = ctx.first_child(parameters)?;
                is_object_param(ctx, first)?
            }
            && has_body;
        let parent = ctx.parent(ast)?;
        if let State::EqualsHashCode {
            with_equals,
            with_hash_code,
        } = &mut self.state
        {
            if is_equals {
                with_equals.insert(parent, ast);
            } else {
                let name = Ctx::some(ctx.find(ast, IDENT))?;
                if ctx.ast.text(name) == "hashCode"
                    && ctx.ast.first_child(parameters).is_none()
                    && has_body
                {
                    with_hash_code.insert(parent, ast);
                }
            }
        }
        Ok(())
    }

    fn fall_through(
        &mut self,
        ctx: &Ctx,
        ast: NodeId,
        check_last_case_group: bool,
        relief: &Regex,
    ) -> Result<(), Crash> {
        let next_group = ctx.next_sibling(ast)?;
        let is_last_group = ctx.ty(next_group) != CASE_GROUP;
        if is_last_group && !check_last_case_group {
            return Ok(());
        }
        let Some(slist) = ctx.find(ast, SLIST) else {
            return Ok(());
        };
        if is_terminated(ctx, slist, true, true)?
            || has_fall_through_comment(ctx, ast, next_group, relief)?
        {
            return Ok(());
        }
        if is_last_group {
            self.log(ctx, ast, "fall.through.last", vec![])
        } else {
            self.log(ctx, next_group, "fall.through", vec![])
        }
    }

    fn modified_control_variable_visit(&mut self, ctx: &Ctx, ast: NodeId) -> Result<(), Crash> {
        let ty = ctx.ty(ast);
        let State::ModifiedControlVariable { stack } = &mut self.state else {
            return Ok(());
        };
        if ty == OBJBLOCK {
            stack.push(Vec::new());
            return Ok(());
        }
        if matches!(ty, LITERAL_for | FOR_ITERATOR | FOR_EACH_CLAUSE) {
            return Ok(());
        }
        let current = stack.last();
        let Some(ident) = ctx.ast.first_child(ast) else {
            return Ok(());
        };
        if ctx.ty(ident) != IDENT {
            return Ok(());
        }
        let Some(current) = current else {
            return Err(NPE.to_string());
        };
        let name = ctx.ast.text(ident).to_string();
        if current.contains(&name) {
            self.log(ctx, ast, "modified.control.variable", vec![Arg::Str(name)])?;
        }
        Ok(())
    }

    fn modified_control_variable_leave(
        &mut self,
        ctx: &Ctx,
        ast: NodeId,
        skip_enhanced: bool,
    ) -> Result<(), Crash> {
        let ty = ctx.ty(ast);
        let pushes: Vec<String> = match ty {
            FOR_ITERATOR => variables_managed_by_for_loop(ctx, ctx.parent(ast)?)?
                .into_iter()
                .collect(),
            FOR_EACH_CLAUSE if !skip_enhanced => {
                let def = Ctx::some(ctx.find(ast, VARIABLE_DEF))?;
                let name = Ctx::some(ctx.find(def, IDENT))?;
                vec![ctx.ast.text(name).to_string()]
            }
            _ => Vec::new(),
        };
        let pops = if ty == LITERAL_for {
            if ctx.find(ast, FOR_INIT).is_none() {
                usize::from(!skip_enhanced)
            } else {
                variables_managed_by_for_loop(ctx, ast)?.len()
            }
        } else {
            0
        };
        let State::ModifiedControlVariable { stack } = &mut self.state else {
            return Ok(());
        };
        if ty == OBJBLOCK {
            stack.pop();
            return Ok(());
        }
        if pushes.is_empty() && pops == 0 {
            return Ok(());
        }
        let Some(current) = stack.last_mut() else {
            return Err(NPE.to_string());
        };
        current.extend(pushes);
        for _ in 0..pops {
            if current.pop().is_none() {
                return Err("java.util.NoSuchElementException".to_string());
            }
        }
        Ok(())
    }

    fn one_statement_visit(
        &mut self,
        ctx: &Ctx,
        ast: NodeId,
        treat_resources: bool,
    ) -> Result<(), Crash> {
        let ty = ctx.ty(ast);
        let line = ctx.ast.line(ast);
        let mut log_here = false;
        {
            let State::OneStatementPerLine {
                count_of_semi_in_lambda,
                last_statement_end,
                for_statement_end,
                in_for_header,
                is_in_lambda,
                lambda_statement_end,
                last_variable_resource_statement_end,
            } = &mut self.state
            else {
                return Ok(());
            };
            match ty {
                SEMI => {
                    let mut current = ast;
                    let prev = ctx.ast.previous_sibling(ast);
                    let has_resources_prev = prev.is_some_and(|p| ctx.ty(p) == RESOURCES);
                    if !has_resources_prev && is_multiline_statement(ctx, ast) {
                        current = ctx.previous_sibling(ast)?;
                    }
                    let same_line = |cur: NodeId, last: i32, fore: i32, lambda: i32| {
                        let l = ctx.ast.line(cur);
                        last == l && fore != l && lambda != l
                    };
                    let parent_is_resource = ctx
                        .ast
                        .parent(ast)
                        .is_some_and(|p| matches!(ctx.ty(p), RESOURCES | RESOURCE_SPECIFICATION));
                    if *is_in_lambda {
                        let count = count_of_semi_in_lambda
                            .last_mut()
                            .ok_or_else(|| "java.util.NoSuchElementException".to_string())?;
                        *count += 1;
                        if !*in_for_header
                            && *count > 1
                            && same_line(
                                current,
                                *last_statement_end,
                                *for_statement_end,
                                *lambda_statement_end,
                            )
                        {
                            log_here = true;
                        }
                    } else if parent_is_resource {
                        if treat_resources {
                            let next = ctx.next_sibling(ast)?;
                            if ctx.find(ctx.previous_sibling(ast)?, ASSIGN).is_some() {
                                *last_variable_resource_statement_end = line;
                            }
                            if ctx.find(next, ASSIGN).is_some()
                                && ctx.ast.line(next) == *last_variable_resource_statement_end
                            {
                                log_here = true;
                            }
                        }
                    } else if !*in_for_header
                        && same_line(
                            current,
                            *last_statement_end,
                            *for_statement_end,
                            *lambda_statement_end,
                        )
                    {
                        log_here = true;
                    }
                }
                FOR_ITERATOR => *for_statement_end = line,
                LAMBDA => {
                    *is_in_lambda = true;
                    count_of_semi_in_lambda.push(0);
                }
                _ => *in_for_header = true,
            }
        }
        if log_here {
            self.log(ctx, ast, "multiple.statements.line", vec![])?;
        }
        Ok(())
    }

    fn one_statement_leave(&mut self, ctx: &Ctx, ast: NodeId) {
        let State::OneStatementPerLine {
            count_of_semi_in_lambda,
            last_statement_end,
            for_statement_end,
            in_for_header,
            is_in_lambda,
            lambda_statement_end,
            ..
        } = &mut self.state
        else {
            return;
        };
        match ctx.ty(ast) {
            SEMI => {
                *last_statement_end = ctx.ast.line(ast);
                *for_statement_end = -1;
                *lambda_statement_end = -1;
            }
            FOR_ITERATOR => *in_for_header = false,
            LAMBDA => {
                count_of_semi_in_lambda.pop();
                if count_of_semi_in_lambda.is_empty() {
                    *is_in_lambda = false;
                }
                *lambda_statement_end = ctx.ast.line(ast);
            }
            _ => {}
        }
    }

    fn unused_imports_visit(
        &mut self,
        ctx: &Ctx,
        ast: NodeId,
        process_javadoc: bool,
    ) -> Result<(), Crash> {
        let ty = ctx.ty(ast);
        let State::UnusedImports {
            collect,
            imports,
            referenced,
        } = &mut self.state
        else {
            return Ok(());
        };
        match ty {
            IDENT => {
                if *collect {
                    let parent = ctx.parent(ast)?;
                    let parent_ty = ctx.ty(parent);
                    if parent_ty != DOT && parent_ty != METHOD_DEF
                        || parent_ty == DOT && ctx.ast.next_sibling(ast).is_some()
                    {
                        referenced.insert(ctx.ast.text(ast).to_string());
                    }
                }
            }
            IMPORT => {
                let (name, detail) = ctx.full_ident(ctx.ast.first_child(ast))?;
                if !name.ends_with(".*") {
                    imports.push((name, Ctx::some(detail)?));
                }
            }
            STATIC_IMPORT => {
                let (name, detail) = ctx.full_ident(ctx.ast.next_sibling(ctx.first_child(ast)?))?;
                if !name.ends_with(".*") {
                    imports.push((name, Ctx::some(detail)?));
                }
            }
            _ => {
                *collect = true;
                if process_javadoc
                    && let Some(comment) =
                        ctx.contents.javadoc_before(i64::from(ctx.ast.line(ast)))?
                {
                    referenced.extend(javadoc_references(comment));
                }
            }
        }
        Ok(())
    }
}

/// `^java\.lang\.[a-zA-Z]+$`.
fn is_java_lang_type(name: &str) -> bool {
    name.strip_prefix("java.lang.")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphabetic()))
}

fn can_return_only_boolean_literal(ctx: &Ctx, ast: Option<NodeId>) -> Result<bool, Crash> {
    if is_boolean_literal_return(ctx, ast)? {
        return Ok(true);
    }
    let first = ctx.ast.first_child(Ctx::some(ast)?);
    is_boolean_literal_return(ctx, first)
}

fn is_boolean_literal_return(ctx: &Ctx, ast: Option<NodeId>) -> Result<bool, Crash> {
    let Some(ast) = ast else {
        return Ok(false);
    };
    if ctx.ty(ast) != LITERAL_return {
        return Ok(false);
    }
    let expr = ctx.first_child(ast)?;
    if ctx.ty(expr) == SEMI {
        return Ok(false);
    }
    let value = ctx.first_child(expr)?;
    Ok(matches!(ctx.ty(value), LITERAL_true | LITERAL_false))
}

fn is_multiline_statement(ctx: &Ctx, ast: NodeId) -> bool {
    ctx.ast.previous_sibling(ast).is_some_and(|prev| {
        ctx.ast.line(prev) != ctx.ast.line(ast) && ctx.ast.parent(ast).is_some()
    })
}

/// `ModifiedControlVariableCheck.getVariablesManagedByForLoop`.
fn variables_managed_by_for_loop(ctx: &Ctx, for_ast: NodeId) -> Result<HashSet<String>, Crash> {
    let mut initialized = HashSet::new();
    let for_init = Ctx::some(ctx.find(for_ast, FOR_INIT))?;
    let mut def = ctx.find(for_init, VARIABLE_DEF);
    while let Some(d) = def {
        if ctx.ty(d) == VARIABLE_DEF {
            let name = Ctx::some(ctx.find(d, IDENT))?;
            initialized.insert(ctx.ast.text(name).to_string());
        }
        def = ctx.ast.next_sibling(d);
    }
    let mut iterating = HashSet::new();
    let for_iterator = Ctx::some(ctx.find(for_ast, FOR_ITERATOR))?;
    if let Some(elist) = ctx.find(for_iterator, ELIST) {
        let mut expr = ctx.find(elist, EXPR);
        while let Some(e) = expr {
            if ctx.ty(e) == EXPR {
                let op = ctx.first_child(e)?;
                if MUTATION_OPERATIONS.contains(&ctx.ty(op)) {
                    let child = ctx.first_child(op)?;
                    iterating.insert(ctx.ast.text(child).to_string());
                }
            }
            expr = ctx.ast.next_sibling(e);
        }
    }
    Ok(initialized
        .into_iter()
        .filter(|v| iterating.contains(v))
        .collect())
}

/// `FallThroughCheck.isTerminated`.
fn is_terminated(
    ctx: &Ctx,
    ast: NodeId,
    use_break: bool,
    use_continue: bool,
) -> Result<bool, Crash> {
    Ok(match ctx.ty(ast) {
        LITERAL_return | LITERAL_throw => true,
        LITERAL_break => use_break,
        LITERAL_continue => use_continue,
        SLIST => {
            let mut last = ctx.ast.last_child(ast);
            if let Some(l) = last
                && ctx.ty(l) == RCURLY
            {
                last = ctx.ast.previous_sibling(l);
            }
            match last {
                Some(l) => is_terminated(ctx, l, use_break, use_continue)?,
                None => false,
            }
        }
        LITERAL_if => {
            let then_stmt = ctx.next_sibling(Ctx::some(ctx.find(ast, RPAREN))?)?;
            match ctx.ast.next_sibling(then_stmt) {
                Some(else_stmt) => {
                    is_terminated(ctx, then_stmt, use_break, use_continue)?
                        && is_terminated(ctx, ctx.first_child(else_stmt)?, use_break, use_continue)?
                }
                None => false,
            }
        }
        LITERAL_for | LITERAL_while | LITERAL_do => {
            let body = if ctx.ty(ast) == LITERAL_do {
                ctx.previous_sibling(Ctx::some(ctx.find(ast, DO_WHILE))?)?
            } else {
                ctx.next_sibling(Ctx::some(ctx.find(ast, RPAREN))?)?
            };
            is_terminated(ctx, body, false, false)?
        }
        LITERAL_try => {
            let final_stmt = Ctx::some(ctx.ast.last_child(ast))?;
            let mut terminated = if ctx.ty(final_stmt) == LITERAL_finally {
                is_terminated(
                    ctx,
                    Ctx::some(ctx.find(final_stmt, SLIST))?,
                    use_break,
                    use_continue,
                )?
            } else {
                false
            };
            if !terminated {
                let mut first = ctx.first_child(ast)?;
                if ctx.ty(first) == RESOURCE_SPECIFICATION {
                    first = ctx.next_sibling(first)?;
                }
                terminated = is_terminated(ctx, first, use_break, use_continue)?;
                let mut catch = ctx.find(ast, LITERAL_catch);
                while let Some(c) = catch {
                    if !terminated || ctx.ty(c) != LITERAL_catch {
                        break;
                    }
                    let body = Ctx::some(ctx.find(c, SLIST))?;
                    terminated = is_terminated(ctx, body, use_break, use_continue)?;
                    catch = ctx.ast.next_sibling(c);
                }
            }
            terminated
        }
        LITERAL_switch => {
            let mut group = ctx.find(ast, CASE_GROUP);
            let mut terminated = group.is_some();
            while terminated {
                let Some(g) = group else {
                    return Err(NPE.to_string());
                };
                if ctx.ty(g) == RCURLY {
                    break;
                }
                terminated = match ctx.find(g, SLIST) {
                    Some(body) => is_terminated(ctx, body, false, use_continue)?,
                    None => false,
                };
                group = ctx.ast.next_sibling(g);
            }
            terminated
        }
        LITERAL_synchronized => is_terminated(
            ctx,
            Ctx::some(ctx.find(ast, SLIST))?,
            use_break,
            use_continue,
        )?,
        _ => false,
    })
}

/// `FallThroughCheck.hasFallThroughComment`.
fn has_fall_through_comment(
    ctx: &Ctx,
    current: NodeId,
    next: NodeId,
    relief: &Regex,
) -> Result<bool, Crash> {
    let end_line = ctx.ast.line(next);
    let end_col = ctx.ast.column(next);
    let lines = &ctx.contents.text;
    let line = lines.get(i64::from(end_line) - 1)?;
    let part = substring(line, 0, i64::from(end_col))?;
    if matches_comment(ctx, relief, part, end_line) {
        return Ok(true);
    }
    let start_line = ctx.ast.line(current);
    let mut i = i64::from(end_line) - 2;
    while i > i64::from(start_line) - 1 {
        let line = lines.get(i)?;
        if !is_blank(line) {
            return Ok(matches_comment(ctx, relief, line, (i + 1) as i32));
        }
        i -= 1;
    }
    Ok(false)
}

/// The relief pattern found on the line, inside a comment.
fn matches_comment(ctx: &Ctx, relief: &Regex, line: &[u16], line_no: i32) -> bool {
    let text = String::from_utf16_lossy(line);
    let Some(m) = relief.find(&text) else {
        return false;
    };
    let utf16 = |byte: usize| text[..byte].encode_utf16().count() as i64;
    ctx.contents.has_intersection_with_comment(
        i64::from(line_no),
        utf16(m.start()),
        i64::from(line_no),
        utf16(m.end()),
    )
}

/// `TreeWalker.walk`: begin, visit and leave every node in document order,
/// finish.
pub fn walk(ctx: &Ctx, root: Option<NodeId>, checks: &mut [Check<'_>]) -> Result<(), Crash> {
    let mut by_type: HashMap<u16, Vec<usize>> = HashMap::new();
    for (i, check) in checks.iter().enumerate() {
        for &ty in tokens(&check.module.kind) {
            by_type.entry(ty).or_default().push(i);
        }
    }
    for check in checks.iter_mut() {
        check.begin_tree();
    }
    let mut cur = root;
    while let Some(node) = cur {
        if let Some(list) = by_type.get(&ctx.ty(node)) {
            for &i in list {
                checks[i].visit(ctx, node)?;
            }
        }
        let mut to_visit = ctx.ast.first_child(node);
        let mut climbing = Some(node);
        while let (Some(c), None) = (climbing, to_visit) {
            if let Some(list) = by_type.get(&ctx.ty(c)) {
                for &i in list {
                    checks[i].leave(ctx, c)?;
                }
            }
            to_visit = ctx.ast.next_sibling(c);
            climbing = ctx.ast.parent(c);
        }
        cur = to_visit;
    }
    for check in checks.iter_mut() {
        check.finish_tree(ctx)?;
    }
    Ok(())
}

/// A message ready to sort: `LocalizedMessage` with its text formatted.
pub fn render(check: &CheckModule, v: &Violation) -> Result<Message, Crash> {
    let pattern = check.common.messages.get(v.key).map(String::as_str);
    let text = super::message::format_key(v.key, pattern, &v.args)?;
    Ok(Message {
        line: v.line,
        col: v.col,
        severity: check.common.severity,
        module_id: check.common.id.clone(),
        source: check.kind.name(),
        text,
    })
}

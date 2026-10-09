//! cpplint's per-file state machines: the nesting stack of blocks
//! (`NestingState`), the include-order tracker (`IncludeState`) and the
//! function-length counter (`FunctionState`).

use super::Lint;
use super::lines::{CleansedLines, advance_char, close_expression, from, get_indent_level, icount};
use super::regex::{escape, g, is_match, is_search, pmatch, search};
use super::tables::MATCH_ASM;

/// The kinds of block cpplint distinguishes on its stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Any other `{ ... }`.
    Block,
    /// An `extern "C" { ... }` block.
    ExternC,
    /// A class or struct.
    Class,
    /// A constructor definition (for member initializer lists).
    Constructor,
    /// A namespace.
    Namespace,
    /// A member initializer list (a `_WrappedInfo`).
    MemInitList,
}

impl Kind {
    /// `issubclass(type, _WrappedInfo)`.
    pub const fn is_wrapped(self) -> bool {
        matches!(self, Self::MemInitList)
    }
}

/// cpplint's inline-assembly states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asm {
    No,
    Inside,
    End,
    Block,
}

/// One entry of the nesting stack: cpplint's `_BlockInfo` and its
/// subclasses, flattened.
#[derive(Clone, Debug)]
pub struct BlockInfo {
    pub kind: Kind,
    pub starting_linenum: usize,
    pub seen_open_brace: bool,
    pub open_parentheses: i64,
    pub inline_asm: Asm,
    /// Class or namespace name ("" for an anonymous namespace).
    pub name: String,
    pub is_struct: bool,
    pub class_indent: usize,
    /// The line the class ends on, as a brace count guesses it.
    pub last_line: usize,
}

impl BlockInfo {
    const fn new(kind: Kind, linenum: usize, seen_open_brace: bool) -> Self {
        Self {
            kind,
            starting_linenum: linenum,
            seen_open_brace,
            open_parentheses: 0,
            inline_asm: Asm::No,
            name: String::new(),
            is_struct: false,
            class_indent: 0,
            last_line: 0,
        }
    }

    fn class(name: &str, class_or_struct: &str, cl: &CleansedLines, linenum: usize) -> Self {
        let mut info = Self::new(Kind::Class, linenum, false);
        info.name = name.to_string();
        info.is_struct = class_or_struct == "struct";
        info.class_indent = get_indent_level(&cl.raw_lines[linenum]);
        let mut depth = 0;
        for i in linenum..cl.num_lines {
            let line = &cl.elided[i];
            depth += icount(line, "{") - icount(line, "}");
            if depth == 0 {
                info.last_line = i;
                break;
            }
        }
        info
    }

    fn namespace(name: &str, linenum: usize) -> Self {
        let mut info = Self::new(Kind::Namespace, linenum, false);
        info.name = name.to_string();
        info
    }

    fn check_end(&self, lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
        match self.kind {
            Kind::Class => self.check_class_end(lint, cl, linenum),
            Kind::Namespace => self.check_namespace_end(lint, cl, linenum),
            _ => {}
        }
    }

    fn check_class_end(&self, lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
        let mut seen_last_thing_in_class = false;
        let mut i = linenum;
        while i > self.starting_linenum + 1 {
            i -= 1;
            let pattern = format!(
                r"\b(DISALLOW_COPY_AND_ASSIGN|DISALLOW_IMPLICIT_CONSTRUCTORS)\({}\)",
                self.name
            );
            if let Some(m) = search(&pattern, &cl.elided[i]) {
                if seen_last_thing_in_class {
                    lint.error(
                        i,
                        "readability/constructors",
                        3,
                        format!("{} should be the last thing in the class", g(&m, 1)),
                    );
                }
                break;
            }
            if !is_match(r"\s*$", &cl.elided[i]) {
                seen_last_thing_in_class = true;
            }
        }
        if let Some(indent) = pmatch(r"( *)\}", &cl.elided[linenum])
            && g(&indent, 1).len() != self.class_indent
        {
            let parent = if self.is_struct {
                format!("struct {}", self.name)
            } else {
                format!("class {}", self.name)
            };
            lint.error(
                linenum,
                "whitespace/indent",
                3,
                format!("Closing brace should be aligned with beginning of {parent}"),
            );
        }
    }

    fn check_namespace_end(&self, lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
        let line = &cl.raw_lines[linenum];
        if linenum - self.starting_linenum < 10
            && !is_match(r"\s*};*\s*(//|/\*).*\bnamespace\b", line)
        {
            return;
        }
        if self.name.is_empty() {
            if !is_match(r"\s*};*\s*(//|/\*).*\bnamespace[*/.\\\s]*$", line) {
                if is_match(r"\s*}.*\b(namespace anonymous|anonymous namespace)\b", line) {
                    lint.error(
                        linenum,
                        "readability/namespace",
                        5,
                        "Anonymous namespace should be terminated with \"// namespace\" or \"// anonymous namespace\""
                            .to_string(),
                    );
                } else {
                    lint.error(
                        linenum,
                        "readability/namespace",
                        5,
                        "Anonymous namespace should be terminated with \"// namespace\""
                            .to_string(),
                    );
                }
            }
        } else {
            let pattern = format!(
                r"\s*}};*\s*(//|/\*).*\bnamespace\s+{}[*/.\\\s]*$",
                escape(&self.name)
            );
            if !is_match(&pattern, line) {
                lint.error(
                    linenum,
                    "readability/namespace",
                    5,
                    format!(
                        "Namespace should be terminated with \"// namespace {}\"",
                        self.name
                    ),
                );
            }
        }
    }
}

/// A checkpoint of the nesting stack at a `#if`.
#[derive(Clone, Debug)]
struct PreprocessorInfo {
    stack_before_if: Vec<BlockInfo>,
    stack_before_else: Vec<BlockInfo>,
    seen_else: bool,
}

/// cpplint's `NestingState`.
pub struct NestingState {
    pub stack: Vec<BlockInfo>,
    /// The kind of the top of the stack before the last `update`.
    pub previous_stack_top: Option<Kind>,
    /// Its open-parenthesis count before the last `update`.
    pub previous_open_parentheses: i64,
    /// The kind of the last entry popped.
    pub popped_top: Option<Kind>,
    pp_stack: Vec<PreprocessorInfo>,
}

impl NestingState {
    pub const fn new() -> Self {
        Self {
            stack: Vec::new(),
            previous_stack_top: None,
            previous_open_parentheses: 0,
            popped_top: None,
            pp_stack: Vec::new(),
        }
    }

    pub fn top_kind(&self) -> Option<Kind> {
        self.stack.last().map(|b| b.kind)
    }

    /// Has the innermost block seen its opening brace?
    pub fn seen_open_brace(&self) -> bool {
        self.stack.last().is_none_or(|b| b.seen_open_brace)
    }

    pub fn in_namespace_body(&self) -> bool {
        self.top_kind() == Some(Kind::Namespace)
    }

    pub fn in_extern_c(&self) -> bool {
        self.top_kind() == Some(Kind::ExternC)
    }

    pub fn in_asm_block(&self) -> bool {
        self.stack.last().is_some_and(|b| b.inline_asm != Asm::No)
    }

    /// cpplint's `InTemplateArgumentList`: is `(linenum, pos)` inside
    /// template arguments?
    pub fn in_template_argument_list(
        cl: &CleansedLines,
        mut linenum: usize,
        mut pos: usize,
    ) -> bool {
        while linenum < cl.num_lines {
            let line = &cl.elided[linenum];
            let Some(m) = pmatch(r"[^{};=\[\]\.<>]*(.)", from(line, pos)) else {
                linenum += 1;
                pos = 0;
                continue;
            };
            let token = g(&m, 1);
            pos += g(&m, 0).len();
            if matches!(token, "{" | "}" | ";") {
                return false;
            }
            if matches!(token, ">" | "=" | "[" | "]" | ".") {
                return true;
            }
            if token != "<" {
                pos = advance_char(line, pos);
                if pos >= line.len() {
                    linenum += 1;
                    pos = 0;
                }
                continue;
            }
            let (_, end_line, end_pos) = close_expression(cl, linenum, pos - 1);
            let Some(end_pos) = end_pos else {
                return false;
            };
            linenum = end_line;
            pos = end_pos;
        }
        false
    }

    fn update_preprocessor(&mut self, line: &str) {
        if is_match(r"\s*#\s*(if|ifdef|ifndef)\b", line) {
            self.pp_stack.push(PreprocessorInfo {
                stack_before_if: self.stack.clone(),
                stack_before_else: Vec::new(),
                seen_else: false,
            });
        } else if is_match(r"\s*#\s*(else|elif)\b", line) {
            if let Some(top) = self.pp_stack.last_mut() {
                if !top.seen_else {
                    top.seen_else = true;
                    top.stack_before_else.clone_from(&self.stack);
                }
                self.stack.clone_from(&top.stack_before_if);
            }
        } else if is_match(r"\s*#\s*endif\b", line)
            && let Some(top) = self.pp_stack.pop()
            && top.seen_else
        {
            self.stack = top.stack_before_else;
        }
    }

    fn pop(&mut self) {
        self.popped_top = self.stack.pop().map(|b| b.kind);
    }

    fn count_open_parentheses(&mut self, line: &str) {
        let Some(inner) = self.stack.last_mut() else {
            return;
        };
        let depth_change = icount(line, "(") - icount(line, ")");
        inner.open_parentheses += depth_change;
        if matches!(inner.inline_asm, Asm::No | Asm::End) {
            if depth_change != 0 && inner.open_parentheses == 1 && is_search(MATCH_ASM, line) {
                inner.inline_asm = Asm::Inside;
            } else {
                inner.inline_asm = Asm::No;
            }
        } else if inner.inline_asm == Asm::Inside && inner.open_parentheses == 0 {
            inner.inline_asm = Asm::End;
        }
    }

    /// cpplint's `_UpdateNamesapce`: consume a namespace declaration at the
    /// start of `line`, pushing it; the rest of the line when one was there.
    fn update_namespace(&mut self, line: &str, linenum: usize) -> Option<String> {
        let m = pmatch(r"\s*namespace\b\s*([:\w]+)?(.*)$", line)?;
        let mut info = BlockInfo::namespace(g(&m, 1), linenum);
        let mut rest = g(&m, 2).to_string();
        if let Some(brace) = rest.find('{') {
            info.seen_open_brace = true;
            rest = rest[brace + 1..].to_string();
        }
        self.stack.push(info);
        Some(rest)
    }

    fn update_constructor(&mut self, line: &str, linenum: usize, class_name: Option<&str>) {
        match class_name {
            Some(name) if !name.is_empty() => {
                if !is_match(&format!(r"\s*{}\s*\(", escape(name)), line) {
                    return;
                }
            }
            _ => {
                if !is_match(r"\s*(\w*)\s*::\s*\1\s*\(", line) {
                    return;
                }
            }
        }
        self.stack
            .push(BlockInfo::new(Kind::Constructor, linenum, false));
    }

    /// cpplint's `Update`: advance the state over one line.
    pub fn update(&mut self, lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
        let mut line = cl.elided[linenum].clone();
        if let Some(top) = self.stack.last() {
            self.previous_stack_top = Some(top.kind);
            self.previous_open_parentheses = top.open_parentheses;
        } else {
            self.previous_stack_top = None;
        }
        self.update_preprocessor(&line);
        self.count_open_parentheses(&line);
        while let Some(rest) = self.update_namespace(&line, linenum) {
            line = rest;
        }
        let class_decl = pmatch(
            r"(\s*(?:template\s*<[\w\s<>,:=]*>\s*)?(class|struct)\s+(?:[a-zA-Z0-9_]+\s+)*(\w+(?:::\w+)*))(.*)$",
            &line,
        );
        if let Some(m) = class_decl
            && self.stack.last().is_none_or(|b| b.open_parentheses == 0)
        {
            let end_declaration = g(&m, 1).len();
            if !Self::in_template_argument_list(cl, linenum, end_declaration) {
                self.stack
                    .push(BlockInfo::class(g(&m, 3), g(&m, 2), cl, linenum));
                line = g(&m, 4).to_string();
            }
        }
        // cpplint runs `CheckBegin` here for a block still waiting for its
        // brace; `_ClassInfo.CheckBegin` only records `is_derived`, which
        // nothing reads, and the other kinds check nothing.
        if self.top_kind() == Some(Kind::Class) {
            let (seen, name, is_struct, class_indent) = {
                let top = &self.stack[self.stack.len() - 1];
                (
                    top.seen_open_brace,
                    top.name.clone(),
                    top.is_struct,
                    top.class_indent,
                )
            };
            if seen {
                let access = pmatch(
                    r"(.*)\b(public|private|protected|signals)(\s+(?:slots\s*)?)?:([^:].*|$)",
                    &line,
                );
                if let Some(m) = access {
                    let indent = g(&m, 1);
                    if indent.len() != class_indent + 1 && is_match(r"\s*$", indent) {
                        let parent = if is_struct {
                            format!("struct {name}")
                        } else {
                            format!("class {name}")
                        };
                        let slots = g(&m, 3);
                        lint.error(
                            linenum,
                            "whitespace/indent",
                            3,
                            format!(
                                "{}{slots}: should be indented +1 space inside {parent}",
                                g(&m, 2)
                            ),
                        );
                    }
                    line = g(&m, 4).to_string();
                } else {
                    self.update_constructor(&line, linenum, Some(&name));
                }
            }
        } else {
            self.update_constructor(&line, linenum, None);
        }
        if let Some(top) = self.stack.last()
            && (top.kind == Kind::Constructor || self.previous_stack_top == Some(Kind::Constructor))
            && !top.seen_open_brace
            && is_search(r"[^:]:[^:]", &line)
        {
            self.stack
                .push(BlockInfo::new(Kind::MemInitList, linenum, false));
        }
        while let Some(m) = pmatch(r"[^{;)}]*([{;)}])(.*)$", &line) {
            let token = g(&m, 1);
            let rest = g(&m, 2).to_string();
            match token {
                "{" => {
                    if !self.seen_open_brace() {
                        if self.top_kind() == Some(Kind::MemInitList) {
                            self.pop();
                        }
                        if let Some(top) = self.stack.last_mut() {
                            top.seen_open_brace = true;
                        }
                    } else if is_match(r#"extern\s*"[^"]*"\s*\{"#, &line) {
                        self.stack
                            .push(BlockInfo::new(Kind::ExternC, linenum, true));
                    } else {
                        let mut block = BlockInfo::new(Kind::Block, linenum, true);
                        if is_search(MATCH_ASM, &line) {
                            block.inline_asm = Asm::Block;
                        }
                        self.stack.push(block);
                    }
                }
                ";" => {
                    if !self.seen_open_brace() {
                        self.pop();
                    }
                }
                ")" => {
                    if self
                        .stack
                        .last()
                        .is_some_and(|b| !b.seen_open_brace && b.kind == Kind::Class)
                    {
                        self.pop();
                    }
                }
                _ => {
                    if let Some(top) = self.stack.last() {
                        let top = top.clone();
                        top.check_end(lint, cl, linenum);
                        self.pop();
                    }
                }
            }
            line = rest;
        }
    }

    /// The innermost class on the stack.
    pub fn innermost_class(&self) -> Option<&BlockInfo> {
        self.stack.iter().rev().find(|b| b.kind == Kind::Class)
    }
}

/// cpplint's `_FunctionState`.
pub struct FunctionState {
    /// Inside a function body (`in_a_function`).
    active: bool,
    /// Non-blank, non-comment lines seen in it (`lines_in_function`).
    lines: u64,
    /// Its name, with `()` or the TEST macro's arguments (`current_function`).
    name: String,
}

impl FunctionState {
    const NORMAL_TRIGGER: u64 = 250;
    const TEST_TRIGGER: u64 = 400;

    pub const fn new() -> Self {
        Self {
            active: false,
            lines: 0,
            name: String::new(),
        }
    }

    pub fn begin(&mut self, function_name: String) {
        self.active = true;
        self.lines = 0;
        self.name = function_name;
    }

    pub const fn count(&mut self) {
        if self.active {
            self.lines += 1;
        }
    }

    pub fn check(&self, lint: &mut Lint<'_>, linenum: usize) {
        if !self.active {
            return;
        }
        let base_trigger = if is_match(r"T(EST|est)", &self.name) {
            Self::TEST_TRIGGER
        } else {
            Self::NORMAL_TRIGGER
        };
        let trigger = base_trigger * 2u64.pow(lint.verbose_level());
        if self.lines > trigger {
            // Precision is not a concern: the ratio is a small number and the
            // result is floored to one of 0..=5.
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let error_level = {
                let ratio = self.lines as f64 / base_trigger as f64;
                (ratio.log2().floor() as u32).min(5)
            };
            lint.error(
                linenum,
                "readability/fn_size",
                error_level,
                format!(
                    "Small and focused functions are preferred: {} has {} non-comment lines (error triggered by exceeding {trigger} lines).",
                    self.name, self.lines
                ),
            );
        }
    }

    pub const fn end(&mut self) {
        self.active = false;
    }
}

/// The classification of one `#include`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderType {
    CSys,
    CppSys,
    OtherSys,
    LikelyMy,
    PossibleMy,
    Other,
}

impl HeaderType {
    const fn name(self) -> &'static str {
        match self {
            Self::CSys => "C system header",
            Self::CppSys => "C++ system header",
            Self::OtherSys => "other system header",
            Self::LikelyMy => "header this file implements",
            Self::PossibleMy => "header this file may implement",
            Self::Other => "other header",
        }
    }
}

/// The include sections, which must appear in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    Initial,
    MyH,
    C,
    Cpp,
    OtherSys,
    OtherH,
}

impl Section {
    const fn name(self) -> &'static str {
        match self {
            Self::Initial => "... nothing. (This can't be an error.)",
            Self::MyH => "a header this file implements",
            Self::C => "C system header",
            Self::Cpp => "C++ system header",
            Self::OtherSys => "other system header",
            Self::OtherH => "other header",
        }
    }
}

/// cpplint's `_IncludeState`.
pub struct IncludeState {
    /// The includes seen, one list per preprocessor section.
    pub include_list: Vec<Vec<(String, usize)>>,
    section: Section,
    last_header: String,
}

impl IncludeState {
    pub fn new() -> Self {
        Self {
            include_list: vec![Vec::new()],
            section: Section::Initial,
            last_header: String::new(),
        }
    }

    /// The line a header was first included on, if it was.
    pub fn find_header(&self, header: &str) -> Option<usize> {
        self.include_list
            .iter()
            .flatten()
            .find(|(h, _)| h == header)
            .map(|(_, line)| *line)
    }

    /// Reset section checking at a preprocessor directive.
    pub fn reset_section(&mut self, directive: &str) {
        self.section = Section::Initial;
        self.last_header = String::new();
        match directive {
            "if" | "ifdef" | "ifndef" => self.include_list.push(Vec::new()),
            "else" | "elif" => {
                if let Some(last) = self.include_list.last_mut() {
                    *last = Vec::new();
                }
            }
            _ => {}
        }
    }

    pub fn set_last_header(&mut self, header_path: &str) {
        self.last_header = header_path.to_string();
    }

    pub fn canonicalize_alphabetical_order(header_path: &str) -> String {
        header_path
            .replace("-inl.h", ".h")
            .replace('-', "_")
            .to_lowercase()
    }

    pub fn is_in_alphabetical_order(
        &self,
        cl: &CleansedLines,
        linenum: usize,
        header_path: &str,
    ) -> bool {
        !(self.last_header.as_str() > header_path
            && is_match(r"\s*#\s*include\b", &cl.elided[linenum - 1]))
    }

    /// The error for an out-of-order header, or "" when the order is fine;
    /// moves the section on either way.
    pub fn check_next_include_order(&mut self, header_type: HeaderType) -> String {
        let error_message = format!("Found {} after {}", header_type.name(), self.section.name());
        let last_section = self.section;
        let wanted = match header_type {
            HeaderType::CSys => Some(Section::C),
            HeaderType::CppSys => Some(Section::Cpp),
            HeaderType::OtherSys => Some(Section::OtherSys),
            HeaderType::LikelyMy | HeaderType::PossibleMy | HeaderType::Other => None,
        };
        match (header_type, wanted) {
            (_, Some(section)) => {
                if self.section <= section {
                    self.section = section;
                } else {
                    self.last_header = String::new();
                    return error_message;
                }
            }
            (HeaderType::LikelyMy | HeaderType::PossibleMy, None) => {
                if self.section <= Section::MyH {
                    self.section = Section::MyH;
                } else {
                    self.section = Section::OtherH;
                }
            }
            _ => self.section = Section::OtherH,
        }
        if last_section != self.section {
            self.last_header = String::new();
        }
        String::new()
    }
}

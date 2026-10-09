//! cpplint's file-level and style checks: copyright, header guards, the
//! `CheckStyle` family (whitespace, braces, semicolons, CHECK macros) and
//! the small per-line checks `ProcessLine` runs after them.
//!
//! Every function here is a port of the cpplint 2.0.2 function of the same
//! name (snake-cased), with its messages and confidence levels verbatim.

use super::Lint;
use super::lines::{
    CleansedLines, Scan, close_expression, count, find_end_of_expression_in_line, from,
    get_indent_level, get_line_width, get_previous_non_blank_line, is_blank_line,
    reverse_close_expression, sl, upto,
};
use super::nesting::{BlockInfo, FunctionState, Kind, NestingState};
use super::regex::{end, escape, find_all, g, group, is_match, is_search, pmatch, search, sub};
use super::tables::{
    ALT_TOKEN_REPLACEMENT_PATTERN, CHECK_MACROS, THREADING_LIST, TYPES_PATTERN,
    alt_token_replacement, check_replacement,
};

/// `CheckForCopyright`: a "Copyright" within the first ten lines.
pub fn check_for_copyright(lint: &mut Lint<'_>, lines: &[String]) {
    let limit = lines.len().min(11);
    if !(1..limit).any(|i| is_search("(?i)Copyright", &lines[i])) {
        lint.error(
            0,
            "legal/copyright",
            5,
            "No copyright message found.  You should have a line: \"Copyright [year] <Copyright Owner>\"".to_string(),
        );
    }
}

/// `CheckForHeaderGuard`.
pub fn check_for_header_guard(lint: &mut Lint<'_>, cl: &CleansedLines, cppvar: &str) {
    let raw_lines = &cl.lines_without_raw_strings;
    if raw_lines
        .iter()
        .any(|l| is_search(r"//\s*NOLINT\(build/header_guard\)", l))
    {
        return;
    }
    if raw_lines
        .iter()
        .any(|l| is_search(r"^\s*#pragma\s+once", l))
    {
        return;
    }
    let mut ifndef = "";
    let mut ifndef_linenum = 0;
    let mut define = "";
    let mut endif = "";
    let mut endif_linenum = 0;
    for (linenum, line) in raw_lines.iter().enumerate() {
        let linesplit: Vec<&str> = line.split_whitespace().collect();
        if linesplit.len() >= 2 {
            if ifndef.is_empty() && linesplit[0] == "#ifndef" {
                ifndef = linesplit[1];
                ifndef_linenum = linenum;
            }
            if define.is_empty() && linesplit[0] == "#define" {
                define = linesplit[1];
            }
        }
        if line.starts_with("#endif") {
            endif = line;
            endif_linenum = linenum;
        }
    }
    if ifndef.is_empty() || define.is_empty() || ifndef != define {
        lint.error(
            0,
            "build/header_guard",
            5,
            format!("No #ifndef header guard found, suggested CPP variable is: {cppvar}"),
        );
        return;
    }
    if ifndef != cppvar {
        let error_level = if ifndef == format!("{cppvar}_") { 0 } else { 5 };
        lint.parse_nolint_suppressions(&raw_lines[ifndef_linenum], ifndef_linenum);
        lint.error(
            ifndef_linenum,
            "build/header_guard",
            error_level,
            format!("#ifndef header guard has wrong style, please use: {cppvar}"),
        );
    }
    lint.parse_nolint_suppressions(&raw_lines[endif_linenum], endif_linenum);
    let escaped = escape(cppvar);
    if let Some(m) = pmatch(&format!(r"#endif\s*//\s*{escaped}(_)?\b"), endif) {
        if group(&m, 1) == Some("_") {
            lint.error(
                endif_linenum,
                "build/header_guard",
                0,
                format!("#endif line should be \"#endif  // {cppvar}\""),
            );
        }
        return;
    }
    let mut no_single_line_comments = true;
    for line in raw_lines
        .iter()
        .take(raw_lines.len().saturating_sub(1))
        .skip(1)
    {
        if is_match(
            r#"(?:(?:'(?:\.|[^'])*')|(?:"(?:\.|[^"])*")|[^'"])*//"#,
            line,
        ) {
            no_single_line_comments = false;
            break;
        }
    }
    if no_single_line_comments
        && let Some(m) = pmatch(&format!(r"#endif\s*/\*\s*{escaped}(_)?\s*\*/"), endif)
    {
        if group(&m, 1) == Some("_") {
            lint.error(
                endif_linenum,
                "build/header_guard",
                0,
                format!("#endif line should be \"#endif  /* {cppvar} */\""),
            );
        }
        return;
    }
    lint.error(
        endif_linenum,
        "build/header_guard",
        5,
        format!("#endif line should be \"#endif  // {cppvar}\""),
    );
}

/// `CheckForBadCharacters`: replacement characters and NUL bytes.
pub fn check_for_bad_characters(lint: &mut Lint<'_>, lines: &[String]) {
    for (linenum, line) in lines.iter().enumerate() {
        if line.contains('\u{fffd}') {
            lint.error(
                linenum,
                "readability/utf8",
                5,
                "Line contains invalid UTF-8 (or Unicode replacement character).".to_string(),
            );
        }
        if line.contains('\0') {
            lint.error(
                linenum,
                "readability/nul",
                5,
                "Line contains NUL byte.".to_string(),
            );
        }
    }
}

/// `CheckForNewlineAtEOF`.
pub fn check_for_newline_at_eof(lint: &mut Lint<'_>, lines: &[String]) {
    if lines.len() < 3 || !lines[lines.len() - 2].is_empty() {
        lint.error(
            lines.len().saturating_sub(2),
            "whitespace/ending_newline",
            5,
            "Could not find a newline character at the end of the file.".to_string(),
        );
    }
}

/// `CheckForMultilineCommentsAndStrings`.
pub fn check_for_multiline_comments_and_strings(
    lint: &mut Lint<'_>,
    cl: &CleansedLines,
    linenum: usize,
) {
    let line = cl.elided[linenum].replace("\\\\", "");
    if count(&line, "/*") > count(&line, "*/") {
        lint.error(
            linenum,
            "readability/multiline_comment",
            5,
            "Complex multi-line /*...*/-style comment found. Lint may give bogus warnings.  Consider replacing these with //-style comments, with #if 0...#endif, or with more clearly structured multi-line comments.".to_string(),
        );
    }
    if (count(&line, "\"") - count(&line, "\\\"")) % 2 == 1 {
        lint.error(
            linenum,
            "readability/multiline_string",
            5,
            "Multi-line string (\"...\") found.  This lint script doesn't do well with such strings, and may give bogus warnings.  Use C++11 raw strings or concatenation instead.".to_string(),
        );
    }
}

/// `CheckPosixThreading`.
pub fn check_posix_threading(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let line = &cl.elided[linenum];
    for (single, multi, pattern) in THREADING_LIST {
        if is_search(pattern, line) {
            lint.error(
                linenum,
                "runtime/threadsafe_fn",
                2,
                format!(
                    "Consider using {multi}...) instead of {single}...) for improved thread safety."
                ),
            );
        }
    }
}

/// `CheckVlogArguments`.
pub fn check_vlog_arguments(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    if is_search(
        r"\bVLOG\((INFO|ERROR|WARNING|DFATAL|FATAL)\)",
        &cl.elided[linenum],
    ) {
        lint.error(
            linenum,
            "runtime/vlog",
            5,
            "VLOG() should be used with numeric verbosity level.  Use LOG() if you want symbolic severity levels."
                .to_string(),
        );
    }
}

/// `CheckInvalidIncrement`.
pub fn check_invalid_increment(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    if is_match(r"\s*\*\w+(\+\+|--);", &cl.elided[linenum]) {
        lint.error(
            linenum,
            "runtime/invalid_increment",
            5,
            "Changing pointer instead of value (or unused value of operator*).".to_string(),
        );
    }
}

/// `CheckForNonStandardConstructs`.
pub fn check_for_non_standard_constructs(
    lint: &mut Lint<'_>,
    cl: &CleansedLines,
    linenum: usize,
    nesting: &NestingState,
) {
    let line = &cl.lines[linenum];
    if is_search(r#"printf\s*\(.*".*%[-+ ]?\d*q"#, line) {
        lint.error(
            linenum,
            "runtime/printf_format",
            3,
            "%q in format strings is deprecated.  Use %ll instead.".to_string(),
        );
    }
    if is_search(r#"printf\s*\(.*".*%\d+\$"#, line) {
        lint.error(
            linenum,
            "runtime/printf_format",
            2,
            "%N$ formats are unconventional.  Try rewriting to avoid them.".to_string(),
        );
    }
    let line = line.replace("\\\\", "");
    if is_search(r#"("|').*\\(%|\[|\(|{)"#, &line) {
        lint.error(
            linenum,
            "build/printf_format",
            3,
            "%, [, (, and { are undefined character escapes.  Unescape them.".to_string(),
        );
    }
    let line = &cl.elided[linenum];
    if is_search(
        r"\b(const|volatile|void|char|short|int|long|float|double|signed|unsigned|schar|u?int8_t|u?int16_t|u?int32_t|u?int64_t)\s+(register|static|extern|typedef)\b",
        line,
    ) {
        lint.error(
            linenum,
            "build/storage_class",
            5,
            "Storage-class specifier (static, extern, typedef, etc) should be at the beginning of the declaration."
                .to_string(),
        );
    }
    if is_match(r"\s*#\s*endif\s*[^/\s]+", line) {
        lint.error(
            linenum,
            "build/endif_comment",
            5,
            "Uncommented text after #endif is non-standard.  Use a comment.".to_string(),
        );
    }
    if is_match(r"\s*class\s+(\w+\s*::\s*)+\w+\s*;", line) {
        lint.error(
            linenum,
            "build/forward_decl",
            5,
            "Inner-style forward declarations are invalid.  Remove this line.".to_string(),
        );
    }
    if is_search(
        r"(\w+|[+-]?\d+(\.\d*)?)\s*(<|>)\?=?\s*(\w+|[+-]?\d+)(\.\d*)?",
        line,
    ) {
        lint.error(
            linenum,
            "build/deprecated",
            3,
            ">? and <? (max and min) operators are non-standard and deprecated.".to_string(),
        );
    }
    if is_search(r"^\s*const\s*string\s*&\s*\w+\s*;", line) {
        lint.error(
            linenum,
            "runtime/member_string_references",
            2,
            "const string& members are dangerous. It is much better to use alternatives, such as pointers or simple constants."
                .to_string(),
        );
    }
    let Some(classinfo) = nesting.innermost_class() else {
        return;
    };
    if !classinfo.seen_open_brace {
        return;
    }
    let base_classname = classinfo.name.rsplit("::").next().unwrap_or("");
    let escaped = escape(base_classname);
    let pattern = format!(
        r"\s+(?:(?:inline|constexpr)\s+)*(explicit\s+)?(?:(?:inline|constexpr)\s+)*{escaped}\s*\(((?:[^()]|\([^()]*\))*)\)"
    );
    let Some(m) = pmatch(&pattern, line) else {
        return;
    };
    let is_marked_explicit = group(&m, 1).is_some();
    let mut constructor_args: Vec<String> = match group(&m, 2) {
        None | Some("") => Vec::new(),
        Some(args) => args.split(',').map(str::to_string).collect(),
    };
    let mut i = 0;
    while i < constructor_args.len() {
        let mut arg = constructor_args[i].clone();
        while count(&arg, "<") > count(&arg, ">") || count(&arg, "(") > count(&arg, ")") {
            if i + 1 >= constructor_args.len() {
                break;
            }
            let next = constructor_args.remove(i + 1);
            arg.push(',');
            arg.push_str(&next);
        }
        constructor_args[i] = arg;
        i += 1;
    }
    let variadic_args = constructor_args
        .iter()
        .filter(|a| a.contains("&&..."))
        .count();
    let defaulted_args = constructor_args.iter().filter(|a| a.contains('=')).count();
    let noarg_constructor = constructor_args.is_empty()
        || (constructor_args.len() == 1 && constructor_args[0].trim() == "void");
    let onearg_constructor = (constructor_args.len() == 1 && !noarg_constructor)
        || (!constructor_args.is_empty()
            && !noarg_constructor
            && defaulted_args >= constructor_args.len() - 1)
        || (constructor_args.len() <= 2 && variadic_args >= 1);
    let initializer_list_constructor =
        onearg_constructor && is_search(r"\bstd\s*::\s*initializer_list\b", &constructor_args[0]);
    let copy_pattern = format!(
        r"((const\s+(volatile\s+)?)?|(volatile\s+(const\s+)?))?{escaped}(\s*<[^>]*>)?(\s+const)?\s*(?:<\w+>\s*)?&"
    );
    let copy_constructor =
        onearg_constructor && is_match(&copy_pattern, constructor_args[0].trim());
    if !is_marked_explicit
        && onearg_constructor
        && !initializer_list_constructor
        && !copy_constructor
    {
        if defaulted_args > 0 || variadic_args > 0 {
            lint.error(
                linenum,
                "runtime/explicit",
                4,
                "Constructors callable with one argument should be marked explicit.".to_string(),
            );
        } else {
            lint.error(
                linenum,
                "runtime/explicit",
                4,
                "Single-parameter constructors should be marked explicit.".to_string(),
            );
        }
    }
}

/// `CheckSpacingForFunctionCall`.
pub fn check_spacing_for_function_call(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let line = &cl.elided[linenum];
    let mut fncall = line.as_str();
    for pattern in [
        r"\bif\s*\((.*)\)\s*{",
        r"\bfor\s*\((.*)\)\s*{",
        r"\bwhile\s*\((.*)\)\s*[{;]",
        r"\bswitch\s*\((.*)\)\s*{",
    ] {
        if let Some(m) = search(pattern, line) {
            fncall = g(&m, 1);
            break;
        }
    }
    if is_search(
        r"\b(if|elif|for|while|switch|return|new|delete|catch|sizeof)\b",
        fncall,
    ) || is_search(r" \([^)]+\)\([^)]*(\)|,$)", fncall)
        || is_search(r" \([^)]+\)\[[^\]]+\]", fncall)
    {
        return;
    }
    if is_search(r"\w\s*\(\s(?!\s*\\$)", fncall) {
        lint.error(
            linenum,
            "whitespace/parens",
            4,
            "Extra space after ( in function call".to_string(),
        );
    } else if is_search(r"\(\s+(?!(\s*\\)|\()", fncall) {
        lint.error(
            linenum,
            "whitespace/parens",
            2,
            "Extra space after (".to_string(),
        );
    }
    if is_search(r"\w\s+\(", fncall)
        && !is_search(r"_{0,2}asm_{0,2}\s+_{0,2}volatile_{0,2}\s+\(", fncall)
        && !is_search(r"#\s*define|typedef|using\s+\w+\s*=", fncall)
        && !is_search(r"\w\s+\((\w+::)*\*\w+\)\(", fncall)
        && !is_search(r"\bcase\s+\(", fncall)
    {
        if is_search(r"\boperator_*\b", line) {
            lint.error(
                linenum,
                "whitespace/parens",
                0,
                "Extra space before ( in function call".to_string(),
            );
        } else {
            lint.error(
                linenum,
                "whitespace/parens",
                4,
                "Extra space before ( in function call".to_string(),
            );
        }
    }
    if is_search(r"[^)]\s+\)\s*[^{\s]", fncall) {
        if is_search(r"^\s+\)", fncall) {
            lint.error(
                linenum,
                "whitespace/parens",
                2,
                "Closing ) should be moved to the previous line".to_string(),
            );
        } else {
            lint.error(
                linenum,
                "whitespace/parens",
                2,
                "Extra space before )".to_string(),
            );
        }
    }
}

fn is_macro_definition(lines: &[String], linenum: usize) -> bool {
    if is_search(r"^#define", &lines[linenum]) {
        return true;
    }
    linenum > 0 && is_search(r"\\$", &lines[linenum - 1])
}

fn is_forward_class_declaration(lines: &[String], linenum: usize) -> bool {
    is_match(r"\s*(\btemplate\b)*.*class\s+\w+;\s*$", &lines[linenum])
}

/// `IsBlockInNameSpace`.
fn is_block_in_namespace(nesting: &NestingState, is_forward_declaration: bool) -> bool {
    let stack = &nesting.stack;
    if is_forward_declaration {
        return !stack.is_empty() && stack[stack.len() - 1].kind == Kind::Namespace;
    }
    if stack.is_empty() {
        return false;
    }
    let top = &stack[stack.len() - 1];
    if top.kind == Kind::Namespace {
        return true;
    }
    if stack.len() > 1 && nesting.previous_stack_top == Some(Kind::Namespace) {
        let second = &stack[stack.len() - 2];
        if second.kind == Kind::Namespace
            || (stack.len() > 2
                && top.kind.is_wrapped()
                && !second.seen_open_brace
                && stack[stack.len() - 3].kind == Kind::Namespace)
        {
            return true;
        }
    }
    false
}

/// `ShouldCheckNamespaceIndentation`.
fn should_check_namespace_indentation(
    nesting: &NestingState,
    is_namespace_indent_item: bool,
    raw_lines_no_comments: &[String],
    linenum: usize,
) -> bool {
    if nesting.stack.is_empty() {
        return false;
    }
    let is_forward_declaration = is_forward_class_declaration(raw_lines_no_comments, linenum);
    if !(is_namespace_indent_item || is_forward_declaration) {
        return false;
    }
    if is_macro_definition(raw_lines_no_comments, linenum) {
        return false;
    }
    if nesting.previous_stack_top.is_some() && nesting.previous_open_parentheses > 0 {
        return false;
    }
    let top = nesting.top_kind();
    if (nesting.previous_stack_top == Some(Kind::Constructor)
        && (top == Some(Kind::MemInitList) || nesting.popped_top == Some(Kind::MemInitList)))
        || (nesting.previous_stack_top == Some(Kind::Constructor)
            && nesting.popped_top == Some(Kind::Constructor)
            && is_search(r"[^:]:[^:]", &raw_lines_no_comments[linenum]))
    {
        return false;
    }
    is_block_in_namespace(nesting, is_forward_declaration)
}

/// `CheckForNamespaceIndentation`.
pub fn check_for_namespace_indentation(
    lint: &mut Lint<'_>,
    nesting: &NestingState,
    cl: &CleansedLines,
    linenum: usize,
) {
    let is_namespace_indent_item = !nesting.stack.is_empty()
        && (nesting.top_kind() == Some(Kind::Namespace)
            || nesting.previous_stack_top == Some(Kind::Namespace));
    if should_check_namespace_indentation(nesting, is_namespace_indent_item, &cl.elided, linenum)
        && is_match(r"\s+", &cl.elided[linenum])
    {
        lint.error(
            linenum,
            "whitespace/indent_namespace",
            4,
            "Do not indent within a namespace.".to_string(),
        );
    }
}

/// `CheckForFunctionLengths`.
pub fn check_for_function_lengths(
    lint: &mut Lint<'_>,
    cl: &CleansedLines,
    linenum: usize,
    function_state: &mut FunctionState,
) {
    let lines = &cl.lines;
    let line = &lines[linenum];
    let mut joined_line = String::new();
    let mut starting_func = false;
    if let Some(m) = pmatch(r"(\w(\w|::|\*|\&|\s)*)\(", line) {
        let function_name = g(&m, 1).split_whitespace().last().unwrap_or("");
        if function_name == "TEST"
            || function_name == "TEST_F"
            || !is_match(r"[A-Z_]+$", function_name)
        {
            starting_func = true;
        }
    }
    if starting_func {
        let mut body_found = false;
        for start_line in lines.iter().take(cl.num_lines).skip(linenum) {
            joined_line.push(' ');
            joined_line.push_str(start_line.trim_start());
            if is_search(r"(;|})", start_line) {
                body_found = true;
                break;
            }
            if is_search(r"\{", start_line) {
                body_found = true;
                let mut function = search(r"((\w|:)*)\(", line)
                    .map_or("", |m| g(&m, 1))
                    .to_string();
                if is_match("TEST", &function) {
                    if let Some(params) = search(r"(\(.*\))", &joined_line) {
                        function.push_str(g(&params, 1));
                    }
                } else {
                    function.push_str("()");
                }
                function_state.begin(function);
                break;
            }
        }
        if !body_found {
            lint.error(
                linenum,
                "readability/fn_size",
                5,
                "Lint failed to find start of function body.".to_string(),
            );
        }
    } else if is_match(r"\}\s*$", line) {
        function_state.check(lint, linenum);
        function_state.end();
    } else if !is_match(r"\s*$", line) {
        function_state.count();
    }
}

const fn is_ascii_space_class(c: char) -> bool {
    // Python's string.whitespace.
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

/// `CheckComment`.
pub fn check_comment(lint: &mut Lint<'_>, line: &str, linenum: usize, next_line_start: usize) {
    let Some(commentpos) = line.find("//") else {
        return;
    };
    if !count(&sub(r"\\.", "", &line[..commentpos]), "\"").is_multiple_of(2) {
        return;
    }
    let before: Vec<char> = line[..commentpos].chars().rev().take(2).collect();
    let too_close = (!before.is_empty() && !is_ascii_space_class(before[0]))
        || (before.len() >= 2 && !is_ascii_space_class(before[1]));
    if !(is_match(r".*{ *//", line) && next_line_start == commentpos) && too_close {
        lint.error(
            linenum,
            "whitespace/comments",
            2,
            "At least two spaces is best between code and comments".to_string(),
        );
    }
    let comment = &line[commentpos..];
    if let Some(m) = pmatch(r"//(\s*)TODO(\(.+?\))?:?(\s|$)?", comment) {
        if g(&m, 1).chars().count() > 1 {
            lint.error(
                linenum,
                "whitespace/todo",
                2,
                "Too many spaces before TODO".to_string(),
            );
        }
        if group(&m, 2).is_none() {
            lint.error(
                linenum,
                "readability/todo",
                2,
                "Missing username in TODO; it should look like \"// TODO(my_username): Stuff.\""
                    .to_string(),
            );
        }
        if !matches!(group(&m, 3), Some(" " | "")) {
            lint.error(
                linenum,
                "whitespace/todo",
                2,
                "TODO(my_username) should be followed by a space".to_string(),
            );
        }
    }
    if is_match(r"//[^ ]*\w", comment) && !is_match(r"(///|//\!)(\s+|$)", comment) {
        lint.error(
            linenum,
            "whitespace/comments",
            4,
            "Should have a space between // and comment".to_string(),
        );
    }
}

/// `CheckSpacing`.
pub fn check_spacing(
    lint: &mut Lint<'_>,
    cl: &CleansedLines,
    linenum: usize,
    nesting: &NestingState,
) {
    let raw = &cl.lines_without_raw_strings;
    let line = &raw[linenum];
    if is_blank_line(line) && !nesting.in_namespace_body() && !nesting.in_extern_c() {
        let elided = &cl.elided;
        let prev_line = &elided[linenum.saturating_sub(1)];
        if let Some(prevbrace) = prev_line.rfind('{')
            && !prev_line[prevbrace..].contains('}')
        {
            let exception = if is_match(r" {6}\w", prev_line) {
                let mut search_position: isize = isize::try_from(linenum).unwrap_or(0) - 2;
                while search_position >= 0
                    && is_match(
                        r" {6}\w",
                        &elided[usize::try_from(search_position).unwrap_or(0)],
                    )
                {
                    search_position -= 1;
                }
                search_position >= 0
                    && upto(&elided[usize::try_from(search_position).unwrap_or(0)], 5) == "    :"
            } else {
                is_match(r" {4}\w[^\(]*\)\s*(const\s*)?(\{\s*$|:)", prev_line)
                    || is_match(r" {4}:", prev_line)
            };
            if !exception {
                lint.error(
                    linenum,
                    "whitespace/blank_line",
                    2,
                    "Redundant blank line at the start of a code block should be deleted."
                        .to_string(),
                );
            }
        }
        if linenum + 1 < cl.num_lines {
            let next_line = &raw[linenum + 1];
            if !next_line.is_empty()
                && is_match(r"\s*}", next_line)
                && !next_line.contains("} else ")
            {
                lint.error(
                    linenum,
                    "whitespace/blank_line",
                    3,
                    "Redundant blank line at the end of a code block should be deleted."
                        .to_string(),
                );
            }
        }
        if let Some(m) = pmatch(r"\s*(public|protected|private):", prev_line) {
            lint.error(
                linenum,
                "whitespace/blank_line",
                3,
                format!("Do not leave a blank line after \"{}:\"", g(&m, 1)),
            );
        }
    }
    let next_line_start = if linenum + 1 < cl.num_lines {
        let next_line = &raw[linenum + 1];
        next_line.len() - next_line.trim_start().len()
    } else {
        0
    };
    check_comment(lint, line, linenum, next_line_start);
    let line = &cl.elided[linenum];
    if is_search(r"\w\s+\[(?!\[)", line) && !is_search(r"(?:auto&?|delete|return)\s+\[", line) {
        lint.error(
            linenum,
            "whitespace/braces",
            5,
            "Extra space before [".to_string(),
        );
    }
    if is_search(r"for *\(.*[^:]:[^: ]", line) || is_search(r"for *\(.*[^: ]:[^:]", line) {
        lint.error(
            linenum,
            "whitespace/forcolon",
            2,
            "Missing space around colon in range-based for loop".to_string(),
        );
    }
}

/// `CheckOperatorSpacing`.
pub fn check_operator_spacing(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let mut line = cl.elided[linenum].clone();
    while let Some(m) = pmatch(r"(.*\boperator\b)(\S+)(\s*\(.*)$", &line) {
        let replaced = format!(
            "{}{}{}",
            g(&m, 1),
            "_".repeat(g(&m, 2).chars().count()),
            g(&m, 3)
        );
        line = replaced;
    }
    if (is_search(r"[\w.]=", &line) || is_search(r"=[\w.]", &line))
        && !is_search(r"\b(if|while|for) ", &line)
        && !is_search(r"(>=|<=|==|!=|&=|\^=|\|=|\+=|\*=|\/=|\%=)", &line)
        && !is_search("operator=", &line)
    {
        lint.error(
            linenum,
            "whitespace/operators",
            4,
            "Missing spaces around =".to_string(),
        );
    }
    if let Some(m) = search(r"[^<>=!\s](==|!=|<=|>=|\|\|)[^<>=!\s,;\)]", &line) {
        lint.error(
            linenum,
            "whitespace/operators",
            3,
            format!("Missing spaces around {}", g(&m, 1)),
        );
    } else if !is_match("#.*include", &line) {
        if let Some(m) = pmatch(r"(.*[^\s<])<[^\s=<,]", &line) {
            let (_, _, end_pos) = close_expression(cl, linenum, g(&m, 1).len());
            if end_pos.is_none() {
                lint.error(
                    linenum,
                    "whitespace/operators",
                    3,
                    "Missing spaces around <".to_string(),
                );
            }
        }
        if let Some(m) = pmatch(r"(.*[^-\s>])>[^\s=>,]", &line) {
            let (_, _, start_pos) = reverse_close_expression(cl, linenum, g(&m, 1).len());
            if start_pos.is_none() {
                lint.error(
                    linenum,
                    "whitespace/operators",
                    3,
                    "Missing spaces around >".to_string(),
                );
            }
        }
    }
    if let Some(m) = search(
        r"(operator|[^\s(<])(?:L|UL|LL|ULL|l|ul|ll|ull)?<<([^\s,=<])",
        &line,
    ) {
        let left = g(&m, 1);
        let right = g(&m, 2);
        let both_digits = !left.is_empty()
            && left.chars().all(|c| c.is_ascii_digit())
            && right.chars().all(|c| c.is_ascii_digit());
        if !(both_digits || (left == "operator" && right == ";")) {
            lint.error(
                linenum,
                "whitespace/operators",
                3,
                "Missing spaces around <<".to_string(),
            );
        }
    }
    if is_search(r">>[a-zA-Z_]", &line) {
        lint.error(
            linenum,
            "whitespace/operators",
            3,
            "Missing spaces around >>".to_string(),
        );
    }
    if let Some(m) = search(r"(!\s|~\s|[\s]--[\s;]|[\s]\+\+[\s;])", &line) {
        lint.error(
            linenum,
            "whitespace/operators",
            4,
            format!("Extra space for operator {}", g(&m, 1)),
        );
    }
}

/// `CheckParenthesisSpacing`.
pub fn check_parenthesis_spacing(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let line = &cl.elided[linenum];
    if let Some(m) = search(r" (if\(|for\(|while\(|switch\()", line) {
        lint.error(
            linenum,
            "whitespace/parens",
            5,
            format!("Missing space before ( in {}", g(&m, 1)),
        );
    }
    if let Some(m) = search(
        r"\b(if|for|while|switch)\s*\(([ ]*)(.).*[^ ]+([ ]*)\)\s*{\s*$",
        line,
    ) {
        let open_spaces = g(&m, 2).len();
        let close_spaces = g(&m, 4).len();
        if open_spaces != close_spaces
            && !((g(&m, 3) == ";" && open_spaces == 1 + close_spaces)
                || (open_spaces == 0 && is_search(r"\bfor\s*\(.*; \)", line)))
        {
            lint.error(
                linenum,
                "whitespace/parens",
                5,
                format!("Mismatching spaces inside () in {}", g(&m, 1)),
            );
        }
        if open_spaces > 1 {
            lint.error(
                linenum,
                "whitespace/parens",
                5,
                format!(
                    "Should have zero or one spaces inside ( and ) in {}",
                    g(&m, 1)
                ),
            );
        }
    }
}

/// `CheckCommaSpacing`.
pub fn check_comma_spacing(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let raw = &cl.lines_without_raw_strings;
    let line = &cl.elided[linenum];
    let reduced = sub(
        r"\b__VA_OPT__\s*\(,\)",
        "",
        &sub(r"\boperator\s*,\s*\(", "F(", line),
    );
    if is_search(r",[^,\s]", &reduced) && is_search(r",[^,\s]", &raw[linenum]) {
        lint.error(
            linenum,
            "whitespace/comma",
            3,
            "Missing space after ,".to_string(),
        );
    }
    if is_search(r";[^\s};\\)/]", line) {
        lint.error(
            linenum,
            "whitespace/semicolon",
            3,
            "Missing space after ;".to_string(),
        );
    }
}

/// `_IsType`: does `expr` end in something that looks like a type name?
fn is_type(cl: &CleansedLines, nesting: &NestingState, expr: &str) -> bool {
    let token = pmatch(r".*(\b\S+)$", expr)
        .map_or(expr, |m| g(&m, 1))
        .to_string();
    if is_match(TYPES_PATTERN, &token) {
        return true;
    }
    let typename_pattern = format!(r"\b(?:typename|class|struct)\s+{}\b", escape(&token));
    let mut block_index = nesting.stack.len();
    while block_index > 0 {
        block_index -= 1;
        let block = &nesting.stack[block_index];
        if block.kind == Kind::Namespace {
            return false;
        }
        let last_line = block.starting_linenum;
        let next_block_start = if block_index > 0 {
            nesting.stack[block_index - 1].starting_linenum
        } else {
            0
        };
        let mut first_line: isize = isize::try_from(last_line).unwrap_or(0);
        let floor: isize = isize::try_from(next_block_start).unwrap_or(0);
        while first_line >= floor {
            if cl.elided[usize::try_from(first_line).unwrap_or(0)].contains("template") {
                break;
            }
            first_line -= 1;
        }
        if first_line < floor {
            continue;
        }
        let first = usize::try_from(first_line).unwrap_or(0);
        for i in first..=last_line {
            if is_search(&typename_pattern, &cl.elided[i]) {
                return true;
            }
        }
    }
    false
}

/// `CheckBracesSpacing`.
pub fn check_braces_spacing(
    lint: &mut Lint<'_>,
    cl: &CleansedLines,
    linenum: usize,
    nesting: &NestingState,
) {
    let line = &cl.elided[linenum];
    if let Some(m) = pmatch(r"(.*[^ ({>]){", line) {
        let leading_text = g(&m, 1);
        let (endline, endlinenum, endpos) = close_expression(cl, linenum, leading_text.len());
        let mut trailing_text = String::new();
        if let Some(endpos) = endpos {
            trailing_text.push_str(from(endline, endpos));
        }
        let stop = (endlinenum + 3).min(cl.num_lines.saturating_sub(1));
        for offset in (endlinenum + 1)..stop {
            trailing_text.push_str(&cl.elided[offset]);
        }
        if !is_match(r"[\s}]*[{.;,)<>\]:]", &trailing_text) && !is_type(cl, nesting, leading_text) {
            lint.error(
                linenum,
                "whitespace/braces",
                5,
                "Missing space before {".to_string(),
            );
        }
    }
    if is_search("}else", line) {
        lint.error(
            linenum,
            "whitespace/braces",
            5,
            "Missing space before else".to_string(),
        );
    }
    if is_search(r":\s*;\s*$", line) {
        lint.error(
            linenum,
            "whitespace/semicolon",
            5,
            "Semicolon defining empty statement. Use {} instead.".to_string(),
        );
    } else if is_search(r"^\s*;\s*$", line) {
        lint.error(
            linenum,
            "whitespace/semicolon",
            5,
            "Line contains only semicolon. If this should be an empty statement, use {} instead."
                .to_string(),
        );
    } else if is_search(r"\s+;\s*$", line) && !is_search(r"\bfor\b", line) {
        lint.error(
            linenum,
            "whitespace/semicolon",
            5,
            "Extra space before last semicolon. If this should be an empty statement, use {} instead.".to_string(),
        );
    }
}

/// `CheckSectionSpacing`: a blank line before `public:` and friends.
pub fn check_section_spacing(
    lint: &mut Lint<'_>,
    cl: &CleansedLines,
    class_info: &BlockInfo,
    linenum: usize,
) {
    if class_info
        .last_line
        .saturating_sub(class_info.starting_linenum)
        <= 24
        || class_info.last_line < class_info.starting_linenum
        || linenum <= class_info.starting_linenum
    {
        return;
    }
    let Some(m) = pmatch(r"\s*(public|protected|private):", &cl.lines[linenum]) else {
        return;
    };
    let prev_line = &cl.lines[linenum - 1];
    if !is_blank_line(prev_line)
        && !is_search(r"\b(class|struct)\b", prev_line)
        && !is_search(r"\\$", prev_line)
    {
        let mut end_class_head = class_info.starting_linenum;
        for i in class_info.starting_linenum..linenum {
            if is_search(r"\{\s*$", &cl.lines[i]) {
                end_class_head = i;
                break;
            }
        }
        if end_class_head + 1 < linenum {
            lint.error(
                linenum,
                "whitespace/blank_line",
                3,
                format!("\"{}:\" should be preceded by a blank line", g(&m, 1)),
            );
        }
    }
}

/// `CheckBraces`.
pub fn check_braces(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let line = &cl.elided[linenum];
    if is_match(r"\s*{\s*$", line) {
        let prevline = get_previous_non_blank_line(cl, linenum).0;
        let long_array =
            get_line_width(prevline) + 2 > lint.line_length() && prevline.contains("[]");
        if !(is_search(r"[,;:}{(]\s*$", prevline) || is_match(r"\s*#", prevline) || long_array) {
            lint.error(
                linenum,
                "whitespace/braces",
                4,
                "{ should almost always be at the end of the previous line".to_string(),
            );
        }
    }
    let mut last_wrong = false;
    if is_match(r"\s*else\b\s*(?:if\b|\{|$)", line) {
        let prevline = get_previous_non_blank_line(cl, linenum).0;
        if is_match(r"\s*}\s*$", prevline) {
            lint.error(
                linenum,
                "whitespace/newline",
                4,
                "An else should appear on the same line as the preceding }".to_string(),
            );
            last_wrong = true;
        }
    }
    if is_search(r"else if\s*\(", line) {
        let brace_on_left = is_search(r"}\s*else if\s*\(", line);
        if let Some(else_pos) = line.find("else if")
            && let Some(pos) = line[else_pos..].find('(').map(|p| p + else_pos)
            && pos > 0
        {
            let (endline, _, endpos) = close_expression(cl, linenum, pos);
            let brace_on_right = from(endline, endpos.unwrap_or(0)).contains('{');
            if brace_on_left != brace_on_right {
                lint.error(
                    linenum,
                    "readability/braces",
                    5,
                    "If an else has a brace on one side, it should have it on both".to_string(),
                );
            }
        }
    } else if is_search(r"}\s*else[^{]*$", line)
        || (is_match(r"[^}]*else\s*{", line) && !last_wrong)
    {
        lint.error(
            linenum,
            "readability/braces",
            5,
            "If an else has a brace on one side, it should have it on both".to_string(),
        );
    }
    let keyword = search(
        r"\b(else if|if|while|for|switch)\s*\(.*\)\s*(?:\[\[(?:un)?likely\]\]\s*)?{\s*[^\s\\};]",
        line,
    )
    .or_else(|| {
        search(
            r"\b(else|do|try)\s*(?:\[\[(?:un)?likely\]\]\s*)?{\s*[^\s\\}]",
            line,
        )
    });
    if let Some(m) = keyword {
        lint.error(
            linenum,
            "whitespace/newline",
            5,
            format!(
                "Controlled statements inside brackets of {} clause should be on a separate line",
                g(&m, 1)
            ),
        );
    }
    let Some(if_else_match) = search(r"\b(if\s*(|constexpr)\s*\(|else\b)", line) else {
        return;
    };
    if is_match(r"\s*#", line) {
        return;
    }
    let if_indent = get_indent_level(line);
    let mut endline = line.as_str();
    let mut endlinenum = linenum;
    let mut endpos = end(&if_else_match, 0);
    let if_match = search(r"\bif\s*(|constexpr)\s*\(", line);
    if let Some(m) = &if_match {
        let pos = end(m, 0) - 1;
        let (l, n, p) = close_expression(cl, linenum, pos);
        let Some(p) = p else {
            return;
        };
        endline = l;
        endlinenum = n;
        endpos = p;
    }
    let after = from(endline, endpos);
    if is_match(r"\s*(?:\[\[(?:un)?likely\]\]\s*)?{", after)
        || (is_match(r"\s*$", after)
            && endlinenum + 1 < cl.num_lines
            && is_match(r"\s*{", &cl.elided[endlinenum + 1]))
    {
        return;
    }
    while endlinenum < cl.num_lines && !from(&cl.elided[endlinenum], endpos).contains(';') {
        endlinenum += 1;
        endpos = 0;
    }
    if endlinenum >= cl.num_lines {
        return;
    }
    let endline = &cl.elided[endlinenum];
    let semi = endline.find(';').unwrap_or(0);
    if !is_match(r";[\s}]*(\\?)$", &endline[semi..]) {
        if !is_match(
            r"[^{};]*\[[^\[\]]*\][^{}]*\{[^{}]*\}\s*\)*[;,]\s*$",
            endline,
        ) {
            lint.error(
                linenum,
                "readability/braces",
                4,
                "If/else bodies with multiple statements require braces".to_string(),
            );
        }
    } else if endlinenum + 1 < cl.num_lines {
        let next_line = &cl.elided[endlinenum + 1];
        let next_indent = get_indent_level(next_line);
        if if_match.is_some() && is_match(r"\s*else\b", next_line) && next_indent != if_indent {
            lint.error(
                linenum,
                "readability/braces",
                4,
                "Else clause should be indented at the same level as if. Ambiguous nested if/else chains require braces."
                    .to_string(),
            );
        } else if next_indent > if_indent {
            lint.error(
                linenum,
                "readability/braces",
                4,
                "If/else bodies with multiple statements require braces".to_string(),
            );
        }
    }
}

/// `CheckTrailingSemicolon`: a `;` after a block's closing brace.
pub fn check_trailing_semicolon(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let line = &cl.elided[linenum];
    let mut brace_prefix: Option<usize> = None;
    if let Some(m) = pmatch(r"(.*\)\s*)\{", line) {
        brace_prefix = Some(g(&m, 1).len());
        let closing_paren_pos = g(&m, 1).rfind(')').unwrap_or(0);
        let (open_line, open_linenum, open_pos) =
            reverse_close_expression(cl, linenum, closing_paren_pos);
        if let Some(open_pos) = open_pos {
            let line_prefix = upto(open_line, open_pos);
            let macro_name =
                search(r"\b([A-Z_][A-Z0-9_]*)\s*$", line_prefix).map(|m| g(&m, 1).to_string());
            let func = pmatch(r"(.*\])\s*$", line_prefix).map(|m| g(&m, 1).to_string());
            let safe_macro = |name: &str| {
                matches!(
                    name,
                    "TEST"
                        | "TEST_F"
                        | "MATCHER"
                        | "MATCHER_P"
                        | "TYPED_TEST"
                        | "EXCLUSIVE_LOCKS_REQUIRED"
                        | "SHARED_LOCKS_REQUIRED"
                        | "LOCKS_EXCLUDED"
                        | "INTERFACE_DEF"
                )
            };
            if macro_name.as_deref().is_some_and(|n| !safe_macro(n))
                || func
                    .as_deref()
                    .is_some_and(|f| !is_search(r"\boperator\s*\[\s*\]", f))
                || is_search(r"\b(?:struct|union)\s+alignas\s*$", line_prefix)
                || is_search(r"\bdecltype$", line_prefix)
                || is_search(r"\brequires.*$", line_prefix)
                || is_search(r"\s+=\s*$", line_prefix)
            {
                brace_prefix = None;
            }
            if brace_prefix.is_some()
                && open_linenum > 1
                && is_search(r"\]\s*$", &cl.elided[open_linenum - 1])
            {
                brace_prefix = None;
            }
        }
    } else if let Some(m) = pmatch(r"(.*(?:else|\)\s*const)\s*)\{", line) {
        brace_prefix = Some(g(&m, 1).len());
    } else {
        let prevline = get_previous_non_blank_line(cl, linenum).0;
        if !prevline.is_empty()
            && is_search(r"[;{}]\s*$", prevline)
            && let Some(m) = pmatch(r"(\s*)\{", line)
        {
            brace_prefix = Some(g(&m, 1).len());
        }
    }
    if let Some(prefix_len) = brace_prefix {
        let (endline, endlinenum, endpos) = close_expression(cl, linenum, prefix_len);
        if let Some(endpos) = endpos
            && is_match(r"\s*;", from(endline, endpos))
        {
            let raw_lines = &cl.raw_lines;
            lint.parse_nolint_suppressions(&raw_lines[endlinenum - 1], endlinenum - 1);
            lint.parse_nolint_suppressions(&raw_lines[endlinenum], endlinenum);
            lint.error(
                endlinenum,
                "readability/braces",
                4,
                "You don't need a ; after a }".to_string(),
            );
        }
    }
}

/// `CheckEmptyBlockBody`.
pub fn check_empty_block_body(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let line = &cl.elided[linenum];
    let Some(matched) = pmatch(r"\s*(for|while|if)\s*\(", line) else {
        return;
    };
    let keyword = g(&matched, 1);
    let paren = line.find('(').unwrap_or(0);
    let (end_line, end_linenum, end_pos) = close_expression(cl, linenum, paren);
    let Some(end_pos) = end_pos else {
        return;
    };
    if is_match(";", from(end_line, end_pos)) {
        if keyword == "if" {
            lint.error(
                end_linenum,
                "whitespace/empty_conditional_body",
                5,
                "Empty conditional bodies should use {}".to_string(),
            );
        } else {
            lint.error(
                end_linenum,
                "whitespace/empty_loop_body",
                5,
                "Empty loop bodies should use {} or continue".to_string(),
            );
        }
    }
    if keyword != "if" {
        return;
    }
    let mut opening_linenum = end_linenum;
    let mut opening_line_fragment = from(end_line, end_pos).to_string();
    while !is_search(r"^\s*\{", &opening_line_fragment) {
        if is_search(r"^(?!\s*$)", &opening_line_fragment) {
            return;
        }
        opening_linenum += 1;
        if opening_linenum == cl.num_lines {
            return;
        }
        opening_line_fragment.clone_from(&cl.elided[opening_linenum]);
    }
    let opening_line = &cl.elided[opening_linenum];
    let mut opening_pos = opening_line_fragment.find('{').unwrap_or(0);
    if opening_linenum == end_linenum {
        opening_pos += end_pos;
    }
    let (closing_line, closing_linenum, closing_pos) =
        close_expression(cl, opening_linenum, opening_pos);
    let Some(closing_pos) = closing_pos else {
        return;
    };
    if cl.raw_lines[opening_linenum]
        != super::lines::cleanse_comments(&cl.raw_lines[opening_linenum])
    {
        return;
    }
    let body = if closing_linenum > opening_linenum {
        let mut parts: Vec<String> = vec![from(opening_line, opening_pos + 1).to_string()];
        parts.extend(
            cl.raw_lines[opening_linenum + 1..closing_linenum]
                .iter()
                .cloned(),
        );
        parts.push(upto(&cl.elided[closing_linenum], closing_pos.saturating_sub(1)).to_string());
        parts.join("\n")
    } else {
        sl(opening_line, opening_pos + 1, closing_pos.saturating_sub(1)).to_string()
    };
    if !is_search(r"(?s)^\s*$", &body) {
        return;
    }
    let mut current_linenum = closing_linenum;
    let mut current_line_fragment = from(closing_line, closing_pos).to_string();
    while is_search(r"^\s*$|^(?=\s*else)", &current_line_fragment) {
        if is_search(r"^(?=\s*else)", &current_line_fragment) {
            return;
        }
        current_linenum += 1;
        if current_linenum == cl.num_lines {
            break;
        }
        current_line_fragment.clone_from(&cl.elided[current_linenum]);
    }
    lint.error(
        end_linenum,
        "whitespace/empty_if_body",
        4,
        "If statement had no body and no else clause".to_string(),
    );
}

/// `FindCheckMacro`: a replaceable CHECK-like macro and where its `(` is.
fn find_check_macro(line: &str) -> Option<(&'static str, usize)> {
    for macro_name in CHECK_MACROS {
        if line.contains(macro_name)
            && let Some(m) = pmatch(&format!(r"(.*\b{macro_name}\s*)\("), line)
        {
            return Some((macro_name, g(&m, 1).len()));
        }
    }
    None
}

/// `CheckCheck`: `CHECK(a == b)` that should be `CHECK_EQ(a, b)`.
pub fn check_check(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let lines = &cl.elided;
    let Some((check_macro, start_pos)) = find_check_macro(&lines[linenum]) else {
        return;
    };
    let (last_line, end_line, end_pos) = close_expression(cl, linenum, start_pos);
    let Some(end_pos) = end_pos else {
        return;
    };
    if !is_match(r"\s*;", from(last_line, end_pos)) {
        return;
    }
    let mut expression = if linenum == end_line {
        sl(&lines[linenum], start_pos + 1, end_pos.saturating_sub(1)).to_string()
    } else {
        let mut e = from(&lines[linenum], start_pos + 1).to_string();
        for l in &lines[linenum + 1..end_line] {
            e.push_str(l);
        }
        e.push_str(upto(last_line, end_pos.saturating_sub(1)));
        e
    };
    let mut lhs = String::new();
    let mut rhs = String::new();
    let mut operator: Option<String> = None;
    while !expression.is_empty() {
        let matched = pmatch(
            r"\s*(<<|<<=|>>|>>=|->\*|->|&&|\|\||==|!=|>=|>|<=|<|\()(.*)$",
            &expression,
        );
        if let Some(m) = matched {
            let token = g(&m, 1).to_string();
            let rest = g(&m, 2).to_string();
            match token.as_str() {
                "(" => {
                    expression = rest;
                    let Scan::Found(end) =
                        find_end_of_expression_in_line(&expression, 0, vec!['('])
                    else {
                        return;
                    };
                    lhs.push('(');
                    lhs.push_str(&expression[..end]);
                    expression = expression[end..].to_string();
                }
                "&&" | "||" => return,
                "<<" | "<<=" | ">>" | ">>=" | "->*" | "->" => {
                    lhs.push_str(&token);
                    expression = rest;
                }
                _ => {
                    operator = Some(token);
                    rhs = rest;
                    break;
                }
            }
        } else {
            let m = pmatch(r"([^-=!<>()&|]+)(.*)$", &expression)
                .or_else(|| pmatch(r"(\s*\S)(.*)$", &expression));
            let Some(m) = m else {
                break;
            };
            let head = g(&m, 1).to_string();
            let rest = g(&m, 2).to_string();
            lhs.push_str(&head);
            expression = rest;
        }
    }
    let Some(operator) = operator else {
        return;
    };
    if lhs.is_empty() || rhs.is_empty() {
        return;
    }
    if rhs.contains("&&") || rhs.contains("||") {
        return;
    }
    let lhs = lhs.trim();
    let rhs = rhs.trim();
    let constant = r#"([-+]?(\d+|0[xX][0-9a-fA-F]+)[lLuU]{0,3}|".*"|'.*')$"#;
    if is_match(constant, lhs) || is_match(constant, rhs) {
        lint.error(
            linenum,
            "readability/check",
            2,
            format!(
                "Consider using {} instead of {check_macro}(a {operator} b)",
                check_replacement(check_macro, &operator)
            ),
        );
    }
}

/// `CheckAltTokens`.
pub fn check_alt_tokens(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let line = &cl.elided[linenum];
    if is_match(r"\s*#", line) {
        return;
    }
    if line.contains("/*") || line.contains("*/") {
        return;
    }
    for m in find_all(ALT_TOKEN_REPLACEMENT_PATTERN, line) {
        let word = g(&m, 2);
        lint.error(
            linenum,
            "readability/alt_tokens",
            2,
            format!(
                "Use operator {} instead of {word}",
                alt_token_replacement(word)
            ),
        );
    }
}

/// `CheckStyle`: the C++ style rules section.
pub fn check_style(
    lint: &mut Lint<'_>,
    cl: &CleansedLines,
    linenum: usize,
    file_extension: &str,
    nesting: &NestingState,
    cppvar: Option<&str>,
) {
    let raw_lines = &cl.lines_without_raw_strings;
    let line = &raw_lines[linenum];
    let prev = if linenum > 0 {
        raw_lines[linenum - 1].as_str()
    } else {
        ""
    };
    if line.contains('\t') {
        lint.error(
            linenum,
            "whitespace/tab",
            1,
            "Tab found; better to use spaces".to_string(),
        );
    }
    let scope_or_label_pattern =
        r"\s*(?:public|private|protected|signals)(?:\s+(?:slots\s*)?)?:\s*\\?$";
    let cleansed_line = &cl.elided[linenum];
    let initial_spaces = line.len() - line.trim_start_matches(' ').len();
    if !is_search(r#"[",=><] *$"#, prev)
        && (initial_spaces == 1 || initial_spaces == 3)
        && !is_match(scope_or_label_pattern, cleansed_line)
        && !(cl.raw_lines[linenum] != *line && is_match(r#"\s*"""#, line))
    {
        lint.error(
            linenum,
            "whitespace/indent",
            3,
            "Weird number of spaces at line-start.  Are you using a 2-space indent?".to_string(),
        );
    }
    if line.chars().last().is_some_and(char::is_whitespace) {
        lint.error(
            linenum,
            "whitespace/end_of_line",
            4,
            "Line ends in whitespace.  Consider deleting these extra spaces.".to_string(),
        );
    }
    let is_header_guard = lint.is_header_extension(file_extension)
        && cppvar.is_some_and(|v| {
            line.starts_with(&format!("#ifndef {v}"))
                || line.starts_with(&format!("#define {v}"))
                || line.starts_with(&format!("#endif  // {v}"))
        });
    if !line.starts_with("#include")
        && !is_header_guard
        && !is_match(r"\s*//.*http(s?)://\S*$", line)
        && !is_match(r"\s*//\s*[^\s]*$", line)
        && !is_match(r"// \$Id:.*#[0-9]+ \$$", line)
        && !is_match(r"\s*/// [@\\](copydoc|copydetails|copybrief) .*$", line)
    {
        let line_width = get_line_width(line);
        if line_width > lint.line_length() {
            lint.error(
                linenum,
                "whitespace/line_length",
                2,
                format!("Lines should be <= {} characters long", lint.line_length()),
            );
        }
    }
    if count(cleansed_line, ";") > 1
        && !is_match(r"[^{};]*\[[^\[\]]*\][^{}]*\{[^{}\n\r]*\}", line)
        && !cleansed_line.contains("for")
        && (!get_previous_non_blank_line(cl, linenum).0.contains("for")
            || get_previous_non_blank_line(cl, linenum).0.contains(';'))
        && !((cleansed_line.contains("case ") || cleansed_line.contains("default:"))
            && cleansed_line.contains("break;"))
    {
        lint.error(
            linenum,
            "whitespace/newline",
            0,
            "More than one command on the same line".to_string(),
        );
    }
    check_braces(lint, cl, linenum);
    check_trailing_semicolon(lint, cl, linenum);
    check_empty_block_body(lint, cl, linenum);
    check_spacing(lint, cl, linenum, nesting);
    check_operator_spacing(lint, cl, linenum);
    check_parenthesis_spacing(lint, cl, linenum);
    check_comma_spacing(lint, cl, linenum);
    check_braces_spacing(lint, cl, linenum, nesting);
    check_spacing_for_function_call(lint, cl, linenum);
    check_check(lint, cl, linenum);
    check_alt_tokens(lint, cl, linenum);
    if let Some(classinfo) = nesting.innermost_class() {
        check_section_spacing(lint, cl, classinfo, linenum);
    }
}

/// `CheckMakePairUsesDeduction`.
pub fn check_make_pair_uses_deduction(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    if is_search(r"\bmake_pair\s*<", &cl.elided[linenum]) {
        lint.error(
            linenum,
            "build/explicit_make_pair",
            4,
            "For C++11-compatibility, omit template arguments from make_pair OR use pair directly OR if appropriate, construct a pair directly".to_string(),
        );
    }
}

/// `CheckRedundantVirtual`.
pub fn check_redundant_virtual(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let line = &cl.elided[linenum];
    let Some(virtual_match) = pmatch(r"(.*)(\bvirtual\b)(.*)$", line) else {
        return;
    };
    if is_search(r"\b(public|protected|private)\s+$", g(&virtual_match, 1))
        || is_match(r"\s+(public|protected|private)\b", g(&virtual_match, 3))
    {
        return;
    }
    if is_match(r".*[^:]:[^:].*$", line) {
        return;
    }
    let mut end_col: Option<usize> = None;
    let mut end_line = 0;
    // cpplint starts the search at len("virtual"), not at the keyword's end.
    let mut start_col = g(&virtual_match, 2).len();
    let stop = (linenum + 3).min(cl.num_lines);
    for start_line in linenum..stop {
        let rest = from(&cl.elided[start_line], start_col);
        if let Some(params) = pmatch(r"([^(]*)\(", rest) {
            let (_, l, c) = close_expression(cl, start_line, start_col + g(&params, 1).len());
            end_line = l;
            end_col = c;
            break;
        }
        start_col = 0;
    }
    let Some(mut end_col) = end_col else {
        return;
    };
    let stop = (end_line + 3).min(cl.num_lines);
    for i in end_line..stop {
        let rest = from(&cl.elided[i], end_col);
        if let Some(m) = search(r"\b(override|final)\b", rest) {
            lint.error(
                linenum,
                "readability/inheritance",
                4,
                format!(
                    "\"virtual\" is redundant since function is already declared as \"{}\"",
                    g(&m, 1)
                ),
            );
        }
        end_col = 0;
        if is_search(r"[^\w]\s*$", rest) {
            break;
        }
    }
}

/// `CheckRedundantOverrideOrFinal`.
pub fn check_redundant_override_or_final(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let line = &cl.elided[linenum];
    let fragment = if let Some(declarator_end) = line.rfind(')') {
        &line[declarator_end..]
    } else if linenum > 1 && cl.elided[linenum - 1].contains(')') {
        line.as_str()
    } else {
        return;
    };
    if is_search(r"\boverride\b", fragment) && is_search(r"\bfinal\b", fragment) {
        lint.error(
            linenum,
            "readability/inheritance",
            4,
            "\"override\" is redundant since function is already declared as \"final\"".to_string(),
        );
    }
}

/// `FlagCxxHeaders`: unapproved C++11/17 headers.
pub fn flag_cxx_headers(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let line = &cl.elided[linenum];
    let Some(include) = pmatch(r#"\s*#\s*include\s+[<"]([^<"]+)[">]"#, line) else {
        return;
    };
    let name = g(&include, 1);
    if matches!(name, "cfenv" | "fenv.h" | "ratio") {
        lint.error(
            linenum,
            "build/c++11",
            5,
            format!("<{name}> is an unapproved C++11 header."),
        );
    }
    if name == "filesystem" {
        lint.error(
            linenum,
            "build/c++17",
            5,
            "<filesystem> is an unapproved C++17 header.".to_string(),
        );
    }
}

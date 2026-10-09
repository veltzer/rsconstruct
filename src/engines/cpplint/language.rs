//! cpplint's language-rule checks: includes (order, duplicates, what you
//! use, the file's own header), casts, printf, global strings, non-const
//! references, variable-length arrays.
//!
//! Every function here is a port of the cpplint 2.0.2 function of the same
//! name (snake-cased), with its messages and confidence levels verbatim.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use fancy_regex::Regex;

use super::lines::{CleansedLines, close_expression, from, reverse_close_expression, upto};
use super::nesting::{HeaderType, IncludeState, Kind, NestingState};
use super::regex::{end, g, group, is_match, is_search, pmatch, rx, search, split, start, sub};
use super::tables::{
    C_HEADERS, C_STANDARD_HEADER_FOLDERS_PATTERN, CPP_HEADERS, HEADERS_CONTAINING_TEMPLATES,
    HEADERS_FUNCTIONS, HEADERS_MAYBE_TEMPLATES, HEADERS_TYPES_OR_OBJS, TEST_FILE_SUFFIX,
    TEST_SUFFIXES, THIRD_PARTY_HEADERS_PATTERN,
};
use super::{IncludeOrder, Lint, paths};

/// `_DropCommonSuffixes`: `foo/foo_test.cc` → `foo/foo`, `foo/foo-inl.h` →
/// `foo/foo`.
fn drop_common_suffixes(lint: &Lint<'_>, filename: &str) -> String {
    let mut suffixes: Vec<String> = Vec::new();
    for test_suffix in TEST_SUFFIXES {
        for ext in lint.non_header_extensions() {
            suffixes.push(format!("{test_suffix}.{ext}"));
        }
    }
    for suffix in ["inl", "imp", "internal"] {
        for ext in lint.header_extensions() {
            suffixes.push(format!("{suffix}.{ext}"));
        }
    }
    for suffix in suffixes {
        if filename.ends_with(&suffix) && filename.len() > suffix.len() {
            let cut = filename.len() - suffix.len() - 1;
            if matches!(filename.as_bytes()[cut], b'-' | b'_') {
                return filename[..cut].to_string();
            }
        }
    }
    paths::splitext(filename).0.to_string()
}

/// `_ClassifyInclude`.
fn classify_include(lint: &Lint<'_>, include: &str, used_angle_brackets: bool) -> HeaderType {
    let is_cpp_header = CPP_HEADERS.contains(&include);
    let is_std_c_header = lint.include_order() == IncludeOrder::Default
        || C_HEADERS.contains(&include)
        || is_search(C_STANDARD_HEADER_FOLDERS_PATTERN, include);
    let include_ext = paths::splitext(include).1;
    let is_system = used_angle_brackets && !matches!(include_ext, ".hh" | ".hpp" | ".hxx" | ".h++");
    if is_system {
        if is_cpp_header {
            return HeaderType::CppSys;
        }
        if is_std_c_header {
            return HeaderType::CSys;
        }
        return HeaderType::OtherSys;
    }
    let target = drop_common_suffixes(lint, &lint.repository_name(lint.filename()));
    let (target_dir, target_base) = paths::split(&target);
    let dropped_include = drop_common_suffixes(lint, include);
    let (include_dir, include_base) = paths::split(&dropped_include);
    let target_dir_pub = paths::normpath(&format!("{target_dir}/../public"));
    if target_base == include_base && (include_dir == target_dir || include_dir == target_dir_pub) {
        return HeaderType::LikelyMy;
    }
    let first_component = |s: &str| pmatch(r"[^-_.]+", s).map(|m| g(&m, 0).to_string());
    if let (Some(t), Some(i)) = (first_component(target_base), first_component(include_base))
        && t == i
    {
        return HeaderType::PossibleMy;
    }
    HeaderType::Other
}

/// `CheckIncludeLine`.
fn check_include_line(
    lint: &mut Lint<'_>,
    cl: &CleansedLines,
    linenum: usize,
    include_state: &mut IncludeState,
) {
    let line = &cl.lines[linenum];
    if let Some(m) = pmatch(r#"#include\s*"([^/]+\.(.*))""#, line)
        && lint.is_header_extension(g(&m, 2))
        && !is_match(THIRD_PARTY_HEADERS_PATTERN, g(&m, 1))
    {
        lint.error(
            linenum,
            "build/include_subdir",
            4,
            "Include the directory when naming header files".to_string(),
        );
    }
    let Some(m) = search(super::lines::INCLUDE_PATTERN, line) else {
        return;
    };
    let include = g(&m, 2).to_string();
    let used_angle_brackets = g(&m, 1) == "<";
    if let Some(duplicate_line) = include_state.find_header(&include) {
        let filename = lint.filename().to_string();
        lint.error(
            linenum,
            "build/include",
            4,
            format!("\"{include}\" already included at {filename}:{duplicate_line}"),
        );
        return;
    }
    let repository_name = lint.repository_name(lint.filename());
    for extension in lint.non_header_extensions() {
        if include.ends_with(&format!(".{extension}"))
            && paths::dirname(&repository_name) != paths::dirname(&include)
        {
            lint.error(
                linenum,
                "build/include",
                4,
                format!("Do not include .{extension} files from other packages"),
            );
            return;
        }
    }
    let mut third_src_header = false;
    let filename = lint.filename().to_string();
    let own_extension = lint.file_extension_with_dot(&filename);
    let basefilename = &filename[..filename.len() - own_extension.len()];
    for ext in lint.header_extensions() {
        let headername = lint.repository_name(&format!("{basefilename}.{ext}"));
        if include.contains(&headername) || headername.contains(&include) {
            third_src_header = true;
            break;
        }
    }
    if third_src_header || !is_match(THIRD_PARTY_HEADERS_PATTERN, &include) {
        if let Some(section) = include_state.include_list.last_mut() {
            section.push((include.clone(), linenum));
        }
        let header_type = classify_include(lint, &include, used_angle_brackets);
        let error_message = include_state.check_next_include_order(header_type);
        if !error_message.is_empty() {
            let base = lint.base_name(&filename);
            lint.error(
                linenum,
                "build/include_order",
                4,
                format!("{error_message}. Should be: {base}.h, c system, c++ system, other."),
            );
        }
        let canonical_include = IncludeState::canonicalize_alphabetical_order(&include);
        if !include_state.is_in_alphabetical_order(cl, linenum, &canonical_include) {
            lint.error(
                linenum,
                "build/include_alpha",
                4,
                format!("Include \"{include}\" not in alphabetical order"),
            );
        }
        include_state.set_last_header(&canonical_include);
    }
}

/// `_GetTextInside`: the text between the opening punctuation that ends
/// the first match of `start_pattern` and its matching closer.
fn get_text_inside(text: &str, start_pattern: &str) -> Option<String> {
    let m = search(&format!("(?m){start_pattern}"), text)?;
    let start_position = end(&m, 0);
    let opener = text[..start_position].chars().last()?;
    let closer_of = |c: char| match c {
        '(' => Some(')'),
        '{' => Some('}'),
        '[' => Some(']'),
        _ => None,
    };
    let mut punctuation_stack = vec![closer_of(opener)?];
    let mut position = start_position;
    let mut last_char_len = 0;
    for (i, c) in text[start_position..].char_indices() {
        if punctuation_stack.is_empty() {
            break;
        }
        position = start_position + i;
        last_char_len = c.len_utf8();
        if Some(&c) == punctuation_stack.last() {
            punctuation_stack.pop();
        } else if matches!(c, ')' | '}' | ']') {
            return None;
        } else if let Some(closer) = closer_of(c) {
            punctuation_stack.push(closer);
        }
        position += last_char_len;
    }
    if !punctuation_stack.is_empty() {
        return None;
    }
    Some(text[start_position..position - last_char_len].to_string())
}

/// `CheckLanguage`.
pub fn check_language(
    lint: &mut Lint<'_>,
    cl: &CleansedLines,
    linenum: usize,
    file_extension: &str,
    include_state: &mut IncludeState,
) {
    let line = cl.elided[linenum].as_str();
    if line.is_empty() {
        return;
    }
    if is_search(super::lines::INCLUDE_PATTERN, line) {
        check_include_line(lint, cl, linenum, include_state);
        return;
    }
    if let Some(m) = pmatch(r"\s*#\s*(if|ifdef|ifndef|elif|else|endif)\b", line) {
        include_state.reset_section(g(&m, 1));
    }
    check_casts(lint, cl, linenum);
    check_global_static(lint, cl, linenum);
    check_printf(lint, cl, linenum);
    if is_search(r"\bshort port\b", line) {
        if !is_search(r"\bunsigned short port\b", line) {
            lint.error(
                linenum,
                "runtime/int",
                4,
                "Use \"unsigned short\" for ports, not \"short\"".to_string(),
            );
        }
    } else if let Some(m) = search(r"\b(short|long(?! +double)|long long)\b", line) {
        lint.error(
            linenum,
            "runtime/int",
            4,
            format!(
                "Use int16_t/int64_t/etc, rather than the C type {}",
                g(&m, 1)
            ),
        );
    }
    if is_search(r"\boperator\s*&\s*\(\s*\)", line) {
        lint.error(
            linenum,
            "runtime/operator",
            4,
            "Unary operator& is dangerous.  Do not use it.".to_string(),
        );
    }
    if is_search(r"\}\s*if\s*\(", line) {
        lint.error(
            linenum,
            "readability/braces",
            4,
            "Did you mean \"else if\"? If not, start a new line for \"if\".".to_string(),
        );
    }
    if let Some(printf_args) = get_text_inside(line, r"(?i)\b(string)?printf\s*\(")
        && let Some(m) = pmatch(r"([\w.\->()]+)$", &printf_args)
        && g(&m, 1) != "__VA_ARGS__"
    {
        let function_name = search(r"(?i)\b((?:string)?printf)\s*\(", line)
            .map_or("", |f| g(&f, 1))
            .to_string();
        lint.error(
            linenum,
            "runtime/printf",
            4,
            format!(
                "Potential format string bug. Do {function_name}(\"%s\", {}) instead.",
                g(&m, 1)
            ),
        );
    }
    if let Some(m) = search(r"memset\s*\(([^,]*),\s*([^,]*),\s*0\s*\)", line)
        && !is_match(r"''|-?[0-9]+|0x[0-9A-Fa-f]$", g(&m, 2))
    {
        lint.error(
            linenum,
            "runtime/memset",
            4,
            format!("Did you mean \"memset({}, 0, {})\"?", g(&m, 1), g(&m, 2)),
        );
    }
    if is_search(r"\busing namespace\b", line) {
        if is_search(r"\bliterals\b", line) {
            lint.error(
                linenum,
                "build/namespaces_literals",
                5,
                "Do not use namespace using-directives.  Use using-declarations instead."
                    .to_string(),
            );
        } else {
            lint.error(
                linenum,
                "build/namespaces",
                5,
                "Do not use namespace using-directives.  Use using-declarations instead."
                    .to_string(),
            );
        }
    }
    if let Some(m) = pmatch(r"\s*(.+::)?(\w+) [a-z]\w*\[(.+)];", line)
        && g(&m, 2) != "return"
        && g(&m, 2) != "delete"
        && !g(&m, 3).contains(']')
    {
        let mut is_const = true;
        let mut skip_next = false;
        for tok in split(r"\s|\+|\-|\*|\/|<<|>>]", g(&m, 3)) {
            if skip_next {
                skip_next = false;
                continue;
            }
            if is_search(r"sizeof\(.+\)", tok) || is_search(r"arraysize\(\w+\)", tok) {
                continue;
            }
            let tok = tok.trim_start_matches('(').trim_end_matches(')');
            if tok.is_empty()
                || is_match(r"\d+", tok)
                || is_match(r"0[xX][0-9a-fA-F]+", tok)
                || is_match(r"k[A-Z0-9]\w*", tok)
                || is_match(r"(.+::)?k[A-Z0-9]\w*", tok)
                || is_match(r"(.+::)?[A-Z][A-Z0-9_]*", tok)
            {
                continue;
            }
            if tok.starts_with("sizeof") {
                skip_next = true;
                continue;
            }
            is_const = false;
            break;
        }
        if !is_const {
            lint.error(
                linenum,
                "runtime/arrays",
                1,
                "Do not use variable-length arrays.  Use an appropriately named ('k' followed by CamelCase) compile-time constant for the size.".to_string(),
            );
        }
    }
    if lint.is_header_extension(file_extension)
        && is_search(r"\bnamespace\s*{", line)
        && !line.ends_with('\\')
    {
        lint.error(
            linenum,
            "build/namespaces_headers",
            4,
            "Do not use unnamed namespaces in header files.  See https://google-styleguide.googlecode.com/svn/trunk/cppguide.xml#Namespaces for more information.".to_string(),
        );
    }
}

/// `CheckGlobalStatic`.
fn check_global_static(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let mut line = cl.elided[linenum].clone();
    if linenum + 1 < cl.num_lines && !is_search(r"[;({]", &line) {
        line.push_str(cl.elided[linenum + 1].trim());
    }
    let matched = pmatch(
        r"((?:|static +)(?:|const +))(?::*std::)?string( +const)? +([a-zA-Z0-9_:]+)\b(.*)",
        &line,
    );
    if let Some(m) = matched
        && !is_search(r"\bstring\b(\s+const)?\s*[\*\&]\s*(const\s+)?\w", &line)
        && !is_search(r"\boperator\W", &line)
        && !is_match(r#"\s*(<.*>)?(::[a-zA-Z0-9_]+)*\s*\(([^"]|$)"#, g(&m, 4))
    {
        if is_search(r"\bconst\b", &line) {
            lint.error(
                linenum,
                "runtime/string",
                4,
                format!(
                    "For a static/global string constant, use a C style string instead: \"{}char{} {}[]\".",
                    g(&m, 1),
                    g(&m, 2),
                    g(&m, 3)
                ),
            );
        } else {
            lint.error(
                linenum,
                "runtime/string",
                4,
                "Static/global string variables are not permitted.".to_string(),
            );
        }
    }
    if is_search(r"\b([A-Za-z0-9_]*_)\(\1\)", &line)
        || is_search(r"\b([A-Za-z0-9_]*_)\(CHECK_NOTNULL\(\1\)\)", &line)
    {
        lint.error(
            linenum,
            "runtime/init",
            4,
            "You seem to be initializing a member variable with itself.".to_string(),
        );
    }
}

/// `CheckPrintf`.
fn check_printf(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let line = &cl.elided[linenum];
    if let Some(m) = search(r"snprintf\s*\(([^,]*),\s*([0-9]*)\s*,", line)
        && g(&m, 2) != "0"
    {
        lint.error(
            linenum,
            "runtime/printf",
            3,
            format!(
                "If you can, use sizeof({}) instead of {} as the 2nd arg to snprintf.",
                g(&m, 1),
                g(&m, 2)
            ),
        );
    }
    if is_search(r"\bsprintf\s*\(", line) {
        lint.error(
            linenum,
            "runtime/printf",
            5,
            "Never use sprintf. Use snprintf instead.".to_string(),
        );
    }
    if let Some(m) = search(r"\b(strcpy|strcat)\s*\(", line) {
        lint.error(
            linenum,
            "runtime/printf",
            4,
            format!("Almost always, snprintf is better than {}", g(&m, 1)),
        );
    }
}

/// `IsDerivedFunction`: the function this line belongs to is `override`.
fn is_derived_function(cl: &CleansedLines, linenum: usize) -> bool {
    let floor = linenum.saturating_sub(9);
    for i in (floor..=linenum).rev() {
        if let Some(m) = pmatch(r"([^()]*\w+)\(", &cl.elided[i]) {
            let (line, _, closing_paren) = close_expression(cl, i, g(&m, 1).len());
            return closing_paren.is_some_and(|p| is_search(r"\boverride\b", from(line, p)));
        }
    }
    false
}

/// `IsOutOfLineMethodDefinition`.
fn is_out_of_line_method_definition(cl: &CleansedLines, linenum: usize) -> bool {
    let floor = linenum.saturating_sub(9);
    for i in (floor..=linenum).rev() {
        if is_match(r"([^()]*\w+)\(", &cl.elided[i]) {
            return is_match(r"[^()]*\w+::\w+\(", &cl.elided[i]);
        }
    }
    false
}

/// `IsInitializerList`: is this line inside a constructor initializer list?
fn is_initializer_list(cl: &CleansedLines, linenum: usize) -> bool {
    let mut i = linenum;
    while i > 1 {
        let mut line = cl.elided[i].as_str();
        if i == linenum
            && let Some(m) = pmatch(r"(.*)\{\s*$", line)
        {
            line = g(&m, 1);
        }
        if is_search(r"\s:\s*\w+[({]", line) {
            return true;
        }
        if is_search(r"\}\s*,\s*$", line) {
            return true;
        }
        if is_search(r"[{};]\s*$", line) {
            return false;
        }
        i -= 1;
    }
    false
}

const RE_PATTERN_IDENT: &str = r"[_a-zA-Z]\w*";
const RE_PATTERN_TYPE: &str = r"(?:const\s+)?(?:typename\s+|class\s+|struct\s+|union\s+|enum\s+)?(?:\w|\s*<(?:<(?:<[^<>]*>|[^<>])*>|[^<>])*>|::)+";

/// `CheckForNonConstReference`.
pub fn check_for_non_const_reference(
    lint: &mut Lint<'_>,
    cl: &CleansedLines,
    linenum: usize,
    nesting: &NestingState,
) {
    let mut line = cl.elided[linenum].clone();
    if !line.contains('&') {
        return;
    }
    if is_derived_function(cl, linenum) {
        return;
    }
    if is_out_of_line_method_definition(cl, linenum) {
        return;
    }
    if linenum > 1 {
        let mut previous: Option<String> = None;
        if is_match(r"\s*::(?:[\w<>]|::)+\s*&\s*\S", &line) {
            previous = search(
                r"\b((?:const\s*)?(?:[\w<>]|::)+[\w<>])\s*$",
                &cl.elided[linenum - 1],
            )
            .map(|m| g(&m, 1).to_string());
        } else if is_match(r"\s*[a-zA-Z_]([\w<>]|::)+\s*&\s*\S", &line) {
            previous = search(
                r"\b((?:const\s*)?(?:[\w<>]|::)+::)\s*$",
                &cl.elided[linenum - 1],
            )
            .map(|m| g(&m, 1).to_string());
        }
        if let Some(prev) = previous {
            line = format!("{prev}{}", line.trim_start());
        } else if let Some(endpos) = line.rfind('>') {
            let (_, startline, startpos) = reverse_close_expression(cl, linenum, endpos);
            if startpos.is_some() && startline < linenum {
                line = String::new();
                for i in startline..=linenum {
                    line.push_str(cl.elided[i].trim());
                }
            }
        }
    }
    if nesting
        .previous_stack_top
        .is_some_and(|k| !matches!(k, Kind::Class | Kind::Namespace))
    {
        return;
    }
    if linenum > 0 {
        let floor = linenum.saturating_sub(10);
        let mut i = linenum - 1;
        while i > floor {
            let previous_line = &cl.elided[i];
            if !is_search(r"[),]\s*$", previous_line) {
                break;
            }
            if is_match(r"\s*:\s+\S", previous_line) {
                return;
            }
            i -= 1;
        }
    }
    if is_search(r"\\\s*$", &line) {
        return;
    }
    if is_initializer_list(cl, linenum) {
        return;
    }
    let allowed_functions =
        r"(?:[sS]wap(?:<\w:+>)?|operator\s*[<>][<>]|static_assert|COMPILE_ASSERT)\s*\(";
    if is_search(allowed_functions, &line) {
        return;
    }
    if !is_search(r"\S+\([^)]*$", &line) {
        for i in 0..2 {
            if linenum > i && is_search(allowed_functions, &cl.elided[linenum - i - 1]) {
                return;
            }
        }
    }
    let decls = sub(r"{[^}]*}", " ", &line);
    let ref_param = format!(
        r"({RE_PATTERN_TYPE}(?:\s*(?:\bconst\b|[*]))*\s*&\s*{RE_PATTERN_IDENT})\s*(?:=[^,()]+)?[,)]"
    );
    let const_ref_param = format!(
        r"(?:.*\s*\bconst\s*&\s*{RE_PATTERN_IDENT}|const\s+{RE_PATTERN_TYPE}\s*&\s*{RE_PATTERN_IDENT})"
    );
    let ref_stream_param = format!(r"(?:.*stream\s*&\s*{RE_PATTERN_IDENT})");
    for caps in super::regex::find_all(&ref_param, &decls) {
        let parameter = g(&caps, 1);
        if !is_match(&const_ref_param, parameter) && !is_match(&ref_stream_param, parameter) {
            lint.error(
                linenum,
                "runtime/references",
                2,
                format!(
                    "Is this a non-const reference? If so, make const or use a pointer: {}",
                    sub(" *<", "<", parameter)
                ),
            );
        }
    }
}

/// `CheckCasts`.
fn check_casts(lint: &mut Lint<'_>, cl: &CleansedLines, linenum: usize) {
    let line = &cl.elided[linenum];
    let matched = search(
        r"(\bnew\s+(?:const\s+)?|\S<\s*(?:const\s+)?)?\b(int|float|double|bool|char|int16_t|uint16_t|int32_t|uint32_t|int64_t|uint64_t)(\([^)].*)",
        line,
    );
    let expecting_function = expecting_function_args(cl, linenum);
    if let Some(m) = matched
        && !expecting_function
    {
        let matched_type = g(&m, 2);
        let matched_new_or_template = group(&m, 1);
        if is_match(r"\([^()]+\)\s*\[", g(&m, 3)) {
            return;
        }
        let matched_funcptr = g(&m, 3);
        let function_pointer = !matched_funcptr.is_empty()
            && (is_match(r"\((?:[^() ]+::\s*\*\s*)?[^() ]+\)\s*\(", matched_funcptr)
                || matched_funcptr.starts_with("(*)"));
        if matched_new_or_template.is_none()
            && !function_pointer
            && !is_match(&format!(r"\s*using\s+\S+\s*=\s*{matched_type}"), line)
            && !is_search(&format!(r"new\(\S+\)\s*{matched_type}"), line)
        {
            lint.error(
                linenum,
                "readability/casting",
                4,
                format!(
                    "Using deprecated casting style.  Use static_cast<{matched_type}>(...) instead"
                ),
            );
        }
    }
    if !expecting_function {
        check_c_style_cast(
            lint,
            cl,
            linenum,
            "static_cast",
            r"\((int|float|double|bool|char|u?int(16|32|64)_t|size_t)\)",
        );
    }
    if !check_c_style_cast(
        lint,
        cl,
        linenum,
        "const_cast",
        r#"\((char\s?\*+\s?)\)\s*""#,
    ) {
        check_c_style_cast(lint, cl, linenum, "reinterpret_cast", r"\((\w+\s?\*+\s?)\)");
    }
    if is_search(
        r"(?:[^\w]&\(([^)*][^)]*)\)[\w(])|(?:[^\w]&(static|dynamic|down|reinterpret)_cast\b)",
        line,
    ) {
        let mut parenthesis_error = false;
        if let Some(m) = pmatch(r"(.*&(?:static|dynamic|down|reinterpret)_cast\b)<", line) {
            let (_, y1, x1) = close_expression(cl, linenum, g(&m, 1).len());
            if let Some(x1) = x1
                && super::lines::char_at(&cl.elided[y1], x1) == Some('(')
            {
                let (_, y2, x2) = close_expression(cl, y1, x1);
                if let Some(x2) = x2 {
                    let mut extended_line = from(&cl.elided[y2], x2).to_string();
                    if y2 + 1 < cl.num_lines {
                        extended_line.push_str(&cl.elided[y2 + 1]);
                    }
                    if is_match(r"\s*(?:->|\[)", &extended_line) {
                        parenthesis_error = true;
                    }
                }
            }
        }
        if parenthesis_error {
            lint.error(
                linenum,
                "readability/casting",
                4,
                "Are you taking an address of something dereferenced from a cast?  Wrapping the dereferenced expression in parentheses will make the binding more obvious".to_string(),
            );
        } else {
            lint.error(
                linenum,
                "runtime/casting",
                4,
                "Are you taking an address of a cast?  This is dangerous: could be a temp var.  Take the address before doing the cast, rather than after".to_string(),
            );
        }
    }
}

/// `CheckCStyleCast`: true when an error was reported.
fn check_c_style_cast(
    lint: &mut Lint<'_>,
    cl: &CleansedLines,
    linenum: usize,
    cast_type: &str,
    pattern: &str,
) -> bool {
    let line = &cl.elided[linenum];
    let Some(m) = search(pattern, line) else {
        return false;
    };
    let mut context = upto(line, start(&m, 1).saturating_sub(1)).to_string();
    if is_match(
        r".*\b(?:sizeof|alignof|alignas|[_A-Z][_A-Z0-9]*)\s*$",
        &context,
    ) {
        return false;
    }
    if linenum > 0 {
        let floor = linenum.saturating_sub(5);
        let mut i = linenum - 1;
        while i > floor {
            context = format!("{}{context}", cl.elided[i]);
            i -= 1;
        }
    }
    if is_match(r".*\b[_A-Z][_A-Z0-9]*\s*\((?:\([^()]*\)|[^()])*$", &context) {
        return false;
    }
    if [" operator++", " operator--", "::operator++", "::operator--"]
        .iter()
        .any(|suffix| context.ends_with(suffix))
    {
        return false;
    }
    let remainder = from(line, end(&m, 0));
    if is_match(
        r"\s*(?:;|const\b|throw\b|final\b|override\b|[=>{),]|->)",
        remainder,
    ) {
        return false;
    }
    lint.error(
        linenum,
        "readability/casting",
        4,
        format!(
            "Using C-style cast.  Use {cast_type}<{}>(...) instead",
            g(&m, 1)
        ),
    );
    true
}

/// `ExpectingFunctionArgs`: inside a `MOCK_METHOD` or `std::function<`.
fn expecting_function_args(cl: &CleansedLines, linenum: usize) -> bool {
    let line = &cl.elided[linenum];
    is_match(r"\s*MOCK_(CONST_)?METHOD\d+(_T)?\(", line)
        || (linenum >= 2
            && (is_match(
                r"\s*MOCK_(?:CONST_)?METHOD\d+(?:_T)?\((?:\S+,)?\s*$",
                &cl.elided[linenum - 1],
            ) || is_match(
                r"\s*MOCK_(?:CONST_)?METHOD\d+(?:_T)?\(\s*$",
                &cl.elided[linenum - 2],
            ) || is_search(r"\bstd::m?function\s*<\s*$", &cl.elided[linenum - 1])))
}

struct IwyuPattern {
    regex: &'static Regex,
    item: String,
    header: &'static str,
}

static IWYU_TYPES_OR_OBJS: LazyLock<Vec<IwyuPattern>> = LazyLock::new(|| {
    let mut out = Vec::new();
    for (header, items) in HEADERS_TYPES_OR_OBJS {
        for item in *items {
            out.push(IwyuPattern {
                regex: rx(&format!(r"\b{item}\b")),
                item: (*item).to_string(),
                header,
            });
        }
    }
    out
});

static IWYU_FUNCTIONS: LazyLock<Vec<IwyuPattern>> = LazyLock::new(|| {
    let mut out = Vec::new();
    for (header, items) in HEADERS_FUNCTIONS {
        for item in *items {
            out.push(IwyuPattern {
                regex: rx(&format!(r"([^>.]|^)\b{item}\([^\)]")),
                item: (*item).to_string(),
                header,
            });
        }
    }
    out
});

static IWYU_MAYBE_TEMPLATES: LazyLock<Vec<IwyuPattern>> = LazyLock::new(|| {
    let mut out = Vec::new();
    for (header, items) in HEADERS_MAYBE_TEMPLATES {
        for item in *items {
            out.push(IwyuPattern {
                regex: rx(&format!(r"((\bstd::)|[^>.:])\b{item}(<.*?>)?\([^\)]")),
                item: (*item).to_string(),
                header,
            });
        }
    }
    out.push(IwyuPattern {
        regex: rx(r"(std\b::\bmap\s*<)|(^(std\b::\b)map\b\(\s*<)"),
        item: "map<>".to_string(),
        header: "<map>",
    });
    out
});

static IWYU_TEMPLATES: LazyLock<Vec<IwyuPattern>> = LazyLock::new(|| {
    let mut out = Vec::new();
    for (header, items) in HEADERS_CONTAINING_TEMPLATES {
        for item in *items {
            out.push(IwyuPattern {
                regex: rx(&format!(r"((^|(^|\s|((^|\W)::))std::)|[^>.:]\b){item}\s*<")),
                item: format!("{item}<>"),
                header,
            });
        }
    }
    out
});

fn iwyu_match(pattern: &IwyuPattern, line: &str) -> Option<usize> {
    pattern
        .regex
        .find(line)
        .unwrap_or_else(|e| {
            panic!(
                "cpplint: regular expression {:?} failed: {e}",
                pattern.regex.as_str()
            )
        })
        .map(|m| m.start())
}

/// `CheckForIncludeWhatYouUse`: STL headers the file uses but does not
/// include.
pub fn check_for_include_what_you_use(
    lint: &mut Lint<'_>,
    cl: &CleansedLines,
    include_state: &IncludeState,
) {
    let mut required: BTreeMap<&'static str, (usize, String)> = BTreeMap::new();
    for linenum in 0..cl.num_lines {
        let line = cl.elided[linenum].as_str();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        for pattern in IWYU_TYPES_OR_OBJS.iter().chain(IWYU_FUNCTIONS.iter()) {
            if let Some(pos) = iwyu_match(pattern, line) {
                let prefix = &line[..pos];
                if prefix.ends_with("std::") || !prefix.ends_with("::") {
                    required.insert(pattern.header, (linenum, pattern.item.clone()));
                }
            }
        }
        for pattern in IWYU_MAYBE_TEMPLATES.iter() {
            if iwyu_match(pattern, line).is_some() {
                required.insert(pattern.header, (linenum, pattern.item.clone()));
            }
        }
        if !line.contains('<') {
            continue;
        }
        for pattern in IWYU_TEMPLATES.iter() {
            if let Some(pos) = iwyu_match(pattern, line) {
                let prefix = &line[..pos];
                if prefix.ends_with("std::") || !prefix.ends_with("::") {
                    required.insert(pattern.header, (linenum, pattern.item.clone()));
                }
            }
        }
    }
    let included: BTreeSet<&str> = include_state
        .include_list
        .iter()
        .flatten()
        .map(|(h, _)| h.as_str())
        .collect();
    let mut ordered: Vec<(&'static str, &(usize, String))> =
        required.iter().map(|(h, v)| (*h, v)).collect();
    ordered.sort_by(|a, b| a.1.cmp(b.1));
    for (header, (linenum, template)) in ordered {
        let header_stripped = header.trim_matches(|c| c == '<' || c == '>' || c == '"');
        let c_alias = header_stripped
            .strip_prefix('c')
            .is_some_and(|rest| included.contains(format!("{rest}.h").as_str()));
        if !included.contains(header_stripped) && !c_alias {
            lint.error(
                *linenum,
                "build/include_what_you_use",
                4,
                format!("Add #include {header} for {template}"),
            );
        }
    }
}

/// `CheckHeaderFileIncluded`: a source file includes its own header.
pub fn check_header_file_included(lint: &mut Lint<'_>, include_state: &IncludeState) {
    let filename = lint.filename().to_string();
    if is_search(TEST_FILE_SUFFIX, &lint.base_name(&filename)) {
        return;
    }
    let mut first_include: Option<usize> = None;
    let mut message: Option<String> = None;
    let extension = lint.file_extension_with_dot(&filename);
    let basefilename = &filename[..filename.len() - extension.len()];
    for ext in lint.header_extensions() {
        let headerfile = format!("{basefilename}.{ext}");
        if !paths::exists(&headerfile) {
            continue;
        }
        let headername = lint.repository_name(&headerfile);
        let mut include_uses_unix_dir_aliases = false;
        for (include_text, line) in include_state.include_list.iter().flatten() {
            if include_text.contains("./") {
                include_uses_unix_dir_aliases = true;
            }
            if include_text.contains(&headername) || headername.contains(include_text.as_str()) {
                return;
            }
            if first_include.is_none() {
                first_include = Some(*line);
            }
        }
        let mut text = format!(
            "{} should include its header file {headername}",
            lint.repository_name(&filename)
        );
        if include_uses_unix_dir_aliases {
            text.push_str(". Relative paths like . and .. are not allowed.");
        }
        message = Some(text);
    }
    if let Some(message) = message {
        lint.error(
            first_include.unwrap_or(super::LINE_NONE),
            "build/include",
            5,
            message,
        );
    }
}

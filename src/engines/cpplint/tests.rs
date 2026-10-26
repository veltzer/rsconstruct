//! Unit tests for the cpplint port: the regular-expression dialect, the
//! line views and a few whole-file runs against cpplint's known output.

use std::fs;

use super::lines::{
    CleansedLines, cleanse_comments, close_expression, collapse_strings, get_line_width,
};
use super::regex::{rx, search, take_failure};
use super::{Finding, Options, Outcome, lint_file};

#[test]
fn python_escapes_compile() {
    // cpplint escapes punctuation Python allows; the dialect here must too.
    for pattern in [
        r"\/=|\%=",
        r"(///|//\!)",
        r"[\*\&]",
        r"\s|\+|\-|\*|\/|<<|>>]",
        r"[\w.\->()]+",
        r"(?:[-+*/=%^&|(<]\s*|>\s+)rand\(\)",
        r"^(?:(char(16_t|32_t)?)|wchar_t|)$",
        r"\s*(\w*)\s*::\s*\1\s*\(",
        r"\w\s*\(\s(?!\s*\\$)",
    ] {
        let _ = rx(pattern);
    }
}

#[test]
fn cleansing_matches_cpplint() {
    assert_eq!(cleanse_comments("int a;  // c"), "int a;");
    assert_eq!(cleanse_comments("int /* x */ a;"), "int a;");
    // A `//` inside a string makes cpplint keep the line as it is.
    assert_eq!(cleanse_comments("f(\"//\");  // c"), "f(\"//\");  // c");
    assert_eq!(cleanse_comments("f(\"a\");  // c"), "f(\"a\");");
    assert_eq!(
        collapse_strings("f(\"a\\\"b\", 'c', 1'000);"),
        "f(\"\", '', 1000);"
    );
    assert_eq!(collapse_strings("#include \"a/b.h\""), "#include \"a/b.h\"");
    let cl = CleansedLines::new(
        vec![
            "R\"(raw".to_string(),
            "text)\" + f(x,".to_string(),
            "       y);".to_string(),
        ],
        false,
    );
    assert_eq!(cl.lines_without_raw_strings[0], "\"\"");
    assert_eq!(cl.lines_without_raw_strings[1], "\"\" + f(x,");
    assert_eq!(close_expression(&cl, 1, 6), ("       y);", 2, Some(9)));
    assert_eq!(get_line_width("abc"), 3);
    assert_eq!(get_line_width("日本"), 4);
    assert_eq!(get_line_width("e\u{301}"), 1);
}

fn lint_source(name: &str, source: &str, options: &Options) -> Vec<String> {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir(dir.path().join(".git")).expect(".git");
    let path = dir.path().join(name);
    fs::write(&path, source).expect("write");
    // cpplint works from the current directory; run from the temp project.
    // The directory is process-wide, so the switch is serialized and the
    // original directory is read under the same lock.
    let result = {
        let _guard = CWD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(dir.path()).expect("chdir");
        let result = lint_file(name, options);
        std::env::set_current_dir(cwd).expect("chdir back");
        result
    };
    match result.expect("lint") {
        Outcome::Linted(findings) => findings.iter().map(|f: &Finding| f.render(name)).collect(),
        Outcome::Excluded => vec!["excluded".to_string()],
    }
}

static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn clean_header_passes() {
    let source = "// Copyright 2024 Example\n#ifndef FOO_H_\n#define FOO_H_\n\nclass Foo {\n public:\n  Foo();\n};\n\n#endif  // FOO_H_\n";
    assert_eq!(
        lint_source("foo.h", source, &Options::default()),
        Vec::<String>::new()
    );
}

#[test]
fn findings_render_like_cpplint() {
    let source = "int main(){\n\tint x = (int)3.5;\n  return x;\n}\n";
    let findings = lint_source("main.cc", source, &Options::default());
    assert_eq!(
        findings,
        vec![
            "main.cc:0:  No copyright message found.  You should have a line: \"Copyright [year] <Copyright Owner>\"  [legal/copyright] [5]",
            "main.cc:1:  Missing space before {  [whitespace/braces] [5]",
            "main.cc:2:  Tab found; better to use spaces  [whitespace/tab] [1]",
            "main.cc:2:  Using C-style cast.  Use static_cast<int>(...) instead  [readability/casting] [4]",
        ]
    );
}

#[test]
fn filters_and_nolint_apply() {
    let source = "// Copyright 2024 Example\nint main(){\n\tint x = (int)3.5;  // NOLINT(readability/casting)\n  return x;\n}\n";
    let options = Options {
        filters: vec!["-whitespace/braces".to_string()],
        ..Options::default()
    };
    assert_eq!(
        lint_source("main.cc", source, &options),
        vec!["main.cc:3:  Tab found; better to use spaces  [whitespace/tab] [1]"]
    );
}

/// `CheckRedundantVirtual` searches from byte 7 of the line (cpplint's
/// `len("virtual")`); a multi-byte character straddling that byte moves
/// the slice back to its boundary, and the opener's offset must follow.
/// The findings are cpplint 2.0.2's on this file.
#[test]
fn redundant_virtual_survives_a_multibyte_character_before_it() {
    let source = "// Copyright 2024 Example\n#ifndef V_H_\n#define V_H_\nclass A {\n public:\n  int ab\u{e9}; virtual void f() override;\n  int ab; virtual void g() override;\n};\n#endif  // V_H_\n";
    assert_eq!(
        lint_source("v.h", source, &Options::default()),
        vec![
            "v.h:6:  \"virtual\" is redundant since function is already declared as \"override\"  [readability/inheritance] [4]",
            "v.h:7:  \"virtual\" is redundant since function is already declared as \"override\"  [readability/inheritance] [4]",
        ]
    );
}

/// A match fancy-regex gives up on (its backtrack limit; Python's `re` has
/// none) is not a non-match: it is recorded, and `lint_file` fails the
/// file with it instead of reporting cpplint-unlike findings.
#[test]
fn a_match_over_the_backtrack_limit_fails_the_file() {
    assert!(take_failure().is_none());
    // Nested quantifiers around a lookahead keep the match in the
    // backtracking engine, where it fails after exponentially many steps.
    let pattern = r"(?:(?=a)a+)+b";
    let text = "a".repeat(40);
    assert!(search(pattern, &text).is_none());
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir(dir.path().join(".git")).expect(".git");
    let path = dir.path().join("main.cc");
    fs::write(
        &path,
        "// Copyright 2024 Example\nint main() { return 0; }\n",
    )
    .expect("write");
    let result = {
        let _guard = CWD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(dir.path()).expect("chdir");
        let result = lint_file("main.cc", &Options::default());
        std::env::set_current_dir(cwd).expect("chdir back");
        result
    };
    let err = result.expect_err("the recorded failure fails the file");
    let message = err.to_string();
    assert!(
        message.contains("checking main.cc: regular expression")
            && message.contains(pattern)
            && message.contains("backtracking count exceeded"),
        "{message}"
    );
    // Collected: the next file on this thread starts clean.
    assert!(take_failure().is_none());
}

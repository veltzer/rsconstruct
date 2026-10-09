//! Rule behaviour, checked against the PASS/FAIL examples in yamllint's own
//! rule documentation. Each test configures one rule the way the example
//! does and asserts the problems (line, column, rule) yamllint reports.

use std::path::Path;

use super::{Config, Level, Problem, lint};

fn conf(text: &str) -> Config {
    Config::parse(text, Path::new("")).expect("test config parses")
}

/// Problems as `(line, column, rule)` triples, in report order.
fn problems(config: &Config, yaml: &str) -> Vec<(usize, usize, &'static str)> {
    lint(yaml, config, None)
        .iter()
        .map(|p| (p.line, p.column, p.rule.unwrap_or("syntax")))
        .collect()
}

/// A config enabling only the rules named, each with the given settings.
fn only(rules: &str) -> Config {
    conf(&format!("rules:\n{rules}"))
}

fn messages(config: &Config, yaml: &str) -> Vec<String> {
    lint(yaml, config, None)
        .iter()
        .map(Problem::message)
        .collect()
}

#[test]
fn default_config_passes_a_clean_document() {
    let config = Config::default_conf();
    let yaml = "---\nkey: value\nlist:\n  - a\n  - b\nflow: {a: 1, b: [1, 2]}\n";
    assert_eq!(problems(&config, yaml), vec![]);
}

#[test]
fn syntax_errors_are_reported_once_at_their_position() {
    let config = Config::default_conf();
    let yaml = "---\nkey: [1, 2\nother: 3\n";
    let found = lint(yaml, &config, None);
    let syntax: Vec<&Problem> = found.iter().filter(|p| p.rule.is_none()).collect();
    assert_eq!(syntax.len(), 1, "{found:?}");
    assert!(
        syntax[0].desc.starts_with("syntax error: "),
        "{}",
        syntax[0].desc
    );
    assert_eq!(syntax[0].level, Level::Error);
}

#[test]
fn disable_file_directive_silences_everything() {
    let config = Config::default_conf();
    assert_eq!(
        problems(&config, "# yamllint disable-file\nkey: value   \n"),
        vec![]
    );
}

#[test]
fn disable_and_enable_directives() {
    let config = only("  trailing-spaces: enable\n  colons: enable\n");
    let yaml = "a: 1  \n# yamllint disable rule:trailing-spaces\nb: 2  \n# yamllint enable rule:trailing-spaces\nc: 3  \n# yamllint disable\nd  : 4  \n";
    assert_eq!(
        problems(&config, yaml),
        vec![(1, 5, "trailing-spaces"), (5, 5, "trailing-spaces")]
    );
}

#[test]
fn disable_line_directive_applies_to_its_line_or_the_next() {
    let config = only("  trailing-spaces: enable\n");
    let yaml = "a: 1  # yamllint disable-line\n# yamllint disable-line rule:trailing-spaces\nb: 2  \nc: 3  \n";
    // Line 1 has no trailing spaces (the comment ends the line); line 3 is
    // covered by the directive on line 2; line 4 is not.
    assert_eq!(problems(&config, yaml), vec![(4, 5, "trailing-spaces")]);
}

#[test]
fn anchors() {
    let config = only("  anchors: enable\n");
    assert_eq!(
        problems(&config, "---\n- &anchor\n  foo: bar\n- *anchor\n"),
        vec![]
    );
    assert_eq!(
        problems(&config, "---\n- &anchor\n  foo: bar\n- *unknown\n"),
        vec![(4, 3, "anchors")]
    );
    let config = only("  anchors:\n    forbid-duplicated-anchors: true\n");
    assert_eq!(
        problems(
            &config,
            "---\n- &anchor Foo Bar\n- &anchor [item 1, item 2]\n"
        ),
        vec![(3, 3, "anchors")]
    );
    let config = only("  anchors:\n    forbid-unused-anchors: true\n");
    assert_eq!(
        problems(
            &config,
            "---\n- &anchor\n  foo: bar\n- items:\n  - item1\n  - item2\n"
        ),
        vec![(2, 3, "anchors")]
    );
}

#[test]
fn braces_and_brackets() {
    let config = only("  braces:\n    forbid: true\n  brackets:\n    forbid: non-empty\n");
    // `forbid` reports the opening bracket; the spacing check still runs on
    // the closing one, as it does in yamllint.
    assert_eq!(
        problems(&config, "object: { key1: 4 }\n"),
        vec![(1, 10, "braces"), (1, 18, "braces")]
    );
    assert_eq!(problems(&config, "object: []\n"), vec![]);
    assert_eq!(
        problems(&config, "object: [ 1, 2 ]\n"),
        vec![(1, 10, "brackets"), (1, 15, "brackets")]
    );

    let config = only("  braces:\n    min-spaces-inside: 1\n    max-spaces-inside: 3\n");
    assert_eq!(problems(&config, "object: { key1: 4, key2: 8 }\n"), vec![]);
    assert_eq!(
        problems(&config, "object: { key1: 4, key2: 8   }\n"),
        vec![]
    );
    assert_eq!(
        problems(&config, "object: {    key1: 4, key2: 8   }\n"),
        vec![(1, 13, "braces")]
    );
    assert_eq!(
        problems(&config, "object: {key1: 4, key2: 8 }\n"),
        vec![(1, 10, "braces")]
    );

    let config =
        only("  brackets:\n    min-spaces-inside-empty: 1\n    max-spaces-inside-empty: -1\n");
    // yamllint's docs say `[         ]` passes here, but yamllint itself
    // reports it: `max-spaces-inside-empty: -1` falls back to
    // `max-spaces-inside` (0), not to "no limit". The port does what the
    // tool does.
    assert_eq!(
        problems(&config, "object: [         ]\n"),
        vec![(1, 18, "brackets")]
    );
    assert_eq!(problems(&config, "object: []\n"), vec![(1, 10, "brackets")]);
}

#[test]
fn colons() {
    let config = only("  colons: enable\n");
    assert_eq!(
        problems(&config, "object:\n  - a\n  - b\nkey: value\n"),
        vec![]
    );
    assert_eq!(
        problems(&config, "object  :\n  - a\n"),
        vec![(1, 8, "colons")]
    );
    let config = only("  colons:\n    max-spaces-after: 2\n");
    assert_eq!(
        problems(&config, "first:  1\nsecond: 2\nthird:  3\n"),
        vec![]
    );
    assert_eq!(
        problems(&config, "first: 1\n2nd:   2\nthird: 3\n"),
        vec![(2, 7, "colons")]
    );
}

#[test]
fn commas() {
    let config = only("  commas: enable\n");
    assert_eq!(
        problems(&config, "strange var:\n  [10, 20, 30, {x: 1, y: 2}]\n"),
        vec![]
    );
    assert_eq!(
        problems(&config, "strange var:\n  [10, 20 , 30, {x: 1, y: 2}]\n"),
        vec![(2, 10, "commas")]
    );
    assert_eq!(
        problems(&config, "strange var:\n  [10, 20,30,   {x: 1,   y: 2}]\n"),
        vec![(2, 11, "commas"), (2, 16, "commas"), (2, 25, "commas")]
    );
    let config = only("  commas:\n    max-spaces-before: -1\n");
    // yamllint's docs show this as passing; yamllint itself reports the
    // three spaces after the comma on line 4 (`max-spaces-after` is still
    // 1). The port does what the tool does.
    assert_eq!(
        problems(
            &config,
            "strange var:\n  [10,\n   20   , 30\n   ,   {x: 1, y: 2}]\n"
        ),
        vec![(4, 7, "commas")]
    );
}

#[test]
fn comments() {
    let config = only("  comments: enable\n");
    assert_eq!(
        problems(&config, "# This sentence\n# is a block comment\n"),
        vec![]
    );
    assert_eq!(
        problems(
            &config,
            "##############################\n## This is some documentation\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(&config, "#This sentence\n#is a block comment\n"),
        vec![(1, 2, "comments"), (2, 2, "comments")]
    );
    assert_eq!(
        problems(&config, "x: 2 ^ 127 - 1  # Mersenne prime number\n"),
        vec![]
    );
    assert_eq!(
        problems(&config, "x: 2 ^ 127 - 1 # Mersenne prime number\n"),
        vec![(1, 16, "comments")]
    );
    assert_eq!(
        problems(&config, "#!/usr/bin/env something\nkey: value\n"),
        vec![]
    );
}

#[test]
fn comments_indentation() {
    let config = only("  comments-indentation: enable\n");
    assert_eq!(
        problems(&config, "# Fibonacci\n[0, 1, 1, 2, 3, 5]\n"),
        vec![]
    );
    assert_eq!(
        problems(&config, "  # Fibonacci\n[0, 1, 1, 2, 3, 5]\n"),
        vec![(1, 3, "comments-indentation")]
    );
    assert_eq!(
        problems(&config, "list:\n    - 2\n    - 3\n    # - 4\n    - 5\n"),
        vec![]
    );
    assert_eq!(
        problems(&config, "list:\n    - 2\n    - 3\n#    - 4\n    - 5\n"),
        vec![(4, 1, "comments-indentation")]
    );
    assert_eq!(
        problems(
            &config,
            "# This is the first object\nobj1:\n  - item A\n  # - item B\n# This is the second object\nobj2: []\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(&config, "# This sentence\n # is a block comment\n"),
        vec![(2, 2, "comments-indentation")]
    );
}

#[test]
fn document_start_and_end() {
    let config = only("  document-start: enable\n");
    assert_eq!(
        problems(&config, "---\nthis:\n  is: [a, document]\n---\n- this\n"),
        vec![]
    );
    assert_eq!(
        problems(&config, "this:\n  is: [a, document]\n---\n- this\n"),
        vec![(1, 1, "document-start")]
    );
    let config = only("  document-start:\n    present: false\n");
    assert_eq!(
        problems(&config, "---\nthis:\n  is: [a, document]\n...\n"),
        vec![(1, 1, "document-start")]
    );

    let config = only("  document-end: enable\n");
    assert_eq!(
        problems(
            &config,
            "---\nthis:\n  is: [a, document]\n...\n---\n- this\n- is: another one\n...\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(
            &config,
            "---\nthis:\n  is: [a, document]\n---\n- this\n- is: another one\n...\n"
        ),
        vec![(4, 1, "document-end")]
    );
    let config = only("  document-end:\n    present: false\n");
    assert_eq!(
        problems(
            &config,
            "---\nthis:\n  is: [a, document]\n...\n---\n- this\n"
        ),
        vec![(4, 1, "document-end")]
    );
}

#[test]
fn empty_lines() {
    let config = only("  empty-lines:\n    max: 1\n");
    assert_eq!(
        problems(&config, "- foo:\n    - 1\n    - 2\n\n- bar: [3, 4]\n"),
        vec![]
    );
    assert_eq!(
        problems(&config, "- foo:\n    - 1\n    - 2\n\n\n- bar: [3, 4]\n"),
        vec![(5, 1, "empty-lines")]
    );
    let config = only("  empty-lines: enable\n");
    assert_eq!(
        problems(&config, "\nkey: value\n"),
        vec![(1, 1, "empty-lines")]
    );
    assert_eq!(
        problems(&config, "key: value\n\n"),
        vec![(2, 1, "empty-lines")]
    );
}

#[test]
fn empty_values() {
    let config = only("  empty-values: enable\n");
    assert_eq!(
        problems(
            &config,
            "some-mapping:\n  sub-element: correctly indented\n"
        ),
        vec![]
    );
    assert_eq!(problems(&config, "explicitly-null: null\n"), vec![]);
    assert_eq!(
        problems(&config, "implicitly-null:\n"),
        vec![(1, 17, "empty-values")]
    );
    assert_eq!(
        problems(&config, "{prop: }\n"),
        vec![(1, 7, "empty-values")]
    );
    // yamllint's example spells this `b:,`; libyaml's scanner rejects a
    // colon directly followed by a flow indicator ("found unexpected ':'"),
    // where PyYAML accepts it. See the docs page.
    assert_eq!(
        problems(&config, "{a: 1, b: , c: 3}\n"),
        vec![(1, 10, "empty-values")]
    );
    assert_eq!(
        problems(&config, "some-sequence:\n  -\n"),
        vec![(2, 4, "empty-values")]
    );
}

#[test]
fn float_values() {
    let config = only(
        "  float-values:\n    require-numeral-before-decimal: true\n    forbid-scientific-notation: true\n    forbid-nan: true\n    forbid-inf: true\n",
    );
    assert_eq!(problems(&config, "anemometer:\n  angle: 0.0\n"), vec![]);
    assert_eq!(
        problems(&config, "anemometer:\n  angle: .0\n"),
        vec![(2, 10, "float-values")]
    );
    assert_eq!(
        problems(&config, "anemometer:\n  angle: 10e-6\n"),
        vec![(2, 10, "float-values")]
    );
    assert_eq!(
        problems(&config, "anemometer:\n  angle: .NaN\n"),
        vec![(2, 10, "float-values")]
    );
    assert_eq!(
        problems(&config, "anemometer:\n  angle: .inf\n"),
        vec![(2, 10, "float-values")]
    );
    assert_eq!(problems(&config, "anemometer:\n  angle: '.inf'\n"), vec![]);
}

#[test]
fn hyphens() {
    let config = only("  hyphens: enable\n");
    assert_eq!(
        problems(
            &config,
            "- first list:\n    - a\n    - b\n- - 1\n  - 2\n  - 3\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(&config, "-  first list:\n     - a\n     - b\n"),
        vec![(1, 3, "hyphens")]
    );
    assert_eq!(
        problems(&config, "- - 1\n  -  2\n  - 3\n"),
        vec![(2, 5, "hyphens")]
    );
}

#[test]
fn indentation() {
    let config = only("  indentation:\n    spaces: 1\n");
    assert_eq!(
        problems(
            &config,
            "history:\n - name: Unix\n   date: 1969\n - name: Linux\n   date: 1991\nnest:\n recurse:\n  - haystack:\n     needle\n"
        ),
        vec![]
    );
    let config = only("  indentation:\n    spaces: 4\n");
    assert_eq!(
        problems(
            &config,
            "history:\n    - name: Unix\n      date: 1969\nnest:\n    recurse:\n        - haystack:\n              needle\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(&config, "history:\n  - name: Unix\n    date: 1969\n"),
        vec![(2, 3, "indentation")]
    );
    let config = only("  indentation:\n    spaces: consistent\n");
    assert_eq!(
        problems(
            &config,
            "history:\n   - name: Unix\n     date: 1969\nnest:\n   recurse:\n      - haystack:\n           needle\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(&config, "some:\n  Russian:\n      dolls\n"),
        vec![(3, 7, "indentation")]
    );
    let config = only("  indentation:\n    spaces: 2\n    indent-sequences: false\n");
    assert_eq!(problems(&config, "list:\n- flying\n- spaghetti\n"), vec![]);
    assert_eq!(
        problems(&config, "list:\n  - flying\n  - spaghetti\n"),
        vec![(2, 3, "indentation")]
    );
    let config = only("  indentation:\n    spaces: 2\n    indent-sequences: whatever\n");
    assert_eq!(
        problems(
            &config,
            "list:\n- flying:\n  - spaghetti\n  - monster\n- not flying:\n    - spaghetti\n    - sauce\n"
        ),
        vec![]
    );
    let config = only("  indentation:\n    spaces: 2\n    indent-sequences: consistent\n");
    assert_eq!(
        problems(
            &config,
            "- flying:\n  - spaghetti\n  - monster\n- not flying:\n  - spaghetti\n  - sauce\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(
            &config,
            "- flying:\n    - spaghetti\n    - monster\n- not flying:\n  - spaghetti\n  - sauce\n"
        ),
        vec![(5, 3, "indentation")]
    );
}

#[test]
fn indentation_of_multi_line_strings() {
    let config = only("  indentation:\n    spaces: 4\n    check-multi-line-strings: true\n");
    assert_eq!(
        problems(
            &config,
            "Blaise Pascal:\n    Je vous écris une longue lettre parce que\n    je n'ai pas le temps d'en écrire une courte.\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(
            &config,
            "Blaise Pascal: Je vous écris une longue lettre parce que\n               je n'ai pas le temps d'en écrire une courte.\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(
            &config,
            "Blaise Pascal: Je vous écris une longue lettre parce que\n  je n'ai pas le temps d'en écrire une courte.\n"
        ),
        vec![(2, 3, "indentation")]
    );
    assert_eq!(
        problems(
            &config,
            "C code:\n    void main() {\n        printf(\"foo\");\n    }\n"
        ),
        vec![(3, 9, "indentation")]
    );
    assert_eq!(
        problems(
            &config,
            "C code:\n    void main() {\n    printf(\"bar\");\n    }\n"
        ),
        vec![]
    );
}

#[test]
fn key_duplicates() {
    let config = only("  key-duplicates: enable\n");
    assert_eq!(
        problems(
            &config,
            "- key 1: v\n  key 2: val\n  key 3: value\n- {a: 1, b: 2, c: 3}\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(&config, "- key 1: v\n  key 2: val\n  key 1: value\n"),
        vec![(3, 3, "key-duplicates")]
    );
    assert_eq!(
        problems(&config, "- {a: 1, b: 2, b: 3}\n"),
        vec![(1, 16, "key-duplicates")]
    );
    assert_eq!(
        problems(
            &config,
            "duplicated key: 1\n\"duplicated key\": 2\n\nother duplication: 1\n? >-\n    other\n    duplication\n: 2\n"
        ),
        vec![(2, 1, "key-duplicates"), (5, 3, "key-duplicates")]
    );
    assert_eq!(
        problems(
            &config,
            "anchor_reference:\n  <<: *anchor_one\n  <<: *anchor_two\n"
        ),
        vec![]
    );
    let config = only("  key-duplicates:\n    forbid-duplicated-merge-keys: true\n");
    assert_eq!(
        problems(
            &config,
            "anchor_reference:\n  <<: *anchor_one\n  <<: *anchor_two\n"
        ),
        vec![(3, 3, "key-duplicates")]
    );
}

#[test]
fn key_ordering() {
    let config = only("  key-ordering: enable\n");
    assert_eq!(
        problems(
            &config,
            "- key 1: v\n  key 2: val\n  key 3: value\n- {a: 1, b: 2, c: 3}\n- T-shirt: 1\n  T-shirts: 2\n  t-shirt: 3\n  t-shirts: 4\n- hair: true\n  hais: true\n  haïr: true\n  haïssable: true\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(&config, "- key 2: v\n  key 1: val\n"),
        vec![(2, 3, "key-ordering")]
    );
    assert_eq!(
        problems(&config, "- {b: 1, a: 2}\n"),
        vec![(1, 10, "key-ordering")]
    );
    assert_eq!(
        problems(
            &config,
            "- T-shirt: 1\n  t-shirt: 2\n  T-shirts: 3\n  t-shirts: 4\n"
        ),
        vec![(3, 3, "key-ordering")]
    );
    assert_eq!(
        problems(&config, "- haïr: true\n  hais: true\n"),
        vec![(2, 3, "key-ordering")]
    );
    let config = only("  key-ordering:\n    ignored-keys: [\"name\"]\n");
    assert_eq!(
        problems(
            &config,
            "- a:\n  b:\n  name: ignored\n  first-name: ignored\n  c:\n  d:\n"
        ),
        vec![]
    );
}

#[test]
fn line_length() {
    let config = only("  line-length:\n    max: 70\n");
    assert_eq!(
        problems(
            &config,
            "long sentence:\n  Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do\n  eiusmod tempor incididunt ut labore et dolore magna aliqua.\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(
            &config,
            "long sentence:\n  Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod\n  tempor incididunt ut labore et dolore magna aliqua.\n"
        ),
        vec![(2, 71, "line-length")]
    );
    let config = only("  line-length:\n    max: 60\n    allow-non-breakable-words: true\n");
    assert_eq!(
        problems(
            &config,
            "this:\n  is:\n    - a:\n        http://localhost/very/very/very/very/very/very/very/very/long/url\n\n# this comment is too long,\n# but hard to split:\n# http://localhost/another/very/very/very/very/very/very/very/very/long/url\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(
            &config,
            "- this line is waaaaaaaaaaaaaay too long but could be easily split...\n"
        ),
        vec![(1, 61, "line-length")]
    );
    assert_eq!(
        problems(
            &config,
            "- foobar: http://localhost/very/very/very/very/very/very/very/very/long/url\n"
        ),
        vec![(1, 61, "line-length")]
    );
    let config = only(
        "  line-length:\n    max: 60\n    allow-non-breakable-words: true\n    allow-non-breakable-inline-mappings: true\n",
    );
    assert_eq!(
        problems(
            &config,
            "- foobar: http://localhost/very/very/very/very/very/very/very/very/long/url\n"
        ),
        vec![]
    );
    let config = only("  line-length:\n    max: 60\n    allow-non-breakable-words: false\n");
    assert_eq!(
        problems(
            &config,
            "this:\n  is:\n    - a:\n        http://localhost/very/very/very/very/very/very/very/very/long/url\n"
        ),
        vec![(4, 61, "line-length")]
    );
}

#[test]
fn new_line_at_end_of_file_and_new_lines() {
    let config = only("  new-line-at-end-of-file: enable\n");
    assert_eq!(problems(&config, "key: value\n"), vec![]);
    assert_eq!(
        problems(&config, "key: value"),
        vec![(1, 11, "new-line-at-end-of-file")]
    );
    let config = only("  new-lines: enable\n");
    assert_eq!(problems(&config, "key: value\n"), vec![]);
    assert_eq!(
        problems(&config, "key: value\r\n"),
        vec![(1, 11, "new-lines")]
    );
    let config = only("  new-lines:\n    type: dos\n");
    assert_eq!(problems(&config, "key: value\r\n"), vec![]);
    assert_eq!(
        problems(&config, "key: value\n"),
        vec![(1, 11, "new-lines")]
    );
}

#[test]
fn octal_values() {
    let config = only("  octal-values: enable\n");
    assert_eq!(problems(&config, "user:\n  city-code: '010'\n"), vec![]);
    assert_eq!(problems(&config, "user:\n  city-code: 010,021\n"), vec![]);
    assert_eq!(
        problems(&config, "user:\n  city-code: 010\n"),
        vec![(2, 17, "octal-values")]
    );
    assert_eq!(problems(&config, "user:\n  city-code: '0o10'\n"), vec![]);
    assert_eq!(
        problems(&config, "user:\n  city-code: 0o10\n"),
        vec![(2, 18, "octal-values")]
    );
}

#[test]
fn quoted_strings() {
    let config = only("  quoted-strings:\n    quote-type: any\n    required: true\n");
    assert_eq!(
        problems(
            &config,
            "foo: \"bar\"\nbar: 'foo'\nnumber: 123\nboolean: true\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(&config, "foo: bar\n"),
        vec![(1, 6, "quoted-strings")]
    );

    let config =
        only("  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n");
    assert_eq!(
        problems(
            &config,
            "foo: bar\nbar: foo\nnot_number: '123'\nnot_boolean: 'true'\nnot_comment: '# comment'\nnot_list: '[1, 2, 3]'\nnot_map: '{a: 1, b: 2}'\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(&config, "foo: 'bar'\n"),
        vec![(1, 6, "quoted-strings")]
    );

    let config = only(
        "  quoted-strings:\n    required: false\n    extra-required: ['^http://', '^ftp://']\n",
    );
    assert_eq!(
        problems(
            &config,
            "- localhost\n- \"localhost\"\n- \"http://localhost\"\n- \"ftp://localhost\"\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(&config, "- http://localhost\n- ftp://localhost\n"),
        vec![(1, 3, "quoted-strings"), (2, 3, "quoted-strings")]
    );

    let config = only(
        "  quoted-strings:\n    required: only-when-needed\n    extra-allowed: ['^http://', '^ftp://']\n    extra-required: [QUOTED]\n",
    );
    assert_eq!(
        problems(
            &config,
            "- localhost\n- \"http://localhost\"\n- \"ftp://localhost\"\n- \"this is a string that needs to be QUOTED\"\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(
            &config,
            "- \"localhost\"\n- this is a string that needs to be QUOTED\n"
        ),
        vec![(1, 3, "quoted-strings"), (2, 3, "quoted-strings")]
    );

    let config =
        only("  quoted-strings:\n    quote-type: double\n    allow-quoted-quotes: false\n");
    assert_eq!(problems(&config, "foo: \"bar\\\"baz\"\n"), vec![]);
    assert_eq!(
        problems(&config, "foo: 'bar\"baz'\n"),
        vec![(1, 6, "quoted-strings")]
    );
    let config = only("  quoted-strings:\n    quote-type: double\n    allow-quoted-quotes: true\n");
    assert_eq!(problems(&config, "foo: 'bar\"baz'\n"), vec![]);

    let config = only("  quoted-strings:\n    quote-type: consistent\n");
    assert_eq!(problems(&config, "foo: 'bar'\nbaz: 'quux'\n"), vec![]);
    assert_eq!(
        problems(&config, "foo: 'bar'\nbaz: \"quux\"\n"),
        vec![(2, 6, "quoted-strings")]
    );

    let config = only(
        "  quoted-strings:\n    required: only-when-needed\n    check-keys: true\n    extra-required: [\"[:]\"]\n",
    );
    assert_eq!(
        problems(&config, "foo:bar: baz\n"),
        vec![(1, 1, "quoted-strings")]
    );
    assert_eq!(problems(&config, "\"foo:bar\": baz\n"), vec![]);
}

#[test]
fn trailing_spaces() {
    let config = only("  trailing-spaces: enable\n");
    assert_eq!(
        problems(
            &config,
            "this document doesn't contain\nany trailing\nspaces\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(
            &config,
            "this document contains     \ntrailing spaces\non lines 1 and 3         \n"
        ),
        vec![(1, 23, "trailing-spaces"), (3, 17, "trailing-spaces")]
    );
}

#[test]
fn truthy() {
    let config = only("  truthy: enable\n");
    assert_eq!(
        problems(
            &config,
            "boolean: true\n\nobject: {\"True\": 1, 1: \"True\"}\n\n\"yes\":  1\n\"on\":   2\n\"True\": 3\n\nexplicit:\n  string1: !!str True\n  boolean1: !!bool true\n  boolean6: !!bool NO\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(&config, "object: {True: 1, 1: True}\n"),
        vec![(1, 10, "truthy"), (1, 22, "truthy")]
    );
    assert_eq!(
        problems(&config, "%YAML 1.1\n---\nyes:  1\non:   2\nTrue: 3\n"),
        vec![(3, 1, "truthy"), (4, 1, "truthy"), (5, 1, "truthy")]
    );
    assert_eq!(
        problems(&config, "%YAML 1.2\n---\nyes:  1\non:   2\ntrue: 3\n"),
        vec![]
    );

    let config = only("  truthy:\n    allowed-values: [\"yes\", \"no\"]\n");
    assert_eq!(
        problems(
            &config,
            "- yes\n- no\n- \"true\"\n- 'false'\n- foo\n- bar\n"
        ),
        vec![]
    );
    assert_eq!(
        problems(&config, "- true\n- false\n- on\n- off\n"),
        vec![
            (1, 3, "truthy"),
            (2, 3, "truthy"),
            (3, 3, "truthy"),
            (4, 3, "truthy")
        ]
    );

    let config = only("  truthy:\n    check-keys: false\n");
    assert_eq!(problems(&config, "yes:  1\non:   2\ntrue: 3\n"), vec![]);
    assert_eq!(
        problems(&config, "yes:  Yes\non:   On\ntrue: True\n"),
        vec![(1, 7, "truthy"), (2, 7, "truthy"), (3, 7, "truthy")]
    );
    assert_eq!(
        messages(&config, "a: Yes\n"),
        vec!["truthy value should be one of [false, true] (truthy)"]
    );
}

#[test]
fn levels_follow_the_config() {
    let config = conf("extends: default\nrules:\n  line-length:\n    max: 20\n");
    let found = lint(
        "key: value\nlong: a line that is clearly too long\n",
        &config,
        None,
    );
    let levels: Vec<(Level, &str)> = found
        .iter()
        .map(|p| (p.level, p.rule.unwrap_or("syntax")))
        .collect();
    assert_eq!(
        levels,
        vec![
            (Level::Warning, "document-start"),
            (Level::Error, "line-length")
        ]
    );
}

#[test]
fn problems_display_in_parsable_format() {
    let config = only("  trailing-spaces: enable\n");
    let found = lint("a: 1 \n", &config, None);
    assert_eq!(
        found[0].to_string(),
        "1:5: [error] trailing spaces (trailing-spaces)"
    );
}

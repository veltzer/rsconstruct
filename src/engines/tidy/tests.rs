//! Unit tests for the tidy port: small documents whose expected output
//! was taken from `tidy -errors -q` 5.8.0.

use super::{Options, check};

fn run(input: &str) -> (Vec<String>, u32, u32) {
    let opts = Options {
        quiet: true,
        ..Options::default()
    };
    let report = check(input.as_bytes(), &opts).expect("input is UTF-8");
    (report.lines, report.warnings, report.errors)
}

#[test]
fn clean_document_reports_nothing() {
    let (lines, warnings, errors) = run(
        "<!DOCTYPE html>\n<html>\n<head><title>t</title></head>\n<body><p>hello</p></body>\n</html>\n",
    );
    assert_eq!(lines, Vec::<String>::new());
    assert_eq!((warnings, errors), (0, 0));
}

#[test]
fn missing_doctype_and_title_are_warnings() {
    let (lines, warnings, errors) = run("<p>hello</p>\n");
    assert_eq!(
        lines,
        vec![
            "line 1 column 1 - Warning: missing <!DOCTYPE> declaration",
            "line 1 column 1 - Warning: inserting implicit <body>",
            "line 1 column 1 - Warning: inserting missing 'title' element",
        ]
    );
    assert_eq!((warnings, errors), (3, 0));
}

#[test]
fn unknown_tag_is_an_error() {
    let (lines, warnings, errors) = run(
        "<!DOCTYPE html>\n<html><head><title>t</title></head>\n<body><foo>x</foo></body></html>\n",
    );
    assert_eq!(
        lines,
        vec![
            "line 3 column 7 - Error: <foo> is not recognized!",
            "line 3 column 7 - Warning: discarding unexpected <foo>",
            "line 3 column 13 - Warning: discarding unexpected </foo>",
        ]
    );
    assert_eq!((warnings, errors), (2, 1));
}

#[test]
fn tabs_count_as_tab_size_columns() {
    let (lines, warnings, errors) = run(
        "<!DOCTYPE html>\n<html><head><title>t</title></head>\n<body>\n\t<p>x<b>y</p>\n</body></html>\n",
    );
    assert_eq!(
        lines,
        vec!["line 4 column 13 - Warning: missing </b> before </p>"]
    );
    assert_eq!((warnings, errors), (1, 0));
}

#[test]
fn reports_stop_at_show_errors_but_totals_do_not() {
    let body = "<u0>x</u0><u1>x</u1><u2>x</u2><u3>x</u3><u4>x</u4><u5>x</u5><u6>x</u6>";
    let input = format!(
        "<!DOCTYPE html>\n<html><head><title>t</title></head>\n<body>{body}</body></html>\n"
    );
    let (lines, warnings, errors) = run(&input);
    // tidy prints nothing more once the sixth error has been counted:
    // five elements' worth of reports, three lines each.
    assert_eq!(lines.len(), 15);
    assert_eq!(
        lines[12],
        "line 3 column 47 - Error: <u4> is not recognized!"
    );
    assert_eq!((warnings, errors), (14, 7));
}

#[test]
fn options_change_the_checks() {
    let input = "<!DOCTYPE html>\n<html><head><title>t</title></head>\n<body><my-tag>x</my-tag></body></html>\n";
    let (lines, _, errors) = run(input);
    assert_eq!(
        lines[0],
        "line 3 column 7 - Error: <my-tag> is not recognized! Did you mean to enable the custom-tags option?"
    );
    assert_eq!(errors, 1);

    let mut opts = Options {
        quiet: true,
        ..Options::default()
    };
    opts.set("custom-tags", "blocklevel")
        .expect("a valid option");
    let report = check(input.as_bytes(), &opts).expect("input is UTF-8");
    assert_eq!(report.lines, Vec::<String>::new());
    assert_eq!((report.warnings, report.errors), (0, 0));
}

#[test]
fn config_text_follows_tidy_grammar() {
    let mut opts = Options {
        quiet: true,
        ..Options::default()
    };
    opts.apply_config_text(
        "# a comment\nnew-blocklevel-tags: foo,\n  bar\ndrop-empty-elements: no\n",
    )
    .expect("a valid config");
    let report = check(
        b"<!DOCTYPE html>\n<html><head><title>t</title></head>\n<body><foo>x</foo><bar></bar></body></html>\n",
        &opts,
    )
    .expect("input is UTF-8");
    assert_eq!(
        report.lines,
        vec![
            "line 3 column 7 - Warning: <foo> is not approved by W3C",
            "line 3 column 19 - Warning: <bar> is not approved by W3C",
        ]
    );
    assert_eq!((report.warnings, report.errors), (2, 0));
}

#[test]
fn unsupported_and_unknown_options_are_refused() {
    let mut opts = Options::default();
    assert_eq!(
        opts.set("no-such-option", "yes"),
        Err("unknown option: no-such-option".to_string())
    );
    assert!(opts.set("clean", "yes").is_err());
    assert!(opts.set("input-encoding", "latin1").is_err());
    assert_eq!(opts.set("indent", "auto"), Ok(()));
    assert!(opts.set("indent", "sometimes").is_err());
}

#[test]
fn utf16_input_is_refused() {
    let err = check(b"\xff\xfe<\x00p\x00>\x00", &Options::default())
        .expect_err("UTF-16 is not supported");
    assert!(err.contains("UTF-16"), "{err}");
}

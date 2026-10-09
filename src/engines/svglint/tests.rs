//! Unit tests for the svglint port. The expected findings were taken from
//! svglint 4.2.1 run with the same rules (svglint prints 0-based positions
//! and miscounts lines after a leading newline; the positions here are the
//! real 1-based ones of the same elements).

use super::dom;
use super::selector::SelectorList;
use super::validator::validate;
use super::{At, Report, Rules, lint};

fn rules(text: &str) -> Rules {
    let table: toml::Table = toml::from_str(text).expect("valid TOML");
    let valid = table
        .get("valid")
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);
    let empty = toml::Table::new();
    let elm = table
        .get("elm")
        .and_then(toml::Value::as_table)
        .unwrap_or(&empty);
    let attr: Vec<toml::Table> = table
        .get("attr")
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_table().cloned()).collect())
        .unwrap_or_default();
    Rules::from_config(valid, elm, &attr).expect("valid rules")
}

const RULES: &str = r#"
[elm]
"svg" = 1
"svg > title" = 1
"svg > path" = [1, 3]
"rect" = false
"*" = false

[[attr]]
"rule::selector" = "svg"
"rule::whitelist" = true
"rule::order" = true
role = "img"
viewBox = "0 0 24 24"
xmlns = "http://www.w3.org/2000/svg"
"width?" = { regex = "^\\d+$" }

[[attr]]
"rule::selector" = "svg > path"
d = { regex = "^m[-mzlhvcsqtae\\d,. ]+$", flags = "i" }
fill = ["red", "blue"]
"#;

fn report(rule: &str, text: &str, at: Option<(usize, usize, &str)>) -> Report {
    Report {
        rule: rule.to_string(),
        text: text.to_string(),
        at: at.map(|(line, column, name)| At {
            line,
            column,
            name: name.to_string(),
        }),
    }
}

#[test]
fn clean_document_with_default_rules_reports_nothing() {
    let r = rules("");
    let doc = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M1 1\"/></svg>\n";
    assert_eq!(lint(doc, &r).unwrap(), Vec::<Report>::new());
}

#[test]
fn findings_in_svglint_order() {
    let r = rules(RULES);
    let doc = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" role=\"img\">\n  <title>t</title>\n  <path d=\"M1 1L2 2z\" fill=\"green\"/>\n  <rect width=\"1\" height=\"1\"/>\n  <path d=\"X\" fill=\"red\" fill=\"blue\"/>\n</svg>\n";
    assert_eq!(
        lint(doc, &r).unwrap(),
        vec![
            report(
                "attr-1",
                "Wrong ordering of attributes, found \"xmlns, viewBox, role\", expected \"role, viewBox, xmlns\"",
                Some((1, 1, "svg"))
            ),
            report(
                "attr-2",
                "Expected attribute 'fill' to be one of [\"red\",\"blue\"], was \"green\"",
                Some((3, 3, "path"))
            ),
            report(
                "attr-2",
                "Expected attribute 'd' to match /^m[-mzlhvcsqtae\\d,. ]+$/i, was \"X\"",
                Some((5, 3, "path"))
            ),
            report("elm", "Element disallowed", Some((4, 3, "rect"))),
            report("elm", "Element disallowed", Some((4, 3, "rect"))),
            report("valid", "Attribute 'fill' is repeated.", None),
        ]
    );
}

#[test]
fn missing_elements_and_mismatched_closing_tag() {
    let r = rules(RULES);
    let doc = "<svg>\n<g>\n<path d=\"M1 1\"/>\n</svg>\n";
    assert_eq!(
        lint(doc, &r).unwrap(),
        vec![
            report(
                "attr-1",
                "Expected attribute 'role', didn't find it",
                Some((1, 1, "svg"))
            ),
            report(
                "attr-1",
                "Expected attribute 'viewBox', didn't find it",
                Some((1, 1, "svg"))
            ),
            report(
                "attr-1",
                "Expected attribute 'xmlns', didn't find it",
                Some((1, 1, "svg"))
            ),
            report("elm", "Element disallowed", Some((2, 1, "g"))),
            report("elm", "Element disallowed", Some((3, 1, "path"))),
            report(
                "elm",
                "Found 0 elements for 'svg > path', expected between 1 and 3",
                None
            ),
            report(
                "elm",
                "Found 0 elements for 'svg > title', expected 1",
                None
            ),
            report(
                "valid",
                "Expected closing tag 'g' (opened in line 2, col 1) instead of closing tag 'svg'.",
                None
            ),
        ]
    );
}

#[test]
fn positions_are_real_lines_even_after_a_leading_newline() {
    let r = rules(RULES);
    let doc = "\n\n<svg viewBox=\"0 0 24 24\" role=\"img\" xmlns=\"http://www.w3.org/2000/svg\">\n<title>t</title><path d=\"M1 1\" fill=\"red\" bogus=\"1\"/>\n<title>u</title></svg>\n";
    assert_eq!(
        lint(doc, &r).unwrap(),
        vec![
            report(
                "attr-1",
                "Wrong ordering of attributes, found \"viewBox, role, xmlns\", expected \"role, viewBox, xmlns\"",
                Some((3, 1, "svg"))
            ),
            report("elm", "Element disallowed", Some((4, 1, "title"))),
            report("elm", "Element disallowed", Some((5, 1, "title"))),
            report(
                "elm",
                "Found 2 elements for 'svg > title', expected 1",
                Some((4, 1, "title"))
            ),
            report(
                "elm",
                "Found 2 elements for 'svg > title', expected 1",
                Some((5, 1, "title"))
            ),
        ]
    );
}

#[test]
fn empty_file_is_valid_but_lacks_required_elements() {
    let r = rules("[elm]\nsvg = true\n");
    assert_eq!(
        lint("", &r).unwrap(),
        vec![report("elm", "Expected 'svg', none found", None)]
    );
    assert_eq!(lint("", &rules("")).unwrap(), Vec::<Report>::new());
}

#[test]
fn whitelist_and_optional_attributes() {
    let r = rules(
        "[[attr]]\n\"rule::selector\" = \"path\"\n\"rule::whitelist\" = true\nd = true\n\"fill?\" = false\n",
    );
    let doc = "<svg><path d=\"M0 0\" stroke=\"red\"/><path d=\"M0 0\" fill=\"x\"/><path/></svg>";
    assert_eq!(
        lint(doc, &r).unwrap(),
        vec![
            report(
                "attr-1",
                "Found extra attributes [\"stroke\"] with whitelisting enabled",
                Some((1, 6, "path"))
            ),
            report(
                "attr-1",
                "Attribute 'fill' is disallowed",
                Some((1, 35, "path"))
            ),
            report(
                "attr-1",
                "Expected attribute 'd', didn't find it",
                Some((1, 60, "path"))
            ),
        ]
    );
}

#[test]
fn validator_messages() {
    assert_eq!(
        validate("<svg><g></svg>"),
        Some(
            "Expected closing tag 'g' (opened in line 1, col 6) instead of closing tag 'svg'."
                .to_string()
        )
    );
    assert_eq!(validate("<svg>"), Some("Unclosed tag 'svg'.".to_string()));
    assert_eq!(
        validate("<svg><a><b>"),
        Some("Invalid '[    \"svg\",    \"a\",    \"b\"]' found.".to_string())
    );
    assert_eq!(
        validate("hello"),
        Some("char 'h' is not expected.".to_string())
    );
    assert_eq!(validate("  \n"), Some("Start tag expected.".to_string()));
    assert_eq!(
        validate("<svg a=\"1\" a=\"2\"/>"),
        Some("Attribute 'a' is repeated.".to_string())
    );
    assert_eq!(
        validate("<svg a/>"),
        Some("boolean attribute 'a' is not allowed.".to_string())
    );
    assert_eq!(
        validate("<svg a=/>"),
        Some("Attribute 'a' is without value.".to_string())
    );
    assert_eq!(
        validate("<svg a=\"1\"b=\"2\"/>"),
        Some("Attribute 'b' has no space in starting.".to_string())
    );
    assert_eq!(
        validate("<svg a=\"1/>"),
        Some("Attributes for 'svg' have open quote.".to_string())
    );
    // Only an open tag after the root is "multiple roots", only a
    // closed root makes trailing text "extra": self-closing roots see
    // neither check, as in fast-xml-parser.
    assert_eq!(validate("<svg/><svg/>"), None);
    assert_eq!(
        validate("<svg></svg><a></a>"),
        Some("Multiple possible root nodes found.".to_string())
    );
    assert_eq!(validate("<svg/>x"), None);
    assert_eq!(
        validate("<svg></svg>x"),
        Some("Extra text at the end".to_string())
    );
    assert_eq!(
        validate("<svg>a & b</svg>"),
        Some("char '&' is not expected.".to_string())
    );
    assert_eq!(
        validate("<svg>&foo</svg>"),
        Some("char '&' is not expected.".to_string())
    );
    assert_eq!(validate("<svg>&foo;&#12;&#xAB;</svg>"), None);
    assert_eq!(
        validate("<1svg/>"),
        Some("Tag '1svg' is an invalid name.".to_string())
    );
    assert_eq!(
        validate("< svg/>"),
        Some("Invalid space after '<'.".to_string())
    );
    // A declaration after a tag is read as an instruction named "?xml"
    // and passes; only one met at the top level (before the first tag, or
    // right after a comment or doctype) is caught.
    assert_eq!(validate("<svg/><?xml version=\"1.0\"?>"), None);
    assert_eq!(validate("<svg></svg><?xml version=\"1.0\"?>"), None);
    assert_eq!(
        validate("<!-- c --><?xml version=\"1.0\"?><svg/>"),
        Some("XML declaration allowed only at the start of the document.".to_string())
    );
    assert_eq!(
        validate("<svg a=\"x\" b=\"y\"c/>"),
        Some("Attribute 'c' has no space in starting.".to_string())
    );
    assert_eq!(validate("<svg>text</svg >"), None);
    assert_eq!(
        validate("<svg>t</svg x>"),
        Some("Closing tag 'svg' can't have attributes or invalid starting.".to_string())
    );
    assert_eq!(
        validate("<svg><![CDATA[ ]]</svg>"),
        Some("Unclosed tag 'svg'.".to_string())
    );
    assert_eq!(validate("<svg><!-- -- --></svg>"), None);
    assert_eq!(
        validate("<svg>\n<path d=\"1\"/ >\n</svg>"),
        Some("Attribute '/' has no space in starting.".to_string())
    );
    assert_eq!(
        validate(
            "<?xml version=\"1.0\"?>\n<!-- c --><!DOCTYPE svg>\n<svg xmlns:a=\"u\" b='&#x20;&amp;'><![CDATA[<>&]]></svg>\n"
        ),
        None
    );
}

#[test]
fn lenient_dom_positions() {
    let d = dom::parse("<a>\n <b x='1' x='2'/><c>t</d></c></a>");
    let els = d.elements();
    assert_eq!(els.len(), 3);
    assert_eq!(d.position(d.node(els[1]).start_index), (2, 2));
    assert_eq!(d.attrs(els[1]), &[("x".to_string(), "1".to_string())]);
    // `</d>` matches nothing and is dropped; `</c>` closes c.
    assert_eq!(d.node(els[2]).children.len(), 1);
    assert_eq!(d.text(els[2]), "t");
}

/// An unquoted value with spaces makes its words attribute names; those
/// that look like array indices come first, in numeric order, as keys of a
/// JavaScript object do.
#[test]
fn attribute_names_in_javascript_object_order() {
    let d = dom::parse("<svg viewBox=0 0 24 24 role=img 007=\"x\" 10=\"y\"/>");
    let names: Vec<&str> = d
        .attrs(d.elements()[0])
        .iter()
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(names, vec!["0", "10", "24", "viewBox", "role", "007"]);
}

#[test]
fn selectors() {
    let d = dom::parse(
        "<svg class=\"a b\"><g id=\"x\"><path d=\"M\"/><path d=\"L\" fill=\"Red\"/></g><title>Hi there</title></svg>",
    );
    let count = |s: &str| SelectorList::parse(s).unwrap().select_all(&d).len();
    assert_eq!(count("path"), 2);
    assert_eq!(count("svg > path"), 0);
    assert_eq!(count("svg path"), 2);
    assert_eq!(count("g > path:first-child"), 1);
    assert_eq!(count("path + path"), 1);
    assert_eq!(count("g ~ title"), 1);
    assert_eq!(count("#x"), 1);
    assert_eq!(count(".b"), 1);
    assert_eq!(count("[fill]"), 1);
    assert_eq!(count("[fill=red]"), 0);
    assert_eq!(count("[fill=red i]"), 1);
    assert_eq!(count("[fill!=Red]"), 4);
    assert_eq!(count("path[d^=M]"), 1);
    assert_eq!(count("path:not([fill])"), 1);
    assert_eq!(count("g:has(> path)"), 1);
    assert_eq!(count("svg:has(> path)"), 0);
    assert_eq!(count(":is(g, title)"), 2);
    assert_eq!(count("path:nth-child(2)"), 1);
    assert_eq!(count("path:nth-of-type(2n+1)"), 1);
    assert_eq!(count("title:contains(there)"), 1);
    assert_eq!(count("title:icontains('HI')"), 1);
    assert_eq!(count(":empty"), 2);
    assert_eq!(count("* > svg"), 1);
    assert_eq!(count("*"), 5);
    assert!(SelectorList::parse(":root").is_err());
    assert!(SelectorList::parse("a::before").is_err());
    assert!(SelectorList::parse("> a").is_err());
    assert!(SelectorList::parse("").is_err());
}

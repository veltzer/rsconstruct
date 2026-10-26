use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const SCAN_ROOT: &str = "[processor.checker.ixmllint]\nsrc_dirs = [\"\"]\n";

/// A project whose root is scanned by ixmllint, holding one XML file.
fn setup_project_with(config: &str, xml: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), config)
        .expect("Failed to write rsconstruct.toml");
    fs::write(temp_dir.path().join("test.xml"), xml).expect("Failed to write test.xml");
    temp_dir
}

fn setup_project(xml: &str) -> TempDir {
    setup_project_with(SCAN_ROOT, xml)
}

fn build(project_path: &Path) -> std::process::Output {
    run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")])
}

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_passes(output: &std::process::Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} must pass: {}",
        combined_output(output)
    );
}

fn assert_fails_with(output: &std::process::Output, what: &str, needle: &str) {
    assert!(!output.status.success(), "{what} must fail");
    let text = combined_output(output);
    assert!(
        text.contains(needle),
        "{what}: {needle:?} not reported: {text}"
    );
}

#[test]
fn ixmllint_valid_file() {
    let temp_dir = setup_project("<?xml version=\"1.0\"?>\n<a><b x=\"1\"/>text</a>\n");
    assert_passes(&build(temp_dir.path()), "a well-formed file");
}

/// SVG is XML; the default extensions cover it as xmllint's processor does.
#[test]
fn ixmllint_checks_svg_too() {
    let temp_dir = setup_project("<a/>\n");
    fs::write(
        temp_dir.path().join("bad.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><g></svg>\n",
    )
    .unwrap();
    assert_fails_with(
        &build(temp_dir.path()),
        "a malformed SVG",
        "bad.svg:1: parser error : Opening and ending tag mismatch: g line 1 and svg",
    );
}

#[test]
fn ixmllint_reports_in_xmllint_format() {
    let temp_dir = setup_project("<a>\n  <b>\n</a>\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "a tag mismatch",
        "test.xml:3: parser error : Opening and ending tag mismatch: b line 2 and a",
    );

    let temp_dir = setup_project("<a b=\"1\" b=\"2\"/>\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "a duplicate attribute",
        "test.xml:1: parser error : Attribute b redefined",
    );

    let temp_dir = setup_project("<a>&nbsp;</a>\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "an undefined entity",
        "test.xml:1: parser error : Entity 'nbsp' not defined",
    );
}

/// xmllint exits 0 on namespace errors; so does ixmllint unless `strict`.
#[test]
fn ixmllint_namespace_errors_fail_only_when_strict() {
    let xml = "<a><x:b/></a>\n";
    let temp_dir = setup_project(xml);
    let output = build(temp_dir.path());
    assert_passes(&output, "an undeclared prefix without strict");
    assert!(
        combined_output(&output)
            .contains("namespace error : Namespace prefix x on b is not defined"),
        "the namespace error must still be printed: {}",
        combined_output(&output)
    );

    let temp_dir = setup_project_with(
        "[processor.checker.ixmllint]\nsrc_dirs = [\"\"]\nstrict = true\n",
        xml,
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "an undeclared prefix under strict",
        "test.xml:1: namespace error : Namespace prefix x on b is not defined",
    );
}

const SCHEMA: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="a">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="b" type="xs:int" maxOccurs="unbounded"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>
"#;

fn setup_schema_project(xml: &str) -> TempDir {
    let temp_dir = setup_project_with(
        "[processor.checker.ixmllint]\nsrc_dirs = [\"\"]\nsrc_extensions = [\".xml\"]\nschema = \"xsd/a.xsd\"\n",
        xml,
    );
    fs::create_dir_all(temp_dir.path().join("xsd")).unwrap();
    fs::write(temp_dir.path().join("xsd/a.xsd"), SCHEMA).unwrap();
    temp_dir
}

#[test]
fn ixmllint_validates_against_a_schema() {
    let temp_dir = setup_schema_project("<a><b>1</b><b>2</b></a>\n");
    assert_passes(&build(temp_dir.path()), "a valid document");

    let temp_dir = setup_schema_project("<a>\n  <b>1</b>\n  <c/>\n</a>\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "an unexpected element",
        "test.xml:3: Schemas validity error : Element 'c': This element is not expected. Expected is ( b ).",
    );

    let temp_dir = setup_schema_project("<a><b>x</b></a>\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "a bad value",
        "Element 'b': 'x' is not a valid value of the atomic type 'xs:int'.",
    );
}

/// The schema is an input of every product: editing it re-checks a file
/// that passed, without a `dep_inputs` entry naming it.
#[test]
fn ixmllint_schema_edit_rechecks() {
    let temp_dir = setup_schema_project("<a><b>1</b><b>2</b></a>\n");
    assert_passes(&build(temp_dir.path()), "a valid document");
    fs::write(
        temp_dir.path().join("xsd/a.xsd"),
        SCHEMA.replace(" maxOccurs=\"unbounded\"", ""),
    )
    .unwrap();
    assert_fails_with(
        &build(temp_dir.path()),
        "the same document once the schema allows one b",
        "Schemas validity error",
    );
}

/// A schema ixmllint cannot enforce fully is a build failure, not a partial
/// check.
#[test]
fn ixmllint_rejects_an_unsupported_schema() {
    let temp_dir = setup_schema_project("<a><b>1</b></a>\n");
    fs::write(
        temp_dir.path().join("xsd/a.xsd"),
        "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\">\n  <xs:attributeGroup name=\"g\"/>\n</xs:schema>\n",
    )
    .unwrap();
    assert_fails_with(
        &build(temp_dir.path()),
        "a schema with an attributeGroup",
        "xsd/a.xsd:2: unsupported XSD construct: xs:attributeGroup",
    );
}

const INCLUDING_SCHEMA_CONFIG: &str = "[processor.checker.ixmllint]\nsrc_dirs = [\"\"]\nsrc_extensions = [\".xml\"]\nschema = \"xsd/main.xsd\"\n";

/// A schema that takes its one type from `types.xsd` beside it.
const MAIN_XSD: &str = "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\">\n  <xs:include schemaLocation=\"types.xsd\"/>\n  <xs:element name=\"n\" type=\"Small\"/>\n</xs:schema>\n";

fn types_xsd(max: u32) -> String {
    format!(
        "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\">\n  <xs:simpleType name=\"Small\">\n    <xs:restriction base=\"xs:int\"><xs:maxInclusive value=\"{max}\"/></xs:restriction>\n  </xs:simpleType>\n</xs:schema>\n"
    )
}

/// A schema the named one `include`s is an input of every product too:
/// editing it re-checks a file that passed.
#[test]
fn ixmllint_included_schema_edit_rechecks() {
    let temp_dir = setup_project_with(INCLUDING_SCHEMA_CONFIG, "<n>70</n>\n");
    fs::create_dir_all(temp_dir.path().join("xsd")).unwrap();
    fs::write(temp_dir.path().join("xsd/main.xsd"), MAIN_XSD).unwrap();
    fs::write(temp_dir.path().join("xsd/types.xsd"), types_xsd(99)).unwrap();
    assert_passes(&build(temp_dir.path()), "a value the included type allows");
    fs::write(temp_dir.path().join("xsd/types.xsd"), types_xsd(9)).unwrap();
    assert_fails_with(
        &build(temp_dir.path()),
        "the same value once the included type caps it at 9",
        "[facet 'maxInclusive'] The value '70' is greater than the maximum value allowed ('9').",
    );
}

/// A missing included schema fails the build naming it.
#[test]
fn ixmllint_missing_included_schema_fails() {
    let temp_dir = setup_project_with(INCLUDING_SCHEMA_CONFIG, "<n>7</n>\n");
    fs::create_dir_all(temp_dir.path().join("xsd")).unwrap();
    fs::write(temp_dir.path().join("xsd/main.xsd"), MAIN_XSD).unwrap();
    assert_fails_with(
        &build(temp_dir.path()),
        "a schema whose include is missing",
        "Failed to read XSD schema xsd/types.xsd",
    );
}

/// A `schema` that does not exist fails the build naming it.
#[test]
fn ixmllint_missing_schema_fails() {
    let temp_dir = setup_project_with(INCLUDING_SCHEMA_CONFIG, "<n>7</n>\n");
    assert_fails_with(
        &build(temp_dir.path()),
        "a missing schema",
        "ixmllint schema not found: xsd/main.xsd",
    );
}

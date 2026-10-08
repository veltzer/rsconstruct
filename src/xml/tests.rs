//! The verdicts here were taken from `xmllint --noout` (libxml2 2.15) on the
//! same inputs: exit status, line, and the gist of the message.

// xmllint writes namespaced names as `{uri}local`; the expected messages
// quote them verbatim, braces and all.
#![allow(
    clippy::literal_string_with_formatting_args,
    reason = "expected xmllint messages contain {namespace}local names"
)]

use std::path::Path;

use super::{Child, Document, Kind, Schema, parse, validate};

fn doc(text: &str) -> Document {
    parse(text.as_bytes())
}

fn report(text: &str) -> Vec<(u32, Kind, String)> {
    doc(text)
        .problems
        .into_iter()
        .map(|p| (p.line, p.kind, p.message))
        .collect()
}

fn error(line: u32, message: &str) -> (u32, Kind, String) {
    (line, Kind::ParserError, message.to_string())
}

#[test]
fn a_clean_document_has_no_problems() {
    for text in [
        "<a/>\n",
        "  <a/>\n",
        "<a/>\n<!-- c -->\n",
        "<a>\r\n</a>\n",
        "<a xml:lang=\"en\" xml:space=\"preserve\"/>\n",
        "<a><![CDATA[x]]></a>\n",
        "<!DOCTYPE a [<!ELEMENT a (b)>]><a><c/></a>\n",
        "<a xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"nope.xsd\"/>\n",
        "<a>\u{feff}</a>\n",
    ] {
        let d = doc(text);
        assert!(d.problems.is_empty(), "{text:?}: {:?}", d.problems);
        assert!(d.is_well_formed());
    }
}

#[test]
fn byte_order_mark_and_declared_encodings() {
    let d = parse(b"\xef\xbb\xbf<a/>\n");
    assert!(d.problems.is_empty(), "{:?}", d.problems);

    let d = parse(b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>\xe9</a>\n");
    assert!(d.problems.is_empty(), "{:?}", d.problems);
    assert_eq!(d.root.unwrap().text(), "\u{e9}");

    let d = parse(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><a>\xe9</a>\n");
    assert_eq!(
        d.problems,
        vec![super::Problem {
            line: 1,
            kind: Kind::ParserError,
            message: "Invalid bytes in character encoding".to_string()
        }]
    );

    let d = parse(b"<?xml version=\"1.0\" encoding=\"EBCDIC-CP-US\"?><a/>\n");
    assert_eq!(d.problems[0].message, "Unsupported encoding ebcdic-cp-us");

    let utf16: Vec<u8> = [0xFEu8, 0xFF]
        .into_iter()
        .chain("<a>x</a>".encode_utf16().flat_map(u16::to_be_bytes))
        .collect();
    let d = parse(&utf16);
    assert!(d.problems.is_empty(), "{:?}", d.problems);
    assert_eq!(d.root.unwrap().text(), "x");
}

#[test]
fn undeclared_entity_is_an_error_unless_a_dtd_might_define_it() {
    assert_eq!(
        report("<a>&nbsp;</a>\n"),
        vec![error(1, "Entity 'nbsp' not defined")]
    );
    assert_eq!(
        report("<!DOCTYPE a SYSTEM \"http://example.invalid/a.dtd\"><a>&nbsp;</a>\n"),
        vec![(
            1,
            Kind::ParserWarning,
            "Entity 'nbsp' not defined".to_string()
        )]
    );
}

#[test]
fn internal_entities_expand_and_external_ones_are_left_alone() {
    let d = doc("<!DOCTYPE a [<!ENTITY e \"x&amp;y\">]><a>&e;</a>\n");
    assert!(d.problems.is_empty(), "{:?}", d.problems);
    assert_eq!(d.root.unwrap().text(), "x&y");

    let d = doc("<!DOCTYPE a [<!ENTITY e SYSTEM \"nope.txt\">]><a>&e;</a>\n");
    assert!(d.problems.is_empty(), "{:?}", d.problems);
    assert_eq!(d.root.unwrap().text(), "");

    assert_eq!(
        report("<!DOCTYPE a [<!ENTITY e \"&e;\">]><a>&e;</a>\n"),
        vec![error(1, "Detected an entity reference loop")]
    );
}

#[test]
fn character_references() {
    assert_eq!(doc("<a>&#60;&#x3C;</a>\n").root.unwrap().text(), "<<");
    assert_eq!(
        report("<a>&#0;</a>\n"),
        vec![error(1, "xmlParseCharRef: invalid xmlChar value 0")]
    );
    assert_eq!(
        report("<a>&#xD800;</a>\n"),
        vec![error(1, "xmlParseCharRef: invalid xmlChar value 55296")]
    );
    assert_eq!(
        report("<a>&;</a>\n"),
        vec![error(1, "xmlParseEntityRef: no name")]
    );
    assert_eq!(
        report("<a>& b</a>\n"),
        vec![error(1, "EntityRef: expecting ';'")]
    );
}

#[test]
fn duplicate_attribute_is_reported_where_the_start_tag_ends() {
    assert_eq!(
        report("<a b=\"1\" b=\"2\"/>\n"),
        vec![error(1, "Attribute b redefined")]
    );
    assert_eq!(
        report("<a\n  b=\"1\"\n  b=\"2\"/>\n"),
        vec![error(3, "Attribute b redefined")]
    );
    // libxml2 checks duplicates once the tag is complete: the `>` line.
    assert_eq!(
        report("<a\n  b=\"1\"\n  c=\"2\"\n  b=\"3\"\n  d=\"4\"\n>\n</a>\n"),
        vec![error(6, "Attribute b redefined")]
    );
    assert_eq!(
        report("<a xmlns:p=\"urn:p\" xmlns:q=\"urn:p\" p:x=\"1\" q:x=\"2\"/>\n"),
        vec![error(1, "Namespaced Attribute x in 'urn:p' redefined")]
    );
}

#[test]
fn tag_mismatch_reports_each_closing_tag() {
    // The element left open after the mismatch is not reported again.
    assert_eq!(
        report("<a><b></a>\n"),
        vec![error(1, "Opening and ending tag mismatch: b line 1 and a")]
    );
    assert_eq!(
        report("<a>\n  <b>\n    <c>\n  </b>\n</a>\n"),
        vec![
            error(4, "Opening and ending tag mismatch: c line 3 and b"),
            error(5, "Opening and ending tag mismatch: b line 2 and a"),
        ]
    );
}

#[test]
fn document_shape_errors() {
    assert_eq!(
        report("<a/><b/>\n"),
        vec![error(1, "Extra content at the end of the document")]
    );
    assert_eq!(report(""), vec![error(1, "Document is empty")]);
    assert_eq!(report("   \n"), vec![error(1, "Document is empty")]);
    assert_eq!(
        report("{% block %}\n"),
        vec![error(1, "Start tag expected, '<' not found")]
    );
    assert_eq!(
        report("<?xml version=\"1.0\"?>\n<?xml version=\"1.0\"?><a/>\n"),
        vec![error(
            2,
            "XML declaration allowed only at the start of the document"
        )]
    );
    assert_eq!(
        report("<a><?xml foo?></a>\n"),
        vec![error(
            1,
            "XML declaration allowed only at the start of the document"
        )]
    );
    // xmllint places "premature end" on the last line, counting the one
    // after a trailing newline.
    assert_eq!(
        report("<a>\n<b>\n"),
        vec![error(3, "Premature end of data in tag b line 2")]
    );
    assert_eq!(
        report("<a>"),
        vec![error(1, "Premature end of data in tag a line 1")]
    );
    // ... and only when nothing else went wrong first.
    assert_eq!(
        report("<a>&unknown;"),
        vec![error(1, "Entity 'unknown' not defined")]
    );
    assert_eq!(
        report("<a><b></c></b></a>\n"),
        vec![
            error(1, "Opening and ending tag mismatch: b line 1 and c"),
            error(1, "Opening and ending tag mismatch: a line 1 and b"),
        ]
    );
}

#[test]
fn lexical_errors_from_the_tokenizer() {
    assert_eq!(
        report("<a>\u{1}</a>\n"),
        vec![error(1, "PCDATA invalid Char value 1")]
    );
    assert_eq!(
        report("<a>]]></a>\n"),
        vec![error(1, "Sequence ']]>' not allowed in content")]
    );
    assert_eq!(
        report("<a attr=\"<\"/>\n"),
        vec![error(1, "Unescaped '<' not allowed in attributes values")]
    );
    // libxml2 quotes the comment up to the `--`, 50 characters at most.
    assert_eq!(
        report("<a>--</a><!-- -- -->\n"),
        vec![error(1, "Double hyphen within comment: <!-- ")]
    );
    assert_eq!(
        report(
            "<a>\n<!-- 0123456789012345678901234567890123456789012345678901234567890123456789 -- y -->\n</a>\n"
        ),
        vec![error(
            2,
            "Double hyphen within comment: <!-- 0123456789012345678901234567890123456789012345678"
        )]
    );
    assert_eq!(
        report("<1a/>\n"),
        vec![error(1, "StartTag: invalid element name")]
    );
}

#[test]
fn namespace_problems_do_not_fail_the_file() {
    let d = doc("<a xmlns:x=\"urn:x\"><x:b/><y:c/></a>\n");
    assert!(d.is_well_formed());
    assert_eq!(
        d.problems,
        vec![super::Problem {
            line: 1,
            kind: Kind::NamespaceError,
            message: "Namespace prefix y on c is not defined".to_string()
        }]
    );

    let messages: Vec<String> = doc("<a x:y=\"1\"/>\n")
        .problems
        .into_iter()
        .map(|p| p.message)
        .collect();
    assert_eq!(
        messages,
        vec!["Namespace prefix x for y on a is not defined"]
    );

    let messages: Vec<String> = doc("<a xmlns:p=\"\"><p:b/></a>\n")
        .problems
        .into_iter()
        .map(|p| p.message)
        .collect();
    assert_eq!(
        messages,
        vec![
            "xmlns:p: Empty XML namespace is not allowed",
            "Namespace prefix p on b is not defined"
        ]
    );

    let d = doc("<a xmlns:xml=\"wrong\"/>\n");
    assert!(d.is_well_formed());
    assert_eq!(
        d.problems[0].message,
        "xml namespace prefix mapped to wrong URI"
    );

    let d = doc("<a xmlns=\"u\"/>\n");
    assert!(d.is_well_formed());
    assert_eq!(d.problems[0].kind, Kind::NamespaceWarning);
    assert_eq!(d.problems[0].message, "xmlns: URI u is not absolute");

    // Prefixes are resolved once the start tag is complete, so an
    // undeclared one is reported on the tag's last line; a bad `xmlns`
    // declaration is reported on its own line.
    let d = doc(
        "<a\n  xmlns:p=\"urn:p\"\n  p:x=\"1\"\n  y:z=\"2\"\n  w=\"3\"\n>\n  <q:e\n    r=\"1\"\n  />\n</a>\n",
    );
    let lines: Vec<u32> = d.problems.iter().map(|p| p.line).collect();
    assert_eq!(lines, vec![6, 9]);
    let d = doc("<a\n  xmlns:p=\"\"\n  xmlns:xml=\"wrong\"\n  v=\"1\"\n>\n</a>\n");
    let lines: Vec<u32> = d.problems.iter().map(|p| p.line).collect();
    assert_eq!(lines, vec![2, 3]);
}

#[test]
fn unsupported_version_is_a_warning() {
    let d = doc("<?xml version=\"1.1\"?><a/>\n");
    assert!(d.is_well_formed());
    assert_eq!(d.problems[0].kind, Kind::ParserWarning);
    assert_eq!(d.problems[0].message, "Unsupported version '1.1'");
}

#[test]
fn the_tree_resolves_names_and_normalizes_attributes() {
    let d = doc(
        "<r xmlns=\"urn:d\" xmlns:p=\"urn:p\">\n  <p:a p:x=\"1\n2\" y=\"&lt;\"/>\n  <b xmlns=\"\"/>text</r>\n",
    );
    assert!(d.problems.is_empty(), "{:?}", d.problems);
    let root = d.root.unwrap();
    assert_eq!(root.namespace.as_deref(), Some("urn:d"));
    assert_eq!(root.line, 1);
    let elements: Vec<_> = root.child_elements().collect();
    assert_eq!(elements[0].namespace.as_deref(), Some("urn:p"));
    assert_eq!(elements[0].line, 2);
    assert_eq!(
        elements[0].attributes[0].namespace.as_deref(),
        Some("urn:p")
    );
    assert_eq!(elements[0].attributes[0].value, "1 2");
    assert_eq!(elements[0].attributes[1].namespace, None);
    assert_eq!(elements[0].attributes[1].value, "<");
    assert_eq!(
        elements[1].namespace, None,
        "xmlns=\"\" undeclares the default"
    );
    assert_eq!(root.text(), "\n  \n  text");
    assert!(matches!(root.children.last(), Some(Child::Text(t)) if t == "text"));
}

// ---------------------------------------------------------------------------
// XSD
// ---------------------------------------------------------------------------

const KEYNOTE_XSD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema"
    targetNamespace="urn:keynote" xmlns:tns="urn:keynote" elementFormDefault="qualified">
  <xsd:element name="presentation" type="tns:Presentation"/>
  <xsd:complexType name="Presentation">
    <xsd:sequence>
      <xsd:element name="meta" type="tns:Meta" maxOccurs="1"/>
      <xsd:element name="slide" type="tns:Slide" maxOccurs="unbounded"/>
    </xsd:sequence>
  </xsd:complexType>
  <xsd:complexType name="Meta">
    <xsd:sequence>
      <xsd:element name="title" minOccurs="0">
        <xsd:simpleType><xsd:restriction base="xsd:string"/></xsd:simpleType>
      </xsd:element>
      <xsd:element name="level" type="xsd:positiveInteger" minOccurs="0"/>
      <xsd:element name="kind" minOccurs="0">
        <xsd:simpleType>
          <xsd:restriction base="xsd:string">
            <xsd:enumeration value="talk"/>
            <xsd:enumeration value="workshop"/>
          </xsd:restriction>
        </xsd:simpleType>
      </xsd:element>
    </xsd:sequence>
    <xsd:attribute name="id" type="xsd:string" use="required"/>
  </xsd:complexType>
  <xsd:complexType name="Slide">
    <xsd:sequence>
      <xsd:element name="title" type="xsd:string"/>
      <xsd:element name="bullet" minOccurs="1" maxOccurs="unbounded">
        <xsd:complexType mixed="true">
          <xsd:sequence>
            <xsd:element name="code" minOccurs="0" maxOccurs="unbounded">
              <xsd:complexType mixed="true">
                <xsd:sequence/>
                <xsd:attribute name="language" type="xsd:string"/>
              </xsd:complexType>
            </xsd:element>
            <xsd:element name="emphasis" type="xsd:string" minOccurs="0" maxOccurs="unbounded"/>
          </xsd:sequence>
          <xsd:attribute name="size" type="xsd:string"/>
        </xsd:complexType>
      </xsd:element>
    </xsd:sequence>
    <xsd:attribute name="template" type="xsd:string"/>
  </xsd:complexType>
</xsd:schema>
"#;

fn schema(xsd: &str) -> anyhow::Result<Schema> {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("schema.xsd");
    std::fs::write(&path, xsd).unwrap();
    Schema::from_file(&path)
}

fn schema_report(xsd: &str, xml: &str) -> Vec<(u32, String)> {
    let s = schema(xsd).unwrap();
    let d = doc(xml);
    assert!(d.is_well_formed(), "{:?}", d.problems);
    validate(&d.root.unwrap(), &s)
        .unwrap()
        .into_iter()
        .map(|p| {
            assert_eq!(p.kind, Kind::SchemaError);
            (p.line, p.message)
        })
        .collect()
}

const VALID_KEYNOTE: &str = r#"<presentation xmlns="urn:keynote">
  <meta id="k1">
    <title>T</title>
    <level>2</level>
    <kind>talk</kind>
  </meta>
  <slide template="plain">
    <title>S</title>
    <bullet size="big">Text <code language="rust">let x;</code> and <emphasis>more</emphasis> text</bullet>
    <bullet>Plain</bullet>
  </slide>
</presentation>
"#;

#[test]
fn a_valid_document_validates() {
    assert_eq!(
        schema_report(KEYNOTE_XSD, VALID_KEYNOTE),
        Vec::<(u32, String)>::new()
    );
}

#[test]
fn unexpected_and_missing_elements() {
    let xml = VALID_KEYNOTE
        .replace("<slide template=\"plain\">", "<slidex>")
        .replace("</slide>", "</slidex>");
    assert_eq!(
        schema_report(KEYNOTE_XSD, &xml),
        vec![(
            7,
            "Element '{urn:keynote}slidex': This element is not expected. Expected is ( {urn:keynote}slide )."
                .to_string()
        )]
    );

    let xml = "<presentation xmlns=\"urn:keynote\"><meta id=\"x\"/></presentation>";
    assert_eq!(
        schema_report(KEYNOTE_XSD, xml),
        vec![(
            1,
            "Element '{urn:keynote}presentation': Missing child element(s). Expected is ( {urn:keynote}slide )."
                .to_string()
        )]
    );

    // Out of order inside a sequence: the parent is reported first (its
    // own content model), then the child's problem.
    let xml = "<presentation xmlns=\"urn:keynote\"><meta id=\"x\"><kind>talk</kind><title>T</title></meta></presentation>";
    assert_eq!(
        schema_report(KEYNOTE_XSD, xml),
        vec![
            (
                1,
                "Element '{urn:keynote}presentation': Missing child element(s). Expected is ( {urn:keynote}slide )."
                    .to_string()
            ),
            (
                1,
                "Element '{urn:keynote}title': This element is not expected.".to_string()
            ),
        ]
    );

    // Several alternatives at the failing position.
    let xml = "<presentation xmlns=\"urn:keynote\"><meta id=\"x\"><bogus/></meta></presentation>";
    assert_eq!(
        schema_report(KEYNOTE_XSD, xml),
        vec![
            (
                1,
                "Element '{urn:keynote}presentation': Missing child element(s). Expected is ( {urn:keynote}slide )."
                    .to_string()
            ),
            (
                1,
                "Element '{urn:keynote}bogus': This element is not expected. Expected is one of ( {urn:keynote}title, {urn:keynote}level, {urn:keynote}kind )."
                    .to_string()
            ),
        ]
    );
}

#[test]
fn attributes_are_checked() {
    let xml = VALID_KEYNOTE.replace("<meta id=\"k1\">", "<meta>");
    assert_eq!(
        schema_report(KEYNOTE_XSD, &xml),
        vec![(
            2,
            "Element '{urn:keynote}meta': The attribute 'id' is required but missing.".to_string()
        )]
    );

    let xml = VALID_KEYNOTE.replace("<meta id=\"k1\">", "<meta id=\"k1\" extra=\"1\">");
    assert_eq!(
        schema_report(KEYNOTE_XSD, &xml),
        vec![(
            2,
            "Element '{urn:keynote}meta', attribute 'extra': The attribute 'extra' is not allowed."
                .to_string()
        )]
    );
}

#[test]
fn simple_types_and_facets() {
    let xml = VALID_KEYNOTE.replace("<level>2</level>", "<level>zero</level>");
    assert_eq!(
        schema_report(KEYNOTE_XSD, &xml),
        vec![(
            4,
            "Element '{urn:keynote}level': 'zero' is not a valid value of the atomic type 'xs:positiveInteger'."
                .to_string()
        )]
    );

    let xml = VALID_KEYNOTE.replace("<kind>talk</kind>", "<kind>lecture</kind>");
    assert_eq!(
        schema_report(KEYNOTE_XSD, &xml),
        vec![(
            5,
            "Element '{urn:keynote}kind': [facet 'enumeration'] The value 'lecture' is not an element of the set {'talk', 'workshop'}."
                .to_string()
        )]
    );

    let xml = VALID_KEYNOTE.replace("<title>T</title>", "<title><b>T</b></title>");
    assert_eq!(
        schema_report(KEYNOTE_XSD, &xml),
        vec![(
            3,
            "Element '{urn:keynote}title': Element content is not allowed, because the content type is a simple type definition."
                .to_string()
        )]
    );
}

#[test]
fn text_in_element_only_content() {
    let xml = VALID_KEYNOTE.replace("<meta id=\"k1\">", "<meta id=\"k1\">stray");
    assert_eq!(
        schema_report(KEYNOTE_XSD, &xml),
        vec![(
            2,
            "Element '{urn:keynote}meta': Character content other than whitespace is not allowed because the content type is 'element-only'."
                .to_string()
        )]
    );
}

#[test]
fn root_without_a_declaration() {
    assert_eq!(
        schema_report(KEYNOTE_XSD, "<slide xmlns=\"urn:keynote\"/>"),
        vec![(
            1,
            "Element '{urn:keynote}slide': No matching global declaration available for the validation root."
                .to_string()
        )]
    );
    assert_eq!(
        schema_report(KEYNOTE_XSD, "<presentation/>"),
        vec![(
            1,
            "Element 'presentation': No matching global declaration available for the validation root.".to_string()
        )]
    );
}

const SHAPES_XSD: &str = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:simpleType name="Color">
    <xs:restriction base="xs:string"><xs:pattern value="#[0-9a-f]{6}"/></xs:restriction>
  </xs:simpleType>
  <xs:simpleType name="Sizes">
    <xs:list itemType="xs:int"/>
  </xs:simpleType>
  <xs:simpleType name="ColorOrNumber">
    <xs:union memberTypes="Color xs:decimal"/>
  </xs:simpleType>
  <xs:attribute name="unit" type="xs:string"/>
  <xs:element name="shapes">
    <xs:complexType>
      <xs:choice minOccurs="0" maxOccurs="unbounded">
        <xs:element name="circle">
          <xs:complexType>
            <xs:all>
              <xs:element name="r" type="xs:decimal"/>
              <xs:element name="fill" type="Color" minOccurs="0"/>
            </xs:all>
            <xs:attribute ref="unit" use="required"/>
          </xs:complexType>
        </xs:element>
        <xs:element name="sizes" type="Sizes"/>
        <xs:element name="paint" type="ColorOrNumber"/>
        <xs:element ref="note"/>
        <xs:any namespace="##other" processContents="lax"/>
      </xs:choice>
      <xs:attribute name="version" type="xs:int" fixed="2"/>
    </xs:complexType>
  </xs:element>
  <xs:element name="note" type="xs:string"/>
</xs:schema>
"###;

#[test]
fn choice_all_list_union_refs_and_wildcards() {
    let xml = r#"<shapes version="2">
  <circle unit="px"><fill>#00ff00</fill><r>1.5</r></circle>
  <sizes>1 2 3</sizes>
  <paint>#123456</paint>
  <paint>0.5</paint>
  <note>hello</note>
  <x:extra xmlns:x="urn:x"><anything/></x:extra>
</shapes>"#;
    assert_eq!(schema_report(SHAPES_XSD, xml), Vec::<(u32, String)>::new());

    let bad = r#"<shapes version="3">
  <circle><r>1.5</r><r>2</r></circle>
  <sizes>1 two</sizes>
  <paint>red</paint>
  <local/>
</shapes>"#;
    let report = schema_report(SHAPES_XSD, bad);
    let messages: Vec<&str> = report.iter().map(|(_, m)| m.as_str()).collect();
    assert_eq!(
        messages,
        vec![
            "Element 'shapes', attribute 'version': The value '3' does not match the fixed value constraint '2'.",
            "Element 'local': This element is not expected. Expected is one of ( circle, sizes, paint, note, ##any ).",
            "Element 'circle': The attribute 'unit' is required but missing.",
            "Element 'r': This element is not expected. Expected is ( fill ).",
            "Element 'sizes': '1 two' is not a valid value of the list type: 'two' is not a valid value of the atomic type 'xs:int'.",
            "Element 'paint': 'red' is not a valid value of the union type.",
        ]
    );
    let lines: Vec<u32> = report.iter().map(|(l, _)| *l).collect();
    assert_eq!(lines, vec![1, 5, 2, 2, 3, 4]);
}

#[test]
fn pattern_facet() {
    let xml = "<shapes><circle unit=\"px\"><r>1</r><fill>green</fill></circle></shapes>";
    assert_eq!(
        schema_report(SHAPES_XSD, xml),
        vec![(
            1,
            "Element 'fill': [facet 'pattern'] The value 'green' is not accepted by the pattern '#[0-9a-f]{6}'."
                .to_string()
        )]
    );
}

#[test]
fn unsupported_schema_constructs_are_refused() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:group name="g"><xs:sequence/></xs:group>
</xs:schema>"#;
    let err = schema(xsd).unwrap_err().to_string();
    assert!(
        err.contains(":2: unsupported XSD construct: xs:group"),
        "{err}"
    );

    let err = schema("<not-a-schema/>").unwrap_err().to_string();
    assert!(err.contains("not an XML Schema"), "{err}");

    let err = schema(
        "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:element/></xs:schema>",
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("lacks the 'name' attribute"), "{err}");
}

#[test]
fn a_schema_can_include_another() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("main.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:include schemaLocation="types.xsd"/>
  <xs:element name="n" type="Small"/>
</xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("types.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:simpleType name="Small">
    <xs:restriction base="xs:int"><xs:maxInclusive value="9"/></xs:restriction>
  </xs:simpleType>
</xs:schema>"#,
    )
    .unwrap();
    let s = Schema::from_file(&dir.path().join("main.xsd")).unwrap();
    let ok = doc("<n>7</n>");
    assert_eq!(validate(ok.root.as_ref().unwrap(), &s).unwrap(), Vec::new());
    let bad = doc("<n>70</n>");
    let problems = validate(bad.root.as_ref().unwrap(), &s).unwrap();
    assert_eq!(
        problems[0].message,
        "Element 'n': [facet 'maxInclusive'] The value '70' is greater than the maximum value allowed ('9')."
    );
    assert!(Path::new(&dir.path().join("types.xsd")).exists());
}

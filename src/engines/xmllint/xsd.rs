//! XSD validation, as `xmllint --schema` does it, for the schema constructs
//! in use: global and local elements, named and anonymous complex types with
//! `sequence`/`choice`/`all` content models, `any`, mixed and simple content,
//! attributes with `use`/`fixed`, `anyAttribute`, simple types restricted by
//! facets, lists and unions, `extension` of simple and complex types, and
//! `include`. A schema using anything else (`group`, `attributeGroup`,
//! `import`, `redefine`, identity constraints, substitution groups, complex
//! `restriction`) is rejected with an error naming the construct: a schema
//! that is only half enforced would pass files xmllint rejects.
//!
//! Messages follow xmllint's wording so a file's report reads the same from
//! either tool.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use regex::Regex;

use super::{Child, Element, Kind, Problem, expanded_name};

const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema";
const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// Type references nested deeper than this are a cycle in the schema.
const MAX_TYPE_DEPTH: usize = 64;

/// A namespace-qualified name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QName {
    pub namespace: Option<String>,
    pub local: String,
}

impl QName {
    fn of(element: &Element) -> Self {
        Self {
            namespace: element.namespace.clone(),
            local: element.local.clone(),
        }
    }

    fn expanded(&self) -> String {
        expanded_name(self.namespace.as_deref(), &self.local)
    }
}

/// A compiled schema: the global declarations, by name.
#[derive(Debug, Default)]
pub struct Schema {
    target_namespace: Option<String>,
    elements: HashMap<QName, ElementDecl>,
    attributes: HashMap<QName, AttributeDecl>,
    types: HashMap<QName, Type>,
}

#[derive(Debug, Clone)]
enum Type {
    Simple(SimpleType),
    Complex(ComplexType),
}

#[derive(Debug, Clone)]
enum TypeRef {
    /// `xs:anyType`, and an element with neither a type nor a type child.
    AnyType,
    Named(QName),
    Simple(Box<SimpleType>),
    Complex(Box<ComplexType>),
}

#[derive(Debug, Clone)]
struct ElementDecl {
    name: QName,
    type_ref: TypeRef,
    nillable: bool,
    fixed: Option<String>,
}

#[derive(Debug, Clone)]
struct AttributeDecl {
    name: QName,
    type_ref: SimpleRef,
    fixed: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Use {
    Optional,
    Required,
    Prohibited,
}

#[derive(Debug, Clone)]
enum AttributeSource {
    Local(AttributeDecl),
    Ref(QName),
}

#[derive(Debug, Clone)]
struct AttributeUse {
    source: AttributeSource,
    use_: Use,
}

#[derive(Debug, Clone)]
struct ComplexType {
    mixed: bool,
    content: Content,
    attributes: Vec<AttributeUse>,
    any_attribute: bool,
    /// `complexContent/extension base`: the base's content precedes this
    /// type's, and its attributes are inherited.
    extends: Option<QName>,
}

#[derive(Debug, Clone)]
enum Content {
    Empty,
    Simple(SimpleRef),
    Particle(Particle),
}

#[derive(Debug, Clone)]
struct Particle {
    min: u32,
    /// `None` is `unbounded`.
    max: Option<u32>,
    term: Term,
}

#[derive(Debug, Clone)]
enum Term {
    Element(ElementDecl),
    ElementRef(QName),
    Sequence(Vec<Particle>),
    Choice(Vec<Particle>),
    All(Vec<Particle>),
    Any(Wildcard),
}

#[derive(Debug, Clone)]
struct Wildcard {
    namespace: WildcardNamespace,
    process: ProcessContents,
}

#[derive(Debug, Clone)]
enum WildcardNamespace {
    Any,
    /// `##other`: any namespace but the target one (and not absent).
    Other,
    /// A list of namespaces (`None` is `##local`).
    List(Vec<Option<String>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessContents {
    Strict,
    Lax,
    Skip,
}

#[derive(Debug, Clone)]
enum SimpleRef {
    Builtin(Builtin),
    Named(QName),
    Inline(Box<SimpleType>),
}

#[derive(Debug, Clone)]
struct SimpleType {
    variety: Variety,
    facets: Facets,
}

#[derive(Debug, Clone)]
enum Variety {
    Restriction(SimpleRef),
    List(SimpleRef),
    Union(Vec<SimpleRef>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhiteSpace {
    Preserve,
    Replace,
    Collapse,
}

#[derive(Debug, Clone, Default)]
struct Facets {
    enumeration: Vec<String>,
    /// `(source, compiled)`: the source is for the message.
    patterns: Vec<(String, Regex)>,
    length: Option<usize>,
    min_length: Option<usize>,
    max_length: Option<usize>,
    min_inclusive: Option<f64>,
    max_inclusive: Option<f64>,
    min_exclusive: Option<f64>,
    max_exclusive: Option<f64>,
    total_digits: Option<usize>,
    fraction_digits: Option<usize>,
    white_space: Option<WhiteSpace>,
}

impl Facets {
    /// The facets of a type derived from one with `base` facets: a derived
    /// enumeration or bound replaces the base's, patterns accumulate.
    fn on_top_of(&self, base: &Self) -> Self {
        let mut merged = base.clone();
        if !self.enumeration.is_empty() {
            merged.enumeration.clone_from(&self.enumeration);
        }
        merged.patterns.extend(self.patterns.iter().cloned());
        merged.length = self.length.or(base.length);
        merged.min_length = self.min_length.or(base.min_length);
        merged.max_length = self.max_length.or(base.max_length);
        merged.min_inclusive = self.min_inclusive.or(base.min_inclusive);
        merged.max_inclusive = self.max_inclusive.or(base.max_inclusive);
        merged.min_exclusive = self.min_exclusive.or(base.min_exclusive);
        merged.max_exclusive = self.max_exclusive.or(base.max_exclusive);
        merged.total_digits = self.total_digits.or(base.total_digits);
        merged.fraction_digits = self.fraction_digits.or(base.fraction_digits);
        merged.white_space = self.white_space.or(base.white_space);
        merged
    }
}

/// A simple type with every derivation step folded in.
#[derive(Debug, Clone)]
struct Flat {
    kind: FlatKind,
    facets: Facets,
}

#[derive(Debug, Clone)]
enum FlatKind {
    Atomic(Builtin),
    List(Box<Flat>),
    Union(Vec<Flat>),
}

// ---------------------------------------------------------------------------
// Built-in simple types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Builtin {
    AnySimpleType,
    String,
    NormalizedString,
    Token,
    Language,
    Name,
    NcName,
    Id,
    IdRef,
    IdRefs,
    Entity,
    Entities,
    NmToken,
    NmTokens,
    Boolean,
    Decimal,
    Integer,
    NonNegativeInteger,
    PositiveInteger,
    NonPositiveInteger,
    NegativeInteger,
    Long,
    Int,
    Short,
    Byte,
    UnsignedLong,
    UnsignedInt,
    UnsignedShort,
    UnsignedByte,
    Float,
    Double,
    AnyUri,
    QName,
    Duration,
    DateTime,
    Date,
    Time,
    GYear,
    GYearMonth,
    GMonth,
    GMonthDay,
    GDay,
    HexBinary,
    Base64Binary,
}

impl Builtin {
    fn from_local(local: &str) -> Option<Self> {
        Some(match local {
            "anySimpleType" => Self::AnySimpleType,
            "string" => Self::String,
            "normalizedString" => Self::NormalizedString,
            "token" => Self::Token,
            "language" => Self::Language,
            "Name" => Self::Name,
            "NCName" => Self::NcName,
            "ID" => Self::Id,
            "IDREF" => Self::IdRef,
            "IDREFS" => Self::IdRefs,
            "ENTITY" => Self::Entity,
            "ENTITIES" => Self::Entities,
            "NMTOKEN" => Self::NmToken,
            "NMTOKENS" => Self::NmTokens,
            "boolean" => Self::Boolean,
            "decimal" => Self::Decimal,
            "integer" => Self::Integer,
            "nonNegativeInteger" => Self::NonNegativeInteger,
            "positiveInteger" => Self::PositiveInteger,
            "nonPositiveInteger" => Self::NonPositiveInteger,
            "negativeInteger" => Self::NegativeInteger,
            "long" => Self::Long,
            "int" => Self::Int,
            "short" => Self::Short,
            "byte" => Self::Byte,
            "unsignedLong" => Self::UnsignedLong,
            "unsignedInt" => Self::UnsignedInt,
            "unsignedShort" => Self::UnsignedShort,
            "unsignedByte" => Self::UnsignedByte,
            "float" => Self::Float,
            "double" => Self::Double,
            "anyURI" => Self::AnyUri,
            "QName" => Self::QName,
            "duration" => Self::Duration,
            "dateTime" => Self::DateTime,
            "date" => Self::Date,
            "time" => Self::Time,
            "gYear" => Self::GYear,
            "gYearMonth" => Self::GYearMonth,
            "gMonth" => Self::GMonth,
            "gMonthDay" => Self::GMonthDay,
            "gDay" => Self::GDay,
            "hexBinary" => Self::HexBinary,
            "base64Binary" => Self::Base64Binary,
            _ => return None,
        })
    }

    /// The type's name as xmllint prints it.
    const fn xs_name(self) -> &'static str {
        match self {
            Self::AnySimpleType => "xs:anySimpleType",
            Self::String => "xs:string",
            Self::NormalizedString => "xs:normalizedString",
            Self::Token => "xs:token",
            Self::Language => "xs:language",
            Self::Name => "xs:Name",
            Self::NcName => "xs:NCName",
            Self::Id => "xs:ID",
            Self::IdRef => "xs:IDREF",
            Self::IdRefs => "xs:IDREFS",
            Self::Entity => "xs:ENTITY",
            Self::Entities => "xs:ENTITIES",
            Self::NmToken => "xs:NMTOKEN",
            Self::NmTokens => "xs:NMTOKENS",
            Self::Boolean => "xs:boolean",
            Self::Decimal => "xs:decimal",
            Self::Integer => "xs:integer",
            Self::NonNegativeInteger => "xs:nonNegativeInteger",
            Self::PositiveInteger => "xs:positiveInteger",
            Self::NonPositiveInteger => "xs:nonPositiveInteger",
            Self::NegativeInteger => "xs:negativeInteger",
            Self::Long => "xs:long",
            Self::Int => "xs:int",
            Self::Short => "xs:short",
            Self::Byte => "xs:byte",
            Self::UnsignedLong => "xs:unsignedLong",
            Self::UnsignedInt => "xs:unsignedInt",
            Self::UnsignedShort => "xs:unsignedShort",
            Self::UnsignedByte => "xs:unsignedByte",
            Self::Float => "xs:float",
            Self::Double => "xs:double",
            Self::AnyUri => "xs:anyURI",
            Self::QName => "xs:QName",
            Self::Duration => "xs:duration",
            Self::DateTime => "xs:dateTime",
            Self::Date => "xs:date",
            Self::Time => "xs:time",
            Self::GYear => "xs:gYear",
            Self::GYearMonth => "xs:gYearMonth",
            Self::GMonth => "xs:gMonth",
            Self::GMonthDay => "xs:gMonthDay",
            Self::GDay => "xs:gDay",
            Self::HexBinary => "xs:hexBinary",
            Self::Base64Binary => "xs:base64Binary",
        }
    }

    const fn white_space(self) -> WhiteSpace {
        match self {
            Self::String | Self::AnySimpleType => WhiteSpace::Preserve,
            Self::NormalizedString => WhiteSpace::Replace,
            _ => WhiteSpace::Collapse,
        }
    }

    /// Whether `value` (already whitespace-normalized) is in the type's
    /// lexical space. `scope` resolves `QName` prefixes.
    fn accepts(self, value: &str, scope: &Element) -> bool {
        match self {
            Self::AnySimpleType
            | Self::String
            | Self::NormalizedString
            | Self::Token
            | Self::AnyUri => true,
            Self::Language => LANGUAGE.is_match(value),
            Self::Name => is_name(value),
            Self::NcName | Self::Id | Self::IdRef | Self::Entity => is_ncname(value),
            Self::IdRefs | Self::Entities => !value.is_empty() && value.split(' ').all(is_ncname),
            Self::NmToken => is_nmtoken(value),
            Self::NmTokens => !value.is_empty() && value.split(' ').all(is_nmtoken),
            Self::Boolean => matches!(value, "true" | "false" | "1" | "0"),
            Self::Decimal => DECIMAL.is_match(value),
            Self::Integer => INTEGER.is_match(value),
            Self::NonNegativeInteger => integer_in(value, 0, i128::MAX),
            Self::PositiveInteger => integer_in(value, 1, i128::MAX),
            Self::NonPositiveInteger => integer_in(value, i128::MIN, 0),
            Self::NegativeInteger => integer_in(value, i128::MIN, -1),
            Self::Long => integer_in(value, i128::from(i64::MIN), i128::from(i64::MAX)),
            Self::Int => integer_in(value, i128::from(i32::MIN), i128::from(i32::MAX)),
            Self::Short => integer_in(value, i128::from(i16::MIN), i128::from(i16::MAX)),
            Self::Byte => integer_in(value, i128::from(i8::MIN), i128::from(i8::MAX)),
            Self::UnsignedLong => integer_in(value, 0, i128::from(u64::MAX)),
            Self::UnsignedInt => integer_in(value, 0, i128::from(u32::MAX)),
            Self::UnsignedShort => integer_in(value, 0, i128::from(u16::MAX)),
            Self::UnsignedByte => integer_in(value, 0, i128::from(u8::MAX)),
            Self::Float | Self::Double => FLOAT.is_match(value),
            Self::QName => match value.split_once(':') {
                Some((prefix, local)) => {
                    is_ncname(prefix) && is_ncname(local) && scope.lookup_prefix(prefix).is_some()
                }
                None => is_ncname(value),
            },
            Self::Duration => {
                DURATION.is_match(value) && value.trim_end_matches('T') != "P" && value != "-P"
            }
            Self::DateTime => DATE_TIME.is_match(value),
            Self::Date => DATE.is_match(value),
            Self::Time => TIME.is_match(value),
            Self::GYear => G_YEAR.is_match(value),
            Self::GYearMonth => G_YEAR_MONTH.is_match(value),
            Self::GMonth => G_MONTH.is_match(value),
            Self::GMonthDay => G_MONTH_DAY.is_match(value),
            Self::GDay => G_DAY.is_match(value),
            Self::HexBinary => HEX_BINARY.is_match(value),
            Self::Base64Binary => {
                let compact: String = value.chars().filter(|c| *c != ' ').collect();
                BASE64.is_match(&compact) && compact.len().is_multiple_of(4)
            }
        }
    }
}

fn integer_in(value: &str, min: i128, max: i128) -> bool {
    INTEGER.is_match(value)
        && value
            .trim_start_matches('+')
            .parse::<i128>()
            .is_ok_and(|n| n >= min && n <= max)
}

fn is_name(value: &str) -> bool {
    use xmlparser::XmlCharExt;
    let mut chars = value.chars();
    chars.next().is_some_and(|c| c.is_xml_name_start()) && chars.all(|c| c.is_xml_name())
}

fn is_ncname(value: &str) -> bool {
    is_name(value) && !value.contains(':')
}

fn is_nmtoken(value: &str) -> bool {
    use xmlparser::XmlCharExt;
    !value.is_empty() && value.chars().all(|c| c.is_xml_name())
}

const TZ: &str = r"(Z|[+-]\d{2}:\d{2})?";

fn anchored(pattern: &str) -> Regex {
    Regex::new(&format!("^(?:{pattern})$")).expect("built-in XSD type regex is valid")
}

static LANGUAGE: LazyLock<Regex> = LazyLock::new(|| anchored(r"[a-zA-Z]{1,8}(-[a-zA-Z0-9]{1,8})*"));
static DECIMAL: LazyLock<Regex> = LazyLock::new(|| anchored(r"[+-]?(\d+(\.\d*)?|\.\d+)"));
static INTEGER: LazyLock<Regex> = LazyLock::new(|| anchored(r"[+-]?\d+"));
static FLOAT: LazyLock<Regex> =
    LazyLock::new(|| anchored(r"[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?|[+-]?INF|NaN"));
static DURATION: LazyLock<Regex> =
    LazyLock::new(|| anchored(r"-?P(\d+Y)?(\d+M)?(\d+D)?(T(\d+H)?(\d+M)?(\d+(\.\d+)?S)?)?"));
static DATE_TIME: LazyLock<Regex> = LazyLock::new(|| {
    anchored(&format!(
        r"-?\d{{4,}}-\d{{2}}-\d{{2}}T\d{{2}}:\d{{2}}:\d{{2}}(\.\d+)?{TZ}"
    ))
});
static DATE: LazyLock<Regex> =
    LazyLock::new(|| anchored(&format!(r"-?\d{{4,}}-\d{{2}}-\d{{2}}{TZ}")));
static TIME: LazyLock<Regex> =
    LazyLock::new(|| anchored(&format!(r"\d{{2}}:\d{{2}}:\d{{2}}(\.\d+)?{TZ}")));
static G_YEAR: LazyLock<Regex> = LazyLock::new(|| anchored(&format!(r"-?\d{{4,}}{TZ}")));
static G_YEAR_MONTH: LazyLock<Regex> =
    LazyLock::new(|| anchored(&format!(r"-?\d{{4,}}-\d{{2}}{TZ}")));
static G_MONTH: LazyLock<Regex> = LazyLock::new(|| anchored(&format!(r"--\d{{2}}{TZ}")));
static G_MONTH_DAY: LazyLock<Regex> =
    LazyLock::new(|| anchored(&format!(r"--\d{{2}}-\d{{2}}{TZ}")));
static G_DAY: LazyLock<Regex> = LazyLock::new(|| anchored(&format!(r"---\d{{2}}{TZ}")));
static HEX_BINARY: LazyLock<Regex> = LazyLock::new(|| anchored(r"([0-9a-fA-F]{2})*"));
static BASE64: LazyLock<Regex> = LazyLock::new(|| anchored(r"[A-Za-z0-9+/]*={0,2}"));

// ---------------------------------------------------------------------------
// Loading a schema
// ---------------------------------------------------------------------------

/// Reads schema documents: the one named plus whatever it `include`s.
struct Loader<'a> {
    schema: &'a mut Schema,
    /// The file being read, for messages.
    path: String,
    element_form_qualified: bool,
    attribute_form_qualified: bool,
}

impl Schema {
    /// Compiles the schema in `path`, a well-formed XSD document.
    pub fn from_file(path: &Path) -> Result<Self> {
        let mut schema = Self::default();
        schema.load(path, true)?;
        Ok(schema)
    }

    fn load(&mut self, path: &Path, first: bool) -> Result<()> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read XSD schema {}", path.display()))?;
        let document = super::parse(&bytes);
        if let Some(problem) = document.problems.iter().find(|p| p.kind.is_fatal()) {
            bail!("{}:{problem}", path.display());
        }
        let Some(root) = document.root else {
            bail!("{}: no root element", path.display());
        };
        if root.namespace.as_deref() != Some(XSD_NS) || root.local != "schema" {
            bail!(
                "{}: not an XML Schema: the root element is '{}', not '{{{XSD_NS}}}schema'",
                path.display(),
                root.expanded_name()
            );
        }
        let target = attr(&root, "targetNamespace").map(str::to_string);
        if first {
            self.target_namespace = target;
        } else if target.is_some() && target != self.target_namespace {
            bail!(
                "{}: included schema has target namespace {:?}, the including one {:?}",
                path.display(),
                target,
                self.target_namespace
            );
        }
        let mut loader = Loader {
            schema: self,
            path: path.display().to_string(),
            element_form_qualified: attr(&root, "elementFormDefault") == Some("qualified"),
            attribute_form_qualified: attr(&root, "attributeFormDefault") == Some("qualified"),
        };
        let includes = loader.schema_children(&root)?;
        for include in includes {
            let include_path = path.parent().map_or_else(
                || Path::new(&include).to_path_buf(),
                |dir| dir.join(&include),
            );
            self.load(&include_path, false)?;
        }
        Ok(())
    }
}

/// The value of an unqualified attribute.
fn attr<'e>(element: &'e Element, local: &str) -> Option<&'e str> {
    element.attribute(None, local).map(|a| a.value.as_str())
}

impl Loader<'_> {
    fn unsupported(&self, element: &Element, what: &str) -> anyhow::Error {
        anyhow::anyhow!(
            "{}:{}: unsupported XSD construct: {what}. ixmllint implements the schema \
             constructs in use across the fleet; keep this file on xmllint or extend \
             src/engines/xmllint/xsd.rs",
            self.path,
            element.line
        )
    }

    fn xsd_children<'e>(&self, element: &'e Element) -> Vec<&'e Element> {
        element
            .child_elements()
            .filter(|c| c.namespace.as_deref() == Some(XSD_NS) && c.local != "annotation")
            .collect()
    }

    /// Resolves a `QName` written in an attribute value, in the element's
    /// namespace scope. An unprefixed name is in the default namespace,
    /// which for schema documents is usually the target namespace or none.
    fn qname(&self, element: &Element, value: &str) -> Result<QName> {
        let (prefix, local) = value.split_once(':').unwrap_or(("", value));
        let namespace = match element.lookup_prefix(prefix) {
            Some(uri) => Some(uri.to_string()),
            None if prefix.is_empty() => None,
            None => bail!(
                "{}:{}: undeclared namespace prefix in '{value}'",
                self.path,
                element.line
            ),
        };
        Ok(QName {
            namespace,
            local: local.to_string(),
        })
    }

    /// The `xs:schema` children; returns the `include`d schema locations.
    fn schema_children(&mut self, root: &Element) -> Result<Vec<String>> {
        let mut includes = Vec::new();
        for child in self.xsd_children(root) {
            match child.local.as_str() {
                "element" => {
                    let decl = self.element_decl(child, true)?;
                    self.schema.elements.insert(decl.name.clone(), decl);
                }
                "attribute" => {
                    let decl = self.attribute_decl(child, true)?;
                    self.schema.attributes.insert(decl.name.clone(), decl);
                }
                "complexType" => {
                    let name = self.required(child, "name")?;
                    let ty = self.complex_type(child)?;
                    self.schema
                        .types
                        .insert(self.target_qname(name), Type::Complex(ty));
                }
                "simpleType" => {
                    let name = self.required(child, "name")?;
                    let ty = self.simple_type(child)?;
                    self.schema
                        .types
                        .insert(self.target_qname(name), Type::Simple(ty));
                }
                "include" => includes.push(self.required(child, "schemaLocation")?.to_string()),
                "import" => return Err(self.unsupported(child, "xs:import")),
                other => return Err(self.unsupported(child, &format!("xs:{other}"))),
            }
        }
        Ok(includes)
    }

    fn required<'e>(&self, element: &'e Element, name: &str) -> Result<&'e str> {
        attr(element, name).ok_or_else(|| {
            anyhow::anyhow!(
                "{}:{}: xs:{} lacks the '{name}' attribute",
                self.path,
                element.line,
                element.local
            )
        })
    }

    fn target_qname(&self, local: &str) -> QName {
        QName {
            namespace: self.schema.target_namespace.clone(),
            local: local.to_string(),
        }
    }

    fn occurs(&self, element: &Element) -> Result<(u32, Option<u32>)> {
        let min = match attr(element, "minOccurs") {
            None => 1,
            Some(v) => v
                .parse()
                .with_context(|| format!("{}:{}: bad minOccurs {v:?}", self.path, element.line))?,
        };
        let max =
            match attr(element, "maxOccurs") {
                None => Some(1),
                Some("unbounded") => None,
                Some(v) => Some(v.parse().with_context(|| {
                    format!("{}:{}: bad maxOccurs {v:?}", self.path, element.line)
                })?),
            };
        Ok((min, max))
    }

    fn element_decl(&self, element: &Element, global: bool) -> Result<ElementDecl> {
        for forbidden in ["abstract", "substitutionGroup"] {
            if attr(element, forbidden).is_some() {
                return Err(self.unsupported(element, &format!("xs:element {forbidden}")));
            }
        }
        let name = self.required(element, "name")?;
        let qualified = global
            || match attr(element, "form") {
                Some(form) => form == "qualified",
                None => self.element_form_qualified,
            };
        let name = QName {
            namespace: if qualified {
                self.schema.target_namespace.clone()
            } else {
                None
            },
            local: name.to_string(),
        };
        let mut type_ref = match attr(element, "type") {
            Some(type_name) => self.type_ref(element, type_name)?,
            None => TypeRef::AnyType,
        };
        for child in self.xsd_children(element) {
            match child.local.as_str() {
                "complexType" => type_ref = TypeRef::Complex(Box::new(self.complex_type(child)?)),
                "simpleType" => type_ref = TypeRef::Simple(Box::new(self.simple_type(child)?)),
                other => return Err(self.unsupported(child, &format!("xs:{other} in xs:element"))),
            }
        }
        Ok(ElementDecl {
            name,
            type_ref,
            nillable: matches!(attr(element, "nillable"), Some("true" | "1")),
            fixed: attr(element, "fixed").map(str::to_string),
        })
    }

    fn type_ref(&self, element: &Element, type_name: &str) -> Result<TypeRef> {
        let qname = self.qname(element, type_name)?;
        if qname.namespace.as_deref() == Some(XSD_NS) {
            if qname.local == "anyType" {
                return Ok(TypeRef::AnyType);
            }
            return match Builtin::from_local(&qname.local) {
                Some(builtin) => Ok(TypeRef::Simple(Box::new(SimpleType {
                    variety: Variety::Restriction(SimpleRef::Builtin(builtin)),
                    facets: Facets::default(),
                }))),
                None => {
                    Err(self.unsupported(element, &format!("built-in type xs:{}", qname.local)))
                }
            };
        }
        Ok(TypeRef::Named(qname))
    }

    fn simple_ref(&self, element: &Element, type_name: &str) -> Result<SimpleRef> {
        let qname = self.qname(element, type_name)?;
        if qname.namespace.as_deref() == Some(XSD_NS) {
            return match Builtin::from_local(&qname.local) {
                Some(builtin) => Ok(SimpleRef::Builtin(builtin)),
                None => {
                    Err(self.unsupported(element, &format!("built-in type xs:{}", qname.local)))
                }
            };
        }
        Ok(SimpleRef::Named(qname))
    }

    fn attribute_decl(&self, element: &Element, global: bool) -> Result<AttributeDecl> {
        let name = self.required(element, "name")?;
        let qualified = global
            || match attr(element, "form") {
                Some(form) => form == "qualified",
                None => self.attribute_form_qualified,
            };
        let mut type_ref = match attr(element, "type") {
            Some(type_name) => self.simple_ref(element, type_name)?,
            None => SimpleRef::Builtin(Builtin::AnySimpleType),
        };
        for child in self.xsd_children(element) {
            match child.local.as_str() {
                "simpleType" => type_ref = SimpleRef::Inline(Box::new(self.simple_type(child)?)),
                other => {
                    return Err(self.unsupported(child, &format!("xs:{other} in xs:attribute")));
                }
            }
        }
        Ok(AttributeDecl {
            name: QName {
                namespace: if qualified {
                    self.schema.target_namespace.clone()
                } else {
                    None
                },
                local: name.to_string(),
            },
            type_ref,
            fixed: attr(element, "fixed").map(str::to_string),
        })
    }

    fn attribute_use(&self, element: &Element) -> Result<AttributeUse> {
        let use_ = match attr(element, "use") {
            None | Some("optional") => Use::Optional,
            Some("required") => Use::Required,
            Some("prohibited") => Use::Prohibited,
            Some(other) => bail!(
                "{}:{}: bad attribute use {other:?}",
                self.path,
                element.line
            ),
        };
        let source = match attr(element, "ref") {
            Some(reference) => AttributeSource::Ref(self.qname(element, reference)?),
            None => AttributeSource::Local(self.attribute_decl(element, false)?),
        };
        Ok(AttributeUse { source, use_ })
    }

    fn complex_type(&self, element: &Element) -> Result<ComplexType> {
        if attr(element, "abstract") == Some("true") {
            return Err(self.unsupported(element, "xs:complexType abstract"));
        }
        let mut ty = ComplexType {
            mixed: matches!(attr(element, "mixed"), Some("true" | "1")),
            content: Content::Empty,
            attributes: Vec::new(),
            any_attribute: false,
            extends: None,
        };
        for child in self.xsd_children(element) {
            match child.local.as_str() {
                "sequence" | "choice" | "all" => {
                    ty.content = Content::Particle(self.particle(child)?);
                }
                "attribute" => ty.attributes.push(self.attribute_use(child)?),
                "anyAttribute" => ty.any_attribute = true,
                "simpleContent" => self.simple_content(child, &mut ty)?,
                "complexContent" => self.complex_content(child, &mut ty)?,
                other => {
                    return Err(self.unsupported(child, &format!("xs:{other} in xs:complexType")));
                }
            }
        }
        Ok(ty)
    }

    fn simple_content(&self, element: &Element, ty: &mut ComplexType) -> Result<()> {
        for child in self.xsd_children(element) {
            match child.local.as_str() {
                "extension" => {
                    let base = self.required(child, "base")?;
                    ty.content = Content::Simple(self.simple_ref(child, base)?);
                    self.attributes_of(child, ty)?;
                }
                other => {
                    return Err(self.unsupported(child, &format!("xs:{other} in xs:simpleContent")));
                }
            }
        }
        Ok(())
    }

    fn complex_content(&self, element: &Element, ty: &mut ComplexType) -> Result<()> {
        if matches!(attr(element, "mixed"), Some("true" | "1")) {
            ty.mixed = true;
        }
        for child in self.xsd_children(element) {
            match child.local.as_str() {
                "extension" => {
                    let base = self.required(child, "base")?;
                    let base = self.qname(child, base)?;
                    if base.namespace.as_deref() == Some(XSD_NS) {
                        if base.local != "anyType" {
                            return Err(
                                self.unsupported(child, &format!("extension of xs:{}", base.local))
                            );
                        }
                    } else {
                        ty.extends = Some(base);
                    }
                    for part in self.xsd_children(child) {
                        match part.local.as_str() {
                            "sequence" | "choice" | "all" => {
                                ty.content = Content::Particle(self.particle(part)?);
                            }
                            "attribute" => ty.attributes.push(self.attribute_use(part)?),
                            "anyAttribute" => ty.any_attribute = true,
                            other => {
                                return Err(
                                    self.unsupported(part, &format!("xs:{other} in xs:extension"))
                                );
                            }
                        }
                    }
                }
                other => {
                    return Err(
                        self.unsupported(child, &format!("xs:{other} in xs:complexContent"))
                    );
                }
            }
        }
        Ok(())
    }

    /// `attribute`/`anyAttribute` children of an extension.
    fn attributes_of(&self, element: &Element, ty: &mut ComplexType) -> Result<()> {
        for child in self.xsd_children(element) {
            match child.local.as_str() {
                "attribute" => ty.attributes.push(self.attribute_use(child)?),
                "anyAttribute" => ty.any_attribute = true,
                other => {
                    return Err(self.unsupported(child, &format!("xs:{other} in xs:extension")));
                }
            }
        }
        Ok(())
    }

    /// A `sequence`, `choice`, `all`, `element` or `any` as a particle.
    fn particle(&self, element: &Element) -> Result<Particle> {
        let (min, max) = self.occurs(element)?;
        let term = match element.local.as_str() {
            "sequence" => Term::Sequence(self.particles(element)?),
            "choice" => Term::Choice(self.particles(element)?),
            "all" => Term::All(self.particles(element)?),
            "element" => match attr(element, "ref") {
                Some(reference) => Term::ElementRef(self.qname(element, reference)?),
                None => Term::Element(self.element_decl(element, false)?),
            },
            "any" => Term::Any(self.wildcard(element)?),
            other => return Err(self.unsupported(element, &format!("xs:{other} as a particle"))),
        };
        Ok(Particle { min, max, term })
    }

    fn particles(&self, element: &Element) -> Result<Vec<Particle>> {
        self.xsd_children(element)
            .into_iter()
            .map(|child| self.particle(child))
            .collect()
    }

    fn wildcard(&self, element: &Element) -> Result<Wildcard> {
        let namespace = match attr(element, "namespace") {
            None | Some("##any") => WildcardNamespace::Any,
            Some("##other") => WildcardNamespace::Other,
            Some(list) => WildcardNamespace::List(
                list.split_whitespace()
                    .map(|item| match item {
                        "##local" => None,
                        "##targetNamespace" => self.schema.target_namespace.clone(),
                        uri => Some(uri.to_string()),
                    })
                    .collect(),
            ),
        };
        let process = match attr(element, "processContents") {
            None | Some("strict") => ProcessContents::Strict,
            Some("lax") => ProcessContents::Lax,
            Some("skip") => ProcessContents::Skip,
            Some(other) => bail!(
                "{}:{}: bad processContents {other:?}",
                self.path,
                element.line
            ),
        };
        Ok(Wildcard { namespace, process })
    }

    fn simple_type(&self, element: &Element) -> Result<SimpleType> {
        let children = self.xsd_children(element);
        let [derivation] = children.as_slice() else {
            bail!(
                "{}:{}: xs:simpleType needs exactly one of restriction, list or union",
                self.path,
                element.line
            );
        };
        match derivation.local.as_str() {
            "restriction" => {
                let mut facets = Facets::default();
                let mut base = match attr(derivation, "base") {
                    Some(base) => Some(self.simple_ref(derivation, base)?),
                    None => None,
                };
                for facet in self.xsd_children(derivation) {
                    if facet.local == "simpleType" {
                        base = Some(SimpleRef::Inline(Box::new(self.simple_type(facet)?)));
                    } else {
                        self.facet(facet, &mut facets)?;
                    }
                }
                let Some(base) = base else {
                    bail!(
                        "{}:{}: xs:restriction without a base",
                        self.path,
                        derivation.line
                    );
                };
                Ok(SimpleType {
                    variety: Variety::Restriction(base),
                    facets,
                })
            }
            "list" => {
                let item = match attr(derivation, "itemType") {
                    Some(item) => self.simple_ref(derivation, item)?,
                    None => match self.xsd_children(derivation).as_slice() {
                        [inline] if inline.local == "simpleType" => {
                            SimpleRef::Inline(Box::new(self.simple_type(inline)?))
                        }
                        _ => bail!(
                            "{}:{}: xs:list without an item type",
                            self.path,
                            derivation.line
                        ),
                    },
                };
                Ok(SimpleType {
                    variety: Variety::List(item),
                    facets: Facets::default(),
                })
            }
            "union" => {
                let mut members = Vec::new();
                if let Some(names) = attr(derivation, "memberTypes") {
                    for name in names.split_whitespace() {
                        members.push(self.simple_ref(derivation, name)?);
                    }
                }
                for inline in self.xsd_children(derivation) {
                    if inline.local != "simpleType" {
                        return Err(
                            self.unsupported(inline, &format!("xs:{} in xs:union", inline.local))
                        );
                    }
                    members.push(SimpleRef::Inline(Box::new(self.simple_type(inline)?)));
                }
                if members.is_empty() {
                    bail!(
                        "{}:{}: xs:union without member types",
                        self.path,
                        derivation.line
                    );
                }
                Ok(SimpleType {
                    variety: Variety::Union(members),
                    facets: Facets::default(),
                })
            }
            other => Err(self.unsupported(derivation, &format!("xs:{other} in xs:simpleType"))),
        }
    }

    fn facet(&self, facet: &Element, facets: &mut Facets) -> Result<()> {
        let value = self.required(facet, "value")?;
        let number = |what: &str| -> Result<usize> {
            value.parse().with_context(|| {
                format!("{}:{}: bad {what} value {value:?}", self.path, facet.line)
            })
        };
        let float = |what: &str| -> Result<f64> {
            value.parse().with_context(|| {
                format!("{}:{}: bad {what} value {value:?}", self.path, facet.line)
            })
        };
        match facet.local.as_str() {
            "enumeration" => facets.enumeration.push(value.to_string()),
            "pattern" => {
                let regex = Regex::new(&format!("^(?:{value})$")).with_context(|| {
                    format!(
                        "{}:{}: pattern {value:?} is not a supported regex",
                        self.path, facet.line
                    )
                })?;
                facets.patterns.push((value.to_string(), regex));
            }
            "length" => facets.length = Some(number("length")?),
            "minLength" => facets.min_length = Some(number("minLength")?),
            "maxLength" => facets.max_length = Some(number("maxLength")?),
            "minInclusive" => facets.min_inclusive = Some(float("minInclusive")?),
            "maxInclusive" => facets.max_inclusive = Some(float("maxInclusive")?),
            "minExclusive" => facets.min_exclusive = Some(float("minExclusive")?),
            "maxExclusive" => facets.max_exclusive = Some(float("maxExclusive")?),
            "totalDigits" => facets.total_digits = Some(number("totalDigits")?),
            "fractionDigits" => facets.fraction_digits = Some(number("fractionDigits")?),
            "whiteSpace" => {
                facets.white_space = Some(match value {
                    "preserve" => WhiteSpace::Preserve,
                    "replace" => WhiteSpace::Replace,
                    "collapse" => WhiteSpace::Collapse,
                    other => bail!(
                        "{}:{}: bad whiteSpace value {other:?}",
                        self.path,
                        facet.line
                    ),
                });
            }
            other => return Err(self.unsupported(facet, &format!("facet xs:{other}"))),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// What the content-model matcher saw: how far into the children it got and
/// which element names it was prepared to accept at each position, in the
/// order they were tried (the schema's order, which is how xmllint lists
/// them). That is what the "not expected / expected is" message is built
/// from.
#[derive(Default)]
struct Trace {
    furthest: usize,
    expected: BTreeMap<usize, Vec<String>>,
}

impl Trace {
    fn expect(&mut self, position: usize, name: String) {
        let names = self.expected.entry(position).or_default();
        if !names.contains(&name) {
            names.push(name);
        }
    }
}

struct Validator<'s> {
    schema: &'s Schema,
    problems: Vec<Problem>,
}

pub(super) fn validate(root: &Element, schema: &Schema) -> Result<Vec<Problem>> {
    let mut validator = Validator {
        schema,
        problems: Vec::new(),
    };
    match schema.elements.get(&QName::of(root)) {
        Some(decl) => validator.element(root, decl)?,
        None => validator.problem(
            root,
            format!(
                "Element '{}': No matching global declaration available for the validation root.",
                root.expanded_name()
            ),
        ),
    }
    Ok(validator.problems)
}

impl Validator<'_> {
    fn problem(&mut self, element: &Element, message: String) {
        self.problems.push(Problem {
            line: element.end_line,
            kind: Kind::SchemaError,
            message,
        });
    }

    fn named_type(&self, name: &QName) -> Result<&Type> {
        self.schema.types.get(name).ok_or_else(|| {
            anyhow::anyhow!("schema refers to the undefined type '{}'", name.expanded())
        })
    }

    fn element(&mut self, element: &Element, decl: &ElementDecl) -> Result<()> {
        let shown = element.expanded_name();
        if element.attribute(Some(XSI_NS), "type").is_some() {
            self.problem(
                element,
                format!("Element '{shown}': xsi:type is not supported by ixmllint."),
            );
            return Ok(());
        }
        if let Some(nil) = element.attribute(Some(XSI_NS), "nil") {
            if !decl.nillable {
                self.problem(
                    element,
                    format!(
                        "Element '{shown}', attribute '{{{XSI_NS}}}nil': The element is not 'nillable'."
                    ),
                );
            } else if matches!(nil.value.as_str(), "true" | "1") {
                if !element
                    .children
                    .iter()
                    .all(|c| matches!(c, Child::Text(t) if t.trim().is_empty()))
                {
                    self.problem(
                        element,
                        format!("Element '{shown}': The content must be empty since it's nilled."),
                    );
                }
                return Ok(());
            }
        }
        let fixed = decl.fixed.as_deref();
        match &decl.type_ref {
            TypeRef::AnyType => self.lax_children(element),
            TypeRef::Named(name) => match self.named_type(name)?.clone() {
                Type::Simple(simple) => self.simple_element(element, &simple, fixed),
                Type::Complex(complex) => self.complex_element(element, &complex, fixed),
            },
            TypeRef::Simple(simple) => self.simple_element(element, simple, fixed),
            TypeRef::Complex(complex) => self.complex_element(element, complex, fixed),
        }
    }

    /// Children of an `anyType` element or a lax wildcard: validated when a
    /// global declaration exists, accepted otherwise.
    fn lax_children(&mut self, element: &Element) -> Result<()> {
        for child in element.child_elements() {
            if let Some(decl) = self.schema.elements.get(&QName::of(child)) {
                self.element(child, decl)?;
            } else {
                self.lax_children(child)?;
            }
        }
        Ok(())
    }

    fn simple_element(
        &mut self,
        element: &Element,
        simple: &SimpleType,
        fixed: Option<&str>,
    ) -> Result<()> {
        let shown = element.expanded_name();
        if let Some(child) = element.child_elements().next() {
            self.problem(
                child,
                format!(
                    "Element '{shown}': Element content is not allowed, because the content type is a simple type definition."
                ),
            );
        }
        for attribute in &element.attributes {
            if attribute.namespace.as_deref() != Some(XSI_NS) {
                let name = expanded_name(attribute.namespace.as_deref(), &attribute.local);
                self.problem(
                    element,
                    format!("Element '{shown}', attribute '{name}': The attribute '{name}' is not allowed."),
                );
            }
        }
        let flat = self.flatten(&SimpleRef::Inline(Box::new(simple.clone())), 0)?;
        self.text_value(element, &flat, fixed);
        Ok(())
    }

    /// Checks an element's text against a simple type.
    fn text_value(&mut self, element: &Element, flat: &Flat, fixed: Option<&str>) {
        let shown = element.expanded_name();
        let text = element.text();
        match check_value(flat, &text, element) {
            Ok(normalized) => {
                if let Some(fixed) = fixed
                    && normalized != fixed
                {
                    self.problem(
                        element,
                        format!(
                            "Element '{shown}': The value '{normalized}' does not match the fixed value constraint '{fixed}'."
                        ),
                    );
                }
            }
            Err(message) => self.problem(element, format!("Element '{shown}': {message}")),
        }
    }

    fn complex_element(
        &mut self,
        element: &Element,
        complex: &ComplexType,
        fixed: Option<&str>,
    ) -> Result<()> {
        let complex = self.effective(complex, 0)?;
        self.attributes(element, &complex)?;
        let shown = element.expanded_name();
        let has_text = element
            .children
            .iter()
            .any(|c| matches!(c, Child::Text(t) if !t.trim().is_empty()));
        match &complex.content {
            Content::Empty => {
                if let Some(child) = element.child_elements().next() {
                    self.problem(
                        child,
                        format!(
                            "Element '{shown}': Element content is not allowed, because the content type is empty."
                        ),
                    );
                }
                if has_text {
                    self.problem(
                        element,
                        format!(
                            "Element '{shown}': Character content is not allowed, because the content type is empty."
                        ),
                    );
                }
            }
            Content::Simple(simple) => {
                if let Some(child) = element.child_elements().next() {
                    self.problem(
                        child,
                        format!(
                            "Element '{shown}': Element content is not allowed, because the content type is a simple type definition."
                        ),
                    );
                }
                let flat = self.flatten(simple, 0)?;
                self.text_value(element, &flat, fixed);
            }
            Content::Particle(particle) => {
                if has_text && !complex.mixed {
                    self.problem(
                        element,
                        format!(
                            "Element '{shown}': Character content other than whitespace is not allowed because the content type is 'element-only'."
                        ),
                    );
                }
                self.content_model(element, particle)?;
            }
        }
        Ok(())
    }

    /// A complex type with its `extension` bases folded in: base content
    /// first, then its own; attributes of both.
    fn effective(&self, complex: &ComplexType, depth: usize) -> Result<ComplexType> {
        let Some(base_name) = &complex.extends else {
            return Ok(complex.clone());
        };
        if depth > MAX_TYPE_DEPTH {
            bail!("schema type '{}' extends itself", base_name.expanded());
        }
        let Type::Complex(base) = self.named_type(base_name)? else {
            bail!(
                "schema type '{}' is extended as a complex type but is a simple type",
                base_name.expanded()
            );
        };
        let base = self.effective(base, depth + 1)?;
        let content = match (base.content, complex.content.clone()) {
            (Content::Particle(b), Content::Particle(own)) => Content::Particle(Particle {
                min: 1,
                max: Some(1),
                term: Term::Sequence(vec![b, own]),
            }),
            (Content::Empty, own) | (own, Content::Empty) => own,
            (Content::Simple(_), own @ Content::Simple(_)) => own,
            (Content::Simple(_), Content::Particle(_))
            | (Content::Particle(_), Content::Simple(_)) => {
                bail!(
                    "schema type extending '{}' mixes simple and element content",
                    base_name.expanded()
                )
            }
        };
        let mut attributes = base.attributes;
        attributes.extend(complex.attributes.iter().cloned());
        Ok(ComplexType {
            mixed: complex.mixed || base.mixed,
            content,
            attributes,
            any_attribute: complex.any_attribute || base.any_attribute,
            extends: None,
        })
    }

    fn attributes(&mut self, element: &Element, complex: &ComplexType) -> Result<()> {
        let shown = element.expanded_name();
        let mut uses: Vec<(AttributeDecl, Use)> = Vec::new();
        for use_ in &complex.attributes {
            let decl = match &use_.source {
                AttributeSource::Local(decl) => decl.clone(),
                AttributeSource::Ref(name) => {
                    self.schema.attributes.get(name).cloned().ok_or_else(|| {
                        anyhow::anyhow!(
                            "schema refers to the undefined attribute '{}'",
                            name.expanded()
                        )
                    })?
                }
            };
            uses.push((decl, use_.use_));
        }
        for attribute in &element.attributes {
            if attribute.namespace.as_deref() == Some(XSI_NS) {
                continue;
            }
            let name = QName {
                namespace: attribute.namespace.clone(),
                local: attribute.local.clone(),
            };
            let shown_attr = name.expanded();
            match uses.iter().find(|(decl, _)| decl.name == name) {
                None if complex.any_attribute => {}
                None | Some((_, Use::Prohibited)) => self.problem(
                    element,
                    format!(
                        "Element '{shown}', attribute '{shown_attr}': The attribute '{shown_attr}' is not allowed."
                    ),
                ),
                Some((decl, _)) => {
                    let flat = self.flatten(&decl.type_ref, 0)?;
                    match check_value(&flat, &attribute.value, element) {
                        Ok(normalized) => {
                            if let Some(fixed) = &decl.fixed
                                && &normalized != fixed
                            {
                                self.problem(
                                    element,
                                    format!(
                                        "Element '{shown}', attribute '{shown_attr}': The value '{normalized}' does not match the fixed value constraint '{fixed}'."
                                    ),
                                );
                            }
                        }
                        Err(message) => self.problem(
                            element,
                            format!("Element '{shown}', attribute '{shown_attr}': {message}"),
                        ),
                    }
                }
            }
        }
        for (decl, use_) in &uses {
            if *use_ == Use::Required
                && element
                    .attribute(decl.name.namespace.as_deref(), &decl.name.local)
                    .is_none()
            {
                self.problem(
                    element,
                    format!(
                        "Element '{shown}': The attribute '{}' is required but missing.",
                        decl.name.expanded()
                    ),
                );
            }
        }
        Ok(())
    }

    /// Matches the child elements against the content model, reports the
    /// first child that does not fit (or what is missing), then validates
    /// each child against the declaration the model gives its name.
    fn content_model(&mut self, element: &Element, particle: &Particle) -> Result<()> {
        let children: Vec<&Element> = element.child_elements().collect();
        let names: Vec<QName> = children.iter().map(|c| QName::of(c)).collect();
        let mut trace = Trace::default();
        let ends = self.particle_ends(particle, &names, 0, &mut trace)?;
        if !ends.contains(&names.len()) {
            let expected = trace
                .expected
                .get(&trace.furthest)
                .cloned()
                .unwrap_or_default();
            let expected_text = match expected.len() {
                0 => String::new(),
                1 => format!(" Expected is ( {} ).", expected[0]),
                _ => format!(" Expected is one of ( {} ).", expected.join(", ")),
            };
            match children.get(trace.furthest) {
                Some(child) => self.problem(
                    child,
                    format!(
                        "Element '{}': This element is not expected.{expected_text}",
                        child.expanded_name()
                    ),
                ),
                None => self.problem(
                    element,
                    format!(
                        "Element '{}': Missing child element(s).{expected_text}",
                        element.expanded_name()
                    ),
                ),
            }
        }
        let mut decls: HashMap<QName, ElementDecl> = HashMap::new();
        let mut wildcards: Vec<Wildcard> = Vec::new();
        self.collect_decls(&particle.term, &mut decls, &mut wildcards)?;
        for child in children {
            let name = QName::of(child);
            if let Some(decl) = decls.get(&name) {
                self.element(child, decl)?;
                continue;
            }
            let Some(wildcard) = wildcards.iter().find(|w| w.admits(&name, self.schema)) else {
                continue; // already reported as not expected
            };
            match wildcard.process {
                ProcessContents::Skip => {}
                ProcessContents::Lax => self.lax_children_or_self(child)?,
                ProcessContents::Strict => match self.schema.elements.get(&name) {
                    Some(decl) => self.element(child, decl)?,
                    None => self.problem(
                        child,
                        format!(
                            "Element '{}': No matching global element declaration available, but demanded by the strict wildcard.",
                            child.expanded_name()
                        ),
                    ),
                },
            }
        }
        Ok(())
    }

    fn lax_children_or_self(&mut self, element: &Element) -> Result<()> {
        match self.schema.elements.get(&QName::of(element)) {
            Some(decl) => self.element(element, decl),
            None => self.lax_children(element),
        }
    }

    /// Every element declaration a content model can match, by name. XSD's
    /// "element declarations consistent" rule makes the name decisive.
    fn collect_decls(
        &self,
        term: &Term,
        decls: &mut HashMap<QName, ElementDecl>,
        wildcards: &mut Vec<Wildcard>,
    ) -> Result<()> {
        match term {
            Term::Element(decl) => {
                decls
                    .entry(decl.name.clone())
                    .or_insert_with(|| decl.clone());
            }
            Term::ElementRef(name) => {
                let decl = self.global_element(name)?;
                decls
                    .entry(decl.name.clone())
                    .or_insert_with(|| decl.clone());
            }
            Term::Sequence(parts) | Term::Choice(parts) | Term::All(parts) => {
                for part in parts {
                    self.collect_decls(&part.term, decls, wildcards)?;
                }
            }
            Term::Any(wildcard) => wildcards.push(wildcard.clone()),
        }
        Ok(())
    }

    fn global_element(&self, name: &QName) -> Result<&ElementDecl> {
        self.schema.elements.get(name).ok_or_else(|| {
            anyhow::anyhow!(
                "schema refers to the undefined element '{}'",
                name.expanded()
            )
        })
    }

    /// The positions a particle can end at when it starts matching `names`
    /// at `start`.
    fn particle_ends(
        &self,
        particle: &Particle,
        names: &[QName],
        start: usize,
        trace: &mut Trace,
    ) -> Result<BTreeSet<usize>> {
        let mut results = BTreeSet::new();
        let mut frontier: BTreeSet<usize> = BTreeSet::from([start]);
        let mut count: u32 = 0;
        loop {
            if count >= particle.min {
                results.extend(frontier.iter().copied());
            }
            if particle.max.is_some_and(|max| count >= max) {
                break;
            }
            let mut next = BTreeSet::new();
            for &position in &frontier {
                next.extend(self.term_ends(&particle.term, names, position, trace)?);
            }
            if next.is_empty() {
                break;
            }
            count += 1;
            if next.is_subset(&frontier) {
                // Only empty matches are left; they can repeat to any
                // minimum, so these positions are final.
                results.extend(next.iter().copied());
                break;
            }
            frontier = next;
        }
        Ok(results)
    }

    fn term_ends(
        &self,
        term: &Term,
        names: &[QName],
        position: usize,
        trace: &mut Trace,
    ) -> Result<BTreeSet<usize>> {
        Ok(match term {
            Term::Element(decl) => self.element_step(&decl.name, names, position, trace),
            Term::ElementRef(name) => {
                let decl = self.global_element(name)?;
                self.element_step(&decl.name, names, position, trace)
            }
            Term::Any(wildcard) => {
                trace.expect(position, "##any".to_string());
                match names.get(position) {
                    Some(name) if wildcard.admits(name, self.schema) => {
                        trace.furthest = trace.furthest.max(position + 1);
                        BTreeSet::from([position + 1])
                    }
                    _ => BTreeSet::new(),
                }
            }
            Term::Sequence(parts) => {
                let mut frontier = BTreeSet::from([position]);
                for part in parts {
                    let mut next = BTreeSet::new();
                    for &p in &frontier {
                        next.extend(self.particle_ends(part, names, p, trace)?);
                    }
                    if next.is_empty() {
                        return Ok(next);
                    }
                    frontier = next;
                }
                frontier
            }
            Term::Choice(parts) => {
                let mut ends = BTreeSet::new();
                for part in parts {
                    ends.extend(self.particle_ends(part, names, position, trace)?);
                }
                ends
            }
            Term::All(parts) => {
                let mut used = vec![false; parts.len()];
                let mut ends = BTreeSet::new();
                self.all_ends(parts, &mut used, names, position, trace, &mut ends)?;
                ends
            }
        })
    }

    fn element_step(
        &self,
        expected: &QName,
        names: &[QName],
        position: usize,
        trace: &mut Trace,
    ) -> BTreeSet<usize> {
        trace.expect(position, expected.expanded());
        if names.get(position) == Some(expected) {
            trace.furthest = trace.furthest.max(position + 1);
            BTreeSet::from([position + 1])
        } else {
            BTreeSet::new()
        }
    }

    /// `xs:all`: each particle at most once, in any order.
    fn all_ends(
        &self,
        parts: &[Particle],
        used: &mut Vec<bool>,
        names: &[QName],
        position: usize,
        trace: &mut Trace,
        ends: &mut BTreeSet<usize>,
    ) -> Result<()> {
        if parts.iter().zip(used.iter()).all(|(p, &u)| u || p.min == 0) {
            ends.insert(position);
        }
        for i in 0..parts.len() {
            if used[i] {
                continue;
            }
            let steps = self.particle_ends(&parts[i], names, position, trace)?;
            for end in steps {
                if end == position {
                    continue;
                }
                used[i] = true;
                self.all_ends(parts, used, names, end, trace, ends)?;
                used[i] = false;
            }
        }
        Ok(())
    }

    /// A simple type with every derivation folded in.
    fn flatten(&self, reference: &SimpleRef, depth: usize) -> Result<Flat> {
        if depth > MAX_TYPE_DEPTH {
            bail!("schema simple type derivation is circular");
        }
        let simple = match reference {
            SimpleRef::Builtin(builtin) => {
                return Ok(Flat {
                    kind: FlatKind::Atomic(*builtin),
                    facets: Facets::default(),
                });
            }
            SimpleRef::Named(name) => match self.named_type(name)? {
                Type::Simple(simple) => simple,
                Type::Complex(_) => bail!(
                    "schema type '{}' is used as a simple type but is a complex type",
                    name.expanded()
                ),
            },
            SimpleRef::Inline(simple) => simple,
        };
        Ok(match &simple.variety {
            Variety::Restriction(base) => {
                let base = self.flatten(base, depth + 1)?;
                Flat {
                    kind: base.kind,
                    facets: simple.facets.on_top_of(&base.facets),
                }
            }
            Variety::List(item) => Flat {
                kind: FlatKind::List(Box::new(self.flatten(item, depth + 1)?)),
                facets: simple.facets.clone(),
            },
            Variety::Union(members) => Flat {
                kind: FlatKind::Union(
                    members
                        .iter()
                        .map(|m| self.flatten(m, depth + 1))
                        .collect::<Result<Vec<_>>>()?,
                ),
                facets: simple.facets.clone(),
            },
        })
    }
}

impl Wildcard {
    fn admits(&self, name: &QName, schema: &Schema) -> bool {
        match &self.namespace {
            WildcardNamespace::Any => true,
            WildcardNamespace::Other => {
                name.namespace.is_some() && name.namespace != schema.target_namespace
            }
            WildcardNamespace::List(list) => list.contains(&name.namespace),
        }
    }
}

fn normalize_whitespace(value: &str, mode: WhiteSpace) -> String {
    match mode {
        WhiteSpace::Preserve => value.to_string(),
        WhiteSpace::Replace => value
            .chars()
            .map(|c| {
                if matches!(c, '\t' | '\n' | '\r') {
                    ' '
                } else {
                    c
                }
            })
            .collect(),
        WhiteSpace::Collapse => value.split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

/// Checks a value against a flattened simple type. `Ok` carries the
/// whitespace-normalized value (what a `fixed` constraint compares with);
/// `Err` carries xmllint's message tail, after `Element 'x': `.
fn check_value(flat: &Flat, raw: &str, scope: &Element) -> Result<String, String> {
    let mode = flat.facets.white_space.unwrap_or_else(|| match &flat.kind {
        FlatKind::Atomic(builtin) => builtin.white_space(),
        FlatKind::List(_) => WhiteSpace::Collapse,
        FlatKind::Union(_) => WhiteSpace::Preserve,
    });
    let value = normalize_whitespace(raw, mode);
    let length = match &flat.kind {
        FlatKind::Atomic(builtin) => {
            if !builtin.accepts(&value, scope) {
                return Err(format!(
                    "'{value}' is not a valid value of the atomic type '{}'.",
                    builtin.xs_name()
                ));
            }
            value.chars().count()
        }
        FlatKind::List(item) => {
            let items: Vec<&str> = value.split(' ').filter(|s| !s.is_empty()).collect();
            for element in &items {
                check_value(item, element, scope).map_err(|message| {
                    format!("'{value}' is not a valid value of the list type: {message}")
                })?;
            }
            items.len()
        }
        FlatKind::Union(members) => {
            if !members
                .iter()
                .any(|m| check_value(m, &value, scope).is_ok())
            {
                return Err(format!("'{value}' is not a valid value of the union type."));
            }
            value.chars().count()
        }
    };
    check_facets(&flat.facets, &value, length)?;
    Ok(value)
}

fn check_facets(facets: &Facets, value: &str, length: usize) -> Result<(), String> {
    if !facets.enumeration.is_empty() && !facets.enumeration.iter().any(|e| e == value) {
        let set = facets
            .enumeration
            .iter()
            .map(|e| format!("'{e}'"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "[facet 'enumeration'] The value '{value}' is not an element of the set {{{set}}}."
        ));
    }
    for (source, regex) in &facets.patterns {
        if !regex.is_match(value) {
            return Err(format!(
                "[facet 'pattern'] The value '{value}' is not accepted by the pattern '{source}'."
            ));
        }
    }
    if let Some(expected) = facets.length
        && length != expected
    {
        return Err(format!(
            "[facet 'length'] The value has a length of '{length}'; this differs from the allowed length of '{expected}'."
        ));
    }
    if let Some(min) = facets.min_length
        && length < min
    {
        return Err(format!(
            "[facet 'minLength'] The value has a length of '{length}'; this underruns the allowed minimum length of '{min}'."
        ));
    }
    if let Some(max) = facets.max_length
        && length > max
    {
        return Err(format!(
            "[facet 'maxLength'] The value has a length of '{length}'; this exceeds the allowed maximum length of '{max}'."
        ));
    }
    let number = value.parse::<f64>().ok();
    if let (Some(min), Some(n)) = (facets.min_inclusive, number)
        && n < min
    {
        return Err(format!(
            "[facet 'minInclusive'] The value '{value}' is less than the minimum value allowed ('{min}')."
        ));
    }
    if let (Some(max), Some(n)) = (facets.max_inclusive, number)
        && n > max
    {
        return Err(format!(
            "[facet 'maxInclusive'] The value '{value}' is greater than the maximum value allowed ('{max}')."
        ));
    }
    if let (Some(min), Some(n)) = (facets.min_exclusive, number)
        && n <= min
    {
        return Err(format!(
            "[facet 'minExclusive'] The value '{value}' must be greater than '{min}'."
        ));
    }
    if let (Some(max), Some(n)) = (facets.max_exclusive, number)
        && n >= max
    {
        return Err(format!(
            "[facet 'maxExclusive'] The value '{value}' must be less than '{max}'."
        ));
    }
    if let Some(total) = facets.total_digits {
        let digits = value.chars().filter(char::is_ascii_digit).count();
        if digits > total {
            return Err(format!(
                "[facet 'totalDigits'] The value '{value}' has more digits than are allowed ('{total}')."
            ));
        }
    }
    if let Some(fraction) = facets.fraction_digits {
        let digits = value
            .split_once('.')
            .map_or(0, |(_, f)| f.chars().filter(char::is_ascii_digit).count());
        if digits > fraction {
            return Err(format!(
                "[facet 'fractionDigits'] The value '{value}' has more fractional digits than are allowed ('{fraction}')."
            ));
        }
    }
    Ok(())
}

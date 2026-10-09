//! tidy's static tables: HTML version bits, content models, the tag and
//! attribute dictionaries, per-tag attribute versions and the entity list.
//!
//! The dictionaries are data extracted from tidy's C sources by
//! `scripts/gen-itidy-tables.py` into `tables.json` and read once per
//! process; the version and content-model bits are the constants of
//! tidy's lexer.h.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

// HTML versions (lexer.h).
pub const HT20: u32 = 1;
pub const HT32: u32 = 2;
pub const H40S: u32 = 4;
pub const H40T: u32 = 8;
pub const H40F: u32 = 16;
pub const H41S: u32 = 32;
pub const H41T: u32 = 64;
pub const H41F: u32 = 128;
pub const X10S: u32 = 256;
pub const X10T: u32 = 512;
pub const X10F: u32 = 1024;
pub const XH11: u32 = 2048;
pub const XB10: u32 = 4096;
pub const VERS_SUN: u32 = 8192;
pub const VERS_NETSCAPE: u32 = 16384;
pub const VERS_MICROSOFT: u32 = 32768;
pub const HT50: u32 = 131_072;
pub const XH50: u32 = 262_144;

pub const VERS_UNKNOWN: u32 = 0;
pub const VERS_HTML20: u32 = HT20;
pub const VERS_HTML32: u32 = HT32;
pub const VERS_HTML40_STRICT: u32 = H40S | H41S | X10S;
pub const VERS_HTML40_LOOSE: u32 = H40T | H41T | X10T;
pub const VERS_FRAMESET: u32 = H40F | H41F | X10F;
pub const VERS_XHTML11: u32 = XH11;
pub const VERS_BASIC: u32 = XB10;
pub const VERS_HTML5: u32 = HT50 | XH50;
pub const VERS_HTML40: u32 = VERS_HTML40_STRICT | VERS_HTML40_LOOSE | VERS_FRAMESET;
pub const VERS_IFRAME: u32 = VERS_HTML40_LOOSE | VERS_FRAMESET;
pub const VERS_LOOSE: u32 = VERS_HTML20 | VERS_HTML32 | VERS_IFRAME;
pub const VERS_FROM40: u32 = VERS_HTML40 | VERS_XHTML11 | VERS_BASIC | VERS_HTML5;
pub const VERS_XHTML: u32 = X10S | X10T | X10F | XH11 | XB10 | XH50;
pub const VERS_STRICT: u32 = VERS_HTML5 | VERS_HTML40_STRICT;
pub const VERS_ALL: u32 = VERS_HTML20 | VERS_HTML32 | VERS_FROM40 | XH50 | HT50;
pub const VERS_PROPRIETARY: u32 = VERS_NETSCAPE | VERS_MICROSOFT | VERS_SUN;
/// HTML 4 plus XHTML 1.x, the former meaning of `VERS_FROM40` (lexer.c).
pub const VERS_HTML40PX: u32 = VERS_HTML40 | VERS_XHTML11 | VERS_BASIC;

// Content models (lexer.h).
pub const CM_EMPTY: u32 = 1 << 0;
pub const CM_HTML: u32 = 1 << 1;
pub const CM_HEAD: u32 = 1 << 2;
pub const CM_BLOCK: u32 = 1 << 3;
pub const CM_INLINE: u32 = 1 << 4;
pub const CM_LIST: u32 = 1 << 5;
pub const CM_DEFLIST: u32 = 1 << 6;
pub const CM_TABLE: u32 = 1 << 7;
pub const CM_ROWGRP: u32 = 1 << 8;
pub const CM_ROW: u32 = 1 << 9;
pub const CM_FIELD: u32 = 1 << 10;
pub const CM_OBJECT: u32 = 1 << 11;
pub const CM_PARAM: u32 = 1 << 12;
pub const CM_FRAMES: u32 = 1 << 13;
pub const CM_HEADING: u32 = 1 << 14;
pub const CM_OPT: u32 = 1 << 15;
pub const CM_IMG: u32 = 1 << 16;
pub const CM_MIXED: u32 = 1 << 17;
pub const CM_NO_INDENT: u32 = 1 << 18;
pub const CM_NEW: u32 = 1 << 20;

/// tidy's `TidyTagId`: the identity of a built-in tag. The variant names
/// are tidy's, which is what the generated table uses.
#[allow(
    non_camel_case_types,
    clippy::upper_case_acronyms,
    reason = "the variants are tidy's TidyTagId names, matched against the generated tables"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub enum TagId {
    UNKNOWN,
    A,
    ABBR,
    ACRONYM,
    ADDRESS,
    APPLET,
    AREA,
    B,
    BASE,
    BASEFONT,
    BDO,
    BIG,
    BLOCKQUOTE,
    BODY,
    BR,
    BUTTON,
    CAPTION,
    CENTER,
    CITE,
    CODE,
    COL,
    COLGROUP,
    DD,
    DEL,
    DFN,
    DIR,
    DIV,
    DL,
    DT,
    EM,
    FIELDSET,
    FONT,
    FORM,
    FRAME,
    FRAMESET,
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
    HEAD,
    HR,
    HTML,
    I,
    IFRAME,
    IMG,
    INPUT,
    INS,
    ISINDEX,
    KBD,
    LABEL,
    LEGEND,
    LI,
    LINK,
    LISTING,
    MAP,
    MATHML,
    META,
    NOFRAMES,
    NOSCRIPT,
    OBJECT,
    OL,
    OPTGROUP,
    OPTION,
    P,
    PARAM,
    PICTURE,
    PLAINTEXT,
    PRE,
    Q,
    RB,
    RBC,
    RP,
    RT,
    RTC,
    RUBY,
    S,
    SAMP,
    SCRIPT,
    SELECT,
    SMALL,
    SPAN,
    STRIKE,
    STRONG,
    STYLE,
    SUB,
    SUP,
    SVG,
    TABLE,
    TBODY,
    TD,
    TEXTAREA,
    TFOOT,
    TH,
    THEAD,
    TITLE,
    TR,
    TT,
    U,
    UL,
    VAR,
    XMP,
    NEXTID,
    ALIGN,
    BGSOUND,
    BLINK,
    COMMENT,
    ILAYER,
    LAYER,
    MARQUEE,
    MULTICOL,
    NOBR,
    NOEMBED,
    NOLAYER,
    NOSAVE,
    SERVER,
    SERVLET,
    SPACER,
    ARTICLE,
    ASIDE,
    AUDIO,
    BDI,
    CANVAS,
    COMMAND,
    DATALIST,
    DATA,
    DETAILS,
    DIALOG,
    EMBED,
    FIGCAPTION,
    FIGURE,
    FOOTER,
    HEADER,
    HGROUP,
    KEYGEN,
    MAIN,
    MARK,
    MENU,
    MENUITEM,
    METER,
    NAV,
    OUTPUT,
    PROGRESS,
    SECTION,
    SLOT,
    SOURCE,
    SUMMARY,
    TEMPLATE,
    TIME,
    TRACK,
    VIDEO,
    WBR,
}

/// Which parser.c routine parses a tag's content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ParserKind {
    ParseBlock,
    ParseBody,
    ParseColGroup,
    ParseDatalist,
    ParseDefList,
    ParseEmpty,
    ParseFrameSet,
    ParseHTML,
    ParseHead,
    ParseInline,
    ParseList,
    ParseNamespace,
    ParseNoFrames,
    ParseOptGroup,
    ParsePre,
    ParseRow,
    ParseRowGroup,
    ParseScript,
    ParseSelect,
    ParseTableTag,
    ParseText,
    ParseTitle,
}

/// The per-element attribute checks of tags.c.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum TagCheck {
    CheckAREA,
    CheckCaption,
    CheckHTML,
    CheckIMG,
    CheckLINK,
    CheckTABLE,
}

/// The attribute value checks of attrs.c.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum AttrCheck {
    CheckAction,
    CheckAlign,
    CheckBool,
    CheckClear,
    CheckColor,
    CheckFsubmit,
    CheckId,
    CheckIs,
    CheckLang,
    CheckLength,
    CheckLoading,
    CheckName,
    CheckNumber,
    CheckRDFaPrefix,
    CheckRDFaSafeCURIE,
    CheckRDFaTerm,
    CheckScope,
    CheckScript,
    CheckScroll,
    CheckShape,
    CheckSvgAttr,
    CheckTarget,
    CheckTextDir,
    CheckType,
    CheckUrl,
    CheckVType,
    CheckValign,
}

/// tidy's `Dict`: one tag of the dictionary. `attrvers` is the list of
/// (attribute index, versions) the W3C allows on the element.
#[derive(Clone, Debug)]
pub struct TagDef {
    pub id: TagId,
    pub name: String,
    pub versions: u32,
    pub attrvers: Option<Vec<(usize, u32)>>,
    pub model: u32,
    pub parser: Option<ParserKind>,
    pub check: Option<TagCheck>,
}

/// tidy's `Attribute`: one attribute of the dictionary.
#[derive(Clone, Debug)]
pub struct AttrDef {
    pub name: String,
    pub check: Option<AttrCheck>,
}

/// The indices of the attributes the code refers to by name.
#[derive(Clone, Copy, Debug)]
pub struct KnownAttrs {
    pub alt: usize,
    pub src: usize,
    pub usemap: usize,
    pub ismap: usize,
    pub datafld: usize,
    pub href: usize,
    pub nohref: usize,
    pub rel: usize,
    pub itemprop: usize,
    pub summary: usize,
    pub border: usize,
    pub align: usize,
    pub id: usize,
    pub name: usize,
    pub class: usize,
    pub style: usize,
    pub xmlns: usize,
    pub xml_lang: usize,
    pub xml_space: usize,
    pub lang: usize,
    pub type_: usize,
    pub language: usize,
    pub charset: usize,
    pub http_equiv: usize,
    pub content: usize,
    pub width: usize,
    pub cols: usize,
    pub rows: usize,
    pub background: usize,
    pub bgcolor: usize,
    pub text: usize,
    pub link: usize,
    pub vlink: usize,
    pub alink: usize,
    pub color: usize,
    pub fill: usize,
    pub fill_rule: usize,
    pub stroke: usize,
    pub stroke_dasharray: usize,
    pub stroke_dashoffset: usize,
    pub stroke_linecap: usize,
    pub stroke_linejoin: usize,
    pub stroke_miterlimit: usize,
    pub stroke_width: usize,
    pub color_interpolation: usize,
    pub color_rendering: usize,
    pub opacity: usize,
    pub stroke_opacity: usize,
    pub fill_opacity: usize,
}

pub struct Tables {
    pub tags: Vec<TagDef>,
    pub tag_by_id: HashMap<TagId, usize>,
    pub attributes: Vec<AttrDef>,
    /// Lower-cased attribute name to index; the lookup is case-insensitive.
    pub attr_by_name: HashMap<String, usize>,
    /// Entity name to (versions, code point); the lookup is case-sensitive.
    pub entities: HashMap<String, (u32, u32)>,
    pub known: KnownAttrs,
}

#[derive(Deserialize)]
struct JsonAttr {
    id: String,
    name: String,
    check: Option<AttrCheck>,
}

#[derive(Deserialize)]
struct JsonTag {
    id: TagId,
    name: String,
    versions: u32,
    attrvers: Option<String>,
    model: u32,
    parser: Option<ParserKind>,
    check: Option<TagCheck>,
}

#[derive(Deserialize)]
struct JsonTables {
    attributes: Vec<JsonAttr>,
    attrvers: HashMap<String, Vec<(String, u32)>>,
    tags: Vec<JsonTag>,
    entities: Vec<(String, u32, u32)>,
}

const TABLES_JSON: &str = include_str!("tables.json");

fn build() -> Tables {
    let json: JsonTables = serde_json::from_str(TABLES_JSON)
        .expect("src/engines/tidy/tables.json is generated and must parse");
    let attr_by_id: HashMap<&str, usize> = json
        .attributes
        .iter()
        .enumerate()
        .map(|(i, a)| (a.id.as_str(), i))
        .collect();
    let attributes: Vec<AttrDef> = json
        .attributes
        .iter()
        .map(|a| AttrDef {
            name: a.name.clone(),
            check: a.check,
        })
        .collect();
    let attr_by_name: HashMap<String, usize> = attributes
        .iter()
        .enumerate()
        .map(|(i, a)| (a.name.to_ascii_lowercase(), i))
        .collect();
    let tags: Vec<TagDef> = json
        .tags
        .iter()
        .map(|t| TagDef {
            id: t.id,
            name: t.name.clone(),
            versions: t.versions,
            attrvers: t.attrvers.as_ref().map(|key| {
                json.attrvers[key]
                    .iter()
                    .map(|(id, vers)| {
                        (
                            *attr_by_id
                                .get(id.as_str())
                                .unwrap_or_else(|| panic!("tables.json: attribute {id} in attrvers of {key} is not in the attribute table")),
                            *vers,
                        )
                    })
                    .collect()
            }),
            model: t.model,
            parser: t.parser,
            check: t.check,
        })
        .collect();
    let tag_by_id = tags.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
    let entities = json
        .entities
        .into_iter()
        .map(|(name, vers, code)| (name, (vers, code)))
        .collect();
    let attr = |name: &str| -> usize {
        *attr_by_name
            .get(name)
            .unwrap_or_else(|| panic!("tables.json: attribute {name} is missing"))
    };
    let known = KnownAttrs {
        alt: attr("alt"),
        src: attr("src"),
        usemap: attr("usemap"),
        ismap: attr("ismap"),
        datafld: attr("datafld"),
        href: attr("href"),
        nohref: attr("nohref"),
        rel: attr("rel"),
        itemprop: attr("itemprop"),
        summary: attr("summary"),
        border: attr("border"),
        align: attr("align"),
        id: attr("id"),
        name: attr("name"),
        class: attr("class"),
        style: attr("style"),
        xmlns: attr("xmlns"),
        xml_lang: attr("xml:lang"),
        xml_space: attr("xml:space"),
        lang: attr("lang"),
        type_: attr("type"),
        language: attr("language"),
        charset: attr("charset"),
        http_equiv: attr("http-equiv"),
        content: attr("content"),
        width: attr("width"),
        cols: attr("cols"),
        rows: attr("rows"),
        background: attr("background"),
        bgcolor: attr("bgcolor"),
        text: attr("text"),
        link: attr("link"),
        vlink: attr("vlink"),
        alink: attr("alink"),
        color: attr("color"),
        fill: attr("fill"),
        fill_rule: attr("fill-rule"),
        stroke: attr("stroke"),
        stroke_dasharray: attr("stroke-dasharray"),
        stroke_dashoffset: attr("stroke-dashoffset"),
        stroke_linecap: attr("stroke-linecap"),
        stroke_linejoin: attr("stroke-linejoin"),
        stroke_miterlimit: attr("stroke-miterlimit"),
        stroke_width: attr("stroke-width"),
        color_interpolation: attr("color-interpolation"),
        color_rendering: attr("color-rendering"),
        opacity: attr("opacity"),
        stroke_opacity: attr("stroke-opacity"),
        fill_opacity: attr("fill-opacity"),
    };
    Tables {
        tags,
        tag_by_id,
        attributes,
        attr_by_name,
        entities,
        known,
    }
}

/// The tables, built on first use.
pub fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(build)
}

impl Tables {
    /// `attrsLookup`: the dictionary index of an attribute name.
    pub fn attr_index(&self, name: &str) -> Option<usize> {
        self.attr_by_name.get(&name.to_ascii_lowercase()).copied()
    }

    /// `TY_(LookupTagDef)`: the dictionary index of a built-in tag.
    pub fn tag_index(&self, id: TagId) -> usize {
        self.tag_by_id[&id]
    }

    /// `TY_(EntityInfo)` for a named entity (`name` without the `&`).
    pub fn entity(&self, name: &str) -> Option<(u32, u32)> {
        self.entities.get(name).copied()
    }
}

/// The W3C doctypes tidy recognizes (lexer.c `W3C_Doctypes`).
pub struct Doctype {
    pub score: u32,
    pub vers: u32,
    pub name: &'static str,
    pub fpi: Option<&'static str>,
    pub si: Option<&'static str>,
}

pub const W3C_DOCTYPES: &[Doctype] = &[
    Doctype {
        score: 2,
        vers: HT20,
        name: "HTML 2.0",
        fpi: Some("-//IETF//DTD HTML 2.0//EN"),
        si: None,
    },
    Doctype {
        score: 2,
        vers: HT20,
        name: "HTML 2.0",
        fpi: Some("-//IETF//DTD HTML//EN"),
        si: None,
    },
    Doctype {
        score: 2,
        vers: HT20,
        name: "HTML 2.0",
        fpi: Some("-//W3C//DTD HTML 2.0//EN"),
        si: None,
    },
    Doctype {
        score: 1,
        vers: HT32,
        name: "HTML 3.2",
        fpi: Some("-//W3C//DTD HTML 3.2//EN"),
        si: None,
    },
    Doctype {
        score: 1,
        vers: HT32,
        name: "HTML 3.2",
        fpi: Some("-//W3C//DTD HTML 3.2 Final//EN"),
        si: None,
    },
    Doctype {
        score: 1,
        vers: HT32,
        name: "HTML 3.2",
        fpi: Some("-//W3C//DTD HTML 3.2 Draft//EN"),
        si: None,
    },
    Doctype {
        score: 6,
        vers: H40S,
        name: "HTML 4.0 Strict",
        fpi: Some("-//W3C//DTD HTML 4.0//EN"),
        si: Some("http://www.w3.org/TR/REC-html40/strict.dtd"),
    },
    Doctype {
        score: 8,
        vers: H40T,
        name: "HTML 4.0 Transitional",
        fpi: Some("-//W3C//DTD HTML 4.0 Transitional//EN"),
        si: Some("http://www.w3.org/TR/REC-html40/loose.dtd"),
    },
    Doctype {
        score: 7,
        vers: H40F,
        name: "HTML 4.0 Frameset",
        fpi: Some("-//W3C//DTD HTML 4.0 Frameset//EN"),
        si: Some("http://www.w3.org/TR/REC-html40/frameset.dtd"),
    },
    Doctype {
        score: 3,
        vers: H41S,
        name: "HTML 4.01 Strict",
        fpi: Some("-//W3C//DTD HTML 4.01//EN"),
        si: Some("http://www.w3.org/TR/html4/strict.dtd"),
    },
    Doctype {
        score: 5,
        vers: H41T,
        name: "HTML 4.01 Transitional",
        fpi: Some("-//W3C//DTD HTML 4.01 Transitional//EN"),
        si: Some("http://www.w3.org/TR/html4/loose.dtd"),
    },
    Doctype {
        score: 4,
        vers: H41F,
        name: "HTML 4.01 Frameset",
        fpi: Some("-//W3C//DTD HTML 4.01 Frameset//EN"),
        si: Some("http://www.w3.org/TR/html4/frameset.dtd"),
    },
    Doctype {
        score: 9,
        vers: X10S,
        name: "XHTML 1.0 Strict",
        fpi: Some("-//W3C//DTD XHTML 1.0 Strict//EN"),
        si: Some("http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd"),
    },
    Doctype {
        score: 11,
        vers: X10T,
        name: "XHTML 1.0 Transitional",
        fpi: Some("-//W3C//DTD XHTML 1.0 Transitional//EN"),
        si: Some("http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd"),
    },
    Doctype {
        score: 10,
        vers: X10F,
        name: "XHTML 1.0 Frameset",
        fpi: Some("-//W3C//DTD XHTML 1.0 Frameset//EN"),
        si: Some("http://www.w3.org/TR/xhtml1/DTD/xhtml1-frameset.dtd"),
    },
    Doctype {
        score: 12,
        vers: XH11,
        name: "XHTML 1.1",
        fpi: Some("-//W3C//DTD XHTML 1.1//EN"),
        si: Some("http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd"),
    },
    Doctype {
        score: 13,
        vers: XB10,
        name: "XHTML Basic 1.0",
        fpi: Some("-//W3C//DTD XHTML Basic 1.0//EN"),
        si: Some("http://www.w3.org/TR/xhtml-basic/xhtml-basic10.dtd"),
    },
    Doctype {
        score: 20,
        vers: HT50,
        name: "HTML5",
        fpi: None,
        si: None,
    },
    Doctype {
        score: 21,
        vers: XH50,
        name: "XHTML5",
        fpi: None,
        si: None,
    },
];

pub fn fpi_from_vers(vers: u32) -> Option<&'static str> {
    W3C_DOCTYPES
        .iter()
        .find(|d| d.vers == vers)
        .and_then(|d| d.fpi)
}

pub fn si_from_vers(vers: u32) -> Option<&'static str> {
    W3C_DOCTYPES
        .iter()
        .find(|d| d.vers == vers)
        .and_then(|d| d.si)
}

pub fn name_from_vers(vers: u32) -> Option<&'static str> {
    W3C_DOCTYPES.iter().find(|d| d.vers == vers).map(|d| d.name)
}

/// `GetVersFromFPI`: the version named by a public identifier, compared
/// case-insensitively; 0 when unknown.
pub fn vers_from_fpi(fpi: &str) -> u32 {
    W3C_DOCTYPES
        .iter()
        .find(|d| d.fpi.is_some_and(|f| f.eq_ignore_ascii_case(fpi)))
        .map_or(0, |d| d.vers)
}

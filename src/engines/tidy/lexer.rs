//! The lexer (tidy's lexer.c): characters in, tokens out. Tokens are
//! nodes: start and end tags with their attributes, text, comments,
//! doctypes, processing instructions, CDATA and server-side sections. The
//! lexer also owns the text buffer every text node points into, the
//! document's version bits, and the inline stack.

use super::Doc;
use super::istack::IStack;
use super::message::Code;
use super::node::{AttVal, Node, NodeId, NodeType};
use super::stream::END_OF_STREAM;
use super::tables::{self, AttrCheck, CM_EMPTY, TagId, VERS_HTML5, VERS_PROPRIETARY, VERS_XHTML};

/// The lexer's finite state (`LexerState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexState {
    Content,
    Gt,
    EndTag,
    StartTag,
    Comment,
    Doctype,
    ProcInstr,
    Cdata,
    Section,
    Asp,
    Jste,
    Php,
    XmlDecl,
}

/// `GetTokenMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenMode {
    IgnoreWhitespace,
    MixedContent,
    Preformatted,
    IgnoreMarkup,
    OtherNamespace,
    CdataContent,
}

impl TokenMode {
    /// tidy tests `mode & Preformatted` on the enum's values, which holds
    /// for `Preformatted` and `IgnoreMarkup`.
    pub const fn is_preformatted_bit(self) -> bool {
        matches!(self, Self::Preformatted | Self::IgnoreMarkup)
    }
}

pub struct Lexer {
    pub lines: u32,
    pub columns: u32,
    pub waswhite: bool,
    pub pushed: bool,
    pub insertspace: bool,
    pub exclude_blocks: bool,
    pub exiled: bool,
    pub isvoyager: bool,
    pub versions: u32,
    pub doctype: u32,
    pub version_emitted: u32,
    pub txtstart: usize,
    pub txtend: usize,
    pub state: LexState,
    pub token: Option<NodeId>,
    pub itoken: Option<NodeId>,
    pub parent: Option<NodeId>,
    pub seen_end_body: bool,
    pub seen_end_html: bool,
    /// The text of every text node (`lexbuf`). Bytes beyond `lexsize` are
    /// stale but still readable, as in tidy, whose code occasionally looks
    /// at the byte just past a node it emptied.
    pub lexbuf: Vec<u8>,
    lexsize: usize,
    pub inode: Option<NodeId>,
    pub insert: Option<usize>,
    pub istack: Vec<IStack>,
    pub istackbase: usize,
}

impl Lexer {
    pub const fn new() -> Self {
        Self {
            lines: 1,
            columns: 1,
            waswhite: false,
            pushed: false,
            insertspace: false,
            exclude_blocks: false,
            exiled: false,
            isvoyager: false,
            versions: tables::VERS_ALL | VERS_PROPRIETARY,
            doctype: tables::VERS_UNKNOWN,
            version_emitted: 0,
            txtstart: 0,
            txtend: 0,
            state: LexState::Content,
            token: None,
            itoken: None,
            parent: None,
            seen_end_body: false,
            seen_end_html: false,
            lexbuf: Vec::new(),
            lexsize: 0,
            inode: None,
            insert: None,
            istack: Vec::new(),
            istackbase: 0,
        }
    }

    pub const fn lexsize(&self) -> usize {
        self.lexsize
    }

    /// `lexer->lexsize = size`: forget the tail, keeping its bytes.
    pub fn truncate(&mut self, size: usize) {
        debug_assert!(size <= self.lexsize);
        self.lexsize = size;
    }

    /// `AddByte`.
    pub fn push_byte(&mut self, b: u8) {
        if self.lexsize < self.lexbuf.len() {
            self.lexbuf[self.lexsize] = b;
        } else {
            self.lexbuf.push(b);
        }
        self.lexsize += 1;
    }

    /// `lexbuf[lexsize - 1]`.
    pub fn last_byte(&self) -> Option<u8> {
        self.lexsize.checked_sub(1).map(|i| self.lexbuf[i])
    }

    /// The live bytes from `start`.
    pub fn from(&self, start: usize) -> &[u8] {
        &self.lexbuf[start..self.lexsize]
    }
}

impl Default for Lexer {
    fn default() -> Self {
        Self::new()
    }
}

// Character classes of lexer.c's `lexmap`.

pub const fn is_white(c: u32) -> bool {
    matches!(c, 0x0d | 0x0a | 0x0c | 0x20 | 0x09)
}

pub fn is_digit(c: u32) -> bool {
    (0x30..=0x39).contains(&c)
}

pub fn is_digit_hex(c: u32) -> bool {
    is_digit(c) || (0x61..=0x66).contains(&c) || (0x41..=0x46).contains(&c)
}

pub fn is_letter(c: u32) -> bool {
    (0x61..=0x7a).contains(&c) || (0x41..=0x5a).contains(&c)
}

pub fn is_namechar(c: u32) -> bool {
    is_letter(c) || is_digit(c) || matches!(c, 0x2d | 0x2e | 0x3a | 0x5f)
}

pub fn is_upper(c: u32) -> bool {
    (0x41..=0x5a).contains(&c)
}

pub fn to_lower(c: u32) -> u32 {
    if is_upper(c) { c + 32 } else { c }
}

pub fn to_upper(c: u32) -> u32 {
    if (0x61..=0x7a).contains(&c) {
        c - 32
    } else {
        c
    }
}

pub const fn is_html_space(c: u32) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0a | 0x0c | 0x0d)
}

const XML_LETTER_RANGES: &[(u32, u32)] = &[
    (0x41, 0x5a),
    (0x61, 0x7a),
    (0xc0, 0xd6),
    (0xd8, 0xf6),
    (0xf8, 0xff),
    (0x100, 0x131),
    (0x134, 0x13e),
    (0x141, 0x148),
    (0x14a, 0x17e),
    (0x180, 0x1c3),
    (0x1cd, 0x1f0),
    (0x1f4, 0x1f5),
    (0x1fa, 0x217),
    (0x250, 0x2a8),
    (0x2bb, 0x2c1),
    (0x386, 0x386),
    (0x388, 0x38a),
    (0x38c, 0x38c),
    (0x38e, 0x3a1),
    (0x3a3, 0x3ce),
    (0x3d0, 0x3d6),
    (0x3da, 0x3da),
    (0x3dc, 0x3dc),
    (0x3de, 0x3de),
    (0x3e0, 0x3e0),
    (0x3e2, 0x3f3),
    (0x401, 0x40c),
    (0x40e, 0x44f),
    (0x451, 0x45c),
    (0x45e, 0x481),
    (0x490, 0x4c4),
    (0x4c7, 0x4c8),
    (0x4cb, 0x4cc),
    (0x4d0, 0x4eb),
    (0x4ee, 0x4f5),
    (0x4f8, 0x4f9),
    (0x531, 0x556),
    (0x559, 0x559),
    (0x561, 0x586),
    (0x5d0, 0x5ea),
    (0x5f0, 0x5f2),
    (0x621, 0x63a),
    (0x641, 0x64a),
    (0x671, 0x6b7),
    (0x6ba, 0x6be),
    (0x6c0, 0x6ce),
    (0x6d0, 0x6d3),
    (0x6d5, 0x6d5),
    (0x6e5, 0x6e6),
    (0x905, 0x939),
    (0x93d, 0x93d),
    (0x958, 0x961),
    (0x985, 0x98c),
    (0x98f, 0x990),
    (0x993, 0x9a8),
    (0x9aa, 0x9b0),
    (0x9b2, 0x9b2),
    (0x9b6, 0x9b9),
    (0x9dc, 0x9dd),
    (0x9df, 0x9e1),
    (0x9f0, 0x9f1),
    (0xa05, 0xa0a),
    (0xa0f, 0xa10),
    (0xa13, 0xa28),
    (0xa2a, 0xa30),
    (0xa32, 0xa33),
    (0xa35, 0xa36),
    (0xa38, 0xa39),
    (0xa59, 0xa5c),
    (0xa5e, 0xa5e),
    (0xa72, 0xa74),
    (0xa85, 0xa8b),
    (0xa8d, 0xa8d),
    (0xa8f, 0xa91),
    (0xa93, 0xaa8),
    (0xaaa, 0xab0),
    (0xab2, 0xab3),
    (0xab5, 0xab9),
    (0xabd, 0xabd),
    (0xae0, 0xae0),
    (0xb05, 0xb0c),
    (0xb0f, 0xb10),
    (0xb13, 0xb28),
    (0xb2a, 0xb30),
    (0xb32, 0xb33),
    (0xb36, 0xb39),
    (0xb3d, 0xb3d),
    (0xb5c, 0xb5d),
    (0xb5f, 0xb61),
    (0xb85, 0xb8a),
    (0xb8e, 0xb90),
    (0xb92, 0xb95),
    (0xb99, 0xb9a),
    (0xb9c, 0xb9c),
    (0xb9e, 0xb9f),
    (0xba3, 0xba4),
    (0xba8, 0xbaa),
    (0xbae, 0xbb5),
    (0xbb7, 0xbb9),
    (0xc05, 0xc0c),
    (0xc0e, 0xc10),
    (0xc12, 0xc28),
    (0xc2a, 0xc33),
    (0xc35, 0xc39),
    (0xc60, 0xc61),
    (0xc85, 0xc8c),
    (0xc8e, 0xc90),
    (0xc92, 0xca8),
    (0xcaa, 0xcb3),
    (0xcb5, 0xcb9),
    (0xcde, 0xcde),
    (0xce0, 0xce1),
    (0xd05, 0xd0c),
    (0xd0e, 0xd10),
    (0xd12, 0xd28),
    (0xd2a, 0xd39),
    (0xd60, 0xd61),
    (0xe01, 0xe2e),
    (0xe30, 0xe30),
    (0xe32, 0xe33),
    (0xe40, 0xe45),
    (0xe81, 0xe82),
    (0xe84, 0xe84),
    (0xe87, 0xe88),
    (0xe8a, 0xe8a),
    (0xe8d, 0xe8d),
    (0xe94, 0xe97),
    (0xe99, 0xe9f),
    (0xea1, 0xea3),
    (0xea5, 0xea5),
    (0xea7, 0xea7),
    (0xeaa, 0xeab),
    (0xead, 0xeae),
    (0xeb0, 0xeb0),
    (0xeb2, 0xeb3),
    (0xebd, 0xebd),
    (0xec0, 0xec4),
    (0xf40, 0xf47),
    (0xf49, 0xf69),
    (0x10a0, 0x10c5),
    (0x10d0, 0x10f6),
    (0x1100, 0x1100),
    (0x1102, 0x1103),
    (0x1105, 0x1107),
    (0x1109, 0x1109),
    (0x110b, 0x110c),
    (0x110e, 0x1112),
    (0x113c, 0x113c),
    (0x113e, 0x113e),
    (0x1140, 0x1140),
    (0x114c, 0x114c),
    (0x114e, 0x114e),
    (0x1150, 0x1150),
    (0x1154, 0x1155),
    (0x1159, 0x1159),
    (0x115f, 0x1161),
    (0x1163, 0x1163),
    (0x1165, 0x1165),
    (0x1167, 0x1167),
    (0x1169, 0x1169),
    (0x116d, 0x116e),
    (0x1172, 0x1173),
    (0x1175, 0x1175),
    (0x119e, 0x119e),
    (0x11a8, 0x11a8),
    (0x11ab, 0x11ab),
    (0x11ae, 0x11af),
    (0x11b7, 0x11b8),
    (0x11ba, 0x11ba),
    (0x11bc, 0x11c2),
    (0x11eb, 0x11eb),
    (0x11f0, 0x11f0),
    (0x11f9, 0x11f9),
    (0x1e00, 0x1e9b),
    (0x1ea0, 0x1ef9),
    (0x1f00, 0x1f15),
    (0x1f18, 0x1f1d),
    (0x1f20, 0x1f45),
    (0x1f48, 0x1f4d),
    (0x1f50, 0x1f57),
    (0x1f59, 0x1f59),
    (0x1f5b, 0x1f5b),
    (0x1f5d, 0x1f5d),
    (0x1f5f, 0x1f7d),
    (0x1f80, 0x1fb4),
    (0x1fb6, 0x1fbc),
    (0x1fbe, 0x1fbe),
    (0x1fc2, 0x1fc4),
    (0x1fc6, 0x1fcc),
    (0x1fd0, 0x1fd3),
    (0x1fd6, 0x1fdb),
    (0x1fe0, 0x1fec),
    (0x1ff2, 0x1ff4),
    (0x1ff6, 0x1ffc),
    (0x2126, 0x2126),
    (0x212a, 0x212b),
    (0x212e, 0x212e),
    (0x2180, 0x2182),
    (0x3041, 0x3094),
    (0x30a1, 0x30fa),
    (0x3105, 0x312c),
    (0xac00, 0xd7a3),
    (0x4e00, 0x9fa5),
    (0x3007, 0x3007),
    (0x3021, 0x3029),
];

const XML_NAMECHAR_EXTRA_RANGES: &[(u32, u32)] = &[
    (0x300, 0x345),
    (0x360, 0x361),
    (0x483, 0x486),
    (0x591, 0x5a1),
    (0x5a3, 0x5b9),
    (0x5bb, 0x5bd),
    (0x5bf, 0x5bf),
    (0x5c1, 0x5c2),
    (0x5c4, 0x5c4),
    (0x64b, 0x652),
    (0x670, 0x670),
    (0x6d6, 0x6dc),
    (0x6dd, 0x6df),
    (0x6e0, 0x6e4),
    (0x6e7, 0x6e8),
    (0x6ea, 0x6ed),
    (0x901, 0x903),
    (0x93c, 0x93c),
    (0x93e, 0x94c),
    (0x94d, 0x94d),
    (0x951, 0x954),
    (0x962, 0x963),
    (0x981, 0x983),
    (0x9bc, 0x9bc),
    (0x9be, 0x9be),
    (0x9bf, 0x9bf),
    (0x9c0, 0x9c4),
    (0x9c7, 0x9c8),
    (0x9cb, 0x9cd),
    (0x9d7, 0x9d7),
    (0x9e2, 0x9e3),
    (0xa02, 0xa02),
    (0xa3c, 0xa3c),
    (0xa3e, 0xa3e),
    (0xa3f, 0xa3f),
    (0xa40, 0xa42),
    (0xa47, 0xa48),
    (0xa4b, 0xa4d),
    (0xa70, 0xa71),
    (0xa81, 0xa83),
    (0xabc, 0xabc),
    (0xabe, 0xac5),
    (0xac7, 0xac9),
    (0xacb, 0xacd),
    (0xb01, 0xb03),
    (0xb3c, 0xb3c),
    (0xb3e, 0xb43),
    (0xb47, 0xb48),
    (0xb4b, 0xb4d),
    (0xb56, 0xb57),
    (0xb82, 0xb83),
    (0xbbe, 0xbc2),
    (0xbc6, 0xbc8),
    (0xbca, 0xbcd),
    (0xbd7, 0xbd7),
    (0xc01, 0xc03),
    (0xc3e, 0xc44),
    (0xc46, 0xc48),
    (0xc4a, 0xc4d),
    (0xc55, 0xc56),
    (0xc82, 0xc83),
    (0xcbe, 0xcc4),
    (0xcc6, 0xcc8),
    (0xcca, 0xccd),
    (0xcd5, 0xcd6),
    (0xd02, 0xd03),
    (0xd3e, 0xd43),
    (0xd46, 0xd48),
    (0xd4a, 0xd4d),
    (0xd57, 0xd57),
    (0xe31, 0xe31),
    (0xe34, 0xe3a),
    (0xe47, 0xe4e),
    (0xeb1, 0xeb1),
    (0xeb4, 0xeb9),
    (0xebb, 0xebc),
    (0xec8, 0xecd),
    (0xf18, 0xf19),
    (0xf35, 0xf35),
    (0xf37, 0xf37),
    (0xf39, 0xf39),
    (0xf3e, 0xf3e),
    (0xf3f, 0xf3f),
    (0xf71, 0xf84),
    (0xf86, 0xf8b),
    (0xf90, 0xf95),
    (0xf97, 0xf97),
    (0xf99, 0xfad),
    (0xfb1, 0xfb7),
    (0xfb9, 0xfb9),
    (0x20d0, 0x20dc),
    (0x20e1, 0x20e1),
    (0x302a, 0x302f),
    (0x3099, 0x3099),
    (0x309a, 0x309a),
    (0x30, 0x39),
    (0x660, 0x669),
    (0x6f0, 0x6f9),
    (0x966, 0x96f),
    (0x9e6, 0x9ef),
    (0xa66, 0xa6f),
    (0xae6, 0xaef),
    (0xb66, 0xb6f),
    (0xbe7, 0xbef),
    (0xc66, 0xc6f),
    (0xce6, 0xcef),
    (0xd66, 0xd6f),
    (0xe50, 0xe59),
    (0xed0, 0xed9),
    (0xf20, 0xf29),
    (0xb7, 0xb7),
    (0x2d0, 0x2d0),
    (0x2d1, 0x2d1),
    (0x387, 0x387),
    (0x640, 0x640),
    (0xe46, 0xe46),
    (0xec6, 0xec6),
    (0x3005, 0x3005),
    (0x3031, 0x3035),
    (0x309d, 0x309e),
    (0x30fc, 0x30fe),
];

pub fn is_xml_letter(c: u32) -> bool {
    XML_LETTER_RANGES.iter().any(|&(lo, hi)| c >= lo && c <= hi)
}

pub fn is_xml_namechar(c: u32) -> bool {
    is_xml_letter(c)
        || matches!(c, 0x2e | 0x5f | 0x3a | 0x2d)
        || XML_NAMECHAR_EXTRA_RANGES
            .iter()
            .any(|&(lo, hi)| c >= lo && c <= hi)
}

/// Windows-1252 code points 128..159 to Unicode (streamio.c
/// `Win2Unicode`), 0 where undefined.
const WIN2UNICODE: [u32; 32] = [
    0x20AC, 0x0000, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0x0000, 0x017D, 0x0000, 0x0000, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x0000, 0x017E, 0x0178,
];

pub fn decode_win1252(c: u32) -> u32 {
    if (128..160).contains(&c) {
        WIN2UNICODE[(c - 128) as usize]
    } else {
        c
    }
}

fn is_low_surrogate(c: u32) -> bool {
    (0xD800..=0xDBFF).contains(&c)
}

fn is_high_surrogate(c: u32) -> bool {
    (0xDC00..=0xDFFF).contains(&c)
}

const fn combine_surrogate_pair(high: u32, low: u32) -> u32 {
    ((high - 0xDC00) + 0x400 * (low - 0xD800)) + 0x10000
}

fn is_valid_combined_char(c: u32) -> bool {
    (0x10000..=0x0010_FFFF).contains(&c)
}

/// The text of `&name;` lookups: `name` is the entity with its leading
/// `&` (lexer.c passes `lexbuf + start`). Returns (found, code, versions).
fn entity_info(name: &[u8]) -> (bool, u32, u32) {
    if name.len() >= 2 && name[1] == b'#' {
        let (digits, radix) = if name.len() >= 3 && (name[2] == b'x' || name[2] == b'X') {
            (&name[3..], 16)
        } else {
            (&name[2..], 10)
        };
        let mut value: u32 = 0;
        let mut any = false;
        for &b in digits {
            let Some(d) = (b as char).to_digit(radix) else {
                break;
            };
            any = true;
            value = value.wrapping_mul(radix).wrapping_add(d);
        }
        if any {
            return (true, value, tables::VERS_ALL);
        }
        return (false, 0, VERS_PROPRIETARY);
    }
    let text = std::str::from_utf8(&name[1..]).unwrap_or("");
    match tables::tables().entity(text) {
        Some((versions, code)) => (true, code, versions),
        None => (false, 0, VERS_PROPRIETARY),
    }
}

#[derive(PartialEq, Eq)]
enum SurrogateStatus {
    Ok,
    Failed,
    Error,
}

impl Doc {
    // ---- buffer and node helpers ------------------------------------------

    /// `TY_(AddCharToLexer)`: append a code point as UTF-8.
    pub(crate) fn add_char_to_lexer(&mut self, c: u32) {
        match char::from_u32(c) {
            Some(ch) => {
                let mut buf = [0u8; 4];
                for &b in ch.encode_utf8(&mut buf).as_bytes() {
                    self.lexer.push_byte(b);
                }
            }
            None => {
                for b in [0xEF, 0xBF, 0xBD] {
                    self.lexer.push_byte(b);
                }
            }
        }
    }

    pub(crate) fn add_string_to_lexer(&mut self, s: &str) {
        for &b in s.as_bytes() {
            self.lexer.push_byte(b);
        }
    }

    /// `ChangeChar`: overwrite the last live byte.
    fn change_char(&mut self, c: u8) {
        if let Some(i) = self.lexer.lexsize().checked_sub(1) {
            self.lexer.lexbuf[i] = c;
        }
    }

    pub(crate) const fn set_lexer_locus(&mut self) {
        self.lexer.lines = self.stream.curline as u32;
        self.lexer.columns = self.stream.curcol as u32;
    }

    /// `TY_(NewNode)` with the lexer's position.
    pub(crate) fn new_node(&mut self) -> NodeId {
        self.tree
            .alloc(Node::new(self.lexer.lines, self.lexer.columns))
    }

    /// `TY_(CloneNode)`.
    pub(crate) fn clone_node(&mut self, element: Option<NodeId>) -> NodeId {
        let size = self.lexer.lexsize();
        let id = self.new_node();
        self.tree.node_mut(id).start = size;
        self.tree.node_mut(id).end = size;
        if let Some(e) = element {
            let src = self.tree.node(e).clone();
            let attributes = self.dup_attrs(&src.attributes);
            let n = self.tree.node_mut(id);
            n.parent = src.parent;
            n.ntype = src.ntype;
            n.closed = src.closed;
            n.implicit = src.implicit;
            n.tag = src.tag;
            n.element = src.element;
            n.attributes = attributes;
        }
        id
    }

    /// `TY_(TextToken)`.
    pub(crate) fn text_token(&mut self) -> NodeId {
        let id = self.new_node();
        let n = self.tree.node_mut(id);
        n.start = self.lexer.txtstart;
        n.end = self.lexer.txtend;
        id
    }

    /// `TagToken`: a tag node named by the buffer span.
    fn tag_token(&mut self, ntype: NodeType) -> NodeId {
        let name =
            String::from_utf8_lossy(&self.lexer.lexbuf[self.lexer.txtstart..self.lexer.txtend])
                .into_owned();
        let id = self.new_node();
        {
            let n = self.tree.node_mut(id);
            n.ntype = ntype;
            n.element = Some(name);
            n.start = self.lexer.txtstart;
            n.end = self.lexer.txtstart;
        }
        if matches!(
            ntype,
            NodeType::StartTag | NodeType::StartEndTag | NodeType::EndTag
        ) {
            self.find_tag(id);
        }
        id
    }

    fn new_token(&mut self, ntype: NodeType) -> NodeId {
        let id = self.new_node();
        let n = self.tree.node_mut(id);
        n.ntype = ntype;
        n.start = self.lexer.txtstart;
        n.end = self.lexer.txtend;
        id
    }

    /// `TY_(elementIsAutonomousCustomFormat)`: a hyphen not in first place.
    pub(crate) fn element_is_autonomous_custom_format(element: &str) -> bool {
        element.find('-').is_some_and(|p| p > 0)
    }

    /// `TY_(nodeIsAutonomousCustomFormat)`.
    pub(crate) fn node_is_autonomous_custom_format(&self, node: NodeId) -> bool {
        self.tree
            .node(node)
            .element
            .as_deref()
            .is_some_and(Self::element_is_autonomous_custom_format)
    }

    /// `TY_(FindTag)`: resolve a node's tag, declaring an autonomous
    /// custom tag when the option allows.
    pub(crate) fn find_tag(&mut self, node: NodeId) -> bool {
        let element = self.tree.node(node).element.clone();
        if let Some(name) = element.as_deref()
            && let Some(index) = self.lookup_tag(name)
        {
            self.tree.node_mut(node).tag = Some(index);
            return true;
        }
        if self.node_is_autonomous_custom_format(node)
            && self.opts.custom_tags != super::CustomTags::No
        {
            let name = element.unwrap_or_default();
            self.declare_custom_tag(&name);
            self.tree.node_mut(node).tag = self.lookup_tag(&name);
            self.report(Some(node), Some(node), Code::CustomTagDetected);
            return true;
        }
        false
    }

    /// `TY_(InferredTag)`.
    pub(crate) fn inferred_tag(&mut self, id: TagId) -> NodeId {
        let index = self.tag_index(id);
        let name = self.tags[index].name.clone();
        let node = self.new_node();
        let n = self.tree.node_mut(node);
        n.ntype = NodeType::StartTag;
        n.implicit = true;
        n.element = Some(name);
        n.tag = Some(index);
        n.start = self.lexer.txtstart;
        n.end = self.lexer.txtend;
        node
    }

    /// `TY_(ConstrainVersion)`.
    pub(crate) const fn constrain_version(&mut self, vers: u32) {
        self.lexer.versions &= vers | VERS_PROPRIETARY;
    }

    /// `ExpectsContent`: a start tag of a non-empty element.
    fn expects_content(&self, node: NodeId) -> bool {
        let n = self.tree.node(node);
        if n.ntype != NodeType::StartTag {
            return false;
        }
        match n.tag {
            None => true,
            Some(t) => (self.tags[t].model & CM_EMPTY) == 0,
        }
    }

    /// `TY_(IsJavaScript)`.
    pub(crate) fn is_javascript(&self, node: NodeId) -> bool {
        let n = self.tree.node(node);
        if n.attributes.is_empty() {
            return true;
        }
        let known = tables::tables().known;
        n.attributes.iter().any(|a| {
            (a.dict == Some(known.language) || a.dict == Some(known.type_))
                && a.value_contains("javascript")
        })
    }

    pub(crate) fn is_url_attr(&self, name: &str) -> bool {
        let t = tables::tables();
        t.attr_index(name)
            .is_some_and(|i| t.attributes[i].check == Some(AttrCheck::CheckUrl))
    }

    fn is_script_attr(&self, name: &str) -> bool {
        let t = tables::tables();
        t.attr_index(name)
            .is_some_and(|i| t.attributes[i].check == Some(AttrCheck::CheckScript))
    }

    fn lexbuf_from(&self, start: usize) -> Vec<u8> {
        self.lexer.from(start).to_vec()
    }

    fn lexbuf_string(&self, start: usize) -> String {
        String::from_utf8_lossy(self.lexer.from(start)).into_owned()
    }

    // ---- entities -----------------------------------------------------------

    /// `GetSurrogatePair`: after `&` following a leading surrogate entity.
    fn get_surrogate_pair(&mut self, pch: &mut u32) -> SurrogateStatus {
        let mut buf: Vec<u32> = Vec::new();
        let mut status = SurrogateStatus::Error;
        let mut hex = false;
        let fch = *pch;
        let mut c;
        loop {
            c = self.read_char();
            if c == END_OF_STREAM {
                break;
            }
            if c == u32::from(b';') {
                break;
            }
            buf.push(c);
            let offset = buf.len();
            if offset == 1 {
                if c != u32::from(b'#') {
                    break;
                }
            } else if offset == 2 && (c == u32::from(b'x') || c == u32::from(b'X')) {
                hex = true;
            } else if hex {
                if !is_digit_hex(c) {
                    break;
                }
            } else if !is_digit(c) {
                break;
            }
        }
        if c == u32::from(b';') {
            let text: String = buf.iter().filter_map(|&c| char::from_u32(c)).collect();
            let parsed = if hex {
                u32::from_str_radix(text.get(2..).unwrap_or(""), 16).ok()
            } else {
                text.get(1..).unwrap_or("").parse::<u32>().ok()
            };
            if let Some(ch) = parsed
                && is_high_surrogate(ch)
            {
                let combined = combine_surrogate_pair(ch, fch);
                if is_valid_combined_char(combined) {
                    *pch = combined;
                    status = SurrogateStatus::Ok;
                } else {
                    status = SurrogateStatus::Failed;
                    *pch = 0xFFFD;
                    self.report_surrogate(Code::BadSurrogatePair, fch, combined);
                }
            }
        }
        if status == SurrogateStatus::Error {
            if c == u32::from(b';') {
                self.unget_char(c);
            }
            for &c in buf.iter().rev() {
                self.unget_char(c);
            }
        }
        status
    }

    /// `ParseEntity`: the `&` has just been added to the buffer.
    fn parse_entity(&mut self, mode: TokenMode) {
        #[derive(PartialEq, Eq, Clone, Copy)]
        enum EntState {
            Default,
            NumDec,
            NumHex,
        }
        let start = self.lexer.lexsize() - 1;
        let startcol = (self.stream.curcol - 1) as u32;
        let mut state = EntState::Default;
        let mut char_read = 0;
        let mut semicolon = false;
        let mut preserve_entities = self.opts.preserve_entities;
        let mut c;
        loop {
            c = self.read_char();
            if c == END_OF_STREAM {
                break;
            }
            if c == u32::from(b';') {
                semicolon = true;
                break;
            }
            char_read += 1;
            if char_read == 1 && c == u32::from(b'#') {
                if !self.opts.ncr {
                    self.unget_char(u32::from(b'#'));
                    return;
                }
                self.add_char_to_lexer(c);
                state = EntState::NumDec;
                continue;
            } else if char_read == 2
                && state == EntState::NumDec
                && (c == u32::from(b'x') || c == u32::from(b'X'))
            {
                self.add_char_to_lexer(c);
                state = EntState::NumHex;
                continue;
            }
            let accepted = match state {
                EntState::Default => is_namechar(c),
                EntState::NumDec => is_digit(c),
                EntState::NumHex => is_digit_hex(c),
            };
            if accepted {
                self.add_char_to_lexer(c);
                continue;
            }
            self.unget_char(c);
            break;
        }

        if self.lexer.from(start) == b"&apos"
            && !self.opts.output_xml
            && !self.lexer.isvoyager
            && !self.opts.output_xhtml
            && self.html_version() != tables::HT50
        {
            let name = self.lexbuf_string(start);
            self.report_entity(Code::AposUndefined, &name);
        }

        let (found, mut ch, entver) = if mode == TokenMode::OtherNamespace && c == u32::from(b';') {
            preserve_entities = true;
            (true, 255, tables::XH50 | tables::HT50)
        } else {
            let name = self.lexbuf_from(start);
            entity_info(&name)
        };

        if !preserve_entities && found && is_low_surrogate(ch) {
            let c1 = self.read_char();
            if c1 == u32::from(b'&') {
                let first = ch;
                let status = self.get_surrogate_pair(&mut ch);
                if status == SurrogateStatus::Error {
                    self.report_surrogate(Code::BadSurrogateTail, first, 0);
                    self.unget_char(u32::from(b'&'));
                }
            } else {
                self.unget_char(c1);
                self.report_surrogate(Code::BadSurrogateTail, ch, 0);
                ch = 0xFFFD;
            }
        } else if !preserve_entities && found && is_high_surrogate(ch) {
            self.report_surrogate(Code::BadSurrogateLead, ch, 0);
            ch = 0xFFFD;
        }

        if !found || (128..=159).contains(&ch) || (ch >= 256 && c != u32::from(b';')) {
            self.set_lexer_locus();
            self.lexer.columns = startcol;
            if self.lexer.lexsize() > start + 1 {
                if (128..=159).contains(&ch) {
                    let c1 = decode_win1252(ch);
                    if c != u32::from(b';') {
                        let name = self.lexbuf_string(start);
                        self.report_entity(Code::MissingSemicolonNcr, &name);
                    }
                    self.report_encoding(Code::InvalidNcr, ch, c1 == 0);
                    self.lexer.truncate(start);
                    if c1 != 0 {
                        self.add_char_to_lexer(c1);
                    }
                    semicolon = false;
                } else {
                    let name = self.lexbuf_string(start);
                    self.report_entity(Code::UnknownEntity, &name);
                }
                if semicolon {
                    self.add_char_to_lexer(u32::from(b';'));
                }
            } else if self.html_version() != tables::HT50 {
                let name = self.lexbuf_string(start);
                self.report_entity(Code::UnescapedAmpersand, &name);
            }
        } else {
            if c != u32::from(b';') {
                self.set_lexer_locus();
                self.lexer.columns = startcol;
                let name = self.lexbuf_string(start);
                self.report_entity(Code::MissingSemicolon, &name);
            }
            if preserve_entities {
                self.add_char_to_lexer(u32::from(b';'));
            } else {
                self.lexer.truncate(start);
                if ch == 160 && mode == TokenMode::Preformatted {
                    ch = u32::from(b' ');
                }
                self.add_char_to_lexer(ch);
                if ch == u32::from(b'&') && !self.opts.quote_ampersand {
                    self.add_string_to_lexer("amp;");
                }
            }
            self.constrain_version(entver);
        }
    }

    /// `ParseTagName`: the first letter is in the buffer; read the rest,
    /// folding case. Returns the character that ended the name.
    fn parse_tag_name(&mut self) -> u32 {
        let first = self.lexer.lexbuf[self.lexer.txtstart];
        if is_upper(u32::from(first)) {
            self.lexer.lexbuf[self.lexer.txtstart] = to_lower(u32::from(first)) as u8;
        }
        let mut c;
        loop {
            c = self.read_char();
            if c == END_OF_STREAM {
                break;
            }
            if !is_namechar(c) {
                break;
            }
            let c = to_lower(c);
            self.add_char_to_lexer(c);
        }
        self.lexer.txtend = self.lexer.lexsize();
        c
    }

    // ---- CDATA content (script, style) ------------------------------------

    /// `GetCDATA`: the text of a script or style element, up to its end
    /// tag.
    fn get_cdata(&mut self, container: NodeId) -> Option<NodeId> {
        #[derive(PartialEq, Eq, Clone, Copy)]
        enum State {
            Intermediate,
            StartTag,
            EndTag,
        }
        let known = tables::tables().known;
        let has_src = self.tree.attr_by_id(container, known.src).is_some();
        let nonested = (self.node_is(container, TagId::SCRIPT)
            || self.node_is(container, TagId::STYLE))
            && self.opts.skip_nested;
        let container_name = self
            .tree
            .node(container)
            .element
            .clone()
            .unwrap_or_default();
        self.set_lexer_locus();
        self.lexer.waswhite = false;
        let size = self.lexer.lexsize();
        self.lexer.txtstart = size;
        self.lexer.txtend = size;
        let mut start = 0usize;
        let mut nested: i32 = 0;
        let mut state = State::Intermediate;
        let mut is_empty = true;
        let mut c;
        loop {
            c = self.read_char();
            if c == END_OF_STREAM {
                break;
            }
            self.add_char_to_lexer(c);
            self.lexer.txtend = self.lexer.lexsize();
            match state {
                State::Intermediate => {
                    if c != u32::from(b'<') {
                        if is_empty && !is_white(c) {
                            is_empty = false;
                        }
                        continue;
                    }
                    let c2 = self.read_char();
                    if is_letter(c2) {
                        if has_src && is_empty && self.node_is(container, TagId::SCRIPT) {
                            self.lexer.truncate(self.lexer.txtstart);
                            self.unget_char(c2);
                            self.unget_char(u32::from(b'<'));
                            return None;
                        }
                        self.add_char_to_lexer(c2);
                        start = self.lexer.lexsize() - 1;
                        state = State::StartTag;
                    } else if c2 == u32::from(b'/') {
                        self.add_char_to_lexer(c2);
                        let c3 = self.read_char();
                        if !is_letter(c3) {
                            self.unget_char(c3);
                            continue;
                        }
                        self.unget_char(c3);
                        start = self.lexer.lexsize();
                        state = State::EndTag;
                    } else if c2 == u32::from(b'\\') {
                        self.add_char_to_lexer(c2);
                        let c3 = self.read_char();
                        if c3 != u32::from(b'/') {
                            self.unget_char(c3);
                            continue;
                        }
                        self.add_char_to_lexer(c3);
                        if nonested {
                            continue;
                        }
                        let c4 = self.read_char();
                        if !is_letter(c4) {
                            self.unget_char(c4);
                            continue;
                        }
                        self.unget_char(c4);
                        start = self.lexer.lexsize();
                        state = State::EndTag;
                    } else {
                        self.unget_char(c2);
                    }
                }
                State::StartTag => {
                    if is_letter(c) {
                        continue;
                    }
                    let matches = self.cdata_name_matches(&container_name, start);
                    if matches && !nonested {
                        nested += 1;
                    }
                    state = State::Intermediate;
                }
                State::EndTag => {
                    if is_letter(c) {
                        continue;
                    }
                    let matches = self.cdata_name_matches(&container_name, start);
                    if is_empty && !matches {
                        let mut i = self.lexer.lexsize() - 1;
                        loop {
                            self.unget_char(u32::from(self.lexer.lexbuf[i]));
                            if i == start {
                                break;
                            }
                            i -= 1;
                        }
                        self.unget_char(u32::from(b'/'));
                        self.unget_char(u32::from(b'<'));
                        break;
                    }
                    let was_nested = nested;
                    if matches {
                        nested -= 1;
                    }
                    if matches && was_nested <= 0 {
                        let mut i = self.lexer.lexsize() - 1;
                        loop {
                            self.unget_char(u32::from(self.lexer.lexbuf[i]));
                            if i == start {
                                break;
                            }
                            i -= 1;
                        }
                        self.unget_char(u32::from(b'/'));
                        self.unget_char(u32::from(b'<'));
                        let new_size = self.lexer.lexsize() - ((self.lexer.lexsize() - start) + 2);
                        self.lexer.truncate(new_size);
                        break;
                    } else if start < 2 || self.lexer.lexbuf[start - 2] != b'\\' {
                        self.set_lexer_locus();
                        self.lexer.columns = self.lexer.columns.saturating_sub(3);
                        if self.is_javascript(container)
                            && self.opts.escape_scripts
                            && !self.html5_mode
                        {
                            self.report(None, None, Code::BadCdataContent);
                            // Shift the end tag right by one and put the
                            // backslash before it.
                            let size = self.lexer.lexsize();
                            self.lexer.push_byte(0);
                            for i in (start..=size).rev() {
                                let prev = self.lexer.lexbuf[i - 1];
                                self.lexer.lexbuf[i] = prev;
                            }
                            self.lexer.lexbuf[start - 1] = b'\\';
                        }
                    }
                    state = State::Intermediate;
                }
            }
        }
        if is_empty {
            let s = self.lexer.txtstart;
            self.lexer.truncate(s);
            self.lexer.txtend = s;
        } else {
            self.lexer.txtend = self.lexer.lexsize();
        }
        if c == END_OF_STREAM {
            self.report(Some(container), None, Code::MissingEndtagFor);
        }
        Some(self.text_token())
    }

    /// `tmbstrncasecmp(container->element, lexbuf + start, strlen(element)) == 0`.
    fn cdata_name_matches(&self, name: &str, start: usize) -> bool {
        let candidate = self.lexer.from(start);
        candidate.len() >= name.len()
            && candidate[..name.len()].eq_ignore_ascii_case(name.as_bytes())
    }

    // ---- tokens --------------------------------------------------------------

    /// `TY_(UngetToken)`.
    pub(crate) const fn unget_token(&mut self) {
        self.lexer.pushed = true;
    }

    /// `TY_(GetToken)`.
    pub(crate) fn get_token(&mut self, mode: TokenMode) -> Option<NodeId> {
        if self.lexer.pushed || self.lexer.itoken.is_some() {
            if self.lexer.itoken.is_some() {
                if self.lexer.pushed {
                    self.lexer.pushed = false;
                    return self.lexer.itoken;
                }
                self.lexer.itoken = None;
            }
            self.lexer.pushed = false;
            let token = self.lexer.token;
            let is_text = token.is_some_and(|t| self.tree.node(t).ntype == NodeType::Text);
            if !is_text || !(self.lexer.insert.is_some() || self.lexer.inode.is_some()) {
                return token;
            }
            self.lexer.itoken = self.inserted_token();
            return self.lexer.itoken;
        }

        if (self.lexer.insert.is_some() || self.lexer.inode.is_some())
            && !self.lexer.istack.is_empty()
        {
            self.lexer.token = self.inserted_token();
            return self.lexer.token;
        }

        if mode == TokenMode::CdataContent {
            let parent = self
                .lexer
                .parent
                .expect("CdataContent needs the lexer's parent");
            return self.get_cdata(parent);
        }

        self.get_token_from_stream(mode)
    }

    /// `CondReturnTextNode`: return the text read so far as a token.
    fn pending_text(&mut self) -> Option<NodeId> {
        if self.lexer.txtend > self.lexer.txtstart {
            let t = self.text_token();
            self.lexer.token = Some(t);
            return Some(t);
        }
        None
    }

    fn fix_comments(&self) -> bool {
        match self.opts.fix_bad_comments {
            super::TriState::Yes => true,
            super::TriState::No => false,
            super::TriState::Auto => (self.html_version() & tables::HT50) == 0,
        }
    }

    /// `GetTokenFromStream`.
    #[allow(
        clippy::too_many_lines,
        reason = "a one-to-one port of tidy's tokenizer state machine"
    )]
    fn get_token_from_stream(&mut self, mut mode: TokenMode) -> Option<NodeId> {
        let fix_comments = self.fix_comments();
        let mut badcomment = 0u32;
        let mut isempty: bool;
        let mut attributes: Vec<AttVal> = Vec::new();

        self.lexer.token = None;
        self.set_lexer_locus();
        self.lexer.waswhite = false;
        let size = self.lexer.lexsize();
        self.lexer.txtstart = size;
        self.lexer.txtend = size;

        let mut c;
        'main: loop {
            c = self.read_char();
            if c == END_OF_STREAM {
                break;
            }
            if self.lexer.insertspace {
                self.add_char_to_lexer(u32::from(b' '));
                self.lexer.waswhite = true;
                self.lexer.insertspace = false;
            }
            if c == 160 && mode == TokenMode::Preformatted {
                c = u32::from(b' ');
            }
            self.add_char_to_lexer(c);

            match self.lexer.state {
                LexState::Content => {
                    if is_white(c)
                        && mode == TokenMode::IgnoreWhitespace
                        && self.lexer.lexsize() == self.lexer.txtstart + 1
                    {
                        let s = self.lexer.lexsize() - 1;
                        self.lexer.truncate(s);
                        self.lexer.waswhite = false;
                        self.set_lexer_locus();
                        continue;
                    }
                    if c == u32::from(b'<') {
                        self.lexer.state = LexState::Gt;
                        continue;
                    }
                    if is_white(c) {
                        if self.lexer.waswhite {
                            if mode != TokenMode::Preformatted && mode != TokenMode::IgnoreMarkup {
                                let s = self.lexer.lexsize() - 1;
                                self.lexer.truncate(s);
                                self.set_lexer_locus();
                            }
                        } else {
                            self.lexer.waswhite = true;
                            if mode != TokenMode::Preformatted
                                && mode != TokenMode::IgnoreMarkup
                                && c != u32::from(b' ')
                            {
                                self.change_char(b' ');
                            }
                        }
                        continue;
                    } else if c == u32::from(b'&') && mode != TokenMode::IgnoreMarkup {
                        self.parse_entity(mode);
                    }
                    if mode == TokenMode::IgnoreWhitespace {
                        mode = TokenMode::MixedContent;
                    }
                    self.lexer.waswhite = false;
                }
                LexState::Gt => {
                    if c == u32::from(b'/') {
                        c = self.read_char();
                        if c == END_OF_STREAM {
                            self.unget_char(c);
                            continue;
                        }
                        self.add_char_to_lexer(c);
                        if is_letter(c) {
                            let s = self.lexer.lexsize() - 3;
                            self.lexer.truncate(s);
                            self.lexer.txtend = s;
                            self.unget_char(c);
                            self.lexer.state = LexState::EndTag;
                            self.stream.curcol -= 2;
                            if self.lexer.txtend > self.lexer.txtstart {
                                if mode == TokenMode::IgnoreWhitespace
                                    && self.lexer.last_byte() == Some(b' ')
                                {
                                    let s = self.lexer.lexsize() - 1;
                                    self.lexer.truncate(s);
                                    self.lexer.txtend = s;
                                }
                                let t = self.text_token();
                                self.lexer.token = Some(t);
                                return Some(t);
                            }
                            continue;
                        }
                        self.lexer.waswhite = false;
                        self.lexer.state = LexState::Content;
                        continue;
                    }
                    if mode == TokenMode::IgnoreMarkup {
                        self.lexer.waswhite = false;
                        self.lexer.state = LexState::Content;
                        continue;
                    }
                    if c == u32::from(b'!') {
                        c = self.read_char();
                        if c == u32::from(b'-') {
                            c = self.read_char();
                            if c == u32::from(b'-') {
                                self.lexer.state = LexState::Comment;
                                let s = self.lexer.lexsize() - 2;
                                self.lexer.truncate(s);
                                self.lexer.txtend = s;
                                if let Some(t) = self.pending_text() {
                                    return Some(t);
                                }
                                self.lexer.txtstart = self.lexer.lexsize();
                                continue;
                            }
                        } else if c == u32::from(b'd') || c == u32::from(b'D') {
                            self.lexer.state = LexState::Doctype;
                            let s = self.lexer.lexsize() - 2;
                            self.lexer.truncate(s);
                            self.lexer.txtend = s;
                            mode = TokenMode::IgnoreWhitespace;
                            // skip until white space or '>'
                            loop {
                                c = self.read_char();
                                if c == END_OF_STREAM || c == u32::from(b'>') {
                                    self.unget_char(c);
                                    break;
                                }
                                if !is_white(c) {
                                    continue;
                                }
                                loop {
                                    c = self.read_char();
                                    if c == END_OF_STREAM || c == u32::from(b'>') {
                                        self.unget_char(c);
                                        break;
                                    }
                                    if is_white(c) {
                                        continue;
                                    }
                                    self.unget_char(c);
                                    break;
                                }
                                break;
                            }
                            if let Some(t) = self.pending_text() {
                                return Some(t);
                            }
                            self.lexer.txtstart = self.lexer.lexsize();
                            continue;
                        } else if c == u32::from(b'[') {
                            let s = self.lexer.lexsize() - 2;
                            self.lexer.truncate(s);
                            self.lexer.state = LexState::Section;
                            self.lexer.txtend = s;
                            if let Some(t) = self.pending_text() {
                                return Some(t);
                            }
                            self.lexer.txtstart = self.lexer.lexsize();
                            continue;
                        }
                        self.report(None, None, Code::MalformedCommentDropping);
                        loop {
                            c = self.read_char();
                            if c == u32::from(b'>') {
                                break;
                            }
                            if c == END_OF_STREAM {
                                self.unget_char(c);
                                break;
                            }
                        }
                        let s = self.lexer.lexsize() - 2;
                        self.lexer.truncate(s);
                        self.lexer.state = LexState::Content;
                        continue;
                    }
                    if c == u32::from(b'?') {
                        let s = self.lexer.lexsize() - 2;
                        self.lexer.truncate(s);
                        self.lexer.state = LexState::ProcInstr;
                        self.lexer.txtend = s;
                        if let Some(t) = self.pending_text() {
                            return Some(t);
                        }
                        self.lexer.txtstart = self.lexer.lexsize();
                        continue;
                    }
                    if c == u32::from(b'%') {
                        let s = self.lexer.lexsize() - 2;
                        self.lexer.truncate(s);
                        self.lexer.state = LexState::Asp;
                        self.lexer.txtend = s;
                        if let Some(t) = self.pending_text() {
                            return Some(t);
                        }
                        self.lexer.txtstart = self.lexer.lexsize();
                        continue;
                    }
                    if c == u32::from(b'#') {
                        let s = self.lexer.lexsize() - 2;
                        self.lexer.truncate(s);
                        self.lexer.state = LexState::Jste;
                        self.lexer.txtend = s;
                        if let Some(t) = self.pending_text() {
                            return Some(t);
                        }
                        self.lexer.txtstart = self.lexer.lexsize();
                        continue;
                    }
                    if is_letter(c) {
                        self.unget_char(c);
                        self.unget_char(u32::from(b'<'));
                        let s = self.lexer.lexsize() - 2;
                        self.lexer.truncate(s);
                        self.lexer.txtend = s;
                        self.lexer.state = LexState::StartTag;
                        if let Some(t) = self.pending_text() {
                            return Some(t);
                        }
                        continue;
                    }
                    self.unget_char(c);
                    let s = self.lexer.lexsize() - 1;
                    self.lexer.truncate(s);
                    self.lexer.state = LexState::Content;
                    self.lexer.waswhite = false;
                }
                LexState::EndTag => {
                    self.lexer.txtstart = self.lexer.lexsize() - 1;
                    self.stream.curcol += 2;
                    c = self.parse_tag_name();
                    let token = self.tag_token(NodeType::EndTag);
                    self.lexer.token = Some(token);
                    let s = self.lexer.txtstart;
                    self.lexer.truncate(s);
                    self.lexer.txtend = s;
                    while c != u32::from(b'>') && c != END_OF_STREAM {
                        c = self.read_char();
                    }
                    if c == END_OF_STREAM {
                        continue;
                    }
                    self.lexer.state = LexState::Content;
                    self.lexer.waswhite = false;
                    return Some(token);
                }
                LexState::StartTag => {
                    c = self.read_char();
                    self.change_char(c as u8);
                    self.lexer.txtstart = self.lexer.lexsize() - 1;
                    c = self.parse_tag_name();
                    isempty = false;
                    attributes.clear();
                    let token = self.tag_token(NodeType::StartTag);
                    self.lexer.token = Some(token);
                    if c != u32::from(b'>') {
                        if c == u32::from(b'/') {
                            self.unget_char(c);
                        }
                        attributes = self.parse_attrs(&mut isempty);
                    }
                    if isempty {
                        self.tree.node_mut(token).ntype = NodeType::StartEndTag;
                    }
                    self.tree.node_mut(token).attributes = std::mem::take(&mut attributes);
                    let s = self.lexer.txtstart;
                    self.lexer.truncate(s);
                    self.lexer.txtend = s;

                    if self.node_is(token, TagId::PRE) {
                        mode = TokenMode::Preformatted;
                    }
                    if (mode != TokenMode::Preformatted && self.expects_content(token))
                        || self.node_is(token, TagId::BR)
                        || self.node_is(token, TagId::HR)
                    {
                        // A newline right after the tag is swallowed in
                        // IgnoreWhitespace mode (and a form feed always);
                        // anything else is put back.
                        c = self.read_char();
                        let swallow = if c == u32::from(b'\n') {
                            mode == TokenMode::IgnoreWhitespace
                        } else {
                            c == 0x0c
                        };
                        if !swallow {
                            self.unget_char(c);
                        }
                        self.lexer.waswhite = true;
                    } else {
                        self.lexer.waswhite = false;
                    }

                    self.lexer.state = LexState::Content;
                    if self.tree.node(token).tag.is_none() {
                        if mode != TokenMode::OtherNamespace {
                            let looks_custom = (self.lexer.doctype & VERS_HTML5) != 0
                                && self
                                    .tree
                                    .node(token)
                                    .element
                                    .as_deref()
                                    .is_some_and(Self::element_is_autonomous_custom_format);
                            if looks_custom {
                                self.report(None, Some(token), Code::UnknownElementLooksCustom);
                            } else {
                                self.report(None, Some(token), Code::UnknownElement);
                            }
                        }
                    } else {
                        let versions = self.tags[self.tree.node(token).tag.unwrap()].versions;
                        self.constrain_version(versions);
                        self.repair_duplicate_attributes(token);
                    }
                    return Some(token);
                }
                LexState::Comment => {
                    if c != u32::from(b'-') {
                        continue;
                    }
                    c = self.read_char();
                    if c != u32::from(b'-')
                        && fix_comments
                        && self.lexer.lexsize() - self.lexer.txtstart == 1
                    {
                        self.change_char(b'=');
                    }
                    self.add_char_to_lexer(c);
                    if c != u32::from(b'-') {
                        continue;
                    }
                    loop {
                        // end_comment:
                        c = self.read_char();
                        if c == u32::from(b'>') {
                            if badcomment > 0 {
                                if (self.html_version() & tables::HT50) != 0 {
                                    if fix_comments {
                                        self.report(None, None, Code::MalformedComment);
                                    }
                                } else if fix_comments {
                                    self.report(None, None, Code::MalformedComment);
                                } else {
                                    self.report(None, None, Code::MalformedCommentWarn);
                                }
                            }
                            let s = self.lexer.lexsize() - 2;
                            self.lexer.truncate(s);
                            self.lexer.txtend = s;
                            self.lexer.state = LexState::Content;
                            self.lexer.waswhite = false;
                            let token = self.new_token(NodeType::Comment);
                            self.lexer.token = Some(token);
                            c = self.read_char();
                            if c == u32::from(b'\n') {
                                self.tree.node_mut(token).linebreak = true;
                            } else {
                                self.unget_char(c);
                            }
                            return Some(token);
                        }
                        if badcomment == 0 {
                            self.set_lexer_locus();
                            self.lexer.columns = self.lexer.columns.saturating_sub(3);
                        }
                        badcomment += 1;
                        if fix_comments {
                            let len = self.lexer.lexsize();
                            self.lexer.lexbuf[len - 2] = b'=';
                        }
                        if c == u32::from(b'-') {
                            self.add_char_to_lexer(c);
                            continue;
                        }
                        if fix_comments {
                            self.change_char(b'=');
                        }
                        self.add_char_to_lexer(c);
                        continue 'main;
                    }
                }
                LexState::Doctype => {
                    self.unget_char(c);
                    let s = self.lexer.lexsize() - 1;
                    self.lexer.truncate(s);
                    let token = self.parse_doctype_decl();
                    self.lexer.token = token;
                    self.lexer.txtend = self.lexer.lexsize();
                    self.lexer.state = LexState::Content;
                    self.lexer.waswhite = false;
                    if self.lexer.doctype == tables::VERS_UNKNOWN
                        && let Some(t) = token
                    {
                        self.lexer.doctype = self.find_given_version(t);
                        if self.lexer.doctype != VERS_HTML5 {
                            self.adjust_tags();
                        }
                    }
                    return token;
                }
                LexState::ProcInstr => {
                    if self.lexer.lexsize() - self.lexer.txtstart == 3
                        && self.lexer.from(self.lexer.txtstart) == b"php"
                    {
                        self.lexer.state = LexState::Php;
                        continue;
                    }
                    if self.lexer.lexsize() - self.lexer.txtstart == 4
                        && &self.lexer.lexbuf[self.lexer.txtstart..self.lexer.txtstart + 3]
                            == b"xml"
                        && is_white(u32::from(self.lexer.lexbuf[self.lexer.txtstart + 3]))
                    {
                        self.lexer.state = LexState::XmlDecl;
                        attributes.clear();
                        continue;
                    }
                    if self.opts.assume_xml_procins || self.lexer.isvoyager {
                        if c != u32::from(b'?') {
                            continue;
                        }
                        c = self.read_char();
                        if c == END_OF_STREAM {
                            self.report(None, None, Code::UnexpectedEndOfFile);
                            self.unget_char(c);
                            continue;
                        }
                        self.add_char_to_lexer(c);
                    }
                    if c != u32::from(b'>') {
                        continue;
                    }
                    let s = self.lexer.lexsize() - 1;
                    self.lexer.truncate(s);
                    let token;
                    if self.lexer.lexsize() > 0 {
                        let txtstart = self.lexer.txtstart;
                        let mut i = 0;
                        while i < self.lexer.lexsize() - txtstart
                            && !is_white(u32::from(self.lexer.lexbuf[i + txtstart]))
                        {
                            i += 1;
                        }
                        let closed = self.lexer.last_byte() == Some(b'?');
                        if closed {
                            let s = self.lexer.lexsize() - 1;
                            self.lexer.truncate(s);
                        }
                        self.lexer.txtstart += i;
                        self.lexer.txtend = self.lexer.lexsize();
                        token = self.new_token(NodeType::ProcIns);
                        let name = String::from_utf8_lossy(
                            &self.lexer.lexbuf[self.lexer.txtstart - i..self.lexer.txtstart],
                        )
                        .into_owned();
                        let n = self.tree.node_mut(token);
                        n.closed = closed;
                        n.element = if i > 0 { Some(name) } else { None };
                    } else {
                        self.lexer.txtend = self.lexer.lexsize();
                        token = self.new_token(NodeType::ProcIns);
                    }
                    self.lexer.token = Some(token);
                    self.lexer.state = LexState::Content;
                    self.lexer.waswhite = false;
                    return Some(token);
                }
                LexState::Asp => {
                    if c != u32::from(b'%') {
                        continue;
                    }
                    c = self.read_char();
                    if c != u32::from(b'>') {
                        self.unget_char(c);
                        continue;
                    }
                    let s = self.lexer.lexsize() - 1;
                    self.lexer.truncate(s);
                    self.lexer.txtend = s;
                    self.lexer.state = LexState::Content;
                    self.lexer.waswhite = false;
                    let token = self.new_token(NodeType::Asp);
                    self.lexer.token = Some(token);
                    return Some(token);
                }
                LexState::Jste => {
                    if c != u32::from(b'#') {
                        continue;
                    }
                    c = self.read_char();
                    if c != u32::from(b'>') {
                        self.unget_char(c);
                        continue;
                    }
                    let s = self.lexer.lexsize() - 1;
                    self.lexer.truncate(s);
                    self.lexer.txtend = s;
                    self.lexer.state = LexState::Content;
                    self.lexer.waswhite = false;
                    let token = self.new_token(NodeType::Jste);
                    self.lexer.token = Some(token);
                    return Some(token);
                }
                LexState::Php => {
                    if c != u32::from(b'?') {
                        continue;
                    }
                    c = self.read_char();
                    if c != u32::from(b'>') {
                        self.unget_char(c);
                        continue;
                    }
                    let s = self.lexer.lexsize() - 1;
                    self.lexer.truncate(s);
                    self.lexer.txtend = s;
                    self.lexer.state = LexState::Content;
                    self.lexer.waswhite = false;
                    let token = self.new_token(NodeType::Php);
                    self.lexer.token = Some(token);
                    return Some(token);
                }
                LexState::XmlDecl => {
                    if is_white(c) && c != u32::from(b'?') {
                        continue;
                    }
                    if c != u32::from(b'?') {
                        isempty = false;
                        self.unget_char(c);
                        let (name, asp, php) = self.parse_attribute(&mut isempty);
                        let Some(name) = name else {
                            if let Some(asp) = asp {
                                let mut av = AttVal::new();
                                av.asp = Some(asp);
                                attributes.push(av);
                            }
                            if let Some(php) = php {
                                let mut av = AttVal::new();
                                av.php = Some(php);
                                attributes.push(av);
                            }
                            let s = self.lexer.lexsize() - 1;
                            self.lexer.truncate(s);
                            self.lexer.txtend = self.lexer.txtstart;
                            self.lexer.state = LexState::Content;
                            self.lexer.waswhite = false;
                            let token = self.new_token(NodeType::XmlDecl);
                            self.tree.node_mut(token).attributes = std::mem::take(&mut attributes);
                            self.lexer.token = Some(token);
                            return Some(token);
                        };
                        let mut pdelim = 0;
                        let value = self.parse_value(&name, true, &mut isempty, &mut pdelim);
                        let mut av = AttVal::new();
                        av.dict = tables::tables().attr_index(&name);
                        av.attribute = Some(name);
                        av.value = value;
                        av.delim = pdelim;
                        attributes.push(av);
                    }
                    c = self.read_char();
                    if c != u32::from(b'>') {
                        self.unget_char(c);
                        continue;
                    }
                    let s = self.lexer.lexsize() - 1;
                    self.lexer.truncate(s);
                    self.lexer.txtend = self.lexer.txtstart;
                    self.lexer.state = LexState::Content;
                    self.lexer.waswhite = false;
                    let token = self.new_token(NodeType::XmlDecl);
                    self.tree.node_mut(token).attributes = std::mem::take(&mut attributes);
                    self.lexer.token = Some(token);
                    return Some(token);
                }
                LexState::Section => {
                    if c == u32::from(b'[')
                        && self.lexer.lexsize() == self.lexer.txtstart + 6
                        && self.lexer.from(self.lexer.txtstart) == b"CDATA["
                    {
                        self.lexer.state = LexState::Cdata;
                        let s = self.lexer.lexsize() - 6;
                        self.lexer.truncate(s);
                        continue;
                    }
                    if c == u32::from(b'>') {
                        self.unget_char(c);
                    } else if c != u32::from(b']') {
                        continue;
                    }
                    c = self.read_char();
                    let lexdump = 1;
                    if c != u32::from(b'>') {
                        if c == u32::from(b'-') {
                            c = self.read_char();
                            if c == u32::from(b'-') {
                                c = self.read_char();
                                if c != u32::from(b'>') {
                                    self.unget_char(c);
                                    self.unget_char(u32::from(b'-'));
                                    self.unget_char(u32::from(b'-'));
                                    continue;
                                }
                            } else {
                                self.unget_char(c);
                                self.unget_char(u32::from(b'-'));
                                continue;
                            }
                        } else {
                            self.unget_char(c);
                            continue;
                        }
                    }
                    let s = self.lexer.lexsize() - lexdump;
                    self.lexer.truncate(s);
                    self.lexer.txtend = s;
                    self.lexer.state = LexState::Content;
                    self.lexer.waswhite = false;
                    let token = self.new_token(NodeType::Section);
                    self.lexer.token = Some(token);
                    return Some(token);
                }
                LexState::Cdata => {
                    if c != u32::from(b']') {
                        continue;
                    }
                    c = self.read_char();
                    if c != u32::from(b']') {
                        self.unget_char(c);
                        continue;
                    }
                    c = self.read_char();
                    if c != u32::from(b'>') {
                        self.unget_char(c);
                        self.unget_char(u32::from(b']'));
                        continue;
                    }
                    let s = self.lexer.lexsize() - 1;
                    self.lexer.truncate(s);
                    self.lexer.txtend = s;
                    self.lexer.state = LexState::Content;
                    self.lexer.waswhite = false;
                    let token = self.new_token(NodeType::Cdata);
                    self.lexer.token = Some(token);
                    return Some(token);
                }
            }
        }

        if self.lexer.state == LexState::Content {
            self.lexer.txtend = self.lexer.lexsize();
            if self.lexer.txtend > self.lexer.txtstart {
                self.unget_char(c);
                if self.lexer.last_byte() == Some(b' ') {
                    let s = self.lexer.lexsize() - 1;
                    self.lexer.truncate(s);
                    self.lexer.txtend = s;
                }
                let t = self.text_token();
                self.lexer.token = Some(t);
                return Some(t);
            }
        } else if self.lexer.state == LexState::Comment {
            if c == END_OF_STREAM {
                self.report(None, None, Code::MalformedCommentEos);
            }
            self.lexer.txtend = self.lexer.lexsize();
            self.lexer.state = LexState::Content;
            self.lexer.waswhite = false;
            let token = self.new_token(NodeType::Comment);
            self.lexer.token = Some(token);
            return Some(token);
        }
        None
    }

    // ---- attributes ---------------------------------------------------------

    /// `ParseAsp`: the `<%` has been read.
    fn parse_asp(&mut self) -> Option<NodeId> {
        self.lexer.txtstart = self.lexer.lexsize();
        loop {
            let c = self.read_char();
            if c == END_OF_STREAM {
                break;
            }
            self.add_char_to_lexer(c);
            if c != u32::from(b'%') {
                continue;
            }
            let c = self.read_char();
            if c == END_OF_STREAM {
                break;
            }
            self.add_char_to_lexer(c);
            if c == u32::from(b'>') {
                let s = self.lexer.lexsize() - 2;
                self.lexer.truncate(s);
                break;
            }
        }
        self.lexer.txtend = self.lexer.lexsize();
        let asp = if self.lexer.txtend > self.lexer.txtstart {
            Some(self.new_token(NodeType::Asp))
        } else {
            None
        };
        self.lexer.txtstart = self.lexer.txtend;
        asp
    }

    /// `ParsePhp`: the `<?` has been read.
    fn parse_php(&mut self) -> Option<NodeId> {
        self.lexer.txtstart = self.lexer.lexsize();
        loop {
            let c = self.read_char();
            if c == END_OF_STREAM {
                break;
            }
            self.add_char_to_lexer(c);
            if c != u32::from(b'?') {
                continue;
            }
            let c = self.read_char();
            if c == END_OF_STREAM {
                break;
            }
            self.add_char_to_lexer(c);
            if c == u32::from(b'>') {
                let s = self.lexer.lexsize() - 2;
                self.lexer.truncate(s);
                break;
            }
        }
        self.lexer.txtend = self.lexer.lexsize();
        let php = if self.lexer.txtend > self.lexer.txtstart {
            Some(self.new_token(NodeType::Php))
        } else {
            None
        };
        self.lexer.txtstart = self.lexer.txtend;
        php
    }

    /// `ParseAttribute`: the next attribute name, or none at the end of
    /// the tag (with any server-side section found instead).
    fn parse_attribute(
        &mut self,
        isempty: &mut bool,
    ) -> (Option<String>, Option<NodeId>, Option<NodeId>) {
        let token = self.lexer.token;
        let mut c;
        loop {
            c = self.read_char();
            if c == u32::from(b'/') {
                c = self.read_char();
                if c == u32::from(b'>') {
                    *isempty = true;
                    return (None, None, None);
                }
                self.unget_char(c);
                c = u32::from(b'/');
                break;
            }
            if c == u32::from(b'>') {
                return (None, None, None);
            }
            if c == u32::from(b'<') {
                c = self.read_char();
                if c == u32::from(b'%') {
                    let asp = self.parse_asp();
                    return (None, asp, None);
                } else if c == u32::from(b'?') {
                    let php = self.parse_php();
                    return (None, None, php);
                }
                self.unget_char(c);
                self.unget_char(u32::from(b'<'));
                self.report_attr(token, None, Code::UnexpectedGt);
                return (None, None, None);
            }
            if c == u32::from(b'=') {
                self.report_attr(token, None, Code::UnexpectedEqualsign);
                continue;
            }
            if c == u32::from(b'"') || c == u32::from(b'\'') {
                self.report_attr(token, None, Code::UnexpectedQuotemark);
                continue;
            }
            if c == END_OF_STREAM {
                self.report_attr(token, None, Code::UnexpectedEndOfFileAttr);
                self.unget_char(c);
                return (None, None, None);
            }
            if !is_white(c) {
                break;
            }
        }

        let start = self.lexer.lexsize();
        let mut lastc = c;
        loop {
            if c == u32::from(b'=') || c == u32::from(b'>') {
                self.unget_char(c);
                break;
            }
            if c == u32::from(b'<') || c == END_OF_STREAM {
                self.unget_char(c);
                break;
            }
            if lastc == u32::from(b'-') && (c == u32::from(b'"') || c == u32::from(b'\'')) {
                let s = self.lexer.lexsize() - 1;
                self.lexer.truncate(s);
                self.unget_char(c);
                break;
            }
            if is_white(c) {
                break;
            }
            if c == u32::from(b'/') {
                let c2 = self.read_char();
                if c2 == u32::from(b'>') {
                    self.unget_char(c2);
                    break;
                }
                self.unget_char(c2);
                c = u32::from(b'/');
            }
            if self.opts.uppercase_attributes != super::UppercaseAttrs::Preserve && is_upper(c) {
                c = to_lower(c);
            }
            self.add_char_to_lexer(c);
            lastc = c;
            c = self.read_char();
        }
        let len = self.lexer.lexsize() - start;
        let attr = if len > 0 {
            Some(String::from_utf8_lossy(self.lexer.from(start)).into_owned())
        } else {
            None
        };
        self.lexer.truncate(start);
        (attr, None, None)
    }

    /// `ParseServerInstruction`: a `<` where an attribute value should be.
    fn parse_server_instruction(&mut self) -> u32 {
        let token = self.lexer.token;
        let mut delim = u32::from(b'"');
        let mut c = self.read_char();
        self.add_char_to_lexer(c);
        let isrule = c == u32::from(b'%') || c == u32::from(b'?') || c == u32::from(b'@');
        loop {
            c = self.read_char();
            if c == END_OF_STREAM {
                break;
            }
            if c == u32::from(b'>') {
                if isrule {
                    self.add_char_to_lexer(c);
                } else {
                    self.unget_char(c);
                }
                break;
            }
            if !isrule && is_white(c) {
                break;
            }
            self.add_char_to_lexer(c);
            if c == u32::from(b'"') {
                loop {
                    c = self.read_char();
                    if c == END_OF_STREAM {
                        self.report_attr(token, None, Code::UnexpectedEndOfFileAttr);
                        self.unget_char(c);
                        return 0;
                    }
                    if c == u32::from(b'>') {
                        self.unget_char(c);
                        self.report_attr(token, None, Code::UnexpectedGt);
                        return 0;
                    }
                    self.add_char_to_lexer(c);
                    if c == u32::from(b'"') {
                        break;
                    }
                }
                delim = u32::from(b'\'');
                continue;
            }
            if c == u32::from(b'\'') {
                loop {
                    c = self.read_char();
                    if c == END_OF_STREAM {
                        self.report_attr(token, None, Code::UnexpectedEndOfFileAttr);
                        self.unget_char(c);
                        return 0;
                    }
                    if c == u32::from(b'>') {
                        self.unget_char(c);
                        self.report_attr(token, None, Code::UnexpectedGt);
                        return 0;
                    }
                    self.add_char_to_lexer(c);
                    if c == u32::from(b'\'') {
                        break;
                    }
                }
            }
        }
        delim
    }

    /// `ParseValue`: the value after an attribute name, if any.
    fn parse_value(
        &mut self,
        name: &str,
        fold_case: bool,
        isempty: &mut bool,
        pdelim: &mut u32,
    ) -> Option<String> {
        let token = self.lexer.token;
        let munge = !self.opts.literal_attributes;
        *pdelim = u32::from(b'"');
        let mut c;
        loop {
            c = self.read_char();
            if c == END_OF_STREAM {
                self.unget_char(c);
                break;
            }
            if !is_white(c) {
                break;
            }
        }
        if c != u32::from(b'=') && c != u32::from(b'"') && c != u32::from(b'\'') {
            self.unget_char(c);
            return None;
        }
        loop {
            c = self.read_char();
            if c == END_OF_STREAM {
                self.unget_char(c);
                break;
            }
            if !is_white(c) {
                break;
            }
        }
        let mut delim = 0u32;
        if c == u32::from(b'"') || c == u32::from(b'\'') {
            delim = c;
        } else if c == u32::from(b'<') {
            let start = self.lexer.lexsize();
            self.add_char_to_lexer(c);
            *pdelim = self.parse_server_instruction();
            let len = self.lexer.lexsize() - start;
            let value = if len > 0 {
                Some(String::from_utf8_lossy(self.lexer.from(start)).into_owned())
            } else {
                None
            };
            self.lexer.truncate(start);
            return value;
        } else {
            self.unget_char(c);
        }

        let mut quotewarning = 0u32;
        let mut seen_gt = false;
        let mut start = self.lexer.lexsize();
        c = 0;
        loop {
            let lastc = c;
            c = self.read_char();
            if c == END_OF_STREAM {
                self.report_attr(token, None, Code::UnexpectedEndOfFileAttr);
                self.unget_char(c);
                break;
            }
            if delim == 0 {
                if c == u32::from(b'>') {
                    self.unget_char(c);
                    break;
                }
                if c == u32::from(b'"') || c == u32::from(b'\'') {
                    let q = c;
                    c = self.read_char();
                    if c == u32::from(b'>') {
                        self.add_char_to_lexer(q);
                        self.unget_char(c);
                        break;
                    }
                    self.unget_char(c);
                    c = q;
                }
                if c == u32::from(b'<') {
                    self.unget_char(c);
                    c = u32::from(b'>');
                    self.unget_char(c);
                    self.report_attr(token, None, Code::UnexpectedGt);
                    break;
                }
                if c == u32::from(b'/') {
                    c = self.read_char();
                    if c == u32::from(b'>') && !self.is_url_attr(name) {
                        *isempty = true;
                        self.unget_char(c);
                        break;
                    }
                    self.unget_char(c);
                    c = u32::from(b'/');
                }
            } else {
                if c == delim {
                    break;
                }
                if c == u32::from(b'\n') || c == u32::from(b'<') || c == u32::from(b'>') {
                    quotewarning += 1;
                }
                if c == u32::from(b'>') {
                    seen_gt = true;
                }
            }
            if c == u32::from(b'&') {
                self.add_char_to_lexer(c);
                self.parse_entity(TokenMode::IgnoreWhitespace);
                if self.lexer.last_byte() == Some(b'\n') && munge {
                    self.change_char(b' ');
                }
                continue;
            }
            if c == u32::from(b'\\') {
                c = self.read_char();
                if c != u32::from(b'\n') {
                    self.unget_char(c);
                    c = u32::from(b'\\');
                }
            }
            if is_white(c) {
                if delim == 0 {
                    break;
                }
                if munge {
                    if c == u32::from(b'\n') && self.is_url_attr(name) {
                        self.report_attr(token, None, Code::NewlineInUri);
                        continue;
                    }
                    c = u32::from(b' ');
                    if lastc == u32::from(b' ') {
                        if self.is_url_attr(name) {
                            self.report_attr(token, None, Code::WhiteInUri);
                        }
                        continue;
                    }
                }
            } else if fold_case && is_upper(c) {
                c = to_lower(c);
            }
            self.add_char_to_lexer(c);
        }

        if quotewarning > 10 && seen_gt && munge {
            let value = self.lexer.from(start);
            let exempt = self.is_script_attr(name)
                || (self.is_url_attr(name) && value.starts_with(b"javascript:"))
                || value.starts_with(b"<xml ");
            if !exempt {
                self.report(None, None, Code::SuspectedMissingQuote);
            }
        }

        let orig_start = start;
        let mut len = self.lexer.lexsize() - start;
        // tidy's tmbstrndup returns NULL for an empty value, so `alt=""`
        // and `alt` alike carry no value.
        let value = if len > 0 {
            if munge
                && !name.eq_ignore_ascii_case("alt")
                && !name.eq_ignore_ascii_case("title")
                && !name.eq_ignore_ascii_case("value")
                && !name.eq_ignore_ascii_case("prompt")
            {
                while len > 0 && is_white(u32::from(self.lexer.lexbuf[start + len - 1])) {
                    len -= 1;
                }
                while len > 0 && is_white(u32::from(self.lexer.lexbuf[start])) {
                    start += 1;
                    len -= 1;
                }
            }
            if len > 0 {
                Some(String::from_utf8_lossy(&self.lexer.lexbuf[start..start + len]).into_owned())
            } else {
                None
            }
        } else {
            None
        };
        self.lexer.truncate(orig_start);
        *pdelim = delim;
        value
    }

    /// `IsValidAttrName`.
    fn is_valid_attr_name(attr: &str) -> bool {
        let mut chars = attr.chars();
        match chars.next() {
            Some(c) if is_letter(c as u32) => {}
            _ => return false,
        }
        chars.all(|c| is_namechar(c as u32))
    }

    /// `ParseAttrs`: all attributes of a start tag, consuming the `>`.
    fn parse_attrs(&mut self, isempty: &mut bool) -> Vec<AttVal> {
        let token = self.lexer.token;
        let mut list: Vec<AttVal> = Vec::new();
        while !self.stream.end_of_input() {
            let (attribute, asp, php) = self.parse_attribute(isempty);
            let Some(attribute) = attribute else {
                if let Some(asp) = asp {
                    let mut av = AttVal::new();
                    av.asp = Some(asp);
                    list.push(av);
                    continue;
                }
                if let Some(php) = php {
                    let mut av = AttVal::new();
                    av.php = Some(php);
                    list.push(av);
                    continue;
                }
                break;
            };
            let mut delim = 0;
            let value = self.parse_value(&attribute, false, isempty, &mut delim);
            let mut av = AttVal::new();
            av.value = value;
            if Self::is_valid_attr_name(&attribute) {
                av.delim = if delim != 0 { delim } else { u32::from(b'"') };
                av.dict = tables::tables().attr_index(&attribute);
                av.attribute = Some(attribute);
                let report_open = delim == 0 && av.value.is_some();
                if report_open {
                    self.report_attr(token, Some(&av), Code::MissingQuotemarkOpen);
                }
                list.push(av);
            } else {
                av.attribute = Some(attribute.clone());
                if attribute.ends_with('"') {
                    self.report_attr(token, Some(&av), Code::MissingQuotemark);
                } else if av.value.is_none() {
                    self.report_attr(token, Some(&av), Code::MissingAttrValue);
                } else {
                    self.report_attr(token, Some(&av), Code::InvalidAttribute);
                }
            }
        }
        list
    }

    // ---- doctype -------------------------------------------------------------

    /// `ParseDocTypeDecl`.
    fn parse_doctype_decl(&mut self) -> Option<NodeId> {
        #[derive(PartialEq, Eq, Clone, Copy)]
        enum State {
            Intermediate,
            DoctypeName,
            PublicSystem,
            QuotedString,
            IntSubset,
        }
        let mut start = self.lexer.lexsize();
        let mut state = State::DoctypeName;
        let mut delim = 0u32;
        let mut hasfpi = true;
        let node = self.new_node();
        {
            let n = self.tree.node_mut(node);
            n.ntype = NodeType::DocType;
            n.start = self.lexer.txtstart;
            n.end = self.lexer.txtend;
        }
        self.lexer.waswhite = false;
        loop {
            let mut c = self.read_char();
            if c == END_OF_STREAM {
                break;
            }
            if state != State::IntSubset && c == u32::from(b'\n') {
                c = u32::from(b' ');
            }
            if is_white(c) && state != State::IntSubset {
                if self.lexer.waswhite {
                    continue;
                }
                self.add_char_to_lexer(c);
                self.lexer.waswhite = true;
            } else {
                self.add_char_to_lexer(c);
                self.lexer.waswhite = false;
            }
            match state {
                State::Intermediate => {
                    if to_upper(c) == u32::from(b'P') || to_upper(c) == u32::from(b'S') {
                        start = self.lexer.lexsize() - 1;
                        state = State::PublicSystem;
                    } else if c == u32::from(b'[') {
                        start = self.lexer.lexsize();
                        state = State::IntSubset;
                    } else if c == u32::from(b'\'') || c == u32::from(b'"') {
                        start = self.lexer.lexsize();
                        delim = c;
                        state = State::QuotedString;
                    } else if c == u32::from(b'>') {
                        let s = self.lexer.lexsize() - 1;
                        self.lexer.truncate(s);
                        self.tree.node_mut(node).end = s;
                        if let Some(si) = self.tree.attr_by_name(node, "SYSTEM") {
                            self.check_url(node, si);
                        }
                        let valid = self
                            .tree
                            .node(node)
                            .element
                            .as_deref()
                            .is_some_and(Self::is_valid_xml_id);
                        if !valid {
                            self.report(None, None, Code::MalformedDoctype);
                            return None;
                        }
                        return Some(node);
                    }
                }
                State::DoctypeName => {
                    if is_white(c) || c == u32::from(b'>') || c == u32::from(b'[') {
                        let name = String::from_utf8_lossy(
                            &self.lexer.lexbuf[start..self.lexer.lexsize() - 1],
                        )
                        .into_owned();
                        self.tree.node_mut(node).element =
                            if name.is_empty() { None } else { Some(name) };
                        if c == u32::from(b'>') || c == u32::from(b'[') {
                            let s = self.lexer.lexsize() - 1;
                            self.lexer.truncate(s);
                            self.unget_char(c);
                        }
                        state = State::Intermediate;
                    }
                }
                State::PublicSystem => {
                    if is_white(c) || c == u32::from(b'>') {
                        let attname = &self.lexer.lexbuf[start..self.lexer.lexsize() - 1];
                        hasfpi = !attname.eq_ignore_ascii_case(b"SYSTEM");
                        if c == u32::from(b'>') {
                            let s = self.lexer.lexsize() - 1;
                            self.lexer.truncate(s);
                            self.unget_char(c);
                        }
                        state = State::Intermediate;
                    }
                }
                State::QuotedString => {
                    if c == delim {
                        let value = String::from_utf8_lossy(
                            &self.lexer.lexbuf[start..self.lexer.lexsize() - 1],
                        )
                        .into_owned();
                        let name = if hasfpi { "PUBLIC" } else { "SYSTEM" };
                        let value = if value.is_empty() { None } else { Some(value) };
                        let index = self.add_attribute(node, name, value.as_deref());
                        self.tree.node_mut(node).attributes[index].delim = delim;
                        hasfpi = false;
                        state = State::Intermediate;
                        delim = 0;
                    }
                }
                State::IntSubset => {
                    if c == u32::from(b']') {
                        self.lexer.txtstart = start;
                        self.lexer.txtend = self.lexer.lexsize() - 1;
                        let subset = self.text_token();
                        self.tree.insert_node_at_end(node, subset);
                        state = State::Intermediate;
                    }
                }
            }
        }
        self.report(None, None, Code::MalformedDoctype);
        None
    }

    /// `FindGivenVersion`: the version a doctype names.
    fn find_given_version(&mut self, doctype: NodeId) -> u32 {
        let fpi = self
            .tree
            .attr_by_name(doctype, "PUBLIC")
            .and_then(|i| self.tree.node(doctype).attributes[i].value.clone());
        let Some(fpi) = fpi else {
            let element = self.tree.node(doctype).element.as_deref();
            if element.is_some_and(|e| e.eq_ignore_ascii_case("html")) {
                return VERS_HTML5;
            }
            return tables::VERS_UNKNOWN;
        };
        let vers = tables::vers_from_fpi(&fpi);
        if (VERS_XHTML & vers) != 0 {
            self.opts.output_xml = true;
            self.opts.output_xhtml = true;
            self.lexer.isvoyager = true;
        }
        let repaired = tables::fpi_from_vers(vers).map(str::to_string);
        if let Some(i) = self.tree.attr_by_name(doctype, "PUBLIC") {
            self.tree.node_mut(doctype).attributes[i].value = repaired;
        }
        vers
    }

    /// `TY_(IsValidXMLID)`.
    pub(crate) fn is_valid_xml_id(id: &str) -> bool {
        let mut chars = id.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        let c = first as u32;
        if !(is_xml_letter(c) || c == u32::from(b'_') || c == u32::from(b':')) {
            return false;
        }
        chars.all(|ch| is_xml_namechar(ch as u32))
    }
}

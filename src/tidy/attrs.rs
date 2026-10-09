//! Attribute handling (tidy's attrs.c and the element checks of tags.c):
//! which attributes a tag allows in which HTML version, the value checks,
//! the anchor namespace shared by `id` and `name`, duplicate attributes,
//! and the per-element checks (`<img>` needs `alt` and `src`, ...).

use std::fmt::Write;

use super::Doc;
use super::lexer::{is_digit, is_letter, is_upper, is_xml_namechar};
use super::message::Code;
use super::node::{AttVal, NodeId};
use super::tables::{
    self, AttrCheck, CM_IMG, CM_ROW, CM_TABLE, HT50, TagCheck, TagId, VERS_ALL, VERS_HTML20,
    VERS_HTML32, VERS_HTML40_LOOSE, VERS_PROPRIETARY, VERS_UNKNOWN, XH50,
};
use super::{
    Anchor, BA_MISSING_IMAGE_ALT, BA_MISSING_IMAGE_MAP, BA_MISSING_LINK_ALT, BA_MISSING_SUMMARY,
};

const COLORS: &[(&str, &str)] = &[
    ("black", "#000000"),
    ("green", "#008000"),
    ("silver", "#C0C0C0"),
    ("lime", "#00FF00"),
    ("gray", "#808080"),
    ("olive", "#808000"),
    ("white", "#FFFFFF"),
    ("yellow", "#FFFF00"),
    ("maroon", "#800000"),
    ("navy", "#000080"),
    ("red", "#FF0000"),
    ("blue", "#0000FF"),
    ("purple", "#800080"),
    ("teal", "#008080"),
    ("fuchsia", "#FF00FF"),
    ("aqua", "#00FFFF"),
];

const EXTENDED_COLORS: &[&str] = &[
    "aliceblue",
    "antiquewhite",
    "aquamarine",
    "azure",
    "beige",
    "bisque",
    "blanchedalmond",
    "blueviolet",
    "brown",
    "burlywood",
    "cadetblue",
    "chartreuse",
    "chocolate",
    "coral",
    "cornflowerblue",
    "cornsilk",
    "crimson",
    "cyan",
    "darkblue",
    "darkcyan",
    "darkgoldenrod",
    "darkgray",
    "darkgreen",
    "darkgrey",
    "darkkhaki",
    "darkmagenta",
    "darkolivegreen",
    "darkorange",
    "darkorchid",
    "darkred",
    "darksalmon",
    "darkseagreen",
    "darkslateblue",
    "darkslategray",
    "darkslategrey",
    "darkturquoise",
    "darkviolet",
    "deeppink",
    "deepskyblue",
    "dimgray",
    "dimgrey",
    "dodgerblue",
    "firebrick",
    "floralwhite",
    "forestgreen",
    "gainsboro",
    "ghostwhite",
    "gold",
    "goldenrod",
    "greenyellow",
    "grey",
    "honeydew",
    "hotpink",
    "indianred",
    "indigo",
    "ivory",
    "khaki",
    "lavender",
    "lavenderblush",
    "lawngreen",
    "lemonchiffon",
    "lightblue",
    "lightcoral",
    "lightcyan",
    "lightgoldenrodyellow",
    "lightgray",
    "lightgreen",
    "lightgrey",
    "lightpink",
    "lightsalmon",
    "lightseagreen",
    "lightskyblue",
    "lightslategray",
    "lightslategrey",
    "lightsteelblue",
    "lightyellow",
    "limegreen",
    "linen",
    "magenta",
    "mediumaquamarine",
    "mediumblue",
    "mediumorchid",
    "mediumpurple",
    "mediumseagreen",
    "mediumslateblue",
    "mediumspringgreen",
    "mediumturquoise",
    "mediumvioletred",
    "midnightblue",
    "mintcream",
    "mistyrose",
    "moccasin",
    "navajowhite",
    "oldlace",
    "olivedrab",
    "orange",
    "orangered",
    "orchid",
    "palegoldenrod",
    "palegreen",
    "paleturquoise",
    "palevioletred",
    "papayawhip",
    "peachpuff",
    "peru",
    "pink",
    "plum",
    "powderblue",
    "rebeccapurple",
    "rosybrown",
    "royalblue",
    "saddlebrown",
    "salmon",
    "sandybrown",
    "seagreen",
    "seashell",
    "sienna",
    "skyblue",
    "slateblue",
    "slategray",
    "slategrey",
    "snow",
    "springgreen",
    "steelblue",
    "tan",
    "thistle",
    "tomato",
    "turquoise",
    "violet",
    "wheat",
    "whitesmoke",
    "yellowgreen",
];

/// `GetColorCode`: is the name a known color?
fn is_color_name(name: &str, use_css_colors: bool) -> bool {
    COLORS.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
        || (use_css_colors && EXTENDED_COLORS.iter().any(|n| n.eq_ignore_ascii_case(name)))
}

/// `IsValidColorCode`: six hex digits.
fn is_valid_color_code(color: &str) -> bool {
    color.len() == 6 && color.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `TY_(IsValidHTMLID)`: no HTML whitespace.
fn is_valid_html_id(id: &str) -> bool {
    !id.chars().any(|c| super::lexer::is_html_space(c as u32))
}

/// `IsValidNMTOKEN`.
fn is_valid_nmtoken(name: &str) -> bool {
    name.chars().all(|c| is_xml_namechar(c as u32))
}

/// `IsURLCodePoint`.
fn is_url_code_point(c: char) -> bool {
    let c = c as u32;
    is_letter(c)
        || is_digit(c)
        || matches!(
            c,
            0x25 | 0x23
                | 0x21
                | 0x24
                | 0x26
                | 0x27
                | 0x28
                | 0x29
                | 0x2a
                | 0x2b
                | 0x2c
                | 0x2d
                | 0x2e
                | 0x2f
                | 0x3a
                | 0x3b
                | 0x3d
                | 0x3f
                | 0x40
                | 0x5f
                | 0x7e
        )
        || (0x00A0..=0xD7FF).contains(&c)
        || (0xE000..=0xFDCF).contains(&c)
        || (0xFDF0..=0xFFEF).contains(&c)
        || ((0x10000..=0x0010_FFFD).contains(&c) && (c & 0xFFFF) <= 0xFFFD)
}

impl Doc {
    // ---- attribute list maintenance -------------------------------------------

    /// `TY_(AddAttribute)`: append an attribute; returns its index.
    pub(crate) fn add_attribute(&mut self, node: NodeId, name: &str, value: Option<&str>) -> usize {
        let mut av = AttVal::new();
        av.delim = u32::from(b'"');
        av.attribute = Some(name.to_string());
        av.value = value.map(str::to_string);
        av.dict = tables::tables().attr_index(name);
        let attrs = &mut self.tree.node_mut(node).attributes;
        attrs.push(av);
        attrs.len() - 1
    }

    /// `TY_(RepairAttrValue)`: set the value of an attribute, adding it
    /// when missing.
    pub(crate) fn repair_attr_value(
        &mut self,
        node: NodeId,
        name: &str,
        value: Option<&str>,
    ) -> usize {
        if let Some(i) = self.tree.attr_by_name(node, name) {
            self.tree.node_mut(node).attributes[i].value = value.map(str::to_string);
            return i;
        }
        self.add_attribute(node, name, value)
    }

    /// `TY_(RemoveAttribute)` by index, dropping its anchor if any.
    pub(crate) fn remove_attribute(&mut self, node: NodeId, index: usize) {
        let av = self.tree.node(node).attributes[index].clone();
        let known = tables::tables().known;
        if (av.dict == Some(known.id) || av.dict == Some(known.name))
            && self.is_anchor_element(node)
            && let Some(value) = av.value.as_deref()
        {
            self.remove_anchor_by_node(value, node);
        }
        self.tree.node_mut(node).attributes.remove(index);
    }

    /// `TY_(FreeAttrs)`: drop every attribute (and its anchors).
    pub(crate) fn free_attrs(&mut self, node: NodeId) {
        while !self.tree.node(node).attributes.is_empty() {
            self.remove_attribute(node, 0);
        }
    }

    /// `AttributeVersions`: the versions allowing the attribute on this
    /// element.
    fn attribute_versions(&self, node: NodeId, av: &AttVal) -> u32 {
        if let Some(name) = av.attribute.as_deref()
            && name.starts_with("data-")
        {
            return XH50 | HT50;
        }
        let Some(dict) = av.dict else {
            return VERS_UNKNOWN;
        };
        if let Some(tag) = self.tree.node(node).tag
            && let Some(vers) = self.tags[tag].attrvers.as_ref()
        {
            for &(attr, versions) in vers {
                if attr == dict {
                    return versions;
                }
            }
        }
        VERS_PROPRIETARY
    }

    /// `TY_(NodeAttributeVersions)`.
    pub(crate) fn node_attribute_versions(&self, node: NodeId, attr: usize) -> u32 {
        let Some(tag) = self.tree.node(node).tag else {
            return VERS_UNKNOWN;
        };
        let Some(vers) = self.tags[tag].attrvers.as_ref() else {
            return VERS_UNKNOWN;
        };
        vers.iter()
            .find(|(a, _)| *a == attr)
            .map_or(VERS_UNKNOWN, |(_, v)| *v)
    }

    /// `TY_(AttributeIsProprietary)`.
    pub(crate) fn attribute_is_proprietary(&self, node: NodeId, av: &AttVal) -> bool {
        let Some(tag) = self.tree.node(node).tag else {
            return false;
        };
        if (self.tags[tag].versions & VERS_ALL) == 0 {
            return false;
        }
        (self.attribute_versions(node, av) & VERS_ALL) == 0
    }

    /// `TY_(AttributeIsMismatched)`.
    pub(crate) fn attribute_is_mismatched(&self, node: NodeId, av: &AttVal) -> bool {
        let Some(tag) = self.tree.node(node).tag else {
            return false;
        };
        if (self.tags[tag].versions & VERS_ALL) == 0 {
            return false;
        }
        let doctype = if self.lexer.version_emitted == 0 {
            self.lexer.doctype
        } else {
            self.lexer.version_emitted
        };
        (self.attribute_versions(node, av) & doctype) == 0
    }

    // ---- anchors --------------------------------------------------------------

    /// `TY_(IsAnchorElement)`.
    pub(crate) fn is_anchor_element(&self, node: NodeId) -> bool {
        matches!(
            self.tag_id(node),
            TagId::A
                | TagId::APPLET
                | TagId::FORM
                | TagId::FRAME
                | TagId::IFRAME
                | TagId::IMG
                | TagId::MAP
        )
    }

    /// The form a name is stored in (`NewAnchor` lower-cases it unless in
    /// HTML5 mode).
    fn anchor_key(&self, name: &str) -> String {
        if self.html5_mode {
            name.to_string()
        } else {
            name.to_ascii_lowercase()
        }
    }

    /// `TY_(RemoveAnchorByNode)`.
    pub(crate) fn remove_anchor_by_node(&mut self, _name: &str, node: NodeId) {
        if let Some(pos) = self.anchors.iter().position(|a| a.node == node) {
            self.anchors.remove(pos);
        }
    }

    fn add_anchor(&mut self, name: &str, node: NodeId) {
        let name = self.anchor_key(name);
        self.anchors.push(Anchor { name, node });
    }

    /// `GetNodeByAnchor`: the lookup is case-insensitive unless the
    /// document is HTML5.
    fn get_node_by_anchor(&self, name: &str) -> Option<NodeId> {
        let lname = if self.html_version() == HT50 {
            name.to_string()
        } else {
            name.to_ascii_lowercase()
        };
        self.anchors
            .iter()
            .find(|a| a.name == lname)
            .map(|a| a.node)
    }

    // ---- duplicates -----------------------------------------------------------

    /// `AttrsHaveSameName`.
    fn attrs_have_same_name(a: &AttVal, b: &AttVal) -> bool {
        match (a.dict, b.dict) {
            (Some(x), Some(y)) => x == y,
            (None, None) => match (&a.attribute, &b.attribute) {
                (Some(x), Some(y)) => x == y,
                _ => false,
            },
            _ => false,
        }
    }

    /// `TY_(RepairDuplicateAttributes)`.
    pub(crate) fn repair_duplicate_attributes(&mut self, node: NodeId) {
        let known = tables::tables().known;
        let mut first = 0usize;
        while first < self.tree.node(node).attributes.len() {
            let first_av = self.tree.node(node).attributes[first].clone();
            if !(first_av.asp.is_none() && first_av.php.is_none()) {
                first += 1;
                continue;
            }
            let mut first_redefined = false;
            let mut second = first + 1;
            while second < self.tree.node(node).attributes.len() {
                let second_av = self.tree.node(node).attributes[second].clone();
                let first_av = self.tree.node(node).attributes[first].clone();
                if !(second_av.asp.is_none()
                    && second_av.php.is_none()
                    && Self::attrs_have_same_name(&first_av, &second_av))
                {
                    second += 1;
                    continue;
                }
                if first_av.dict == Some(known.class)
                    && self.opts.join_classes
                    && first_av.has_value()
                    && second_av.has_value()
                {
                    let joined = format!(
                        "{} {}",
                        first_av.value.as_deref().unwrap_or(""),
                        second_av.value.as_deref().unwrap_or("")
                    );
                    self.tree.node_mut(node).attributes[first].value = Some(joined);
                    self.report_attr(Some(node), Some(&second_av), Code::JoiningAttribute);
                    self.remove_attribute(node, second);
                } else if first_av.dict == Some(known.style)
                    && self.opts.join_styles
                    && first_av.has_value()
                    && second_av.has_value()
                {
                    let merged = append_to_style_attr(
                        first_av.value.as_deref().unwrap_or(""),
                        second_av.value.as_deref().unwrap_or(""),
                    );
                    self.tree.node_mut(node).attributes[first].value = Some(merged);
                    self.report_attr(Some(node), Some(&second_av), Code::JoiningAttribute);
                    self.remove_attribute(node, second);
                } else if self.opts.keep_last_attribute {
                    self.report_attr(Some(node), Some(&first_av), Code::RepeatedAttribute);
                    self.remove_attribute(node, first);
                    first_redefined = true;
                    // `first` now names what was the next attribute; the
                    // scan continues from the attribute after `second`,
                    // which moved down by one.
                } else {
                    self.report_attr(Some(node), Some(&second_av), Code::RepeatedAttribute);
                    self.remove_attribute(node, second);
                }
            }
            if !first_redefined {
                first += 1;
            }
        }
    }

    // ---- checks ---------------------------------------------------------------

    /// `TY_(CheckAttribute)`: constrain the version and run the value check.
    fn check_attribute(&mut self, node: NodeId, index: usize) {
        let av = self.tree.node(node).attributes[index].clone();
        let Some(dict) = av.dict else {
            return;
        };
        let known = tables::tables().known;
        if dict == known.xml_lang || dict == known.xml_space {
            self.lexer.isvoyager = true;
            if !self.opts.output_html {
                self.opts.output_xhtml = true;
                self.opts.output_xml = true;
            }
        }
        let versions = self.attribute_versions(node, &av);
        self.constrain_version(versions);
        if let Some(check) = tables::tables().attributes[dict].check {
            self.run_attr_check(check, node, index);
        }
    }

    /// `TY_(CheckAttributes)`: check every attribute of the node.
    pub(crate) fn check_attributes(&mut self, node: NodeId) {
        let mut i = 0;
        while i < self.tree.node(node).attributes.len() {
            self.check_attribute(node, i);
            i += 1;
        }
    }

    fn attr(&self, node: NodeId, index: usize) -> AttVal {
        self.tree.node(node).attributes[index].clone()
    }

    fn set_attr_value(&mut self, node: NodeId, index: usize, value: String) {
        self.tree.node_mut(node).attributes[index].value = Some(value);
    }

    fn run_attr_check(&mut self, check: AttrCheck, node: NodeId, index: usize) {
        match check {
            AttrCheck::CheckUrl => self.check_url(node, index),
            AttrCheck::CheckAction => {
                if self.attr(node, index).has_value() {
                    self.check_url(node, index);
                }
            }
            AttrCheck::CheckScript => {}
            AttrCheck::CheckName => self.check_name(node, index),
            AttrCheck::CheckId => self.check_id(node, index),
            AttrCheck::CheckIs => self.check_is(node, index),
            AttrCheck::CheckBool => {
                if self.attr(node, index).has_value() {
                    self.check_lower_case_attr_value(node, index);
                }
            }
            AttrCheck::CheckAlign => self.check_align(node, index),
            AttrCheck::CheckValign => self.check_valign(node, index),
            AttrCheck::CheckLength => self.check_length(node, index),
            AttrCheck::CheckTarget => self.check_target(node, index),
            AttrCheck::CheckFsubmit => self.check_attr_validity(node, index, &["get", "post"]),
            AttrCheck::CheckClear => self.check_clear(node, index),
            AttrCheck::CheckShape => {
                self.check_attr_validity(node, index, &["rect", "default", "circle", "poly"]);
            }
            AttrCheck::CheckScope => {
                self.check_attr_validity(node, index, &["row", "rowgroup", "col", "colgroup"]);
            }
            AttrCheck::CheckNumber => self.check_number(node, index),
            AttrCheck::CheckColor => self.check_color(node, index),
            AttrCheck::CheckVType => {
                self.check_attr_validity(node, index, &["data", "object", "ref"]);
            }
            AttrCheck::CheckScroll => self.check_attr_validity(node, index, &["no", "auto", "yes"]),
            AttrCheck::CheckTextDir => {
                if self.html5_mode {
                    self.check_attr_validity(node, index, &["rtl", "ltr", "auto"]);
                } else {
                    self.check_attr_validity(node, index, &["rtl", "ltr"]);
                }
            }
            AttrCheck::CheckLang => self.check_lang(node, index),
            AttrCheck::CheckLoading => self.check_attr_validity(node, index, &["lazy", "eager"]),
            AttrCheck::CheckType => self.check_type(node, index),
            AttrCheck::CheckRDFaPrefix => self.check_rdfa_prefix(node, index),
            AttrCheck::CheckRDFaTerm | AttrCheck::CheckRDFaSafeCURIE => {
                if !self.attr(node, index).has_value() {
                    let av = self.attr(node, index);
                    self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
                }
            }
            AttrCheck::CheckSvgAttr => self.check_svg_attr(node, index),
        }
    }

    fn report_missing_value(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
    }

    fn report_bad_value(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
    }

    /// `CheckLowerCaseAttrValue`.
    fn check_lower_case_attr_value(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        let Some(value) = av.value.as_deref() else {
            return;
        };
        if value.chars().any(|c| is_upper(c as u32)) {
            if self.lexer.isvoyager {
                self.report_attr(Some(node), Some(&av), Code::AttrValueNotLcase);
            }
            if self.lexer.isvoyager || self.opts.lower_literals {
                let lowered = value.to_ascii_lowercase();
                self.set_attr_value(node, index, lowered);
            }
        }
    }

    /// `CheckAttrValidity`: a value from a fixed list.
    fn check_attr_validity(&mut self, node: NodeId, index: usize, list: &[&str]) {
        if !self.attr(node, index).has_value() {
            self.report_missing_value(node, index);
            return;
        }
        self.check_lower_case_attr_value(node, index);
        let av = self.attr(node, index);
        if !list.iter().any(|v| av.value_is(v)) {
            self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
        }
    }

    /// `TY_(CheckUrl)`.
    pub(crate) fn check_url(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        let Some(value) = av.value.clone() else {
            self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
            return;
        };
        let is_javascript = value.starts_with("javascript:");
        let mut escape_count = 0;
        let mut backslash_count = 0;
        let mut fixed: Vec<char> = value.chars().collect();
        for ch in &mut fixed {
            let c = *ch as u32;
            if *ch == '\\' {
                backslash_count += 1;
                if self.opts.fix_backslash && !is_javascript {
                    *ch = '/';
                }
            } else if c > 0x7e || c <= 0x20 || *ch == '<' || *ch == '>' {
                escape_count += 1;
            }
        }
        let bad_codepoint_count = value.chars().filter(|&c| !is_url_code_point(c)).count();
        let mut new_value: String = fixed.iter().collect();
        if self.opts.fix_uri && escape_count > 0 {
            let mut dest = String::new();
            let mut hadnonspace = false;
            for ch in new_value.chars() {
                let c = ch as u32;
                if c > 0x7e || c <= 0x20 || ch == '<' || ch == '>' {
                    if c == 0x20 {
                        if hadnonspace {
                            dest.push_str("%20");
                        }
                    } else {
                        let mut buf = [0u8; 4];
                        for b in ch.encode_utf8(&mut buf).bytes() {
                            write!(dest, "%{b:02X}").expect("writing to a String cannot fail");
                        }
                        hadnonspace = true;
                    }
                } else {
                    hadnonspace = true;
                    dest.push(ch);
                }
            }
            new_value = dest;
        }
        if new_value != value {
            self.set_attr_value(node, index, new_value);
        }
        let av = self.attr(node, index);
        if backslash_count > 0 {
            if self.opts.fix_backslash && !is_javascript {
                self.report_attr(Some(node), Some(&av), Code::FixedBackslash);
            } else {
                self.report_attr(Some(node), Some(&av), Code::BackslashInUri);
            }
        }
        if escape_count > 0 {
            if self.opts.fix_uri {
                self.report_attr(Some(node), Some(&av), Code::EscapedIllegalUri);
            } else if (self.html_version() & tables::VERS_HTML5) == 0 {
                self.report_attr(Some(node), Some(&av), Code::IllegalUriReference);
            }
            self.bad_chars |= super::message::BC_OTHER;
        }
        if bad_codepoint_count > 0 {
            self.report_attr(Some(node), Some(&av), Code::IllegalUriCodepoint);
        }
    }

    /// `CheckName`.
    fn check_name(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        let Some(value) = av.value.clone() else {
            self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
            return;
        };
        if self.is_anchor_element(node) {
            if self.opts.output_xml && !is_valid_nmtoken(&value) {
                self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
            }
            match self.get_node_by_anchor(&value) {
                Some(old) if old != node => {
                    if self.tree.node(node).implicit {
                        self.report_attr(Some(node), Some(&av), Code::AnchorDuplicated);
                    } else {
                        self.report_attr(Some(node), Some(&av), Code::AnchorNotUnique);
                    }
                }
                _ => self.add_anchor(&value, node),
            }
        }
    }

    /// `CheckId`.
    fn check_id(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        let Some(value) = av.value.clone() else {
            self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
            return;
        };
        if !is_valid_html_id(&value) {
            if self.lexer.isvoyager && Self::is_valid_xml_id(&value) {
                self.report_attr(Some(node), Some(&av), Code::XmlIdSyntax);
            } else {
                self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
            }
        }
        match self.get_node_by_anchor(&value) {
            Some(old) if old != node => {
                if self.tree.node(node).implicit {
                    self.report_attr(Some(node), Some(&av), Code::AnchorDuplicated);
                } else {
                    self.report_attr(Some(node), Some(&av), Code::AnchorNotUnique);
                }
            }
            _ => self.add_anchor(&value, node),
        }
    }

    /// `CheckIs`.
    fn check_is(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        let element = self.tree.node(node).element.clone().unwrap_or_default();
        if element.find('-').is_some_and(|p| p > 0) {
            self.report_attr(Some(node), Some(&av), Code::AttributeIsNotAllowed);
        }
        let Some(value) = av.value.as_deref() else {
            self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
            return;
        };
        let go = value.find('-').is_some_and(|p| p > 0) && !value.contains(' ');
        if !go {
            self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
        }
    }

    /// `CheckAlign`.
    fn check_align(&mut self, node: NodeId, index: usize) {
        if self.node_has_cm(node, CM_IMG) {
            self.check_valign(node, index);
            return;
        }
        if !self.attr(node, index).has_value() {
            self.report_missing_value(node, index);
            return;
        }
        self.check_lower_case_attr_value(node, index);
        if self.node_is(node, TagId::CAPTION) {
            return;
        }
        let av = self.attr(node, index);
        let allowed = ["left", "right", "center", "justify"]
            .iter()
            .any(|v| av.value_is(v))
            || (av.value_is("char") && self.node_has_cm(node, CM_TABLE | CM_ROW));
        if !allowed {
            self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
        }
    }

    /// `CheckValign`.
    fn check_valign(&mut self, node: NodeId, index: usize) {
        if !self.attr(node, index).has_value() {
            self.report_missing_value(node, index);
            return;
        }
        self.check_lower_case_attr_value(node, index);
        let av = self.attr(node, index);
        if ["top", "middle", "bottom", "baseline"]
            .iter()
            .any(|v| av.value_is(v))
        {
            // fine
        } else if ["left", "right"].iter().any(|v| av.value_is(v)) {
            if !self.node_has_cm(node, CM_IMG) {
                self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
            }
        } else if ["texttop", "absmiddle", "absbottom", "textbottom"]
            .iter()
            .any(|v| av.value_is(v))
        {
            self.constrain_version(VERS_PROPRIETARY);
            self.report_attr(Some(node), Some(&av), Code::ProprietaryAttrValue);
        } else {
            self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
        }
    }

    /// `CheckLength`.
    fn check_length(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        let Some(value) = av.value.as_deref() else {
            self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
            return;
        };
        let known = tables::tables().known;
        if av.dict == Some(known.width)
            && (self.node_is(node, TagId::COL) || self.node_is(node, TagId::COLGROUP))
        {
            return;
        }
        let bytes = value.as_bytes();
        if bytes.first().is_none_or(|b| !b.is_ascii_digit()) {
            self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
            return;
        }
        let mut percent_found = false;
        for &b in &bytes[1..] {
            if !percent_found && b == b'%' {
                percent_found = true;
            } else if percent_found || !b.is_ascii_digit() {
                self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
                break;
            }
        }
    }

    /// `CheckTarget`.
    fn check_target(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        let Some(value) = av.value.as_deref() else {
            self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
            return;
        };
        if value
            .bytes()
            .next()
            .is_some_and(|b| is_letter(u32::from(b)))
        {
            return;
        }
        if !["_blank", "_self", "_parent", "_top"]
            .iter()
            .any(|v| av.value_is(v))
        {
            self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
        }
    }

    /// `CheckClear`.
    fn check_clear(&mut self, node: NodeId, index: usize) {
        if !self.attr(node, index).has_value() {
            self.report_missing_value(node, index);
            self.set_attr_value(node, index, "none".to_string());
            return;
        }
        self.check_lower_case_attr_value(node, index);
        let av = self.attr(node, index);
        if !["none", "left", "right", "all"]
            .iter()
            .any(|v| av.value_is(v))
        {
            self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
        }
    }

    /// `CheckNumber`.
    fn check_number(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        let Some(value) = av.value.as_deref() else {
            self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
            return;
        };
        let known = tables::tables().known;
        if self.node_is(node, TagId::FRAMESET)
            && (av.dict == Some(known.cols) || av.dict == Some(known.rows))
        {
            return;
        }
        let mut p = value.as_bytes();
        if self.node_is(node, TagId::FONT) && p.first().is_some_and(|&b| b == b'+' || b == b'-') {
            p = &p[1..];
        }
        if av.attribute.as_deref() == Some("tabindex") && p.first() == Some(&b'-') {
            p = &p[1..];
        }
        if p.iter().any(|b| !b.is_ascii_digit()) {
            self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
        }
    }

    /// `CheckColor`.
    fn check_color(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        let Some(mut given) = av.value.clone() else {
            self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
            return;
        };
        let mut valid = false;
        if !given.starts_with('#') && is_valid_color_code(&given) {
            valid = true;
            given = format!("#{given}");
            self.report_attr(Some(node), Some(&av), Code::BadAttributeValueReplaced);
            self.set_attr_value(node, index, given.clone());
        }
        if !valid && given.starts_with('#') {
            valid = is_valid_color_code(&given[1..]);
        }
        if valid && given.starts_with('#') && self.opts.replace_color {
            let name = COLORS
                .iter()
                .find(|(_, hex)| hex.eq_ignore_ascii_case(&given))
                .map(|(n, _)| (*n).to_string());
            if let Some(name) = name {
                given = name;
                self.set_attr_value(node, index, given.clone());
            }
        }
        if !valid {
            valid = is_color_name(&given, self.html5_mode);
        }
        if valid && given.starts_with('#') {
            self.set_attr_value(node, index, given.to_ascii_uppercase());
        } else if valid {
            self.set_attr_value(node, index, given.to_ascii_lowercase());
        }
        if !valid {
            let av = self.attr(node, index);
            self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
        }
    }

    /// `CheckLang`.
    fn check_lang(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        let known = tables::tables().known;
        if !av.has_value() && av.dict != Some(known.xml_lang) {
            self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
        }
    }

    /// `CheckType`.
    fn check_type(&mut self, node: NodeId, index: usize) {
        const INPUT: &[&str] = &[
            "text",
            "password",
            "checkbox",
            "radio",
            "submit",
            "reset",
            "file",
            "hidden",
            "image",
            "button",
            "color",
            "date",
            "datetime",
            "datetime-local",
            "email",
            "month",
            "number",
            "range",
            "search",
            "tel",
            "time",
            "url",
            "week",
        ];
        const UL: &[&str] = &["disc", "square", "circle"];
        const OL: &[&str] = &["1", "a", "i"];
        if self.node_is(node, TagId::INPUT) {
            self.check_attr_validity(node, index, INPUT);
        } else if self.node_is(node, TagId::BUTTON) {
            self.check_attr_validity(node, index, &["button", "submit", "reset"]);
        } else if self.node_is(node, TagId::UL) {
            self.check_attr_validity(node, index, UL);
        } else if self.node_is(node, TagId::OL) {
            let av = self.attr(node, index);
            if !av.has_value() {
                self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
                return;
            }
            if !OL.iter().any(|v| av.value_is(v)) {
                self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
            }
        } else if self.node_is(node, TagId::LI) {
            let av = self.attr(node, index);
            if !av.has_value() {
                self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
                return;
            }
            if UL.iter().any(|v| av.value_is(v)) {
                self.check_lower_case_attr_value(node, index);
            } else if !OL.iter().any(|v| av.value_is(v)) {
                self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
            }
        }
    }

    /// `CheckDecimal`.
    fn check_decimal(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        let value = av.value.clone().unwrap_or_default();
        let mut p = value.as_bytes();
        if p.first().is_some_and(|&b| b == b'+' || b == b'-') {
            p = &p[1..];
        }
        for &b in p {
            // tidy's `break` sits inside the `if (*p == '.')` block, so the
            // first decimal point ends the scan: a second one is never seen.
            if b == b'.' {
                break;
            }
            if !b.is_ascii_digit() {
                self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
                break;
            }
        }
    }

    /// `CheckRDFaPrefix`.
    fn check_rdfa_prefix(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        let Some(value) = av.value.clone() else {
            self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
            return;
        };
        let mut is_prefix = true;
        for t in value.split(' ').filter(|s| !s.is_empty()) {
            if is_prefix {
                match t.find(':') {
                    None => self.report_attr(Some(node), Some(&av), Code::BadAttributeValue),
                    Some(i) if i != t.len() - 1 => {
                        self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
                    }
                    _ => {}
                }
            }
            is_prefix = !is_prefix;
        }
    }

    /// `CheckSvgAttr`.
    fn check_svg_attr(&mut self, node: NodeId, index: usize) {
        let av = self.attr(node, index);
        if !self.node_is(node, TagId::SVG) {
            self.report_attr(Some(node), Some(&av), Code::AttributeIsNotAllowed);
            return;
        }
        let known = tables::tables().known;
        let Some(dict) = av.dict else {
            return;
        };
        let paint = [
            known.color,
            known.fill,
            known.fill_rule,
            known.stroke,
            known.stroke_dasharray,
            known.stroke_dashoffset,
            known.stroke_linecap,
            known.stroke_linejoin,
            known.stroke_miterlimit,
            known.stroke_width,
            known.color_interpolation,
            known.color_rendering,
            known.opacity,
            known.stroke_opacity,
            known.fill_opacity,
        ];
        if !paint.contains(&dict) {
            return;
        }
        if !av.has_value() {
            self.report_attr(Some(node), Some(&av), Code::MissingAttrValue);
            return;
        }
        if av.value_is("inherit") {
            return;
        }
        let among = |list: &[&str]| list.iter().any(|v| av.value_is(v));
        if dict == known.fill || dict == known.stroke {
            if among(&["none", "currentColor"]) {
                self.check_lower_case_attr_value(node, index);
            } else {
                self.check_color(node, index);
            }
        } else if dict == known.fill_rule {
            if among(&["nonzero", "evenodd"]) {
                self.check_lower_case_attr_value(node, index);
            } else {
                self.report_bad_value(node, index);
            }
        } else if dict == known.stroke_dasharray {
            if among(&["none"]) {
                self.check_lower_case_attr_value(node, index);
            }
        } else if dict == known.stroke_dashoffset || dict == known.stroke_width {
            self.check_length(node, index);
        } else if dict == known.stroke_linecap {
            if among(&["butt", "round", "square"]) {
                self.check_lower_case_attr_value(node, index);
            } else {
                self.report_bad_value(node, index);
            }
        } else if dict == known.stroke_linejoin {
            if among(&["miter", "round", "bevel"]) {
                self.check_lower_case_attr_value(node, index);
            } else {
                self.report_bad_value(node, index);
            }
        } else if dict == known.stroke_miterlimit {
            self.check_number(node, index);
        } else if dict == known.color_interpolation {
            if among(&["auto", "sRGB", "linearRGB"]) {
                self.check_lower_case_attr_value(node, index);
            } else {
                self.report_bad_value(node, index);
            }
        } else if dict == known.color_rendering {
            if among(&["auto", "optimizeSpeed", "optimizeQuality"]) {
                self.check_lower_case_attr_value(node, index);
            } else {
                self.report_bad_value(node, index);
            }
        } else if dict == known.opacity
            || dict == known.stroke_opacity
            || dict == known.fill_opacity
        {
            self.check_decimal(node, index);
        }
    }

    // ---- element checks (tags.c) --------------------------------------------

    /// `CheckIMG`.
    fn check_img(&mut self, node: NodeId) {
        let known = tables::tables().known;
        let has_alt = self.tree.attr_by_id(node, known.alt).is_some();
        let has_src = self.tree.attr_by_id(node, known.src).is_some();
        let has_usemap = self.tree.attr_by_id(node, known.usemap).is_some();
        let has_ismap = self.tree.attr_by_id(node, known.ismap).is_some();
        let has_datafld = self.tree.attr_by_id(node, known.datafld).is_some();
        self.check_attributes(node);
        if !has_alt {
            let alttext = self.opts.alt_text.clone();
            if alttext.is_none() {
                self.bad_access |= BA_MISSING_IMAGE_ALT;
                self.report_missing_attr(node, "alt");
            }
            if let Some(text) = alttext {
                let index = self.add_attribute(node, "alt", Some(&text));
                let av = self.attr(node, index);
                self.report_attr(Some(node), Some(&av), Code::InsertingAutoAttribute);
            }
        }
        if !has_src && !has_datafld {
            self.report_missing_attr(node, "src");
        }
        if has_ismap && !has_usemap {
            self.report_attr(Some(node), None, Code::MissingImagemap);
            self.bad_access |= BA_MISSING_IMAGE_MAP;
        }
    }

    /// `CheckCaption`.
    fn check_caption(&mut self, node: NodeId) {
        self.check_attributes(node);
        let known = tables::tables().known;
        let Some(index) = self.tree.attr_by_id(node, known.align) else {
            return;
        };
        let av = self.attr(node, index);
        if !av.has_value() {
            return;
        }
        if av.value_is("left") || av.value_is("right") {
            self.constrain_version(VERS_HTML40_LOOSE);
        } else if av.value_is("top") || av.value_is("bottom") {
            self.constrain_version(!(VERS_HTML20 | VERS_HTML32));
        } else {
            self.report_attr(Some(node), Some(&av), Code::BadAttributeValue);
        }
    }

    /// `CheckAREA`.
    fn check_area(&mut self, node: NodeId) {
        let known = tables::tables().known;
        let has_alt = self.tree.attr_by_id(node, known.alt).is_some();
        let has_href = self.tree.attr_by_id(node, known.href).is_some();
        let has_nohref = self.tree.attr_by_id(node, known.nohref).is_some();
        self.check_attributes(node);
        if !has_alt {
            self.bad_access |= BA_MISSING_LINK_ALT;
            self.report_missing_attr(node, "alt");
        }
        if !has_href && !has_nohref {
            self.report_missing_attr(node, "href");
        }
    }

    /// `CheckTABLE`.
    fn check_table(&mut self, node: NodeId) {
        let known = tables::tables().known;
        let has_summary = self.tree.attr_by_id(node, known.summary).is_some();
        let vers = self.html_version();
        let is_html5 = vers == HT50 || vers == XH50;
        self.check_attributes(node);
        if has_summary && is_html5 {
            self.report(Some(node), Some(node), Code::BadSummaryHtml5);
        } else if !has_summary && !is_html5 {
            self.bad_access |= BA_MISSING_SUMMARY;
            self.report_missing_attr(node, "summary");
        }
        if self.opts.output_xml
            && let Some(index) = self.tree.attr_by_id(node, known.border)
            && self.tree.node(node).attributes[index].value.is_none()
        {
            self.set_attr_value(node, index, "1".to_string());
        }
    }

    /// `CheckLINK`.
    fn check_link(&mut self, node: NodeId) {
        let known = tables::tables().known;
        let has_href = self.tree.attr_by_id(node, known.href).is_some();
        let has_rel = self.tree.attr_by_id(node, known.rel).is_some();
        let has_itemprop = self.tree.attr_by_id(node, known.itemprop).is_some();
        if !has_href {
            self.report_missing_attr(node, "href");
        }
        if !has_itemprop && !has_rel {
            self.report_missing_attr(node, "rel");
        }
    }

    /// The element's check function, or the generic one.
    pub(crate) fn check_element_attributes(&mut self, node: NodeId) {
        let check = self.tree.node(node).tag.and_then(|t| self.tags[t].check);
        match check {
            Some(TagCheck::CheckIMG) => self.check_img(node),
            Some(TagCheck::CheckCaption) => self.check_caption(node),
            Some(TagCheck::CheckAREA) => self.check_area(node),
            Some(TagCheck::CheckTABLE) => self.check_table(node),
            Some(TagCheck::CheckLINK) => self.check_link(node),
            Some(TagCheck::CheckHTML) | None => self.check_attributes(node),
        }
    }

    /// parser.c `AttributeChecks`: run the checks over a subtree.
    pub(crate) fn attribute_checks(&mut self, mut node: Option<NodeId>) {
        while let Some(n) = node {
            let next = self.tree.next(n);
            if self.tree.node(n).is_element() {
                self.check_element_attributes(n);
            }
            if let Some(content) = self.tree.content(n) {
                self.attribute_checks(Some(content));
            }
            node = next;
        }
    }
}

/// `AppendToStyleAttr`: add a declaration to a style value.
fn append_to_style_attr(style: &str, prop: &str) -> String {
    if style.ends_with(';') {
        format!("{style} {prop}")
    } else if style.ends_with('}') {
        format!("{style} {{ {prop} }}")
    } else if style.is_empty() {
        prop.to_string()
    } else {
        format!("{style}; {prop}")
    }
}

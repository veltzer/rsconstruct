//! tidy's configuration (config.c): the option table, the `name: value`
//! file grammar, and the subset of options that steer the checks itidy
//! runs. Every option tidy knows is accepted so that an existing tidy
//! config file parses; the ones that only shape tidy's pretty-printed
//! output are validated and ignored, and the few whose effect itidy does
//! not reproduce are refused by name.

use std::fmt::Write as _;

/// tidy's `TidyTriState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriState {
    No,
    Yes,
    Auto,
}

/// The `custom-tags` option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomTags {
    No,
    Blocklevel,
    Empty,
    Inline,
    Pre,
}

/// The `doctype` option's mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoctypeMode {
    Html5,
    Omit,
    Auto,
    Strict,
    Loose,
    User,
}

/// The `uppercase-attributes` option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UppercaseAttrs {
    No,
    Yes,
    Preserve,
}

/// The options that affect what itidy reports, with tidy's defaults.
#[derive(Clone, Debug)]
pub struct Options {
    pub alt_text: Option<String>,
    pub anchor_as_name: bool,
    pub coerce_endtags: bool,
    pub custom_tags: CustomTags,
    pub new_blocklevel_tags: Vec<String>,
    pub new_inline_tags: Vec<String>,
    pub new_empty_tags: Vec<String>,
    pub new_pre_tags: Vec<String>,
    pub decorate_inferred_ul: bool,
    pub doctype_mode: DoctypeMode,
    pub doctype_user: Option<String>,
    pub drop_empty_elements: bool,
    pub drop_empty_paras: bool,
    pub drop_proprietary_attributes: bool,
    /// `repeated-attributes`: true keeps the last (tidy's default).
    pub keep_last_attribute: bool,
    pub enclose_block_text: bool,
    pub enclose_text: bool,
    pub escape_scripts: bool,
    pub fix_backslash: bool,
    pub fix_bad_comments: TriState,
    pub fix_uri: bool,
    pub join_classes: bool,
    pub join_styles: bool,
    pub literal_attributes: bool,
    pub lower_literals: bool,
    pub merge_emphasis: bool,
    pub add_meta_charset: bool,
    pub show_meta_change: bool,
    pub ncr: bool,
    pub omit_optional_tags: bool,
    pub output_html: bool,
    pub output_xhtml: bool,
    pub output_xml: bool,
    pub preserve_entities: bool,
    pub quote_ampersand: bool,
    pub show_errors: u32,
    pub show_info: bool,
    pub show_warnings: bool,
    pub quiet: bool,
    pub skip_nested: bool,
    pub strict_tags_attributes: bool,
    pub fix_style_tags: bool,
    pub tab_size: u32,
    pub keep_tabs: bool,
    pub uppercase_attributes: UppercaseAttrs,
    pub warn_proprietary_attributes: bool,
    pub assume_xml_procins: bool,
    pub show_body_only: TriState,
    pub replace_color: bool,
    pub force_output: bool,
    pub make_clean: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            alt_text: None,
            anchor_as_name: true,
            coerce_endtags: true,
            custom_tags: CustomTags::No,
            new_blocklevel_tags: Vec::new(),
            new_inline_tags: Vec::new(),
            new_empty_tags: Vec::new(),
            new_pre_tags: Vec::new(),
            decorate_inferred_ul: false,
            doctype_mode: DoctypeMode::Auto,
            doctype_user: None,
            drop_empty_elements: true,
            drop_empty_paras: true,
            drop_proprietary_attributes: false,
            keep_last_attribute: true,
            enclose_block_text: false,
            enclose_text: false,
            escape_scripts: true,
            fix_backslash: true,
            fix_bad_comments: TriState::Auto,
            fix_uri: true,
            join_classes: false,
            join_styles: true,
            literal_attributes: false,
            lower_literals: true,
            merge_emphasis: true,
            add_meta_charset: false,
            show_meta_change: false,
            ncr: true,
            omit_optional_tags: false,
            output_html: false,
            output_xhtml: false,
            output_xml: false,
            preserve_entities: false,
            quote_ampersand: true,
            show_errors: 6,
            show_info: true,
            show_warnings: true,
            quiet: false,
            skip_nested: true,
            strict_tags_attributes: false,
            fix_style_tags: true,
            tab_size: 8,
            keep_tabs: false,
            uppercase_attributes: UppercaseAttrs::No,
            warn_proprietary_attributes: true,
            assume_xml_procins: false,
            show_body_only: TriState::No,
            replace_color: false,
            force_output: false,
            make_clean: false,
        }
    }
}

/// How an option's value is parsed (the `parser` column of tidy's table).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    AutoBool,
    Int,
    Str,
    List,
    Doctype,
    CharEnc,
    Pick(&'static [&'static [&'static str]]),
}

const REPEAT_PICKS: &[&[&str]] = &[&["keep-first"], &["keep-last"]];
const ACCESS_PICKS: &[&[&str]] = &[
    &["0", "0 (Tidy Classic)"],
    &["1", "1 (Priority 1 Checks)"],
    &["2", "2 (Priority 2 Checks)"],
    &["3", "3 (Priority 3 Checks)"],
];
const NEWLINE_PICKS: &[&[&str]] = &[&["lf"], &["crlf"], &["cr"]];
const SORTER_PICKS: &[&[&str]] = &[&["none"], &["alpha"]];
const CUSTOM_PICKS: &[&[&str]] = &[
    &["no", "n"],
    &["blocklevel"],
    &["empty"],
    &["inline", "y", "yes"],
    &["pre"],
];
const CASE_PICKS: &[&[&str]] = &[
    &["0", "n", "f", "no", "false"],
    &["1", "y", "t", "yes", "true"],
    &["preserve"],
];
const BOOL_PICKS: &[&[&str]] = &[
    &["0", "n", "f", "no", "false"],
    &["1", "y", "t", "yes", "true"],
];
const AUTO_PICKS: &[&[&str]] = &[
    &["0", "n", "f", "no", "false"],
    &["1", "y", "t", "yes", "true"],
    &["auto"],
];
const DOCTYPE_PICKS: &[&[&str]] = &[
    &["html5"],
    &["omit"],
    &["auto"],
    &["strict"],
    &["loose", "transitional"],
    &["user"],
];

/// Every option of tidy 5.8.0 and how its value is read.
const OPTION_KINDS: &[(&str, Kind)] = &[
    ("accessibility-check", Kind::Pick(ACCESS_PICKS)),
    ("alt-text", Kind::Str),
    ("anchor-as-name", Kind::Bool),
    ("ascii-chars", Kind::Bool),
    ("new-blocklevel-tags", Kind::List),
    ("show-body-only", Kind::AutoBool),
    ("break-before-br", Kind::Bool),
    ("char-encoding", Kind::CharEnc),
    ("coerce-endtags", Kind::Bool),
    ("css-prefix", Kind::Str),
    ("new-custom-tags", Kind::List),
    ("decorate-inferred-ul", Kind::Bool),
    ("doctype", Kind::Doctype),
    ("doctype-mode", Kind::Pick(DOCTYPE_PICKS)),
    ("drop-empty-elements", Kind::Bool),
    ("drop-empty-paras", Kind::Bool),
    ("drop-proprietary-attributes", Kind::Bool),
    ("repeated-attributes", Kind::Pick(REPEAT_PICKS)),
    ("gnu-emacs", Kind::Bool),
    ("gnu-emacs-file", Kind::Str),
    ("new-empty-tags", Kind::List),
    ("enclose-block-text", Kind::Bool),
    ("enclose-text", Kind::Bool),
    ("error-file", Kind::Str),
    ("escape-cdata", Kind::Bool),
    ("escape-scripts", Kind::Bool),
    ("fix-backslash", Kind::Bool),
    ("fix-bad-comments", Kind::AutoBool),
    ("fix-uri", Kind::Bool),
    ("force-output", Kind::Bool),
    ("gdoc", Kind::Bool),
    ("hide-comments", Kind::Bool),
    ("output-html", Kind::Bool),
    ("input-encoding", Kind::CharEnc),
    ("indent-attributes", Kind::Bool),
    ("indent-cdata", Kind::Bool),
    ("indent", Kind::AutoBool),
    ("indent-spaces", Kind::Int),
    ("new-inline-tags", Kind::List),
    ("join-classes", Kind::Bool),
    ("join-styles", Kind::Bool),
    ("keep-time", Kind::Bool),
    ("keep-tabs", Kind::Bool),
    ("literal-attributes", Kind::Bool),
    ("logical-emphasis", Kind::Bool),
    ("lower-literals", Kind::Bool),
    ("bare", Kind::Bool),
    ("clean", Kind::Bool),
    ("tidy-mark", Kind::Bool),
    ("merge-divs", Kind::AutoBool),
    ("merge-emphasis", Kind::Bool),
    ("merge-spans", Kind::AutoBool),
    ("add-meta-charset", Kind::Bool),
    ("mute", Kind::List),
    ("mute-id", Kind::Bool),
    ("ncr", Kind::Bool),
    ("newline", Kind::Pick(NEWLINE_PICKS)),
    ("numeric-entities", Kind::Bool),
    ("omit-optional-tags", Kind::Bool),
    ("output-encoding", Kind::CharEnc),
    ("output-file", Kind::Str),
    ("output-bom", Kind::AutoBool),
    ("indent-with-tabs", Kind::Bool),
    ("preserve-entities", Kind::Bool),
    ("new-pre-tags", Kind::List),
    ("priority-attributes", Kind::List),
    ("punctuation-wrap", Kind::Bool),
    ("quiet", Kind::Bool),
    ("quote-ampersand", Kind::Bool),
    ("quote-marks", Kind::Bool),
    ("quote-nbsp", Kind::Bool),
    ("replace-color", Kind::Bool),
    ("show-errors", Kind::Int),
    ("show-filename", Kind::Bool),
    ("show-info", Kind::Bool),
    ("markup", Kind::Bool),
    ("show-meta-change", Kind::Bool),
    ("show-warnings", Kind::Bool),
    ("skip-nested", Kind::Bool),
    ("sort-attributes", Kind::Pick(SORTER_PICKS)),
    ("strict-tags-attributes", Kind::Bool),
    ("fix-style-tags", Kind::Bool),
    ("tab-size", Kind::Int),
    ("uppercase-attributes", Kind::Pick(CASE_PICKS)),
    ("uppercase-tags", Kind::Bool),
    ("custom-tags", Kind::Pick(CUSTOM_PICKS)),
    ("vertical-space", Kind::AutoBool),
    ("warn-proprietary-attributes", Kind::Bool),
    ("word-2000", Kind::Bool),
    ("wrap-asp", Kind::Bool),
    ("wrap-attributes", Kind::Bool),
    ("wrap-jste", Kind::Bool),
    ("wrap", Kind::Int),
    ("wrap-php", Kind::Bool),
    ("wrap-script-literals", Kind::Bool),
    ("wrap-sections", Kind::Bool),
    ("write-back", Kind::Bool),
    ("output-xhtml", Kind::Bool),
    ("add-xml-decl", Kind::Bool),
    ("output-xml", Kind::Bool),
    ("assume-xml-procins", Kind::Bool),
    ("add-xml-space", Kind::Bool),
    ("input-xml", Kind::Bool),
];

/// A parsed option value.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Bool(bool),
    Tri(TriState),
    Int(u32),
    Str(String),
    List(Vec<String>),
    /// The index of the pick list entry.
    Pick(usize),
    /// `doctype` given as a public identifier.
    DoctypeFpi(String),
}

const fn is_white(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 0x0c)
}

fn pick(value: &str, picks: &[&[&str]], option: &str) -> Result<usize, String> {
    let word: String = value
        .trim_start_matches(|c: char| c.is_ascii_whitespace())
        .chars()
        .take_while(|c| !c.is_ascii_whitespace())
        .take(15)
        .collect();
    picks
        .iter()
        .position(|inputs| inputs.iter().any(|i| i.eq_ignore_ascii_case(&word)))
        .ok_or_else(|| format!("option \"{option}\" given bad argument \"{word}\""))
}

/// tidy's `ParseString`: optional quotes, whitespace runs collapsed.
fn parse_string(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() && is_white(bytes[i]) && bytes[i] != b'\r' && bytes[i] != b'\n' {
        i += 1;
    }
    let mut delim = 0u8;
    if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
        delim = bytes[i];
        i += 1;
    }
    let mut out = Vec::new();
    let mut waswhite = true;
    while i < bytes.len() && bytes[i] != b'\r' && bytes[i] != b'\n' {
        let c = bytes[i];
        if delim != 0 && c == delim {
            break;
        }
        if is_white(c) {
            if waswhite {
                i += 1;
                continue;
            }
            out.push(b' ');
        } else {
            waswhite = false;
            out.push(c);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// tidy's `ParseList`: items separated by blanks or commas, possibly
/// continued on following lines that start with whitespace.
fn parse_list(value: &str) -> Vec<String> {
    value
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_value(option: &str, kind: Kind, value: &str) -> Result<Value, String> {
    let trimmed = value.trim_start_matches([' ', '\t']);
    Ok(match kind {
        Kind::Bool => Value::Bool(pick(trimmed, BOOL_PICKS, option)? == 1),
        Kind::AutoBool => Value::Tri(match pick(trimmed, AUTO_PICKS, option)? {
            0 => TriState::No,
            1 => TriState::Yes,
            _ => TriState::Auto,
        }),
        Kind::Int => {
            let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                return Err(format!(
                    "missing or malformed argument for option: {option}"
                ));
            }
            Value::Int(
                digits
                    .parse()
                    .map_err(|_| format!("missing or malformed argument for option: {option}"))?,
            )
        }
        Kind::Str => Value::Str(parse_string(trimmed)),
        Kind::List => Value::List(parse_list(trimmed)),
        Kind::Doctype => {
            if trimmed.is_empty() {
                Value::Pick(2)
            } else if trimmed.starts_with(['"', '\'', '-', '+']) {
                Value::DoctypeFpi(parse_string(trimmed))
            } else {
                Value::Pick(pick(trimmed, DOCTYPE_PICKS, option)?)
            }
        }
        Kind::CharEnc => {
            let word: String = trimmed
                .chars()
                .take_while(|c| !c.is_ascii_whitespace())
                .collect::<String>()
                .to_ascii_lowercase();
            match word.as_str() {
                "utf8" | "raw" => Value::Str(word),
                "ascii" | "latin0" | "latin1" | "iso2022" | "mac" | "win1252" | "ibm858"
                | "utf16le" | "utf16be" | "utf16" | "big5" | "shiftjis" => {
                    return Err(format!(
                        "option \"{option}\" is set to {word}: itidy reads UTF-8 input only"
                    ));
                }
                _ => return Err(format!("option \"{option}\" given bad argument \"{word}\"")),
            }
        }
        Kind::Pick(picks) => Value::Pick(pick(trimmed, picks, option)?),
    })
}

impl Options {
    /// Apply one `name: value` setting as tidy would. Unknown options and
    /// the few whose effect itidy cannot reproduce are errors.
    pub fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        let lname = name.trim().to_ascii_lowercase();
        let Some((option, kind)) = OPTION_KINDS.iter().find(|(n, _)| *n == lname).copied() else {
            return Err(format!("unknown option: {}", name.trim()));
        };
        let parsed = parse_value(option, kind, value)?;
        let as_bool = |v: &Value| match v {
            Value::Bool(b) => *b,
            _ => false,
        };
        let as_tri = |v: &Value| match v {
            Value::Tri(t) => *t,
            _ => TriState::No,
        };
        let as_int = |v: &Value| match v {
            Value::Int(i) => *i,
            _ => 0,
        };
        let as_list = |v: &Value| match v {
            Value::List(l) => l.clone(),
            _ => Vec::new(),
        };
        let as_str = |v: &Value| match v {
            Value::Str(s) => s.clone(),
            _ => String::new(),
        };
        let unsupported = |what: &str| -> Result<(), String> {
            Err(format!(
                "option \"{option}\" set to {what} is not supported by itidy"
            ))
        };
        match option {
            "alt-text" => {
                let s = as_str(&parsed);
                self.alt_text = if s.is_empty() { None } else { Some(s) };
            }
            "anchor-as-name" => self.anchor_as_name = as_bool(&parsed),
            "coerce-endtags" => self.coerce_endtags = as_bool(&parsed),
            "custom-tags" => {
                self.custom_tags = match parsed {
                    Value::Pick(0) => CustomTags::No,
                    Value::Pick(1) => CustomTags::Blocklevel,
                    Value::Pick(2) => CustomTags::Empty,
                    Value::Pick(3) => CustomTags::Inline,
                    _ => CustomTags::Pre,
                }
            }
            "new-blocklevel-tags" => self.new_blocklevel_tags = as_list(&parsed),
            "new-inline-tags" => self.new_inline_tags = as_list(&parsed),
            "new-empty-tags" => self.new_empty_tags = as_list(&parsed),
            "new-pre-tags" => self.new_pre_tags = as_list(&parsed),
            "new-custom-tags" => {
                if !as_list(&parsed).is_empty() {
                    return unsupported("a list");
                }
            }
            "decorate-inferred-ul" => self.decorate_inferred_ul = as_bool(&parsed),
            "doctype" | "doctype-mode" => match parsed {
                Value::DoctypeFpi(fpi) => {
                    self.doctype_mode = DoctypeMode::User;
                    self.doctype_user = if fpi.is_empty() { None } else { Some(fpi) };
                }
                Value::Pick(i) => {
                    self.doctype_mode = match i {
                        0 => DoctypeMode::Html5,
                        1 => DoctypeMode::Omit,
                        2 => DoctypeMode::Auto,
                        3 => DoctypeMode::Strict,
                        4 => DoctypeMode::Loose,
                        _ => DoctypeMode::User,
                    }
                }
                _ => {}
            },
            "drop-empty-elements" => self.drop_empty_elements = as_bool(&parsed),
            "drop-empty-paras" => self.drop_empty_paras = as_bool(&parsed),
            "drop-proprietary-attributes" => self.drop_proprietary_attributes = as_bool(&parsed),
            "repeated-attributes" => self.keep_last_attribute = parsed == Value::Pick(1),
            "enclose-block-text" => self.enclose_block_text = as_bool(&parsed),
            "enclose-text" => self.enclose_text = as_bool(&parsed),
            "escape-scripts" => self.escape_scripts = as_bool(&parsed),
            "fix-backslash" => self.fix_backslash = as_bool(&parsed),
            "fix-bad-comments" => self.fix_bad_comments = as_tri(&parsed),
            "fix-uri" => self.fix_uri = as_bool(&parsed),
            "force-output" => self.force_output = as_bool(&parsed),
            "join-classes" => self.join_classes = as_bool(&parsed),
            "join-styles" => self.join_styles = as_bool(&parsed),
            "keep-tabs" => self.keep_tabs = as_bool(&parsed),
            "literal-attributes" => self.literal_attributes = as_bool(&parsed),
            "lower-literals" => self.lower_literals = as_bool(&parsed),
            "merge-emphasis" => self.merge_emphasis = as_bool(&parsed),
            "add-meta-charset" => self.add_meta_charset = as_bool(&parsed),
            "show-meta-change" => self.show_meta_change = as_bool(&parsed),
            "ncr" => self.ncr = as_bool(&parsed),
            "omit-optional-tags" => self.omit_optional_tags = as_bool(&parsed),
            "output-html" => self.output_html = as_bool(&parsed),
            "output-xhtml" => {
                self.output_xhtml = as_bool(&parsed);
                if self.output_xhtml {
                    self.output_xml = true;
                }
            }
            "output-xml" => self.output_xml = as_bool(&parsed),
            "preserve-entities" => self.preserve_entities = as_bool(&parsed),
            "quiet" => self.quiet = as_bool(&parsed),
            "quote-ampersand" => self.quote_ampersand = as_bool(&parsed),
            "replace-color" => self.replace_color = as_bool(&parsed),
            "show-errors" => self.show_errors = as_int(&parsed),
            "show-info" => self.show_info = as_bool(&parsed),
            "show-warnings" => self.show_warnings = as_bool(&parsed),
            "show-body-only" => self.show_body_only = as_tri(&parsed),
            "skip-nested" => self.skip_nested = as_bool(&parsed),
            "strict-tags-attributes" => self.strict_tags_attributes = as_bool(&parsed),
            "fix-style-tags" => self.fix_style_tags = as_bool(&parsed),
            "tab-size" => self.tab_size = as_int(&parsed),
            "uppercase-attributes" => {
                self.uppercase_attributes = match parsed {
                    Value::Pick(1) => UppercaseAttrs::Yes,
                    Value::Pick(2) => UppercaseAttrs::Preserve,
                    _ => UppercaseAttrs::No,
                }
            }
            "warn-proprietary-attributes" => self.warn_proprietary_attributes = as_bool(&parsed),
            "assume-xml-procins" => self.assume_xml_procins = as_bool(&parsed),
            // Options whose "yes" rewrites the document in ways itidy does
            // not follow.
            "clean" | "gdoc" | "bare" | "word-2000" | "logical-emphasis" | "input-xml" => {
                if as_bool(&parsed) {
                    return unsupported("yes");
                }
                if option == "clean" {
                    self.make_clean = false;
                }
            }
            "accessibility-check" => {
                if parsed != Value::Pick(0) {
                    return unsupported("a level above 0");
                }
            }
            "mute" => {
                if !as_list(&parsed).is_empty() {
                    return unsupported("a list");
                }
            }
            "output-encoding" | "char-encoding" | "input-encoding" if as_str(&parsed) != "utf8" => {
                return unsupported("anything but utf8");
            }
            // Pretty-printing, file and encoding details without a bearing
            // on the checks: accepted, validated and ignored.
            _ => {}
        }
        Ok(())
    }

    /// Apply a tidy configuration file (`TY_(ParseConfigFileEnc)`).
    pub fn apply_config_text(&mut self, text: &str) -> Result<(), String> {
        let bytes = text.as_bytes();
        let mut i = 0;
        let mut errors = String::new();
        while i < bytes.len() {
            // Leading blanks on the line.
            while i < bytes.len() && is_white(bytes[i]) && bytes[i] != b'\n' && bytes[i] != b'\r' {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let c = bytes[i];
            if c == b'/' || c == b'#' || c == b'\n' || c == b'\r' {
                i = next_property(bytes, i);
                continue;
            }
            let name_start = i;
            while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b':' && i - name_start < 63 {
                i += 1;
            }
            let name = String::from_utf8_lossy(&bytes[name_start..i]).into_owned();
            if i < bytes.len() && bytes[i] == b':' {
                i += 1;
                let value_start = i;
                let mut end = i;
                while end < bytes.len() && bytes[end] != b'\n' && bytes[end] != b'\r' {
                    end += 1;
                }
                // A list may continue on following lines that start with
                // whitespace.
                let kind = OPTION_KINDS
                    .iter()
                    .find(|(n, _)| *n == name.trim().to_ascii_lowercase())
                    .map(|(_, k)| *k);
                if kind == Some(Kind::List) {
                    let mut probe = end;
                    loop {
                        let mut j = probe;
                        if j < bytes.len() && bytes[j] == b'\r' {
                            j += 1;
                        }
                        if j < bytes.len() && bytes[j] == b'\n' {
                            j += 1;
                        }
                        if j < bytes.len()
                            && j > probe
                            && is_white(bytes[j])
                            && bytes[j] != b'\n'
                            && bytes[j] != b'\r'
                        {
                            while j < bytes.len() && bytes[j] != b'\n' && bytes[j] != b'\r' {
                                j += 1;
                            }
                            probe = j;
                            end = j;
                        } else {
                            break;
                        }
                    }
                }
                let value = String::from_utf8_lossy(&bytes[value_start..end]).into_owned();
                if let Err(e) = self.set(&name, &value) {
                    let _ = writeln!(errors, "{e}");
                }
                i = end;
            }
            i = next_property(bytes, i);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.trim_end().to_string())
        }
    }
}

/// `NextProperty`: skip to the end of the line, then over continuation
/// lines (lines starting with whitespace).
const fn next_property(bytes: &[u8], mut i: usize) -> usize {
    loop {
        while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'\r' {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'\n' {
            i += 1;
        }
        if !(i < bytes.len() && is_white(bytes[i])) {
            return i;
        }
    }
}

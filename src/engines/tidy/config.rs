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

/// Options about the markup tidy accepts and rewrites.
#[derive(Clone, Debug)]
pub struct MarkupOptions {
    pub anchor_as_name: bool,
    pub coerce_endtags: bool,
    pub decorate_inferred_ul: bool,
}

/// Options that drop what is empty or unknown.
#[derive(Clone, Debug)]
pub struct DropOptions {
    pub drop_empty_elements: bool,
    pub drop_empty_paras: bool,
    pub drop_proprietary_attributes: bool,
}

/// Options that wrap or escape text.
#[derive(Clone, Debug)]
pub struct EncloseOptions {
    pub enclose_block_text: bool,
    pub enclose_text: bool,
    pub escape_scripts: bool,
}

/// Options that repair what is malformed.
#[derive(Clone, Debug)]
pub struct FixOptions {
    pub fix_backslash: bool,
    pub fix_uri: bool,
    pub fix_style_tags: bool,
}

/// Options that merge or join what is repeated.
#[derive(Clone, Debug)]
pub struct MergeOptions {
    pub join_classes: bool,
    pub join_styles: bool,
    pub merge_emphasis: bool,
}

/// Options about attribute values.
#[derive(Clone, Debug)]
pub struct AttributeOptions {
    pub literal_attributes: bool,
    pub lower_literals: bool,
    /// `repeated-attributes`: true keeps the last (tidy's default).
    pub keep_last_attribute: bool,
}

/// Options about `<meta>` and XML processing instructions.
#[derive(Clone, Debug)]
pub struct MetaOptions {
    pub add_meta_charset: bool,
    pub show_meta_change: bool,
    pub assume_xml_procins: bool,
}

/// Options about character entities.
#[derive(Clone, Debug)]
pub struct EntityOptions {
    pub ncr: bool,
    pub preserve_entities: bool,
    pub quote_ampersand: bool,
}

/// Which markup language the output is.
#[derive(Clone, Debug)]
pub struct OutputOptions {
    pub output_html: bool,
    pub output_xhtml: bool,
    pub output_xml: bool,
}

/// What is reported.
#[derive(Clone, Debug)]
pub struct ReportOptions {
    pub show_info: bool,
    pub show_warnings: bool,
    pub quiet: bool,
}

/// How strictly tags and attributes are checked.
#[derive(Clone, Debug)]
pub struct StrictnessOptions {
    pub strict_tags_attributes: bool,
    pub warn_proprietary_attributes: bool,
    pub skip_nested: bool,
}

/// What a run does with the document.
#[derive(Clone, Debug)]
pub struct RunOptions {
    pub force_output: bool,
    pub make_clean: bool,
    pub replace_color: bool,
}

/// How the document is laid out.
#[derive(Clone, Debug)]
pub struct LayoutOptions {
    pub omit_optional_tags: bool,
    pub keep_tabs: bool,
}

/// The options that affect what itidy reports, with tidy's defaults.
#[derive(Clone, Debug)]
pub struct Options {
    pub alt_text: Option<String>,
    pub markup: MarkupOptions,
    pub custom_tags: CustomTags,
    pub new_blocklevel_tags: Vec<String>,
    pub new_inline_tags: Vec<String>,
    pub new_empty_tags: Vec<String>,
    pub new_pre_tags: Vec<String>,
    pub doctype_mode: DoctypeMode,
    pub doctype_user: Option<String>,
    pub drop: DropOptions,
    pub attributes: AttributeOptions,
    pub enclose: EncloseOptions,
    pub fix: FixOptions,
    pub fix_bad_comments: TriState,
    pub merge: MergeOptions,
    pub meta: MetaOptions,
    pub entities: EntityOptions,
    pub output: OutputOptions,
    pub show_errors: u32,
    pub report: ReportOptions,
    pub strictness: StrictnessOptions,
    pub tab_size: u32,
    pub layout: LayoutOptions,
    pub uppercase_attributes: UppercaseAttrs,
    pub show_body_only: TriState,
    pub run: RunOptions,
}

impl Options {
    /// The defaults with only the reports shown, as `tidy -errors -q` does:
    /// not the chatter.
    pub fn quiet() -> Self {
        let mut opts = Self::default();
        opts.report.quiet = true;
        opts
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            alt_text: None,
            markup: MarkupOptions {
                anchor_as_name: true,
                coerce_endtags: true,
                decorate_inferred_ul: false,
            },
            custom_tags: CustomTags::No,
            new_blocklevel_tags: Vec::new(),
            new_inline_tags: Vec::new(),
            new_empty_tags: Vec::new(),
            new_pre_tags: Vec::new(),
            doctype_mode: DoctypeMode::Auto,
            doctype_user: None,
            drop: DropOptions {
                drop_empty_elements: true,
                drop_empty_paras: true,
                drop_proprietary_attributes: false,
            },
            attributes: AttributeOptions {
                literal_attributes: false,
                lower_literals: true,
                keep_last_attribute: true,
            },
            enclose: EncloseOptions {
                enclose_block_text: false,
                enclose_text: false,
                escape_scripts: true,
            },
            fix: FixOptions {
                fix_backslash: true,
                fix_uri: true,
                fix_style_tags: true,
            },
            fix_bad_comments: TriState::Auto,
            merge: MergeOptions {
                join_classes: false,
                join_styles: true,
                merge_emphasis: true,
            },
            meta: MetaOptions {
                add_meta_charset: false,
                show_meta_change: false,
                assume_xml_procins: false,
            },
            entities: EntityOptions {
                ncr: true,
                preserve_entities: false,
                quote_ampersand: true,
            },
            output: OutputOptions {
                output_html: false,
                output_xhtml: false,
                output_xml: false,
            },
            show_errors: 6,
            report: ReportOptions {
                show_info: true,
                show_warnings: true,
                quiet: false,
            },
            strictness: StrictnessOptions {
                strict_tags_attributes: false,
                warn_proprietary_attributes: true,
                skip_nested: true,
            },
            tab_size: 8,
            layout: LayoutOptions {
                omit_optional_tags: false,
                keep_tabs: false,
            },
            uppercase_attributes: UppercaseAttrs::No,
            show_body_only: TriState::No,
            run: RunOptions {
                force_output: false,
                make_clean: false,
                replace_color: false,
            },
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

impl Value {
    /// The boolean, false for any other kind of value.
    const fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            _ => false,
        }
    }

    /// The tri-state, `No` for any other kind of value.
    const fn as_tri(&self) -> TriState {
        match self {
            Self::Tri(t) => *t,
            _ => TriState::No,
        }
    }

    /// The integer, 0 for any other kind of value.
    const fn as_int(&self) -> u32 {
        match self {
            Self::Int(i) => *i,
            _ => 0,
        }
    }

    /// The list, empty for any other kind of value.
    fn as_list(&self) -> Vec<String> {
        match self {
            Self::List(l) => l.clone(),
            _ => Vec::new(),
        }
    }

    /// The string, empty for any other kind of value.
    fn as_str(&self) -> String {
        match self {
            Self::Str(s) => s.clone(),
            _ => String::new(),
        }
    }
}

/// The error for an option value itidy accepts but cannot act on.
fn unsupported(option: &str, what: &str) -> Result<(), String> {
    Err(format!(
        "option \"{option}\" set to {what} is not supported by itidy"
    ))
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
        match option {
            "alt-text" => {
                let s = parsed.as_str();
                self.alt_text = if s.is_empty() { None } else { Some(s) };
            }
            "anchor-as-name" => self.markup.anchor_as_name = parsed.as_bool(),
            "coerce-endtags" => self.markup.coerce_endtags = parsed.as_bool(),
            "custom-tags" => {
                self.custom_tags = match parsed {
                    Value::Pick(0) => CustomTags::No,
                    Value::Pick(1) => CustomTags::Blocklevel,
                    Value::Pick(2) => CustomTags::Empty,
                    Value::Pick(3) => CustomTags::Inline,
                    _ => CustomTags::Pre,
                }
            }
            "new-blocklevel-tags" => self.new_blocklevel_tags = parsed.as_list(),
            "new-inline-tags" => self.new_inline_tags = parsed.as_list(),
            "new-empty-tags" => self.new_empty_tags = parsed.as_list(),
            "new-pre-tags" => self.new_pre_tags = parsed.as_list(),
            "new-custom-tags" | "mute" => {
                if !parsed.as_list().is_empty() {
                    return unsupported(option, "a list");
                }
            }
            "decorate-inferred-ul" => self.markup.decorate_inferred_ul = parsed.as_bool(),
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
            "drop-empty-elements" => self.drop.drop_empty_elements = parsed.as_bool(),
            "drop-empty-paras" => self.drop.drop_empty_paras = parsed.as_bool(),
            "drop-proprietary-attributes" => {
                self.drop.drop_proprietary_attributes = parsed.as_bool();
            }
            "repeated-attributes" => self.attributes.keep_last_attribute = parsed == Value::Pick(1),
            "enclose-block-text" => self.enclose.enclose_block_text = parsed.as_bool(),
            "enclose-text" => self.enclose.enclose_text = parsed.as_bool(),
            "escape-scripts" => self.enclose.escape_scripts = parsed.as_bool(),
            "fix-backslash" => self.fix.fix_backslash = parsed.as_bool(),
            "fix-bad-comments" => self.fix_bad_comments = parsed.as_tri(),
            _ => return self.set_more(option, &parsed),
        }
        Ok(())
    }

    /// The second half of `set`: the options from `fix-uri` on.
    fn set_more(&mut self, option: &str, parsed: &Value) -> Result<(), String> {
        match option {
            "fix-uri" => self.fix.fix_uri = parsed.as_bool(),
            "force-output" => self.run.force_output = parsed.as_bool(),
            "join-classes" => self.merge.join_classes = parsed.as_bool(),
            "join-styles" => self.merge.join_styles = parsed.as_bool(),
            "keep-tabs" => self.layout.keep_tabs = parsed.as_bool(),
            "literal-attributes" => self.attributes.literal_attributes = parsed.as_bool(),
            "lower-literals" => self.attributes.lower_literals = parsed.as_bool(),
            "merge-emphasis" => self.merge.merge_emphasis = parsed.as_bool(),
            "add-meta-charset" => self.meta.add_meta_charset = parsed.as_bool(),
            "show-meta-change" => self.meta.show_meta_change = parsed.as_bool(),
            "ncr" => self.entities.ncr = parsed.as_bool(),
            "omit-optional-tags" => self.layout.omit_optional_tags = parsed.as_bool(),
            "output-html" => self.output.output_html = parsed.as_bool(),
            "output-xhtml" => {
                self.output.output_xhtml = parsed.as_bool();
                if self.output.output_xhtml {
                    self.output.output_xml = true;
                }
            }
            "output-xml" => self.output.output_xml = parsed.as_bool(),
            "preserve-entities" => self.entities.preserve_entities = parsed.as_bool(),
            "quiet" => self.report.quiet = parsed.as_bool(),
            "quote-ampersand" => self.entities.quote_ampersand = parsed.as_bool(),
            "replace-color" => self.run.replace_color = parsed.as_bool(),
            "show-errors" => self.show_errors = parsed.as_int(),
            "show-info" => self.report.show_info = parsed.as_bool(),
            "show-warnings" => self.report.show_warnings = parsed.as_bool(),
            "show-body-only" => self.show_body_only = parsed.as_tri(),
            "skip-nested" => self.strictness.skip_nested = parsed.as_bool(),
            "strict-tags-attributes" => self.strictness.strict_tags_attributes = parsed.as_bool(),
            "fix-style-tags" => self.fix.fix_style_tags = parsed.as_bool(),
            "tab-size" => self.tab_size = parsed.as_int(),
            "uppercase-attributes" => {
                self.uppercase_attributes = match parsed {
                    Value::Pick(1) => UppercaseAttrs::Yes,
                    Value::Pick(2) => UppercaseAttrs::Preserve,
                    _ => UppercaseAttrs::No,
                }
            }
            "warn-proprietary-attributes" => {
                self.strictness.warn_proprietary_attributes = parsed.as_bool();
            }
            "assume-xml-procins" => self.meta.assume_xml_procins = parsed.as_bool(),
            // Options whose "yes" rewrites the document in ways itidy does
            // not follow.
            "clean" | "gdoc" | "bare" | "word-2000" | "logical-emphasis" | "input-xml" => {
                if parsed.as_bool() {
                    return unsupported(option, "yes");
                }
                if option == "clean" {
                    self.run.make_clean = false;
                }
            }
            "accessibility-check" => {
                if *parsed != Value::Pick(0) {
                    return unsupported(option, "a level above 0");
                }
            }
            "output-encoding" | "char-encoding" | "input-encoding" if parsed.as_str() != "utf8" => {
                return unsupported(option, "anything but utf8");
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

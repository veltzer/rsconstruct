//! cpplint 2.0.2, in Rust.
//!
//! A port of Google's C++ style checker as the cpplint fork ships it: the
//! same line views, nesting state and regular expressions, the same
//! messages, categories and confidence levels, the same `CPPLINT.cfg`
//! chain, `NOLINT` comments and `--filter` semantics. `lint_file` runs it
//! over one file and returns what cpplint would have printed to stderr for
//! it, one `Finding` per line.
//!
//! What is deliberately not here: the output formats other than emacs
//! (vs7, eclipse, junit, sed), `--counting` totals, `--recursive` and
//! `--exclude` (the build tool scans files itself), the "Unexpected \r"
//! warning (dead code in cpplint: Python's text mode translates line
//! endings before it looks), and the way cpplint lets one directory's
//! `linelength`/`root`/`headers`/`extensions` leak into files it processes
//! afterwards in the same run (every file here starts from the configured
//! options).

// Every check takes the error sink (`lint`) and works on a `line`; the two
// names are cpplint's own and read right next to each other. Renaming
// either across forty functions would make the port harder to compare with
// the original, so the one-letter-apart lint is off for this module.
#![allow(clippy::similar_names)]

pub mod checks;
pub mod language;
pub mod lines;
pub mod nesting;
pub mod paths;
pub mod regex;
pub mod tables;

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;

use anyhow::{Context, Result, bail};

use self::lines::CleansedLines;
use self::nesting::{FunctionState, IncludeState, NestingState};
use self::regex::{group, is_search, search, sub};
use self::tables::{
    DEFAULT_C_SUPPRESSED_CATEGORIES, DEFAULT_FILTERS, DEFAULT_HEADER_EXTENSIONS,
    DEFAULT_KERNEL_SUPPRESSED_CATEGORIES, DEFAULT_SOURCE_EXTENSIONS, LEGACY_ERROR_CATEGORIES,
    OTHER_NOLINT_CATEGORY_PREFIXES, SEARCH_C_FILE, SEARCH_KERNEL_FILE,
};

/// The line number cpplint reports as `None`: a file-level finding with no
/// line to point at (a source file with no includes that misses its own
/// header).
pub const LINE_NONE: usize = usize::MAX;

/// `--includeorder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncludeOrder {
    /// Angle-bracket includes with an extension are C system headers.
    Default,
    /// Only the known C headers are; the rest are "other system headers".
    StandardCFirst,
}

impl IncludeOrder {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "" | "default" => Ok(Self::Default),
            "standardcfirst" => Ok(Self::StandardCFirst),
            other => bail!("Invalid includeorder value {other}. Expected default|standardcfirst"),
        }
    }
}

/// cpplint's command-line options, as far as they shape the checks.
#[derive(Clone, Debug)]
pub struct Options {
    /// `--filter` entries, each `+cat` or `-cat[:file[:line]]`; an entry may
    /// hold several, comma-separated.
    pub filters: Vec<String>,
    /// `--verbose`: findings below this confidence are not reported.
    pub verbose: u32,
    /// `--linelength`.
    pub line_length: usize,
    /// `--root`.
    pub root: Option<String>,
    /// `--repository`.
    pub repository: Option<String>,
    /// `--headers`.
    pub headers: Vec<String>,
    /// `--extensions`.
    pub extensions: Vec<String>,
    /// `--includeorder`.
    pub include_order: IncludeOrder,
    /// `--config`: the name of the per-directory configuration file.
    pub config_filename: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            filters: Vec::new(),
            verbose: 1,
            line_length: 80,
            root: None,
            repository: None,
            headers: Vec::new(),
            extensions: Vec::new(),
            include_order: IncludeOrder::Default,
            config_filename: "CPPLINT.cfg".to_string(),
        }
    }
}

/// The settings in force for one file: the options after the
/// configuration-file chain has been applied.
#[derive(Clone, Debug)]
struct Settings {
    verbose: u32,
    line_length: usize,
    root: Option<String>,
    repository: Option<String>,
    hpp_headers: BTreeSet<String>,
    valid_extensions: BTreeSet<String>,
    include_order: IncludeOrder,
}

fn comma_set(value: &str) -> BTreeSet<String> {
    value.split(',').map(|s| s.trim().to_string()).collect()
}

impl Settings {
    fn from_options(options: &Options) -> Self {
        Self {
            verbose: options.verbose,
            line_length: options.line_length,
            root: options.root.clone(),
            repository: options.repository.clone(),
            hpp_headers: options
                .headers
                .iter()
                .map(|s| s.trim().to_string())
                .collect(),
            valid_extensions: options
                .extensions
                .iter()
                .map(|s| s.trim().to_string())
                .collect(),
            include_order: options.include_order,
        }
    }

    /// `GetHeaderExtensions`.
    fn header_extensions(&self) -> BTreeSet<String> {
        if !self.hpp_headers.is_empty() {
            return self.hpp_headers.clone();
        }
        if !self.valid_extensions.is_empty() {
            return self
                .valid_extensions
                .iter()
                .filter(|h| h.contains('h'))
                .cloned()
                .collect();
        }
        DEFAULT_HEADER_EXTENSIONS
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }

    /// `GetAllExtensions`.
    fn all_extensions(&self) -> BTreeSet<String> {
        let mut all = self.header_extensions();
        if self.valid_extensions.is_empty() {
            all.extend(DEFAULT_SOURCE_EXTENSIONS.iter().map(|s| (*s).to_string()));
        } else {
            all.extend(self.valid_extensions.iter().cloned());
        }
        all
    }

    /// `GetNonHeaderExtensions`.
    fn non_header_extensions(&self) -> BTreeSet<String> {
        let headers = self.header_extensions();
        self.all_extensions()
            .into_iter()
            .filter(|e| !headers.contains(e))
            .collect()
    }
}

/// One line cpplint would have printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// 1-based line number; 0 for file-level findings, `LINE_NONE` for
    /// cpplint's `None`.
    pub line: usize,
    pub category: &'static str,
    pub confidence: u32,
    pub message: String,
}

impl Finding {
    /// cpplint's emacs-format line.
    pub fn render(&self, filename: &str) -> String {
        let line = if self.line == LINE_NONE {
            "None".to_string()
        } else {
            self.line.to_string()
        };
        format!(
            "{filename}:{line}:  {}  [{}] [{}]",
            self.message, self.category, self.confidence
        )
    }
}

#[derive(Clone, Copy, Debug)]
struct LineRange {
    begin: usize,
    end: usize,
}

impl LineRange {
    const fn contains(self, line: usize) -> bool {
        self.begin <= line && line <= self.end
    }

    const fn contains_range(self, other: Self) -> bool {
        self.begin <= other.begin && self.end >= other.end
    }
}

/// A suppression: its own range, or a shared `NOLINTBEGIN` block whose end
/// is filled in when `NOLINTEND` arrives.
#[derive(Clone, Copy, Debug)]
enum Suppression {
    Range(LineRange),
    Block(usize),
}

const fn resolve(blocks: &[LineRange], suppression: Suppression) -> LineRange {
    match suppression {
        Suppression::Range(range) => range,
        Suppression::Block(index) => blocks[index],
    }
}

/// cpplint's `ErrorSuppressions`.
#[derive(Default)]
struct ErrorSuppressions {
    /// Per category (`None`: every category), the suppressed ranges.
    suppressions: HashMap<Option<&'static str>, Vec<Suppression>>,
    blocks: Vec<LineRange>,
    open_block: Option<usize>,
}

impl ErrorSuppressions {
    fn add(&mut self, category: Option<&'static str>, suppression: Suppression) {
        let range = resolve(&self.blocks, suppression);
        let blocks = &self.blocks;
        let list = self.suppressions.entry(category).or_default();
        if !list
            .last()
            .is_some_and(|last| resolve(blocks, *last).contains_range(range))
        {
            list.push(suppression);
        }
    }

    fn open_block_start(&self) -> Option<usize> {
        self.open_block.map(|i| self.blocks[i].begin)
    }

    fn add_global(&mut self, category: &'static str) {
        self.add(
            Some(category),
            Suppression::Range(LineRange {
                begin: 0,
                end: usize::MAX,
            }),
        );
    }

    fn add_line(&mut self, category: Option<&'static str>, linenum: usize) {
        self.add(
            category,
            Suppression::Range(LineRange {
                begin: linenum,
                end: linenum,
            }),
        );
    }

    fn start_block(&mut self, category: Option<&'static str>, linenum: usize) {
        let index = if let Some(index) = self.open_block {
            index
        } else {
            self.blocks.push(LineRange {
                begin: linenum,
                end: usize::MAX,
            });
            let index = self.blocks.len() - 1;
            self.open_block = Some(index);
            index
        };
        self.add(category, Suppression::Block(index));
    }

    fn end_block(&mut self, linenum: usize) {
        if let Some(index) = self.open_block.take() {
            self.blocks[index].end = linenum;
        }
    }

    fn is_suppressed(&self, category: &'static str, linenum: usize) -> bool {
        let in_list = |key: Option<&'static str>| {
            self.suppressions.get(&key).is_some_and(|list| {
                list.iter()
                    .any(|s| resolve(&self.blocks, *s).contains(linenum))
            })
        };
        in_list(Some(category)) || in_list(None)
    }

    const fn has_open_block(&self) -> bool {
        self.open_block.is_some()
    }
}

/// One parsed `--filter` entry.
#[derive(Clone, Debug)]
struct Filter {
    negative: bool,
    category: String,
    file: String,
    line: Option<usize>,
}

fn parse_filters(raw: &[String]) -> Result<Vec<Filter>> {
    let mut filters = Vec::with_capacity(raw.len());
    for entry in raw {
        let Some(sign) = entry.chars().next() else {
            continue;
        };
        if sign != '+' && sign != '-' {
            bail!("Every filter in --filters must start with + or - ({entry} does not)");
        }
        let selector = &entry[1..];
        let (category, file, line) = match selector.find(':') {
            None => (selector.to_string(), String::new(), None),
            Some(colon) => {
                let category = selector[..colon].to_string();
                let rest = &selector[colon + 1..];
                match rest.find(':') {
                    None => (category, rest.to_string(), None),
                    Some(second) => {
                        let line: usize = rest[second + 1..]
                            .parse()
                            .with_context(|| format!("Invalid filter line number in {entry}"))?;
                        (category, rest[..second].to_string(), Some(line))
                    }
                }
            }
        };
        filters.push(Filter {
            negative: sign == '-',
            category,
            file,
            line,
        });
    }
    Ok(filters)
}

/// `_CppLintState.AddFilters`: comma-separated filters appended.
fn add_filters(list: &mut Vec<String>, filters: &str) {
    for filt in filters.split(',') {
        let clean = filt.trim();
        if !clean.is_empty() {
            list.push(clean.to_string());
        }
    }
}

/// The per-file linting state: cpplint's module globals for one file.
pub struct Lint<'s> {
    settings: &'s Settings,
    filters: Vec<Filter>,
    filename: String,
    suppressions: ErrorSuppressions,
    /// What cpplint would have printed, in order.
    pub findings: Vec<Finding>,
}

impl Lint<'_> {
    /// `Error`: report a finding unless NOLINT, verbosity or a filter
    /// suppresses it.
    pub fn error(
        &mut self,
        linenum: usize,
        category: &'static str,
        confidence: u32,
        message: String,
    ) {
        if self.should_print(category, confidence, linenum) {
            self.findings.push(Finding {
                line: linenum,
                category,
                confidence,
                message,
            });
        }
    }

    fn should_print(&self, category: &'static str, confidence: u32, linenum: usize) -> bool {
        if self.suppressions.is_suppressed(category, linenum) {
            return false;
        }
        if confidence < self.settings.verbose {
            return false;
        }
        let mut is_filtered = false;
        for filter in &self.filters {
            let category_match = category.starts_with(&filter.category);
            let file_match = filter.file.is_empty() || filter.file == self.filename;
            let line_match = filter.line.is_none_or(|l| l == linenum);
            if category_match && file_match && line_match {
                is_filtered = filter.negative;
            }
        }
        !is_filtered
    }

    pub const fn verbose_level(&self) -> u32 {
        self.settings.verbose
    }

    pub const fn line_length(&self) -> usize {
        self.settings.line_length
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub const fn include_order(&self) -> IncludeOrder {
        self.settings.include_order
    }

    pub fn header_extensions(&self) -> BTreeSet<String> {
        self.settings.header_extensions()
    }

    pub fn non_header_extensions(&self) -> BTreeSet<String> {
        self.settings.non_header_extensions()
    }

    pub fn is_header_extension(&self, extension: &str) -> bool {
        self.settings.header_extensions().contains(extension)
    }

    /// `FileInfo(path).RepositoryName()`.
    pub fn repository_name(&self, path: &str) -> String {
        repository_name(self.settings, path)
    }

    /// `FileInfo(path).BaseName()`.
    pub fn base_name(&self, path: &str) -> String {
        file_split(self.settings, path).1
    }

    /// `FileInfo(path).Extension()`: the extension with its dot.
    pub fn file_extension_with_dot(&self, path: &str) -> String {
        file_split(self.settings, path).2
    }

    /// `ParseNolintSuppressions`.
    pub fn parse_nolint_suppressions(&mut self, raw_line: &str, linenum: usize) {
        let Some(m) = search(r"\bNOLINT(NEXTLINE|BEGIN|END)?\b(\([^)]+\))?", raw_line) else {
            return;
        };
        let kind = group(&m, 1);
        if kind == Some("BEGIN") && self.suppressions.has_open_block() {
            let start = self.suppressions.open_block_start().unwrap_or(0);
            self.error(
                linenum,
                "readability/nolint",
                5,
                format!("NONLINT block already defined on line {start}"),
            );
        }
        if kind == Some("END") && !self.suppressions.has_open_block() {
            self.error(
                linenum,
                "readability/nolint",
                5,
                "Not in a NOLINT block".to_string(),
            );
        }
        let categories = group(&m, 2);
        match categories {
            None | Some("(*)") => self.process_nolint_category(kind, None, linenum),
            Some(list) => {
                let inner = &list[1..list.len() - 1];
                let names: BTreeSet<&str> = inner.split(',').map(str::trim).collect();
                for name in names {
                    if let Some(category) = tables::category(name) {
                        self.process_nolint_category(kind, Some(category), linenum);
                    } else if OTHER_NOLINT_CATEGORY_PREFIXES
                        .iter()
                        .any(|p| name.starts_with(p))
                    {
                        // Another tool's category.
                    } else if !LEGACY_ERROR_CATEGORIES.contains(&name) {
                        self.error(
                            linenum,
                            "readability/nolint",
                            5,
                            format!("Unknown NOLINT error category: {name}"),
                        );
                    }
                }
            }
        }
    }

    fn process_nolint_category(
        &mut self,
        kind: Option<&str>,
        category: Option<&'static str>,
        linenum: usize,
    ) {
        match kind {
            Some("NEXTLINE") => self.suppressions.add_line(category, linenum + 1),
            Some("BEGIN") => self.suppressions.start_block(category, linenum),
            Some("END") => {
                if let Some(category) = category {
                    self.error(
                        linenum,
                        "readability/nolint",
                        5,
                        format!("NOLINT categories not supported in block END: {category}"),
                    );
                }
                self.suppressions.end_block(linenum);
            }
            _ => self.suppressions.add_line(category, linenum),
        }
    }

    /// `ProcessGlobalSuppressions`.
    fn process_global_suppressions(&mut self, lines: &[String]) {
        // filename.lower().endswith((".c", ".cu"))
        let c_file = std::path::Path::new(&self.filename)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("c") || ext.eq_ignore_ascii_case("cu"));
        for line in lines {
            if c_file || is_search(SEARCH_C_FILE, line) {
                for category in DEFAULT_C_SUPPRESSED_CATEGORIES {
                    self.suppressions.add_global(category);
                }
            }
            if is_search(SEARCH_KERNEL_FILE, line) {
                for category in DEFAULT_KERNEL_SUPPRESSED_CATEGORIES {
                    self.suppressions.add_global(category);
                }
            }
        }
    }
}

fn vcs_root_marker(dir: &str) -> bool {
    paths::exists(&paths::join(dir, ".git"))
        || paths::exists(&paths::join(dir, ".hg"))
        || paths::exists(&paths::join(dir, ".svn"))
}

/// `FileInfo.RepositoryName`: the path relative to the repository root
/// (the nearest directory with `.git`, `.hg` or `.svn`, or `--repository`),
/// or the absolute path when there is none.
fn repository_name(settings: &Settings, filename: &str) -> String {
    let fullname = paths::abspath(filename);
    if !paths::exists(&fullname) {
        return fullname;
    }
    let project_dir = paths::dirname(&fullname).to_string();
    if let Some(repository) = &settings.repository {
        let repo = paths::abspath(repository);
        let mut root_dir = project_dir.as_str();
        while paths::exists(root_dir) {
            if root_dir == repo {
                return paths::relpath(&fullname, root_dir);
            }
            let one_up = paths::dirname(root_dir);
            if one_up == root_dir {
                break;
            }
            root_dir = one_up;
        }
    }
    if paths::exists(&paths::join(&project_dir, ".svn")) {
        let mut root_dir = project_dir.clone();
        let mut one_up = paths::dirname(&root_dir).to_string();
        while paths::exists(&paths::join(&one_up, ".svn")) {
            root_dir = paths::dirname(&root_dir).to_string();
            one_up = paths::dirname(&one_up).to_string();
        }
        let prefix = paths::commonprefix(&root_dir, &project_dir).len();
        return lines::from(&fullname, prefix + 1).to_string();
    }
    let mut root_dir = project_dir.as_str();
    let mut current_dir = project_dir.as_str();
    while current_dir != paths::dirname(current_dir) {
        if vcs_root_marker(current_dir) {
            root_dir = current_dir;
            break;
        }
        current_dir = paths::dirname(current_dir);
    }
    if vcs_root_marker(root_dir) {
        let prefix = paths::commonprefix(root_dir, &project_dir).len();
        return lines::from(&fullname, prefix + 1).to_string();
    }
    fullname
}

/// `FileInfo.Split`: (directory, basename, extension with dot) of the
/// repository-relative name.
fn file_split(settings: &Settings, filename: &str) -> (String, String, String) {
    let googlename = repository_name(settings, filename);
    let (project, rest) = paths::split(&googlename);
    let (base, ext) = paths::splitext(rest);
    (project.to_string(), base.to_string(), ext.to_string())
}

fn strip_list_prefix(list: &[String], prefix: &[String]) -> Option<Vec<String>> {
    if list.len() < prefix.len() || list[..prefix.len()] != *prefix {
        return None;
    }
    Some(list[prefix.len()..].to_vec())
}

/// `GetHeaderGuardCPPVariable`.
fn get_header_guard_cpp_variable(settings: &Settings, filename: &str) -> String {
    let filename = sub(r"_flymake\.h$", ".h", filename);
    let filename = sub(r"/\.flymake/([^/]*)$", "/${1}", &filename);
    let filename = filename.replace("C++", "cpp").replace("c++", "cpp");
    let file_path_from_root = repository_name(settings, &filename);
    let fixed = match &settings.root {
        None => file_path_from_root,
        Some(root) => {
            let stripped = strip_list_prefix(
                &paths::split_to_list(&file_path_from_root),
                &paths::split_to_list(root),
            );
            if let Some(rest) = stripped.filter(|r| !r.is_empty()) {
                paths::join_all(&rest)
            } else {
                let full_path = paths::abspath(&filename);
                let root_abspath = paths::abspath(root);
                let stripped = strip_list_prefix(
                    &paths::split_to_list(&full_path),
                    &paths::split_to_list(&root_abspath),
                );
                if let Some(rest) = stripped.filter(|r| !r.is_empty()) {
                    paths::join_all(&rest)
                } else {
                    file_path_from_root
                }
            }
        }
    };
    let mut guard = sub("[^a-zA-Z0-9]", "_", &fixed).to_uppercase();
    guard.push('_');
    guard
}

/// What the configuration-file chain decided for one file.
struct Overrides {
    settings: Settings,
    cfg_filters: Vec<String>,
    /// An `exclude_files` pattern matched a component of the file's path.
    excluded: bool,
}

/// `ProcessConfigOverrides`: walk up from the file's directory reading
/// every configuration file (until `set noparent`).
fn process_config_overrides(options: &Options, filename: &str) -> Result<Overrides> {
    let mut settings = Settings::from_options(options);
    let mut cfg_filters: Vec<String> = Vec::new();
    let mut abs_filename = paths::abspath(filename);
    let mut keep_looking = true;
    while keep_looking {
        let (abs_path, base_name) = {
            let (head, tail) = paths::split(&abs_filename);
            (head.to_string(), tail.to_string())
        };
        if base_name.is_empty() {
            break;
        }
        let cfg_file = paths::join(&abs_path, &options.config_filename);
        abs_filename = abs_path;
        if !paths::isfile(&cfg_file) {
            continue;
        }
        let bytes = std::fs::read(&cfg_file)
            .with_context(|| format!("Failed to read cpplint config file {cfg_file}"))?;
        let text = String::from_utf8_lossy(&bytes);
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("");
            if line.trim().is_empty() {
                continue;
            }
            let (name, val) = match line.find('=') {
                Some(eq) => (line[..eq].trim(), line[eq + 1..].trim()),
                None => (line.trim(), ""),
            };
            match name {
                "set noparent" => keep_looking = false,
                "filter" => cfg_filters.push(val.to_string()),
                "exclude_files" => {
                    let pattern =
                        fancy_regex::Regex::new(&format!("^(?:{val})")).with_context(|| {
                            format!("{cfg_file}: exclude_files pattern {val:?} does not compile")
                        })?;
                    let matched = pattern.is_match(&base_name).with_context(|| {
                        format!("{cfg_file}: exclude_files pattern {val:?} failed")
                    })?;
                    if matched {
                        return Ok(Overrides {
                            settings,
                            cfg_filters,
                            excluded: true,
                        });
                    }
                }
                "linelength" => {
                    settings.line_length = val
                        .parse()
                        .with_context(|| format!("{cfg_file}: Line length must be numeric."))?;
                }
                "extensions" => settings.valid_extensions = comma_set(val),
                "root" => settings.root = Some(paths::join(paths::dirname(&cfg_file), val)),
                "headers" => settings.hpp_headers = comma_set(val),
                "includeorder" => {
                    // cpplint leaves the setting alone for "default".
                    if IncludeOrder::parse(val).with_context(|| cfg_file.clone())?
                        == IncludeOrder::StandardCFirst
                    {
                        settings.include_order = IncludeOrder::StandardCFirst;
                    }
                }
                other => bail!("Invalid configuration option ({other}) in file {cfg_file}"),
            }
        }
    }
    Ok(Overrides {
        settings,
        cfg_filters,
        excluded: false,
    })
}

/// The result of linting one file.
#[derive(Debug)]
pub enum Outcome {
    /// A configuration file's `exclude_files` matched: cpplint skips the
    /// file ("Ignoring ...: file excluded by ...") and it counts as clean.
    Excluded,
    /// The findings cpplint would print, in order (empty: clean).
    Linted(Vec<Finding>),
}

/// `ProcessFile`: lint one file as cpplint would when given `filename` on
/// its command line (the name is used as given, for messages, filters and
/// the repository-relative path).
pub fn lint_file(filename: &str, options: &Options) -> Result<Outcome> {
    let overrides = process_config_overrides(options, filename)?;
    if overrides.excluded {
        return Ok(Outcome::Excluded);
    }
    let settings = overrides.settings;
    let mut raw_filters: Vec<String> = DEFAULT_FILTERS.iter().map(|s| (*s).to_string()).collect();
    for entry in &options.filters {
        add_filters(&mut raw_filters, entry);
    }
    for entry in overrides.cfg_filters.iter().rev() {
        add_filters(&mut raw_filters, entry);
    }
    let filters = parse_filters(&raw_filters)?;
    let bytes = std::fs::read(filename).with_context(|| format!("Failed to read {filename}"))?;
    // cpplint opens the file in Python text mode, so universal newlines
    // apply: "\r\n" and a lone "\r" both arrive as "\n". Its own check for
    // "Unexpected \r (^M)" can therefore never fire, and is not ported.
    let text = String::from_utf8_lossy(&bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let file_extension = filename
        .rfind('.')
        .map_or(filename, |dot| &filename[dot + 1..]);
    let all_extensions = settings.all_extensions();
    if !all_extensions.contains(file_extension) {
        let mut listed = String::new();
        for (i, ext) in all_extensions.iter().enumerate() {
            if i > 0 {
                listed.push_str(", ");
            }
            let _ = write!(listed, "{ext}");
        }
        bail!("Ignoring {filename}; not a valid file name ({listed})");
    }
    let mut lint = Lint {
        settings: &settings,
        filters,
        filename: filename.to_string(),
        suppressions: ErrorSuppressions::default(),
        findings: Vec::new(),
    };
    process_file_data(&mut lint, file_extension, lines);
    Ok(Outcome::Linted(lint.findings))
}

/// `ProcessFileData`.
fn process_file_data(lint: &mut Lint<'_>, file_extension: &str, lines: Vec<String>) {
    let mut lines = lines;
    lines.insert(
        0,
        "// marker so line numbers and indices both start at 1".to_string(),
    );
    lines.push("// marker so line numbers end in a known way".to_string());
    let mut include_state = IncludeState::new();
    let mut function_state = FunctionState::new();
    let mut nesting_state = NestingState::new();
    checks::check_for_copyright(lint, &lines);
    lint.process_global_suppressions(&lines);
    lines::remove_multi_line_comments(lint, &mut lines);
    let replace_alt_tokens = lint.filters.iter().any(|f| {
        f.negative
            && f.category == "readability/alt_tokens"
            && f.file.is_empty()
            && f.line.is_none()
    });
    let cl = CleansedLines::new(lines, replace_alt_tokens);
    let cppvar = if lint.is_header_extension(file_extension) {
        let cppvar = get_header_guard_cpp_variable(lint.settings, &lint.filename);
        checks::check_for_header_guard(lint, &cl, &cppvar);
        Some(cppvar)
    } else {
        None
    };
    for line in 0..cl.num_lines {
        process_line(
            lint,
            file_extension,
            &cl,
            line,
            &mut include_state,
            &mut function_state,
            &mut nesting_state,
            cppvar.as_deref(),
        );
        checks::flag_cxx_headers(lint, &cl, line);
    }
    if lint.suppressions.has_open_block() {
        let start = lint.suppressions.open_block_start().unwrap_or(0);
        lint.error(
            start,
            "readability/nolint",
            5,
            "NONLINT block never ended".to_string(),
        );
    }
    language::check_for_include_what_you_use(lint, &cl, &include_state);
    if lint.non_header_extensions().contains(file_extension) {
        language::check_header_file_included(lint, &include_state);
    }
    checks::check_for_bad_characters(lint, &cl.raw_lines);
    checks::check_for_newline_at_eof(lint, &cl.raw_lines);
}

/// `ProcessLine`.
#[allow(clippy::too_many_arguments)]
fn process_line(
    lint: &mut Lint<'_>,
    file_extension: &str,
    cl: &CleansedLines,
    line: usize,
    include_state: &mut IncludeState,
    function_state: &mut FunctionState,
    nesting_state: &mut NestingState,
    cppvar: Option<&str>,
) {
    lint.parse_nolint_suppressions(&cl.raw_lines[line], line);
    nesting_state.update(lint, cl, line);
    checks::check_for_namespace_indentation(lint, nesting_state, cl, line);
    if nesting_state.in_asm_block() {
        return;
    }
    checks::check_for_function_lengths(lint, cl, line, function_state);
    checks::check_for_multiline_comments_and_strings(lint, cl, line);
    checks::check_style(lint, cl, line, file_extension, nesting_state, cppvar);
    language::check_language(lint, cl, line, file_extension, include_state);
    language::check_for_non_const_reference(lint, cl, line, nesting_state);
    checks::check_for_non_standard_constructs(lint, cl, line, nesting_state);
    checks::check_vlog_arguments(lint, cl, line);
    checks::check_posix_threading(lint, cl, line);
    checks::check_invalid_increment(lint, cl, line);
    checks::check_make_pair_uses_deduction(lint, cl, line);
    checks::check_redundant_virtual(lint, cl, line);
    checks::check_redundant_override_or_final(lint, cl, line);
}

#[cfg(test)]
mod tests;

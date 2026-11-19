use std::collections::HashMap;
use std::fmt;

use toml_edit::{Document, Item, Table};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldProvenance {
    UserToml {
        line: usize,
    },
    ProcessorDefault,
    ScanDefault,
    OutputDirDefault,
    SerdeDefault,
    CliOverride,
    /// `enabled` was turned off at load because the directory named by
    /// `enabled_if_dir_exists` does not exist on this machine.
    DirMissing {
        dir: String,
    },
}

impl fmt::Display for FieldProvenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UserToml { line } => write!(f, "from rsconstruct.toml:{line}"),
            Self::ProcessorDefault => write!(f, "processor default"),
            Self::ScanDefault => write!(f, "scan default"),
            Self::OutputDirDefault => write!(f, "output dir default"),
            Self::SerdeDefault => write!(f, "serde default"),
            Self::CliOverride => write!(f, "CLI override"),
            Self::DirMissing { dir } => write!(f, "off: {dir} does not exist"),
        }
    }
}

pub type ProvenanceMap = HashMap<String, FieldProvenance>;

pub fn record_if_absent(map: &mut ProvenanceMap, field: &str, source: FieldProvenance) {
    if !map.contains_key(field) {
        map.insert(field.to_string(), source);
    }
}

/// Which config section a user-set field lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Processor,
    Analyzer,
}

/// A map from (section, `instance_name`, field) to the line number in the user's
/// rsconstruct.toml where the field was set. Populated by walking a
/// [`DocumentMut`] once at config load time.
///
/// `instance_name` is what ProcessorInstance/AnalyzerInstance use: "ruff" for
/// single-instance, "pylint.core" for multi-instance.
pub type SpanMap = HashMap<(Section, String, String), usize>;

/// Per-section field line numbers for top-level sections like `[build]`,
/// `[cache]`. Outer key = section name; inner key = field name; value = 1-based
/// line number.
pub type GlobalSpanMap = HashMap<String, HashMap<String, usize>>;

/// Parse the user TOML source and build a map of field-level line numbers for
/// every key under `[processor.*]` and `[analyzer.*]`. Returns an empty map if
/// parsing fails (the caller already ran the `toml` crate's parser and
/// validated the config, so we treat span capture as best-effort enrichment).
/// Both span maps from a single parse. They are always wanted together — the
/// two used to be separate entry points that each re-parsed the same source.
///
/// `Document` preserves byte spans on every `Key` and `Item`. `DocumentMut`
/// strips them on construction (`despan`), so we can't use it here.
pub fn build_span_maps(source: &str) -> (SpanMap, GlobalSpanMap) {
    match Document::parse(source) {
        Ok(doc) => (span_map_of(&doc, source), global_span_map_of(&doc, source)),
        Err(_) => (SpanMap::new(), GlobalSpanMap::new()),
    }
}

fn span_map_of(doc: &Document<&str>, source: &str) -> SpanMap {
    let mut map = SpanMap::new();
    let root = doc.as_table();
    if let Some(item) = root.get("processor")
        && let Some(table) = item.as_table()
    {
        // Processors nest one level deeper than analyzers: `[processor]` →
        // type → name → fields, and the instance name carries the whole
        // path (`processor.checker.ruff`).
        for (type_name, type_item) in table {
            let Some(type_table) = type_item.as_table() else {
                continue;
            };
            let prefix = format!("processor.{type_name}.");
            walk_instance_section(type_table, Section::Processor, &prefix, source, &mut map);
        }
    }
    if let Some(item) = root.get("analyzer")
        && let Some(table) = item.as_table()
    {
        walk_instance_section(table, Section::Analyzer, "", source, &mut map);
    }
    map
}

/// Walk a table of instances — each child is either a single instance
/// (direct config fields) or a multi-instance container (each grand-child is an
/// instance). `prefix` is prepended to every instance name recorded.
fn walk_instance_section(
    table: &Table,
    section: Section,
    prefix: &str,
    source: &str,
    map: &mut SpanMap,
) {
    for (type_name, item) in table {
        let Some(sub) = item.as_table() else { continue };
        // Heuristic matches ProcessorConfig::is_multi_instance's shape: if
        // every child is itself a table, this section holds named
        // sub-instances; otherwise it's a single instance with direct fields.
        let all_children_are_tables = !sub.is_empty() && sub.iter().all(|(_, v)| v.is_table());

        if all_children_are_tables {
            // Multi-instance: [processor.checker.pylint.core], [processor.checker.pylint.tests]
            for (inst_suffix, inst_item) in sub {
                if let Some(inst_table) = inst_item.as_table() {
                    let instance_name = format!("{prefix}{type_name}.{inst_suffix}");
                    record_field_lines(inst_table, section, &instance_name, source, map);
                }
            }
        } else {
            // Single instance
            let instance_name = format!("{prefix}{type_name}");
            record_field_lines(sub, section, &instance_name, source, map);
        }
    }
}

fn record_field_lines(
    table: &Table,
    section: Section,
    instance_name: &str,
    source: &str,
    map: &mut SpanMap,
) {
    for (key, item) in table {
        let span = key_span(table, key).or_else(|| item_span(item));
        if let Some(range) = span {
            let line = byte_offset_to_line(source, range.start);
            map.insert((section, instance_name.to_string(), key.to_string()), line);
        }
    }
}

fn key_span(table: &Table, key: &str) -> Option<std::ops::Range<usize>> {
    table.key(key).and_then(toml_edit::Key::span)
}

fn item_span(item: &Item) -> Option<std::ops::Range<usize>> {
    item.span()
}

/// Collect line numbers for every field in top-level `[section]` tables
/// other than `processor` and `analyzer` (handled by [`span_map_of`]).
fn global_span_map_of(doc: &Document<&str>, source: &str) -> GlobalSpanMap {
    let mut map = GlobalSpanMap::new();
    for (section_name, item) in doc.as_table() {
        if section_name == "processor" || section_name == "analyzer" {
            continue;
        }
        let Some(table) = item.as_table() else {
            continue;
        };
        let mut fields = HashMap::new();
        for (key, field_item) in table {
            let span = key_span(table, key).or_else(|| item_span(field_item));
            if let Some(range) = span {
                fields.insert(key.to_string(), byte_offset_to_line(source, range.start));
            }
        }
        if !fields.is_empty() {
            map.insert(section_name.to_string(), fields);
        }
    }
    map
}

/// One config file as it was loaded.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// The path as it is shown to the user.
    pub name: String,
    /// The file's text after `[vars]` substitution, which keeps every line
    /// where it was: a line number here is a line number in the file.
    pub content: String,
    /// The user config: only its `[build]` table is merged, and it may hold
    /// a `[tables]` section the project files may not.
    pub build_only: bool,
}

/// The files a config was merged from, highest precedence first, so the
/// first file that has a key is the one whose value won the merge.
///
/// An error found in the merged value is found without a position, and the
/// key may come from any of the files; this is what turns the key's path
/// back into the `file:line` the user has to edit.
#[derive(Debug, Clone, Default)]
pub struct SourceFiles(Vec<SourceFile>);

impl SourceFiles {
    pub const fn new(files: Vec<SourceFile>) -> Self {
        Self(files)
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.0
    }

    /// `file:line` of the key at `path` (`["build", "parallel"]`,
    /// `["processor", "checker", "ruff", "args"]`). When no file has the
    /// key — a required field that is missing — the nearest enclosing
    /// section some file has is given instead. `None` only when not even
    /// the first segment is in any file.
    pub fn locate(&self, path: &[String]) -> Option<String> {
        let docs: Vec<(&SourceFile, Document<&str>)> = self
            .0
            .iter()
            .filter_map(|file| {
                Document::parse(file.content.as_str())
                    .ok()
                    .map(|doc| (file, doc))
            })
            .collect();
        for len in (1..=path.len()).rev() {
            for (file, doc) in &docs {
                if let Some(offset) = offset_of(doc.as_table(), &path[..len]) {
                    let line = byte_offset_to_line(&file.content, offset);
                    return Some(format!("{}:{line}", file.name));
                }
            }
        }
        None
    }
}

/// Byte offset of the key at `path` under `root`, when it is there.
fn offset_of(root: &Table, path: &[String]) -> Option<usize> {
    let (last, parents) = path.split_last()?;
    let mut table: &dyn toml_edit::TableLike = root;
    for segment in parents {
        table = table.get(segment)?.as_table_like()?;
    }
    let item = table.get(last)?;
    // A `[section]` is found by its header and a `key = value` line by its
    // key. A section that exists only as the prefix of deeper headers
    // (`[processor.checker]` when the file has `[processor.checker.ruff]`)
    // has neither, and is where its first descendant is.
    item.as_table()
        .and_then(Table::span)
        .or_else(|| table.key(last).and_then(toml_edit::Key::span))
        .map(|span| span.start)
        .or_else(|| first_offset(item))
}

/// The smallest byte offset of anything in `item` that has a position.
fn first_offset(item: &Item) -> Option<usize> {
    if let Some(span) = item.span() {
        return Some(span.start);
    }
    let table = item.as_table()?;
    table
        .iter()
        .filter_map(|(key, child)| {
            key_span(table, key)
                .map(|span| span.start)
                .or_else(|| first_offset(child))
        })
        .min()
}

/// Convert a byte offset into a 1-based line number, counting `\n` bytes.
fn byte_offset_to_line(source: &str, offset: usize) -> usize {
    let clamped = offset.min(source.len());
    let mut line = 1usize;
    for b in &source.as_bytes()[..clamped] {
        if *b == b'\n' {
            line += 1;
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_capture_instance_fields() {
        let src = r#"[build]
output_dir = "build"
parallel = 4

[processor.checker.ruff]
src_dirs = ["src"]
args = ["--fix"]

[processor.checker.pylint.core]
src_dirs = ["src/core"]

[analyzer.tera]
strict = true
"#;
        let (spans, _) = build_span_maps(src);
        let key = (
            Section::Processor,
            "processor.checker.ruff".to_string(),
            "args".to_string(),
        );
        assert_eq!(spans.get(&key), Some(&7), "span map: {spans:?}");
        let key = (
            Section::Processor,
            "processor.checker.pylint.core".to_string(),
            "src_dirs".to_string(),
        );
        assert_eq!(spans.get(&key), Some(&10), "span map: {spans:?}");
        let key = (Section::Analyzer, "tera".to_string(), "strict".to_string());
        assert_eq!(spans.get(&key), Some(&13), "span map: {spans:?}");
    }

    fn path(segments: &[&str]) -> Vec<String> {
        segments.iter().map(ToString::to_string).collect()
    }

    fn source(name: &str, content: &str) -> SourceFile {
        SourceFile {
            name: name.to_string(),
            content: content.to_string(),
            build_only: false,
        }
    }

    /// A key is where the highest-precedence file that has it puts it; a
    /// key no file has is at its nearest enclosing section.
    #[test]
    fn locate_finds_keys_sections_and_the_winning_file() {
        let main = "[build]\nparallel = 4\n\n[processor.checker.ruff]\nargs = [\"--fix\"]\n\n[processor.checker.pylint.core]\nsrc_dirs = [\"src\"]\n";
        let user = "# user\n[build]\nparallel = 2\nbatch_size = 5\n";
        let sources = SourceFiles::new(vec![
            source("rsconstruct.toml", main),
            source("config.toml", user),
        ]);
        let at = |segments: &[&str]| sources.locate(&path(segments));
        // Set in both files: the project file's line, which won the merge.
        assert_eq!(
            at(&["build", "parallel"]).as_deref(),
            Some("rsconstruct.toml:2")
        );
        // Set in the user config only.
        assert_eq!(
            at(&["build", "batch_size"]).as_deref(),
            Some("config.toml:4")
        );
        assert_eq!(
            at(&["processor", "checker", "ruff", "args"]).as_deref(),
            Some("rsconstruct.toml:5")
        );
        // A missing field is at its section's header.
        assert_eq!(
            at(&["processor", "checker", "ruff", "command"]).as_deref(),
            Some("rsconstruct.toml:4")
        );
        assert_eq!(
            at(&["processor", "checker", "pylint", "core"]).as_deref(),
            Some("rsconstruct.toml:7")
        );
        // A section that is only a prefix of deeper headers is where its
        // first descendant is.
        assert_eq!(
            at(&["processor", "checker"]).as_deref(),
            Some("rsconstruct.toml:4")
        );
        assert_eq!(at(&["cache", "compression"]), None);
    }

    #[test]
    fn locate_finds_inline_and_dotted_keys() {
        let main = "build = { parallel = 4 }\nprocessor.checker.ruff.args = [\"x\"]\n";
        let sources = SourceFiles::new(vec![source("rsconstruct.toml", main)]);
        assert_eq!(
            sources.locate(&path(&["build", "parallel"])).as_deref(),
            Some("rsconstruct.toml:1")
        );
        assert_eq!(
            sources
                .locate(&path(&["processor", "checker", "ruff", "args"]))
                .as_deref(),
            Some("rsconstruct.toml:2")
        );
    }

    #[test]
    fn global_spans_capture_build_fields() {
        let src = r#"[build]
output_dir = "build"
parallel = 4
"#;
        let (_, spans) = build_span_maps(src);
        let build = spans
            .get("build")
            .expect("expected [build] section in span map");
        assert_eq!(
            build.get("parallel"),
            Some(&3),
            "expected parallel on line 3"
        );
    }
}

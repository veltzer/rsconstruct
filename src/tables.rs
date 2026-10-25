//! Centralized table rendering for CLI output.
//!
//! All tables emitted by `rsconstruct` go through this module. Changing
//! the visual style here changes every table at once, and every table is
//! a named [`TableDef`] in [`defs`], so the columns a table shows can be
//! chosen in one place too:
//!
//! 1. `--fields` prints the table's name and its column names only, no
//!    data: the list to pick from when writing the config below.
//! 2. `--all` shows every column the table has.
//! 3. `[tables] <name> = ["Col", ...]` in the user config
//!    (`~/.config/rsconstruct/config.toml`) shows exactly those columns, in
//!    that order.
//! 4. Otherwise the table's default set: every column except the ones its
//!    definition marks `verbose_only`, which `--verbose` adds back.
//!
//! Both renderers use `tabled::Style::rounded()` and always draw a
//! horizontal separator below the header row.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use tabled::Table;
use tabled::builder::Builder as TableBuilder;
use tabled::settings::Style;
use tabled::settings::style::HorizontalLine;

/// A named table: which command prints it, every column it can show, and
/// which of those are hidden unless asked for. The name is what the user
/// config and `--fields` refer to.
#[derive(Debug)]
pub struct TableDef {
    /// Config key and `--fields` label: lower-case, words joined by `_`.
    pub name: &'static str,
    /// The command that prints it, for the docs and `--fields`.
    pub command: &'static str,
    /// Every column, in display order. Empty for a table whose columns are
    /// only known at run time; `dynamic` then says what they are.
    pub columns: &'static [&'static str],
    /// Columns hidden unless `--verbose`, `--all` or the config names them.
    pub verbose_only: &'static [&'static str],
    /// For a table whose columns depend on the data (a tag matrix with one
    /// column per category): a description of them. Such a table's config
    /// entry is checked against the real columns when it is printed.
    pub dynamic: Option<&'static str>,
}

/// Every table rsconstruct prints. Adding a table means adding a definition
/// here and listing it in [`defs::ALL`], which the config validator, the
/// docs check and `--fields` all read.
pub mod defs {
    use super::TableDef;

    const fn def(
        name: &'static str,
        command: &'static str,
        columns: &'static [&'static str],
        verbose_only: &'static [&'static str],
    ) -> TableDef {
        TableDef {
            name,
            command,
            columns,
            verbose_only,
            dynamic: None,
        }
    }

    const fn dynamic(name: &'static str, command: &'static str, columns: &'static str) -> TableDef {
        TableDef {
            name,
            command,
            columns: &[],
            verbose_only: &[],
            dynamic: Some(columns),
        }
    }

    pub const ANALYZER_LIST: TableDef = def(
        "analyzer_list",
        "analyzer list",
        &["Name", "Builtin", "Description"],
        &["Description"],
    );
    pub const ANALYZER_DEFCONFIG: TableDef = def(
        "analyzer_defconfig",
        "analyzer defconfig",
        &["Field", "Type", "Default"],
        &[],
    );
    pub const ANALYZER_STATS: TableDef = def(
        "analyzer_stats",
        "analyzer stats, analyzer build",
        &["Analyzer", "Files", "Dependencies"],
        &[],
    );
    pub const ANALYZER_USED: TableDef = def(
        "analyzer_used",
        "analyzer used",
        &["Name", "Detected", "Description"],
        &["Description"],
    );
    pub const BUILD_TIMINGS: TableDef = def(
        "build_timings",
        "build --timings",
        &["Phase", "Duration"],
        &[],
    );
    pub const ERROR_LIST: TableDef = def(
        "error_list",
        "error",
        &["Code", "Name", "Description"],
        &["Description"],
    );
    pub const HOOK_LIST: TableDef = def(
        "hook_list",
        "hook",
        &["Hook", "Function", "Location", "Description"],
        &["Function", "Location"],
    );
    pub const INFO_SOURCE: TableDef =
        def("info_source", "info source", &["Extension", "Count"], &[]);
    pub const PROCESSOR_CONFIG: TableDef = def(
        "processor_config",
        "processor config",
        &["Field", "Value"],
        &[],
    );
    pub const PROCESSOR_FIELDS: TableDef = def(
        "processor_fields",
        "processor defconfig, processor info",
        &[
            "Field",
            "Type",
            "Default",
            "Required",
            "Checksum",
            "Description",
        ],
        &["Description"],
    );
    pub const PROCESSOR_LIST: TableDef = def(
        "processor_list",
        "processor list",
        &[
            "Name",
            "Type",
            "Builtin",
            "Rust",
            "Batch",
            "Cache",
            "Rust alternative",
            "Description",
        ],
        &["Description"],
    );
    pub const PROCESSOR_RECOMMEND: TableDef = def(
        "processor_recommend",
        "processor recommend",
        &["Extension / File", "Processor", "Also", "Reason"],
        &[],
    );
    pub const PROCESSOR_SEARCH: TableDef = def(
        "processor_search",
        "processor search",
        &["Name", "Type", "Description", "Keywords"],
        &[],
    );
    pub const PROCESSOR_TYPES: TableDef = def(
        "processor_types",
        "processor types",
        &["Type", "Batch", "Description", "Batching"],
        &["Batching"],
    );
    pub const PROCESSOR_USED: TableDef = def(
        "processor_used",
        "processor used",
        &["Name", "Type", "Detected", "Description"],
        &["Description"],
    );
    pub const PRODUCT_STATUS: TableDef = dynamic(
        "product_status",
        "status, build --dry-run",
        "Processor, one column per product state (up-to-date, restorable, stale, new for \
         `status`; skip, restore, build, build-new for `build --dry-run`), builtin, rust",
    );
    pub const SLOC: TableDef = def(
        "sloc",
        "sloc",
        &["Language", "Files", "Blank", "Comment", "Code"],
        &[],
    );
    pub const STATUS_BREAKDOWN: TableDef = def(
        "status_breakdown",
        "status --breakdown",
        &["Processor", "Files", "Breakdown"],
        &[],
    );
    pub const TAG_COUNT: TableDef = def("tag_count", "tag count", &["Count", "Tag"], &[]);
    pub const TAG_COVERAGE: TableDef = def(
        "tag_coverage",
        "tag coverage",
        &["Category", "Files", "Coverage"],
        &[],
    );
    pub const TAG_MATRIX: TableDef = dynamic(
        "tag_matrix",
        "tag matrix",
        "File, then one column per tag category",
    );
    pub const TAG_SUGGEST: TableDef = def("tag_suggest", "tag suggest", &["Tag", "Score"], &[]);
    pub const TOOL_LIST: TableDef = def(
        "tool_list",
        "tool list",
        &["Tool", "Status", "Runtime", "Install"],
        &["Install"],
    );
    pub const TOOL_RUNTIMES: TableDef = def(
        "tool_runtimes",
        "tool stats",
        &["Runtime", "Installed"],
        &[],
    );
    pub const TOOL_STATS: TableDef = def(
        "tool_stats",
        "tool stats",
        &["Tool", "Status", "Processors", "Install"],
        &[],
    );
    pub const WEBCACHE_LIST: TableDef = def(
        "webcache_list",
        "webcache list",
        &["URL", "Size", "Age", "State"],
        &[],
    );

    /// Every table, in name order.
    pub const ALL: &[&TableDef] = &[
        &ANALYZER_DEFCONFIG,
        &ANALYZER_LIST,
        &ANALYZER_STATS,
        &ANALYZER_USED,
        &BUILD_TIMINGS,
        &ERROR_LIST,
        &HOOK_LIST,
        &INFO_SOURCE,
        &PROCESSOR_CONFIG,
        &PROCESSOR_FIELDS,
        &PROCESSOR_LIST,
        &PROCESSOR_RECOMMEND,
        &PROCESSOR_SEARCH,
        &PROCESSOR_TYPES,
        &PROCESSOR_USED,
        &PRODUCT_STATUS,
        &SLOC,
        &STATUS_BREAKDOWN,
        &TAG_COUNT,
        &TAG_COVERAGE,
        &TAG_MATRIX,
        &TAG_SUGGEST,
        &TOOL_LIST,
        &TOOL_RUNTIMES,
        &TOOL_STATS,
        &WEBCACHE_LIST,
    ];

    /// The definition named `name`, if any.
    pub fn find(name: &str) -> Option<&'static TableDef> {
        ALL.iter().copied().find(|d| d.name == name)
    }
}

/// The user's column choice per table name, from `[tables]` in the user
/// config. Set once at startup by [`init`]; empty until then, so unit tests
/// and early-startup paths see the defaults.
static CONFIG: OnceLock<BTreeMap<String, Vec<String>>> = OnceLock::new();

/// Install the `[tables]` choices from the user config. Called once from
/// main; a second call is ignored.
pub fn init(config: BTreeMap<String, Vec<String>>) {
    let _ = CONFIG.set(config);
}

fn configured(name: &str) -> Option<&'static [String]> {
    CONFIG.get()?.get(name).map(Vec::as_slice)
}

/// Check a `[tables]` section against the table definitions: every key must
/// name a table, and for a table with fixed columns every field must be one
/// of them. A table with run-time columns is checked when it is printed.
/// Returns the problems found, one line each, empty when the section is fine.
#[must_use]
pub fn validate_config(config: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, fields) in config {
        let Some(def) = defs::find(name) else {
            errors.push(format!(
                "[tables] {name}: no table has this name; the tables are {}",
                defs::ALL
                    .iter()
                    .map(|d| d.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            continue;
        };
        if fields.is_empty() {
            errors.push(format!(
                "[tables] {name}: the field list is empty; name at least one field, or remove the entry to show the default columns"
            ));
            continue;
        }
        if def.dynamic.is_some() {
            continue;
        }
        for field in fields {
            if !def.columns.contains(&field.as_str()) {
                errors.push(format!(
                    "[tables] {name}: no field named \"{field}\"; the fields are \"{}\"",
                    def.columns.join("\", \"")
                ));
            }
        }
    }
    errors
}

fn build_table(headers: &[&str], rows: &[Vec<String>]) -> Table {
    let mut builder = TableBuilder::new();
    builder.push_record(headers.iter().copied());
    for row in rows {
        builder.push_record(row.iter().map(std::string::String::as_str));
    }
    builder.build()
}

/// The indices of the columns to show, in display order. `verbose` is the
/// caller's `--verbose` (or its own switch for the same columns), which
/// only matters when neither `--all` nor the config decides.
fn visible_columns(def: &TableDef, headers: &[&str], verbose: bool) -> Vec<usize> {
    if crate::runtime_flags::all_fields() {
        return (0..headers.len()).collect();
    }
    if let Some(fields) = configured(def.name) {
        let mut indices = Vec::new();
        for field in fields {
            match headers.iter().position(|h| h == field) {
                Some(i) => indices.push(i),
                // Only reachable for a table with run-time columns; the fixed
                // ones were checked at startup.
                None => crate::output::warn(&format!(
                    "[tables] {}: no field named \"{field}\" in this table; the fields are \"{}\"",
                    def.name,
                    headers.join("\", \"")
                )),
            }
        }
        if !indices.is_empty() {
            return indices;
        }
    }
    (0..headers.len())
        .filter(|&i| verbose || !def.verbose_only.contains(&headers[i]))
        .collect()
}

fn project(row: &[String], indices: &[usize]) -> Vec<String> {
    indices.iter().map(|&i| row[i].clone()).collect()
}

/// `--fields`: the table's name (the `[tables]` key), the command that
/// prints it, and its column names, nothing else.
fn print_fields_only(def: &TableDef, headers: &[&str]) {
    let mut table = build_table(headers, &[]);
    crate::output::info(&format!(
        "{} (rsconstruct {})\n{}",
        crate::color::bold(def.name),
        def.command,
        table.with(Style::rounded())
    ));
}

fn render(
    def: &TableDef,
    headers: &[&str],
    verbose: bool,
    rows: &[Vec<String>],
    total: Option<&[String]>,
) {
    if crate::runtime_flags::fields_only() {
        print_fields_only(def, headers);
        return;
    }
    let indices = visible_columns(def, headers, verbose);
    let headers: Vec<&str> = indices.iter().map(|&i| headers[i]).collect();
    let mut all_rows: Vec<Vec<String>> = rows.iter().map(|r| project(r, &indices)).collect();
    let Some(total) = total else {
        let mut table = build_table(&headers, &all_rows);
        // Routed through `output::info`: tables are human prose, so `--quiet`
        // silences them and `--json` keeps them out of the machine stream.
        crate::output::info(&format!("{}", table.with(Style::rounded())));
        return;
    };
    all_rows.push(project(total, &indices));
    let mut table = build_table(&headers, &all_rows);
    let n_rows = table.count_rows();
    let header_line = HorizontalLine::inherit(Style::modern());
    let total_line = HorizontalLine::inherit(Style::modern());
    let style = Style::rounded().horizontals([(1, header_line), (n_rows - 1, total_line)]);
    crate::output::info(&format!("{}", table.with(style)));
}

/// Print a table with fixed columns. Every row carries every column of
/// `def.columns`; which ones show is decided here (see the module docs).
/// `verbose` reveals the `verbose_only` columns when nothing else decides.
pub fn print(def: &TableDef, verbose: bool, rows: &[Vec<String>]) {
    debug_assert!(def.dynamic.is_none(), "{} has run-time columns", def.name);
    render(def, def.columns, verbose, rows, None);
}

/// [`print`] with a summary ("Total") row, set off by a separator.
pub fn print_with_total(def: &TableDef, verbose: bool, rows: &[Vec<String>], total: &[String]) {
    debug_assert!(def.dynamic.is_none(), "{} has run-time columns", def.name);
    render(def, def.columns, verbose, rows, Some(total));
}

/// Print a table whose columns are only known at run time (`def.dynamic`).
pub fn print_dynamic(def: &TableDef, headers: &[&str], rows: &[Vec<String>]) {
    debug_assert!(def.dynamic.is_some(), "{} has fixed columns", def.name);
    render(def, headers, true, rows, None);
}

/// [`print_dynamic`] with a summary ("Total") row.
pub fn print_dynamic_with_total(
    def: &TableDef,
    headers: &[&str],
    rows: &[Vec<String>],
    total: &[String],
) {
    debug_assert!(def.dynamic.is_some(), "{} has fixed columns", def.name);
    render(def, headers, true, rows, Some(total));
}

/// A processor name as tables print it: without the `processor.` prefix
/// every name carries (`checker.ruff`, `mass_generator.cargo.fmt`), which
/// says nothing in a column of processors and costs the width of a word on
/// every row. Only for table cells: JSON, errors and `processor info` keep
/// the full name, which is what config files and scripts use.
pub fn short_processor_name(name: &str) -> &str {
    name.strip_prefix("processor.").unwrap_or(name)
}

/// Render a boolean as "Yes" or "No" — the canonical formatting used in tables.
/// Change this in one place to rename/translate the representation everywhere.
pub const fn yes_no(b: bool) -> &'static str {
    if b { "Yes" } else { "No" }
}

/// Canonical rendering for "no value set" in tables and config dumps.
/// Use everywhere we'd otherwise emit ad-hoc strings like "(none)" or "(global)".
pub const NONE_LABEL: &str = "None";

/// Render an optional JSON value for display. `None` becomes `NONE_LABEL`;
/// `Some(v)` is JSON-serialized (strings stay quoted, arrays/objects stay compact).
pub fn opt_json(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(v) => serde_json::to_string(v).unwrap_or_default(),
        None => NONE_LABEL.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Table names are unique, listed in order, and every fixed-column
    /// table's `verbose_only` names real columns.
    #[test]
    fn definitions_are_consistent() {
        let names: Vec<&str> = defs::ALL.iter().map(|d| d.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            names, sorted,
            "defs::ALL must be sorted by name, without duplicates"
        );
        for def in defs::ALL {
            assert!(
                def.name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{}: names are lower-case words joined by _",
                def.name
            );
            assert!(
                def.columns.is_empty() == def.dynamic.is_some(),
                "{}: fixed columns or a dynamic description, not both",
                def.name
            );
            for v in def.verbose_only {
                assert!(
                    def.columns.contains(v),
                    "{}: verbose_only column {v} is not a column",
                    def.name
                );
            }
        }
    }

    #[test]
    fn validate_config_names_the_problem() {
        let mut cfg = BTreeMap::new();
        cfg.insert("nope".to_string(), vec!["Name".to_string()]);
        cfg.insert(
            "processor_list".to_string(),
            vec!["Name".to_string(), "Colour".to_string()],
        );
        cfg.insert("tool_list".to_string(), Vec::new());
        cfg.insert("tag_matrix".to_string(), vec!["anything".to_string()]);
        let errors = validate_config(&cfg);
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(errors[0].contains("no table has this name") && errors[0].contains("tool_list"));
        assert!(errors[1].contains("\"Colour\"") && errors[1].contains("\"Rust alternative\""));
        assert!(errors[2].contains("empty"));
    }

    #[test]
    fn default_hides_verbose_only_columns_and_verbose_shows_them() {
        let def = &defs::PROCESSOR_LIST;
        let quiet = visible_columns(def, def.columns, false);
        assert_eq!(quiet.len(), def.columns.len() - 1);
        assert!(!quiet.contains(&(def.columns.len() - 1)));
        let verbose = visible_columns(def, def.columns, true);
        assert_eq!(verbose.len(), def.columns.len());
    }

    /// The user config section in the docs lists every table with its
    /// command and columns, so a table added here must be documented.
    #[test]
    fn docs_list_every_table() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/src/configuration.md");
        let text = std::fs::read_to_string(&path).unwrap();
        let start = text
            .find("### `[tables]`")
            .expect("configuration.md has a [tables] section");
        let section = &text[start..];
        let end = section[4..].find("\n### ").map_or(section.len(), |i| i + 4);
        let section = &section[..end];
        let mut missing = Vec::new();
        for def in defs::ALL {
            let row = section
                .lines()
                .find(|l| l.starts_with(&format!("| `{}` |", def.name)));
            let Some(row) = row else {
                missing.push(format!("{}: no row", def.name));
                continue;
            };
            if !row.contains(def.command) {
                missing.push(format!("{}: row does not name `{}`", def.name, def.command));
            }
            for col in def.columns {
                if !row.contains(&format!("`{col}`")) {
                    missing.push(format!("{}: row does not list column `{col}`", def.name));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "docs/src/configuration.md [tables] section is out of date:\n{}",
            missing.join("\n")
        );
    }
}

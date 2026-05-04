use std::borrow::Cow;
use tabled::Table;
use tabled::builder::Builder as TableBuilder;
use tabled::settings::Style;
use tabled::settings::style::HorizontalLine;

/// Color is enabled based on the global runtime flag (--color auto|always|never).
/// Default resolution: off unless `--color always`, or `--color auto` with a tty
/// stdout and no `NO_COLOR` env var set.
fn no_color() -> bool {
    !crate::runtime_flags::color_enabled()
}

fn wrap<'a>(code: &str, text: &'a str) -> Cow<'a, str> {
    if no_color() {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(format!("\x1b[{code}m{text}\x1b[0m"))
    }
}

pub fn red(text: &str) -> Cow<'_, str> {
    wrap("31", text)
}

pub fn green(text: &str) -> Cow<'_, str> {
    wrap("32", text)
}

pub fn yellow(text: &str) -> Cow<'_, str> {
    wrap("33", text)
}

pub fn magenta(text: &str) -> Cow<'_, str> {
    wrap("35", text)
}

pub fn cyan(text: &str) -> Cow<'_, str> {
    wrap("36", text)
}

pub fn bold(text: &str) -> Cow<'_, str> {
    wrap("1", text)
}

pub fn dim(text: &str) -> Cow<'_, str> {
    wrap("2", text)
}

fn build_table(headers: &[&str], rows: &[Vec<String>]) -> Table {
    let mut builder = TableBuilder::new();
    builder.push_record(headers.iter().copied());
    for row in rows {
        builder.push_record(row.iter().map(|s| s.as_str()));
    }
    builder.build()
}

/// Print a table with an explicit header row. A horizontal separator is drawn
/// between the header and the data rows.
pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let mut table = build_table(headers, rows);
    println!("{}", table.with(Style::rounded()));
}

/// Print a table with an explicit header row and a summary ("Total") row.
/// Horizontal separators are drawn after the header and before the total row.
pub fn print_table_with_total(headers: &[&str], rows: &[Vec<String>], total: &[String]) {
    let mut all_rows: Vec<Vec<String>> = rows.to_vec();
    all_rows.push(total.to_vec());
    let mut table = build_table(headers, &all_rows);
    let n_rows = table.count_rows();
    let header_line = HorizontalLine::inherit(Style::modern());
    let total_line = HorizontalLine::inherit(Style::modern());
    let style = Style::rounded().horizontals([
        (1, header_line),
        (n_rows - 1, total_line),
    ]);
    println!("{}", table.with(style));
}

/// Render a boolean as "Yes" or "No" — the canonical formatting used in tables.
/// Change this in one place to rename/translate the representation everywhere.
pub fn yes_no(b: bool) -> &'static str {
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
        None    => NONE_LABEL.to_string(),
    }
}

use std::borrow::Cow;
use tabled::Table;
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
        Cow::Owned(format!("\x1b[{}m{}\x1b[0m", code, text))
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

/// Apply the standard rsconstruct table style and print to stdout.
pub fn print_table(mut table: Table) {
    println!("{}", table.with(Style::rounded()));
}

/// Print a table whose last row is a summary ("Total") row. A horizontal
/// separator is drawn between the data rows and the total row so the total
/// visually stands apart. Caller must have pushed the total row last.
pub fn print_table_with_total(mut table: Table) {
    let n_rows = table.count_rows();
    let style = Style::rounded().horizontals([
        (n_rows - 1, HorizontalLine::inherit(Style::modern()))
    ]);
    println!("{}", table.with(style));
}

/// Render a boolean as "Yes" or "No" — the canonical formatting used in tables.
/// Change this in one place to rename/translate the representation everywhere.
pub fn yes_no(b: bool) -> &'static str {
    if b { "Yes" } else { "No" }
}

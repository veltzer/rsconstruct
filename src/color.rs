use std::borrow::Cow;
use std::env;
use std::sync::OnceLock;
use tabled::Table;
use tabled::settings::Style;

/// Check if color output is disabled via NO_COLOR env var
fn no_color() -> bool {
    static NO_COLOR: OnceLock<bool> = OnceLock::new();
    *NO_COLOR.get_or_init(|| env::var("NO_COLOR").is_ok())
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
    println!("{}", table.with(Style::modern_rounded()));
}

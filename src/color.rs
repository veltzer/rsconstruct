use std::env;
use std::sync::OnceLock;

/// Check if color output is disabled via NO_COLOR env var
fn no_color() -> bool {
    static NO_COLOR: OnceLock<bool> = OnceLock::new();
    *NO_COLOR.get_or_init(|| env::var("NO_COLOR").is_ok())
}

fn wrap(code: &str, text: &str) -> String {
    if no_color() {
        text.to_string()
    } else {
        format!("\x1b[{}m{}\x1b[0m", code, text)
    }
}

pub fn red(text: &str) -> String {
    wrap("31", text)
}

pub fn green(text: &str) -> String {
    wrap("32", text)
}

pub fn yellow(text: &str) -> String {
    wrap("33", text)
}

pub fn cyan(text: &str) -> String {
    wrap("36", text)
}

pub fn bold(text: &str) -> String {
    wrap("1", text)
}

pub fn dim(text: &str) -> String {
    wrap("2", text)
}

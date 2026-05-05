use std::borrow::Cow;

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


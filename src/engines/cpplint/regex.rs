//! Python `re`-shaped helpers for the cpplint port.
//!
//! cpplint is written around Python regular expressions. The port keeps the
//! patterns exactly as cpplint has them (lookarounds and backreferences
//! included, hence fancy-regex) and reaches them through these few
//! functions, which give `re.match` (anchored at the start) and `re.search`
//! their Python shapes. Every pattern is compiled once and cached for the
//! life of the process; the patterns are constants in this module's code,
//! so a pattern that does not compile is a bug, not an input error.
//!
//! Matching can still fail at run time: fancy-regex gives up on a match
//! that backtracks more than its limit allows, where Python's `re` would
//! keep going. The checks call these helpers from hundreds of places that
//! return a bare match, so the failure is not threaded back through them:
//! the first one on the thread is recorded (and the call reports no match),
//! and `lint_file` collects it with `take_failure` once the file is
//! checked, failing the file with the pattern and text that tripped it. No
//! finding from a file whose matching failed is ever reported.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use fancy_regex::Regex;

/// The captures of a match over a `str`.
pub type Captures<'t> = fancy_regex::Captures<'t, str>;

static CACHE: LazyLock<Mutex<HashMap<String, &'static Regex>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

thread_local! {
    /// The first match that failed at run time on this thread, until
    /// `take_failure` collects it.
    static FAILURE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Record a match that failed at run time; the first one is kept, since
/// everything matched after it ran on a file already known to fail.
fn record_failure(re: &Regex, text: &str, e: &fancy_regex::Error) {
    FAILURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(format!(
                "regular expression {:?} failed on {text:?}: {e}",
                re.as_str()
            ));
        }
    });
}

/// The failure recorded on this thread since the last call, if any.
pub fn take_failure() -> Option<String> {
    FAILURE.with(|slot| slot.borrow_mut().take())
}

/// The compiled form of `pattern`, compiled on first use.
pub fn rx(pattern: &str) -> &'static Regex {
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = cache.get(pattern) {
        return found;
    }
    let compiled = Regex::new(pattern).unwrap_or_else(|e| {
        panic!("cpplint: regular expression {pattern:?} does not compile: {e}")
    });
    let leaked: &'static Regex = Box::leak(Box::new(compiled));
    cache.insert(pattern.to_string(), leaked);
    leaked
}

fn run<'t>(re: &Regex, text: &'t str) -> Option<Captures<'t>> {
    match re.captures(text) {
        Ok(caps) => caps,
        Err(e) => {
            record_failure(re, text, &e);
            None
        }
    }
}

/// `re.search(...).start()` for an already compiled pattern.
pub fn find_start(re: &Regex, text: &str) -> Option<usize> {
    match re.find(text) {
        Ok(found) => found.map(|m| m.start()),
        Err(e) => {
            record_failure(re, text, &e);
            None
        }
    }
}

/// `re.search(pattern, text)`: the first match anywhere in `text`.
pub fn search<'t>(pattern: &str, text: &'t str) -> Option<Captures<'t>> {
    run(rx(pattern), text)
}

/// `re.match(pattern, text)`: a match that starts at the beginning of `text`.
pub fn pmatch<'t>(pattern: &str, text: &'t str) -> Option<Captures<'t>> {
    run(rx(&format!("^(?:{pattern})")), text)
}

/// `re.search(pattern, text) is not None`.
pub fn is_search(pattern: &str, text: &str) -> bool {
    search(pattern, text).is_some()
}

/// `re.match(pattern, text) is not None`.
pub fn is_match(pattern: &str, text: &str) -> bool {
    pmatch(pattern, text).is_some()
}

/// `match.group(i)`, the empty string when the group did not take part.
pub fn g<'t>(caps: &Captures<'t>, i: usize) -> &'t str {
    caps.get(i).map_or("", |m| m.as_str())
}

/// `match.group(i)` as Python returns it: `None` when the group did not
/// take part in the match.
pub fn group<'t>(caps: &Captures<'t>, i: usize) -> Option<&'t str> {
    caps.get(i).map(|m| m.as_str())
}

/// `match.start(i)`; the group must have taken part in the match.
pub fn start(caps: &Captures<'_>, i: usize) -> usize {
    caps.get(i).map_or(0, |m| m.start())
}

/// `match.end(i)`; the group must have taken part in the match.
pub fn end(caps: &Captures<'_>, i: usize) -> usize {
    caps.get(i).map_or(0, |m| m.end())
}

/// `re.sub(pattern, replacement, text)`; `replacement` uses `${n}` for
/// groups.
pub fn sub(pattern: &str, replacement: &str, text: &str) -> String {
    rx(pattern).replace_all(text, replacement).into_owned()
}

/// `re.split(pattern, text)`.
pub fn split<'t>(pattern: &str, text: &'t str) -> Vec<&'t str> {
    let re = rx(pattern);
    let mut parts = Vec::new();
    let mut last = 0;
    for found in re.find_iter(text) {
        let m = match found {
            Ok(m) => m,
            Err(e) => {
                record_failure(re, text, &e);
                break;
            }
        };
        parts.push(&text[last..m.start()]);
        last = m.end();
    }
    parts.push(&text[last..]);
    parts
}

/// `re.finditer(pattern, text)`, collected.
pub fn find_all<'t>(pattern: &str, text: &'t str) -> Vec<Captures<'t>> {
    let re = rx(pattern);
    let mut all = Vec::new();
    for found in re.captures_iter(text) {
        match found {
            Ok(caps) => all.push(caps),
            Err(e) => {
                record_failure(re, text, &e);
                break;
            }
        }
    }
    all
}

/// `re.escape(text)`.
pub fn escape(text: &str) -> String {
    regex::escape(text)
}

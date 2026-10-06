//! Sass dependency analyzer (`sass`).
//!
//! Scans `.scss` and `.sass` files for `@use`, `@forward` and `@import` rules
//! and adds every local file they load — transitively — as an input of the
//! product, so that editing a partial rebuilds the stylesheets that use it.
//! Resolution follows the Sass compiler's rules: relative to the importing
//! file, then `load_paths`; `.scss`, `.sass` and `.css`, each also as a
//! `_partial`, then the directory's `index` file.

use anyhow::{Result, bail};
use indicatif::ProgressBar;
use regex::Regex;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::SassAnalyzerConfig;
use crate::deps_cache::DepsCache;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};

use super::{DepAnalyzer, ScanResult};

/// Extensions of the files this analyzer scans (as product primary inputs).
const SASS_MATCH_EXTENSIONS: &[&str] = &["scss", "sass"];

/// Extensions a load URL without one is tried with, in this order.
const LOAD_EXTENSIONS: &[&str] = &["scss", "sass", "css"];

/// Which rule a load URL came from: `@import` has plain-CSS forms that
/// `@use`/`@forward` do not, so the two are filtered differently.
#[derive(Debug, PartialEq, Eq)]
enum Rule {
    UseOrForward,
    Import,
}

/// Sass dependency analyzer.
pub struct SassDepAnalyzer {
    iname: String,
    config: SassAnalyzerConfig,
}

impl SassDepAnalyzer {
    pub fn new(iname: &str, config: SassAnalyzerConfig) -> Self {
        Self {
            iname: iname.to_string(),
            config,
        }
    }

    /// Resolve a load URL against the importing file's directory, then each
    /// configured load path. Returns the first file that exists.
    fn resolve(&self, url: &str, including_dir: &Path) -> Option<PathBuf> {
        std::iter::once(including_dir.to_path_buf())
            .chain(self.config.load_paths.iter().map(PathBuf::from))
            .find_map(|dir| resolve_in(&dir.join(url)))
    }

    /// Scan `source` and everything it loads. Rescanned on every build (see
    /// `analyze`), so the result is never stale.
    fn scan(&self, source: &Path) -> Result<ScanResult> {
        let mut deps: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut scanned: HashSet<PathBuf> = HashSet::new();
        self.scan_recursive(source, &mut deps, &mut seen, &mut scanned)?;
        Ok(ScanResult {
            deps,
            hash_pieces: Vec::new(),
        })
    }

    /// `deps` and `seen` accumulate the input set; `scanned` is the cycle
    /// guard, keyed by canonical path so `a/../a/x.scss` cannot loop.
    fn scan_recursive(
        &self,
        file: &Path,
        deps: &mut Vec<PathBuf>,
        seen: &mut HashSet<PathBuf>,
        scanned: &mut HashSet<PathBuf>,
    ) -> Result<()> {
        let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        if !scanned.insert(canonical) {
            return Ok(());
        }
        let content = errors::ctx(
            fs::read_to_string(file),
            &format!("Failed to read {}", file.display()),
        )?;
        let indented = file.extension().is_some_and(|e| e == "sass");
        let including_dir = crate::processor::parent_dir_or_empty(file);

        for url in load_urls(&content, indented) {
            let Some(resolved) = self.resolve(&url, including_dir) else {
                if self.config.skip_not_found {
                    continue;
                }
                bail!(
                    "Sass import not found: \"{}\" in {} (searched {} and load_paths {:?}). \
                     Add its directory to load_paths, or set skip_not_found = true.",
                    url,
                    file.display(),
                    if including_dir.as_os_str().is_empty() {
                        Path::new(".").display()
                    } else {
                        including_dir.display()
                    },
                    self.config.load_paths,
                );
            };
            if seen.insert(resolved.clone()) {
                deps.push(resolved.clone());
            }
            // A loaded .css file is plain CSS: it loads nothing further.
            let is_sass = resolved
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| SASS_MATCH_EXTENSIONS.contains(&e));
            if is_sass {
                self.scan_recursive(&resolved, deps, seen, scanned)?;
            }
        }
        Ok(())
    }
}

/// The local load URLs of every `@use`, `@forward` and `@import` in a file,
/// in order. Built-in modules, URLs and plain-CSS imports are left out: they
/// load no file of the project.
fn load_urls(content: &str, indented: bool) -> Vec<String> {
    // SCSS rules end at `;`; the indented syntax ends them at the line end.
    static SCSS_RULE_RE: OnceLock<Regex> = OnceLock::new();
    static SASS_RULE_RE: OnceLock<Regex> = OnceLock::new();
    let rule_re = if indented {
        SASS_RULE_RE.get_or_init(|| {
            Regex::new(r"@(use|forward|import)\b([^\n]*)").expect(errors::INVALID_REGEX)
        })
    } else {
        SCSS_RULE_RE.get_or_init(|| {
            Regex::new(r"@(use|forward|import)\b([^;{}]*)").expect(errors::INVALID_REGEX)
        })
    };
    static QUOTED_RE: OnceLock<Regex> = OnceLock::new();
    let quoted_re = QUOTED_RE
        .get_or_init(|| Regex::new(r#""([^"]*)"|'([^']*)'"#).expect(errors::INVALID_REGEX));

    let content = strip_comments(content);
    let mut urls: Vec<String> = Vec::new();
    for caps in rule_re.captures_iter(&content) {
        let rule = if &caps[1] == "import" {
            Rule::Import
        } else {
            Rule::UseOrForward
        };
        let clause = caps[2].trim();
        let quoted: Vec<String> = quoted_re
            .captures_iter(clause)
            .filter_map(|c| c.get(1).or_else(|| c.get(2)))
            .map(|m| m.as_str().to_string())
            .collect();
        let candidates: Vec<String> = match rule {
            // `@use "x" with ($s: "str")`: only the first string is the URL.
            Rule::UseOrForward => quoted.into_iter().take(1).collect(),
            // `@import url(x)` is a plain CSS import.
            Rule::Import if clause.starts_with("url(") => Vec::new(),
            // The indented syntax allows unquoted `@import a, b`.
            Rule::Import if quoted.is_empty() && indented => clause
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
            Rule::Import => quoted,
        };
        for url in candidates {
            // `@import "x.css"` is a plain CSS import, left to the browser;
            // `@use "x.css"` loads the file as a module.
            let plain_css_import = rule == Rule::Import
                && Path::new(&url)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("css"));
            if !is_external(&url) && !plain_css_import {
                urls.push(url);
            }
        }
    }
    urls
}

/// A load URL that names no file of the project: a built-in module
/// (`sass:math`), a package importer URL, or a remote URL.
fn is_external(url: &str) -> bool {
    url.is_empty()
        || url.starts_with("sass:")
        || url.starts_with("pkg:")
        || url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("//")
        || url.starts_with("data:")
}

/// Remove `//` and `/* */` comments outside string literals, keeping line
/// breaks, so a commented-out `@use` neither adds a dependency nor fails
/// resolution.
fn strip_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"' | '\'', _) => {
                quote = Some(c);
                out.push(c);
            }
            ('/', Some('/')) => {
                for skipped in chars.by_ref() {
                    if skipped == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = '\0';
                for skipped in chars.by_ref() {
                    if skipped == '\n' {
                        out.push('\n');
                    }
                    if prev == '*' && skipped == '/' {
                        break;
                    }
                    prev = skipped;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// The Sass resolution rules for one base path (`dir.join(url)`): a path
/// with a Sass/CSS extension is tried as written and as a partial;
/// otherwise each of `.scss`, `.sass` and `.css`, also as a partial, then
/// `index` / `_index` inside the directory of that name.
fn resolve_in(base: &Path) -> Option<PathBuf> {
    let has_load_ext = base
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| LOAD_EXTENSIONS.contains(&e));
    let mut candidates: Vec<PathBuf> = Vec::new();
    if has_load_ext {
        candidates.push(base.to_path_buf());
        candidates.extend(partial_of(base));
    } else {
        for ext in LOAD_EXTENSIONS {
            let with_ext = with_appended_extension(base, ext);
            candidates.extend(partial_of(&with_ext));
            candidates.push(with_ext);
        }
        for ext in LOAD_EXTENSIONS {
            candidates.push(base.join(format!("_index.{ext}")));
            candidates.push(base.join(format!("index.{ext}")));
        }
    }
    candidates.into_iter().find(|c| c.is_file())
}

/// `dir/name.ext` → `dir/_name.ext`; None when the name is already a partial.
fn partial_of(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    if name.starts_with('_') {
        return None;
    }
    Some(path.with_file_name(format!("_{name}")))
}

/// Append `.ext` to the file name, without replacing an existing dot
/// segment (`theme.dark` + `scss` is `theme.dark.scss`).
fn with_appended_extension(path: &Path, ext: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(".");
    name.push(ext);
    PathBuf::from(name)
}

impl DepAnalyzer for SassDepAnalyzer {
    fn description(&self) -> &'static str {
        "Scan Sass/SCSS files for @use, @forward and @import dependencies"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        file_index.has_extension(".scss") || file_index.has_extension(".sass")
    }

    fn match_product(&self, p: &Product) -> Option<PathBuf> {
        let source = p.inputs.first()?;
        let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
        if SASS_MATCH_EXTENSIONS.contains(&ext) {
            Some(source.clone())
        } else {
            None
        }
    }

    /// Uses the full scanner, which rescans every source on every build
    /// instead of trusting the per-source deps cache. The cache validates
    /// only the importing file's own checksum, so a `@use` added inside a
    /// partial would leave the cached list short — the same stale-output
    /// bug this analyzer exists to prevent. Scanning is a few regexes per
    /// file, cheap enough to always do.
    fn analyze(
        &self,
        ctx: &crate::build_context::BuildContext,
        graph: &mut BuildGraph,
        deps_cache: &mut DepsCache,
        _file_index: &FileIndex,
        _verbose: bool,
        progress: &ProgressBar,
    ) -> Result<()> {
        super::analyze_with_full_scanner(
            ctx,
            graph,
            deps_cache,
            &self.iname,
            |p| self.match_product(p),
            |source| self.scan(source),
            progress,
        )
    }
}

inventory::submit! {
    crate::registries::AnalyzerPlugin {
        name: "sass",
        description: "Scan Sass/SCSS files for @use, @forward and @import dependencies",
        is_native: true,
        create: |iname, toml_value, _| {
            let cfg: SassAnalyzerConfig = toml::from_str(&toml::to_string(toml_value)?)?;
            Ok(Box::new(SassDepAnalyzer::new(iname, cfg)))
        },
        defconfig_toml: || {
            toml::to_string_pretty(&SassAnalyzerConfig::default()).ok()
        },
        known_fields: crate::registries::typed_known_fields::<SassAnalyzerConfig>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_urls_scss_rules() {
        let src = r#"
@use "sass:math";
@use "vars" as v;
@use 'theme' with ($accent: "red", $font: 'x');
@forward "mixins" show button;
@import "a", 'b';
@import "plain.css";
@import url(remote.css);
@import "https://example.com/x";
"#;
        assert_eq!(
            load_urls(src, false),
            vec!["vars", "theme", "mixins", "a", "b"]
        );
    }

    #[test]
    fn load_urls_ignore_comments() {
        let src = "// @use \"gone\";\n/* @import \"also-gone\"; */\n@use \"kept\"; // trailing\n";
        assert_eq!(load_urls(src, false), vec!["kept"]);
    }

    #[test]
    fn load_urls_keep_double_slash_inside_strings() {
        // `//` inside a string is not a comment, so the rule after it survives.
        let src = "$u: \"http://x\";\n@use \"kept\";\n";
        assert_eq!(load_urls(src, false), vec!["kept"]);
    }

    #[test]
    fn load_urls_indented_unquoted_import() {
        let src = "@import reset, base\n@use \"vars\"\n";
        assert_eq!(load_urls(src, true), vec!["reset", "base", "vars"]);
    }

    #[test]
    fn resolve_in_follows_sass_rules() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        fs::write(d.join("_vars.scss"), "").unwrap();
        fs::write(d.join("plain.scss"), "").unwrap();
        fs::write(d.join("theme.dark.scss"), "").unwrap();
        fs::create_dir(d.join("comp")).unwrap();
        fs::write(d.join("comp/_index.scss"), "").unwrap();
        fs::write(d.join("lib.css"), "").unwrap();

        assert_eq!(resolve_in(&d.join("vars")), Some(d.join("_vars.scss")));
        assert_eq!(resolve_in(&d.join("_vars")), Some(d.join("_vars.scss")));
        assert_eq!(resolve_in(&d.join("vars.scss")), Some(d.join("_vars.scss")));
        assert_eq!(resolve_in(&d.join("plain")), Some(d.join("plain.scss")));
        assert_eq!(
            resolve_in(&d.join("theme.dark")),
            Some(d.join("theme.dark.scss"))
        );
        assert_eq!(
            resolve_in(&d.join("comp")),
            Some(d.join("comp/_index.scss"))
        );
        assert_eq!(resolve_in(&d.join("lib")), Some(d.join("lib.css")));
        assert_eq!(resolve_in(&d.join("missing")), None);
    }
}

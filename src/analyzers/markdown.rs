//! Markdown dependency analyzer for scanning image and file references.
//!
//! Scans Markdown source files for image references (`![alt](path)`) and
//! adds referenced local files as dependencies to products in the build graph.

use anyhow::Result;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::MarkdownAnalyzerConfig;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::Product;

use super::{DepAnalyzer, ScanResult};

/// Markdown dependency analyzer that scans source files for image and link references.
pub struct MarkdownDepAnalyzer {
    config: MarkdownAnalyzerConfig,
}

impl MarkdownDepAnalyzer {
    pub const fn new(config: MarkdownAnalyzerConfig) -> Self {
        Self { config }
    }

    /// Scan a Markdown file for local file references.
    /// Returns paths to local files referenced via `![alt](path)` or
    /// `[text](path)` syntax, and the candidate paths probed and found
    /// missing — a reference to a file that does not exist yet must be
    /// picked up once it does. A reference resolves to a file on disk or to
    /// one another product will generate (in `file_index`).
    fn scan_references(&self, source: &Path, file_index: &FileIndex) -> Result<ScanResult> {
        let content = crate::errors::ctx(
            fs::read_to_string(source),
            &format!("Failed to read markdown: {}", source.display()),
        )?;
        let mut refs = Vec::new();
        let mut absent = Vec::new();
        let mut seen = HashSet::new();

        let source_dir = crate::processor::parent_dir(source);

        for path_str in reference_targets(&content) {
            let path_str = path_str.as_str();

            // Try resolving relative to the source file's directory first,
            // then relative to the project root (cwd)
            let candidates = [
                super::lexical_normalize(&source_dir.join(path_str)),
                super::lexical_normalize(Path::new(path_str)),
            ];
            let found = candidates
                .iter()
                .find(|c| c.is_file() || file_index.contains(c));
            absent.extend(super::probed_before(&candidates, found));
            if let Some(found) = found
                && seen.insert(found.clone())
            {
                refs.push(found.clone());
            }
        }
        absent.sort();
        absent.dedup();

        Ok(ScanResult::deps(refs, absent))
    }
}

/// The local paths a Markdown text refers to, each as written with its
/// decoration removed. Recognised:
///
/// - inline links and images, `[text](path)` and `![alt](path "title")`,
///   the path optionally in angle brackets (`(<my file.png>)`);
/// - reference definitions, `[label]: path`, which is where the path of a
///   `![alt][label]` lives;
/// - the `src` and `href` attributes of inline HTML (`<img src="a.png">`).
///
/// URLs and bare `#anchors` are left out, a `#fragment` is cut off, and
/// percent-escapes are decoded (`my%20file.png` is the file `my file.png`).
fn reference_targets(content: &str) -> Vec<String> {
    static INLINE_RE: OnceLock<Regex> = OnceLock::new();
    static DEFINITION_RE: OnceLock<Regex> = OnceLock::new();
    static ATTRIBUTE_RE: OnceLock<Regex> = OnceLock::new();
    // The target of an inline link or a definition: `<anything>` or a run
    // without whitespace; a title may follow and is not captured.
    let inline_re = INLINE_RE.get_or_init(|| {
        Regex::new(r"!?\[[^\]]*\]\(\s*(?:<([^>\n]*)>|([^)\s]+))[^)]*\)")
            .expect(errors::INVALID_REGEX)
    });
    let definition_re = DEFINITION_RE.get_or_init(|| {
        Regex::new(r"(?m)^ {0,3}\[[^\]\n]+\]:\s*(?:<([^>\n]*)>|(\S+))")
            .expect(errors::INVALID_REGEX)
    });
    let attribute_re = ATTRIBUTE_RE.get_or_init(|| {
        Regex::new(r#"(?i)<[a-z][^>]*?\s(?:src|href)\s*=\s*(?:"([^"]*)"|'([^']*)')"#)
            .expect(errors::INVALID_REGEX)
    });

    let mut targets = Vec::new();
    for re in [inline_re, definition_re, attribute_re] {
        for caps in re.captures_iter(content) {
            let Some(raw) = caps.get(1).or_else(|| caps.get(2)) else {
                continue;
            };
            let raw = raw.as_str().trim();
            // A scheme (`https:`, `mailto:`, `data:`) or a protocol-relative
            // `//host` is not a file of the project.
            let has_scheme = raw.split_once(':').is_some_and(|(scheme, _)| {
                !scheme.is_empty()
                    && scheme
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            });
            if has_scheme || raw.starts_with("//") {
                continue;
            }
            let path = raw.split('#').next().unwrap_or(raw);
            if !path.is_empty() {
                targets.push(percent_decode(path));
            }
        }
    }
    targets
}

/// `text` with its `%XX` escapes decoded. Text whose escapes do not decode
/// to UTF-8 is returned as written.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| text.get(i + 1..i + 3))
            .flatten()
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        if let Some(byte) = hex {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

impl DepAnalyzer for MarkdownDepAnalyzer {
    fn description(&self) -> &'static str {
        "Scan Markdown files for local file dependencies"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        file_index.has_extension(".md")
    }

    fn match_product(&self, p: &Product) -> Option<PathBuf> {
        if p.inputs.is_empty() {
            return None;
        }
        let source = &p.inputs[0];
        let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext == "md" {
            Some(source.clone())
        } else {
            None
        }
    }

    fn scan(
        &self,
        _ctx: &crate::build_context::BuildContext,
        source: &Path,
        file_index: &FileIndex,
    ) -> Result<ScanResult> {
        self.scan_references(source, file_index)
    }

    fn fingerprint_parts(&self, _ctx: &crate::build_context::BuildContext) -> Result<Vec<String>> {
        super::config_fingerprint(&self.config)
    }
}

inventory::submit! {
    crate::registries::AnalyzerPlugin {
        name: "markdown",
        description: "Scan Markdown files for local file dependencies",
        is_builtin: true,
        // 2: reference-style links, HTML `src`/`href`, `<...>` and
        // percent-encoded targets.
        version: 2,
        create: |toml_value, _| {
            let cfg: MarkdownAnalyzerConfig = crate::registries::deserialize_config(toml_value)?;
            Ok(Box::new(MarkdownDepAnalyzer::new(cfg)))
        },
        defconfig_toml: || {
            toml::to_string_pretty(&MarkdownAnalyzerConfig::default()).ok()
        },
        known_fields: crate::registries::typed_known_fields::<MarkdownAnalyzerConfig>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every form a Markdown file names a local file in. Only the plain
    /// inline form used to be seen, so an image named any other way was
    /// not a dependency and editing it left the rendered output cached.
    #[test]
    fn reference_targets_cover_the_forms_markdown_names_a_file_in() {
        let content = "\
![plain](c.png)
![titled](d.png \"a title\")
[text](doc/page.md#section)
![encoded](my%20img.png)
![angle](<an image.png>)
![by reference][r]
<img src=\"b.png\" alt=\"x\">
<a href='notes.txt'>notes</a>

[r]: a.png \"title\"
[site]: https://example.com/x.png
[web](https://example.com) [mail](mailto:a@b.c) [top](#top) [cdn](//cdn/x.js)
<img src=\"https://example.com/y.png\">
";
        let mut targets = reference_targets(content);
        targets.sort();
        assert_eq!(
            targets,
            [
                "a.png",
                "an image.png",
                "b.png",
                "c.png",
                "d.png",
                "doc/page.md",
                "my img.png",
                "notes.txt",
            ]
        );
    }

    #[test]
    fn percent_decode_leaves_what_is_not_an_escape() {
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_decode("100%.png"), "100%.png");
        assert_eq!(percent_decode("bad%zz"), "bad%zz");
        assert_eq!(percent_decode("caf%C3%A9"), "café");
        assert_eq!(percent_decode("lone%FF"), "lone%FF");
    }
}

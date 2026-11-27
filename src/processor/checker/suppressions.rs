//! A lint suppression must name what it suppresses and say why.
//!
//! Every linter has an inline comment that silences it. This checker reads
//! those comments, in the syntax of the language of each file, and fails on
//! one that
//!
//! - names no rule (`# noqa`, `// NOLINT`, `## no critic`, `# type: ignore`),
//! - covers a whole file or block rather than one line (`/* eslint-disable */`,
//!   `# yamllint disable`, `# ruff: noqa`, `#[allow(...)]`),
//! - or gives no reason: text after the directive on its line, or a comment
//!   on the line directly above it.
//!
//! It does not judge the reason, and it does not know whether the
//! suppression is still needed; the linters' own "unused suppression" rules
//! (ruff's RUF100, rumdl's MD087, cppcheck's unmatchedSuppression, rustc's
//! `expect`) do that.

use std::path::Path;
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;

use crate::config::CheckerConfig;
use crate::graph::Product;

/// The comment syntax and the directives of one family of files.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Python,
    Shell,
    C,
    Java,
    Perl,
    Script,
    Css,
    Yaml,
    Docker,
    Markdown,
    Lua,
    Rust,
}

impl Kind {
    fn of(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        if name == "Dockerfile" || name.starts_with("Dockerfile.") {
            return Some(Self::Docker);
        }
        let ext = path.extension()?.to_str()?;
        Some(match ext {
            "py" => Self::Python,
            "sh" | "bash" => Self::Shell,
            "c" | "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" => Self::C,
            "java" => Self::Java,
            "pl" | "pm" => Self::Perl,
            "js" | "mjs" | "cjs" | "ts" => Self::Script,
            "css" => Self::Css,
            "yml" | "yaml" => Self::Yaml,
            "md" => Self::Markdown,
            "lua" => Self::Lua,
            "rs" => Self::Rust,
            _ => return None,
        })
    }

    /// What a line that is only a comment starts with.
    const fn comment_leaders(self) -> &'static [&'static str] {
        match self {
            Self::Python | Self::Shell | Self::Perl | Self::Yaml | Self::Docker => &["#"],
            Self::C | Self::Java | Self::Script | Self::Css | Self::Rust => &["//", "/*", "*"],
            Self::Markdown => &["<!--"],
            Self::Lua => &["--"],
        }
    }

    fn directives(self) -> &'static [Directive] {
        static TABLE: LazyLock<Vec<(Kind, Vec<Directive>)>> = LazyLock::new(build_table);
        TABLE
            .iter()
            .find(|(kind, _)| *kind == self)
            .map_or(&[], |(_, directives)| directives.as_slice())
    }
}

/// One linter's inline suppression. The regex's named groups say what was
/// written: `rule` (what is suppressed), `rest` (the text after it, where a
/// reason may be), and `scope` for the linters whose plain form covers a
/// block and whose `-line` forms cover one line.
struct Directive {
    /// How the message names the directive.
    name: &'static str,
    regex: Regex,
    /// The plain form covers the rest of the file: only a scoped form
    /// (`-line`, `-next-line`) is accepted.
    needs_scope: bool,
    /// The directive is refused whatever it says.
    refused: Option<&'static str>,
}

fn directive(name: &'static str, pattern: &str) -> Directive {
    Directive {
        name,
        regex: Regex::new(pattern).expect("the suppression patterns are valid"),
        needs_scope: false,
        refused: None,
    }
}

fn scoped(name: &'static str, pattern: &str) -> Directive {
    Directive {
        needs_scope: true,
        ..directive(name, pattern)
    }
}

fn refused(name: &'static str, pattern: &str, why: &'static str) -> Directive {
    Directive {
        refused: Some(why),
        ..directive(name, pattern)
    }
}

/// The suppression comments of the Python linters: ruff, flake8, pyrefly, mypy and pylint.
fn python_directives() -> Vec<Directive> {
    vec![
        refused(
            "ruff: noqa",
            r"#\s*(?:ruff|flake8):\s*noqa",
            "covers the whole file: suppress the one line with `# noqa: CODE`",
        ),
        refused(
            "pyrefly: ignore-errors",
            r"#\s*pyrefly:\s*ignore-errors",
            "covers the whole file: suppress the one line with `# pyrefly: ignore[code]`",
        ),
        directive(
            "type: ignore",
            r"#\s*type:\s*ignore(?P<rule>\[[^\]]+\])?(?P<rest>.*)",
        ),
        directive(
            "pyrefly: ignore",
            r"#\s*pyrefly:\s*ignore(?P<rule>\[[^\]]+\])?(?P<rest>.*)",
        ),
        directive(
            "noqa",
            r"#\s*noqa\b(?::\s*(?P<rule>[A-Z]+[0-9]+(?:\s*,\s*[A-Z]+[0-9]+)*))?(?P<rest>.*)",
        ),
        directive(
            "pylint: disable",
            r"#\s*pylint:\s*disable=(?P<rule>[\w-]+(?:,[\w-]+)*)(?P<rest>.*)",
        ),
    ]
}

/// The suppression comments of the JavaScript and TypeScript linters.
fn script_directives() -> Vec<Directive> {
    vec![
        scoped(
            "eslint-disable",
            r"\b(?:eslint|oxlint)-disable(?P<scope>-next-line|-line)?\b(?:\s+(?P<rule>[@\w/-]+(?:\s*,\s*[@\w/-]+)*))?(?P<rest>.*)",
        ),
        directive(
            "biome-ignore",
            r"\bbiome-ignore\b(?:\s+(?P<rule>lint/[\w/-]+))?(?P<rest>.*)",
        ),
    ]
}

/// The suppression comments of the CSS linters.
fn css_directives() -> Vec<Directive> {
    vec![
        directive(
            "biome-ignore",
            r"\bbiome-ignore\b(?:\s+(?P<rule>lint/[\w/-]+))?(?P<rest>.*)",
        ),
        scoped(
            "stylelint-disable",
            r"\bstylelint-disable(?P<scope>-next-line|-line)?\b(?:\s+(?P<rule>[\w/-]+(?:\s*,\s*[\w/-]+)*))?(?P<rest>.*)",
        ),
    ]
}

fn build_table() -> Vec<(Kind, Vec<Directive>)> {
    let shellcheck = || {
        directive(
            "shellcheck disable",
            r"#\s*shellcheck\s+disable=(?P<rule>SC\d+(?:,SC\d+)*)(?P<rest>.*)",
        )
    };
    vec![
        (Kind::Python, python_directives()),
        (Kind::Shell, vec![shellcheck()]),
        (
            Kind::C,
            vec![
                directive(
                    "cppcheck-suppress",
                    r"cppcheck-suppress(?:\s+(?P<rule>\[[^\]]+\]|\w+))?(?P<rest>.*)",
                ),
                directive(
                    "NOLINT",
                    r"\bNOLINT(?:NEXTLINE|BEGIN)?\b(?P<rule>\([^)]+\))?(?P<rest>.*)",
                ),
            ],
        ),
        (
            Kind::Java,
            vec![directive(
                "CHECKSTYLE:OFF",
                r"CHECKSTYLE[:.]OFF(?::\s*(?P<rule>\w+))?(?P<rest>.*)",
            )],
        ),
        (
            Kind::Perl,
            vec![directive(
                "no critic",
                r"##\s*no\s+critic\b\s*(?P<rule>\([^)]+\))?(?P<rest>.*)",
            )],
        ),
        (Kind::Script, script_directives()),
        (Kind::Css, css_directives()),
        (
            Kind::Yaml,
            vec![
                scoped(
                    "yamllint disable",
                    r"#\s*yamllint\s+disable(?P<scope>-line)?\b(?:\s+(?P<rule>rule:[\w-]+(?:\s+rule:[\w-]+)*))?(?P<rest>.*)",
                ),
                directive(
                    "zizmor: ignore",
                    r"#\s*zizmor:\s*ignore(?P<rule>\[[^\]]+\])?(?P<rest>.*)",
                ),
            ],
        ),
        (
            Kind::Docker,
            vec![
                directive(
                    "hadolint ignore",
                    r"#\s*hadolint\s+ignore=(?P<rule>[A-Z]+\d+(?:,[A-Z]+\d+)*)(?P<rest>.*)",
                ),
                shellcheck(),
            ],
        ),
        (
            Kind::Markdown,
            vec![scoped(
                "rumdl-disable",
                r"<!--\s*(?:rumdl|markdownlint)-disable(?P<scope>-next-line|-line)?\b(?:\s+(?P<rule>MD\d+(?:[\s,]+MD\d+)*))?(?P<rest>.*)",
            )],
        ),
        (
            Kind::Lua,
            vec![directive(
                "luacheck: ignore",
                r"--\s*luacheck:\s*ignore\b(?:\s+(?P<rule>[\w/.]+(?:\s+[\w/.]+)*))?(?P<rest>)",
            )],
        ),
        (
            Kind::Rust,
            vec![refused(
                "#[allow]",
                r"^\s*#!?\[\s*(?:cfg_attr\([^,]+,\s*)?allow\s*\(",
                "use #[expect(..., reason = \"...\")], which also fails once it suppresses nothing",
            )],
        ),
    ]
}

/// A Rust `#[expect(...)]`, which carries its reason as an argument and may
/// run over several lines.
static RUST_EXPECT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*#!?\[\s*(?:cfg_attr\([^,]+,\s*)?expect\s*\(")
        .expect("the suppression patterns are valid")
});

/// Whether `text` says something: at least one word of two letters, once the
/// comment punctuation around it is set aside.
fn says_something(text: &str) -> bool {
    let text = text
        .trim()
        .trim_end_matches("*/")
        .trim_end_matches("-->")
        .trim();
    let mut letters = 0;
    for c in text.chars() {
        if c.is_alphabetic() {
            letters += 1;
            if letters >= 2 {
                return true;
            }
        } else {
            letters = 0;
        }
    }
    false
}

/// Whether `line` holds a suppression directive of `kind`.
fn is_directive(kind: Kind, line: &str) -> bool {
    kind.directives().iter().any(|d| d.regex.is_match(line))
        || (kind == Kind::Rust && RUST_EXPECT.is_match(line))
}

/// Whether the reason for the directive on `lines[index]` stands directly
/// above it: a comment line that is not itself a directive. Directive lines
/// stacked above are passed over, so that several of them share one reason.
fn reason_above(kind: Kind, lines: &[&str], index: usize) -> bool {
    let mut at = index;
    while at > 0 {
        at -= 1;
        let line = lines[at].trim();
        if is_directive(kind, line) {
            continue;
        }
        return kind
            .comment_leaders()
            .iter()
            .any(|leader| line.strip_prefix(leader).is_some_and(says_something));
    }
    false
}

/// The findings in one file's text, as `path:line: message` lines.
fn check_text(path: &Path, kind: Kind, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut findings = Vec::new();
    let mut report = |index: usize, message: String| {
        findings.push(format!("{}:{}: {message}", path.display(), index + 1));
    };
    for (index, line) in lines.iter().enumerate() {
        for directive in kind.directives() {
            let Some(caps) = directive.regex.captures(line) else {
                continue;
            };
            let name = directive.name;
            if let Some(why) = directive.refused {
                report(index, format!("`{name}` {why}"));
                break;
            }
            if directive.needs_scope && caps.name("scope").is_none() {
                report(
                    index,
                    format!("`{name}` covers the rest of the file: suppress the one line"),
                );
                break;
            }
            if caps.name("rule").is_none() {
                report(index, format!("`{name}` names no rule"));
                break;
            }
            let rest = caps.name("rest").map_or("", |m| m.as_str());
            if !says_something(rest) && !reason_above(kind, &lines, index) {
                report(
                    index,
                    format!(
                        "`{name}` gives no reason: say why on the same line or in a comment directly above"
                    ),
                );
            }
            break;
        }
        if kind == Kind::Rust && RUST_EXPECT.is_match(line) {
            // the attribute may run over several lines; its reason is an argument
            let end = lines[index..]
                .iter()
                .position(|l| l.contains(")]"))
                .map_or(lines.len() - 1, |offset| index + offset);
            if !lines[index..=end].iter().any(|l| l.contains("reason")) {
                report(
                    index,
                    "`#[expect]` gives no reason: add `reason = \"...\"`".to_string(),
                );
            }
        }
    }
    findings
}

pub struct SuppressionsProcessor {
    config: CheckerConfig,
}

impl SuppressionsProcessor {
    pub const fn new(config: CheckerConfig) -> Self {
        Self { config }
    }

    fn check_file(file: &Path) -> Result<()> {
        let Some(kind) = Kind::of(file) else {
            return Ok(());
        };
        let bytes = crate::errors::ctx(
            std::fs::read(file),
            &format!("Failed to read {}", file.display()),
        )?;
        let text = String::from_utf8_lossy(&bytes);
        let findings = check_text(file, kind, &text);
        if findings.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("{}", findings.join("\n"))
        }
    }
}

impl crate::processor::Processor for SuppressionsProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    /// One product per scanned file (the default `discover`).
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    fn execute(&self, _ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        Self::check_file(product.primary_input())
    }

    fn execute_batch(
        &self,
        _ctx: &crate::build_context::BuildContext,
        products: &[&Product],
    ) -> Vec<Result<()>> {
        crate::processor::execute_checker_batch_per_file(products, Self::check_file)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(SuppressionsProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "suppressions",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<crate::config::CheckerConfig>,
        fields: &[],
        omit_standard_fields: &["command", "formats", "args", "output_dir", "required_tools"],
        scan_defaults: Some(crate::config::ScanDefaultsData {
            src_extensions: &[
                ".py", ".sh", ".bash", ".c", ".cc", ".cpp", ".cxx", ".h", ".hh", ".hpp", ".java",
                ".pl", ".pm", ".js", ".mjs", ".cjs", ".ts", ".css", ".yml", ".yaml", ".md", ".lua",
                ".rs", "Dockerfile",
            ],
            src_exclude_dirs: &[],
        }),
        defaults: None,
        keywords: &["checker", "lint", "suppression", "noqa", "policy"],
        description: "Check that every lint suppression names a rule and gives a reason",
        implementation: crate::registries::Implementation::Builtin,
        supports_batch: true,
        caches_outputs: true,
        sandboxable: true,
        max_jobs_cap: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(name: &str, text: &str) -> Vec<String> {
        let path = Path::new(name);
        check_text(path, Kind::of(path).expect("a known kind"), text)
    }

    #[test]
    fn a_rule_and_a_reason_pass() {
        assert_eq!(
            check(
                "a.py",
                "x = f()  # noqa: E731  teaching: a lambda on purpose\n"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            check(
                "a.c",
                "// cppcheck-suppress nullPointer // teaching: the crash shown\n*p=0;\n"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            check(
                "a.c",
                "/* cppcheck-suppress unreadVariable ; kept for the reader */\n"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            check(
                "a.sh",
                "# shellcheck disable=SC2034  the variable is read by the caller\n"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            check(
                "a.rs",
                "#[expect(clippy::foo, reason = \"teaching: x\")]\nfn f() {}\n"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            check(
                "a.rs",
                "#[expect(\n    clippy::foo,\n    reason = \"x\"\n)]\nfn f() {}\n"
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_reason_may_stand_on_the_line_above() {
        assert_eq!(
            check(
                "a.yml",
                "  # the trigger is safe here: pushes only\n  workflow_run:  # zizmor: ignore[dangerous-triggers]\n"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            check(
                "Dockerfile",
                "# this example shows that only the last CMD counts\n# hadolint ignore=DL4003\nCMD [\"a\"]\n"
            ),
            Vec::<String>::new()
        );
        // one reason for two stacked directives
        assert_eq!(
            check(
                "a.c",
                "// teaching: read before it is set\n// cppcheck-suppress uninitvar\n// cppcheck-suppress uninitStructMember\nf(v.a);\n"
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn no_reason_fails() {
        let found = check("a.py", "import os\nx = f()  # noqa: E731\n");
        assert_eq!(found.len(), 1);
        assert!(
            found[0].starts_with("a.py:2: `noqa` gives no reason"),
            "{found:?}"
        );
        // code above is not a reason
        assert_eq!(
            check("a.c", "int x;\n// cppcheck-suppress nullPointer\n*p=0;\n").len(),
            1
        );
        assert_eq!(
            check("a.rs", "#[expect(clippy::foo)]\nfn f() {}\n").len(),
            1
        );
    }

    #[test]
    fn no_rule_fails() {
        for (name, text) in [
            ("a.py", "x = 1  # noqa  teaching: why\n"),
            ("a.py", "x = 1  # type: ignore  teaching: why\n"),
            ("a.cc", "delete this;  // NOLINT teaching: why\n"),
            ("a.pl", "eval $x;  ## no critic  teaching: why\n"),
        ] {
            let found = check(name, text);
            assert_eq!(found.len(), 1, "{name}: {found:?}");
            assert!(found[0].contains("names no rule"), "{found:?}");
        }
    }

    #[test]
    fn file_wide_forms_fail() {
        for (name, text) in [
            ("a.py", "# ruff: noqa\n"),
            ("a.js", "/* eslint-disable no-undef -- why */\n"),
            ("a.yaml", "# yamllint disable rule:line-length  why\n"),
            ("a.md", "<!-- why -->\n<!-- rumdl-disable MD013 -->\n"),
            ("a.rs", "#[allow(dead_code)]\nfn f() {}\n"),
        ] {
            assert_eq!(check(name, text).len(), 1, "{name}");
        }
        assert_eq!(
            check(
                "a.js",
                "f(); // eslint-disable-line no-undef -- teaching: the bug shown\n"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            check(
                "a.md",
                "<!-- teaching: the idiom shown -->\n<!-- rumdl-disable-next-line MD053 -->\n[//]: <> (x)\n"
            ),
            Vec::<String>::new()
        );
    }

    /// A page that describes the directives in prose or in a table, and a
    /// Rust string that holds an attribute, are not suppressions.
    #[test]
    fn a_directive_that_is_only_written_about_is_not_read() {
        assert_eq!(
            check(
                "a.md",
                "| `.md` | `rumdl-disable`, `markdownlint-disable` |\n"
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            check("a.rs", "    let s = \"#[allow(dead_code)]\";\n"),
            Vec::<String>::new()
        );
    }

    /// Each directive needs its own reason when content stands between them.
    #[test]
    fn a_reason_does_not_reach_past_content() {
        let text = "<!-- teaching: why -->\n<!-- rumdl-disable-next-line MD053 -->\n[//1]: <> (x)\n<!-- rumdl-disable-next-line MD053 -->\n[//2]: <> (y)\n";
        let found = check("a.md", text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("a.md:4:"), "{found:?}");
    }

    #[test]
    fn another_language_s_directive_in_a_string_is_not_read() {
        assert_eq!(
            check("a.rs", "let s = \"# shellcheck disable=SC2207\";\n"),
            Vec::<String>::new()
        );
        assert_eq!(
            check("a.py", "print('// cppcheck-suppress nullPointer')\n"),
            Vec::<String>::new()
        );
    }
}

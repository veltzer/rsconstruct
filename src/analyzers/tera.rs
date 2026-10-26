//! Tera template dependency analyzer for scanning include and import directives.
//!
//! Scans Tera template files for `{% include %}`, `{% import %}`, and `{% extends %}`
//! directives and adds referenced template files as dependencies to products in the build graph.
//! Also scans for the file-reading template functions (`load_python`/`load_lua`/`load_data`/
//! `load_json`/`load_toml`/`load_csv`, `toml_get` and `version_str`) so the files they read are
//! content-tracked inputs, and for `glob`/`git_count_files`/`grep_count`/`shell_output`
//! whose resolved path sets and command literals enter the cache hash.
//!
//! Every path is resolved against the project root and nothing else, exactly
//! as the tera processor does: it registers each includable template under
//! its project-relative path, and the loader functions open their `path`
//! relative to the current directory. Resolving against the template's own
//! directory too, as this analyzer once did, tracked files the renderer
//! never read.

use anyhow::{Result, bail};
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::TeraAnalyzerConfig;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::Product;

use super::{DepAnalyzer, ScanResult};

/// Tera template dependency analyzer that scans for include/import/extends directives.
pub struct TeraDepAnalyzer {
    config: TeraAnalyzerConfig,
}

impl TeraDepAnalyzer {
    pub const fn new(config: TeraAnalyzerConfig) -> Self {
        Self { config }
    }

    /// Scan a Tera template file for all dependency-affecting constructs.
    /// Returns the resolved file paths (added to product.inputs) and the
    /// hash pieces that capture non-content state — the sorted set of paths
    /// matching each glob, plus the literal text of each shell command.
    ///
    /// A referenced file resolves when it is on disk or in `file_index`,
    /// which also holds the outputs other products declare.
    pub(crate) fn scan_template(
        &self,
        ctx: &crate::build_context::BuildContext,
        source: &Path,
        file_index: &FileIndex,
    ) -> Result<ScanResult> {
        let mut paths: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        // Pieces accumulated into the config_hash: sorted paths from each glob,
        // plus literal command strings. Order matters and is determined by the
        // order in which they appear in the template, which is stable.
        let mut hash_pieces: Vec<String> = Vec::new();
        // Templates whose contents we've already scanned, to avoid infinite
        // recursion on cyclic includes.
        let mut scanned: HashSet<PathBuf> = HashSet::new();

        scan_template_recursive(
            ctx,
            source,
            file_index,
            &mut paths,
            &mut seen,
            &mut hash_pieces,
            &mut scanned,
        )?;

        Ok(ScanResult {
            deps: paths,
            hash_pieces,
            absent: Vec::new(),
        })
    }
}

/// The template functions whose `path=` argument names a file the renderer
/// reads: the file is a content-tracked input. `toml_get` must be tracked
/// or a changed value (a version bump in pyproject.toml) would leave
/// rendered output cached against the old content.
const LOADER_FUNCTIONS: &[&str] = &[
    "load_python",
    "load_lua",
    "load_data",
    "load_json",
    "load_toml",
    "load_csv",
    "toml_get",
];

/// The file `version_str()` reads when called without `path`. Must mirror
/// `VersionStrFunction` in processor/generator/tera.rs or the analyzer would
/// track a different file than the renderer reads.
const VERSION_STR_DEFAULT_PATH: &str = "config/version.py";

/// Whether `c` can be part of a Tera identifier.
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Quote and bracket state while walking a Tera argument list. Tera string
/// literals (`"…"`, `'…'`, `` `…` ``) have no escape sequences: a string
/// ends at the next quote of its own kind.
#[derive(Default)]
struct ArgScanner {
    depth: usize,
    quote: Option<char>,
}

impl ArgScanner {
    /// Whether the scanner currently sits outside every string and bracket.
    const fn at_top_level(&self) -> bool {
        self.quote.is_none() && self.depth == 0
    }

    /// Advance over one character.
    const fn step(&mut self, c: char) {
        if let Some(q) = self.quote {
            if c == q {
                self.quote = None;
            }
            return;
        }
        match c {
            '"' | '\'' | '`' => self.quote = Some(c),
            '(' | '[' | '{' => self.depth += 1,
            ')' | ']' | '}' => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
    }
}

/// Index of the `)` that closes the argument list `s` starts inside of, or
/// None when no top-level `)` closes it before the end of `s`.
fn matching_close(s: &str) -> Option<usize> {
    let mut scanner = ArgScanner::default();
    for (i, c) in s.char_indices() {
        if scanner.at_top_level() && c == ')' {
            return Some(i);
        }
        scanner.step(c);
    }
    None
}

/// Split `s` at every top-level `sep` (one outside every string and bracket).
fn top_level_split(s: &str, sep: char) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut scanner = ArgScanner::default();
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if scanner.at_top_level() && c == sep {
            pieces.push(&s[start..i]);
            start = i + c.len_utf8();
        }
        scanner.step(c);
    }
    pieces.push(&s[start..]);
    pieces
}

/// Every call of one of `names` in `content`, in order of appearance, as
/// the text between its parentheses. This follows the argument grammar the
/// Tera parser accepts for a function call: string literals are opaque, so a
/// `)` or `,` inside one neither closes the call nor splits an argument, and
/// nested brackets balance. A regex stopping at the first `)` truncated
/// `shell_output(command="echo $(uname -r)", depends_on=[...])` before its
/// `depends_on` and reported it missing.
///
/// A name that is part of a longer identifier (`my_glob(`) or a keyword
/// argument (`glob=`) is not a call. A call with no closing parenthesis is
/// skipped: Tera rejects the template when it renders.
fn calls<'a>(content: &'a str, names: &[&str]) -> Vec<&'a str> {
    let mut found = Vec::new();
    let mut i = 0;
    let mut prev_ident = false;
    while i < content.len() {
        let rest = &content[i..];
        let c = rest.chars().next().expect("index is on a char boundary");
        if !prev_ident
            && let Some(name) = names
                .iter()
                .find(|n| rest.starts_with(**n) && !rest[n.len()..].starts_with(is_ident_char))
        {
            let after = rest[name.len()..].trim_start();
            if let Some(inside) = after.strip_prefix('(')
                && let Some(close) = matching_close(inside)
            {
                found.push(&inside[..close]);
                i = content.len() - inside.len() + close + 1;
                prev_ident = false;
                continue;
            }
        }
        prev_ident = is_ident_char(c);
        i += c.len_utf8();
    }
    found
}

/// The raw value of the `key=` argument in a call body, trimmed, or None
/// when the call has no such argument.
fn arg<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    top_level_split(body, ',').into_iter().find_map(|piece| {
        let (k, v) = piece.split_once('=')?;
        (k.trim() == key).then(|| v.trim())
    })
}

/// The text of a string literal value, without its quotes; None for any
/// other value (a variable, a number, a list).
fn string_literal(raw: &str) -> Option<&str> {
    let first = raw.chars().next()?;
    if !matches!(first, '"' | '\'' | '`') || raw.len() < 2 || !raw.ends_with(first) {
        return None;
    }
    Some(&raw[1..raw.len() - 1])
}

/// The `key="..."` argument of a call body, or None when it is absent or
/// not a string literal.
fn string_arg<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    arg(body, key).and_then(string_literal)
}

/// The string literals of a `[...]` list value; None when the value is not
/// a literal list. Entries that are not string literals are not patterns
/// the analyzer can expand and are left out.
fn string_list(raw: &str) -> Option<Vec<String>> {
    let inner = raw.strip_prefix('[')?.strip_suffix(']')?;
    Some(
        top_level_split(inner, ',')
            .into_iter()
            .map(str::trim)
            .filter(|piece| !piece.is_empty())
            .filter_map(string_literal)
            .map(str::to_string)
            .collect(),
    )
}

/// Resolve a path a template names, as the tera processor does: against
/// the project root only. A file that another product generates is in
/// `file_index` before it exists.
fn resolve_project_path(path_str: &str, file_index: &FileIndex) -> Option<PathBuf> {
    if path_str.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(path_str);
    (candidate.is_file() || file_index.contains(&candidate)).then_some(candidate)
}

/// Add `path` to the dependency list once.
fn add_dep(paths: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if seen.insert(path.clone()) {
        paths.push(path);
    }
}

/// Scan `source` for dependencies and recurse into any `{% include %}`,
/// `{% import %}`, or `{% extends %}` referenced templates so that
/// `glob/git_count_files/shell_output` calls in *any* transitively-included
/// template participate in the parent product's dependency set and cache key.
///
/// `paths` and `seen` accumulate the input file set; `hash_pieces` accumulates
/// the config-hash contribution; `scanned` prevents revisiting the same
/// template (cycle guard).
fn scan_template_recursive(
    ctx: &crate::build_context::BuildContext,
    source: &Path,
    file_index: &FileIndex,
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    hash_pieces: &mut Vec<String>,
    scanned: &mut HashSet<PathBuf>,
) -> Result<()> {
    let canonical = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    if !scanned.insert(canonical) {
        return Ok(());
    }

    // A template that is itself a build output (e.g. a generated driver
    // template, or an included one that resolved through `file_index`)
    // does not exist before its producer runs, which is exactly the
    // situation on a cold build. Skip reading it: it is already a
    // dependency, which orders the consuming product behind the producer,
    // and once it is written the mid-build re-analysis reads it and picks
    // up its transitive dependencies. A genuinely missing hand-written
    // template still fails loudly when the tera processor tries to render
    // it.
    if !source.exists() {
        return Ok(());
    }

    let content = crate::errors::ctx(
        fs::read_to_string(source),
        &format!("Failed to read template: {}", source.display()),
    )?;

    // {% include "path" %}, {% import "path" %}, {% extends "path" %}
    static INCLUDE_RE: OnceLock<Regex> = OnceLock::new();
    let include_re = INCLUDE_RE.get_or_init(|| {
        Regex::new(r#"\{%[-~]?\s*(?:include|import|extends)\s+["']([^"']+)["']"#)
            .expect(errors::INVALID_REGEX)
    });

    // 1) include/import/extends and the loader functions. For
    // include/import/extends, also recurse into the included template so
    // its glob/shell_output/git_count calls participate in this product's
    // dependency set. The tera processor registers a template under its
    // project-relative path, so that is the only name an include resolves.
    for caps in include_re.captures_iter(&content) {
        let Some(candidate) = resolve_project_path(&caps[1], file_index) else {
            continue;
        };
        add_dep(paths, seen, candidate.clone());
        scan_template_recursive(
            ctx,
            &candidate,
            file_index,
            paths,
            seen,
            hash_pieces,
            scanned,
        )?;
    }
    for body in calls(&content, LOADER_FUNCTIONS) {
        if let Some(path_str) = string_arg(body, "path")
            && let Some(candidate) = resolve_project_path(path_str, file_index)
        {
            add_dep(paths, seen, candidate);
        }
    }
    // version_str() / version_str(path="...") — reads a `tup` from the file
    // at `path`, defaulting to config/version.py when called without one. A
    // `path` that is not a literal cannot be tracked.
    for body in calls(&content, &["version_str"]) {
        let path_str = match arg(body, "path") {
            None => VERSION_STR_DEFAULT_PATH,
            Some(raw) => match string_literal(raw) {
                Some(literal) => literal,
                None => continue,
            },
        };
        if let Some(candidate) = resolve_project_path(path_str, file_index) {
            add_dep(paths, seen, candidate);
        }
    }

    // 2) glob(pattern="...") — contribute only the resolved path set to the
    // cache hash. The matched files are NOT added as inputs: the template
    // consumes the list of *names*, not their content, so editing one of
    // those files must not invalidate this product. Adding/removing/renaming
    // a matching file changes the path-set fingerprint and rebuilds.
    for body in calls(&content, &["glob"]) {
        let Some(pattern) = string_arg(body, "pattern") else {
            continue;
        };
        let matched = expand_glob(pattern)?;
        hash_pieces.push(format!("glob:{pattern}"));
        hash_pieces.push(format!("glob_resolved:{}", matched.join("\n")));
    }

    // 3) git_count_files(pattern="...") — same path-set-only semantics as
    // glob. Only the count/identity of tracked files matters, not content.
    for body in calls(&content, &["git_count_files"]) {
        let Some(pattern) = string_arg(body, "pattern") else {
            continue;
        };
        let matched = git_ls_files(ctx, pattern);
        hash_pieces.push(format!("git_count:{pattern}"));
        hash_pieces.push(format!("git_count_resolved:{}", matched.join("\n")));
    }

    // 3b) grep_count(pattern="<regex>", glob="<file_glob>") — content-tracked.
    // Both the regex and the resolved file set go into the hash, AND the
    // matched files are added as inputs so mtime/checksum-tracked content
    // changes invalidate the product.
    for body in calls(&content, &["grep_count"]) {
        let Some(regex_pat) = string_arg(body, "pattern") else {
            bail!(
                "[tera] {}: grep_count(...) is missing pattern=\"<regex>\". Found: grep_count({})",
                source.display(),
                body.trim(),
            );
        };
        let Some(file_glob) = string_arg(body, "glob") else {
            bail!(
                "[tera] {}: grep_count(pattern=\"{}\") is missing glob=\"<file_glob>\".",
                source.display(),
                regex_pat,
            );
        };
        let matched = expand_glob(file_glob)?;
        hash_pieces.push(format!("grep_count_re:{regex_pat}"));
        hash_pieces.push(format!("grep_count_glob:{file_glob}"));
        hash_pieces.push(format!("grep_count_resolved:{}", matched.join("\n")));
        for p in matched {
            add_dep(paths, seen, PathBuf::from(p));
        }
    }

    // 3c) workflow_names() — content-tracked like grep_count: the renderer
    // reads each workflow file's `name:` field, so the matched files are
    // inputs (renaming a workflow's name must invalidate the product) and
    // the resolved path set is a hash piece (adding/removing a workflow file
    // must too). The pattern mirrors WorkflowNamesFunction in
    // processor/generator/tera.rs; this used to be untracked — a renamed
    // workflow left stale rendered output cached indefinitely.
    if !calls(&content, &["workflow_names"]).is_empty() {
        let matched = expand_glob(".github/workflows/*.yml")?;
        hash_pieces.push(format!("workflow_names_resolved:{}", matched.join("\n")));
        for p in matched {
            add_dep(paths, seen, PathBuf::from(p));
        }
    }

    // 4) shell_output(...): require depends_on, harvest patterns and command
    for body in calls(&content, &["shell_output"]) {
        let Some(command) = string_arg(body, "command") else {
            bail!(
                "[tera] {}: shell_output(...) call has no command= argument. \
                 Found: shell_output({})",
                source.display(),
                body.trim(),
            );
        };
        let Some(deps_raw) = arg(body, "depends_on") else {
            bail!(
                "[tera] {}: shell_output(command=\"{}\") is missing depends_on=[...].\n\
                 rsconstruct cannot otherwise tell when its output should be invalidated.\n\
                 Migrate to glob(pattern=\"...\") for directory queries, or pass an explicit \
                 list (e.g. depends_on=[\"marp/**/*.md\"]).\n\
                 If your command genuinely has no file dependencies, pass depends_on=[] \
                 to acknowledge that.",
                source.display(),
                command,
            );
        };
        let Some(patterns) = string_list(deps_raw) else {
            bail!(
                "[tera] {}: shell_output(command=\"{}\"): depends_on must be a literal list \
                 of glob patterns, found depends_on={}",
                source.display(),
                command,
                deps_raw,
            );
        };

        hash_pieces.push(format!("shell_cmd:{command}"));

        if patterns.is_empty() {
            hash_pieces.push("shell_deps:[]".to_string());
            continue;
        }
        for pattern in &patterns {
            let matched = expand_glob(pattern)?;
            hash_pieces.push(format!("shell_dep:{pattern}"));
            hash_pieces.push(format!("shell_dep_resolved:{}", matched.join("\n")));
            for p in matched {
                add_dep(paths, seen, PathBuf::from(p));
            }
        }
    }

    Ok(())
}

/// Run `git ls-files -- <pattern>` and return the sorted list of tracked
/// files matching the pathspec. This mirrors the runtime semantics of the
/// `git_count_files` Tera function so the analyzer's invalidation set
/// matches what the renderer actually counts. A failed git invocation
/// (e.g. not a git repository) yields an empty list — the function is
/// best-effort by design.
fn git_ls_files(ctx: &crate::build_context::BuildContext, pattern: &str) -> Vec<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["ls-files", "--", pattern]);
    let output = match crate::processor::run_command_capture(ctx, &cmd) {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut paths: Vec<String> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(std::string::ToString::to_string)
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// Expand a glob pattern into a sorted list of file paths (as strings, relative
/// to project root). Symlinks and directories are skipped — only regular files
/// participate. The sorted order matters for the deterministic hash piece.
fn expand_glob(pattern: &str) -> Result<Vec<String>> {
    let mut paths: Vec<String> = Vec::new();
    for entry in
        glob::glob(pattern).map_err(|e| anyhow::anyhow!("Invalid glob pattern '{pattern}': {e}"))?
    {
        let path =
            entry.map_err(|e| anyhow::anyhow!("Glob iteration error for '{pattern}': {e}"))?;
        if path.is_file() {
            paths.push(path.to_string_lossy().into_owned());
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

impl DepAnalyzer for TeraDepAnalyzer {
    fn description(&self) -> &'static str {
        "Scan Tera templates for include/import/extends dependencies"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        file_index.has_extension(".tera")
    }

    fn match_product(&self, p: &Product) -> Option<PathBuf> {
        if p.inputs.is_empty() {
            return None;
        }
        let source = &p.inputs[0];
        let ext = source.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext == "tera" {
            Some(source.clone())
        } else {
            None
        }
    }

    fn scan(
        &self,
        ctx: &crate::build_context::BuildContext,
        source: &Path,
        file_index: &FileIndex,
    ) -> Result<ScanResult> {
        self.scan_template(ctx, source, file_index)
    }

    /// The hash pieces (glob results, command text) depend on filesystem
    /// state the deps cache cannot represent.
    fn always_rescan(&self) -> bool {
        true
    }

    fn fingerprint_parts(&self, _ctx: &crate::build_context::BuildContext) -> Result<Vec<String>> {
        super::config_fingerprint(&self.config)
    }

    fn scan_hash_pieces(
        &self,
        ctx: &crate::build_context::BuildContext,
        source: &Path,
    ) -> Result<Option<Vec<String>>> {
        // Hash pieces come only from templates that are read, and a
        // template is read only once it exists on disk, so resolving
        // against the disk alone yields the pieces the build used.
        let disk_only = FileIndex::from_paths(Vec::new());
        Ok(Some(
            self.scan_template(ctx, source, &disk_only)?.hash_pieces,
        ))
    }
}

inventory::submit! {
    crate::registries::AnalyzerPlugin {
        name: "tera",
        description: "Scan Tera templates for include/import/extends dependencies",
        is_builtin: true,
        create: |toml_value, _| {
            let cfg: TeraAnalyzerConfig = toml::from_str(&toml::to_string(toml_value)?)?;
            Ok(Box::new(TeraDepAnalyzer::new(cfg)))
        },
        defconfig_toml: || {
            toml::to_string_pretty(&TeraAnalyzerConfig::default()).ok()
        },
        known_fields: crate::registries::typed_known_fields::<TeraAnalyzerConfig>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bodies<'a>(content: &'a str, name: &str) -> Vec<&'a str> {
        calls(content, &[name])
    }

    /// A `)` inside a quoted argument does not close the call, whichever
    /// quote style the string uses, and the arguments after it survive.
    #[test]
    fn calls_keep_parentheses_inside_quoted_arguments() {
        let content = r#"{{ shell_output(command="echo $(uname -r)", depends_on=["foo"]) }}"#;
        let found = bodies(content, "shell_output");
        assert_eq!(
            found,
            vec![r#"command="echo $(uname -r)", depends_on=["foo"]"#]
        );
        assert_eq!(string_arg(found[0], "command"), Some("echo $(uname -r)"));
        assert_eq!(
            arg(found[0], "depends_on").and_then(string_list),
            Some(vec!["foo".to_string()])
        );

        let single = "{{ shell_output(command='ls (x)', depends_on=[]) }}";
        let found = bodies(single, "shell_output");
        assert_eq!(string_arg(found[0], "command"), Some("ls (x)"));
        assert_eq!(
            arg(found[0], "depends_on").and_then(string_list),
            Some(vec![])
        );

        let backtick = "{{ grep_count(pattern=`a)b`, glob=\"src/*.rs\") }}";
        let found = bodies(backtick, "grep_count");
        assert_eq!(string_arg(found[0], "pattern"), Some("a)b"));
        assert_eq!(string_arg(found[0], "glob"), Some("src/*.rs"));
    }

    /// A quote of the other kind inside a string is just a character, so
    /// `command="echo 'hi'"` is the whole command, not `echo `.
    #[test]
    fn string_arguments_may_contain_the_other_quote() {
        let content = r#"{{ shell_output(command="echo 'a, b'", depends_on=['x', "y"]) }}"#;
        let found = bodies(content, "shell_output");
        assert_eq!(string_arg(found[0], "command"), Some("echo 'a, b'"));
        assert_eq!(
            arg(found[0], "depends_on").and_then(string_list),
            Some(vec!["x".to_string(), "y".to_string()])
        );
    }

    /// Arguments come in any order, several calls are all found in file
    /// order, and whitespace around `(`, `=` and `,` is free.
    #[test]
    fn calls_find_every_call_with_arguments_in_any_order() {
        let content = "A {{ grep_count(glob=\"a/*.md\", pattern=\"x\") }}\n\
                       B {{ grep_count (\n pattern = \"y\" ,\n glob = \"b/*.md\"\n) }}";
        let found = bodies(content, "grep_count");
        assert_eq!(found.len(), 2);
        assert_eq!(string_arg(found[0], "pattern"), Some("x"));
        assert_eq!(string_arg(found[0], "glob"), Some("a/*.md"));
        assert_eq!(string_arg(found[1], "pattern"), Some("y"));
        assert_eq!(string_arg(found[1], "glob"), Some("b/*.md"));
        assert_eq!(string_arg(found[1], "nope"), None);
    }

    /// `glob=` inside `grep_count(...)` and `my_glob(...)` are not calls of
    /// `glob`; an unterminated call is skipped; a non-literal value is not
    /// a string argument.
    #[test]
    fn calls_respect_identifier_boundaries_and_literals() {
        let content = "{{ grep_count(pattern=\"x\", glob=\"*.md\") }} {{ my_glob(pattern=\"y\") }} \
                       {{ glob_two(pattern=\"z\") }} {{ glob(pattern=\"ok\") }}";
        assert_eq!(bodies(content, "glob"), vec!["pattern=\"ok\""]);

        assert_eq!(
            bodies("{{ glob(pattern=\"never closed\" }}", "glob"),
            Vec::<&str>::new()
        );

        let found = bodies("{{ glob(pattern=some_var) }}", "glob");
        assert_eq!(found, vec!["pattern=some_var"]);
        assert_eq!(arg(found[0], "pattern"), Some("some_var"));
        assert_eq!(string_arg(found[0], "pattern"), None);
        assert_eq!(string_list("some_list"), None);

        assert_eq!(bodies("{{ version_str() }}", "version_str"), vec![""]);
    }

    fn scan(dir: &Path, template: &str, index: &[&str]) -> ScanResult {
        let source = dir.join("tpl/main.tera");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, template).unwrap();
        let file_index = FileIndex::from_paths(index.iter().map(PathBuf::from).collect());
        let ctx = crate::build_context::BuildContext::new();
        TeraDepAnalyzer::new(TeraAnalyzerConfig::default())
            .scan_template(&ctx, &source, &file_index)
            .unwrap()
    }

    /// Paths resolve against the project root only, as the tera processor
    /// resolves them: a file next to the template that is not also a
    /// project-relative path is not a dependency, while a project-relative
    /// path is, whether on disk or only declared in the file index.
    #[test]
    fn paths_resolve_against_the_project_root_only() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path();
        // Beside the template: the old source-directory resolution found
        // these, the renderer never reads them.
        fs::create_dir_all(dir.join("tpl")).unwrap();
        fs::write(dir.join("tpl/part.tera"), "").unwrap();
        fs::write(dir.join("tpl/cfg.py"), "").unwrap();

        let result = scan(
            dir,
            "{% include \"part.tera\" %}\n\
             {% import \"tpl/macros.tera\" as m %}\n\
             {% set c = load_python(path='cfg.py') %}\n\
             {% set d = load_toml(path=\"cfg/data.toml\") %}\n\
             {{ version_str() }}\n\
             {{ version_str(path=\"cfg/version.lua\") }}\n\
             {{ toml_get(key=\"a.b\", path=\"pyproject.toml\") }}\n",
            &[
                "tpl/macros.tera",
                "cfg/data.toml",
                "config/version.py",
                "cfg/version.lua",
                "pyproject.toml",
            ],
        );
        let deps: Vec<String> = result
            .deps
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert_eq!(
            deps,
            vec![
                "tpl/macros.tera",
                "cfg/data.toml",
                "pyproject.toml",
                "config/version.py",
                "cfg/version.lua",
            ]
        );
    }

    /// A `shell_output` whose command contains `)` is complete: its
    /// `depends_on` is seen, and the command literal is the hash piece.
    #[test]
    fn shell_output_with_parentheses_in_command_is_accepted() {
        let temp = tempfile::TempDir::new().unwrap();
        let result = scan(
            temp.path(),
            "{{ shell_output(command=\"echo $(uname -r)\", depends_on=[]) }}\n",
            &[],
        );
        assert_eq!(
            result.hash_pieces,
            vec!["shell_cmd:echo $(uname -r)", "shell_deps:[]"]
        );
    }

    /// `depends_on` is required, and must be a literal list.
    #[test]
    fn shell_output_requires_a_literal_depends_on() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path();
        let source = dir.join("main.tera");
        let ctx = crate::build_context::BuildContext::new();
        let index = FileIndex::from_paths(Vec::new());
        let analyzer = TeraDepAnalyzer::new(TeraAnalyzerConfig::default());

        fs::write(&source, "{{ shell_output(command=\"date\") }}").unwrap();
        let err = analyzer.scan_template(&ctx, &source, &index).unwrap_err();
        assert!(
            err.to_string().contains("is missing depends_on=[...]"),
            "{err}"
        );

        fs::write(
            &source,
            "{{ shell_output(command=\"date\", depends_on=deps) }}",
        )
        .unwrap();
        let err = analyzer.scan_template(&ctx, &source, &index).unwrap_err();
        assert!(
            err.to_string()
                .contains("depends_on must be a literal list"),
            "{err}"
        );
    }
}

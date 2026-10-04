//! The zola mass generator: plans what `zola build` writes by reading the
//! site's sources the way zola does, then runs the official zola once and
//! moves its output into place.
//!
//! Zola has no plan mode of its own (see `contrib/zola` for a patch that adds
//! one), so this file reimplements the part of zola that decides *where*
//! files go — content paths, sections, pagination, taxonomies, feeds, aliases,
//! Sass, static files — against one pinned zola version. Everything it does
//! not model it refuses by name instead of guessing, and the plan-vs-build
//! check fails the build if the plan and zola's output ever disagree.

// Every suffix test here (`.md`, `.html`, `.scss`) is case-sensitive on
// purpose: zola compares exactly, and the planner exists to match zola.
#![allow(clippy::case_sensitive_file_extension_comparisons)]

use anyhow::{Context, Result};
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, LazyLock};

use super::{OnceTool, PlannedOutput, add_planned_products, require_planned_output};
use crate::config::{StandardConfig, checksum_fields_of, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{
    Processor, ProcessorBase, check_command_output, format_command, log_command, run_command,
};

/// The zola release whose path rules this file reproduces. Any other version
/// is refused: a zola upgrade can move files, and the right reaction is to
/// re-verify this planner against it, not to hope.
const ZOLA_VERSION: &str = "0.23.3";

/// Above this many URLs zola splits the sitemap into `sitemapN.xml` files,
/// which this planner does not model.
const SITEMAP_SPLIT: usize = 30_000;

/// Where zola builds before its files are moved into place.
const STAGING_DIR: &str = ".rsconstruct/mass_generator";

/// Zola processor config.
/// Custom fields: `root`, `loose_manifest`. `output_dir` (standard) overrides
/// the site's own `output_dir`. Unused `StandardConfig` fields: formats,
/// `dep_auto`, batch, and the scan fields — zola decides what it reads.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ZolaConfig {
    /// The site directory (holding `config.toml` or `zola.toml`), relative
    /// to the project root; empty means the project root.
    #[serde(default)]
    pub root: String,
    /// Report plan-vs-build mismatches as warnings instead of errors.
    #[serde(default)]
    pub loose_manifest: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

/// The computed plan of one instance.
struct Plan {
    /// Planned outputs, paths relative to the project root.
    outputs: Vec<PlannedOutput>,
    /// The same files relative to the output directory, as zola writes them.
    relative: BTreeSet<String>,
    /// The output directory, relative to the project root.
    output_dir: PathBuf,
}

pub struct ZolaProcessor {
    config: ZolaConfig,
    plan: Mutex<Option<Arc<Plan>>>,
    tool_run: OnceTool,
}

impl ZolaProcessor {
    pub const fn new(config: ZolaConfig) -> Self {
        Self {
            config,
            plan: Mutex::new(None),
            tool_run: OnceTool::new(),
        }
    }

    fn site_root(&self) -> PathBuf {
        PathBuf::from(&self.config.root)
    }

    /// The plan, computed once per process (discovery runs several passes).
    fn plan(&self, instance_name: &str) -> Result<Arc<Plan>> {
        let mut guard = self.plan.lock();
        if let Some(plan) = guard.as_ref() {
            return Ok(Arc::clone(plan));
        }
        check_zola_version(&self.config.standard.command)?;
        let site = Site::load(&self.site_root())
            .with_context(|| format!("[{instance_name}] cannot plan the zola site"))?;
        let output_dir = if self.config.standard.output_dir.is_empty() {
            self.site_root().join(&site.config.output_dir)
        } else {
            PathBuf::from(&self.config.standard.output_dir)
        };
        let relative_plan = site
            .plan()
            .with_context(|| format!("[{instance_name}] cannot plan the zola site"))?;
        let relative: BTreeSet<String> = relative_plan.keys().cloned().collect();
        let outputs = relative_plan
            .into_iter()
            .map(|(rel, sources)| PlannedOutput {
                path: output_dir.join(rel),
                sources,
            })
            .collect();
        let plan = Arc::new(Plan {
            outputs,
            relative,
            output_dir,
        });
        *guard = Some(Arc::clone(&plan));
        Ok(plan)
    }

    /// One `zola build` into a private staging directory, the plan check
    /// against what it wrote, and the move of the planned files into the
    /// real output directory.
    ///
    /// Staging is what makes a shared output directory safe: `zola build`
    /// deletes its output directory before writing, which would take every
    /// other processor's files in `_site/` with it — files rsconstruct then
    /// believes are up to date.
    fn run_tool(
        &self,
        ctx: &crate::build_context::BuildContext,
        instance_name: &str,
    ) -> Result<()> {
        let plan =
            self.plan.lock().clone().with_context(|| {
                format!("[{instance_name}] executed before its plan was loaded")
            })?;
        let staging = Path::new(STAGING_DIR).join(format!("{instance_name}.staging"));
        remove_dir_if_present(&staging)?;
        if let Some(parent) = staging.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }

        let mut cmd = Command::new(&self.config.standard.command);
        if !self.config.root.is_empty() {
            cmd.arg("--root").arg(&self.config.root);
        }
        cmd.arg("build")
            .arg("--output-dir")
            .arg(&staging)
            .arg("--force");
        cmd.args(&self.config.standard.args);
        let output = run_command(ctx, &cmd)?;
        check_command_output(
            &output,
            format_args!("[{instance_name}] {}", format_command(&cmd)),
        )?;

        let written: BTreeSet<String> = files_under(&staging)?
            .into_iter()
            .map(|p| relative_slash_path(&staging, &p))
            .collect();
        let mut mismatches: Vec<String> = plan
            .relative
            .difference(&written)
            .map(|p| format!("missing: {p} (planned but not written by zola)"))
            .collect();
        mismatches.extend(
            written
                .difference(&plan.relative)
                .map(|p| format!("unexpected: {p} (written by zola but not planned)")),
        );
        if !mismatches.is_empty() {
            let msg = format!(
                "[{instance_name}] zola's output does not match the plan (staging kept at {}):\n  {}",
                staging.display(),
                mismatches.join("\n  ")
            );
            if self.config.loose_manifest {
                crate::output::warn(&format!("{msg}\n  (loose_manifest = true: continuing)"));
            } else {
                anyhow::bail!("{msg}");
            }
        }

        // Move every planned file zola wrote into place. A planned file that
        // is already there may be a read-only hardlink from a cache restore,
        // so it is unlinked rather than overwritten.
        for rel in plan.relative.intersection(&written) {
            let from = staging.join(rel);
            let to = plan.output_dir.join(rel);
            match std::fs::remove_file(&to) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(anyhow::Error::from(e)
                        .context(format!("Failed to remove stale output {}", to.display())));
                }
            }
            crate::processor::ensure_output_dir(&to)?;
            std::fs::rename(&from, &to).with_context(|| {
                format!("Failed to move {} to {}", from.display(), to.display())
            })?;
        }
        remove_dir_if_present(&staging)
    }
}

/// `zola --version` must print exactly the version this planner models.
fn check_zola_version(command: &str) -> Result<()> {
    let mut cmd = Command::new(command);
    cmd.arg("--version");
    log_command(&cmd);
    let output = cmd
        .output()
        .with_context(|| format!("Failed to run {}", format_command(&cmd)))?;
    check_command_output(&output, format_args!("{}", format_command(&cmd)))?;
    let printed = String::from_utf8_lossy(&output.stdout);
    let expected = format!("zola {ZOLA_VERSION}");
    if printed.trim() != expected {
        anyhow::bail!(
            "{} printed `{}`; the zola processor models {expected} exactly. Install zola \
             {ZOLA_VERSION} (`rsconstruct tools install zola`), or verify the planner against \
             the new version before changing ZOLA_VERSION",
            format_command(&cmd),
            printed.trim()
        );
    }
    Ok(())
}

fn remove_dir_if_present(dir: &Path) -> Result<()> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            Err(anyhow::Error::from(e).context(format!("Failed to remove {}", dir.display())))
        }
    }
}

/// Every regular file under `dir`, following symlinks as zola's walkers do.
/// A missing directory is empty.
fn files_under(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("Failed to read directory {}", current.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("Failed to read {}", current.display()))?;
            let path = entry.path();
            let meta = std::fs::metadata(&path)
                .with_context(|| format!("Failed to stat {}", path.display()))?;
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// `path` below `base`, with `/` separators.
fn relative_slash_path(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

// ---------------------------------------------------------------------------
// The site model: zola's config and content, read the way zola 0.23.3 reads
// them. Field defaults are zola's.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
enum SlugifyStrategy {
    #[default]
    On,
    Safe,
    Off,
}

/// zola's `slugify_paths`.
fn slugify_paths(s: &str, strategy: SlugifyStrategy) -> String {
    match strategy {
        SlugifyStrategy::On => slug::slugify(s),
        SlugifyStrategy::Safe => {
            let trimmed = s.trim_end_matches([' ', '.']);
            trimmed
                .chars()
                .filter(|c| !r#"<>:"/\|?*"#.contains(*c))
                .collect()
        }
        SlugifyStrategy::Off => s.to_string(),
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct SlugifyConfig {
    paths: SlugifyStrategy,
    paths_keep_dates: bool,
    taxonomies: SlugifyStrategy,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
struct TaxonomyConfig {
    name: String,
    paginate_by: Option<usize>,
    paginate_path: Option<String>,
    render: bool,
    feed: bool,
}

impl Default for TaxonomyConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            paginate_by: None,
            paginate_path: None,
            render: true,
            feed: false,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
struct LanguageOptions {
    generate_feeds: bool,
    feed_filenames: Vec<String>,
    taxonomies: Vec<TaxonomyConfig>,
    build_search_index: bool,
}

impl Default for LanguageOptions {
    fn default() -> Self {
        Self {
            generate_feeds: false,
            feed_filenames: vec!["atom.xml".to_string()],
            taxonomies: Vec::new(),
            build_search_index: false,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct SearchConfig {
    index_format: String,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            index_format: "elasticlunr_javascript".to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct HighlightingConfig {
    style: String,
    theme: Option<String>,
    light_theme: Option<String>,
    dark_theme: Option<String>,
}

impl Default for HighlightingConfig {
    fn default() -> Self {
        Self {
            style: "inline".to_string(),
            theme: None,
            light_theme: None,
            dark_theme: None,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct MarkdownConfig {
    highlighting: Option<HighlightingConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct SiteConfig {
    default_language: String,
    languages: BTreeMap<String, LanguageOptions>,
    generate_feeds: bool,
    feed_filenames: Vec<String>,
    taxonomies: Vec<TaxonomyConfig>,
    build_search_index: bool,
    taxonomy_root: Option<String>,
    compile_sass: bool,
    generate_sitemap: bool,
    generate_robots_txt: bool,
    output_dir: String,
    slugify: SlugifyConfig,
    search: SearchConfig,
    markdown: MarkdownConfig,
    theme: Option<String>,
    ignored_content: Vec<String>,
    ignored_static: Vec<String>,
}

impl Default for SiteConfig {
    fn default() -> Self {
        Self {
            default_language: "en".to_string(),
            languages: BTreeMap::new(),
            generate_feeds: false,
            feed_filenames: vec!["atom.xml".to_string()],
            taxonomies: Vec::new(),
            build_search_index: false,
            taxonomy_root: None,
            compile_sass: false,
            generate_sitemap: true,
            generate_robots_txt: true,
            output_dir: "public".to_string(),
            slugify: SlugifyConfig::default(),
            search: SearchConfig::default(),
            markdown: MarkdownConfig::default(),
            theme: None,
            ignored_content: Vec::new(),
            ignored_static: Vec::new(),
        }
    }
}

impl SiteConfig {
    /// Every language with its options: the configured `[languages.*]` plus
    /// the default language built from the top-level settings (zola's
    /// `add_default_language`).
    fn all_languages(&self) -> BTreeMap<String, LanguageOptions> {
        let mut all = self.languages.clone();
        all.insert(
            self.default_language.clone(),
            LanguageOptions {
                generate_feeds: self.generate_feeds,
                feed_filenames: self.feed_filenames.clone(),
                taxonomies: self.taxonomies.clone(),
                build_search_index: self.build_search_index,
            },
        );
        all
    }

    /// `/{lang}` for a non-default language, nothing for the default one.
    fn lang_prefix(&self, lang: &str) -> Option<String> {
        (lang != self.default_language).then(|| lang.to_string())
    }

    fn index_filename(&self, lang: &str) -> String {
        if lang == self.default_language {
            "_index.md".to_string()
        } else {
            format!("_index.{lang}.md")
        }
    }
}

/// Page front matter fields that decide where (and whether) a page renders.
#[derive(Debug, Deserialize)]
#[serde(default)]
struct PageMeta {
    title: Option<toml::Value>,
    date: Option<toml::Value>,
    updated: Option<toml::Value>,
    draft: bool,
    render: bool,
    slug: Option<String>,
    path: Option<String>,
    taxonomies: BTreeMap<String, Vec<String>>,
    weight: Option<toml::Value>,
    aliases: Vec<String>,
    hidden: Option<bool>,
}

impl Default for PageMeta {
    fn default() -> Self {
        Self {
            title: None,
            date: None,
            updated: None,
            draft: false,
            render: true,
            slug: None,
            path: None,
            taxonomies: BTreeMap::new(),
            weight: None,
            aliases: Vec::new(),
            hidden: None,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
struct SectionMeta {
    draft: bool,
    render: bool,
    transparent: bool,
    hidden: Option<bool>,
    sort_by: String,
    paginate_by: Option<usize>,
    paginate_path: String,
    generate_feeds: bool,
    aliases: Vec<String>,
}

impl Default for SectionMeta {
    fn default() -> Self {
        Self {
            draft: false,
            render: true,
            transparent: false,
            hidden: None,
            sort_by: "none".to_string(),
            paginate_by: None,
            paginate_path: "page".to_string(),
            generate_feeds: false,
            aliases: Vec::new(),
        }
    }
}

static TOML_FRONT_MATTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[[:space:]]*\+\+\+[[:space:]]*(\r?\n(?s).*?(?-s))\+\+\+[[:space:]]*(?:$|(?:\r?\n((?s).*(?-s))$))",
    )
    .expect(crate::errors::INVALID_REGEX)
});

static YAML_FRONT_MATTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[[:space:]]*---[[:space:]]*(\r?\n(?s).*?(?-s))---[[:space:]]*(?:$|(?:\r?\n((?s).*(?-s))$))",
    )
    .expect(crate::errors::INVALID_REGEX)
});

/// A date at the start of a file or directory name, optionally followed by
/// `_` or `-` and the slug (zola's `RFC3339_DATE`).
static DATED_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<datetime>(\d{4})-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])(T([01][0-9]|2[0-3]):([0-5][0-9]):([0-5][0-9]|60)(\.[0-9]+)?(Z|(\+|-)([01][0-9]|2[0-3]):([0-5][0-9])))?)(\s?(_|-)(?P<slug>.+$))?",
    )
    .expect(crate::errors::INVALID_REGEX)
});

/// A literal `load_data(path = "...")` in a template or content file.
static LOAD_DATA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"load_data\s*\(\s*path\s*=\s*["']([^"']+)["']"#)
        .expect(crate::errors::INVALID_REGEX)
});

/// Parse a file's front matter into `T` (TOML between `+++`, YAML between
/// `---`), as zola's `split_content` does.
fn parse_front_matter<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    if let Some(caps) = TOML_FRONT_MATTER.captures(&content) {
        toml::from_str(&caps[1])
            .with_context(|| format!("Invalid TOML front matter in {}", path.display()))
    } else if let Some(caps) = YAML_FRONT_MATTER.captures(&content) {
        serde_yaml_ng::from_str(&caps[1])
            .with_context(|| format!("Invalid YAML front matter in {}", path.display()))
    } else {
        anyhow::bail!(
            "{} has no front matter (zola requires `+++` or `---`)",
            path.display()
        )
    }
}

/// Whether a front-matter date value is one zola parses (its
/// `parse_datetime`): a TOML datetime, or an RFC 3339 / `YYYY-MM-DD` string.
fn is_parseable_date(value: &toml::Value) -> bool {
    match value {
        toml::Value::Datetime(_) => true,
        toml::Value::String(s) => {
            chrono::DateTime::parse_from_rfc3339(s).is_ok()
                || chrono::DateTime::parse_from_rfc3339(&format!("{s}Z")).is_ok()
                || chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
        }
        _ => false,
    }
}

/// zola's `is_temp_file`: editor leftovers that are never assets.
fn is_temp_file(path: &Path) -> bool {
    let Some(ext) = path.extension().map(|e| e.to_string_lossy().into_owned()) else {
        return false;
    };
    matches!(
        ext.as_str(),
        "swp" | "swx" | "tmp" | ".DS_STORE" | ".DS_Store" | "kate-swp"
    ) || ext.ends_with("jb_old___")
        || ext.ends_with("jb_tmp___")
        || ext.ends_with("jb_bak___")
        || ext.ends_with('~')
        || ext.ends_with("bck")
}

/// Non-markdown, non-temporary files of a content directory: a section's
/// assets (`recursive = false`) or a colocated page's (`recursive = true`).
fn related_assets(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut assets = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("Failed to read directory {}", current.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("Failed to read {}", current.display()))?;
            let path = entry.path();
            let meta = std::fs::metadata(&path)
                .with_context(|| format!("Failed to stat {}", path.display()))?;
            if meta.is_dir() {
                if recursive {
                    stack.push(path);
                }
            } else if meta.is_file()
                && !is_temp_file(&path)
                && path.extension().is_none_or(|e| e != "md")
            {
                assets.push(path);
            }
        }
    }
    assets.sort();
    Ok(assets)
}

struct Section {
    /// The `_index*.md` file; `None` for a default index section zola
    /// creates when a language has none.
    file: Option<PathBuf>,
    lang: String,
    components: Vec<String>,
    /// `components` joined + the index filename: the key zola resolves
    /// hidden-ness by.
    relative: String,
    meta: SectionMeta,
    hidden: bool,
    /// Rendered pages, sorted out of the ones the sort key cannot order.
    pages: Vec<usize>,
    hidden_pages: Vec<usize>,
    assets: Vec<PathBuf>,
    /// The directory holding the section's index file.
    dir: PathBuf,
}

impl Section {
    /// Output directory of the section's pages, relative to the output root:
    /// language prefix + components (zola's `Queue::add_section_jobs`).
    fn base(&self, config: &SiteConfig) -> String {
        let mut parts: Vec<String> = config.lang_prefix(&self.lang).into_iter().collect();
        parts.extend(self.components.iter().cloned());
        parts.join("/")
    }

    /// `section.path` without its leading `/` (zola's section feed base).
    fn path_without_slash(&self, config: &SiteConfig) -> String {
        let base = self.base(config);
        if base.is_empty() {
            String::new()
        } else {
            format!("{base}/")
        }
    }
}

struct Page {
    file: PathBuf,
    lang: String,
    /// The page's output directory with a trailing `/` and no leading one
    /// (`en/blog/post/`); empty for a page at the site root.
    out_dir: String,
    /// The directory the page's section index file would be in.
    parent: PathBuf,
    meta: PageMeta,
    /// Has a date (front matter or file name): sortable by date.
    dated: bool,
    /// Colocated assets and the directory they are relative to.
    assets: Vec<PathBuf>,
    asset_root: PathBuf,
    hidden: bool,
    in_a_section: bool,
}

/// The sources of one planned file: the shared set every rendered file
/// depends on, and/or files of its own. Rendered files share one list —
/// copying ~900 paths into a per-file set for ~1,400 files was most of the
/// planner's run time.
#[derive(Default)]
struct PlanSources {
    rendered: bool,
    files: BTreeSet<PathBuf>,
}

type PlanMap = BTreeMap<String, PlanSources>;

struct Site {
    root: PathBuf,
    config_file: PathBuf,
    config: SiteConfig,
    sections: Vec<Section>,
    pages: Vec<Page>,
}

/// `name` split into stem and language the way zola's `find_language` does:
/// `foo.he` is `foo` in `he` when `he` is configured, the default language
/// otherwise. An unconfigured code is an error, as in zola.
fn split_language(name: &str, config: &SiteConfig, path: &Path) -> Result<(String, String)> {
    if config.languages.is_empty() || !name.contains('.') {
        return Ok((name.to_string(), config.default_language.clone()));
    }
    let (stem, code) = name.split_once('.').unwrap_or((name, ""));
    if code == config.default_language {
        return Ok((name.to_string(), config.default_language.clone()));
    }
    if !config.languages.contains_key(code) {
        anyhow::bail!(
            "{} has a language code of {code} which isn't present in the config's `languages`",
            path.display()
        );
    }
    Ok((stem.to_string(), code.to_string()))
}

impl Site {
    fn load(root: &Path) -> Result<Self> {
        let config_file = ["zola.toml", "config.toml"]
            .iter()
            .map(|name| root.join(name))
            .find(|p| p.is_file())
            .with_context(|| format!("no zola.toml or config.toml in {}", root.display()))?;
        let text = std::fs::read_to_string(&config_file)
            .with_context(|| format!("Failed to read {}", config_file.display()))?;
        let config: SiteConfig = toml::from_str(&text)
            .with_context(|| format!("Invalid zola config {}", config_file.display()))?;
        Self::refuse_unmodeled_config(&config, &config_file)?;
        let mut site = Self {
            root: root.to_path_buf(),
            config_file,
            config,
            sections: Vec::new(),
            pages: Vec::new(),
        };
        site.load_content()?;
        site.assign_pages();
        Ok(site)
    }

    /// Features whose effect on output paths this planner does not model.
    fn refuse_unmodeled_config(config: &SiteConfig, file: &Path) -> Result<()> {
        let refuse = |what: &str| -> Result<()> {
            anyhow::bail!(
                "{}: {what} is not supported by the zola processor (it cannot predict the \
                 files that produces); see docs/src/processor/mass_generator/zola.md",
                file.display()
            )
        };
        if config.theme.is_some() {
            return refuse("`theme`");
        }
        if !config.ignored_content.is_empty() {
            return refuse("`ignored_content`");
        }
        if !config.ignored_static.is_empty() {
            return refuse("`ignored_static`");
        }
        if config.languages.contains_key(&config.default_language) {
            return refuse("a `[languages]` table for the default language");
        }
        Ok(())
    }

    fn content_dir(&self) -> PathBuf {
        self.root.join("content")
    }

    /// Walk `content/` the way `Site::load` does: a directory's index files
    /// first (a draft one stops the walk below that directory), then its
    /// pages, then its subdirectories.
    fn load_content(&mut self) -> Result<()> {
        let languages = self.config.all_languages();
        let mut index_names: BTreeMap<String, String> = BTreeMap::new();
        for lang in languages.keys() {
            index_names.insert(self.config.index_filename(lang), lang.clone());
        }
        let content = self.content_dir();
        let mut page_files = Vec::new();
        let mut stack = vec![content.clone()];
        while let Some(dir) = stack.pop() {
            let mut skip_below = false;
            let mut files = Vec::new();
            let mut subdirs = Vec::new();
            let entries = std::fs::read_dir(&dir)
                .with_context(|| format!("Failed to read directory {}", dir.display()))?;
            for entry in entries {
                let entry = entry.with_context(|| format!("Failed to read {}", dir.display()))?;
                let path = entry.path();
                let meta = std::fs::metadata(&path)
                    .with_context(|| format!("Failed to stat {}", path.display()))?;
                if meta.is_dir() {
                    subdirs.push(path);
                } else {
                    files.push(path);
                }
            }
            files.sort();
            subdirs.sort();
            for file in &files {
                let name = file.file_name().unwrap_or_default().to_string_lossy();
                if let Some(lang) = index_names.get(name.as_ref()) {
                    let meta: SectionMeta = parse_front_matter(file)?;
                    if meta.draft {
                        skip_below = true;
                        continue;
                    }
                    let components = content_components(&content, &dir);
                    self.sections.push(Section {
                        file: Some(file.clone()),
                        lang: lang.clone(),
                        relative: join_relative(&components, &name),
                        components,
                        meta,
                        hidden: false,
                        pages: Vec::new(),
                        hidden_pages: Vec::new(),
                        assets: related_assets(&dir, false)?,
                        dir: dir.clone(),
                    });
                }
            }
            if skip_below {
                continue;
            }
            for file in files {
                let name = file
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                if name.starts_with("_index.") || name.starts_with('.') || !name.ends_with(".md") {
                    continue;
                }
                page_files.push(file);
            }
            stack.extend(subdirs.into_iter().rev());
        }
        for file in page_files {
            if let Some(page) = self.load_page(&file)? {
                self.pages.push(page);
            }
        }
        // zola creates a default index section for every language without one.
        for lang in languages.keys() {
            let filename = self.config.index_filename(lang);
            if !self
                .sections
                .iter()
                .any(|s| s.dir == content && s.lang == *lang)
            {
                self.sections.push(Section {
                    file: None,
                    lang: lang.clone(),
                    components: Vec::new(),
                    relative: filename,
                    meta: SectionMeta::default(),
                    hidden: false,
                    pages: Vec::new(),
                    hidden_pages: Vec::new(),
                    assets: Vec::new(),
                    dir: content.clone(),
                });
            }
        }
        Ok(())
    }

    /// A page's language, slug and output directory, as zola's `Page::parse`
    /// computes them. `None` for a draft.
    fn load_page(&self, file: &Path) -> Result<Option<Page>> {
        let meta: PageMeta = parse_front_matter(file)?;
        if meta.draft {
            return Ok(None);
        }
        let content = self.content_dir();
        let stem = file
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut components = content_components(&content, file.parent().unwrap_or(&content));
        let mut parent = file.parent().unwrap_or(&content).to_path_buf();
        // `index.md` (or `index.<lang>.md`) in a directory is a colocated
        // page: the directory is the page, not a component.
        let colocated = !components.is_empty() && stem.split('.').next() == Some("index");
        if colocated {
            components.pop();
            parent = parent.parent().unwrap_or(&content).to_path_buf();
        }
        let (name, lang) = split_language(&stem, &self.config, file)?;

        let slug_source = if name == "index" {
            file.parent()
                .and_then(Path::file_name)
                .map_or_else(|| name.clone(), |n| n.to_string_lossy().into_owned())
        } else {
            name.clone()
        };
        let mut dated = meta.date.is_some();
        let mut slug_from_dated_name = None;
        if let Some(caps) = DATED_NAME.captures(&slug_source) {
            if let Some(slug) = caps.name("slug")
                && !self.config.slugify.paths_keep_dates
            {
                slug_from_dated_name = Some(slug.as_str().to_string());
            }
            dated = true;
        }
        let strategy = self.config.slugify.paths;
        let slug = if let Some(ref s) = meta.slug {
            slugify_paths(s, strategy)
        } else if let Some(s) = slug_from_dated_name {
            slugify_paths(&s, strategy)
        } else {
            slugify_paths(&slug_source, strategy)
        };

        let mut path = if let Some(ref p) = meta.path {
            let p = p.trim();
            if p.starts_with('/') {
                p.to_string()
            } else {
                format!("/{p}")
            }
        } else {
            let mut path = if components.is_empty() {
                if name == "index" && !colocated {
                    String::new()
                } else {
                    slug
                }
            } else {
                format!("{}/{slug}", components.join("/"))
            };
            if lang != self.config.default_language {
                path = format!("{lang}/{path}");
            }
            format!("/{path}")
        };
        if !path.ends_with('/') {
            path.push('/');
        }
        let out_dir = path.trim_start_matches('/').to_string();

        let page_dir = file.parent().unwrap_or(&content).to_path_buf();
        let assets = if name == "index" {
            related_assets(&page_dir, true)?
        } else {
            Vec::new()
        };
        Ok(Some(Page {
            file: file.to_path_buf(),
            lang,
            out_dir,
            parent,
            meta,
            dated,
            assets,
            asset_root: page_dir,
            hidden: false,
            in_a_section: false,
        }))
    }

    /// Section visibility, page-to-section assignment (through transparent
    /// sections), and the pages each section's sort key cannot order — zola
    /// renders none of those (`populate_sections`, `sort_section_pages`).
    fn assign_pages(&mut self) {
        let by_file: HashMap<PathBuf, usize> = self
            .sections
            .iter()
            .enumerate()
            .map(|(i, s)| (s.dir.join(self.config.index_filename(&s.lang)), i))
            .collect();
        let hidden_by_relative: HashMap<String, Option<bool>> = self
            .sections
            .iter()
            .map(|s| (s.relative.clone(), s.meta.hidden))
            .collect();

        // A section without `hidden` takes its nearest ancestor's.
        for section in &mut self.sections {
            let filename = self.config.index_filename(&section.lang);
            let mut hidden = section.meta.hidden;
            if hidden.is_none() && !section.components.is_empty() {
                let mut ancestors = vec![filename.clone()];
                for depth in 1..section.components.len() {
                    ancestors.push(join_relative(&section.components[..depth], &filename));
                }
                for ancestor in ancestors.iter().rev() {
                    if let Some(Some(value)) = hidden_by_relative.get(ancestor) {
                        hidden = Some(*value);
                        break;
                    }
                }
            }
            section.hidden = hidden.unwrap_or(false);
        }

        for (index, page) in self.pages.iter_mut().enumerate() {
            if !page.meta.render {
                continue;
            }
            let filename = self.config.index_filename(&page.lang);
            let mut section_file = page.parent.join(&filename);
            page.hidden = page.meta.hidden.unwrap_or_else(|| {
                by_file
                    .get(&section_file)
                    .is_some_and(|&i| self.sections[i].hidden)
            });
            while let Some(&i) = by_file.get(&section_file) {
                let section = &mut self.sections[i];
                if page.hidden {
                    section.hidden_pages.push(index);
                } else {
                    section.pages.push(index);
                }
                page.in_a_section = true;
                if !section.meta.transparent {
                    break;
                }
                match section_file.parent().and_then(Path::parent) {
                    Some(up) => section_file = up.join(&filename),
                    None => break,
                }
            }
        }

        for section in &mut self.sections {
            let sort_by = section.meta.sort_by.as_str();
            if sort_by == "none" {
                continue;
            }
            let pages = &self.pages;
            section.pages.retain(|&i| sortable(&pages[i], sort_by));
        }
    }

    /// Every file `zola build` writes, relative to the output directory,
    /// with its sources.
    fn plan(&self) -> Result<BTreeMap<String, Vec<PathBuf>>> {
        self.refuse_image_processing()?;
        let rendered_sources: Vec<PathBuf> = self.rendered_sources()?.into_iter().collect();
        let mut plan: PlanMap = BTreeMap::new();
        let rendered = |plan: &mut PlanMap, path: String| {
            plan.entry(path).or_default().rendered = true;
        };

        rendered(&mut plan, "404.html".to_string());
        if self.config.generate_sitemap {
            rendered(&mut plan, "sitemap.xml".to_string());
        }
        if self.config.generate_robots_txt {
            rendered(&mut plan, "robots.txt".to_string());
        }

        // Aliases of every page and section, rendered or not.
        let aliases = self
            .pages
            .iter()
            .flat_map(|p| &p.meta.aliases)
            .chain(self.sections.iter().flat_map(|s| &s.meta.aliases));
        for alias in aliases {
            let mut path = alias.trim_start_matches('/').to_string();
            if !path.ends_with(".html") {
                if !path.is_empty() && !path.ends_with('/') {
                    path.push('/');
                }
                path.push_str("index.html");
            }
            rendered(&mut plan, path);
        }

        let languages = self.config.all_languages();
        let mut rendered_pages: BTreeSet<usize> = BTreeSet::new();
        for section in &self.sections {
            rendered_pages.extend(section.pages.iter().copied());
            rendered_pages.extend(section.hidden_pages.iter().copied());
            if section.meta.generate_feeds {
                for feed in &languages[&section.lang].feed_filenames {
                    rendered(
                        &mut plan,
                        format!("{}{feed}", section.path_without_slash(&self.config)),
                    );
                }
            }
            let base = section.base(&self.config);
            if let Some(dir) = section.file.as_ref().and_then(|f| f.parent()) {
                for asset in &section.assets {
                    plan.entry(join_path(&base, &relative_slash_path(dir, asset)))
                        .or_default()
                        .files
                        .insert(asset.clone());
                }
            }
            if !section.meta.render {
                continue;
            }
            match section.meta.paginate_by {
                Some(per_page) if per_page > 0 => {
                    for path in pager_paths(
                        &base,
                        &section.meta.paginate_path,
                        section.pages.len(),
                        per_page,
                    ) {
                        rendered(&mut plan, path);
                    }
                }
                _ => rendered(&mut plan, join_path(&base, "index.html")),
            }
        }
        for (index, page) in self.pages.iter().enumerate() {
            if page.meta.render && !page.in_a_section {
                rendered_pages.insert(index);
            }
        }
        for &index in &rendered_pages {
            let page = &self.pages[index];
            rendered(&mut plan, format!("{}index.html", page.out_dir));
            for asset in &page.assets {
                plan.entry(format!(
                    "{}{}",
                    page.out_dir,
                    relative_slash_path(&page.asset_root, asset)
                ))
                .or_default()
                .files
                .insert(asset.clone());
            }
        }

        self.plan_taxonomies(&languages, &mut plan)?;

        for (lang, options) in &languages {
            if options.generate_feeds {
                for feed in &options.feed_filenames {
                    let path = match self.config.lang_prefix(lang) {
                        Some(prefix) => format!("{prefix}/{feed}"),
                        None => feed.clone(),
                    };
                    rendered(&mut plan, path);
                }
            }
        }

        self.plan_search_index(&languages, &mut plan)?;
        self.plan_highlight_css(&mut plan);
        self.plan_sass(&mut plan)?;
        self.plan_static(&mut plan)?;

        let html = plan.keys().filter(|p| p.ends_with(".html")).count();
        if html >= SITEMAP_SPLIT {
            anyhow::bail!(
                "the site renders {html} HTML files; at {SITEMAP_SPLIT} sitemap entries zola \
                 splits the sitemap, which the zola processor does not model"
            );
        }
        Ok(plan
            .into_iter()
            .map(|(path, sources)| {
                let mut files: Vec<PathBuf> = if sources.rendered {
                    rendered_sources.clone()
                } else {
                    Vec::new()
                };
                files.extend(sources.files);
                (path, files)
            })
            .collect())
    }

    fn plan_taxonomies(
        &self,
        languages: &BTreeMap<String, LanguageOptions>,
        plan: &mut PlanMap,
    ) -> Result<()> {
        for (lang, options) in languages {
            for taxonomy in &options.taxonomies {
                let taxonomy_slug = slugify_paths(&taxonomy.name, self.config.slugify.taxonomies);
                // Terms grouped by slug: zola merges terms whose slugs (and so
                // permalinks) collide.
                let mut terms: BTreeMap<String, usize> = BTreeMap::new();
                for page in &self.pages {
                    if page.lang != *lang || !page.meta.render || page.hidden {
                        continue;
                    }
                    for term in page
                        .meta
                        .taxonomies
                        .get(&taxonomy.name)
                        .into_iter()
                        .flatten()
                    {
                        let term_slug = slugify_paths(term, self.config.slugify.taxonomies);
                        if term_slug.is_empty() {
                            anyhow::bail!(
                                "{}: the term `{term}` in `{}` slugifies to an empty string",
                                page.file.display(),
                                taxonomy.name
                            );
                        }
                        *terms.entry(term_slug).or_default() += 1;
                    }
                }
                if !taxonomy.render || terms.is_empty() {
                    continue;
                }
                let mut base_parts: Vec<String> =
                    self.config.lang_prefix(lang).into_iter().collect();
                base_parts.extend(self.config.taxonomy_root.clone());
                base_parts.push(taxonomy_slug);
                let base = base_parts.join("/");
                let mut add = |path: String| {
                    plan.entry(path).or_default().rendered = true;
                };
                add(join_path(&base, "index.html"));
                for (term_slug, count) in &terms {
                    let term_base = join_path(&base, term_slug);
                    match taxonomy.paginate_by {
                        Some(per_page) if per_page > 0 => {
                            let paginate_path = taxonomy.paginate_path.as_deref().unwrap_or("page");
                            for path in pager_paths(&term_base, paginate_path, *count, per_page) {
                                add(path);
                            }
                        }
                        _ => add(join_path(&term_base, "index.html")),
                    }
                    if taxonomy.feed {
                        for feed in &options.feed_filenames {
                            add(join_path(&term_base, feed));
                        }
                    }
                }
            }
        }
        // Every page's taxonomies must exist for its language, as zola checks.
        for page in &self.pages {
            let configured = &languages[&page.lang].taxonomies;
            for name in page.meta.taxonomies.keys() {
                if !configured.iter().any(|t| t.name == *name) {
                    anyhow::bail!(
                        "{} uses the taxonomy `{name}`, which is not configured for language `{}`",
                        page.file.display(),
                        page.lang
                    );
                }
            }
        }
        Ok(())
    }

    fn plan_search_index(
        &self,
        languages: &BTreeMap<String, LanguageOptions>,
        plan: &mut PlanMap,
    ) -> Result<()> {
        let indexed: Vec<&String> = languages
            .iter()
            .filter(|(_, options)| options.build_search_index)
            .map(|(lang, _)| lang)
            .collect();
        if indexed.is_empty() {
            return Ok(());
        }
        let (extension, elasticlunr) = match self.config.search.index_format.as_str() {
            "elasticlunr_javascript" => ("js", true),
            "elasticlunr_json" => ("json", true),
            "fuse_javascript" => ("js", false),
            "fuse_json" => ("json", false),
            other => anyhow::bail!("unknown search.index_format `{other}`"),
        };
        for lang in indexed {
            plan.entry(format!("search_index.{lang}.{extension}"))
                .or_default()
                .rendered = true;
        }
        if elasticlunr {
            plan.entry("elasticlunr.min.js".to_string())
                .or_default()
                .files
                .insert(self.config_file.clone());
        }
        Ok(())
    }

    /// Syntax-highlighting stylesheets zola writes for the `class` style.
    fn plan_highlight_css(&self, plan: &mut PlanMap) {
        let Some(highlighting) = &self.config.markdown.highlighting else {
            return;
        };
        if highlighting.style != "class" {
            return;
        }
        let files: &[&str] = if highlighting.theme.is_some() {
            &["giallo.css"]
        } else if highlighting.light_theme.is_some() && highlighting.dark_theme.is_some() {
            &["giallo-light.css", "giallo-dark.css"]
        } else {
            &[]
        };
        for file in files {
            plan.entry((*file).to_string())
                .or_default()
                .files
                .insert(self.config_file.clone());
        }
    }

    /// Every `.sass`/`.scss` under `sass/` that is not a partial (a file or
    /// directory starting with `_` is skipped) compiles to a `.css` at the
    /// same relative path. Any file of the tree can be imported by any other.
    fn plan_sass(&self, plan: &mut PlanMap) -> Result<()> {
        if !self.config.compile_sass {
            return Ok(());
        }
        let sass_dir = self.root.join("sass");
        let all = files_under(&sass_dir)?;
        for file in &all {
            let rel = relative_slash_path(&sass_dir, file);
            let partial = rel.split('/').any(|part| part.starts_with('_'));
            let is_sass = rel.ends_with(".sass") || rel.ends_with(".scss");
            if partial || !is_sass {
                continue;
            }
            let css = Path::new(&rel).with_extension("css");
            plan.entry(css.to_string_lossy().into_owned())
                .or_default()
                .files
                .extend(all.iter().cloned());
        }
        Ok(())
    }

    /// The static tree is copied verbatim. `processed_images/` is zola's own
    /// resize cache, which it prunes and which this planner does not model.
    fn plan_static(&self, plan: &mut PlanMap) -> Result<()> {
        let static_dir = self.root.join("static");
        let processed = static_dir.join("processed_images");
        for file in files_under(&static_dir)? {
            if file.starts_with(&processed) {
                continue;
            }
            plan.entry(relative_slash_path(&static_dir, &file))
                .or_default()
                .files
                .insert(file);
        }
        Ok(())
    }

    /// The inputs of every rendered file. A zola template can read any page
    /// or section (`get_page`, `get_section`, sibling links, taxonomy lists),
    /// so a rendered page depends on the whole content tree, not only on its
    /// own markdown; plus the config, every template, and the data files the
    /// templates `load_data` by a literal path.
    fn rendered_sources(&self) -> Result<BTreeSet<PathBuf>> {
        let mut sources: BTreeSet<PathBuf> = BTreeSet::new();
        sources.insert(self.config_file.clone());
        let templates = files_under(&self.root.join("templates"))?;
        let content = files_under(&self.content_dir())?;
        for file in templates.iter().chain(content.iter()) {
            if file
                .extension()
                .is_some_and(|e| e == "md" || e == "html" || e == "xml" || e == "txt")
            {
                let text = std::fs::read_to_string(file).unwrap_or_default();
                for caps in LOAD_DATA.captures_iter(&text) {
                    sources.insert(self.resolve_load_data(&caps[1]));
                }
            }
        }
        sources.extend(templates);
        sources.extend(content);
        Ok(sources)
    }

    /// zola's `search_for_file`: the site root, then `static/`, then
    /// `content/`. A file found in none of them is taken at the site root,
    /// zola's first choice: it may be the output of another processor that
    /// has not run yet, and listing it is what orders that processor before
    /// zola (a missing input that nothing produces is simply missing).
    fn resolve_load_data(&self, path: &str) -> PathBuf {
        let actual = path.strip_prefix("@/").map_or_else(
            || path.trim_start_matches('/').to_string(),
            |rest| format!("content/{rest}"),
        );
        let candidates = [
            self.root.join(&actual),
            self.root.join("static").join(&actual),
            self.root.join("content").join(&actual),
        ];
        let found = candidates.iter().find(|p| p.is_file()).cloned();
        found.unwrap_or_else(|| candidates[0].clone())
    }

    /// Resized images are only known by rendering: refuse sites that resize.
    fn refuse_image_processing(&self) -> Result<()> {
        let files = files_under(&self.root.join("templates"))?
            .into_iter()
            .chain(files_under(&self.content_dir())?);
        for file in files {
            if file.extension().is_some_and(|e| e == "md" || e == "html") {
                let text = std::fs::read_to_string(&file).unwrap_or_default();
                if text.contains("resize_image(") {
                    anyhow::bail!(
                        "{} calls resize_image; the zola processor cannot predict processed \
                         images (it would have to render the site)",
                        file.display()
                    );
                }
            }
        }
        Ok(())
    }
}

/// Whether `page` has the key `sort_by` orders by (zola's `sort_pages`).
fn sortable(page: &Page, sort_by: &str) -> bool {
    match sort_by {
        "date" => page.dated,
        "update_date" => page.dated || page.meta.updated.as_ref().is_some_and(is_parseable_date),
        "title" | "title_bytes" => page.meta.title.is_some(),
        "weight" => page.meta.weight.is_some(),
        _ => true,
    }
}

/// The directories below `content/` holding `dir`.
fn content_components(content: &Path, dir: &Path) -> Vec<String> {
    dir.strip_prefix(content)
        .map(|rel| {
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn join_relative(components: &[String], filename: &str) -> String {
    if components.is_empty() {
        filename.to_string()
    } else {
        format!("{}/{filename}", components.join("/"))
    }
}

fn join_path(base: &str, rest: &str) -> String {
    if base.is_empty() {
        rest.to_string()
    } else {
        format!("{base}/{rest}")
    }
}

/// The files a paginated listing writes: the first pager at `base`, a
/// redirect from `base/<paginate_path>/1/` to it, and `base/<paginate_path>/N/`
/// for every further pager. There is always at least one pager.
fn pager_paths(base: &str, paginate_path: &str, items: usize, per_page: usize) -> Vec<String> {
    let pagers = items.div_ceil(per_page).max(1);
    let pager_dir = |n: usize| {
        let dir = if paginate_path.is_empty() {
            join_path(base, &n.to_string())
        } else {
            join_path(&join_path(base, paginate_path), &n.to_string())
        };
        join_path(&dir, "index.html")
    };
    let mut paths = vec![join_path(base, "index.html"), pager_dir(1)];
    paths.extend((2..=pagers).map(pager_dir));
    paths
}

impl Processor for ZolaProcessor {
    fn scan_config(&self) -> &StandardConfig {
        &self.config.standard
    }

    fn auto_detect(&self, _file_index: &FileIndex) -> bool {
        let root = self.site_root();
        (root.join("zola.toml").is_file() || root.join("config.toml").is_file())
            && root.join("content").is_dir()
            && root.join("templates").is_dir()
    }

    fn required_tools(&self) -> Vec<String> {
        let mut tools = vec![self.config.standard.command.clone()];
        tools.extend(self.config.standard.required_tools.iter().cloned());
        tools
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        _file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let plan = self.plan(instance_name)?;
        let config_hash = output_config_hash(&self.config, &checksum_fields_of(instance_name));
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;
        add_planned_products(graph, &plan.outputs, instance_name, &config_hash, &extra)
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let instance_name = &product.processor;
        self.tool_run.run(&self.config.standard.command, || {
            self.run_tool(ctx, instance_name)
        })?;
        require_planned_output(product, &self.config.standard.command)
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn config_json(&self) -> Option<String> {
        ProcessorBase::config_json(&self.config)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(ZolaProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "zola",
        processor_type: crate::processor::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "root", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "The zola site directory, relative to the project root (empty = project root)" },
            crate::config::FieldSpec { name: "output_dir", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Where the site is written; empty means the site config's output_dir" },
            crate::config::FieldSpec { name: "loose_manifest", ty: crate::config::FieldType::Bool,
                affects_output: false, required: false,
                doc: "Report plan-vs-build mismatches as warnings instead of failing the build" },
        ],
        omit_standard_fields: &["formats", "dep_auto", "batch"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "zola", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<ZolaConfig>,
        keywords: &["zola", "static-site", "site", "html", "blog", "mass", "rust"],
        description: "Build a zola site; one cached product per file zola writes, planned natively",
        is_native: false,
        is_rust: true,
        can_fix: false,
        supports_batch: false,
        max_jobs_cap: Some(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pager_paths_follow_zola() {
        assert_eq!(
            pager_paths("blog", "page", 5, 2),
            vec![
                "blog/index.html",
                "blog/page/1/index.html",
                "blog/page/2/index.html",
                "blog/page/3/index.html",
            ]
        );
        // Empty listings still get the first pager and its redirect.
        assert_eq!(
            pager_paths("", "page", 0, 10),
            vec!["index.html", "page/1/index.html"]
        );
        // An empty paginate_path puts pagers straight below the base.
        assert_eq!(
            pager_paths("tags/rust", "", 3, 2),
            vec![
                "tags/rust/index.html",
                "tags/rust/1/index.html",
                "tags/rust/2/index.html",
            ]
        );
    }

    #[test]
    fn slugify_strategies_match_zola() {
        assert_eq!(
            slugify_paths("Hello World", SlugifyStrategy::On),
            "hello-world"
        );
        assert_eq!(slugify_paths("日本", SlugifyStrategy::On), "ri-ben");
        assert_eq!(
            slugify_paths("C++ / Rust?.", SlugifyStrategy::Safe),
            "C++  Rust"
        );
        assert_eq!(slugify_paths("dot. ", SlugifyStrategy::Safe), "dot");
        assert_eq!(slugify_paths("as is ", SlugifyStrategy::Off), "as is ");
    }

    #[test]
    fn language_suffixes_follow_zola() {
        let mut config = SiteConfig::default();
        let path = Path::new("content/x.md");
        assert_eq!(
            split_language("post.fr", &config, path).unwrap(),
            ("post.fr".to_string(), "en".to_string()),
            "without other languages a dot is part of the name"
        );
        config
            .languages
            .insert("fr".to_string(), LanguageOptions::default());
        assert_eq!(
            split_language("post.fr", &config, path).unwrap(),
            ("post".to_string(), "fr".to_string())
        );
        assert_eq!(
            split_language("post.en", &config, path).unwrap(),
            ("post.en".to_string(), "en".to_string()),
            "the default language's own code is not stripped"
        );
        assert!(split_language("post.de", &config, path).is_err());
    }

    #[test]
    fn dates_parse_like_zola() {
        assert!(is_parseable_date(&toml::Value::String("2024-01-02".into())));
        assert!(is_parseable_date(&toml::Value::String(
            "2024-01-02T03:04:05".into()
        )));
        assert!(is_parseable_date(&toml::Value::String(
            "2024-01-02T03:04:05+02:00".into()
        )));
        assert!(!is_parseable_date(&toml::Value::String("yesterday".into())));
        assert!(DATED_NAME.is_match("2024-01-02-post"));
        assert_eq!(
            DATED_NAME
                .captures("2024-01-02_post")
                .unwrap()
                .name("slug")
                .unwrap()
                .as_str(),
            "post"
        );
    }
}

mod analyzer_configs;
mod processor_configs;
mod variables;
#[cfg(test)]
mod tests;

pub(crate) use analyzer_configs::*;
pub(crate) use processor_configs::*;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::errors;
use crate::processors::names as processor_names;

use variables::substitute_variables;

const CONFIG_FILE: &str = "rsb.toml";

pub(crate) trait KnownFields {
    fn known_fields() -> &'static [&'static str];
}


/// Validate extra_inputs paths exist and return them as PathBufs.
/// Paths are relative to project root (which is cwd).
pub(crate) fn resolve_extra_inputs(extra_inputs: &[String]) -> Result<Vec<PathBuf>> {
    let mut resolved = Vec::new();
    for p in extra_inputs {
        let path = PathBuf::from(p);
        if !path.exists() {
            anyhow::bail!("extra_inputs file not found: {}", p);
        }
        resolved.push(path);
    }
    Ok(resolved)
}

/// Compute a SHA-256 hash of any serializable config value.
/// Uses JSON serialization (deterministic for structs) to produce the hash input.
pub(crate) fn config_hash(value: &impl Serialize) -> String {
    let json = serde_json::to_string(value).expect(errors::CONFIG_SERIALIZE);
    let hash = Sha256::digest(json.as_bytes());
    hex::encode(hash)
}

/// Common scan configuration shared by all processors.
/// Each processor embeds this via `#[serde(flatten)]` and provides its own defaults.
///
/// Fields use `Option` so that serde can distinguish "not specified" (None) from
/// "explicitly set" (Some). `resolve_scan_defaults()` fills in None values after
/// loading, so processors can always unwrap safely.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub(crate) struct ScanConfig {
    /// Directory to scan for source files ("" means project root)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_dir: Option<String>,

    /// File extensions to match
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,

    /// Directory path segments to exclude from scanning
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_dirs: Option<Vec<String>>,

    /// File names to exclude from scanning
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_files: Option<Vec<String>>,

    /// Paths (relative to project root) to exclude from scanning
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_paths: Option<Vec<String>>,
}

impl ScanConfig {
    /// Fill in None fields with the given defaults (mutates in place).
    pub(crate) fn resolve(&mut self, scan_dir: &str, extensions: &[&str], exclude_dirs: &[&str]) {
        if self.scan_dir.is_none() {
            self.scan_dir = Some(scan_dir.to_string());
        }
        if self.extensions.is_none() {
            self.extensions = Some(extensions.iter().map(|s| s.to_string()).collect());
        }
        if self.exclude_dirs.is_none() {
            self.exclude_dirs = Some(exclude_dirs.iter().map(|s| s.to_string()).collect());
        }
        if self.exclude_files.is_none() {
            self.exclude_files = Some(Vec::new());
        }
        if self.exclude_paths.is_none() {
            self.exclude_paths = Some(Vec::new());
        }
    }

    /// Get the resolved scan directory. Panics if called before resolve().
    pub(crate) fn scan_dir(&self) -> &str {
        self.scan_dir.as_deref().expect(errors::SCAN_CONFIG_NOT_RESOLVED)
    }

    /// Get the resolved extensions. Panics if called before resolve().
    pub(crate) fn extensions(&self) -> &[String] {
        self.extensions.as_deref().expect(errors::SCAN_CONFIG_NOT_RESOLVED)
    }

    /// Get the resolved exclude dirs. Panics if called before resolve().
    pub(crate) fn exclude_dirs(&self) -> &[String] {
        self.exclude_dirs.as_deref().expect(errors::SCAN_CONFIG_NOT_RESOLVED)
    }

    /// Get the resolved exclude files. Panics if called before resolve().
    pub(crate) fn exclude_files(&self) -> &[String] {
        self.exclude_files.as_deref().expect(errors::SCAN_CONFIG_NOT_RESOLVED)
    }

    /// Get the resolved exclude paths. Panics if called before resolve().
    pub(crate) fn exclude_paths(&self) -> &[String] {
        self.exclude_paths.as_deref().expect(errors::SCAN_CONFIG_NOT_RESOLVED)
    }
}

/// Base exclude dirs shared by all processors.
const COMMON_EXCLUDE_DIRS: &[&str] = &["/.git/", "/out/", "/build/", "/dist/"];

/// Common + build tool dirs (/.rsb/, /node_modules/, /target/).
/// Used by processors that scan broadly and need to skip build artifacts.
const BUILD_TOOL_EXCLUDES: &[&str] = &[
    "/.git/", "/out/", "/build/", "/dist/",
    "/.rsb/", "/node_modules/", "/target/",
];

const PYTHON_EXCLUDE_DIRS: &[&str] = &[
    "/.git/", "/out/", "/build/", "/dist/",
    "/.venv/", "/__pycache__/", "/node_modules/", "/.tox/", "/.eggs/",
];

const CC_EXCLUDE_DIRS: &[&str] = COMMON_EXCLUDE_DIRS;
const SPELLCHECK_EXCLUDE_DIRS: &[&str] = BUILD_TOOL_EXCLUDES;
const SHELL_EXCLUDE_DIRS: &[&str] = BUILD_TOOL_EXCLUDES;
const MARKDOWN_EXCLUDE_DIRS: &[&str] = BUILD_TOOL_EXCLUDES;

/// MAKE and Cargo exclude node_modules-free build tool dirs.
const MAKE_CARGO_EXCLUDES: &[&str] = &[
    "/.git/", "/out/", "/build/", "/dist/",
    "/.rsb/", "/target/",
];

const DEFAULT_PLUGINS_DIR: &str = "plugins";

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginsConfig {
    #[serde(default = "default_plugins_dir")]
    pub dir: String,
}

fn default_plugins_dir() -> String {
    DEFAULT_PLUGINS_DIR.into()
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self { dir: DEFAULT_PLUGINS_DIR.into() }
    }
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    #[serde(default)]
    pub build: BuildConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub processor: ProcessorConfig,
    #[serde(default)]
    pub analyzer: AnalyzerConfig,
    #[serde(default)]
    pub completions: CompletionsConfig,
    #[serde(default)]
    pub graph: GraphConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct BuildConfig {
    /// Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)
    #[serde(default = "default_parallel")]
    pub parallel: usize,
    /// Maximum files per batch for batch-capable processors.
    /// 0 = no limit (all files in one batch), None = disable batching entirely.
    #[serde(default)]
    pub batch_size: Option<usize>,
}

fn default_parallel() -> usize {
    1
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            parallel: 1,
            batch_size: Some(0), // Default: batching enabled, no size limit
        }
    }
}

/// Method used to restore files from cache
#[derive(Debug, Deserialize, Serialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RestoreMethod {
    #[default]
    Hardlink,
    Copy,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct CacheConfig {
    #[serde(default)]
    pub restore_method: RestoreMethod,
    /// Remote cache URL (e.g., "s3://bucket/prefix", "http://host:port/path", or local "file:///path")
    #[serde(default)]
    pub remote: Option<String>,
    /// Whether to push local builds to remote cache (default: true)
    #[serde(default = "default_true")]
    pub remote_push: bool,
    /// Whether to pull from remote cache on miss (default: true)
    #[serde(default = "default_true")]
    pub remote_pull: bool,
    /// Whether to use mtime pre-check to skip unchanged file checksums (default: true)
    #[serde(default = "default_true")]
    pub mtime_check: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            restore_method: RestoreMethod::default(),
            remote: None,
            remote_push: true,
            remote_pull: true,
            mtime_check: true,
        }
    }
}

pub(crate) fn default_true() -> bool {
    true
}

fn default_processors() -> Vec<String> {
    use crate::processors::names;
    vec![
        names::TERA.into(), names::RUFF.into(), names::PYLINT.into(),
        names::MYPY.into(), names::PYREFLY.into(), names::CC_SINGLE_FILE.into(),
        names::CPPCHECK.into(), names::CLANG_TIDY.into(),
        names::SHELLCHECK.into(), names::SPELLCHECK.into(), names::MAKE.into(),
        names::CARGO.into(), names::RUMDL.into(),
        names::YAMLLINT.into(), names::JQ.into(), names::JSONLINT.into(), names::TAPLO.into(),
        names::JSON_SCHEMA.into(),
        names::TAGS.into(),
        names::PIP.into(), names::SPHINX.into(), names::NPM.into(), names::GEM.into(),
        names::MDL.into(), names::MARKDOWNLINT.into(),
        names::ASPELL.into(), names::PANDOC.into(), names::MARKDOWN.into(),
        names::PDFLATEX.into(), names::A2X.into(), names::ASCII_CHECK.into(),
    ]
}

pub(crate) fn default_cc_compiler() -> String {
    "gcc".into()
}

pub(crate) fn default_cxx_compiler() -> String {
    "g++".into()
}

pub(crate) fn default_output_suffix() -> String {
    ".elf".into()
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ProcessorConfig {
    #[serde(default = "default_true")]
    pub auto_detect: bool,
    #[serde(default = "default_processors")]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub tera: TeraConfig,
    #[serde(default)]
    pub ruff: RuffConfig,
    #[serde(default)]
    pub pylint: PylintConfig,
    #[serde(default)]
    pub cc_single_file: CcConfig,
    #[serde(default)]
    pub cppcheck: CppcheckConfig,
    #[serde(default)]
    pub clang_tidy: ClangTidyConfig,
    #[serde(default)]
    pub spellcheck: SpellcheckConfig,
    #[serde(default)]
    pub shellcheck: ShellcheckConfig,
    #[serde(default)]
    pub sleep: SleepConfig,
    #[serde(default)]
    pub make: MakeConfig,
    #[serde(default)]
    pub cargo: CargoConfig,
    #[serde(default)]
    pub rumdl: RumdlConfig,
    #[serde(default)]
    pub mypy: MypyConfig,
    #[serde(default)]
    pub pyrefly: PyreflyConfig,
    #[serde(default)]
    pub yamllint: YamllintConfig,
    #[serde(default)]
    pub jq: JqConfig,
    #[serde(default)]
    pub jsonlint: JsonlintConfig,
    #[serde(default)]
    pub taplo: TaploConfig,
    #[serde(default)]
    pub json_schema: JsonSchemaConfig,
    #[serde(default)]
    pub tags: TagsConfig,
    #[serde(default)]
    pub pip: PipConfig,
    #[serde(default)]
    pub sphinx: SphinxConfig,
    #[serde(default)]
    pub npm: NpmConfig,
    #[serde(default)]
    pub gem: GemConfig,
    #[serde(default)]
    pub mdl: MdlConfig,
    #[serde(default)]
    pub markdownlint: MarkdownlintConfig,
    #[serde(default)]
    pub aspell: AspellConfig,
    #[serde(default)]
    pub pandoc: PandocConfig,
    #[serde(default)]
    pub markdown: MarkdownGenConfig,
    #[serde(default)]
    pub pdflatex: PdflatexConfig,
    #[serde(default)]
    pub a2x: A2xConfig,
    #[serde(default)]
    pub ascii_check: AsciiCheckConfig,
    /// Captures unknown [processor.PLUGIN_NAME] sections for Lua plugins
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            auto_detect: true,
            enabled: default_processors(),
            tera: TeraConfig::default(),
            ruff: RuffConfig::default(),
            pylint: PylintConfig::default(),
            cc_single_file: CcConfig::default(),
            cppcheck: CppcheckConfig::default(),
            clang_tidy: ClangTidyConfig::default(),
            shellcheck: ShellcheckConfig::default(),
            spellcheck: SpellcheckConfig::default(),
            sleep: SleepConfig::default(),
            make: MakeConfig::default(),
            cargo: CargoConfig::default(),
            rumdl: RumdlConfig::default(),
            mypy: MypyConfig::default(),
            pyrefly: PyreflyConfig::default(),
            yamllint: YamllintConfig::default(),
            jq: JqConfig::default(),
            jsonlint: JsonlintConfig::default(),
            taplo: TaploConfig::default(),
            json_schema: JsonSchemaConfig::default(),
            tags: TagsConfig::default(),
            pip: PipConfig::default(),
            sphinx: SphinxConfig::default(),
            npm: NpmConfig::default(),
            gem: GemConfig::default(),
            mdl: MdlConfig::default(),
            markdownlint: MarkdownlintConfig::default(),
            aspell: AspellConfig::default(),
            pandoc: PandocConfig::default(),
            markdown: MarkdownGenConfig::default(),
            pdflatex: PdflatexConfig::default(),
            a2x: A2xConfig::default(),
            ascii_check: AsciiCheckConfig::default(),
            extra: HashMap::new(),
        }
    }
}

impl ProcessorConfig {
    fn processor_enabled_field(&self, name: &str) -> bool {
        match name {
            "tera" => self.tera.enabled,
            "ruff" => self.ruff.enabled,
            "pylint" => self.pylint.enabled,
            "cc_single_file" => self.cc_single_file.enabled,
            "cppcheck" => self.cppcheck.enabled,
            "clang_tidy" => self.clang_tidy.enabled,
            "shellcheck" => self.shellcheck.enabled,
            "spellcheck" => self.spellcheck.enabled,
            "sleep" => self.sleep.enabled,
            "make" => self.make.enabled,
            "cargo" => self.cargo.enabled,
            "rumdl" => self.rumdl.enabled,
            "mypy" => self.mypy.enabled,
            "pyrefly" => self.pyrefly.enabled,
            "yamllint" => self.yamllint.enabled,
            "jq" => self.jq.enabled,
            "jsonlint" => self.jsonlint.enabled,
            "taplo" => self.taplo.enabled,
            "json_schema" => self.json_schema.enabled,
            "tags" => self.tags.enabled,
            "pip" => self.pip.enabled,
            "sphinx" => self.sphinx.enabled,
            "npm" => self.npm.enabled,
            "gem" => self.gem.enabled,
            "mdl" => self.mdl.enabled,
            "markdownlint" => self.markdownlint.enabled,
            "aspell" => self.aspell.enabled,
            "pandoc" => self.pandoc.enabled,
            "markdown" => self.markdown.enabled,
            "pdflatex" => self.pdflatex.enabled,
            "a2x" => self.a2x.enabled,
            "ascii_check" => self.ascii_check.enabled,
            _ => true, // unknown processors (plugins) default to enabled
        }
    }

    pub(crate) fn is_enabled(&self, name: &str) -> bool {
        self.enabled.iter().any(|p| p == name) && self.processor_enabled_field(name)
    }

    /// Collect unique scan directories from all processor configs.
    /// Returns non-empty directory names (empty means project root, handled separately).
    pub(crate) fn scan_dirs(&self) -> Vec<String> {
        let scans = [
            &self.tera.scan, &self.ruff.scan, &self.pylint.scan,
            &self.cc_single_file.scan, &self.cppcheck.scan, &self.clang_tidy.scan,
            &self.shellcheck.scan, &self.spellcheck.scan, &self.sleep.scan,
            &self.make.scan, &self.cargo.scan, &self.rumdl.scan, &self.mypy.scan,
            &self.pyrefly.scan, &self.yamllint.scan, &self.jq.scan, &self.jsonlint.scan,
            &self.taplo.scan, &self.json_schema.scan, &self.tags.scan,
            &self.pip.scan, &self.sphinx.scan, &self.npm.scan, &self.gem.scan,
            &self.mdl.scan, &self.markdownlint.scan,
            &self.aspell.scan, &self.pandoc.scan, &self.markdown.scan,
            &self.pdflatex.scan, &self.a2x.scan, &self.ascii_check.scan,
        ];
        let mut dirs: Vec<String> = scans.iter()
            .filter_map(|s| s.scan_dir.as_deref())
            .filter(|d| !d.is_empty())
            .map(|d| d.to_string())
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    /// Fill in None scan fields with per-processor defaults.
    /// Called after loading from TOML so that `config show` displays resolved values
    /// and processors can access fields without fallbacks.
    pub(crate) fn resolve_scan_defaults(&mut self) {
        self.tera.scan.resolve("templates", &[".tera"], &[]);
        self.ruff.scan.resolve("", &[".py"], PYTHON_EXCLUDE_DIRS);
        self.pylint.scan.resolve("", &[".py"], PYTHON_EXCLUDE_DIRS);
        self.cc_single_file.scan.resolve("src", &[".c", ".cc"], &[]);
        self.cppcheck.scan.resolve("src", &[".c", ".cc"], CC_EXCLUDE_DIRS);
        self.clang_tidy.scan.resolve("src", &[".c", ".cc"], CC_EXCLUDE_DIRS);
        self.shellcheck.scan.resolve("", &[".sh", ".bash"], SHELL_EXCLUDE_DIRS);
        self.spellcheck.scan.resolve("", &[".md"], SPELLCHECK_EXCLUDE_DIRS);
        self.sleep.scan.resolve("sleep", &[".sleep"], &[]);
        self.make.scan.resolve("", &["Makefile"], MAKE_CARGO_EXCLUDES);
        self.cargo.scan.resolve("", &["Cargo.toml"], MAKE_CARGO_EXCLUDES);
        self.rumdl.scan.resolve("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
        self.mypy.scan.resolve("", &[".py"], PYTHON_EXCLUDE_DIRS);
        self.pyrefly.scan.resolve("", &[".py"], PYTHON_EXCLUDE_DIRS);
        self.yamllint.scan.resolve("", &[".yml", ".yaml"], BUILD_TOOL_EXCLUDES);
        self.jq.scan.resolve("", &[".json"], BUILD_TOOL_EXCLUDES);
        self.jsonlint.scan.resolve("", &[".json"], BUILD_TOOL_EXCLUDES);
        self.taplo.scan.resolve("", &[".toml"], BUILD_TOOL_EXCLUDES);
        self.json_schema.scan.resolve("", &[".json"], BUILD_TOOL_EXCLUDES);
        self.tags.scan.resolve("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
        self.pip.scan.resolve("", &["requirements.txt"], MAKE_CARGO_EXCLUDES);
        self.sphinx.scan.resolve("", &["conf.py"], BUILD_TOOL_EXCLUDES);
        self.npm.scan.resolve("", &["package.json"], MAKE_CARGO_EXCLUDES);
        self.gem.scan.resolve("", &["Gemfile"], MAKE_CARGO_EXCLUDES);
        self.mdl.scan.resolve("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
        self.markdownlint.scan.resolve("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
        self.aspell.scan.resolve("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
        self.pandoc.scan.resolve("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
        self.markdown.scan.resolve("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
        self.pdflatex.scan.resolve("", &[".tex"], BUILD_TOOL_EXCLUDES);
        self.a2x.scan.resolve("", &[".txt"], BUILD_TOOL_EXCLUDES);
        self.ascii_check.scan.resolve("", &[".md"], MARKDOWN_EXCLUDE_DIRS);
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompletionsConfig {
    #[serde(default = "default_shells")]
    pub shells: Vec<String>,
}

fn default_shells() -> Vec<String> {
    vec!["bash".into()]
}

impl Default for CompletionsConfig {
    fn default() -> Self {
        Self { shells: vec!["bash".into()] }
    }
}

fn default_analyzers() -> Vec<String> {
    vec!["cpp".into(), "python".into()]
}

/// Configuration for dependency analyzers
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalyzerConfig {
    /// Whether to auto-detect which analyzers are relevant
    #[serde(default = "default_true")]
    pub auto_detect: bool,
    /// List of enabled analyzer names
    #[serde(default = "default_analyzers")]
    pub enabled: Vec<String>,
    /// C/C++ analyzer configuration
    #[serde(default)]
    pub cpp: CppAnalyzerConfig,
    /// Python analyzer configuration
    #[serde(default)]
    pub python: PythonAnalyzerConfig,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            auto_detect: true,
            enabled: default_analyzers(),
            cpp: CppAnalyzerConfig::default(),
            python: PythonAnalyzerConfig::default(),
        }
    }
}

impl AnalyzerConfig {
    pub(crate) fn is_enabled(&self, name: &str) -> bool {
        self.enabled.iter().any(|a| a == name)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct GraphConfig {
    #[serde(default)]
    pub viewer: Option<String>,
}

/// Validate that all fields in `[processor.X]` sections are known fields for that processor.
/// Returns an error listing unknown fields. Skips non-table entries (like `auto_detect`)
/// and unknown processor names (those are Lua plugin sections).
fn validate_processor_fields(raw: &toml::Value) -> Result<()> {
    let processor_table = match raw.get("processor").and_then(|v| v.as_table()) {
        Some(t) => t,
        None => return Ok(()),
    };

    let mut errors = Vec::new();

    for (name, value) in processor_table {
        let table = match value.as_table() {
            Some(t) => t,
            None => continue, // skip scalar fields like auto_detect, enabled
        };

        let known: &[&str] = match name.as_str() {
            processor_names::TERA => TeraConfig::known_fields(),
            processor_names::RUFF => RuffConfig::known_fields(),
            processor_names::PYLINT => PylintConfig::known_fields(),
            processor_names::CC_SINGLE_FILE => CcConfig::known_fields(),
            processor_names::CPPCHECK => CppcheckConfig::known_fields(),
            processor_names::CLANG_TIDY => ClangTidyConfig::known_fields(),
            processor_names::SHELLCHECK => ShellcheckConfig::known_fields(),
            processor_names::SPELLCHECK => SpellcheckConfig::known_fields(),
            processor_names::SLEEP => SleepConfig::known_fields(),
            processor_names::MAKE => MakeConfig::known_fields(),
            processor_names::CARGO => CargoConfig::known_fields(),
            processor_names::RUMDL => RumdlConfig::known_fields(),
            processor_names::MYPY => MypyConfig::known_fields(),
            processor_names::PYREFLY => PyreflyConfig::known_fields(),
            processor_names::YAMLLINT => YamllintConfig::known_fields(),
            processor_names::JQ => JqConfig::known_fields(),
            processor_names::JSONLINT => JsonlintConfig::known_fields(),
            processor_names::TAPLO => TaploConfig::known_fields(),
            processor_names::JSON_SCHEMA => JsonSchemaConfig::known_fields(),
            processor_names::TAGS => TagsConfig::known_fields(),
            processor_names::PIP => PipConfig::known_fields(),
            processor_names::SPHINX => SphinxConfig::known_fields(),
            processor_names::NPM => NpmConfig::known_fields(),
            processor_names::GEM => GemConfig::known_fields(),
            processor_names::MDL => MdlConfig::known_fields(),
            processor_names::MARKDOWNLINT => MarkdownlintConfig::known_fields(),
            processor_names::ASPELL => AspellConfig::known_fields(),
            processor_names::PANDOC => PandocConfig::known_fields(),
            processor_names::MARKDOWN => MarkdownGenConfig::known_fields(),
            processor_names::PDFLATEX => PdflatexConfig::known_fields(),
            processor_names::A2X => A2xConfig::known_fields(),
            processor_names::ASCII_CHECK => AsciiCheckConfig::known_fields(),
            _ => continue, // unknown processor name = Lua plugin, skip
        };

        for key in table.keys() {
            if !known.contains(&key.as_str()) {
                errors.push(format!(
                    "[processor.{}]: unknown field '{}' (valid fields: {})",
                    name, key, known.join(", ")
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("Invalid config:\n{}", errors.join("\n"))
    }
}

impl Config {
    pub(crate) fn require_config() -> Result<()> {
        let config_path = Path::new(CONFIG_FILE);
        if !config_path.exists() {
            return Err(crate::exit_code::RsbError::new(
                crate::exit_code::RsbExitCode::ConfigError,
                "No rsb.toml found. Run 'rsb init' to create one.",
            ).into());
        }
        Ok(())
    }

    pub(crate) fn load() -> Result<Self> {
        let config_path = Path::new(CONFIG_FILE);

        let mut config = if config_path.exists() {
            let content = fs::read_to_string(config_path)
                .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;
            let substituted = substitute_variables(&content)
                .with_context(|| format!("Failed to substitute variables in: {}", config_path.display()))?;
            let raw: toml::Value = toml::from_str(&substituted)
                .with_context(|| format!("Failed to parse config file: {}", config_path.display()))?;
            validate_processor_fields(&raw)?;
            toml::from_str(&substituted)
                .with_context(|| format!("Failed to parse config file: {}", config_path.display()))?
        } else {
            Config::default()
        };
        config.processor.resolve_scan_defaults();
        Ok(config)
    }
}

/// Extract a `ScanConfig` from a dynamic TOML table (used by Lua plugins).
/// Falls back to the given defaults for any missing fields.
pub(crate) fn scan_config_from_toml(
    value: &toml::Value,
    default_scan_dir: &str,
    default_extensions: &[&str],
    default_exclude_dirs: &[&str],
) -> ScanConfig {
    let table = value.as_table();

    let scan_dir = table
        .and_then(|t| t.get("scan_dir"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let extensions = table
        .and_then(|t| t.get("extensions"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());

    let exclude_dirs = table
        .and_then(|t| t.get("exclude_dirs"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());

    let exclude_files = table
        .and_then(|t| t.get("exclude_files"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());

    let exclude_paths = table
        .and_then(|t| t.get("exclude_paths"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());

    let mut scan = ScanConfig {
        scan_dir,
        extensions,
        exclude_dirs,
        exclude_files,
        exclude_paths,
    };
    scan.resolve(default_scan_dir, default_extensions, default_exclude_dirs);
    scan
}

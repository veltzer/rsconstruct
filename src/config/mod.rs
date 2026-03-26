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
use variables::substitute_variables;

const CONFIG_FILE: &str = "rsconstruct.toml";

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

/// Common + build tool dirs (/.rsconstruct/, /node_modules/, /target/).
/// Used by processors that scan broadly and need to skip build artifacts.
const BUILD_TOOL_EXCLUDES: &[&str] = &[
    "/.git/", "/out/", "/build/", "/dist/",
    "/.rsconstruct/", "/node_modules/", "/target/", "/gems/",
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
    "/.rsconstruct/", "/target/",
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
    /// Whether to compress cached objects with zstd (default: false).
    /// Incompatible with hardlink restore method.
    #[serde(default)]
    pub compression: bool,
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
            compression: false,
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

/// Auto-generate `ProcessorConfig` struct, `Default`, and all per-processor wiring
/// from the central registry in `src/registry.rs`.
macro_rules! gen_processor_config {
    ( $( $const_name:ident, $field:ident, $config_type:ty, $proc_type:ty,
         ($scan_dir:expr, $exts:expr, $excl:expr); )* ) => {

        pub(crate) fn default_processors() -> Vec<String> {
            vec![ $( crate::processors::names::$const_name.into(), )* ]
        }

        #[derive(Debug, Deserialize, Serialize)]
        pub(crate) struct ProcessorConfig {
            #[serde(default = "default_true")]
            pub auto_detect: bool,
            #[serde(default = "default_processors")]
            pub enabled: Vec<String>,
            $(
                #[serde(default)]
                pub $field: $config_type,
            )*
            /// Captures unknown [processor.PLUGIN_NAME] sections for Lua plugins
            #[serde(flatten)]
            pub extra: HashMap<String, toml::Value>,
        }

        impl Default for ProcessorConfig {
            fn default() -> Self {
                Self {
                    auto_detect: true,
                    enabled: default_processors(),
                    $( $field: <$config_type>::default(), )*
                    extra: HashMap::new(),
                }
            }
        }

        impl ProcessorConfig {
            fn processor_enabled_field(&self, name: &str) -> bool {
                match name {
                    $( stringify!($field) => self.$field.enabled, )*
                    _ => true, // unknown processors (plugins) default to enabled
                }
            }

            pub(crate) fn is_enabled(&self, name: &str) -> bool {
                self.enabled.iter().any(|p| p == name) && self.processor_enabled_field(name)
            }

            /// Collect unique scan directories from all processor configs.
            /// Returns non-empty directory names (empty means project root, handled separately).
            pub(crate) fn scan_dirs(&self) -> Vec<String> {
                let scans: &[&ScanConfig] = &[ $( &self.$field.scan, )* ];
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
                $( self.$field.scan.resolve($scan_dir, $exts, $excl); )*
            }

            /// Return known fields for a builtin processor, or None for Lua plugins.
            pub(crate) fn known_fields_for(name: &str) -> Option<&'static [&'static str]> {
                match name {
                    $( stringify!($field) => Some(<$config_type as KnownFields>::known_fields()), )*
                    _ => None,
                }
            }

            /// Return the default config for a processor as pretty JSON, or None if unknown.
            pub(crate) fn defconfig_json(name: &str) -> Option<String> {
                let json: serde_json::Value = match name {
                    $( stringify!($field) => {
                        let mut cfg = <$config_type>::default();
                        cfg.scan.resolve($scan_dir, $exts, $excl);
                        serde_json::to_value(cfg).ok()?
                    }, )*
                    _ => return None,
                };
                serde_json::to_string_pretty(&json).ok()
            }
        }
    };
}
for_each_processor!(gen_processor_config);

pub(crate) fn default_script_check_linter() -> String {
    "true".into()
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

/// Expected TOML type for a config field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldType {
    String,
    Bool,
    Integer,
    StringArray,
    /// Array of tables (e.g., [[processor.cc_single_file.compilers]])
    TableArray,
}

impl FieldType {
    fn label(self) -> &'static str {
        match self {
            FieldType::String => "a string",
            FieldType::Bool => "a boolean",
            FieldType::Integer => "an integer",
            FieldType::StringArray => "an array of strings",
            FieldType::TableArray => "an array of tables",
        }
    }

    /// Check whether a TOML value matches this expected type.
    fn matches(self, value: &toml::Value) -> bool {
        match self {
            FieldType::String => value.is_str(),
            FieldType::Bool => value.as_bool().is_some(),
            FieldType::Integer => value.is_integer(),
            FieldType::StringArray => value.as_array()
                .is_some_and(|arr| arr.iter().all(|v| v.is_str())),
            FieldType::TableArray => value.as_array()
                .is_some_and(|arr| arr.iter().all(|v| v.is_table())),
        }
    }

    fn describe_value(value: &toml::Value) -> &'static str {
        match value {
            toml::Value::String(_) => "a string",
            toml::Value::Integer(_) => "an integer",
            toml::Value::Float(_) => "a float",
            toml::Value::Boolean(_) => "a boolean",
            toml::Value::Datetime(_) => "a datetime",
            toml::Value::Array(_) => "an array",
            toml::Value::Table(_) => "a table",
        }
    }
}

/// Return the expected TOML type for a processor config field.
/// Fields common to all processors (ScanConfig fields, enabled, args, extra_inputs)
/// are handled generically. Processor-specific fields are looked up by processor name.
fn expected_field_type(processor: &str, field: &str) -> Option<FieldType> {
    // ScanConfig fields — shared by all processors
    match field {
        "scan_dir" => return Some(FieldType::String),
        "extensions" => return Some(FieldType::StringArray),
        "exclude_dirs" => return Some(FieldType::StringArray),
        "exclude_files" => return Some(FieldType::StringArray),
        "exclude_paths" => return Some(FieldType::StringArray),
        // Common processor fields
        "enabled" => return Some(FieldType::Bool),
        "args" => return Some(FieldType::StringArray),
        "extra_inputs" => return Some(FieldType::StringArray),
        "auto_inputs" => return Some(FieldType::StringArray),
        _ => {}
    }

    // Processor-specific fields
    match (processor, field) {
        // tera
        ("tera", "strict" | "trim_blocks") => Some(FieldType::Bool),
        // ruff
        ("ruff", "linter") => Some(FieldType::String),
        // cc_single_file
        ("cc_single_file", "cc" | "cxx" | "output_suffix") => Some(FieldType::String),
        ("cc_single_file", "cflags" | "cxxflags" | "ldflags" | "include_paths") => Some(FieldType::StringArray),
        ("cc_single_file", "include_scanner") => Some(FieldType::String),
        ("cc_single_file", "compilers") => Some(FieldType::TableArray),
        // cc (full project builds)
        ("cc", "cc" | "cxx") => Some(FieldType::String),
        ("cc", "cflags" | "cxxflags" | "ldflags" | "include_dirs") => Some(FieldType::StringArray),
        ("cc", "single_invocation" | "cache_output_dir") => Some(FieldType::Bool),
        // cppcheck — only common fields
        // clang_tidy
        ("clang_tidy", "compiler_args") => Some(FieldType::StringArray),
        // spellcheck
        ("spellcheck", "language" | "words_file") => Some(FieldType::String),
        ("spellcheck", "auto_add_words") => Some(FieldType::Bool),
        // make
        ("make", "make" | "target") => Some(FieldType::String),
        // cargo / clippy
        ("cargo", "profiles") => Some(FieldType::StringArray),
        ("cargo" | "clippy", "cargo" | "command") => Some(FieldType::String),
        // mypy, pyrefly, shellcheck, rumdl, yamllint, jq, jsonlint, taplo
        ("mypy" | "pyrefly" | "shellcheck" | "luacheck" | "script_check", "checker") => Some(FieldType::String),
        ("rumdl" | "yamllint" | "jsonlint" | "taplo", "linter") => Some(FieldType::String),
        ("jq", "checker") => Some(FieldType::String),
        // tags
        ("tags", "output" | "tags_file") => Some(FieldType::String),
        ("tags", "tags_file_strict") => Some(FieldType::Bool),
        // pip
        ("pip", "pip") => Some(FieldType::String),
        // sphinx
        ("sphinx", "sphinx_build" | "output_dir" | "working_dir") => Some(FieldType::String),
        // mdbook
        ("mdbook", "mdbook" | "output_dir") => Some(FieldType::String),
        // npm
        ("npm", "npm" | "command") => Some(FieldType::String),
        // gem
        ("gem", "bundler" | "command" | "gem_home") => Some(FieldType::String),
        // mdl
        ("mdl", "gem_home" | "mdl_bin" | "gem_stamp") => Some(FieldType::String),
        ("mdl", "local_repo") => Some(FieldType::Bool),
        // markdownlint
        ("markdownlint", "markdownlint_bin" | "npm_stamp") => Some(FieldType::String),
        ("markdownlint", "local_repo") => Some(FieldType::Bool),
        // aspell
        ("aspell", "aspell" | "conf_dir" | "conf" | "words_file") => Some(FieldType::String),
        ("aspell", "auto_add_words") => Some(FieldType::Bool),
        // marp
        ("marp", "marp_bin" | "output_dir") => Some(FieldType::String),
        ("marp", "formats") => Some(FieldType::StringArray),
        // pandoc
        ("pandoc", "pandoc" | "from" | "output_dir") => Some(FieldType::String),
        ("pandoc", "formats") => Some(FieldType::StringArray),
        // markdown
        ("markdown", "markdown_bin" | "output_dir") => Some(FieldType::String),
        // pdflatex
        ("pdflatex", "pdflatex" | "output_dir") => Some(FieldType::String),
        ("pdflatex", "runs") => Some(FieldType::Integer),
        ("pdflatex", "qpdf") => Some(FieldType::Bool),
        // a2x
        ("a2x", "a2x" | "format" | "output_dir") => Some(FieldType::String),
        ("chromium", "chromium_bin" | "output_dir") => Some(FieldType::String),
        // mermaid
        ("mermaid", "mmdc_bin" | "output_dir") => Some(FieldType::String),
        ("mermaid", "formats") => Some(FieldType::StringArray),
        // drawio
        ("drawio", "drawio_bin" | "output_dir") => Some(FieldType::String),
        ("drawio", "formats") => Some(FieldType::StringArray),
        // libreoffice
        ("libreoffice", "libreoffice_bin" | "output_dir") => Some(FieldType::String),
        ("libreoffice", "formats") => Some(FieldType::StringArray),
        // pdfunite
        ("pdfunite", "pdfunite_bin" | "source_dir" | "source_ext" | "source_output_dir" | "output_dir") => Some(FieldType::String),
        // objdump
        ("objdump", "output_dir") => Some(FieldType::String),
        // cache_output_dir — shared by mass generators
        ("cargo" | "sphinx" | "mdbook" | "npm" | "gem", "cache_output_dir") => Some(FieldType::Bool),
        _ => None,
    }
}

/// Validate that all fields in `[processor.X]` sections are known fields for that processor
/// and have the correct TOML types.
/// Returns an error listing unknown fields and type mismatches. Skips non-table entries
/// (like `auto_detect`) and unknown processor names (those are Lua plugin sections).
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

        let known: &[&str] = match ProcessorConfig::known_fields_for(name.as_str()) {
            Some(fields) => fields,
            None => continue, // unknown processor name = Lua plugin, skip
        };

        for (key, field_value) in table {
            if !known.contains(&key.as_str()) {
                errors.push(format!(
                    "[processor.{}]: unknown field '{}' (valid fields: {})",
                    name, key, known.join(", ")
                ));
                continue;
            }

            // Check field type if we know the expected type
            if let Some(expected) = expected_field_type(name, key)
                && !expected.matches(field_value)
            {
                errors.push(format!(
                    "[processor.{}]: field '{}' must be {}, got {} ({})",
                    name, key, expected.label(),
                    FieldType::describe_value(field_value),
                    field_value,
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
            return Err(crate::exit_code::RsconstructError::new(
                crate::exit_code::RsconstructExitCode::ConfigError,
                "No rsconstruct.toml found. Run 'rsconstruct init' to create one.",
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

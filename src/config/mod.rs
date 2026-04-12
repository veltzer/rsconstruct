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

/// Scan field names in StandardConfig.
/// These are automatically appended to every processor's known fields during validation.
pub(crate) const SCAN_CONFIG_FIELDS: &[&str] = &[
    "src_dirs", "src_extensions", "src_exclude_dirs", "src_exclude_files", "src_exclude_paths", "src_files",
];

pub(crate) trait KnownFields {
    /// Return the known fields for this config struct, excluding scan fields.
    fn known_fields() -> &'static [&'static str];

    /// Return only the fields that affect build output.
    /// Changes to these fields should trigger config change detection.
    /// Fields not listed here (e.g., src_dirs, src_exclude_dirs, batch, max_jobs)
    /// are discovery or execution parameters that don't affect what the tool produces.
    fn output_fields() -> &'static [&'static str];

    /// Return fields that must be explicitly set (non-empty) for the processor to work.
    /// If any of these fields are absent or empty in the user's config, an error is reported.
    /// Default is no required fields.
    fn must_fields() -> &'static [&'static str] {
        &[]
    }

    /// Return (field_name, description) pairs for processor-specific fields only.
    /// Shared scan/dep/exec field descriptions are added by the display layer.
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[]
    }
}

/// Default scan configuration for a processor, as plain data.
/// Used to resolve None scan fields in StandardConfig after TOML deserialization.
pub(crate) struct ScanDefaultsData {
    pub src_dirs: &'static [&'static str],
    pub src_extensions: &'static [&'static str],
    pub src_exclude_dirs: &'static [&'static str],
}

/// Per-processor default values applied after TOML deserialization.
pub(crate) struct ProcessorDefaults {
    /// Default command binary name. Empty if not applicable.
    pub command: &'static str,
    /// Default dep_auto entries.
    pub dep_auto: &'static [&'static str],
    /// Default output directory (generators only). Empty if not applicable.
    pub output_dir: &'static str,
    /// Default output formats (generators only). Empty if not applicable.
    pub formats: &'static [&'static str],
    /// Default args. Empty if not applicable.
    pub args: &'static [&'static str],
    /// Override for batch. None means leave the StandardConfig default (true).
    pub batch: Option<bool>,
}

impl Default for ProcessorDefaults {
    fn default() -> Self {
        Self {
            command: "",
            dep_auto: &[],
            output_dir: "",
            formats: &[],
            args: &[],
            batch: None,
        }
    }
}

/// Parameters for a simple checker processor — pure data, no macros.
#[derive(Copy, Clone)]
pub(crate) struct SimpleCheckerParams {
    /// Human-readable description
    pub description: &'static str,
    /// Optional subcommand (e.g., "check" for ruff)
    pub subcommand: Option<&'static str>,
    /// Args prepended before config args (e.g., ["--check"] for black)
    pub prepend_args: &'static [&'static str],
    /// Additional tools required beyond the command (e.g., ["python3", "node"])
    pub extra_tools: &'static [&'static str],
}

/// Validate dep_inputs paths exist and return them as PathBufs.
/// Paths are relative to project root (which is cwd).
pub(crate) fn resolve_extra_inputs(dep_inputs: &[String]) -> Result<Vec<PathBuf>> {
    let mut resolved = Vec::new();
    for p in dep_inputs {
        if p.contains('*') || p.contains('?') || p.contains('[') {
            // Glob pattern: expand to matching files
            for entry in glob::glob(p)
                .with_context(|| format!("Invalid glob pattern in dep_inputs: {}", p))?
            {
                let path = crate::errors::ctx(entry, &format!("Failed to read glob entry for: {}", p))?;
                if path.is_file() {
                    resolved.push(path);
                }
            }
        } else {
            let path = PathBuf::from(p);
            if !path.exists() {
                anyhow::bail!("dep_inputs file not found: {}", p);
            }
            resolved.push(path);
        }
    }
    Ok(resolved)
}

/// Descriptions for scan fields shared by every processor.
pub(crate) const SCAN_FIELD_DESCRIPTIONS: &[(&str, &str)] = &[
    ("src_dirs",            "Directories to scan for source files"),
    ("src_extensions",      "File extensions to match during scanning"),
    ("src_exclude_dirs",    "Directory path segments to skip during scanning"),
    ("src_exclude_files",   "File names to exclude from scanning"),
    ("src_exclude_paths",   "Relative paths to exclude from scanning"),
    ("src_files",           "Additional files to include alongside normal scanning"),
];

/// Descriptions for execution/dependency fields shared by most processors.
pub(crate) const SHARED_FIELD_DESCRIPTIONS: &[(&str, &str)] = &[
    ("dep_inputs",  "Extra files that trigger a rebuild when their content changes"),
    ("dep_auto",    "Config files silently added as dep_inputs when they exist on disk"),
    ("batch",       "Pass all matched files to the tool in a single invocation"),
    ("max_jobs",    "Maximum parallel jobs for this processor (overrides global --jobs)"),
];

/// Fields that never affect product output and are excluded from the output config hash.
/// These control file discovery, caching strategy, and execution batching.
const NON_OUTPUT_FIELDS: &[&str] = &[
    "src_dirs", "src_extensions", "src_exclude_dirs", "src_exclude_files", "src_exclude_paths",
    "dep_inputs", "dep_auto", "batch",
];

/// Compute a config hash including only fields that affect the product output.
/// Strips scan config, discovery, and batching fields. Additional non-output
/// fields can be excluded via `extra_exclude`.
pub(crate) fn output_config_hash(value: &impl Serialize, extra_exclude: &[&str]) -> String {
    let json_value: serde_json::Value = serde_json::to_value(value).expect(errors::CONFIG_SERIALIZE);
    let filtered = if let serde_json::Value::Object(mut map) = json_value {
        for field in NON_OUTPUT_FIELDS {
            map.remove(*field);
        }
        for field in extra_exclude {
            map.remove(*field);
        }
        serde_json::Value::Object(map)
    } else {
        json_value
    };
    let json = serde_json::to_string(&filtered).expect(errors::CONFIG_SERIALIZE);
    let hash = Sha256::digest(json.as_bytes());
    hex::encode(hash)
}



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

/// Declared project dependencies by package manager.
/// Used by `rsconstruct doctor` to verify and `rsconstruct tools install-deps` to install.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct DependenciesConfig {
    /// Python packages (installed via pip)
    #[serde(default)]
    pub pip: Vec<String>,
    /// Node.js packages (installed via npm)
    #[serde(default)]
    pub npm: Vec<String>,
    /// Ruby gems (installed via gem)
    #[serde(default)]
    pub gem: Vec<String>,
    /// System packages (checked via `which`, not auto-installed)
    #[serde(default)]
    pub system: Vec<String>,
}

impl DependenciesConfig {
    pub fn is_empty(&self) -> bool {
        self.pip.is_empty() && self.npm.is_empty() && self.gem.is_empty() && self.system.is_empty()
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
    #[serde(default)]
    pub dependencies: DependenciesConfig,
    #[serde(default)]
    pub command: CommandsConfig,
}

/// Configuration for the `symlink-install` command.
/// `sources[i]` is symlinked to `targets[i]`. Both arrays must be the same length.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct SymlinkInstallConfig {
    /// Source folders containing files to symlink
    #[serde(default)]
    pub sources: Vec<String>,
    /// Target folders where symlinks are created (same length as sources)
    #[serde(default)]
    pub targets: Vec<String>,
}

/// Configuration for custom commands.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommandsConfig {
    #[serde(default)]
    pub symlink_install: SymlinkInstallConfig,
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
    /// Global output directory prefix (default: "out").
    /// Processor output_dir fields that start with "out/" will use this as the base instead.
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
}

fn default_parallel() -> usize {
    1
}

fn default_output_dir() -> String {
    "out".into()
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            parallel: 1,
            batch_size: Some(0), // Default: batching enabled, no size limit
            output_dir: "out".into(),
        }
    }
}

/// Method used to restore files from cache
#[derive(Debug, Deserialize, Serialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RestoreMethod {
    /// Auto-detect: use copy in CI environments (CI=true), hardlink otherwise.
    #[default]
    Auto,
    Hardlink,
    Copy,
}

impl RestoreMethod {
    /// Resolve `Auto` to a concrete method based on environment.
    pub fn resolve(self) -> Self {
        match self {
            RestoreMethod::Auto => {
                if std::env::var("CI").is_ok_and(|v| v == "true") {
                    RestoreMethod::Copy
                } else {
                    RestoreMethod::Hardlink
                }
            }
            other => other,
        }
    }
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

/// A single processor instance parsed from the TOML config.
/// `[processor.pylint]` produces one instance with type_name="pylint", instance_name="pylint".
/// `[processor.pylint.core]` produces one with type_name="pylint", instance_name="pylint.core".
#[derive(Debug, Clone)]
pub(crate) struct ProcessorInstance {
    /// Instance name: "pylint" for single, "pylint.core" for named
    pub instance_name: String,
    /// Processor type name: always "pylint"
    pub type_name: String,
    /// The raw TOML config for this instance (deserialized lazily per processor type)
    pub config_toml: toml::Value,
}

use crate::registries::{self as registry, ProcessorPlugin};

pub(crate) fn find_registry_entry(type_name: &str) -> Option<&'static ProcessorPlugin> {
    registry::all_plugins().find(|e| e.name == type_name)
}

/// Return all registered processor plugins.
pub(crate) fn registry_entries() -> impl Iterator<Item = &'static ProcessorPlugin> {
    registry::all_plugins()
}

/// Return all known builtin processor type names.
pub(crate) fn all_type_names() -> Vec<&'static str> {
    registry::all_plugins().map(|e| e.name).collect()
}

/// Check if a name is a known builtin processor type.
pub(crate) fn is_builtin_type(name: &str) -> bool {
    find_registry_entry(name).is_some()
}

/// Resolve scan and processor defaults for an instance config in-place.
pub(crate) fn resolve_instance_defaults(type_name: &str, value: &mut toml::Value) -> anyhow::Result<()> {
    if find_registry_entry(type_name).is_some() {
        registry::apply_all_defaults(type_name, value);
    }
    Ok(())
}

impl ProcessorConfig {
    /// Collect unique scan directories from all declared instances.
    pub(crate) fn src_dirs(&self) -> Vec<String> {
        let mut dirs: Vec<String> = self.instances.iter()
            .flat_map(|inst| {
                inst.config_toml.get("src_dirs")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flat_map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())))
                    .filter(|d| !d.is_empty())
            })
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    /// Return known fields for a builtin processor type, or None for Lua plugins.
    pub(crate) fn known_fields_for(type_name: &str) -> Option<&'static [&'static str]> {
        find_registry_entry(type_name).map(|e| (e.known_fields)())
    }

    /// Return output-affecting fields for a builtin processor type, or None for Lua plugins.
    pub(crate) fn output_fields_for(type_name: &str) -> Option<&'static [&'static str]> {
        find_registry_entry(type_name).map(|e| (e.output_fields)())
    }

    /// Return must fields (required non-empty fields) for a builtin processor type, or None for Lua plugins.
    pub(crate) fn must_fields_for(type_name: &str) -> Option<&'static [&'static str]> {
        find_registry_entry(type_name).map(|e| (e.must_fields)())
    }

    /// Return (field, description) pairs for a builtin processor type, or None for Lua plugins.
    pub(crate) fn field_descriptions_for(type_name: &str) -> Option<&'static [(&'static str, &'static str)]> {
        find_registry_entry(type_name).map(|e| (e.field_descriptions)())
    }

    /// Return the default src_dirs for a builtin processor type, or None for Lua plugins.
    pub(crate) fn default_src_dirs_for(type_name: &str) -> Option<&'static [&'static str]> {
        scan_defaults_for(type_name).map(|d| d.src_dirs)
    }

    /// Return the default config for a processor type as pretty JSON, or None if unknown.
    pub(crate) fn defconfig_json(type_name: &str) -> Option<String> {
        let entry = find_registry_entry(type_name)?;
        (entry.defconfig_json)(entry.name)
    }
}

/// Return scan defaults for a builtin processor type.
pub(crate) fn scan_defaults_for(type_name: &str) -> Option<ScanDefaultsData> {
    Some(match type_name {
        "tera" => ScanDefaultsData { src_dirs: &["tera.templates"], src_extensions: &[".tera"], src_exclude_dirs: &[] },
        "ruff" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".py"], src_exclude_dirs: &[] },
        "pylint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".py"], src_exclude_dirs: &[] },
        "mypy" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".py"], src_exclude_dirs: &[] },
        "pyrefly" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".py"], src_exclude_dirs: &[] },
        "black" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".py"], src_exclude_dirs: &[] },
        "doctest" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".py"], src_exclude_dirs: &[] },
        "pytest" => ScanDefaultsData { src_dirs: &["tests"], src_extensions: &[".py"], src_exclude_dirs: &[] },
        "cc_single_file" => ScanDefaultsData { src_dirs: &["src"], src_extensions: &[".c", ".cc"], src_exclude_dirs: &[] },
        "cc" => ScanDefaultsData { src_dirs: &[], src_extensions: &["cc.yaml"], src_exclude_dirs: &[] },
        "cppcheck" => ScanDefaultsData { src_dirs: &["src"], src_extensions: &[".c", ".cc"], src_exclude_dirs: &[] },
        "clang_tidy" => ScanDefaultsData { src_dirs: &["src"], src_extensions: &[".c", ".cc"], src_exclude_dirs: &[] },
        "zspell" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "shellcheck" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".sh", ".bash"], src_exclude_dirs: &[] },
        "luacheck" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".lua"], src_exclude_dirs: &[] },
        "make" => ScanDefaultsData { src_dirs: &[], src_extensions: &["Makefile"], src_exclude_dirs: &[] },
        "cargo" => ScanDefaultsData { src_dirs: &[], src_extensions: &["Cargo.toml"], src_exclude_dirs: &[] },
        "clippy" => ScanDefaultsData { src_dirs: &[], src_extensions: &["Cargo.toml"], src_exclude_dirs: &[] },
        "rumdl" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "yamllint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".yml", ".yaml"], src_exclude_dirs: &[] },
        "jq" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".json"], src_exclude_dirs: &[] },
        "jsonlint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".json"], src_exclude_dirs: &[] },
        "taplo" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".toml"], src_exclude_dirs: &[] },
        "json_schema" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".json"], src_exclude_dirs: &[] },
        "tags" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "pip" => ScanDefaultsData { src_dirs: &[], src_extensions: &["requirements.txt"], src_exclude_dirs: &[] },
        "sphinx" => ScanDefaultsData { src_dirs: &[], src_extensions: &["conf.py"], src_exclude_dirs: &[] },
        "mdbook" => ScanDefaultsData { src_dirs: &[], src_extensions: &["book.toml"], src_exclude_dirs: &[] },
        "npm" => ScanDefaultsData { src_dirs: &[], src_extensions: &["package.json"], src_exclude_dirs: &[] },
        "gem" => ScanDefaultsData { src_dirs: &[], src_extensions: &["Gemfile"], src_exclude_dirs: &[] },
        "mdl" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "markdownlint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "aspell" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "marp" => ScanDefaultsData { src_dirs: &["marp"], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "pandoc" => ScanDefaultsData { src_dirs: &["pandoc"], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "markdown2html" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "pdflatex" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".tex"], src_exclude_dirs: &[] },
        "a2x" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".txt"], src_exclude_dirs: &[] },
        "ascii" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "terms" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "chromium" => ScanDefaultsData { src_dirs: &["out/marp"], src_extensions: &[".html"], src_exclude_dirs: &[] },
        "mako" => ScanDefaultsData { src_dirs: &["templates.mako"], src_extensions: &[".mako"], src_exclude_dirs: &[] },
        "jinja2" => ScanDefaultsData { src_dirs: &["templates.jinja2"], src_extensions: &[".j2"], src_exclude_dirs: &[] },
        "mermaid" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".mmd"], src_exclude_dirs: &[] },
        "drawio" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".drawio"], src_exclude_dirs: &[] },
        "libreoffice" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".odp"], src_exclude_dirs: &[] },
        "protobuf" => ScanDefaultsData { src_dirs: &["proto"], src_extensions: &[".proto"], src_exclude_dirs: &[] },
        "pdfunite" => ScanDefaultsData { src_dirs: &[], src_extensions: &["course.yaml"], src_exclude_dirs: &[] },
        "ipdfunite" => ScanDefaultsData { src_dirs: &[], src_extensions: &["course.yaml"], src_exclude_dirs: &[] },
        "script" => ScanDefaultsData { src_dirs: &[], src_extensions: &[], src_exclude_dirs: &[] },
        "creator" => ScanDefaultsData { src_dirs: &[], src_extensions: &[], src_exclude_dirs: &[] },
        "generator" => ScanDefaultsData { src_dirs: &[], src_extensions: &[], src_exclude_dirs: &[] },
        "explicit" => ScanDefaultsData { src_dirs: &[], src_extensions: &[], src_exclude_dirs: &[] },
        "linux_module" => ScanDefaultsData { src_dirs: &[], src_extensions: &["linux-module.yaml"], src_exclude_dirs: &[] },
        "cpplint" => ScanDefaultsData { src_dirs: &["src"], src_extensions: &[".c", ".cc", ".h", ".hh"], src_exclude_dirs: &[] },
        "checkpatch" => ScanDefaultsData { src_dirs: &["src"], src_extensions: &[".c", ".h"], src_exclude_dirs: &[] },
        "objdump" => ScanDefaultsData { src_dirs: &["out/cc_single_file"], src_extensions: &[".elf"], src_exclude_dirs: &[] },
        "eslint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"], src_exclude_dirs: &[] },
        "jshint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".js", ".jsx", ".mjs", ".cjs"], src_exclude_dirs: &[] },
        "htmlhint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".html", ".htm"], src_exclude_dirs: &[] },
        "tidy" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".html", ".htm"], src_exclude_dirs: &[] },
        "stylelint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".css", ".scss", ".sass", ".less"], src_exclude_dirs: &[] },
        "jslint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".js"], src_exclude_dirs: &[] },
        "standard" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".js"], src_exclude_dirs: &[] },
        "htmllint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".html", ".htm"], src_exclude_dirs: &[] },
        "php_lint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".php"], src_exclude_dirs: &[] },
        "perlcritic" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".pl", ".pm"], src_exclude_dirs: &[] },
        "xmllint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".xml", ".svg"], src_exclude_dirs: &[] },
        "svglint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".svg"], src_exclude_dirs: &[] },
        "svgo" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".svg"], src_exclude_dirs: &[] },
        "checkstyle" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".java"], src_exclude_dirs: &[] },
        "yq" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".yml", ".yaml"], src_exclude_dirs: &[] },
        "cmake" => ScanDefaultsData { src_dirs: &[], src_extensions: &["CMakeLists.txt"], src_exclude_dirs: &[] },
        "hadolint" => ScanDefaultsData { src_dirs: &[], src_extensions: &["Dockerfile"], src_exclude_dirs: &[] },
        "jekyll" => ScanDefaultsData { src_dirs: &[], src_extensions: &["_config.yml"], src_exclude_dirs: &[] },
        "sass" => ScanDefaultsData { src_dirs: &["sass"], src_extensions: &[".scss", ".sass"], src_exclude_dirs: &[] },
        "ijq" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".json"], src_exclude_dirs: &[] },
        "ijsonlint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".json"], src_exclude_dirs: &[] },
        "iyamllint" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".yml", ".yaml"], src_exclude_dirs: &[] },
        "iyamlschema" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".yml", ".yaml"], src_exclude_dirs: &[] },
        "itaplo" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".toml"], src_exclude_dirs: &[] },
        "imarkdown2html" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "isass" => ScanDefaultsData { src_dirs: &["sass"], src_extensions: &[".scss", ".sass"], src_exclude_dirs: &[] },
        "yaml2json" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".yml", ".yaml"], src_exclude_dirs: &[] },
        "rust_single_file" => ScanDefaultsData { src_dirs: &["src"], src_extensions: &[".rs"], src_exclude_dirs: &[] },
        "slidev" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "encoding" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".py", ".rs", ".js", ".ts", ".c", ".cc", ".h", ".hh", ".java", ".rb", ".go", ".sh", ".bash", ".lua", ".pl", ".pm", ".php", ".md", ".yaml", ".yml", ".json", ".toml", ".xml", ".html", ".htm", ".css", ".scss", ".sass", ".tex", ".txt"], src_exclude_dirs: &[] },
        "duplicate_files" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".py", ".rs", ".js", ".ts", ".c", ".cc", ".h", ".hh", ".java", ".rb", ".go", ".sh", ".md", ".yaml", ".yml", ".json", ".toml", ".xml", ".html", ".css"], src_exclude_dirs: &[] },
        "marp_images" => ScanDefaultsData { src_dirs: &["marp"], src_extensions: &[".md"], src_exclude_dirs: &[] },
        "license_header" => ScanDefaultsData { src_dirs: &[], src_extensions: &[".py", ".rs", ".js", ".ts", ".c", ".cc", ".h", ".hh", ".java", ".rb", ".go", ".sh", ".bash"], src_exclude_dirs: &[] },
        _ => return None,
    })
}

/// Return per-processor default values (command, dep_auto, batch override).
/// Only needed for processors whose defaults differ from the struct's Default impl.
pub(crate) fn processor_defaults_for(type_name: &str) -> Option<ProcessorDefaults> {
    let d = ProcessorDefaults::default();
    Some(match type_name {
        "ruff" => ProcessorDefaults { command: "ruff", dep_auto: &["ruff.toml", ".ruff.toml", "pyproject.toml"], ..d },
        "pylint" => ProcessorDefaults { command: "pylint", dep_auto: &[".pylintrc"], ..d },
        "pytest" => ProcessorDefaults { command: "pytest", dep_auto: &["conftest.py", "pytest.ini", "pyproject.toml"], ..d },
        "black" => ProcessorDefaults { command: "black", dep_auto: &["pyproject.toml"], ..d },
        "mypy" => ProcessorDefaults { command: "mypy", dep_auto: &["mypy.ini"], ..d },
        "pyrefly" => ProcessorDefaults { command: "pyrefly", dep_auto: &["pyproject.toml"], ..d },
        "rumdl" => ProcessorDefaults { command: "rumdl", dep_auto: &[".rumdl.toml"], ..d },
        "yamllint" => ProcessorDefaults { command: "yamllint", dep_auto: &[".yamllint", ".yamllint.yml", ".yamllint.yaml"], ..d },
        "jq" => ProcessorDefaults { command: "jq", ..d },
        "jsonlint" => ProcessorDefaults { command: "jsonlint", ..d },
        "taplo" => ProcessorDefaults { command: "taplo", dep_auto: &["taplo.toml", ".taplo.toml"], ..d },
        "cppcheck" => ProcessorDefaults { command: "cppcheck", args: &["--error-exitcode=1", "--enable=warning,style,performance,portability"], dep_auto: &[".cppcheck"], batch: Some(false), ..d },
        "cpplint" => ProcessorDefaults { command: "cpplint", ..d },
        "checkpatch" => ProcessorDefaults { command: "checkpatch.pl", ..d },
        "shellcheck" => ProcessorDefaults { command: "shellcheck", dep_auto: &[".shellcheckrc"], ..d },
        "luacheck" => ProcessorDefaults { command: "luacheck", dep_auto: &[".luacheckrc"], ..d },
        "eslint" => ProcessorDefaults { command: "eslint", dep_auto: &[".eslintrc", ".eslintrc.json", ".eslintrc.js", ".eslintrc.yml", ".eslintrc.yaml", ".eslintrc.cjs", "eslint.config.js", "eslint.config.mjs", "eslint.config.cjs"], ..d },
        "jshint" => ProcessorDefaults { command: "jshint", dep_auto: &[".jshintrc"], ..d },
        "htmlhint" => ProcessorDefaults { command: "htmlhint", dep_auto: &[".htmlhintrc"], ..d },
        "stylelint" => ProcessorDefaults { command: "stylelint", dep_auto: &[".stylelintrc", ".stylelintrc.json", ".stylelintrc.yml", ".stylelintrc.yaml", ".stylelintrc.js", ".stylelintrc.cjs", "stylelint.config.js", "stylelint.config.cjs"], ..d },
        "perlcritic" => ProcessorDefaults { command: "perlcritic", dep_auto: &[".perlcriticrc"], ..d },
        "svglint" => ProcessorDefaults { command: "svglint", dep_auto: &[".svglintrc.js"], ..d },
        "svgo" => ProcessorDefaults { command: "svgo", dep_auto: &["svgo.config.js", "svgo.config.mjs", "svgo.config.cjs"], ..d },
        "checkstyle" => ProcessorDefaults { command: "checkstyle", dep_auto: &["checkstyle.xml"], ..d },
        "cmake" => ProcessorDefaults { command: "cmake", ..d },
        "doctest" => ProcessorDefaults { command: "python3", ..d },
        "hadolint" => ProcessorDefaults { command: "hadolint", ..d },
        "htmllint" => ProcessorDefaults { command: "htmllint", ..d },
        "jslint" => ProcessorDefaults { command: "jslint", ..d },
        "php_lint" => ProcessorDefaults { command: "php", ..d },
        "slidev" => ProcessorDefaults { command: "slidev", ..d },
        "standard" => ProcessorDefaults { command: "standard", ..d },
        "tidy" => ProcessorDefaults { command: "tidy", ..d },
        "xmllint" => ProcessorDefaults { command: "xmllint", ..d },
        "yq" => ProcessorDefaults { command: "yq", ..d },
        // Generators
        "marp" => ProcessorDefaults { output_dir: "out/marp", formats: &["pdf"], args: &["--html", "--allow-local-files"], command: "marp", ..d },
        "markdown2html" => ProcessorDefaults { output_dir: "out/markdown2html", command: "markdown", ..d },
        "chromium" => ProcessorDefaults { output_dir: "out/chromium", command: "google-chrome", ..d },
        "mermaid" => ProcessorDefaults { output_dir: "out/mermaid", formats: &["png"], command: "mmdc", ..d },
        "drawio" => ProcessorDefaults { output_dir: "out/drawio", formats: &["png"], command: "drawio", ..d },
        "libreoffice" => ProcessorDefaults { output_dir: "out/libreoffice", formats: &["pdf"], command: "libreoffice", ..d },
        "protobuf" => ProcessorDefaults { output_dir: "out/protobuf", command: "protoc", ..d },
        "cc_single_file" => ProcessorDefaults { output_dir: "out/cc_single_file", ..d },
        "cargo" => ProcessorDefaults { command: "build", ..d },
        "clippy" => ProcessorDefaults { command: "clippy", ..d },
        "generator" => ProcessorDefaults { output_dir: "out/generator", ..d },
        "explicit" => ProcessorDefaults { ..d },
        "sphinx" => ProcessorDefaults { output_dir: "docs", ..d },
        "mdbook" => ProcessorDefaults { output_dir: "book", ..d },
        "npm" => ProcessorDefaults { command: "install", ..d },
        "sass" => ProcessorDefaults { output_dir: "out/sass", command: "sass", ..d },
        "pandoc" => ProcessorDefaults { output_dir: "out/pandoc", formats: &["pdf", "html", "docx"], command: "pandoc", ..d },
        "a2x" => ProcessorDefaults { output_dir: "out/a2x", command: "a2x", ..d },
        "objdump" => ProcessorDefaults { output_dir: "out/objdump", command: "objdump", ..d },
        "imarkdown2html" => ProcessorDefaults { output_dir: "out/imarkdown2html", ..d },
        "isass" => ProcessorDefaults { output_dir: "out/isass", ..d },
        "yaml2json" => ProcessorDefaults { output_dir: "out/yaml2json", ..d },
        // Checkers with custom dep_auto
        "mdl" => ProcessorDefaults { dep_auto: &[".mdlrc"], ..d },
        "markdownlint" => ProcessorDefaults { dep_auto: &[".markdownlint.json", ".markdownlint.jsonc", ".markdownlint.yaml"], ..d },
        "aspell" => ProcessorDefaults { dep_auto: &[".aspell.conf", ".aspell.en.pws", ".aspell.en.prepl"], ..d },
        // Generators with custom output_dir
        "pdflatex" => ProcessorDefaults { output_dir: "out/pdflatex", ..d },
        "rust_single_file" => ProcessorDefaults { output_dir: "out/rust_single_file", ..d },
        "pdfunite" => ProcessorDefaults { output_dir: "out/pdfunite", ..d },
        "ipdfunite" => ProcessorDefaults { output_dir: "out/ipdfunite", ..d },
        // Creators with custom command
        "gem" => ProcessorDefaults { command: "install", ..d },
        _ => return None,
    })
}

/// Apply processor-specific defaults to a config TOML value.
/// Sets command and dep_auto if they weren't explicitly provided by the user.
pub(crate) fn apply_processor_defaults(type_name: &str, value: &mut toml::Value) {
    if let Some(defaults) = processor_defaults_for(type_name) {
        let table = match value.as_table_mut() {
            Some(t) => t,
            None => return,
        };
        let set_string = |t: &mut toml::map::Map<String, toml::Value>, key: &str, val: &str| {
            if !val.is_empty() && !t.contains_key(key) {
                t.insert(key.into(), toml::Value::String(val.into()));
            }
        };
        let set_array = |t: &mut toml::map::Map<String, toml::Value>, key: &str, vals: &[&str]| {
            if !vals.is_empty() && !t.contains_key(key) {
                let arr: Vec<toml::Value> = vals.iter().map(|s| toml::Value::String(s.to_string())).collect();
                t.insert(key.into(), toml::Value::Array(arr));
            }
        };
        set_string(table, "command", defaults.command);
        set_string(table, "output_dir", defaults.output_dir);
        set_array(table, "dep_auto", defaults.dep_auto);
        set_array(table, "formats", defaults.formats);
        set_array(table, "args", defaults.args);
        if let Some(batch) = defaults.batch {
            if !table.contains_key("batch") {
                table.insert("batch".into(), toml::Value::Boolean(batch));
            }
        }
    }
}

/// Apply scan defaults to a config TOML value.
/// Sets src_dirs, src_extensions, and src_exclude_dirs if not explicitly provided.
pub(crate) fn apply_scan_defaults(type_name: &str, value: &mut toml::Value) {
    if let Some(defaults) = scan_defaults_for(type_name) {
        let table = match value.as_table_mut() {
            Some(t) => t,
            None => return,
        };
        let set_array = |t: &mut toml::map::Map<String, toml::Value>, key: &str, vals: &[&str]| {
            if !t.contains_key(key) {
                let arr: Vec<toml::Value> = vals.iter().map(|s| toml::Value::String(s.to_string())).collect();
                t.insert(key.into(), toml::Value::Array(arr));
            }
        };
        set_array(table, "src_dirs", defaults.src_dirs);
        set_array(table, "src_extensions", defaults.src_extensions);
        set_array(table, "src_exclude_dirs", defaults.src_exclude_dirs);
        // resolve_with also fills these with empty vecs if None
        let set_empty = |t: &mut toml::map::Map<String, toml::Value>, key: &str| {
            if !t.contains_key(key) {
                t.insert(key.into(), toml::Value::Array(Vec::new()));
            }
        };
        set_empty(table, "src_exclude_files");
        set_empty(table, "src_exclude_paths");
        set_empty(table, "src_files");
    }
}

/// Processor configuration: a collection of declared processor instances.
/// Each `[processor.TYPE]` or `[processor.TYPE.NAME]` section in rsconstruct.toml
/// creates a ProcessorInstance. No instances exist by default — only what's declared.
#[derive(Debug, Default)]
pub(crate) struct ProcessorConfig {
    /// All declared processor instances
    pub instances: Vec<ProcessorInstance>,
    /// Lua plugin configs (processor types not in the builtin registry)
    pub extra: HashMap<String, toml::Value>,
}

impl Serialize for ProcessorConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        for inst in &self.instances {
            // For named instances (type.name), we need to nest
            if inst.instance_name.contains('.') {
                // Handled as part of the parent type table
            } else {
                map.serialize_entry(&inst.instance_name, &inst.config_toml)?;
            }
        }
        // Group named instances by type
        let mut types: HashMap<&str, Vec<&ProcessorInstance>> = HashMap::new();
        for inst in &self.instances {
            if let Some(dot) = inst.instance_name.find('.') {
                let type_name = &inst.instance_name[..dot];
                types.entry(type_name).or_default().push(inst);
            }
        }
        for (type_name, insts) in &types {
            let mut table = toml::map::Map::new();
            for inst in insts {
                let name = &inst.instance_name[type_name.len() + 1..];
                table.insert(name.to_string(), inst.config_toml.clone());
            }
            map.serialize_entry(type_name, &toml::Value::Table(table))?;
        }
        for (name, value) in &self.extra {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ProcessorConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let table = toml::Value::deserialize(deserializer)?;
        ProcessorConfig::from_toml(&table).map_err(serde::de::Error::custom)
    }
}

impl ProcessorConfig {
    /// Parse the `[processor]` table from TOML into instances.
    pub(crate) fn from_toml(value: &toml::Value) -> Result<Self> {
        let table = match value.as_table() {
            Some(t) => t,
            None => return Ok(Self::default()),
        };

        let mut instances = Vec::new();
        let mut extra = HashMap::new();

        for (key, val) in table {
            let sub_table = match val.as_table() {
                Some(t) => t,
                None => continue, // skip non-table entries
            };

            if is_builtin_type(key) {
                // Check if this is single-instance or multi-instance
                if Self::is_multi_instance(key, sub_table) {
                    // Multi-instance: [processor.pylint.core], [processor.pylint.tests]
                    for (name, inst_val) in sub_table {
                        let instance_name = format!("{}.{}", key, name);
                        let mut config = inst_val.clone();
                        resolve_instance_defaults(key, &mut config)?;
                        instances.push(ProcessorInstance {
                            instance_name,
                            type_name: key.clone(),
                            config_toml: config,
                        });
                    }
                } else {
                    // Single instance: [processor.pylint]
                    let mut config = val.clone();
                    resolve_instance_defaults(key, &mut config)?;
                    instances.push(ProcessorInstance {
                        instance_name: key.clone(),
                        type_name: key.clone(),
                        config_toml: config,
                    });
                }
            } else {
                // Unknown type — Lua plugin
                extra.insert(key.clone(), val.clone());
            }
        }

        Ok(Self { instances, extra })
    }

    /// Determine if a processor section contains named sub-instances (multi-instance)
    /// or direct config fields (single-instance).
    ///
    /// Heuristic: if ALL values are tables AND none of the keys match known config
    /// field names for this processor type, it's multi-instance.
    fn is_multi_instance(type_name: &str, table: &toml::map::Map<String, toml::Value>) -> bool {
        if table.is_empty() {
            return false;
        }

        let known = Self::known_fields_for(type_name);
        let known_fields: Vec<&str> = match known {
            Some(fields) => fields.iter()
                .chain(SCAN_CONFIG_FIELDS.iter())
                .copied()
                .collect(),
            None => return false,
        };

        // If ANY key is a known config field, it's single-instance
        for key in table.keys() {
            if known_fields.contains(&key.as_str()) {
                return false;
            }
        }

        // If ALL values are tables, it's multi-instance
        table.values().all(|v| v.is_table())
    }

    /// Resolve scan defaults for all instances.
    pub(crate) fn resolve_scan_defaults(&mut self) {
        for inst in &mut self.instances {
            resolve_instance_defaults(&inst.type_name, &mut inst.config_toml).ok();
        }
    }

    /// Rewrite output paths in processor configs:
    /// 1. For named instances (e.g., `pylint.core`), remap default `out/{type_name}`
    ///    to `out/{instance_name}` so each instance gets its own output directory.
    /// 2. If the global `output_dir` is not "out", replace the `out/` prefix with the
    ///    global value (e.g., `build/` → `build/marp`).
    pub(crate) fn apply_output_dir_defaults(&mut self, global_output_dir: &str) {
        for inst in &mut self.instances {
            let type_default_prefix = format!("out/{}", inst.type_name);
            let instance_prefix = format!("{}/{}", global_output_dir, inst.instance_name);

            for field in &["output_dir", "output"] {
                let val = match inst.config_toml.get(field).and_then(|v| v.as_str()).map(|s| s.to_string()) {
                    Some(v) => v,
                    None => continue,
                };
                let new_val = if inst.instance_name != inst.type_name && val.starts_with(&type_default_prefix) {
                    // Named instance: remap out/{type} → {global}/{instance}
                    format!("{}{}", instance_prefix, &val[type_default_prefix.len()..])
                } else if global_output_dir != "out" && val.starts_with("out/") {
                    // Global output dir override: remap out/ → {global}/
                    format!("{}/{}", global_output_dir, &val[4..])
                } else {
                    continue;
                };
                if let Some(table) = inst.config_toml.as_table_mut() {
                    table.insert(field.to_string(), toml::Value::String(new_val));
                }
            }
        }
    }

    /// Get the first instance of a given type (for single-instance access).
    /// Returns None if no instance of that type is declared.
    pub(crate) fn first_instance_of_type(&self, type_name: &str) -> Option<&ProcessorInstance> {
        self.instances.iter().find(|i| i.type_name == type_name)
    }

    /// Get a typed config value from an instance's TOML config.
    /// Returns the default if the instance is not declared or the field is missing.
    pub(crate) fn instance_field_str(&self, type_name: &str, field: &str) -> Option<String> {
        self.first_instance_of_type(type_name)
            .and_then(|inst| inst.config_toml.get(field))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
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

/// A single analyzer instance parsed from the TOML config.
/// `[analyzer.cpp]` produces one instance with type_name="cpp", instance_name="cpp".
/// `[analyzer.cpp.kernel]` produces one with type_name="cpp", instance_name="cpp.kernel".
#[derive(Debug, Clone)]
pub(crate) struct AnalyzerInstance {
    /// Instance name: "cpp" for single, "cpp.kernel" for named
    pub instance_name: String,
    /// Analyzer type name (must match a registered AnalyzerPlugin name)
    pub type_name: String,
    /// The raw TOML config for this instance
    pub config_toml: toml::Value,
}

/// Configuration for dependency analyzers.
/// Each `[analyzer.NAME]` section in rsconstruct.toml creates an AnalyzerInstance.
/// No analyzers run unless explicitly declared in the config.
#[derive(Debug, Default)]
pub(crate) struct AnalyzerConfig {
    /// All declared analyzer instances
    pub instances: Vec<AnalyzerInstance>,
}

impl Serialize for AnalyzerConfig {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        // Emit single instances directly; group named sub-instances under the type.
        for inst in &self.instances {
            if inst.instance_name == inst.type_name {
                map.serialize_entry(&inst.instance_name, &inst.config_toml)?;
            }
        }
        let mut types: HashMap<&str, Vec<&AnalyzerInstance>> = HashMap::new();
        for inst in &self.instances {
            if inst.instance_name != inst.type_name {
                types.entry(inst.type_name.as_str()).or_default().push(inst);
            }
        }
        for (type_name, insts) in &types {
            let mut table = toml::map::Map::new();
            for inst in insts {
                let name = &inst.instance_name[type_name.len() + 1..];
                table.insert(name.to_string(), inst.config_toml.clone());
            }
            map.serialize_entry(type_name, &toml::Value::Table(table))?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for AnalyzerConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let table = toml::Value::deserialize(deserializer)?;
        AnalyzerConfig::from_toml(&table).map_err(serde::de::Error::custom)
    }
}

impl AnalyzerConfig {
    /// Parse the `[analyzer]` table from TOML into instances.
    ///
    /// Supports both single-instance and multi-instance syntax:
    /// - `[analyzer.cpp]` with config fields → one instance, instance_name="cpp"
    /// - `[analyzer.cpp.kernel]` + `[analyzer.cpp.userspace]` → two instances,
    ///   instance_name="cpp.kernel" and "cpp.userspace"
    pub(crate) fn from_toml(value: &toml::Value) -> Result<Self> {
        let table = match value.as_table() {
            Some(t) => t,
            None => return Ok(Self::default()),
        };

        let mut instances = Vec::new();
        for (type_name, val) in table {
            if registry::find_analyzer_plugin(type_name).is_none() {
                anyhow::bail!("Unknown analyzer '{}'. Run 'rsconstruct analyzers list' to see available analyzers.", type_name);
            }
            let sub_table = match val.as_table() {
                Some(t) => t,
                None => anyhow::bail!("Expected [analyzer.{}] to be a table", type_name),
            };
            if Self::is_multi_instance(sub_table) {
                for (name, inst_val) in sub_table {
                    instances.push(AnalyzerInstance {
                        instance_name: format!("{}.{}", type_name, name),
                        type_name: type_name.clone(),
                        config_toml: inst_val.clone(),
                    });
                }
            } else {
                instances.push(AnalyzerInstance {
                    instance_name: type_name.clone(),
                    type_name: type_name.clone(),
                    config_toml: val.clone(),
                });
            }
        }
        Ok(Self { instances })
    }

    /// Multi-instance iff the table is non-empty and every value is itself a
    /// table. Single-instance if any value is a scalar/array (i.e. a config field).
    fn is_multi_instance(table: &toml::map::Map<String, toml::Value>) -> bool {
        !table.is_empty() && table.values().all(|v| v.is_table())
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
/// Fields common to all processors (scan fields, enabled, args, dep_inputs)
/// are handled generically. Processor-specific fields are looked up by processor name.
fn expected_field_type(processor: &str, field: &str) -> Option<FieldType> {
    // Scan fields — shared by all processors
    match field {
        "src_dirs" => return Some(FieldType::StringArray),
        "src_extensions" => return Some(FieldType::StringArray),
        "src_exclude_dirs" => return Some(FieldType::StringArray),
        "src_exclude_files" => return Some(FieldType::StringArray),
        "src_exclude_paths" => return Some(FieldType::StringArray),
        // Common processor fields
        "args" => return Some(FieldType::StringArray),
        "dep_inputs" => return Some(FieldType::StringArray),
        "dep_auto" => return Some(FieldType::StringArray),
        _ => {}
    }

    // Processor-specific fields
    match (processor, field) {
        // tera
        // tera has no processor-specific fields
        // ruff
        ("ruff", "command") => Some(FieldType::String),
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
        // zspell
        ("zspell", "language" | "words_file") => Some(FieldType::String),
        ("zspell", "auto_add_words") => Some(FieldType::Bool),
        // make
        ("make", "make" | "target") => Some(FieldType::String),
        // cargo / clippy
        ("cargo", "profiles") => Some(FieldType::StringArray),
        ("cargo" | "clippy", "cargo" | "command") => Some(FieldType::String),
        // mypy, pyrefly, shellcheck, rumdl, yamllint, jq, jsonlint, taplo
        ("mypy" | "pyrefly" | "shellcheck" | "luacheck" | "script", "checker") => Some(FieldType::String),
        ("rumdl" | "yamllint" | "jsonlint" | "taplo", "linter") => Some(FieldType::String),
        ("jq", "checker") => Some(FieldType::String),
        // tags
        ("tags", "output" | "tags_dir") => Some(FieldType::String),
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
        ("aspell", "aspell" | "conf" | "words_file") => Some(FieldType::String),
        ("aspell", "auto_add_words") => Some(FieldType::Bool),
        // marp
        ("marp", "marp_bin" | "output_dir") => Some(FieldType::String),
        ("marp", "formats") => Some(FieldType::StringArray),
        // pandoc
        ("pandoc", "pandoc" | "output_dir") => Some(FieldType::String),
        ("pandoc", "formats") => Some(FieldType::StringArray),
        // markdown
        ("markdown2html", "markdown_bin" | "output_dir") => Some(FieldType::String),
        // pdflatex
        ("pdflatex", "pdflatex" | "output_dir") => Some(FieldType::String),
        ("pdflatex", "runs") => Some(FieldType::Integer),
        ("pdflatex", "qpdf") => Some(FieldType::Bool),
        // a2x
        ("a2x", "a2x" | "output_dir") => Some(FieldType::String),
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
        // cache_output_dir — shared by creators
        ("cargo" | "sphinx" | "mdbook" | "npm" | "gem", "cache_output_dir") => Some(FieldType::Bool),
        _ => None,
    }
}

/// Validate fields in a single processor config table.
fn validate_single_processor(
    type_name: &str,
    section_label: &str,
    table: &toml::map::Map<String, toml::Value>,
    errors: &mut Vec<String>,
) {
    let own_fields: &[&str] = match ProcessorConfig::known_fields_for(type_name) {
        Some(fields) => fields,
        None => return, // unknown = Lua plugin, skip
    };

    for (key, field_value) in table {
        if !own_fields.contains(&key.as_str()) && !SCAN_CONFIG_FIELDS.contains(&key.as_str()) {
            let all_fields: Vec<&str> = own_fields.iter()
                .chain(SCAN_CONFIG_FIELDS.iter())
                .copied()
                .collect();
            errors.push(format!(
                "[{}]: unknown field '{}' (valid fields: {})",
                section_label, key, all_fields.join(", ")
            ));
            continue;
        }

        if let Some(expected) = expected_field_type(type_name, key)
            && !expected.matches(field_value)
        {
            errors.push(format!(
                "[{}]: field '{}' must be {}, got {} ({})",
                section_label, key, expected.label(),
                FieldType::describe_value(field_value),
                field_value,
            ));
        }
    }

    // Check that all must_fields are present and non-empty
    if let Some(must) = ProcessorConfig::must_fields_for(type_name) {
        for field in must {
            match table.get(*field) {
                None => {
                    errors.push(format!(
                        "[{}]: required field '{}' must be specified",
                        section_label, field,
                    ));
                }
                Some(toml::Value::Array(arr)) if arr.is_empty() => {
                    errors.push(format!(
                        "[{}]: required field '{}' must not be empty",
                        section_label, field,
                    ));
                }
                Some(toml::Value::String(s)) if s.is_empty() => {
                    errors.push(format!(
                        "[{}]: required field '{}' must not be empty",
                        section_label, field,
                    ));
                }
                _ => {} // present and non-empty: OK
            }
        }
    }

    // Require src_dirs for processors whose default would scan the project root.
    // This prevents accidentally scanning everything when the user forgets to set src_dirs.
    // Exempt processors that don't use src_dirs for file discovery,
    // and processors that specify src_files (files are explicitly listed).
    const SCAN_DIRS_EXEMPT: &[&str] = &["explicit", "pdfunite", "ipdfunite"];
    let has_match_paths = table.get("src_files")
        .and_then(|v| v.as_array())
        .is_some_and(|arr| !arr.is_empty());
    if ProcessorConfig::default_src_dirs_for(type_name).is_some_and(|dirs| dirs.is_empty())
    && !SCAN_DIRS_EXEMPT.contains(&type_name)
    && !has_match_paths {
        match table.get("src_dirs") {
            None => {
                errors.push(format!(
                    "[{}]: 'src_dirs' must be specified (this processor defaults to scanning the project root)",
                    section_label,
                ));
            }
            Some(toml::Value::Array(arr))
                if arr.len() == 1
                    && arr[0].as_str().is_some_and(|s| s.is_empty()) =>
            {
                errors.push(format!(
                    "[{}]: 'src_dirs' must not contain empty strings; specify actual directories to scan",
                    section_label,
                ));
            }
            _ => {} // present and non-empty: OK
        }
    }
}

/// Validate that all fields in `[processor.X]` sections are known fields for that processor
/// and have the correct TOML types. Supports both single-instance and multi-instance formats.
fn validate_processor_fields(raw: &toml::Value) -> Result<()> {
    let processor_table = match raw.get("processor").and_then(|v| v.as_table()) {
        Some(t) => t,
        None => return Ok(()),
    };

    let mut errors = Vec::new();

    for (name, value) in processor_table {
        let table = match value.as_table() {
            Some(t) => t,
            None => continue, // skip non-table entries
        };

        if !is_builtin_type(name) {
            // Check if there's a matching Lua plugin file
            let plugins_dir = raw.get("plugins")
                .and_then(|p| p.get("dir"))
                .and_then(|d| d.as_str())
                .unwrap_or(DEFAULT_PLUGINS_DIR);
            let plugin_path = std::path::Path::new(plugins_dir)
                .join(format!("{}.lua", name));
            if !plugin_path.exists() {
                errors.push(format!(
                    "[processor.{}]: unknown processor type '{}' (not a builtin processor or Lua plugin at {})",
                    name, name, plugin_path.display(),
                ));
            }
            continue;
        }

        // Check if multi-instance
        if ProcessorConfig::is_multi_instance(name, table) {
            for (inst_name, inst_value) in table {
                if let Some(inst_table) = inst_value.as_table() {
                    let section = format!("processor.{}.{}", name, inst_name);
                    validate_single_processor(name, &section, inst_table, &mut errors);
                }
            }
        } else {
            let section = format!("processor.{}", name);
            validate_single_processor(name, &section, table, &mut errors);
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
        config.processor.apply_output_dir_defaults(&config.build.output_dir);
        Ok(config)
    }
}

/// Extract a `StandardConfig` with scan fields from a dynamic TOML table (used by Lua plugins).
/// Falls back to the given defaults for any missing scan fields.
pub(crate) fn standard_config_from_toml(
    value: &toml::Value,
    default_src_dirs: &[&str],
    default_src_extensions: &[&str],
    default_exclude_dirs: &[&str],
) -> StandardConfig {
    let table = value.as_table();

    let toml_array = |key: &str| -> Option<Vec<String>> {
        table
            .and_then(|t| t.get(key))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
    };

    let mut cfg = StandardConfig {
        src_dirs: toml_array("src_dirs"),
        src_extensions: toml_array("src_extensions"),
        src_exclude_dirs: toml_array("src_exclude_dirs"),
        src_exclude_files: toml_array("src_exclude_files"),
        src_exclude_paths: toml_array("src_exclude_paths"),
        src_files: toml_array("src_files"),
        ..StandardConfig::default()
    };
    // Fill defaults for None fields
    if cfg.src_dirs.is_none() {
        cfg.src_dirs = Some(default_src_dirs.iter().map(|s| s.to_string()).collect());
    }
    if cfg.src_extensions.is_none() {
        cfg.src_extensions = Some(default_src_extensions.iter().map(|s| s.to_string()).collect());
    }
    if cfg.src_exclude_dirs.is_none() {
        cfg.src_exclude_dirs = Some(default_exclude_dirs.iter().map(|s| s.to_string()).collect());
    }
    if cfg.src_exclude_files.is_none() {
        cfg.src_exclude_files = Some(Vec::new());
    }
    if cfg.src_exclude_paths.is_none() {
        cfg.src_exclude_paths = Some(Vec::new());
    }
    if cfg.src_files.is_none() {
        cfg.src_files = Some(Vec::new());
    }
    cfg
}

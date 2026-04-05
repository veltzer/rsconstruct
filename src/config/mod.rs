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

/// Fields contributed by ScanConfig via `#[serde(flatten)]`.
/// These are automatically appended to every processor's known fields during validation.
pub(crate) const SCAN_CONFIG_FIELDS: &[&str] = &[
    "scan_dirs", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
];

pub(crate) trait KnownFields {
    /// Return the known fields for this config struct, excluding ScanConfig fields.
    fn known_fields() -> &'static [&'static str];
}


/// Validate extra_inputs paths exist and return them as PathBufs.
/// Paths are relative to project root (which is cwd).
pub(crate) fn resolve_extra_inputs(extra_inputs: &[String]) -> Result<Vec<PathBuf>> {
    let mut resolved = Vec::new();
    for p in extra_inputs {
        if p.contains('*') || p.contains('?') || p.contains('[') {
            // Glob pattern: expand to matching files
            for entry in glob::glob(p)
                .with_context(|| format!("Invalid glob pattern in extra_inputs: {}", p))?
            {
                let path = entry.with_context(|| format!("Failed to read glob entry for: {}", p))?;
                if path.is_file() {
                    resolved.push(path);
                }
            }
        } else {
            let path = PathBuf::from(p);
            if !path.exists() {
                anyhow::bail!("extra_inputs file not found: {}", p);
            }
            resolved.push(path);
        }
    }
    Ok(resolved)
}

/// Fields that never affect product output and are excluded from the output config hash.
/// These control file discovery, caching strategy, and execution batching.
const NON_OUTPUT_FIELDS: &[&str] = &[
    "scan_dirs", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
    "extra_inputs", "auto_inputs", "batch",
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

/// Common scan configuration shared by all processors.
/// Each processor embeds this via `#[serde(flatten)]` and provides its own defaults.
///
/// Fields use `Option` so that serde can distinguish "not specified" (None) from
/// "explicitly set" (Some). `resolve_scan_defaults()` fills in None values after
/// loading, so processors can always unwrap safely.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub(crate) struct ScanConfig {
    /// Directories to scan for source files ("" means project root)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_dirs: Option<Vec<String>>,

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
        if self.scan_dirs.is_none() {
            self.scan_dirs = Some(vec![scan_dir.to_string()]);
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

    /// Get the resolved scan directories. Panics if called before resolve().
    pub(crate) fn scan_dirs(&self) -> &[String] {
        self.scan_dirs.as_deref().expect(errors::SCAN_CONFIG_NOT_RESOLVED)
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
pub(crate) const BUILD_TOOL_EXCLUDES: &[&str] = &[];
pub(crate) const PYTHON_EXCLUDE_DIRS: &[&str] = &[];
pub(crate) const CC_EXCLUDE_DIRS: &[&str] = &[];
pub(crate) const ZSPELL_EXCLUDE_DIRS: &[&str] = &[];
pub(crate) const SHELL_EXCLUDE_DIRS: &[&str] = &[];
pub(crate) const MARKDOWN_EXCLUDE_DIRS: &[&str] = &[];
pub(crate) const MAKE_CARGO_EXCLUDES: &[&str] = &[];

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

/// Auto-generate `ProcessorConfig` and all per-processor wiring
/// from the central registry in `src/registry.rs`.
macro_rules! gen_processor_config {
    ( $( $const_name:ident, $field:ident, $config_type:ty, $proc_type:ty,
         ($scan_dir:expr, $exts:expr, $excl:expr); )* ) => {

        /// Return all known builtin processor type names.
        pub(crate) fn all_type_names() -> Vec<&'static str> {
            vec![ $( stringify!($field), )* ]
        }

        /// Check if a name is a known builtin processor type.
        pub(crate) fn is_builtin_type(name: &str) -> bool {
            matches!(name, $( stringify!($field) )|*)
        }


        /// Resolve scan defaults for an instance config in-place.
        pub(crate) fn resolve_instance_scan_defaults(type_name: &str, value: &mut toml::Value) -> anyhow::Result<()> {
            match type_name {
                $(
                    stringify!($field) => {
                        let mut cfg: $config_type = toml::from_str(&toml::to_string(value)?)?;
                        cfg.scan.resolve($scan_dir, $exts, $excl);
                        *value = toml::Value::try_from(&cfg)?;
                        Ok(())
                    }
                )*
                _ => Ok(()), // Lua plugins handle their own defaults
            }
        }

        impl ProcessorConfig {
            /// Collect unique scan directories from all declared instances.
            pub(crate) fn scan_dirs(&self) -> Vec<String> {
                let mut dirs: Vec<String> = self.instances.iter()
                    .flat_map(|inst| {
                        inst.config_toml.get("scan_dirs")
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
                match type_name {
                    $( stringify!($field) => Some(<$config_type as KnownFields>::known_fields()), )*
                    _ => None,
                }
            }

            /// Return the default config for a processor type as pretty JSON, or None if unknown.
            pub(crate) fn defconfig_json(type_name: &str) -> Option<String> {
                let json: serde_json::Value = match type_name {
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
                        resolve_instance_scan_defaults(key, &mut config).ok();
                        instances.push(ProcessorInstance {
                            instance_name,
                            type_name: key.clone(),
                            config_toml: config,
                        });
                    }
                } else {
                    // Single instance: [processor.pylint]
                    let mut config = val.clone();
                    resolve_instance_scan_defaults(key, &mut config).ok();
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
            resolve_instance_scan_defaults(&inst.type_name, &mut inst.config_toml).ok();
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

pub(crate) fn default_script_linter() -> String {
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
        "scan_dirs" => return Some(FieldType::StringArray),
        "extensions" => return Some(FieldType::StringArray),
        "exclude_dirs" => return Some(FieldType::StringArray),
        "exclude_files" => return Some(FieldType::StringArray),
        "exclude_paths" => return Some(FieldType::StringArray),
        // Common processor fields
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

/// Extract a `ScanConfig` from a dynamic TOML table (used by Lua plugins).
/// Falls back to the given defaults for any missing fields.
pub(crate) fn scan_config_from_toml(
    value: &toml::Value,
    default_scan_dir: &str,
    default_extensions: &[&str],
    default_exclude_dirs: &[&str],
) -> ScanConfig {
    let table = value.as_table();

    let scan_dirs = table
        .and_then(|t| t.get("scan_dirs"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());

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
        scan_dirs,
        extensions,
        exclude_dirs,
        exclude_files,
        exclude_paths,
    };
    scan.resolve(default_scan_dir, default_extensions, default_exclude_dirs);
    scan
}

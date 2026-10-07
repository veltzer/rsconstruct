mod analyzer_configs;
mod dependencies;
mod overrides;
mod processor_configs;
mod processors;
mod provenance;
mod settings;
#[cfg(test)]
mod tests;
mod validation;
mod variables;

pub use analyzer_configs::*;
pub use dependencies::*;
use overrides::{apply_override_to_instances, parse_override_entry};
pub use processor_configs::*;
pub use processors::*;
pub use provenance::{FieldProvenance, ProvenanceMap, Section, SpanMap};
pub use settings::*;
pub use validation::FieldType;
use validation::{
    expected_field_type, validate_analyzer_fields_raw, validate_build_config,
    validate_dep_auto_exist, validate_no_dot_src_dirs, validate_processor_fields_raw,
    validate_single_processor,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::errors;
use crate::registries as registry;
use variables::substitute_variables;

const CONFIG_FILE: &str = "rsconstruct.toml";
/// Optional per-repo overlay merged over `rsconstruct.toml` at load time.
/// Lets many repos share one canonical main config while keeping
/// repo-specific sections (dependencies, explicit generators, excludes)
/// in a separate local file.
pub const LOCAL_CONFIG_FILE: &str = "rsconstruct.local.toml";

/// The user-level config file, relative to `$XDG_CONFIG_HOME` (or
/// `~/.config`). It may set `[build]` keys only and sits *under* the repo's
/// files: rsconstruct.toml overrides it, rsconstruct.local.toml overrides both.
pub const USER_CONFIG_FILE: &str = "rsconstruct/config.toml";

/// Path of the user-level config file, or `None` when neither
/// `$XDG_CONFIG_HOME` nor `$HOME` is set.
#[must_use]
pub fn user_config_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    Some(base.join(USER_CONFIG_FILE))
}

/// One file rsconstruct may read settings from, as reported by
/// `rsconstruct toml files`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConfigFile {
    /// Position in the merge order, 1 = lowest; a later file overrides an
    /// earlier one key by key. `None` for files outside the merge chain.
    pub precedence: Option<usize>,
    /// Short role name: "user", "project", "local overlay", ...
    pub role: &'static str,
    /// Where the file is looked for. `None` when the location cannot be
    /// determined (the user config with neither `$XDG_CONFIG_HOME` nor
    /// `$HOME` set).
    pub path: Option<std::path::PathBuf>,
    /// Whether a regular file is there right now.
    pub exists: bool,
    /// What the file is for and any restriction on it.
    pub note: &'static str,
}

/// Every file rsconstruct may read settings from, resolved against the
/// current directory, lowest precedence first.
///
/// The first three form the merge chain (`user` < `project` < `local
/// overlay`); the rest are project files read for other purposes and never
/// merged. The list is the single place that knows these names, so a new
/// config file must be added here to be reported.
#[must_use]
pub fn config_files() -> Vec<ConfigFile> {
    let cwd = std::env::current_dir().ok();
    let in_project = |name: &str| {
        cwd.as_ref()
            .map_or_else(|| std::path::PathBuf::from(name), |c| c.join(name))
    };
    let entry = |precedence: Option<usize>,
                 role: &'static str,
                 path: Option<std::path::PathBuf>,
                 note: &'static str| {
        let exists = path.as_deref().is_some_and(Path::is_file);
        ConfigFile {
            precedence,
            role,
            path,
            exists,
            note,
        }
    };
    vec![
        entry(
            Some(1),
            "user",
            user_config_path(),
            "[build] keys only; sets per-user defaults, never read in CI",
        ),
        entry(
            Some(2),
            "project",
            Some(in_project(CONFIG_FILE)),
            "the main config; required by most commands",
        ),
        entry(
            Some(3),
            "local overlay",
            Some(in_project(LOCAL_CONFIG_FILE)),
            "merged over rsconstruct.toml when present",
        ),
        entry(
            None,
            "ignore rules",
            Some(in_project(".rsconstructignore")),
            "extra ignore patterns in gitignore syntax, honoured in any directory of the tree",
        ),
        entry(
            None,
            "tool lock",
            Some(in_project(crate::tool_lock::LOCK_FILE)),
            "pinned tool versions, verified on request during build",
        ),
    ]
}

/// Read the user-level config file, if present, as a raw TOML value.
///
/// Only a `[build]` table is accepted. A machine-wide file that could add
/// processors or change what a repo scans would make the same repo build
/// differently on two machines — and differently from CI, which never sees
/// this file. Build-policy switches are the one thing that is safe to
/// default per user, and they stay overridable per repo.
fn read_user_config() -> Result<Option<toml::Value>> {
    let Some(path) = user_config_path() else {
        return Ok(None);
    };
    if !path.is_file() {
        return Ok(None);
    }
    let content = crate::errors::ctx(
        std::fs::read_to_string(&path),
        &format!("Failed to read user config file {}", path.display()),
    )?;
    let raw: toml::Value = toml::from_str(&content).map_err(|e| {
        crate::exit_code::config_error(format!(
            "Failed to parse user config file {}: {e}",
            path.display()
        ))
    })?;
    if let Some(table) = raw.as_table() {
        let foreign: Vec<&String> = table.keys().filter(|k| k.as_str() != "build").collect();
        if !foreign.is_empty() {
            return Err(crate::exit_code::config_error(format!(
                "Invalid user config {}: only a [build] table is allowed here, found [{}] — \
                 processors, analyzers and every other section belong in the repo's rsconstruct.toml, \
                 where CI sees them too",
                path.display(),
                foreign
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join("], [")
            )));
        }
    }
    Ok(Some(raw))
}

/// Scan field names in `StandardConfig`.
/// These are automatically appended to every processor's known fields during validation.
pub const SCAN_CONFIG_FIELDS: &[&str] = &[
    "src_dirs",
    "src_extensions",
    "src_exclude_dirs",
    "src_exclude_files",
    "src_exclude_paths",
    "src_files",
];

/// Universal `StandardConfig` fields that apply to every processor.
/// Automatically appended to every processor's `known_fields` list during validation
/// and to the defconfig display table — individual processors don't need to repeat them.
pub const STANDARD_EXTRA_FIELDS: &[&str] = &["enabled", OUTPUT_DEPENDS_ON_INPUT_NAME];

/// The universal field that keys a processor's products by input path as
/// well as content (see `StandardConfig::output_depends_on_input_name`).
pub const OUTPUT_DEPENDS_ON_INPUT_NAME: &str = "output_depends_on_input_name";

pub trait KnownFields {
    /// Return the known fields for this config struct, excluding scan fields.
    fn known_fields() -> &'static [&'static str];

    /// Return only the fields that affect build output.
    /// Changes to these fields should trigger config change detection.
    /// Fields not listed here (e.g., `src_dirs`, `src_exclude_dirs`, batch, `max_jobs`)
    /// are discovery or execution parameters that don't affect what the tool produces.
    fn checksum_fields() -> &'static [&'static str];

    /// Return (`field_name`, description) pairs for processor-specific fields only.
    /// Shared scan/dep/exec field descriptions are added by the display layer.
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[]
    }
}

/// Default scan configuration for a processor, as plain data.
/// Used to resolve None scan fields in `StandardConfig` after TOML deserialization.
///
/// The shared `src_` prefix is deliberate: these field names are the TOML
/// keys users write in `rsconstruct.toml`, so renaming them to satisfy
/// `struct_field_names` would desynchronize the struct from the config
/// format it mirrors.
#[allow(clippy::struct_field_names)]
#[derive(Clone, Copy)]
pub struct ScanDefaultsData {
    /// Always `&[]` — every processor defaults to scanning nothing.
    ///
    /// No processor guesses where its files live. A default like `["src"]` or
    /// `["tests"]` is a guess that silently matches nothing in a project with
    /// a different layout, or worse, silently matches the wrong directory;
    /// `&[]` makes the processor build nothing until the user says where to
    /// look, which is visible and unambiguous. Scanning the whole tree remains
    /// available as an explicit `src_dirs = [""]`.
    ///
    /// Do not reintroduce a non-empty default here. `src_extensions` is the
    /// field that carries a processor's identity (which file types it handles);
    /// `src_dirs` is the user's to state.
    pub src_dirs: &'static [&'static str],
    pub src_extensions: &'static [&'static str],
    pub src_exclude_dirs: &'static [&'static str],
}

/// One custom config field of a processor — THE schema entry. Declared once
/// in the processor's own file (on its `ProcessorPlugin.fields`); every
/// projection derives from it: `known_fields` (name), `checksum_fields`
/// (`affects_output`), `must_fields` (`required`), `field_descriptions`
/// (`doc`), and the validator's expected type (`ty`). The four
/// hand-maintained parallel lists and the central `(processor, field)` type
/// table this replaces drifted repeatedly — a field is now declared exactly
/// once or not at all.
pub struct FieldSpec {
    pub name: &'static str,
    pub ty: FieldType,
    /// Include in `checksum_fields`: changes to this field alter the
    /// produced output bytes and must invalidate cached results.
    pub affects_output: bool,
    /// Include in `must_fields`: the processor cannot run without this
    /// field explicitly set non-empty.
    pub required: bool,
    pub doc: &'static str,
}

/// Per-processor default values applied after TOML deserialization.
#[derive(Default, Clone, Copy)]
pub struct ProcessorDefaults {
    /// Default command binary name. Empty if not applicable.
    pub command: &'static str,
    /// Default `dep_auto` entries.
    pub dep_auto: &'static [&'static str],
    /// Default output directory (generators only). Empty if not applicable.
    pub output_dir: &'static str,
    /// Default output formats (generators only). Empty if not applicable.
    pub formats: &'static [&'static str],
    /// Default args. Empty if not applicable.
    pub args: &'static [&'static str],
    /// Override for batch. None means leave the `StandardConfig` default (true).
    pub batch: Option<bool>,
    /// Default `output_depends_on_input_name`: true for a tool whose verdict
    /// depends on the file's path (per-file rules, module names).
    pub output_depends_on_input_name: bool,
}

impl ProcessorDefaults {
    /// All-empty defaults, for `..ProcessorDefaults::EMPTY` in static plugin
    /// entries (`Default::default()` is not const-evaluable there).
    pub const EMPTY: Self = Self {
        command: "",
        dep_auto: &[],
        output_dir: "",
        formats: &[],
        args: &[],
        batch: None,
        output_depends_on_input_name: false,
    };
}

/// Parameters for a simple checker processor — pure data, no macros.
#[derive(Copy, Clone)]
pub struct SimpleCheckerParams {
    /// Human-readable description
    pub description: &'static str,
    /// Optional subcommand (e.g., "check" for ruff)
    pub subcommand: Option<&'static str>,
    /// Args prepended before config args (e.g., ["--check"] for black)
    pub prepend_args: &'static [&'static str],
    /// Additional tools required beyond the command (e.g., ["python3", "node"])
    pub extra_tools: &'static [&'static str],
    /// Fix mode: subcommand to use (e.g., Some("format") for ruff).
    /// None means same command/subcommand as check but with different args.
    /// If both `fix_subcommand` and `fix_prepend_args` are unset, the
    /// processor has no fix capability.
    pub fix_subcommand: Option<&'static str>,
    /// Args prepended in fix mode (e.g., &["--write"] for prettier, &["--fix"] for eslint).
    /// Empty means no fix capability (unless `fix_subcommand` is set).
    pub fix_prepend_args: &'static [&'static str],
    /// Whether fix mode supports batch execution. Defaults follow check batch.
    pub fix_batch: Option<bool>,
}

/// Validate `dep_inputs` paths exist and return them as `PathBufs`.
/// Paths are relative to project root (which is cwd).
pub fn resolve_extra_inputs(dep_inputs: &[String]) -> Result<Vec<PathBuf>> {
    let mut resolved = Vec::new();
    for p in dep_inputs {
        if p.contains('*') || p.contains('?') || p.contains('[') {
            // Glob pattern: expand to matching files
            for entry in
                glob::glob(p).with_context(|| format!("Invalid glob pattern in dep_inputs: {p}"))?
            {
                let path =
                    crate::errors::ctx(entry, &format!("Failed to read glob entry for: {p}"))?;
                if path.is_file() {
                    resolved.push(path);
                }
            }
        } else {
            let path = PathBuf::from(p);
            if !path.exists() {
                anyhow::bail!("dep_inputs file not found: {p}");
            }
            resolved.push(path);
        }
    }
    Ok(resolved)
}

/// Descriptions for scan fields shared by every processor.
pub const SCAN_FIELD_DESCRIPTIONS: &[(&str, &str)] = &[
    ("src_dirs", "Directories to scan for source files"),
    ("src_extensions", "File extensions to match during scanning"),
    (
        "src_exclude_dirs",
        "Directory path segments to skip during scanning",
    ),
    ("src_exclude_files", "File names to exclude from scanning"),
    (
        "src_exclude_paths",
        "Relative paths to exclude from scanning",
    ),
    (
        "src_files",
        "Additional files to include alongside normal scanning",
    ),
];

/// Descriptions for execution/dependency fields shared by most processors.
pub const SHARED_FIELD_DESCRIPTIONS: &[(&str, &str)] = &[
    (
        "dep_inputs",
        "Extra files that trigger a rebuild when their content changes",
    ),
    (
        "dep_auto",
        "Config files added as dep_inputs; processor defaults are skipped when absent, entries you list must exist",
    ),
    (
        "batch",
        "Pass all matched files to the tool in a single invocation",
    ),
    (
        "max_jobs",
        "Maximum parallel jobs for this processor (overrides global --jobs)",
    ),
    (
        "enabled",
        "Set to false to disable this processor without removing the stanza",
    ),
    (
        OUTPUT_DEPENDS_ON_INPUT_NAME,
        "The result depends on input paths, not only content: a renamed or identical file is rebuilt, not restored from cache",
    ),
];

/// Compute a config hash including only the fields named in `checksum_fields`.
/// This is allowlist-based: any key not in `checksum_fields` is removed before
/// hashing. Each processor declares its own `checksum_fields()` list, which is the
/// single source of truth for which config fields trigger cache invalidation.
/// Checksum-affecting fields for a processor, derived from its plugin's
/// `FieldSpec` list — the convenience form for processor `discover()` call
/// sites feeding [`output_config_hash`]. Accepts instance names
/// ("pylint.core") and strips the suffix, so discover sites can pass their
/// `instance_name` directly. Empty for unregistered types (Lua).
pub fn checksum_fields_of(name: &str) -> Vec<&'static str> {
    ProcessorConfig::checksum_fields_for(&registry::pname_of(name)).unwrap_or_default()
}

pub fn output_config_hash(value: &impl Serialize, checksum_fields: &[&str]) -> String {
    let json_value: serde_json::Value =
        serde_json::to_value(value).expect(errors::CONFIG_SERIALIZE);
    let filtered = if let serde_json::Value::Object(map) = json_value {
        let kept: serde_json::Map<String, serde_json::Value> = map
            .into_iter()
            .filter(|(k, _)| checksum_fields.contains(&k.as_str()))
            .collect();
        serde_json::Value::Object(kept)
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
pub struct PluginsConfig {
    #[serde(default = "default_plugins_dir")]
    pub dir: String,
}

fn default_plugins_dir() -> String {
    DEFAULT_PLUGINS_DIR.into()
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            dir: DEFAULT_PLUGINS_DIR.into(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
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
    /// Present only when this repo publishes to GitHub Pages; absence means
    /// "not a Pages repo", which is why this is an Option and not a struct
    /// with defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<PagesConfig>,
    /// Field-level provenance for every top-level `[section]` (build, cache,
    /// graph, plugins, dependencies, command, completions). Each key is the
    /// section name; the inner map's keys are field names within that section.
    /// Populated by `Config::load` from a `toml_edit` walk — never present in
    /// user TOML, so skipped during (de)serialization.
    #[serde(skip)]
    pub global_provenance: HashMap<String, ProvenanceMap>,
}

pub fn default_cc_compiler() -> String {
    "gcc".into()
}

pub fn default_cxx_compiler() -> String {
    "g++".into()
}

pub fn default_output_suffix() -> String {
    ".elf".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CompletionsConfig {
    #[serde(default = "default_shells")]
    pub shells: Vec<String>,
}

fn default_shells() -> Vec<String> {
    vec!["bash".into()]
}

impl Default for CompletionsConfig {
    fn default() -> Self {
        Self {
            shells: vec!["bash".into()],
        }
    }
}

/// A single analyzer instance parsed from the TOML config.
/// `[analyzer.cpp]` produces one instance with `type_name="cpp`", `instance_name="cpp`".
/// `[analyzer.cpp.kernel]` produces one with `type_name="cpp`", `instance_name="cpp.kernel`".
#[derive(Debug, Clone)]
pub struct AnalyzerInstance {
    /// Instance name: "cpp" for single, "cpp.kernel" for named
    pub instance_name: String,
    /// Analyzer type name (must match a registered `AnalyzerPlugin` name)
    pub type_name: String,
    /// The raw TOML config for this instance
    pub config_toml: toml::Value,
    /// Source of every field in `config_toml` (user TOML, serde default, …).
    pub provenance: ProvenanceMap,
}

/// Configuration for dependency analyzers.
/// Each `[analyzer.NAME]` section in rsconstruct.toml creates an `AnalyzerInstance`.
/// No analyzers run unless explicitly declared in the config.
#[derive(Debug, Default)]
pub struct AnalyzerConfig {
    /// All declared analyzer instances
    pub instances: Vec<AnalyzerInstance>,
}

impl Serialize for AnalyzerConfig {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
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
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let table = toml::Value::deserialize(deserializer)?;
        Self::from_toml(&table).map_err(serde::de::Error::custom)
    }
}

impl AnalyzerConfig {
    /// Parse the `[analyzer]` table from TOML into instances.
    ///
    /// Supports both single-instance and multi-instance syntax:
    /// - `[analyzer.cpp]` with config fields → one instance, `instance_name="cpp`"
    /// - `[analyzer.cpp.kernel]` + `[analyzer.cpp.userspace]` → two instances,
    ///   `instance_name="cpp.kernel`" and "cpp.userspace"
    pub(crate) fn from_toml(value: &toml::Value) -> Result<Self> {
        let Some(table) = value.as_table() else {
            return Ok(Self::default());
        };

        let mut instances = Vec::new();
        for (type_name, val) in table {
            if registry::find_analyzer_plugin(type_name).is_none() {
                anyhow::bail!(
                    "Unknown analyzer '{type_name}'. Run 'rsconstruct analyzer list' to see available analyzers."
                );
            }
            let Some(sub_table) = val.as_table() else {
                anyhow::bail!("Expected [analyzer.{type_name}] to be a table");
            };
            if Self::is_multi_instance(sub_table) {
                for (name, inst_val) in sub_table {
                    let provenance = seed_user_provenance(inst_val);
                    instances.push(AnalyzerInstance {
                        instance_name: format!("{type_name}.{name}"),
                        type_name: type_name.clone(),
                        config_toml: inst_val.clone(),
                        provenance,
                    });
                }
            } else {
                let provenance = seed_user_provenance(val);
                instances.push(AnalyzerInstance {
                    instance_name: type_name.clone(),
                    type_name: type_name.clone(),
                    config_toml: val.clone(),
                    provenance,
                });
            }
        }
        Ok(Self { instances })
    }

    /// Multi-instance iff the table is non-empty and every value is itself a
    /// table. Single-instance if any value is a scalar/array (i.e. a config field).
    fn is_multi_instance(table: &toml::map::Map<String, toml::Value>) -> bool {
        !table.is_empty() && table.values().all(toml::Value::is_table)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct GraphConfig {
    #[serde(default)]
    pub viewer: Option<String>,
    /// Reject products with no input files. Default: true.
    #[serde(default = "default_true")]
    pub validate_empty_inputs: bool,
    /// Validate that dependency references point to existing products. Default: true.
    #[serde(default = "default_true")]
    pub validate_dep_references: bool,
    /// Warn when the same input file appears in multiple products of the same processor. Default: false.
    #[serde(default)]
    pub validate_duplicate_inputs: bool,
    /// Check for cycles immediately after resolving dependencies (rather than at topological sort time). Default: false.
    #[serde(default)]
    pub validate_early_cycles: bool,
}

/// Read a config file and apply `[vars]` substitution to its content.
/// Substitution is per-file: each file's `${...}` references resolve against
/// its own `[vars]` section only.
fn read_and_substitute(path: &Path) -> Result<String> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    substitute_variables(&content).map_err(|e| {
        crate::exit_code::config_error(format!(
            "Failed to substitute variables in {}: {e:#}",
            path.display()
        ))
    })
}

/// Deep-merge `overlay` into `base`: tables merge recursively, while arrays
/// and scalars from the overlay replace the base value wholesale. Keys only
/// present in the overlay are added.
fn merge_toml_values(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base_table), toml::Value::Table(overlay_table)) => {
            for (key, overlay_val) in overlay_table {
                match base_table.get_mut(&key) {
                    Some(base_val) if base_val.is_table() && overlay_val.is_table() => {
                        merge_toml_values(base_val, overlay_val);
                    }
                    _ => {
                        base_table.insert(key, overlay_val);
                    }
                }
            }
        }
        (base_val, overlay_val) => *base_val = overlay_val,
    }
}

impl Config {
    pub(crate) fn require_config() -> Result<()> {
        let config_path = Path::new(CONFIG_FILE);
        if !config_path.exists() {
            let message = if Path::new(LOCAL_CONFIG_FILE).exists() {
                format!(
                    "{LOCAL_CONFIG_FILE} found without {CONFIG_FILE} — the local overlay only extends a main config file. Run 'rsconstruct init' to create one."
                )
            } else {
                "No rsconstruct.toml found. Run 'rsconstruct init' to create one.".to_string()
            };
            return Err(crate::exit_code::RsconstructError::new(
                crate::exit_code::RsconstructExitCode::ConfigError,
                message,
            )
            .into());
        }
        Ok(())
    }

    /// Compute the directories the file-index walk must treat specially,
    /// from the loaded config:
    ///
    /// - excluded output roots: the global `[build] output_dir` plus every
    ///   instance's `output_dir`/`output`/`output_dirs` values (already
    ///   resolved — `out/` rebasing has been applied by load time). The walk
    ///   skips these so generated files are never discovered as source.
    /// - forced scan dirs: `src_dirs` entries that fall under an excluded
    ///   root. Naming such a directory in `src_dirs` is the explicit opt-in
    ///   to scan generated files, so it is walked anyway.
    ///
    /// Empty and `"."` values are dropped: they mean in-place output at the
    /// project root, which cannot be excluded from the walk.
    pub fn file_index_walk_dirs(&self) -> (Vec<String>, Vec<String>) {
        fn normalized(dir: &str) -> Option<String> {
            let dir = dir.trim_end_matches('/');
            if dir.is_empty() || dir == "." {
                return None;
            }
            Some(dir.to_string())
        }

        let mut exclude: Vec<String> = Vec::new();
        exclude.extend(normalized(&self.build.output_dir));
        for inst in &self.processor.instances {
            let Some(table) = inst.config_toml.as_table() else {
                continue;
            };
            for field in ["output_dir", "output"] {
                if let Some(v) = table.get(field).and_then(toml::Value::as_str) {
                    exclude.extend(normalized(v));
                }
            }
            if let Some(dirs) = table.get("output_dirs").and_then(toml::Value::as_array) {
                for v in dirs {
                    if let Some(s) = v.as_str() {
                        exclude.extend(normalized(s));
                    }
                }
            }
        }
        exclude.sort();
        exclude.dedup();

        let mut force: Vec<String> = Vec::new();
        for inst in &self.processor.instances {
            let Some(dirs) = inst
                .config_toml
                .as_table()
                .and_then(|t| t.get("src_dirs"))
                .and_then(toml::Value::as_array)
            else {
                continue;
            };
            for v in dirs {
                let Some(dir) = v.as_str().and_then(normalized) else {
                    continue;
                };
                if exclude.iter().any(|root| Path::new(&dir).starts_with(root)) {
                    force.push(dir);
                }
            }
        }
        force.sort();
        force.dedup();

        (exclude, force)
    }

    pub(crate) fn load() -> Result<Self> {
        let config_path = Path::new(CONFIG_FILE);
        let local_path = Path::new(LOCAL_CONFIG_FILE);

        let (mut config, span_map, global_span_map, local_span_map, local_global_span_map) =
            if config_path.exists() {
                let substituted = read_and_substitute(config_path)?;
                let repo_raw: toml::Value = toml::from_str(&substituted).map_err(|e| {
                    crate::exit_code::config_error(format!(
                        "Failed to parse config file {}: {e}",
                        config_path.display()
                    ))
                })?;
                // The user-level file sits underneath: its [build] keys are
                // defaults that the repo's own [build] overrides key by key.
                let mut raw = read_user_config()?
                    .unwrap_or_else(|| toml::Value::Table(toml::map::Map::new()));
                merge_toml_values(&mut raw, repo_raw);
                // Overlay: rsconstruct.local.toml, when present, is deep-merged
                // over the main config. Tables merge recursively; arrays and
                // scalars from the local file replace the main file's values.
                // `[vars]` substitution is per-file: each file's `${...}`
                // references resolve against its own `[vars]` section only.
                let local_substituted = if local_path.exists() {
                    let local_content = read_and_substitute(local_path)?;
                    let local_raw: toml::Value = toml::from_str(&local_content).map_err(|e| {
                        crate::exit_code::config_error(format!(
                            "Failed to parse config file {}: {e}",
                            local_path.display()
                        ))
                    })?;
                    merge_toml_values(&mut raw, local_raw);
                    Some(local_content)
                } else {
                    None
                };
                // Run both schema validators before serde sees the config, so users
                // get pretty per-section errors instead of serde's raw messages.
                // Errors from both validators are surfaced together under a single
                // "Invalid config:" header.
                let mut all_errors = validate_processor_fields_raw(&raw);
                all_errors.extend(validate_analyzer_fields_raw(&raw));
                if !all_errors.is_empty() {
                    return Err(crate::exit_code::config_error(format!(
                        "Invalid config:\n{}",
                        all_errors.join("\n")
                    )));
                }
                let config: Self = raw.try_into().map_err(|e| {
                    crate::exit_code::config_error(format!(
                        "Failed to parse config file {}: {e}",
                        config_path.display()
                    ))
                })?;
                validate_build_config(&config.build)?;
                // Capture byte-level spans from the substituted sources so we can
                // report user-set fields as `rsconstruct.toml:<line>` (or
                // `rsconstruct.local.toml:<line>`) instead of the sentinel
                // `line: 0` we seeded during deserialization.
                let (spans, global_spans) = provenance::build_span_maps(&substituted);
                let (local_spans, local_global_spans) = match &local_substituted {
                    Some(content) => provenance::build_span_maps(content),
                    None => (SpanMap::new(), provenance::GlobalSpanMap::new()),
                };
                (config, spans, global_spans, local_spans, local_global_spans)
            } else {
                if local_path.exists() {
                    anyhow::bail!(
                        "{LOCAL_CONFIG_FILE} found without {CONFIG_FILE} — the local overlay only extends a main config file",
                    );
                }
                let config = match read_user_config()? {
                    Some(raw) => raw.try_into().map_err(|e| {
                        crate::exit_code::config_error(format!(
                            "Failed to parse user config file: {e}"
                        ))
                    })?,
                    None => Self::default(),
                };
                validate_build_config(&config.build)?;
                (
                    config,
                    SpanMap::new(),
                    provenance::GlobalSpanMap::new(),
                    SpanMap::new(),
                    provenance::GlobalSpanMap::new(),
                )
            };
        config.processor.resolve_scan_defaults();
        config
            .processor
            .apply_output_dir_defaults(&config.build.output_dir);
        config.apply_span_map(&span_map, &local_span_map);
        validate_dep_auto_exist(&config.processor.instances, &config.build)?;
        validate_no_dot_src_dirs(&config.processor.instances, &config.build)?;
        config.populate_global_provenance(&global_span_map, &local_global_span_map)?;
        crate::phases::run_post_config_hooks(&mut config)?;
        Ok(config)
    }

    /// Apply CLI-level config overrides from `--iset` and `--pset` flags.
    ///
    /// `iset` entries match a single processor instance by `iname` (the section
    /// name in `[processor.<iname>]`). `pset` entries match every instance whose
    /// processor type (`pname`) equals the prefix.
    ///
    /// Each entry has the form `<name>.<field>=<value>`. The value is parsed as
    /// a TOML scalar/array/table; if it fails to parse, it is treated as a bare
    /// string. The resolved value's TOML type must match the field's declared
    /// type (e.g. `max_jobs` must be an integer).
    ///
    /// Errors hard on: malformed entry, unknown iname/pname, no matching
    /// instances for a pname, unknown field for the processor type, and
    /// type mismatch.
    pub(crate) fn apply_overrides(&mut self, iset: &[String], pset: &[String]) -> Result<()> {
        for raw in iset {
            let (iname, field, value) = parse_override_entry(raw, "--iset")?;
            apply_override_to_instances(
                &mut self.processor.instances,
                field,
                &value,
                |inst| inst.instance_name == iname,
                "iname",
                iname,
            )?;
        }
        for raw in pset {
            let (pname, field, value) = parse_override_entry(raw, "--pset")?;
            apply_override_to_instances(
                &mut self.processor.instances,
                field,
                &value,
                |inst| inst.pname == pname,
                "pname",
                pname,
            )?;
        }

        // Overrides bypass load-time validation (it runs on the raw TOML,
        // before overrides exist), so the semantic rules — the max_jobs > 0
        // deadlock guard, must_fields, the src_dirs rule — must be re-checked
        // against the effective config. Otherwise `--iset x.max_jobs=0`
        // hangs the build forever.
        let mut errors = Vec::new();
        for inst in &self.processor.instances {
            if let Some(table) = inst.config_toml.as_table() {
                let section_label = format!("processor.{}", inst.instance_name);
                validate_single_processor(&inst.pname, &section_label, table, &mut errors);
            }
        }
        if !errors.is_empty() {
            return Err(crate::exit_code::config_error(format!(
                "Invalid config after CLI overrides:\n{}",
                errors.join("\n")
            )));
        }
        validate_dep_auto_exist(&self.processor.instances, &self.build)?;
        validate_no_dot_src_dirs(&self.processor.instances, &self.build)?;
        Ok(())
    }

    /// Seed `global_provenance` for every top-level section. Every field
    /// listed in the span map is `UserToml { line }`; every other field of
    /// the section — discovered by serializing the section struct and reading
    /// its keys — is `SerdeDefault`.
    fn populate_global_provenance(
        &mut self,
        global_spans: &provenance::GlobalSpanMap,
        local_global_spans: &provenance::GlobalSpanMap,
    ) -> Result<()> {
        // Serialize the whole config once so we can walk each section's
        // effective keys without reaching into every section struct.
        let serialized = toml::Value::try_from(&*self)
            .context("Failed to serialize config for global provenance walk")?;
        let Some(root) = serialized.as_table() else {
            return Ok(());
        };
        for (section_name, section_value) in root {
            // Skip processor/analyzer — those are tracked per-instance in the
            // instances' own provenance maps.
            if section_name == "processor" || section_name == "analyzer" {
                continue;
            }
            let Some(section_table) = section_value.as_table() else {
                continue;
            };
            let user_fields = global_spans.get(section_name);
            let local_fields = local_global_spans.get(section_name);
            let mut map = ProvenanceMap::new();
            for field in section_table.keys() {
                // The local overlay wins the merge, so a field set in both
                // files is attributed to rsconstruct.local.toml.
                let source = if let Some(&line) = local_fields.and_then(|f| f.get(field)) {
                    FieldProvenance::LocalToml { line }
                } else if let Some(&line) = user_fields.and_then(|f| f.get(field)) {
                    FieldProvenance::UserToml { line }
                } else {
                    FieldProvenance::SerdeDefault
                };
                map.insert(field.clone(), source);
            }
            self.global_provenance.insert(section_name.clone(), map);
        }
        Ok(())
    }

    /// Replace sentinel `UserToml { line: 0 }` entries with real line numbers
    /// from the `toml_edit` pass. Any user-set field that didn't get a span
    /// stays at line 0 (fine — the `config show` formatter falls back to
    /// "from rsconstruct.toml" without a line number).
    fn apply_span_map(&mut self, spans: &SpanMap, local_spans: &SpanMap) {
        for inst in &mut self.processor.instances {
            apply_spans_to_instance(
                &mut inst.provenance,
                spans,
                local_spans,
                Section::Processor,
                &inst.instance_name,
            );
        }
        for inst in &mut self.analyzer.instances {
            apply_spans_to_instance(
                &mut inst.provenance,
                spans,
                local_spans,
                Section::Analyzer,
                &inst.instance_name,
            );
        }
    }
}

fn apply_spans_to_instance(
    provenance: &mut ProvenanceMap,
    spans: &SpanMap,
    local_spans: &SpanMap,
    section: Section,
    instance_name: &str,
) {
    let keys: Vec<String> = provenance.keys().cloned().collect();
    for key in keys {
        if let Some(FieldProvenance::UserToml { line }) = provenance.get(&key) {
            if *line != 0 {
                continue; // already enriched
            }
            let span_key = (section, instance_name.to_string(), key.clone());
            // The local overlay wins the merge, so a field set in both files
            // is attributed to rsconstruct.local.toml.
            if let Some(&real_line) = local_spans.get(&span_key) {
                provenance.insert(key, FieldProvenance::LocalToml { line: real_line });
            } else if let Some(&real_line) = spans.get(&span_key) {
                provenance.insert(key, FieldProvenance::UserToml { line: real_line });
            }
        }
    }
}

/// Extract a `StandardConfig` with scan fields from a dynamic TOML table (used by Lua plugins).
/// Falls back to the given defaults for any missing scan fields.
pub fn standard_config_from_toml(
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
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
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
        cfg.src_dirs = Some(
            default_src_dirs
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        );
    }
    if cfg.src_extensions.is_none() {
        cfg.src_extensions = Some(
            default_src_extensions
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        );
    }
    if cfg.src_exclude_dirs.is_none() {
        cfg.src_exclude_dirs = Some(
            default_exclude_dirs
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        );
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

/// Post-config hook: when `CI=true` is in the environment, enable the
/// `eatmydata` wrap for `tool install` / `tool install-deps`. The
/// trade-off (loss-on-power-cut for a 3-10× speedup) is right for
/// transient CI hosts and wrong for developer workstations, so we tie
/// the policy to `CI=true`. Users who want a different policy adjust
/// the env var.
#[allow(clippy::unnecessary_wraps)] // Result<()> required by PhaseHook::run signature.
fn eatmydata_ci_default(config: &mut Config) -> anyhow::Result<()> {
    if running_in_ci() {
        config.dependencies.eatmydata = true;
    }
    Ok(())
}

/// The `CI=true` policy predicate behind [`eatmydata_ci_default`], shared with
/// the config-independent `tool install --all` path (which has no `Config` to
/// run the hook against).
pub fn running_in_ci() -> bool {
    std::env::var("CI").is_ok_and(|v| v == "true")
}

inventory::submit! { crate::phases::PhaseHook {
    name: "eatmydata_ci_default",
    description: "When CI=true, enable eatmydata wrapping for apt/dnf/pacman installs",
    function: concat!(module_path!(), "::eatmydata_ci_default"),
    location: concat!(file!(), ":", line!()),
    run: eatmydata_ci_default,
} }

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
pub use provenance::{FieldProvenance, ProvenanceMap, Section, SourceFile, SourceFiles, SpanMap};
pub use settings::*;
pub use validation::FieldType;
use validation::{
    describe_issues, expected_field_type, validate_analyzer_fields_raw, validate_build_config,
    validate_cache_config, validate_dep_auto_exist, validate_no_dot_src_dirs,
    validate_processor_fields_raw, validate_single_processor,
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

pub const CONFIG_FILE: &str = "rsconstruct.toml";

/// The user-level config file, relative to `$XDG_CONFIG_HOME` (or
/// `~/.config`). It may set `[build]` keys, which sit *under* the repo's
/// rsconstruct.toml (which overrides it key by key), and `[tables]`, which
/// chooses the columns the CLI's tables show and is read by nothing else.
pub const USER_CONFIG_FILE: &str = "rsconstruct/config.toml";

/// The top-level tables the user config may contain.
const USER_CONFIG_SECTIONS: &[&str] = &["build", "tables"];

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
    /// Short role name: "user", "project", "tool lock", ...
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
/// The first two form the merge chain (`user` < `project`); the rest are
/// project files read for other purposes and never merged. The list is the single place that knows these names, so a new
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
            "[build] and [tables] only; per-user defaults and table columns, never read in CI",
        ),
        entry(
            Some(2),
            "project",
            Some(in_project(CONFIG_FILE)),
            "the main config; required by most commands",
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
            "pinned tool versions: written by `tool lock`, installed from, enforced in CI",
        ),
    ]
}

/// Read the user-level config file, if present, as a raw TOML value.
///
/// Only `[build]` and `[tables]` are accepted. A machine-wide file that
/// could add processors or change what a repo scans would make the same
/// repo build differently on two machines — and differently from CI, which
/// never sees this file. Build-policy switches are the one thing that is
/// safe to default per user, and they stay overridable per repo; `[tables]`
/// only changes which columns the CLI's tables print.
///
/// Returns the parsed file and its text, which is what an error found
/// after the merge is located in.
fn read_user_config() -> Result<Option<(toml::Value, String)>> {
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
        let foreign: Vec<&String> = table
            .keys()
            .filter(|k| !USER_CONFIG_SECTIONS.contains(&k.as_str()))
            .collect();
        if !foreign.is_empty() {
            return Err(crate::exit_code::config_error(format!(
                "Invalid user config {}: only a [build] table and a [tables] table are allowed here, found [{}] — \
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
    Ok(Some((raw, content)))
}

/// The `[build]` table of one config file, whatever else the file holds.
#[derive(Deserialize)]
struct BuildSection {
    /// Deserialized for its errors only, never read.
    #[serde(default, rename = "build")]
    _build: BuildConfig,
}

/// The parse error of the first of `files` that does not deserialize as
/// `T` on its own. Every file is checked by itself, not only the merged
/// value, for two reasons: a mistake in one file that a later file
/// overrides is gone from the merged value and would go unreported until
/// the override is removed; and an error found in the merged value has no
/// position, while the file's own error has line and column and shows the
/// line.
fn parse_error_in<'a, T: serde::de::DeserializeOwned>(
    files: impl Iterator<Item = &'a SourceFile>,
) -> Option<String> {
    files.into_iter().find_map(|file| {
        toml::from_str::<T>(&file.content)
            .err()
            .map(|e| format!("Failed to parse config file {}: {e}", file.name))
    })
}

/// Which files a merged config came from, for an error that cannot be
/// pinned on one of them.
fn merged_from(sources: &SourceFiles) -> String {
    // Lowest precedence first, the order the files are merged in.
    let names: Vec<&str> = sources
        .files()
        .iter()
        .rev()
        .map(|f| f.name.as_str())
        .collect();
    match names.as_slice() {
        [one] => format!("config file {one}"),
        _ => format!(
            "config (merged from {}; the error is in one of them)",
            names.join(", ")
        ),
    }
}

/// Record the merged config's `[build] python` before the rest of the
/// config is deserialized: processor defaults (a `python_command`
/// processor's `command`, sphinx's `python`) are filled in during that
/// deserialization and must see the configured interpreter.
///
/// This is the first place a mistyped `[build]` key is found, and the key
/// may be in any of the merged files.
fn set_python_interpreter(raw: &toml::Value, sources: &SourceFiles) -> Result<()> {
    let build: BuildConfig = raw
        .get("build")
        .cloned()
        .unwrap_or_else(|| toml::Value::Table(toml::map::Map::new()))
        .try_into()
        .map_err(|e| {
            crate::exit_code::config_error(format!(
                "Failed to parse [build] of {}: {e}",
                merged_from(sources)
            ))
        })?;
    crate::python_env::set_interpreter(&build.python);
    Ok(())
}

/// The user config's `[build]` part, for merging under the repo's files,
/// and the file as a source to locate errors in. `[tables]` is presentation
/// only and is not part of `Config`, so it is dropped from the value here;
/// `user_tables_config` reads it.
fn read_user_build_config() -> Result<Option<(toml::Value, SourceFile)>> {
    let Some((mut raw, content)) = read_user_config()? else {
        return Ok(None);
    };
    if let Some(table) = raw.as_table_mut() {
        table.remove("tables");
    }
    let name = user_config_path().map_or_else(String::new, |p| p.display().to_string());
    Ok(Some((
        raw,
        SourceFile {
            name,
            content,
            build_only: true,
        },
    )))
}

/// The `[tables]` section of the user config: table name to the columns it
/// shows, validated against the table definitions. Empty when the file or
/// the section is absent. A key that names no table, a field that is not a
/// column of its table, or a value that is not a list of strings is a
/// config error, so a typo shows up on the next command instead of quietly
/// leaving the default columns in place.
pub fn user_tables_config() -> Result<std::collections::BTreeMap<String, Vec<String>>> {
    let mut config = std::collections::BTreeMap::new();
    let Some((raw, _)) = read_user_config()? else {
        return Ok(config);
    };
    let Some(tables) = raw.get("tables") else {
        return Ok(config);
    };
    let path = user_config_path().map_or_else(String::new, |p| p.display().to_string());
    let Some(tables) = tables.as_table() else {
        return Err(crate::exit_code::config_error(format!(
            "Invalid user config {path}: [tables] must be a table of `<table name> = [\"Field\", ...]` entries"
        )));
    };
    let mut errors = Vec::new();
    for (name, value) in tables {
        let fields: Option<Vec<String>> = value.as_array().and_then(|items| {
            items
                .iter()
                .map(|v| v.as_str().map(str::to_string))
                .collect()
        });
        match fields {
            Some(fields) => {
                config.insert(name.clone(), fields);
            }
            None => errors.push(format!(
                "[tables] {name}: the value must be a list of field names, like [\"Name\", \"Type\"]"
            )),
        }
    }
    errors.extend(crate::tables::validate_config(&config));
    if !errors.is_empty() {
        return Err(crate::exit_code::config_error(format!(
            "Invalid user config {path}:\n{}\nRun a command with --fields to see a table's name and fields.",
            errors.join("\n")
        )));
    }
    Ok(config)
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
pub const STANDARD_EXTRA_FIELDS: &[&str] = &[
    "enabled",
    ENABLED_IF_DIR_EXISTS,
    OUTPUT_DEPENDS_ON_INPUT_NAME,
    HASH_PYTHON_ENV,
    EXECUTION_MODEL,
    SANDBOX_NETWORK,
    SANDBOX_WRITABLE,
];

/// The universal field that turns a processor off on a machine where a
/// directory is missing (see `StandardConfig::enabled_if_dir_exists`).
pub const ENABLED_IF_DIR_EXISTS: &str = "enabled_if_dir_exists";

/// The universal field naming state directories a sandboxed processor may
/// write beyond those its tools declare (see `StandardConfig::sandbox_writable`).
pub const SANDBOX_WRITABLE: &str = "sandbox_writable";

/// The universal field overriding `[build] execution_model` for one
/// processor (see `StandardConfig::execution_model`).
pub const EXECUTION_MODEL: &str = "execution_model";

/// The universal field letting a sandboxed processor reach the network
/// (see `StandardConfig::sandbox_network`).
pub const SANDBOX_NETWORK: &str = "sandbox_network";

/// The universal field that keys a processor's products by input path as
/// well as content (see `StandardConfig::output_depends_on_input_name`).
pub const OUTPUT_DEPENDS_ON_INPUT_NAME: &str = "output_depends_on_input_name";

/// The universal field that keys a processor's products by the project's
/// installed Python packages (see `StandardConfig::hash_python_env`).
pub const HASH_PYTHON_ENV: &str = "hash_python_env";

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
/// There is no default for `src_dirs`: every processor defaults to scanning
/// nothing, and `apply_scan_defaults` writes the empty list itself.
///
/// No processor guesses where its files live. A default like `["src"]` or
/// `["tests"]` is a guess that silently matches nothing in a project with
/// a different layout, or worse, silently matches the wrong directory;
/// `[]` makes the processor build nothing until the user says where to
/// look, which is visible and unambiguous. Scanning the whole tree remains
/// available as an explicit `src_dirs = [""]`.
///
/// Do not reintroduce a non-empty default here. `src_extensions` is the
/// field that carries a processor's identity (which file types it handles);
/// `src_dirs` is the user's to state.
#[derive(Clone, Copy)]
pub struct ScanDefaultsData {
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
    /// Default `hash_python_env`: true for a tool whose result depends on
    /// the project's Python environment but which detection cannot see
    /// running it (ruff and pyrefly are native binaries), and for the
    /// Python checkers generally, so the behavior is stated rather than
    /// detected. False leaves the field unset, i.e. detected.
    pub hash_python_env: bool,
    /// The default `command` is the Python interpreter (`[build] python`)
    /// rather than `command`, which must then be empty: for processors that
    /// run `python -m <module>` or a Python program on the interpreter.
    pub python_command: bool,
    /// Default `sandbox_network`: true for a builtin processor that reaches
    /// the network from rsconstruct's own code (iyamlschema fetches JSON
    /// schemas), which has no tool to declare it (`ToolInfo::needs_network`).
    pub sandbox_network: bool,
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
        hash_python_env: false,
        python_command: false,
        sandbox_network: false,
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
    /// Additional tools required beyond the command
    /// (e.g. `[ExtraTool::Python]`, `[ExtraTool::Named("node")]`)
    pub extra_tools: &'static [ExtraTool],
}

/// A tool a simple checker requires besides its command.
#[derive(Copy, Clone, Debug)]
pub enum ExtraTool {
    /// A tool with a fixed name.
    Named(&'static str),
    /// The Python interpreter, as named by `[build] python`.
    Python,
}

impl ExtraTool {
    /// The executable name to require.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            Self::Named(name) => name.to_string(),
            Self::Python => crate::python_env::interpreter(),
        }
    }
}

/// The `dep_inputs` as `PathBufs`, globs expanded to the files they match.
/// Paths are relative to project root (which is cwd).
///
/// A literal entry is returned whether or not it is on disk: it may name a
/// file an upstream processor only produces during the build (a compiled
/// dictionary a spellcheck reads), and the product must carry it as an input
/// for the two to be ordered. Whether such an entry is backed by anything is
/// checked once discovery has settled (`Builder::discover_products`), which
/// fails with a "`dep_inputs` file not found" error for one that is neither on disk
/// nor a declared output.
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
            resolved.push(PathBuf::from(p));
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
        ENABLED_IF_DIR_EXISTS,
        "Run only on a machine where this directory exists (~/ expands to home); elsewhere the processor is off, as with enabled = false",
    ),
    (
        OUTPUT_DEPENDS_ON_INPUT_NAME,
        "The result depends on input paths, not only content: a renamed or identical file is rebuilt, not restored from cache",
    ),
    (
        HASH_PYTHON_ENV,
        "The result depends on the packages installed in ./.venv: upgrading them rebuilds. Unset means detect (the tool runs the venv's Python)",
    ),
    (
        EXECUTION_MODEL,
        "\"native\" or \"sandbox\": overrides [build] execution_model for this processor. Unset follows the build-wide setting",
    ),
    (
        SANDBOX_NETWORK,
        "Under the sandbox execution model, let this processor's tools reach the network",
    ),
    (
        SANDBOX_WRITABLE,
        "Under the sandbox execution model, state directories this processor may write beyond those its tools declare (a path, or **/NAME for a directory named NAME anywhere)",
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
/// `instance_name` directly. Empty for a name that is not registered.
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
    /// The files this config was merged from, kept so that a mistake found
    /// after loading (a stanza whose processor cannot be built) can still
    /// be reported at its file and line.
    #[serde(skip)]
    pub sources: SourceFiles,
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

#[derive(Debug, Deserialize, Serialize, Clone)]
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

/// Hand-written so an absent `[graph]` table gets the same defaults as an
/// empty one. `#[derive(Default)]` gave `false` for every bool, so the two
/// "default: true" checks only ran for projects that happened to write a
/// `[graph]` header — a product with no inputs then reached the executor
/// and panicked instead of failing validation.
impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            viewer: None,
            validate_empty_inputs: true,
            validate_dep_references: true,
            validate_duplicate_inputs: false,
            validate_early_cycles: false,
        }
    }
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
            return Err(crate::exit_code::config_error(
                "No rsconstruct.toml found. Run 'rsconstruct init' to create one.",
            ));
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

        let (mut config, span_map, global_span_map) = if config_path.exists() {
            let substituted = read_and_substitute(config_path)?;
            let repo_raw: toml::Value = toml::from_str(&substituted).map_err(|e| {
                crate::exit_code::config_error(format!(
                    "Failed to parse config file {}: {e}",
                    config_path.display()
                ))
            })?;
            // The user-level file sits underneath: its [build] keys are
            // defaults that the repo's own [build] overrides key by key.
            // Every file merged into `raw` is kept as a source: a
            // mistake is found in the merged value, which has no
            // positions, and is located in these afterwards.
            let mut files: Vec<SourceFile> = Vec::new();
            let mut raw = match read_user_build_config()? {
                Some((user_raw, user_file)) => {
                    files.push(user_file);
                    user_raw
                }
                None => toml::Value::Table(toml::map::Map::new()),
            };
            merge_toml_values(&mut raw, repo_raw);
            files.push(SourceFile {
                name: config_path.display().to_string(),
                content: substituted.clone(),
                build_only: false,
            });
            // Highest precedence first: the first file that has a key is
            // the one whose value won the merge.
            files.reverse();
            let sources = SourceFiles::new(files);
            // Run both schema validators before serde sees the config, so users
            // get pretty per-section errors instead of serde's raw messages.
            // Errors from both validators are surfaced together under a single
            // "Invalid config:" header.
            // `[build]` of every file first: the interpreter below is
            // read from it, and the user config holds nothing else.
            if let Some(message) = parse_error_in::<BuildSection>(sources.files().iter()) {
                return Err(crate::exit_code::config_error(message));
            }
            set_python_interpreter(&raw, &sources)?;
            let mut all_errors = validate_processor_fields_raw(&raw);
            all_errors.extend(validate_analyzer_fields_raw(&raw));
            if !all_errors.is_empty() {
                return Err(crate::exit_code::config_error(format!(
                    "Invalid config:\n{}",
                    describe_issues(&all_errors, &sources)
                )));
            }
            // Then the rest of the project file. The user config holds
            // only `[build]`, checked above; with its `[tables]` it is
            // not a `Config`.
            let project_files = sources.files().iter().filter(|f| !f.build_only);
            if let Some(message) = parse_error_in::<Self>(project_files) {
                return Err(crate::exit_code::config_error(message));
            }
            let mut config: Self = raw.try_into().map_err(|e| {
                crate::exit_code::config_error(format!(
                    "Failed to parse {}: {e}",
                    merged_from(&sources)
                ))
            })?;
            validate_build_config(&config.build).map_err(|i| i.into_error(&sources))?;
            validate_cache_config(&config.cache).map_err(|i| i.into_error(&sources))?;
            config.sources = sources;
            // Capture byte-level spans from the substituted source so we can
            // report user-set fields as `rsconstruct.toml:<line>` instead
            // of the sentinel `line: 0` we seeded during deserialization.
            let (spans, global_spans) = provenance::build_span_maps(&substituted);
            (config, spans, global_spans)
        } else {
            let config: Self = match read_user_build_config()? {
                Some((raw, user_file)) => {
                    let sources = SourceFiles::new(vec![user_file]);
                    let mut config: Self = raw.try_into().map_err(|e| {
                        crate::exit_code::config_error(
                            parse_error_in::<BuildSection>(sources.files().iter()).unwrap_or_else(
                                || format!("Failed to parse {}: {e}", merged_from(&sources)),
                            ),
                        )
                    })?;
                    config.sources = sources;
                    config
                }
                None => Self::default(),
            };
            // No processor sections here, so nothing was deserialized
            // against the interpreter yet; recording it now suffices.
            crate::python_env::set_interpreter(&config.build.python);
            validate_build_config(&config.build).map_err(|i| i.into_error(&config.sources))?;
            (config, SpanMap::new(), provenance::GlobalSpanMap::new())
        };
        config.processor.resolve_scan_defaults();
        config
            .processor
            .apply_output_dir_defaults(&config.build.output_dir);
        config.apply_span_map(&span_map);
        config.apply_dir_conditions();
        validate_dep_auto_exist(&config.processor.instances, &config.build)?;
        validate_no_dot_src_dirs(&config.processor.instances, &config.build)?;
        config.populate_global_provenance(&global_span_map)?;
        crate::phases::run_post_config_hooks(&mut config);
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
                // instance_name is already the full section name
                // (`processor.checker.ruff`, `processor.checker.ascii.one`).
                // The section path is left empty: a value that is wrong
                // after an override is wrong on the command line, and
                // pointing at the file would send the user to a line that
                // is fine.
                validate_single_processor(
                    &inst.pname,
                    &inst.instance_name,
                    &[],
                    table,
                    &mut errors,
                );
            }
        }
        if !errors.is_empty() {
            let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
            return Err(crate::exit_code::config_error(format!(
                "Invalid config after CLI overrides:\n{}",
                messages.join("\n")
            )));
        }
        // An override may have set `enabled = true` or a new
        // `enabled_if_dir_exists`; the condition still has the last word.
        self.apply_dir_conditions();
        validate_dep_auto_exist(&self.processor.instances, &self.build)?;
        validate_no_dot_src_dirs(&self.processor.instances, &self.build)?;
        Ok(())
    }

    /// Turn off every enabled instance whose `enabled_if_dir_exists` names a
    /// directory that does not exist on this machine. The instance then
    /// behaves exactly as with `enabled = false` everywhere `enabled` is
    /// read, and the provenance of `enabled` records why, which is how
    /// [`Self::dir_condition_skips`] finds it for the build and `processor
    /// used` to report.
    pub(crate) fn apply_dir_conditions(&mut self) {
        for inst in &mut self.processor.instances {
            let Some(table) = inst.config_toml.as_table_mut() else {
                continue;
            };
            let Some(dir) = table
                .get(ENABLED_IF_DIR_EXISTS)
                .and_then(toml::Value::as_str)
                .map(str::to_string)
            else {
                continue;
            };
            if table.get("enabled").and_then(toml::Value::as_bool) == Some(false) {
                continue;
            }
            if Path::new(&crate::platform::expand_tilde(&dir)).is_dir() {
                continue;
            }
            table.insert("enabled".to_string(), toml::Value::Boolean(false));
            inst.provenance
                .insert("enabled".to_string(), FieldProvenance::DirMissing { dir });
        }
    }

    /// The instances `apply_dir_conditions` turned off, with the directory
    /// each one is waiting for, sorted by instance name.
    pub(crate) fn dir_condition_skips(&self) -> Vec<(String, String)> {
        let mut skips: Vec<(String, String)> = self
            .processor
            .instances
            .iter()
            .filter_map(|inst| match inst.provenance.get("enabled") {
                Some(FieldProvenance::DirMissing { dir }) => {
                    Some((inst.instance_name.clone(), dir.clone()))
                }
                _ => None,
            })
            .collect();
        skips.sort();
        skips
    }

    /// Seed `global_provenance` for every top-level section. Every field
    /// listed in the span map is `UserToml { line }`; every other field of
    /// the section — discovered by serializing the section struct and reading
    /// its keys — is `SerdeDefault`.
    fn populate_global_provenance(
        &mut self,
        global_spans: &provenance::GlobalSpanMap,
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
            let mut map = ProvenanceMap::new();
            for field in section_table.keys() {
                let source = if let Some(&line) = user_fields.and_then(|f| f.get(field)) {
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
    fn apply_span_map(&mut self, spans: &SpanMap) {
        for inst in &mut self.processor.instances {
            apply_spans_to_instance(
                &mut inst.provenance,
                spans,
                Section::Processor,
                &inst.instance_name,
            );
        }
        for inst in &mut self.analyzer.instances {
            apply_spans_to_instance(
                &mut inst.provenance,
                spans,
                Section::Analyzer,
                &inst.instance_name,
            );
        }
    }
}

fn apply_spans_to_instance(
    provenance: &mut ProvenanceMap,
    spans: &SpanMap,
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
            if let Some(&real_line) = spans.get(&span_key) {
                provenance.insert(key, FieldProvenance::UserToml { line: real_line });
            }
        }
    }
}

/// Post-config hook: when `CI=true` is in the environment, enable the
/// `eatmydata` wrap for `tool install machine`. The
/// trade-off (loss-on-power-cut for a 3-10× speedup) is right for
/// transient CI hosts and wrong for developer workstations, so we tie
/// the policy to `CI=true`. Users who want a different policy adjust
/// the env var.
fn eatmydata_ci_default(config: &mut Config) {
    if running_in_ci() {
        config.dependencies.eatmydata = true;
    }
}

/// The `CI=true` policy predicate behind [`eatmydata_ci_default`], shared with
/// the config-independent `tool install machine --all` path (which has no `Config` to
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

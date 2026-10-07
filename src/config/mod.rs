mod analyzer_configs;
mod dependencies;
mod processor_configs;
mod provenance;
#[cfg(test)]
mod tests;
mod validation;
mod variables;

pub use analyzer_configs::*;
pub use dependencies::*;
pub use processor_configs::*;
pub use provenance::{FieldProvenance, ProvenanceMap, Section, SpanMap};
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
pub const STANDARD_EXTRA_FIELDS: &[&str] = &["enabled"];

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

/// Configuration for the `symlink-install` command.
/// `sources[i]` is symlinked to `targets[i]`. Both arrays must be the same length.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct SymlinkInstallConfig {
    /// Source folders containing files to symlink
    #[serde(default)]
    pub sources: Vec<String>,
    /// Target folders where symlinks are created (same length as sources)
    #[serde(default)]
    pub targets: Vec<String>,
}

/// Configuration for publishing to GitHub Pages. Declaring this section marks
/// the repo as a Pages site; CI asks via `rsconstruct page dir` and only
/// then uploads/deploys, so one workflow file serves Pages and non-Pages
/// repos alike.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct PagesConfig {
    /// Directory whose contents are published (e.g. "out/web" or "_site")
    pub dir: String,
}

/// Configuration for custom commands.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct CommandsConfig {
    #[serde(default)]
    pub symlink_install: SymlinkInstallConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    /// Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)
    #[serde(default = "default_parallel")]
    pub parallel: usize,
    /// Maximum files per batch for batch-capable processors.
    /// 0 = no limit (all files in one batch). To disable batching use
    /// `--batch-size -1` on the CLI or `batch = false` per processor.
    #[serde(default)]
    pub batch_size: usize,
    /// Global output directory prefix (default: "out").
    /// Processor `output_dir` fields that start with "out/" will use this as the base instead.
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    /// Maximum total argument length (in bytes) before splitting a checker
    /// invocation into multiple invocations. Linux's `ARG_MAX` is typically
    /// ~2MB; the default leaves headroom for env vars and the tool path.
    #[serde(default = "default_max_arg_len")]
    pub max_arg_len: usize,
    /// Maximum fixed-point discovery passes. Discovery repeats while
    /// processors keep adding products from each other's declared outputs;
    /// a config still adding products at the cap fails the build instead of
    /// silently truncating the graph.
    #[serde(default = "default_max_discovery_passes")]
    pub max_discovery_passes: usize,
    /// When true (the default), the versions of the external tools a
    /// processor invokes are mixed into its products' cache keys, so
    /// upgrading a tool invalidates everything that tool produced.
    ///
    /// This used to be opt-in via `rsconstruct tool lock`: without a lock
    /// file no version was mixed in at all, and a tool upgrade silently left
    /// every cached PASS valid. Versions are now queried live (cached per
    /// build in `.rsconstruct/toolver.redb`, keyed by the tool's path, size
    /// and mtime, so the `--version` subprocesses are paid once per upgrade
    /// rather than once per build).
    ///
    /// Set to false for a build that must not shell out to `--version` at
    /// all; cache keys then ignore tool identity, as they did before.
    #[serde(default = "default_hash_tool_versions")]
    pub hash_tool_versions: bool,
    /// When true, every symlink encountered during the file-index walk is
    /// reported as a warning. The walker never follows symlinks, so a
    /// symlinked source (or a symlinked directory of sources) is silently
    /// absent from the index — never checked, never built. Turn this on to
    /// make that gap visible; it is off by default because projects that
    /// deliberately keep symlinks around (vendored trees, `node_modules`,
    /// dotfile farms) drown in the warnings.
    #[serde(default = "default_warn_symlinks")]
    pub warn_symlinks: bool,
    /// When true, a `dep_auto` entry the user listed that names a missing
    /// file is skipped, as every `dep_auto` entry used to be. Off by default:
    /// an entry written into the config is a claim about the project, and a
    /// missing file there is a typo or a stale copy of a shared config —
    /// either way a dependency that silently tracks nothing, so the build
    /// fails at config load naming the entry. Processor defaults
    /// (`.pylintrc`, `ruff.toml`, ...) are optional by nature and are never
    /// checked, whatever this is set to.
    #[serde(default = "default_allow_missing_dep_auto")]
    pub allow_missing_dep_auto: bool,
    /// When true, a `src_dirs` entry naming a directory that does not exist
    /// is skipped, as every entry used to be. Off by default: an entry in
    /// the config is a claim about where the project keeps its sources, and
    /// a directory that is not there is a typo, a stanza left behind when
    /// the directory moved, or a copy of another repo's config — in every
    /// case a processor that checks nothing while the build stays green, so
    /// discovery fails naming the processor and the entry. A directory an
    /// upstream processor declares as its output is never reported.
    #[serde(default = "default_allow_missing_src_dirs")]
    pub allow_missing_src_dirs: bool,
    /// When true, a `src_dirs` entry of `"."` is a config error. `"."` is
    /// the same as `""`: the whole project, recursively. Written as `"."`
    /// it reads like "the root directory" and hides that the stanza sweeps
    /// every subdirectory too, so a project that wants its stanzas to name
    /// the directories they cover can forbid the spelling outright. Off by
    /// default: `"."` is not wrong, only easy to misread.
    #[serde(default = "default_reject_dot_src_dirs")]
    pub reject_dot_src_dirs: bool,
    /// Wall-clock limit, in seconds, for every external command a processor
    /// runs; a command still running at the limit is killed and its product
    /// fails with "Command timed out after Ns". `0` (the default) means no
    /// limit: a build waits for its tools however long they take.
    ///
    /// Opt-in on purpose. A tool that hangs is a bug -- in the tool, in the
    /// input it was given, or in the environment -- and the fix is to find
    /// that cause, not to cut the tool off and move on. This knob is for
    /// the case where a build must not be allowed to sit forever (an
    /// unattended runner, say), where a loud kill is preferable to a
    /// silent hang. Processors with their own timeout (marp's
    /// `timeout_secs`) keep it: an explicit per-processor value wins over
    /// this default.
    #[serde(default)]
    pub command_timeout_secs: u64,
}

const fn default_parallel() -> usize {
    0
}

fn default_output_dir() -> String {
    "out".into()
}

const fn default_max_arg_len() -> usize {
    1_000_000
}

const fn default_max_discovery_passes() -> usize {
    10
}

const fn default_hash_tool_versions() -> bool {
    true
}

const fn default_warn_symlinks() -> bool {
    false
}

const fn default_allow_missing_dep_auto() -> bool {
    false
}

const fn default_allow_missing_src_dirs() -> bool {
    false
}

const fn default_reject_dot_src_dirs() -> bool {
    false
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            parallel: 0,
            batch_size: 0, // Default: batching enabled, no size limit
            output_dir: "out".into(),
            max_arg_len: default_max_arg_len(),
            max_discovery_passes: default_max_discovery_passes(),
            hash_tool_versions: default_hash_tool_versions(),
            warn_symlinks: default_warn_symlinks(),
            allow_missing_dep_auto: default_allow_missing_dep_auto(),
            allow_missing_src_dirs: default_allow_missing_src_dirs(),
            reject_dot_src_dirs: default_reject_dot_src_dirs(),
            command_timeout_secs: 0,
        }
    }
}

/// Method used to restore files from cache
#[derive(Debug, Deserialize, Serialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RestoreMethod {
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
            Self::Auto => {
                if std::env::var("CI").is_ok_and(|v| v == "true") {
                    Self::Copy
                } else {
                    Self::Hardlink
                }
            }
            other => other,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    #[serde(default)]
    pub restore_method: RestoreMethod,
    /// Whether to compress cached objects with zstd (default: false).
    /// Incompatible with hardlink restore method.
    #[serde(default)]
    pub compression: bool,
    /// Remote cache URL (e.g., "<s3://bucket/prefix>", "http://host:port/path", or local "<file:///path>")
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
    /// How long a cached HTTP response (remote JSON schemas, etc.) stays
    /// fresh, in seconds. Default: 7 days.
    ///
    /// The webcache previously had no expiry at all: a URL fetched once was
    /// served from disk forever, so a schema that changed upstream was never
    /// picked up and the database only ever grew. Set to 0 to disable the
    /// webcache (always re-fetch).
    #[serde(default = "default_webcache_ttl_secs")]
    pub webcache_ttl_secs: u64,
}

/// Seven days. Long enough that a normal working week of builds hits the
/// cache, short enough that an upstream schema change lands without anyone
/// having to know `rsconstruct cache webcache clear` exists.
const fn default_webcache_ttl_secs() -> u64 {
    7 * 24 * 60 * 60
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
            webcache_ttl_secs: default_webcache_ttl_secs(),
        }
    }
}

pub const fn default_true() -> bool {
    true
}

/// A single processor instance parsed from the TOML config.
/// `[processor.checker.pylint]` produces one instance with
/// `pname="processor.checker.pylint"`, `instance_name="processor.checker.pylint"`.
/// `[processor.checker.pylint.core]` produces one with the same `pname` and
/// `instance_name="processor.checker.pylint.core"`.
#[derive(Debug, Clone)]
pub struct ProcessorInstance {
    /// Instance name: the pname for a single instance, `pname.<name>` for a named one.
    pub instance_name: String,
    /// The processor's full name, `processor.<type>.<name>`.
    pub pname: String,
    /// The raw TOML config for this instance (deserialized lazily per processor type)
    pub config_toml: toml::Value,
    /// Source of every field in `config_toml` (user TOML, processor default, scan default, …).
    pub provenance: ProvenanceMap,
}

use crate::registries::{self as registry, ProcessorPlugin};

/// The plugin for a processor name (`processor.checker.ruff`); an instance
/// name finds the same plugin.
pub fn find_registry_entry(pname: &str) -> Option<&'static ProcessorPlugin> {
    registry::find_plugin(pname)
}

/// Return all registered processor plugins.
pub fn registry_entries() -> impl Iterator<Item = &'static ProcessorPlugin> {
    registry::all_plugins()
}

/// Check if a name is a registered processor (`processor.<type>.<name>`).
pub fn is_builtin_type(name: &str) -> bool {
    find_registry_entry(name).is_some()
}

/// Seed a provenance map with every top-level key currently present in `value`,
/// marking each as originating from the user's TOML. Defaults applied afterwards
/// will see these entries and not overwrite them.
///
/// Line numbers are filled in later by the `toml_edit` span pass; until then we
/// use line 0 as a sentinel meaning "from user TOML, line unknown".
pub fn seed_user_provenance(value: &toml::Value) -> ProvenanceMap {
    let mut map = ProvenanceMap::new();
    if let Some(table) = value.as_table() {
        for key in table.keys() {
            map.insert(key.clone(), FieldProvenance::UserToml { line: 0 });
        }
    }
    map
}

/// Resolve scan and processor defaults for an instance config in-place.
pub fn resolve_instance_defaults(
    type_name: &str,
    value: &mut toml::Value,
    provenance: &mut ProvenanceMap,
) {
    if find_registry_entry(type_name).is_some() {
        registry::apply_all_defaults(type_name, value, provenance);
    }
}

impl ProcessorConfig {
    /// Collect unique scan directories from all declared instances.
    pub(crate) fn src_dirs(&self) -> Vec<String> {
        let mut dirs: Vec<String> = self
            .instances
            .iter()
            .flat_map(|inst| {
                inst.config_toml
                    .get("src_dirs")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flat_map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(std::string::ToString::to_string))
                    })
                    .filter(|d| !d.is_empty())
            })
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    /// Return known fields for a builtin processor type, or None for Lua plugins.
    pub(crate) fn known_fields_for(type_name: &str) -> Option<Vec<&'static str>> {
        let e = find_registry_entry(type_name)?;
        // Standard fields (minus this processor's omissions) plus the
        // plugin's own FieldSpec names. A spec sharing a standard field's
        // name replaces it, so the dedup keeps one copy.
        let mut fields: Vec<&'static str> = StandardConfig::known_fields()
            .iter()
            .copied()
            .filter(|f| !e.omit_standard_fields.contains(f))
            .collect();
        for spec in e.fields {
            if !fields.contains(&spec.name) {
                fields.push(spec.name);
            }
        }
        Some(fields)
    }

    /// Return checksum-affecting fields for a builtin processor type, or None for Lua plugins.
    pub(crate) fn checksum_fields_for(type_name: &str) -> Option<Vec<&'static str>> {
        let e = find_registry_entry(type_name)?;
        // A spec that shadows a standard field owns its checksum membership.
        let shadowed = |f: &&'static str| e.fields.iter().any(|s| s.name == *f);
        let mut fields: Vec<&'static str> = StandardConfig::checksum_fields()
            .iter()
            .copied()
            .filter(|f| !e.omit_standard_fields.contains(f))
            .filter(|f| !shadowed(f))
            .collect();
        for spec in e.fields {
            if spec.affects_output {
                fields.push(spec.name);
            }
        }
        Some(fields)
    }

    /// Return must fields (required non-empty fields) for a builtin processor type, or None for Lua plugins.
    pub(crate) fn must_fields_for(type_name: &str) -> Option<Vec<&'static str>> {
        let e = find_registry_entry(type_name)?;
        Some(
            e.fields
                .iter()
                .filter(|s| s.required)
                .map(|s| s.name)
                .collect(),
        )
    }

    /// Return (field, description) pairs for a builtin processor type, or None for Lua plugins.
    /// Standard-field descriptions come from `StandardConfig` (minus omissions,
    /// minus shadowed); the plugin's spec docs follow.
    pub(crate) fn field_descriptions_for(
        type_name: &str,
    ) -> Option<Vec<(&'static str, &'static str)>> {
        let e = find_registry_entry(type_name)?;
        let shadowed = |f: &str| e.fields.iter().any(|s| s.name == f);
        let mut descs: Vec<(&'static str, &'static str)> = StandardConfig::field_descriptions()
            .iter()
            .copied()
            .filter(|(f, _)| !e.omit_standard_fields.contains(f) && !shadowed(f))
            .collect();
        for spec in e.fields {
            descs.push((spec.name, spec.doc));
        }
        Some(descs)
    }

    /// Return the default config for a processor type as pretty JSON, or None if unknown.
    pub(crate) fn defconfig_json(type_name: &str) -> Option<String> {
        let entry = find_registry_entry(type_name)?;
        // The defaults machinery is keyed by the full name; the plugin's
        // short `name` finds nothing there and yields an all-empty config.
        (entry.defconfig_json)(&entry.pname())
    }
}

/// Return scan defaults for a builtin processor type. The data lives on the
/// processor's own plugin entry (`ProcessorPlugin.scan_defaults`) — this used
/// to be a 92-arm name-keyed match table here, which every new processor had
/// to remember to extend (and `prettier` once shipped without its arm,
/// silently mis-defaulted).
pub fn scan_defaults_for(type_name: &str) -> Option<ScanDefaultsData> {
    find_registry_entry(type_name)?.scan_defaults
}

/// Return per-processor default values (command, `dep_auto`, batch override).
/// Only needed for processors whose defaults differ from the struct's Default impl.
pub fn processor_defaults_for(type_name: &str) -> Option<ProcessorDefaults> {
    // Data lives on the plugin entry — see `scan_defaults_for`'s note; this
    // was the second of the two hand-synchronized default tables (V6's marp
    // args divergence was between this table and MarpConfig::default()).
    find_registry_entry(type_name)?.defaults
}

/// Apply processor-specific defaults to a config TOML value.
/// Sets command and `dep_auto` if they weren't explicitly provided by the user.
/// Every field that's actually injected is recorded in `provenance` as a processor default.
/// Fields already present in `provenance` (i.e. user-set) are skipped.
pub fn apply_processor_defaults(
    type_name: &str,
    value: &mut toml::Value,
    provenance: &mut ProvenanceMap,
) {
    let Some(defaults) = processor_defaults_for(type_name) else {
        return;
    };
    let Some(table) = value.as_table_mut() else {
        return;
    };
    set_string_default(
        table,
        "command",
        defaults.command,
        provenance,
        FieldProvenance::ProcessorDefault,
    );
    set_string_default(
        table,
        "output_dir",
        defaults.output_dir,
        provenance,
        FieldProvenance::ProcessorDefault,
    );
    set_string_array_default(
        table,
        "dep_auto",
        defaults.dep_auto,
        provenance,
        FieldProvenance::ProcessorDefault,
    );
    set_string_array_default(
        table,
        "formats",
        defaults.formats,
        provenance,
        FieldProvenance::ProcessorDefault,
    );
    set_string_array_default(
        table,
        "args",
        defaults.args,
        provenance,
        FieldProvenance::ProcessorDefault,
    );
    if let Some(batch) = defaults.batch
        && !table.contains_key("batch")
    {
        table.insert("batch".into(), toml::Value::Boolean(batch));
        provenance::record_if_absent(provenance, "batch", FieldProvenance::ProcessorDefault);
    }
}

fn set_string_default(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    val: &str,
    provenance: &mut ProvenanceMap,
    source: FieldProvenance,
) {
    if !val.is_empty() && !table.contains_key(key) {
        table.insert(key.into(), toml::Value::String(val.into()));
        provenance::record_if_absent(provenance, key, source);
    }
}

fn set_string_array_default(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    vals: &[&str],
    provenance: &mut ProvenanceMap,
    source: FieldProvenance,
) {
    if !vals.is_empty() && !table.contains_key(key) {
        let arr: Vec<toml::Value> = vals
            .iter()
            .map(|s| toml::Value::String(s.to_string()))
            .collect();
        table.insert(key.into(), toml::Value::Array(arr));
        provenance::record_if_absent(provenance, key, source);
    }
}

fn set_empty_array_default(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    provenance: &mut ProvenanceMap,
    source: FieldProvenance,
) {
    if !table.contains_key(key) {
        table.insert(key.into(), toml::Value::Array(Vec::new()));
        provenance::record_if_absent(provenance, key, source);
    }
}

fn set_maybe_empty_array_default(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    vals: &[&str],
    provenance: &mut ProvenanceMap,
    source: FieldProvenance,
) {
    if !table.contains_key(key) {
        let arr: Vec<toml::Value> = vals
            .iter()
            .map(|s| toml::Value::String(s.to_string()))
            .collect();
        table.insert(key.into(), toml::Value::Array(arr));
        provenance::record_if_absent(provenance, key, source);
    }
}

/// Apply scan defaults to a config TOML value.
/// Sets `src_dirs`, `src_extensions`, and `src_exclude_dirs` if not explicitly provided.
/// Every field that's actually injected is recorded in `provenance` as a scan default.
pub fn apply_scan_defaults(
    type_name: &str,
    value: &mut toml::Value,
    provenance: &mut ProvenanceMap,
) {
    let Some(defaults) = scan_defaults_for(type_name) else {
        return;
    };
    let Some(table) = value.as_table_mut() else {
        return;
    };
    set_maybe_empty_array_default(
        table,
        "src_dirs",
        defaults.src_dirs,
        provenance,
        FieldProvenance::ScanDefault,
    );
    set_maybe_empty_array_default(
        table,
        "src_extensions",
        defaults.src_extensions,
        provenance,
        FieldProvenance::ScanDefault,
    );
    set_maybe_empty_array_default(
        table,
        "src_exclude_dirs",
        defaults.src_exclude_dirs,
        provenance,
        FieldProvenance::ScanDefault,
    );
    set_empty_array_default(
        table,
        "src_exclude_files",
        provenance,
        FieldProvenance::ScanDefault,
    );
    set_empty_array_default(
        table,
        "src_exclude_paths",
        provenance,
        FieldProvenance::ScanDefault,
    );
    set_empty_array_default(table, "src_files", provenance, FieldProvenance::ScanDefault);
}

/// Processor configuration: a collection of declared processor instances.
/// Each `[processor.TYPE]` or `[processor.TYPE.NAME]` section in rsconstruct.toml
/// creates a `ProcessorInstance`. No instances exist by default — only what's declared.
#[derive(Debug, Default)]
pub struct ProcessorConfig {
    /// All declared processor instances
    pub instances: Vec<ProcessorInstance>,
    /// Lua plugin configs (processor types not in the builtin registry)
    pub extra: HashMap<String, toml::Value>,
}

impl Serialize for ProcessorConfig {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        // Rebuild the `[processor]` tree the instances were read from:
        // processor → type → name → (fields | instance → fields).
        let mut root = toml::map::Map::new();
        for inst in &self.instances {
            let Some(parsed) = registry::parse_name(&inst.instance_name) else {
                continue;
            };
            let type_table = root
                .entry(parsed.processor_type.as_str().to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
            let Some(type_table) = type_table.as_table_mut() else {
                continue;
            };
            match parsed.instance {
                None => {
                    type_table.insert(parsed.short.to_string(), inst.config_toml.clone());
                }
                Some(instance) => {
                    let name_table = type_table
                        .entry(parsed.short.to_string())
                        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
                    if let Some(name_table) = name_table.as_table_mut() {
                        name_table.insert(instance.to_string(), inst.config_toml.clone());
                    }
                }
            }
        }
        if !self.extra.is_empty() {
            let lua = root
                .entry(crate::processor::ProcessorType::Lua.as_str().to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
            if let Some(lua) = lua.as_table_mut() {
                for (name, value) in &self.extra {
                    lua.insert(name.clone(), value.clone());
                }
            }
        }
        toml::Value::Table(root).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ProcessorConfig {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let table = toml::Value::deserialize(deserializer)?;
        Ok(Self::from_toml(&table))
    }
}

/// What a `[processor.TYPE.NAME]` section turned out to be.
///
/// The shape is inferred from the section's contents, so it can be
/// genuinely undecidable — which is why this is an enum rather than a bool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionShape {
    /// Direct config fields: `[processor.checker.pylint]` with `args = [...]`.
    SingleInstance,
    /// Named sub-instances: `[processor.checker.pylint.core]`, `[processor.checker.pylint.tests]`.
    MultiInstance,
    /// Reads as both. Carries the keys that are simultaneously known config
    /// field names and plausible instance names.
    Ambiguous { colliding: Vec<String> },
}

impl ProcessorConfig {
    /// Parse the `[processor]` table from TOML into instances.
    pub(crate) fn from_toml(value: &toml::Value) -> Self {
        let Some(table) = value.as_table() else {
            return Self::default();
        };

        let mut instances = Vec::new();
        let mut extra = HashMap::new();

        // The tree is processor → type → name → (fields | instances). Anything
        // that does not fit is skipped here and reported by
        // `validate_processor_fields_raw`, which runs on the same raw value.
        for (type_key, type_val) in table {
            let Some(type_table) = type_val.as_table() else {
                continue;
            };
            let Some(processor_type) = crate::processor::ProcessorType::parse(type_key) else {
                continue;
            };
            for (short, val) in type_table {
                let Some(sub_table) = val.as_table() else {
                    continue;
                };
                if processor_type == crate::processor::ProcessorType::Lua {
                    // Lua plugins are keyed by the plugin file's stem.
                    extra.insert(short.clone(), val.clone());
                    continue;
                }
                let pname = format!("{}{type_key}.{short}", registry::NAME_PREFIX);
                if !is_builtin_type(&pname) {
                    continue;
                }
                if Self::is_multi_instance(&pname, sub_table) {
                    // Multi-instance: [processor.checker.pylint.core], [processor.checker.pylint.tests]
                    for (name, inst_val) in sub_table {
                        let instance_name = format!("{pname}.{name}");
                        let mut config = inst_val.clone();
                        let mut provenance = seed_user_provenance(&config);
                        resolve_instance_defaults(&pname, &mut config, &mut provenance);
                        instances.push(ProcessorInstance {
                            instance_name,
                            pname: pname.clone(),
                            config_toml: config,
                            provenance,
                        });
                    }
                } else {
                    // Single instance: [processor.checker.pylint]
                    let mut config = val.clone();
                    let mut provenance = seed_user_provenance(&config);
                    resolve_instance_defaults(&pname, &mut config, &mut provenance);
                    instances.push(ProcessorInstance {
                        instance_name: pname.clone(),
                        pname,
                        config_toml: config,
                        provenance,
                    });
                }
            }
        }

        Self { instances, extra }
    }

    /// Determine if a processor section contains named sub-instances (multi-instance)
    /// or direct config fields (single-instance).
    ///
    /// Heuristic: if ALL values are tables AND none of the keys match known config
    /// field names for this processor type, it's multi-instance. An ambiguous
    /// section is treated as single-instance here; `parse_processors` rejects
    /// it outright, so this only affects callers that have already validated.
    fn is_multi_instance(type_name: &str, table: &toml::map::Map<String, toml::Value>) -> bool {
        matches!(
            Self::classify_section(type_name, table),
            SectionShape::MultiInstance
        )
    }

    /// The shape of a `[processor.TYPE.NAME]` section, and whether it is even
    /// decidable.
    ///
    /// Separated from `is_multi_instance` so the ambiguous case can be
    /// reported rather than silently resolved. The heuristic reads a
    /// section's *shape* to guess the user's intent, which means a section
    /// that looks like both is a genuine ambiguity — and resolving it
    /// quietly (as returning a bare bool forced) is how a config could
    /// change meaning under the user without warning.
    fn classify_section(
        type_name: &str,
        table: &toml::map::Map<String, toml::Value>,
    ) -> SectionShape {
        if table.is_empty() {
            return SectionShape::SingleInstance;
        }

        let Some(known) = Self::known_fields_for(type_name) else {
            // Unknown type (Lua plugin): no field list to compare against, so
            // multi-instance is undecidable and the section is taken as one
            // instance. See `multi_instance_requires_known_fields` in the docs.
            return SectionShape::SingleInstance;
        };
        let known_fields: Vec<&str> = known
            .iter()
            .chain(SCAN_CONFIG_FIELDS.iter())
            .chain(STANDARD_EXTRA_FIELDS.iter())
            .copied()
            .collect();

        let field_keys: Vec<&str> = table
            .keys()
            .filter(|k| known_fields.contains(&k.as_str()))
            .map(String::as_str)
            .collect();
        let all_values_are_tables = table.values().all(toml::Value::is_table);

        match (field_keys.is_empty(), all_values_are_tables) {
            // Only sub-tables, no known field names: unambiguously instances.
            (true, true) => SectionShape::MultiInstance,
            // At least one known field name present.
            (false, _) => {
                // A known field whose value is a table, alongside nothing but
                // tables, is the ambiguous case: it reads as both "a config
                // field" and "an instance named after a field".
                if all_values_are_tables {
                    SectionShape::Ambiguous {
                        colliding: field_keys.iter().map(|s| (*s).to_string()).collect(),
                    }
                } else {
                    SectionShape::SingleInstance
                }
            }
            // Scalar values present and no known fields: malformed, but
            // validation reports unknown fields far better than we could.
            (true, false) => SectionShape::SingleInstance,
        }
    }

    /// Resolve scan defaults for all instances.
    pub(crate) fn resolve_scan_defaults(&mut self) {
        for inst in &mut self.instances {
            resolve_instance_defaults(&inst.pname, &mut inst.config_toml, &mut inst.provenance);
        }
    }

    /// Rewrite output paths in processor configs:
    /// 1. For named instances (e.g., `pylint.core`), remap default `out/{type_name}`
    ///    to `out/{instance_name}` so each instance gets its own output directory.
    /// 2. If the global `output_dir` is not "out", replace the `out/` prefix with the
    ///    global value (e.g., `build/` → `build/marp`).
    ///
    /// When a field was originally a `ProcessorDefault` and gets rewritten here, its
    /// provenance is upgraded to `OutputDirDefault` so `config show` can explain why
    /// the value differs from the processor's own default.
    pub(crate) fn apply_output_dir_defaults(&mut self, global_output_dir: &str) {
        for inst in &mut self.instances {
            let type_default_prefix = format!("out/{}", inst.pname);
            let instance_prefix = format!("{}/{}", global_output_dir, inst.instance_name);

            for field in &["output_dir", "output"] {
                // A user-set or CLI-set value is explicitly chosen — never
                // rewrite it (previously only the provenance label was
                // guarded while the value itself was still rewritten).
                if matches!(
                    inst.provenance.get(*field),
                    Some(
                        FieldProvenance::UserToml { .. }
                            | FieldProvenance::LocalToml { .. }
                            | FieldProvenance::CliOverride
                    ),
                ) {
                    continue;
                }
                let Some(val) = inst
                    .config_toml
                    .get(field)
                    .and_then(|v| v.as_str())
                    .map(std::string::ToString::to_string)
                else {
                    continue;
                };
                // Prefix matches must respect path boundaries: `out/marp`
                // must not match `out/marpdeck/...`.
                let type_rest = val
                    .strip_prefix(&type_default_prefix)
                    .filter(|r| r.is_empty() || r.starts_with('/'));
                let new_val = if inst.instance_name != inst.pname
                    && let Some(rest) = type_rest
                {
                    // Named instance: remap out/{type} → {global}/{instance}
                    format!("{instance_prefix}{rest}")
                } else if global_output_dir != "out"
                    && let Some(rest) = val.strip_prefix("out/")
                {
                    // Global output dir override: remap out/ → {global}/
                    format!("{global_output_dir}/{rest}")
                } else {
                    continue;
                };
                if let Some(table) = inst.config_toml.as_table_mut() {
                    table.insert(field.to_string(), toml::Value::String(new_val));
                }
                inst.provenance
                    .insert((*field).to_string(), FieldProvenance::OutputDirDefault);
            }
        }
    }

    /// Get the first instance of a given type (for single-instance access).
    /// Returns None if no instance of that type is declared.
    pub(crate) fn first_instance_of_type(&self, type_name: &str) -> Option<&ProcessorInstance> {
        self.instances.iter().find(|i| i.pname == type_name)
    }

    /// Deserialize the config for `type_name`, whether or not the user
    /// declared the instance.
    ///
    /// A declared instance already has registry defaults baked into its
    /// `config_toml` by `Config::load`. An undeclared one has nothing, and
    /// `C::default()` leaves scan fields unresolved — reaching a scan
    /// accessor then panics with "scan fields not resolved". So the
    /// undeclared case is routed through the same `apply_all_defaults` the
    /// declared case used, instead of each caller hand-resolving.
    pub(crate) fn instance_config_or_default<C: serde::de::DeserializeOwned>(
        &self,
        type_name: &str,
    ) -> Result<C> {
        if let Some(inst) = self.first_instance_of_type(type_name) {
            return inst
                .config_toml
                .clone()
                .try_into()
                .with_context(|| format!("Failed to parse [{type_name}] config"));
        }
        let mut value = toml::Value::Table(toml::map::Map::new());
        let mut provenance = ProvenanceMap::new();
        crate::registries::processor::apply_all_defaults(type_name, &mut value, &mut provenance);
        value
            .try_into()
            .with_context(|| format!("Failed to build default config for processor '{type_name}'"))
    }

    /// Get a typed config value from an instance's TOML config.
    /// Returns the default if the instance is not declared or the field is missing.
    pub(crate) fn instance_field_str(&self, type_name: &str, field: &str) -> Option<String> {
        self.first_instance_of_type(type_name)
            .and_then(|inst| inst.config_toml.get(field))
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string)
    }
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

/// Parse a single `--iset`/`--pset` entry of the form `<name>.<field>=<value>`.
/// Returns `(name, field, parsed_value)`. The value is parsed as a TOML scalar/
/// array/table; if parsing fails it is treated as a bare string. Hard-errors on
/// missing dot, missing equals, or empty name/field.
fn parse_override_entry<'a>(raw: &'a str, flag: &str) -> Result<(&'a str, &'a str, toml::Value)> {
    let (lhs, value_str) = raw.split_once('=').ok_or_else(|| {
        anyhow::anyhow!("{flag} '{raw}': missing '=' (expected <name>.<field>=<value>)")
    })?;
    // Split on the LAST dot: instance names may contain dots (`pylint.core`)
    // while field names never do.
    let (name, field) = lhs.rsplit_once('.').ok_or_else(|| {
        anyhow::anyhow!(
            "{flag} '{raw}': missing '.' between name and field (expected <name>.<field>=<value>)"
        )
    })?;
    if name.is_empty() {
        anyhow::bail!("{flag} '{raw}': empty name before '.'");
    }
    if field.is_empty() {
        anyhow::bail!("{flag} '{raw}': empty field between '.' and '='");
    }
    // Parse as a TOML value via a synthetic "v = <value>" doc; fall back to a
    // bare string so users can write `--iset marp.command=marp` without quoting.
    let parsed: toml::Value = match toml::from_str::<toml::Value>(&format!("v = {value_str}")) {
        Ok(toml::Value::Table(mut t)) => t
            .remove("v")
            .unwrap_or_else(|| toml::Value::String(value_str.to_string())),
        _ => toml::Value::String(value_str.to_string()),
    };
    Ok((name, field, parsed))
}

/// Apply an override to every instance matching `predicate`. Hard-errors when
/// no instances match, when the field is unknown for the processor type, or
/// when the value's TOML type doesn't match the field's declared type.
fn apply_override_to_instances(
    instances: &mut [ProcessorInstance],
    field: &str,
    value: &toml::Value,
    predicate: impl Fn(&ProcessorInstance) -> bool,
    name_kind: &str,
    name: &str,
) -> Result<()> {
    let matching_indices: Vec<usize> = instances
        .iter()
        .enumerate()
        .filter(|(_, inst)| predicate(inst))
        .map(|(i, _)| i)
        .collect();
    if matching_indices.is_empty() {
        anyhow::bail!("no processor instance with {name_kind} '{name}'");
    }
    for i in matching_indices {
        let inst = &mut instances[i];
        let type_name = inst.pname.clone();
        validate_override_field(&type_name, field, value, &inst.instance_name)?;
        if let Some(table) = inst.config_toml.as_table_mut() {
            table.insert(field.to_string(), value.clone());
            inst.provenance
                .insert(field.to_string(), FieldProvenance::CliOverride);
        } else {
            anyhow::bail!(
                "instance '{}' config is not a table (cannot apply override)",
                inst.instance_name
            );
        }
    }
    Ok(())
}

/// Validate that `field` is a known config field for `type_name` and that
/// `value`'s TOML type matches the field's declared type.
fn validate_override_field(
    type_name: &str,
    field: &str,
    value: &toml::Value,
    instance_label: &str,
) -> Result<()> {
    let own_fields = ProcessorConfig::known_fields_for(type_name).unwrap_or_default();
    let is_known = own_fields.contains(&field)
        || SCAN_CONFIG_FIELDS.contains(&field)
        || STANDARD_EXTRA_FIELDS.contains(&field);
    if !is_known {
        let mut all_fields: Vec<&str> = own_fields
            .iter()
            .chain(SCAN_CONFIG_FIELDS.iter())
            .chain(STANDARD_EXTRA_FIELDS.iter())
            .copied()
            .collect();
        all_fields.sort_unstable();
        all_fields.dedup();
        anyhow::bail!(
            "instance '{instance_label}' (type {type_name}): unknown field '{field}' (valid fields: {})",
            all_fields.join(", ")
        );
    }
    if let Some(expected) = expected_field_type(type_name, field)
        && !expected.matches(value)
    {
        anyhow::bail!(
            "instance '{instance_label}' (type {type_name}): field '{field}' must be {}, got {} ({value})",
            expected.label(),
            FieldType::describe_value(value),
        );
    }
    Ok(())
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

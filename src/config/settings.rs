//! The project-wide settings sections of rsconstruct.toml: `[build]`,
//! `[cache]`, `[pages]` and `[commands]`.

use serde::{Deserialize, Serialize};

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

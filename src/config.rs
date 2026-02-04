use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_FILE: &str = "rsb.toml";

/// Validate extra_inputs paths exist and return them as PathBufs.
/// Paths are relative to project root (which is cwd).
pub fn resolve_extra_inputs(extra_inputs: &[String]) -> Result<Vec<PathBuf>> {
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
pub fn config_hash(value: &impl Serialize) -> String {
    let json = serde_json::to_string(value).expect("config serialization failed");
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
pub struct ScanConfig {
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
    pub fn resolve(&mut self, scan_dir: &str, extensions: &[&str], exclude_dirs: &[&str]) {
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
    pub fn scan_dir(&self) -> &str {
        self.scan_dir.as_deref().expect("ScanConfig not resolved")
    }

    /// Get the resolved extensions. Panics if called before resolve().
    pub fn extensions(&self) -> &[String] {
        self.extensions.as_deref().expect("ScanConfig not resolved")
    }

    /// Get the resolved exclude dirs. Panics if called before resolve().
    pub fn exclude_dirs(&self) -> &[String] {
        self.exclude_dirs.as_deref().expect("ScanConfig not resolved")
    }

    /// Get the resolved exclude files. Panics if called before resolve().
    pub fn exclude_files(&self) -> &[String] {
        self.exclude_files.as_deref().expect("ScanConfig not resolved")
    }

    /// Get the resolved exclude paths. Panics if called before resolve().
    pub fn exclude_paths(&self) -> &[String] {
        self.exclude_paths.as_deref().expect("ScanConfig not resolved")
    }
}

const PYTHON_EXCLUDE_DIRS: &[&str] = &[
    "/.venv/", "/__pycache__/", "/.git/", "/out/",
    "/node_modules/", "/.tox/", "/build/", "/dist/", "/.eggs/",
];

const CC_EXCLUDE_DIRS: &[&str] = &["/.git/", "/out/", "/build/", "/dist/"];

const SPELLCHECK_EXCLUDE_DIRS: &[&str] = &[
    "/.git/", "/out/", "/.rsb/", "/node_modules/", "/build/", "/dist/", "/target/",
];

const MAKE_EXCLUDE_DIRS: &[&str] = &["/.git/", "/out/", "/.rsb/", "/build/", "/dist/", "/target/"];

const SHELL_EXCLUDE_DIRS: &[&str] = &["/.git/", "/out/", "/.rsb/", "/node_modules/", "/build/", "/dist/", "/target/"];

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PluginsConfig {
    #[serde(default = "default_plugins_dir")]
    pub dir: String,
}

fn default_plugins_dir() -> String {
    "plugins".into()
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self { dir: "plugins".into() }
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
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BuildConfig {
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
pub enum RestoreMethod {
    #[default]
    Hardlink,
    Copy,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CacheConfig {
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
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            restore_method: RestoreMethod::default(),
            remote: None,
            remote_push: true,
            remote_pull: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_processors() -> Vec<String> {
    vec![
        "template".into(), "ruff".into(), "pylint".into(),
        "cc_single_file".into(), "cpplint".into(), "shellcheck".into(), "spellcheck".into(), "make".into(),
    ]
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProcessorConfig {
    #[serde(default = "default_true")]
    pub auto_detect: bool,
    #[serde(default = "default_processors")]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub template: TemplateConfig,
    #[serde(default)]
    pub ruff: RuffConfig,
    #[serde(default)]
    pub pylint: PylintConfig,
    #[serde(default)]
    pub cc_single_file: CcConfig,
    #[serde(default)]
    pub cpplint: CpplintConfig,
    #[serde(default)]
    pub spellcheck: SpellcheckConfig,
    #[serde(default)]
    pub shellcheck: ShellcheckConfig,
    #[serde(default)]
    pub sleep: SleepConfig,
    #[serde(default)]
    pub make: MakeConfig,
    /// Captures unknown [processor.PLUGIN_NAME] sections for Lua plugins
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            auto_detect: true,
            enabled: default_processors(),
            template: TemplateConfig::default(),
            ruff: RuffConfig::default(),
            pylint: PylintConfig::default(),
            cc_single_file: CcConfig::default(),
            cpplint: CpplintConfig::default(),
            shellcheck: ShellcheckConfig::default(),
            spellcheck: SpellcheckConfig::default(),
            sleep: SleepConfig::default(),
            make: MakeConfig::default(),
            extra: HashMap::new(),
        }
    }
}

impl ProcessorConfig {
    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled.iter().any(|p| p == name)
    }

    /// Fill in None scan fields with per-processor defaults.
    /// Called after loading from TOML so that `config show` displays resolved values
    /// and processors can access fields without fallbacks.
    pub fn resolve_scan_defaults(&mut self) {
        self.template.scan.resolve("templates", &[".tera"], &[]);
        self.ruff.scan.resolve("", &[".py"], PYTHON_EXCLUDE_DIRS);
        self.pylint.scan.resolve("", &[".py"], PYTHON_EXCLUDE_DIRS);
        self.cc_single_file.scan.resolve("src", &[".c", ".cc"], &[]);
        self.cpplint.scan.resolve("src", &[".c", ".cc"], CC_EXCLUDE_DIRS);
        self.shellcheck.scan.resolve("", &[".sh", ".bash"], SHELL_EXCLUDE_DIRS);
        self.spellcheck.scan.resolve("", &[".md"], SPELLCHECK_EXCLUDE_DIRS);
        self.sleep.scan.resolve("sleep", &[".sleep"], &[]);
        self.make.scan.resolve("", &["Makefile"], MAKE_EXCLUDE_DIRS);
    }
}

// --- Processor configs ---

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TemplateConfig {
    #[serde(default = "default_true")]
    pub strict: bool,
    #[serde(default)]
    pub trim_blocks: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            strict: true,
            trim_blocks: false,
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: Some("templates".into()),
                extensions: Some(vec![".tera".into()]),
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

fn default_ruff_linter() -> String {
    "ruff".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RuffConfig {
    #[serde(default = "default_ruff_linter")]
    pub linter: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for RuffConfig {
    fn default() -> Self {
        Self {
            linter: "ruff".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: None,
                extensions: Some(vec![".py".into()]),
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PylintConfig {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PylintConfig {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: None,
                extensions: Some(vec![".py".into()]),
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

fn default_cpplint_checker() -> String {
    "cppcheck".into()
}

fn default_cpplint_args() -> Vec<String> {
    vec![
        "--error-exitcode=1".into(),
        "--enable=warning,style,performance,portability".into(),
    ]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CpplintConfig {
    #[serde(default = "default_cpplint_checker")]
    pub checker: String,
    #[serde(default = "default_cpplint_args")]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for CpplintConfig {
    fn default() -> Self {
        Self {
            checker: "cppcheck".into(),
            args: default_cpplint_args(),
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: Some("src".into()),
                extensions: Some(vec![".c".into(), ".cc".into()]),
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

fn default_cc_compiler() -> String {
    "gcc".into()
}

fn default_cxx_compiler() -> String {
    "g++".into()
}

fn default_output_suffix() -> String {
    ".elf".into()
}

/// Method for scanning C/C++ header dependencies
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IncludeScanner {
    /// Native regex-based scanner (fast, no external process)
    #[default]
    Native,
    /// Use gcc/g++ -MM (accurate but slower, spawns external process)
    Compiler,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CcConfig {
    #[serde(default = "default_cc_compiler")]
    pub cc: String,
    #[serde(default = "default_cxx_compiler")]
    pub cxx: String,
    #[serde(default)]
    pub cflags: Vec<String>,
    #[serde(default)]
    pub cxxflags: Vec<String>,
    #[serde(default)]
    pub ldflags: Vec<String>,
    #[serde(default)]
    pub include_paths: Vec<String>,
    #[serde(default = "default_output_suffix")]
    pub output_suffix: String,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    /// Method for scanning header dependencies (native or compiler)
    #[serde(default)]
    pub include_scanner: IncludeScanner,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for CcConfig {
    fn default() -> Self {
        Self {
            cc: "gcc".into(),
            cxx: "g++".into(),
            cflags: Vec::new(),
            cxxflags: Vec::new(),
            ldflags: Vec::new(),
            include_paths: Vec::new(),
            output_suffix: ".elf".into(),
            extra_inputs: Vec::new(),
            include_scanner: IncludeScanner::default(),
            scan: ScanConfig {
                scan_dir: Some("src".into()),
                extensions: Some(vec![".c".into(), ".cc".into()]),
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

fn default_spellcheck_language() -> String {
    "en_US".into()
}

fn default_spellcheck_words_file() -> String {
    ".spellcheck-words".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SpellcheckConfig {
    #[serde(default = "default_spellcheck_language")]
    pub language: String,
    #[serde(default = "default_spellcheck_words_file")]
    pub words_file: String,
    #[serde(default)]
    pub use_words_file: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for SpellcheckConfig {
    fn default() -> Self {
        Self {
            language: "en_US".into(),
            words_file: ".spellcheck-words".into(),
            use_words_file: false,
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: None,
                extensions: Some(vec![".md".into()]),
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SleepConfig {
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for SleepConfig {
    fn default() -> Self {
        Self {
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: Some("sleep".into()),
                extensions: Some(vec![".sleep".into()]),
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

fn default_make() -> String {
    "make".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MakeConfig {
    #[serde(default = "default_make")]
    pub make: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MakeConfig {
    fn default() -> Self {
        Self {
            make: "make".into(),
            args: Vec::new(),
            target: String::new(),
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: None,
                extensions: Some(vec!["Makefile".into()]),
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

fn default_shellcheck_checker() -> String {
    "shellcheck".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShellcheckConfig {
    #[serde(default = "default_shellcheck_checker")]
    pub checker: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for ShellcheckConfig {
    fn default() -> Self {
        Self {
            checker: "shellcheck".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: None,
                extensions: Some(vec![".sh".into(), ".bash".into()]),
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CompletionsConfig {
    #[serde(default = "default_shells")]
    pub shells: Vec<String>,
}

fn default_shells() -> Vec<String> {
    vec!["bash".into()]
}

fn default_analyzers() -> Vec<String> {
    vec!["cpp".into(), "python".into()]
}

/// Configuration for dependency analyzers
#[derive(Debug, Deserialize, Serialize)]
pub struct AnalyzerConfig {
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
    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled.iter().any(|a| a == name)
    }
}

/// Configuration for the C/C++ dependency analyzer
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct CppAnalyzerConfig {
    /// Method for scanning header dependencies (native or compiler)
    #[serde(default)]
    pub include_scanner: IncludeScanner,
    /// Additional include paths for header search
    #[serde(default)]
    pub include_paths: Vec<String>,
    /// C compiler (for -MM scanning with compiler method)
    #[serde(default = "default_cc_compiler")]
    pub cc: String,
    /// C++ compiler (for -MM scanning with compiler method)
    #[serde(default = "default_cxx_compiler")]
    pub cxx: String,
    /// C compiler flags (for -MM scanning)
    #[serde(default)]
    pub cflags: Vec<String>,
    /// C++ compiler flags (for -MM scanning)
    #[serde(default)]
    pub cxxflags: Vec<String>,
}

/// Configuration for the Python dependency analyzer
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct PythonAnalyzerConfig {
    // Currently no specific configuration needed
    // Could add: package_paths, ignore_stdlib, etc.
}

impl Default for CompletionsConfig {
    fn default() -> Self {
        Self { shells: vec!["bash".into()] }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct GraphConfig {
    #[serde(default)]
    pub viewer: Option<String>,
}

impl Config {
    pub fn require_config(project_root: &Path) -> Result<()> {
        let config_path = project_root.join(CONFIG_FILE);
        if !config_path.exists() {
            anyhow::bail!(
                "No rsb.toml found in {}. Run 'rsb init' to create one.",
                project_root.display()
            );
        }
        Ok(())
    }

    pub fn load(project_root: &Path) -> Result<Self> {
        let config_path = project_root.join(CONFIG_FILE);

        let mut config = if config_path.exists() {
            let content = fs::read_to_string(&config_path)
                .context(format!("Failed to read config file: {}", config_path.display()))?;
            toml::from_str(&content)
                .context(format!("Failed to parse config file: {}", config_path.display()))?
        } else {
            Config::default()
        };
        config.processor.resolve_scan_defaults();
        Ok(config)
    }
}

/// Extract a `ScanConfig` from a dynamic TOML table (used by Lua plugins).
/// Falls back to the given defaults for any missing fields.
pub fn scan_config_from_toml(
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

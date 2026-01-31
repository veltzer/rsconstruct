use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_FILE: &str = "rsb.toml";

/// Resolve extra_inputs paths relative to project root, failing if any file does not exist.
pub fn resolve_extra_inputs(project_root: &Path, extra_inputs: &[String]) -> Result<Vec<PathBuf>> {
    let mut resolved = Vec::new();
    for p in extra_inputs {
        let path = project_root.join(p);
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
/// "explicitly set" (Some). Each processor resolves None to its own defaults.
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
}

impl ScanConfig {
    /// Resolve scan_dir, falling back to the provided default
    pub fn scan_dir_or(&self, default: &str) -> String {
        self.scan_dir.clone().unwrap_or_else(|| default.to_string())
    }

    /// Resolve extensions, falling back to the provided defaults
    pub fn extensions_or(&self, defaults: &[&str]) -> Vec<String> {
        self.extensions.clone().unwrap_or_else(|| defaults.iter().map(|s| s.to_string()).collect())
    }

    /// Resolve exclude_dirs, falling back to the provided defaults
    pub fn exclude_dirs_or(&self, defaults: &[&str]) -> Vec<String> {
        self.exclude_dirs.clone().unwrap_or_else(|| defaults.iter().map(|s| s.to_string()).collect())
    }

    /// Fill in None fields with the given defaults (mutates in place)
    fn resolve(&mut self, scan_dir: &str, extensions: &[&str], exclude_dirs: &[&str]) {
        if self.scan_dir.is_none() {
            self.scan_dir = Some(scan_dir.to_string());
        }
        if self.extensions.is_none() {
            self.extensions = Some(extensions.iter().map(|s| s.to_string()).collect());
        }
        if self.exclude_dirs.is_none() {
            self.exclude_dirs = Some(exclude_dirs.iter().map(|s| s.to_string()).collect());
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
    pub completions: CompletionsConfig,
    #[serde(default)]
    pub graph: GraphConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BuildConfig {
    /// Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)
    #[serde(default = "default_parallel")]
    pub parallel: usize,
}

fn default_parallel() -> usize {
    1  // Default to sequential execution
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            parallel: default_parallel(),
        }
    }
}

/// Method used to restore files from cache
#[derive(Debug, Deserialize, Serialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RestoreMethod {
    /// Use hard links (fast, no disk space duplication)
    #[default]
    Hardlink,
    /// Use file copy (works across filesystems, safer)
    Copy,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CacheConfig {
    /// Method to restore files from cache: "hardlink" or "copy"
    #[serde(default)]
    pub restore_method: RestoreMethod,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            restore_method: RestoreMethod::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProcessorConfig {
    /// Use auto-detection to discover relevant processors (default: true)
    #[serde(default = "default_true")]
    pub auto_detect: bool,
    /// List of enabled processors (e.g., ["template", "ruff"])
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
    pub sleep: SleepConfig,
}

fn default_processors() -> Vec<String> {
    vec!["template".to_string(), "ruff".to_string(), "pylint".to_string(), "sleep".to_string(), "cc_single_file".to_string(), "cpplint".to_string(), "spellcheck".to_string()]
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
            spellcheck: SpellcheckConfig::default(),
            sleep: SleepConfig::default(),
        }
    }
}

impl ProcessorConfig {
    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled.iter().any(|p| p == name)
    }

    /// Fill in None scan fields with per-processor defaults.
    /// Called after loading from TOML so that `config show` displays resolved values.
    pub fn resolve_scan_defaults(&mut self) {
        self.template.scan.resolve("templates", &[".tera"], &[]);
        self.ruff.scan.resolve("", &[".py"], &[
            "/.venv/", "/__pycache__/", "/.git/", "/out/",
            "/node_modules/", "/.tox/", "/build/", "/dist/", "/.eggs/",
        ]);
        self.pylint.scan.resolve("", &[".py"], &[
            "/.venv/", "/__pycache__/", "/.git/", "/out/",
            "/node_modules/", "/.tox/", "/build/", "/dist/", "/.eggs/",
        ]);
        self.cc_single_file.scan.resolve("src", &[".c", ".cc"], &[]);
        self.cpplint.scan.resolve("src", &[".c", ".cc"], &[
            "/.git/", "/out/", "/build/", "/dist/",
        ]);
        self.spellcheck.scan.resolve("", &[".md"], &[
            "/.git/", "/out/", "/.rsb/", "/node_modules/", "/build/", "/dist/", "/target/",
        ]);
        self.sleep.scan.resolve("sleep", &[".sleep"], &[]);
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TemplateConfig {
    /// Fail on undefined variables (default: true)
    #[serde(default = "default_true")]
    pub strict: bool,

    /// Remove first newline after block tags (default: false)
    #[serde(default)]
    pub trim_blocks: bool,

    /// Additional input files that trigger rebuilds when changed
    #[serde(default)]
    pub extra_inputs: Vec<String>,

    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_true() -> bool {
    true
}

fn default_template_scan_dir() -> String {
    "templates".to_string()
}

fn default_template_extensions() -> Vec<String> {
    vec![".tera".to_string()]
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            strict: default_true(),
            trim_blocks: false,
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: Some(default_template_scan_dir()),
                extensions: Some(default_template_extensions()),
                exclude_dirs: None,
            },
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CompletionsConfig {
    /// The shells to generate completions for by default
    #[serde(default = "default_shells")]
    pub shells: Vec<String>,
}

fn default_shells() -> Vec<String> {
    vec!["bash".to_string()]
}

impl Default for CompletionsConfig {
    fn default() -> Self {
        Self {
            shells: default_shells(),
        }
    }
}

fn default_python_exclude_dirs() -> Vec<String> {
    vec![
        "/.venv/".to_string(), "/__pycache__/".to_string(), "/.git/".to_string(), "/out/".to_string(),
        "/node_modules/".to_string(), "/.tox/".to_string(), "/build/".to_string(), "/dist/".to_string(), "/.eggs/".to_string(),
    ]
}

fn default_python_extensions() -> Vec<String> {
    vec![".py".to_string()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RuffConfig {
    /// The Python linter to use (default: ruff)
    #[serde(default = "default_ruff_linter")]
    pub linter: String,

    /// Additional arguments to pass to the linter
    #[serde(default)]
    pub args: Vec<String>,

    /// Additional input files that trigger rebuilds when changed
    #[serde(default)]
    pub extra_inputs: Vec<String>,

    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_ruff_linter() -> String {
    "ruff".to_string()
}

impl Default for RuffConfig {
    fn default() -> Self {
        Self {
            linter: default_ruff_linter(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: None,
                extensions: Some(default_python_extensions()),
                exclude_dirs: Some(default_python_exclude_dirs()),
            },
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PylintConfig {
    /// Additional arguments to pass to pylint
    #[serde(default)]
    pub args: Vec<String>,

    /// Additional input files that trigger rebuilds when changed
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
                extensions: Some(default_python_extensions()),
                exclude_dirs: Some(default_python_exclude_dirs()),
            },
        }
    }
}

fn default_cc_exclude_dirs() -> Vec<String> {
    vec![
        "/.git/".to_string(), "/out/".to_string(), "/build/".to_string(), "/dist/".to_string(),
    ]
}

fn default_cc_extensions() -> Vec<String> {
    vec![".c".to_string(), ".cc".to_string()]
}

fn default_cc_scan_dir() -> String {
    "src".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CpplintConfig {
    /// The C/C++ static checker to use (default: cppcheck)
    #[serde(default = "default_cpplint_checker")]
    pub checker: String,

    /// Arguments to pass to the checker
    #[serde(default = "default_cpplint_args")]
    pub args: Vec<String>,

    /// Additional input files that trigger rebuilds when changed
    #[serde(default)]
    pub extra_inputs: Vec<String>,

    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_cpplint_checker() -> String {
    "cppcheck".to_string()
}

fn default_cpplint_args() -> Vec<String> {
    vec![
        "--error-exitcode=1".to_string(),
        "--enable=warning,style,performance,portability".to_string(),
    ]
}

impl Default for CpplintConfig {
    fn default() -> Self {
        Self {
            checker: default_cpplint_checker(),
            args: default_cpplint_args(),
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: Some(default_cc_scan_dir()),
                extensions: Some(default_cc_extensions()),
                exclude_dirs: Some(default_cc_exclude_dirs()),
            },
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CcConfig {
    /// C compiler (default: gcc)
    #[serde(default = "default_cc")]
    pub cc: String,

    /// C++ compiler (default: g++)
    #[serde(default = "default_cxx")]
    pub cxx: String,

    /// C compiler flags
    #[serde(default)]
    pub cflags: Vec<String>,

    /// C++ compiler flags
    #[serde(default)]
    pub cxxflags: Vec<String>,

    /// Linker flags
    #[serde(default)]
    pub ldflags: Vec<String>,

    /// Additional include paths (passed as -I flags)
    #[serde(default)]
    pub include_paths: Vec<String>,

    /// Suffix for output executables (default: .elf)
    #[serde(default = "default_output_suffix")]
    pub output_suffix: String,

    /// Additional input files that trigger rebuilds when changed
    #[serde(default)]
    pub extra_inputs: Vec<String>,

    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_cc() -> String {
    "gcc".to_string()
}

fn default_cxx() -> String {
    "g++".to_string()
}

fn default_output_suffix() -> String {
    ".elf".to_string()
}

impl Default for CcConfig {
    fn default() -> Self {
        Self {
            cc: default_cc(),
            cxx: default_cxx(),
            cflags: Vec::new(),
            cxxflags: Vec::new(),
            ldflags: Vec::new(),
            include_paths: Vec::new(),
            output_suffix: default_output_suffix(),
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: Some(default_cc_scan_dir()),
                extensions: Some(default_cc_extensions()),
                exclude_dirs: None,
            },
        }
    }
}

fn default_spellcheck_exclude_dirs() -> Vec<String> {
    vec![
        "/.git/".to_string(), "/out/".to_string(), "/.rsb/".to_string(),
        "/node_modules/".to_string(), "/build/".to_string(), "/dist/".to_string(), "/target/".to_string(),
    ]
}

fn default_spellcheck_extensions() -> Vec<String> {
    vec![".md".to_string()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SpellcheckConfig {
    /// Hunspell dictionary language (default: "en_US")
    #[serde(default = "default_spellcheck_language")]
    pub language: String,

    /// Path to custom words file (default: ".spellcheck-words", set to "" to disable)
    #[serde(default = "default_spellcheck_words_file")]
    pub words_file: String,

    /// Enable custom words file (default: false)
    #[serde(default)]
    pub use_words_file: bool,

    /// Additional input files that trigger rebuilds when changed
    #[serde(default)]
    pub extra_inputs: Vec<String>,

    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_spellcheck_language() -> String {
    "en_US".to_string()
}

fn default_spellcheck_words_file() -> String {
    ".spellcheck-words".to_string()
}

impl Default for SpellcheckConfig {
    fn default() -> Self {
        Self {
            language: default_spellcheck_language(),
            words_file: default_spellcheck_words_file(),
            use_words_file: false,
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: None,
                extensions: Some(default_spellcheck_extensions()),
                exclude_dirs: Some(default_spellcheck_exclude_dirs()),
            },
        }
    }
}

fn default_sleep_scan_dir() -> String {
    "sleep".to_string()
}

fn default_sleep_extensions() -> Vec<String> {
    vec![".sleep".to_string()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SleepConfig {
    /// Additional input files that trigger rebuilds when changed
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
                scan_dir: Some(default_sleep_scan_dir()),
                extensions: Some(default_sleep_extensions()),
                exclude_dirs: None,
            },
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct GraphConfig {
    /// Command to open graph files (default: platform-specific)
    #[serde(default)]
    pub viewer: Option<String>,
}

impl Config {
    /// Check that rsb.toml exists in the given directory
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

    /// Load configuration from rsb.toml in the given directory
    pub fn load(project_root: &Path) -> Result<Self> {
        let config_path = project_root.join(CONFIG_FILE);

        let mut config = if config_path.exists() {
            let content = fs::read_to_string(&config_path)
                .context(format!("Failed to read config file: {}", config_path.display()))?;
            toml::from_str(&content)
                .context(format!("Failed to parse config file: {}", config_path.display()))?
        } else {
            // Return default config if no config file exists
            Config::default()
        };
        config.processor.resolve_scan_defaults();
        Ok(config)
    }
}

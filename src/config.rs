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

#[derive(Debug, Deserialize, Serialize, Default)]
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
    pub cc: CcConfig,
    #[serde(default)]
    pub cpplint: CpplintConfig,
    #[serde(default)]
    pub spellcheck: SpellcheckConfig,
    #[serde(default)]
    pub sleep: SleepConfig,
}

fn default_processors() -> Vec<String> {
    vec!["template".to_string(), "ruff".to_string(), "pylint".to_string(), "sleep".to_string(), "cc".to_string(), "cpplint".to_string(), "spellcheck".to_string()]
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            enabled: default_processors(),
            template: TemplateConfig::default(),
            ruff: RuffConfig::default(),
            pylint: PylintConfig::default(),
            cc: CcConfig::default(),
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
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TemplateConfig {
    /// Fail on undefined variables (default: true)
    #[serde(default = "default_true")]
    pub strict: bool,

    /// File extensions to process (default: [".tera"])
    #[serde(default = "default_template_extensions")]
    pub extensions: Vec<String>,

    /// Remove first newline after block tags (default: false)
    #[serde(default)]
    pub trim_blocks: bool,

    /// Additional input files that trigger rebuilds when changed
    #[serde(default)]
    pub extra_inputs: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_template_extensions() -> Vec<String> {
    vec![".tera".to_string()]
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            strict: default_true(),
            extensions: default_template_extensions(),
            trim_blocks: false,
            extra_inputs: Vec::new(),
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
}

impl Default for PylintConfig {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            extra_inputs: Vec::new(),
        }
    }
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

    /// Source directory (default: src)
    #[serde(default = "default_source_dir")]
    pub source_dir: String,

    /// Suffix for output executables (default: .elf)
    #[serde(default = "default_output_suffix")]
    pub output_suffix: String,

    /// Additional input files that trigger rebuilds when changed
    #[serde(default)]
    pub extra_inputs: Vec<String>,
}

fn default_cc() -> String {
    "gcc".to_string()
}

fn default_cxx() -> String {
    "g++".to_string()
}

fn default_source_dir() -> String {
    "src".to_string()
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
            source_dir: default_source_dir(),
            output_suffix: default_output_suffix(),
            extra_inputs: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SpellcheckConfig {
    /// File extensions to check (default: [".md"])
    #[serde(default = "default_spellcheck_extensions")]
    pub extensions: Vec<String>,

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
}

fn default_spellcheck_extensions() -> Vec<String> {
    vec![".md".to_string()]
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
            extensions: default_spellcheck_extensions(),
            language: default_spellcheck_language(),
            words_file: default_spellcheck_words_file(),
            use_words_file: false,
            extra_inputs: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SleepConfig {
    /// Additional input files that trigger rebuilds when changed
    #[serde(default)]
    pub extra_inputs: Vec<String>,
}

impl Default for SleepConfig {
    fn default() -> Self {
        Self {
            extra_inputs: Vec::new(),
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
    /// Load configuration from rsb.toml in the given directory
    pub fn load(project_root: &Path) -> Result<Self> {
        let config_path = project_root.join(CONFIG_FILE);

        if config_path.exists() {
            let content = fs::read_to_string(&config_path)
                .context(format!("Failed to read config file: {}", config_path.display()))?;
            let config: Config = toml::from_str(&content)
                .context(format!("Failed to parse config file: {}", config_path.display()))?;
            Ok(config)
        } else {
            // Return default config if no config file exists
            Ok(Config::default())
        }
    }
}

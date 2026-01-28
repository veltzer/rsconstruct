use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

const CONFIG_FILE: &str = "rsb.toml";

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub build: BuildConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub processors: ProcessorsConfig,
    #[serde(default)]
    pub template: TemplateConfig,
    #[serde(default)]
    pub lint: LintConfig,
    #[serde(default)]
    pub completions: CompletionsConfig,
    #[serde(default)]
    pub cc: CcConfig,
}

#[derive(Debug, Deserialize, Clone)]
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
#[derive(Debug, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RestoreMethod {
    /// Use hard links (fast, no disk space duplication)
    #[default]
    Hardlink,
    /// Use file copy (works across filesystems, safer)
    Copy,
}

#[derive(Debug, Deserialize, Clone)]
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

#[derive(Debug, Deserialize, Clone)]
pub struct ProcessorsConfig {
    /// List of enabled processors (e.g., ["template", "lint"])
    #[serde(default = "default_processors")]
    pub enabled: Vec<String>,
}

fn default_processors() -> Vec<String> {
    vec!["template".to_string(), "lint".to_string(), "sleep".to_string(), "cc".to_string()]
}

impl Default for ProcessorsConfig {
    fn default() -> Self {
        Self {
            enabled: default_processors(),
        }
    }
}

impl ProcessorsConfig {
    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled.iter().any(|p| p == name)
    }
}

#[derive(Debug, Deserialize, Clone)]
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
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
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

#[derive(Debug, Deserialize, Clone)]
pub struct LintConfig {
    /// The Python linter to use (ruff, pylint, flake8, etc.)
    #[serde(default = "default_linter")]
    pub linter: String,

    /// Additional arguments to pass to the linter
    #[serde(default)]
    pub args: Vec<String>,
}

fn default_linter() -> String {
    "ruff".to_string()
}

impl Default for LintConfig {
    fn default() -> Self {
        Self {
            linter: default_linter(),
            args: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
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
        }
    }
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

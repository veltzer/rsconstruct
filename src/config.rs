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
    pub processors: ProcessorsConfig,
    #[serde(default)]
    pub lint: LintConfig,
    #[serde(default)]
    pub completions: CompletionsConfig,
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

#[derive(Debug, Deserialize, Clone)]
pub struct ProcessorsConfig {
    /// List of enabled processors (e.g., ["template", "lint"])
    #[serde(default = "default_processors")]
    pub enabled: Vec<String>,
}

fn default_processors() -> Vec<String> {
    vec!["template".to_string(), "lint".to_string(), "sleep".to_string()]
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

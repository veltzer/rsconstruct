use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_FILE: &str = "rsb.toml";

/// Convert a toml::Value to its inline TOML string representation.
/// This is used for variable substitution to insert values into the config.
fn value_to_toml_inline(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(value_to_toml_inline).collect();
            format!("[{}]", items.join(", "))
        }
        toml::Value::Table(table) => {
            let items: Vec<String> = table
                .iter()
                .map(|(k, v)| format!("{} = {}", k, value_to_toml_inline(v)))
                .collect();
            format!("{{ {} }}", items.join(", "))
        }
        toml::Value::Datetime(dt) => dt.to_string(),
    }
}

/// Remove the [vars] section from TOML content.
/// Removes from [vars] header until the next section header or EOF.
fn remove_vars_section(content: &str) -> String {
    let mut result = String::new();
    let mut in_vars_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[vars]" {
            in_vars_section = true;
            continue;
        }
        // Check if we hit another section header
        if in_vars_section && trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_vars_section = false;
        }
        if !in_vars_section {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

/// Extract variable names defined in the [vars] section using regex.
/// This is done before TOML parsing to avoid parse errors on variable references.
fn extract_var_names(content: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_vars_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[vars]" {
            in_vars_section = true;
            continue;
        }
        // Check if we hit another section header
        if in_vars_section && trimmed.starts_with('[') && trimmed.ends_with(']') {
            break;
        }
        if in_vars_section {
            // Match key = value pattern
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                if !key.is_empty() && !key.starts_with('#') {
                    names.push(key.to_string());
                }
            }
        }
    }
    names
}

/// Substitute variables defined in [vars] section throughout the config.
/// Variables are referenced using `${var_name}` syntax.
/// The entire `"${var_name}"` (including quotes) is replaced with the TOML-serialized value.
/// After substitution, the [vars] section is removed from the output.
fn substitute_variables(content: &str) -> Result<String> {
    // Check for undefined variables first (before any TOML parsing)
    // This gives a clear error message for undefined vars even without a [vars] section
    let var_pattern = Regex::new(r#""\$\{([^}]+)\}""#).unwrap();

    // Extract defined variable names before TOML parsing
    let defined_vars = extract_var_names(content);

    // Check for undefined variable references
    for captures in var_pattern.captures_iter(content) {
        let var_name = captures.get(1).unwrap().as_str();
        if !defined_vars.contains(&var_name.to_string()) {
            anyhow::bail!("Undefined variable: ${{{}}}", var_name);
        }
    }

    // If no vars defined, return content as-is (we already checked for undefined refs above)
    if defined_vars.is_empty() {
        return Ok(content.to_string());
    }

    // Parse just to extract [vars] section values
    let parsed: toml::Value = toml::from_str(content)
        .context("Failed to parse TOML for variable extraction")?;

    let vars = match parsed.get("vars").and_then(|v| v.as_table()) {
        Some(v) => v,
        None => return Ok(content.to_string()),
    };

    let mut result = content.to_string();

    // Replace "${var_name}" (including quotes) with TOML-serialized value
    for (name, value) in vars {
        let pattern = format!("\"${{{}}}\"", name);
        let replacement = value_to_toml_inline(value);
        result = result.replace(&pattern, &replacement);
    }

    // Remove the [vars] section from the result
    let result = remove_vars_section(&result);

    Ok(result)
}

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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
        "tera".into(), "ruff".into(), "pylint".into(),
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
    pub tera: TeraConfig,
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
            tera: TeraConfig::default(),
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
        self.tera.scan.resolve("templates", &[".tera"], &[]);
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
pub struct TeraConfig {
    #[serde(default = "default_true")]
    pub strict: bool,
    #[serde(default)]
    pub trim_blocks: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for TeraConfig {
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

/// Configuration for a single compiler profile
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CompilerProfile {
    /// Profile name (used in output paths, e.g., "gcc", "clang")
    pub name: String,
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
    #[serde(default = "default_output_suffix")]
    pub output_suffix: String,
}

impl CompilerProfile {
    /// Create a default GCC profile
    #[allow(dead_code)]
    pub fn default_gcc() -> Self {
        Self {
            name: "gcc".into(),
            cc: "gcc".into(),
            cxx: "g++".into(),
            cflags: Vec::new(),
            cxxflags: Vec::new(),
            ldflags: Vec::new(),
            output_suffix: ".elf".into(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CcConfig {
    /// Legacy single-compiler fields (used when `compilers` is empty)
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
    #[serde(default = "default_output_suffix")]
    pub output_suffix: String,

    /// Multiple compiler profiles (if set, overrides legacy fields)
    #[serde(default)]
    pub compilers: Vec<CompilerProfile>,

    /// Shared settings across all compilers
    #[serde(default)]
    pub include_paths: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    /// Method for scanning header dependencies (native or compiler)
    #[serde(default)]
    pub include_scanner: IncludeScanner,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl CcConfig {
    /// Get the list of compiler profiles to use.
    /// If `compilers` is set, returns those profiles.
    /// Otherwise, creates a single profile from the legacy fields.
    pub fn get_compiler_profiles(&self) -> Vec<CompilerProfile> {
        if !self.compilers.is_empty() {
            self.compilers.clone()
        } else {
            // Legacy mode: create single profile from top-level fields
            vec![CompilerProfile {
                name: String::new(), // Empty name = no subdirectory
                cc: self.cc.clone(),
                cxx: self.cxx.clone(),
                cflags: self.cflags.clone(),
                cxxflags: self.cxxflags.clone(),
                ldflags: self.ldflags.clone(),
                output_suffix: self.output_suffix.clone(),
            }]
        }
    }
}

impl Default for CcConfig {
    fn default() -> Self {
        Self {
            cc: "gcc".into(),
            cxx: "g++".into(),
            cflags: Vec::new(),
            cxxflags: Vec::new(),
            ldflags: Vec::new(),
            output_suffix: ".elf".into(),
            compilers: Vec::new(),
            include_paths: Vec::new(),
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CppAnalyzerConfig {
    /// Method for scanning header dependencies (native or compiler)
    #[serde(default)]
    pub include_scanner: IncludeScanner,
    /// Additional include paths for header search
    #[serde(default)]
    pub include_paths: Vec<String>,
    /// pkg-config packages to query for include paths
    #[serde(default)]
    pub pkg_config: Vec<String>,
    /// Commands that output include paths (e.g., ["gcc -print-file-name=plugin"])
    /// Each command is run and its stdout is added to the include search paths.
    #[serde(default)]
    pub include_path_commands: Vec<String>,
    /// Directory path segments to exclude from analysis (e.g., ["/kernel/", "/vendor/"])
    #[serde(default)]
    pub exclude_dirs: Vec<String>,
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

impl Default for CppAnalyzerConfig {
    fn default() -> Self {
        Self {
            include_scanner: IncludeScanner::default(),
            include_paths: Vec::new(),
            pkg_config: Vec::new(),
            include_path_commands: Vec::new(),
            exclude_dirs: Vec::new(),
            cc: default_cc_compiler(),
            cxx: default_cxx_compiler(),
            cflags: Vec::new(),
            cxxflags: Vec::new(),
        }
    }
}

/// Configuration for the Python dependency analyzer
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
            let substituted = substitute_variables(&content)
                .context(format!("Failed to substitute variables in: {}", config_path.display()))?;
            toml::from_str(&substituted)
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

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for value_to_toml_inline

    #[test]
    fn value_to_toml_inline_string() {
        let value = toml::Value::String("hello".into());
        assert_eq!(value_to_toml_inline(&value), "\"hello\"");
    }

    #[test]
    fn value_to_toml_inline_string_with_quotes() {
        let value = toml::Value::String("say \"hello\"".into());
        assert_eq!(value_to_toml_inline(&value), "\"say \\\"hello\\\"\"");
    }

    #[test]
    fn value_to_toml_inline_string_with_backslash() {
        let value = toml::Value::String("path\\to\\file".into());
        assert_eq!(value_to_toml_inline(&value), "\"path\\\\to\\\\file\"");
    }

    #[test]
    fn value_to_toml_inline_integer() {
        let value = toml::Value::Integer(42);
        assert_eq!(value_to_toml_inline(&value), "42");
    }

    #[test]
    fn value_to_toml_inline_negative_integer() {
        let value = toml::Value::Integer(-123);
        assert_eq!(value_to_toml_inline(&value), "-123");
    }

    #[test]
    fn value_to_toml_inline_float() {
        let value = toml::Value::Float(3.14);
        assert_eq!(value_to_toml_inline(&value), "3.14");
    }

    #[test]
    fn value_to_toml_inline_boolean_true() {
        let value = toml::Value::Boolean(true);
        assert_eq!(value_to_toml_inline(&value), "true");
    }

    #[test]
    fn value_to_toml_inline_boolean_false() {
        let value = toml::Value::Boolean(false);
        assert_eq!(value_to_toml_inline(&value), "false");
    }

    #[test]
    fn value_to_toml_inline_array_of_strings() {
        let value = toml::Value::Array(vec![
            toml::Value::String("a".into()),
            toml::Value::String("b".into()),
            toml::Value::String("c".into()),
        ]);
        assert_eq!(value_to_toml_inline(&value), "[\"a\", \"b\", \"c\"]");
    }

    #[test]
    fn value_to_toml_inline_array_of_integers() {
        let value = toml::Value::Array(vec![
            toml::Value::Integer(1),
            toml::Value::Integer(2),
            toml::Value::Integer(3),
        ]);
        assert_eq!(value_to_toml_inline(&value), "[1, 2, 3]");
    }

    #[test]
    fn value_to_toml_inline_empty_array() {
        let value = toml::Value::Array(vec![]);
        assert_eq!(value_to_toml_inline(&value), "[]");
    }

    #[test]
    fn value_to_toml_inline_table() {
        let mut table = toml::map::Map::new();
        table.insert("key".into(), toml::Value::String("value".into()));
        let value = toml::Value::Table(table);
        assert_eq!(value_to_toml_inline(&value), "{ key = \"value\" }");
    }

    // Tests for remove_vars_section

    #[test]
    fn remove_vars_section_basic() {
        let content = "[vars]\nfoo = \"bar\"\n\n[other]\nkey = \"value\"\n";
        let result = remove_vars_section(content);
        assert!(!result.contains("[vars]"));
        assert!(!result.contains("foo = \"bar\""));
        assert!(result.contains("[other]"));
        assert!(result.contains("key = \"value\""));
    }

    #[test]
    fn remove_vars_section_at_end() {
        let content = "[other]\nkey = \"value\"\n\n[vars]\nfoo = \"bar\"\n";
        let result = remove_vars_section(content);
        assert!(!result.contains("[vars]"));
        assert!(!result.contains("foo = \"bar\""));
        assert!(result.contains("[other]"));
        assert!(result.contains("key = \"value\""));
    }

    #[test]
    fn remove_vars_section_no_vars() {
        let content = "[other]\nkey = \"value\"\n";
        let result = remove_vars_section(content);
        assert_eq!(result, "[other]\nkey = \"value\"\n");
    }

    #[test]
    fn remove_vars_section_multiple_vars() {
        let content = "[vars]\nfoo = \"bar\"\nbaz = [1, 2, 3]\n\n[other]\nkey = \"value\"\n";
        let result = remove_vars_section(content);
        assert!(!result.contains("[vars]"));
        assert!(!result.contains("foo = \"bar\""));
        assert!(!result.contains("baz = [1, 2, 3]"));
        assert!(result.contains("[other]"));
    }

    // Tests for extract_var_names

    #[test]
    fn extract_var_names_basic() {
        let content = "[vars]\nfoo = \"bar\"\nbaz = [1, 2]\n\n[other]\nkey = \"value\"\n";
        let names = extract_var_names(content);
        assert_eq!(names, vec!["foo", "baz"]);
    }

    #[test]
    fn extract_var_names_no_vars_section() {
        let content = "[other]\nkey = \"value\"\n";
        let names = extract_var_names(content);
        assert!(names.is_empty());
    }

    #[test]
    fn extract_var_names_empty_vars_section() {
        let content = "[vars]\n\n[other]\nkey = \"value\"\n";
        let names = extract_var_names(content);
        assert!(names.is_empty());
    }

    #[test]
    fn extract_var_names_with_comments() {
        let content = "[vars]\n# This is a comment\nfoo = \"bar\"\n# Another comment\nbaz = 42\n";
        let names = extract_var_names(content);
        assert_eq!(names, vec!["foo", "baz"]);
    }

    #[test]
    fn extract_var_names_with_whitespace() {
        let content = "[vars]\n  foo   =   \"bar\"\n\tbaz\t=\t42\n";
        let names = extract_var_names(content);
        assert_eq!(names, vec!["foo", "baz"]);
    }

    // Tests for substitute_variables

    #[test]
    fn substitute_variables_string() {
        let content = "[vars]\nmy_dir = \"templates\"\n\n[processor]\nscan_dir = \"${my_dir}\"\n";
        let result = substitute_variables(content).unwrap();
        assert!(result.contains("scan_dir = \"templates\""));
        assert!(!result.contains("${my_dir}"));
        assert!(!result.contains("[vars]"));
    }

    #[test]
    fn substitute_variables_array() {
        let content = "[vars]\nexcludes = [\"/a/\", \"/b/\"]\n\n[processor]\nexclude_dirs = \"${excludes}\"\n";
        let result = substitute_variables(content).unwrap();
        assert!(result.contains("exclude_dirs = [\"/a/\", \"/b/\"]"));
        assert!(!result.contains("${excludes}"));
    }

    #[test]
    fn substitute_variables_multiple_uses() {
        let content = "[vars]\nval = \"shared\"\n\n[a]\nx = \"${val}\"\n\n[b]\ny = \"${val}\"\n";
        let result = substitute_variables(content).unwrap();
        assert!(result.contains("x = \"shared\""));
        assert!(result.contains("y = \"shared\""));
    }

    #[test]
    fn substitute_variables_no_vars_section() {
        let content = "[processor]\nscan_dir = \"src\"\n";
        let result = substitute_variables(content).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn substitute_variables_undefined_error() {
        let content = "[processor]\nscan_dir = \"${undefined}\"\n";
        let result = substitute_variables(content);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Undefined variable"));
        assert!(err.contains("undefined"));
    }

    #[test]
    fn substitute_variables_undefined_with_vars_section() {
        let content = "[vars]\nfoo = \"bar\"\n\n[processor]\nx = \"${foo}\"\ny = \"${missing}\"\n";
        let result = substitute_variables(content);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("missing"));
    }

    #[test]
    fn substitute_variables_integer() {
        let content = "[vars]\ncount = 42\n\n[processor]\nvalue = \"${count}\"\n";
        let result = substitute_variables(content).unwrap();
        assert!(result.contains("value = 42"));
    }

    #[test]
    fn substitute_variables_boolean() {
        let content = "[vars]\nenabled = true\n\n[processor]\nflag = \"${enabled}\"\n";
        let result = substitute_variables(content).unwrap();
        assert!(result.contains("flag = true"));
    }
}

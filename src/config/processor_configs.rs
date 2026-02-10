use serde::{Deserialize, Serialize};

use super::{default_true, default_cc_compiler, default_cxx_compiler, default_output_suffix, ScanConfig};

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

fn default_cppcheck_args() -> Vec<String> {
    vec![
        "--error-exitcode=1".into(),
        "--enable=warning,style,performance,portability".into(),
    ]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CppcheckConfig {
    #[serde(default = "default_cppcheck_args")]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for CppcheckConfig {
    fn default() -> Self {
        Self {
            args: default_cppcheck_args(),
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClangTidyConfig {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub compiler_args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for ClangTidyConfig {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            compiler_args: Vec::new(),
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
    /// Whether this profile is enabled (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
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
    /// Get the list of enabled compiler profiles to use.
    /// If `compilers` is set, returns enabled profiles from that list.
    /// Otherwise, creates a single profile from the legacy fields.
    pub fn get_compiler_profiles(&self) -> Vec<CompilerProfile> {
        if !self.compilers.is_empty() {
            self.compilers.iter().filter(|p| p.enabled).cloned().collect()
        } else {
            // Legacy mode: create single profile from top-level fields
            vec![CompilerProfile {
                name: String::new(), // Empty name = no subdirectory
                enabled: true,
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
    /// When true, automatically add misspelled words to words_file instead of failing
    #[serde(default)]
    pub auto_add_words: bool,
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
            auto_add_words: false,
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

fn default_cargo() -> String {
    "cargo".into()
}

fn default_cargo_command() -> String {
    "build".into()
}

pub(super) const CARGO_EXCLUDE_DIRS: &[&str] = &["/.git/", "/target/", "/.rsb/", "/out/"];

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CargoConfig {
    #[serde(default = "default_cargo")]
    pub cargo: String,
    #[serde(default = "default_cargo_command")]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for CargoConfig {
    fn default() -> Self {
        Self {
            cargo: "cargo".into(),
            command: "build".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: None,
                extensions: Some(vec!["Cargo.toml".into()]),
                exclude_dirs: Some(CARGO_EXCLUDE_DIRS.iter().map(|s| s.to_string()).collect()),
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
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

fn default_mypy_checker() -> String {
    "mypy".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MypyConfig {
    #[serde(default = "default_mypy_checker")]
    pub checker: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MypyConfig {
    fn default() -> Self {
        Self {
            checker: "mypy".into(),
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

fn default_rumdl_linter() -> String {
    "rumdl".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RumdlConfig {
    #[serde(default = "default_rumdl_linter")]
    pub linter: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for RumdlConfig {
    fn default() -> Self {
        Self {
            linter: "rumdl".into(),
            args: Vec::new(),
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

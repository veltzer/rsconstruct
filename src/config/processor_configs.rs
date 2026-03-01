use serde::{Deserialize, Serialize};

use super::{default_true, default_cc_compiler, default_cxx_compiler, default_output_suffix, KnownFields, ScanConfig};

/// Generate a simple checker config struct with args, extra_inputs, auto_inputs, and scan fields.
///
/// Generates the struct with serde `Deserialize`/`Serialize`/`Clone` derives and
/// a `Default` impl with the specified scan settings.
///
/// Variants:
/// - `checker_config!(Name, extensions: [".py"])` — default scan from project root
/// - `checker_config!(Name, scan_dir: "src", extensions: [".c"])` — scan in subdirectory
macro_rules! checker_config {
    ($name:ident, extensions: [$($ext:expr),+ $(,)?]) => {
        #[derive(Debug, Deserialize, Serialize, Clone)]
        pub struct $name {
            #[serde(default = "default_true")]
            pub enabled: bool,
            #[serde(default)]
            pub args: Vec<String>,
            #[serde(default)]
            pub extra_inputs: Vec<String>,
            #[serde(default)]
            pub auto_inputs: Vec<String>,
            #[serde(flatten)]
            pub scan: ScanConfig,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    enabled: true,
                    args: Vec::new(),
                    extra_inputs: Vec::new(),
                    auto_inputs: Vec::new(),
                    scan: default_scan!(extensions: [$($ext),+]),
                }
            }
        }

        impl KnownFields for $name {
            fn known_fields() -> &'static [&'static str] {
                static FIELDS: &[&str] = &[
                    "enabled", "args", "extra_inputs", "auto_inputs",
                    "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
                ];
                FIELDS
            }
        }
    };
    ($name:ident, scan_dir: $dir:expr, extensions: [$($ext:expr),+ $(,)?]) => {
        #[derive(Debug, Deserialize, Serialize, Clone)]
        pub struct $name {
            #[serde(default = "default_true")]
            pub enabled: bool,
            #[serde(default)]
            pub args: Vec<String>,
            #[serde(default)]
            pub extra_inputs: Vec<String>,
            #[serde(default)]
            pub auto_inputs: Vec<String>,
            #[serde(flatten)]
            pub scan: ScanConfig,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    enabled: true,
                    args: Vec::new(),
                    extra_inputs: Vec::new(),
                    auto_inputs: Vec::new(),
                    scan: default_scan!(scan_dir: $dir, extensions: [$($ext),+]),
                }
            }
        }

        impl KnownFields for $name {
            fn known_fields() -> &'static [&'static str] {
                static FIELDS: &[&str] = &[
                    "enabled", "args", "extra_inputs", "auto_inputs",
                    "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
                ];
                FIELDS
            }
        }
    };
}

/// Create a default ScanConfig with optional scan_dir and required extensions.
/// All exclude fields default to None (filled by resolve_scan_defaults).
macro_rules! default_scan {
    (extensions: [$($ext:expr),+ $(,)?]) => {
        ScanConfig {
            scan_dir: None,
            extensions: Some(vec![$($ext.into()),+]),
            exclude_dirs: None,
            exclude_files: None,
            exclude_paths: None,
        }
    };
    (scan_dir: $dir:expr, extensions: [$($ext:expr),+ $(,)?]) => {
        ScanConfig {
            scan_dir: Some($dir.into()),
            extensions: Some(vec![$($ext.into()),+]),
            exclude_dirs: None,
            exclude_files: None,
            exclude_paths: None,
        }
    };
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TeraConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub strict: bool,
    #[serde(default)]
    pub trim_blocks: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for TeraConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strict: true,
            trim_blocks: false,
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            scan: default_scan!(scan_dir: "templates.tera", extensions: [".tera"]),
        }
    }
}

impl KnownFields for TeraConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "strict", "trim_blocks", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MakoConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MakoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            scan: default_scan!(scan_dir: "templates.mako", extensions: [".mako"]),
        }
    }
}

impl KnownFields for MakoConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_ruff_linter() -> String {
    "ruff".into()
}

fn default_ruff_auto_inputs() -> Vec<String> {
    vec!["ruff.toml".into(), ".ruff.toml".into(), "pyproject.toml".into()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RuffConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_ruff_linter")]
    pub linter: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_ruff_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for RuffConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            linter: "ruff".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_ruff_auto_inputs(),
            scan: default_scan!(extensions: [".py"]),
        }
    }
}

impl KnownFields for RuffConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "linter", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_pylint_auto_inputs() -> Vec<String> {
    vec![".pylintrc".into()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PylintConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_pylint_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PylintConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_pylint_auto_inputs(),
            scan: default_scan!(extensions: [".py"]),
        }
    }
}

impl KnownFields for PylintConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_cppcheck_args() -> Vec<String> {
    vec![
        "--error-exitcode=1".into(),
        "--enable=warning,style,performance,portability".into(),
    ]
}

fn default_cppcheck_auto_inputs() -> Vec<String> {
    vec![".cppcheck".into()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CppcheckConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_cppcheck_args")]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_cppcheck_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for CppcheckConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            args: default_cppcheck_args(),
            extra_inputs: Vec::new(),
            auto_inputs: default_cppcheck_auto_inputs(),
            scan: default_scan!(scan_dir: "src", extensions: [".c", ".cc"]),
        }
    }
}

impl KnownFields for CppcheckConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClangTidyConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub compiler_args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_clang_tidy_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_clang_tidy_auto_inputs() -> Vec<String> {
    vec![".clang-tidy".into()]
}

impl Default for ClangTidyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            args: Vec::new(),
            compiler_args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_clang_tidy_auto_inputs(),
            scan: default_scan!(scan_dir: "src", extensions: [".c", ".cc"]),
        }
    }
}

impl KnownFields for ClangTidyConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "args", "compiler_args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
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
pub struct CcSingleFileConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
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
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    /// Method for scanning header dependencies (native or compiler)
    #[serde(default)]
    pub include_scanner: IncludeScanner,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl CcSingleFileConfig {
    /// Get the list of enabled compiler profiles to use.
    /// If `compilers` is set, returns enabled profiles from that list.
    /// Otherwise, creates a single profile from the legacy fields.
    pub(crate) fn get_compiler_profiles(&self) -> Vec<CompilerProfile> {
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

impl Default for CcSingleFileConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cc: "gcc".into(),
            cxx: "g++".into(),
            cflags: Vec::new(),
            cxxflags: Vec::new(),
            ldflags: Vec::new(),
            output_suffix: ".elf".into(),
            compilers: Vec::new(),
            include_paths: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            include_scanner: IncludeScanner::default(),
            scan: default_scan!(scan_dir: "src", extensions: [".c", ".cc"]),
        }
    }
}

impl KnownFields for CcSingleFileConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "cc", "cxx", "cflags", "cxxflags", "ldflags", "output_suffix",
            "compilers", "include_paths", "extra_inputs", "auto_inputs", "include_scanner",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
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
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_spellcheck_language")]
    pub language: String,
    #[serde(default = "default_spellcheck_words_file")]
    pub words_file: String,
    /// When true, automatically add misspelled words to words_file instead of failing
    #[serde(default)]
    pub auto_add_words: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_spellcheck_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_spellcheck_auto_inputs() -> Vec<String> {
    vec![".spellcheck-words".into()]
}

impl Default for SpellcheckConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            language: "en_US".into(),
            words_file: ".spellcheck-words".into(),

            auto_add_words: false,
            extra_inputs: Vec::new(),
            auto_inputs: default_spellcheck_auto_inputs(),
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for SpellcheckConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "language", "words_file", "auto_add_words", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SleepConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for SleepConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            scan: default_scan!(scan_dir: "sleep", extensions: [".sleep"]),
        }
    }
}

impl KnownFields for SleepConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
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

fn default_cargo_profiles() -> Vec<String> {
    vec!["dev".into(), "release".into()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CargoConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_cargo")]
    pub cargo: String,
    #[serde(default = "default_cargo_command")]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_cargo_profiles")]
    pub profiles: Vec<String>,
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for CargoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cargo: "cargo".into(),
            command: "build".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            profiles: default_cargo_profiles(),
            cache_output_dir: true,
            scan: default_scan!(extensions: ["Cargo.toml"]),
        }
    }
}

impl KnownFields for CargoConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "cargo", "command", "args", "extra_inputs", "auto_inputs", "profiles",
            "cache_output_dir", "scan_dir", "extensions", "exclude_dirs", "exclude_files",
            "exclude_paths",
        ]
    }
}

fn default_clippy_command() -> String {
    "clippy".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClippyConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_cargo")]
    pub cargo: String,
    #[serde(default = "default_clippy_command")]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for ClippyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cargo: "cargo".into(),
            command: "clippy".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            scan: default_scan!(extensions: ["Cargo.toml"]),
        }
    }
}

impl KnownFields for ClippyConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "cargo", "command", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MakeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_make")]
    pub make: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MakeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            make: "make".into(),
            args: Vec::new(),
            target: String::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            scan: default_scan!(extensions: ["Makefile"]),
        }
    }
}

impl KnownFields for MakeConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "make", "args", "target", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_mypy_checker() -> String {
    "mypy".into()
}

fn default_mypy_auto_inputs() -> Vec<String> {
    vec!["mypy.ini".into()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MypyConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_mypy_checker")]
    pub checker: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_mypy_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MypyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            checker: "mypy".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_mypy_auto_inputs(),
            scan: default_scan!(extensions: [".py"]),
        }
    }
}

impl KnownFields for MypyConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "checker", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_pyrefly_checker() -> String {
    "pyrefly".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PyreflyConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_pyrefly_checker")]
    pub checker: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_pyrefly_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_pyrefly_auto_inputs() -> Vec<String> {
    vec!["pyproject.toml".into()]
}

impl Default for PyreflyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            checker: "pyrefly".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_pyrefly_auto_inputs(),
            scan: default_scan!(extensions: [".py"]),
        }
    }
}

impl KnownFields for PyreflyConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "checker", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_rumdl_linter() -> String {
    "rumdl".into()
}

fn default_rumdl_auto_inputs() -> Vec<String> {
    vec![".rumdl.toml".into()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RumdlConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_rumdl_linter")]
    pub linter: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_rumdl_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for RumdlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            linter: "rumdl".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_rumdl_auto_inputs(),
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for RumdlConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "linter", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_yamllint_linter() -> String {
    "yamllint".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct YamllintConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_yamllint_linter")]
    pub linter: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_yamllint_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_yamllint_auto_inputs() -> Vec<String> {
    vec![".yamllint".into(), ".yamllint.yml".into(), ".yamllint.yaml".into()]
}

impl Default for YamllintConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            linter: "yamllint".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_yamllint_auto_inputs(),
            scan: default_scan!(extensions: [".yml", ".yaml"]),
        }
    }
}

impl KnownFields for YamllintConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "linter", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_jq_checker() -> String {
    "jq".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JqConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_jq_checker")]
    pub checker: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for JqConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            checker: "jq".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            scan: default_scan!(extensions: [".json"]),
        }
    }
}

impl KnownFields for JqConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "checker", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_jsonlint_linter() -> String {
    "jsonlint".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JsonlintConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_jsonlint_linter")]
    pub linter: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for JsonlintConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            linter: "jsonlint".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            scan: default_scan!(extensions: [".json"]),
        }
    }
}

impl KnownFields for JsonlintConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "linter", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_taplo_linter() -> String {
    "taplo".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TaploConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_taplo_linter")]
    pub linter: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_taplo_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_taplo_auto_inputs() -> Vec<String> {
    vec!["taplo.toml".into(), ".taplo.toml".into()]
}

impl Default for TaploConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            linter: "taplo".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_taplo_auto_inputs(),
            scan: default_scan!(extensions: [".toml"]),
        }
    }
}

impl KnownFields for TaploConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "linter", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

checker_config!(JsonSchemaConfig, extensions: [".json"]);

fn default_tags_output() -> String {
    "out/tags/tags.db".into()
}

fn default_tags_file() -> String {
    ".tags".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TagsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_tags_output")]
    pub output: String,
    #[serde(default = "default_tags_file")]
    pub tags_file: String,
    /// When true, a missing .tags file is a build error
    #[serde(default)]
    pub tags_file_strict: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for TagsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            output: "out/tags/tags.db".into(),
            tags_file: ".tags".into(),
            tags_file_strict: false,
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for TagsConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "output", "tags_file", "tags_file_strict", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_shellcheck_checker() -> String {
    "shellcheck".into()
}

fn default_shellcheck_auto_inputs() -> Vec<String> {
    vec![".shellcheckrc".into()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShellcheckConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_shellcheck_checker")]
    pub checker: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_shellcheck_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for ShellcheckConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            checker: "shellcheck".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_shellcheck_auto_inputs(),
            scan: default_scan!(extensions: [".sh", ".bash"]),
        }
    }
}

impl KnownFields for ShellcheckConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "checker", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ScriptCheckConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub checker: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for ScriptCheckConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            checker: String::new(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            scan: ScanConfig {
                scan_dir: None,
                extensions: None,
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

impl KnownFields for ScriptCheckConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "checker", "args", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_pip() -> String {
    "pip".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PipConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_pip")]
    pub pip: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PipConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pip: "pip".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            cache_output_dir: true,
            scan: default_scan!(extensions: ["requirements.txt"]),
        }
    }
}

impl KnownFields for PipConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "pip", "args", "extra_inputs", "auto_inputs", "cache_output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_sphinx_build() -> String {
    "sphinx-build".into()
}

fn default_sphinx_output_dir() -> String {
    "_build".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SphinxConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_sphinx_build")]
    pub sphinx_build: String,
    #[serde(default = "default_sphinx_output_dir")]
    pub output_dir: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for SphinxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sphinx_build: "sphinx-build".into(),
            output_dir: "_build".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            cache_output_dir: true,
            scan: default_scan!(extensions: ["conf.py"]),
        }
    }
}

impl KnownFields for SphinxConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "sphinx_build", "output_dir", "args", "extra_inputs", "auto_inputs", "cache_output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_mdbook() -> String {
    "mdbook".into()
}

fn default_mdbook_output_dir() -> String {
    "book".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MdbookConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_mdbook")]
    pub mdbook: String,
    #[serde(default = "default_mdbook_output_dir")]
    pub output_dir: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MdbookConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mdbook: "mdbook".into(),
            output_dir: "book".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            cache_output_dir: true,
            scan: default_scan!(extensions: ["book.toml"]),
        }
    }
}

impl KnownFields for MdbookConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "mdbook", "output_dir", "args", "extra_inputs", "auto_inputs", "cache_output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_npm() -> String {
    "npm".into()
}

fn default_npm_command() -> String {
    "install".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct NpmConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_npm")]
    pub npm: String,
    #[serde(default = "default_npm_command")]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for NpmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            npm: "npm".into(),
            command: "install".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            cache_output_dir: true,
            scan: default_scan!(extensions: ["package.json"]),
        }
    }
}

impl KnownFields for NpmConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "npm", "command", "args", "extra_inputs", "auto_inputs", "cache_output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_mdl_bin() -> String {
    "gems/bin/mdl".into()
}

fn default_gem_home() -> String {
    "gems".into()
}

fn default_gem_stamp() -> String {
    "out/gem/root.stamp".into()
}

fn default_mdl_auto_inputs() -> Vec<String> {
    vec![".mdlrc".into(), ".mdl.style.rb".into()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MdlConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_gem_home")]
    pub gem_home: String,
    #[serde(default = "default_mdl_bin")]
    pub mdl_bin: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_mdl_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_gem_stamp")]
    pub gem_stamp: String,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MdlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            gem_home: "gems".into(),
            mdl_bin: "gems/bin/mdl".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_mdl_auto_inputs(),
            gem_stamp: "out/gem/root.stamp".into(),
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for MdlConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "gem_home", "mdl_bin", "args", "extra_inputs", "auto_inputs", "gem_stamp",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_markdownlint_bin() -> String {
    "node_modules/.bin/markdownlint".into()
}

fn default_npm_stamp() -> String {
    "out/npm/root.stamp".into()
}

fn default_markdownlint_auto_inputs() -> Vec<String> {
    vec![".markdownlint.json".into(), ".markdownlint.jsonc".into(), ".markdownlint.yaml".into()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MarkdownlintConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_markdownlint_bin")]
    pub markdownlint_bin: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_markdownlint_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_npm_stamp")]
    pub npm_stamp: String,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MarkdownlintConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            markdownlint_bin: "node_modules/.bin/markdownlint".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_markdownlint_auto_inputs(),
            npm_stamp: "out/npm/root.stamp".into(),
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for MarkdownlintConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "markdownlint_bin", "args", "extra_inputs", "auto_inputs", "npm_stamp",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_aspell() -> String {
    "aspell".into()
}

fn default_aspell_conf_dir() -> String {
    ".".into()
}

fn default_aspell_conf() -> String {
    ".aspell.conf".into()
}

fn default_aspell_words_file() -> String {
    ".aspell.en.pws".into()
}

fn default_aspell_auto_inputs() -> Vec<String> {
    vec![".aspell.conf".into(), ".aspell.en.pws".into(), ".aspell.en.prepl".into()]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AspellConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_aspell")]
    pub aspell: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_aspell_conf_dir")]
    pub conf_dir: String,
    #[serde(default = "default_aspell_conf")]
    pub conf: String,
    #[serde(default)]
    pub auto_add_words: bool,
    #[serde(default = "default_aspell_words_file")]
    pub words_file: String,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_aspell_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for AspellConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            aspell: "aspell".into(),
            args: Vec::new(),
            conf_dir: ".".into(),
            conf: ".aspell.conf".into(),
            auto_add_words: false,
            words_file: ".aspell.en.pws".into(),
            extra_inputs: Vec::new(),
            auto_inputs: default_aspell_auto_inputs(),
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for AspellConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "aspell", "args", "conf_dir", "conf", "auto_add_words", "words_file", "extra_inputs", "auto_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

checker_config!(AsciiCheckConfig, extensions: [".md"]);

fn default_pandoc() -> String {
    "pandoc".into()
}

fn default_pandoc_from() -> String {
    "markdown".into()
}

fn default_pandoc_formats() -> Vec<String> {
    vec!["pdf".into(), "html".into(), "docx".into()]
}

fn default_pandoc_output_dir() -> String {
    "out/pandoc".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PandocConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_pandoc")]
    pub pandoc: String,
    #[serde(default = "default_pandoc_from")]
    pub from: String,
    #[serde(default = "default_pandoc_formats")]
    pub formats: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_pandoc_output_dir")]
    pub output_dir: String,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PandocConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pandoc: "pandoc".into(),
            from: "markdown".into(),
            formats: vec!["pdf".into(), "html".into(), "docx".into()],
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/pandoc".into(),
            scan: default_scan!(scan_dir: "pandoc", extensions: [".md"]),
        }
    }
}

impl KnownFields for PandocConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "pandoc", "from", "formats", "args", "extra_inputs", "auto_inputs", "output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_marp_bin() -> String {
    "marp".into()
}

fn default_marp_formats() -> Vec<String> {
    vec!["pdf".into()]
}

fn default_marp_args() -> Vec<String> {
    vec!["--html".into(), "--allow-local-files".into()]
}

fn default_marp_output_dir() -> String {
    "out/marp".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MarpConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_marp_bin")]
    pub marp_bin: String,
    #[serde(default = "default_marp_formats")]
    pub formats: Vec<String>,
    #[serde(default = "default_marp_args")]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_marp_output_dir")]
    pub output_dir: String,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MarpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            marp_bin: "marp".into(),
            formats: vec!["pdf".into()],
            args: vec!["--html".into(), "--allow-local-files".into()],
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/marp".into(),
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for MarpConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "marp_bin", "formats", "args", "extra_inputs", "auto_inputs", "output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_markdown_bin() -> String {
    "markdown".into()
}

fn default_markdown_output_dir() -> String {
    "out/markdown".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MarkdownConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_markdown_bin")]
    pub markdown_bin: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_markdown_output_dir")]
    pub output_dir: String,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MarkdownConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            markdown_bin: "markdown".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/markdown".into(),
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for MarkdownConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "markdown_bin", "args", "extra_inputs", "auto_inputs", "output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_pdflatex() -> String {
    "pdflatex".into()
}

fn default_pdflatex_runs() -> usize {
    2
}

fn default_pdflatex_output_dir() -> String {
    "out/pdflatex".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PdflatexConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_pdflatex")]
    pub pdflatex: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_pdflatex_runs")]
    pub runs: usize,
    #[serde(default = "default_true")]
    pub qpdf: bool,
    #[serde(default = "default_pdflatex_output_dir")]
    pub output_dir: String,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PdflatexConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pdflatex: "pdflatex".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            runs: 2,
            qpdf: true,
            output_dir: "out/pdflatex".into(),
            scan: default_scan!(extensions: [".tex"]),
        }
    }
}

impl KnownFields for PdflatexConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "pdflatex", "args", "extra_inputs", "auto_inputs", "runs", "qpdf", "output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_a2x() -> String {
    "a2x".into()
}

fn default_a2x_format() -> String {
    "pdf".into()
}

fn default_a2x_output_dir() -> String {
    "out/a2x".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct A2xConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_a2x")]
    pub a2x: String,
    #[serde(default = "default_a2x_format")]
    pub format: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_a2x_output_dir")]
    pub output_dir: String,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for A2xConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            a2x: "a2x".into(),
            format: "pdf".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/a2x".into(),
            scan: default_scan!(extensions: [".txt"]),
        }
    }
}

impl KnownFields for A2xConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "a2x", "format", "args", "extra_inputs", "auto_inputs", "output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_bundler() -> String {
    "bundle".into()
}

fn default_bundler_command() -> String {
    "install".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GemConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_bundler")]
    pub bundler: String,
    #[serde(default = "default_bundler_command")]
    pub command: String,
    #[serde(default = "default_gem_home")]
    pub gem_home: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for GemConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bundler: "bundle".into(),
            command: "install".into(),
            gem_home: "gems".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            cache_output_dir: true,
            scan: default_scan!(extensions: ["Gemfile"]),
        }
    }
}

impl KnownFields for GemConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "bundler", "command", "gem_home", "args", "extra_inputs", "auto_inputs", "cache_output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_mmdc_bin() -> String {
    "mmdc".into()
}

fn default_mermaid_formats() -> Vec<String> {
    vec!["png".into()]
}

fn default_mermaid_output_dir() -> String {
    "out/mermaid".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MermaidConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_mmdc_bin")]
    pub mmdc_bin: String,
    #[serde(default = "default_mermaid_formats")]
    pub formats: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_mermaid_output_dir")]
    pub output_dir: String,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MermaidConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mmdc_bin: "mmdc".into(),
            formats: vec!["png".into()],
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/mermaid".into(),
            scan: default_scan!(extensions: [".mmd"]),
        }
    }
}

impl KnownFields for MermaidConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "mmdc_bin", "formats", "args", "extra_inputs", "auto_inputs", "output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_drawio_bin() -> String {
    "drawio".into()
}

fn default_drawio_formats() -> Vec<String> {
    vec!["png".into()]
}

fn default_drawio_output_dir() -> String {
    "out/drawio".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DrawioConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_drawio_bin")]
    pub drawio_bin: String,
    #[serde(default = "default_drawio_formats")]
    pub formats: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_drawio_output_dir")]
    pub output_dir: String,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for DrawioConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            drawio_bin: "drawio".into(),
            formats: vec!["png".into()],
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/drawio".into(),
            scan: default_scan!(extensions: [".drawio"]),
        }
    }
}

impl KnownFields for DrawioConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "drawio_bin", "formats", "args", "extra_inputs", "auto_inputs", "output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_libreoffice_bin() -> String {
    "libreoffice".into()
}

fn default_libreoffice_formats() -> Vec<String> {
    vec!["pdf".into()]
}

fn default_libreoffice_output_dir() -> String {
    "out/libreoffice".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LibreofficeConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_libreoffice_bin")]
    pub libreoffice_bin: String,
    #[serde(default = "default_libreoffice_formats")]
    pub formats: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_libreoffice_output_dir")]
    pub output_dir: String,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for LibreofficeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            libreoffice_bin: "libreoffice".into(),
            formats: vec!["pdf".into()],
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/libreoffice".into(),
            scan: default_scan!(extensions: [".odp"]),
        }
    }
}

impl KnownFields for LibreofficeConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "libreoffice_bin", "formats", "args", "extra_inputs", "auto_inputs", "output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_pdfunite_bin() -> String {
    "pdfunite".into()
}

fn default_pdfunite_source_dir() -> String {
    "marp/courses".into()
}

fn default_pdfunite_source_ext() -> String {
    ".md".into()
}

fn default_pdfunite_source_output_dir() -> String {
    "out/marp/pdf".into()
}

fn default_pdfunite_output_dir() -> String {
    "out/courses".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PdfuniteConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_pdfunite_bin")]
    pub pdfunite_bin: String,
    #[serde(default = "default_pdfunite_source_dir")]
    pub source_dir: String,
    #[serde(default = "default_pdfunite_source_ext")]
    pub source_ext: String,
    #[serde(default = "default_pdfunite_source_output_dir")]
    pub source_output_dir: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_pdfunite_output_dir")]
    pub output_dir: String,
}

impl Default for PdfuniteConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pdfunite_bin: "pdfunite".into(),
            source_dir: "marp/courses".into(),
            source_ext: ".md".into(),
            source_output_dir: "out/marp/pdf".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/courses".into(),
        }
    }
}

impl KnownFields for PdfuniteConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "pdfunite_bin", "source_dir", "source_ext", "source_output_dir",
            "args", "extra_inputs", "auto_inputs", "output_dir",
        ]
    }
}

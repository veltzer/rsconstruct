use serde::{Deserialize, Serialize};

use super::{default_true, default_cc_compiler, default_cxx_compiler, default_output_suffix, KnownFields, ScanConfig};

/// Generate a simple checker config struct with args, extra_inputs, and scan fields.
///
/// Generates the struct with serde `Deserialize`/`Serialize`/`Clone` derives and
/// a `Default` impl with the specified scan settings.
///
/// Variants:
/// - `checker_config!(Name, extensions: [".py"])` — default scan from project root
/// - `checker_config!(Name, scan_dir: "src", extensions: [".c"])` — scan in subdirectory
/// - `checker_config!(Name, default_args: fn_name, extensions: [".py"])` — custom default args
/// - `checker_config!(Name, default_args: fn_name, scan_dir: "src", extensions: [".c"])` — both
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
            #[serde(flatten)]
            pub scan: ScanConfig,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    enabled: true,
                    args: Vec::new(),
                    extra_inputs: Vec::new(),
                    scan: default_scan!(extensions: [$($ext),+]),
                }
            }
        }

        impl KnownFields for $name {
            fn known_fields() -> &'static [&'static str] {
                static FIELDS: &[&str] = &[
                    "enabled", "args", "extra_inputs",
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
            #[serde(flatten)]
            pub scan: ScanConfig,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    enabled: true,
                    args: Vec::new(),
                    extra_inputs: Vec::new(),
                    scan: default_scan!(scan_dir: $dir, extensions: [$($ext),+]),
                }
            }
        }

        impl KnownFields for $name {
            fn known_fields() -> &'static [&'static str] {
                static FIELDS: &[&str] = &[
                    "enabled", "args", "extra_inputs",
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
            scan: default_scan!(scan_dir: "templates", extensions: [".tera"]),
        }
    }
}

impl KnownFields for TeraConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "strict", "trim_blocks", "extra_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_ruff_linter() -> String {
    "ruff".into()
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
            scan: default_scan!(extensions: [".py"]),
        }
    }
}

impl KnownFields for RuffConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "linter", "args", "extra_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

checker_config!(PylintConfig, extensions: [".py"]);

fn default_cppcheck_args() -> Vec<String> {
    vec![
        "--error-exitcode=1".into(),
        "--enable=warning,style,performance,portability".into(),
    ]
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CppcheckConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
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
            enabled: true,
            args: default_cppcheck_args(),
            extra_inputs: Vec::new(),
            scan: default_scan!(scan_dir: "src", extensions: [".c", ".cc"]),
        }
    }
}

impl KnownFields for CppcheckConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "args", "extra_inputs",
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
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for ClangTidyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            args: Vec::new(),
            compiler_args: Vec::new(),
            extra_inputs: Vec::new(),
            scan: default_scan!(scan_dir: "src", extensions: [".c", ".cc"]),
        }
    }
}

impl KnownFields for ClangTidyConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "args", "compiler_args", "extra_inputs",
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
pub struct CcConfig {
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

impl Default for CcConfig {
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
            include_scanner: IncludeScanner::default(),
            scan: default_scan!(scan_dir: "src", extensions: [".c", ".cc"]),
        }
    }
}

impl KnownFields for CcConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "cc", "cxx", "cflags", "cxxflags", "ldflags", "output_suffix",
            "compilers", "include_paths", "extra_inputs", "include_scanner",
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
            enabled: true,
            language: "en_US".into(),
            words_file: ".spellcheck-words".into(),
            use_words_file: false,
            auto_add_words: false,
            extra_inputs: Vec::new(),
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for SpellcheckConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "language", "words_file", "use_words_file", "auto_add_words", "extra_inputs",
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
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for SleepConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            extra_inputs: Vec::new(),
            scan: default_scan!(scan_dir: "sleep", extensions: [".sleep"]),
        }
    }
}

impl KnownFields for SleepConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "extra_inputs",
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
            scan: default_scan!(extensions: ["Cargo.toml"]),
        }
    }
}

impl KnownFields for CargoConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "cargo", "command", "args", "extra_inputs",
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
            scan: default_scan!(extensions: ["Makefile"]),
        }
    }
}

impl KnownFields for MakeConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "make", "args", "target", "extra_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_mypy_checker() -> String {
    "mypy".into()
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
            scan: default_scan!(extensions: [".py"]),
        }
    }
}

impl KnownFields for MypyConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "checker", "args", "extra_inputs",
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
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PyreflyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            checker: "pyrefly".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            scan: default_scan!(extensions: [".py"]),
        }
    }
}

impl KnownFields for PyreflyConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "checker", "args", "extra_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_rumdl_linter() -> String {
    "rumdl".into()
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
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for RumdlConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "linter", "args", "extra_inputs",
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
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for YamllintConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            linter: "yamllint".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            scan: default_scan!(extensions: [".yml", ".yaml"]),
        }
    }
}

impl KnownFields for YamllintConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "linter", "args", "extra_inputs",
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
            scan: default_scan!(extensions: [".json"]),
        }
    }
}

impl KnownFields for JqConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "checker", "args", "extra_inputs",
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
            scan: default_scan!(extensions: [".json"]),
        }
    }
}

impl KnownFields for JsonlintConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "linter", "args", "extra_inputs",
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
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for TaploConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            linter: "taplo".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            scan: default_scan!(extensions: [".toml"]),
        }
    }
}

impl KnownFields for TaploConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "linter", "args", "extra_inputs",
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
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for TagsConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "output", "tags_file", "tags_file_strict", "extra_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_shellcheck_checker() -> String {
    "shellcheck".into()
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
            scan: default_scan!(extensions: [".sh", ".bash"]),
        }
    }
}

impl KnownFields for ShellcheckConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "checker", "args", "extra_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

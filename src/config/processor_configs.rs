use serde::{Deserialize, Serialize};

use super::{default_true, default_script_check_linter, default_cc_compiler, default_cxx_compiler, default_output_suffix, KnownFields, ScanConfig};

/// Generate a checker config struct with standard fields.
///
/// Generates the struct with serde `Deserialize`/`Serialize`/`Clone` derives and
/// a `Default` impl with the specified scan settings.
///
/// Variants:
/// - `checker_config!(Name, extensions: [".py"])` — basic checker
/// - `checker_config!(Name, scan_dir: "src", extensions: [".c"])` — with scan dir
/// - `checker_config!(Name, extensions: [".py"], linter: "ruff")` — with configurable tool name
/// - `checker_config!(Name, extensions: [".py"], auto_inputs: [".pylintrc"])` — with auto inputs
/// - `checker_config!(Name, extensions: [".py"], linter: "ruff", auto_inputs: [".ruff.toml"])` — both
macro_rules! checker_config {
    // Internal: generate struct body, Default, and KnownFields
    (@impl $name:ident,
     scan: $scan:expr,
     linter: [$($linter:expr)?],
     auto_inputs: [$($ai:expr),*]
    ) => {
        #[derive(Debug, Deserialize, Serialize, Clone)]
        pub struct $name {
            #[serde(default = "default_true")]
            pub enabled: bool,
            $(
                #[serde(default = $crate::config::processor_configs::checker_config!(@linter_default_name $name))]
                pub linter: String,
                const _: () = { // phantom use of $linter to bind it
                    fn _unused() { let _ = $linter; }
                };
            )?
            #[serde(default)]
            pub args: Vec<String>,
            #[serde(default)]
            pub extra_inputs: Vec<String>,
            #[serde(default)]
            pub auto_inputs: Vec<String>,
            #[serde(flatten)]
            pub scan: ScanConfig,
        }
    };

    // Basic: extensions only
    ($name:ident, extensions: [$($ext:expr),+ $(,)?]) => {
        checker_config!(@basic $name, default_scan!(extensions: [$($ext),+]));
    };
    // With scan_dir
    ($name:ident, scan_dir: $dir:expr, extensions: [$($ext:expr),+ $(,)?]) => {
        checker_config!(@basic $name, default_scan!(scan_dir: $dir, extensions: [$($ext),+]));
    };
    // With auto_inputs only
    ($name:ident, extensions: [$($ext:expr),+ $(,)?], auto_inputs: [$($ai:expr),+ $(,)?]) => {
        checker_config!(@with_auto_inputs $name, default_scan!(extensions: [$($ext),+]), [$($ai),+]);
    };
    // With linter only
    ($name:ident, extensions: [$($ext:expr),+ $(,)?], linter: $linter:expr) => {
        checker_config!(@with_linter $name, default_scan!(extensions: [$($ext),+]), $linter, []);
    };
    // With linter + auto_inputs
    ($name:ident, extensions: [$($ext:expr),+ $(,)?], linter: $linter:expr, auto_inputs: [$($ai:expr),+ $(,)?]) => {
        checker_config!(@with_linter $name, default_scan!(extensions: [$($ext),+]), $linter, [$($ai),+]);
    };

    // --- Internal generation rules ---

    // Basic struct (no linter, no custom auto_inputs)
    (@basic $name:ident, $scan:expr) => {
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
                    scan: $scan,
                }
            }
        }

        impl KnownFields for $name {
            fn known_fields() -> &'static [&'static str] {
                &["enabled", "args", "extra_inputs", "auto_inputs",
                  "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths"]
            }
        }
    };

    // With auto_inputs (no linter)
    (@with_auto_inputs $name:ident, $scan:expr, [$($ai:expr),+]) => {
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
                    auto_inputs: vec![$($ai.into()),+],
                    scan: $scan,
                }
            }
        }

        impl KnownFields for $name {
            fn known_fields() -> &'static [&'static str] {
                &["enabled", "args", "extra_inputs", "auto_inputs",
                  "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths"]
            }
        }
    };

    // With linter (and optional auto_inputs)
    (@with_linter $name:ident, $scan:expr, $linter:expr, [$($ai:expr),*]) => {
        // Generate a serde default function for the linter field.
        // Using paste to create a unique function name per config type.
        paste::paste! {
            fn [<default_ $name:lower _linter>]() -> String {
                $linter.into()
            }
        }

        paste::paste! {
            #[derive(Debug, Deserialize, Serialize, Clone)]
            pub struct $name {
                #[serde(default = "default_true")]
                pub enabled: bool,
                #[serde(default = "" [<default_ $name:lower _linter>] "")]
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
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    enabled: true,
                    linter: $linter.into(),
                    args: Vec::new(),
                    extra_inputs: Vec::new(),
                    auto_inputs: vec![$($ai.into()),*],
                    scan: $scan,
                }
            }
        }

        impl KnownFields for $name {
            fn known_fields() -> &'static [&'static str] {
                &["enabled", "linter", "args", "extra_inputs", "auto_inputs",
                  "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths"]
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

checker_config!(RuffConfig, extensions: [".py"], linter: "ruff", auto_inputs: ["ruff.toml", ".ruff.toml", "pyproject.toml"]);

checker_config!(PylintConfig, extensions: [".py"], auto_inputs: [".pylintrc"]);

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

// --- cc (full C/C++ project builds) ---

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CcLibraryDef {
    pub name: String,
    #[serde(default = "default_cc_lib_type")]
    pub lib_type: String,
    pub sources: Vec<String>,
    #[serde(default)]
    pub include_dirs: Vec<String>,
    #[serde(default)]
    pub cflags: Vec<String>,
    #[serde(default)]
    pub cxxflags: Vec<String>,
    #[serde(default)]
    pub ldflags: Vec<String>,
}

fn default_cc_lib_type() -> String {
    "shared".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CcProgramDef {
    pub name: String,
    pub sources: Vec<String>,
    #[serde(default)]
    pub link: Vec<String>,
    #[serde(default)]
    pub include_dirs: Vec<String>,
    #[serde(default)]
    pub cflags: Vec<String>,
    #[serde(default)]
    pub cxxflags: Vec<String>,
    #[serde(default)]
    pub ldflags: Vec<String>,
}

/// Parsed contents of a cc.yaml file.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CcManifest {
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
    pub include_dirs: Vec<String>,
    #[serde(default)]
    pub libraries: Vec<CcLibraryDef>,
    #[serde(default)]
    pub programs: Vec<CcProgramDef>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CcConfig {
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
    #[serde(default)]
    pub include_dirs: Vec<String>,
    #[serde(default)]
    pub single_invocation: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
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
            include_dirs: Vec::new(),
            single_invocation: false,
            extra_inputs: Vec::new(),
            cache_output_dir: true,
            scan: default_scan!(extensions: ["cc.yaml"]),
        }
    }
}

impl KnownFields for CcConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "cc", "cxx", "cflags", "cxxflags", "ldflags",
            "include_dirs", "single_invocation",
            "extra_inputs", "cache_output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

// --- linux_module (Linux kernel module builds) ---

/// A single kernel module definition inside linux-module.yaml.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LinuxModuleModuleDef {
    pub name: String,
    pub sources: Vec<String>,
    #[serde(default)]
    pub extra_cflags: Vec<String>,
}

/// Parsed contents of a linux-module.yaml file.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LinuxModuleManifest {
    #[serde(default = "default_make_tool")]
    pub make: String,
    #[serde(default)]
    pub kdir: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub cross_compile: Option<String>,
    #[serde(default = "default_linux_module_v")]
    pub v: u32,
    #[serde(default = "default_linux_module_w")]
    pub w: u32,
    pub modules: Vec<LinuxModuleModuleDef>,
}

fn default_make_tool() -> String {
    "make".into()
}

fn default_linux_module_v() -> u32 {
    0
}

fn default_linux_module_w() -> u32 {
    1
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LinuxModuleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for LinuxModuleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            extra_inputs: Vec::new(),
            scan: default_scan!(extensions: ["linux-module.yaml"]),
        }
    }
}

impl KnownFields for LinuxModuleConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "extra_inputs",
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
            profiles: default_cargo_profiles(),
            cache_output_dir: true,
            scan: default_scan!(extensions: ["Cargo.toml"]),
        }
    }
}

impl KnownFields for CargoConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "cargo", "command", "args", "extra_inputs", "profiles",
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

checker_config!(MypyConfig, extensions: [".py"], linter: "mypy", auto_inputs: ["mypy.ini"]);

checker_config!(PyreflyConfig, extensions: [".py"], linter: "pyrefly", auto_inputs: ["pyproject.toml"]);

checker_config!(RumdlConfig, extensions: [".md"], linter: "rumdl", auto_inputs: [".rumdl.toml"]);

checker_config!(YamllintConfig, extensions: [".yml", ".yaml"], linter: "yamllint", auto_inputs: [".yamllint", ".yamllint.yml", ".yamllint.yaml"]);

checker_config!(JqConfig, extensions: [".json"], linter: "jq");

checker_config!(JsonlintConfig, extensions: [".json"], linter: "jsonlint");

checker_config!(TaploConfig, extensions: [".toml"], linter: "taplo", auto_inputs: ["taplo.toml", ".taplo.toml"]);

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

checker_config!(ShellcheckConfig, extensions: [".sh", ".bash"], linter: "shellcheck", auto_inputs: [".shellcheckrc"]);

checker_config!(LuacheckConfig, extensions: [".lua"], linter: "luacheck", auto_inputs: [".luacheckrc"]);

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ScriptCheckConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_script_check_linter")]
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

impl Default for ScriptCheckConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            linter: "true".into(),
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
            "enabled", "linter", "args", "extra_inputs", "auto_inputs",
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
            scan: default_scan!(extensions: ["requirements.txt"]),
        }
    }
}

impl KnownFields for PipConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "pip", "args", "extra_inputs",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_sphinx_build() -> String {
    "sphinx-build".into()
}

fn default_sphinx_output_dir() -> String {
    "docs".into()
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
    pub working_dir: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
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
            output_dir: "docs".into(),
            working_dir: None,
            args: Vec::new(),
            extra_inputs: Vec::new(),
            cache_output_dir: true,
            scan: default_scan!(extensions: ["conf.py"]),
        }
    }
}

impl KnownFields for SphinxConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "sphinx_build", "output_dir", "working_dir", "args", "extra_inputs", "cache_output_dir",
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
            cache_output_dir: true,
            scan: default_scan!(extensions: ["book.toml"]),
        }
    }
}

impl KnownFields for MdbookConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "mdbook", "output_dir", "args", "extra_inputs", "cache_output_dir",
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
            cache_output_dir: true,
            scan: default_scan!(extensions: ["package.json"]),
        }
    }
}

impl KnownFields for NpmConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "npm", "command", "args", "extra_inputs", "cache_output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_mdl_bin() -> String {
    "mdl".into()
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
    #[serde(default)]
    pub local_repo: bool,
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
            local_repo: false,
            gem_home: "gems".into(),
            mdl_bin: "mdl".into(),
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
            "enabled", "local_repo", "gem_home", "mdl_bin", "args", "extra_inputs", "auto_inputs", "gem_stamp",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

fn default_markdownlint_bin() -> String {
    "markdownlint".into()
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
    #[serde(default)]
    pub local_repo: bool,
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
            local_repo: false,
            markdownlint_bin: "markdownlint".into(),
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
            "enabled", "local_repo", "markdownlint_bin", "args", "extra_inputs", "auto_inputs", "npm_stamp",
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

fn default_chromium_bin() -> String {
    "google-chrome".into()
}

fn default_chromium_output_dir() -> String {
    "out/chromium".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChromiumConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_chromium_bin")]
    pub chromium_bin: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_chromium_output_dir")]
    pub output_dir: String,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for ChromiumConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            chromium_bin: "google-chrome".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/chromium".into(),
            scan: default_scan!(extensions: [".html"]),
        }
    }
}

impl KnownFields for ChromiumConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "chromium_bin", "args", "extra_inputs", "auto_inputs", "output_dir",
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
            cache_output_dir: true,
            scan: default_scan!(extensions: ["Gemfile"]),
        }
    }
}

impl KnownFields for GemConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "bundler", "command", "gem_home", "args", "extra_inputs", "cache_output_dir",
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
    "out/marp".into()
}

fn default_pdfunite_output_dir() -> String {
    "out/pdfunite".into()
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
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PdfuniteConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pdfunite_bin: "pdfunite".into(),
            source_dir: "marp/courses".into(),
            source_ext: ".md".into(),
            source_output_dir: "out/marp".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/pdfunite".into(),
            scan: default_scan!(extensions: ["course.yaml"]),
        }
    }
}

impl KnownFields for PdfuniteConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "pdfunite_bin", "source_dir", "source_ext", "source_output_dir",
            "args", "extra_inputs", "auto_inputs", "output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

checker_config!(CpplintConfig, scan_dir: "src", extensions: [".c", ".cc", ".h", ".hh"]);

checker_config!(CheckpatchConfig, scan_dir: "src", extensions: [".c", ".h"]);

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ObjdumpConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_objdump_output_dir")]
    pub output_dir: String,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_objdump_output_dir() -> String {
    "out/objdump".into()
}

impl Default for ObjdumpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            args: Vec::new(),
            extra_inputs: Vec::new(),
            output_dir: default_objdump_output_dir(),
            scan: default_scan!(scan_dir: "out/cc_single_file", extensions: [".elf"]),
        }
    }
}

impl KnownFields for ObjdumpConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "enabled", "args", "extra_inputs", "output_dir",
            "scan_dir", "extensions", "exclude_dirs", "exclude_files", "exclude_paths",
        ]
    }
}

checker_config!(EslintConfig, extensions: [".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"], linter: "eslint", auto_inputs: [".eslintrc", ".eslintrc.json", ".eslintrc.js", ".eslintrc.yml", ".eslintrc.yaml", ".eslintrc.cjs", "eslint.config.js", "eslint.config.mjs", "eslint.config.cjs"]);

checker_config!(JshintConfig, extensions: [".js", ".jsx", ".mjs", ".cjs"], linter: "jshint", auto_inputs: [".jshintrc"]);

checker_config!(HtmlhintConfig, extensions: [".html", ".htm"], linter: "htmlhint", auto_inputs: [".htmlhintrc"]);

// --- tidy (HTML validator) ---
checker_config!(TidyConfig, extensions: [".html", ".htm"]);

// --- stylelint (CSS linter) ---
checker_config!(StylelintConfig, extensions: [".css", ".scss", ".sass", ".less"], linter: "stylelint", auto_inputs: [".stylelintrc", ".stylelintrc.json", ".stylelintrc.yml", ".stylelintrc.yaml", ".stylelintrc.js", ".stylelintrc.cjs", "stylelint.config.js", "stylelint.config.cjs"]);

// --- jslint (JavaScript linter) ---
checker_config!(JslintConfig, extensions: [".js"]);

// --- standard (JavaScript style checker) ---
checker_config!(StandardConfig, extensions: [".js"]);

// --- htmllint (HTML linter) ---
checker_config!(HtmllintConfig, extensions: [".html", ".htm"]);

// --- php_lint (PHP syntax checker) ---
checker_config!(PhpLintConfig, extensions: [".php"]);

// --- perlcritic (Perl code analyzer) ---
checker_config!(PerlcriticConfig, extensions: [".pl", ".pm"], auto_inputs: [".perlcriticrc"]);

// --- xmllint (XML validator) ---
checker_config!(XmllintConfig, extensions: [".xml"]);

// --- checkstyle (Java style checker) ---
checker_config!(CheckstyleConfig, extensions: [".java"], auto_inputs: ["checkstyle.xml"]);

// --- yq (YAML processor/validator) ---
checker_config!(YqConfig, extensions: [".yml", ".yaml"]);

// --- cmake (CMake build system) ---
checker_config!(CmakeConfig, extensions: ["CMakeLists.txt"]);

// --- docker (Docker image build) ---
checker_config!(HadolintConfig, extensions: ["Dockerfile"]);

// --- jekyll (Static site generator) ---
checker_config!(JekyllConfig, extensions: ["_config.yml"]);

// --- slidev (Slidev presentations) ---
checker_config!(SlidevConfig, extensions: [".md"]);

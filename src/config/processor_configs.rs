use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{default_true, default_script_linter, default_cc_compiler, default_cxx_compiler, default_output_suffix, KnownFields, ScanConfig};

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
    // --- Public entry points ---

    // Basic: extensions only
    ($name:ident, extensions: [$($ext:expr),+ $(,)?]) => {
        checker_config!(@no_linter $name, default_scan!(extensions: [$($ext),+]), []);
    };
    // With scan_dir
    ($name:ident, scan_dir: $dir:expr, extensions: [$($ext:expr),+ $(,)?]) => {
        checker_config!(@no_linter $name, default_scan!(scan_dir: $dir, extensions: [$($ext),+]), []);
    };
    // With auto_inputs only
    ($name:ident, extensions: [$($ext:expr),+ $(,)?], auto_inputs: [$($ai:expr),+ $(,)?]) => {
        checker_config!(@no_linter $name, default_scan!(extensions: [$($ext),+]), [$($ai),+]);
    };
    // With linter only
    ($name:ident, extensions: [$($ext:expr),+ $(,)?], linter: $linter:expr) => {
        checker_config!(@with_linter $name, default_scan!(extensions: [$($ext),+]), $linter, []);
    };
    // With linter + auto_inputs
    ($name:ident, extensions: [$($ext:expr),+ $(,)?], linter: $linter:expr, auto_inputs: [$($ai:expr),+ $(,)?]) => {
        checker_config!(@with_linter $name, default_scan!(extensions: [$($ext),+]), $linter, [$($ai),+]);
    };

    // --- Internal: without linter field ---
    (@no_linter $name:ident, $scan:expr, [$($ai:expr),*]) => {
        #[derive(Debug, Deserialize, Serialize, Clone)]
        pub struct $name {
            #[serde(default)]
            pub args: Vec<String>,
            #[serde(default)]
            pub extra_inputs: Vec<String>,
            #[serde(default)]
            pub auto_inputs: Vec<String>,
            #[serde(default = "default_true")]
            pub batch: bool,
            #[serde(flatten)]
            pub scan: ScanConfig,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    args: Vec::new(),
                    extra_inputs: Vec::new(),
                    auto_inputs: vec![$($ai.into()),*],
                    batch: true,
                    scan: $scan,
                }
            }
        }

        impl KnownFields for $name {
            fn known_fields() -> &'static [&'static str] {
                &["args", "extra_inputs", "auto_inputs", "batch"]
            }
        }
    };

    // --- Internal: with linter field ---
    (@with_linter $name:ident, $scan:expr, $linter:expr, [$($ai:expr),*]) => {
        paste::paste! {
            fn [<default_ $name:lower _linter>]() -> String {
                $linter.into()
            }
        }

        paste::paste! {
            #[derive(Debug, Deserialize, Serialize, Clone)]
            pub struct $name {
                #[serde(default = "" [<default_ $name:lower _linter>] "")]
                pub linter: String,
                #[serde(default)]
                pub args: Vec<String>,
                #[serde(default)]
                pub extra_inputs: Vec<String>,
                #[serde(default)]
                pub auto_inputs: Vec<String>,
                #[serde(default = "default_true")]
                pub batch: bool,
                #[serde(flatten)]
                pub scan: ScanConfig,
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    linter: $linter.into(),
                    args: Vec::new(),
                    extra_inputs: Vec::new(),
                    auto_inputs: vec![$($ai.into()),*],
                    batch: true,
                    scan: $scan,
                }
            }
        }

        impl KnownFields for $name {
            fn known_fields() -> &'static [&'static str] {
                &["linter", "args", "extra_inputs", "auto_inputs", "batch"]
            }
        }
    };
}

/// Create a default ScanConfig with optional scan_dirs and required extensions.
/// All exclude fields default to None (filled by resolve_scan_defaults).
macro_rules! default_scan {
    (extensions: [$($ext:expr),+ $(,)?]) => {
        ScanConfig {
            scan_dirs: None,
            extensions: Some(vec![$($ext.into()),+]),
            exclude_dirs: None,
            exclude_files: None,
            exclude_paths: None,
        }
    };
    (scan_dir: $dir:expr, extensions: [$($ext:expr),+ $(,)?]) => {
        ScanConfig {
            scan_dirs: Some(vec![$dir.into()]),
            extensions: Some(vec![$($ext.into()),+]),
            exclude_dirs: None,
            exclude_files: None,
            exclude_paths: None,
        }
    };
}

/// Generate a generator config struct with standard fields plus a tool binary and output directory.
///
/// Variants:
/// - `generator_config!(Name, tool: "bin_name" "field_name", output_dir: "out/x", scan)` — single output
/// - `generator_config!(Name, tool: "bin_name" "field_name", formats: ["pdf"], output_dir: "out/x", scan)` — multi-format
/// - Add `args: ["--flag"]` for non-empty default args
/// - Add `auto_inputs: [".config"]` for auto input files
macro_rules! generator_config {
    // Multi-format variant with custom default args
    ($name:ident, tool: $tool_default:expr, $tool_field:ident,
     formats: [$($fmt:expr),+ $(,)?], output_dir: $output_dir:expr,
     $scan:expr, args: [$($arg:expr),+ $(,)?] $(, auto_inputs: [$($ai:expr),+ $(,)?])? $(,)?
    ) => {
        paste::paste! {
            fn [<default_ $name:lower _tool>]() -> String { $tool_default.into() }
            fn [<default_ $name:lower _formats>]() -> Vec<String> { vec![$($fmt.into()),+] }
            fn [<default_ $name:lower _output_dir>]() -> String { $output_dir.into() }
            fn [<default_ $name:lower _args>]() -> Vec<String> { vec![$($arg.into()),+] }
        }

        paste::paste! {
            #[derive(Debug, Deserialize, Serialize, Clone)]
            pub struct $name {
                #[serde(default = "" [<default_ $name:lower _tool>] "")]
                pub $tool_field: String,
                #[serde(default = "" [<default_ $name:lower _formats>] "")]
                pub formats: Vec<String>,
                #[serde(default = "" [<default_ $name:lower _args>] "")]
                pub args: Vec<String>,
                #[serde(default)]
                pub extra_inputs: Vec<String>,
                #[serde(default)]
                pub auto_inputs: Vec<String>,
                #[serde(default = "" [<default_ $name:lower _output_dir>] "")]
                pub output_dir: String,
                #[serde(default = "default_true")]
                pub batch: bool,
                #[serde(flatten)]
                pub scan: ScanConfig,
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    $tool_field: $tool_default.into(),
                    formats: vec![$($fmt.into()),+],
                    args: vec![$($arg.into()),+],
                    extra_inputs: Vec::new(),
                    auto_inputs: generator_config!(@default_auto_inputs $($($ai),+)?),
                    output_dir: $output_dir.into(),
                    batch: true,
                    scan: $scan,
                }
            }
        }

        impl KnownFields for $name {
            fn known_fields() -> &'static [&'static str] {
                &[stringify!($tool_field), "formats", "args",
                  "extra_inputs", "auto_inputs", "output_dir", "batch"]
            }
        }
    };

    // Multi-format variant without custom args
    ($name:ident, tool: $tool_default:expr, $tool_field:ident,
     formats: [$($fmt:expr),+ $(,)?], output_dir: $output_dir:expr,
     $scan:expr $(, auto_inputs: [$($ai:expr),+ $(,)?])? $(,)?
    ) => {
        paste::paste! {
            fn [<default_ $name:lower _tool>]() -> String { $tool_default.into() }
            fn [<default_ $name:lower _formats>]() -> Vec<String> { vec![$($fmt.into()),+] }
            fn [<default_ $name:lower _output_dir>]() -> String { $output_dir.into() }
        }

        paste::paste! {
            #[derive(Debug, Deserialize, Serialize, Clone)]
            pub struct $name {
                #[serde(default = "" [<default_ $name:lower _tool>] "")]
                pub $tool_field: String,
                #[serde(default = "" [<default_ $name:lower _formats>] "")]
                pub formats: Vec<String>,
                #[serde(default)]
                pub args: Vec<String>,
                #[serde(default)]
                pub extra_inputs: Vec<String>,
                #[serde(default)]
                pub auto_inputs: Vec<String>,
                #[serde(default = "" [<default_ $name:lower _output_dir>] "")]
                pub output_dir: String,
                #[serde(default = "default_true")]
                pub batch: bool,
                #[serde(flatten)]
                pub scan: ScanConfig,
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    $tool_field: $tool_default.into(),
                    formats: vec![$($fmt.into()),+],
                    args: Vec::new(),
                    extra_inputs: Vec::new(),
                    auto_inputs: generator_config!(@default_auto_inputs $($($ai),+)?),
                    output_dir: $output_dir.into(),
                    batch: true,
                    scan: $scan,
                }
            }
        }

        impl KnownFields for $name {
            fn known_fields() -> &'static [&'static str] {
                &[stringify!($tool_field), "formats", "args",
                  "extra_inputs", "auto_inputs", "output_dir", "batch"]
            }
        }
    };

    // Single-output variant (no formats field)
    ($name:ident, tool: $tool_default:expr, $tool_field:ident,
     output_dir: $output_dir:expr,
     $scan:expr $(, auto_inputs: [$($ai:expr),+ $(,)?])? $(,)?
    ) => {
        paste::paste! {
            fn [<default_ $name:lower _tool>]() -> String { $tool_default.into() }
            fn [<default_ $name:lower _output_dir>]() -> String { $output_dir.into() }
        }

        paste::paste! {
            #[derive(Debug, Deserialize, Serialize, Clone)]
            pub struct $name {
                #[serde(default = "" [<default_ $name:lower _tool>] "")]
                pub $tool_field: String,
                #[serde(default)]
                pub args: Vec<String>,
                #[serde(default)]
                pub extra_inputs: Vec<String>,
                #[serde(default)]
                pub auto_inputs: Vec<String>,
                #[serde(default = "" [<default_ $name:lower _output_dir>] "")]
                pub output_dir: String,
                #[serde(default = "default_true")]
                pub batch: bool,
                #[serde(flatten)]
                pub scan: ScanConfig,
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    $tool_field: $tool_default.into(),
                    args: Vec::new(),
                    extra_inputs: Vec::new(),
                    auto_inputs: generator_config!(@default_auto_inputs $($($ai),+)?),
                    output_dir: $output_dir.into(),
                    batch: true,
                    scan: $scan,
                }
            }
        }

        impl KnownFields for $name {
            fn known_fields() -> &'static [&'static str] {
                &[stringify!($tool_field), "args",
                  "extra_inputs", "auto_inputs", "output_dir", "batch"]
            }
        }
    };

    // Helper: default auto_inputs (empty or provided)
    (@default_auto_inputs) => { Vec::new() };
    (@default_auto_inputs $($ai:expr),+) => { vec![$($ai.into()),+] };
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TeraConfig {
    #[serde(default = "default_true")]
    pub strict: bool,
    #[serde(default)]
    pub trim_blocks: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for TeraConfig {
    fn default() -> Self {
        Self {
            strict: true,
            trim_blocks: false,
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            batch: true,
            scan: default_scan!(scan_dir: "tera.templates", extensions: [".tera"]),
        }
    }
}

impl KnownFields for TeraConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "strict", "trim_blocks", "extra_inputs", "auto_inputs", "batch",
        ]
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MakoConfig {
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MakoConfig {
    fn default() -> Self {
        Self {
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            batch: true,
            scan: default_scan!(scan_dir: "templates.mako", extensions: [".mako"]),
        }
    }
}

impl KnownFields for MakoConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "extra_inputs", "auto_inputs", "batch",
        ]
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Jinja2Config {
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for Jinja2Config {
    fn default() -> Self {
        Self {
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            batch: true,
            scan: default_scan!(scan_dir: "templates.jinja2", extensions: [".j2"]),
        }
    }
}

impl KnownFields for Jinja2Config {
    fn known_fields() -> &'static [&'static str] {
        &[
            "extra_inputs", "auto_inputs", "batch",
        ]
    }
}

checker_config!(RuffConfig, extensions: [".py"], linter: "ruff", auto_inputs: ["ruff.toml", ".ruff.toml", "pyproject.toml"]);

checker_config!(PylintConfig, extensions: [".py"], auto_inputs: [".pylintrc"]);

checker_config!(PytestConfig, extensions: [".py"], auto_inputs: ["conftest.py", "pytest.ini", "pyproject.toml"]);

checker_config!(BlackConfig, extensions: [".py"], auto_inputs: ["pyproject.toml"]);

checker_config!(DoctestConfig, extensions: [".py"]);

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
    #[serde(default = "default_cppcheck_args")]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_cppcheck_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for CppcheckConfig {
    fn default() -> Self {
        Self {
            args: default_cppcheck_args(),
            extra_inputs: Vec::new(),
            auto_inputs: default_cppcheck_auto_inputs(),
            batch: true,
            scan: default_scan!(scan_dir: "src", extensions: [".c", ".cc"]),
        }
    }
}

impl KnownFields for CppcheckConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "args", "extra_inputs", "auto_inputs", "batch",
        ]
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
    #[serde(default = "default_clang_tidy_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_clang_tidy_auto_inputs() -> Vec<String> {
    vec![".clang-tidy".into()]
}

impl Default for ClangTidyConfig {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            compiler_args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_clang_tidy_auto_inputs(),
            batch: true,
            scan: default_scan!(scan_dir: "src", extensions: [".c", ".cc"]),
        }
    }
}

impl KnownFields for ClangTidyConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "args", "compiler_args", "extra_inputs", "auto_inputs", "batch",
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
    /// Output directory for compiled executables
    #[serde(default = "default_cc_single_file_output_dir")]
    pub output_dir: String,
    /// Method for scanning header dependencies (native or compiler)
    #[serde(default)]
    pub include_scanner: IncludeScanner,
    #[serde(default = "default_true")]
    pub batch: bool,
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

fn default_cc_single_file_output_dir() -> String { "out/cc_single_file".into() }

impl Default for CcSingleFileConfig {
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
            auto_inputs: Vec::new(),
            output_dir: "out/cc_single_file".into(),
            include_scanner: IncludeScanner::default(),
            batch: true,
            scan: default_scan!(scan_dir: "src", extensions: [".c", ".cc"]),
        }
    }
}

impl KnownFields for CcSingleFileConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "cc", "cxx", "cflags", "cxxflags", "ldflags", "output_suffix",
            "compilers", "include_paths", "extra_inputs", "auto_inputs", "output_dir",
            "include_scanner", "batch",
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
    #[serde(default = "default_true")]
    pub batch: bool,
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
            include_dirs: Vec::new(),
            single_invocation: false,
            extra_inputs: Vec::new(),
            cache_output_dir: true,
            batch: true,
            scan: default_scan!(extensions: ["cc.yaml"]),
        }
    }
}

impl KnownFields for CcConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "cc", "cxx", "cflags", "cxxflags", "ldflags",
            "include_dirs", "single_invocation",
            "extra_inputs", "cache_output_dir", "batch",
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
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for LinuxModuleConfig {
    fn default() -> Self {
        Self {
            extra_inputs: Vec::new(),
            batch: true,
            scan: default_scan!(extensions: ["linux-module.yaml"]),
        }
    }
}

impl KnownFields for LinuxModuleConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "extra_inputs", "batch",
        ]
    }
}

fn default_zspell_language() -> String {
    "en_US".into()
}

fn default_zspell_words_file() -> String {
    ".zspell-words".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ZspellConfig {
    #[serde(default = "default_zspell_language")]
    pub language: String,
    #[serde(default = "default_zspell_words_file")]
    pub words_file: String,
    /// When true, automatically add misspelled words to words_file instead of failing
    #[serde(default)]
    pub auto_add_words: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_zspell_auto_inputs")]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_zspell_auto_inputs() -> Vec<String> {
    vec![".zspell-words".into()]
}

impl Default for ZspellConfig {
    fn default() -> Self {
        Self {
            language: "en_US".into(),
            words_file: ".zspell-words".into(),
            auto_add_words: false,
            extra_inputs: Vec::new(),
            auto_inputs: default_zspell_auto_inputs(),
            batch: true,
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for ZspellConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "language", "words_file", "auto_add_words", "extra_inputs", "auto_inputs", "batch",
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
    #[serde(default = "default_true")]
    pub batch: bool,
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
            profiles: default_cargo_profiles(),
            cache_output_dir: true,
            batch: true,
            scan: default_scan!(extensions: ["Cargo.toml"]),
        }
    }
}

impl KnownFields for CargoConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "cargo", "command", "args", "extra_inputs", "profiles",
            "cache_output_dir", "batch", "scan_dir", "extensions", "exclude_dirs", "exclude_files",
            "exclude_paths",
        ]
    }
}

fn default_clippy_command() -> String {
    "clippy".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClippyConfig {
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
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for ClippyConfig {
    fn default() -> Self {
        Self {
            cargo: "cargo".into(),
            command: "clippy".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            batch: true,
            scan: default_scan!(extensions: ["Cargo.toml"]),
        }
    }
}

impl KnownFields for ClippyConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "cargo", "command", "args", "extra_inputs", "auto_inputs", "batch",
        ]
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
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
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
            auto_inputs: Vec::new(),
            batch: true,
            scan: default_scan!(extensions: ["Makefile"]),
        }
    }
}

impl KnownFields for MakeConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "make", "args", "target", "extra_inputs", "auto_inputs", "batch",
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

fn default_tags_dir() -> String {
    "tags".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TagsConfig {
    #[serde(default = "default_tags_output")]
    pub output: String,
    /// Directory containing tag list files.
    /// Each `<name>.txt` file defines allowed tags as `<name>:<line>` pairs.
    #[serde(default = "default_tags_dir")]
    pub tags_dir: String,
    /// Frontmatter fields that every markdown file must have.
    #[serde(default)]
    pub required_fields: Vec<String>,
    /// Scalar fields whose values must exist in the corresponding tag_lists file.
    #[serde(default)]
    pub required_values: Vec<String>,
    /// Fields whose values must be unique across all files.
    #[serde(default)]
    pub unique_fields: Vec<String>,
    /// Expected types for fields: "scalar", "list", or "number".
    #[serde(default)]
    pub field_types: HashMap<String, String>,
    /// Groups of fields where at least one group must be fully present.
    /// Each inner Vec is a group; a file passes if all fields in any one group are present.
    #[serde(default)]
    pub required_field_groups: Vec<Vec<String>>,
    /// Require list-type fields to have items in sorted order.
    #[serde(default)]
    pub sorted_tags: bool,
    /// Fail the build when tags in the allowlist are not used by any file.
    #[serde(default)]
    pub check_unused: bool,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for TagsConfig {
    fn default() -> Self {
        Self {
            output: "out/tags/tags.db".into(),
            tags_dir: "tags".into(),
            required_fields: Vec::new(),
            required_values: Vec::new(),
            unique_fields: Vec::new(),
            field_types: HashMap::new(),
            required_field_groups: Vec::new(),
            sorted_tags: false,
            check_unused: false,
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            batch: true,
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for TagsConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "output", "tags_dir", "required_fields", "required_values",
            "unique_fields", "field_types", "required_field_groups", "sorted_tags",
            "check_unused", "extra_inputs", "auto_inputs", "batch",
        ]
    }
}

checker_config!(ShellcheckConfig, extensions: [".sh", ".bash"], linter: "shellcheck", auto_inputs: [".shellcheckrc"]);

checker_config!(LuacheckConfig, extensions: [".lua"], linter: "luacheck", auto_inputs: [".luacheckrc"]);

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ScriptConfig {
    #[serde(default = "default_script_linter")]
    pub linter: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for ScriptConfig {
    fn default() -> Self {
        Self {
            linter: "true".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            batch: true,
            scan: ScanConfig {
                scan_dirs: None,
                extensions: None,
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

impl KnownFields for ScriptConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "linter", "args", "extra_inputs", "auto_inputs", "batch",
        ]
    }
}

fn default_generator_command() -> String {
    "true".into()
}

fn default_generator_output_dir() -> String {
    "out/generator".into()
}

fn default_generator_output_extension() -> String {
    "out".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GeneratorConfig {
    #[serde(default = "default_generator_command")]
    pub command: String,
    #[serde(default = "default_generator_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_generator_output_extension")]
    pub output_extension: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            command: "true".into(),
            output_dir: "out/generator".into(),
            output_extension: "out".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            batch: true,
            scan: ScanConfig {
                scan_dirs: None,
                extensions: None,
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

impl KnownFields for GeneratorConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "command", "output_dir", "output_extension", "args",
            "extra_inputs", "auto_inputs", "batch",
        ]
    }
}

// --- explicit processor (many inputs → few outputs, fully declared) ---

fn default_explicit_command() -> String {
    "true".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ExplicitConfig {
    /// Script or binary to execute
    #[serde(default = "default_explicit_command")]
    pub command: String,
    /// Extra arguments passed before --inputs
    #[serde(default)]
    pub args: Vec<String>,
    /// Literal input file paths
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Glob patterns resolved to input files
    #[serde(default)]
    pub input_globs: Vec<String>,
    /// Output file paths produced by the command
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Unused — present for compatibility with the processor macro system
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for ExplicitConfig {
    fn default() -> Self {
        Self {
            command: "true".into(),
            args: Vec::new(),
            inputs: Vec::new(),
            input_globs: Vec::new(),
            outputs: Vec::new(),
            scan: ScanConfig {
                scan_dirs: None,
                extensions: None,
                exclude_dirs: None,
                exclude_files: None,
                exclude_paths: None,
            },
        }
    }
}

impl KnownFields for ExplicitConfig {
    fn known_fields() -> &'static [&'static str] {
        &["command", "args", "inputs", "input_globs", "outputs"]
    }
}

fn default_pip() -> String {
    "pip".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PipConfig {
    #[serde(default = "default_pip")]
    pub pip: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PipConfig {
    fn default() -> Self {
        Self {
            pip: "pip".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            batch: true,
            scan: default_scan!(extensions: ["requirements.txt"]),
        }
    }
}

impl KnownFields for PipConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "pip", "args", "extra_inputs", "batch",
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
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for SphinxConfig {
    fn default() -> Self {
        Self {
            sphinx_build: "sphinx-build".into(),
            output_dir: "docs".into(),
            working_dir: None,
            args: Vec::new(),
            extra_inputs: Vec::new(),
            cache_output_dir: true,
            batch: true,
            scan: default_scan!(extensions: ["conf.py"]),
        }
    }
}

impl KnownFields for SphinxConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "sphinx_build", "output_dir", "working_dir", "args", "extra_inputs", "cache_output_dir", "batch",
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
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MdbookConfig {
    fn default() -> Self {
        Self {
            mdbook: "mdbook".into(),
            output_dir: "book".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            cache_output_dir: true,
            batch: true,
            scan: default_scan!(extensions: ["book.toml"]),
        }
    }
}

impl KnownFields for MdbookConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "mdbook", "output_dir", "args", "extra_inputs", "cache_output_dir", "batch",
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
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for NpmConfig {
    fn default() -> Self {
        Self {
            npm: "npm".into(),
            command: "install".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            cache_output_dir: true,
            batch: true,
            scan: default_scan!(extensions: ["package.json"]),
        }
    }
}

impl KnownFields for NpmConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "npm", "command", "args", "extra_inputs", "cache_output_dir", "batch",
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
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MdlConfig {
    fn default() -> Self {
        Self {
            local_repo: false,
            gem_home: "gems".into(),
            mdl_bin: "mdl".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_mdl_auto_inputs(),
            gem_stamp: "out/gem/root.stamp".into(),
            batch: true,
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for MdlConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "local_repo", "gem_home", "mdl_bin", "args", "extra_inputs", "auto_inputs", "gem_stamp", "batch",
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
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MarkdownlintConfig {
    fn default() -> Self {
        Self {
            local_repo: false,
            markdownlint_bin: "markdownlint".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: default_markdownlint_auto_inputs(),
            npm_stamp: "out/npm/root.stamp".into(),
            batch: true,
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for MarkdownlintConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "local_repo", "markdownlint_bin", "args", "extra_inputs", "auto_inputs", "npm_stamp", "batch",
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
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for AspellConfig {
    fn default() -> Self {
        Self {
            aspell: "aspell".into(),
            args: Vec::new(),
            conf_dir: ".".into(),
            conf: ".aspell.conf".into(),
            auto_add_words: false,
            words_file: ".aspell.en.pws".into(),
            extra_inputs: Vec::new(),
            auto_inputs: default_aspell_auto_inputs(),
            batch: true,
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for AspellConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "aspell", "args", "conf_dir", "conf", "auto_add_words", "words_file", "extra_inputs", "auto_inputs", "batch",
        ]
    }
}

checker_config!(AsciiConfig, extensions: [".md"]);

fn default_terms_dir() -> String {
    "terms".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TermsConfig {
    #[serde(default = "default_terms_dir")]
    pub terms_dir: String,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for TermsConfig {
    fn default() -> Self {
        Self {
            terms_dir: "terms".into(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            batch: true,
            scan: default_scan!(extensions: [".md"]),
        }
    }
}

impl KnownFields for TermsConfig {
    fn known_fields() -> &'static [&'static str] {
        &["terms_dir", "extra_inputs", "auto_inputs", "batch"]
    }
}

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
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PandocConfig {
    fn default() -> Self {
        Self {
            pandoc: "pandoc".into(),
            from: "markdown".into(),
            formats: vec!["pdf".into(), "html".into(), "docx".into()],
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/pandoc".into(),
            batch: true,
            scan: default_scan!(scan_dir: "pandoc", extensions: [".md"]),
        }
    }
}

impl KnownFields for PandocConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "pandoc", "from", "formats", "args", "extra_inputs", "auto_inputs", "output_dir", "batch",
        ]
    }
}

generator_config!(MarpConfig, tool: "marp", marp_bin,
    formats: ["pdf"], output_dir: "out/marp",
    default_scan!(extensions: [".md"]),
    args: ["--html", "--allow-local-files"],
);

generator_config!(MarkdownConfig, tool: "markdown", markdown_bin,
    output_dir: "out/markdown",
    default_scan!(extensions: [".md"]),
);

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
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PdflatexConfig {
    fn default() -> Self {
        Self {
            pdflatex: "pdflatex".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            runs: 2,
            qpdf: true,
            output_dir: "out/pdflatex".into(),
            batch: true,
            scan: default_scan!(extensions: [".tex"]),
        }
    }
}

impl KnownFields for PdflatexConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "pdflatex", "args", "extra_inputs", "auto_inputs", "runs", "qpdf", "output_dir", "batch",
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
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for A2xConfig {
    fn default() -> Self {
        Self {
            a2x: "a2x".into(),
            format: "pdf".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/a2x".into(),
            batch: true,
            scan: default_scan!(extensions: [".txt"]),
        }
    }
}

impl KnownFields for A2xConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "a2x", "format", "args", "extra_inputs", "auto_inputs", "output_dir", "batch",
        ]
    }
}

generator_config!(ChromiumConfig, tool: "google-chrome", chromium_bin,
    output_dir: "out/chromium",
    default_scan!(extensions: [".html"]),
);

fn default_bundler() -> String {
    "bundle".into()
}

fn default_bundler_command() -> String {
    "install".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GemConfig {
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
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for GemConfig {
    fn default() -> Self {
        Self {
            bundler: "bundle".into(),
            command: "install".into(),
            gem_home: "gems".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            cache_output_dir: true,
            batch: true,
            scan: default_scan!(extensions: ["Gemfile"]),
        }
    }
}

impl KnownFields for GemConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "bundler", "command", "gem_home", "args", "extra_inputs", "cache_output_dir", "batch",
        ]
    }
}

generator_config!(MermaidConfig, tool: "mmdc", mmdc_bin,
    formats: ["png"], output_dir: "out/mermaid",
    default_scan!(extensions: [".mmd"]),
);

generator_config!(DrawioConfig, tool: "drawio", drawio_bin,
    formats: ["png"], output_dir: "out/drawio",
    default_scan!(extensions: [".drawio"]),
);

generator_config!(LibreofficeConfig, tool: "libreoffice", libreoffice_bin,
    formats: ["pdf"], output_dir: "out/libreoffice",
    default_scan!(extensions: [".odp"]),
);

generator_config!(ProtobufConfig, tool: "protoc", protoc_bin,
    output_dir: "out/protobuf",
    default_scan!(scan_dir: "proto", extensions: [".proto"]),
);

generator_config!(SassConfig, tool: "sass", sass_bin,
    output_dir: "out/sass",
    default_scan!(scan_dir: "sass", extensions: [".scss", ".sass"]),
);

fn default_rustc() -> String { "rustc".into() }
fn default_rust_single_file_output_suffix() -> String { ".elf".into() }
fn default_rust_single_file_output_dir() -> String { "out/rust_single_file".into() }

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RustSingleFileConfig {
    #[serde(default = "default_rustc")]
    pub rustc: String,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default = "default_rust_single_file_output_suffix")]
    pub output_suffix: String,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_rust_single_file_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for RustSingleFileConfig {
    fn default() -> Self {
        Self {
            rustc: "rustc".into(),
            flags: Vec::new(),
            output_suffix: ".elf".into(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/rust_single_file".into(),
            batch: true,
            scan: default_scan!(scan_dir: "src", extensions: [".rs"]),
        }
    }
}

impl KnownFields for RustSingleFileConfig {
    fn known_fields() -> &'static [&'static str] {
        &["rustc", "flags", "output_suffix", "extra_inputs", "auto_inputs", "output_dir", "batch"]
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
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PdfuniteConfig {
    fn default() -> Self {
        Self {
            pdfunite_bin: "pdfunite".into(),
            source_dir: "marp/courses".into(),
            source_ext: ".md".into(),
            source_output_dir: "out/marp".into(),
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            output_dir: "out/pdfunite".into(),
            batch: true,
            scan: default_scan!(extensions: ["course.yaml"]),
        }
    }
}

impl KnownFields for PdfuniteConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "pdfunite_bin", "source_dir", "source_ext", "source_output_dir",
            "args", "extra_inputs", "auto_inputs", "output_dir", "batch",
        ]
    }
}

checker_config!(CpplintConfig, scan_dir: "src", extensions: [".c", ".cc", ".h", ".hh"]);

checker_config!(CheckpatchConfig, scan_dir: "src", extensions: [".c", ".h"]);

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ObjdumpConfig {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default = "default_objdump_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

fn default_objdump_output_dir() -> String {
    "out/objdump".into()
}

impl Default for ObjdumpConfig {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            extra_inputs: Vec::new(),
            output_dir: default_objdump_output_dir(),
            batch: true,
            scan: default_scan!(scan_dir: "out/cc_single_file", extensions: [".elf"]),
        }
    }
}

impl KnownFields for ObjdumpConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "args", "extra_inputs", "output_dir", "batch",
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

// --- encoding (UTF-8 validation) ---
checker_config!(EncodingConfig, extensions: [".py", ".rs", ".js", ".ts", ".c", ".cc", ".h", ".hh", ".java", ".rb", ".go", ".sh", ".bash", ".lua", ".pl", ".pm", ".php", ".md", ".yaml", ".yml", ".json", ".toml", ".xml", ".html", ".htm", ".css", ".scss", ".sass", ".tex", ".txt"]);

// --- duplicate_files (duplicate detection by SHA-256) ---
checker_config!(DuplicateFilesConfig, extensions: [".py", ".rs", ".js", ".ts", ".c", ".cc", ".h", ".hh", ".java", ".rb", ".go", ".sh", ".md", ".yaml", ".yml", ".json", ".toml", ".xml", ".html", ".css"]);

// --- marp_images (validate image references in Marp presentations) ---
checker_config!(MarpImagesConfig, extensions: [".md"]);

// --- license_header (verify license headers in source files) ---
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LicenseHeaderConfig {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub extra_inputs: Vec<String>,
    #[serde(default)]
    pub auto_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default)]
    pub header_lines: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for LicenseHeaderConfig {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            extra_inputs: Vec::new(),
            auto_inputs: Vec::new(),
            batch: true,
            header_lines: Vec::new(),
            scan: default_scan!(extensions: [".py", ".rs", ".js", ".ts", ".c", ".cc", ".h", ".hh", ".java", ".rb", ".go", ".sh", ".bash"]),
        }
    }
}

impl KnownFields for LicenseHeaderConfig {
    fn known_fields() -> &'static [&'static str] {
        &["args", "extra_inputs", "auto_inputs", "batch", "header_lines"]
    }
}

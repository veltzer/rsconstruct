use serde::{Deserialize, Serialize};

use super::{default_cc_compiler, default_cxx_compiler, default_true};

/// Configuration for the C/C++ dependency analyzer (`cpp`) — uses compiler -MM scanning.
/// External analyzer: runs gcc/g++ -MM and optional pkg-config/include_path_commands.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct CppAnalyzerConfig {
    /// Whether this analyzer is active. Set to false to disable without
    /// removing the stanza from rsconstruct.toml.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Additional include paths for header search
    #[serde(default)]
    pub include_paths: Vec<String>,
    /// pkg-config packages to query for include paths
    #[serde(default)]
    pub pkg_config: Vec<String>,
    /// Commands that output include paths (e.g., ["gcc -print-file-name=plugin"])
    #[serde(default)]
    pub include_path_commands: Vec<String>,
    /// Directory path segments to exclude from analysis (e.g., ["/kernel/", "/vendor/"])
    #[serde(default)]
    pub src_exclude_dirs: Vec<String>,
    /// C compiler (for -MM scanning)
    #[serde(default = "default_cc_compiler")]
    pub cc: String,
    /// C++ compiler (for -MM scanning)
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
            enabled: true,
            include_paths: Vec::new(),
            pkg_config: Vec::new(),
            include_path_commands: Vec::new(),
            src_exclude_dirs: Vec::new(),
            cc: default_cc_compiler(),
            cxx: default_cxx_compiler(),
            cflags: Vec::new(),
            cxxflags: Vec::new(),
        }
    }
}

/// Configuration for the in-process C/C++ dependency analyzer (`icpp`) — native regex scanner.
/// Native analyzer by default: pure Rust, no external commands.
/// Setting `pkg_config` or `include_path_commands` will invoke external tools once at
/// startup to discover additional include paths, but dependency scanning itself
/// remains in-process.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct IcppAnalyzerConfig {
    /// Whether this analyzer is active. Set to false to disable without
    /// removing the stanza from rsconstruct.toml.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Additional include paths for header search
    #[serde(default)]
    pub include_paths: Vec<String>,
    /// pkg-config packages to query for include paths (optional; invokes pkg-config if set)
    #[serde(default)]
    pub pkg_config: Vec<String>,
    /// Commands that output include paths (e.g., ["gcc -print-file-name=plugin"])
    /// Each command is run via `sh -c` and its stdout is added to the include search paths.
    #[serde(default)]
    pub include_path_commands: Vec<String>,
    /// Directory path segments to exclude from analysis
    #[serde(default)]
    pub src_exclude_dirs: Vec<String>,
    /// Whether to follow angle-bracket includes (`#include <foo.h>`).
    /// When false (default), `<angle>` includes are skipped entirely — even if they
    /// resolve through configured include paths — so system headers do not bloat
    /// the dependency graph. When true, angle-bracket includes are resolved and
    /// followed like quoted includes, but missing ones are still tolerated.
    #[serde(default)]
    pub follow_angle_brackets: bool,
    /// Whether to silently skip includes that cannot be resolved.
    /// When false (default), a quoted include (`#include "foo.h"`) that does not
    /// resolve produces a hard error. When true, unresolved includes of any kind
    /// are skipped without error.
    #[serde(default)]
    pub skip_not_found: bool,
}

impl Default for IcppAnalyzerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            include_paths: Vec::new(),
            pkg_config: Vec::new(),
            include_path_commands: Vec::new(),
            src_exclude_dirs: Vec::new(),
            follow_angle_brackets: false,
            skip_not_found: false,
        }
    }
}

/// Configuration for the Python dependency analyzer (`python`).
/// Scans Python source files for `import` statements.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct PythonAnalyzerConfig {
    /// Whether this analyzer is active. Set to false to disable without
    /// removing the stanza from rsconstruct.toml.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for PythonAnalyzerConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Configuration for the Markdown dependency analyzer (`markdown`).
/// Scans Markdown source files for image references.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct MarkdownAnalyzerConfig {
    /// Whether this analyzer is active. Set to false to disable without
    /// removing the stanza from rsconstruct.toml.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for MarkdownAnalyzerConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Configuration for the Tera template dependency analyzer (`tera`).
/// Scans Tera template source files for includes.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct TeraAnalyzerConfig {
    /// Whether this analyzer is active. Set to false to disable without
    /// removing the stanza from rsconstruct.toml.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for TeraAnalyzerConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}


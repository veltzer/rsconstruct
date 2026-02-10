use serde::{Deserialize, Serialize};

use super::{default_cc_compiler, default_cxx_compiler};
use super::processor_configs::IncludeScanner;

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

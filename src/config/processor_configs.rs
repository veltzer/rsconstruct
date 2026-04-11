use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{default_true, default_cc_compiler, default_cxx_compiler, default_output_suffix, KnownFields, ScanConfig,};

/// Universal processor config with all standard fields.
/// Checkers, generators, and simple processors all use this.
/// Fields not relevant to a given processor type are simply ignored.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StandardConfig {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub formats: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub dep_inputs: Vec<String>,
    #[serde(default)]
    pub dep_auto: Vec<String>,
    #[serde(default)]
    pub output_dir: String,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    /// Whether to cache this processor's outputs. Default true.
    /// Set to false to always rebuild and never store results.
    #[serde(default = "default_true")]
    pub cache: bool,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for StandardConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            formats: Vec::new(),
            args: Vec::new(),
            dep_inputs: Vec::new(),
            dep_auto: Vec::new(),
            output_dir: String::new(),
            batch: true,
            max_jobs: None,
            cache: true,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for StandardConfig {
    fn known_fields() -> &'static [&'static str] {
        &["command", "formats", "args", "dep_inputs", "dep_auto", "output_dir", "batch", "max_jobs"]
    }
    fn output_fields() -> &'static [&'static str] {
        &["command", "formats", "args", "output_dir"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("command",    "Path to the tool executable"),
            ("formats",    "Output formats to generate"),
            ("args",       "Extra arguments passed to the tool"),
            ("output_dir", "Directory where generated output files are written"),
        ]
    }
}

/// Simple checker config. No custom fields.
/// Unused StandardConfig fields: formats, output_dir.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CheckerConfig {
    #[serde(flatten)]
    pub standard: StandardConfig,
}
impl Default for CheckerConfig {
    fn default() -> Self { Self { standard: StandardConfig::default() } }
}
impl KnownFields for CheckerConfig {
    fn known_fields() -> &'static [&'static str] { StandardConfig::known_fields() }
    fn output_fields() -> &'static [&'static str] { StandardConfig::output_fields() }
    fn must_fields() -> &'static [&'static str] { StandardConfig::must_fields() }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] { StandardConfig::field_descriptions() }
}

/// Alias for CheckerConfig (used by SimpleChecker).
pub type CheckerConfigWithCommand = CheckerConfig;

/// Config for Creator processors — run a command and cache declared outputs.
#[derive(Debug, Deserialize, Serialize, Clone)]
/// Creator processor config.
/// Custom fields: output_dirs, output_files.
/// Unused StandardConfig fields: formats, output_dir.
pub struct CreatorConfig {
    /// Directories to cache after the command runs.
    #[serde(default)]
    pub output_dirs: Vec<String>,
    /// Individual files to cache after the command runs.
    #[serde(default)]
    pub output_files: Vec<String>,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for CreatorConfig {
    fn default() -> Self {
        Self {
            output_dirs: Vec::new(),
            output_files: Vec::new(),
            standard: StandardConfig::default(),
        }
    }
}

impl KnownFields for CreatorConfig {
    fn known_fields() -> &'static [&'static str] {
        &["command", "args", "dep_inputs", "dep_auto", "output_dirs", "output_files", "batch", "max_jobs", "cache"]
    }
    fn output_fields() -> &'static [&'static str] {
        &["command", "args", "output_dirs", "output_files"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("command",      "Command to run"),
            ("args",         "Extra arguments passed to the command"),
            ("output_dirs",  "Directories to cache after the command runs"),
            ("output_files", "Individual files to cache after the command runs"),
        ]
    }
}
/// Tera template processor config. No custom fields.
/// Unused StandardConfig fields: command, formats, output_dir.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TeraConfig {
    #[serde(flatten)]
    pub standard: StandardConfig,
}
impl Default for TeraConfig {
    fn default() -> Self { Self { standard: StandardConfig::default() } }
}
impl KnownFields for TeraConfig {
    fn known_fields() -> &'static [&'static str] { StandardConfig::known_fields() }
    fn output_fields() -> &'static [&'static str] { StandardConfig::output_fields() }
    fn must_fields() -> &'static [&'static str] { StandardConfig::must_fields() }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] { StandardConfig::field_descriptions() }
}

/// Mako template processor config. No custom fields.
/// Unused StandardConfig fields: command, formats, output_dir.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MakoConfig {
    #[serde(flatten)]
    pub standard: StandardConfig,
}
impl Default for MakoConfig {
    fn default() -> Self { Self { standard: StandardConfig::default() } }
}
impl KnownFields for MakoConfig {
    fn known_fields() -> &'static [&'static str] { StandardConfig::known_fields() }
    fn output_fields() -> &'static [&'static str] { StandardConfig::output_fields() }
    fn must_fields() -> &'static [&'static str] { StandardConfig::must_fields() }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] { StandardConfig::field_descriptions() }
}

/// Jinja2 template processor config. No custom fields.
/// Unused StandardConfig fields: command, formats, output_dir.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Jinja2Config {
    #[serde(flatten)]
    pub standard: StandardConfig,
}
impl Default for Jinja2Config {
    fn default() -> Self { Self { standard: StandardConfig::default() } }
}
impl KnownFields for Jinja2Config {
    fn known_fields() -> &'static [&'static str] { StandardConfig::known_fields() }
    fn output_fields() -> &'static [&'static str] { StandardConfig::output_fields() }
    fn must_fields() -> &'static [&'static str] { StandardConfig::must_fields() }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] { StandardConfig::field_descriptions() }
}






pub type MarpImagesConfig = CheckerConfig;

/// ClangTidy config. Custom fields: compiler_args.
/// Unused StandardConfig fields: command, formats, output_dir.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClangTidyConfig {
    #[serde(default)]
    pub compiler_args: Vec<String>,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for ClangTidyConfig {
    fn default() -> Self {
        Self {
            compiler_args: Vec::new(),
            standard: StandardConfig::default(),
        }
    }
}

impl KnownFields for ClangTidyConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "args", "compiler_args", "dep_inputs", "dep_auto", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["args", "compiler_args"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("args",          "Extra arguments passed to clang-tidy"),
            ("compiler_args", "Compiler flags forwarded to clang-tidy for parsing"),
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

/// CcSingleFile config. Custom fields: cc, cxx, cflags, cxxflags, ldflags, output_suffix, compilers, include_paths, include_scanner.
/// Unused StandardConfig fields: command, formats.
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
    /// Method for scanning header dependencies (native or compiler)
    #[serde(default)]
    pub include_scanner: IncludeScanner,
    #[serde(flatten)]
    pub standard: StandardConfig,
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
            cc: "gcc".into(),
            cxx: "g++".into(),
            cflags: Vec::new(),
            cxxflags: Vec::new(),
            ldflags: Vec::new(),
            output_suffix: ".elf".into(),
            compilers: Vec::new(),
            include_paths: Vec::new(),
            include_scanner: IncludeScanner::default(),
            standard: StandardConfig {
                output_dir: "out/cc_single_file".into(),
                ..StandardConfig::default()
            },
        }
    }
}

impl KnownFields for CcSingleFileConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "cc", "cxx", "cflags", "cxxflags", "ldflags", "output_suffix",
            "compilers", "include_paths", "dep_inputs", "dep_auto", "output_dir",
            "include_scanner", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &[
            "cc", "cxx", "cflags", "cxxflags", "ldflags", "output_suffix",
            "compilers", "include_paths", "output_dir", "include_scanner",
        ]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("cc",              "C compiler executable"),
            ("cxx",             "C++ compiler executable"),
            ("output_suffix",   "Suffix appended to output binary names"),
            ("compilers",       "Named compiler profiles (overrides cc/cxx when set)"),
            ("include_paths",   "Additional header search directories"),
            ("output_dir",      "Directory where compiled binaries are written"),
            ("include_scanner", "Header dependency scanner: native (fast) or compiler (accurate)"),
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

/// CC (full C/C++ project) config. Custom: cc, cxx, cflags, cxxflags, ldflags, include_dirs, single_invocation, cache_output_dir.
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
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
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
            cache_output_dir: true,
            standard: StandardConfig::default(),
        }
    }
}

impl KnownFields for CcConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "cc", "cxx", "cflags", "cxxflags", "ldflags",
            "include_dirs", "single_invocation",
            "dep_inputs", "cache_output_dir", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &[
            "cc", "cxx", "cflags", "cxxflags", "ldflags",
            "include_dirs", "single_invocation",
        ]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("cc",                "C compiler executable"),
            ("cxx",               "C++ compiler executable"),
            ("cflags",            "Flags passed to the C compiler"),
            ("cxxflags",          "Flags passed to the C++ compiler"),
            ("ldflags",           "Flags passed to the linker"),
            ("include_dirs",      "Additional header search directories"),
            ("single_invocation", "Compile all sources in one compiler call"),
            ("cache_output_dir",  "Cache the entire output directory as a unit"),
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

/// Linux module config. No custom fields.
/// Unused StandardConfig fields: command, formats, output_dir, args.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LinuxModuleConfig {
    #[serde(flatten)]
    pub standard: StandardConfig,
}
impl Default for LinuxModuleConfig {
    fn default() -> Self { Self { standard: StandardConfig::default() } }
}
impl KnownFields for LinuxModuleConfig {
    fn known_fields() -> &'static [&'static str] { StandardConfig::known_fields() }
    fn output_fields() -> &'static [&'static str] { StandardConfig::output_fields() }
    fn must_fields() -> &'static [&'static str] { StandardConfig::must_fields() }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] { StandardConfig::field_descriptions() }
}

fn default_zspell_language() -> String {
    "en_US".into()
}

fn default_zspell_words_file() -> String {
    ".zspell-words".into()
}

/// Zspell config. Custom fields: language, words_file, auto_add_words.
/// Unused StandardConfig fields: command, formats, output_dir.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ZspellConfig {
    #[serde(default = "default_zspell_language")]
    pub language: String,
    #[serde(default = "default_zspell_words_file")]
    pub words_file: String,
    /// When true, automatically add misspelled words to words_file instead of failing
    #[serde(default)]
    pub auto_add_words: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for ZspellConfig {
    fn default() -> Self {
        Self {
            language: "en_US".into(),
            words_file: ".zspell-words".into(),
            auto_add_words: false,
            standard: StandardConfig::default(),
        }
    }
}

impl KnownFields for ZspellConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "language", "words_file", "auto_add_words", "dep_inputs", "dep_auto", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["language", "auto_add_words"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("language",       "Language/locale for spell checking"),
            ("words_file",     "Path to a personal dictionary file"),
            ("auto_add_words", "When true, automatically add misspelled words to words_file instead of failing"),
        ]
    }
}

fn default_make() -> String {
    "make".into()
}

fn default_cargo() -> String {
    "cargo".into()
}

fn default_cargo_profiles() -> Vec<String> {
    vec!["dev".into(), "release".into()]
}

/// Cargo config. Custom: cargo, profiles, cache_output_dir.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CargoConfig {
    #[serde(default = "default_cargo")]
    pub cargo: String,
    #[serde(default = "default_cargo_profiles")]
    pub profiles: Vec<String>,
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for CargoConfig {
    fn default() -> Self {
        Self {
            cargo: "cargo".into(),
            profiles: default_cargo_profiles(),
            cache_output_dir: true,
            standard: StandardConfig {
                command: "build".into(),
                ..StandardConfig::default()
            },
        }
    }
}

impl KnownFields for CargoConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "cargo", "command", "args", "dep_inputs", "profiles",
            "cache_output_dir", "batch", "max_jobs", "src_extensions", "src_exclude_dirs", "src_exclude_files",
            "src_exclude_paths",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["cargo", "command", "args", "profiles"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("cargo",   "Path to the cargo executable"),
            ("command", "Cargo subcommand to run (e.g. build, test)"),
            ("args",    "Extra arguments passed to cargo"),
            ("profiles", "Build profiles to run (e.g. dev, release)"),
        ]
    }
}


#[derive(Debug, Deserialize, Serialize, Clone)]
/// Clippy config. Custom: cargo.
pub struct ClippyConfig {
    #[serde(default = "default_cargo")]
    pub cargo: String,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for ClippyConfig {
    fn default() -> Self {
        Self {
            cargo: "cargo".into(),
            standard: StandardConfig { command: "clippy".into(), ..StandardConfig::default() },
        }
    }
}

impl KnownFields for ClippyConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "cargo", "command", "args", "dep_inputs", "dep_auto", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["cargo", "command", "args"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("cargo",   "Path to the cargo executable"),
            ("command", "Cargo subcommand to run (defaults to clippy)"),
            ("args",    "Extra arguments passed to cargo clippy"),
        ]
    }
}

/// Make config. Custom: make, target.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MakeConfig {
    #[serde(default = "default_make")]
    pub make: String,
    #[serde(default)]
    pub target: String,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for MakeConfig {
    fn default() -> Self {
        Self { make: "make".into(), target: String::new(), standard: StandardConfig::default() }
    }
}

impl KnownFields for MakeConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "make", "args", "target", "dep_inputs", "dep_auto", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["make", "args", "target"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("make",   "Path to the make executable"),
            ("args",   "Extra arguments passed to make"),
            ("target", "Make target to build"),
        ]
    }
}








pub type JsonSchemaConfig = CheckerConfig;

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
    #[serde(flatten)]
    pub standard: StandardConfig,
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
            standard: StandardConfig::default(),
        }
    }
}

impl KnownFields for TagsConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "output", "tags_dir", "required_fields", "required_values",
            "unique_fields", "field_types", "required_field_groups", "sorted_tags",
            "check_unused", "dep_inputs", "dep_auto", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &[
            "output", "tags_dir", "required_fields", "required_values",
            "unique_fields", "field_types", "required_field_groups", "sorted_tags",
            "check_unused",
        ]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("output",                "Output tags database file path"),
            ("tags_dir",              "Directory containing tag list files"),
            ("required_fields",       "Frontmatter fields that every markdown file must have"),
            ("required_values",       "Scalar fields whose values must exist in the tag lists file"),
            ("unique_fields",         "Fields whose values must be unique across all files"),
            ("field_types",           "Expected types for fields: scalar, list, or number"),
            ("required_field_groups", "Groups of fields where at least one group must be fully present"),
            ("sorted_tags",           "Require list-type fields to have items in sorted order"),
            ("check_unused",          "Fail the build when tags in the allowlist are not used by any file"),
        ]
    }
}


/// Script processor config. No custom fields.
/// Unused StandardConfig fields: formats, output_dir.
/// Note: empty command means "no command configured".
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ScriptConfig {
    #[serde(flatten)]
    pub standard: StandardConfig,
}
impl Default for ScriptConfig {
    fn default() -> Self { Self { standard: StandardConfig::default() } }
}
impl KnownFields for ScriptConfig {
    fn known_fields() -> &'static [&'static str] { StandardConfig::known_fields() }
    fn output_fields() -> &'static [&'static str] { StandardConfig::output_fields() }
    fn must_fields() -> &'static [&'static str] { StandardConfig::must_fields() }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] { StandardConfig::field_descriptions() }
}

fn default_generator_output_dir() -> String {
    "out/generator".into()
}

fn default_generator_output_extension() -> String {
    "out".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GeneratorConfig {
    pub command: Option<String>,
    #[serde(default = "default_generator_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_generator_output_extension")]
    pub output_extension: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub dep_inputs: Vec<String>,
    #[serde(default)]
    pub dep_auto: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            command: None,
            output_dir: "out/generator".into(),
            output_extension: "out".into(),
            args: Vec::new(),
            dep_inputs: Vec::new(),
            dep_auto: Vec::new(),
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for GeneratorConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "command", "output_dir", "output_extension", "args",
            "dep_inputs", "dep_auto", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["command", "output_dir", "output_extension", "args"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("command",          "Script or executable to run as a generator (required)"),
            ("args",             "Extra arguments passed to the command before file paths"),
            ("output_dir",       "Directory where generated output files are written"),
            ("output_extension", "File extension for generated output files"),
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
    pub output_files: Vec<String>,
    /// Output directories produced by the command
    #[serde(default)]
    pub output_dirs: Vec<String>,
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
            output_files: Vec::new(),
            output_dirs: Vec::new(),
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for ExplicitConfig {
    fn known_fields() -> &'static [&'static str] {
        &["command", "args", "inputs", "input_globs", "output_files", "output_dirs"]
    }
    fn output_fields() -> &'static [&'static str] {
        &["command", "args", "inputs", "input_globs", "output_files", "output_dirs"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("command",      "Command to run to produce the outputs"),
            ("args",         "Extra arguments passed before input/output paths"),
            ("inputs",       "Explicit list of input files"),
            ("input_globs",  "Glob patterns for input files"),
            ("output_files", "Output files produced by the command"),
            ("output_dirs",  "Output directories produced by the command"),
        ]
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
    pub dep_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PipConfig {
    fn default() -> Self {
        Self {
            pip: "pip".into(),
            args: Vec::new(),
            dep_inputs: Vec::new(),
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for PipConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "pip", "args", "dep_inputs", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["pip", "args"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("pip",  "Path to the pip executable"),
            ("args", "Extra arguments passed to pip install"),
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
    pub dep_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
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
            dep_inputs: Vec::new(),
            cache_output_dir: true,
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for SphinxConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "sphinx_build", "output_dir", "working_dir", "args", "dep_inputs", "cache_output_dir", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["sphinx_build", "output_dir", "working_dir", "args"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("sphinx_build", "Path to the sphinx-build executable"),
            ("output_dir",   "Directory where built docs are written"),
            ("working_dir",  "Working directory for sphinx-build (defaults to conf.py location)"),
            ("args",         "Extra arguments passed to sphinx-build"),
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
    pub dep_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MdbookConfig {
    fn default() -> Self {
        Self {
            mdbook: "mdbook".into(),
            output_dir: "book".into(),
            args: Vec::new(),
            dep_inputs: Vec::new(),
            cache_output_dir: true,
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for MdbookConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "mdbook", "output_dir", "args", "dep_inputs", "cache_output_dir", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["mdbook", "output_dir", "args"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("mdbook",     "Path to the mdbook executable"),
            ("output_dir", "Directory where the built book is written"),
            ("args",       "Extra arguments passed to mdbook build"),
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
    pub dep_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for NpmConfig {
    fn default() -> Self {
        Self {
            npm: "npm".into(),
            command: "install".into(),
            args: Vec::new(),
            dep_inputs: Vec::new(),
            cache_output_dir: true,
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for NpmConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "npm", "command", "args", "dep_inputs", "cache_output_dir", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["npm", "command", "args"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("npm",     "Path to the npm executable"),
            ("command", "npm command to run (e.g. install, run)"),
            ("args",    "Arguments passed to the npm command"),
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
    pub dep_inputs: Vec<String>,
    #[serde(default = "default_mdl_auto_inputs")]
    pub dep_auto: Vec<String>,
    #[serde(default = "default_gem_stamp")]
    pub gem_stamp: String,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
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
            dep_inputs: Vec::new(),
            dep_auto: default_mdl_auto_inputs(),
            gem_stamp: "out/gem/root.stamp".into(),
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for MdlConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "local_repo", "gem_home", "mdl_bin", "args", "dep_inputs", "dep_auto", "gem_stamp", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["local_repo", "gem_home", "mdl_bin", "args", "gem_stamp"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("local_repo", "Use a local gem repository instead of system install"),
            ("gem_home",   "Path to the local gem repository"),
            ("mdl_bin",    "Path to the mdl executable"),
            ("args",       "Extra arguments passed to mdl"),
            ("gem_stamp",  "Stamp file tracking the local gem installation"),
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
    pub dep_inputs: Vec<String>,
    #[serde(default = "default_markdownlint_auto_inputs")]
    pub dep_auto: Vec<String>,
    #[serde(default = "default_npm_stamp")]
    pub npm_stamp: String,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for MarkdownlintConfig {
    fn default() -> Self {
        Self {
            local_repo: false,
            markdownlint_bin: "markdownlint".into(),
            args: Vec::new(),
            dep_inputs: Vec::new(),
            dep_auto: default_markdownlint_auto_inputs(),
            npm_stamp: "out/npm/root.stamp".into(),
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for MarkdownlintConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "local_repo", "markdownlint_bin", "args", "dep_inputs", "dep_auto", "npm_stamp", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["local_repo", "markdownlint_bin", "args", "npm_stamp"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("local_repo",       "Use a local npm repository instead of system install"),
            ("markdownlint_bin", "Path to the markdownlint executable"),
            ("args",             "Extra arguments passed to markdownlint"),
            ("npm_stamp",        "Stamp file tracking the local npm installation"),
        ]
    }
}

fn default_aspell() -> String {
    "aspell".into()
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
    #[serde(default = "default_aspell_conf")]
    pub conf: String,
    #[serde(default)]
    pub auto_add_words: bool,
    #[serde(default = "default_aspell_words_file")]
    pub words_file: String,
    #[serde(default)]
    pub dep_inputs: Vec<String>,
    #[serde(default = "default_aspell_auto_inputs")]
    pub dep_auto: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for AspellConfig {
    fn default() -> Self {
        Self {
            aspell: "aspell".into(),
            args: Vec::new(),
            conf: ".aspell.conf".into(),
            auto_add_words: false,
            words_file: ".aspell.en.pws".into(),
            dep_inputs: Vec::new(),
            dep_auto: default_aspell_auto_inputs(),
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for AspellConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "aspell", "args", "conf", "auto_add_words", "words_file", "dep_inputs", "dep_auto", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["aspell", "args", "conf", "auto_add_words"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("aspell",         "Path to the aspell executable"),
            ("args",           "Extra arguments passed to aspell"),
            ("conf",           "Path to the aspell configuration file"),
            ("auto_add_words", "When true, automatically add misspelled words to words_file instead of failing"),
            ("words_file",     "Path to the personal word list file"),
        ]
    }
}

pub type AsciiConfig = CheckerConfig;

fn default_terms_dir() -> String {
    "terms".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TermsConfig {
    #[serde(default = "default_terms_dir")]
    pub terms_dir: String,
    #[serde(default)]
    pub dep_inputs: Vec<String>,
    #[serde(default)]
    pub dep_auto: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for TermsConfig {
    fn default() -> Self {
        Self {
            terms_dir: "terms".into(),
            dep_inputs: Vec::new(),
            dep_auto: Vec::new(),
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for TermsConfig {
    fn known_fields() -> &'static [&'static str] {
        &["terms_dir", "dep_inputs", "dep_auto", "batch", "max_jobs"]
    }
    fn output_fields() -> &'static [&'static str] {
        &["terms_dir"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("terms_dir", "Directory containing term definition files"),
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
    #[serde(default = "default_pdflatex")]
    pub pdflatex: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub dep_inputs: Vec<String>,
    #[serde(default)]
    pub dep_auto: Vec<String>,
    #[serde(default = "default_pdflatex_runs")]
    pub runs: usize,
    #[serde(default = "default_true")]
    pub qpdf: bool,
    #[serde(default = "default_pdflatex_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for PdflatexConfig {
    fn default() -> Self {
        Self {
            pdflatex: "pdflatex".into(),
            args: Vec::new(),
            dep_inputs: Vec::new(),
            dep_auto: Vec::new(),
            runs: 2,
            qpdf: true,
            output_dir: "out/pdflatex".into(),
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for PdflatexConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "pdflatex", "args", "dep_inputs", "dep_auto", "runs", "qpdf", "output_dir", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["pdflatex", "args", "runs", "qpdf", "output_dir"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("pdflatex",  "Path to the pdflatex executable"),
            ("args",      "Extra arguments passed to pdflatex"),
            ("runs",      "Number of pdflatex compilation passes"),
            ("qpdf",      "Run qpdf to optimize the output PDF"),
            ("output_dir","Directory where compiled PDFs are written"),
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
    #[serde(default = "default_bundler")]
    pub bundler: String,
    #[serde(default = "default_bundler_command")]
    pub command: String,
    #[serde(default = "default_gem_home")]
    pub gem_home: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub dep_inputs: Vec<String>,
    #[serde(default = "default_true")]
    pub cache_output_dir: bool,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
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
            dep_inputs: Vec::new(),
            cache_output_dir: true,
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for GemConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "bundler", "command", "gem_home", "args", "dep_inputs", "cache_output_dir", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &["bundler", "command", "gem_home", "args"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("bundler",  "Path to the bundle executable"),
            ("command",  "Bundler subcommand to run (e.g. install, exec)"),
            ("gem_home", "Directory where gems are installed"),
            ("args",     "Extra arguments passed to bundler"),
        ]
    }
}






pub type IjqConfig = CheckerConfig;

pub type IjsonlintConfig = CheckerConfig;

pub type IyamllintConfig = CheckerConfig;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IyamlschemaConfig {
    #[serde(default = "default_true")]
    pub check_ordering: bool,
    #[serde(default)]
    pub dep_inputs: Vec<String>,
    #[serde(default)]
    pub dep_auto: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for IyamlschemaConfig {
    fn default() -> Self {
        Self {
            check_ordering: true,
            dep_inputs: Vec::new(),
            dep_auto: Vec::new(),
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for IyamlschemaConfig {
    fn known_fields() -> &'static [&'static str] {
        &["check_ordering", "dep_inputs", "dep_auto", "batch", "max_jobs"]
    }
    fn output_fields() -> &'static [&'static str] {
        &["check_ordering"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("check_ordering", "Require YAML keys to appear in the order defined by the schema"),
        ]
    }
}

pub type ItaploConfig = CheckerConfig;

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
    pub dep_inputs: Vec<String>,
    #[serde(default)]
    pub dep_auto: Vec<String>,
    #[serde(default = "default_rust_single_file_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for RustSingleFileConfig {
    fn default() -> Self {
        Self {
            rustc: "rustc".into(),
            flags: Vec::new(),
            output_suffix: ".elf".into(),
            dep_inputs: Vec::new(),
            dep_auto: Vec::new(),
            output_dir: "out/rust_single_file".into(),
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for RustSingleFileConfig {
    fn known_fields() -> &'static [&'static str] {
        &["rustc", "flags", "output_suffix", "dep_inputs", "dep_auto", "output_dir", "batch", "max_jobs"]
    }
    fn output_fields() -> &'static [&'static str] {
        &["rustc", "flags", "output_suffix", "output_dir"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("rustc",         "Path to the rustc executable"),
            ("flags",         "Extra flags passed to rustc"),
            ("output_suffix", "Suffix appended to output binary names"),
            ("output_dir",    "Directory where compiled binaries are written"),
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
    pub dep_inputs: Vec<String>,
    #[serde(default)]
    pub dep_auto: Vec<String>,
    #[serde(default = "default_pdfunite_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
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
            dep_inputs: Vec::new(),
            dep_auto: Vec::new(),
            output_dir: "out/pdfunite".into(),
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for PdfuniteConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "pdfunite_bin", "source_dir", "source_ext", "source_output_dir",
            "args", "dep_inputs", "dep_auto", "output_dir", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &[
            "pdfunite_bin", "source_dir", "source_ext", "source_output_dir",
            "args", "output_dir",
        ]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("pdfunite_bin",      "Path to the pdfunite executable"),
            ("source_dir",        "Directory containing course YAML files listing PDFs to merge"),
            ("source_ext",        "Extension of source files used to find PDFs"),
            ("source_output_dir", "Directory where source PDFs (to be merged) are located"),
            ("args",              "Extra arguments passed to pdfunite"),
            ("output_dir",        "Directory where merged PDFs are written"),
        ]
    }
}

// --- ipdfunite (internal PDF merge, no external binary) ---

fn default_ipdfunite_source_dir() -> String {
    "marp/courses".into()
}

fn default_ipdfunite_source_ext() -> String {
    ".md".into()
}

fn default_ipdfunite_source_output_dir() -> String {
    "out/marp".into()
}

fn default_ipdfunite_output_dir() -> String {
    "out/ipdfunite".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IpdfuniteConfig {
    #[serde(default = "default_ipdfunite_source_dir")]
    pub source_dir: String,
    #[serde(default = "default_ipdfunite_source_ext")]
    pub source_ext: String,
    #[serde(default = "default_ipdfunite_source_output_dir")]
    pub source_output_dir: String,
    #[serde(default)]
    pub dep_inputs: Vec<String>,
    #[serde(default)]
    pub dep_auto: Vec<String>,
    #[serde(default = "default_ipdfunite_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for IpdfuniteConfig {
    fn default() -> Self {
        Self {
            source_dir: "marp/courses".into(),
            source_ext: ".md".into(),
            source_output_dir: "out/marp".into(),
            dep_inputs: Vec::new(),
            dep_auto: Vec::new(),
            output_dir: "out/ipdfunite".into(),
            batch: true,
            max_jobs: None,
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for IpdfuniteConfig {
    fn known_fields() -> &'static [&'static str] {
        &[
            "source_dir", "source_ext", "source_output_dir",
            "dep_inputs", "dep_auto", "output_dir", "batch", "max_jobs",
        ]
    }
    fn output_fields() -> &'static [&'static str] {
        &[
            "source_dir", "source_ext", "source_output_dir", "output_dir",
        ]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("source_dir",        "Directory containing course YAML files listing PDFs to merge"),
            ("source_ext",        "Extension of source files used to find PDFs"),
            ("source_output_dir", "Directory where source PDFs (to be merged) are located"),
            ("output_dir",        "Directory where merged PDFs are written"),
        ]
    }
}



// --- tidy (HTML validator) ---

// --- stylelint (CSS linter) ---

// --- jslint (JavaScript linter) ---

// --- standard (JavaScript style checker) ---

// --- htmllint (HTML linter) ---

// --- php_lint (PHP syntax checker) ---

// --- perlcritic (Perl code analyzer) ---

// --- xmllint (XML validator) ---

// --- svglint (SVG linter) ---

// --- checkstyle (Java style checker) ---

// --- yq (YAML processor/validator) ---

// --- cmake (CMake build system) ---

// --- docker (Docker image build) ---

// --- jekyll (Static site generator) ---
pub type JekyllConfig = CheckerConfig;

// --- slidev (Slidev presentations) ---

// --- encoding (UTF-8 validation) ---
pub type EncodingConfig = CheckerConfig;

// --- duplicate_files (duplicate detection by SHA-256) ---
pub type DuplicateFilesConfig = CheckerConfig;

// --- marp_images (validate image references in Marp presentations) ---

// --- license_header (verify license headers in source files) ---
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LicenseHeaderConfig {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub dep_inputs: Vec<String>,
    #[serde(default)]
    pub dep_auto: Vec<String>,
    #[serde(default = "default_true")]
    pub batch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_jobs: Option<usize>,
    #[serde(default)]
    pub header_lines: Vec<String>,
    #[serde(flatten)]
    pub scan: ScanConfig,
}

impl Default for LicenseHeaderConfig {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            dep_inputs: Vec::new(),
            dep_auto: Vec::new(),
            batch: true,
            max_jobs: None,
            header_lines: Vec::new(),
            scan: ScanConfig::default(),
        }
    }
}

impl KnownFields for LicenseHeaderConfig {
    fn known_fields() -> &'static [&'static str] {
        &["args", "dep_inputs", "dep_auto", "batch", "max_jobs", "header_lines"]
    }
    fn output_fields() -> &'static [&'static str] {
        &["args", "header_lines"]
    }
    fn field_descriptions() -> &'static [(&'static str, &'static str)] {
        &[
            ("args",         "Extra arguments passed to the license header checker"),
            ("header_lines", "Lines of the license header that must appear at the top of each file"),
        ]
    }

}


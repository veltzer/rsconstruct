use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use std::str::FromStr;

#[derive(Parser)]
#[command(name = "rsconstruct")]
#[command(version = concat!(env!("CARGO_PKG_VERSION")))]
#[command(about = "Rust Build Tool - Incremental build system with templates", long_about = None)]
pub struct Cli {
    /// Show skip/restore/cache messages during build
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// What to show for output files (none, basename, path)
    #[arg(short = 'O', long, global = true, value_enum, default_value = "none")]
    pub output_display: OutputDisplay,

    /// What to show for input files (none, source, all)
    #[arg(short = 'I', long, global = true, value_enum, default_value = "source")]
    pub input_display: InputDisplay,

    /// Path format for displayed files (basename, path)
    #[arg(short = 'P', long, global = true, value_enum, default_value = "path")]
    pub path_format: PathFormat,

    /// Print each child process command before it is executed
    #[arg(long, global = true)]
    pub show_child_processes: bool,

    /// Show tool output even on success (default: only show on failure)
    #[arg(long, global = true)]
    pub show_output: bool,

    /// Output in JSON Lines format (machine-readable)
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress all output except errors (useful for CI)
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Show build phase messages (discover, add_dependencies, etc.)
    #[arg(long, global = true)]
    pub phases: bool,

    /// Disable persistent mtime checksum cache (useful for CI/CD where the
    /// cache won't survive the build and the write overhead isn't worth it)
    #[arg(long, global = true)]
    pub no_mtime_cache: bool,

    #[command(subcommand)]
    pub command: Commands,
}

impl Cli {
    /// Get the display options from CLI arguments
    pub fn display_options(&self) -> DisplayOptions {
        DisplayOptions {
            output: self.output_display,
            input: self.input_display,
            path_format: self.path_format,
        }
    }
}

/// Output format for the dependency graph
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum GraphFormat {
    /// DOT format (Graphviz)
    Dot,
    /// Mermaid diagram format (Markdown-friendly)
    Mermaid,
    /// JSON format (machine-readable)
    Json,
    /// Plain text hierarchical view
    Text,
    /// SVG format (requires Graphviz dot)
    #[default]
    Svg,
}

/// Viewer for opening the graph
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum GraphViewer {
    /// Open as HTML with Mermaid in browser (no dependencies)
    Mermaid,
    /// Generate and open SVG using Graphviz dot
    #[default]
    Svg,
}

/// Build phases that can be stopped after
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum BuildPhase {
    /// Stop after discovering products (before dependency scanning)
    Discover,
    /// Stop after adding dependencies (before resolving graph)
    AddDependencies,
    /// Stop after resolving the dependency graph (before execution)
    Resolve,
    /// Stop after classifying products (show skip/restore/build counts)
    Classify,
    /// Run the full build (default)
    #[default]
    Build,
}

/// What to show for output files in build messages
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputDisplay {
    /// Don't show output files
    #[default]
    None,
    /// Show only the filename (e.g., "main.elf")
    Basename,
    /// Show full relative path (e.g., "out/cc_single_file/main.elf")
    Path,
}

/// What to show for input files in build messages
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum InputDisplay {
    /// Don't show input files
    None,
    /// Show only the primary source file (first input)
    #[default]
    Source,
    /// Show all input files including headers/dependencies
    All,
}

/// Path format for displayed files
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum PathFormat {
    /// Show only the filename (e.g., "main.c")
    Basename,
    /// Show full relative path (e.g., "src/main.c")
    #[default]
    Path,
}

/// Display options for product output in build messages
#[derive(Debug, Clone, Copy)]
pub struct DisplayOptions {
    pub output: OutputDisplay,
    pub input: InputDisplay,
    pub path_format: PathFormat,
}

impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            output: OutputDisplay::None,
            input: InputDisplay::Source,
            path_format: PathFormat::Path,
        }
    }
}

impl DisplayOptions {
    /// Minimal display: just input source basename
    pub fn minimal() -> Self {
        Self {
            output: OutputDisplay::None,
            input: InputDisplay::Source,
            path_format: PathFormat::Basename,
        }
    }
}

#[derive(Subcommand)]
pub enum Commands {
    /// Execute an incremental build
    Build {
        /// Force rebuild even if files haven't changed
        #[arg(short, long)]
        force: bool,

        /// Show what would be built without executing anything
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Verify tool versions against .tools.versions before building
        #[arg(long)]
        verify_tool_versions: bool,

        /// Stop after a specific build phase
        #[arg(long, value_enum, default_value = "build")]
        stop_after: BuildPhase,

        #[command(flatten)]
        shared: SharedBuildArgs,
    },
    /// Manage the build cache
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
    /// Clean build artifacts
    Clean {
        #[command(subcommand)]
        action: Option<CleanAction>,
    },
    /// Generate shell completion scripts
    Complete {
        /// The shells to generate completions for (if none specified, uses config file)
        #[arg(value_enum)]
        shells: Vec<Shell>,
    },
    /// Show or inspect configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Show source file dependencies
    Deps {
        #[command(subcommand)]
        action: DepsAction,
    },
    /// Check build environment (requires config)
    Doctor,
    /// Display the build dependency graph
    Graph {
        #[command(subcommand)]
        action: GraphAction,
    },
    /// Show project information
    Info {
        #[command(subcommand)]
        action: InfoAction,
    },
    /// Initialize a new rsconstruct project (no config needed)
    Init,
    /// Manage processors
    Processors {
        #[command(subcommand)]
        action: ProcessorAction,
    },
    /// Count source lines of code (SLOC) by language (no config needed)
    Sloc {
        /// Show COCOMO effort/cost estimation
        #[arg(long)]
        cocomo: bool,
        /// Annual salary for COCOMO cost estimation (default: 56286)
        #[arg(long, default_value = "56286")]
        salary: u64,
    },
    /// Smart config manipulation commands
    Smart {
        #[command(subcommand)]
        action: SmartAction,
    },
    /// Show the status of each product (requires config)
    Status {
        /// Show source file counts by extension per processor
        #[arg(long)]
        breakdown: bool,
    },
    /// Manage term checking and fixing in markdown files
    Terms {
        #[command(subcommand)]
        action: TermsAction,
    },
    /// Search and query frontmatter tags from markdown files
    Tags {
        #[command(subcommand)]
        action: TagsAction,
    },
    /// Manage external tool dependencies
    Tools {
        #[command(subcommand)]
        action: ToolsAction,
    },
    /// Create symlinks from source folders to target folders (requires config)
    SymlinkInstall,
    /// Print version information (no config needed)
    Version,
    /// Watch source files and auto-rebuild on changes (requires config)
    Watch {
        #[command(flatten)]
        shared: SharedBuildArgs,
    },
    /// Manage the web request cache
    #[command(name = "webcache")]
    WebCache {
        #[command(subcommand)]
        action: WebCacheAction,
    },
}

#[derive(Subcommand)]
pub enum SmartAction {
    /// Disable all processors in rsconstruct.toml (no config needed)
    DisableAll,
    /// Enable all processors in rsconstruct.toml (no config needed)
    EnableAll,
    /// Enable only processors whose files are detected in the project (requires config)
    EnableDetected,
    /// Disable a single processor in rsconstruct.toml (no config needed)
    Disable {
        /// Processor name
        name: String,
    },
    /// Enable a single processor in rsconstruct.toml (no config needed)
    Enable {
        /// Processor name
        name: String,
    },
    /// Disable all, then enable only detected processors (requires config)
    Minimal,
    /// Remove all [processor.*] sections, returning to pure defaults (no config needed)
    Reset,
    /// Auto-detect relevant processors and add them to rsconstruct.toml (requires config)
    Auto,
    /// Enable only processors whose files are detected and tools are installed (requires config)
    EnableIfAvailable,
    /// Disable all, then enable only the listed processors (no config needed)
    Only {
        /// Processor names to enable
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Remove processors from rsconstruct.toml that don't match any files (requires config)
    RemoveNoFileProcessors,
}

#[derive(Subcommand)]
pub enum InfoAction {
    /// Show source file counts by extension (requires config)
    Source,
}

#[derive(Subcommand)]
pub enum GraphAction {
    /// Print the dependency graph to stdout (requires config)
    Show {
        /// Output format
        #[arg(short, long, value_enum, default_value = "svg")]
        format: GraphFormat,
    },
    /// Open the dependency graph in a viewer (requires config)
    View {
        /// Viewer to use
        #[arg(long, value_enum, default_value = "svg")]
        viewer: GraphViewer,
    },
    /// Show graph statistics (requires config)
    Stats,
    /// List files on disk not referenced by any product in the graph (requires config)
    Unreferenced {
        /// File extensions to check, comma-separated (e.g. .svg,.png)
        #[arg(short, long, value_delimiter = ',', required = true)]
        extensions: Vec<String>,
        /// Delete the unreferenced files
        #[arg(long)]
        rm: bool,
    },
}

#[derive(Subcommand)]
pub enum CleanAction {
    /// Remove build output files, preserves cache (requires config) [default]
    Outputs,
    /// Remove all build outputs and cache directories (requires config)
    All,
    /// Hard clean using git clean (requires config)
    Git,
    /// Remove files not tracked by git and not known as build outputs (requires config)
    Unknown {
        /// Show what would be removed without actually deleting
        #[arg(long)]
        dry_run: bool,
        /// Include gitignored files as unknown (by default they are skipped)
        #[arg(long)]
        no_gitignore: bool,
    },
}

#[derive(Subcommand)]
pub enum CacheAction {
    /// Clear the entire cache (no config needed)
    Clear,
    /// Show cache size (requires config)
    Size,
    /// Remove unreferenced objects from cache (requires config)
    Trim,
    /// Remove stale index entries not matching any current product (requires config)
    RemoveStale,
    /// List all cache entries and their status (requires config)
    List,
    /// Show which cache entries are stale vs current (requires config)
    Stale,
    /// Show per-processor cache statistics (requires config)
    Stats,
}

#[derive(Subcommand)]
pub enum WebCacheAction {
    /// Clear the web cache (no config needed)
    Clear,
    /// Show web cache statistics (no config needed)
    Stats,
    /// List all cached entries (no config needed)
    List,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show the active configuration (requires config)
    Show,
    /// Show the default configuration (requires config)
    ShowDefault,
    /// Validate the configuration for errors and warnings (requires config)
    Validate,
}

#[derive(Subcommand)]
pub enum ProcessorAction {
    /// List all built-in processors with type and description (no config needed)
    List,
    /// Show which processors are enabled and detected (requires config)
    Used,
    /// Show source and target files for each processor (requires config)
    Files {
        /// Processor name (omit to show all enabled processors)
        name: Option<String>,
        /// Show processor headers (e.g., "[ruff] (42 products)")
        #[arg(long)]
        headers: bool,
    },
    /// Show resolved configuration for a processor (uses config if available)
    Config {
        /// Processor name (omit to show all enabled processors)
        name: Option<String>,
        /// Show only fields that differ from the default configuration
        #[arg(short, long)]
        diff: bool,
    },
    /// Show default configuration for a processor (no config needed)
    Defconfig {
        /// Processor name
        name: String,
    },
    /// Show the current processor allowlist (requires config)
    Allowlist,
    /// Show names of all enabled processors (one per line, requires config)
    Names,
    /// Show the recommended processor for each file extension (no config needed)
    Recommend,
    /// Show inter-processor dependencies (requires config)
    Graph {
        /// Output format
        #[arg(short, long, value_enum, default_value = "text")]
        format: GraphFormat,
    },
    /// List all processor types with descriptions
    Types,
}

#[derive(Subcommand)]
pub enum ToolsAction {
    /// List all required external tools (uses config if available)
    List {
        /// Include tools from disabled processors too
        #[arg(short, long)]
        all: bool,
        /// Show all available installation methods for each tool
        #[arg(short = 'M', long)]
        methods: bool,
    },
    /// Verify tool versions against .tools.versions lock file (uses config if available)
    Check,
    /// Lock tool versions to .tools.versions (uses config if available)
    Lock,
    /// Install missing external tools (uses config if available)
    Install {
        /// Tool name to install (omit to install missing tools for enabled processors)
        name: Option<String>,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },
    /// Install declared dependencies from the [dependencies] config section (uses config if available)
    InstallDeps {
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },
    /// Show tool availability statistics (uses config if available)
    Stats,
    /// Show tool-to-processor dependency graph (uses config if available)
    Graph {
        /// Output format
        #[arg(short, long, value_enum, default_value = "dot")]
        format: GraphFormat,
        /// Open the graph in a browser instead of printing to stdout
        #[arg(long)]
        view: bool,
    },
}

#[derive(Subcommand)]
pub enum DepsAction {
    /// List all available dependency analyzers (no config needed)
    List,
    /// Show which dependency analyzers are enabled and detected (requires config)
    Used,
    /// Run dependency analysis without building (requires config)
    Build,
    /// Show analyzer configuration (requires config)
    Config {
        /// Analyzer name (e.g., "cpp", "python"); omit to show all
        name: Option<String>,
    },
    /// Show cached dependencies (requires config)
    Show {
        #[command(subcommand)]
        filter: DepsShowFilter,
    },
    /// Show statistics about cached dependencies by analyzer (requires config)
    Stats,
    /// Clear the dependency cache (requires config)
    Clean {
        /// Only clear entries from this analyzer (e.g., "cpp", "python")
        #[arg(long)]
        analyzer: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum DepsShowFilter {
    /// Show dependencies for all source files
    All,
    /// Show dependencies for specific files
    Files {
        /// Source files to show dependencies for
        #[arg(required = true)]
        files: Vec<String>,
    },
    /// Show dependencies for files handled by specific analyzers
    Analyzers {
        /// Analyzer names (e.g., "cpp", "python")
        #[arg(required = true)]
        analyzers: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum TermsAction {
    /// Auto-fix: add backticks to terms (requires config)
    Fix {
        /// Also remove backticks from non-terms
        #[arg(long, default_value_t = false)]
        remove_non_terms: bool,
    },
    /// Merge terms from another project's terms directory (requires config)
    Merge {
        /// Path to the other project's terms directory
        path: String,
    },
    /// Show term file and term count statistics (requires config)
    Stats,
}

#[derive(Subcommand)]
pub enum TagsAction {
    /// List files matching given tags, AND by default, --or for OR (requires config)
    Files {
        /// Tags: bare values (e.g. "docker") or key:value (e.g. "level:advanced")
        #[arg(required = true)]
        tags: Vec<String>,
        /// Use OR semantics (match files with any of the given tags)
        #[arg(long, short)]
        or: bool,
    },
    /// Search for tags containing a substring (requires config)
    Grep {
        /// Text to search for in tag names
        text: String,
        /// Case-insensitive search
        #[arg(short, long)]
        ignore_case: bool,
    },
    /// List all unique tags (requires config)
    List,
    /// Show each tag with its file count, sorted by frequency (requires config)
    Count,
    /// Show tags grouped by prefix/category (requires config)
    Tree,
    /// Show statistics about the tags database (requires config)
    Stats,
    /// List all tags for a specific file (requires config)
    ForFile {
        /// Path to the file
        path: String,
    },
    /// Show the raw frontmatter for a specific file (requires config)
    Frontmatter {
        /// Path to the file
        path: String,
    },
    /// List tags in the allowlist (tags_dir) that are not used by any file (requires config)
    Unused {
        /// Exit with error if unused tags are found (useful for CI)
        #[arg(long)]
        strict: bool,
    },
    /// Validate tags against the allowlist without building (requires config)
    Validate,
    /// Show a coverage matrix of tag categories per file (requires config)
    Matrix,
    /// Show percentage of files that have each tag category (requires config)
    Coverage,
    /// Find markdown files with no tags at all (requires config)
    Orphans,
    /// Run all tag validations without building (requires config)
    Check,
    /// Suggest tags for a file based on similarity (requires config)
    Suggest {
        /// Path to the file
        path: String,
    },
    /// Merge tags from another project's tags directory (requires config)
    Merge {
        /// Path to the other project's tags directory
        path: String,
    },
    /// Scan source files and add missing tags back to the tag collection (requires config)
    Collect,
}

/// CLI arguments shared between Build and Watch commands.
#[derive(Args, Clone)]
pub struct SharedBuildArgs {
    /// Number of parallel jobs (overrides config file)
    #[arg(short, long)]
    pub jobs: Option<usize>,

    /// Show per-product and total build timing information
    #[arg(long)]
    pub timings: bool,

    /// Continue building after errors, skipping dependents of failed products
    #[arg(short = 'k', long)]
    pub keep_going: bool,

    /// Suppress the build summary
    #[arg(long)]
    pub no_summary: bool,

    /// Batch size for batch-capable processors (0 = no limit, -1 = disable, omit to use config)
    #[arg(long, allow_negative_numbers = true)]
    pub batch_size: Option<i32>,

    /// Only run specific processors (comma-separated list)
    #[arg(short, long, value_delimiter = ',')]
    pub processors: Option<Vec<String>>,

    /// Automatically add misspelled words to words files instead of failing (zspell + aspell)
    #[arg(long)]
    pub auto_add_words: bool,

    /// Show why each product is skipped, restored, or rebuilt
    #[arg(long)]
    pub explain: bool,

    /// Retry failed products up to N times to detect flakiness
    #[arg(long, value_name = "N", default_value = "0")]
    pub retry: usize,

    /// Disable mtime pre-check (always compute full checksums)
    #[arg(long)]
    pub no_mtime: bool,

    /// Only build products matching these file patterns (glob syntax, repeatable)
    #[arg(short, long = "target")]
    pub targets: Option<Vec<String>>,

    /// Only build products whose inputs are under these directories (repeatable)
    #[arg(short, long = "dir")]
    pub dirs: Option<Vec<String>>,

    /// Write a Chrome trace JSON file for build visualization (open in chrome://tracing or Perfetto)
    #[arg(long, value_name = "FILE")]
    pub trace: Option<String>,

    /// Show all config changes between runs (not just output-affecting fields)
    #[arg(long)]
    pub show_all_config_changes: bool,
}

impl SharedBuildArgs {
    /// Convert to BuildOptions with the given overrides for build-only fields.
    pub fn to_build_options(&self, cli: &Cli, force: bool, stop_after: BuildPhase) -> BuildOptions {
        // Merge --dir values into targets as glob patterns
        let targets = match (&self.targets, &self.dirs) {
            (None, None) => None,
            (Some(t), None) => Some(t.clone()),
            (None, Some(d)) => Some(d.iter().map(|dir| format!("{dir}/**")).collect()),
            (Some(t), Some(d)) => {
                let mut merged = t.clone();
                merged.extend(d.iter().map(|dir| format!("{dir}/**")));
                Some(merged)
            }
        };
        BuildOptions {
            force,
            verbose: cli.verbose,
            display_opts: cli.display_options(),
            jobs: self.jobs,
            timings: self.timings,
            keep_going: self.keep_going,
            summary: !self.no_summary,
            batch_size: self.batch_size.map(|n| if n < 0 { None } else { Some(n as usize) }),
            stop_after,
            processor_filter: self.processors.clone(),
            auto_add_words: self.auto_add_words,
            explain: self.explain,
            no_mtime: self.no_mtime,
            retry: self.retry,
            targets,
            trace: self.trace.clone(),
            show_all_config_changes: self.show_all_config_changes,
        }
    }
}

/// Options shared by build and watch commands.
#[derive(Clone)]
pub struct BuildOptions {
    pub force: bool,
    pub verbose: bool,
    pub display_opts: DisplayOptions,
    pub jobs: Option<usize>,
    pub timings: bool,
    pub keep_going: bool,
    pub summary: bool,
    pub batch_size: Option<Option<usize>>,
    pub stop_after: BuildPhase,
    pub processor_filter: Option<Vec<String>>,
    pub auto_add_words: bool,
    pub explain: bool,
    pub no_mtime: bool,
    pub retry: usize,
    pub targets: Option<Vec<String>>,
    pub trace: Option<String>,
    pub show_all_config_changes: bool,
}

/// Parse a shell name string into a Shell enum
pub fn parse_shell(name: &str) -> Option<Shell> {
    <Shell as FromStr>::from_str(name).ok()
}

/// Recursively set `hide_short_help = true` on all arguments in a command and its subcommands.
fn hide_all_flags(cmd: clap::Command) -> clap::Command {
    let cmd = cmd.mut_args(|arg| {
        if arg.get_long().is_some() || arg.get_short().is_some() {
            arg.hide_short_help(true)
        } else {
            arg
        }
    });
    cmd.mut_subcommands(hide_all_flags)
}

/// Parse CLI arguments with all flags hidden from short help (`-h`).
/// Use `--help` to see all flags.
pub fn parse_cli() -> Cli {
    let cmd = hide_all_flags(Cli::command());
    let matches = cmd.get_matches();
    Cli::from_arg_matches(&matches).expect("failed to parse CLI arguments")
}

/// Generate shell completions and print to stdout.
/// Post-processes the generated script to inject processor name completions
/// for `processors config`, `processors defconfig`, and `processors files`.
pub fn print_completions(shell: Shell) {
    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    generate(shell, &mut cmd, "rsconstruct", &mut buf);
    let script = String::from_utf8(buf).expect("completion script should be UTF-8");

    match shell {
        Shell::Bash => print!("{}", inject_bash_processor_completions(&script)),
        _ => print!("{}", script),
    }
}

/// Inject processor name completions into a bash completion script.
/// Replaces the empty `COMPREPLY=()` in the defconfig/config/files cases
/// with processor name suggestions.
fn inject_bash_processor_completions(script: &str) -> String {
    let names = crate::config::all_type_names();
    let names_str = names.join(" ");

    // For each of these commands, inject processor names into the opts variable
    // so they appear as completions.
    // Process from last to first in the script so position shifts don't affect earlier sections.
    let targets = [
        "rsconstruct__processors__files)",
        "rsconstruct__processors__defconfig)",
        "rsconstruct__processors__config)",
    ];

    let mut result = script.to_string();
    for target in &targets {
        if let Some(section_start) = result.find(target) {
            let after_start = section_start + target.len();
            // Section ends at the next command case
            let section_len = result[after_start..]
                .find("\n        rsconstruct__")
                .unwrap_or(result.len() - after_start);
            let section_end = after_start + section_len;

            // The generated bash completion has an early-return `if` that checks
            // `COMP_CWORD -eq N` and only offers flags. We need to inject processor
            // names into the `opts` variable so they appear in that early-return path.
            let section_slice = &result[section_start..section_end];
            if let Some(opts_pos) = section_slice.find("opts=\"") {
                let abs_opts = section_start + opts_pos + "opts=\"".len();
                result.insert_str(abs_opts, &format!("{} ", names_str));
            }
        }
    }

    // Inject static processor names for --processors/-p flags in build/watch commands.
    let old_processors = "                --processors)\n                    COMPREPLY=($(compgen -f \"${cur}\"))";
    let new_processors = format!("                --processors)\n                    COMPREPLY=($(compgen -W \"{names_str}\" -- \"${{cur}}\"))");
    let old_p = "                -p)\n                    COMPREPLY=($(compgen -f \"${cur}\"))";
    let new_p = format!("                -p)\n                    COMPREPLY=($(compgen -W \"{names_str}\" -- \"${{cur}}\"))");

    result.replace(old_processors, &new_processors).replace(old_p, &new_p)
}

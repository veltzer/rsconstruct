use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use std::io;
use std::str::FromStr;

#[derive(Parser)]
#[command(name = "rsb")]
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

    /// Print each external command before it is executed
    #[arg(long, global = true)]
    pub process: bool,

    /// Show tool output even on success (default: only show on failure)
    #[arg(long, global = true)]
    pub show_output: bool,

    /// Output in JSON Lines format (machine-readable)
    #[arg(long, global = true)]
    pub json: bool,

    /// Show build phase messages (discover, add_dependencies, etc.)
    #[arg(long, global = true)]
    pub phases: bool,

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

        /// Number of parallel jobs (overrides config file)
        #[arg(short, long)]
        jobs: Option<usize>,

        /// Show per-product and total build timing information
        #[arg(long)]
        timings: bool,

        /// Continue building after errors, skipping dependents of failed products
        #[arg(short = 'k', long)]
        keep_going: bool,

        /// Show what would be built without executing anything
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Suppress the build summary
        #[arg(long)]
        no_summary: bool,

        /// Skip tool version verification against .tools.versions
        #[arg(long)]
        ignore_tool_versions: bool,

        /// Batch size for batch-capable processors (0 = no limit, -1 = disable, omit to use config)
        #[arg(long, allow_negative_numbers = true)]
        batch_size: Option<i32>,

        /// Stop after a specific build phase
        #[arg(long, value_enum, default_value = "build")]
        stop_after: BuildPhase,

        /// Only run specific processors (comma-separated list)
        #[arg(short, long, value_delimiter = ',')]
        processors: Option<Vec<String>>,
    },
    /// Clean build artifacts
    Clean {
        #[command(subcommand)]
        action: Option<CleanAction>,
    },
    /// Show the status of each product (up-to-date, stale, or restorable)
    Status,
    /// Initialize a new rsb project in the current directory
    Init,
    /// Manage the build cache
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
    /// Generate shell completion scripts
    Complete {
        /// The shells to generate completions for (if none specified, uses config file)
        #[arg(value_enum)]
        shells: Vec<Shell>,
    },
    /// Watch source files and auto-rebuild on changes
    Watch {
        /// Number of parallel jobs (overrides config file)
        #[arg(short, long)]
        jobs: Option<usize>,

        /// Show per-product and total build timing information
        #[arg(long)]
        timings: bool,

        /// Continue building after errors, skipping dependents of failed products
        #[arg(short = 'k', long)]
        keep_going: bool,

        /// Suppress the build summary
        #[arg(long)]
        no_summary: bool,

        /// Batch size for batch-capable processors (0 = no limit, -1 = disable, omit to use config)
        #[arg(long, allow_negative_numbers = true)]
        batch_size: Option<i32>,

        /// Only run specific processors (comma-separated list)
        #[arg(short, long, value_delimiter = ',')]
        processors: Option<Vec<String>>,
    },
    /// Manage processors
    Processors {
        #[command(subcommand)]
        action: ProcessorAction,
    },
    /// Print version information
    Version,
    /// Show or inspect configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Manage external tool dependencies
    Tools {
        #[command(subcommand)]
        action: ToolsAction,
    },
    /// Display the build dependency graph
    Graph {
        /// Output format (ignored if --view is used)
        #[arg(short, long, value_enum, default_value = "svg")]
        format: GraphFormat,

        /// Open graph in viewer
        #[arg(long, value_enum, num_args = 0..=1, default_missing_value = "svg")]
        view: Option<GraphViewer>,
    },
    /// Show source file dependencies (e.g., header files for C/C++)
    Deps {
        #[command(subcommand)]
        action: DepsAction,
    },
}

#[derive(Subcommand)]
pub enum CleanAction {
    /// Remove build output files (preserves cache) [default]
    Outputs,
    /// Remove all build outputs and cache directories (.rsb/ and out/)
    All,
    /// Hard clean using git clean (requires git repository)
    Git,
}

#[derive(Subcommand)]
pub enum CacheAction {
    /// Clear the entire cache
    Clear,
    /// Show cache size
    Size,
    /// Remove unreferenced objects from cache
    Trim,
    /// Remove stale index entries not matching any current product
    RemoveStale,
    /// List all cache entries and their status
    List,
    /// Show which cache entries are stale vs current
    Stale,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show the active configuration (defaults merged with rsb.toml overrides)
    Show,
    /// Show the default configuration (without rsb.toml overrides)
    ShowDefault,
}

#[derive(Subcommand)]
pub enum ProcessorAction {
    /// List available processors and their status
    List {
        /// Show all processors, including hidden ones
        #[arg(short, long)]
        all: bool,
    },
    /// Show all processors (including hidden), with enabled and hidden status
    All,
    /// Auto-detect which processors are relevant for this project
    Auto,
    /// Show source and target files for each processor
    Files {
        /// Processor name (omit to show all enabled processors)
        name: Option<String>,
        /// Include disabled and hidden processors
        #[arg(short, long)]
        all: bool,
    },
}

#[derive(Subcommand)]
pub enum ToolsAction {
    /// List all required external tools
    List {
        /// Include tools from disabled processors too
        #[arg(short, long)]
        all: bool,
    },
    /// Check if required external tools are available on PATH
    Check {
        /// Check tools from all processors (including disabled)
        #[arg(short, long)]
        all: bool,
    },
    /// Lock tool versions to .tools.versions (creates or updates the lock file)
    Lock {
        /// Only verify the lock file without writing (exit with error if mismatched)
        #[arg(long)]
        check: bool,
    },
}

#[derive(Subcommand)]
pub enum DepsAction {
    /// List all available dependency analyzers
    List,
    /// Show cached dependencies
    Show {
        #[command(subcommand)]
        filter: DepsShowFilter,
    },
    /// Show statistics about cached dependencies by analyzer
    Stats,
    /// Clear the dependency cache (all analyzers, or specific one)
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

/// Parse a shell name string into a Shell enum
pub fn parse_shell(name: &str) -> Option<Shell> {
    <Shell as FromStr>::from_str(name).ok()
}

/// Generate shell completions and print to stdout
pub fn print_completions(shell: Shell) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "rsb", &mut io::stdout());
}
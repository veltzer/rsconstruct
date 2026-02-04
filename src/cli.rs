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

    /// File name detail level in output (0=basename, 1=relative path, 2=+source, 3=+all inputs)
    #[arg(long, global = true, default_value = "0")]
    pub file_names: u8,

    /// Print each external command before it is executed
    #[arg(long, global = true)]
    pub process: bool,

    /// Output in JSON Lines format (machine-readable)
    #[arg(long, global = true)]
    pub json: bool,

    /// Show build phase messages (discover, add_dependencies, etc.)
    #[arg(long, global = true)]
    pub phases: bool,

    #[command(subcommand)]
    pub command: Commands,
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
    },
    /// Manage processors
    Processor {
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
    /// Show dependencies for all source files
    All,
    /// Show dependencies for specific files
    For {
        /// Source files to show dependencies for
        #[arg(required = true)]
        files: Vec<String>,
    },
    /// Clear the dependency cache
    Clean,
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
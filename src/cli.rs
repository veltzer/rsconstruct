use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use std::io;
use std::str::FromStr;

#[derive(Parser)]
#[command(name = "rsb")]
#[command(about = "Rust Build Tool - Incremental build system with templates", long_about = None)]
pub struct Cli {
    /// Show verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

/// Output format for the dependency graph
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum GraphFormat {
    /// DOT format (Graphviz)
    #[default]
    Dot,
    /// Mermaid diagram format (Markdown-friendly)
    Mermaid,
    /// JSON format (machine-readable)
    Json,
    /// Plain text hierarchical view
    Text,
}

/// Viewer for opening the graph
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum GraphViewer {
    /// Open as HTML with Mermaid in browser (no dependencies)
    #[default]
    Mermaid,
    /// Use Graphviz dot command to generate and open image
    Dot,
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

        /// Processor verbosity level: 1 = print commands, 2 = also show all inputs (e.g. headers)
        #[arg(long, default_value = "0")]
        processor_verbose: u8,
    },
    /// Clean all build artifacts
    Clean,
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
    },
    /// Manage processors
    Processor {
        #[command(subcommand)]
        action: ProcessorAction,
    },
    /// Display the build dependency graph
    Graph {
        /// Output format (ignored if --view is used)
        #[arg(short, long, value_enum, default_value = "dot")]
        format: GraphFormat,

        /// Open graph in viewer
        #[arg(long, value_enum)]
        view: Option<GraphViewer>,
    },
}

#[derive(Subcommand)]
pub enum CacheAction {
    /// Clear the entire cache
    Clear,
    /// Show cache size
    Size,
    /// Remove unreferenced objects from cache
    Trim,
    /// List all cache entries and their status
    List,
}

#[derive(Subcommand)]
pub enum ProcessorAction {
    /// List all available processors and their status
    List,
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
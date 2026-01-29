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

        /// Processor verbosity level (0=target basename, 1=target full path,
        /// 2=add source path, 3=add all inputs including headers)
        #[arg(long, default_value = "0")]
        processor_verbose: u8,
    },
    /// Clean all build artifacts
    Clean,
    /// Remove all build outputs and cache directories (.rsb/ and out/)
    Distclean,
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
        #[arg(short, long, value_enum, default_value = "svg")]
        format: GraphFormat,

        /// Open graph in viewer
        #[arg(long, value_enum, num_args = 0..=1, default_missing_value = "svg")]
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
    List {
        /// Show all processors, including hidden ones
        #[arg(short, long)]
        all: bool,
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
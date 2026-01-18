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

#[derive(Subcommand)]
pub enum Commands {
    /// Execute an incremental build
    Build {
        /// Force rebuild even if files haven't changed
        #[arg(short, long)]
        force: bool,
    },
    /// Clean all build artifacts
    Clean,
    /// Generate shell completion scripts
    Complete {
        /// The shells to generate completions for (if none specified, uses config file)
        #[arg(value_enum)]
        shells: Vec<Shell>,
    },
    /// Display the build dependency graph
    Graph {
        /// Output format
        #[arg(short, long, value_enum, default_value = "dot")]
        format: GraphFormat,
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
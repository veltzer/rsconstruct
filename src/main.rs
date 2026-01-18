mod builder;
mod checksum;
mod cli;
mod config;
mod linter;
mod processor;
mod template;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, print_completions};
use builder::Builder;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { force } => {
            let mut builder = Builder::new()?;
            builder.build(force, cli.verbose)?;
        }
        Commands::Clean => {
            let mut builder = Builder::new()?;
            builder.clean()?;
        }
        Commands::Complete { shell } => {
            print_completions(shell);
        }
    }

    Ok(())
}

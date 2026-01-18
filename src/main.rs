mod builder;
mod checksum;
mod cli;
mod config;
mod graph;
mod processors;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{Cli, Commands, parse_shell, print_completions};
use config::Config;
use builder::Builder;
use std::env;

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
        Commands::Complete { shells } => {
            let shells_to_generate = if shells.is_empty() {
                // Load from config file
                let config = Config::load(&env::current_dir()?)?;
                let mut parsed_shells = Vec::new();
                for shell_name in &config.completions.shells {
                    match parse_shell(shell_name) {
                        Some(shell) => parsed_shells.push(shell),
                        None => bail!("Unknown shell in config: {}", shell_name),
                    }
                }
                parsed_shells
            } else {
                shells
            };

            for shell in shells_to_generate {
                print_completions(shell);
            }
        }
        Commands::Graph { format } => {
            let builder = Builder::new()?;
            builder.print_graph(format)?;
        }
    }

    Ok(())
}

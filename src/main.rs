mod builder;
mod checksum;
mod cli;
mod template;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};
use builder::Builder;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut builder = Builder::new()?;

    match cli.command {
        Commands::Build { force } => {
            builder.build(force)?;
        }
        Commands::Clean => {
            builder.clean()?;
        }
    }

    Ok(())
}

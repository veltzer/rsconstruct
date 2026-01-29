mod builder;
mod cli;
mod color;
mod config;
mod executor;
mod graph;
mod ignore;
mod object_store;
mod processors;
mod watcher;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{CacheAction, Cli, Commands, ProcessorAction, parse_shell, print_completions};
use config::Config;
use builder::Builder;
use object_store::ObjectStore;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up Ctrl+C handler: sets a flag so the executor can stop gracefully
    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let interrupted = Arc::clone(&interrupted);
        ctrlc::set_handler(move || {
            interrupted.store(true, std::sync::atomic::Ordering::SeqCst);
        })?;
    }

    match cli.command {
        Commands::Build { force, jobs, timings, keep_going, dry_run, processor_verbose } => {
            if dry_run {
                let builder = Builder::new()?;
                builder.dry_run(force)?;
            } else {
                let mut builder = Builder::new()?;
                builder.build(force, cli.verbose, jobs, timings, keep_going, processor_verbose, Arc::clone(&interrupted))?;
            }
        }
        Commands::Clean => {
            let mut builder = Builder::new()?;
            builder.clean()?;
        }
        Commands::Distclean => {
            let builder = Builder::new()?;
            builder.distclean()?;
        }
        Commands::Status => {
            let builder = Builder::new()?;
            builder.status()?;
        }
        Commands::Init => {
            init_project()?;
        }
        Commands::Cache { action } => {
            let project_root = env::current_dir()?;
            let config = Config::load(&project_root)?;
            let mut store = ObjectStore::new(project_root, config.cache.restore_method)?;

            match action {
                CacheAction::Clear => {
                    store.clear()?;
                    println!("Cache cleared.");
                }
                CacheAction::Size => {
                    let (bytes, count) = store.size()?;
                    println!("Cache size: {} bytes ({} objects)", bytes, count);
                }
                CacheAction::Trim => {
                    let (bytes, count) = store.trim()?;
                    store.save()?;
                    println!("Removed {} bytes ({} unreferenced objects)", bytes, count);
                }
                CacheAction::List => {
                    let entries = store.list();
                    if entries.is_empty() {
                        println!("No cache entries.");
                    } else {
                        for entry in &entries {
                            println!("{} [{}]", color::bold(&entry.cache_key), entry.input_checksum);
                            for (path, exists) in &entry.outputs {
                                let status = if *exists {
                                    color::green("ok")
                                } else {
                                    color::red("missing")
                                };
                                println!("  {} {}", status, path);
                            }
                        }
                        println!();
                        println!("{} cache entries.", entries.len());
                    }
                }
            }
        }
        Commands::Processor { action } => {
            let project_root = env::current_dir()?;
            let config = Config::load(&project_root)?;

            let all_processors: [(&str, bool); 6] = [
                ("template", false),
                ("pylint", false),
                ("sleep", true),
                ("cc", false),
                ("cpplint", false),
                ("spellcheck", false),
            ];

            match action {
                ProcessorAction::List { all } => {
                    for (name, hidden) in &all_processors {
                        if *hidden && !all {
                            continue;
                        }
                        let status = if config.processor.is_enabled(name) {
                            color::green("enabled")
                        } else {
                            color::dim("disabled")
                        };
                        println!("{} {}", name, status);
                    }
                }
                ProcessorAction::All => {
                    for (name, hidden) in &all_processors {
                        let enabled_status = if config.processor.is_enabled(name) {
                            color::green("enabled")
                        } else {
                            color::dim("disabled")
                        };
                        let hidden_status = if *hidden {
                            format!(" {}", color::dim("(hidden)"))
                        } else {
                            String::new()
                        };
                        println!("{} {}{}", name, enabled_status, hidden_status);
                    }
                }
            }
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
        Commands::Watch { jobs, timings, keep_going } => {
            watcher::watch(cli.verbose, jobs, timings, keep_going, Arc::clone(&interrupted))?;
        }
        Commands::Version => {
            println!("rsb {}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Graph { format, view } => {
            let builder = Builder::new()?;
            if let Some(viewer) = view {
                builder.view_graph(viewer)?;
            } else {
                builder.print_graph(format)?;
            }
        }
    }

    Ok(())
}

/// Initialize a new rsb project in the current directory
fn init_project() -> Result<()> {
    let cwd = env::current_dir()?;
    let config_path = cwd.join("rsb.toml");

    if config_path.exists() {
        bail!("rsb.toml already exists in the current directory");
    }

    // Create rsb.toml with commented defaults
    let config_content = r#"# RSB Build Tool Configuration

[build]
# Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)
# parallel = 1

[processor]
# enabled = ["template", "pylint", "sleep", "cc", "cpplint"]

[cache]
# restore_method = "hardlink"  # or "copy"

[processor.template]
# strict = true
# extensions = [".tera"]
# trim_blocks = false

[processor.pylint]
# linter = "ruff"
# args = []

[processor.cc]
# cc = "gcc"
# cxx = "g++"
# cflags = []
# cxxflags = []
# ldflags = []
# include_paths = []
# source_dir = "src"
# output_suffix = ".elf"

[processor.cpplint]
# checker = "cppcheck"
# args = ["--error-exitcode=1", "--enable=warning,style,performance,portability"]

[graph]
# viewer = "google-chrome"

[completions]
# shells = ["bash"]
"#;
    fs::write(&config_path, config_content)?;
    println!("Created {}", config_path.display());

    // Create directories (preserve existing)
    let templates_dir = cwd.join("templates");
    let config_dir = cwd.join("config");

    if !templates_dir.exists() {
        create_dir_and_print(&templates_dir)?;
    } else {
        println!("Directory already exists: {}", templates_dir.display());
    }

    if !config_dir.exists() {
        create_dir_and_print(&config_dir)?;
    } else {
        println!("Directory already exists: {}", config_dir.display());
    }

    println!("{}", color::green("Project initialized successfully!"));
    Ok(())
}

fn create_dir_and_print(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    println!("Created {}", path.display());
    Ok(())
}

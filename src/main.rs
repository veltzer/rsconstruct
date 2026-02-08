mod analyzers;
mod builder;
mod cli;
mod color;
mod config;
mod deps_cache;
mod executor;
mod exit_code;
mod file_index;
mod graph;
mod json_output;
mod object_store;
mod processors;
mod remote_cache;
mod tool_lock;
mod watcher;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{BuildPhase, CacheAction, CleanAction, Cli, Commands, parse_shell, print_completions};
use config::Config;
use builder::Builder;
use exit_code::{RsbExitCode, RsbError, classify_error};
use std::env;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn main() -> std::process::ExitCode {
    // Reset SIGPIPE to default so piping to head/more/less exits cleanly
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    match run() {
        Ok(()) => std::process::ExitCode::from(RsbExitCode::Success.code()),
        Err(err) => {
            let exit_code = classify_error(&err);
            if json_output::is_json_mode() {
                let error_event = serde_json::json!({
                    "event": "error",
                    "exit_code": exit_code.code(),
                    "exit_code_name": exit_code.name(),
                    "message": format!("{:#}", err),
                });
                eprintln!("{}", error_event);
            } else {
                eprintln!("Error [{}]: {:#}", exit_code.name(), err);
            }
            std::process::ExitCode::from(exit_code.code())
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Enable process debug logging if --process flag is set
    processors::set_process_debug(cli.process);

    // Enable showing tool output even on success if --show-output flag is set
    processors::set_show_output(cli.show_output);

    // Enable JSON output mode if --json flag is set
    json_output::set_json_mode(cli.json);

    // Enable phases debug logging if --phases flag is set
    builder::set_phases_debug(cli.phases);

    // Set up Ctrl+C handler: sets a flag so the executor can stop gracefully
    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let interrupted = Arc::clone(&interrupted);
        // Spawn a background thread to handle Ctrl+C using tokio's signal handling
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create signal handler runtime");
            rt.block_on(async {
                tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
                interrupted.store(true, std::sync::atomic::Ordering::SeqCst);
                processors::set_interrupted();
            });
        });
    }

    match cli.command {
        Commands::Build { force, dry_run, ignore_tool_versions, stop_after, ref shared } => {
            if dry_run {
                let builder = Builder::new()?;
                builder.dry_run(force, shared.explain)?;
            } else {
                let mut builder = Builder::new()?;
                if !ignore_tool_versions {
                    builder.verify_tool_versions()?;
                }
                let opts = shared.to_build_options(&cli, force, stop_after);
                builder.build(&opts, Arc::clone(&interrupted))?;
            }
        }
        Commands::Clean { action } => {
            match action.unwrap_or(CleanAction::Outputs) {
                CleanAction::Outputs => {
                    let builder = Builder::new()?;
                    builder.clean()?;
                }
                CleanAction::All => {
                    let builder = Builder::new()?;
                    builder.distclean()?;
                }
                CleanAction::Git => {
                    let builder = Builder::new()?;
                    builder.hardclean()?;
                }
            }
        }
        Commands::Status => {
            let builder = Builder::new()?;
            builder.status()?;
        }
        Commands::Init => {
            init_project()?;
        }
        Commands::Cache { action } => {
            match action {
                CacheAction::Clear => {
                    let mut builder = Builder::new()?;
                    builder.object_store_mut().clear()?;
                    println!("Cache cleared.");
                }
                CacheAction::Size => {
                    let builder = Builder::new()?;
                    let (bytes, count) = builder.object_store().size()?;
                    println!("Cache size: {} ({} objects)", humansize::format_size(bytes, humansize::BINARY), count);
                }
                CacheAction::Trim => {
                    let builder = Builder::new()?;
                    let (bytes, count) = builder.object_store().trim()?;
                    builder.object_store().save()?;
                    println!("Removed {} bytes ({} unreferenced objects)", bytes, count);
                }
                CacheAction::RemoveStale => {
                    let builder = Builder::new()?;
                    let valid_keys = builder.valid_cache_keys()?;
                    let stale_count = builder.object_store().remove_stale(&valid_keys);
                    let (bytes, trim_count) = builder.object_store().trim()?;
                    builder.object_store().save()?;
                    println!("Removed {} stale index entries", stale_count);
                    if trim_count > 0 {
                        println!("Removed {} bytes ({} orphaned objects)", bytes, trim_count);
                    }
                }
                CacheAction::List => {
                    let builder = Builder::new()?;
                    let entries = builder.object_store().list();
                    println!("{}", serde_json::to_string_pretty(&entries)?);
                }
                CacheAction::Stats => {
                    let builder = Builder::new()?;
                    let stats = builder.object_store().stats_by_processor();

                    if stats.is_empty() {
                        println!("Cache is empty.");
                    } else if json_output::is_json_mode() {
                        println!("{}", serde_json::to_string_pretty(&stats)?);
                    } else {
                        let mut total_entries = 0usize;
                        let mut total_outputs = 0usize;
                        let mut total_bytes = 0u64;
                        for (processor, proc_stats) in &stats {
                            total_entries += proc_stats.entry_count;
                            total_outputs += proc_stats.output_count;
                            total_bytes += proc_stats.output_bytes;
                            println!("{}: {} entries, {} outputs, {}",
                                color::bold(processor),
                                proc_stats.entry_count,
                                proc_stats.output_count,
                                humansize::format_size(proc_stats.output_bytes, humansize::BINARY));
                        }
                        println!();
                        println!("{}: {} entries, {} outputs, {}",
                            color::bold("Total"),
                            total_entries,
                            total_outputs,
                            humansize::format_size(total_bytes, humansize::BINARY));
                    }
                }
                CacheAction::Stale => {
                    let builder = Builder::new()?;
                    let valid_keys = builder.valid_cache_keys()?;
                    let entries = builder.object_store().list();
                    let mut current_count = 0usize;
                    let mut stale_count = 0usize;
                    for entry in &entries {
                        if valid_keys.contains(&entry.cache_key) {
                            println!("{} {}", color::green("current"), entry.cache_key);
                            current_count += 1;
                        } else {
                            println!("{} {}", color::yellow("stale"), entry.cache_key);
                            stale_count += 1;
                        }
                    }
                    println!();
                    println!("{}: {} current, {} stale",
                        color::bold("Summary"), current_count, stale_count);
                }
            }
        }
        Commands::Processors { action } => {
            let builder = Builder::new()?;
            builder.processor(action)?;
        }
        Commands::Tools { action } => {
            let builder = Builder::new()?;
            builder.tools(action)?;
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
        Commands::Watch { ref shared } => {
            let opts = shared.to_build_options(&cli, false, BuildPhase::Build);
            watcher::watch(&opts, Arc::clone(&interrupted))?;
        }
        Commands::Version => {
            println!("rsb {}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Config { action } => {
            let builder = Builder::new()?;
            builder.config(action)?;
        }
        Commands::Graph { format, view } => {
            let builder = Builder::new()?;
            if let Some(viewer) = view {
                builder.view_graph(viewer)?;
            } else {
                builder.print_graph(format)?;
            }
        }
        Commands::Deps { action } => {
            let builder = Builder::new()?;
            builder.deps(action)?;
        }
    }

    Ok(())
}

/// Initialize a new rsb project in the current directory
fn init_project() -> Result<()> {
    let cwd = env::current_dir()?;
    let config_path = cwd.join("rsb.toml");

    if config_path.exists() {
        return Err(RsbError::new(
            RsbExitCode::ConfigError,
            "rsb.toml already exists in the current directory",
        ).into());
    }

    // Create rsb.toml with commented defaults
    let config_content = r#"# RSB Build Tool Configuration

[build]
# Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)
# parallel = 1
# Max files per batch for batch-capable processors (0 = no limit, omit to disable batching)
# batch_size = 0

[processor]
# auto_detect = true
# enabled = ["template", "ruff", "pylint", "cc_single_file", "cppcheck", "shellcheck", "spellcheck", "make"]

[cache]
# restore_method = "hardlink"  # or "copy"

[processor.template]
# strict = true
# scan_dir = "templates"
# extensions = [".tera"]
# trim_blocks = false

[processor.ruff]
# linter = "ruff"
# args = []
# scan_dir = ""
# extensions = [".py"]

[processor.pylint]
# args = []
# scan_dir = ""
# extensions = [".py"]

[processor.cc_single_file]
# cc = "gcc"
# cxx = "g++"
# cflags = []
# cxxflags = []
# ldflags = []
# include_paths = []
# scan_dir = "src"
# extensions = [".c", ".cc"]
# output_suffix = ".elf"

[processor.cppcheck]
# args = ["--error-exitcode=1", "--enable=warning,style,performance,portability"]
# scan_dir = "src"
# extensions = [".c", ".cc"]

[processor.shellcheck]
# checker = "shellcheck"
# args = []
# scan_dir = ""
# extensions = [".sh", ".bash"]

[processor.make]
# make = "make"
# args = []
# target = ""
# scan_dir = ""
# extensions = ["Makefile"]
# exclude_paths = []

[graph]
# viewer = "google-chrome"

[completions]
# shells = ["bash"]

# [plugins]
# dir = "plugins"  # directory containing .lua processor plugins
"#;
    fs::write(&config_path, config_content)?;
    println!("Created {}", config_path.display());

    // Create .rsbignore if it doesn't exist
    let rsbignore_path = cwd.join(".rsbignore");
    if !rsbignore_path.exists() {
        let rsbignore_content = r#"# .rsbignore - Exclude files from rsb processing
# Uses .gitignore syntax (glob patterns, one per line)
# Lines starting with # are comments
#
# Examples:
# /build/           # Exclude a top-level directory
# *.generated.*     # Exclude generated files by pattern
# /src/vendor/**    # Exclude vendored source code
# /experiments/     # Exclude experimental code
# *.bak             # Exclude backup files
"#;
        fs::write(&rsbignore_path, rsbignore_content)?;
        println!("Created .rsbignore");
    }

    println!("{}", color::green("Project initialized successfully!"));
    println!("{}", color::dim("Hint: edit .rsbignore to exclude files from processing"));
    Ok(())
}


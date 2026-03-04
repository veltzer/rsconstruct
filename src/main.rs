#![deny(clippy::all)]
#![deny(warnings)]

mod analyzers;
mod builder;
mod checksum;
mod cli;
mod color;
mod config;
mod db;
mod deps_cache;
mod errors;
mod executor;
mod exit_code;
mod file_index;
mod graph;
mod json_output;
mod object_store;
mod processors;
mod progress;
mod runtime_flags;
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
use std::time::Instant;

fn main() -> std::process::ExitCode {
    // SAFETY: Resetting SIGPIPE to default behavior is safe — this is a standard
    // pattern for CLI tools to avoid "broken pipe" errors when piping to head/less/etc.
    // No Rust invariants are affected; we're just restoring the OS default signal handler.
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
    let t_start = Instant::now();
    let cli = Cli::parse();
    let cli_parse_dur = t_start.elapsed();

    // Initialize runtime flags from CLI arguments (once, before any reads)
    let t = Instant::now();
    runtime_flags::init(runtime_flags::RuntimeFlags {
        show_child_processes: cli.show_child_processes,
        show_output: cli.show_output,
        phases_debug: cli.phases,
        json_mode: cli.json,
        quiet: cli.quiet,
    });

    // Set up Ctrl+C handler: sets a flag so the executor can stop gracefully
    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let interrupted = Arc::clone(&interrupted);
        // Spawn a background thread to handle Ctrl+C using tokio's signal handling
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect(errors::SIGNAL_HANDLER_RUNTIME);
            rt.block_on(async {
                tokio::signal::ctrl_c().await.expect(errors::SIGNAL_LISTEN);
                interrupted.store(true, std::sync::atomic::Ordering::SeqCst);
                processors::set_interrupted();
                eprintln!("\nInterrupted. Press Ctrl+C again to force exit.");
                tokio::signal::ctrl_c().await.expect(errors::SIGNAL_LISTEN);
                std::process::exit(130);
            });
        });
    }
    let init_dur = t.elapsed();

    match cli.command {
        Commands::Build { force, dry_run, verify_tool_versions, stop_after, ref shared } => {
            if dry_run {
                let builder = Builder::new()?;
                builder.dry_run(force, shared.explain)?;
            } else {
                let t = Instant::now();
                let mut builder = Builder::new()?;
                let builder_new_dur = t.elapsed();
                let t = Instant::now();
                if verify_tool_versions {
                    builder.verify_tool_versions()?;
                }
                let verify_tools_dur = t.elapsed();
                let init_timings = vec![
                    ("cli_parse".to_string(), cli_parse_dur),
                    ("init".to_string(), init_dur),
                    ("builder_new".to_string(), builder_new_dur),
                    ("verify_tools".to_string(), verify_tools_dur),
                ];
                let opts = shared.to_build_options(&cli, force, stop_after);
                builder.build(&opts, Arc::clone(&interrupted), init_timings)?;
            }
        }
        Commands::Clean { action } => {
            match action.unwrap_or(CleanAction::Outputs) {
                CleanAction::Outputs => {
                    let builder = Builder::new()?;
                    builder.clean(cli.verbose)?;
                }
                CleanAction::All => {
                    let builder = Builder::new()?;
                    builder.distclean()?;
                }
                CleanAction::Git => {
                    let builder = Builder::new()?;
                    builder.hardclean()?;
                }
                CleanAction::Unknown { force } => {
                    let builder = Builder::new()?;
                    builder.clean_unknown(force, cli.verbose)?;
                }
            }
        }
        Commands::Status => {
            let builder = Builder::new()?;
            builder.status(cli.verbose)?;
        }
        Commands::Init => {
            init_project()?;
        }
        Commands::Cache { action } => {
            match action {
                CacheAction::Clear => {
                    // Delete .rsb directory directly — must work even if the
                    // database is corrupted and Builder::new() would fail.
                    let rsb_dir = std::path::Path::new(".rsb");
                    if rsb_dir.exists() {
                        fs::remove_dir_all(rsb_dir)?;
                    }
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
                    println!("Removed {} bytes ({} unreferenced objects)", bytes, count);
                }
                CacheAction::RemoveStale => {
                    let builder = Builder::new()?;
                    let valid_keys = builder.valid_cache_keys()?;
                    let stale_count = builder.object_store().remove_stale(&valid_keys);
                    let (bytes, trim_count) = builder.object_store().trim()?;
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
            match action {
                cli::ProcessorAction::List { all } => {
                    // Try with project config; fall back to no-config listing
                    match Builder::new() {
                        Ok(builder) => builder.processor(cli::ProcessorAction::List { all })?,
                        Err(_) => builder::processors::list_processors_no_config(all)?,
                    }
                }
                cli::ProcessorAction::Defconfig { ref name } => {
                    match Builder::new() {
                        Ok(builder) => builder.processor(action)?,
                        Err(_) => builder::processors::processor_defconfig(name)?,
                    }
                }
                action => {
                    let builder = Builder::new()?;
                    builder.processor(action)?;
                }
            }
        }
        Commands::Tools { action } => {
            let builder = Builder::new()?;
            builder.tools(action, cli.verbose)?;
        }
        Commands::Complete { shells } => {
            let shells_to_generate = if shells.is_empty() {
                // Load from config file
                let config = Config::load()?;
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
            let is_dirty = std::process::Command::new("git")
                .args(["diff", "--quiet", "HEAD"])
                .status()
                .is_ok_and(|s| !s.success());
            let dirty_str = if is_dirty { "true" } else { "false" };
            let describe = if is_dirty {
                format!("{}-dirty", env!("RSB_GIT_DESCRIBE"))
            } else {
                env!("RSB_GIT_DESCRIBE").to_string()
            };
            println!("rsb {} by {}", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_AUTHORS"));
            println!("RSB_GIT_DESCRIBE: {}", describe);
            println!("VERGEN_GIT_SHA: {}", env!("VERGEN_GIT_SHA"));
            println!("VERGEN_GIT_BRANCH: {}", env!("VERGEN_GIT_BRANCH"));
            println!("VERGEN_GIT_DIRTY: {}", dirty_str);
            println!("VERGEN_RUSTC_SEMVER: {}", env!("VERGEN_RUSTC_SEMVER"));
        }
        Commands::Config { action } => {
            let builder = Builder::new()?;
            builder.config(action)?;
        }
        Commands::Graph { action } => {
            let builder = Builder::new()?;
            builder.graph(action)?;
        }
        Commands::Deps { action } => {
            let builder = Builder::new()?;
            builder.deps(action)?;
        }
        Commands::Doctor => {
            let builder = Builder::new()?;
            builder.doctor()?;
        }
        Commands::Sloc { cocomo, salary } => {
            let file_index = file_index::FileIndex::build()?;
            builder::sloc::run_sloc(&file_index, cocomo, salary)?;
        }
        Commands::Smart { action } => {
            match action {
                cli::SmartAction::DisableAll => {
                    builder::smart::disable_all()?;
                }
                cli::SmartAction::EnableAll => {
                    builder::smart::enable_all()?;
                }
                cli::SmartAction::Disable { ref name } => {
                    builder::smart::disable(name)?;
                }
                cli::SmartAction::Enable { ref name } => {
                    builder::smart::enable(name)?;
                }
                cli::SmartAction::EnableDetected => {
                    let builder = Builder::new()?;
                    let detected = builder.detected_processors()?;
                    builder::smart::enable_detected(&detected)?;
                }
                cli::SmartAction::Minimal => {
                    let builder = Builder::new()?;
                    let detected = builder.detected_processors()?;
                    builder::smart::minimal(&detected)?;
                }
                cli::SmartAction::Reset => {
                    builder::smart::reset()?;
                }
                cli::SmartAction::EnableIfAvailable => {
                    let builder = Builder::new()?;
                    let available = builder.detected_and_available_processors()?;
                    builder::smart::enable_if_available(&available)?;
                }
                cli::SmartAction::Only { ref names } => {
                    builder::smart::only(names)?;
                }
            }
        }
        Commands::Tags { action } => {
            let config = Config::load()?;
            let db_path = &config.processor.tags.output;
            let tags_file = &config.processor.tags.tags_file;
            match action {
                cli::TagsAction::Files { tags, or } => processors::tags_cmd::files_for_tags(db_path, &tags, or)?,
                cli::TagsAction::Grep { text, ignore_case } => processors::tags_cmd::grep_tags(db_path, &text, ignore_case)?,
                cli::TagsAction::List => processors::tags_cmd::list_tags(db_path)?,
                cli::TagsAction::Count => processors::tags_cmd::count_tags(db_path)?,
                cli::TagsAction::Tree => processors::tags_cmd::tree_tags(db_path)?,
                cli::TagsAction::Stats => processors::tags_cmd::stats_tags(db_path)?,
                cli::TagsAction::ForFile { path } => processors::tags_cmd::tags_for_file(db_path, &path)?,
                cli::TagsAction::Frontmatter { path } => processors::tags_cmd::frontmatter_for_file(db_path, &path)?,
                cli::TagsAction::Unused { strict } => processors::tags_cmd::unused_tags(db_path, tags_file, strict)?,
                cli::TagsAction::Validate => processors::tags_cmd::validate_tags(db_path, tags_file)?,
                cli::TagsAction::Init => processors::tags_cmd::init_tags(db_path, tags_file)?,
                cli::TagsAction::Add { tag } => processors::tags_cmd::add_tag(tags_file, &tag)?,
                cli::TagsAction::Remove { tag } => processors::tags_cmd::remove_tag(tags_file, &tag)?,
                cli::TagsAction::Sync { prune } => processors::tags_cmd::sync_tags(db_path, tags_file, prune, cli.verbose)?,
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
# enabled = ["tera", "ruff", "pylint", "cc_single_file", "cppcheck", "shellcheck", "spellcheck", "make"]

[cache]
# restore_method = "hardlink"  # or "copy"

[processor.tera]
# strict = true
# scan_dir = "templates.tera"
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

[processor.gem]
# bundler = "bundle"
# command = "install"
# gem_home = "gems"

[processor.mdl]
# gem_home = "gems"
# mdl_bin = "gems/bin/mdl"
# args = []
# extra_inputs = []
# gem_stamp = "out/gem/root.stamp"
# extensions = [".md"]

[processor.markdownlint]
# markdownlint_bin = "node_modules/.bin/markdownlint"
# args = []
# extra_inputs = []
# npm_stamp = "out/npm/root.stamp"
# extensions = [".md"]

[processor.aspell]
# aspell = "aspell"
# conf_dir = "."
# conf = ".aspell.conf"
# args = []
# extra_inputs = []
# extensions = [".md"]

[processor.pandoc]
# pandoc = "pandoc"
# from = "markdown"
# formats = ["pdf"]
# args = []
# output_dir = "out/pandoc"
# extensions = [".md"]

[processor.markdown]
# markdown_bin = "markdown"
# args = []
# output_dir = "out/markdown"
# extensions = [".md"]

[processor.pdflatex]
# pdflatex = "pdflatex"
# args = []
# runs = 2
# qpdf = true
# output_dir = "out/pdflatex"
# extensions = [".tex"]

[processor.a2x]
# a2x = "a2x"
# format = "pdf"
# args = []
# output_dir = "out/a2x"
# extensions = [".txt"]

[processor.ascii_check]
# args = []
# extensions = [".md"]

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


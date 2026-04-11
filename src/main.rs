#![deny(clippy::all)]
#![deny(warnings)]

#[macro_use]
mod registry;

#[macro_use]
mod errors;
mod analyzers;
mod builder;
mod checksum;
mod cli;
mod color;
mod config;
mod db;
mod deps_cache;
mod executor;
mod exit_code;
mod file_index;
mod graph;
mod json_output;
mod object_store;
mod platform;
mod processors;
mod progress;
pub(crate) mod word_manager;
mod runtime_flags;
mod remote_cache;
mod tool_lock;
mod watcher;
mod webcache;

use anyhow::{Context, bail, Result};
use cli::{BuildPhase, CacheAction, CleanAction, Commands, WebCacheAction, parse_shell, print_completions};
use config::Config;
use builder::Builder;
use exit_code::{RsconstructExitCode, RsconstructError, classify_error};
use std::env;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

fn main() -> std::process::ExitCode {
    platform::reset_sigpipe();

    match run() {
        Ok(()) => std::process::ExitCode::from(RsconstructExitCode::Success.code()),
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
    let cli = cli::parse_cli();
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

    // Apply CLI override for mtime cache before any Builder is created
    if cli.no_mtime_cache {
        checksum::set_mtime_check(false);
    }

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
        Commands::Cache { action } => {
            match action {
                CacheAction::Clear => {
                    // Delete .rsconstruct directory directly — must work even if the
                    // database is corrupted and Builder::new() would fail.
                    let rsconstruct_dir = std::path::Path::new(".rsconstruct");
                    if rsconstruct_dir.exists() {
                        ctx!(fs::remove_dir_all(rsconstruct_dir), "Failed to remove .rsconstruct directory")?;
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
                CleanAction::Unknown { dry_run, no_gitignore } => {
                    let builder = Builder::new()?;
                    builder.clean_unknown(!dry_run, cli.verbose, !no_gitignore)?;
                }
            }
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
        Commands::Config { action } => {
            let builder = Builder::new()?;
            builder.config(action)?;
        }
        Commands::Deps { action } => {
            if matches!(action, cli::DepsAction::List) {
                builder::deps::list_analyzers();
            } else {
                let builder = Builder::new()?;
                builder.deps(action)?;
            }
        }
        Commands::Doctor => {
            let builder = Builder::new()?;
            builder.doctor()?;
        }
        Commands::Graph { action } => {
            let builder = Builder::new()?;
            builder.graph(action)?;
        }
        Commands::Info { action } => {
            match action {
                cli::InfoAction::Source => {
                    let builder = Builder::new()?;
                    builder.info_source()?;
                }
            }
        }
        Commands::Init => {
            init_project()?;
        }
        Commands::Processors { action } => {
            let has_config = std::path::Path::new("rsconstruct.toml").exists();
            match action {
                cli::ProcessorAction::List => {
                    builder::processors::list_processors_no_config(cli.verbose)?;
                }
                cli::ProcessorAction::Types => {
                    builder::processors::list_processor_types()?;
                }
                cli::ProcessorAction::Recommend => {
                    builder::processors::list_recommendations();
                }
                cli::ProcessorAction::Defconfig { ref pname } => {
                    builder::processors::processor_defconfig(pname)?;
                }
                cli::ProcessorAction::Config { .. } if !has_config => {
                    bail!("No rsconstruct.toml found. Use 'processors defconfig <name>' to see default config without a project.");
                }
                action => {
                    let builder = Builder::new()?;
                    builder.processor(action, cli.verbose)?;
                }
            }
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
                cli::SmartAction::Auto => {
                    let builder = Builder::new()?;
                    let detected = builder.detected_and_available_processors()?;
                    builder::smart::auto(&detected)?;
                }
                cli::SmartAction::RemoveNoFileProcessors => {
                    let builder = Builder::new()?;
                    let empty = builder.no_file_processors()?;
                    builder::smart::remove_no_file_processors(&empty)?;
                }
            }
        }
        Commands::Status { breakdown } => {
            let builder = Builder::new()?;
            builder.status(cli.verbose, breakdown)?;
        }
        Commands::SymlinkInstall => {
            let config = Config::load()?;
            builder::symlink_install::run(&config.command.symlink_install)?;
        }
        Commands::Terms { action } => {
            let config = Config::load()?;
            let mut terms_config: config::TermsConfig = config.processor
                .first_instance_of_type("terms")
                .map(|inst| inst.config_toml.clone().try_into())
                .transpose()
                .context("Failed to parse terms config")?
                .unwrap_or_default();
            if let Some(defaults) = config::scan_defaults_for("terms") {
                terms_config.standard.scan.resolve_with(&defaults);
            }
            match action {
                cli::TermsAction::Fix { remove_non_terms } => {
                    processors::terms::fix_all(&terms_config, remove_non_terms)?;
                }
                cli::TermsAction::Merge { path } => {
                    processors::terms::merge_terms(&terms_config, &path)?;
                }
                cli::TermsAction::Stats => {
                    processors::terms::stats(&terms_config)?;
                }
            }
        }
        Commands::Tags { action } => {
            let config = Config::load()?;
            let db_path = config.processor.instance_field_str("tags", "output")
                .unwrap_or_else(|| "out/tags/tags.db".into());
            let tags_dir = config.processor.instance_field_str("tags", "tags_dir")
                .unwrap_or_else(|| "tags".into());
            match action {
                cli::TagsAction::Files { tags, or } => processors::tags_cmd::files_for_tags(&db_path, &tags, or)?,
                cli::TagsAction::Grep { text, ignore_case } => processors::tags_cmd::grep_tags(&db_path, &text, ignore_case)?,
                cli::TagsAction::List => processors::tags_cmd::list_tags(&db_path)?,
                cli::TagsAction::Count => processors::tags_cmd::count_tags(&db_path)?,
                cli::TagsAction::Tree => processors::tags_cmd::tree_tags(&db_path)?,
                cli::TagsAction::Stats => processors::tags_cmd::stats_tags(&db_path)?,
                cli::TagsAction::ForFile { path } => processors::tags_cmd::tags_for_file(&db_path, &path)?,
                cli::TagsAction::Frontmatter { path } => processors::tags_cmd::frontmatter_for_file(&db_path, &path)?,
                cli::TagsAction::Unused { strict } => processors::tags_cmd::unused_tags(&db_path, &tags_dir, strict)?,
                cli::TagsAction::Validate => processors::tags_cmd::validate_tags(&db_path, &tags_dir)?,
                cli::TagsAction::Matrix => processors::tags_cmd::matrix_tags(&db_path)?,
                cli::TagsAction::Coverage => processors::tags_cmd::coverage_tags(&db_path)?,
                cli::TagsAction::Orphans => processors::tags_cmd::orphan_files(&db_path)?,
                cli::TagsAction::Check => {
                    let tags_config: config::TagsConfig = config.processor
                        .first_instance_of_type("tags")
                        .map(|inst| inst.config_toml.clone().try_into())
                        .transpose()
                        .context("Failed to parse tags config")?
                        .unwrap_or_default();
                    processors::tags_cmd::check_tags(&tags_config)?;
                }
                cli::TagsAction::Suggest { path } => processors::tags_cmd::suggest_tags(&db_path, &path)?,
                cli::TagsAction::Merge { path } => processors::tags_cmd::merge_tags(&tags_dir, &path)?,
                cli::TagsAction::Collect => processors::tags_cmd::collect_tags(&db_path, &tags_dir)?,
            }
        }
        Commands::Toml { action } => {
            match action {
                cli::TomlAction::Check => {
                    config::Config::require_config()?;
                    // Config::load() validates all fields — unknown fields, types, required fields.
                    // If it succeeds, the config is valid.
                    let _config = config::Config::load()?;
                    println!("rsconstruct.toml is valid.");
                }
            }
        }
        Commands::Tools { action } => {
            // Fall back to default config only if no config file exists.
            // If config exists but is broken, fail — don't silently use defaults.
            if std::path::Path::new("rsconstruct.toml").exists() {
                let builder = Builder::new()?;
                builder.tools(action, cli.verbose)?;
            } else {
                builder::tools::tools_no_config(action, cli.verbose)?;
            }
        }
        Commands::Version => {
            println!("rsconstruct {} by {}", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_AUTHORS"));
            println!("GIT_DESCRIBE: {}", env!("GIT_DESCRIBE"));
            println!("GIT_SHA: {}", env!("GIT_SHA"));
            println!("GIT_BRANCH: {}", env!("GIT_BRANCH"));
            println!("GIT_DIRTY: {}", env!("GIT_DIRTY"));
            println!("RUSTC_SEMVER: {}", env!("RUSTC_SEMVER"));
            println!("RUST_EDITION: {}", env!("RUST_EDITION"));
            println!("BUILD_TIMESTAMP: {}", env!("BUILD_TIMESTAMP"));
        }
        Commands::WebCache { action } => {
            match action {
                WebCacheAction::Clear => {
                    let count = webcache::clear()?;
                    println!("Removed {} cached entries.", count);
                }
                WebCacheAction::Stats => {
                    let (bytes, count) = webcache::stats()?;
                    println!("Web cache: {} ({} entries)",
                        humansize::format_size(bytes, humansize::BINARY), count);
                }
                WebCacheAction::List => {
                    let entries = webcache::list()?;
                    if entries.is_empty() {
                        println!("Web cache is empty.");
                    } else {
                        let mut builder = tabled::builder::Builder::new();
                        builder.push_record(["URL", "Size"]);
                        for entry in &entries {
                            builder.push_record([
                                entry.url.clone(),
                                humansize::format_size(entry.size, humansize::BINARY),
                            ]);
                        }
                        color::print_table(builder.build());
                    }
                }
            }
        }
        Commands::Watch { ref shared } => {
            let opts = shared.to_build_options(&cli, false, BuildPhase::Build);
            watcher::watch(&opts, Arc::clone(&interrupted))?;
        }
    }

    Ok(())
}

/// Initialize a new rsconstruct project in the current directory
fn init_project() -> Result<()> {
    let cwd = env::current_dir()?;
    let config_path = cwd.join("rsconstruct.toml");

    if config_path.exists() {
        return Err(RsconstructError::new(
            RsconstructExitCode::ConfigError,
            "rsconstruct.toml already exists in the current directory",
        ).into());
    }

    // Create rsconstruct.toml with commented defaults
    let config_content = r#"# RSConstruct Build Tool Configuration
# Uncomment [processor.NAME] sections to enable processors.
# Each section declares a processor instance; removing it disables the processor.
# For multiple instances: [processor.pylint.core] and [processor.pylint.tests]

[build]
# Number of parallel jobs (1 = sequential, 0 = auto-detect CPU cores)
# parallel = 1
# Max files per batch for batch-capable processors (0 = no limit, omit to disable batching)
# batch_size = 0

[cache]
# restore_method = "auto"  # auto (default: copy in CI, hardlink otherwise), hardlink, or copy

# Uncomment processors you want to use:

# [processor.tera]
# strict = true
# src_dirs = ["tera.templates"]
# src_extensions = [".tera"]

# [processor.ruff]
# command = "ruff"
# args = []

# [processor.pylint]
# args = []

# [processor.cc_single_file]
# cc = "gcc"
# cxx = "g++"
# src_dirs = ["src"]
# src_extensions = [".c", ".cc"]

# [processor.cppcheck]
# args = ["--error-exitcode=1", "--enable=warning,style,performance,portability"]

# [processor.shellcheck]
# args = []

# [processor.make]
# make = "make"

# [processor.cargo]
# cargo = "cargo"

[graph]
# viewer = "google-chrome"

[completions]
# shells = ["bash"]

# [plugins]
# dir = "plugins"  # directory containing .lua processor plugins
"#;
    ctx!(fs::write(&config_path, config_content), format!("Failed to write {}", config_path.display()))?;
    println!("Created {}", config_path.display());

    // Create .rsconstructignore if it doesn't exist
    let rsconstructignore_path = cwd.join(".rsconstructignore");
    if !rsconstructignore_path.exists() {
        let rsconstructignore_content = r#"# .rsconstructignore - Exclude files from rsconstruct processing
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
        ctx!(fs::write(&rsconstructignore_path, rsconstructignore_content), "Failed to write .rsconstructignore")?;
        println!("Created .rsconstructignore");
    }

    println!("{}", color::green("Project initialized successfully!"));
    println!("{}", color::dim("Hint: edit .rsconstructignore to exclude files from processing"));
    Ok(())
}


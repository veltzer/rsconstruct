//! rsconstruct: Rust based fast build system.

mod analyzers;
mod build_context;
mod builder;
mod cache_key;
mod checksum;
mod cli;
mod color;
mod config;
mod db;
mod deps_cache;
mod display;
mod download;
mod engines;
mod errors;
mod executor;
mod exit_code;
mod file_index;
mod graph;
mod graph_render;
mod json_output;
mod numeric;
mod object_store;
mod orphans;
mod output;
mod phases;
mod platform;
mod processor;
mod progress;
mod python_env;
mod read_trace;
mod registries;
mod remote_cache;
mod runtime_flags;
mod sandbox;
mod stats;
mod tables;
mod tool_cache;
mod tool_lock;
mod tool_store;
mod tools;
mod watcher;
mod webcache;
pub(crate) mod word_manager;

use anyhow::{Context, Result};
use builder::Builder;
use cli::{BuildPhase, CleanAction, Commands, WebCacheAction, parse_shell, print_completions};
use config::Config;
use exit_code::{RsconstructError, RsconstructExitCode, classify_error};
use std::env;
use std::fs;
use std::sync::Arc;
use std::time::Instant;

fn main() -> std::process::ExitCode {
    platform::reset_sigpipe();
    // Must run before any thread exists (see platform::set_env): make
    // user-installed gem executables, the project's own node_modules
    // executables, the repo's .venv and the project's tool directory
    // resolvable for tool probes and every child process. The tool
    // directory goes last so it ends up first: the project's locked tools
    // win over everything else.
    tools::augment_path_with_user_gem_bins();
    tools::augment_path_with_node_modules_bin();
    tools::augment_path_with_repo_venv();
    tool_store::augment_env();

    let (result, show_status) = run();
    let exit_code = match result {
        Ok(()) => RsconstructExitCode::Success,
        Err(err) => {
            let exit_code = classify_error(&err);
            if json_output::is_json_mode() {
                let error_event = serde_json::json!({
                    "event": "error",
                    "exit_code": exit_code.code(),
                    "exit_code_name": exit_code.name(),
                    "message": format!("{:#}", err),
                });
                eprintln!("{error_event}");
            } else {
                eprintln!("Error [{}]: {:#}", exit_code.name(), err);
            }
            exit_code
        }
    };

    // Final status line — only for build/watch/clean where pass/fail matters.
    // Suppressed in quiet mode, JSON mode, and for informational commands.
    if show_status && !runtime_flags::quiet_or_default() && !runtime_flags::json_mode_or_default() {
        let line = format!("Exited with {} ({})", exit_code.name(), exit_code.code());
        if exit_code == RsconstructExitCode::Success {
            eprintln!("{}", color::green(&line));
        } else {
            eprintln!("{}", color::red(&line));
        }
    }

    std::process::ExitCode::from(exit_code.code())
}

/// Initialize the runtime flags from the command line (once, before any read).
fn init_runtime_flags(cli: &cli::Cli) {
    // JSON mode never emits color: stdout must be machine-readable, and
    // --color=always must not smuggle ANSI escapes into it.
    let color_enabled = !cli.format.json
        && match cli.color {
            cli::ColorMode::Always => true,
            cli::ColorMode::Never => false,
            cli::ColorMode::Auto => {
                // Disable if NO_COLOR is set to a non-empty value (per the
                // no-color.org spec, an empty value does NOT disable color)
                // or if stdout is not a tty.
                std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
                    && std::io::IsTerminal::is_terminal(&std::io::stdout())
            }
        };
    runtime_flags::init(runtime_flags::RuntimeFlags {
        diagnostics: runtime_flags::Diagnostics {
            show_child_processes: cli.tool_output.show_child_processes,
            phases_debug: cli.debug.phases,
            graph_stats: cli.debug.graph_stats,
        },
        output: runtime_flags::OutputFlags {
            show_output: cli.tool_output.show_output,
            json_mode: cli.format.json,
            quiet: cli.format.quiet,
        },
        tables: runtime_flags::TableFlags {
            all_fields: cli.tables.all,
            fields_only: cli.tables.fields,
        },
        color_enabled,
    });
}

/// Install the Ctrl+C handler, and return once it is in place.
fn install_interrupt_handler(ctx: &Arc<build_context::BuildContext>) {
    {
        let ctx_for_signal = Arc::clone(ctx);
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<()>(0);
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect(errors::SIGNAL_HANDLER_RUNTIME);
            rt.block_on(async {
                let mut interrupt = platform::interrupt_signal().expect(errors::SIGNAL_LISTEN);
                // Handler installed — let run() proceed. Before this rendezvous
                // a Ctrl+C would hit the default disposition and kill the
                // process instantly, bypassing interrupt() and every Drop.
                let _ = ready_tx.send(());
                interrupt.recv().await;
                ctx_for_signal.interrupt();
                eprintln!("\nInterrupted. Press Ctrl+C again to force exit.");
                interrupt.recv().await;
                std::process::exit(130);
            });
        });
        // Bounded by thread startup + signal registration: microseconds.
        let _ = ready_rx.recv();
    }
}

/// `rsconstruct clean`.
fn cmd_clean(ctx: &build_context::BuildContext, action: CleanAction, verbose: bool) -> Result<()> {
    match action {
        CleanAction::Outputs {
            processor,
            no_empty_dirs,
        } => {
            let builder = Builder::new(ctx)?;
            let filter = if processor.is_empty() {
                None
            } else {
                Some(processor)
            };
            builder.clean(ctx, verbose, filter.as_deref(), !no_empty_dirs)?;
        }
        CleanAction::All {
            processor,
            dry_run,
            no_empty_dirs,
        } => {
            let builder = Builder::new(ctx)?;
            if processor.is_empty() {
                builder.distclean(dry_run)?;
            } else {
                builder.clean_all_of(ctx, verbose, &processor, dry_run, !no_empty_dirs)?;
            }
        }
        CleanAction::Git => {
            // Constructing the builder validates the config, as every
            // other clean action does; the clean itself needs no state.
            Builder::new(ctx)?;
            Builder::hardclean(ctx)?;
        }
        CleanAction::Unknown {
            dry_run,
            no_gitignore,
        } => {
            let builder = Builder::new(ctx)?;
            builder.clean_unknown(ctx, !dry_run, verbose, !no_gitignore)?;
        }
    }
    Ok(())
}

/// `rsconstruct complete`: the named shells, or the ones the config lists.
fn cmd_complete(shells: Vec<clap_complete::Shell>) -> Result<()> {
    let shells_to_generate = if shells.is_empty() {
        // Load from config file
        let config = Config::load()?;
        let mut parsed_shells = Vec::new();
        for shell_name in &config.completions.shells {
            match parse_shell(shell_name) {
                Some(shell) => parsed_shells.push(shell),
                None => {
                    return Err(crate::exit_code::config_error(format!(
                        "Unknown shell in config: {shell_name}"
                    )));
                }
            }
        }
        parsed_shells
    } else {
        shells
    };

    for shell in shells_to_generate {
        print_completions(shell)?;
    }
    Ok(())
}

/// `rsconstruct analyzer`: what needs no project first, the rest through a builder.
fn cmd_analyzer(
    ctx: &build_context::BuildContext,
    action: cli::AnalyzerAction,
    verbose: bool,
) -> Result<()> {
    match &action {
        cli::AnalyzerAction::List => {
            builder::analyzers::list_analyzers(verbose);
        }
        cli::AnalyzerAction::Defconfig { pname } => {
            builder::analyzers::analyzer_defconfig(pname.as_deref())?;
        }
        cli::AnalyzerAction::Add { pname, dry_run } => {
            builder::add_analyzer(pname, *dry_run)?;
        }
        cli::AnalyzerAction::Delete { iname } => {
            builder::smart::delete_analyzer(iname)?;
        }
        cli::AnalyzerAction::Disable { iname } => {
            builder::smart::disable_analyzer(iname)?;
        }
        cli::AnalyzerAction::Enable { iname } => {
            builder::smart::enable_analyzer(iname)?;
        }
        _ => {
            let builder = Builder::new(ctx)?;
            builder.analyzers(ctx, action, verbose)?;
        }
    }
    Ok(())
}

/// `rsconstruct page`.
fn cmd_page(action: &cli::PageAction) -> Result<()> {
    match action {
        cli::PageAction::Dir => {
            config::Config::require_config()?;
            let config = config::Config::load()?;
            if json_output::is_json_mode() {
                println!(
                    "{}",
                    serde_json::json!({
                        "configured": config.pages.is_some(),
                        "dir": config.pages.as_ref().map_or("", |p| p.dir.as_str()),
                    })
                );
            } else if let Some(pages) = &config.pages {
                println!("{}", pages.dir);
            }
            // Not configured: print nothing, exit 0 — CI branches on
            // empty output, not on exit codes.
        }
    }
    Ok(())
}

/// `rsconstruct processor`: what needs no project first, the rest through a builder.
fn cmd_processor(
    ctx: &build_context::BuildContext,
    action: cli::ProcessorAction,
    verbose: bool,
) -> Result<()> {
    let has_config = std::path::Path::new("rsconstruct.toml").exists();
    match action {
        cli::ProcessorAction::List { ref processor_type } => {
            builder::processor::list_processors_no_config(verbose, processor_type.as_deref())?;
        }
        cli::ProcessorAction::Types => {
            builder::processor::list_processor_types(verbose)?;
        }
        cli::ProcessorAction::Recommend => {
            builder::processor::list_recommendations();
        }
        cli::ProcessorAction::Defconfig { ref pname } => {
            builder::processor::processor_defconfig(pname, verbose)?;
        }
        cli::ProcessorAction::Info { ref pname } => {
            builder::processor::processor_info(pname.as_deref())?;
        }
        cli::ProcessorAction::Add { ref pname, dry_run } => {
            builder::add_processor(pname, dry_run)?;
        }
        cli::ProcessorAction::Search { ref query } => {
            builder::processor::search_processors(query)?;
        }
        cli::ProcessorAction::Delete { ref iname } => {
            builder::smart::delete_processor(iname)?;
        }
        cli::ProcessorAction::Disable { ref iname } => {
            builder::smart::disable_processor(iname)?;
        }
        cli::ProcessorAction::Enable { ref iname } => {
            builder::smart::enable_processor(iname)?;
        }
        cli::ProcessorAction::Config { .. } if !has_config => {
            return Err(exit_code::config_error(
                "No rsconstruct.toml found. Use 'processor defconfig <name>' to see default config without a project.",
            ));
        }
        action => {
            let builder = Builder::new(ctx)?;
            builder.processor(ctx, action, verbose)?;
        }
    }
    Ok(())
}

/// `rsconstruct sloc`.
fn cmd_sloc(cocomo: bool, salary: u64) -> Result<()> {
    // Generated files are not project code: exclude configured
    // output roots from the count. Config::load falls back to
    // defaults when no rsconstruct.toml exists, so sloc still works
    // outside a project.
    let config = Config::load()?;
    let (exclude_roots, _) = config.file_index_walk_dirs();
    let file_index = file_index::FileIndex::build_with_force_dirs(
        &[],
        &exclude_roots,
        config.build.warn_symlinks,
    )?;
    builder::sloc::run_sloc(&file_index, cocomo, salary)?;
    Ok(())
}

/// `rsconstruct smart`.
fn cmd_smart(ctx: &build_context::BuildContext, action: &cli::SmartAction) -> Result<()> {
    match action {
        cli::SmartAction::DisableAll => {
            builder::smart::disable_all()?;
        }
        cli::SmartAction::EnableAll => {
            builder::smart::enable_all()?;
        }
        cli::SmartAction::Disable { name } => {
            builder::smart::disable(name)?;
        }
        cli::SmartAction::Enable { name } => {
            builder::smart::enable(name)?;
        }
        cli::SmartAction::EnableDetected => {
            let builder = Builder::new(ctx)?;
            let detected = builder.detected_processors()?;
            builder::smart::enable_detected(&detected)?;
        }
        cli::SmartAction::Minimal => {
            let builder = Builder::new(ctx)?;
            let detected = builder.detected_processors()?;
            builder::smart::minimal(&detected)?;
        }
        cli::SmartAction::Reset => {
            builder::smart::reset()?;
        }
        cli::SmartAction::EnableIfAvailable => {
            let builder = Builder::new(ctx)?;
            let available = builder.detected_and_available_processors()?;
            builder::smart::enable_if_available(&available)?;
        }
        cli::SmartAction::Only { names } => {
            builder::smart::only(names)?;
        }
        cli::SmartAction::Auto => {
            let builder = Builder::new(ctx)?;
            let detected = builder.detected_and_available_processors()?;
            builder::smart::auto(&detected)?;
        }
        cli::SmartAction::RemoveNoFileProcessors => {
            let builder = Builder::new(ctx)?;
            let empty = builder.no_file_processors(ctx)?;
            builder::smart::remove_no_file_processors(&empty)?;
        }
    }
    Ok(())
}

/// `rsconstruct term`.
fn cmd_term(action: cli::TermAction) -> Result<()> {
    Config::require_config()?;
    let config = Config::load()?;
    let terms_config: processor::terms::TermsConfig = config
        .processor
        .instance_config_or_default(processor::names::TERMS)?;
    match action {
        cli::TermAction::Fix { remove_non_terms } => {
            processor::terms::fix_all(&terms_config, remove_non_terms, config.build.warn_symlinks)?;
        }
        cli::TermAction::Merge { path } => {
            processor::terms::merge_terms(&terms_config, &path)?;
        }
        cli::TermAction::Stats => {
            processor::terms::stats(&terms_config)?;
        }
    }
    Ok(())
}

/// `rsconstruct tag`.
fn cmd_tag(action: cli::TagAction) -> Result<()> {
    Config::require_config()?;
    let config = Config::load()?;
    let db_path = config
        .processor
        .instance_field_str(processor::names::TAGS, "output")
        .unwrap_or_else(|| "out/processor.generator.tags/tags.db".into());
    let tags_dir = config
        .processor
        .instance_field_str(processor::names::TAGS, "tags_dir")
        .unwrap_or_else(|| "tags".into());
    match action {
        cli::TagAction::Files { tags, or } => {
            processor::tags_cmd::files_for_tags(&db_path, &tags, or)?;
        }
        cli::TagAction::Grep { text, ignore_case } => {
            processor::tags_cmd::grep_tags(&db_path, &text, ignore_case)?;
        }
        cli::TagAction::List => processor::tags_cmd::list_tags(&db_path)?,
        cli::TagAction::Count => processor::tags_cmd::count_tags(&db_path)?,
        cli::TagAction::Tree => processor::tags_cmd::tree_tags(&db_path)?,
        cli::TagAction::Stats => processor::tags_cmd::stats_tags(&db_path)?,
        cli::TagAction::ForFile { path } => {
            processor::tags_cmd::tags_for_file(&db_path, &path)?;
        }
        cli::TagAction::Frontmatter { path } => {
            processor::tags_cmd::frontmatter_for_file(&db_path, &path)?;
        }
        cli::TagAction::Unused { strict } => {
            processor::tags_cmd::unused_tags(&db_path, &tags_dir, strict)?;
        }
        cli::TagAction::Validate => {
            processor::tags_cmd::validate_tags(&db_path, &tags_dir)?;
        }
        cli::TagAction::Matrix => processor::tags_cmd::matrix_tags(&db_path)?,
        cli::TagAction::Coverage => processor::tags_cmd::coverage_tags(&db_path)?,
        cli::TagAction::Orphans => processor::tags_cmd::orphan_files(&db_path)?,
        cli::TagAction::Check => {
            let tags_config: processor::tags_cmd::TagsConfig = config
                .processor
                .instance_config_or_default(processor::names::TAGS)?;
            processor::tags_cmd::check_tags(&tags_config, config.build.warn_symlinks)?;
        }
        cli::TagAction::Suggest { path } => {
            let tags_config: processor::tags_cmd::TagsConfig = config
                .processor
                .instance_config_or_default(processor::names::TAGS)?;
            processor::tags_cmd::suggest_tags(&db_path, &path, &tags_config)?;
        }
        cli::TagAction::Merge { path } => {
            processor::tags_cmd::merge_tags(&tags_dir, &path)?;
        }
        cli::TagAction::Collect => {
            processor::tags_cmd::collect_tags(&db_path, &tags_dir)?;
        }
    }
    Ok(())
}

/// `rsconstruct toml`.
fn cmd_toml(action: &cli::TomlAction) -> Result<()> {
    match action {
        cli::TomlAction::Check => {
            config::Config::require_config()?;
            // Config::load() validates field names and the
            // shape of the file. A field's value is checked by
            // the config type of its processor or analyzer, so
            // every one is built too: a config that `check`
            // calls valid must be one `build` can start on.
            let config = config::Config::load()?;
            builder::create_processors_of(&config)?;
            builder::create_analyzers_of(&config, false)?;
            if json_output::is_json_mode() {
                println!("{}", serde_json::json!({ "valid": true }));
            } else {
                output::info("rsconstruct.toml is valid.");
            }
        }
        cli::TomlAction::Files => {
            let files = config::config_files();
            if json_output::is_json_mode() {
                println!("{}", serde_json::to_string_pretty(&files)?);
            } else {
                print_config_files(&files);
            }
        }
    }
    Ok(())
}

/// `rsconstruct function`.
fn cmd_function(action: &cli::FunctionAction) -> Result<()> {
    use cli::FunctionAction;
    use processor::generator::tera::TERA_FUNCTIONS;
    match action {
        FunctionAction::List => {
            if json_output::is_json_mode() {
                let arr: Vec<serde_json::Value> = TERA_FUNCTIONS
                    .iter()
                    .map(|f| {
                        serde_json::json!({
                            "name": f.name,
                            "summary": f.summary,
                            "args": f.args,
                            "returns": f.returns,
                            "dep_tracking": f.dep_tracking,
                            "example": f.example,
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::Value::Array(arr))?
                );
            } else {
                for f in TERA_FUNCTIONS {
                    println!("{}({})", f.name, f.args);
                    println!("  {}", f.summary);
                    println!("  returns:       {}", f.returns);
                    println!("  dep tracking:  {}", f.dep_tracking);
                    println!("  example:       {}", f.example);
                    println!();
                }
            }
        }
    }
    Ok(())
}

/// `rsconstruct tool`.
fn cmd_tool(
    ctx: &build_context::BuildContext,
    action: cli::ToolAction,
    verbose: bool,
) -> Result<()> {
    // Fall back to default config only if no config file exists.
    // If config exists but is broken, fail — don't silently use defaults.
    if std::path::Path::new("rsconstruct.toml").exists() {
        let builder = Builder::new(ctx)?;
        builder.tools(ctx, action, verbose)?;
    } else {
        builder::tools::tools_no_config(ctx, action, verbose)?;
    }
    Ok(())
}

/// `rsconstruct version`.
fn print_version() -> Result<()> {
    if json_output::is_json_mode() {
        let info = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "authors": env!("CARGO_PKG_AUTHORS"),
            "git_describe": env!("GIT_DESCRIBE"),
            "git_sha": env!("GIT_SHA"),
            "git_branch": env!("GIT_BRANCH"),
            "git_dirty": env!("GIT_DIRTY"),
            "rustc_semver": env!("RUSTC_SEMVER"),
            "rust_edition": env!("RUST_EDITION"),
            "build_timestamp": env!("BUILD_TIMESTAMP"),
        });
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!(
            "rsconstruct {} by {}",
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_AUTHORS")
        );
        println!("GIT_DESCRIBE: {}", env!("GIT_DESCRIBE"));
        println!("GIT_SHA: {}", env!("GIT_SHA"));
        println!("GIT_BRANCH: {}", env!("GIT_BRANCH"));
        println!("GIT_DIRTY: {}", env!("GIT_DIRTY"));
        println!("RUSTC_SEMVER: {}", env!("RUSTC_SEMVER"));
        println!("RUST_EDITION: {}", env!("RUST_EDITION"));
        println!("BUILD_TIMESTAMP: {}", env!("BUILD_TIMESTAMP"));
    }
    Ok(())
}

/// `rsconstruct webcache`.
fn cmd_webcache(action: &WebCacheAction) -> Result<()> {
    match action {
        WebCacheAction::Clear => {
            let count = webcache::clear()?;
            if json_output::is_json_mode() {
                println!("{}", serde_json::json!({ "removed_entries": count }));
            } else {
                output::info(&format!("Removed {count} cached entries."));
            }
        }
        WebCacheAction::Stats => {
            let (bytes, count) = webcache::stats()?;
            if json_output::is_json_mode() {
                let out = serde_json::json!({ "bytes": bytes, "entries": count });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!(
                    "Web cache: {} ({} entries)",
                    humansize::format_size(bytes, humansize::BINARY),
                    count
                );
            }
        }
        WebCacheAction::List => {
            // Freshness is judged by the project's TTL, as a build
            // and `cache trim` judge it; outside a project, by the
            // default.
            let ttl_secs = if std::path::Path::new(config::CONFIG_FILE).exists() {
                config::Config::load()?.cache.webcache_ttl_secs
            } else {
                config::DEFAULT_WEBCACHE_TTL_SECS
            };
            let entries = webcache::list_with_ttl(ttl_secs)?;
            if json_output::is_json_mode() {
                let rows: Vec<serde_json::Value> = entries
                    .iter()
                    .map(|e| {
                        serde_json::json!({
                            "url": e.url,
                            "size": e.size,
                            "age_secs": e.age_secs,
                            "expired": e.expired,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else if entries.is_empty() {
                println!("Web cache is empty.");
            } else {
                let rows: Vec<Vec<String>> = entries
                    .iter()
                    .map(|entry| {
                        vec![
                            entry.url.clone(),
                            humansize::format_size(entry.size, humansize::BINARY),
                            format_age(entry.age_secs),
                            if entry.expired { "expired" } else { "fresh" }.to_string(),
                        ]
                    })
                    .collect();
                tables::print(&tables::defs::WEBCACHE_LIST, false, &rows);
            }
        }
    }
    Ok(())
}

/// Returns (result, `show_status_line`). The status line is only shown for
/// build, watch, and clean — commands where pass/fail matters to the user.
fn run() -> (Result<()>, bool) {
    let t_start = Instant::now();
    let cli = cli::parse_cli();
    let cli_parse_dur = t_start.elapsed();

    let t = Instant::now();
    init_runtime_flags(&cli);
    // Which columns each table shows, from the user config. Read here, once,
    // because tables are printed by commands with and without a project
    // config; a bad entry is a config error like any other.
    match config::user_tables_config() {
        Ok(tables_config) => tables::init(tables_config),
        Err(err) => return (Err(err), false),
    }

    // Create the build context before the signal handler so the handler can
    // call ctx.interrupt() to signal all running subprocesses.
    let ctx = Arc::new(build_context::BuildContext::new());

    // Apply CLI override for mtime cache
    if cli.no_mtime_cache {
        ctx.set_mtime_check(false);
    }

    // Set up Ctrl+C handler. `BuildContext::interrupt()` is the single
    // source of truth: it sets the flag the executor polls between products
    // AND broadcasts on the watch channel that wakes every waiting
    // subprocess. A second standalone `Arc<AtomicBool>` used to be threaded
    // through the executor and watcher in parallel with this, set on the
    // same line and only ever read OR'd with it — pure redundancy.
    install_interrupt_handler(&ctx);
    let init_timings = vec![
        ("cli_parse".to_string(), cli_parse_dur),
        ("init".to_string(), t.elapsed()),
    ];

    let show_status = matches!(
        cli.command,
        Commands::Build { .. } | Commands::Watch { .. } | Commands::Clean { .. }
    );

    let result = dispatch(cli, &ctx, init_timings);
    // Mtime entries staged by any command (status, dry-run, graph, ...) are
    // written once it is done. A command's own error outranks a failed write.
    let flushed = checksum::flush_mtime_cache(&ctx);
    let result = match (result, flushed) {
        (Ok(()), flushed) => flushed,
        (Err(e), Ok(())) => Err(e),
        (Err(e), Err(flush)) => {
            output::warn(&format!("{flush:#}"));
            Err(e)
        }
    };
    (result, show_status)
}

/// `rsconstruct build`: a preview with `dry_run`, the build otherwise.
/// `init_timings` are the startup phases measured so far.
fn cmd_build(
    ctx: &build_context::BuildContext,
    shared: &cli::SharedBuildArgs,
    opts: &cli::BuildOptions,
    dry_run: bool,
    mut init_timings: Vec<(String, std::time::Duration)>,
) -> Result<()> {
    let t = Instant::now();
    let mut builder = Builder::new_with_overrides(ctx, &shared.iset, &shared.pset)?;
    if dry_run {
        return builder.dry_run(ctx, opts);
    }
    init_timings.push(("builder_new".to_string(), t.elapsed()));
    builder.build(ctx, opts, init_timings)
}

/// Run the command the command line asked for.
fn dispatch(
    cli: cli::Cli,
    ctx: &build_context::BuildContext,
    init_timings: Vec<(String, std::time::Duration)>,
) -> Result<()> {
    let verbose = cli.verbose;
    match cli.command {
        Commands::Build {
            force,
            dry_run,
            verify_tool_versions,
            stop_after,
            ref shared,
        } => {
            // A preview never consults the lock. In CI the lock is enforced,
            // not opt-in: a runner whose tools differ from the committed lock
            // fails rather than producing a result no other runner
            // reproduces. The check itself runs inside the build, once
            // discovery has settled which processors (and so which tools) it
            // needs.
            let verify = !dry_run && (verify_tool_versions || crate::config::running_in_ci());
            let opts = shared.to_build_options(&cli, force, stop_after, verify);
            cmd_build(ctx, shared, &opts, dry_run, init_timings)
        }
        Commands::Cache { action } => builder::cache_cmd::handle(ctx, &action),
        Commands::Clean { action } => cmd_clean(ctx, action, verbose),
        Commands::Complete { shells } => cmd_complete(shells),
        Commands::Analyzer { action } => cmd_analyzer(ctx, action, verbose),
        Commands::Doctor => Builder::new(ctx)?.doctor(ctx),
        Commands::Error => list_exit_codes(verbose),
        Commands::Graph { action } => Builder::new(ctx)?.graph(ctx, action),
        Commands::Info {
            action: cli::InfoAction::Source,
        } => Builder::new(ctx)?.info_source(ctx),
        Commands::Init => init_project(),
        Commands::Page { action } => cmd_page(&action),
        Commands::Hook => list_hooks(verbose),
        Commands::Processor { action } => cmd_processor(ctx, action, verbose),
        Commands::Product {
            action: cli::ProductAction::Show { ref path },
        } => Builder::new(ctx)?.product_show(ctx, path, verbose),
        Commands::SandboxRun => crate::sandbox::sandbox_run(ctx),
        Commands::Sloc { cocomo, salary } => cmd_sloc(cocomo, salary),
        Commands::Smart { action } => cmd_smart(ctx, &action),
        Commands::Status { breakdown } => Builder::new(ctx)?.status(ctx, verbose, breakdown),
        Commands::SymlinkInstall => {
            Config::require_config()?;
            let config = Config::load()?;
            builder::symlink_install::run(&config.command.symlink_install)
        }
        Commands::Term { action } => cmd_term(action),
        Commands::Tag { action } => cmd_tag(action),
        Commands::Toml { action } => cmd_toml(&action),
        Commands::Function { action } => cmd_function(&action),
        Commands::Tool { action } => cmd_tool(ctx, action, verbose),
        Commands::Version => print_version(),
        Commands::WebCache { action } => cmd_webcache(&action),
        Commands::Watch { ref shared } => {
            let opts = shared.to_build_options(&cli, false, BuildPhase::Build, false);
            watcher::watch(ctx, &opts)
        }
    }
}

/// Render an age in seconds as a compact human string ("3d", "5h", "12m").
/// Coarse on purpose — this exists to answer "is this entry old?", and the
/// exact second a schema was fetched is never the question.
fn format_age(secs: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    if secs >= DAY {
        format!("{}d", secs / DAY)
    } else if secs >= HOUR {
        format!("{}h", secs / HOUR)
    } else if secs >= MINUTE {
        format!("{}m", secs / MINUTE)
    } else {
        format!("{secs}s")
    }
}

/// List all exit codes and their meanings.
fn list_exit_codes(verbose: bool) -> Result<()> {
    use exit_code::RsconstructExitCode;
    use strum::IntoEnumIterator;

    if json_output::is_json_mode() {
        #[derive(serde::Serialize)]
        struct Entry {
            code: u8,
            name: &'static str,
            description: &'static str,
        }
        let entries: Vec<Entry> = RsconstructExitCode::iter()
            .map(|e| Entry {
                code: e.code(),
                name: e.name(),
                description: e.description(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    let rows: Vec<Vec<String>> = RsconstructExitCode::iter()
        .map(|e| {
            vec![
                e.code().to_string(),
                e.name().to_string(),
                e.description().to_string(),
            ]
        })
        .collect();
    tables::print(&tables::defs::ERROR_LIST, verbose, &rows);
    Ok(())
}

fn list_hooks(verbose: bool) -> Result<()> {
    let hooks: Vec<&phases::PhaseHook> = phases::all_hooks().collect();
    if json_output::is_json_mode() {
        #[derive(serde::Serialize)]
        struct Entry {
            name: &'static str,
            description: &'static str,
            function: &'static str,
            location: &'static str,
        }
        let entries: Vec<Entry> = hooks
            .iter()
            .map(|h| Entry {
                name: h.name,
                description: h.description,
                function: h.function,
                location: h.location,
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        let rows: Vec<Vec<String>> = hooks
            .iter()
            .map(|h| {
                vec![
                    h.name.to_string(),
                    h.function.to_string(),
                    h.location.to_string(),
                    h.description.to_string(),
                ]
            })
            .collect();
        tables::print(&tables::defs::HOOK_LIST, verbose, &rows);
    }
    Ok(())
}

/// Initialize a new rsconstruct project in the current directory
fn init_project() -> Result<()> {
    let cwd = env::current_dir().context("Failed to get current directory for project init")?;
    let config_path = cwd.join("rsconstruct.toml");

    if config_path.exists() {
        return Err(RsconstructError::new(
            RsconstructExitCode::ConfigError,
            "rsconstruct.toml already exists in the current directory",
        )
        .into());
    }

    // Create rsconstruct.toml with commented defaults
    let config_content = r#"# RSConstruct Build Tool Configuration
# Uncomment [processor.TYPE.NAME] sections to enable processors.
# Each section declares a processor instance; removing it disables the processor.
# For multiple instances: [processor.checker.pylint.core] and [processor.checker.pylint.tests]

[build]
# Number of parallel jobs (0 = auto-detect CPU cores, 1 = sequential)
# parallel = 0
# Max files per batch for batch-capable processors (0 = no limit, the default; batching is on unless --batch-size -1)
# batch_size = 0

[cache]
# restore_method = "auto"  # auto (default: copy in CI, hardlink otherwise), hardlink, or copy

# Uncomment processors you want to use:

# [processor.generator.tera]
# src_dirs = ["tera.templates"]
# src_extensions = [".tera"]

# [processor.checker.ruff]
# command = "ruff"
# args = []

# [processor.checker.pylint]
# args = []

# [processor.generator.cc_single_file]
# cc = "gcc"
# cxx = "g++"
# src_dirs = ["src"]
# src_extensions = [".c", ".cc"]

# [processor.checker.cppcheck]
# args = ["--error-exitcode=1", "--enable=warning,style,performance,portability"]

# [processor.checker.shellcheck]
# args = []

# [processor.checker.make]
# target = "all"

# [processor.mass_generator.cargo]
# cargo = "cargo"

[graph]
# viewer = "google-chrome"

[completions]
# shells = ["bash"]
"#;
    crate::errors::ctx(
        fs::write(&config_path, config_content),
        &format!("Failed to write {}", config_path.display()),
    )?;
    println!("Created {}", config_path.display());

    // Create .rsconstructignore if it doesn't exist
    let rsconstructignore_path = cwd.join(".rsconstructignore");
    if !rsconstructignore_path.exists() {
        let rsconstructignore_content = r"# .rsconstructignore - Exclude files from rsconstruct processing
# Uses .gitignore syntax (glob patterns, one per line)
# Lines starting with # are comments
#
# Examples:
# /build/           # Exclude a top-level directory
# *.generated.*     # Exclude generated files by pattern
# /src/vendor/**    # Exclude vendored source code
# /experiments/     # Exclude experimental code
# *.bak             # Exclude backup files
";
        crate::errors::ctx(
            fs::write(&rsconstructignore_path, rsconstructignore_content),
            "Failed to write .rsconstructignore",
        )?;
        println!("Created .rsconstructignore");
    }

    println!("{}", color::green("Project initialized successfully!"));
    println!(
        "{}",
        color::dim("Hint: edit .rsconstructignore to exclude files from processing")
    );
    Ok(())
}

/// Text rendering of `rsconstruct toml files`: the merge chain first, then
/// the project files read for other purposes, one line each.
fn print_config_files(files: &[config::ConfigFile]) {
    let width = files
        .iter()
        .filter_map(|f| f.path.as_ref())
        .map(|p| p.display().to_string().len())
        .max()
        .unwrap_or(0);
    let render = |f: &config::ConfigFile| {
        let path = f.path.as_ref().map_or_else(
            || "(unknown: neither $XDG_CONFIG_HOME nor $HOME is set)".to_string(),
            |p| p.display().to_string(),
        );
        let state = if f.exists { "present" } else { "absent" };
        println!("  {:<13}  {path:<width$}  {state:<7}  {}", f.role, f.note);
    };
    println!(
        "Config files, lowest precedence first (a later file overrides an earlier one key by key):"
    );
    for f in files.iter().filter(|f| f.precedence.is_some()) {
        render(f);
    }
    println!("Other files read from the project:");
    for f in files.iter().filter(|f| f.precedence.is_none()) {
        render(f);
    }
}

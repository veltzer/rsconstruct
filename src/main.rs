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
use cli::{CacheAction, Cli, Commands, ConfigAction, ProcessorAction, ToolsAction, parse_shell, print_completions};
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
        Commands::Build { force, jobs, timings, keep_going, dry_run, processor_verbose, no_summary } => {
            if dry_run {
                let builder = Builder::new()?;
                builder.dry_run(force)?;
            } else {
                let mut builder = Builder::new()?;
                builder.build(force, cli.verbose, jobs, timings, keep_going, processor_verbose, Arc::clone(&interrupted), !no_summary)?;
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
            Config::require_config(&project_root)?;
            let config = Config::load(&project_root)?;
            let mut store = ObjectStore::new(project_root, config.cache.restore_method)?;

            match action {
                CacheAction::Clear => {
                    store.clear()?;
                    println!("Cache cleared.");
                }
                CacheAction::Size => {
                    let (bytes, count) = store.size()?;
                    println!("Cache size: {} ({} objects)", humansize::format_size(bytes, humansize::BINARY), count);
                }
                CacheAction::Trim => {
                    let (bytes, count) = store.trim()?;
                    store.save()?;
                    println!("Removed {} bytes ({} unreferenced objects)", bytes, count);
                }
                CacheAction::List => {
                    let entries = store.list();
                    println!("{}", serde_json::to_string_pretty(&entries)?);
                }
            }
        }
        Commands::Processor { action } => {
            let project_root = env::current_dir()?;
            Config::require_config(&project_root)?;
            let config = Config::load(&project_root)?;

            let all_processors: [(&str, &str, bool); 7] = [
                ("template", "Render Tera templates into output files", false),
                ("ruff", "Lint Python files with ruff", false),
                ("pylint", "Lint Python files with pylint", false),
                ("sleep", "Sleep for a duration (testing)", true),
                ("cc_single_file", "Compile C/C++ source files into executables (single-file)", false),
                ("cpplint", "Run static analysis on C/C++ source files", false),
                ("spellcheck", "Check documentation files for spelling errors", false),
            ];

            match action {
                ProcessorAction::List { all } => {
                    for (name, _desc, hidden) in &all_processors {
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
                    for (name, desc, hidden) in &all_processors {
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
                        println!("{} {}{} — {}", name, enabled_status, hidden_status, color::dim(desc));
                    }
                }
                ProcessorAction::Auto => {
                    let builder = Builder::new()?;
                    let processors = builder.create_processors(0)?;
                    for (name, _desc, _hidden) in &all_processors {
                        let detected = processors.get(*name)
                            .map_or(false, |p| p.auto_detect());
                        let enabled = config.processor.is_enabled(name);
                        let status = match (detected, enabled) {
                            (true, true) => color::green("detected, enabled"),
                            (true, false) => color::yellow("detected, disabled"),
                            (false, true) => color::yellow("not detected, enabled"),
                            (false, false) => color::dim("not detected, disabled"),
                        };
                        println!("{:<12} {}", name, status);
                    }
                }
                ProcessorAction::Files { name, all } => {
                    // Validate processor name if given
                    if let Some(ref n) = name {
                        if !all_processors.iter().any(|(pname, _, _)| *pname == n.as_str()) {
                            bail!("Unknown processor: '{}'. Run 'rsb processor list' to see available processors.", n);
                        }
                    }

                    let builder = Builder::new()?;
                    let graph = builder.build_graph_filtered(name.as_deref(), all)?;

                    let products = graph.products();
                    if products.is_empty() {
                        if let Some(ref n) = name {
                            println!("[{}] (no files)", n);
                        } else {
                            println!("No files discovered by any processor.");
                        }
                        return Ok(());
                    }

                    // Pre-count per processor for the header
                    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
                    for p in products {
                        *counts.entry(p.processor.as_str()).or_insert(0) += 1;
                    }

                    let project_root = env::current_dir()?;
                    let mut current_processor = "";
                    for product in products {
                        if product.processor.as_str() != current_processor {
                            if !current_processor.is_empty() {
                                println!();
                            }
                            current_processor = product.processor.as_str();
                            let n = counts[current_processor];
                            println!("[{}] ({} {})", current_processor, n, if n == 1 { "product" } else { "products" });
                        }
                        let inputs: Vec<String> = product.inputs.iter()
                            .map(|p| p.strip_prefix(&project_root).unwrap_or(p).display().to_string())
                            .collect();
                        let outputs: Vec<String> = product.outputs.iter()
                            .map(|p| p.strip_prefix(&project_root).unwrap_or(p).display().to_string())
                            .collect();
                        println!("{} \u{2192} {}", inputs.join(", "), outputs.join(", "));
                    }
                }
            }
        }
        Commands::Tools { action } => {
            let builder = Builder::new()?;
            let processors = builder.create_processors(0)?;
            let config = Config::load(&env::current_dir()?)?;

            let show_all = matches!(&action, ToolsAction::List { all: true } | ToolsAction::Check { all: true });

            // Collect (tool_name, processor_name) pairs
            let mut tool_pairs: Vec<(String, String)> = Vec::new();
            let mut names: Vec<&String> = processors.keys().collect();
            names.sort();
            for name in names {
                if !show_all && !config.processor.is_enabled(name) {
                    continue;
                }
                for tool in processors[name].required_tools() {
                    tool_pairs.push((tool, name.clone()));
                }
            }
            tool_pairs.sort();
            tool_pairs.dedup();

            match action {
                ToolsAction::List { .. } => {
                    for (tool, processor) in &tool_pairs {
                        println!("{} ({})", tool, processor);
                    }
                }
                ToolsAction::Check { .. } => {
                    let mut any_missing = false;
                    for (tool, processor) in &tool_pairs {
                        if let Ok(path) = which::which(tool) {
                            println!("{} ({}) {} {}", tool, processor, color::green("found"), color::dim(&path.display().to_string()));
                        } else {
                            println!("{} ({}) {}", tool, processor, color::red("missing"));
                            any_missing = true;
                        }
                    }
                    if any_missing {
                        bail!("Some required tools are missing");
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
        Commands::Watch { jobs, timings, keep_going, no_summary } => {
            watcher::watch(cli.verbose, jobs, timings, keep_going, !no_summary, Arc::clone(&interrupted))?;
        }
        Commands::Version => {
            println!("rsb {}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Config { action } => {
            match action {
                ConfigAction::Show => {
                    let project_root = env::current_dir()?;
                    Config::require_config(&project_root)?;
                    let config = Config::load(&project_root)?;
                    let output = toml::to_string_pretty(&config)?;
                    let annotated = annotate_config(&output);
                    println!("{}", annotated);
                }
                ConfigAction::ShowDefault => {
                    let config = Config::default();
                    let output = toml::to_string_pretty(&config)?;
                    let annotated = annotate_config(&output);
                    println!("{}", annotated);
                }
            }
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

/// Annotate TOML config output with comments for constrained values
fn annotate_config(toml: &str) -> String {
    toml.lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("parallel = ") {
                format!("{} # 0 = auto-detect CPU cores", line)
            } else if trimmed.starts_with("restore_method = ") {
                format!("{} # options: hardlink, copy", line)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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
# auto_detect = true
# enabled = ["template", "ruff", "pylint", "sleep", "cc_single_file", "cpplint", "spellcheck"]

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

[processor.cpplint]
# checker = "cppcheck"
# args = ["--error-exitcode=1", "--enable=warning,style,performance,portability"]
# scan_dir = "src"
# extensions = [".c", ".cc"]

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

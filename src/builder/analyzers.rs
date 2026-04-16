use std::fs;
use std::path::PathBuf;
use anyhow::{Context, Result, bail};
use tabled::builder::Builder as TableBuilder;
use crate::color;
use crate::deps_cache::DepsCache;
use super::{Builder, sorted_keys};

/// List all available dependency analyzers (works without rsconstruct.toml).
pub fn list_analyzers(verbose: bool) {
    use crate::registries as registry;

    let mut plugins: Vec<_> = registry::all_analyzer_plugins().collect();
    plugins.sort_by_key(|p| p.name);

    if crate::json_output::is_json_mode() {
        #[derive(serde::Serialize)]
        struct Entry { name: &'static str, native: bool, description: &'static str }
        let entries: Vec<Entry> = plugins.iter()
            .map(|p| Entry { name: p.name, native: p.is_native, description: p.description })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries).expect("JSON serialize"));
        return;
    }

    let mut builder = TableBuilder::new();
    if verbose {
        builder.push_record(["Name", "Native", "Description"]);
        for plugin in &plugins {
            let native_tag = if plugin.is_native { "native" } else { "external" };
            builder.push_record([plugin.name, native_tag, plugin.description]);
        }
    } else {
        builder.push_record(["Name", "Native"]);
        for plugin in &plugins {
            let native_tag = if plugin.is_native { "native" } else { "external" };
            builder.push_record([plugin.name, native_tag]);
        }
    }
    color::print_table(builder.build());
}

/// Show default analyzer configuration (works without rsconstruct.toml).
pub fn analyzer_defconfig(name: Option<&str>) -> Result<()> {
    use crate::registries as registry;

    let names: Vec<String> = if let Some(name) = name {
        if registry::find_analyzer_plugin(name).is_none() {
            anyhow::bail!("Unknown analyzer '{}'. Run 'rsconstruct analyzers list' to see available analyzers.", name);
        }
        vec![name.to_string()]
    } else {
        registry::all_analyzer_names().iter().map(|s| s.to_string()).collect()
    };

    if crate::json_output::is_json_mode() {
        #[derive(serde::Serialize)]
        struct Entry { name: String, config: serde_json::Value }
        let entries: Vec<Entry> = names.iter().map(|n| {
            let plugin = registry::find_analyzer_plugin(n).expect("checked above");
            let config = match (plugin.defconfig_toml)() {
                Some(toml_str) => toml::from_str::<serde_json::Value>(&toml_str).unwrap_or(serde_json::Value::Null),
                None => serde_json::Value::Null,
            };
            Entry { name: n.clone(), config }
        }).collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    for (i, n) in names.iter().enumerate() {
        if i > 0 { println!(); }
        let plugin = registry::find_analyzer_plugin(n).expect("checked above");
        println!("[analyzer.{}]", n);
        match (plugin.defconfig_toml)() {
            Some(toml_str) => print_config_table(&toml_str)?,
            None => println!("(no configuration options)"),
        }
    }
    Ok(())
}

/// Parse a TOML string and print its fields as a table (Field, Type, Default).
fn print_config_table(toml_str: &str) -> Result<()> {
    let value: toml::Value = toml::from_str(toml_str).context("Failed to parse analyzer defconfig TOML")?;
    let table = value.as_table().context("Analyzer defconfig is not a TOML table")?;

    let mut builder = TableBuilder::new();
    builder.push_record(["Field", "Type", "Default"]);
    for (key, val) in table {
        let type_str = match val {
            toml::Value::String(_)  => "string",
            toml::Value::Integer(_) => "int",
            toml::Value::Float(_)   => "float",
            toml::Value::Boolean(_) => "bool",
            toml::Value::Array(_)   => "string[]",
            toml::Value::Table(_)   => "table",
            toml::Value::Datetime(_) => "datetime",
        };
        let default_str = match val {
            toml::Value::String(s) => format!("\"{}\"", s),
            toml::Value::Array(a) if a.is_empty() => "[]".to_string(),
            _ => val.to_string(),
        };
        builder.push_record([key.as_str(), type_str, &default_str]);
    }
    color::print_table(builder.build());
    Ok(())
}

/// Print per-analyzer dependency stats with a total line.
/// `declared` is the list of analyzer names declared in rsconstruct.toml; any
/// declared analyzer missing from `stats` is shown as a zero row so users can
/// spot silent no-ops.
fn print_deps_stats(
    stats: &std::collections::HashMap<String, (usize, usize)>,
    declared: &[String],
) {
    let mut all: std::collections::BTreeMap<String, (usize, usize)> = std::collections::BTreeMap::new();
    for name in declared {
        all.insert(name.clone(), (0, 0));
    }
    for (name, &v) in stats {
        all.insert(name.clone(), v);
    }

    let mut total_files = 0;
    let mut total_deps = 0;
    let mut builder = TableBuilder::new();
    builder.push_record(["Analyzer", "Files", "Dependencies"]);
    for (name, (files, deps)) in &all {
        total_files += files;
        total_deps += deps;
        builder.push_record([name.clone(), files.to_string(), deps.to_string()]);
    }
    builder.push_record(["Total".to_string(), total_files.to_string(), total_deps.to_string()]);
    color::print_table_with_total(builder.build());
}

impl Builder {
    /// Handle `rsconstruct analyzers` subcommands
    pub fn analyzers(&self, ctx: &crate::build_context::BuildContext, action: crate::cli::AnalyzersAction, verbose: bool) -> Result<()> {
        use crate::cli::AnalyzersAction;

        match action {
            AnalyzersAction::List | AnalyzersAction::Defconfig { .. } | AnalyzersAction::Add { .. }
            | AnalyzersAction::Delete { .. } | AnalyzersAction::Disable { .. } | AnalyzersAction::Enable { .. } => unreachable!("handled in main.rs"),
            AnalyzersAction::Used => {
                let analyzers = self.create_analyzers(false)?;
                let mut builder = TableBuilder::new();
                if verbose {
                    builder.push_record(["Name", "Detected", "Description"]);
                    for name in sorted_keys(&analyzers) {
                        let analyzer = &analyzers[name];
                        let detected = color::yes_no(analyzer.auto_detect(&self.file_index));
                        builder.push_record([name.as_str(), detected, analyzer.description()]);
                    }
                } else {
                    builder.push_record(["Name", "Detected"]);
                    for name in sorted_keys(&analyzers) {
                        let analyzer = &analyzers[name];
                        let detected = color::yes_no(analyzer.auto_detect(&self.file_index));
                        builder.push_record([name.as_str(), detected]);
                    }
                }
                color::print_table(builder.build());
            }
            AnalyzersAction::Build => {
                let processors = self.create_processors()?;
                let mut graph = crate::graph::BuildGraph::new();

                // Phase 1: Discover products (fixed-point loop for cross-processor deps)
                let active: Vec<String> = sorted_keys(&processors).into_iter()
                    .filter(|name| self.is_processor_active(name, processors[*name].as_ref()))
                    .cloned()
                    .collect();
                self.discover_products(&mut graph, &processors, &active, false)?;

                let product_count = graph.products().len();
                if product_count == 0 {
                    println!("No products discovered.");
                    return Ok(());
                }

                // Phase 2: Run dependency analyzers
                self.run_analyzers(ctx, &mut graph, true)?;

                // Show summary from cache
                let deps_cache = DepsCache::open()?;
                let stats = deps_cache.stats_by_analyzer();
                let declared: Vec<String> = self.config.analyzer.instances.iter()
                    .map(|i| i.instance_name.clone()).collect();
                if !stats.is_empty() || !declared.is_empty() {
                    print_deps_stats(&stats, &declared);
                }
            }
            AnalyzersAction::Config { iname } => {
                let instances: Vec<&crate::config::AnalyzerInstance> = if let Some(ref n) = iname {
                    let inst = self.config.analyzer.instances.iter().find(|i| &i.instance_name == n)
                        .ok_or_else(|| anyhow::anyhow!("Analyzer instance '{}' is not declared in rsconstruct.toml", n))?;
                    vec![inst]
                } else {
                    self.config.analyzer.instances.iter().collect()
                };

                if instances.is_empty() {
                    println!("No analyzers declared in rsconstruct.toml. Add `[analyzer.NAME]` sections to enable.");
                } else {
                    for (i, inst) in instances.iter().enumerate() {
                        if i > 0 { println!(); }
                        let toml_str = crate::errors::ctx(toml::to_string_pretty(&inst.config_toml), &format!("Failed to serialize {} analyzer config", inst.instance_name))?;
                        println!("[analyzer.{}]", inst.instance_name);
                        print!("{}", toml_str);
                    }
                }
            }
            AnalyzersAction::Clean { analyzer } => {
                if let Some(analyzer_name) = analyzer {
                    // Clear only entries from specific analyzer
                    let deps_cache = DepsCache::open()?;
                    let removed = crate::errors::ctx(deps_cache.remove_by_analyzer(&analyzer_name), &format!("Failed to remove deps for analyzer '{}'", analyzer_name))?;
                    if removed > 0 {
                        println!("Removed {} entries from '{}' analyzer.", removed, analyzer_name);
                    } else {
                        println!("No entries found for '{}' analyzer.", analyzer_name);
                    }
                } else {
                    // Clear the entire dependency cache
                    let deps_file = PathBuf::from(".rsconstruct/deps.redb");
                    if deps_file.exists() {
                        fs::remove_file(&deps_file)
                            .context("Failed to remove dependency cache")?;
                        println!("Dependency cache cleared.");
                    } else {
                        println!("Dependency cache is already empty.");
                    }
                }
            }
            AnalyzersAction::Stats => {
                // Show statistics by analyzer
                let deps_cache = DepsCache::open()?;
                let stats = deps_cache.stats_by_analyzer();
                let declared: Vec<String> = self.config.analyzer.instances.iter()
                    .map(|i| i.instance_name.clone()).collect();
                if stats.is_empty() && declared.is_empty() {
                    println!("Dependency cache is empty. Run a build first.");
                    return Ok(());
                }
                print_deps_stats(&stats, &declared);
            }
            AnalyzersAction::Show { filter } => {
                use crate::cli::AnalyzersShowFilter;
                let deps_cache = DepsCache::open()?;

                match filter {
                    AnalyzersShowFilter::All => {
                        // List all entries
                        let mut entries: Vec<_> = deps_cache.list_all();
                        if entries.is_empty() {
                            println!("Dependency cache is empty. Run a build first.");
                            return Ok(());
                        }
                        // Sort by source path for consistent output
                        entries.sort_by(|a, b| a.0.cmp(&b.0));
                        for (source, deps, analyzer) in entries {
                            Self::print_deps(&source, &deps, &analyzer);
                        }
                    }
                    AnalyzersShowFilter::Files { files } => {
                        // Query specific files. One path can have multiple
                        // entries — one per analyzer that scanned it.
                        let mut found_any = false;
                        for file_arg in &files {
                            let file_path = PathBuf::from(file_arg);
                            let entries = deps_cache.get_raw_for_path(&file_path);
                            if entries.is_empty() {
                                eprintln!("{}: '{}' not in dependency cache", color::yellow("Warning"), file_arg);
                            } else {
                                found_any = true;
                                for (deps, analyzer) in entries {
                                    Self::print_deps(&file_path, &deps, &analyzer);
                                }
                            }
                        }
                        if !found_any {
                            bail!("No cached dependencies found for the specified files");
                        }
                    }
                    AnalyzersShowFilter::Analyzers { analyzers } => {
                        // Filter by analyzer names
                        let mut entries: Vec<_> = deps_cache.list_by_analyzers(&analyzers);
                        if entries.is_empty() {
                            println!("No cached dependencies found for analyzers: {}", analyzers.join(", "));
                            return Ok(());
                        }
                        // Sort by source path for consistent output
                        entries.sort_by(|a, b| a.0.cmp(&b.0));
                        for (source, deps, analyzer) in entries {
                            Self::print_deps(&source, &deps, &analyzer);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Print dependencies for a source file
    fn print_deps(source: &std::path::Path, deps: &[PathBuf], analyzer: &str) {
        let analyzer_tag = if analyzer.is_empty() {
            String::new()
        } else {
            format!(" {}", color::dim(&format!("[{}]", analyzer)))
        };
        if deps.is_empty() {
            println!("{}:{} {}", source.display(), analyzer_tag, color::dim("(no dependencies)"));
        } else {
            println!("{}:{}", source.display(), analyzer_tag);
            for dep in deps {
                println!("  {}", dep.display());
            }
        }
    }
}

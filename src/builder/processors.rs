use std::collections::HashMap;
use anyhow::{Result, bail};
use crate::cli::ProcessorAction;
use crate::color;
use crate::config::ProcessorConfig;
use super::{Builder, create_builtin_processors, sorted_keys};

/// List all built-in processors (works without rsb.toml).
/// Used when no project config is available.
pub fn list_processors_no_config(all: bool) -> Result<()> {
    let mut cfg = ProcessorConfig::default();
    cfg.resolve_scan_defaults();
    let processors = create_builtin_processors(&cfg);
    let proc_names = sorted_keys(&processors);

    if crate::json_output::is_json_mode() {
        let entries: Vec<crate::json_output::ProcessorListEntry> = proc_names.iter()
            .filter(|name| all || !processors[name.as_str()].hidden())
            .map(|name| {
                let proc = &processors[name.as_str()];
                crate::json_output::ProcessorListEntry {
                    name: name.to_string(),
                    processor_type: proc.processor_type().as_str().to_string(),
                    enabled: false,
                    hidden: proc.hidden(),
                    batch: proc.supports_batch(),
                    description: proc.description().to_string(),
                }
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    for name in &proc_names {
        let proc = &processors[name.as_str()];
        if proc.hidden() && !all {
            continue;
        }
        let hidden_tag = if proc.hidden() {
            format!(" {}", color::dim("(hidden)"))
        } else {
            String::new()
        };
        let type_str = format!("[{}]", proc.processor_type().as_str());
        let proc_type = color::dim(&type_str);
        let batch = if proc.supports_batch() {
            format!(" {}", color::dim("[batch]"))
        } else {
            String::new()
        };
        println!("{} {}{}{} \u{2014} {}", name, proc_type, batch, hidden_tag, color::dim(proc.description()));
    }

    Ok(())
}

impl Builder {
    /// Handle `rsb processor` subcommands
    pub fn processor(&self, action: ProcessorAction) -> Result<()> {
        let processors = self.create_processors()?;

        let proc_names = sorted_keys(&processors);

        match action {
            ProcessorAction::List { all } => {
                if crate::json_output::is_json_mode() {
                    let entries: Vec<crate::json_output::ProcessorListEntry> = proc_names.iter()
                        .filter(|name| all || !processors[name.as_str()].hidden())
                        .map(|name| {
                            let proc = &processors[name.as_str()];
                            crate::json_output::ProcessorListEntry {
                                name: name.to_string(),
                                processor_type: proc.processor_type().as_str().to_string(),
                                enabled: self.config.processor.is_enabled(name),
                                hidden: proc.hidden(),
                                batch: proc.supports_batch(),
                                description: proc.description().to_string(),
                            }
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&entries)?);
                    return Ok(());
                }

                for name in &proc_names {
                    let proc = &processors[name.as_str()];
                    if proc.hidden() && !all {
                        continue;
                    }
                    let enabled_status = if self.config.processor.is_enabled(name) {
                        color::green("enabled")
                    } else {
                        color::dim("disabled")
                    };
                    let hidden_tag = if proc.hidden() {
                        format!(" {}", color::dim("(hidden)"))
                    } else {
                        String::new()
                    };
                    let type_str = format!("[{}]", proc.processor_type().as_str());
                    let proc_type = color::dim(&type_str);
                    let batch = if proc.supports_batch() {
                        format!(" {}", color::dim("[batch]"))
                    } else {
                        String::new()
                    };
                    println!("{} {}{} {}{} \u{2014} {}", name, proc_type, batch, enabled_status, hidden_tag, color::dim(proc.description()));
                }
            }
            ProcessorAction::Auto => {
                for name in &proc_names {
                    let proc = &processors[name.as_str()];
                    let detected = proc.auto_detect(&self.file_index);
                    let enabled = self.config.processor.is_enabled(name);
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
                if let Some(ref n) = name
                    && !processors.contains_key(n.as_str()) {
                        bail!("Unknown processor: '{}'. Run 'rsb processor list' to see available processors.", n);
                    }

                let graph = self.build_graph_filtered(name.as_deref(), all)?;

                let products = graph.products();

                if crate::json_output::is_json_mode() {
                    let entries: Vec<crate::json_output::ProcessorFileEntry> = products.iter()
                        .map(|p| {
                            let proc_type = if p.outputs.is_empty() { "checker" } else { "generator" };
                            crate::json_output::ProcessorFileEntry {
                                processor: p.processor.clone(),
                                processor_type: proc_type.to_string(),
                                inputs: p.inputs.iter().map(|i| i.display().to_string()).collect(),
                                outputs: p.outputs.iter().map(|o| o.display().to_string()).collect(),
                            }
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&entries)?);
                    return Ok(());
                }

                if products.is_empty() {
                    if let Some(ref n) = name {
                        println!("[{}] (no files)", n);
                    } else {
                        println!("No files discovered by any processor.");
                    }
                    return Ok(());
                }

                let mut counts: HashMap<&str, usize> = HashMap::new();
                for p in products {
                    *counts.entry(p.processor.as_str()).or_insert(0) += 1;
                }

                let mut current_processor = "";
                for product in products {
                    if product.processor.as_str() != current_processor {
                        if !current_processor.is_empty() {
                            println!();
                        }
                        current_processor = product.processor.as_str();
                        let n = counts.get(current_processor).copied().unwrap_or(0);
                        println!("[{}] ({} {})", current_processor, n, if n == 1 { "product" } else { "products" });
                    }
                    let inputs: Vec<String> = product.inputs.iter()
                        .map(|p| p.display().to_string())
                        .collect();
                    // For checkers (empty outputs), display "(checker)" instead of output paths
                    if product.outputs.is_empty() {
                        println!("{} \u{2192} {}", inputs.join(", "), color::dim("(checker)"));
                    } else {
                        let outputs: Vec<String> = product.outputs.iter()
                            .map(|p| p.display().to_string())
                            .collect();
                        println!("{} \u{2192} {}", inputs.join(", "), outputs.join(", "));
                    }
                }
            }
        }

        Ok(())
    }
}

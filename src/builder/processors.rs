use std::collections::HashMap;
use anyhow::{Result, bail};
use crate::cli::ProcessorAction;
use crate::color;
use super::{Builder, sorted_keys};

impl Builder {
    /// Handle `rsb processor` subcommands
    pub fn processor(&self, action: ProcessorAction) -> Result<()> {
        let processors = self.create_processors()?;

        let proc_names = sorted_keys(&processors);

        match action {
            ProcessorAction::List { all } => {
                for name in &proc_names {
                    let proc = &processors[name.as_str()];
                    if proc.hidden() && !all {
                        continue;
                    }
                    let status = if self.config.processor.is_enabled(name) {
                        color::green("enabled")
                    } else {
                        color::dim("disabled")
                    };
                    let type_str = format!("[{}]", proc.processor_type().as_str());
                    let proc_type = color::dim(&type_str);
                    let batch = if proc.supports_batch() {
                        format!(" {}", color::dim("[batch]"))
                    } else {
                        String::new()
                    };
                    println!("{} {}{} {}", name, proc_type, batch, status);
                }
            }
            ProcessorAction::All => {
                for name in &proc_names {
                    let proc = &processors[name.as_str()];
                    let enabled_status = if self.config.processor.is_enabled(name) {
                        color::green("enabled")
                    } else {
                        color::dim("disabled")
                    };
                    let hidden_status = if proc.hidden() {
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
                    println!("{} {}{} {}{} \u{2014} {}", name, proc_type, batch, enabled_status, hidden_status, color::dim(proc.description()));
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
                        let n = counts[current_processor];
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

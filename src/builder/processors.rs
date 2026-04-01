use std::collections::HashMap;
use anyhow::{Result, bail};
use crate::cli::ProcessorAction;
use crate::color;
use crate::config::ProcessorConfig;
use super::{Builder, create_all_default_processors, sorted_keys};

/// List all built-in processors (works without rsconstruct.toml).
/// Used when no project config is available.
pub fn list_processors_no_config(all: bool) -> Result<()> {
    let processors = create_all_default_processors();
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
                    detected: false,
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

/// Return a JSON value containing only fields that differ from the default config.
fn config_diff(name: &str, current: &serde_json::Value) -> serde_json::Value {
    let default_json = ProcessorConfig::defconfig_json(name);
    let default_value = default_json
        .and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok());
    let (Some(serde_json::Value::Object(default_obj)), serde_json::Value::Object(current_obj)) =
        (default_value.as_ref(), current)
    else {
        return current.clone();
    };
    let mut diff = serde_json::Map::new();
    for (key, val) in current_obj {
        match default_obj.get(key) {
            Some(def_val) if def_val == val => {}
            _ => { diff.insert(key.clone(), val.clone()); }
        }
    }
    serde_json::Value::Object(diff)
}

/// Show default configuration for a processor (works without rsconstruct.toml).
pub fn processor_defconfig(name: &str) -> Result<()> {
    match ProcessorConfig::defconfig_json(name) {
        Some(json) => {
            println!("{}", json);
            Ok(())
        }
        None => bail!("Unknown processor: '{}'. Run 'rsconstruct processors list' to see available processors.", name),
    }
}

impl Builder {
    /// Handle `rsconstruct processor` subcommands
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
                                enabled: true,
                                detected: true,
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
            }
            ProcessorAction::Config { ref name, diff } => {
                let names: Vec<&str> = if let Some(n) = name {
                    if !processors.contains_key(n.as_str()) {
                        bail!("Unknown processor: '{}'. Run 'rsconstruct processors list' to see available processors.", n);
                    }
                    vec![n.as_str()]
                } else {
                    proc_names.iter()
                        .map(|s| s.as_str())
                        .collect()
                };

                if crate::json_output::is_json_mode() {
                    let mut map = serde_json::Map::new();
                    for n in &names {
                        let proc = &processors[*n];
                        if let Some(json) = proc.config_json() {
                            let value: serde_json::Value = serde_json::from_str(&json)?;
                            let value = if diff {
                                config_diff(n, &value)
                            } else {
                                value
                            };
                            map.insert(n.to_string(), value);
                        }
                    }
                    println!("{}", serde_json::to_string_pretty(&serde_json::Value::Object(map))?);
                    return Ok(());
                }

                for (i, n) in names.iter().enumerate() {
                    let proc = &processors[*n];
                    if let Some(json) = proc.config_json() {
                        let value: serde_json::Value = serde_json::from_str(&json)?;
                        let value = if diff {
                            config_diff(n, &value)
                        } else {
                            value
                        };
                        if names.len() > 1 {
                            println!("{}:", n);
                        }
                        println!("{}", serde_json::to_string_pretty(&value)?);
                        if i + 1 < names.len() {
                            println!();
                        }
                    } else if name.is_some() {
                        println!("Processor '{}' does not expose configuration.", n);
                    }
                }
            }
            ProcessorAction::Defconfig { ref name } => {
                processor_defconfig(name)?;
            }
            ProcessorAction::Allowlist => {
                let enabled: Vec<&str> = proc_names.iter()
                    .map(|s| s.as_str())
                    .collect();
                if crate::json_output::is_json_mode() {
                    println!("{}", serde_json::to_string_pretty(&enabled)?);
                } else {
                    println!("enabled = [{}]", enabled.iter()
                        .map(|n| format!("\"{}\"", n))
                        .collect::<Vec<_>>()
                        .join(", "));
                }
            }
            ProcessorAction::Graph { format } => {
                let graph = self.build_graph()?;
                let proc_deps = graph.processor_dependencies();
                match format {
                    crate::cli::GraphFormat::Text => {
                        for (proc, deps) in &proc_deps {
                            if deps.is_empty() {
                                println!("{}", proc);
                            } else {
                                println!("{} \u{2192} {}", proc, deps.iter().cloned().collect::<Vec<_>>().join(", "));
                            }
                        }
                    }
                    crate::cli::GraphFormat::Dot => {
                        println!("digraph processors {{");
                        println!("    rankdir=LR;");
                        println!("    node [fontname=\"sans-serif\" shape=box style=filled fillcolor=lightyellow];");
                        for (proc, deps) in &proc_deps {
                            for dep in deps {
                                println!("    \"{}\" -> \"{}\";", proc, dep);
                            }
                        }
                        println!("}}");
                    }
                    crate::cli::GraphFormat::Mermaid => {
                        println!("graph LR");
                        for (proc, deps) in &proc_deps {
                            for dep in deps {
                                println!("    {} --> {}", proc, dep);
                            }
                        }
                    }
                    crate::cli::GraphFormat::Json => {
                        println!("{}", serde_json::to_string_pretty(&proc_deps)?);
                    }
                    crate::cli::GraphFormat::Svg => {
                        let mut dot = String::from("digraph processors {\n    rankdir=LR;\n    node [fontname=\"sans-serif\" shape=box style=filled fillcolor=lightyellow];\n");
                        for (proc, deps) in &proc_deps {
                            for dep in deps {
                                dot.push_str(&format!("    \"{}\" -> \"{}\";\n", proc, dep));
                            }
                        }
                        dot.push_str("}\n");
                        let svg = crate::processors::dot_to_svg(&dot)?;
                        println!("{}", svg);
                    }
                }
            }
            ProcessorAction::Files { name, all } => {
                if let Some(ref n) = name
                    && !processors.contains_key(n.as_str()) {
                        bail!("Unknown processor: '{}'. Run 'rsconstruct processors list' to see available processors.", n);
                    }

                let graph = self.build_graph_filtered(name.as_deref(), all)?;

                let products = graph.products();

                if crate::json_output::is_json_mode() {
                    let entries: Vec<crate::json_output::ProcessorFileEntry> = products.iter()
                        .map(|p| {
                            let proc_type = processors.get(p.processor.as_str())
                                .map(|proc| proc.processor_type().as_str())
                                .unwrap_or("unknown");
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
                    let proc_type = processors.get(product.processor.as_str())
                        .map(|proc| proc.processor_type());
                    if product.outputs.is_empty() {
                        let label = match proc_type {
                            Some(crate::processors::ProcessorType::MassGenerator) => "(mass_generator)",
                            _ => "(checker)",
                        };
                        println!("{} \u{2192} {}", inputs.join(", "), color::dim(label));
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

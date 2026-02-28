use std::collections::HashMap;
use anyhow::{Result, bail};
use crate::cli::ProcessorAction;
use crate::color;
use crate::config::{
    ProcessorConfig,
    TeraConfig, MakoConfig, RuffConfig, PylintConfig, CcSingleFileConfig, CppcheckConfig, ClangTidyConfig,
    ShellcheckConfig, SpellcheckConfig, SleepConfig, MakeConfig, CargoConfig, ClippyConfig,
    RumdlConfig, MypyConfig, PyreflyConfig, YamllintConfig, JqConfig, JsonlintConfig, TaploConfig,
    JsonSchemaConfig, TagsConfig, PipConfig, SphinxConfig, NpmConfig, GemConfig, MdlConfig,
    MarkdownlintConfig, AspellConfig, PandocConfig, MarkdownConfig, PdflatexConfig,
    A2xConfig, AsciiCheckConfig,
};
use crate::processors::names;
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

/// Return the default config for a processor as pretty JSON, or None if the name is unknown.
fn defconfig_json(name: &str) -> Option<String> {
    let json: serde_json::Value = match name {
        names::TERA => serde_json::to_value(TeraConfig::default()).ok()?,
        names::MAKO => serde_json::to_value(MakoConfig::default()).ok()?,
        names::RUFF => serde_json::to_value(RuffConfig::default()).ok()?,
        names::PYLINT => serde_json::to_value(PylintConfig::default()).ok()?,
        names::CC_SINGLE_FILE => serde_json::to_value(CcSingleFileConfig::default()).ok()?,
        names::CPPCHECK => serde_json::to_value(CppcheckConfig::default()).ok()?,
        names::CLANG_TIDY => serde_json::to_value(ClangTidyConfig::default()).ok()?,
        names::SHELLCHECK => serde_json::to_value(ShellcheckConfig::default()).ok()?,
        names::SPELLCHECK => serde_json::to_value(SpellcheckConfig::default()).ok()?,
        names::SLEEP => serde_json::to_value(SleepConfig::default()).ok()?,
        names::MAKE => serde_json::to_value(MakeConfig::default()).ok()?,
        names::CARGO => serde_json::to_value(CargoConfig::default()).ok()?,
        names::CLIPPY => serde_json::to_value(ClippyConfig::default()).ok()?,
        names::RUMDL => serde_json::to_value(RumdlConfig::default()).ok()?,
        names::MYPY => serde_json::to_value(MypyConfig::default()).ok()?,
        names::PYREFLY => serde_json::to_value(PyreflyConfig::default()).ok()?,
        names::YAMLLINT => serde_json::to_value(YamllintConfig::default()).ok()?,
        names::JQ => serde_json::to_value(JqConfig::default()).ok()?,
        names::JSONLINT => serde_json::to_value(JsonlintConfig::default()).ok()?,
        names::TAPLO => serde_json::to_value(TaploConfig::default()).ok()?,
        names::JSON_SCHEMA => serde_json::to_value(JsonSchemaConfig::default()).ok()?,
        names::TAGS => serde_json::to_value(TagsConfig::default()).ok()?,
        names::PIP => serde_json::to_value(PipConfig::default()).ok()?,
        names::SPHINX => serde_json::to_value(SphinxConfig::default()).ok()?,
        names::NPM => serde_json::to_value(NpmConfig::default()).ok()?,
        names::GEM => serde_json::to_value(GemConfig::default()).ok()?,
        names::MDL => serde_json::to_value(MdlConfig::default()).ok()?,
        names::MARKDOWNLINT => serde_json::to_value(MarkdownlintConfig::default()).ok()?,
        names::ASPELL => serde_json::to_value(AspellConfig::default()).ok()?,
        names::PANDOC => serde_json::to_value(PandocConfig::default()).ok()?,
        names::MARKDOWN => serde_json::to_value(MarkdownConfig::default()).ok()?,
        names::PDFLATEX => serde_json::to_value(PdflatexConfig::default()).ok()?,
        names::A2X => serde_json::to_value(A2xConfig::default()).ok()?,
        names::ASCII_CHECK => serde_json::to_value(AsciiCheckConfig::default()).ok()?,
        _ => return None,
    };
    serde_json::to_string_pretty(&json).ok()
}

/// Return a JSON value containing only fields that differ from the default config.
fn config_diff(name: &str, current: &serde_json::Value) -> serde_json::Value {
    let default_json = defconfig_json(name);
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

/// Show default configuration for a processor (works without rsb.toml).
pub fn processor_defconfig(name: &str) -> Result<()> {
    match defconfig_json(name) {
        Some(json) => {
            println!("{}", json);
            Ok(())
        }
        None => bail!("Unknown processor: '{}'. Run 'rsb processors list' to see available processors.", name),
    }
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
                                detected: proc.auto_detect(&self.file_index),
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
                    let enabled = self.config.processor.is_enabled(name);
                    let detected = proc.auto_detect(&self.file_index);
                    let status = match (enabled, detected) {
                        (true, true) => color::green("enabled, detected"),
                        (true, false) => color::yellow("enabled, not detected"),
                        (false, true) => color::yellow("disabled, detected"),
                        (false, false) => color::dim("disabled"),
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
                    println!("{} {}{} {}{} \u{2014} {}", name, proc_type, batch, status, hidden_tag, color::dim(proc.description()));
                }
            }
            ProcessorAction::Config { ref name, diff } => {
                let names: Vec<&str> = if let Some(n) = name {
                    if !processors.contains_key(n.as_str()) {
                        bail!("Unknown processor: '{}'. Run 'rsb processors list' to see available processors.", n);
                    }
                    vec![n.as_str()]
                } else {
                    proc_names.iter()
                        .map(|s| s.as_str())
                        .filter(|n| self.config.processor.is_enabled(n))
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
                    .filter(|n| self.config.processor.is_enabled(n))
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

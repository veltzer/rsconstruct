use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tera::{Context as TeraContext, Function, Tera, Value as TeraValue, to_value};

use crate::config::TemplateConfig;
use crate::graph::{BuildGraph, Product};
use crate::ignore::IgnoreRules;
use super::ProductDiscovery;

/// Represents a single template file to be processed
struct TemplateItem {
    /// Path to the .tera template file
    source_path: PathBuf,
    /// Path where the rendered output will be written
    output_path: PathBuf,
}

impl TemplateItem {
    fn new(source_path: PathBuf, output_path: PathBuf) -> Self {
        Self {
            source_path,
            output_path,
        }
    }

    /// Render the template and write to output
    fn render(&self, config: &TemplateConfig) -> Result<()> {
        // Read template content
        let template_content = fs::read_to_string(&self.source_path)?;

        // Optionally trim blocks (remove first newline after block tags)
        let template_content = if config.trim_blocks {
            trim_block_newlines(&template_content)
        } else {
            template_content
        };

        // Create a new Tera instance for this template
        let mut tera = Tera::default();

        // Register the load_python function
        tera.register_function("load_python", LoadPythonFunction);

        // Add the template
        tera.add_raw_template("template", &template_content)
            .context("Failed to parse template")?;

        // Configure strict mode (fail on undefined variables)
        tera.set_escape_fn(|s| s.to_string()); // No HTML escaping by default

        // Create an empty context (load_python will be called from within the template)
        let context = TeraContext::new();

        // Render the template
        let rendered = tera
            .render("template", &context)
            .with_context(|| {
                if config.strict {
                    format!("Failed to render template (strict mode enabled): {}", self.source_path.display())
                } else {
                    format!("Failed to render template: {}", self.source_path.display())
                }
            })?;

        // Write to output file
        fs::write(&self.output_path, rendered)?;

        Ok(())
    }
}

/// Remove first newline after block tags ({% ... %})
fn trim_block_newlines(content: &str) -> String {
    // Simple implementation: remove newline immediately after %}
    content.replace("%}\n", "%}")
}

pub struct TemplateProcessor {
    templates_dir: PathBuf,
    output_dir: PathBuf,
    config: TemplateConfig,
    ignore_rules: Arc<IgnoreRules>,
}

impl TemplateProcessor {
    pub fn new(templates_dir: PathBuf, output_dir: PathBuf, config: TemplateConfig, ignore_rules: Arc<IgnoreRules>) -> Result<Self> {
        Ok(Self {
            templates_dir,
            output_dir,
            config,
            ignore_rules,
        })
    }

    /// Find all template files matching configured extensions
    fn find_templates(&self) -> Result<Vec<TemplateItem>> {
        let mut items = Vec::new();

        if !self.templates_dir.exists() {
            return Ok(items);
        }

        for entry in fs::read_dir(&self.templates_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if self.ignore_rules.is_ignored(&path) {
                    continue;
                }

                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                // Check if file matches any configured extension
                for ext in &self.config.extensions {
                    if filename.ends_with(ext) {
                        // Get the output filename (remove the extension)
                        let output_name = &filename[..filename.len() - ext.len()];
                        if !output_name.is_empty() {
                            let output_path = self.output_dir.join(output_name);
                            items.push(TemplateItem::new(path.clone(), output_path));
                            break;
                        }
                    }
                }
            }
        }

        items.sort_by(|a, b| a.source_path.cmp(&b.source_path));
        Ok(items)
    }
}

impl ProductDiscovery for TemplateProcessor {
    fn discover(&self, graph: &mut BuildGraph) -> Result<()> {
        let items = self.find_templates()?;

        for item in items {
            graph.add_product(
                vec![item.source_path.clone()],
                vec![item.output_path.clone()],
                "template",
            );
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        if product.inputs.len() != 1 || product.outputs.len() != 1 {
            anyhow::bail!("Template product must have exactly one input and one output");
        }

        let item = TemplateItem::new(
            product.inputs[0].clone(),
            product.outputs[0].clone(),
        );
        item.render(&self.config)
    }

    fn clean(&self, product: &Product) -> Result<()> {
        for output in &product.outputs {
            if output.exists() && output.is_file() {
                fs::remove_file(output)?;
                println!("Removed generated file: {}", output.display());
            }
        }
        Ok(())
    }
}

/// Custom Tera function to load Python configuration files
struct LoadPythonFunction;

impl Function for LoadPythonFunction {
    fn call(&self, args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        // Get the path argument
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| tera::Error::msg("load_python requires a 'path' argument"))?;

        // Execute Python and load the config
        let result = load_python_config(Path::new(path))
            .map_err(|e| tera::Error::msg(format!("Failed to load Python config: {}", e)))?;

        Ok(to_value(result).unwrap_or(TeraValue::Null))
    }
}

/// Load configuration from a Python file
fn load_python_config(python_file: &Path) -> Result<Map<String, Value>> {
    // Resolve the path relative to current working directory
    let absolute_path = if python_file.is_absolute() {
        python_file.to_path_buf()
    } else {
        std::env::current_dir()?.join(python_file)
    };

    if !absolute_path.exists() {
        anyhow::bail!("Python config file not found: {}", absolute_path.display());
    }

    // Create a Python script that will execute the config file and output variables as JSON
    let python_script = format!(
        r#"
import sys
import json
import os

# Set the working directory to the config file's directory
config_dir = r'{}'
if config_dir:
    sys.path.insert(0, config_dir)

# Create a namespace for execution
namespace = {{}}

# Execute the config file
with open(r'{}', 'r') as f:
    exec(f.read(), namespace)

# Filter out built-in variables and convert to JSON-serializable format
result = {{}}
for key, value in namespace.items():
    if not key.startswith('__'):
        try:
            # Try to serialize the value
            json.dumps(value)
            result[key] = value
        except:
            # If not serializable, convert to string
            result[key] = str(value)

print(json.dumps(result))
"#,
        absolute_path.parent().unwrap_or(Path::new(".")).display(),
        absolute_path.display()
    );

    // Execute Python and capture output
    let output = Command::new("python3")
        .arg("-c")
        .arg(&python_script)
        .output()
        .context("Failed to execute Python for config loading")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Python config execution failed: {}", stderr);
    }

    // Parse the JSON output
    let stdout = String::from_utf8_lossy(&output.stdout);
    let variables: Map<String, Value> =
        serde_json::from_str(&stdout).context("Failed to parse Python config output")?;

    Ok(variables)
}

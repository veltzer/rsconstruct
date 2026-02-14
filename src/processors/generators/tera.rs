use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tera::{Context as TeraContext, Function, Tera, Value as TeraValue, to_value};

use crate::config::{TeraConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, clean_outputs, run_command_capture};

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
    fn render(&self, config: &TeraConfig) -> Result<()> {
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

pub struct TeraProcessor {
    config: TeraConfig,
}

impl TeraProcessor {
    pub fn new(config: TeraConfig) -> Result<Self> {
        Ok(Self {
            config,
        })
    }

    /// Find all template files matching configured extensions
    fn find_templates(&self, file_index: &FileIndex) -> Result<Vec<TemplateItem>> {
        let paths = file_index.scan(&self.config.scan, false);
        let extensions = self.config.scan.extensions();

        let mut items = Vec::new();
        for path in paths {
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            for ext in extensions {
                if filename.ends_with(ext.as_str()) {
                    let output_name = &filename[..filename.len() - ext.len()];
                    if !output_name.is_empty() {
                        // Output is at project root with the .tera extension stripped
                        let output_path = PathBuf::from(output_name);
                        items.push(TemplateItem::new(path.clone(), output_path));
                        break;
                    }
                }
            }
        }

        Ok(items)
    }
}

impl ProductDiscovery for TeraProcessor {
    fn description(&self) -> &str {
        "Render Tera templates into output files"
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        crate::processors::ProcessorType::Generator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        self.find_templates(file_index).is_ok_and(|t| !t.is_empty())
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["python3".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let items = self.find_templates(file_index)?;
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        for item in items {
            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(item.source_path.clone());
            inputs.extend_from_slice(&extra);
            graph.add_product(
                inputs,
                vec![item.output_path.clone()],
                crate::processors::names::TERA,
                Some(config_hash(&self.config)),
            )?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let item = TemplateItem::new(
            product.primary_input().to_path_buf(),
            product.outputs.first().expect(crate::errors::EMPTY_PRODUCT_OUTPUTS).clone(),
        );
        item.render(&self.config)
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::TERA, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
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

        to_value(result).map_err(|e| tera::Error::msg(format!("Failed to convert Python config to template value: {e}")))
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

    // Create a Python script that will execute the config file and output variables as JSON.
    // Escape backslashes and single quotes for safe embedding in Python string literals.
    let config_dir = absolute_path.parent().unwrap_or(Path::new(".")).display().to_string()
        .replace('\\', "\\\\").replace('\'', "\\'");
    let config_path = absolute_path.display().to_string()
        .replace('\\', "\\\\").replace('\'', "\\'");
    let python_script = format!(
        r#"
import sys
import json
import os

# Set the working directory to the config file's directory
config_dir = '{}'
if config_dir:
    sys.path.insert(0, config_dir)

# Create a namespace for execution
namespace = {{}}

# Execute the config file
with open('{}', 'r') as f:
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
        config_dir,
        config_path
    );

    // Execute Python and capture output
    let mut cmd = Command::new("python3");
    cmd.arg("-c").arg(&python_script);
    let output = run_command_capture(&mut cmd)?;

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

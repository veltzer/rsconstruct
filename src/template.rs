use anyhow::{Result, Context};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tera::{Tera, Context as TeraContext, Function, Value as TeraValue, to_value};
use serde_json::{Value, Map};
use crate::checksum::ChecksumCache;

pub struct TemplateProcessor {
    templates_dir: PathBuf,
    output_dir: PathBuf,
}

impl TemplateProcessor {
    pub fn new(templates_dir: PathBuf, output_dir: PathBuf) -> Result<Self> {
        Ok(Self {
            templates_dir,
            output_dir,
        })
    }

    /// Process all .tera template files in the templates directory
    pub fn process_all(&mut self, checksum_cache: &mut ChecksumCache, force: bool) -> Result<()> {
        if !self.templates_dir.exists() {
            println!("No templates directory found at: {}", self.templates_dir.display());
            return Ok(());
        }

        // Create output directory if it doesn't exist
        if !self.output_dir.exists() {
            fs::create_dir_all(&self.output_dir)?;
        }

        // Find all .tera files in the templates directory
        for entry in fs::read_dir(&self.templates_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("tera") {
                // Check if template has changed
                let changed = force || checksum_cache.has_changed(&path)?;

                if changed {
                    self.process_single_template(&path)?;
                    println!("Processed template: {}", path.display());
                } else {
                    println!("Skipping unchanged template: {}", path.display());
                }
            }
        }

        Ok(())
    }

    /// Process a single .tera template file
    fn process_single_template(&mut self, template_path: &Path) -> Result<()> {
        // Get the output filename (remove .tera extension)
        let output_name = template_path
            .file_stem()
            .and_then(|n| n.to_str())
            .context("Invalid template filename")?;

        // Read template content
        let template_content = fs::read_to_string(template_path)?;

        // Create a new Tera instance for this template
        let mut tera = Tera::default();

        // Register the load_python function
        tera.register_function("load_python", LoadPythonFunction);

        // Add the template
        tera.add_raw_template("template", &template_content)
            .context("Failed to parse template")?;

        // Create an empty context (load_python will be called from within the template)
        let context = TeraContext::new();

        // Render the template
        let rendered = tera.render("template", &context)
            .context("Failed to render template")?;

        // Write to output file
        let output_path = self.output_dir.join(output_name);
        fs::write(&output_path, rendered)?;
        println!("Generated: {}", output_path.display());

        Ok(())
    }
}

/// Custom Tera function to load Python configuration files
struct LoadPythonFunction;

impl Function for LoadPythonFunction {
    fn call(&self, args: &HashMap<String, TeraValue>) -> tera::Result<TeraValue> {
        // Get the path argument
        let path = args.get("path")
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
    let variables: Map<String, Value> = serde_json::from_str(&stdout)
        .context("Failed to parse Python config output")?;

    Ok(variables)
}
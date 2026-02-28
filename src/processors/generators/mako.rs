use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::config::{MakoConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, clean_outputs, run_command, check_command_output};

/// Represents a single Mako template file to be processed
struct TemplateItem {
    /// Path to the .mako template file
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

    /// Render the template via python3 and write to output
    fn render(&self) -> Result<()> {
        // Ensure parent directory of output exists
        if let Some(parent) = self.output_path.parent()
            && !parent.as_os_str().is_empty() && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }

        let source = self.source_path.display().to_string()
            .replace('\\', "\\\\").replace('\'', "\\'");
        let target = self.output_path.display().to_string()
            .replace('\\', "\\\\").replace('\'', "\\'");

        let python_script = format!(
            r#"
import mako.template, mako.lookup
lookup = mako.lookup.TemplateLookup(directories=['.'])
t = mako.template.Template(filename='{}', lookup=lookup)
output = t.render()
with open('{}', 'w') as f:
    f.write(output)
"#,
            source, target
        );

        let mut cmd = Command::new("python3");
        cmd.arg("-c").arg(&python_script);
        let output = run_command(&mut cmd)?;
        check_command_output(&output, format!("mako render {}", self.source_path.display()))
    }
}

pub struct MakoProcessor {
    config: MakoConfig,
}

impl MakoProcessor {
    pub fn new(config: MakoConfig) -> Result<Self> {
        Ok(Self { config })
    }

    /// Find all template files matching configured extensions
    fn find_templates(&self, file_index: &FileIndex) -> Result<Vec<TemplateItem>> {
        let paths = file_index.scan(&self.config.scan, true);
        let extensions = self.config.scan.extensions();
        let scan_dir = self.config.scan.scan_dir();

        let mut items = Vec::new();
        for path in paths {
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            for ext in extensions {
                if filename.ends_with(ext.as_str()) {
                    let output_name = &filename[..filename.len() - ext.len()];
                    if !output_name.is_empty() {
                        // Strip the scan_dir prefix to get the output path
                        let output_path = if !scan_dir.is_empty() {
                            if let Ok(relative) = path.strip_prefix(scan_dir) {
                                relative.with_file_name(output_name)
                            } else {
                                PathBuf::from(output_name)
                            }
                        } else {
                            PathBuf::from(output_name)
                        };
                        items.push(TemplateItem::new(path.clone(), output_path));
                        break;
                    }
                }
            }
        }

        Ok(items)
    }
}

impl ProductDiscovery for MakoProcessor {
    fn description(&self) -> &str {
        "Render Mako templates into output files"
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
                crate::processors::names::MAKO,
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
        item.render()
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::MAKO, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}

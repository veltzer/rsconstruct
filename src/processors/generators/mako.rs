use anyhow::Result;
use std::process::Command;

use crate::config::{MakoConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, clean_outputs, run_command, check_command_output};

use super::TemplateItem;

/// Render a Mako template via python3 and write to output
fn render_mako(item: &TemplateItem) -> Result<()> {
    // Ensure parent directory of output exists
    crate::processors::ensure_output_dir(&item.output_path)?;

    let source = item.source_path.display().to_string()
        .replace('\\', "\\\\").replace('\'', "\\'");
    let target = item.output_path.display().to_string()
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
    check_command_output(&output, format!("mako render {}", item.source_path.display()))
}

pub struct MakoProcessor {
    config: MakoConfig,
}

impl MakoProcessor {
    pub fn new(config: MakoConfig) -> Self {
        Self { config }
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
        super::find_templates(&self.config.scan, file_index).is_ok_and(|t| !t.is_empty())
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["python3".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let items = super::find_templates(&self.config.scan, file_index)?;
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        for item in items {
            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(item.source_path.clone());
            inputs.extend_from_slice(&extra);
            graph.add_product(
                inputs,
                vec![item.output_path.clone()],
                crate::processors::names::MAKO,
                Some(output_config_hash(&self.config, &[])),
            )?;
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let item = TemplateItem::new(
            product.primary_input().to_path_buf(),
            product.primary_output().to_path_buf(),
        );
        render_mako(&item)
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::MAKO, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}

use anyhow::Result;
use std::process::Command;

use crate::config::{Jinja2Config, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, clean_outputs, run_command, check_command_output};

use super::TemplateItem;

/// Render a Jinja2 template via python3 and write to output
fn render_jinja2(item: &TemplateItem) -> Result<()> {
    crate::processors::ensure_output_dir(&item.output_path)?;

    let source = item.source_path.display().to_string()
        .replace('\\', "\\\\").replace('\'', "\\'");
    let target = item.output_path.display().to_string()
        .replace('\\', "\\\\").replace('\'', "\\'");

    let python_script = format!(
        r#"
import jinja2, os
loader = jinja2.FileSystemLoader('.')
env = jinja2.Environment(loader=loader)
template = env.get_template('{}')
output = template.render(**os.environ)
with open('{}', 'w') as f:
    f.write(output)
"#,
        source, target
    );

    let mut cmd = Command::new("python3");
    cmd.arg("-c").arg(&python_script);
    let output = run_command(&mut cmd)?;
    check_command_output(&output, format!("jinja2 render {}", item.source_path.display()))
}

pub struct Jinja2Processor {
    config: Jinja2Config,
}

impl Jinja2Processor {
    pub fn new(config: Jinja2Config) -> Self {
        Self { config }
    }
}

impl ProductDiscovery for Jinja2Processor {
    fn description(&self) -> &str {
        "Render Jinja2 templates into output files"
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
                crate::processors::names::JINJA2,
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
        render_jinja2(&item)
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::JINJA2, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}

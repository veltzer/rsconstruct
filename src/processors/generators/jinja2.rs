use anyhow::Result;
use std::process::Command;

use crate::config::{Jinja2Config, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, run_command, check_command_output};

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
    base: ProcessorBase,
    config: Jinja2Config,
}

impl Jinja2Processor {
    pub fn new(config: Jinja2Config) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::JINJA2,
                "Render Jinja2 templates into output files",
            ),
            config,
        }
    }
}

impl Processor for Jinja2Processor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }


    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        self.base.processor_type()
    }

    fn config_json(&self) -> Option<String> {
        crate::processors::ProcessorBase::config_json(&self.config)
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.standard.max_jobs
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processors::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        super::find_templates(&self.config.standard.scan, file_index).is_ok_and(|t| !t.is_empty())
    }

    fn required_tools(&self) -> Vec<String> {
        vec!["python3".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let items = super::find_templates(&self.config.standard.scan, file_index)?;
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        for item in items {
            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(item.source_path.clone());
            inputs.extend_from_slice(&extra);
            graph.add_product(
                inputs,
                vec![item.output_path.clone()],
                instance_name,
                Some(output_config_hash(&self.config, &[])),
            )?;
        }

        Ok(())
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        let item = TemplateItem::new(
            product.primary_input().to_path_buf(),
            product.primary_output().to_path_buf(),
        );
        render_jinja2(&item)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(Jinja2Processor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "jinja2",
        processor_type: crate::processors::ProcessorType::Generator,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::Jinja2Config>,
        known_fields: crate::registry::typed_known_fields::<crate::config::Jinja2Config>,
        output_fields: crate::registry::typed_output_fields::<crate::config::Jinja2Config>,
        must_fields: crate::registry::typed_must_fields::<crate::config::Jinja2Config>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::Jinja2Config>,
    }
}

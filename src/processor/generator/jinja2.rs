use anyhow::Result;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::config::{StandardConfig, output_config_hash};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{Processor, check_command_output, resolve_dep_inputs, run_command};

use super::TemplateItem;

/// Jinja2 template processor config. No custom fields.
/// `command` is the Python interpreter used to render (default: python3).
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Jinja2Config {
    #[serde(flatten)]
    pub standard: StandardConfig,
}

/// Render a Jinja2 template via the configured Python interpreter and write to output
fn render_jinja2(
    ctx: &crate::build_context::BuildContext,
    python: &str,
    item: &TemplateItem,
) -> Result<()> {
    crate::processor::ensure_output_dir(&item.output_path)?;

    let source = item
        .source_path
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('\'', "\\'");
    let target = item
        .output_path
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('\'', "\\'");

    // The context is empty: the environment variables used to be passed in
    // (`**os.environ`), which made the output depend on whatever the shell
    // happened to export, untracked by the cache. A value a template needs
    // comes from a file it includes (tracked through `dep_inputs`).
    let python_script = format!(
        r"
import jinja2
loader = jinja2.FileSystemLoader('.')
env = jinja2.Environment(loader=loader)
template = env.get_template('{source}')
output = template.render()
with open('{target}', 'w') as f:
    f.write(output)
"
    );

    let mut cmd = Command::new(python);
    cmd.arg("-c").arg(&python_script);
    let output = run_command(ctx, &cmd)?;
    check_command_output(
        &output,
        format!("jinja2 render {}", item.source_path.display()),
    )
}

pub struct Jinja2Processor {
    config: Jinja2Config,
}

impl Jinja2Processor {
    pub const fn new(config: Jinja2Config) -> Self {
        Self { config }
    }
}

impl Processor for Jinja2Processor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    /// One product per scanned template (`find_templates`).
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processor::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !super::find_templates(&self.config.standard, file_index).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.standard.command.clone()]
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let items = super::find_templates(&self.config.standard, file_index);
        let extra = resolve_dep_inputs(
            &self.config.standard.dep_inputs,
            &self.config.standard.dep_auto,
        )?;

        for item in items {
            let mut inputs = Vec::with_capacity(1 + extra.len());
            inputs.push(item.source_path.clone());
            inputs.extend_from_slice(&extra);
            graph.add_product(
                inputs,
                vec![item.output_path.clone()],
                instance_name,
                Some(output_config_hash(
                    &self.config,
                    &crate::config::checksum_fields_of(instance_name),
                )),
            )?;
        }

        Ok(())
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let item = TemplateItem::new(
            product.primary_input().to_path_buf(),
            product.primary_output().to_path_buf(),
        );
        let python = self.config.standard.require_command("jinja2")?;
        render_jinja2(ctx, python, &item)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(Jinja2Processor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "jinja2",
        processor_type: crate::processor::ProcessorType::Generator,
        create: plugin_create,
        fields: &[],
        omit_standard_fields: &["formats", "args", "output_dir", "required_tools", "batch"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".j2"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "python3", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<Jinja2Config>,
        keywords: &["python", "template", "generator", "jinja", "pip"],
        description: "Render Jinja2 templates into output files",
        is_builtin: false,
        is_rust: false,
        supports_batch: false,
        caches_outputs: true,
        max_jobs_cap: None,
    }
}

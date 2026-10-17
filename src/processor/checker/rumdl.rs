//! rumdl checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_rumdl(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint Markdown files using rumdl",
                subcommand: Some("check"),
                prepend_args: &[],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! {
    crate::registries::RustAlternative { replaces: "processor.checker.markdownlint", by: "processor.checker.rumdl" }
}
inventory::submit! {
    crate::registries::RustAlternative { replaces: "processor.checker.mdl", by: "processor.checker.rumdl" }
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "rumdl", processor_type: crate::processor::ProcessorType::Checker, create: create_rumdl,
    fields: &[],
    omit_standard_fields: &[],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "rumdl", dep_auto: &[".rumdl.toml"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["markdown", "md", "linter", "rust"],
    description: "Lint Markdown files using rumdl",
    is_native: false,
    is_rust: true,
    supports_batch: true,
    max_jobs_cap: None,
} }

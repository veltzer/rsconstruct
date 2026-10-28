//! jq checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_jq(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Validate JSON files using jq",
                subcommand: None,
                prepend_args: &["empty"],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "jq", processor_type: crate::processor::ProcessorType::Checker, create: create_jq,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".json"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "jq", ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["json", "checker", "jq", "validator"],
    description: "Validate JSON files using jq",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "jq", runtime: "system",
    project: Some(crate::tools::InstallMethod::new("binary", "jq")),
    machine: Some(crate::tools::InstallMethod::new("apt", "jq")),
    alternatives: &[], prefer: None,
} }

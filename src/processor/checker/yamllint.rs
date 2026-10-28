//! yamllint checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_yamllint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint YAML files using yamllint",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &["python3"],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "yamllint", processor_type: crate::processor::ProcessorType::Checker, create: create_yamllint,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".yml", ".yaml"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "yamllint", dep_auto: &[".yamllint", ".yamllint.yml", ".yamllint.yaml"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["yaml", "yml", "linter", "validator", "pip"],
    description: "Lint YAML files using yamllint",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "yamllint", runtime: "python", install_methods: &[
        crate::tools::InstallMethod { method: "pip", package: "yamllint" },
    ],
} }

//! taplo checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_taplo(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Check TOML files using taplo",
                subcommand: Some("check"),
                prepend_args: &[],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "taplo", processor_type: crate::processor::ProcessorType::Checker, create: create_taplo,
    fields: &[],
    omit_standard_fields: &[],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".toml"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "taplo", dep_auto: &["taplo.toml", ".taplo.toml"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["toml", "formatter", "linter", "validator"],
    description: "Check TOML files using taplo",
    is_builtin: false,
    is_rust: true,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }

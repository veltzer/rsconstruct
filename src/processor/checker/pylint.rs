//! pylint checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_pylint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint Python files using pylint",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &["python3"],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "pylint", processor_type: crate::processor::ProcessorType::Checker, create: create_pylint,
    fields: &[],
    omit_standard_fields: &[],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".py"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "pylint", dep_auto: &[".pylintrc"], output_depends_on_input_name: true, ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["python", "linter", "py", "pip"],
    description: "Lint Python files using pylint",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }

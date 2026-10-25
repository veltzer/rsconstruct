//! jshint checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_jshint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint JavaScript files using JSHint",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &["node"],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "jshint", processor_type: crate::processor::ProcessorType::Checker, create: create_jshint,
    fields: &[],
    omit_standard_fields: &[],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".js", ".jsx", ".mjs", ".cjs"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "jshint", dep_auto: &[".jshintrc"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["javascript", "linter", "js", "node", "npm", "web", "frontend"],
    description: "Lint JavaScript files using JSHint",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }

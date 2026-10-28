//! jslint checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_jslint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint JavaScript files using JSLint",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &["node"],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "jslint", processor_type: crate::processor::ProcessorType::Checker, create: create_jslint,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".js"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "jslint", ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["javascript", "linter", "js", "node", "web", "frontend"],
    description: "Lint JavaScript files using JSLint",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "jslint", runtime: "node", install_methods: &[
        crate::tools::InstallMethod { method: "npm", package: "jslint" },
    ],
} }

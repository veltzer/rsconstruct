//! svglint checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_svglint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint SVG files using svglint",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "svglint", processor_type: crate::processor::ProcessorType::Checker, create: create_svglint,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".svg"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "svglint", dep_auto: &[".svglintrc.js"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["svg", "linter", "xml", "validator", "node", "npm"],
    description: "Lint SVG files using svglint",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "svglint", runtime: "node", install_methods: &[
        crate::tools::InstallMethod { method: "npm", package: "svglint" },
    ],
} }

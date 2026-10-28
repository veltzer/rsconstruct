//! jsonlint checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_jsonlint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint JSON files using jsonlint",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &["python3"],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "jsonlint", processor_type: crate::processor::ProcessorType::Checker, create: create_jsonlint,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".json"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "jsonlint", ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["json", "linter", "validator", "node", "npm"],
    description: "Lint JSON files using jsonlint",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "jsonlint", runtime: "python", install_methods: &[
        crate::tools::InstallMethod { method: "pip", package: "demjson3" },
    ],
} }

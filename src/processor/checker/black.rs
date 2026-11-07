//! black checker — registered as a {`SimpleChecker`}.

use crate::config::{ExtraTool, SimpleCheckerParams};
use crate::processor::SimpleChecker;

fn create_black(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Check Python code formatting using black",
                subcommand: None,
                prepend_args: &["--check"],
                extra_tools: &[ExtraTool::Python],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "black", processor_type: crate::processor::ProcessorType::Checker, create: create_black,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".py"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "black", dep_auto: &["pyproject.toml"], hash_python_env: true, ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["python", "formatter", "py", "pip"],
    description: "Check Python code formatting using black",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    sandboxable: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "black", runtime: "python",
    project: Some(crate::tools::InstallMethod::new("pip", "black")),
    machine: Some(crate::tools::InstallMethod::new("pip", "black")),
    alternatives: &[], prefer: None, sandbox_writable: &["**/__pycache__"], needs_network: false,
} }

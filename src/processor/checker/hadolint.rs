//! hadolint checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_hadolint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint Dockerfiles using hadolint",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "hadolint", processor_type: crate::processor::ProcessorType::Checker, create: create_hadolint,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["Dockerfile"], src_exclude_dirs: &[] }),
    // hadolint looks for `$PWD/.hadolint.yaml`, then `$PWD/.hadolint.yml`,
    // before the user-level locations (the same list as idockerfile).
    defaults: Some(crate::config::ProcessorDefaults { command: "hadolint", dep_auto: &[".hadolint.yaml", ".hadolint.yml"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["docker", "dockerfile", "linter", "container"],
    description: "Lint Dockerfiles using hadolint",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "hadolint", runtime: "system",
    project: Some(crate::tools::InstallMethod::new("binary", "hadolint")),
    machine: Some(crate::tools::InstallMethod::new("binary", "hadolint")),
    alternatives: &[
        crate::tools::InstallMethod::new("brew", "hadolint"),
        crate::tools::InstallMethod::new("apt", "hadolint"),
    ],
    prefer: None,
} }

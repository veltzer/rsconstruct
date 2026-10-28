//! actionlint checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_actionlint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint GitHub Actions workflow files using actionlint",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "actionlint", processor_type: crate::processor::ProcessorType::Checker, create: create_actionlint,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".yml", ".yaml"], src_exclude_dirs: &[] }),
    // actionlint auto-discovers its config at .github/actionlint.yaml (or
    // .yml); track both so config edits retrigger the check.
    defaults: Some(crate::config::ProcessorDefaults { command: "actionlint", dep_auto: &[".github/actionlint.yaml", ".github/actionlint.yml"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["github", "actions", "workflow", "ci", "linter", "yaml"],
    description: "Lint GitHub Actions workflow files using actionlint",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "actionlint", runtime: "system",
    project: Some(crate::tools::InstallMethod::new("binary", "actionlint")),
    machine: Some(crate::tools::InstallMethod::new("binary", "actionlint")),
    alternatives: &[crate::tools::InstallMethod::new("brew", "actionlint")],
    prefer: None,
} }

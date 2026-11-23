//! zizmor checker — registered as a {`SimpleChecker`}.
//! Runs `zizmor [args] <files>`: a security audit of GitHub Actions
//! workflows (unpinned actions, credential persistence, template injection,
//! excessive permissions). The default `args` keep it offline: the audits
//! that query GitHub need a token and the network, which a build has
//! neither of unless the stanza asks.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_zizmor(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Audit GitHub Actions workflow files for security problems using zizmor",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "zizmor", processor_type: crate::processor::ProcessorType::Checker, create: create_zizmor,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_extensions: &[".yml", ".yaml"], src_exclude_dirs: &[] }),
    // zizmor discovers its config in `.github/` or the working directory;
    // track every name so a config edit retriggers the audit.
    defaults: Some(crate::config::ProcessorDefaults {
        command: "zizmor",
        args: &["--offline", "--no-progress"],
        dep_auto: &[".github/zizmor.yml", ".github/zizmor.yaml", "zizmor.yml", "zizmor.yaml"],
        ..crate::config::ProcessorDefaults::EMPTY
    }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["github", "actions", "workflow", "ci", "security", "audit", "yaml", "rust"],
    description: "Audit GitHub Actions workflow files for security problems using zizmor",
    is_builtin: false,
    is_rust: true,
    supports_batch: true,
    caches_outputs: true,
    sandboxable: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "zizmor", runtime: "rust",
    project: Some(crate::tools::InstallMethod::new("binary", "zizmor")),
    machine: Some(crate::tools::InstallMethod::new("binary", "zizmor")),
    alternatives: &[crate::tools::InstallMethod::new("cargo", "zizmor")],
    prefer: None,
    sandbox_writable: &[],
    needs_network: false,
} }

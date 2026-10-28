//! `php_lint` checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_php_lint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Check PHP syntax using php -l",
                subcommand: Some("-l"),
                prepend_args: &[],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "php_lint", processor_type: crate::processor::ProcessorType::Checker, create: create_php_lint,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".php"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "php", ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["php", "linter"],
    description: "Check PHP syntax using php -l",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "php", runtime: "system", install_methods: &[
        crate::tools::InstallMethod { method: "apt", package: "php-cli" },
    ],
} }

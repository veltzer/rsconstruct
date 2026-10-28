//! htmllint checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_htmllint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint HTML files using htmllint",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &["node"],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "htmllint", processor_type: crate::processor::ProcessorType::Checker, create: create_htmllint,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".html", ".htm"], src_exclude_dirs: &[] }),
    // htmllint reads `.htmllintrc` from the working directory unless `--rc`
    // names another file.
    defaults: Some(crate::config::ProcessorDefaults { command: "htmllint", dep_auto: &[".htmllintrc"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["html", "linter", "web", "frontend", "node", "npm"],
    description: "Lint HTML files using htmllint",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "htmllint", runtime: "node", install_methods: &[
        crate::tools::InstallMethod { method: "npm", package: "htmllint-cli" },
    ],
} }

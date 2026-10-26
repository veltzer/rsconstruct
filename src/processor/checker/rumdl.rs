//! rumdl checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_rumdl(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint Markdown files using rumdl",
                subcommand: Some("check"),
                prepend_args: &[],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! {
    crate::registries::RustAlternative { replaces: "processor.checker.markdownlint", by: "processor.checker.rumdl" }
}
inventory::submit! {
    crate::registries::RustAlternative { replaces: "processor.checker.mdl", by: "processor.checker.rumdl" }
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "rumdl", processor_type: crate::processor::ProcessorType::Checker, create: create_rumdl,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] }),
    // rumdl's config discovery from the working directory: its own files and
    // `pyproject.toml` (`[tool.rumdl]`) first, and when none of those is
    // present, a markdownlint / markdownlint-cli2 configuration.
    defaults: Some(crate::config::ProcessorDefaults { command: "rumdl", dep_auto: &[
        ".rumdl.toml", "rumdl.toml", ".config/rumdl.toml", "pyproject.toml",
        ".markdownlint-cli2.jsonc", ".markdownlint-cli2.yaml", ".markdownlint-cli2.yml",
        ".markdownlint.json", ".markdownlint.jsonc", ".markdownlint.yaml", ".markdownlint.yml",
        "markdownlint.json", "markdownlint.jsonc", "markdownlint.yaml",
    ], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["markdown", "md", "linter", "rust"],
    description: "Lint Markdown files using rumdl",
    is_builtin: false,
    is_rust: true,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }

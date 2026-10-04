//! oxlint checker — registered as a {`SimpleChecker`}.
//!
//! The Rust alternative to the `eslint` processor. oxlint reads an
//! ESLint-style config (`.oxlintrc.json`) and honours `eslint-disable`
//! comments, so a repo moves over by translating its rule list once.
//! A single static binary, so unlike eslint it needs no node runtime and no
//! `node_modules/`.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_oxlint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint JavaScript/TypeScript files using oxlint",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &[],
                fix_subcommand: None,
                fix_prepend_args: &["--fix"],
                fix_batch: None,
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "oxlint", processor_type: crate::processor::ProcessorType::Checker, create: create_oxlint,
    fields: &[],
    omit_standard_fields: &[],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "oxlint", dep_auto: &[".oxlintrc.json", "oxlint.config.js", "oxlint.config.mjs", "oxlint.config.cjs"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["javascript", "typescript", "linter", "js", "ts", "jsx", "tsx", "eslint", "web", "frontend", "rust"],
    description: "Lint JavaScript/TypeScript files using oxlint",
    is_native: false,
    is_rust: true,
    can_fix: false,
    supports_batch: true,
    max_jobs_cap: None,
} }
// oxlint is published three ways: a static binary on the oxc GitHub release,
// an npm package, and a crate. The binary comes first so a repo needs neither
// node nor a Rust toolchain to lint JavaScript.
inventory::submit! { crate::tools::ToolInfo {
    name: "oxlint", runtime: "rust", install_methods: &[
        crate::tools::InstallMethod { method: "binary", package: "oxlint" },
        crate::tools::InstallMethod { method: "npm", package: "oxlint" },
        crate::tools::InstallMethod { method: "cargo", package: "oxlint" },
    ],
} }

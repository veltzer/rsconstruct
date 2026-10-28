//! biome checker — registered as a {`SimpleChecker`}.
//!
//! The Rust alternative to the `stylelint` processor, and a second one to
//! `eslint`: biome lints CSS, JavaScript, TypeScript and JSON from one
//! static binary, with no node runtime and no `node_modules/`. It does not
//! parse SCSS, Sass or Less. Which languages it actually checks is decided
//! by the repo's biome config (`biome.json(c)` or the hidden
//! `.biome.json(c)`, all four read by biome itself; each language has its own
//! `linter.enabled`), so a repo that lints JavaScript with `oxlint` turns
//! biome's JavaScript linter off there and narrows `src_extensions` to
//! `.css`.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_biome(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint CSS/JavaScript/TypeScript/JSON files using biome",
                subcommand: Some("lint"),
                prepend_args: &[],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! {
    crate::registries::RustAlternative { replaces: "processor.checker.stylelint", by: "processor.checker.biome" }
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "biome", processor_type: crate::processor::ProcessorType::Checker, create: create_biome,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".css", ".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs", ".json", ".jsonc"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "biome", dep_auto: &["biome.json", "biome.jsonc", ".biome.json", ".biome.jsonc"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["css", "javascript", "typescript", "json", "linter", "stylelint", "eslint", "web", "frontend", "rust"],
    description: "Lint CSS/JavaScript/TypeScript/JSON files using biome",
    is_builtin: false,
    is_rust: true,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }
// Binary first: biome's GitHub release ships a static Linux binary, so a
// repo needs no node runtime to lint CSS or JS. The npm package is the
// fallback. No cargo method: upstream does not publish the CLI to
// crates.io (the `biome_*` crates there are its internal libraries).
inventory::submit! { crate::tools::ToolInfo {
    name: "biome", runtime: "rust", install_methods: &[
        crate::tools::InstallMethod { method: "binary", package: "biome" },
        crate::tools::InstallMethod { method: "npm", package: "@biomejs/biome" },
    ],
} }

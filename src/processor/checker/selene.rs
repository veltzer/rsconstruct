//! selene checker — registered as a {`SimpleChecker`}.
//!
//! The Rust alternative to the `luacheck` processor. selene reads its
//! configuration from `selene.toml` (lint levels, the standard library to
//! check against) and honours `-- selene: allow(...)` comments. It fails on
//! warnings as well as errors, as luacheck does.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_selene(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint Lua files using selene",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &[],
                fix_subcommand: None,
                fix_prepend_args: &[],
                fix_batch: None,
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "selene", processor_type: crate::processor::ProcessorType::Checker, create: create_selene,
    fields: &[],
    omit_standard_fields: &[],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".lua"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "selene", dep_auto: &["selene.toml"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["lua", "linter", "checker", "luacheck", "rust"],
    description: "Lint Lua files using selene",
    is_native: false,
    is_rust: true,
    can_fix: false,
    supports_batch: true,
    max_jobs_cap: None,
} }
// selene is published as a zipped binary on its GitHub release and as a
// crate. The binary comes first so a repo needs no Rust toolchain to lint Lua.
inventory::submit! { crate::tools::ToolInfo {
    name: "selene", runtime: "rust", install_methods: &[
        crate::tools::InstallMethod { method: "binary", package: "selene" },
        crate::tools::InstallMethod { method: "cargo", package: "selene" },
    ],
} }

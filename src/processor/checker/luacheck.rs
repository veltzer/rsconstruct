//! luacheck checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_luacheck(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint Lua files using luacheck",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "luacheck", processor_type: crate::processor::ProcessorType::Checker, create: create_luacheck,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".lua"], src_exclude_dirs: &[] }),
    // `.luacheckrc` applies options per path (`files["glob"]`) and luacheck
    // picks a default std from the path (`*_spec.lua` under `spec/`), so the
    // verdict depends on the file's name, not only its content — as for
    // iluacheck.
    defaults: Some(crate::config::ProcessorDefaults { command: "luacheck", dep_auto: &[".luacheckrc"], output_depends_on_input_name: true, ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["lua", "linter", "checker"],
    description: "Lint Lua files using luacheck",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }
inventory::submit! { crate::tools::ToolInfo {
    name: "luacheck", runtime: "system",
    project: None,
    machine: Some(crate::tools::InstallMethod::new("apt", "lua-check")),
    alternatives: &[], prefer: None,
} }

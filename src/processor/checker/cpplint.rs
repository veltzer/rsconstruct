//! cpplint checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_cpplint(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint C/C++ files using cpplint",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "cpplint", processor_type: crate::processor::ProcessorType::Checker, create: create_cpplint,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".c", ".cc", ".h", ".hh"], src_exclude_dirs: &[] }),
    // cpplint reads CPPLINT.cfg from the file's directory and every parent,
    // so the project root's copy is an input of every product when it exists.
    defaults: Some(crate::config::ProcessorDefaults { command: "cpplint", dep_auto: &["CPPLINT.cfg"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["c", "cpp", "linter", "google", "cc", "h", "hpp"],
    description: "Lint C/C++ files using cpplint",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }

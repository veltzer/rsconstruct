//! cmake checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_cmake(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Lint CMake files using cmakelint",
                subcommand: None,
                prepend_args: &[],
                extra_tools: &[],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "cmake", processor_type: crate::processor::ProcessorType::Checker, create: create_cmake,
    fields: &[],
    omit_standard_fields: &["formats", "output_dir"],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["CMakeLists.txt"], src_exclude_dirs: &[] }),
    // cmakelint reads `$PWD/.cmakelintrc` by default (then the user-level
    // `~/.config/cmakelintrc` and `~/.cmakelintrc`, which are not inputs).
    defaults: Some(crate::config::ProcessorDefaults { command: "cmakelint", dep_auto: &[".cmakelintrc"], ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["cmake", "builder", "c", "cpp", "checker"],
    description: "Lint CMake files using cmakelint",
    is_builtin: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }

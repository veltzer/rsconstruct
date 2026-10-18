//! slidev checker — registered as a {`SimpleChecker`}.

use crate::config::SimpleCheckerParams;
use crate::processor::SimpleChecker;

fn create_slidev(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleChecker::new(
            cfg,
            SimpleCheckerParams {
                description: "Build Slidev presentations",
                subcommand: Some("build"),
                prepend_args: &[],
                extra_tools: &["node"],
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "slidev", processor_type: crate::processor::ProcessorType::Checker, create: create_slidev,
    fields: &[],
    omit_standard_fields: &[],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".md"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { command: "slidev", ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["markdown", "presentation", "slides", "vue", "web", "frontend", "node", "npm"],
    description: "Build Slidev presentations",
    is_native: false,
    is_rust: false,
    supports_batch: true,
    caches_outputs: true,
    max_jobs_cap: None,
} }

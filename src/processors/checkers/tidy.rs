//! tidy checker — registered as a {SimpleChecker}.

use crate::processors::SimpleChecker;
use crate::config::SimpleCheckerParams;

fn create_tidy(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(SimpleChecker::new(cfg, SimpleCheckerParams { description: "Validate HTML files using tidy", subcommand: Some("-errors"), prepend_args: &[], extra_tools: &[], fix_subcommand: None, fix_prepend_args: &[], fix_batch: None })))
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "tidy", processor_type: crate::processors::ProcessorType::Checker, create: create_tidy,
    known_fields: crate::registries::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registries::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registries::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registries::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["html", "xml", "formatter", "validator", "web"],
} }

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::analyzers::DepAnalyzer;
use crate::config::{AnalyzerConfig, KnownFields};
use crate::processors::{Processor, ProcessorType};

/// A processor plugin. One struct for all processor types.
/// Each processor file submits one of these via `inventory::submit!`.
///
/// The plugin is a factory: it knows its name, type, how to create a processor
/// from TOML config, and metadata about its config fields.
///
/// The framework applies defaults to the TOML before calling `create`.
/// The `create` function deserializes the TOML and returns a fully configured,
/// immutable processor.
#[allow(dead_code)]
pub struct ProcessorPlugin {
    pub name: &'static str,
    pub processor_type: ProcessorType,
    /// Create a processor from resolved TOML config (defaults already applied).
    pub create: fn(&toml::Value) -> Result<Box<dyn Processor>>,
    /// Config metadata
    pub known_fields: fn() -> &'static [&'static str],
    pub output_fields: fn() -> &'static [&'static str],
    pub must_fields: fn() -> &'static [&'static str],
    pub field_descriptions: fn() -> &'static [(&'static str, &'static str)],
    /// Return the default config as pretty JSON. Receives the processor name
    /// so it can apply the correct defaults.
    pub defconfig_json: fn(&str) -> Option<String>,
}

unsafe impl Sync for ProcessorPlugin {}

inventory::collect!(ProcessorPlugin);

pub(crate) fn all_plugins() -> impl Iterator<Item = &'static ProcessorPlugin> {
    inventory::iter::<ProcessorPlugin>.into_iter()
}

// --- Analyzer registry ---

/// An analyzer plugin. Each analyzer file submits one via `inventory::submit!`.
pub struct AnalyzerPlugin {
    pub name: &'static str,
    pub description: &'static str,
    pub is_native: bool,
    /// Create an analyzer from the project's analyzer config.
    pub create: fn(&AnalyzerConfig, bool) -> Box<dyn DepAnalyzer>,
    /// Return the default config as a TOML string, or None if the analyzer has no config.
    pub defconfig_toml: fn() -> Option<String>,
}

unsafe impl Sync for AnalyzerPlugin {}

inventory::collect!(AnalyzerPlugin);

pub(crate) fn all_analyzer_plugins() -> impl Iterator<Item = &'static AnalyzerPlugin> {
    inventory::iter::<AnalyzerPlugin>.into_iter()
}

/// Return sorted analyzer names from the registry.
pub(crate) fn all_analyzer_names() -> Vec<&'static str> {
    let mut names: Vec<&str> = all_analyzer_plugins().map(|p| p.name).collect();
    names.sort();
    names
}

/// Find an analyzer plugin by name.
pub(crate) fn find_analyzer_plugin(name: &str) -> Option<&'static AnalyzerPlugin> {
    all_analyzer_plugins().find(|p| p.name == name)
}

/// Build a clap value parser that accepts any registered analyzer name.
pub(crate) fn analyzer_name_parser() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(all_analyzer_names())
}

/// Apply both processor defaults and scan defaults to a TOML value.
pub fn apply_all_defaults(name: &str, value: &mut toml::Value) {
    crate::config::apply_processor_defaults(name, value);
    crate::config::apply_scan_defaults(name, value);
}

// --- Helpers that processor files call from their create/defconfig functions ---

/// Deserialize TOML into config type C and call the constructor.
/// The TOML should already have defaults applied by the framework.
pub fn deserialize_and_create<C: Default + DeserializeOwned>(
    config_toml: &toml::Value, ctor: fn(C) -> Box<dyn Processor>,
) -> Result<Box<dyn Processor>> {
    let cfg: C = toml::from_str(&toml::to_string(config_toml)?)?;
    Ok(ctor(cfg))
}

/// Build default config JSON for a config type, applying defaults for the given processor name.
pub fn default_config_json<C: Default + DeserializeOwned + Serialize>(name: &str) -> Option<String> {
    let mut val = toml::Value::Table(toml::map::Map::new());
    apply_all_defaults(name, &mut val);
    let cfg: C = toml::from_str(&toml::to_string(&val).ok()?).ok()?;
    serde_json::to_string_pretty(&serde_json::to_value(cfg).ok()?).ok()
}

pub fn typed_known_fields<C: KnownFields>() -> &'static [&'static str] { C::known_fields() }
pub fn typed_output_fields<C: KnownFields>() -> &'static [&'static str] { C::output_fields() }
pub fn typed_must_fields<C: KnownFields>() -> &'static [&'static str] { C::must_fields() }
pub fn typed_field_descriptions<C: KnownFields>() -> &'static [(&'static str, &'static str)] { C::field_descriptions() }

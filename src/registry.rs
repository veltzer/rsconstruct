use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::config::{self, KnownFields, StandardConfig};
use crate::processors::ProductDiscovery;

/// Operations that each processor plugin provides.
/// Implemented generically via `TypedPlugin<C>`.
pub(crate) trait RegistryOps: Send + Sync {
    fn name(&self) -> &'static str;
    fn create(&self, config_toml: &toml::Value) -> Result<Box<dyn ProductDiscovery>>;
    fn create_default(&self) -> Box<dyn ProductDiscovery>;
    fn resolve_defaults(&self, value: &mut toml::Value) -> Result<()>;
    fn known_fields(&self) -> &'static [&'static str];
    fn output_fields(&self) -> &'static [&'static str];
    fn must_fields(&self) -> &'static [&'static str];
    fn field_descriptions(&self) -> &'static [(&'static str, &'static str)];
    fn defconfig_json(&self) -> Option<String>;
}

// Collect all processor plugins via inventory
inventory::collect!(&'static dyn RegistryOps);

/// Iterate over all registered processor plugins.
pub(crate) fn all_plugins() -> impl Iterator<Item = &'static &'static dyn RegistryOps> {
    inventory::iter::<&'static dyn RegistryOps>.into_iter()
}

/// Apply both processor defaults and scan defaults to a TOML value.
fn apply_all_defaults(name: &str, value: &mut toml::Value) {
    config::apply_processor_defaults(name, value);
    config::apply_scan_defaults(name, value);
}

/// Generic implementation of RegistryOps for a (Config, Processor) pair.
pub(crate) struct TypedPlugin<C> {
    name: &'static str,
    ctor: fn(C) -> Box<dyn ProductDiscovery>,
}

// Safety: TypedPlugin contains only static data (name is &'static str, ctor is fn pointer)
unsafe impl<C> Sync for TypedPlugin<C> {}

impl<C> RegistryOps for TypedPlugin<C>
where
    C: Default + DeserializeOwned + Serialize + Clone + KnownFields + Send + Sync + 'static,
{
    fn name(&self) -> &'static str { self.name }

    fn create(&self, config_toml: &toml::Value) -> Result<Box<dyn ProductDiscovery>> {
        let mut config_val = config_toml.clone();
        apply_all_defaults(self.name, &mut config_val);
        let cfg: C = toml::from_str(&toml::to_string(&config_val)?)?;
        Ok((self.ctor)(cfg))
    }

    fn create_default(&self) -> Box<dyn ProductDiscovery> {
        let config_val = toml::Value::Table(toml::map::Map::new());
        self.create(&config_val).unwrap()
    }

    fn resolve_defaults(&self, value: &mut toml::Value) -> Result<()> {
        apply_all_defaults(self.name, value);
        let cfg: C = toml::from_str(&toml::to_string(value)?)?;
        *value = toml::Value::try_from(&cfg)?;
        Ok(())
    }

    fn known_fields(&self) -> &'static [&'static str] { C::known_fields() }
    fn output_fields(&self) -> &'static [&'static str] { C::output_fields() }
    fn must_fields(&self) -> &'static [&'static str] { C::must_fields() }
    fn field_descriptions(&self) -> &'static [(&'static str, &'static str)] { C::field_descriptions() }

    fn defconfig_json(&self) -> Option<String> {
        let mut config_val = toml::Value::Table(toml::map::Map::new());
        apply_all_defaults(self.name, &mut config_val);
        let cfg: C = toml::from_str(&toml::to_string(&config_val).ok()?).ok()?;
        let json = serde_json::to_value(cfg).ok()?;
        serde_json::to_string_pretty(&json).ok()
    }
}

/// Create a static plugin registration for a typed processor.
/// Use with `inventory::submit!` in the processor's module.
pub(crate) const fn typed_plugin<C>(
    name: &'static str,
    ctor: fn(C) -> Box<dyn ProductDiscovery>,
) -> TypedPlugin<C> {
    TypedPlugin { name, ctor }
}

/// Create a static plugin registration for a SimpleChecker.
pub(crate) const fn simple_checker_plugin(
    name: &'static str,
    params: crate::config::SimpleCheckerParams,
) -> SimpleCheckerPlugin {
    SimpleCheckerPlugin { name, params }
}

/// Plugin for data-driven simple checkers.
pub struct SimpleCheckerPlugin {
    name: &'static str,
    params: crate::config::SimpleCheckerParams,
}

unsafe impl Sync for SimpleCheckerPlugin {}

impl RegistryOps for SimpleCheckerPlugin {
    fn name(&self) -> &'static str { self.name }

    fn create(&self, config_toml: &toml::Value) -> Result<Box<dyn ProductDiscovery>> {
        let mut config_val = config_toml.clone();
        apply_all_defaults(self.name, &mut config_val);
        let cfg: StandardConfig = toml::from_str(&toml::to_string(&config_val)?)?;
        Ok(Box::new(crate::processors::SimpleChecker::new(cfg, self.params)))
    }

    fn create_default(&self) -> Box<dyn ProductDiscovery> {
        let config_val = toml::Value::Table(toml::map::Map::new());
        self.create(&config_val).unwrap()
    }

    fn resolve_defaults(&self, value: &mut toml::Value) -> Result<()> {
        apply_all_defaults(self.name, value);
        let cfg: StandardConfig = toml::from_str(&toml::to_string(value)?)?;
        *value = toml::Value::try_from(&cfg)?;
        Ok(())
    }

    fn known_fields(&self) -> &'static [&'static str] { StandardConfig::known_fields() }
    fn output_fields(&self) -> &'static [&'static str] { StandardConfig::output_fields() }
    fn must_fields(&self) -> &'static [&'static str] { StandardConfig::must_fields() }
    fn field_descriptions(&self) -> &'static [(&'static str, &'static str)] { StandardConfig::field_descriptions() }

    fn defconfig_json(&self) -> Option<String> {
        let mut config_val = toml::Value::Table(toml::map::Map::new());
        apply_all_defaults(self.name, &mut config_val);
        let cfg: StandardConfig = toml::from_str(&toml::to_string(&config_val).ok()?).ok()?;
        let json = serde_json::to_value(cfg).ok()?;
        serde_json::to_string_pretty(&json).ok()
    }
}

/// Plugin for data-driven simple generators.
pub struct SimpleGeneratorPlugin {
    name: &'static str,
    params: crate::processors::generators::simple::SimpleGeneratorParams,
}

unsafe impl Sync for SimpleGeneratorPlugin {}

impl RegistryOps for SimpleGeneratorPlugin {
    fn name(&self) -> &'static str { self.name }

    fn create(&self, config_toml: &toml::Value) -> Result<Box<dyn ProductDiscovery>> {
        let mut config_val = config_toml.clone();
        apply_all_defaults(self.name, &mut config_val);
        let cfg: StandardConfig = toml::from_str(&toml::to_string(&config_val)?)?;
        Ok(Box::new(crate::processors::SimpleGenerator::new(cfg, self.params)))
    }

    fn create_default(&self) -> Box<dyn ProductDiscovery> {
        let config_val = toml::Value::Table(toml::map::Map::new());
        self.create(&config_val).unwrap()
    }

    fn resolve_defaults(&self, value: &mut toml::Value) -> Result<()> {
        apply_all_defaults(self.name, value);
        let cfg: StandardConfig = toml::from_str(&toml::to_string(value)?)?;
        *value = toml::Value::try_from(&cfg)?;
        Ok(())
    }

    fn known_fields(&self) -> &'static [&'static str] { StandardConfig::known_fields() }
    fn output_fields(&self) -> &'static [&'static str] { StandardConfig::output_fields() }
    fn must_fields(&self) -> &'static [&'static str] { StandardConfig::must_fields() }
    fn field_descriptions(&self) -> &'static [(&'static str, &'static str)] { StandardConfig::field_descriptions() }

    fn defconfig_json(&self) -> Option<String> {
        let mut config_val = toml::Value::Table(toml::map::Map::new());
        apply_all_defaults(self.name, &mut config_val);
        let cfg: StandardConfig = toml::from_str(&toml::to_string(&config_val).ok()?).ok()?;
        let json = serde_json::to_value(cfg).ok()?;
        serde_json::to_string_pretty(&json).ok()
    }
}

/// Create a static plugin registration for a SimpleGenerator.
pub(crate) const fn simple_generator_plugin(
    name: &'static str,
    params: crate::processors::generators::simple::SimpleGeneratorParams,
) -> SimpleGeneratorPlugin {
    SimpleGeneratorPlugin { name, params }
}

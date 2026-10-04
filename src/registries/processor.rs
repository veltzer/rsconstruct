//! Processor plugin registry.
//!
//! Every built-in processor (checker, generator, creator, mass-generator) submits
//! a [`ProcessorPlugin`] entry via `inventory::submit!`. The inventory is collected
//! at link time.

use anyhow::Result;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::processor::{Processor, ProcessorType};

/// A processor plugin. One struct for all processor types.
/// Each processor file submits one of these via `inventory::submit!`.
///
/// The plugin is a factory: it knows its name, type, how to create a processor
/// from TOML config, and metadata about its config fields.
///
/// The framework applies defaults to the TOML before calling `create`.
/// The `create` function deserializes the TOML and returns a fully configured,
/// immutable processor.
pub struct ProcessorPlugin {
    pub name: &'static str,
    /// Processor type (checker, generator, creator, explicit, mass generator).
    pub processor_type: ProcessorType,
    /// Implementation version. **Bump this when changes would make the processor
    /// produce different output for the same inputs**, or change which inputs are
    /// discovered, which outputs are declared, or how config fields are interpreted.
    /// Do NOT bump for refactors, comments, reformats, or behavior-preserving
    /// bug fixes. See `docs/src/processor-versioning.md` for the full bump rule.
    ///
    /// The version is mixed into every product's cache key, so bumping here
    /// invalidates caches only for this processor (leaves others untouched).
    pub version: u32,
    /// Create a processor from resolved TOML config (defaults already applied).
    pub create: fn(&toml::Value) -> Result<Box<dyn Processor>>,
    /// The processor's custom config fields — THE schema. Every projection
    /// (known/checksum/must fields, descriptions, expected types) derives
    /// from this list plus the implicit `StandardConfig` fields, so a field
    /// is declared exactly once, in the processor's own file. Standard
    /// fields need no entry unless overridden (e.g. `required: true` on
    /// "command"); a spec whose name matches a standard field replaces the
    /// inherited one.
    pub fields: &'static [crate::config::FieldSpec],
    /// Standard fields this processor does NOT accept (e.g. `cc` takes no
    /// "command" — compilers come from cc.yaml). Listed fields are removed
    /// from the derived known/checksum sets, so the validator rejects them.
    pub omit_standard_fields: &'static [&'static str],
    /// Scan defaults (`src_extensions` etc.), applied with provenance before
    /// deserialization. `None` for processors that scan nothing by default
    /// (script, creator, generator, explicit).
    pub scan_defaults: Option<crate::config::ScanDefaultsData>,
    /// Processor defaults (`command`, `dep_auto`, ...), applied with
    /// provenance before deserialization. `None` when every default is empty.
    pub defaults: Option<crate::config::ProcessorDefaults>,
    /// Return the default config as pretty JSON. Receives the processor name
    /// so it can apply the correct defaults.
    pub defconfig_json: fn(&str) -> Option<String>,
    /// Search keywords for `processor search`.
    pub keywords: &'static [&'static str],
    /// Human-readable description (static, no instantiation needed).
    pub description: &'static str,
    /// Whether this is a native (pure Rust) processor.
    pub is_native: bool,
    /// Whether the code that does the work is written in Rust. Every native
    /// processor is (it is rsconstruct itself); an external processor is only
    /// when the tool it runs is (ruff, taplo, rumdl, clippy, cargo, mdbook,
    /// pyrefly, rustc). Processors that run a user-supplied command (script,
    /// explicit, generator, creator) are `false`: the language is unknown.
    /// Shown by `processor list` and `status` so the language of the toolchain
    /// can be read off the table.
    pub is_rust: bool,
    /// Whether this processor has fix capability (`rsconstruct fix`).
    pub can_fix: bool,
    /// Whether this processor can execute multiple products in one invocation.
    /// Static capability — if false, the `batch` config field has no effect at runtime.
    pub supports_batch: bool,
    /// Hard cap on parallel jobs for this processor. `None` means no cap.
    /// `Some(1)` means the processor must run one product at a time (e.g. package
    /// managers, whole-project aggregators). The effective `max_jobs` is
    /// `min(config.max_jobs, max_jobs_cap)` with `None` treated as unlimited.
    pub max_jobs_cap: Option<usize>,
}

unsafe impl Sync for ProcessorPlugin {}

inventory::collect!(ProcessorPlugin);

pub fn all_plugins() -> impl Iterator<Item = &'static ProcessorPlugin> {
    inventory::iter::<ProcessorPlugin>.into_iter()
}

/// Every processor name starts with this: a processor is named by its full
/// path, `processor.<type>.<name>`, exactly as its module is
/// (`processor::checker::ruff`), and an instance appends one more segment.
pub const NAME_PREFIX: &str = "processor.";

/// A processor name taken apart: `processor.<type>.<name>[.<instance>]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedName<'a> {
    pub processor_type: ProcessorType,
    /// The last segment of the processor's own name (`ruff`), unique only
    /// within its type: `processor.creator.generic` and
    /// `processor.explicit.generic` are different processors.
    pub short: &'a str,
    /// The instance segment of a multi-instance name, if any.
    pub instance: Option<&'a str>,
}

impl ParsedName<'_> {
    /// The processor name without the instance segment.
    pub fn pname(&self) -> String {
        format!(
            "{NAME_PREFIX}{}.{}",
            self.processor_type.as_str(),
            self.short
        )
    }
}

/// Take a processor or instance name apart. `None` when it is not of the form
/// `processor.<known type>.<name>[.<instance>]`; every segment must be
/// non-empty. Lua plugin names (`processor.lua.<name>`) parse too — the type
/// is known even though no plugin entry exists for them.
pub fn parse_name(name: &str) -> Option<ParsedName<'_>> {
    let rest = name.strip_prefix(NAME_PREFIX)?;
    let (type_str, rest) = rest.split_once('.')?;
    let processor_type = ProcessorType::parse(type_str)?;
    let (short, instance) = match rest.split_once('.') {
        Some((short, instance)) => (short, Some(instance)),
        None => (rest, None),
    };
    if short.is_empty() || instance.is_some_and(str::is_empty) {
        return None;
    }
    Some(ParsedName {
        processor_type,
        short,
        instance,
    })
}

/// The processor name (`processor.<type>.<name>`) of a processor or instance
/// name, or the input itself when it does not parse.
pub fn pname_of(name: &str) -> String {
    parse_name(name).map_or_else(|| name.to_string(), |p| p.pname())
}

impl ProcessorPlugin {
    /// The processor's full name, `processor.<type>.<name>` — the name used
    /// in `rsconstruct.toml`, on the command line and in every product.
    pub fn pname(&self) -> String {
        format!(
            "{NAME_PREFIX}{}.{}",
            self.processor_type.as_str(),
            self.name
        )
    }
}

/// Look up a processor plugin by its full name (`processor.checker.ruff`).
/// An instance name (`processor.checker.ruff.core`) finds the same plugin.
pub fn find_plugin(name: &str) -> Option<&'static ProcessorPlugin> {
    let ParsedName {
        processor_type,
        short,
        ..
    } = parse_name(name)?;
    all_plugins().find(|p| p.processor_type == processor_type && p.name == short)
}

/// Return the static description for a processor by instance name, or `""` if unknown.
pub fn description_of(name: &str) -> &'static str {
    find_plugin(name).map_or("", |p| p.description)
}

/// Return the processor type for a processor by instance name. The type is a
/// segment of the name, so this works for Lua plugins too; a name that does
/// not parse at all reports `Checker`.
pub fn processor_type_of(name: &str) -> crate::processor::ProcessorType {
    parse_name(name).map_or(crate::processor::ProcessorType::Checker, |p| {
        p.processor_type
    })
}

/// Return whether a processor is native (pure Rust) by instance name.
pub fn is_native(name: &str) -> bool {
    find_plugin(name).is_some_and(|p| p.is_native)
}

/// Return whether a processor's implementation is written in Rust, by instance
/// name. See [`ProcessorPlugin::is_rust`].
pub fn is_rust(name: &str) -> bool {
    find_plugin(name).is_some_and(|p| p.is_rust)
}

/// Return whether a processor can fix by instance name.
pub fn can_fix(name: &str) -> bool {
    find_plugin(name).is_some_and(|p| p.can_fix)
}

/// Look up a processor's implementation version by instance name.
/// Returns `None` for processor names not in the builtin registry (e.g. Lua plugins).
/// Used by `Product::descriptor_key` to mix the processor's version into every
/// cache key, so bumping a processor's `version` invalidates exactly that
/// processor's cached outputs.
///
/// Must route through `find_plugin`: products carry *instance* names
/// ("pylint.core"), and a hand-rolled lookup that misses the suffix strip
/// silently keys every multi-instance processor at v0, so version bumps
/// never invalidate their cached results.
pub fn processor_version(name: &str) -> Option<u32> {
    find_plugin(name).map(|p| p.version)
}

/// Every registered processor's full name, sorted.
pub fn all_pnames() -> Vec<String> {
    let mut names: Vec<String> = all_plugins().map(ProcessorPlugin::pname).collect();
    names.sort_unstable();
    names
}

/// Build a clap value parser that accepts any registered processor name (pname).
pub fn processor_name_parser() -> clap::builder::PossibleValuesParser {
    // clap wants `&'static str` possible values and the full names are
    // built at runtime; the parser is constructed once per process, so
    // leaking these ~100 short strings is the whole cost.
    let names: Vec<&'static str> = all_pnames()
        .into_iter()
        .map(|n| &*Box::leak(n.into_boxed_str()))
        .collect();
    clap::builder::PossibleValuesParser::new(names)
}

/// Apply both processor defaults and scan defaults to a TOML value.
/// Every field that's injected is recorded in `provenance`.
pub fn apply_all_defaults(
    name: &str,
    value: &mut toml::Value,
    provenance: &mut crate::config::ProvenanceMap,
) {
    crate::config::apply_processor_defaults(name, value, provenance);
    crate::config::apply_scan_defaults(name, value, provenance);
}

// --- Helpers that processor files call from their create/defconfig functions ---

/// Deserialize TOML into config type C and call the constructor.
/// The TOML should already have defaults applied by the framework.
pub fn deserialize_and_create<C: Default + DeserializeOwned>(
    config_toml: &toml::Value,
    ctor: fn(C) -> Box<dyn Processor>,
) -> Result<Box<dyn Processor>> {
    let cfg: C = toml::from_str(&toml::to_string(config_toml)?)?;
    Ok(ctor(cfg))
}

/// Like [`deserialize_and_create`], for processors whose constructor can fail
/// (e.g. reading a support file like a personal dictionary).
pub fn deserialize_and_try_create<C: Default + DeserializeOwned>(
    config_toml: &toml::Value,
    ctor: fn(C) -> Result<Box<dyn Processor>>,
) -> Result<Box<dyn Processor>> {
    let cfg: C = toml::from_str(&toml::to_string(config_toml)?)?;
    ctor(cfg)
}

/// Build default config JSON for a config type, applying defaults for the given processor name.
pub fn default_config_json<C: Default + DeserializeOwned + Serialize>(
    name: &str,
) -> Option<String> {
    let mut val = toml::Value::Table(toml::map::Map::new());
    let mut prov = crate::config::ProvenanceMap::new();
    apply_all_defaults(name, &mut val, &mut prov);
    let cfg: C = toml::from_str(&toml::to_string(&val).ok()?).ok()?;
    let json_val = serde_json::to_value(&cfg).ok()?;

    // Defense-in-depth check (debug builds only): every key the config
    // serializes must be recognized by the derived schema (the plugin's
    // FieldSpec list plus the implicit StandardConfig/scan fields) — a
    // serialized-but-undeclared field would be invisible to validation and
    // to checksum membership.
    // Runtime-gated, not `#[cfg]`-forked: a cfg'd-out block is code that
    // `cargo test` can never compile. `cfg!` keeps it compiled everywhere
    // and evaluated only in debug builds.
    if cfg!(debug_assertions)
        && let Some(obj) = json_val.as_object()
    {
        use crate::config::KnownFields as _;
        let known: std::collections::HashSet<&str> =
            crate::config::ProcessorConfig::known_fields_for(name)
                .unwrap_or_default()
                .into_iter()
                .chain(
                    crate::config::StandardConfig::known_fields()
                        .iter()
                        .copied(),
                )
                .chain(crate::config::SCAN_CONFIG_FIELDS.iter().copied())
                .chain(crate::config::STANDARD_EXTRA_FIELDS.iter().copied())
                .collect();
        for key in obj.keys() {
            debug_assert!(
                known.contains(key.as_str()),
                "Processor '{name}': default config field '{key}' is serialized but not declared in its FieldSpec list or scan fields"
            );
        }
    }

    serde_json::to_string_pretty(&json_val).ok()
}

// Typed KnownFields projection — used by ANALYZER plugin entries only
// (AnalyzerPlugin still carries metadata as fn pointers). Processor plugins
// declare a FieldSpec list instead; do not use this in ProcessorPlugin
// entries.
pub fn typed_known_fields<C: crate::config::KnownFields>() -> &'static [&'static str] {
    C::known_fields()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every registry accessor must answer identically for a type name and
    /// for an instance name of that type. Products carry instance names
    /// ("pylint.core"), so an accessor that skips the suffix strip silently
    /// misbehaves only for multi-instance configs — `processor_version` did
    /// exactly that, keying every multi-instance processor's cache at v0 so
    /// version bumps never invalidated their cached results.
    #[test]
    fn accessors_resolve_instance_names_like_type_names() {
        for plugin in all_plugins() {
            let pname = plugin.pname();
            let instance = format!("{pname}.someinst");
            assert_eq!(
                processor_version(&instance),
                Some(plugin.version),
                "processor_version must strip the instance suffix for '{instance}'"
            );
            assert_eq!(processor_version(&pname), Some(plugin.version));
            assert_eq!(
                is_native(&instance),
                is_native(&pname),
                "is_native must strip the instance suffix for '{instance}'"
            );
            assert_eq!(
                is_rust(&instance),
                is_rust(&pname),
                "is_rust must strip the instance suffix for '{instance}'"
            );
            assert_eq!(
                can_fix(&instance),
                can_fix(&pname),
                "can_fix must strip the instance suffix for '{instance}'"
            );
            assert_eq!(
                description_of(&instance),
                description_of(&pname),
                "description_of must strip the instance suffix for '{instance}'"
            );
            assert_eq!(
                processor_type_of(&instance),
                plugin.processor_type,
                "the type is a segment of the name: '{instance}'"
            );
        }
    }

    /// Names are `processor.<type>.<name>[.<instance>]`, nothing else: the
    /// bare short name a config used to carry is no longer a name at all.
    #[test]
    fn parse_name_decision_table() {
        let p = parse_name("processor.checker.ruff").unwrap();
        assert_eq!(p.processor_type, ProcessorType::Checker);
        assert_eq!(p.short, "ruff");
        assert_eq!(p.instance, None);
        assert_eq!(p.pname(), "processor.checker.ruff");

        let p = parse_name("processor.checker.script.lint_a").unwrap();
        assert_eq!(p.instance, Some("lint_a"));
        assert_eq!(p.pname(), "processor.checker.script");
        assert_eq!(
            pname_of("processor.checker.script.lint_a"),
            "processor.checker.script"
        );

        let p = parse_name("processor.lua.myplugin").unwrap();
        assert_eq!(p.processor_type, ProcessorType::Lua);
        assert!(find_plugin("processor.lua.myplugin").is_none());

        for bad in [
            "ruff",
            "checker.ruff",
            "processor.ruff",
            "processor.nosuchtype.ruff",
            "processor.checker.",
            "processor.checker.ruff.",
            "processor..ruff",
        ] {
            assert!(parse_name(bad).is_none(), "{bad} must not parse");
        }
    }

    /// The same short name may exist under several types; the full name
    /// keeps them apart.
    #[test]
    fn short_names_are_scoped_by_type() {
        let creator = find_plugin("processor.creator.generic").unwrap();
        let explicit = find_plugin("processor.explicit.generic").unwrap();
        assert_eq!(creator.processor_type, ProcessorType::Creator);
        assert_eq!(explicit.processor_type, ProcessorType::Explicit);
        assert!(find_plugin("processor.checker.generic").is_none());
    }

    /// A native processor is rsconstruct's own Rust code, so `is_rust` cannot
    /// be false for it. The two flags are declared separately because the
    /// reverse does not hold (ruff is external and Rust), and a plugin entry
    /// that says native but not Rust is a copy-paste mistake.
    #[test]
    fn native_processors_are_rust() {
        for plugin in all_plugins() {
            if plugin.is_native {
                assert!(
                    plugin.is_rust,
                    "Processor '{}' is native (pure Rust) but declares is_rust: false",
                    plugin.name
                );
            }
        }
    }
}

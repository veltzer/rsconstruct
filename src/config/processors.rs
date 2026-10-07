//! The `[processor]` section: declared processor instances, the per-processor
//! schema lookups (known/checksum/required fields) and the default values
//! applied to each instance before it is deserialized.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    FieldProvenance, KnownFields, ProcessorDefaults, ProvenanceMap, SCAN_CONFIG_FIELDS,
    STANDARD_EXTRA_FIELDS, ScanDefaultsData, StandardConfig, provenance,
};
use crate::registries::{self as registry, ProcessorPlugin};

/// A single processor instance parsed from the TOML config.
/// `[processor.checker.pylint]` produces one instance with
/// `pname="processor.checker.pylint"`, `instance_name="processor.checker.pylint"`.
/// `[processor.checker.pylint.core]` produces one with the same `pname` and
/// `instance_name="processor.checker.pylint.core"`.
#[derive(Debug, Clone)]
pub struct ProcessorInstance {
    /// Instance name: the pname for a single instance, `pname.<name>` for a named one.
    pub instance_name: String,
    /// The processor's full name, `processor.<type>.<name>`.
    pub pname: String,
    /// The raw TOML config for this instance (deserialized lazily per processor type)
    pub config_toml: toml::Value,
    /// Source of every field in `config_toml` (user TOML, processor default, scan default, …).
    pub provenance: ProvenanceMap,
}

/// The plugin for a processor name (`processor.checker.ruff`); an instance
/// name finds the same plugin.
pub fn find_registry_entry(pname: &str) -> Option<&'static ProcessorPlugin> {
    registry::find_plugin(pname)
}

/// Return all registered processor plugins.
pub fn registry_entries() -> impl Iterator<Item = &'static ProcessorPlugin> {
    registry::all_plugins()
}

/// Check if a name is a registered processor (`processor.<type>.<name>`).
pub fn is_builtin_type(name: &str) -> bool {
    find_registry_entry(name).is_some()
}

/// Seed a provenance map with every top-level key currently present in `value`,
/// marking each as originating from the user's TOML. Defaults applied afterwards
/// will see these entries and not overwrite them.
///
/// Line numbers are filled in later by the `toml_edit` span pass; until then we
/// use line 0 as a sentinel meaning "from user TOML, line unknown".
pub fn seed_user_provenance(value: &toml::Value) -> ProvenanceMap {
    let mut map = ProvenanceMap::new();
    if let Some(table) = value.as_table() {
        for key in table.keys() {
            map.insert(key.clone(), FieldProvenance::UserToml { line: 0 });
        }
    }
    map
}

/// Resolve scan and processor defaults for an instance config in-place.
pub fn resolve_instance_defaults(
    type_name: &str,
    value: &mut toml::Value,
    provenance: &mut ProvenanceMap,
) {
    if find_registry_entry(type_name).is_some() {
        registry::apply_all_defaults(type_name, value, provenance);
    }
}

impl ProcessorConfig {
    /// Collect unique scan directories from all declared instances.
    pub(crate) fn src_dirs(&self) -> Vec<String> {
        let mut dirs: Vec<String> = self
            .instances
            .iter()
            .flat_map(|inst| {
                inst.config_toml
                    .get("src_dirs")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flat_map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(std::string::ToString::to_string))
                    })
                    .filter(|d| !d.is_empty())
            })
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    /// Return known fields for a builtin processor type, or None for Lua plugins.
    pub(crate) fn known_fields_for(type_name: &str) -> Option<Vec<&'static str>> {
        let e = find_registry_entry(type_name)?;
        // Standard fields (minus this processor's omissions) plus the
        // plugin's own FieldSpec names. A spec sharing a standard field's
        // name replaces it, so the dedup keeps one copy.
        let mut fields: Vec<&'static str> = StandardConfig::known_fields()
            .iter()
            .copied()
            .filter(|f| !e.omit_standard_fields.contains(f))
            .collect();
        for spec in e.fields {
            if !fields.contains(&spec.name) {
                fields.push(spec.name);
            }
        }
        Some(fields)
    }

    /// Return checksum-affecting fields for a builtin processor type, or None for Lua plugins.
    pub(crate) fn checksum_fields_for(type_name: &str) -> Option<Vec<&'static str>> {
        let e = find_registry_entry(type_name)?;
        // A spec that shadows a standard field owns its checksum membership.
        let shadowed = |f: &&'static str| e.fields.iter().any(|s| s.name == *f);
        let mut fields: Vec<&'static str> = StandardConfig::checksum_fields()
            .iter()
            .copied()
            .filter(|f| !e.omit_standard_fields.contains(f))
            .filter(|f| !shadowed(f))
            .collect();
        for spec in e.fields {
            if spec.affects_output {
                fields.push(spec.name);
            }
        }
        Some(fields)
    }

    /// Return must fields (required non-empty fields) for a builtin processor type, or None for Lua plugins.
    pub(crate) fn must_fields_for(type_name: &str) -> Option<Vec<&'static str>> {
        let e = find_registry_entry(type_name)?;
        Some(
            e.fields
                .iter()
                .filter(|s| s.required)
                .map(|s| s.name)
                .collect(),
        )
    }

    /// Return (field, description) pairs for a builtin processor type, or None for Lua plugins.
    /// Standard-field descriptions come from `StandardConfig` (minus omissions,
    /// minus shadowed); the plugin's spec docs follow.
    pub(crate) fn field_descriptions_for(
        type_name: &str,
    ) -> Option<Vec<(&'static str, &'static str)>> {
        let e = find_registry_entry(type_name)?;
        let shadowed = |f: &str| e.fields.iter().any(|s| s.name == f);
        let mut descs: Vec<(&'static str, &'static str)> = StandardConfig::field_descriptions()
            .iter()
            .copied()
            .filter(|(f, _)| !e.omit_standard_fields.contains(f) && !shadowed(f))
            .collect();
        for spec in e.fields {
            descs.push((spec.name, spec.doc));
        }
        Some(descs)
    }

    /// Return the default config for a processor type as pretty JSON, or None if unknown.
    pub(crate) fn defconfig_json(type_name: &str) -> Option<String> {
        let entry = find_registry_entry(type_name)?;
        // The defaults machinery is keyed by the full name; the plugin's
        // short `name` finds nothing there and yields an all-empty config.
        (entry.defconfig_json)(&entry.pname())
    }
}

/// Return scan defaults for a builtin processor type. The data lives on the
/// processor's own plugin entry (`ProcessorPlugin.scan_defaults`) — this used
/// to be a 92-arm name-keyed match table here, which every new processor had
/// to remember to extend (and `prettier` once shipped without its arm,
/// silently mis-defaulted).
pub fn scan_defaults_for(type_name: &str) -> Option<ScanDefaultsData> {
    find_registry_entry(type_name)?.scan_defaults
}

/// Return per-processor default values (command, `dep_auto`, batch override).
/// Only needed for processors whose defaults differ from the struct's Default impl.
pub fn processor_defaults_for(type_name: &str) -> Option<ProcessorDefaults> {
    // Data lives on the plugin entry — see `scan_defaults_for`'s note; this
    // was the second of the two hand-synchronized default tables (V6's marp
    // args divergence was between this table and MarpConfig::default()).
    find_registry_entry(type_name)?.defaults
}

/// Apply processor-specific defaults to a config TOML value.
/// Sets command and `dep_auto` if they weren't explicitly provided by the user.
/// Every field that's actually injected is recorded in `provenance` as a processor default.
/// Fields already present in `provenance` (i.e. user-set) are skipped.
pub fn apply_processor_defaults(
    type_name: &str,
    value: &mut toml::Value,
    provenance: &mut ProvenanceMap,
) {
    let Some(defaults) = processor_defaults_for(type_name) else {
        return;
    };
    let Some(table) = value.as_table_mut() else {
        return;
    };
    set_string_default(
        table,
        "command",
        defaults.command,
        provenance,
        FieldProvenance::ProcessorDefault,
    );
    set_string_default(
        table,
        "output_dir",
        defaults.output_dir,
        provenance,
        FieldProvenance::ProcessorDefault,
    );
    set_string_array_default(
        table,
        "dep_auto",
        defaults.dep_auto,
        provenance,
        FieldProvenance::ProcessorDefault,
    );
    set_string_array_default(
        table,
        "formats",
        defaults.formats,
        provenance,
        FieldProvenance::ProcessorDefault,
    );
    set_string_array_default(
        table,
        "args",
        defaults.args,
        provenance,
        FieldProvenance::ProcessorDefault,
    );
    if let Some(batch) = defaults.batch
        && !table.contains_key("batch")
    {
        table.insert("batch".into(), toml::Value::Boolean(batch));
        provenance::record_if_absent(provenance, "batch", FieldProvenance::ProcessorDefault);
    }
}

fn set_string_default(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    val: &str,
    provenance: &mut ProvenanceMap,
    source: FieldProvenance,
) {
    if !val.is_empty() && !table.contains_key(key) {
        table.insert(key.into(), toml::Value::String(val.into()));
        provenance::record_if_absent(provenance, key, source);
    }
}

fn set_string_array_default(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    vals: &[&str],
    provenance: &mut ProvenanceMap,
    source: FieldProvenance,
) {
    if !vals.is_empty() && !table.contains_key(key) {
        let arr: Vec<toml::Value> = vals
            .iter()
            .map(|s| toml::Value::String(s.to_string()))
            .collect();
        table.insert(key.into(), toml::Value::Array(arr));
        provenance::record_if_absent(provenance, key, source);
    }
}

fn set_empty_array_default(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    provenance: &mut ProvenanceMap,
    source: FieldProvenance,
) {
    if !table.contains_key(key) {
        table.insert(key.into(), toml::Value::Array(Vec::new()));
        provenance::record_if_absent(provenance, key, source);
    }
}

fn set_maybe_empty_array_default(
    table: &mut toml::map::Map<String, toml::Value>,
    key: &str,
    vals: &[&str],
    provenance: &mut ProvenanceMap,
    source: FieldProvenance,
) {
    if !table.contains_key(key) {
        let arr: Vec<toml::Value> = vals
            .iter()
            .map(|s| toml::Value::String(s.to_string()))
            .collect();
        table.insert(key.into(), toml::Value::Array(arr));
        provenance::record_if_absent(provenance, key, source);
    }
}

/// Apply scan defaults to a config TOML value.
/// Sets `src_dirs`, `src_extensions`, and `src_exclude_dirs` if not explicitly provided.
/// Every field that's actually injected is recorded in `provenance` as a scan default.
pub fn apply_scan_defaults(
    type_name: &str,
    value: &mut toml::Value,
    provenance: &mut ProvenanceMap,
) {
    let Some(defaults) = scan_defaults_for(type_name) else {
        return;
    };
    let Some(table) = value.as_table_mut() else {
        return;
    };
    set_maybe_empty_array_default(
        table,
        "src_dirs",
        defaults.src_dirs,
        provenance,
        FieldProvenance::ScanDefault,
    );
    set_maybe_empty_array_default(
        table,
        "src_extensions",
        defaults.src_extensions,
        provenance,
        FieldProvenance::ScanDefault,
    );
    set_maybe_empty_array_default(
        table,
        "src_exclude_dirs",
        defaults.src_exclude_dirs,
        provenance,
        FieldProvenance::ScanDefault,
    );
    set_empty_array_default(
        table,
        "src_exclude_files",
        provenance,
        FieldProvenance::ScanDefault,
    );
    set_empty_array_default(
        table,
        "src_exclude_paths",
        provenance,
        FieldProvenance::ScanDefault,
    );
    set_empty_array_default(table, "src_files", provenance, FieldProvenance::ScanDefault);
}

/// Processor configuration: a collection of declared processor instances.
/// Each `[processor.TYPE]` or `[processor.TYPE.NAME]` section in rsconstruct.toml
/// creates a `ProcessorInstance`. No instances exist by default — only what's declared.
#[derive(Debug, Default)]
pub struct ProcessorConfig {
    /// All declared processor instances
    pub instances: Vec<ProcessorInstance>,
    /// Lua plugin configs (processor types not in the builtin registry)
    pub extra: HashMap<String, toml::Value>,
}

impl Serialize for ProcessorConfig {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        // Rebuild the `[processor]` tree the instances were read from:
        // processor → type → name → (fields | instance → fields).
        let mut root = toml::map::Map::new();
        for inst in &self.instances {
            let Some(parsed) = registry::parse_name(&inst.instance_name) else {
                continue;
            };
            let type_table = root
                .entry(parsed.processor_type.as_str().to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
            let Some(type_table) = type_table.as_table_mut() else {
                continue;
            };
            match parsed.instance {
                None => {
                    type_table.insert(parsed.short.to_string(), inst.config_toml.clone());
                }
                Some(instance) => {
                    let name_table = type_table
                        .entry(parsed.short.to_string())
                        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
                    if let Some(name_table) = name_table.as_table_mut() {
                        name_table.insert(instance.to_string(), inst.config_toml.clone());
                    }
                }
            }
        }
        if !self.extra.is_empty() {
            let lua = root
                .entry(crate::processor::ProcessorType::Lua.as_str().to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
            if let Some(lua) = lua.as_table_mut() {
                for (name, value) in &self.extra {
                    lua.insert(name.clone(), value.clone());
                }
            }
        }
        toml::Value::Table(root).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ProcessorConfig {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let table = toml::Value::deserialize(deserializer)?;
        Ok(Self::from_toml(&table))
    }
}

/// What a `[processor.TYPE.NAME]` section turned out to be.
///
/// The shape is inferred from the section's contents, so it can be
/// genuinely undecidable — which is why this is an enum rather than a bool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionShape {
    /// Direct config fields: `[processor.checker.pylint]` with `args = [...]`.
    SingleInstance,
    /// Named sub-instances: `[processor.checker.pylint.core]`, `[processor.checker.pylint.tests]`.
    MultiInstance,
    /// Reads as both. Carries the keys that are simultaneously known config
    /// field names and plausible instance names.
    Ambiguous { colliding: Vec<String> },
}

impl ProcessorConfig {
    /// Parse the `[processor]` table from TOML into instances.
    pub(crate) fn from_toml(value: &toml::Value) -> Self {
        let Some(table) = value.as_table() else {
            return Self::default();
        };

        let mut instances = Vec::new();
        let mut extra = HashMap::new();

        // The tree is processor → type → name → (fields | instances). Anything
        // that does not fit is skipped here and reported by
        // `validate_processor_fields_raw`, which runs on the same raw value.
        for (type_key, type_val) in table {
            let Some(type_table) = type_val.as_table() else {
                continue;
            };
            let Some(processor_type) = crate::processor::ProcessorType::parse(type_key) else {
                continue;
            };
            for (short, val) in type_table {
                let Some(sub_table) = val.as_table() else {
                    continue;
                };
                if processor_type == crate::processor::ProcessorType::Lua {
                    // Lua plugins are keyed by the plugin file's stem.
                    extra.insert(short.clone(), val.clone());
                    continue;
                }
                let pname = format!("{}{type_key}.{short}", registry::NAME_PREFIX);
                if !is_builtin_type(&pname) {
                    continue;
                }
                if Self::is_multi_instance(&pname, sub_table) {
                    // Multi-instance: [processor.checker.pylint.core], [processor.checker.pylint.tests]
                    for (name, inst_val) in sub_table {
                        let instance_name = format!("{pname}.{name}");
                        let mut config = inst_val.clone();
                        let mut provenance = seed_user_provenance(&config);
                        resolve_instance_defaults(&pname, &mut config, &mut provenance);
                        instances.push(ProcessorInstance {
                            instance_name,
                            pname: pname.clone(),
                            config_toml: config,
                            provenance,
                        });
                    }
                } else {
                    // Single instance: [processor.checker.pylint]
                    let mut config = val.clone();
                    let mut provenance = seed_user_provenance(&config);
                    resolve_instance_defaults(&pname, &mut config, &mut provenance);
                    instances.push(ProcessorInstance {
                        instance_name: pname.clone(),
                        pname,
                        config_toml: config,
                        provenance,
                    });
                }
            }
        }

        Self { instances, extra }
    }

    /// Determine if a processor section contains named sub-instances (multi-instance)
    /// or direct config fields (single-instance).
    ///
    /// Heuristic: if ALL values are tables AND none of the keys match known config
    /// field names for this processor type, it's multi-instance. An ambiguous
    /// section is treated as single-instance here; `parse_processors` rejects
    /// it outright, so this only affects callers that have already validated.
    fn is_multi_instance(type_name: &str, table: &toml::map::Map<String, toml::Value>) -> bool {
        matches!(
            Self::classify_section(type_name, table),
            SectionShape::MultiInstance
        )
    }

    /// The shape of a `[processor.TYPE.NAME]` section, and whether it is even
    /// decidable.
    ///
    /// Separated from `is_multi_instance` so the ambiguous case can be
    /// reported rather than silently resolved. The heuristic reads a
    /// section's *shape* to guess the user's intent, which means a section
    /// that looks like both is a genuine ambiguity — and resolving it
    /// quietly (as returning a bare bool forced) is how a config could
    /// change meaning under the user without warning.
    pub(super) fn classify_section(
        type_name: &str,
        table: &toml::map::Map<String, toml::Value>,
    ) -> SectionShape {
        if table.is_empty() {
            return SectionShape::SingleInstance;
        }

        let Some(known) = Self::known_fields_for(type_name) else {
            // Unknown type (Lua plugin): no field list to compare against, so
            // multi-instance is undecidable and the section is taken as one
            // instance. See `multi_instance_requires_known_fields` in the docs.
            return SectionShape::SingleInstance;
        };
        let known_fields: Vec<&str> = known
            .iter()
            .chain(SCAN_CONFIG_FIELDS.iter())
            .chain(STANDARD_EXTRA_FIELDS.iter())
            .copied()
            .collect();

        let field_keys: Vec<&str> = table
            .keys()
            .filter(|k| known_fields.contains(&k.as_str()))
            .map(String::as_str)
            .collect();
        let all_values_are_tables = table.values().all(toml::Value::is_table);

        match (field_keys.is_empty(), all_values_are_tables) {
            // Only sub-tables, no known field names: unambiguously instances.
            (true, true) => SectionShape::MultiInstance,
            // At least one known field name present.
            (false, _) => {
                // A known field whose value is a table, alongside nothing but
                // tables, is the ambiguous case: it reads as both "a config
                // field" and "an instance named after a field".
                if all_values_are_tables {
                    SectionShape::Ambiguous {
                        colliding: field_keys.iter().map(|s| (*s).to_string()).collect(),
                    }
                } else {
                    SectionShape::SingleInstance
                }
            }
            // Scalar values present and no known fields: malformed, but
            // validation reports unknown fields far better than we could.
            (true, false) => SectionShape::SingleInstance,
        }
    }

    /// Resolve scan defaults for all instances.
    pub(crate) fn resolve_scan_defaults(&mut self) {
        for inst in &mut self.instances {
            resolve_instance_defaults(&inst.pname, &mut inst.config_toml, &mut inst.provenance);
        }
    }

    /// Rewrite output paths in processor configs:
    /// 1. For named instances (e.g., `pylint.core`), remap default `out/{type_name}`
    ///    to `out/{instance_name}` so each instance gets its own output directory.
    /// 2. If the global `output_dir` is not "out", replace the `out/` prefix with the
    ///    global value (e.g., `build/` → `build/marp`).
    ///
    /// When a field was originally a `ProcessorDefault` and gets rewritten here, its
    /// provenance is upgraded to `OutputDirDefault` so `config show` can explain why
    /// the value differs from the processor's own default.
    pub(crate) fn apply_output_dir_defaults(&mut self, global_output_dir: &str) {
        for inst in &mut self.instances {
            let type_default_prefix = format!("out/{}", inst.pname);
            let instance_prefix = format!("{}/{}", global_output_dir, inst.instance_name);

            for field in &["output_dir", "output"] {
                // A user-set or CLI-set value is explicitly chosen — never
                // rewrite it (previously only the provenance label was
                // guarded while the value itself was still rewritten).
                if matches!(
                    inst.provenance.get(*field),
                    Some(
                        FieldProvenance::UserToml { .. }
                            | FieldProvenance::LocalToml { .. }
                            | FieldProvenance::CliOverride
                    ),
                ) {
                    continue;
                }
                let Some(val) = inst
                    .config_toml
                    .get(field)
                    .and_then(|v| v.as_str())
                    .map(std::string::ToString::to_string)
                else {
                    continue;
                };
                // Prefix matches must respect path boundaries: `out/marp`
                // must not match `out/marpdeck/...`.
                let type_rest = val
                    .strip_prefix(&type_default_prefix)
                    .filter(|r| r.is_empty() || r.starts_with('/'));
                let new_val = if inst.instance_name != inst.pname
                    && let Some(rest) = type_rest
                {
                    // Named instance: remap out/{type} → {global}/{instance}
                    format!("{instance_prefix}{rest}")
                } else if global_output_dir != "out"
                    && let Some(rest) = val.strip_prefix("out/")
                {
                    // Global output dir override: remap out/ → {global}/
                    format!("{global_output_dir}/{rest}")
                } else {
                    continue;
                };
                if let Some(table) = inst.config_toml.as_table_mut() {
                    table.insert(field.to_string(), toml::Value::String(new_val));
                }
                inst.provenance
                    .insert((*field).to_string(), FieldProvenance::OutputDirDefault);
            }
        }
    }

    /// Get the first instance of a given type (for single-instance access).
    /// Returns None if no instance of that type is declared.
    pub(crate) fn first_instance_of_type(&self, type_name: &str) -> Option<&ProcessorInstance> {
        self.instances.iter().find(|i| i.pname == type_name)
    }

    /// Deserialize the config for `type_name`, whether or not the user
    /// declared the instance.
    ///
    /// A declared instance already has registry defaults baked into its
    /// `config_toml` by `Config::load`. An undeclared one has nothing, and
    /// `C::default()` leaves scan fields unresolved — reaching a scan
    /// accessor then panics with "scan fields not resolved". So the
    /// undeclared case is routed through the same `apply_all_defaults` the
    /// declared case used, instead of each caller hand-resolving.
    pub(crate) fn instance_config_or_default<C: serde::de::DeserializeOwned>(
        &self,
        type_name: &str,
    ) -> Result<C> {
        if let Some(inst) = self.first_instance_of_type(type_name) {
            return inst
                .config_toml
                .clone()
                .try_into()
                .with_context(|| format!("Failed to parse [{type_name}] config"));
        }
        let mut value = toml::Value::Table(toml::map::Map::new());
        let mut provenance = ProvenanceMap::new();
        crate::registries::processor::apply_all_defaults(type_name, &mut value, &mut provenance);
        value
            .try_into()
            .with_context(|| format!("Failed to build default config for processor '{type_name}'"))
    }

    /// Get a typed config value from an instance's TOML config.
    /// Returns the default if the instance is not declared or the field is missing.
    pub(crate) fn instance_field_str(&self, type_name: &str, field: &str) -> Option<String> {
        self.first_instance_of_type(type_name)
            .and_then(|inst| inst.config_toml.get(field))
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string)
    }
}

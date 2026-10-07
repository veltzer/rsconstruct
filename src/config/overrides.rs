//! `--iset`/`--pset` command-line overrides: parsing `<name>.<field>=<value>`
//! and applying it, type-checked, to the matching processor instances.

use anyhow::Result;

use super::{
    FieldProvenance, FieldType, ProcessorConfig, ProcessorInstance, SCAN_CONFIG_FIELDS,
    STANDARD_EXTRA_FIELDS, expected_field_type,
};

/// Parse a single `--iset`/`--pset` entry of the form `<name>.<field>=<value>`.
/// Returns `(name, field, parsed_value)`. The value is parsed as a TOML scalar/
/// array/table; if parsing fails it is treated as a bare string. Hard-errors on
/// missing dot, missing equals, or empty name/field.
pub(super) fn parse_override_entry<'a>(
    raw: &'a str,
    flag: &str,
) -> Result<(&'a str, &'a str, toml::Value)> {
    let (lhs, value_str) = raw.split_once('=').ok_or_else(|| {
        anyhow::anyhow!("{flag} '{raw}': missing '=' (expected <name>.<field>=<value>)")
    })?;
    // Split on the LAST dot: instance names may contain dots (`pylint.core`)
    // while field names never do.
    let (name, field) = lhs.rsplit_once('.').ok_or_else(|| {
        anyhow::anyhow!(
            "{flag} '{raw}': missing '.' between name and field (expected <name>.<field>=<value>)"
        )
    })?;
    if name.is_empty() {
        anyhow::bail!("{flag} '{raw}': empty name before '.'");
    }
    if field.is_empty() {
        anyhow::bail!("{flag} '{raw}': empty field between '.' and '='");
    }
    // Parse as a TOML value via a synthetic "v = <value>" doc; fall back to a
    // bare string so users can write `--iset marp.command=marp` without quoting.
    let parsed: toml::Value = match toml::from_str::<toml::Value>(&format!("v = {value_str}")) {
        Ok(toml::Value::Table(mut t)) => t
            .remove("v")
            .unwrap_or_else(|| toml::Value::String(value_str.to_string())),
        _ => toml::Value::String(value_str.to_string()),
    };
    Ok((name, field, parsed))
}

/// Apply an override to every instance matching `predicate`. Hard-errors when
/// no instances match, when the field is unknown for the processor type, or
/// when the value's TOML type doesn't match the field's declared type.
pub(super) fn apply_override_to_instances(
    instances: &mut [ProcessorInstance],
    field: &str,
    value: &toml::Value,
    predicate: impl Fn(&ProcessorInstance) -> bool,
    name_kind: &str,
    name: &str,
) -> Result<()> {
    let matching_indices: Vec<usize> = instances
        .iter()
        .enumerate()
        .filter(|(_, inst)| predicate(inst))
        .map(|(i, _)| i)
        .collect();
    if matching_indices.is_empty() {
        anyhow::bail!("no processor instance with {name_kind} '{name}'");
    }
    for i in matching_indices {
        let inst = &mut instances[i];
        let type_name = inst.pname.clone();
        validate_override_field(&type_name, field, value, &inst.instance_name)?;
        if let Some(table) = inst.config_toml.as_table_mut() {
            table.insert(field.to_string(), value.clone());
            inst.provenance
                .insert(field.to_string(), FieldProvenance::CliOverride);
        } else {
            anyhow::bail!(
                "instance '{}' config is not a table (cannot apply override)",
                inst.instance_name
            );
        }
    }
    Ok(())
}

/// Validate that `field` is a known config field for `type_name` and that
/// `value`'s TOML type matches the field's declared type.
fn validate_override_field(
    type_name: &str,
    field: &str,
    value: &toml::Value,
    instance_label: &str,
) -> Result<()> {
    let own_fields = ProcessorConfig::known_fields_for(type_name).unwrap_or_default();
    let is_known = own_fields.contains(&field)
        || SCAN_CONFIG_FIELDS.contains(&field)
        || STANDARD_EXTRA_FIELDS.contains(&field);
    if !is_known {
        let mut all_fields: Vec<&str> = own_fields
            .iter()
            .chain(SCAN_CONFIG_FIELDS.iter())
            .chain(STANDARD_EXTRA_FIELDS.iter())
            .copied()
            .collect();
        all_fields.sort_unstable();
        all_fields.dedup();
        anyhow::bail!(
            "instance '{instance_label}' (type {type_name}): unknown field '{field}' (valid fields: {})",
            all_fields.join(", ")
        );
    }
    if let Some(expected) = expected_field_type(type_name, field)
        && !expected.matches(value)
    {
        anyhow::bail!(
            "instance '{instance_label}' (type {type_name}): field '{field}' must be {}, got {} ({value})",
            expected.label(),
            FieldType::describe_value(value),
        );
    }
    Ok(())
}

//! Config validation: the raw-TOML schema checks run before deserializing
//! (unknown processors, analyzers and fields; field types; required
//! fields), and the semantic checks run on the loaded config.

use anyhow::Result;
use std::path::Path;

use super::{
    BuildConfig, DEFAULT_PLUGINS_DIR, FieldProvenance, ProcessorConfig, ProcessorInstance,
    SCAN_CONFIG_FIELDS, STANDARD_EXTRA_FIELDS, SectionShape, find_registry_entry, is_builtin_type,
};
use crate::registries::{self as registry, ProcessorPlugin};

/// Expected TOML type for a config field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    String,
    Bool,
    Integer,
    StringArray,
    /// Array of tables (e.g., [[`processor.cc_single_file.compilers`]])
    TableArray,
    /// Inline table (e.g., `tags.field_types` = { author = "string" })
    Table,
    /// Array with element types the validator doesn't constrain
    /// (e.g. `tags.required_field_groups` is an array of string arrays).
    Array,
}

impl FieldType {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::String => "a string",
            Self::Bool => "a boolean",
            Self::Integer => "an integer",
            Self::StringArray => "an array of strings",
            Self::TableArray => "an array of tables",
            Self::Table => "a table",
            Self::Array => "an array",
        }
    }

    /// Check whether a TOML value matches this expected type.
    pub(super) fn matches(self, value: &toml::Value) -> bool {
        match self {
            Self::String => value.is_str(),
            Self::Bool => value.as_bool().is_some(),
            Self::Integer => value.is_integer(),
            Self::StringArray => value
                .as_array()
                .is_some_and(|arr| arr.iter().all(toml::Value::is_str)),
            Self::TableArray => value
                .as_array()
                .is_some_and(|arr| arr.iter().all(toml::Value::is_table)),
            Self::Table => value.is_table(),
            Self::Array => value.is_array(),
        }
    }

    pub(super) const fn describe_value(value: &toml::Value) -> &'static str {
        match value {
            toml::Value::String(_) => "a string",
            toml::Value::Integer(_) => "an integer",
            toml::Value::Float(_) => "a float",
            toml::Value::Boolean(_) => "a boolean",
            toml::Value::Datetime(_) => "a datetime",
            toml::Value::Array(_) => "an array",
            toml::Value::Table(_) => "a table",
        }
    }
}

/// Return the expected TOML type for a processor config field.
/// Fields common to all processors (scan fields, enabled, args, `dep_inputs`)
/// are handled generically. Processor-specific fields are looked up by processor name.
pub(super) fn expected_field_type(processor: &str, field: &str) -> Option<FieldType> {
    // Scan fields — shared by all processors
    match field {
        "src_dirs" => return Some(FieldType::StringArray),
        "src_extensions" => return Some(FieldType::StringArray),
        "src_exclude_dirs" => return Some(FieldType::StringArray),
        "src_exclude_files" => return Some(FieldType::StringArray),
        "src_exclude_paths" => return Some(FieldType::StringArray),
        "src_files" => return Some(FieldType::StringArray),
        // Common processor fields
        "args" => return Some(FieldType::StringArray),
        "dep_inputs" => return Some(FieldType::StringArray),
        "dep_auto" => return Some(FieldType::StringArray),
        "max_jobs" => return Some(FieldType::Integer),
        "enabled" => return Some(FieldType::Bool),
        "batch" => return Some(FieldType::Bool),
        // Near-universal fields from StandardConfig — declared by almost
        // every processor, so they are validated generically rather than
        // repeated in every processor-specific arm below.
        "command" => return Some(FieldType::String),
        "output_dir" => return Some(FieldType::String),
        "formats" => return Some(FieldType::StringArray),
        _ => {}
    }

    // Processor-specific fields: the type comes from the processor's own
    // FieldSpec list (a ~120-arm (processor, field) match table used to
    // live here — dead arms in it were provably invisible to the tests).
    find_registry_entry(processor)?
        .fields
        .iter()
        .find(|s| s.name == field)
        .map(|s| s.ty)
}

/// Validate fields in a single processor config table.
pub(super) fn validate_single_processor(
    type_name: &str,
    section_label: &str,
    table: &toml::map::Map<String, toml::Value>,
    errors: &mut Vec<String>,
) {
    let own_fields: Vec<&'static str> = match ProcessorConfig::known_fields_for(type_name) {
        Some(fields) => fields,
        None => return, // unknown = Lua plugin, skip
    };

    // A zero-permit semaphore can never grant a permit: max_jobs = 0 would
    // deadlock the build waiting forever, so reject it at load time.
    if table.get("max_jobs").and_then(toml::Value::as_integer) == Some(0) {
        errors.push(format!(
            "[{section_label}]: 'max_jobs' must be greater than 0 (use 'enabled = false' to turn the processor off)",
        ));
    }

    for (key, field_value) in table {
        if !own_fields.contains(&key.as_str())
            && !SCAN_CONFIG_FIELDS.contains(&key.as_str())
            && !STANDARD_EXTRA_FIELDS.contains(&key.as_str())
        {
            let all_fields: Vec<&str> = own_fields
                .iter()
                .chain(SCAN_CONFIG_FIELDS.iter())
                .chain(STANDARD_EXTRA_FIELDS.iter())
                .copied()
                .collect();
            errors.push(format!(
                "[{}]: unknown field '{}' (valid fields: {})",
                section_label,
                key,
                all_fields.join(", ")
            ));
            continue;
        }

        if let Some(expected) = expected_field_type(type_name, key)
            && !expected.matches(field_value)
        {
            errors.push(format!(
                "[{}]: field '{}' must be {}, got {} ({})",
                section_label,
                key,
                expected.label(),
                FieldType::describe_value(field_value),
                field_value,
            ));
        }
    }

    // Check that all must_fields are present and non-empty
    if let Some(must) = ProcessorConfig::must_fields_for(type_name) {
        for field in &must {
            match table.get(*field) {
                None => {
                    errors.push(format!(
                        "[{section_label}]: required field '{field}' must be specified",
                    ));
                }
                Some(toml::Value::Array(arr)) if arr.is_empty() => {
                    errors.push(format!(
                        "[{section_label}]: required field '{field}' must not be empty",
                    ));
                }
                Some(toml::Value::String(s)) if s.is_empty() => {
                    errors.push(format!(
                        "[{section_label}]: required field '{field}' must not be empty",
                    ));
                }
                _ => {} // present and non-empty: OK
            }
        }
    }
}

/// Range-check `[build]` values whose zero cases silently break the build:
/// `max_discovery_passes = 0` skips discovery entirely and reports an empty
/// build as success; `max_arg_len = 0` degenerates every batch to one file
/// per process spawn. (`parallel = 0` and `batch_size = 0` are documented
/// sentinels and stay valid.) Same message shape as the per-processor
/// `max_jobs != 0` guard.
pub(super) fn validate_build_config(build: &BuildConfig) -> Result<()> {
    if build.max_discovery_passes == 0 {
        return Err(crate::exit_code::config_error(
            "[build] max_discovery_passes must be at least 1 — 0 would skip discovery entirely and report an empty build as success".to_string()));
    }
    if build.max_arg_len == 0 {
        return Err(crate::exit_code::config_error(
            "[build] max_arg_len must be at least 1 — 0 would split every batch into one file per tool invocation".to_string()));
    }
    Ok(())
}

/// Every `dep_auto` entry the user listed must exist on disk.
///
/// Only user-set lists are checked (main config, local overlay, or a CLI
/// override): a processor's default `dep_auto` (`.pylintrc`, `ruff.toml`,
/// ...) is optional by nature and stays skip-if-absent, and entries a
/// processor computes at discovery time never pass through here. Disabled
/// instances are skipped, as they are everywhere else. `[build]
/// allow_missing_dep_auto = true` turns the check off. Runs after the span
/// maps are applied so the message can point at the config line.
pub(super) fn validate_dep_auto_exist(
    instances: &[ProcessorInstance],
    build: &BuildConfig,
) -> Result<()> {
    if build.allow_missing_dep_auto {
        return Ok(());
    }
    let mut errors = Vec::new();
    for inst in instances {
        let Some(source) = inst.provenance.get("dep_auto") else {
            continue;
        };
        let user_set = matches!(
            source,
            FieldProvenance::UserToml { .. }
                | FieldProvenance::LocalToml { .. }
                | FieldProvenance::CliOverride
        );
        if !user_set {
            continue;
        }
        let disabled = inst
            .config_toml
            .get("enabled")
            .and_then(toml::Value::as_bool)
            == Some(false);
        if disabled {
            continue;
        }
        let Some(entries) = inst
            .config_toml
            .get("dep_auto")
            .and_then(toml::Value::as_array)
        else {
            continue;
        };
        for entry in entries.iter().filter_map(toml::Value::as_str) {
            if !Path::new(entry).exists() {
                errors.push(format!(
                    "  [{}] dep_auto file not found: {entry} ({source})",
                    inst.instance_name
                ));
            }
        }
    }
    if errors.is_empty() {
        return Ok(());
    }
    Err(crate::exit_code::config_error(format!(
        "Invalid config:\n{}\nEvery dep_auto entry listed in the config must exist — remove the entry, \
         or set [build] allow_missing_dep_auto = true to skip absent entries as before",
        errors.join("\n")
    )))
}

/// With `[build] reject_dot_src_dirs = true`, no enabled instance may list
/// `"."` in `src_dirs`. `"."` and `""` both mean the whole project tree; the
/// switch exists for projects that want that spelled out — as `""`, which
/// the reference documents as the deliberate whole-tree form — or, better,
/// replaced by the directories the stanza actually covers. Runs after the
/// span maps are applied so the message can point at the config line.
pub(super) fn validate_no_dot_src_dirs(
    instances: &[ProcessorInstance],
    build: &BuildConfig,
) -> Result<()> {
    if !build.reject_dot_src_dirs {
        return Ok(());
    }
    let mut errors = Vec::new();
    for inst in instances {
        let disabled = inst
            .config_toml
            .get("enabled")
            .and_then(toml::Value::as_bool)
            == Some(false);
        if disabled {
            continue;
        }
        let Some(entries) = inst
            .config_toml
            .get("src_dirs")
            .and_then(toml::Value::as_array)
        else {
            continue;
        };
        if entries
            .iter()
            .filter_map(toml::Value::as_str)
            .any(|e| e == ".")
        {
            let source = inst
                .provenance
                .get("src_dirs")
                .map_or_else(String::new, |s| format!(" ({s})"));
            errors.push(format!(
                "  [{}] src_dirs contains \".\"{source}",
                inst.instance_name
            ));
        }
    }
    if errors.is_empty() {
        return Ok(());
    }
    Err(crate::exit_code::config_error(format!(
        "Invalid config:\n{}\n[build] reject_dot_src_dirs is on: \".\" means the whole project tree, \
         the same as \"\" — name the directories the stanza covers, or write \"\" if sweeping \
         the tree is intended",
        errors.join("\n")
    )))
}

/// Validate that all fields in `[processor.X]` sections are known fields for that processor
/// and have the correct TOML types. Supports both single-instance and multi-instance formats.
/// Returns a list of error strings (empty if valid). `Config::load` combines this with the
/// analyzer validator output under a single "Invalid config:" header.
pub(super) fn validate_processor_fields_raw(raw: &toml::Value) -> Vec<String> {
    let Some(processor_table) = raw.get("processor").and_then(|v| v.as_table()) else {
        return Vec::new();
    };

    let mut errors = Vec::new();
    let type_names = {
        use strum::IntoEnumIterator;
        crate::processor::ProcessorType::iter()
            .map(crate::processor::ProcessorType::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    };

    for (type_key, type_value) in processor_table {
        // A scalar under [processor] (e.g. `checker = true`) is a config
        // mistake that would otherwise be silently dropped.
        let Some(type_table) = type_value.as_table() else {
            errors.push(format!(
                "[processor]: '{type_key}' must be a table of processors (e.g. [processor.{type_key}.NAME]), got {}",
                FieldType::describe_value(type_value),
            ));
            continue;
        };
        let Some(processor_type) = crate::processor::ProcessorType::parse(type_key) else {
            // The most likely cause is a pre-rename config: `[processor.ruff]`
            // where `[processor.checker.ruff]` is meant. Say so when the
            // key names a processor of some type.
            let hint = registry::all_plugins()
                .find(|p| p.name == type_key)
                .map(|p| format!(" — did you mean [{}]?", p.pname()))
                .unwrap_or_default();
            errors.push(format!(
                "[processor.{type_key}]: unknown processor type '{type_key}' (processors are named \
                 processor.<type>.<name>; types: {type_names}){hint}",
            ));
            continue;
        };

        for (short, value) in type_table {
            let Some(table) = value.as_table() else {
                errors.push(format!(
                    "[processor.{type_key}]: '{short}' must be a section (e.g. [processor.{type_key}.{short}]), got {}",
                    FieldType::describe_value(value),
                ));
                continue;
            };
            let pname = format!("{}{type_key}.{short}", registry::NAME_PREFIX);

            if processor_type == crate::processor::ProcessorType::Lua {
                // A Lua plugin is a file in the plugins directory.
                let plugins_dir = raw
                    .get("plugins")
                    .and_then(|p| p.get("dir"))
                    .and_then(|d| d.as_str())
                    .unwrap_or(DEFAULT_PLUGINS_DIR);
                let plugin_path = std::path::Path::new(plugins_dir).join(format!("{short}.lua"));
                if !plugin_path.exists() {
                    errors.push(format!(
                        "[{pname}]: no Lua plugin at {}",
                        plugin_path.display(),
                    ));
                }
                continue;
            }

            if !is_builtin_type(&pname) {
                let elsewhere: Vec<String> = registry::all_plugins()
                    .filter(|p| p.name == short)
                    .map(ProcessorPlugin::pname)
                    .collect();
                let hint = if elsewhere.is_empty() {
                    format!(" (run `rsconstruct processor list --type {type_key}`)")
                } else {
                    format!(" — did you mean [{}]?", elsewhere.join("] or ["))
                };
                errors.push(format!(
                    "[{pname}]: there is no {type_key} processor named '{short}'{hint}",
                ));
                continue;
            }

            // Check if multi-instance
            match ProcessorConfig::classify_section(&pname, table) {
                SectionShape::MultiInstance => {
                    for (inst_name, inst_value) in table {
                        if let Some(inst_table) = inst_value.as_table() {
                            let section = format!("{pname}.{inst_name}");
                            validate_single_processor(&pname, &section, inst_table, &mut errors);
                        }
                    }
                }
                SectionShape::SingleInstance => {
                    validate_single_processor(&pname, &pname, table, &mut errors);
                }
                // The section reads as both a config-field table and a set of
                // named instances. Guessing here is what let a config silently
                // change meaning when a future release added a field whose name
                // matched an existing user's instance — so it is rejected
                // instead, naming the exact keys to rename.
                SectionShape::Ambiguous { colliding } => {
                    errors.push(format!(
                        "[{pname}]: ambiguous section — {} also {} a config field of \
                     '{pname}', so this could be read either as config or as named \
                     instance{}. Rename the instance{}, or move the config fields to \
                     [{pname}] and keep only instances as sub-tables.",
                        colliding
                            .iter()
                            .map(|c| format!("'{c}'"))
                            .collect::<Vec<_>>()
                            .join(", "),
                        if colliding.len() == 1 {
                            "names"
                        } else {
                            "name"
                        },
                        if colliding.len() == 1 { "" } else { "s" },
                        if colliding.len() == 1 { "" } else { "s" },
                    ));
                }
            }
        }
    }

    errors
}

/// Validate `[analyzer.X]` sections: reject unknown analyzer types and unknown
/// fields within each section. Runs at config-load time, before any analyzer
/// is instantiated, so users see schema errors up front instead of at build
/// time. Returns a list of error strings (empty if valid). `Config::load`
/// combines this with the processor validator output under a single
/// "Invalid config:" header.
pub(super) fn validate_analyzer_fields_raw(raw: &toml::Value) -> Vec<String> {
    let Some(analyzer_table) = raw.get("analyzer").and_then(|v| v.as_table()) else {
        return Vec::new();
    };

    let mut errors = Vec::new();

    for (type_name, value) in analyzer_table {
        let Some(table) = value.as_table() else {
            errors.push(format!("[analyzer.{type_name}]: expected a table"));
            continue;
        };

        let Some(plugin) = registry::find_analyzer_plugin(type_name) else {
            errors.push(format!(
                "[analyzer.{type_name}]: unknown analyzer type '{type_name}' (run 'rsconstruct analyzer list' to see available)",
            ));
            continue;
        };

        // Multi-instance: `[analyzer.cpp.kernel]` / `[analyzer.cpp.userspace]`.
        // Detected iff every value is itself a table.
        let is_multi_instance = !table.is_empty() && table.values().all(toml::Value::is_table);

        if is_multi_instance {
            for (inst_name, inst_value) in table {
                if let Some(inst_table) = inst_value.as_table() {
                    let section = format!("analyzer.{type_name}.{inst_name}");
                    validate_analyzer_section(plugin, &section, inst_table, &mut errors);
                }
            }
        } else {
            let section = format!("analyzer.{type_name}");
            validate_analyzer_section(plugin, &section, table, &mut errors);
        }
    }

    errors
}

/// Check a single analyzer section's fields against the plugin's `known_fields` list.
fn validate_analyzer_section(
    plugin: &registry::AnalyzerPlugin,
    section_label: &str,
    table: &toml::map::Map<String, toml::Value>,
    errors: &mut Vec<String>,
) {
    let known = (plugin.known_fields)();
    for key in table.keys() {
        if !known.contains(&key.as_str()) {
            errors.push(format!(
                "[{}]: unknown field '{}' (valid fields: {})",
                section_label,
                key,
                known.join(", ")
            ));
        }
    }
}

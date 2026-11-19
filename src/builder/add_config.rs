use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fmt::Write;
use std::fs;

use crate::registries::{all_analyzer_plugins, find_plugin};

const CONFIG_FILE: &str = "rsconstruct.toml";

/// Add a `[processor.TYPE.NAME]` section to rsconstruct.toml, pre-populated with
/// must-fill fields and one-line `#` comments for every known field.
pub fn add_processor(pname: &str, dry_run: bool) -> Result<()> {
    let plugin =
        find_plugin(pname).ok_or_else(|| anyhow::anyhow!("Unknown processor '{pname}'"))?;

    let known: Vec<&str> =
        crate::config::ProcessorConfig::known_fields_for(pname).unwrap_or_default();
    let must: Vec<&str> =
        crate::config::ProcessorConfig::must_fields_for(pname).unwrap_or_default();
    let checksum_fields: Vec<&str> =
        crate::config::ProcessorConfig::checksum_fields_for(pname).unwrap_or_default();
    let mut descs: HashMap<&str, &str> =
        crate::config::ProcessorConfig::field_descriptions_for(pname)
            .unwrap_or_default()
            .into_iter()
            .collect();
    for (f, d) in crate::config::SHARED_FIELD_DESCRIPTIONS {
        descs.entry(f).or_insert(d);
    }
    for (f, d) in crate::config::SCAN_FIELD_DESCRIPTIONS {
        descs.entry(f).or_insert(d);
    }

    let defaults: serde_json::Value = (plugin.defconfig_json)(pname)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::default()));

    let description = processor_description(pname);

    // A processor name already carries its `processor.` prefix; the header
    // is the name itself.
    let snippet = render_section(
        pname,
        description.as_deref(),
        &known,
        &must,
        &checksum_fields,
        &defaults,
        &descs,
    );

    apply_snippet(pname, &snippet, dry_run)
}

/// Add a `[analyzer.NAME]` section to rsconstruct.toml.
pub fn add_analyzer(name: &str, dry_run: bool) -> Result<()> {
    let plugin = all_analyzer_plugins()
        .find(|p| p.name == name)
        .ok_or_else(|| anyhow::anyhow!("Unknown analyzer '{name}'"))?;

    let defaults: serde_json::Value = match (plugin.defconfig_toml)() {
        Some(t) => toml::from_str::<serde_json::Value>(&t)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default())),
        None => serde_json::Value::Object(serde_json::Map::default()),
    };
    let keys: Vec<&str> = match &defaults {
        serde_json::Value::Object(map) => map.keys().map(std::string::String::as_str).collect(),
        _ => Vec::new(),
    };

    let header = format!("analyzer.{name}");
    let snippet = render_section(
        &header,
        Some(plugin.description),
        &keys,
        &[],
        &[],
        &defaults,
        &HashMap::new(),
    );

    apply_snippet(&header, &snippet, dry_run)
}

/// Look up a processor's description from static plugin metadata.
fn processor_description(name: &str) -> Option<String> {
    crate::registries::processor::find_plugin(name).map(|p| p.description.to_string())
}

/// Render a single `[header]` block as a TOML snippet string with comments.
/// `header` is the full dotted table name (`processor.checker.ruff`,
/// `analyzer.tera`).
#[allow(clippy::too_many_arguments)]
fn render_section(
    header: &str,
    description: Option<&str>,
    fields: &[&str],
    must: &[&str],
    checksum_fields: &[&str],
    defaults: &serde_json::Value,
    descs: &HashMap<&str, &str>,
) -> String {
    let mut out = String::new();
    if let Some(d) = description {
        let _ = writeln!(out, "# {d}");
    }
    let _ = writeln!(out, "[{header}]");

    let def_obj = defaults.as_object();
    let must_set: std::collections::HashSet<&str> = must.iter().copied().collect();
    let checksum_set: std::collections::HashSet<&str> = checksum_fields.iter().copied().collect();

    // must-fill fields first, uncommented, with TODO placeholders where the default is empty.
    for field in must {
        let desc = descs.get(field).copied().unwrap_or("");
        if !desc.is_empty() {
            let _ = writeln!(out, "# {desc}");
        }
        let value_str = default_value_for_must(field, def_obj.and_then(|m| m.get(*field)));
        let _ = writeln!(out, "{field} = {value_str}");
    }
    if !must.is_empty() {
        out.push('\n');
    }

    // Remaining fields, all commented out.
    for field in fields {
        if must_set.contains(field) {
            continue;
        }
        let desc = descs.get(field).copied().unwrap_or("");
        let value_str = default_value_for_optional(def_obj.and_then(|m| m.get(*field)));
        let tag = if checksum_set.contains(field) {
            " (affects checksum)"
        } else {
            ""
        };
        if !desc.is_empty() {
            let _ = writeln!(out, "# {desc}{tag}");
        }
        match value_str {
            Some(value_str) => {
                let _ = writeln!(out, "# {field} = {value_str}");
            }
            // No default to show. A made-up `""` here was a type error
            // once uncommented for any field that is not a string
            // (`max_jobs = ""`).
            None => {
                let _ = writeln!(out, "# {field} is unset by default");
            }
        }
    }

    out
}

fn default_value_for_must(field: &str, val: Option<&serde_json::Value>) -> String {
    match val {
        Some(v) if !is_empty_default(v) => toml_value_string(v),
        _ => format!("\"TODO: set {field}\""),
    }
}

/// The default of an optional field as TOML, or `None` when it has none.
fn default_value_for_optional(val: Option<&serde_json::Value>) -> Option<String> {
    match val {
        Some(serde_json::Value::Null) | None => None,
        Some(v) => Some(toml_value_string(v)),
    }
}

const fn is_empty_default(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::String(s) => s.is_empty(),
        serde_json::Value::Array(a) => a.is_empty(),
        serde_json::Value::Null => true,
        _ => false,
    }
}

fn toml_value_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => {
            format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
        }
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => "\"\"".to_string(),
        serde_json::Value::Array(a) => {
            let items: Vec<String> = a.iter().map(toml_value_string).collect();
            format!("[{}]", items.join(", "))
        }
        serde_json::Value::Object(map) => {
            let items: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{} = {}", k, toml_value_string(v)))
                .collect();
            format!("{{ {} }}", items.join(", "))
        }
    }
}

/// Whether the config text already holds a table at the dotted `header`
/// path, in any spelling: `[a.b] # note`, `[ a.b ]`, a dotted key, or a
/// table that only holds instances (`[a.b.core]`). The document is parsed
/// rather than its lines compared, because appending a second `[a.b]` to a
/// file that already has one leaves a config that no longer loads.
fn section_exists(content: &str, header: &str) -> Result<bool> {
    let doc: toml_edit::DocumentMut = content
        .parse()
        .with_context(|| format!("Failed to parse {CONFIG_FILE}"))?;
    let mut current: &toml_edit::Item = doc.as_item();
    for key in header.split('.') {
        match current.get(key) {
            Some(item) => current = item,
            None => return Ok(false),
        }
    }
    Ok(true)
}

fn apply_snippet(header: &str, snippet: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        print!("{snippet}");
        return Ok(());
    }

    let path = std::path::Path::new(CONFIG_FILE);
    if !path.exists() {
        bail!(
            "{CONFIG_FILE} not found. Run 'rsconstruct init' first, or use --dry-run to preview."
        );
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {CONFIG_FILE}"))?;

    if section_exists(&content, header)? {
        bail!(
            "Section [{header}] already exists in {CONFIG_FILE}. Edit it manually or remove it first."
        );
    }

    let mut new_content = content;
    if !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    if !new_content.ends_with("\n\n") {
        new_content.push('\n');
    }
    new_content.push_str(snippet);

    fs::write(path, &new_content).with_context(|| format!("Failed to write {CONFIG_FILE}"))?;

    println!("Added [{header}] to {CONFIG_FILE}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The section is found however it is spelled, and so is one that only
    /// holds instances; a sibling is not mistaken for it.
    #[test]
    fn section_exists_reads_the_document_not_its_lines() {
        let header = "processor.checker.ruff";
        for present in [
            "[processor.checker.ruff]\n",
            "[processor.checker.ruff] # note\n",
            "[ processor.checker.ruff ]\n",
            "[processor.checker]\nruff = { args = [] }\n",
            "[processor.checker.ruff.core]\nargs = []\n",
        ] {
            assert!(section_exists(present, header).unwrap(), "{present}");
        }
        for absent in [
            "",
            "[build]\n",
            "[processor.checker.ruff_extra]\n",
            "# [processor.checker.ruff]\n",
        ] {
            assert!(!section_exists(absent, header).unwrap(), "{absent}");
        }
    }
}

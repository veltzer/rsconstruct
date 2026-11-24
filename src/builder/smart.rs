use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;

use crate::registries::{all_pnames, find_plugin, parse_name};

const CONFIG_FILE: &str = "rsconstruct.toml";

/// Say what a command did: `json` on one line under `--json`, else `text`.
fn report(text: &str, json: &serde_json::Value) {
    if crate::json_output::is_json_mode() {
        println!("{json}");
    } else {
        println!("{text}");
    }
}

/// Load rsconstruct.toml as a `toml_edit` document.
fn load_doc() -> Result<toml_edit::DocumentMut> {
    // The same "no rsconstruct.toml" config error every other command
    // gives, not an IO error about a file the user never made.
    crate::config::Config::require_config()?;
    let content =
        fs::read_to_string(CONFIG_FILE).with_context(|| format!("Failed to read {CONFIG_FILE}"))?;
    content
        .parse()
        .with_context(|| format!("Failed to parse {CONFIG_FILE}"))
}

/// Write a `toml_edit` document back to rsconstruct.toml.
fn save_doc(doc: &toml_edit::DocumentMut) -> Result<()> {
    fs::write(CONFIG_FILE, doc.to_string())
        .with_context(|| format!("Failed to write {CONFIG_FILE}"))
}

/// Get or create the [processor] table in the document.
fn processor_table(doc: &mut toml_edit::DocumentMut) -> Result<&mut toml_edit::Table> {
    doc.entry("processor")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .context("[processor] must be a table")
}

/// Validate that a name is a registered processor (`processor.<type>.<name>`),
/// not an instance of one.
fn validate_name(name: &str) -> Result<()> {
    match parse_name(name) {
        Some(parsed) if parsed.instance.is_none() && find_plugin(name).is_some() => Ok(()),
        _ => bail!(
            "Unknown processor '{name}'. Processors are named processor.<type>.<name>; run \
             'rsconstruct processor list' to see them."
        ),
    }
}

/// The key path of a processor or analyzer name below its section table.
///
/// A processor name `processor.checker.ruff` lives at `[processor]` →
/// `checker` → `ruff`; the instance `processor.checker.ruff.core` one level
/// further down. Analyzer names have no type segment: `tera` or `tera.sub`.
fn key_path(section: &str, name: &str) -> Vec<String> {
    let prefix = format!("{section}.");
    let rest = name.strip_prefix(&prefix).unwrap_or(name);
    rest.split('.').map(str::to_string).collect()
}

/// The table at `path` below `table`, created empty where missing.
fn ensure_table<'a>(
    table: &'a mut toml_edit::Table,
    path: &[String],
) -> Result<&'a mut toml_edit::Table> {
    let mut current = table;
    for key in path {
        current = current
            .entry(key)
            .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_mut()
            .with_context(|| format!("'{key}' must be a table"))?;
    }
    Ok(current)
}

/// The table at `path` below `table`, if every segment exists and is a
/// table, written either as a `[header]` or inline (`ruff = {}`).
fn get_table<'a>(
    table: &'a mut toml_edit::Table,
    path: &[String],
) -> Option<&'a mut dyn toml_edit::TableLike> {
    let mut current: &mut dyn toml_edit::TableLike = table;
    for key in path {
        current = current.get_mut(key)?.as_table_like_mut()?;
    }
    Some(current)
}

/// The instance names `doc` declares in `section` (`processor` or
/// `analyzer`), read the way the build reads them, so a name is "declared"
/// here exactly when the build would create an instance for it.
fn declared_inames(doc: &toml_edit::DocumentMut, section: &str) -> Result<Vec<String>> {
    let value: toml::Value = toml::from_str(&doc.to_string())
        .with_context(|| format!("Failed to parse {CONFIG_FILE}"))?;
    let Some(section_value) = value.get(section) else {
        return Ok(Vec::new());
    };
    let mut names: Vec<String> = if section == "processor" {
        crate::config::ProcessorConfig::from_toml(section_value)
            .instances
            .into_iter()
            .map(|inst| inst.instance_name)
            .collect()
    } else {
        crate::config::AnalyzerConfig::from_toml(section_value)?
            .instances
            .into_iter()
            .map(|inst| inst.instance_name)
            .collect()
    };
    names.sort();
    Ok(names)
}

/// Fail unless `iname` is a declared instance of `section`. The commands
/// that edit one stanza go through this: resolving the name as a key path
/// alone also finds a type table (`processor.checker`) or the parent of
/// named instances (`processor.checker.ruff` above `.core`), and writing
/// `enabled` there, or deleting it, wrecked the file.
fn require_declared(doc: &toml_edit::DocumentMut, section: &str, iname: &str) -> Result<()> {
    let declared = declared_inames(doc, section)?;
    if declared.iter().any(|name| name == iname) {
        return Ok(());
    }
    let listing = if declared.is_empty() {
        "none are declared".to_string()
    } else {
        format!("declared: {}", declared.join(", "))
    };
    bail!("{section} '{iname}' is not declared in {CONFIG_FILE} ({listing}).")
}

/// Whether a section for `name` exists below `table`.
fn has_section(table: &mut toml_edit::Table, section: &str, name: &str) -> bool {
    get_table(table, &key_path(section, name)).is_some()
}

/// Add an empty section for `name` below `table`. Returns false if it existed.
fn add_section(table: &mut toml_edit::Table, section: &str, name: &str) -> Result<bool> {
    if has_section(table, section, name) {
        return Ok(false);
    }
    let path = key_path(section, name);
    let (parents, last) = path.split_at(path.len() - 1);
    let parent = ensure_table(table, parents)?;
    parent.insert(&last[0], toml_edit::Item::Table(toml_edit::Table::new()));
    Ok(true)
}

/// Remove the table at `path` below `table`, then every parent table it left
/// empty. Returns whether anything was removed.
fn remove_at(table: &mut dyn toml_edit::TableLike, path: &[String]) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return false;
    };
    if rest.is_empty() {
        return table.remove(first).is_some();
    }
    let Some(child) = table.get_mut(first).and_then(|t| t.as_table_like_mut()) else {
        return false;
    };
    let removed = remove_at(child, rest);
    if removed && child.is_empty() {
        table.remove(first);
    }
    removed
}

/// Remove the section for `name` below `table`, then every parent table it
/// left empty, so a removed `processor.checker.ruff` does not leave a bare
/// `[processor.checker]` behind. Returns whether anything was removed.
fn remove_section(table: &mut toml_edit::Table, section: &str, name: &str) -> bool {
    let path = key_path(section, name);
    remove_at(table, &path)
}

/// Disable all processors by removing all [processor.*] sections.
pub fn disable_all() -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let count = remove_all_sections(table);

    save_doc(&doc)?;
    report(
        &format!("Removed {count} processor sections from {CONFIG_FILE}."),
        &serde_json::json!({ "removed": count }),
    );
    Ok(())
}

/// Remove every processor section, counting leaf sections (one per
/// processor or instance), and return the count.
fn remove_all_sections(table: &mut toml_edit::Table) -> usize {
    // processor → type → name → (fields | instances): a name table whose
    // values are all tables holds instances, one section each; any other
    // name table is one section itself.
    fn count_sections(name_table: &toml_edit::Table) -> usize {
        let instances = name_table
            .iter()
            .filter(|(_, item)| item.is_table())
            .count();
        if instances > 0 && instances == name_table.len() {
            instances
        } else {
            1
        }
    }
    let count: usize = table
        .iter()
        .filter_map(|(_, type_item)| type_item.as_table())
        .flat_map(|type_table| type_table.iter())
        .filter_map(|(_, name_item)| name_item.as_table())
        .map(count_sections)
        .sum();
    let keys: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
    for key in &keys {
        table.remove(key);
    }
    count
}

/// Enable all processors by adding a section for every registered processor.
pub fn enable_all() -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let mut count = 0;

    for name in all_pnames() {
        if add_section(table, "processor", &name)? {
            count += 1;
        }
    }

    save_doc(&doc)?;
    report(
        &format!("Added {count} processor sections to {CONFIG_FILE}."),
        &serde_json::json!({ "added": count }),
    );
    Ok(())
}

/// Disable a single processor by removing its section.
pub fn disable(name: &str) -> Result<()> {
    validate_name(name)?;
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;

    let removed = remove_section(table, "processor", name);
    let text = if removed {
        save_doc(&doc)?;
        format!("Removed processor '{name}'.")
    } else {
        format!("Processor '{name}' is not declared.")
    };
    report(
        &text,
        &serde_json::json!({ "processor": name, "removed": removed }),
    );
    Ok(())
}

/// Enable a single processor by adding an empty section for it.
pub fn enable(name: &str) -> Result<()> {
    validate_name(name)?;
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;

    let added = add_section(table, "processor", name)?;
    let text = if added {
        save_doc(&doc)?;
        format!("Added processor '{name}'.")
    } else {
        format!("Processor '{name}' is already declared.")
    };
    report(
        &text,
        &serde_json::json!({ "processor": name, "added": added }),
    );
    Ok(())
}

/// The names in sorted order. Sections are written in the order they are
/// added, and a `HashSet` iterates in a different order on every run, so
/// adding straight from the set wrote the same config in a different stanza
/// order each time.
fn sorted(names: &HashSet<String>) -> Vec<&String> {
    let mut names: Vec<&String> = names.iter().collect();
    names.sort();
    names
}

/// Enable only processors whose files are detected in the project.
pub fn enable_detected(detected: &HashSet<String>) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let mut count = 0;

    for name in sorted(detected) {
        if add_section(table, "processor", name)? {
            count += 1;
        }
    }

    save_doc(&doc)?;
    report(
        &format!("Added {count} detected processor sections to {CONFIG_FILE}."),
        &serde_json::json!({ "added": count }),
    );
    Ok(())
}

/// Remove all processor sections, returning to empty config.
pub fn reset() -> Result<()> {
    disable_all()
}

/// Remove all processor sections, then add only the listed ones.
pub fn only(names: &[String]) -> Result<()> {
    for name in names {
        validate_name(name)?;
    }

    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    remove_all_sections(table);
    for name in names {
        add_section(table, "processor", name)?;
    }

    save_doc(&doc)?;
    report(
        &format!("Active processors: {}", names.join(", ")),
        &serde_json::json!({ "active": names }),
    );
    Ok(())
}

/// Remove all processor sections, then add only detected ones.
pub fn minimal(detected: &HashSet<String>) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    remove_all_sections(table);
    let names = sorted(detected);
    for name in &names {
        add_section(table, "processor", name)?;
    }

    save_doc(&doc)?;
    let text = if detected.is_empty() {
        "No processors detected.".to_string()
    } else {
        format!(
            "Minimal config: {}",
            names
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    report(&text, &serde_json::json!({ "active": names }));
    Ok(())
}

/// Add sections for processors whose files are detected AND tools are installed.
pub fn enable_if_available(available: &HashSet<String>) -> Result<()> {
    enable_detected(available)
}

/// Auto-detect relevant processors and add them to rsconstruct.toml.
/// Only adds processors whose files are detected AND whose tools are installed.
/// Does not remove existing processor sections.
pub fn auto(available: &HashSet<String>) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let mut added = Vec::new();

    for name in sorted(available) {
        if add_section(table, "processor", name)? {
            added.push(name.as_str());
        }
    }

    let text = if added.is_empty() {
        "No new processors to add (all detected processors are already declared).".to_string()
    } else {
        save_doc(&doc)?;
        format!("Added {} processor(s): {}", added.len(), added.join(", "))
    };
    report(&text, &serde_json::json!({ "added": added }));
    Ok(())
}

/// Get or create the [analyzer] table in the document.
fn analyzer_table(doc: &mut toml_edit::DocumentMut) -> Result<&mut toml_edit::Table> {
    doc.entry("analyzer")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .context("[analyzer] must be a table")
}

/// The section table (`processor` or `analyzer`) of `doc`.
fn section_table<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    section: &str,
) -> Result<&'a mut toml_edit::Table> {
    if section == "processor" {
        processor_table(doc)
    } else {
        analyzer_table(doc)
    }
}

/// Set `enabled = value` on the declared instance `iname` of `section`.
fn set_enabled_in(
    doc: &mut toml_edit::DocumentMut,
    section: &str,
    iname: &str,
    value: bool,
) -> Result<()> {
    require_declared(doc, section, iname)?;
    let table = section_table(doc, section)?;
    get_table(table, &key_path(section, iname))
        .with_context(|| format!("{section} '{iname}' has no table in {CONFIG_FILE}"))?
        .insert("enabled", toml_edit::value(value));
    Ok(())
}

/// Remove the declared instance `iname` of `section` from `doc`.
fn delete_in(doc: &mut toml_edit::DocumentMut, section: &str, iname: &str) -> Result<()> {
    require_declared(doc, section, iname)?;
    let table = section_table(doc, section)?;
    remove_section(table, section, iname);
    Ok(())
}

/// Set `enabled = value` on the declared instance `iname` of `section` in
/// rsconstruct.toml.
fn set_enabled(section: &str, iname: &str, value: bool) -> Result<()> {
    let mut doc = load_doc()?;
    set_enabled_in(&mut doc, section, iname, value)?;
    save_doc(&doc)?;
    let verb = if value { "Enabled" } else { "Disabled" };
    report(
        &format!("{verb} {section} '{iname}'."),
        &serde_json::json!({ section: iname, "enabled": value }),
    );
    Ok(())
}

/// Delete the declared instance `iname` of `section` from rsconstruct.toml.
fn delete(section: &str, iname: &str) -> Result<()> {
    let mut doc = load_doc()?;
    delete_in(&mut doc, section, iname)?;
    save_doc(&doc)?;
    report(
        &format!("Deleted {section} '{iname}'."),
        &serde_json::json!({ section: iname, "deleted": true }),
    );
    Ok(())
}

/// Delete a processor by iname from rsconstruct.toml.
pub fn delete_processor(iname: &str) -> Result<()> {
    delete("processor", iname)
}

/// Set enabled = false on a processor by iname.
pub fn disable_processor(iname: &str) -> Result<()> {
    set_enabled("processor", iname, false)
}

/// Set enabled = true on a processor by iname.
pub fn enable_processor(iname: &str) -> Result<()> {
    set_enabled("processor", iname, true)
}

/// Delete an analyzer by iname from rsconstruct.toml.
pub fn delete_analyzer(iname: &str) -> Result<()> {
    delete("analyzer", iname)
}

/// Set enabled = false on an analyzer by iname.
pub fn disable_analyzer(iname: &str) -> Result<()> {
    set_enabled("analyzer", iname, false)
}

/// Set enabled = true on an analyzer by iname.
pub fn enable_analyzer(iname: &str) -> Result<()> {
    set_enabled("analyzer", iname, true)
}

/// Remove processors from rsconstruct.toml that don't match any files.
pub fn remove_no_file_processors(empty_processors: &[String]) -> Result<()> {
    if empty_processors.is_empty() {
        report(
            "All processors match at least one file.",
            &serde_json::json!({ "removed": [] }),
        );
        return Ok(());
    }

    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let mut removed = Vec::new();

    for name in empty_processors {
        if remove_section(table, "processor", name) {
            removed.push(name.as_str());
        }
    }

    let text = if removed.is_empty() {
        "No processors to remove.".to_string()
    } else {
        save_doc(&doc)?;
        format!(
            "Removed {} processor(s) with no files: {}",
            removed.len(),
            removed.join(", ")
        )
    };
    report(&text, &serde_json::json!({ "removed": removed }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> toml_edit::DocumentMut {
        src.parse().unwrap()
    }

    /// Sections are added in sorted order, whatever order the set yields,
    /// so the same detection writes the same file every time.
    #[test]
    fn detected_processors_are_written_in_sorted_order() {
        let detected: HashSet<String> = [
            "processor.checker.ruff",
            "processor.generator.tera",
            "processor.checker.ascii",
            "processor.checker.pylint",
            "processor.checker.make",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        let mut d = doc("");
        let table = processor_table(&mut d).unwrap();
        for name in sorted(&detected) {
            add_section(table, "processor", name).unwrap();
        }
        let text = d.to_string();
        let positions: Vec<usize> = [
            "[processor.checker.ascii]",
            "[processor.checker.make]",
            "[processor.checker.pylint]",
            "[processor.checker.ruff]",
            "[processor.generator.tera]",
        ]
        .iter()
        .map(|h| {
            text.find(h)
                .unwrap_or_else(|| panic!("{h} missing in {text}"))
        })
        .collect();
        assert!(positions.is_sorted(), "{text}");
    }

    /// Sections nest by name segment, and removing the last processor of a
    /// type takes the empty type table with it.
    #[test]
    fn sections_nest_by_name_segment() {
        let mut d = doc("");
        let table = processor_table(&mut d).unwrap();
        assert!(add_section(table, "processor", "processor.checker.ruff").unwrap());
        assert!(!add_section(table, "processor", "processor.checker.ruff").unwrap());
        assert!(add_section(table, "processor", "processor.checker.pylint.core").unwrap());
        let text = d.to_string();
        assert!(text.contains("[processor.checker.ruff]"), "{text}");
        assert!(text.contains("[processor.checker.pylint.core]"), "{text}");

        let table = processor_table(&mut d).unwrap();
        assert!(remove_section(table, "processor", "processor.checker.ruff"));
        assert!(remove_section(
            table,
            "processor",
            "processor.checker.pylint.core"
        ));
        assert!(!remove_section(
            table,
            "processor",
            "processor.checker.pylint.core"
        ));
        assert!(
            table.get("checker").is_none(),
            "empty type table must go: {d}"
        );
    }

    #[test]
    fn enabled_flag_lands_on_the_named_section() {
        let mut d = doc("[processor.checker.pylint.core]\nargs = []\n[analyzer.tera]\n");
        set_enabled_in(&mut d, "processor", "processor.checker.pylint.core", false).unwrap();
        assert!(set_enabled_in(&mut d, "processor", "processor.checker.ruff", false).is_err());
        set_enabled_in(&mut d, "analyzer", "tera", false).unwrap();
        let text = d.to_string();
        assert!(
            text.contains("[processor.checker.pylint.core]\nargs = []\nenabled = false"),
            "{text}"
        );
        assert!(text.contains("[analyzer.tera]\nenabled = false"), "{text}");
    }

    /// A name that is a table in the file but not a declared instance (the
    /// parent of named instances, a type table) is refused and the file is
    /// left alone: `enabled = false` on the parent made `core` an unknown
    /// field, and deleting `processor.checker` removed every checker.
    #[test]
    fn a_table_that_is_not_an_instance_is_refused() {
        let src = "[processor.checker.pylint.core]\nargs = []\n[processor.checker.ruff]\n";
        for name in ["processor.checker.pylint", "processor.checker", "processor"] {
            let mut d = doc(src);
            let err = set_enabled_in(&mut d, "processor", name, false).unwrap_err();
            assert!(err.to_string().contains("is not declared"), "{err}");
            assert!(
                err.to_string().contains("processor.checker.pylint.core"),
                "the declared names are listed: {err}"
            );
            assert!(delete_in(&mut d, "processor", name).is_err());
            assert_eq!(d.to_string(), src);
        }
    }

    /// A stanza written as an inline table is declared like any other.
    #[test]
    fn an_inline_table_stanza_is_edited() {
        let mut d = doc("[processor.checker]\nruff = {}\nascii = {}\n");
        set_enabled_in(&mut d, "processor", "processor.checker.ruff", false).unwrap();
        assert!(d.to_string().contains("ruff = { enabled = false }"), "{d}");
        delete_in(&mut d, "processor", "processor.checker.ascii").unwrap();
        assert!(!d.to_string().contains("ascii"), "{d}");
    }

    #[test]
    fn remove_all_counts_processors_and_instances() {
        let mut d = doc(
            "[processor.checker.ruff]\n[processor.checker.pylint.core]\n[processor.checker.pylint.tests]\n[processor.generator.tera]\n",
        );
        let table = processor_table(&mut d).unwrap();
        assert_eq!(remove_all_sections(table), 4);
        assert!(table.is_empty());
    }
}

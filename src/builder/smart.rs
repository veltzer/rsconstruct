use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;

use crate::registries::{all_pnames, find_plugin, parse_name};

const CONFIG_FILE: &str = "rsconstruct.toml";

/// Load rsconstruct.toml as a `toml_edit` document.
fn load_doc() -> Result<toml_edit::DocumentMut> {
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

/// The table at `path` below `table`, if every segment exists and is a table.
fn get_table<'a>(
    table: &'a mut toml_edit::Table,
    path: &[String],
) -> Option<&'a mut toml_edit::Table> {
    let mut current = table;
    for key in path {
        current = current.get_mut(key)?.as_table_mut()?;
    }
    Some(current)
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

/// Remove the section for `name` below `table`, then every parent table it
/// left empty, so a removed `processor.checker.ruff` does not leave a bare
/// `[processor.checker]` behind. Returns whether anything was removed.
fn remove_section(table: &mut toml_edit::Table, section: &str, name: &str) -> bool {
    let path = key_path(section, name);
    fn remove_at(table: &mut toml_edit::Table, path: &[String]) -> bool {
        let Some((first, rest)) = path.split_first() else {
            return false;
        };
        if rest.is_empty() {
            return table.remove(first).is_some();
        }
        let Some(child) = table.get_mut(first).and_then(|t| t.as_table_mut()) else {
            return false;
        };
        let removed = remove_at(child, rest);
        if removed && child.is_empty() {
            table.remove(first);
        }
        removed
    }
    remove_at(table, &path)
}

/// Disable all processors by removing all [processor.*] sections.
pub fn disable_all() -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let count = remove_all_sections(table);

    save_doc(&doc)?;
    println!("Removed {count} processor sections from {CONFIG_FILE}.");
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
    println!("Added {count} processor sections to {CONFIG_FILE}.");
    Ok(())
}

/// Disable a single processor by removing its section.
pub fn disable(name: &str) -> Result<()> {
    validate_name(name)?;
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;

    if remove_section(table, "processor", name) {
        save_doc(&doc)?;
        println!("Removed processor '{name}'.");
    } else {
        println!("Processor '{name}' is not declared.");
    }
    Ok(())
}

/// Enable a single processor by adding an empty section for it.
pub fn enable(name: &str) -> Result<()> {
    validate_name(name)?;
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;

    if add_section(table, "processor", name)? {
        save_doc(&doc)?;
        println!("Added processor '{name}'.");
    } else {
        println!("Processor '{name}' is already declared.");
    }
    Ok(())
}

/// Enable only processors whose files are detected in the project.
pub fn enable_detected(detected: &HashSet<String>) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let mut count = 0;

    for name in detected {
        if add_section(table, "processor", name)? {
            count += 1;
        }
    }

    save_doc(&doc)?;
    println!("Added {count} detected processor sections to {CONFIG_FILE}.");
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
    println!("Active processors: {}", names.join(", "));
    Ok(())
}

/// Remove all processor sections, then add only detected ones.
pub fn minimal(detected: &HashSet<String>) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    remove_all_sections(table);
    for name in detected {
        add_section(table, "processor", name)?;
    }

    save_doc(&doc)?;
    if detected.is_empty() {
        println!("No processors detected.");
    } else {
        let mut names: Vec<&String> = detected.iter().collect();
        names.sort();
        println!(
            "Minimal config: {}",
            names
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
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

    for name in available {
        if add_section(table, "processor", name)? {
            added.push(name.as_str());
        }
    }

    if added.is_empty() {
        println!("No new processors to add (all detected processors are already declared).");
    } else {
        save_doc(&doc)?;
        let mut added = added;
        added.sort_unstable();
        println!("Added {} processor(s): {}", added.len(), added.join(", "));
    }
    Ok(())
}

/// Get or create the [analyzer] table in the document.
fn analyzer_table(doc: &mut toml_edit::DocumentMut) -> Result<&mut toml_edit::Table> {
    doc.entry("analyzer")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .context("[analyzer] must be a table")
}

/// Set `enabled = VALUE` on the entry for `iname` below `section_table`.
/// Returns an error if the iname is not found.
fn set_enabled_iname(
    section_table: &mut toml_edit::Table,
    iname: &str,
    value: bool,
    section: &str,
) -> Result<()> {
    match get_table(section_table, &key_path(section, iname)) {
        Some(t) => {
            t.insert("enabled", toml_edit::value(value));
            Ok(())
        }
        None => bail!("{section} '{iname}' is not declared in rsconstruct.toml."),
    }
}

/// Delete a processor by iname from rsconstruct.toml.
pub fn delete_processor(iname: &str) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    if remove_section(table, "processor", iname) {
        save_doc(&doc)?;
        println!("Deleted processor '{iname}'.");
    } else {
        println!("Processor '{iname}' is not declared.");
    }
    Ok(())
}

/// Set enabled = false on a processor by iname.
pub fn disable_processor(iname: &str) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    set_enabled_iname(table, iname, false, "processor")?;
    save_doc(&doc)?;
    println!("Disabled processor '{iname}'.");
    Ok(())
}

/// Set enabled = true on a processor by iname.
pub fn enable_processor(iname: &str) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    set_enabled_iname(table, iname, true, "processor")?;
    save_doc(&doc)?;
    println!("Enabled processor '{iname}'.");
    Ok(())
}

/// Delete an analyzer by iname from rsconstruct.toml.
pub fn delete_analyzer(iname: &str) -> Result<()> {
    let mut doc = load_doc()?;
    let table = analyzer_table(&mut doc)?;
    if remove_section(table, "analyzer", iname) {
        save_doc(&doc)?;
        println!("Deleted analyzer '{iname}'.");
    } else {
        println!("Analyzer '{iname}' is not declared.");
    }
    Ok(())
}

/// Set enabled = false on an analyzer by iname.
pub fn disable_analyzer(iname: &str) -> Result<()> {
    let mut doc = load_doc()?;
    let table = analyzer_table(&mut doc)?;
    set_enabled_iname(table, iname, false, "analyzer")?;
    save_doc(&doc)?;
    println!("Disabled analyzer '{iname}'.");
    Ok(())
}

/// Set enabled = true on an analyzer by iname.
pub fn enable_analyzer(iname: &str) -> Result<()> {
    let mut doc = load_doc()?;
    let table = analyzer_table(&mut doc)?;
    set_enabled_iname(table, iname, true, "analyzer")?;
    save_doc(&doc)?;
    println!("Enabled analyzer '{iname}'.");
    Ok(())
}

/// Remove processors from rsconstruct.toml that don't match any files.
pub fn remove_no_file_processors(empty_processors: &[String]) -> Result<()> {
    if empty_processors.is_empty() {
        println!("All processors match at least one file.");
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

    if removed.is_empty() {
        println!("No processors to remove.");
    } else {
        save_doc(&doc)?;
        println!(
            "Removed {} processor(s) with no files: {}",
            removed.len(),
            removed.join(", ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(src: &str) -> toml_edit::DocumentMut {
        src.parse().unwrap()
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
        let table = processor_table(&mut d).unwrap();
        set_enabled_iname(table, "processor.checker.pylint.core", false, "processor").unwrap();
        assert!(set_enabled_iname(table, "processor.checker.ruff", false, "processor").is_err());
        let table = analyzer_table(&mut d).unwrap();
        set_enabled_iname(table, "tera", false, "analyzer").unwrap();
        let text = d.to_string();
        assert!(
            text.contains("[processor.checker.pylint.core]\nargs = []\nenabled = false"),
            "{text}"
        );
        assert!(text.contains("[analyzer.tera]\nenabled = false"), "{text}");
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

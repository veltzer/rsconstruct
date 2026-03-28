use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;

use crate::config::all_type_names;

const CONFIG_FILE: &str = "rsconstruct.toml";

/// Load rsconstruct.toml as a toml_edit document.
fn load_doc() -> Result<toml_edit::DocumentMut> {
    let content = fs::read_to_string(CONFIG_FILE)
        .with_context(|| format!("Failed to read {}", CONFIG_FILE))?;
    content.parse()
        .with_context(|| format!("Failed to parse {}", CONFIG_FILE))
}

/// Write a toml_edit document back to rsconstruct.toml.
fn save_doc(doc: &toml_edit::DocumentMut) -> Result<()> {
    fs::write(CONFIG_FILE, doc.to_string())
        .with_context(|| format!("Failed to write {}", CONFIG_FILE))
}

/// Get or create the [processor] table in the document.
fn processor_table(doc: &mut toml_edit::DocumentMut) -> Result<&mut toml_edit::Table> {
    doc.entry("processor")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .context("[processor] must be a table")
}

/// Validate that a processor name is a known builtin type.
fn validate_name(name: &str) -> Result<()> {
    let all = all_type_names();
    if !all.iter().any(|n| *n == name) {
        bail!(
            "Unknown processor '{}'. Run 'rsconstruct processors list --all' to see available processors.",
            name
        );
    }
    Ok(())
}

/// Disable all processors by removing all [processor.*] sections.
pub(crate) fn disable_all() -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let keys: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
    let mut count = 0;

    for key in &keys {
        if table.remove(key).is_some() {
            count += 1;
        }
    }

    save_doc(&doc)?;
    println!("Removed {} processor sections from {}.", count, CONFIG_FILE);
    Ok(())
}

/// Enable all processors by adding [processor.NAME] sections for all builtin types.
pub(crate) fn enable_all() -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let all_names = all_type_names();
    let mut count = 0;

    for name in &all_names {
        if table.get(name).is_none() {
            table.insert(name, toml_edit::Item::Table(toml_edit::Table::new()));
            count += 1;
        }
    }

    save_doc(&doc)?;
    println!("Added {} processor sections to {}.", count, CONFIG_FILE);
    Ok(())
}

/// Disable a single processor by removing its [processor.NAME] section.
pub(crate) fn disable(name: &str) -> Result<()> {
    validate_name(name)?;
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;

    if table.remove(name).is_some() {
        save_doc(&doc)?;
        println!("Removed processor '{}'.", name);
    } else {
        println!("Processor '{}' is not declared.", name);
    }
    Ok(())
}

/// Enable a single processor by adding an empty [processor.NAME] section.
pub(crate) fn enable(name: &str) -> Result<()> {
    validate_name(name)?;
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;

    if table.get(name).is_some() {
        println!("Processor '{}' is already declared.", name);
    } else {
        table.insert(name, toml_edit::Item::Table(toml_edit::Table::new()));
        save_doc(&doc)?;
        println!("Added processor '{}'.", name);
    }
    Ok(())
}

/// Enable only processors whose files are detected in the project.
pub(crate) fn enable_detected(detected: &HashSet<String>) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let mut count = 0;

    for name in detected {
        if table.get(name.as_str()).is_none() {
            table.insert(name.as_str(), toml_edit::Item::Table(toml_edit::Table::new()));
            count += 1;
        }
    }

    save_doc(&doc)?;
    println!("Added {} detected processor sections to {}.", count, CONFIG_FILE);
    Ok(())
}

/// Remove all processor sections, returning to empty config.
pub(crate) fn reset() -> Result<()> {
    disable_all()
}

/// Remove all processor sections, then add only the listed ones.
pub(crate) fn only(names: &[String]) -> Result<()> {
    for name in names {
        validate_name(name)?;
    }

    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;

    // Remove all existing processor sections
    let keys: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
    for key in &keys {
        table.remove(key);
    }

    // Add only the requested ones
    for name in names {
        table.insert(name.as_str(), toml_edit::Item::Table(toml_edit::Table::new()));
    }

    save_doc(&doc)?;
    println!("Active processors: {}", names.join(", "));
    Ok(())
}

/// Remove all processor sections, then add only detected ones.
pub(crate) fn minimal(detected: &HashSet<String>) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;

    // Remove all
    let keys: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
    for key in &keys {
        table.remove(key);
    }

    // Add detected
    for name in detected {
        table.insert(name.as_str(), toml_edit::Item::Table(toml_edit::Table::new()));
    }

    save_doc(&doc)?;
    if !detected.is_empty() {
        let mut names: Vec<&String> = detected.iter().collect();
        names.sort();
        println!("Minimal config: {}", names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
    } else {
        println!("No processors detected.");
    }
    Ok(())
}

/// Add sections for processors whose files are detected AND tools are installed.
pub(crate) fn enable_if_available(available: &HashSet<String>) -> Result<()> {
    enable_detected(available)
}

/// Auto-detect relevant processors and add them to rsconstruct.toml.
/// Only adds processors whose files are detected AND whose tools are installed.
/// Does not remove existing processor sections.
pub(crate) fn auto(available: &HashSet<String>) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let mut added = Vec::new();

    for name in available {
        if table.get(name.as_str()).is_none() {
            table.insert(name.as_str(), toml_edit::Item::Table(toml_edit::Table::new()));
            added.push(name.as_str());
        }
    }

    if added.is_empty() {
        println!("No new processors to add (all detected processors are already declared).");
    } else {
        save_doc(&doc)?;
        let mut added = added;
        added.sort();
        println!("Added {} processor(s): {}", added.len(), added.join(", "));
    }
    Ok(())
}

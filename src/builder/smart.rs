use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;

use crate::color;
use crate::config::default_processors;

const CONFIG_FILE: &str = "rsbuild.toml";

/// Load rsbuild.toml as a toml_edit document.
fn load_doc() -> Result<toml_edit::DocumentMut> {
    let content = fs::read_to_string(CONFIG_FILE)
        .with_context(|| format!("Failed to read {}", CONFIG_FILE))?;
    content.parse()
        .with_context(|| format!("Failed to parse {}", CONFIG_FILE))
}

/// Write a toml_edit document back to rsbuild.toml.
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

/// Validate that a processor name is known.
fn validate_name(name: &str) -> Result<()> {
    let all = default_processors();
    if !all.iter().any(|n| n == name) {
        bail!(
            "Unknown processor '{}'. Run 'rsbuild processors list --all' to see available processors.",
            name
        );
    }
    Ok(())
}

/// Disable all processors by setting `enabled = false` in each `[processor.NAME]` section.
pub(crate) fn disable_all() -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let all_names = default_processors();
    let mut count = 0;

    for name in &all_names {
        let section = table.entry(name.as_str())
            .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_mut();
        let section = match section {
            Some(t) => t,
            None => continue,
        };

        let already_disabled = section.get("enabled")
            .and_then(|v| v.as_bool())
            .is_some_and(|b| !b);
        if !already_disabled {
            section.insert("enabled", toml_edit::value(false));
            count += 1;
        }
    }

    save_doc(&doc)?;
    println!("Disabled {} processors in {}.", count, CONFIG_FILE);
    println!("{}", color::dim("Hint: enable processors one by one with [processor.NAME] enabled = true"));
    Ok(())
}

/// Enable all processors by removing `enabled = false` from each `[processor.NAME]` section.
pub(crate) fn enable_all() -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let all_names = default_processors();
    let mut count = 0;

    for name in &all_names {
        let section = match table.get_mut(name.as_str()).and_then(|v| v.as_table_mut()) {
            Some(t) => t,
            None => continue,
        };

        let is_disabled = section.get("enabled")
            .and_then(|v| v.as_bool())
            .is_some_and(|b| !b);
        if is_disabled {
            section.remove("enabled");
            count += 1;
        }
    }

    save_doc(&doc)?;
    println!("Enabled {} processors in {}.", count, CONFIG_FILE);
    Ok(())
}

/// Disable a single processor by setting `enabled = false`.
pub(crate) fn disable(name: &str) -> Result<()> {
    validate_name(name)?;
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;

    let section = table.entry(name)
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .with_context(|| format!("[processor.{}] must be a table", name))?;

    let already_disabled = section.get("enabled")
        .and_then(|v| v.as_bool())
        .is_some_and(|b| !b);
    if already_disabled {
        println!("Processor '{}' is already disabled.", name);
    } else {
        section.insert("enabled", toml_edit::value(false));
        save_doc(&doc)?;
        println!("Disabled processor '{}'.", name);
    }
    Ok(())
}

/// Enable a single processor by removing `enabled = false`.
pub(crate) fn enable(name: &str) -> Result<()> {
    validate_name(name)?;
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;

    let section = match table.get_mut(name).and_then(|v| v.as_table_mut()) {
        Some(t) => t,
        None => {
            println!("Processor '{}' is already enabled (no override in {}).", name, CONFIG_FILE);
            return Ok(());
        }
    };

    let is_disabled = section.get("enabled")
        .and_then(|v| v.as_bool())
        .is_some_and(|b| !b);
    if is_disabled {
        section.remove("enabled");
        save_doc(&doc)?;
        println!("Enabled processor '{}'.", name);
    } else {
        println!("Processor '{}' is already enabled.", name);
    }
    Ok(())
}

/// Enable only processors whose files are detected in the project.
/// Requires a Builder to run auto-detection.
pub(crate) fn enable_detected(detected: &HashSet<String>) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let all_names = default_processors();
    let mut enabled_count = 0;

    for name in &all_names {
        let section = match table.get_mut(name.as_str()).and_then(|v| v.as_table_mut()) {
            Some(t) => t,
            None => continue,
        };

        if detected.contains(name.as_str()) {
            let is_disabled = section.get("enabled")
                .and_then(|v| v.as_bool())
                .is_some_and(|b| !b);
            if is_disabled {
                section.remove("enabled");
                enabled_count += 1;
            }
        }
    }

    save_doc(&doc)?;
    println!("Enabled {} detected processors in {}.", enabled_count, CONFIG_FILE);
    Ok(())
}

/// Remove all [processor.*] sections from rsbuild.toml, returning to pure defaults.
pub(crate) fn reset() -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let all_names = default_processors();
    let mut count = 0;

    for name in &all_names {
        if table.remove(name.as_str()).is_some() {
            count += 1;
        }
    }

    save_doc(&doc)?;
    println!("Reset {} processor sections in {}.", count, CONFIG_FILE);
    Ok(())
}

/// Disable all, then enable only the listed processors.
pub(crate) fn only(names: &[String]) -> Result<()> {
    for name in names {
        validate_name(name)?;
    }

    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let all_names = default_processors();
    let selected: HashSet<&str> = names.iter().map(|s| s.as_str()).collect();
    let mut disabled_count = 0;
    let mut enabled_count = 0;

    for name in &all_names {
        let section = table.entry(name.as_str())
            .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_mut();
        let section = match section {
            Some(t) => t,
            None => continue,
        };

        if selected.contains(name.as_str()) {
            let is_disabled = section.get("enabled")
                .and_then(|v| v.as_bool())
                .is_some_and(|b| !b);
            if is_disabled {
                section.remove("enabled");
                enabled_count += 1;
            }
        } else {
            let already_disabled = section.get("enabled")
                .and_then(|v| v.as_bool())
                .is_some_and(|b| !b);
            if !already_disabled {
                section.insert("enabled", toml_edit::value(false));
                disabled_count += 1;
            }
        }
    }

    save_doc(&doc)?;
    println!("Only: enabled {}, disabled {} others.", enabled_count, disabled_count);
    println!("{}", color::dim(&format!("Active processors: {}", names.join(", "))));
    Ok(())
}

/// Disable all processors, then enable only detected ones.
pub(crate) fn minimal(detected: &HashSet<String>) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let all_names = default_processors();
    let mut disabled_count = 0;
    let mut enabled_count = 0;

    for name in &all_names {
        let section = table.entry(name.as_str())
            .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_mut();
        let section = match section {
            Some(t) => t,
            None => continue,
        };

        if detected.contains(name.as_str()) {
            // Enable: remove enabled = false if present
            let is_disabled = section.get("enabled")
                .and_then(|v| v.as_bool())
                .is_some_and(|b| !b);
            if is_disabled {
                section.remove("enabled");
                enabled_count += 1;
            }
        } else {
            // Disable: set enabled = false if not already
            let already_disabled = section.get("enabled")
                .and_then(|v| v.as_bool())
                .is_some_and(|b| !b);
            if !already_disabled {
                section.insert("enabled", toml_edit::value(false));
                disabled_count += 1;
            }
        }
    }

    save_doc(&doc)?;
    println!("Minimal config: enabled {} detected, disabled {} others.", enabled_count, disabled_count);
    if !detected.is_empty() {
        let mut names: Vec<&String> = detected.iter().collect();
        names.sort();
        println!("{}", color::dim(&format!("Active processors: {}", names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))));
    }
    Ok(())
}

/// Enable only processors whose files are detected AND tools are installed.
pub(crate) fn enable_if_available(available: &HashSet<String>) -> Result<()> {
    let mut doc = load_doc()?;
    let table = processor_table(&mut doc)?;
    let all_names = default_processors();
    let mut enabled_count = 0;

    for name in &all_names {
        let section = match table.get_mut(name.as_str()).and_then(|v| v.as_table_mut()) {
            Some(t) => t,
            None => continue,
        };

        if available.contains(name.as_str()) {
            let is_disabled = section.get("enabled")
                .and_then(|v| v.as_bool())
                .is_some_and(|b| !b);
            if is_disabled {
                section.remove("enabled");
                enabled_count += 1;
            }
        }
    }

    save_doc(&doc)?;
    println!("Enabled {} available processors in {}.", enabled_count, CONFIG_FILE);
    Ok(())
}

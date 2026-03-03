use anyhow::{Context, Result};
use std::fs;

use crate::color;
use crate::config::default_processors;

const CONFIG_FILE: &str = "rsb.toml";

/// Disable all processors by setting `enabled = false` in each `[processor.NAME]` section.
pub(crate) fn disable_all() -> Result<()> {
    let content = fs::read_to_string(CONFIG_FILE)
        .with_context(|| format!("Failed to read {}", CONFIG_FILE))?;
    let mut doc: toml_edit::DocumentMut = content.parse()
        .with_context(|| format!("Failed to parse {}", CONFIG_FILE))?;

    let processor_table = doc.entry("processor")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .context("[processor] must be a table")?;

    let all_names = default_processors();
    let mut count = 0;

    for name in &all_names {
        let section = processor_table.entry(name.as_str())
            .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_mut();
        let section = match section {
            Some(t) => t,
            None => continue,
        };

        // Set enabled = false if not already
        let already_disabled = section.get("enabled")
            .and_then(|v| v.as_bool())
            .is_some_and(|b| !b);
        if !already_disabled {
            section.insert("enabled", toml_edit::value(false));
            count += 1;
        }
    }

    fs::write(CONFIG_FILE, doc.to_string())
        .with_context(|| format!("Failed to write {}", CONFIG_FILE))?;

    println!("Disabled {} processors in {}.", count, CONFIG_FILE);
    println!("{}", color::dim("Hint: enable processors one by one with [processor.NAME] enabled = true"));
    Ok(())
}

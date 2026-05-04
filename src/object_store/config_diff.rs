use anyhow::{Context, Result};
use redb::ReadableDatabase;
use std::collections::BTreeMap;

use crate::color;

use super::{ObjectStore, CONFIGS_TABLE};

impl ObjectStore {
    /// Store a processor's config JSON for later comparison.
    /// Returns the previous config if it existed and was different.
    pub fn store_processor_config(&self, processor: &str, config_json: &str) -> Result<Option<String>> {
        // Read old value
        let old_value = {
            let read_txn = self.db.begin_read()
                .context("Failed to begin read transaction")?;
            match read_txn.open_table(CONFIGS_TABLE) {
                Ok(table) => {
                    table.get(processor).ok()
                        .flatten()
                        .and_then(|bytes| String::from_utf8(bytes.value().to_vec()).ok())
                }
                Err(_) => None,
            }
        };

        // Only update if changed
        let changed = old_value.as_ref() != Some(&config_json.to_string());
        if changed {
            let write_txn = self.db.begin_write()
                .context("Failed to begin write transaction")?;
            {
                let mut table = write_txn.open_table(CONFIGS_TABLE)
                    .context("Failed to open configs table")?;
                table.insert(processor, config_json.as_bytes())
                    .context("Failed to store processor config")?;
            }
            write_txn.commit()
                .context("Failed to commit processor config")?;
        }

        // Return old value only if it was different
        if changed {
            Ok(old_value)
        } else {
            Ok(None)
        }
    }

    /// Generate a colored diff between old and new config JSON.
    /// Returns None if configs are identical or if diffing fails.
    pub fn diff_configs(old_json: &str, new_json: &str) -> Option<String> {
        // Parse both as generic JSON values
        let old: serde_json::Value = serde_json::from_str(old_json).ok()?;
        let new: serde_json::Value = serde_json::from_str(new_json).ok()?;

        if old == new {
            return None;
        }

        // Convert to sorted maps for comparison
        let old_map = Self::flatten_json(&old, "");
        let new_map = Self::flatten_json(&new, "");

        let mut lines: Vec<String> = Vec::new();

        // Find removed and changed keys
        for (key, old_val) in &old_map {
            match new_map.get(key) {
                None => {
                    let s = format!("- {key}: {old_val}");
                    lines.push(color::red(&s).into_owned());
                }
                Some(new_val) if new_val != old_val => {
                    let old_s = format!("- {key}: {old_val}");
                    lines.push(color::red(&old_s).into_owned());
                    let new_s = format!("+ {key}: {new_val}");
                    lines.push(color::green(&new_s).into_owned());
                }
                _ => {}
            }
        }

        // Find added keys
        for (key, new_val) in &new_map {
            if !old_map.contains_key(key) {
                let s = format!("+ {key}: {new_val}");
                lines.push(color::green(&s).into_owned());
            }
        }

        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    /// Flatten a JSON value into a map of dotted keys to string values
    fn flatten_json(value: &serde_json::Value, prefix: &str) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();

        match value {
            serde_json::Value::Object(obj) => {
                for (k, v) in obj {
                    let key = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    map.extend(Self::flatten_json(v, &key));
                }
            }
            serde_json::Value::Array(arr) => {
                for (i, v) in arr.iter().enumerate() {
                    let key = format!("{prefix}[{i}]");
                    map.extend(Self::flatten_json(v, &key));
                }
            }
            _ => {
                let val_str = match value {
                    serde_json::Value::String(s) => format!("\"{s}\""),
                    serde_json::Value::Null => "null".to_string(),
                    v => v.to_string(),
                };
                map.insert(prefix.to_string(), val_str);
            }
        }

        map
    }
}

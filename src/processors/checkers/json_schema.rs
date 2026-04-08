use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;

use crate::config::JsonSchemaConfig;
use crate::graph::Product;

pub struct JsonSchemaProcessor {
    config: JsonSchemaConfig,
}

impl JsonSchemaProcessor {
    pub fn new(config: JsonSchemaConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        let path = product.primary_input();
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let value: Value = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse JSON in {}", path.display()))?;

        let mut errors = Vec::new();
        check_property_ordering(&value, "$", &mut errors);

        if !errors.is_empty() {
            anyhow::bail!(
                "propertyOrdering mismatch in {}:\n{}",
                path.display(),
                errors.join("\n")
            );
        }
        Ok(())
    }
}

/// Recursively check that every object with `type: "object"` has a
/// `propertyOrdering` array that exactly matches its `properties` keys.
fn check_property_ordering(value: &Value, path: &str, errors: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            // Check if this is a JSON Schema object definition
            let is_object_type = map.get("type")
                .and_then(|v| v.as_str())
                .is_some_and(|t| t == "object");

            if is_object_type {
                let has_properties = map.contains_key("properties");
                let has_ordering = map.contains_key("propertyOrdering");

                if has_properties && has_ordering {
                    let prop_keys: BTreeSet<&str> = map["properties"]
                        .as_object()
                        .map(|o| o.keys().map(|k| k.as_str()).collect())
                        .unwrap_or_default();

                    let ordering_keys: BTreeSet<&str> = map["propertyOrdering"]
                        .as_array()
                        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                        .unwrap_or_default();

                    let missing: Vec<&&str> = prop_keys.difference(&ordering_keys).collect();
                    let extra: Vec<&&str> = ordering_keys.difference(&prop_keys).collect();

                    if !missing.is_empty() {
                        errors.push(format!(
                            "  {}: missing from propertyOrdering: {:?}",
                            path, missing
                        ));
                    }
                    if !extra.is_empty() {
                        errors.push(format!(
                            "  {}: extra in propertyOrdering: {:?}",
                            path, extra
                        ));
                    }
                }
            }

            // Recurse into all values
            for (key, val) in map {
                let child_path = format!("{}.{}", path, key);
                check_property_ordering(val, &child_path, errors);
            }
        }
        Value::Array(arr) => {
            for (i, val) in arr.iter().enumerate() {
                let child_path = format!("{}[{}]", path, i);
                check_property_ordering(val, &child_path, errors);
            }
        }
        _ => {}
    }
}

impl_checker!(JsonSchemaProcessor,
    config: config,
    description: "Validate propertyOrdering in JSON schema files",
    name: crate::processors::names::JSON_SCHEMA,
    execute: execute_product,
    config_json: true,
    native: true,
);

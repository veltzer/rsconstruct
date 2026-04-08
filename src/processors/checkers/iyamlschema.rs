use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::Path;

use crate::config::IyamlschemaConfig;
use crate::graph::Product;

pub struct IyamlschemaProcessor {
    config: IyamlschemaConfig,
}

impl IyamlschemaProcessor {
    pub fn new(config: IyamlschemaConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let mut errors = Vec::new();

        for file in files {
            if let Err(e) = self.validate_file(file) {
                errors.push(format!("{}: {}", file.display(), e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            bail!("YAML schema validation failed:\n{}", errors.join("\n"))
        }
    }

    fn validate_file(&self, path: &Path) -> Result<()> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        // Parse YAML into a JSON Value (for jsonschema validation)
        let data: Value = serde_yml::from_str(&contents)
            .with_context(|| format!("Failed to parse YAML in {}", path.display()))?;

        // Extract $schema URL
        let schema_url = data.get("$schema")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("no $schema field found"))?;

        // Fetch schema (cached)
        let schema_str = crate::webcache::fetch(schema_url)
            .with_context(|| format!("Failed to fetch schema {}", schema_url))?;
        let schema: Value = serde_json::from_str(&schema_str)
            .with_context(|| format!("Failed to parse schema from {}", schema_url))?;

        // Validate data against schema
        let validator = jsonschema::validator_for(&schema)
            .with_context(|| format!("Failed to compile schema from {}", schema_url))?;

        let validation_errors: Vec<String> = validator.iter_errors(&data)
            .map(|e| format!("  {}: {}", e.instance_path(), e))
            .collect();

        if !validation_errors.is_empty() {
            bail!("schema validation errors:\n{}", validation_errors.join("\n"));
        }

        // Check property ordering
        if self.config.check_ordering {
            let mut ordering_errors = Vec::new();
            check_property_ordering(&data, &schema, "", &mut ordering_errors);

            if !ordering_errors.is_empty() {
                bail!("property ordering errors:\n{}", ordering_errors.join("\n"));
            }
        }

        Ok(())
    }
}

/// Recursively check that data object keys match the `propertyOrdering`
/// declared in the schema.
fn check_property_ordering(
    data: &Value,
    schema: &Value,
    path: &str,
    errors: &mut Vec<String>,
) {
    match (data, schema) {
        (Value::Object(data_map), Value::Object(schema_map)) => {
            // Check ordering at this level
            if let Some(Value::Array(expected_order)) = schema_map.get("propertyOrdering") {
                let expected: Vec<&str> = expected_order.iter()
                    .filter_map(|v| v.as_str())
                    .collect();

                let actual_keys: Vec<&str> = data_map.keys()
                    .map(|k| k.as_str())
                    .collect();

                // Filter actual keys to only those in the expected list
                let actual_ordered: Vec<&str> = actual_keys.iter()
                    .copied()
                    .filter(|k| expected.contains(k))
                    .collect();

                // Filter expected to only those present in data
                let expected_ordered: Vec<&str> = expected.iter()
                    .copied()
                    .filter(|k| actual_keys.contains(k))
                    .collect();

                if actual_ordered != expected_ordered {
                    let display_path = if path.is_empty() { "root" } else { path };
                    errors.push(format!(
                        "  {}: expected key order {:?}, got {:?}",
                        display_path, expected_ordered, actual_ordered,
                    ));
                }
            }

            // Recurse into properties
            if let Some(Value::Object(props)) = schema_map.get("properties") {
                for (key, prop_schema) in props {
                    if let Some(value) = data_map.get(key) {
                        let child_path = if path.is_empty() {
                            key.to_string()
                        } else {
                            format!("{}.{}", path, key)
                        };
                        check_property_ordering(value, prop_schema, &child_path, errors);
                    }
                }
            }

            // Recurse into items (for arrays-of-objects)
            if let Some(items_schema) = schema_map.get("items") {
                if let Value::Array(arr) = data {
                    for (i, item) in arr.iter().enumerate() {
                        let child_path = format!("{}[{}]", path, i);
                        check_property_ordering(item, items_schema, &child_path, errors);
                    }
                }
            }
        }
        (Value::Array(arr), schema_val) => {
            // Schema might have "items" at this level
            if let Some(items_schema) = schema_val.get("items") {
                for (i, item) in arr.iter().enumerate() {
                    let child_path = if path.is_empty() {
                        format!("[{}]", i)
                    } else {
                        format!("{}[{}]", path, i)
                    };
                    check_property_ordering(item, items_schema, &child_path, errors);
                }
            }
        }
        _ => {}
    }
}

impl_checker!(IyamlschemaProcessor,
    config: config,
    description: "Validate YAML files against JSON schemas (in-process)",
    name: crate::processors::names::IYAMLSCHEMA,
    execute: execute_product,
    config_json: true,
    native: true,
    batch: check_files,
);

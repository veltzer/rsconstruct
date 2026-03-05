use anyhow::{Context, Result};
use regex::Regex;
use std::sync::OnceLock;

use crate::errors;

/// Convert a toml::Value to its inline TOML string representation.
/// This is used for variable substitution to insert values into the config.
pub(super) fn value_to_toml_inline(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(value_to_toml_inline).collect();
            format!("[{}]", items.join(", "))
        }
        toml::Value::Table(table) => {
            let items: Vec<String> = table
                .iter()
                .map(|(k, v)| format!("{} = {}", k, value_to_toml_inline(v)))
                .collect();
            format!("{{ {} }}", items.join(", "))
        }
        toml::Value::Datetime(dt) => dt.to_string(),
    }
}

/// Check if a trimmed line is a TOML section header (e.g., `[section]` or `[section] # comment`).
fn is_section_header(trimmed: &str) -> bool {
    if !trimmed.starts_with('[') {
        return false;
    }
    // Strip trailing comment: "[section] # comment" -> "[section]"
    let header_part = trimmed.split('#').next().unwrap_or(trimmed).trim_end();
    header_part.ends_with(']')
}

/// Check if a trimmed line is specifically the `[vars]` section header.
fn is_vars_header(trimmed: &str) -> bool {
    if !trimmed.starts_with("[vars]") {
        return false;
    }
    // Allow trailing whitespace/comments: "[vars]", "[vars] # comment"
    let rest = trimmed["[vars]".len()..].trim_start();
    rest.is_empty() || rest.starts_with('#')
}

/// Remove the [vars] section from TOML content.
/// Removes from [vars] header until the next section header or EOF.
pub(super) fn remove_vars_section(content: &str) -> String {
    let mut in_vars_section = false;
    let lines: Vec<&str> = content.lines()
        .filter(|line| {
            let trimmed = line.trim();
            if is_vars_header(trimmed) {
                in_vars_section = true;
                return false;
            }
            if in_vars_section && is_section_header(trimmed) {
                in_vars_section = false;
            }
            !in_vars_section
        })
        .collect();
    let mut result = lines.join("\n");
    result.push('\n');
    result
}

/// Extract variable names defined in the [vars] section using regex.
/// This is done before TOML parsing to avoid parse errors on variable references.
pub(super) fn extract_var_names(content: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_vars_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if is_vars_header(trimmed) {
            in_vars_section = true;
            continue;
        }
        // Check if we hit another section header
        if in_vars_section && is_section_header(trimmed) {
            in_vars_section = false;
            continue;
        }
        if in_vars_section {
            // Match key = value pattern
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                if !key.is_empty() && !key.starts_with('#') {
                    names.push(key.to_string());
                }
            }
        }
    }
    names
}

/// Substitute variables defined in [vars] section throughout the config.
/// Variables are referenced using `${var_name}` syntax.
/// The entire `"${var_name}"` (including quotes) is replaced with the TOML-serialized value.
/// After substitution, the [vars] section is removed from the output.
pub(super) fn substitute_variables(content: &str) -> Result<String> {
    // Check for undefined variables first (before any TOML parsing)
    // This gives a clear error message for undefined vars even without a [vars] section
    // Matches quoted variable references like "${var_name}" (including the surrounding double quotes,
    // since variables in TOML values are written as "value" = "${var}").
    static VAR_PATTERN: OnceLock<Regex> = OnceLock::new();
    let var_pattern = VAR_PATTERN.get_or_init(|| Regex::new(r#""\$\{([^}]+)\}""#).expect(errors::INVALID_REGEX));

    // Extract defined variable names before TOML parsing
    let defined_vars = extract_var_names(content);

    // Check for undefined variable references
    for captures in var_pattern.captures_iter(content) {
        let var_name = captures.get(1).expect(errors::CAPTURE_GROUP_MISSING).as_str();
        if !defined_vars.iter().any(|v| v == var_name) {
            return Err(crate::exit_code::RsbuildError::new(
                crate::exit_code::RsbuildExitCode::ConfigError,
                format!("Undefined variable: ${{{}}}", var_name),
            ).into());
        }
    }

    // If no vars defined, return content as-is (we already checked for undefined refs above)
    if defined_vars.is_empty() {
        return Ok(content.to_string());
    }

    // Parse just to extract [vars] section values
    let parsed: toml::Value = toml::from_str(content)
        .context("Failed to parse TOML for variable extraction")?;

    let vars = match parsed.get("vars").and_then(|v| v.as_table()) {
        Some(v) => v,
        None => return Ok(content.to_string()),
    };

    let mut result = content.to_string();

    // Replace "${var_name}" (including quotes) with TOML-serialized value
    for (name, value) in vars {
        let pattern = format!("\"${{{}}}\"", name);
        let replacement = value_to_toml_inline(value);
        result = result.replace(&pattern, &replacement);
    }

    // Remove the [vars] section from the result
    let result = remove_vars_section(&result);

    Ok(result)
}

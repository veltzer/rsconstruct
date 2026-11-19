//! Variable substitution for `[vars]`.
//!
//! # Line-preservation invariant
//!
//! Substitution runs on the raw TOML text *before* `provenance` walks the
//! document for byte spans, and provenance reports user-set fields as
//! `rsconstruct.toml:<line>`. Those line numbers are only correct if
//! substitution never changes how many lines the file has. Two functions
//! here uphold that, and neither can be changed casually:
//!
//! - [`value_to_toml_inline`] must emit **no newlines**: a multi-line array
//!   or a string containing `\n` would push every later line down, so every
//!   provenance line number after the substitution point would point at the
//!   wrong line. Strings escape `\n`/`\r`; arrays and tables join inline.
//! - [`remove_vars_section`] **blanks** the `[vars]` lines instead of
//!   deleting them, for the same reason — removing them would shift every
//!   line below the section.
//!
//! This was previously stated only in scattered comments with nothing
//! enforcing it; `line_preservation_invariant_holds` and
//! `remove_vars_section_preserves_line_count` now pin both halves.

use anyhow::{Context, Result};
use regex::Regex;
use std::sync::OnceLock;

use crate::errors;

/// Convert a `toml::Value` to its inline TOML string representation.
/// This is used for variable substitution to insert values into the config.
///
/// **Must never emit a newline** — see the module-level invariant.
pub(super) fn value_to_toml_inline(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => format!(
            "\"{}\"",
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t")
        ),
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

/// The byte offset of every table header (`[a]`, `[a.b]`, `[[c]]`) below
/// `table`, in any order.
fn header_starts(table: &toml_edit::Table, starts: &mut Vec<usize>) {
    for (_, item) in table {
        match item {
            toml_edit::Item::Table(sub) => {
                starts.extend(sub.span().map(|span| span.start));
                header_starts(sub, starts);
            }
            toml_edit::Item::ArrayOfTables(array) => {
                for sub in array {
                    starts.extend(sub.span().map(|span| span.start));
                    header_starts(sub, starts);
                }
            }
            _ => {}
        }
    }
}

/// Remove the [vars] section from TOML content by blanking its lines.
/// Blanking (instead of deleting) keeps every remaining line at its original
/// line number, so provenance spans built from the result stay correct.
///
/// The section is found by parsing: it runs from its header to the next
/// table header of the document. Judging each line by its looks took the
/// `["a"]` line of a multi-line array inside `[vars]` for a header, ended
/// the section there, and left half an array behind as a syntax error on
/// valid TOML.
///
/// Variables are a `[vars]` table. Written any other way (`vars.a = 1`,
/// `vars = { a = 1 }`) there is no run of lines that is the section, and
/// that is an error rather than a config the rest of the load then rejects
/// with "unknown field 'vars'".
pub(super) fn remove_vars_section(content: &str) -> Result<String> {
    // A document that does not parse is reported by `Config::load`, which
    // parses it next and has the whole message to give.
    let Ok(doc) = toml_edit::Document::parse(content) else {
        return Ok(content.to_string());
    };
    let Some(vars) = doc.as_table().get("vars") else {
        return Ok(content.to_string());
    };
    let Some(start) = vars
        .as_table()
        .filter(|table| !table.is_dotted())
        .and_then(toml_edit::Table::span)
        .map(|span| span.start)
    else {
        return Err(crate::exit_code::config_error(
            "Variables must be written as a [vars] table with one `name = value` line each; \
             `vars.name = value` and `vars = { ... }` are not supported",
        ));
    };
    let mut starts = Vec::new();
    header_starts(doc.as_table(), &mut starts);
    let end = starts
        .into_iter()
        .filter(|&other| other > start)
        .min()
        .unwrap_or(content.len());

    let mut result = String::with_capacity(content.len());
    let mut offset = 0;
    for line in content.lines() {
        // The header line holds `start`; every later line that begins
        // before the next header belongs to the section too.
        let line_end = offset + line.len();
        let in_section = (offset <= start && start < line_end) || (offset > start && offset < end);
        if !in_section {
            result.push_str(line);
        }
        result.push('\n');
        // `lines` drops the terminator: one byte for `\n`, two for `\r\n`.
        offset = line_end
            + if content[line_end..].starts_with("\r\n") {
                2
            } else {
                1
            };
    }
    Ok(result)
}

/// Strip a TOML line comment (a `#` outside of any string literal).
/// Used only for the undefined-variable scan, so `${...}` inside comments
/// doesn't fail config loading.
fn strip_toml_comment(line: &str) -> &str {
    let mut in_basic = false; // "..."
    let mut in_literal = false; // '...'
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_basic => escaped = true,
            '"' if !in_literal => in_basic = !in_basic,
            '\'' if !in_basic => in_literal = !in_literal,
            '#' if !in_basic && !in_literal => return &line[..i],
            _ => {}
        }
    }
    line
}

/// Resolve a var value, following `${name}` references to other vars.
/// Nested references resolve regardless of definition order; cycles error out.
fn resolve_var_value(
    value: &toml::Value,
    vars: &toml::map::Map<String, toml::Value>,
    depth: usize,
) -> Result<toml::Value> {
    if depth > 32 {
        return Err(crate::exit_code::RsconstructError::new(
            crate::exit_code::RsconstructExitCode::ConfigError,
            "Variable reference cycle in [vars] (nesting exceeds 32 levels)".to_string(),
        )
        .into());
    }
    match value {
        toml::Value::String(s) => {
            if let Some(name) = s.strip_prefix("${").and_then(|r| r.strip_suffix('}'))
                && !name.contains('}')
            {
                let Some(referenced) = vars.get(name) else {
                    return Err(crate::exit_code::RsconstructError::new(
                        crate::exit_code::RsconstructExitCode::ConfigError,
                        format!("Undefined variable: ${{{name}}}"),
                    )
                    .into());
                };
                return resolve_var_value(referenced, vars, depth + 1);
            }
            Ok(value.clone())
        }
        toml::Value::Array(arr) => arr
            .iter()
            .map(|v| resolve_var_value(v, vars, depth + 1))
            .collect::<Result<Vec<_>>>()
            .map(toml::Value::Array),
        toml::Value::Table(table) => table
            .iter()
            .map(|(k, v)| resolve_var_value(v, vars, depth + 1).map(|rv| (k.clone(), rv)))
            .collect::<Result<toml::map::Map<_, _>>>()
            .map(toml::Value::Table),
        _ => Ok(value.clone()),
    }
}

/// Extract the variable names defined in the `[vars]` section.
///
/// Parses the TOML rather than scanning lines. An unsubstituted `${var}` is
/// just string content as far as TOML is concerned, so parsing here is safe
/// — the older line scanner predates that realization and split each line on
/// the first `=`, which recorded junk names for any multi-line array whose
/// items contained one:
///
/// ```toml
/// [vars]
/// patterns = [
///     "a=b",     # recorded a bogus variable named `"a`
/// ]
/// ```
///
/// Junk names made the undefined-variable check too permissive: a genuinely
/// undefined `${a}` would pass validation because a bogus `a` was "defined".
///
/// Returns an empty list when the content does not parse or has no `[vars]`
/// section. A parse error is not reported here — `Config::load` parses again
/// and produces a far better message.
///
/// `substitute_variables` does not call this: it needs the parsed document
/// for the variable *values* too, so it parses once and calls
/// [`var_names_of`] directly. This wrapper is the name-only entry point, kept
/// because the parse-and-extract pair is what the regression tests pin.
#[cfg(test)]
pub(super) fn extract_var_names(content: &str) -> Vec<String> {
    let Ok(parsed) = toml::from_str::<toml::Value>(content) else {
        return Vec::new();
    };
    var_names_of(&parsed)
}

/// The `[vars]` keys of an already-parsed document.
fn var_names_of(parsed: &toml::Value) -> Vec<String> {
    parsed
        .get("vars")
        .and_then(toml::Value::as_table)
        .map(|vars| vars.keys().cloned().collect())
        .unwrap_or_default()
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
    let var_pattern =
        VAR_PATTERN.get_or_init(|| Regex::new(r#""\$\{([^}]+)\}""#).expect(errors::INVALID_REGEX));

    // One parse serves both the name check and the value lookup below.
    // A parse failure is deliberately not fatal here: the undefined-variable
    // scan is textual, so it still produces its (much more specific) message
    // for a config that also has a syntax error. `Config::load` reports the
    // syntax error itself right after.
    let parsed = toml::from_str::<toml::Value>(content).ok();
    let defined_vars = parsed.as_ref().map(var_names_of).unwrap_or_default();

    // Check for undefined variable references, ignoring `${...}` in comments
    for (index, line) in content.lines().enumerate() {
        for captures in var_pattern.captures_iter(strip_toml_comment(line)) {
            let var_name = captures
                .get(1)
                .expect(errors::CAPTURE_GROUP_MISSING)
                .as_str();
            if !defined_vars.iter().any(|v| v == var_name) {
                return Err(crate::exit_code::RsconstructError::new(
                    crate::exit_code::RsconstructExitCode::ConfigError,
                    format!("line {}: Undefined variable: ${{{var_name}}}", index + 1),
                )
                .into());
            }
        }
    }

    // If no vars defined, return content as-is (we already checked for undefined refs above)
    if defined_vars.is_empty() {
        check_no_residual_references(content)?;
        return Ok(content.to_string());
    }

    // defined_vars is non-empty, so the parse succeeded and `[vars]` is a table.
    let parsed = parsed.context("Failed to parse TOML for variable extraction")?;
    let Some(vars) = parsed.get("vars").and_then(|v| v.as_table()) else {
        check_no_residual_references(content)?;
        return Ok(content.to_string());
    };

    let mut result = content.to_string();

    // Replace "${var_name}" (including quotes) with the TOML-serialized value.
    // Values are fully resolved first so a var referencing another var works
    // regardless of definition order.
    for (name, value) in vars {
        let pattern = format!("\"${{{name}}}\"");
        let resolved = resolve_var_value(value, vars, 0)?;
        let replacement = value_to_toml_inline(&resolved);
        result = result.replace(&pattern, &replacement);
    }

    // Remove the [vars] section from the result
    let result = remove_vars_section(&result)?;

    check_no_residual_references(&result)?;
    Ok(result)
}

/// Error on any `${...}` left after substitution. The engine only replaces
/// whole quoted values (`"${var}"`), so a partial reference like
/// `"${base}/src"` matches neither the substitution nor the quoted
/// undefined-var scan and would otherwise flow through silently as a
/// literal. Comments are exempt, like everywhere else in this pipeline.
fn check_no_residual_references(content: &str) -> Result<()> {
    static RESIDUAL_PATTERN: OnceLock<Regex> = OnceLock::new();
    let residual =
        RESIDUAL_PATTERN.get_or_init(|| Regex::new(r"\$\{([^}]+)\}").expect(errors::INVALID_REGEX));
    // Substitution keeps every line where it was (see the module-level
    // invariant), so a line number here is one in the file.
    for (index, line) in content.lines().enumerate() {
        if let Some(captures) = residual.captures(strip_toml_comment(line)) {
            let var_name = captures
                .get(1)
                .expect(errors::CAPTURE_GROUP_MISSING)
                .as_str();
            return Err(crate::exit_code::RsconstructError::new(
                crate::exit_code::RsconstructExitCode::ConfigError,
                format!(
                    "line {}: Unresolved variable reference ${{{var_name}}}: a reference must be the entire quoted value (\"${{{var_name}}}\"), not embedded in a larger string",
                    index + 1
                ),
            ).into());
        }
    }
    Ok(())
}

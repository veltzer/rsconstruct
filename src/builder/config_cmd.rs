use anyhow::Result;
use crate::cli::ConfigAction;
use crate::color;
use crate::config::{Config, FieldProvenance};
use super::{Builder, ValidationSeverity, ValidationIssue, sorted_keys};

impl Builder {
    /// Handle `rsconstruct config` subcommands
    pub fn config(&self, action: ConfigAction) -> Result<()> {
        match action {
            ConfigAction::Show => {
                let output = toml::to_string_pretty(&self.config)?;
                let annotated = Self::annotate_config(&output, Some(&self.config));
                println!("{}", annotated);
            }
            ConfigAction::ShowDefault => {
                let config = Config::default();
                let output = toml::to_string_pretty(&config)?;
                let annotated = Self::annotate_config(&output, None);
                println!("{}", annotated);
            }
            ConfigAction::Validate => {
                let issues = self.validate_config();

                if crate::json_output::is_json_mode() {
                    let json_issues: Vec<serde_json::Value> = issues.iter()
                        .map(|issue| {
                            let severity = match issue.severity {
                                ValidationSeverity::Error => "error",
                                ValidationSeverity::Warning => "warning",
                            };
                            serde_json::json!({
                                "severity": severity,
                                "message": issue.message,
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&json_issues)?);
                } else if issues.is_empty() {
                    println!("{}", color::green("Config OK"));
                } else {
                    for issue in &issues {
                        let label = match issue.severity {
                            ValidationSeverity::Error => color::red("ERROR"),
                            ValidationSeverity::Warning => color::yellow("WARNING"),
                        };
                        println!("{}: {}", label, issue.message);
                    }
                    let error_count = issues.iter()
                        .filter(|i| matches!(i.severity, ValidationSeverity::Error))
                        .count();
                    let warning_count = issues.iter()
                        .filter(|i| matches!(i.severity, ValidationSeverity::Warning))
                        .count();
                    println!();
                    println!("{}: {} error(s), {} warning(s)",
                        color::bold("Summary"), error_count, warning_count);

                    if error_count > 0 {
                        return Err(crate::exit_code::RsconstructError::new(
                            crate::exit_code::RsconstructExitCode::ConfigError,
                            format!("Config validation failed with {} error(s)", error_count),
                        ).into());
                    }
                }
            }
        }

        Ok(())
    }

    /// Validate the configuration and return all issues found.
    pub(super) fn validate_config(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        let processors = match self.create_processors() {
            Ok(p) => p,
            Err(e) => {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    message: format!("Failed to create processors: {}", e),
                });
                return issues;
            }
        };

        // Check: Unknown processor types (not builtin, not a Lua plugin)
        for name in self.config.processor.extra.keys() {
            let plugin_path = std::path::Path::new(&self.config.plugins.dir)
                .join(format!("{}.lua", name));
            if !plugin_path.exists() {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    message: format!("Unknown processor type '{}' (not a builtin processor or Lua plugin)", name),
                });
            }
        }

        // Check: Required tools on PATH for declared processors
        for name in sorted_keys(&processors) {
            let processor = &processors[name];
            for tool in processor.required_tools() {
                if which::which(&tool).is_err() {
                    issues.push(ValidationIssue {
                        severity: ValidationSeverity::Warning,
                        message: format!("Tool '{}' required by processor '{}' not found on PATH", tool, name),
                    });
                }
            }
        }

        // Check: No matching files detected for declared processor
        for name in sorted_keys(&processors) {
            let processor = &processors[name];
            if !processor.auto_detect(&self.file_index) {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    message: format!("Processor '{}' is declared but no matching files detected", name),
                });
            }
        }

        issues
    }

    /// Annotate TOML config output with comments for constrained values, and —
    /// when `config` is provided — append a provenance comment to every field
    /// line showing whether the field came from the user's TOML, a processor
    /// default, a scan default, or a serde default.
    ///
    /// `config` is `None` when printing the default config (`config show --default`)
    /// since provenance isn't meaningful there.
    pub(super) fn annotate_config(toml: &str, config: Option<&Config>) -> String {
        let mut section: Option<SectionContext> = None;
        toml.lines()
            .map(|line| annotate_line(line, &mut section, config))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Which `[section]` a line belongs to. Instance sections carry the instance
/// name so we can look up the right ProcessorInstance/AnalyzerInstance.
#[derive(Debug, Clone)]
enum SectionContext {
    Global(String),                     // [build], [cache], [graph], …
    Processor { instance_name: String }, // [processor.ruff] or [processor.pylint.core]
    Analyzer { instance_name: String },  // [analyzer.cpp] or [analyzer.cpp.kernel]
    Other,                               // unrecognized section — no provenance
}

fn annotate_line(
    line: &str,
    section: &mut Option<SectionContext>,
    config: Option<&Config>,
) -> String {
    let trimmed = line.trim();

    // Section header: [name] or [processor.pylint.core]
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        let inner = &trimmed[1..trimmed.len() - 1];
        *section = Some(classify_section(inner));
        return line.to_string();
    }

    // Field line: key = value. We need the literal key as it appears in the
    // serialized TOML (not stripped, since toml::to_string_pretty preserves
    // the raw field name).
    let key = match extract_key(line) {
        Some(k) => k,
        None => return line.to_string(),
    };

    // Existing baseline annotations — retained.
    let base_comment = if key == "parallel" {
        Some("0 = auto-detect CPU cores".to_string())
    } else if key == "restore_method" {
        Some("options: auto, hardlink, copy (auto = copy in CI, hardlink otherwise)".to_string())
    } else {
        None
    };

    // Provenance annotation.
    let provenance_comment = config
        .zip(section.as_ref())
        .and_then(|(cfg, ctx)| lookup_provenance(cfg, ctx, key))
        .map(|p| p.to_string());

    match (base_comment, provenance_comment) {
        (None, None) => line.to_string(),
        (Some(b), None) => format!("{} # {}", line, b),
        (None, Some(p)) => format!("{}  # {}", line, p),
        (Some(b), Some(p)) => format!("{} # {} | {}", line, b, p),
    }
}

/// Turn a section header's inner text into a SectionContext.
/// `processor.ruff` → Processor { instance_name: "ruff" }.
/// `processor.pylint.core` → Processor { instance_name: "pylint.core" }.
/// `analyzer.cpp.kernel` → Analyzer { instance_name: "cpp.kernel" }.
/// `build` → Global("build").
fn classify_section(inner: &str) -> SectionContext {
    if let Some(rest) = inner.strip_prefix("processor.") {
        return SectionContext::Processor { instance_name: rest.to_string() };
    }
    if let Some(rest) = inner.strip_prefix("analyzer.") {
        return SectionContext::Analyzer { instance_name: rest.to_string() };
    }
    match inner {
        "build" | "cache" | "completions" | "graph" | "plugins"
            | "dependencies" | "command" => SectionContext::Global(inner.to_string()),
        _ => SectionContext::Other,
    }
}

/// Extract the key portion of a `key = value` line. Returns None for comments,
/// blank lines, and section headers.
fn extract_key(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
        return None;
    }
    let eq = line.find('=')?;
    let key_part = line[..eq].trim();
    if key_part.is_empty() {
        return None;
    }
    // toml::to_string_pretty emits unquoted bare keys for ordinary identifiers.
    // If the key is quoted, strip the quotes — but provenance was recorded
    // under the bare key name.
    let unquoted = key_part.trim_matches('"').trim_matches('\'');
    Some(unquoted)
}

fn lookup_provenance(
    config: &Config,
    section: &SectionContext,
    field: &str,
) -> Option<FieldProvenance> {
    match section {
        SectionContext::Processor { instance_name } => config
            .processor
            .instances
            .iter()
            .find(|i| i.instance_name == *instance_name)
            .and_then(|i| i.provenance.get(field).cloned()),
        SectionContext::Analyzer { instance_name } => config
            .analyzer
            .instances
            .iter()
            .find(|i| i.instance_name == *instance_name)
            .and_then(|i| i.provenance.get(field).cloned()),
        SectionContext::Global(name) => config
            .global_provenance
            .get(name)
            .and_then(|m| m.get(field).cloned()),
        SectionContext::Other => None,
    }
}

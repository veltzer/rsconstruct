use anyhow::Result;
use crate::cli::ConfigAction;
use crate::color;
use crate::config::Config;
use super::{Builder, ValidationSeverity, ValidationIssue};

impl Builder {
    /// Handle `rsbuild config` subcommands
    pub fn config(&self, action: ConfigAction) -> Result<()> {
        match action {
            ConfigAction::Show => {
                let output = toml::to_string_pretty(&self.config)?;
                let annotated = Self::annotate_config(&output);
                println!("{}", annotated);
            }
            ConfigAction::ShowDefault => {
                let config = Config::default();
                let output = toml::to_string_pretty(&config)?;
                let annotated = Self::annotate_config(&output);
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
                        return Err(crate::exit_code::RsbuildError::new(
                            crate::exit_code::RsbuildExitCode::ConfigError,
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

        // Check 1: Enabled processor names are valid
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

        for name in &self.config.processor.enabled {
            if !processors.contains_key(name) {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Error,
                    message: format!("Unknown processor '{}' in enabled list", name),
                });
            }
        }

        // Check 2: Required tools on PATH for enabled processors
        for name in &self.config.processor.enabled {
            if let Some(processor) = processors.get(name) {
                for tool in processor.required_tools() {
                    if which::which(&tool).is_err() {
                        issues.push(ValidationIssue {
                            severity: ValidationSeverity::Warning,
                            message: format!("Tool '{}' required by processor '{}' not found on PATH", tool, name),
                        });
                    }
                }
            }
        }

        // Check 3: Auto-detect mismatch (processor enabled but no matching files)
        for name in &self.config.processor.enabled {
            if let Some(processor) = processors.get(name)
                && !processor.auto_detect(&self.file_index)
            {
                issues.push(ValidationIssue {
                    severity: ValidationSeverity::Warning,
                    message: format!("Processor '{}' is enabled but no matching files detected", name),
                });
            }
        }

        issues
    }

    /// Annotate TOML config output with comments for constrained values
    pub(super) fn annotate_config(toml: &str) -> String {
        toml.lines()
            .map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("parallel = ") {
                    format!("{} # 0 = auto-detect CPU cores", line)
                } else if trimmed.starts_with("restore_method = ") {
                    format!("{} # options: hardlink, copy", line)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

use std::fmt;

/// Exit codes for rsb, allowing CI scripts to distinguish error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsbExitCode {
    Success = 0,
    BuildError = 1,
    ConfigError = 2,
    ToolError = 3,
    GraphError = 4,
    #[allow(dead_code)]
    IoError = 5,
    Interrupted = 130,
}

impl RsbExitCode {
    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Success => "SUCCESS",
            Self::BuildError => "BUILD_ERROR",
            Self::ConfigError => "CONFIG_ERROR",
            Self::ToolError => "TOOL_ERROR",
            Self::GraphError => "GRAPH_ERROR",
            Self::IoError => "IO_ERROR",
            Self::Interrupted => "INTERRUPTED",
        }
    }
}

/// A typed error that carries an exit code for classification.
#[derive(Debug)]
pub struct RsbError {
    pub exit_code: RsbExitCode,
    pub message: String,
}

impl RsbError {
    pub fn new(exit_code: RsbExitCode, message: impl Into<String>) -> Self {
        Self {
            exit_code,
            message: message.into(),
        }
    }
}

impl fmt::Display for RsbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RsbError {}

/// Classify an anyhow error into an exit code.
/// First tries downcasting to RsbError, then falls back to message pattern matching.
pub fn classify_error(err: &anyhow::Error) -> RsbExitCode {
    // Primary: downcast to our typed error
    if let Some(rsb_err) = err.downcast_ref::<RsbError>() {
        return rsb_err.exit_code;
    }

    // Fallback: message pattern matching
    let msg = format!("{:#}", err);
    let lower = msg.to_lowercase();

    if lower.contains("interrupted") || lower.contains("ctrl+c") {
        RsbExitCode::Interrupted
    } else if lower.contains("no rsb.toml found")
        || lower.contains("rsb.toml already exists")
        || lower.contains("unknown processor")
        || lower.contains("unknown shell")
        || lower.contains("undefined variable")
        || lower.contains("failed to parse config")
        || lower.contains("failed to substitute variables")
        || lower.contains("deny_unknown_fields")
        || lower.contains("unknown field")
    {
        RsbExitCode::ConfigError
    } else if lower.contains("tool version mismatch")
        || lower.contains("tools are missing")
    {
        RsbExitCode::ToolError
    } else if lower.contains("cycle detected")
        || lower.contains("output conflict")
    {
        RsbExitCode::GraphError
    } else if lower.contains("build completed with") && lower.contains("error") {
        RsbExitCode::BuildError
    } else {
        // Default to BuildError for unclassified errors
        RsbExitCode::BuildError
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_rsb_error_downcast() {
        let err: anyhow::Error = RsbError::new(RsbExitCode::ConfigError, "bad config").into();
        assert_eq!(classify_error(&err), RsbExitCode::ConfigError);

        let err: anyhow::Error = RsbError::new(RsbExitCode::GraphError, "cycle").into();
        assert_eq!(classify_error(&err), RsbExitCode::GraphError);
    }

    #[test]
    fn classify_interrupted() {
        let err = anyhow::anyhow!("Build was interrupted by Ctrl+C");
        assert_eq!(classify_error(&err), RsbExitCode::Interrupted);

        let err = anyhow::anyhow!("operation interrupted");
        assert_eq!(classify_error(&err), RsbExitCode::Interrupted);
    }

    #[test]
    fn classify_config_errors() {
        for msg in [
            "No rsb.toml found in current directory",
            "rsb.toml already exists",
            "unknown processor 'foo'",
            "unknown shell 'fish'",
            "undefined variable 'x'",
            "failed to parse config",
            "failed to substitute variables",
            "unknown field `blah`",
        ] {
            assert_eq!(classify_error(&anyhow::anyhow!("{}", msg)), RsbExitCode::ConfigError,
                "expected ConfigError for: {}", msg);
        }
    }

    #[test]
    fn classify_tool_errors() {
        let err = anyhow::anyhow!("tool version mismatch: gcc 12 vs 13");
        assert_eq!(classify_error(&err), RsbExitCode::ToolError);

        let err = anyhow::anyhow!("Required tools are missing: ruff");
        assert_eq!(classify_error(&err), RsbExitCode::ToolError);
    }

    #[test]
    fn classify_graph_errors() {
        let err = anyhow::anyhow!("Cycle detected in dependency graph");
        assert_eq!(classify_error(&err), RsbExitCode::GraphError);

        let err = anyhow::anyhow!("Output conflict: foo.o produced by both [cc] and [cc2]");
        assert_eq!(classify_error(&err), RsbExitCode::GraphError);
    }

    #[test]
    fn classify_build_error() {
        let err = anyhow::anyhow!("Build completed with 3 error(s)");
        assert_eq!(classify_error(&err), RsbExitCode::BuildError);
    }

    #[test]
    fn classify_unknown_defaults_to_build_error() {
        let err = anyhow::anyhow!("something totally unexpected");
        assert_eq!(classify_error(&err), RsbExitCode::BuildError);
    }

    #[test]
    fn exit_codes_have_correct_values() {
        assert_eq!(RsbExitCode::Success.code(), 0);
        assert_eq!(RsbExitCode::BuildError.code(), 1);
        assert_eq!(RsbExitCode::ConfigError.code(), 2);
        assert_eq!(RsbExitCode::ToolError.code(), 3);
        assert_eq!(RsbExitCode::GraphError.code(), 4);
        assert_eq!(RsbExitCode::IoError.code(), 5);
        assert_eq!(RsbExitCode::Interrupted.code(), 130);
    }
}

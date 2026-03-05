use std::fmt;

/// Exit codes for rsbuild, allowing CI scripts to distinguish error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsbuildExitCode {
    Success = 0,
    BuildError = 1,
    ConfigError = 2,
    ToolError = 3,
    GraphError = 4,
    #[allow(dead_code)] // Reserved for future I/O-specific error classification
    IoError = 5,
    Interrupted = 130,
}

impl RsbuildExitCode {
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
pub struct RsbuildError {
    pub exit_code: RsbuildExitCode,
    pub message: String,
}

impl RsbuildError {
    pub fn new(exit_code: RsbuildExitCode, message: impl Into<String>) -> Self {
        Self {
            exit_code,
            message: message.into(),
        }
    }
}

impl fmt::Display for RsbuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RsbuildError {}

/// Classify an anyhow error into an exit code.
/// First tries downcasting to RsbuildError, then falls back to message pattern matching.
pub fn classify_error(err: &anyhow::Error) -> RsbuildExitCode {
    // Primary: downcast to our typed error
    if let Some(rsbuild_err) = err.downcast_ref::<RsbuildError>() {
        return rsbuild_err.exit_code;
    }

    // Fallback: message pattern matching
    let msg = format!("{:#}", err);
    let lower = msg.to_lowercase();

    if lower.contains("interrupted") || lower.contains("ctrl+c") {
        RsbuildExitCode::Interrupted
    } else if lower.contains("no rsbuild.toml found")
        || lower.contains("rsbuild.toml already exists")
        || lower.contains("unknown processor")
        || lower.contains("unknown shell")
        || lower.contains("undefined variable")
        || lower.contains("failed to parse config")
        || lower.contains("failed to substitute variables")
        || lower.contains("deny_unknown_fields")
        || lower.contains("unknown field")
        || lower.contains("invalid config")
    {
        RsbuildExitCode::ConfigError
    } else if lower.contains("tool version mismatch")
        || lower.contains("tools are missing")
    {
        RsbuildExitCode::ToolError
    } else if lower.contains("cycle detected")
        || lower.contains("output conflict")
    {
        RsbuildExitCode::GraphError
    } else if lower.contains("build completed with") && lower.contains("error") {
        RsbuildExitCode::BuildError
    } else {
        // Default to BuildError for unclassified errors
        RsbuildExitCode::BuildError
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_rsbuild_error_downcast() {
        let err: anyhow::Error = RsbuildError::new(RsbuildExitCode::ConfigError, "bad config").into();
        assert_eq!(classify_error(&err), RsbuildExitCode::ConfigError);

        let err: anyhow::Error = RsbuildError::new(RsbuildExitCode::GraphError, "cycle").into();
        assert_eq!(classify_error(&err), RsbuildExitCode::GraphError);
    }

    #[test]
    fn classify_interrupted() {
        let err = anyhow::anyhow!("Build was interrupted by Ctrl+C");
        assert_eq!(classify_error(&err), RsbuildExitCode::Interrupted);

        let err = anyhow::anyhow!("operation interrupted");
        assert_eq!(classify_error(&err), RsbuildExitCode::Interrupted);
    }

    #[test]
    fn classify_config_errors() {
        for msg in [
            "No rsbuild.toml found in current directory",
            "rsbuild.toml already exists",
            "unknown processor 'foo'",
            "unknown shell 'fish'",
            "undefined variable 'x'",
            "failed to parse config",
            "failed to substitute variables",
            "unknown field `blah`",
            "Invalid config:\n[processor.pandoc]: field 'scan_dir' must be a string",
        ] {
            assert_eq!(classify_error(&anyhow::anyhow!("{}", msg)), RsbuildExitCode::ConfigError,
                "expected ConfigError for: {}", msg);
        }
    }

    #[test]
    fn classify_tool_errors() {
        let err = anyhow::anyhow!("tool version mismatch: gcc 12 vs 13");
        assert_eq!(classify_error(&err), RsbuildExitCode::ToolError);

        let err = anyhow::anyhow!("Required tools are missing: ruff");
        assert_eq!(classify_error(&err), RsbuildExitCode::ToolError);
    }

    #[test]
    fn classify_graph_errors() {
        let err = anyhow::anyhow!("Cycle detected in dependency graph");
        assert_eq!(classify_error(&err), RsbuildExitCode::GraphError);

        let err = anyhow::anyhow!("Output conflict: foo.o produced by both [cc] and [cc2]");
        assert_eq!(classify_error(&err), RsbuildExitCode::GraphError);
    }

    #[test]
    fn classify_build_error() {
        let err = anyhow::anyhow!("Build completed with 3 error(s)");
        assert_eq!(classify_error(&err), RsbuildExitCode::BuildError);
    }

    #[test]
    fn classify_unknown_defaults_to_build_error() {
        let err = anyhow::anyhow!("something totally unexpected");
        assert_eq!(classify_error(&err), RsbuildExitCode::BuildError);
    }

    #[test]
    fn exit_codes_have_correct_values() {
        assert_eq!(RsbuildExitCode::Success.code(), 0);
        assert_eq!(RsbuildExitCode::BuildError.code(), 1);
        assert_eq!(RsbuildExitCode::ConfigError.code(), 2);
        assert_eq!(RsbuildExitCode::ToolError.code(), 3);
        assert_eq!(RsbuildExitCode::GraphError.code(), 4);
        assert_eq!(RsbuildExitCode::IoError.code(), 5);
        assert_eq!(RsbuildExitCode::Interrupted.code(), 130);
    }
}

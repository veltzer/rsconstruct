use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::color;
use crate::processors::ProductDiscovery;

const LOCK_FILE: &str = ".tools.versions";
const LOCK_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolLockFile {
    pub version: u32,
    pub locked_at: String,
    pub tools: BTreeMap<String, LockedTool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LockedTool {
    pub path: String,
    pub version_output: String,
    pub version_args: Vec<String>,
}

/// Query a single tool for its version information.
fn query_tool_version(tool_name: &str, version_args: &[String]) -> Result<LockedTool> {
    let path = which::which(tool_name)
        .with_context(|| format!("Tool not found on PATH: {}", tool_name))?;

    let mut cmd = Command::new(&path);
    for arg in version_args {
        cmd.arg(arg);
    }

    let output = cmd.output()
        .with_context(|| format!("Failed to run: {} {}", path.display(), version_args.join(" ")))?;

    // Some tools write version to stdout, others to stderr; capture both
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let version_output = if stdout.is_empty() {
        stderr
    } else if stderr.is_empty() {
        stdout
    } else {
        format!("{}\n{}", stdout, stderr)
    };

    if version_output.is_empty() {
        anyhow::bail!(
            "Tool '{}' produced no version output with args: {}",
            tool_name,
            version_args.join(" ")
        );
    }

    Ok(LockedTool {
        path: path.display().to_string(),
        version_output,
        version_args: version_args.to_vec(),
    })
}

/// Collect tool version commands from all enabled processors, deduplicated.
pub fn collect_tool_commands(
    processors: &std::collections::HashMap<String, Box<dyn ProductDiscovery>>,
    enabled: &dyn Fn(&str) -> bool,
) -> Vec<(String, Vec<String>)> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    let mut names: Vec<&String> = processors.keys().collect();
    names.sort();

    for name in names {
        if !enabled(name) {
            continue;
        }
        for (tool, args) in processors[name].tool_version_commands() {
            if seen.insert(tool.clone()) {
                result.push((tool, args));
            }
        }
    }

    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

/// Query all tools and build a lock file structure.
pub fn create_lock(
    tool_commands: &[(String, Vec<String>)],
) -> Result<ToolLockFile> {
    let mut tools = BTreeMap::new();

    for (tool_name, version_args) in tool_commands {
        let locked = query_tool_version(tool_name, version_args)?;
        tools.insert(tool_name.clone(), locked);
    }

    let locked_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    Ok(ToolLockFile {
        version: LOCK_VERSION,
        locked_at,
        tools,
    })
}

/// Write the lock file to disk.
pub fn write_lock_file(project_root: &Path, lock: &ToolLockFile) -> Result<()> {
    let path = project_root.join(LOCK_FILE);
    let json = serde_json::to_string_pretty(lock)
        .context("Failed to serialize lock file")?;
    fs::write(&path, format!("{}\n", json))
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Read the lock file from disk. Returns None if it doesn't exist.
pub fn read_lock_file(project_root: &Path) -> Result<Option<ToolLockFile>> {
    let path = project_root.join(LOCK_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let lock: ToolLockFile = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(lock))
}

/// Verify that the current tool versions match the lock file.
/// Returns Ok(()) if everything matches, or an error describing mismatches.
pub fn verify_lock_file(
    project_root: &Path,
    tool_commands: &[(String, Vec<String>)],
) -> Result<()> {
    let lock = match read_lock_file(project_root)? {
        Some(lock) => lock,
        None => {
            eprintln!(
                "{} No .tools.versions file found. Run 'rsb tools lock' to create one.",
                color::yellow("warning:")
            );
            return Ok(());
        }
    };

    let mut mismatches = Vec::new();

    for (tool_name, version_args) in tool_commands {
        let locked = match lock.tools.get(tool_name) {
            Some(l) => l,
            None => {
                mismatches.push(format!(
                    "  {} — not in lock file (new tool?)",
                    tool_name
                ));
                continue;
            }
        };

        // Query current version
        let current = match query_tool_version(tool_name, version_args) {
            Ok(c) => c,
            Err(e) => {
                mismatches.push(format!("  {} — {}", tool_name, e));
                continue;
            }
        };

        if current.version_output != locked.version_output {
            mismatches.push(format!(
                "  {} — version changed\n    locked:  {}\n    current: {}",
                tool_name,
                locked.version_output.lines().next().unwrap_or(""),
                current.version_output.lines().next().unwrap_or(""),
            ));
        } else if current.path != locked.path {
            mismatches.push(format!(
                "  {} — path changed\n    locked:  {}\n    current: {}",
                tool_name, locked.path, current.path,
            ));
        }
    }

    // Check for tools in lock file that are no longer required
    for tool_name in lock.tools.keys() {
        if !tool_commands.iter().any(|(name, _)| name == tool_name) {
            mismatches.push(format!(
                "  {} — in lock file but no longer required",
                tool_name
            ));
        }
    }

    if !mismatches.is_empty() {
        anyhow::bail!(
            "Tool version mismatch (run 'rsb tools lock' to update):\n{}",
            mismatches.join("\n")
        );
    }

    Ok(())
}

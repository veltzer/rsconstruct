use std::process::Command;
use anyhow::Result;
use serde::Serialize;
use crate::color;
use super::{Builder, sorted_keys};

#[derive(Serialize)]
struct DoctorCheck {
    name: String,
    status: &'static str,
    category: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    install_hint: Option<String>,
}

impl Builder {
    /// Run diagnostic checks on the build environment.
    pub fn doctor(&self) -> Result<()> {
        let json_mode = crate::json_output::is_json_mode();
        let mut ok_count = 0usize;
        let mut fail_count = 0usize;
        let mut warn_count = 0usize;
        let mut checks: Vec<DoctorCheck> = Vec::new();

        let mut record = |name: String, status: &'static str, category: &'static str, detail: Option<String>, install_hint: Option<String>, ok: &mut usize, fail: &mut usize, warn: &mut usize| {
            match status {
                "ok" => *ok += 1,
                "fail" => *fail += 1,
                "warn" => *warn += 1,
                _ => {}
            }
            if !json_mode {
                let detail_str = detail.as_deref().map(|d| format!(" ({})", d)).unwrap_or_default();
                let hint_str = install_hint.as_deref().map(|h| format!("  install: {}", h)).unwrap_or_default();
                let tag: String = match status {
                    "ok" => color::green("[ok]").to_string(),
                    "fail" => color::red("[FAIL]").to_string(),
                    "warn" => color::yellow("[warn]").to_string(),
                    _ => status.to_string(),
                };
                println!("{} {}{}{}", tag, name, detail_str, hint_str);
            }
            checks.push(DoctorCheck { name, status, category, detail, install_hint });
        };

        // Check rsconstruct.toml
        if std::path::Path::new("rsconstruct.toml").exists() {
            record("rsconstruct.toml found and valid".to_string(), "ok", "config", None, None, &mut ok_count, &mut fail_count, &mut warn_count);
        } else {
            record("rsconstruct.toml not found".to_string(), "fail", "config", None, None, &mut ok_count, &mut fail_count, &mut warn_count);
        }

        // Check .rsconstructignore
        if std::path::Path::new(".rsconstructignore").exists() {
            record(".rsconstructignore found".to_string(), "ok", "config", None, None, &mut ok_count, &mut fail_count, &mut warn_count);
        } else {
            record(".rsconstructignore not found (optional)".to_string(), "warn", "config", None, None, &mut ok_count, &mut fail_count, &mut warn_count);
        }

        // Check tools for enabled processors
        let processors = self.create_processors()?;
        let mut checked_tools = std::collections::HashSet::new();

        for name in sorted_keys(&processors) {
            let processor = &processors[name];
            for tool in processor.required_tools() {
                if !checked_tools.insert(tool.clone()) {
                    continue;
                }
                match tool_version(&tool) {
                    Some(version) => {
                        record(format!("{} available", tool), "ok", "tool", Some(version), None, &mut ok_count, &mut fail_count, &mut warn_count);
                    }
                    None => {
                        let install_hint = crate::processors::tool_install_command(&tool).map(|s| s.to_string());
                        record(format!("{} not found", tool), "fail", "tool", None, install_hint, &mut ok_count, &mut fail_count, &mut warn_count);
                    }
                }
            }
        }

        // Check declared dependencies
        let deps = &self.config.dependencies;
        if !deps.is_empty() {
            if !json_mode {
                println!();
                println!("{}:", color::bold("Declared dependencies"));
            }

            for pkg in &deps.system {
                if which::which(pkg).is_ok() {
                    record(format!("{} (system)", pkg), "ok", "dependency", None, None, &mut ok_count, &mut fail_count, &mut warn_count);
                } else {
                    record(format!("{} not found", pkg), "fail", "dependency", Some("system".to_string()), Some("rsconstruct tools install-deps".to_string()), &mut ok_count, &mut fail_count, &mut warn_count);
                }
            }

            for pkg in &deps.pip {
                let name = pkg.split(&['>', '<', '=', '!', '~'][..]).next().unwrap_or(pkg);
                let found = Command::new("pip")
                    .args(["show", "--quiet", name])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success());
                if found {
                    record(format!("{} (pip)", pkg), "ok", "dependency", None, None, &mut ok_count, &mut fail_count, &mut warn_count);
                } else {
                    record(format!("{} not installed", pkg), "fail", "dependency", Some("pip".to_string()), Some(format!("pip install {}", pkg)), &mut ok_count, &mut fail_count, &mut warn_count);
                }
            }

            for pkg in &deps.npm {
                let found = Command::new("npm")
                    .args(["list", "--depth=0", pkg])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success());
                if found {
                    record(format!("{} (npm)", pkg), "ok", "dependency", None, None, &mut ok_count, &mut fail_count, &mut warn_count);
                } else {
                    record(format!("{} not installed", pkg), "fail", "dependency", Some("npm".to_string()), Some(format!("npm install {}", pkg)), &mut ok_count, &mut fail_count, &mut warn_count);
                }
            }

            for pkg in &deps.gem {
                let found = Command::new("gem")
                    .args(["list", "--installed", "--exact", pkg])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success());
                if found {
                    record(format!("{} (gem)", pkg), "ok", "dependency", None, None, &mut ok_count, &mut fail_count, &mut warn_count);
                } else {
                    record(format!("{} not installed", pkg), "fail", "dependency", Some("gem".to_string()), Some(format!("gem install {}", pkg)), &mut ok_count, &mut fail_count, &mut warn_count);
                }
            }
        }

        let total = ok_count + fail_count;

        if json_mode {
            let out = serde_json::json!({
                "checks": checks,
                "summary": {
                    "ok": ok_count,
                    "fail": fail_count,
                    "warn": warn_count,
                    "total": total,
                },
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            println!();
            let summary = format!("Summary: {}/{} checks passed", ok_count, total);
            if fail_count == 0 {
                println!("{}", color::green(&summary));
            } else {
                println!("{}", color::yellow(&summary));
            }
        }

        Ok(())
    }
}

/// Try to get the version string of a tool by running `tool --version`.
fn tool_version(tool: &str) -> Option<String> {
    let output = Command::new(tool)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout);
    let version = version.trim();
    if version.is_empty() {
        let version = String::from_utf8_lossy(&output.stderr);
        let first_line = version.lines().next().unwrap_or("").trim();
        if first_line.is_empty() {
            return Some("ok".to_string());
        }
        return Some(first_line.to_string());
    }
    Some(version.lines().next().unwrap_or(version.as_ref()).to_string())
}

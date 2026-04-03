use std::process::Command;
use anyhow::Result;
use crate::color;
use super::{Builder, sorted_keys};

impl Builder {
    /// Run diagnostic checks on the build environment.
    pub fn doctor(&self) -> Result<()> {
        let mut ok_count = 0usize;
        let mut fail_count = 0usize;

        // Check rsconstruct.toml
        if std::path::Path::new("rsconstruct.toml").exists() {
            println!("{} rsconstruct.toml found and valid", color::green("[ok]"));
            ok_count += 1;
        } else {
            println!("{} rsconstruct.toml not found", color::red("[FAIL]"));
            fail_count += 1;
        }

        // Check .rsconstructignore
        if std::path::Path::new(".rsconstructignore").exists() {
            println!("{} .rsconstructignore found", color::green("[ok]"));
            ok_count += 1;
        } else {
            println!("{} .rsconstructignore not found (optional)", color::yellow("[warn]"));
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
                        println!("{} {} available ({})",
                            color::green("[ok]"), tool, color::dim(&version));
                        ok_count += 1;
                    }
                    None => {
                        let install_hint = crate::processors::tool_install_command(&tool)
                            .map(|cmd| format!("  install: {}", cmd))
                            .unwrap_or_default();
                        println!("{} {} not found{}",
                            color::red("[FAIL]"), tool, install_hint);
                        fail_count += 1;
                    }
                }
            }
        }

        // Check declared dependencies
        let deps = &self.config.dependencies;
        if !deps.is_empty() {
            println!();
            println!("{}:", color::bold("Declared dependencies"));

            for pkg in &deps.system {
                if which::which(pkg).is_ok() {
                    println!("{} {} (system)", color::green("[ok]"), pkg);
                    ok_count += 1;
                } else {
                    println!("{} {} not found (system)", color::red("[FAIL]"), pkg);
                    fail_count += 1;
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
                    println!("{} {} (pip)", color::green("[ok]"), pkg);
                    ok_count += 1;
                } else {
                    println!("{} {} not installed (pip install {})", color::red("[FAIL]"), pkg, pkg);
                    fail_count += 1;
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
                    println!("{} {} (npm)", color::green("[ok]"), pkg);
                    ok_count += 1;
                } else {
                    println!("{} {} not installed (npm install {})", color::red("[FAIL]"), pkg, pkg);
                    fail_count += 1;
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
                    println!("{} {} (gem)", color::green("[ok]"), pkg);
                    ok_count += 1;
                } else {
                    println!("{} {} not installed (gem install {})", color::red("[FAIL]"), pkg, pkg);
                    fail_count += 1;
                }
            }
        }

        // Summary
        println!();
        let total = ok_count + fail_count;
        let summary = format!("Summary: {}/{} checks passed", ok_count, total);
        if fail_count == 0 {
            println!("{}", color::green(&summary));
        } else {
            println!("{}", color::yellow(&summary));
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

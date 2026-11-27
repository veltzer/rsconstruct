use super::{Builder, sorted_keys};
use crate::color;
use anyhow::Result;
use serde::Serialize;
use std::process::Command;

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

/// The checks `doctor` made, printed one by one as they are recorded unless
/// the output is JSON.
struct Report {
    json_mode: bool,
    ok: usize,
    fail: usize,
    warn: usize,
    checks: Vec<DoctorCheck>,
}

impl Report {
    /// Record one check, and print it in human mode.
    fn record(
        &mut self,
        name: String,
        status: &'static str,
        category: &'static str,
        detail: Option<String>,
        install_hint: Option<String>,
    ) {
        match status {
            "ok" => self.ok += 1,
            "fail" => self.fail += 1,
            "warn" => self.warn += 1,
            _ => {}
        }
        if !self.json_mode {
            let detail_str = detail
                .as_deref()
                .map(|d| format!(" ({d})"))
                .unwrap_or_default();
            let hint_str = install_hint
                .as_deref()
                .map(|h| format!("  install: {h}"))
                .unwrap_or_default();
            let tag: String = match status {
                "ok" => color::green("[ok]").to_string(),
                "fail" => color::red("[FAIL]").to_string(),
                "warn" => color::yellow("[warn]").to_string(),
                _ => status.to_string(),
            };
            println!("{tag} {name}{detail_str}{hint_str}");
        }
        self.checks.push(DoctorCheck {
            name,
            status,
            category,
            detail,
            install_hint,
        });
    }

    /// A check that passed.
    fn ok(&mut self, name: String, category: &'static str, detail: Option<String>) {
        self.record(name, "ok", category, detail, None);
    }

    /// A declared dependency that is not installed: `kind` is where it comes
    /// from, `install` the command that installs it.
    fn missing_dependency(&mut self, name: String, kind: &str, install: &str) {
        self.record(
            name,
            "fail",
            "dependency",
            Some(kind.to_string()),
            Some(install.to_string()),
        );
    }

    /// Print the summary: the whole report in JSON mode, one line otherwise.
    fn print_summary(&self) -> Result<()> {
        let total = self.ok + self.fail;
        if self.json_mode {
            let out = serde_json::json!({
                "checks": self.checks,
                "summary": {
                    "ok": self.ok,
                    "fail": self.fail,
                    "warn": self.warn,
                    "total": total,
                },
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            println!();
            let summary = format!("Summary: {}/{total} checks passed", self.ok);
            if self.fail == 0 {
                println!("{}", color::green(&summary));
            } else {
                println!("{}", color::yellow(&summary));
            }
        }
        Ok(())
    }
}

/// Check the project's config files are there.
fn check_config_files(report: &mut Report) {
    if std::path::Path::new("rsconstruct.toml").exists() {
        report.ok(
            "rsconstruct.toml found and valid".to_string(),
            "config",
            None,
        );
    } else {
        report.record(
            "rsconstruct.toml not found".to_string(),
            "fail",
            "config",
            None,
            None,
        );
    }
    if std::path::Path::new(".rsconstructignore").exists() {
        report.ok(".rsconstructignore found".to_string(), "config", None);
    } else {
        report.record(
            ".rsconstructignore not found (optional)".to_string(),
            "warn",
            "config",
            None,
            None,
        );
    }
}

impl Builder {
    /// Run diagnostic checks on the build environment.
    pub fn doctor(&self, ctx: &crate::build_context::BuildContext) -> Result<()> {
        let mut report = Report {
            json_mode: crate::json_output::is_json_mode(),
            ok: 0,
            fail: 0,
            warn: 0,
            checks: Vec::new(),
        };
        check_config_files(&mut report);
        self.check_processor_tools(ctx, &mut report)?;
        self.check_dependencies(ctx, &mut report)?;
        report.print_summary()
    }

    /// Check the tools of the enabled processors. A stanza with `enabled =
    /// false` is kept precisely so its tool need not be installed, so its
    /// absence is not a finding.
    fn check_processor_tools(
        &self,
        ctx: &crate::build_context::BuildContext,
        report: &mut Report,
    ) -> Result<()> {
        let processors = self.create_processors()?;
        let mut checked_tools = std::collections::HashSet::new();

        for name in sorted_keys(&processors) {
            if !super::tools::instance_enabled(&processors, name) {
                continue;
            }
            let processor = &processors[name];
            for tool in processor.required_tools() {
                if !checked_tools.insert(tool.clone()) {
                    continue;
                }
                if let Some(version) = tool_version(ctx, &tool) {
                    report.ok(format!("{tool} available"), "tool", Some(version));
                } else {
                    let install_hint = crate::tools::tool_install_command(&tool);
                    report.record(
                        format!("{tool} not found"),
                        "fail",
                        "tool",
                        None,
                        install_hint,
                    );
                }
            }
        }
        Ok(())
    }

    /// Check the declared dependencies. The Python set is uv.lock's closure,
    /// which `tool install project` puts into the repo's .venv.
    fn check_dependencies(
        &self,
        ctx: &crate::build_context::BuildContext,
        report: &mut Report,
    ) -> Result<()> {
        const PROJECT: &str = "rsconstruct tool install project";
        let root = std::path::Path::new(".");
        let deps = &self.config.dependencies;
        let pip_deps = crate::config::python_deps(root)?;
        // The Node set comes from package.json; installed means present in
        // the project's node_modules (a global copy is not what the lock
        // asked for).
        let node_deps = deps.node_deps(root)?;
        if deps.is_empty() && pip_deps.is_empty() && node_deps.is_empty() {
            return Ok(());
        }
        if !report.json_mode {
            println!();
            println!("{}:", color::bold("Declared dependencies"));
        }

        for pkg in &deps.system {
            // A system dependency is a package, not a tool: ask the
            // package manager whether it is installed. which() is wrong
            // here — packages like aspell-en (a dictionary) ship no
            // binary named after themselves, so which() reported them
            // as missing even right after an install put them on.
            if super::tools::is_system_package_installed(ctx, pkg) {
                report.ok(format!("{pkg} (system)"), "dependency", None);
            } else {
                report.missing_dependency(
                    format!("{pkg} not found"),
                    "system",
                    "rsconstruct tool install machine",
                );
            }
        }

        check_python_packages(ctx, &pip_deps, report);

        // `[dependencies]` cargo, npm and gem entries are installed into
        // the project's tool directory, so that is where they are looked
        // for — the same place `tool check` reads — not on the machine.
        // Whether each is at its locked version is `tool check`'s job.
        let in_project = crate::tool_store::installed_versions(root);
        for (method, list) in [
            ("cargo", &deps.cargo),
            ("gem", &deps.gem),
            ("npm", &deps.npm),
        ] {
            for pkg in list {
                let name = crate::tool_lock::split_versioned(method, pkg)
                    .map_or_else(|| pkg.clone(), |(name, _)| name);
                if in_project.contains_key(&(method.to_string(), name)) {
                    report.ok(format!("{pkg} ({method})"), "dependency", None);
                } else {
                    report.missing_dependency(
                        format!(
                            "{pkg} not installed in {}",
                            crate::tool_store::REPO_TOOLS_DIR
                        ),
                        method,
                        PROJECT,
                    );
                }
            }
        }

        for pkg in &node_deps {
            if crate::config::installed_node_package_version(root, pkg).is_some() {
                report.ok(format!("{pkg} (package.json)"), "dependency", None);
            } else {
                report.missing_dependency(
                    format!("{pkg} not installed in node_modules"),
                    "npm",
                    PROJECT,
                );
            }
        }
        Ok(())
    }
}

/// Check the Python packages. They are judged by the interpreter, with the
/// probe `tool check` uses: `pip show` fails outright in a uv-managed venv
/// (the default installer's), which has no pip.
fn check_python_packages(
    ctx: &crate::build_context::BuildContext,
    pip_deps: &[String],
    report: &mut Report,
) {
    let pip_names: Vec<String> = pip_deps
        .iter()
        .map(|pkg| crate::config::normalized_distribution_name(pkg))
        .collect();
    let installed_pip = if pip_names.is_empty() {
        std::collections::BTreeMap::new()
    } else {
        let names: Vec<&str> = pip_names.iter().map(String::as_str).collect();
        crate::tool_lock::installed_pip_versions(ctx, &names)
    };
    for (pkg, name) in pip_deps.iter().zip(&pip_names) {
        if installed_pip.contains_key(name) {
            report.ok(format!("{pkg} (pip)"), "dependency", None);
        } else {
            report.missing_dependency(
                format!("{pkg} not installed"),
                "pip",
                "rsconstruct tool install project",
            );
        }
    }
}

/// Try to get the version string of a tool by running `tool --version`.
fn tool_version(ctx: &crate::build_context::BuildContext, tool: &str) -> Option<String> {
    let mut cmd = Command::new(tool);
    cmd.arg("--version");
    let output = crate::processor::run_command_capture(ctx, &cmd).ok()?;
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
    Some(version.lines().next().unwrap_or(version).to_string())
}

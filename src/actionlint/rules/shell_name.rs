//! The `shell-name` rule: `shell:` values, per the job's platform.

use super::{Rule, RuleBase};
use crate::actionlint::ast::{Exec, Job, Runner, Step, Str, Workflow};
use crate::actionlint::parse::sorted_quotes;
use crate::actionlint::yaml::go_quote;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Any,
    MacOrLinux,
    Windows,
}

const fn available_shell_names(kind: Platform) -> &'static [&'static str] {
    match kind {
        Platform::Any => &["bash", "pwsh", "python", "sh", "cmd", "powershell"],
        Platform::Windows => &["bash", "pwsh", "python", "cmd", "powershell"],
        Platform::MacOrLinux => &["bash", "pwsh", "python", "sh"],
    }
}

pub struct RuleShellName {
    base: RuleBase,
    platform: Platform,
}

impl RuleShellName {
    pub const fn new() -> Self {
        Self {
            base: RuleBase::new("shell-name"),
            platform: Platform::Any,
        }
    }

    fn check_shell_name(&mut self, node: Option<&Str>) {
        let Some(node) = node else {
            return;
        };
        if node.value.contains("{0}") || node.contains_expression() {
            return;
        }
        let name = node.value.to_lowercase();
        let available = available_shell_names(self.platform);
        if available.contains(&name.as_str()) {
            return;
        }
        let on_platform = match self.platform {
            Platform::Windows if available_shell_names(Platform::Any).contains(&name.as_str()) => {
                " on Windows"
            }
            Platform::MacOrLinux
                if available_shell_names(Platform::Any).contains(&name.as_str()) =>
            {
                " on macOS or Linux"
            }
            _ => "",
        };
        self.base.error(
            node.pos,
            format!(
                "shell name {} is invalid{on_platform}. available names are {}",
                go_quote(&node.value),
                sorted_quotes(available)
            ),
        );
    }

    fn platform_from_runner(runner: &Runner) -> Platform {
        let mut ret = Platform::Any;
        for label in &runner.labels {
            let l = label.value.to_lowercase();
            let k = if l.starts_with("windows-") || l == "windows" {
                Platform::Windows
            } else if l.starts_with("macos-")
                || l.starts_with("ubuntu-")
                || l == "macos"
                || l == "linux"
            {
                Platform::MacOrLinux
            } else {
                Platform::Any
            };
            if k == Platform::Any {
                continue;
            }
            if ret != Platform::Any && ret != k {
                return Platform::Any;
            }
            ret = k;
        }
        ret
    }
}

impl Rule for RuleShellName {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_step(&mut self, step: &Step) {
        if let Some(Exec::Run(run)) = &step.exec {
            self.check_shell_name(run.shell.as_ref());
        }
    }

    fn visit_job_pre(&mut self, job: &Job) {
        let Some(runner) = &job.runs_on else {
            return;
        };
        self.platform = Self::platform_from_runner(runner);
        if let Some(d) = &job.defaults
            && let Some(run) = &d.run
        {
            self.check_shell_name(run.shell.as_ref());
        }
    }

    fn visit_job_post(&mut self, _job: &Job) {
        self.platform = Platform::Any;
    }

    fn visit_workflow_pre(&mut self, workflow: &Workflow) {
        if let Some(d) = &workflow.defaults
            && let Some(run) = &d.run
        {
            self.check_shell_name(run.shell.as_ref());
        }
    }
}

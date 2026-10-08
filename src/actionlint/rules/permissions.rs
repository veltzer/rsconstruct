//! The `permissions` rule: scope names and their values.

use super::{Rule, RuleBase};
use crate::actionlint::ast::{Job, Permissions, Workflow};
use crate::actionlint::parse::{quotes, sorted_quotes};
use crate::actionlint::yaml::go_quote;

const ALL_PERMISSION_SCOPES: &[(&str, &[&str])] = &[
    ("actions", &["read", "write", "none"]),
    ("artifact-metadata", &["read", "write", "none"]),
    ("attestations", &["read", "write", "none"]),
    ("checks", &["read", "write", "none"]),
    ("contents", &["read", "write", "none"]),
    ("deployments", &["read", "write", "none"]),
    ("discussions", &["read", "write", "none"]),
    ("id-token", &["write", "none"]),
    ("issues", &["read", "write", "none"]),
    ("models", &["read", "none"]),
    ("packages", &["read", "write", "none"]),
    ("pages", &["read", "write", "none"]),
    ("pull-requests", &["read", "write", "none"]),
    ("repository-projects", &["read", "write", "none"]),
    ("security-events", &["read", "write", "none"]),
    ("statuses", &["read", "write", "none"]),
];

pub struct RulePermissions {
    base: RuleBase,
}

impl RulePermissions {
    pub const fn new() -> Self {
        Self {
            base: RuleBase::new("permissions"),
        }
    }

    fn check_permissions(&mut self, p: Option<&Permissions>) {
        let Some(p) = p else {
            return;
        };
        if let Some(all) = &p.all {
            if all.value != "write-all" && all.value != "read-all" {
                self.base.error(
                    all.pos,
                    format!(
                        "{} is invalid for permission for all the scopes. available values are \"read-all\", \"write-all\" or {{}}",
                        go_quote(&all.value)
                    ),
                );
            }
            return;
        }
        for scope in p.scopes.values() {
            let n = scope.name.value.as_str();
            let Some((_, values)) = ALL_PERMISSION_SCOPES.iter().find(|(name, _)| *name == n)
            else {
                let names: Vec<&str> = ALL_PERMISSION_SCOPES
                    .iter()
                    .map(|(name, _)| *name)
                    .collect();
                self.base.error(
                    scope.name.pos,
                    format!(
                        "unknown permission scope {}. all available permission scopes are {}",
                        go_quote(n),
                        sorted_quotes(&names)
                    ),
                );
                continue;
            };
            if !values.contains(&scope.value.value.as_str()) {
                self.base.error(
                    scope.value.pos,
                    format!(
                        "{} is invalid as permission of scope {}. available values are {}",
                        go_quote(&scope.value.value),
                        go_quote(n),
                        quotes(values)
                    ),
                );
            }
        }
    }
}

impl Rule for RulePermissions {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_job_pre(&mut self, job: &Job) {
        self.check_permissions(job.permissions.as_ref());
    }

    fn visit_workflow_pre(&mut self, workflow: &Workflow) {
        self.check_permissions(workflow.permissions.as_ref());
    }
}

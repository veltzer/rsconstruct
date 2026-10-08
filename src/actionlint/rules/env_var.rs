//! The `env-var` rule: environment variable names.

use super::{Rule, RuleBase};
use crate::actionlint::ast::{Env, Job, Step, Workflow};
use crate::actionlint::yaml::go_quote;

pub struct RuleEnvVar {
    base: RuleBase,
}

impl RuleEnvVar {
    pub const fn new() -> Self {
        Self {
            base: RuleBase::new("env-var"),
        }
    }

    fn check_env(&mut self, env: Option<&Env>) {
        let Some(env) = env else {
            return;
        };
        if env.expression.is_some() {
            return;
        }
        for v in env.vars.values() {
            if v.name.contains_expression() {
                continue;
            }
            if v.name.value.contains(['&', '=', ' ', '\t']) {
                self.base.error(
                    v.name.pos,
                    format!(
                        "environment variable name {} is invalid. '&', '=' and spaces should not be contained",
                        go_quote(&v.name.value)
                    ),
                );
            }
        }
    }
}

impl Rule for RuleEnvVar {
    fn base(&mut self) -> &mut RuleBase {
        &mut self.base
    }

    fn visit_step(&mut self, step: &Step) {
        self.check_env(step.env.as_ref());
    }

    fn visit_job_pre(&mut self, job: &Job) {
        self.check_env(job.env.as_ref());
        if let Some(c) = &job.container {
            self.check_env(c.env.as_ref());
        }
        if let Some(s) = &job.services {
            for service in s.value.values() {
                self.check_env(service.container.env.as_ref());
            }
        }
    }

    fn visit_workflow_pre(&mut self, workflow: &Workflow) {
        self.check_env(workflow.env.as_ref());
    }
}
